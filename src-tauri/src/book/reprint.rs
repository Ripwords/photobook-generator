//! Moving an already-laid-out book to a different print geometry.
//!
//! One pure function, reached two ways: `BookEdit::SetPrintSpec` keeps the
//! book it returns, and `check_print_spec` shows the findings and throws the
//! book away. Neither touches a disk, a `Db` or an `AppHandle` -- the same
//! core/shell split as `preflight_core`, `finalize_photos` and `percentiles`.
//!
//! ## Why the book is kept rather than re-laid-out
//!
//! Slot rects are PAGE-normalised, so nothing about a size change invalidates
//! a template choice: `spread_to_page`'s fold sits at 0.5 whatever the page
//! measures, and `templates.test.ts` checks its safe-area and gutter rules
//! against text zones only, never slots. Every template choice, lock, swap,
//! replacement and hand-moved box therefore survives, and re-laying out would
//! destroy every edit the editor exists to allow -- in an app whose only undo
//! is the 30-day trash.
//!
//! Re-layout is still available, as a choice rather than a side effect of
//! typing in a margin field: `BookEdit::Shuffle` regenerates every unlocked
//! opening and `Regenerate { opening }` does one, both scoring against the
//! new spec.
//!
//! ## Why this never refuses
//!
//! A deliberate asymmetry with `edit.rs`, which refuses a single edit that
//! breaks a hard constraint. Read without this note it looks like an
//! inconsistency.
//!
//! Refusing a book-wide change because one photo of sixty fell to 195 DPI
//! means the user cannot have the bleed their printer requires. It is safe
//! because `export_book` blocks on pre-flight regardless, so nothing bad
//! ships -- and because the findings come back with the new book, the user is
//! told before they commit and again if they try to export.
//!
//! ## What actually changes
//!
//! Only `Placement::crop`, and only when it has to. A crop was cut to its
//! slot's REAL-WORLD aspect while the slot rect is page-normalised, so the
//! rect does not move but its printed shape does, and `cropStyle` would
//! visibly stretch the photo. That shape is `(rect.w * page_w_in) / (rect.h *
//! page_h_in)`, so it changes exactly when the page's own inch aspect does --
//! which is why a bleed, margin, fold or resolution edit leaves every crop
//! byte-identical.

use crate::book::crop::choose_crop;
use crate::book::cull::Photo;
use crate::book::pace::Book;
use crate::book::preflight::{preflight_core, Finding};
use crate::geometry::BleedEdge;
use crate::book::score::slot_aspect;
use crate::preview::{preview_geometry, PreviewGeometry};
use crate::print_spec::{PrintSpec, RawPrintSpec, SpecError};
use crate::templates::{Role, Slot};
use serde::Serialize;

/// A book moved to a new geometry, and what fails at that size.
///
/// `findings` is pre-flight's verdict on `book`, minus the two checks that
/// need a disk: free space and moved source files. Those are export-time
/// facts about the machine, not consequences of the page size.
#[derive(Debug, Clone, PartialEq)]
pub struct Reprint {
    pub book: Book,
    pub findings: Vec<Finding>,
}

/// Re-crops every placement for `to`'s slot aspects and re-runs the hard
/// constraints.
///
/// Template choices, slot rects, locks, rejections, the drop count and the
/// seed are untouched: changing the page size is not a reason to re-roll the
/// book.
pub fn reprint(book: &Book, photos: &[Photo], to: &PrintSpec) -> Reprint {
    let mut next = book.clone();
    next.spec = *to;

    if page_shape_changed(&book.spec, to) {
        for page in &mut next.pages {
            for pl in &mut page.placements {
                let Some(photo) = photos.get(pl.photo_index) else { continue };
                pl.crop = choose_crop(photo, slot_aspect(to, &slot_for(pl.slot_rect)));
            }
        }
    }

    // `u64::MAX` and an empty missing set: this is a question about geometry,
    // asked without a filesystem. A real export re-runs the full `preflight`,
    // which adds both.
    let bleed: Vec<Vec<BleedEdge>> = vec![Vec::new(); next.pages.iter().map(|p| p.placements.len()).sum()];
    let findings = preflight_core(&next, photos, &bleed, u64::MAX, &Default::default());

    Reprint { book: next, findings }
}

