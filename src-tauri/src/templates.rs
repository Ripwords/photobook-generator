//! Loading the authored spread templates and decomposing them into the
//! page layouts the engine actually reasons about.
//!
//! The spread JSON stays the authoring format because cross-fold ALIGNMENT
//! (grid rows, footer bands) is real design intent that survives only if the
//! two pages are authored together. The engine's atom is the page because
//! Pixajoy's boxes are page-local. Decomposition at load reconciles the two.

use crate::geometry::{spread_to_page, BleedEdge, Rect, Side};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Hero,
    Support,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Sparse,
    Medium,
    Dense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Energy {
    Calm,
    Neutral,
    Lively,
}

/// The third pacing axis, derived rather than authored: a page whose slots
/// declare any bleed reads as edge-to-edge, one whose slots do not reads as
/// matted. The user wants both present and alternating, and deriving it
/// avoids editing 19 existing files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeTreatment {
    Bleed,
    Margin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub rect: Rect,
    pub role: Role,
    pub bleed: Vec<BleedEdge>,
    pub aspect_pref: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    pub side: Side,
    pub slots: Vec<Slot>,
    pub edge_treatment: EdgeTreatment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpreadTemplate {
    pub id: String,
    pub left: PageLayout,
    pub right: PageLayout,
    pub density: Density,
    pub energy: Energy,
}

impl SpreadTemplate {
    pub fn photo_count(&self) -> usize {
        self.left.slots.len() + self.right.slots.len()
    }
}

#[derive(Debug)]
pub enum TemplateError {
    Io(String),
    Parse(String),
    SpansFold { template: String, slot: usize },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::Io(m) => write!(f, "template io error: {m}"),
            TemplateError::Parse(m) => write!(f, "template parse error: {m}"),
            TemplateError::SpansFold { template, slot } => write!(
                f,
                "template '{template}' slot {slot} spans the fold; Pixajoy picture \
                 boxes are page-local and cannot cross it"
            ),
        }
    }
}

// --- the on-disk shape, deliberately separate from the in-memory shape so
// the JSON can stay spread-normalised while the engine sees pages.

#[derive(Deserialize)]
struct RawSlot {
    rect: [f64; 4],
    role: Role,
    bleed: Vec<BleedEdge>,
    aspect_pref: [f64; 2],
}

#[derive(Deserialize)]
struct RawTemplate {
    id: String,
    slots: Vec<RawSlot>,
    min_photos: usize,
    max_photos: usize,
    density: Density,
    energy: Energy,
}

#[derive(Debug)]
pub struct Library {
    pub spreads: Vec<SpreadTemplate>,
}

impl Library {
    pub fn load(dir: &Path) -> Result<Library, TemplateError> {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| TemplateError::Io(e.to_string()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("json")
                    && p.file_name().and_then(|s| s.to_str()) != Some("weights.json")
            })
            .collect();
        // Sorted so the library order -- and therefore every tie-break that
        // falls through to it -- is stable across machines and filesystems.
        files.sort();

        let mut spreads = Vec::with_capacity(files.len());
        for path in files {
            let text =
                std::fs::read_to_string(&path).map_err(|e| TemplateError::Io(e.to_string()))?;
            let raw: RawTemplate =
                serde_json::from_str(&text).map_err(|e| TemplateError::Parse(e.to_string()))?;
            spreads.push(decompose(raw)?);
        }
        Ok(Library { spreads })
    }

    /// Templates are exact-count, so eligibility is equality, not a range.
    pub fn spreads_with(&self, n: usize) -> Vec<&SpreadTemplate> {
        self.spreads.iter().filter(|t| t.photo_count() == n).collect()
    }

    /// Every half of every template. Pages 1 and N draw from this: a half is
    /// by construction a valid page layout, so the single pages get a
    /// library for free and one that matches the book's style.
    pub fn page_half_pool(&self) -> Vec<&PageLayout> {
        self.spreads.iter().flat_map(|t| [&t.left, &t.right]).collect()
    }
}

