use crate::book::chapter::LatLon;
use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// Image-normalised, top-left origin -- matching Swift's
    /// `VisionAnalyzer.topLeft`.
    pub box_: Rect,
    pub capture_quality: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaletteColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub weight: f64,
}

/// The engine's view of one analysed photo. Built from the FULL Swift
/// feature record, not from the narrowed `AnalyzedPhoto` the webview sees:
/// the scorer needs face boxes, the saliency box and the palette, none of
/// which `partial_photo` forwards.
///
/// `width`/`height` are post-orientation. `ExifReader` swaps them for EXIF
/// orientations 5-8 before they ever reach this struct, so layout boxes
/// computed here are already correct for portrait photos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub path: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub is_utility: bool,
    pub aesthetic_pct: u8,
    pub sharpness_pct: u8,
    pub near_dup_cluster: u32,
    pub event_cluster: u32,
    pub faces: Vec<Face>,
    pub face_area_fraction: f64,
    pub saliency_box: Option<Rect>,
    pub palette: Vec<PaletteColor>,
    /// Best face capture quality on the photo, or `None` when there are no
    /// faces or Vision returned none. Precomputed because culling reads it
    /// once per comparison.
    pub capture_quality: Option<f64>,
    /// Vision's scene classification tags. Always emitted by Swift, so an
    /// absent KEY is a pipeline bug and is refused; an EMPTY list is a real
    /// answer (Vision recognised nothing) and is accepted.
    pub scene_tags: Vec<String>,
    /// EXIF capture time, seconds since the epoch. Legitimately `None` --
    /// a scan or an export often carries no EXIF date at all -- so absence
    /// is tolerated here, unlike `scene_tags`.
    pub captured_at: Option<i64>,
    /// Fraction of luma below 5% of full scale.
    pub clipped_low: f64,
    /// Fraction of luma above 95% of full scale.
    pub clipped_high: f64,
    /// Vision's image feature print, compared with
    /// `cluster::feature_distance`. `None` when Vision produced none.
    pub feature_print: Option<Vec<f32>>,
    /// EXIF GPS fix. `None` when the photo has none, which is ordinary.
    pub location: Option<LatLon>,
}

impl Photo {
    /// Real-world aspect ratio of the photo itself, width over height.
    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height as f64
    }
}

/// Decodes the sidecar's `featurePrint`: base64 of little-endian Float32s,
/// as Swift's `FeaturePrint.encode` writes it.
pub fn decode_feature_print(encoded: &str) -> Option<Vec<f32>> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

fn rect_from(v: &serde_json::Value) -> Option<Rect> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    Some(Rect::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?, a[3].as_f64()?))
}

/// Clamps a Vision face box to the image, dropping a box with no part of it
/// in the picture.
///
/// Vision does not guarantee its boxes lie inside the frame -- it
/// extrapolates a partially visible face -- and in one real 1133-photo
/// library 9 of the 1164 detected faces overhang, by up to 0.15 of the frame.
/// `VisionAnalyzer.topLeft` is a straight origin flip and is not the source.
///
/// Left alone it is a hard failure, not a rounding nuisance. Every crop
/// window lies inside the frame by construction, so an overhanging box can
/// never be CONTAINED by a crop while it does intersect one, which is exactly
/// `score::rejects`'s definition of `FaceClipped` -- in every slot of every
/// template. The photo becomes unplaceable anywhere and used to take its
/// whole spread down with it, printing two blank pages.
///
/// The frame belongs here, at the boundary the external data arrives at: a
/// box describing a region of the image cannot leave the image, and nothing
/// downstream should have to defend against one that does. Clamping in Rust
/// rather than in Swift also repairs the rows already in the features cache,
/// which a sidecar fix would reach only behind an `ANALYZER_VERSION` bump and
/// a full re-analysis.
fn in_frame(face: Face) -> Option<Face> {
    let frame = Rect::new(0.0, 0.0, 1.0, 1.0);
    // An in-frame box is returned untouched rather than round-tripped through
    // `intersect`, which rebuilds the rect from its edges and so perturbs a
    // clean 0.2 into 0.20000000000000007. `contains` shares the module's
    // float tolerance, so a box over the edge by less than that is already
    // inside every downstream predicate's slack.
    if frame.contains(&face.box_) {
        return Some(face);
    }
    Some(Face { box_: frame.intersect(&face.box_)?, ..face })
}

/// Whether a record is expected to carry the WHOLE-SET derivations --
/// `nearDupCluster`, `eventCluster`, `aestheticPct`, `sharpnessPct` -- that
/// `commands::finalize_photos` stamps once every photo in the folder has been
/// seen.
///
/// Two callers, two genuinely different contracts, and conflating them is
/// what made the old parser dangerous. See `from_features` and
/// `from_cached_features`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Derived {
    /// The record came from `finalize_photos` (directly, or round-tripped
    /// through the webview). Absence is a pipeline bug, not a data shape.
    Required,
    /// The record came straight out of the SQLite features cache, which
    /// stores Swift's per-photo record verbatim. The derivations were never
    /// persisted, so their absence is the expected, correct state.
    AbsentByDesign,
}

/// Builds a `Photo` from one FINALIZED feature record -- the output of
/// `commands::finalize_photos`, whether read back directly or round-tripped
/// through the webview and handed to `recommend_book`/`generate_book`.
///
/// Strict on purpose. Every field it reads is one whose absence changes the
/// printed book while raising nothing anywhere, so `None` (which the callers
/// turn into a skipped photo or a failed command) is the only honest answer:
///
/// * `nearDupCluster` absent -> every photo lands in cluster 0 and `cull`
///   keeps exactly ONE photo for the entire book;
/// * `faces` absent -> every gutter and safe-margin rejection in the scorer
///   and in pre-flight goes inert, and crops re-centre off the subject. That
///   reaches print wrong;
/// * `faceAreaFraction`, `palette`, `aestheticPct`, `sharpnessPct`,
///   `isUtility`, `eventCluster` absent -> scoring, ranking, chaptering or
///   utility-filtering silently degrade to a constant.
///
/// A missing KEY is distinguished from an empty VALUE throughout: `faces: []`
/// and `palette: []` are real answers (a photo with no people in it, an image
/// Vision found no dominant colours in) and are accepted; a missing `faces`
/// key is not. `saliencyBox` is the one field that may legitimately be
/// ABSENT -- Swift declares it `[Double]?` and `encodeIfPresent` omits it
/// entirely when Vision returns no attention box, which happens on real
/// images -- so it stays `Option<Rect>`.
///
/// This is why `AnalyzedPhoto` in `app/types/features.ts` is dangerous to
/// reshape: it declares none of these engine-only keys, so a `.map()` in the
/// webview type-checks while dropping them. It now fails loudly here instead
/// of quietly producing a one-photo book.
pub fn from_features(v: &serde_json::Value) -> Option<Photo> {
    parse_features(v, Derived::Required)
}