/// What the Print size panel learns about a spec the user has typed but not
/// applied.
///
/// A refusal travels as a VALUE rather than a command error. Tauri would
/// otherwise reject the argument before the command ran and hand the webview
/// `SpecError`'s `Display` text, in inches and with Rust's field names, to
/// someone typing millimetres. Structured, the panel renders it in the unit
/// on screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SpecCheck {
    Refused { error: SpecError },
    /// `geometry` is the guides the preview draws while the panel is open,
    /// so the page reshapes as the user types without TypeScript deriving a
    /// single rect. `recrops` says whether applying re-cuts every crop.
    Checked { geometry: PreviewGeometry, findings: Vec<Finding>, recrops: bool },
}

/// Validates `raw` through the one door, then -- when there is a laid-out
/// book to try it on -- runs exactly what `SetPrintSpec` would, keeping only
/// the findings. A preview that predicted something other than what Apply
/// then did would be worse than no preview.
///
/// `book` is `None` before a book exists (the draft screen), where the only
/// questions are whether the spec is buildable and what it looks like.
pub fn check(raw: RawPrintSpec, book: Option<(&Book, &[Photo])>) -> SpecCheck {
    let spec = match PrintSpec::try_from(raw) {
        Ok(spec) => spec,
        Err(error) => return SpecCheck::Refused { error },
    };
    let (findings, recrops) = match book {
        Some((book, photos)) => {
            (reprint(book, photos, &spec).findings, page_shape_changed(&book.spec, &spec))
        }
        None => (Vec::new(), false),
    };
    SpecCheck::Checked { geometry: preview_geometry(&spec), findings, recrops }
}

/// Whether a slot's printed shape moves between two specs.
///
/// Every slot's inch aspect is `(rect.w / rect.h) * (page_w_in / page_h_in)`,
/// so the rect cancels and one comparison answers for the whole book.
fn page_shape_changed(from: &PrintSpec, to: &PrintSpec) -> bool {
    from.page_w_in() / from.page_h_in() != to.page_w_in() / to.page_h_in()
}