fn edge_treatment(slots: &[Slot]) -> EdgeTreatment {
    if slots.iter().any(|s| !s.bleed.is_empty()) {
        EdgeTreatment::Bleed
    } else {
        EdgeTreatment::Margin
    }
}

fn decompose(raw: RawTemplate) -> Result<SpreadTemplate, TemplateError> {
    debug_assert_eq!(
        raw.min_photos, raw.max_photos,
        "templates are exact-count by contract"
    );
    let mut left = Vec::new();
    let mut right = Vec::new();

    for (i, rs) in raw.slots.iter().enumerate() {
        let spread_rect = Rect::new(rs.rect[0], rs.rect[1], rs.rect[2], rs.rect[3]);
        let (side, page_rect) = spread_to_page(&spread_rect).ok_or(TemplateError::SpansFold {
            template: raw.id.clone(),
            slot: i,
        })?;
        let slot = Slot {
            rect: page_rect,
            role: rs.role,
            bleed: rs.bleed.clone(),
            aspect_pref: (rs.aspect_pref[0], rs.aspect_pref[1]),
        };
        match side {
            Side::Left => left.push(slot),
            Side::Right => right.push(slot),
        }
    }

    Ok(SpreadTemplate {
        id: raw.id,
        left: PageLayout { side: Side::Left, edge_treatment: edge_treatment(&left), slots: left },
        right: PageLayout {
            side: Side::Right,
            edge_treatment: edge_treatment(&right),
            slots: right,
        },
        density: raw.density,
        energy: raw.energy,
    })
}

/// Soft-term weights, hot-reloadable from `templates/weights.json` so taste
/// is tunable without a rebuild. Hard constraints are NOT weighted and are
/// deliberately absent here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    pub aspect_fit: f64,
    pub saliency_retention: f64,
    pub face_area_retention: f64,
    pub hero_match: f64,
    pub resolution_headroom: f64,
    pub palette_harmony: f64,
    pub variety: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            aspect_fit: 1.0,
            saliency_retention: 0.8,
            face_area_retention: 1.2,
            hero_match: 0.6,
            resolution_headroom: 0.4,
            palette_harmony: 0.2,
            variety: 0.5,
        }
    }
}

impl Weights {
    pub fn load(path: &Path) -> Result<Weights, TemplateError> {
        let text = std::fs::read_to_string(path).map_err(|e| TemplateError::Io(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| TemplateError::Parse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_template(dir: &std::path::Path, name: &str, json: &str) {
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        f.write_all(json.as_bytes()).unwrap();
    }

    /// Two slots, one per page, with DIFFERENT extents so a left/right mixup
    /// is detectable. Never symmetric: a mirrored fixture passes under a
    /// side-swap bug.
    const TWO_UP: &str = r#"{
        "id": "t-two-up",
        "slots": [
            {"rect":[0.05,0.10,0.30,0.60],"role":"hero","bleed":[],"aspect_pref":[2.2,2.4]},
            {"rect":[0.60,0.20,0.20,0.40],"role":"support","bleed":[],"aspect_pref":[2.4,2.6]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "medium", "energy": "calm"
    }"#;

    #[test]
    fn templates_loads_and_decomposes_into_two_page_layouts() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();

        assert_eq!(lib.spreads.len(), 1);
        let t = &lib.spreads[0];
        assert_eq!(t.id, "t-two-up");
        assert_eq!(t.left.slots.len(), 1);
        assert_eq!(t.right.slots.len(), 1);
        assert_eq!(t.left.side, Side::Left);
        assert_eq!(t.right.side, Side::Right);
        assert_eq!(t.photo_count(), 2);
    }

    #[test]
    fn templates_converts_slot_rects_to_page_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();
        let t = &lib.spreads[0];

        // 0.05 on the spread -> 0.10 on the left page; width 0.30 -> 0.60.
        assert!((t.left.slots[0].rect.x - 0.10).abs() < 1e-9);
        assert!((t.left.slots[0].rect.w - 0.60).abs() < 1e-9);
        // 0.60 on the spread -> (0.60-0.5)/0.5 = 0.20 on the right page.
        assert!((t.right.slots[0].rect.x - 0.20).abs() < 1e-9);
        assert!((t.right.slots[0].rect.w - 0.40).abs() < 1e-9);
    }

    #[test]
    fn templates_rejects_a_fold_spanning_slot() {
        let dir = tempfile::tempdir().unwrap();
        write_template(
            dir.path(),
            "bad.json",
            r#"{"id":"bad","slots":[
                {"rect":[0.2,0.1,0.6,0.8],"role":"hero","bleed":[],"aspect_pref":[3.7,3.9]}],
                "text_zones":[],"min_photos":1,"max_photos":1,
                "density":"sparse","energy":"calm"}"#,
        );
        match Library::load(dir.path()) {
            Err(TemplateError::SpansFold { template, slot }) => {
                assert_eq!(template, "bad");
                assert_eq!(slot, 0);
            }
            other => panic!("expected SpansFold, got {other:?}"),
        }
    }

