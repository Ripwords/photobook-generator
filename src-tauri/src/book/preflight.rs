//! Pre-flight: the last gate before a single output file is written.
//!
//! Runs after the whole `Book` is assembled but before the renderer touches
//! disk, so a bad placement is reported once, up front, rather than
//! discovered after nineteen pages have already been written (spec 5.6).
//!
//! `preflight` is the thin real-world wrapper: it checks the filesystem for
//! moved/deleted source files and reads free disk space on `output_dir`'s
//! volume. `preflight_with_bleed` and `preflight_with_space` are the
//! testable seams -- each takes one piece of "the real world" as a parameter
//! instead of reading it, so tests can pin exact inputs without an actual
//! disk quota or a real bleed-declaring template at hand. This is the same
//! "extract the pure core" pattern the codebase already uses for
//! `finalize_photos`, `analyze_batches`, `lookup_cache`, `percentiles` and
//! `imageNormalizedTopLeft`.
//!
//! Every check here is identical regardless of the delivery mechanic
//! (Pixajoy upload vs. any other export path). The one thing THAT would
//! change is `EST_BYTES_PER_PLACEMENT`, kept as a single named constant for
//! exactly that reason.

use crate::book::cull::Photo;
use crate::book::pace::Book;
use crate::book::score::effective_dpi;
use crate::geometry::{bleeds_correctly, clear_of_gutter, in_safe_margin, in_trim, BleedEdge, Rect};
use crate::templates::{Role, Slot};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Estimated bytes written to disk per placement, used to gate export
/// against free disk space before any file is written.
///
/// One output file per placement: a CROP OF THE SOURCE PHOTOGRAPH at the
/// source's own resolution, encoded JPEG for a lossy source and PNG for a
/// lossless one (`Exporter.outputFormat`). Nothing is composited and nothing
/// is scaled to a page canvas, so this figure tracks what cameras produce and
/// how much of the frame the crop keeps -- not a full page canvas in pixels,
/// which an earlier transparent-PNG export mechanic would have written and
/// which this comment used to claim.
///
/// 3 MB is a placeholder chosen to be conservative for a ~12 MP JPEG; it has
/// never been checked against a real export run, and a folder of lossless
/// sources would blow through it (see PROJECT-STATUS's unmeasured-constants
/// item). Retune it against measurements, not against intuition. Every OTHER
/// check in this module holds regardless of delivery mechanic.
pub const EST_BYTES_PER_PLACEMENT: u64 = 3_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Block,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    pub page: u32,
    pub photo_path: String,
    pub message: String,
}

/// Reads the real world: source files on disk (via `missing_sources`, the
/// one filesystem read pre-flight's shell performs) and free space on
/// `output_dir`'s volume. Placements are treated as declaring no bleed,
/// since a `Placement` does not itself carry the slot's bleed array -- only
/// the originating template does, and this wrapper has no template to
/// consult. A caller that has the template (and therefore the real bleed
/// array) should call `preflight_with_bleed` directly.
pub fn preflight(book: &Book, photos: &[Photo], output_dir: &Path) -> Vec<Finding> {
    let bleed = vec![Vec::new(); placement_count(book)];
    preflight_with_bleed(book, photos, output_dir, &bleed)
}

/// As `preflight`, but with the bleed edges supplied explicitly rather than
/// assumed empty.
///
/// `bleed` carries ONE entry per placement, flattened across pages in page
/// order and then placement order within each page -- the same order the
/// renderer walks the book. This ordering is an implicit contract: get it
/// wrong and a bleed finding silently attributes itself to the wrong photo.
/// Any caller building this vector must walk `book.pages` and then
/// `page.placements` in exactly that nesting, with no reordering in between.
pub fn preflight_with_bleed(
    book: &Book,
    photos: &[Photo],
    output_dir: &Path,
    bleed: &[Vec<BleedEdge>],
) -> Vec<Finding> {
    let available = available_bytes(output_dir);
    let missing = missing_sources(book, photos);
    preflight_core(book, photos, bleed, available, &missing)
}

/// As `preflight`, but with the available disk space supplied explicitly
/// rather than read from `output_dir`'s volume -- so the disk-space check
/// does not depend on the free space of the machine running the test.
pub fn preflight_with_space(
    book: &Book,
    photos: &[Photo],
    _output_dir: &Path,
    available_bytes: u64,
) -> Vec<Finding> {
    let bleed = vec![Vec::new(); placement_count(book)];
    let missing = missing_sources(book, photos);
    preflight_core(book, photos, &bleed, available_bytes, &missing)
}