/// As `from_features`, but for a record read straight out of the features
/// cache, where the whole-set derivations are KNOWN to be absent:
/// `finalize_photos` stamps them onto its return value and never writes them
/// back to SQLite (`commands::gather` caches the raw sidecar `features`
/// object).
///
/// They are zeroed here rather than merely tolerated by the shared parser, so
/// that `from_features` can stay strict for every other caller. Zeroing is
/// safe only because nothing on this path reads them: `resolve_photos` feeds
/// pre-flight and the exporter, neither of which culls, ranks or chapters --
/// the book was assembled long before, and is reloaded whole from the project
/// row. Any future caller that culls must use `from_features`.
pub fn from_cached_features(v: &serde_json::Value) -> Option<Photo> {
    parse_features(v, Derived::AbsentByDesign)
}

fn parse_features(v: &serde_json::Value, derived: Derived) -> Option<Photo> {
    // The KEY must be present and an array; `[]` is a real answer (a photo
    // with nobody in it) and must not be confused with "faces never ran".
    let faces: Vec<Face> = v["faces"]
        .as_array()?
        .iter()
        .map(|f| {
            Some(Face {
                box_: rect_from(&f["box"])?,
                capture_quality: f["captureQuality"].as_f64(),
            })
        })
        .collect::<Option<Vec<Face>>>()?
        .into_iter()
        .filter_map(in_frame)
        .collect();

    let capture_quality = faces
        .iter()
        .filter_map(|f| f.capture_quality)
        .max_by(|a, b| a.total_cmp(b));

    let palette: Vec<PaletteColor> = v["palette"]
        .as_array()?
        .iter()
        .map(|c| {
            Some(PaletteColor {
                r: c["r"].as_f64()?,
                g: c["g"].as_f64()?,
                b: c["b"].as_f64()?,
                weight: c["weight"].as_f64()?,
            })
        })
        .collect::<Option<Vec<PaletteColor>>>()?;

    // `Required` demands the key; `AbsentByDesign` reads it if it happens to
    // be there and otherwise zeroes it.
    let whole_set = |key: &str| -> Option<u64> {
        match derived {
            Derived::Required => v[key].as_u64(),
            Derived::AbsentByDesign => Some(v[key].as_u64().unwrap_or(0)),
        }
    };

    // Required: Swift always emits the key. `[]` is a real answer and is
    // accepted; a missing key is not.
    let scene_tags: Vec<String> = v["sceneTags"]
        .as_array()?
        .iter()
        .map(|t| t.as_str().map(str::to_string))
        .collect::<Option<Vec<String>>>()?;

    // Optional: `encodeIfPresent` omits it, and a photo with no EXIF date
    // is ordinary rather than malformed.
    let captured_at = v["exif"]["captureDate"].as_f64().map(|t| t as i64);

    // Optional like `saliencyBox`: Swift omits it when Vision returns no
    // print. Present but undecodable is corruption and refuses the record.
    let feature_print = match v.get("featurePrint") {
        None => None,
        Some(encoded) => Some(decode_feature_print(encoded.as_str()?)?),
    };

    Some(Photo {
        path: v["path"].as_str()?.to_string(),
        hash: v["hash"].as_str()?.to_string(),
        width: v["width"].as_u64()? as u32,
        height: v["height"].as_u64()? as u32,
        is_utility: v["isUtility"].as_bool()?,
        aesthetic_pct: whole_set("aestheticPct")? as u8,
        sharpness_pct: whole_set("sharpnessPct")? as u8,
        near_dup_cluster: whole_set("nearDupCluster")? as u32,
        event_cluster: whole_set("eventCluster")? as u32,
        faces,
        face_area_fraction: v["faceAreaFraction"].as_f64()?,
        // The ONE field that may legitimately be absent: Vision returns no
        // attention box on some images and Swift's `encodeIfPresent` then
        // omits the key entirely.
        saliency_box: rect_from(&v["saliencyBox"]),
        palette,
        capture_quality,
        scene_tags,
        captured_at,
        clipped_low: v["clippedLow"].as_f64()?,
        clipped_high: v["clippedHigh"].as_f64()?,
        feature_print,
        location: v["exif"]["latitude"]
            .as_f64()
            .zip(v["exif"]["longitude"].as_f64())
            .and_then(|(lat, lon)| LatLon::new(lat, lon)),
    })
}

/// What the USER said about one photo, overriding what the engine would
/// decide on its own. Keyed by CONTENT HASH rather than by path, so a
/// decision survives the file being moved or renamed -- the same reason
/// `Project::photo_hashes` is hash-keyed.
///
/// Two byte-identical files in one folder share a hash and therefore share a
/// decision. That is the honest reading: they are, in pixels, the same
/// photograph, and the user picking "this one" cannot have meant one copy of
/// it and not the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Override {
    /// The engine decides. The default, and the state every photo starts in.
    #[default]
    Auto,
    /// The user wants this photo in the book, whatever the engine thinks.
    Include,
    /// The user wants this photo out of the book, whatever the engine thinks.
    Exclude,
}

impl Override {
    /// The token this state is stored and transmitted as. One spelling, used
    /// by serde (via `rename_all`), by SQLite and by the TypeScript union --
    /// so a state cannot mean one thing on the wire and another in the
    /// database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Include => "include",
            Self::Exclude => "exclude",
        }
    }

    /// Parses a stored token. `None` for anything else: an unrecognised state
    /// is a decision this build cannot honour, and treating it as `Auto`
    /// would silently discard a user's choice -- the one failure this whole
    /// feature exists to prevent. Callers surface it instead.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "auto" => Some(Self::Auto),
            "include" => Some(Self::Include),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }
}