/// A placement's rect as the scorer wants it. Role, bleed and `aspect_pref`
/// do not enter `slot_aspect`, so this is exact rather than an approximation
/// -- the same synthetic slot `preflight_core` builds for `effective_dpi`.
fn slot_for(rect: crate::geometry::Rect) -> Slot {
    Slot { rect, role: Role::Support, bleed: Vec::new(), aspect_pref: (1.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::PaletteColor;
    use crate::book::pace::{Page, Placement};
    use crate::geometry::{Rect, Side};
    use crate::print_spec::{odd_spec, pixajoy_spec, RawPrintSpec};
    use std::collections::BTreeMap;

    /// 3:2 and never square: a square photo has the same crop at every slot
    /// aspect, which would make "re-cropped" and "passed through" identical.
    fn photo(w: u32, h: u32) -> Photo {
        Photo {
            path: "/p/a.jpg".into(), hash: "h".into(), width: w, height: h,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: 0,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
            scene_tags: Vec::new(), captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
        }
    }

    fn photos() -> Vec<Photo> {
        vec![photo(4000, 2667), photo(3000, 4500)]
    }

    /// Two slots of DIFFERENT shapes, so one re-crop rule cannot happen to
    /// suit both. Crops are built the way `pace::assemble` builds them --
    /// `choose_crop` at the slot's own inch aspect -- rather than written as
    /// literals, so the fixture is a book the engine could really have made.
    fn book() -> Book {
        let slots = [Rect::new(0.08, 0.10, 0.50, 0.30), Rect::new(0.20, 0.55, 0.30, 0.35)];
        let ps = photos();
        Book {
            spec: pixajoy_spec(),
            controls: BTreeMap::new(),
            seed: 99,
            dropped: 3,
            pages: vec![Page {
                number: 4,
                side: Side::Left,
                template_id: "fx".into(),
                placements: slots
                    .iter()
                    .enumerate()
                    .map(|(i, rect)| Placement {
                        photo_index: i,
                        slot_rect: *rect,
                        crop: choose_crop(&ps[i], slot_aspect(&pixajoy_spec(), &slot_for(*rect))),
                        z: i as u32 + 1,
                    })
                    .collect(),
            }],
        }
    }

    /// The printed shape of a crop: normalised in the photo's own frame, so
    /// the pixel dimensions have to come back in to get a real-world ratio.
    fn printed_aspect(crop: &Rect, photo: &Photo) -> f64 {
        (crop.w * photo.width as f64) / (crop.h * photo.height as f64)
    }

    fn raw(spec: &PrintSpec) -> RawPrintSpec {
        RawPrintSpec {
            page_w_in: spec.page_w_in(),
            page_h_in: spec.page_h_in(),
            bleed_in: spec.bleed_in(),
            gutter_in: spec.gutter_in(),
            safe_margin_in: spec.safe_margin_in(),
            min_dpi: spec.min_dpi(),
            warn_dpi: spec.warn_dpi(),
        }
    }

    /// The refusal the panel shows is Rust's, carried as a value with the
    /// numbers intact, not a string the webview would have to parse.
    #[test]
    fn check_returns_the_validators_own_refusal_as_a_value() {
        let r = RawPrintSpec { bleed_in: 5.0, safe_margin_in: 3.0, gutter_in: 3.197, ..raw(&pixajoy_spec()) };
        assert_eq!(
            check(r, Some((&book(), &photos()))),
            SpecCheck::Refused {
                error: SpecError::NoSafeArea {
                    axis: crate::print_spec::Axis::Horizontal,
                    insets_in: 11.197,
                    page_in: 11.197,
                },
            }
        );
    }

    /// Before a book exists there is nothing to re-cut and nothing to fail;
    /// the guides still come back, from the spec that was asked about.
    #[test]
    fn check_without_a_book_returns_that_specs_guides_and_nothing_else() {
        assert_eq!(
            check(raw(&odd_spec()), None),
            SpecCheck::Checked { geometry: preview_geometry(&odd_spec()), findings: vec![], recrops: false }
        );
    }

    /// The dry run reports exactly what applying would, and says whether the
    /// crops move -- two-sided, so a hardwired `recrops` fails one half.
    #[test]
    fn check_reports_what_applying_would_and_whether_crops_move() {
        let (b, ps) = (book(), photos());
        let big = spec_with(30.0, 24.0);
        let SpecCheck::Checked { geometry, findings, recrops } = check(raw(&big), Some((&b, &ps))) else {
            panic!("a valid spec was refused");
        };
        assert_eq!(geometry, preview_geometry(&big));
        assert!(!findings.is_empty(), "a 30\" page must push these photos under the floor");
        assert_eq!(findings, reprint(&b, &ps, &big).findings);
        assert!(recrops);

        let margin_only = RawPrintSpec { safe_margin_in: 0.3, ..raw(&pixajoy_spec()) };
        let SpecCheck::Checked { recrops, .. } = check(margin_only, Some((&b, &ps))) else {
            panic!("a valid spec was refused");
        };
        assert!(!recrops);
    }

    /// Pins `SpecCheck` from both sides with `tests/printSpec.test.ts`. The
    /// spec is 16 x 8 so every guide is a dyadic fraction and the fixture
    /// compares as values, not text (see the fixture README).
    #[test]
    fn spec_check_serialises_the_shape_the_panel_reads() {
        let spec = PrintSpec::try_from(RawPrintSpec {
            page_w_in: 16.0,
            page_h_in: 8.0,
            bleed_in: 0.25,
            gutter_in: 0.5,
            safe_margin_in: 0.125,
            min_dpi: 150.0,
            warn_dpi: 250.0,
        })
        .unwrap();
        let cases = vec![
            SpecCheck::Checked {
                geometry: preview_geometry(&spec),
                findings: vec![Finding {
                    severity: crate::book::preflight::Severity::Block,
                    page: 7,
                    photo_path: "/p/a.jpg".into(),
                    message: "would print at 120 DPI, below the 150 DPI minimum".into(),
                }],
                recrops: true,
            },
            SpecCheck::Refused {
                error: SpecError::NoSafeArea {
                    axis: crate::print_spec::Axis::Vertical,
                    insets_in: 8.5,
                    page_in: 8.0,
                },
            },
            SpecCheck::Refused { error: SpecError::WarnBelowFloor { min_dpi: 300.0, warn_dpi: 200.0 } },
            SpecCheck::Refused {
                error: SpecError::Negative { field: crate::print_spec::SpecField::BleedIn, value: -0.5 },
            },
        ];
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/wire/spec-check.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&cases).unwrap(), fixture);
    }

    fn spec_with(page_w_in: f64, page_h_in: f64) -> PrintSpec {
        PrintSpec::try_from(RawPrintSpec {
            page_w_in,
            page_h_in,
            bleed_in: 0.197,
            gutter_in: 0.197,
            safe_margin_in: 0.125,
            min_dpi: 200.0,
            warn_dpi: 300.0,
        })
        .expect("fixture spec must be valid")
    }

    /// The fixture is only worth anything if its crops are not the full
    /// frame: "passed through unchanged" and "correctly recomputed" would be
    /// indistinguishable, which is exactly how eight Phase-1 tests passed
    /// under a broken implementation.
    #[test]
    fn the_fixture_starts_with_crops_that_are_not_the_full_frame() {
        for pl in &book().pages[0].placements {
            assert_ne!(pl.crop, Rect::new(0.0, 0.0, 1.0, 1.0), "{:?}", pl.slot_rect);
        }
    }

    /// R15. Every placement is re-cut for the new page's real-world shape.
    ///
    /// The mutation is `reprint` copying `crop` through untouched. The
    /// aspect assertion is what kills it: a crop cut for a 1.259:1 page is
    /// the wrong shape on a 0.8:1 one, and `cropStyle` would stretch the
    /// photo on screen while the export printed it distorted.
    #[test]
    fn changing_the_spec_recrops_every_placement() {
        let before = book();
        let ps = photos();
        let out = reprint(&before, &ps, &odd_spec());

        assert_eq!(out.book.spec, odd_spec());

        for (i, pl) in out.book.pages[0].placements.iter().enumerate() {
            let want = slot_aspect(&odd_spec(), &slot_for(pl.slot_rect));
            assert!(
                (printed_aspect(&pl.crop, &ps[i]) - want).abs() < 1e-12,
                "placement {i} still prints at {} on a page that wants {want}",
                printed_aspect(&pl.crop, &ps[i])
            );
            assert_ne!(
                pl.crop, before.pages[0].placements[i].crop,
                "placement {i} kept the crop it was cut to for the OLD page"
            );
        }

        // One placement named outright, so a failure says which number moved
        // rather than only that the relation broke. 4000x2667 in a slot that
        // is 0.50 x 0.30 of an 8.0 x 10.0 page prints 4.0" x 3.0", so the
        // crop keeps full height and 4/3 of the photo's own 1.4998:1 width.
        let first = &out.book.pages[0].placements[0];
        assert!((first.crop.h - 1.0).abs() < 1e-12, "a wider photo than slot keeps full height");
        assert!(
            (first.crop.w - (4.0 / 3.0) / (4000.0 / 2667.0)).abs() < 1e-12,
            "got {}",
            first.crop.w
        );
    }

    /// The other half of R15: everything that is not a crop is left alone.
    ///
    /// Changing the page size is not a reason to re-roll the book, and this
    /// is the assertion that stops a future "while we're here, re-run the
    /// packer" from quietly discarding every edit the user made.
    #[test]
    fn changing_the_spec_re_rolls_nothing() {
        let mut before = book();
        before.controls.entry(0).or_default().locked = true;
        before.controls.entry(0).or_default().rejected = vec!["02-duo".into()];

        let out = reprint(&before, &photos(), &odd_spec());

        assert_eq!(out.book.seed, before.seed, "the seed must survive a resize");
        assert_eq!(out.book.dropped, before.dropped);
        assert_eq!(out.book.controls, before.controls, "locks and rejections must survive");
        assert_eq!(out.book.pages.len(), before.pages.len());
        for (after, orig) in out.book.pages[0].placements.iter().zip(&before.pages[0].placements) {
            assert_eq!(after.slot_rect, orig.slot_rect, "a resize must not move a slot");
            assert_eq!(after.photo_index, orig.photo_index);
            assert_eq!(after.z, orig.z);
        }
        assert_eq!(out.book.pages[0].template_id, before.pages[0].template_id);
    }

    /// The condition is the page's *aspect*, so it has to watch both axes.
    ///
    /// The mutation is `page_shape_changed` comparing widths alone, which
    /// every other test here survives: `odd_spec` is narrower as well as
    /// taller. Making a landscape book square by raising the height only is
    /// the largest shape change the panel can produce, and a width-only
    /// check would pass it through with every crop still cut for 1.259:1.
    #[test]
    fn a_page_that_changes_only_its_height_is_still_a_new_shape() {
        let before = book();
        let ps = photos();
        let taller = spec_with(11.197, 11.197);

        let out = reprint(&before, &ps, &taller);

        for (i, pl) in out.book.pages[0].placements.iter().enumerate() {
            assert_ne!(
                pl.crop, before.pages[0].placements[i].crop,
                "placement {i} was not recut for a page that grew taller"
            );
            let want = slot_aspect(&taller, &slot_for(pl.slot_rect));
            assert!((printed_aspect(&pl.crop, &ps[i]) - want).abs() < 1e-12);
        }
    }

    /// The re-crop is conditional, and the condition is the page's inch
    /// aspect -- not "the spec changed".
    ///
    /// Recropping unconditionally would be a silent, invisible rewrite of
    /// every hand-adjusted crop in the book every time somebody nudged a
    /// margin. Exact equality is the assertion; an approximate one would
    /// pass under exactly the bug being guarded.
    ///
    /// The first placement is **panned by hand** first, to the same aspect
    /// `choose_crop` would pick but a different position. Without that, the
    /// test proves nothing: `choose_crop` at an unchanged aspect returns the
    /// same rect, so recomputing and skipping are indistinguishable, and a
    /// `reprint` that recut the whole book on every margin nudge would pass.
    /// The pan is what the user loses under that bug.
    #[test]
    fn an_edit_that_does_not_change_the_page_shape_leaves_a_hand_pan_alone() {
        let mut before = book();
        let panned = &mut before.pages[0].placements[0].crop;
        // Slot 0 is wider than its photo under Pixajoy, so the auto crop keeps
        // full width and centres vertically. Sliding it to the top edge is a
        // pan the user can make and `choose_crop` will never return.
        assert!(panned.y > 0.0, "the fixture must start somewhere it can be panned away from");
        panned.y = 0.0;
        // Same 11.197 x 8.894 page, wider bleed, tighter margin, new DPI band.
        let same_shape = PrintSpec::try_from(RawPrintSpec {
            page_w_in: 11.197,
            page_h_in: 8.894,
            bleed_in: 0.25,
            gutter_in: 0.30,
            safe_margin_in: 0.0625,
            min_dpi: 180.0,
            warn_dpi: 240.0,
        })
        .unwrap();

        let out = reprint(&before, &photos(), &same_shape);

        assert_eq!(out.book.spec, same_shape, "the new spec must still be adopted");
        for (after, orig) in out.book.pages[0].placements.iter().zip(&before.pages[0].placements) {
            assert_eq!(after.crop, orig.crop, "a margin edit must not recut a crop");
        }
    }

    /// R16. The dry run reports what breaks and touches nothing.
    ///
    /// Two-sided on purpose: a `reprint` that returned no findings at all
    /// would satisfy "left the book alone" on its own, and a preview that
    /// silently mutated would satisfy "reported something". The mutation is
    /// either half.
    #[test]
    fn previewing_a_spec_change_reports_what_breaks_and_leaves_the_book_alone() {
        let before = book();
        let snapshot = before.clone();

        // A 30 x 24 inch page at the same 200 DPI floor: the 0.50-wide slot
        // becomes 15 inches, and 4000px across 15 inches is 267 DPI, while
        // the 0.30-wide slot takes 3000px across 9 inches -- under the floor.
        let huge = spec_with(30.0, 24.0);
        let out = reprint(&before, &photos(), &huge);

        assert!(
            !out.findings.is_empty(),
            "a book that cannot print at the new size must say so before it is applied"
        );
        assert_eq!(before, snapshot, "a dry run must not touch the book it was asked about");
    }

    /// The findings are about geometry, asked without a filesystem. A
    /// `check_print_spec` that reported "source file no longer exists" for
    /// every photo, or blocked on free disk space, would be answering a
    /// question the user did not ask while they were still deciding.
    #[test]
    fn the_findings_never_mention_the_disk_or_a_missing_source() {
        let out = reprint(&book(), &photos(), &spec_with(30.0, 24.0));

        for f in &out.findings {
            assert!(!f.message.contains("no longer exists"), "{}", f.message);
            assert!(!f.message.to_lowercase().contains("disk"), "{}", f.message);
        }
    }
}