fn placement_count(book: &Book) -> usize {
    book.pages.iter().map(|p| p.placements.len()).sum()
}

/// The one filesystem read pre-flight's shell performs on behalf of the
/// core: which placed photos are no longer at their analysed path.
///
/// Built once over the distinct paths in the book rather than once per
/// placement, so a photo used twice is statted once.
fn missing_sources(book: &Book, photos: &[Photo]) -> std::collections::BTreeSet<String> {
    book.pages
        .iter()
        .flat_map(|p| p.placements.iter())
        .filter_map(|pl| photos.get(pl.photo_index))
        .map(|photo| photo.path.clone())
        .collect::<std::collections::BTreeSet<String>>()
        .into_iter()
        .filter(|path| !Path::new(path).exists())
        .collect()
}

/// True when `inner` (in the photo's own normalised coordinates) is fully
/// inside the crop window -- the same containment test `score::rejects` uses
/// to decide whether a face is even IN the final picture. A face wholly
/// outside the crop is simply not in the photo at all, so mapping it into
/// page space would produce meaningless coordinates; only a face the scorer
/// would have considered "in frame" gets checked against trim/gutter here.
fn contained_in(inner: &Rect, crop: &Rect) -> bool {
    const EPS: f64 = 1e-9;
    inner.x >= crop.x - EPS
        && inner.y >= crop.y - EPS
        && inner.right() <= crop.right() + EPS
        && inner.bottom() <= crop.bottom() + EPS
}

/// Maps a rect from a photo's own normalised coordinates into the slot's
/// page-normalised coordinates, given the crop window -- the same transform
/// `score::face_in_page` performs, reimplemented here because that helper is
/// private and takes a `Slot` where pre-flight only has a `Rect`. Returns
/// `None` for a degenerate crop window.
fn map_into_page(inner: &Rect, crop: &Rect, slot_rect: &Rect) -> Option<Rect> {
    if crop.w <= 0.0 || crop.h <= 0.0 {
        return None;
    }
    let u = (inner.x - crop.x) / crop.w;
    let v = (inner.y - crop.y) / crop.h;
    let uw = inner.w / crop.w;
    let vh = inner.h / crop.h;
    Some(Rect::new(
        slot_rect.x + u * slot_rect.w,
        slot_rect.y + v * slot_rect.h,
        uw * slot_rect.w,
        vh * slot_rect.h,
    ))
}