/// Every user decision in one map, hash -> state. Absent means `Auto`, so an
/// empty map is exactly "the engine decides everything", i.e. the behaviour
/// this project had before overrides existed.
///
/// `#[serde(transparent)]` so the wire shape is a plain object
/// (`{"<hash>": "include"}`) rather than a wrapper with a field name the
/// TypeScript side would also have to know about. Pinned from both sides in
/// `tests/fixtures/wire/photo-overrides.json`.
/// Deserialisation goes through `FromIterator` rather than the derive, so an
/// explicit `"auto"` arriving from the webview is normalised away exactly as
/// `set` normalises it -- keeping "one state, one representation" true of
/// values that came off the wire too, not only of ones built in Rust.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Overrides(std::collections::BTreeMap<String, Override>);

impl<'de> Deserialize<'de> for Overrides {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = std::collections::BTreeMap::<String, Override>::deserialize(d)?;
        Ok(raw.into_iter().collect())
    }
}

impl Overrides {
    pub fn new() -> Self {
        Self(std::collections::BTreeMap::new())
    }

    /// The user's decision for a photo, `Auto` when they never made one.
    pub fn get(&self, hash: &str) -> Override {
        self.0.get(hash).copied().unwrap_or_default()
    }

    /// Records a decision. `Auto` REMOVES the entry rather than storing it:
    /// "back to automatic" is the absence of a decision, and storing it would
    /// make two representations of the same state that every comparison and
    /// every persisted row would then have to treat as equal.
    pub fn set(&mut self, hash: impl Into<String>, state: Override) {
        let hash = hash.into();
        match state {
            Override::Auto => {
                self.0.remove(&hash);
            }
            _ => {
                self.0.insert(hash, state);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Hash and state for every decision actually made, in hash order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Override)> {
        self.0.iter().map(|(h, &s)| (h.as_str(), s))
    }

    /// How many of `photos` the user explicitly asked for. Counted over the
    /// photo set rather than over the map, because a decision whose photo is
    /// not in this folder must not inflate the figure the user is shown.
    pub fn included_in(&self, photos: &[Photo]) -> usize {
        photos.iter().filter(|p| self.get(&p.hash) == Override::Include).count()
    }
}

impl FromIterator<(String, Override)> for Overrides {
    fn from_iter<I: IntoIterator<Item = (String, Override)>>(iter: I) -> Self {
        let mut out = Self::new();
        for (hash, state) in iter {
            out.set(hash, state);
        }
        out
    }
}

/// The single authority on which photo survives.
///
/// Drops `is_utility`, then keeps one winner per near-duplicate cluster.
/// The winner is ranked by:
///
/// 1. exposure: a frame with more than `CLIPPED_LIMIT` of its pixels crushed
///    to black or blown to white loses to any frame that is not;
/// 2. sharpness percentile -> face capture quality -> aesthetic percentile.
///
/// There is no "most typical frame" term. `cluster::similar_clusters` is
/// complete linkage, so every member is already within
/// `SIMILAR_DISTANCE` of every other and a neighbour count ties: on Bali and
/// Iceland it separated 10 of 282 clusters.
///
/// **Cull proposes; the user disposes.** `overrides` is applied on top of
/// that verdict, and it wins:
///
/// * `Exclude` never survives, whatever the engine thinks of it.
/// * `Include` always survives -- even a `is_utility` image, even a frame
///   that lost its near-duplicate cluster.
///
/// An `Include` is ADDITIVE, not a substitution: it does not displace the
/// `Auto` winner of its own cluster. Two photos from one burst therefore both
/// appear when the user picks one and the engine picks another. The
/// alternative -- letting an `Include` take over its cluster's single slot --
/// would make ticking one photo silently remove a different photo the user
/// never touched, which is the same class of surprise this whole feature
/// exists to remove; and `Exclude` already exists for saying "not that one".
///
/// `smile_fraction` is deliberately NOT in the ranking, departing from
/// section 7.4 of the original design: it is miscalibrated with a proven
/// 100% false-negative rate (see `PROJECT-STATUS.md`), so including it adds
/// noise rather than signal.
///
/// This is now the ONLY implementation of the rule anywhere in the project.
/// Two others used to exist and have both been removed: Rust's
/// `count_keepers` hand-rolled it (it delegates here now), and TypeScript's
/// `keepers()` re-derived it in the webview, ranking sharpness straight to
/// aesthetic with no capture-quality tie-break -- so the contact sheet could
/// show a different set of survivors, in count and identity, from the one the
/// book was built out of. `commands::stamp_kept` now writes this function's
/// verdict onto each photo record as `kept`, and `keepers()` is a filter on
/// that flag with no ranking of its own. Do not reintroduce a second copy of
/// this rule; send this one's answer instead.
pub fn cull(photos: &[Photo], overrides: &Overrides) -> Vec<Photo> {
    use std::collections::BTreeMap;

    let mut contests: BTreeMap<u32, Vec<&Photo>> = BTreeMap::new();
    let mut included: Vec<&Photo> = Vec::new();

    for photo in photos {
        match overrides.get(&photo.hash) {
            // Out, unconditionally -- and BEFORE the cluster contest, so
            // excluding the winner of a burst promotes its runner-up rather
            // than leaving the burst unrepresented.
            Override::Exclude => continue,
            // In, unconditionally, and NOT entered into the cluster contest:
            // an explicit choice neither competes for the cluster's single
            // automatic slot nor takes it away from the photo that won it.
            Override::Include => included.push(photo),
            Override::Auto => {
                if photo.is_utility {
                    continue;
                }
                contests.entry(photo.near_dup_cluster).or_default().push(photo);
            }
        }
    }

    // `BTreeMap` iterates by cluster id, so the output order depends only on
    // cluster ids, never on input order -- the property the ordering test
    // pins with deliberately unsorted input. The final sort by path then
    // interleaves the explicit choices with the automatic survivors rather
    // than appending them, because `pack` cuts chapters in this order.
    let best = contests.into_values().filter_map(|contestants| winner(&contestants));
    let mut kept: Vec<Photo> = best.chain(included).cloned().collect();
    kept.sort_by(|a, b| a.path.cmp(&b.path));
    kept
}

/// The share of clipped pixels past which a frame is badly exposed. Bali's
/// three frames past it are moon shots that are almost entirely black.
const CLIPPED_LIMIT: f64 = 0.9;

fn winner<'a>(contestants: &[&'a Photo]) -> Option<&'a Photo> {
    let exposed = |p: &Photo| p.clipped_low <= CLIPPED_LIMIT && p.clipped_high <= CLIPPED_LIMIT;
    contestants.iter().copied().reduce(|incumbent, challenger| {
        let (ce, ie) = (exposed(challenger), exposed(incumbent));
        if ce > ie || (ce == ie && beats(challenger, incumbent)) {
            challenger
        } else {
            incumbent
        }
    })
}

