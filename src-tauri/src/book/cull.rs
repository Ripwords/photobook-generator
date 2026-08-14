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
}

impl Photo {
    /// Real-world aspect ratio of the photo itself, width over height.
    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height as f64
    }
}

fn rect_from(v: &serde_json::Value) -> Option<Rect> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    Some(Rect::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?, a[3].as_f64()?))
}

/// Builds a `Photo` from one finalized feature record. Returns `None` when a
/// required field is missing or the wrong type, which the caller reports as
/// a skipped photo rather than failing the whole book.
pub fn from_features(v: &serde_json::Value) -> Option<Photo> {
    let faces: Vec<Face> = v["faces"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    Some(Face {
                        box_: rect_from(&f["box"])?,
                        capture_quality: f["captureQuality"].as_f64(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let capture_quality = faces
        .iter()
        .filter_map(|f| f.capture_quality)
        .max_by(|a, b| a.total_cmp(b));

    let palette = v["palette"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(PaletteColor {
                        r: c["r"].as_f64()?,
                        g: c["g"].as_f64()?,
                        b: c["b"].as_f64()?,
                        weight: c["weight"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(Photo {
        path: v["path"].as_str()?.to_string(),
        hash: v["hash"].as_str()?.to_string(),
        width: v["width"].as_u64()? as u32,
        height: v["height"].as_u64()? as u32,
        is_utility: v["isUtility"].as_bool().unwrap_or(false),
        aesthetic_pct: v["aestheticPct"].as_u64().unwrap_or(0) as u8,
        sharpness_pct: v["sharpnessPct"].as_u64().unwrap_or(0) as u8,
        near_dup_cluster: v["nearDupCluster"].as_u64().unwrap_or(0) as u32,
        event_cluster: v["eventCluster"].as_u64().unwrap_or(0) as u32,
        faces,
        face_area_fraction: v["faceAreaFraction"].as_f64().unwrap_or(0.0),
        saliency_box: rect_from(&v["saliencyBox"]),
        palette,
        capture_quality,
    })
}

/// The single authority on which photo survives.
///
/// Drops `is_utility`, then keeps one winner per near-duplicate cluster,
/// ranked sharpness percentile -> face capture quality -> aesthetic
/// percentile.
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
pub fn cull(photos: &[Photo]) -> Vec<Photo> {
    use std::collections::BTreeMap;

    let mut best: BTreeMap<u32, &Photo> = BTreeMap::new();
    for photo in photos.iter().filter(|p| !p.is_utility) {
        best.entry(photo.near_dup_cluster)
            .and_modify(|incumbent| {
                if beats(photo, incumbent) {
                    *incumbent = photo;
                }
            })
            .or_insert(photo);
    }

    // `BTreeMap` iterates by cluster id, so the output order depends only on
    // cluster ids, never on input order -- the property the ordering test
    // pins with deliberately unsorted input.
    let mut kept: Vec<Photo> = best.into_values().cloned().collect();
    kept.sort_by(|a, b| a.path.cmp(&b.path));
    kept
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
        }
    }

    #[test]
    fn cull_drops_utility_images() {
        let mut u = photo("/a.jpg", 0, 90, 90);
        u.is_utility = true;
        let kept = cull(&[u, photo("/b.jpg", 1, 10, 10)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg");
    }

    #[test]
    fn cull_keeps_one_photo_per_near_duplicate_cluster() {
        let kept = cull(&[
            photo("/a.jpg", 7, 50, 90),
            photo("/b.jpg", 7, 80, 10),
            photo("/c.jpg", 8, 20, 20),
        ]);
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
        let kept = cull(&[a, b]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg", "capture quality outranks aesthetic");
    }

    #[test]
    fn cull_breaks_a_sharpness_and_quality_tie_on_aesthetic() {
        let kept = cull(&[photo("/a.jpg", 7, 50, 30), photo("/b.jpg", 7, 50, 80)]);
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
        let kept = cull(&[a, b]);
        assert_eq!(kept[0].path, "/b.jpg", "aesthetic is the only key left that can decide it");
    }

    /// Input is deliberately UNSORTED -- a sorted fixture cannot detect an
    /// ordering bug (a real Phase 1 test failure mode).
    #[test]
    fn cull_output_order_is_stable_regardless_of_input_order() {
        let forward = cull(&[photo("/c.jpg", 3, 10, 10), photo("/a.jpg", 1, 10, 10),
                             photo("/b.jpg", 2, 10, 10)]);
        let reverse = cull(&[photo("/b.jpg", 2, 10, 10), photo("/a.jpg", 1, 10, 10),
                             photo("/c.jpg", 3, 10, 10)]);
        let f: Vec<_> = forward.iter().map(|p| p.path.clone()).collect();
        let r: Vec<_> = reverse.iter().map(|p| p.path.clone()).collect();
        assert_eq!(f, r);
        assert_eq!(f, vec!["/a.jpg", "/b.jpg", "/c.jpg"]);
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
            "palette": [{"r":0.5,"g":0.2,"b":0.1,"weight":0.6}]
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
            "faces": [], "faceAreaFraction": 0.0, "palette": []
        });
        let p = from_features(&v).expect("well-formed record");
        assert!(p.saliency_box.is_none());
        assert_eq!(p.capture_quality, None);
    }
}