/// The pure core: every check in spec 5.6 over already-known data.
///
/// Genuinely no I/O. Both facts it cannot compute -- free disk space and
/// which source files have moved -- are ARGUMENTS, so Phase 3 can re-run
/// this on an edited book in a loop without touching the filesystem. That
/// is the whole reason the core/shell split exists; it previously did one
/// `Path::exists()` per placement and the doc comment claiming otherwise
/// was wrong.
pub(crate) fn preflight_core(
    book: &Book,
    photos: &[Photo],
    bleed: &[Vec<BleedEdge>],
    available_bytes: u64,
    missing: &std::collections::BTreeSet<String>,
) -> Vec<Finding> {
    // The book's own geometry. Pre-flight exists to answer "will this book
    // print", and the answer depends on the size it is being printed at.
    let spec = &book.spec;
    let mut findings = Vec::new();
    let mut placement_idx = 0usize;

    for page in &book.pages {
        for pl in &page.placements {
            let edges = bleed.get(placement_idx).cloned().unwrap_or_default();
            placement_idx += 1;

            let Some(photo) = photos.get(pl.photo_index) else { continue };

            // Source file moved or deleted since analysis -- BLOCK. Decided
            // by the caller, which is the only part of pre-flight that may
            // read a disk.
            if missing.contains(&photo.path) {
                findings.push(Finding {
                    severity: Severity::Block,
                    page: page.number,
                    photo_path: photo.path.clone(),
                    message: format!(
                        "Source file {} no longer exists at its analysed path",
                        photo.path
                    ),
                });
            }

            // DPI floor (BLOCK) and warn band (WARN): `[min_dpi, warn_dpi)`
            // warns and reports the effective value, below `min_dpi` blocks,
            // at or above `warn_dpi` is silent.
            //
            // Both ends come from the book's own spec, which is what makes
            // pre-flight agree with the scorer about what "good enough"
            // means. There used to be three copies of these two numbers --
            // `score::MIN_DPI`, a bare `300.0` literal in
            // `resolution_headroom`, and a `WARN_DPI_CEILING` here, under a
            // doc comment asserting an agreement that nothing enforced.
            // The slot's role/bleed/aspect_pref do not affect the DPI
            // number, so a throwaway `Slot` carrying only the real
            // `slot_rect` is exact, not an approximation.
            let synthetic_slot =
                Slot { rect: pl.slot_rect, role: Role::Support, bleed: Vec::new(), aspect_pref: (1.0, 1.0) };
            let dpi = effective_dpi(spec, photo, &pl.crop, &synthetic_slot);
            if dpi < spec.min_dpi() {
                findings.push(Finding {
                    severity: Severity::Block,
                    page: page.number,
                    photo_path: photo.path.clone(),
                    message: format!(
                        "Photo resolves at {dpi:.0} DPI in this slot, below the {:.0} DPI floor",
                        spec.min_dpi()
                    ),
                });
            } else if dpi < spec.warn_dpi() {
                findings.push(Finding {
                    severity: Severity::Warn,
                    page: page.number,
                    photo_path: photo.path.clone(),
                    message: format!(
                        "Photo resolves at {dpi:.0} DPI in this slot, below the {:.0} DPI target",
                        spec.warn_dpi()
                    ),
                });
            }

            // Bleed slot must actually reach past the canvas edge it
            // declares -- BLOCK. Trivially satisfied when no edges are
            // declared for this placement.
            if !bleeds_correctly(&pl.slot_rect, &edges, page.side) {
                findings.push(Finding {
                    severity: Severity::Block,
                    page: page.number,
                    photo_path: photo.path.clone(),
                    message: "Slot declares bleed but stops short of the canvas edge".into(),
                });
            }

            // Faces: outside the trim rectangle, inside the safe margin
            // beyond it, or inside the gutter dead strip -- all
            // three BLOCK. Trim and safe-margin are checked as an
            // else-if, NOT two independent `if`s: `in_safe_margin` is by
            // construction a strict subset of `in_trim` (geometry.rs), so a
            // face outside trim entirely also always fails the safe-margin
            // check, and reporting both would raise two Block findings for
            // one root cause. The more specific trim diagnosis wins; a face
            // that clears trim but not the extra 1/8" gets exactly the
            // safe-margin finding. Gutter stays an independent check exactly
            // as before: the fold edge has no trim inset but does have a
            // gutter strip, so a face flush to the fold can be in-trim (or
            // in-safe-margin) and in-gutter at once, and both are real,
            // distinct problems worth reporting.
            for face in &photo.faces {
                // A face wholly outside the crop is not in the final
                // picture at all -- matching `score::rejects`, which only
                // checks a face's page position once it has established the
                // face is fully contained in the crop. (A partially clipped
                // face is a separate, scorer-level rejection that should
                // never reach an assembled book in the first place.)
                if !contained_in(&face.box_, &pl.crop) {
                    continue;
                }
                let Some(mapped) = map_into_page(&face.box_, &pl.crop, &pl.slot_rect) else {
                    continue;
                };
                if !in_trim(spec, &mapped, page.side) {
                    findings.push(Finding {
                        severity: Severity::Block,
                        page: page.number,
                        photo_path: photo.path.clone(),
                        message: "Face falls outside the trim rectangle".into(),
                    });
                } else if !in_safe_margin(spec, &mapped, page.side) {
                    findings.push(Finding {
                        severity: Severity::Block,
                        page: page.number,
                        photo_path: photo.path.clone(),
                        message: format!(
                            "Face falls inside the {:.3}\" safe margin",
                            spec.safe_margin_in()
                        ),
                    });
                }
                if !clear_of_gutter(spec, &mapped, page.side) {
                    findings.push(Finding {
                        severity: Severity::Block,
                        page: page.number,
                        photo_path: photo.path.clone(),
                        message: "Face falls inside the gutter dead strip".into(),
                    });
                }
            }

            // Generic (non-face) salient content in the gutter dead strip --
            // WARN only. A face there blocks; anything else merely risks
            // looking awkward once bound, which is a judgement call for the
            // user, not a hard rejection -- otherwise no slot could ever run
            // flush to the fold.
            // Unlike a face, a saliency box is never a hard constraint, so it
            // can be legitimately partial -- only the portion that SURVIVES
            // the crop is checked, via `intersect`, rather than assuming the
            // whole declared box is visible.
            if let Some(saliency) = photo.saliency_box.and_then(|s| s.intersect(&pl.crop)) {
                if let Some(mapped) = map_into_page(&saliency, &pl.crop, &pl.slot_rect) {
                    if !clear_of_gutter(spec, &mapped, page.side) {
                        findings.push(Finding {
                            severity: Severity::Warn,
                            page: page.number,
                            photo_path: photo.path.clone(),
                            message: "Salient content falls inside the gutter dead strip".into(),
                        });
                    }
                }
            }
        }
    }

    // Estimated output size vs. free disk space -- BLOCK. A whole-book
    // concern rather than one photo's, so it is not attributed to a page or
    // photo.
    let estimate = placement_idx as u64 * EST_BYTES_PER_PLACEMENT;
    if estimate > available_bytes {
        findings.push(Finding {
            severity: Severity::Block,
            page: 0,
            photo_path: String::new(),
            message: format!(
                "Estimated output size ({estimate} bytes across {placement_idx} placements) exceeds available disk space ({available_bytes} bytes)"
            ),
        });
    }

    findings
}