fn beats(challenger: &Photo, incumbent: &Photo) -> bool {
    use std::cmp::Ordering;
    match challenger.sharpness_pct.cmp(&incumbent.sharpness_pct) {
        Ordering::Greater => return true,
        Ordering::Less => return false,
        Ordering::Equal => {}
    }
    let cq = |p: &Photo| p.capture_quality.unwrap_or(-1.0);
    match cq(challenger).total_cmp(&cq(incumbent)) {
        Ordering::Greater => return true,
        Ordering::Less => return false,
        Ordering::Equal => {}
    }
    challenger.aesthetic_pct > incumbent.aesthetic_pct
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(path: &str, cluster: u32, sharp: u8, aesth: u8) -> Photo {
        Photo {
            path: path.into(),
            hash: format!("h-{path}"),
            width: 4032,
            height: 3024,
            is_utility: false,
            aesthetic_pct: aesth,
            sharpness_pct: sharp,
            near_dup_cluster: cluster,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None,
        }
    }


    fn winner(photos: &[Photo]) -> String {
        let kept = cull(photos, &Overrides::new());
        assert_eq!(kept.len(), 1, "one cluster keeps one photo");
        kept[0].path.clone()
    }

    /// The clipped frame is the sharper one, so without the screen it wins.
    #[test]
    fn cull_never_picks_a_clipped_frame_over_one_that_is_exposed() {
        let dark = Photo { clipped_low: 0.95, ..photo("/a.jpg", 7, 90, 50) };
        assert_eq!(winner(&[dark, photo("/b.jpg", 7, 10, 50)]), "/b.jpg");
        let blown = Photo { clipped_high: 0.95, ..photo("/a.jpg", 7, 90, 50) };
        assert_eq!(winner(&[blown, photo("/b.jpg", 7, 10, 50)]), "/b.jpg");
    }

    #[test]
    fn cull_still_keeps_one_photo_when_every_frame_is_clipped() {
        let dark = |path, sharp| Photo { clipped_low: 0.95, ..photo(path, 7, sharp, 50) };
        assert_eq!(winner(&[dark("/a.jpg", 10), dark("/b.jpg", 90)]), "/b.jpg");
    }

    #[test]
    fn cull_drops_utility_images() {
        let mut u = photo("/a.jpg", 0, 90, 90);
        u.is_utility = true;
        let kept = cull(&[u, photo("/b.jpg", 1, 10, 10)], &Overrides::new());
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg");
    }

    #[test]
    fn cull_keeps_one_photo_per_near_duplicate_cluster() {
        let kept = cull(
            &[photo("/a.jpg", 7, 50, 90), photo("/b.jpg", 7, 80, 10), photo("/c.jpg", 8, 20, 20)],
            &Overrides::new(),
        );
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|p| p.path == "/b.jpg"), "sharpness wins first");
        assert!(!kept.iter().any(|p| p.path == "/a.jpg"));
    }

    /// The documented ranking is sharpness -> capture quality -> aesthetic.
    /// A fixture where sharpness already decides it cannot test the second
    /// and third keys at all, so these are tied deliberately.
    #[test]
    fn cull_breaks_a_sharpness_tie_on_face_capture_quality() {
        let mut a = photo("/a.jpg", 7, 50, 90);
        a.capture_quality = Some(0.2);
        let mut b = photo("/b.jpg", 7, 50, 10);
        b.capture_quality = Some(0.9);
        let kept = cull(&[a, b], &Overrides::new());
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg", "capture quality outranks aesthetic");
    }

    #[test]
    fn cull_breaks_a_sharpness_and_quality_tie_on_aesthetic() {
        let kept = cull(&[photo("/a.jpg", 7, 50, 30), photo("/b.jpg", 7, 50, 80)], &Overrides::new());
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg");
    }

    /// smile_fraction is NOT in the ranking (spec 3.1): it is miscalibrated
    /// with a proven 100% false-negative rate. This test exists so that
    /// re-adding it fails loudly rather than silently reordering books.
    ///
    /// Sharpness AND capture quality are BOTH tied here -- a fixture where
    /// either already decides the winner (as an earlier version of this test
    /// did, with sharpness 90 vs 10) never reaches the third key at all, so
    /// it cannot detect a smile comparison reinserted at position 2 or 3;
    /// only one placed *before* sharpness, the least likely spot. Tying the
    /// first two keys forces the decision down to aesthetic, so any
    /// additional key spliced in ahead of it -- a reinstated smile
    /// comparison included -- changes this test's outcome. See the mutation
    /// check recorded in task-4-report.md for the proof.
    #[test]
    fn cull_ignores_smile_fraction_entirely() {
        // Photo has no smile field at all; if the ranking ever reads one,
        // deserialisation or ordering would have to change to accommodate it.
        let mut a = photo("/a.jpg", 7, 50, 10);
        a.capture_quality = Some(0.5);
        let mut b = photo("/b.jpg", 7, 50, 90);
        b.capture_quality = Some(0.5);
        let kept = cull(&[a, b], &Overrides::new());
        assert_eq!(kept[0].path, "/b.jpg", "aesthetic is the only key left that can decide it");
    }

    /// Input is deliberately UNSORTED -- a sorted fixture cannot detect an
    /// ordering bug (a real Phase 1 test failure mode).
    #[test]
    fn cull_output_order_is_stable_regardless_of_input_order() {
        let forward = cull(&[photo("/c.jpg", 3, 10, 10), photo("/a.jpg", 1, 10, 10),
                             photo("/b.jpg", 2, 10, 10)], &Overrides::new());
        let reverse = cull(&[photo("/b.jpg", 2, 10, 10), photo("/a.jpg", 1, 10, 10),
                             photo("/c.jpg", 3, 10, 10)], &Overrides::new());
        let f: Vec<_> = forward.iter().map(|p| p.path.clone()).collect();
        let r: Vec<_> = reverse.iter().map(|p| p.path.clone()).collect();
        assert_eq!(f, r);
        assert_eq!(f, vec!["/a.jpg", "/b.jpg", "/c.jpg"]);
    }

    // --- overrides: cull proposes, the user disposes ----------------------

    /// `Overrides::set(_, Auto)` must REMOVE the decision rather than store
    /// it, so "back to automatic" has exactly one representation. Two
    /// representations of one state is a bug factory for every equality
    /// check and every persisted row downstream.
    #[test]
    fn cull_overrides_setting_auto_erases_the_decision_rather_than_storing_it() {
        let mut o = Overrides::new();
        o.set("h1", Override::Include);
        assert_eq!(o.len(), 1);
        assert_eq!(o.get("h1"), Override::Include);

        o.set("h1", Override::Auto);

        assert!(o.is_empty(), "auto must not be stored: {o:?}");
        assert_eq!(o.get("h1"), Override::Auto);
        assert_eq!(o, Overrides::new(), "and must compare equal to never having decided");
    }

    /// Overrides are keyed by CONTENT HASH, not by path. Two byte-identical
    /// files in one folder are, in pixels, the same photograph, so one
    /// decision covers both -- and a photo that moves keeps its decision.
    ///
    /// The fixture gives the two photos DIFFERENT paths and different
    /// clusters: a path-keyed implementation would exclude only `/a.jpg`, so
    /// the surviving count distinguishes the two rules.
    #[test]
    fn cull_overrides_are_keyed_by_content_hash_not_by_path() {
        let mut a = photo("/a.jpg", 1, 50, 50);
        let mut b = photo("/b.jpg", 2, 50, 50);
        a.hash = "same-bytes".into();
        b.hash = "same-bytes".into();
        let mut o = Overrides::new();
        o.set("same-bytes", Override::Exclude);

        let kept = cull(&[a, b, photo("/c.jpg", 3, 50, 50)], &o);

        assert_eq!(
            kept.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/c.jpg"],
            "one decision covers every file with those bytes"
        );
    }

    /// An excluded photo never reaches the book, whatever the engine thinks
    /// of it. `/b.jpg` here is the outright winner of cluster 7 (sharpest)
    /// AND `/solo.jpg` is the only photo in its cluster, so neither exclusion
    /// can be mistaken for the cluster rule doing the work.
    #[test]
    fn cull_never_keeps_an_excluded_photo() {
        let mut o = Overrides::new();
        o.set("h-/b.jpg", Override::Exclude);
        o.set("h-/solo.jpg", Override::Exclude);

        let kept = cull(
            &[
                photo("/a.jpg", 7, 50, 90),
                photo("/b.jpg", 7, 80, 10),
                photo("/solo.jpg", 9, 70, 70),
            ],
            &o,
        );

        assert!(
            !kept.iter().any(|p| p.path == "/b.jpg"),
            "the cluster winner was excluded and must not survive: {:?}",
            kept.iter().map(|p| &p.path).collect::<Vec<_>>()
        );
        assert!(!kept.iter().any(|p| p.path == "/solo.jpg"), "an uncontested photo too");
        assert_eq!(
            kept.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/a.jpg"],
            "and the cluster's next-best frame takes the slot instead"
        );
    }

    /// Excluding the winner of a burst must PROMOTE the next-best frame, not
    /// leave the burst unrepresented. Three photos in one cluster, so
    /// "promoted the runner-up" is distinguishable from "kept everything
    /// left".
    #[test]
    fn cull_promotes_the_runner_up_when_the_cluster_winner_is_excluded() {
        let mut o = Overrides::new();
        o.set("h-/win.jpg", Override::Exclude);

        let kept = cull(
            &[
                photo("/win.jpg", 4, 90, 50),
                photo("/second.jpg", 4, 70, 50),
                photo("/third.jpg", 4, 10, 50),
            ],
            &o,
        );

        assert_eq!(
            kept.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/second.jpg"],
            "exactly one frame from the burst, and it must be the runner-up"
        );
    }

    /// **The near-duplicate case.** A genuine cluster of THREE: a two-photo
    /// cluster cannot distinguish "kept the included one" from "kept both".
    ///
    /// `/loser.jpg` is the worst frame in the cluster by every key, so
    /// nothing but the override can put it in the book. `/win.jpg` is `Auto`
    /// and still wins its cluster -- an `Include` is additive, it does not
    /// displace a photo the user never touched. `/mid.jpg` is the control:
    /// it lost the cluster and was not asked for, so it must stay out.
    #[test]
    fn cull_keeps_an_included_photo_that_lost_its_near_duplicate_cluster() {
        let mut o = Overrides::new();
        o.set("h-/loser.jpg", Override::Include);

        let kept = cull(
            &[
                photo("/win.jpg", 4, 90, 90),
                photo("/mid.jpg", 4, 50, 50),
                photo("/loser.jpg", 4, 10, 10),
            ],
            &o,
        );

        let paths: Vec<&str> = kept.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"/loser.jpg"), "the explicit choice must survive: {paths:?}");
        assert!(paths.contains(&"/win.jpg"), "an Include must not displace the Auto winner");
        assert!(!paths.contains(&"/mid.jpg"), "the cluster rule still governs the rest");
        assert_eq!(paths.len(), 2, "{paths:?}");
    }

    /// Two frames from one burst, both explicitly wanted: the cluster rule
    /// yields to an explicit choice, so BOTH appear. Four photos in the
    /// cluster, so this cannot pass by keeping everything -- the two
    /// untouched frames still compete for one slot between them.
    #[test]
    fn cull_keeps_every_included_frame_from_the_same_burst() {
        let mut o = Overrides::new();
        o.set("h-/pick1.jpg", Override::Include);
        o.set("h-/pick2.jpg", Override::Include);

        let kept = cull(
            &[
                photo("/auto-win.jpg", 4, 95, 95),
                photo("/auto-lose.jpg", 4, 60, 60),
                photo("/pick1.jpg", 4, 20, 20),
                photo("/pick2.jpg", 4, 10, 10),
            ],
            &o,
        );

        let paths: Vec<&str> = kept.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"/pick1.jpg"), "{paths:?}");
        assert!(paths.contains(&"/pick2.jpg"), "{paths:?}");
        assert!(paths.contains(&"/auto-win.jpg"), "{paths:?}");
        assert!(!paths.contains(&"/auto-lose.jpg"), "the untouched frames still cull to one");
        assert_eq!(paths.len(), 3, "{paths:?}");
    }

    /// `is_utility` is the engine's other veto, and an `Include` overrides it
    /// too: a screenshot the user deliberately wants in the book is their
    /// call, not the classifier's.
    #[test]
    fn cull_keeps_an_included_utility_image() {
        let mut screenshot = photo("/shot.png", 1, 90, 90);
        screenshot.is_utility = true;
        let mut other = photo("/other.png", 2, 90, 90);
        other.is_utility = true;
        let mut o = Overrides::new();
        o.set("h-/shot.png", Override::Include);

        let kept = cull(&[screenshot, other], &o);

        assert_eq!(
            kept.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/shot.png"],
            "the included utility image survives and the untouched one does not"
        );
    }

    /// An empty override map must reproduce the engine-only verdict exactly.
    /// This is what makes "overrides are additive to the existing rule"
    /// checkable rather than assumed.
    #[test]
    fn cull_with_no_overrides_is_the_engine_verdict_unchanged() {
        let mut utility = photo("/u.jpg", 5, 99, 99);
        utility.is_utility = true;
        let photos = vec![
            photo("/a.jpg", 7, 50, 90),
            photo("/b.jpg", 7, 80, 10),
            photo("/c.jpg", 8, 20, 20),
            utility,
        ];

        let kept = cull(&photos, &Overrides::new());

        assert_eq!(
            kept.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/b.jpg", "/c.jpg"]
        );
    }

    /// Output order stays path-sorted once includes are mixed in, rather
    /// than appending them after the engine's own survivors -- `pack` cuts
    /// chapters in this order, so an unsorted tail would reorder the book.
    /// The fixture's include sorts FIRST, so an append-at-the-end
    /// implementation is distinguishable.
    #[test]
    fn cull_returns_includes_in_path_order_with_the_rest() {
        let mut o = Overrides::new();
        o.set("h-/aaa.jpg", Override::Include);

        let kept = cull(
            &[photo("/zzz.jpg", 1, 50, 50), photo("/mmm.jpg", 2, 50, 50), photo("/aaa.jpg", 1, 10, 10)],
            &o,
        );

        assert_eq!(
            kept.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/aaa.jpg", "/mmm.jpg", "/zzz.jpg"]
        );
    }

    /// `Overrides` crosses to the webview and back as a plain object. Pinned
    /// as literal wire bytes: a `rename_all` change or a dropped
    /// `transparent` is otherwise a silent `undefined` on the TypeScript
    /// side. The companion fixture assertion lives in `commands.rs`.
    #[test]
    fn cull_overrides_serialise_as_a_plain_hash_to_lowercase_state_object() {
        let mut o = Overrides::new();
        o.set("hb", Override::Exclude);
        o.set("ha", Override::Include);

        assert_eq!(
            serde_json::to_string(&o).unwrap(),
            r#"{"ha":"include","hb":"exclude"}"#
        );
        let back: Overrides = serde_json::from_str(r#"{"ha":"include","hb":"exclude"}"#).unwrap();
        assert_eq!(back, o);
        // An explicit "auto" on the wire is accepted and normalises away, so
        // a webview that sends the default state cannot create a second
        // representation of "no decision".
        let auto: Overrides = serde_json::from_str(r#"{"hc":"auto"}"#).unwrap();
        assert_eq!(auto.get("hc"), Override::Auto);
        assert_eq!(auto, Overrides::new(), "an explicit auto must not be stored");
    }

    // --- from_features: the boundary between Swift's wire JSON and the engine

    #[test]
    fn cull_from_features_reads_boxes_and_palette() {
        let v = serde_json::json!({
            "path": "/p/a.jpg", "hash": "abc", "width": 4032, "height": 3024,
            "isUtility": false, "aestheticPct": 80, "sharpnessPct": 60,
            "nearDupCluster": 2, "eventCluster": 1,
            "faces": [{"box":[0.1,0.2,0.3,0.4],"captureQuality":0.7}],
            "faceAreaFraction": 0.12,
            "saliencyBox": [0.2,0.1,0.5,0.6],
            "palette": [{"r":0.5,"g":0.2,"b":0.1,"weight":0.6}],
            "sceneTags": ["beach", "sunset"],
            "clippedLow": 0.0, "clippedHigh": 0.0
        });
        let p = from_features(&v).expect("well-formed record");
        assert_eq!(p.faces.len(), 1);
        assert!((p.faces[0].box_.w - 0.3).abs() < 1e-9);
        assert!((p.saliency_box.unwrap().h - 0.6).abs() < 1e-9);
        assert_eq!(p.palette.len(), 1);
        assert_eq!(p.capture_quality, Some(0.7));
    }

    /// Vision returns no saliency on some images; the engine must degrade,
    /// not panic.
    #[test]
    fn cull_from_features_tolerates_a_missing_saliency_box() {
        let v = serde_json::json!({
            "path": "/p/a.jpg", "hash": "abc", "width": 4032, "height": 3024,
            "isUtility": false, "aestheticPct": 80, "sharpnessPct": 60,
            "nearDupCluster": 2, "eventCluster": 1,
            "faces": [], "faceAreaFraction": 0.0, "palette": [], "sceneTags": [],
            "clippedLow": 0.0, "clippedHigh": 0.0
        });
        let p = from_features(&v).expect("well-formed record");
        assert!(p.saliency_box.is_none());
        assert_eq!(p.capture_quality, None);
    }

    #[test]
    fn cull_from_features_reads_a_gps_fix_and_tolerates_none() {
        let with = |exif: serde_json::Value| {
            let mut v = full_record();
            v["exif"] = exif;
            from_features(&v).expect("GPS is optional, never a refusal").location
        };
        let fix = with(serde_json::json!({"latitude": 35.0116, "longitude": 135.7681})).unwrap();
        assert_eq!((fix.lat(), fix.lon()), (35.0116, 135.7681));
        assert_eq!(with(serde_json::json!({"latitude": -33.86, "longitude": 151.2})).map(|p| p.lat()), Some(-33.86));
        assert_eq!(with(serde_json::json!({})), None);
        assert_eq!(with(serde_json::json!({"latitude": 35.0})), None, "half a fix is no fix");
        assert_eq!(with(serde_json::json!({"latitude": -0.0, "longitude": -0.0})), None, "DJI's no-lock value");
        let mut no_exif = full_record();
        no_exif.as_object_mut().unwrap().remove("exif");
        assert_eq!(from_features(&no_exif).unwrap().location, None);
    }

    /// A complete finalized record, as the only fixture the strictness tests
    /// below mutate. Every key here is one whose absence changes the book.
    fn full_record() -> serde_json::Value {
        serde_json::json!({
            "path": "/p/a.jpg", "hash": "abc", "width": 4032, "height": 3024,
            "isUtility": false, "aestheticPct": 80, "sharpnessPct": 60,
            "nearDupCluster": 2, "eventCluster": 1,
            "faces": [{"box":[0.1,0.2,0.3,0.4],"captureQuality":0.7}],
            "faceAreaFraction": 0.12,
            "saliencyBox": [0.2,0.1,0.5,0.6],
            "palette": [{"r":0.5,"g":0.2,"b":0.1,"weight":0.6}],
            "sceneTags": ["beach", "sunset"],
            "exif": { "captureDate": 1_700_000_000.0 },
            "clippedLow": 0.25,
            "clippedHigh": 0.125,
            "featurePrint": "AACAPwAAIMA="
        })
    }

    /// Alias for `full_record`, matched to the name Task 9's brief uses --
    /// kept as an alias rather than a rename so existing call sites and their
    /// history stay intact.
    fn complete_record() -> serde_json::Value {
        full_record()
    }

    /// `sceneTags` is always emitted by Swift, so its ABSENCE is a pipeline bug
    /// and must be refused. `exif.captureDate` is legitimately null on a photo
    /// with no EXIF date, so its absence must be tolerated. Getting these two
    /// the wrong way round either fails every book containing one scanned
    /// photo, or silently degrades the diversity term to a constant.
    #[test]
    fn cull_from_features_requires_scene_tags_but_tolerates_a_missing_capture_date() {
        let mut v = complete_record();
        v["sceneTags"] = serde_json::json!(["beach", "sunset"]);
        v["exif"] = serde_json::json!({ "captureDate": 1_700_000_000.0 });
        let photo = from_features(&v).expect("a complete record parses");
        assert_eq!(photo.scene_tags, vec!["beach".to_string(), "sunset".to_string()]);
        assert_eq!(photo.captured_at, Some(1_700_000_000));

        // A null capture date is a real answer, not a malformed record.
        let mut no_date = complete_record();
        no_date["sceneTags"] = serde_json::json!([]);
        no_date["exif"] = serde_json::json!({ "captureDate": serde_json::Value::Null });
        let photo = from_features(&no_date).expect("a photo with no EXIF date still parses");
        assert_eq!(photo.captured_at, None);
        assert!(photo.scene_tags.is_empty(), "an empty tag list is a real answer");

        // A MISSING sceneTags key is not.
        let mut no_tags = complete_record();
        no_tags.as_object_mut().expect("object").remove("sceneTags");
        assert!(
            from_features(&no_tags).is_none(),
            "a record with no sceneTags key must be refused"
        );
    }

    /// The literal Swift pins in `featurePrintEncodesFloat32LittleEndianAsBase64`.
    #[test]
    fn cull_decodes_the_feature_print_swift_pins() {
        assert_eq!(decode_feature_print("AACAPwAAIMA="), Some(vec![1.0, -2.5]));
    }

    #[test]
    fn cull_feature_print_decode_refuses_a_truncated_element_or_non_base64() {
        // Six bytes: one Float32 and half of another.
        assert_eq!(decode_feature_print("AACAPwAA"), None);
        assert_eq!(decode_feature_print("not base64!"), None);
    }

    #[test]
    fn cull_from_features_reads_clipping_and_the_feature_print() {
        let p = from_features(&full_record()).expect("the fixture parses");
        assert_eq!(p.clipped_low, 0.25);
        assert_eq!(p.clipped_high, 0.125);
        assert_eq!(p.feature_print, Some(vec![1.0, -2.5]));
    }

    /// Swift omits `featurePrint` when Vision returns none, so absence is a
    /// real answer. A value that does not decode is corruption.
    #[test]
    fn cull_from_features_tolerates_an_absent_feature_print_but_not_a_corrupt_one() {
        let p = from_features(&without("featurePrint")).expect("absent is legitimate");
        assert_eq!(p.feature_print, None);

        let mut v = full_record();
        v["featurePrint"] = "AACAPwAA".into();
        assert!(from_features(&v).is_none());
    }

    fn without(key: &str) -> serde_json::Value {
        let mut v = full_record();
        v.as_object_mut().unwrap().remove(key).unwrap_or_else(|| panic!("{key} not in fixture"));
        v
    }

    /// Every field whose ABSENCE would silently corrupt the printed book,
    /// one assertion each. These all used to `unwrap_or_default()`, so the
    /// record parsed and the damage surfaced only in the finished book:
    /// no `nearDupCluster` and `cull` keeps one photo for twenty pages; no
    /// `faces` and every gutter/safe-margin rejection goes inert and crops
    /// re-centre off the subject.
    #[test]
    fn cull_from_features_rejects_a_record_missing_a_book_critical_field() {
        assert!(from_features(&full_record()).is_some(), "the fixture itself must parse");
        for key in [
            "path",
            "hash",
            "width",
            "height",
            "isUtility",
            "aestheticPct",
            "sharpnessPct",
            "nearDupCluster",
            "eventCluster",
            "faces",
            "faceAreaFraction",
            "palette",
            "sceneTags",
            "clippedLow",
            "clippedHigh",
        ] {
            assert!(
                from_features(&without(key)).is_none(),
                "a record with no `{key}` must be refused, not defaulted"
            );
        }
    }

    /// The distinction the strictness must NOT flatten: an empty array is a
    /// real answer (nobody in the photo, no dominant colours), a missing key
    /// is a broken pipeline. Only the key is required.
    #[test]
    fn cull_from_features_accepts_empty_faces_and_palette_but_not_absent_ones() {
        let mut v = full_record();
        v["faces"] = serde_json::json!([]);
        v["palette"] = serde_json::json!([]);
        let p = from_features(&v).expect("empty is a real answer");
        assert!(p.faces.is_empty());
        assert!(p.palette.is_empty());
        assert_eq!(p.capture_quality, None);

        assert!(from_features(&without("faces")).is_none());
        assert!(from_features(&without("palette")).is_none());
    }

    /// Vision returns face boxes that leave the image. Nine of the 1164 faces
    /// in the author's own cache do, by as much as 0.15 of the frame:
    /// `MY_03204.ARW` at x 0.495 w 0.5434 (right edge 1.0385) and
    /// `MY_03978.ARW` at x -0.1493 w 1.0659 are the two below, copied
    /// verbatim. `VisionAnalyzer.topLeft` is a straight origin flip and is not
    /// the culprit -- Vision itself extrapolates a box past the frame.
    ///
    /// It is a hard failure downstream. Every crop window lies inside the
    /// frame by construction, so an overhanging box can never be CONTAINED by
    /// a crop while it does intersect one, which is precisely
    /// `score::rejects`'s definition of `FaceClipped` -- in every slot of
    /// every template. The photo becomes unplaceable anywhere and takes its
    /// whole spread down with it.
    ///
    /// So the frame is enforced here, at the parse boundary, where the
    /// external data arrives: a box describing a region of the image cannot
    /// leave the image. Clamping in Rust rather than in Swift also repairs
    /// the rows already sitting in the features cache, which a sidecar fix
    /// would need an `ANALYZER_VERSION` bump and a full re-analysis to reach.
    #[test]
    fn cull_clamps_a_face_box_vision_pushed_past_the_frame_edge() {
        let frame = Rect::new(0.0, 0.0, 1.0, 1.0);
        let mut v = full_record();
        v["faces"] = serde_json::json!([
            {"box": [0.495, 0.4455, 0.5434, 0.3633], "captureQuality": 0.7},
            {"box": [-0.1493, 0.2365, 1.0659, 0.7127], "captureQuality": 0.6},
            {"box": [0.30, 0.40, 0.12, 0.20], "captureQuality": 0.5}
        ]);
        let p = from_features(&v).expect("an overhanging box is a real Vision answer, not corruption");
        assert_eq!(p.faces.len(), 3, "a clamped face is still a face and still scores");
        for f in &p.faces {
            assert!(frame.contains(&f.box_), "face {:?} still leaves the frame", f.box_);
        }
        // Clamped to the frame, not rescaled or recentred: only the overhang
        // goes. `intersect` rebuilds a rect from its edges, so the clamped
        // boxes are compared to within float reconstruction error and the
        // UNTOUCHED one is compared exactly -- that exactness is the claim
        // that an ordinary photo's data is not perturbed by this.
        let near = |a: Rect, b: Rect| {
            let d = (a.x - b.x).abs().max((a.y - b.y).abs()).max((a.w - b.w).abs()).max((a.h - b.h).abs());
            assert!(d < 1e-12, "{a:?} is not {b:?}");
        };
        near(p.faces[0].box_, Rect::new(0.495, 0.4455, 0.505, 0.3633));
        near(p.faces[1].box_, Rect::new(0.0, 0.2365, 0.9166, 0.7127));
        assert_eq!(p.faces[2].box_, Rect::new(0.30, 0.40, 0.12, 0.20), "an in-frame box is untouched");

        // The cache path is the one that has to repair the existing rows.
        let cached = from_cached_features(&v).expect("the cached shape parses");
        assert!(cached.faces.iter().all(|f| frame.contains(&f.box_)));
    }

    /// A box with no part of it in the frame is not a face in this photo, so
    /// it is dropped rather than clamped to a degenerate sliver that the
    /// gutter and safe-margin rules would then reason about.
    #[test]
    fn cull_drops_a_face_box_that_lies_entirely_outside_the_frame() {
        let mut v = full_record();
        v["faces"] = serde_json::json!([
            {"box": [1.4, 0.2, 0.3, 0.3], "captureQuality": 0.9},
            {"box": [0.30, 0.40, 0.12, 0.20], "captureQuality": 0.5}
        ]);
        let p = from_features(&v).expect("a record with an off-frame box still parses");
        assert_eq!(p.faces.len(), 1, "only the face that is in the picture survives");
        assert_eq!(p.faces[0].box_, Rect::new(0.30, 0.40, 0.12, 0.20));
        assert_eq!(
            p.capture_quality,
            Some(0.5),
            "the dropped face must not keep voting on the photo's capture quality"
        );
    }

    /// A face entry without a `box` used to be dropped from the list by
    /// `filter_map`, so a photo with three faces could arrive carrying two
    /// and nothing would say so. Swift declares `box` non-optional, so the
    /// only way to see one is corruption -- refuse the record.
    #[test]
    fn cull_from_features_rejects_a_face_with_no_box_rather_than_dropping_it() {
        let mut v = full_record();
        v["faces"] = serde_json::json!([
            {"box":[0.1,0.2,0.3,0.4],"captureQuality":0.7},
            {"captureQuality":0.9}
        ]);
        assert!(from_features(&v).is_none());
    }

    /// `saliencyBox` is the one field Swift may legitimately omit --
    /// `[Double]?` plus `encodeIfPresent`, and Vision really does return no
    /// attention box on some images. Requiring it would refuse valid photos.
    #[test]
    fn cull_from_features_still_accepts_a_record_with_no_saliency_box() {
        let p = from_features(&without("saliencyBox")).expect("absent saliency is legitimate");
        assert!(p.saliency_box.is_none());
    }

    /// The export path reads Swift's record straight out of SQLite, where the
    /// whole-set derivations were never written. That record must still
    /// parse -- through the constructor that says so -- while the strict one
    /// refuses it.
    #[test]
    fn cull_from_cached_features_accepts_a_record_without_the_whole_set_derivations() {
        let mut v = full_record();
        for key in ["aestheticPct", "sharpnessPct", "nearDupCluster", "eventCluster"] {
            v.as_object_mut().unwrap().remove(key);
        }
        assert!(from_features(&v).is_none(), "the strict parser must refuse it");
        let p = from_cached_features(&v).expect("the cache shape is legitimate here");
        assert_eq!(p.near_dup_cluster, 0);
        assert_eq!(p.aesthetic_pct, 0);
    }

    /// `from_cached_features` relaxes ONLY the derivations. A cached record
    /// with no `faces` key is still broken, and export reads faces.
    #[test]
    fn cull_from_cached_features_is_still_strict_about_swift_emitted_fields() {
        assert!(from_cached_features(&without("faces")).is_none());
        assert!(from_cached_features(&without("faceAreaFraction")).is_none());
        assert!(from_cached_features(&without("isUtility")).is_none());
        assert!(from_cached_features(&without("width")).is_none());
    }
}