    #[test]
    fn templates_selects_spreads_by_exact_photo_count() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();
        assert_eq!(lib.spreads_with(2).len(), 1);
        assert_eq!(lib.spreads_with(3).len(), 0);
        // The gap that matters: no 5-photo template exists yet (spec 6.3).
        assert_eq!(lib.spreads_with(5).len(), 0);
    }

    #[test]
    fn templates_page_half_pool_yields_both_halves() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();
        let pool = lib.page_half_pool();
        assert_eq!(pool.len(), 2);
        assert!(pool.iter().any(|p| p.side == Side::Left));
        assert!(pool.iter().any(|p| p.side == Side::Right));
    }

    /// Edge treatment is DERIVED, not authored -- it is the third pacing
    /// axis and must not require touching 19 existing files.
    #[test]
    fn templates_derives_edge_treatment_from_the_bleed_arrays() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        write_template(
            dir.path(),
            "t-bleed.json",
            r#"{"id":"t-bleed","slots":[
                {"rect":[0.0,0.0,0.5,1.0],"role":"hero","bleed":["left","top","bottom"],
                 "aspect_pref":[1.2,1.3]},
                {"rect":[0.6,0.2,0.2,0.4],"role":"support","bleed":[],"aspect_pref":[2.4,2.6]}],
                "text_zones":[],"min_photos":2,"max_photos":2,
                "density":"sparse","energy":"lively"}"#,
        );
        let lib = Library::load(dir.path()).unwrap();
        let bleedy = lib.spreads.iter().find(|t| t.id == "t-bleed").unwrap();
        let plain = lib.spreads.iter().find(|t| t.id == "t-two-up").unwrap();
        assert_eq!(bleedy.left.edge_treatment, EdgeTreatment::Bleed);
        assert_eq!(bleedy.right.edge_treatment, EdgeTreatment::Margin);
        assert_eq!(plain.left.edge_treatment, EdgeTreatment::Margin);
    }

    #[test]
    fn templates_weights_fall_back_to_defaults_when_the_file_is_absent() {
        let w = Weights::load(std::path::Path::new("/nonexistent/weights.json"));
        assert!(w.is_err());
        let d = Weights::default();
        assert!(d.aspect_fit > 0.0);
        assert!(d.face_area_retention > d.saliency_retention,
            "faces must outweigh generic saliency");
    }

    /// Guards the real authored library, not a fixture: every file in
    /// `templates/` must decompose.
    ///
    /// This and the two below used to be `#[ignore]`d "so the unit tests stay
    /// hermetic", and `bun run test:rust` passes no `--ignored`. There is no
    /// CI. So the only guards on the authored library -- including the 1..=6
    /// group-size coverage the whole packer rests on -- ran nowhere at all.
    /// Measured before un-ignoring: all three together finish in under 10 ms
    /// (three loads of 37 small JSON files from a path derived from
    /// `CARGO_MANIFEST_DIR`, which is always present in a checkout). There
    /// was no cost to weigh against the coverage.
    #[test]
    fn templates_real_library_decomposes_cleanly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = Library::load(&dir).expect("the real library must decompose");
        assert!(lib.spreads.len() >= 19, "got {}", lib.spreads.len());
        assert!(
            lib.page_half_pool().len() >= 38,
            "every spread contributes two halves"
        );
    }

    /// The packer can only emit a group of size N if the library holds a
    /// spread with exactly N slots -- `pack` refuses to fabricate a size it
    /// cannot build. A hole at 5 does not degrade the book, it makes a run of
    /// five keepers unplaceable, so the covered range is a library-level
    /// invariant rather than a nice-to-have.
    #[test]
    fn templates_real_library_covers_every_group_size_from_one_to_six() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = Library::load(&dir).expect("the real library must decompose");

        let mut sizes: Vec<usize> =
            lib.spreads.iter().map(|t| t.photo_count()).collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
        sizes.sort_unstable();

        for n in 1..=6 {
            assert!(
                sizes.contains(&n),
                "no template builds a {n}-photo spread; buildable sizes are {sizes:?}"
            );
        }
        // Gapless, not merely covering 1..=6: a hole above 6 is the same bug
        // one size up, and the packer walks the whole list.
        for pair in sizes.windows(2) {
            assert_eq!(
                pair[1],
                pair[0] + 1,
                "gap in buildable sizes between {} and {}: {sizes:?}",
                pair[0],
                pair[1]
            );
        }
    }

    /// Edge treatment is the third pacing axis, and pacing can only alternate
    /// between looks that both exist in quantity. The library was 86% matted
    /// before Task 13; this pins the floor so a later edit cannot quietly
    /// starve `pace` of bleed pages again.
    #[test]
    fn templates_real_library_offers_both_edge_treatments() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = Library::load(&dir).expect("the real library must decompose");

        let halves = lib.page_half_pool();
        let bleed =
            halves.iter().filter(|p| p.edge_treatment == EdgeTreatment::Bleed).count();
        let margin = halves.len() - bleed;

        assert!(
            bleed * 4 >= halves.len(),
            "only {bleed} of {} page halves bleed; at least a quarter must",
            halves.len()
        );
        assert!(margin > 0, "the library must still offer matted pages");
    }

    /// A spread template with all of its slots on one page half prints the
    /// other half white, every single time it is chosen. For the eight
    /// 1-photo templates that is structural -- one photo cannot fill two
    /// halves -- and `pack` handles them by never cutting a 1-photo group for
    /// a spread slot. For every LARGER template it is an authoring mistake,
    /// and there is nothing downstream that can rescue it.
    ///
    /// This is a structural, whole-library check rather than an assembly-level
    /// one on purpose. `pace_no_printed_page_names_a_real_template_and_holds_nothing_in_the_real_library`
    /// only sees templates the packer actually picks, and it picks 17 of 36:
    /// reverting `20-four-up-windowpane` to its all-on-the-left form leaves
    /// that test green, because `48-four-up-hero-plus-three-margin` outscores
    /// it at every size-4 group in the fixture. A guard that depends on a
    /// scorer's preferences is not a guard on the library.
    #[test]
    fn templates_real_library_multi_photo_spreads_use_both_page_halves() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = Library::load(&dir).expect("the real library must decompose");

        for t in &lib.spreads {
            if t.photo_count() <= 1 {
                continue;
            }
            assert!(
                !t.left.slots.is_empty() && !t.right.slots.is_empty(),
                "`{}` holds {} photos but puts {} slot(s) on the left half and {} on the \
                 right: the empty half prints white on every book that chooses it, and \
                 unlike a 1-photo template it has enough photos not to have to",
                t.id,
                t.photo_count(),
                t.left.slots.len(),
                t.right.slots.len()
            );
        }
    }
}