/// Free bytes on the volume containing `output_dir`, via `statfs`. macOS
/// only, per this project's arm64/macOS-15+ constraint -- there is no
/// cross-platform fallback to maintain. Returns 0 (which always blocks) on
/// any failure: a directory that cannot be statted is not a safe place to
/// write nineteen pages of PNGs, so failing closed is the correct default.
fn available_bytes(output_dir: &Path) -> u64 {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(output_dir.as_os_str().as_bytes()) else {
        return 0;
    };
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c_path` is a valid NUL-terminated C string for the lifetime of
    // this call, and `stat` points at a `libc::statfs`-sized allocation for
    // `statfs` to fill in. `statfs` only reads through `c_path` and writes
    // through `stat`.
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return 0;
    }
    // SAFETY: `statfs` returned 0 (success), so `stat` was fully
    // initialised by the call above.
    let stat = unsafe { stat.assume_init() };
    (stat.f_bavail as u64).saturating_mul(stat.f_bsize as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::{odd_spec, pixajoy_spec};
    use crate::book::cull::{Face, PaletteColor};
    use crate::book::pace::{Book, Page, Placement};
    use crate::geometry::Side;

    /// 3:2, never square -- a square hides every aspect-dependent bug.
    fn photo(w: u32, h: u32) -> Photo {
        Photo {
            path: "/p/a.jpg".into(), hash: "h".into(), width: w, height: h,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: 0,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
            scene_tags: Vec::new(), captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None,
        }
    }

    /// A slot comfortably inside the trim on a LEFT page, with a crop that
    /// keeps the whole frame. Callers mutate one thing at a time.
    fn book_with(slot: Rect, crop: Rect, side: Side) -> Book {
        Book {
            spec: pixajoy_spec(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![Page {
                number: 4,
                side,
                template_id: "fx".into(),
                placements: vec![Placement { photo_index: 0, slot_rect: slot, crop, z: 1 }],
            }],
            options: Default::default(),
        }
    }

    fn clean_slot() -> Rect {
        Rect::new(0.10, 0.10, 0.50, 0.40)
    }

    fn full_crop() -> Rect {
        Rect::new(0.0, 0.0, 1.0, 1.0)
    }

    /// Pixels needed for a given DPI in `clean_slot`, so DPI tests hit the
    /// boundary exactly rather than somewhere near it.
    fn px_for_dpi(dpi: f64) -> u32 {
        (dpi * clean_slot().w * pixajoy_spec().page_w_in()).round() as u32
    }

    /// A non-square photo at `path`, sized well clear of the spec's DPI
    /// floor and ceiling in `clean_slot` (400 DPI, same margin the other
    /// fixtures in this module use), so a test using it exercises only
    /// whatever it's actually testing rather than tripping the DPI checks.
    fn preflight_photo(path: &str) -> Photo {
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = path.into();
        p
    }

    /// A single-placement book in `clean_slot`, on a LEFT page, with a crop
    /// that keeps the whole frame -- the same fixture `book_with` builds,
    /// named for callers that don't need to vary the slot/crop/side.
    fn one_placement_book() -> Book {
        book_with(clean_slot(), full_crop(), Side::Left)
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn preflight_returns_no_findings_for_a_clean_book() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into(); // exists, so the missing-source check passes
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        assert_eq!(preflight(&book, &[p], dir.path()), Vec::new());
    }

    #[test]
    fn preflight_blocks_a_face_outside_the_trim_rectangle() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        // Face at the very top of the frame, in a slot flush to the top
        // bleed -- so it lands above the trim line.
        p.faces = vec![Face { box_: Rect::new(0.4, 0.0, 0.1, 0.02), capture_quality: Some(0.8) }];
        let book = book_with(Rect::new(0.10, 0.0, 0.50, 0.40), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block
                && f.message.contains("trim")),
            "got {findings:?}"
        );
        // `in_safe_margin` is a strict subset of `in_trim`, so a face outside
        // trim entirely ALSO fails the safe-margin check -- but it must
        // report only the more specific trim diagnosis, not a second,
        // redundant safe-margin finding for the same root cause.
        assert!(
            findings.iter().all(|f| !f.message.contains("safe margin")),
            "a face outside trim entirely must report the trim violation, \
             not the safe-margin one: {findings:?}"
        );
    }

    /// The new hard constraint: a face INSIDE the trim rectangle but inside
    /// the additional 1/8" Pixajoy buffer beyond it. `in_trim` on the mapped
    /// rect is comfortably satisfied here -- only `in_safe_margin` catches
    /// it, which is what distinguishes this from the trim test above.
    #[test]
    fn preflight_blocks_a_face_inside_trim_but_inside_the_safe_margin_band() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        p.faces = vec![Face { box_: Rect::new(0.044, 0.4, 0.02, 0.02), capture_quality: Some(0.8) }];
        let slot = Rect::new(0.0, 0.1, 0.5, 0.4);
        let book = book_with(slot, full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block
                && f.message.contains("safe margin")),
            "got {findings:?}"
        );
        assert!(
            findings.iter().all(|f| !f.message.contains("trim rectangle")),
            "a face inside trim must not also report a trim-rectangle finding: {findings:?}"
        );
    }

    #[test]
    fn preflight_blocks_a_face_inside_the_gutter_dead_strip() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        p.faces = vec![Face { box_: Rect::new(0.95, 0.4, 0.05, 0.1), capture_quality: Some(0.8) }];
        // Slot running flush to the fold on a LEFT page.
        let book = book_with(Rect::new(0.5, 0.2, 0.5, 0.4), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block
                && f.message.contains("gutter")),
            "got {findings:?}"
        );
    }

    /// The counterpart to the scorer's rule: generic saliency in the dead
    /// strip WARNS, a face BLOCKS. Conflating them makes fold-flush layouts
    /// impossible, which is the look the user explicitly wants available.
    #[test]
    fn preflight_warns_on_salient_content_in_the_dead_strip_without_blocking() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        p.saliency_box = Some(Rect::new(0.95, 0.4, 0.05, 0.1));
        let book = book_with(Rect::new(0.5, 0.2, 0.5, 0.4), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(findings.iter().all(|f| f.severity != Severity::Block), "got {findings:?}");
        assert!(findings.iter().any(|f| f.severity == Severity::Warn));
    }

    /// A face wholly outside the crop is not in the final picture at all --
    /// mirroring `score::rejects`'s own rule (a face the crop misses entirely
    /// is fine, only a PARTIAL overlap is a rejectable half-face). Mapping
    /// such a face into page space anyway produces garbage coordinates: a
    /// narrow crop turns a small offset into a huge one once divided by
    /// `crop.w`, which can spill the "mapped" rect far past the page edge
    /// and trip `in_trim` on a photo that never shows the face at all.
    ///
    /// The same fixture carries a SALIENCY box outside the crop, because the
    /// saliency arm has its own containment rule (`intersect`, not
    /// `contained_in`) and every other saliency test here uses `full_crop()`,
    /// under which `intersect` is the identity and therefore untested.
    /// Dropping the `intersect` call leaves this box mapping to x >= 2.6 --
    /// far past the fold -- and a spurious gutter warning appears.
    #[test]
    fn preflight_ignores_a_face_or_saliency_box_wholly_outside_the_crop() {
        let dir = tempdir();
        // Cropping to 10% of the frame width divides effective DPI by 10, so
        // the source needs 10x the pixels `px_for_dpi(400.0)` would give a
        // full-frame crop, to stay clear of the (unrelated) DPI checks here.
        let mut p = photo(px_for_dpi(4000.0), px_for_dpi(4000.0) * 2 / 3);
        p.path = "/dev/null".into();
        // The crop keeps only the left 10% of the frame; the face sits at
        // x=0.5, entirely past the crop's right edge.
        p.faces = vec![Face { box_: Rect::new(0.5, 0.4, 0.05, 0.1), capture_quality: Some(0.8) }];
        p.saliency_box = Some(Rect::new(0.5, 0.4, 0.05, 0.1));
        let crop = Rect::new(0.0, 0.0, 0.10, 1.0);
        let book = book_with(clean_slot(), crop, Side::Left);
        assert_eq!(
            preflight(&book, &[p], dir.path()),
            Vec::new(),
            "content outside the crop must not generate a spurious trim/gutter finding"
        );
    }

    /// The clipping half of the same rule, which the wholly-outside case
    /// above cannot reach: a saliency box that OVERLAPS the crop must be
    /// judged on the surviving sliver only. Here the box runs from x=0.05 to
    /// x=0.55 while the crop keeps 0..0.10, so the visible part maps to the
    /// left 60% of a slot that ends well clear of the fold -- no warning.
    /// Map the whole declared box instead and it reaches x=2.85, deep past
    /// the fold, and warns about content the reader never sees.
    #[test]
    fn preflight_judges_a_saliency_box_on_the_part_the_crop_keeps() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(4000.0), px_for_dpi(4000.0) * 2 / 3);
        p.path = "/dev/null".into();
        p.saliency_box = Some(Rect::new(0.05, 0.4, 0.5, 0.1));
        let crop = Rect::new(0.0, 0.0, 0.10, 1.0);
        let book = book_with(clean_slot(), crop, Side::Left);
        assert_eq!(
            preflight(&book, &[p], dir.path()),
            Vec::new(),
            "only the cropped-in part of the saliency box may be checked"
        );
    }

    #[test]
    fn preflight_blocks_a_photo_below_the_dpi_floor() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(100.0), px_for_dpi(100.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("200")), "got {findings:?}");
    }

    /// Pixajoy's floor moved from 150 to 200 DPI: a photo that used to sit
    /// safely in the warn band now falls below the hard floor and must
    /// BLOCK, not warn. Pins that the warn band's LOWER edge moved with
    /// the spec's floor, not just the block/no-block threshold in isolation.
    #[test]
    fn preflight_blocks_at_one_hundred_eighty_dpi_now_that_the_floor_moved_to_two_hundred() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(180.0), px_for_dpi(180.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block && f.message.contains("200")),
            "180 DPI must block under the 200 DPI floor: {findings:?}"
        );
        assert!(
            findings.iter().all(|f| f.severity != Severity::Warn),
            "must not ALSO warn for the same photo: {findings:?}"
        );
    }

    #[test]
    fn preflight_warns_between_200_and_300_dpi_and_reports_the_effective_value() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(250.0), px_for_dpi(250.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        let warn = findings.iter().find(|f| f.severity == Severity::Warn)
            .expect("expected a DPI warning");
        // A warning that does not say HOW soft the photo is cannot be acted on.
        assert!(warn.message.contains("250"), "message was {:?}", warn.message);
    }

    /// R10. Pre-flight reads the BOOK's floor and ceiling, never a copy of
    /// Pixajoy's.
    ///
    /// This supersedes an earlier test that pinned pre-flight's floor to
    /// `score::MIN_DPI` via its FULLY QUALIFIED path. That test existed
    /// because a version written against the bare, in-scope `MIN_DPI` name
    /// was tried first and did NOT catch the mutation -- the name resolved
    /// to whichever constant was locally in scope and passed either way.
    /// The constants are gone now and there is exactly one authority, the
    /// book's own spec, so the drift it guarded is unrepresentable. The
    /// near-miss is recorded here because the replacement has to be at
    /// least as strong, and it is: running under `odd_spec` kills a
    /// hardcoded 200.0 or 300.0 ANYWHERE in this module, not merely a
    /// shadowed name.
    ///
    /// Two-sided under `odd_spec` (floor 150, ceiling 220), which shares no
    /// number with Pixajoy's 200/300:
    ///   - 250 DPI is above 220 and must be SILENT. Under a reintroduced
    ///     `WARN_DPI_CEILING = 300.0` it would warn.
    ///   - 140 DPI is below 150 and must BLOCK. Under a hardcoded 200.0
    ///     floor it would also block, which is why the silent half above is
    ///     the half that actually kills the mutation.
    ///   - 180 DPI sits between them and must WARN, pinning that the band
    ///     is the spec's band and not an empty one.
    #[test]
    fn preflight_warns_below_the_specs_warn_dpi_and_blocks_below_its_floor() {
        let dir = tempdir();
        let spec = odd_spec();
        // `odd_spec` is 8.0" wide, so pixels for a DPI must be computed from
        // it -- `px_for_dpi` is Pixajoy's page and would silently land these
        // fixtures at 11.197/8.0 = 1.4x the DPI they are named for.
        let px_for = |dpi: f64| (dpi * clean_slot().w * spec.page_w_in()).round() as u32;
        let book = Book { spec, ..book_with(clean_slot(), full_crop(), Side::Left) };

        let mut above = photo(px_for(250.0), px_for(250.0) * 2 / 3);
        above.path = "/dev/null".into();
        assert_eq!(
            preflight(&book, &[above], dir.path()),
            Vec::new(),
            "250 DPI is above this book's 220 DPI ceiling and must be silent"
        );

        let mut middle = photo(px_for(180.0), px_for(180.0) * 2 / 3);
        middle.path = "/dev/null".into();
        let findings = preflight(&book, &[middle], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Warn),
            "180 DPI is inside this book's 150-220 band and must warn: {findings:?}"
        );
        assert!(
            findings.iter().all(|f| f.severity != Severity::Block),
            "180 DPI is above this book's floor and must not block: {findings:?}"
        );

        let mut below = photo(px_for(140.0), px_for(140.0) * 2 / 3);
        below.path = "/dev/null".into();
        let findings = preflight(&book, &[below], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block && f.message.contains("150")),
            "140 DPI is below this book's 150 DPI floor and must block, \
             naming the book's own floor: {findings:?}"
        );
    }

    /// Boundary AT the boundary, pinned two-sided: exactly 300 DPI is
    /// silent, meaningfully below it warns.
    ///
    /// `clean_slot` (0.50 normalised) CANNOT construct exactly 300 DPI:
    /// `px_for_dpi(300.0)` rounds `300 * 0.50 * page_w_in()` = 1679.55 up to
    /// 1680px, which resolves at ~300.08 DPI -- silent under EITHER `<` or
    /// `<=` against the ceiling, so a fixture built on it cannot tell the two
    /// comparisons apart and the boundary could be flipped with the suite
    /// still green. This uses a slot exactly 6.0" wide instead (the same
    /// trick `score.rs`'s `six_inch_slot` uses for the 150 DPI floor):
    /// `6.0 / page_w_in()` round-trips back through `* page_w_in()` to exactly
    /// 6.0 in IEEE doubles, so 1800px / 6.0" is bit-exact 300.0 DPI --
    /// verified below via `effective_dpi` directly, not merely assumed.
    #[test]
    fn preflight_is_silent_at_exactly_three_hundred_dpi_and_warns_meaningfully_below() {
        let dir = tempdir();
        let slot = Rect::new(0.10, 0.10, 6.0 / pixajoy_spec().page_w_in(), 0.40);

        let mut at = photo(1800, 1200);
        at.path = "/dev/null".into();
        let dpi = effective_dpi(&pixajoy_spec(),
            &at,
            &full_crop(),
            &Slot { rect: slot, role: Role::Support, bleed: Vec::new(), aspect_pref: (1.0, 1.0) },
        );
        assert_eq!(dpi, 300.0, "the fixture must land ON the boundary, not merely near it");

        let book_at = book_with(slot, full_crop(), Side::Left);
        assert_eq!(
            preflight(&book_at, &[at], dir.path()),
            Vec::new(),
            "300 DPI exactly must be silent"
        );

        // 10px below 1800 is a clearly distinguishable ~298.3 DPI -- far
        // enough from the boundary that rounding in the message text cannot
        // accidentally still read "300".
        let mut below = photo(1790, 1193);
        below.path = "/dev/null".into();
        let book_below = book_with(slot, full_crop(), Side::Left);
        let findings = preflight(&book_below, &[below], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Warn && f.message.contains("298")),
            "just below the 300 DPI ceiling must warn with the effective value: {findings:?}"
        );
    }

    #[test]
    fn preflight_blocks_a_source_file_that_no_longer_exists() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/definitely/not/here.jpg".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("no longer exists")), "got {findings:?}");
    }

    /// The pure core must decide from its arguments alone. Handed a photo whose
    /// path does not exist on disk but which is NOT in the missing set, it must
    /// raise no moved-source finding -- proving it consulted the argument
    /// rather than the filesystem.
    #[test]
    fn preflight_core_reads_the_missing_set_not_the_filesystem() {
        let book = one_placement_book();
        let photos = vec![preflight_photo("/definitely/not/on/disk.jpg")];
        let bleed = vec![Vec::new(); 1];

        let findings =
            preflight_core(&book, &photos, &bleed, u64::MAX, &std::collections::BTreeSet::new());

        assert!(
            !findings.iter().any(|f| f.message.contains("no longer exists")),
            "the core touched the filesystem instead of reading the missing set: {findings:?}"
        );

        let missing: std::collections::BTreeSet<String> =
            ["/definitely/not/on/disk.jpg".to_string()].into_iter().collect();
        let findings = preflight_core(&book, &photos, &bleed, u64::MAX, &missing);
        assert!(
            findings.iter().any(|f| {
                f.severity == Severity::Block && f.message.contains("no longer exists")
            }),
            "a path in the missing set must still Block: {findings:?}"
        );
    }

    #[test]
    fn preflight_blocks_a_bleed_slot_that_stops_short_of_the_canvas_edge() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        // Declared as reaching the left edge but starting at 0.01.
        let mut book = book_with(Rect::new(0.01, 0.0, 0.5, 1.0), full_crop(), Side::Left);
        book.pages[0].placements[0].slot_rect = Rect::new(0.01, 0.0, 0.5, 1.0);
        let findings = preflight_with_bleed(
            &book, &[p], dir.path(), &[vec![crate::geometry::BleedEdge::Left]],
        );
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("bleed")), "got {findings:?}");
    }

    /// A slot that DOES declare bleed and DOES reach the edge must not be
    /// blocked -- otherwise every bleed-correct spread would fail
    /// pre-flight, which would be caught immediately in manual QA but is
    /// exactly the kind of false positive a test suite should catch first.
    #[test]
    fn preflight_does_not_block_a_bleed_slot_that_reaches_the_edge() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(Rect::new(0.0, 0.0, 0.5, 1.0), full_crop(), Side::Left);
        let findings = preflight_with_bleed(
            &book, &[p], dir.path(), &[vec![crate::geometry::BleedEdge::Left]],
        );
        assert!(
            findings.iter().all(|f| !f.message.contains("bleed")),
            "a correct bleed must not be flagged: {findings:?}"
        );
    }

    /// This is why export fails on page one rather than after nineteen.
    #[test]
    fn preflight_blocks_when_the_estimate_exceeds_free_disk_space() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        // One placement against zero available bytes.
        let findings = preflight_with_space(&book, &[p], dir.path(), 0);
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("disk")), "got {findings:?}");
    }

    /// The counterpart: plenty of space must not block.
    #[test]
    fn preflight_does_not_block_when_space_comfortably_exceeds_the_estimate() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight_with_space(&book, &[p], dir.path(), EST_BYTES_PER_PLACEMENT * 10);
        assert!(
            findings.iter().all(|f| !f.message.contains("disk")),
            "got {findings:?}"
        );
    }

    /// `preflight_with_bleed`'s ordering contract: bleed entries are indexed
    /// by FLATTENED placement position (page order, then placement order),
    /// not by anything else. Two pages, one placement each, with the bleed
    /// vector targeting only the SECOND page's placement -- if the ordering
    /// were reversed, or keyed some other way, this finding would attach to
    /// the wrong page.
    #[test]
    fn preflight_with_bleed_indexes_by_flattened_page_then_placement_order() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        let good_slot = Rect::new(0.10, 0.10, 0.50, 0.40); // no declared bleed needed
        let short_slot = Rect::new(0.01, 0.0, 0.5, 1.0); // declares left bleed but stops short
        let book = Book {
            spec: pixajoy_spec(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![
                Page {
                    number: 1,
                    side: Side::Left,
                    template_id: "fx".into(),
                    placements: vec![Placement {
                        photo_index: 0,
                        slot_rect: good_slot,
                        crop: full_crop(),
                        z: 1,
                    }],
                },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "fx".into(),
                    placements: vec![Placement {
                        photo_index: 0,
                        slot_rect: short_slot,
                        crop: full_crop(),
                        z: 1,
                    }],
                },
            ],
            options: Default::default(),
        };
        let bleed = vec![Vec::new(), vec![crate::geometry::BleedEdge::Left]];
        let findings = preflight_with_bleed(&book, &[p], dir.path(), &bleed);
        let bleed_finding = findings
            .iter()
            .find(|f| f.message.contains("bleed"))
            .expect("expected a bleed finding");
        assert_eq!(bleed_finding.page, 2, "the bleed finding must attach to page 2, not page 1");
    }
}
