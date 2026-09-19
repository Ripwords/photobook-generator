//! The print geometry a book was laid out under.
//!
//! Every print measurement used to be a `const` in `geometry.rs`, pinned to
//! one printer (Pixajoy's 11" x 8.5" landscape book). They now travel on the
//! book, so a user can print through a different lab without forking the app.
//!
//! The type is a parse, not a bag of numbers. Fields are private, there is no
//! `Deserialize` that skips the validator, and the only constructor that does
//! not parse is `pixajoy()`. Everything downstream -- `geometry`, `score`,
//! `preflight`, `manifest` -- may therefore assume a spec is sane without
//! re-checking it. See `PrintSpec::try_from` for exactly what it may assume.

use crate::geometry::{CoverSide, Rect, Side};
use serde::{Deserialize, Serialize};

/// Sanity rail, not a Pixajoy fact. A page beyond this is not a book, and
/// the DPI arithmetic downstream stops meaning anything.
const MAX_PAGE_IN: f64 = 100.0;

/// Sanity rail. Real case-bound wraps are well under an inch; past this the
/// number is a typo for the page size, not a board.
const MAX_WRAP_IN: f64 = 3.0;

/// Pixajoy's hardcover board wrap. Also what a spec saved before the cover
/// existed loads with, which is why it is a named function for serde.
fn pixajoy_cover_wrap_in() -> f64 {
    0.75
}

/// Which measurement a `SpecError` is about, so the UI can point at the
/// field and render the number in whatever unit is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpecField {
    PageWIn,
    PageHIn,
    BleedIn,
    GutterIn,
    SafeMarginIn,
    MinDpi,
    WarnDpi,
    CoverWrapIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Why a proposed spec was refused.
///
/// Structured rather than a `String` on purpose: a message built here reads
/// "bleed must be under 5.599 in" to someone typing millimetres. The webview
/// renders these in the unit on screen, and tests assert by variant instead
/// of by substring.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SpecError {
    NotFinite { field: SpecField },
    NotPositive { field: SpecField, value: f64 },
    Negative { field: SpecField, value: f64 },
    TooLarge { field: SpecField, value: f64, limit: f64 },
    NoSafeArea { axis: Axis, insets_in: f64, page_in: f64 },
    WarnBelowFloor { min_dpi: f64, warn_dpi: f64 },
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFinite { field } => write!(f, "{field:?} is not a finite number"),
            Self::NotPositive { field, value } => {
                write!(f, "{field:?} must be greater than zero, got {value}")
            }
            Self::Negative { field, value } => {
                write!(f, "{field:?} cannot be negative, got {value}")
            }
            Self::TooLarge { field, value, limit } => {
                write!(f, "{field:?} must be at most {limit}, got {value}")
            }
            Self::NoSafeArea { axis, insets_in, page_in } => write!(
                f,
                "{axis:?}: {insets_in}\" of bleed, safe margin and fold clearance \
                 leave no usable area on a {page_in}\" page"
            ),
            Self::WarnBelowFloor { min_dpi, warn_dpi } => write!(
                f,
                "the target resolution ({warn_dpi}) must be above the minimum ({min_dpi})"
            ),
        }
    }
}

impl std::error::Error for SpecError {}

/// The wire and storage form. Every field but one is required: a defaulted
/// field means a truncated or misspelled row silently gets a Pixajoy value
/// that looks identical to a correctly written one, which is the reasoning
/// already recorded on the template loader.
///
/// `cover_wrap_in` is the exception, and it is the lesser evil. Every book
/// row and saved draft written before the cover existed carries a seven-key
/// spec; requiring the eighth would make all of them fail to open. A missing
/// wrap only moves the cover, never an interior page.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawPrintSpec {
    pub page_w_in: f64,
    pub page_h_in: f64,
    pub bleed_in: f64,
    pub gutter_in: f64,
    pub safe_margin_in: f64,
    pub min_dpi: f64,
    pub warn_dpi: f64,
    #[serde(default = "pixajoy_cover_wrap_in")]
    pub cover_wrap_in: f64,
}

/// A validated print geometry.
///
/// Private fields with named accessors: the unit is in the name at every
/// call site (`page_w_in()`, `min_dpi()`), which is what an `Inches(f64)`
/// newtype would have bought at the cost of arithmetic impls everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "RawPrintSpec")]
pub struct PrintSpec {
    page_w_in: f64,
    page_h_in: f64,
    bleed_in: f64,
    gutter_in: f64,
    safe_margin_in: f64,
    min_dpi: f64,
    warn_dpi: f64,
    /// How far the cover photo runs past the trim to fold around the board,
    /// on the top, bottom and outer edge. There is none at the spine.
    cover_wrap_in: f64,
}

impl TryFrom<RawPrintSpec> for PrintSpec {
    type Error = SpecError;

    /// The only door. Past it, every consumer may assume without checking
    /// that page extents are finite and positive, that `trim_rect`,
    /// `safe_rect` and `gutter_band` all have positive area on both axes,
    /// and that `warn_dpi > min_dpi > 0` so `resolution_headroom` never
    /// divides by zero and never yields NaN.
    fn try_from(r: RawPrintSpec) -> Result<Self, Self::Error> {
        for (field, value) in [
            (SpecField::PageWIn, r.page_w_in),
            (SpecField::PageHIn, r.page_h_in),
            (SpecField::BleedIn, r.bleed_in),
            (SpecField::GutterIn, r.gutter_in),
            (SpecField::SafeMarginIn, r.safe_margin_in),
            (SpecField::MinDpi, r.min_dpi),
            (SpecField::WarnDpi, r.warn_dpi),
            (SpecField::CoverWrapIn, r.cover_wrap_in),
        ] {
            if !value.is_finite() {
                return Err(SpecError::NotFinite { field });
            }
        }

        for (field, value) in
            [(SpecField::PageWIn, r.page_w_in), (SpecField::PageHIn, r.page_h_in)]
        {
            if value <= 0.0 {
                return Err(SpecError::NotPositive { field, value });
            }
            if value > MAX_PAGE_IN {
                return Err(SpecError::TooLarge { field, value, limit: MAX_PAGE_IN });
            }
        }

        for (field, value) in [
            (SpecField::BleedIn, r.bleed_in),
            (SpecField::GutterIn, r.gutter_in),
            (SpecField::SafeMarginIn, r.safe_margin_in),
            (SpecField::CoverWrapIn, r.cover_wrap_in),
        ] {
            if value < 0.0 {
                return Err(SpecError::Negative { field, value });
            }
        }
        if r.cover_wrap_in > MAX_WRAP_IN {
            return Err(SpecError::TooLarge {
                field: SpecField::CoverWrapIn,
                value: r.cover_wrap_in,
                limit: MAX_WRAP_IN,
            });
        }

        if r.min_dpi <= 0.0 {
            return Err(SpecError::NotPositive {
                field: SpecField::MinDpi,
                value: r.min_dpi,
            });
        }
        // Strict: at equality `resolution_headroom`'s (dpi - min) / (warn - min)
        // is 0/0 = NaN, and a NaN soft term degrades template selection to
        // "whichever candidate the comparator happened to visit first".
        if r.warn_dpi <= r.min_dpi {
            return Err(SpecError::WarnBelowFloor {
                min_dpi: r.min_dpi,
                warn_dpi: r.warn_dpi,
            });
        }

        // The region both inside the safe margin and clear of the gutter is
        // [trim + safe, 1 - gutter]. Empty, and every face-bearing photo is
        // rejected on every template with no way out from inside the app.
        // The cover has no gutter but insets the safe margin at the spine
        // too, so its visible width is empty at trim <= 2 * safe; the larger
        // of the two inner insets covers both.
        let across = r.bleed_in + r.safe_margin_in + r.gutter_in.max(r.safe_margin_in);
        if across >= r.page_w_in {
            return Err(SpecError::NoSafeArea {
                axis: Axis::Horizontal,
                insets_in: across,
                page_in: r.page_w_in,
            });
        }
        let down = 2.0 * (r.bleed_in + r.safe_margin_in);
        if down >= r.page_h_in {
            return Err(SpecError::NoSafeArea {
                axis: Axis::Vertical,
                insets_in: down,
                page_in: r.page_h_in,
            });
        }

        Ok(Self {
            page_w_in: r.page_w_in,
            page_h_in: r.page_h_in,
            bleed_in: r.bleed_in,
            gutter_in: r.gutter_in,
            safe_margin_in: r.safe_margin_in,
            min_dpi: r.min_dpi,
            warn_dpi: r.warn_dpi,
            cover_wrap_in: r.cover_wrap_in,
        })
    }
}

impl PrintSpec {
    /// Pixajoy's 11" x 8.5" landscape book: the geometry every book was laid
    /// out under before the spec became configurable, and the default for a
    /// new one.
    ///
    /// Named after the printer, not `default`, so a reader at a call site can
    /// see that they hardcoded one lab rather than used the book's own. The
    /// call sites are meant to stay countable: the new-book default, `Book`'s
    /// serde default, and tests.
    ///
    /// `0.197` is a rounded inch value of a 5 mm bleed (5 mm is exactly
    /// 0.1968503937"). Do not "correct" it: it moves the golden for no
    /// user-visible gain.
    pub fn pixajoy() -> Self {
        Self {
            page_w_in: 11.197,
            page_h_in: 8.894,
            bleed_in: 0.197,
            gutter_in: 0.197,
            safe_margin_in: 0.125,
            min_dpi: 200.0,
            warn_dpi: 300.0,
            cover_wrap_in: pixajoy_cover_wrap_in(),
        }
    }

    pub fn page_w_in(&self) -> f64 {
        self.page_w_in
    }
    pub fn page_h_in(&self) -> f64 {
        self.page_h_in
    }
    pub fn bleed_in(&self) -> f64 {
        self.bleed_in
    }
    pub fn gutter_in(&self) -> f64 {
        self.gutter_in
    }
    pub fn safe_margin_in(&self) -> f64 {
        self.safe_margin_in
    }
    pub fn min_dpi(&self) -> f64 {
        self.min_dpi
    }
    pub fn warn_dpi(&self) -> f64 {
        self.warn_dpi
    }

    /// Two pages, by definition. Stored nowhere, so it cannot drift.
    pub fn spread_w_in(&self) -> f64 {
        2.0 * self.page_w_in
    }

    /// Page-normalised trim inset on the outer vertical edge.
    pub fn trim_u(&self) -> f64 {
        self.bleed_in / self.page_w_in
    }
    /// Page-normalised trim inset on the top and bottom edges.
    pub fn trim_v(&self) -> f64 {
        self.bleed_in / self.page_h_in
    }
    /// Page-normalised safe-margin inset, horizontally.
    pub fn safe_u(&self) -> f64 {
        self.safe_margin_in / self.page_w_in
    }
    /// Page-normalised safe-margin inset, vertically.
    pub fn safe_v(&self) -> f64 {
        self.safe_margin_in / self.page_h_in
    }
    /// Page-normalised width of the gutter dead strip.
    pub fn gutter_u(&self) -> f64 {
        self.gutter_in / self.page_w_in
    }

    /// The trim rectangle of a page, page-normalised.
    ///
    /// The fold edge carries no inset: the paper is continuous there, so
    /// there is nothing to guillotine. That asymmetry is why this takes a
    /// `Side`.
    pub fn trim_rect(&self, side: Side) -> Rect {
        let t = self.trim_u();
        let (x0, x1) = match side {
            Side::Left => (t, 1.0),
            Side::Right => (0.0, 1.0 - t),
        };
        Rect::new(x0, self.trim_v(), x1 - x0, 1.0 - 2.0 * self.trim_v())
    }

    /// The safe rectangle of a page: `trim_rect` inset by a further safe
    /// margin on every edge EXCEPT the fold.
    ///
    /// No inset at the fold, for the same reason as `trim_rect` plus one
    /// more: the gutter band already governs clearance on that edge, and
    /// insetting here would double-count it.
    pub fn safe_rect(&self, side: Side) -> Rect {
        let i = self.trim_u() + self.safe_u();
        let (x0, x1) = match side {
            Side::Left => (i, 1.0),
            Side::Right => (0.0, 1.0 - i),
        };
        let v = self.trim_v() + self.safe_v();
        Rect::new(x0, v, x1 - x0, 1.0 - 2.0 * v)
    }

    /// The gutter dead band on `side`: the strip nearest the fold that curls
    /// into the binding, spanning the full page height.
    ///
    /// The single definition of the band. `geometry::clear_of_gutter` and
    /// `geometry::gutter_overlap_area` both read it rather than restating
    /// the inset, so there is no second copy to drift.
    pub fn gutter_band(&self, side: Side) -> Rect {
        let g = self.gutter_u();
        match side {
            Side::Left => Rect::new(1.0 - g, 0.0, g, 1.0),
            Side::Right => Rect::new(0.0, 0.0, g, 1.0),
        }
    }

    /// A rect's real-world width/height ratio on this spec's page.
    ///
    /// Replaces the old free `Rect::aspect_in(w, h)`, whose two anonymous
    /// `f64`s were the trap: `aspect_pref` is an inch ratio and a
    /// page-normalised `w / h` is a different number, so passing the spread
    /// canvas where the page was meant mis-scored every slot. There is no
    /// canvas argument here to get wrong.
    pub fn page_aspect(&self, rect: &Rect) -> f64 {
        (rect.w * self.page_w_in) / (rect.h * self.page_h_in)
    }

    pub fn cover_wrap_in(&self) -> f64 {
        self.cover_wrap_in
    }

    fn trim_w_in(&self) -> f64 {
        self.page_w_in - self.bleed_in
    }
    fn trim_h_in(&self) -> f64 {
        self.page_h_in - 2.0 * self.bleed_in
    }

    /// One cover panel, front or back: the trim plus the wrap on its outer
    /// edge. The wrap replaces the bleed rather than adding to it.
    pub fn cover_panel_w_in(&self) -> f64 {
        self.trim_w_in() + self.cover_wrap_in
    }
    /// The trim plus the wrap at the top and bottom.
    pub fn cover_panel_h_in(&self) -> f64 {
        self.trim_h_in() + 2.0 * self.cover_wrap_in
    }
    /// The panel's inch ratio: the aspect a cover crop is cut to.
    pub fn cover_aspect(&self) -> f64 {
        self.cover_panel_w_in() / self.cover_panel_h_in()
    }

    /// The finished board on a cover panel, panel-normalised: the trim,
    /// against the spine edge. The rest of the panel is the wrap.
    pub fn cover_board_rect(&self, side: CoverSide) -> Rect {
        let (pw, ph) = (self.cover_panel_w_in(), self.cover_panel_h_in());
        let x = match side {
            CoverSide::Front => 0.0,
            CoverSide::Back => self.cover_wrap_in,
        };
        Rect::new(x / pw, self.cover_wrap_in / ph, self.trim_w_in() / pw, self.trim_h_in() / ph)
    }

    /// The part of a cover panel that shows on the finished board, less the
    /// safe margin, panel-normalised.
    ///
    /// The wrap folds under the board on the three outer edges. The spine
    /// edge has no wrap, but it does sit on the hinge, so the safe margin is
    /// inset there too; the cover has no gutter band to govern it instead.
    pub fn cover_visible_rect(&self, side: CoverSide) -> Rect {
        let (pw, ph) = (self.cover_panel_w_in(), self.cover_panel_h_in());
        let (wrap, safe) = (self.cover_wrap_in, self.safe_margin_in);
        let x = match side {
            CoverSide::Front => safe,
            CoverSide::Back => wrap + safe,
        };
        Rect::new(
            x / pw,
            (wrap + safe) / ph,
            (self.trim_w_in() - 2.0 * safe) / pw,
            (self.trim_h_in() - 2.0 * safe) / ph,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixajoy's numbers as a raw payload, for tests that vary one field.
    fn raw() -> RawPrintSpec {
        RawPrintSpec {
            page_w_in: 11.197,
            page_h_in: 8.894,
            bleed_in: 0.197,
            gutter_in: 0.197,
            safe_margin_in: 0.125,
            min_dpi: 200.0,
            warn_dpi: 300.0,
            cover_wrap_in: 0.75,
        }
    }

    /// R1. Pinned AT the boundary, both sides. A fixture comfortably past it
    /// survives a `>=`/`>` flip, which is the exact failure this project has
    /// already shipped once.
    #[test]
    fn print_spec_rejects_insets_that_leave_no_safe_area() {
        // Horizontal: bleed + safe + gutter against page width.
        let mut at = raw();
        at.page_w_in = at.bleed_in + at.safe_margin_in + at.gutter_in;
        assert!(
            matches!(
                PrintSpec::try_from(at),
                Err(SpecError::NoSafeArea { axis: Axis::Horizontal, .. })
            ),
            "exactly zero usable width must be refused"
        );

        let mut under = raw();
        under.page_w_in = under.bleed_in + under.safe_margin_in + under.gutter_in + 1e-6;
        assert!(
            PrintSpec::try_from(under).is_ok(),
            "a sliver of usable width must be accepted"
        );

        // Vertical: 2 * (bleed + safe) against page height.
        let mut at_v = raw();
        at_v.page_h_in = 2.0 * (at_v.bleed_in + at_v.safe_margin_in);
        assert!(
            matches!(
                PrintSpec::try_from(at_v),
                Err(SpecError::NoSafeArea { axis: Axis::Vertical, .. })
            ),
            "exactly zero usable height must be refused"
        );

        let mut under_v = raw();
        under_v.page_h_in = 2.0 * (under_v.bleed_in + under_v.safe_margin_in) + 1e-6;
        assert!(
            PrintSpec::try_from(under_v).is_ok(),
            "a sliver of usable height must be accepted"
        );
    }

    /// R2. The negative fixture is the load-bearing one: zero can "pass" a
    /// guard via NaN propagation, which `geometry.rs` already records.
    #[test]
    fn print_spec_rejects_a_non_positive_or_non_finite_page() {
        for bad in [0.0, -1.0] {
            let mut r = raw();
            r.page_w_in = bad;
            assert!(
                matches!(
                    PrintSpec::try_from(r),
                    Err(SpecError::NotPositive { field: SpecField::PageWIn, .. })
                ),
                "page width {bad} must be refused"
            );
        }

        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut r = raw();
            r.page_h_in = bad;
            assert!(
                matches!(
                    PrintSpec::try_from(r),
                    Err(SpecError::NotFinite { field: SpecField::PageHIn })
                ),
                "page height {bad} must be refused"
            );
        }

        let mut huge = raw();
        huge.page_w_in = MAX_PAGE_IN + 1.0;
        assert!(matches!(
            PrintSpec::try_from(huge),
            Err(SpecError::TooLarge { field: SpecField::PageWIn, .. })
        ));
    }

    /// R3. Strict inequality, and the consequence pinned alongside the guard:
    /// nothing that parses can make the headroom term non-finite.
    #[test]
    fn print_spec_rejects_a_warn_dpi_at_or_below_the_floor() {
        let mut equal = raw();
        equal.warn_dpi = equal.min_dpi;
        assert!(
            matches!(PrintSpec::try_from(equal), Err(SpecError::WarnBelowFloor { .. })),
            "warn == min divides by zero in resolution_headroom"
        );

        let mut inverted = raw();
        inverted.warn_dpi = inverted.min_dpi - 1.0;
        assert!(matches!(
            PrintSpec::try_from(inverted),
            Err(SpecError::WarnBelowFloor { .. })
        ));

        let mut zero_floor = raw();
        zero_floor.min_dpi = 0.0;
        assert!(matches!(
            PrintSpec::try_from(zero_floor),
            Err(SpecError::NotPositive { field: SpecField::MinDpi, .. })
        ));

        // The consequence, not just the guard.
        for spec in [PrintSpec::pixajoy(), odd_spec()] {
            let span = spec.warn_dpi() - spec.min_dpi();
            assert!(span.is_finite() && span > 0.0, "headroom span must be usable");
        }
    }

    /// R4. The load-bearing one. This is what makes the persistence read safe
    /// without a second call site: there is no `Deserialize` that skips the
    /// validator, so a hand-edited row cannot put an impossible spec into the
    /// engine.
    #[test]
    fn a_print_spec_cannot_be_deserialised_past_its_own_validator() {
        let json = r#"{"pageWIn":0.0,"pageHIn":8.894,"bleedIn":0.197,"gutterIn":0.197,
                       "safeMarginIn":0.125,"minDpi":200.0,"warnDpi":300.0}"#;
        assert!(
            serde_json::from_str::<PrintSpec>(json).is_err(),
            "a zero-width page must not deserialise"
        );

        let inverted = r#"{"pageWIn":11.197,"pageHIn":8.894,"bleedIn":0.197,"gutterIn":0.197,
                          "safeMarginIn":0.125,"minDpi":300.0,"warnDpi":200.0}"#;
        assert!(
            serde_json::from_str::<PrintSpec>(inverted).is_err(),
            "an inverted DPI band must not deserialise"
        );
    }

    /// R5. A missing field fails loudly rather than silently becoming Pixajoy.
    #[test]
    fn print_spec_deserialisation_refuses_a_row_missing_a_field() {
        let no_gutter = r#"{"pageWIn":11.197,"pageHIn":8.894,"bleedIn":0.197,
                            "safeMarginIn":0.125,"minDpi":200.0,"warnDpi":300.0}"#;
        assert!(
            serde_json::from_str::<PrintSpec>(no_gutter).is_err(),
            "a missing gutterIn must fail, not default to 0.197"
        );

        // All seven fields present AND an eighth key. Nothing is missing, so
        // only `deny_unknown_fields` can reject this -- a misspelled field
        // that also drops the real one is rejected for being absent, and
        // passes with `deny_unknown_fields` deleted. (That weaker fixture is
        // what this test shipped with, and the mutation run caught it.)
        let extra = r#"{"pageWIn":11.197,"pageHIn":8.894,"bleedIn":0.197,"gutterIn":0.197,
                        "safeMarginIn":0.125,"minDpi":200.0,"warnDpi":300.0,"gutterInch":0.4}"#;
        assert!(
            serde_json::from_str::<PrintSpec>(extra).is_err(),
            "a misspelled field must fail, not be silently ignored"
        );
    }

    /// R6. The spread cannot drift from the page, because it is not stored.
    #[test]
    fn the_spread_is_always_twice_the_page_width() {
        for spec in [PrintSpec::pixajoy(), odd_spec()] {
            assert!((spec.spread_w_in() - 2.0 * spec.page_w_in()).abs() < 1e-12);
        }
        assert!((PrintSpec::pixajoy().spread_w_in() - 22.394).abs() < 1e-9);
    }

    /// Pixajoy's numbers, pinned against typed literals rather than against a
    /// surviving constant -- asserting `pixajoy().page_w_in == PAGE_W_IN`
    /// would move both sides together and prove nothing.
    #[test]
    fn pixajoy_is_todays_numbers() {
        let s = PrintSpec::pixajoy();
        assert_eq!(s.page_w_in(), 11.197);
        assert_eq!(s.page_h_in(), 8.894);
        assert_eq!(s.bleed_in(), 0.197);
        assert_eq!(s.gutter_in(), 0.197);
        assert_eq!(s.safe_margin_in(), 0.125);
        assert_eq!(s.min_dpi(), 200.0);
        assert_eq!(s.warn_dpi(), 300.0);
        assert!(PrintSpec::try_from(raw()).is_ok(), "pixajoy must itself validate");
        assert_eq!(PrintSpec::try_from(raw()).unwrap(), s);
    }

    /// Derived insets are functions of the stored fields, on BOTH axes.
    ///
    /// `odd_spec` exists so no two derived values coincide: under Pixajoy's
    /// numbers `gutter_u == trim_u`, so an axis swap or the old
    /// gutter-equals-bleed coupling would survive any default-spec fixture.
    #[test]
    fn derived_insets_read_the_field_and_the_axis_they_name() {
        let s = odd_spec();
        assert!((s.trim_u() - 0.25 / 8.0).abs() < 1e-12, "trim_u is bleed / page_w");
        assert!((s.trim_v() - 0.25 / 10.0).abs() < 1e-12, "trim_v is bleed / page_h");
        assert!((s.safe_u() - 0.05 / 8.0).abs() < 1e-12, "safe_u is safe / page_w");
        assert!((s.safe_v() - 0.05 / 10.0).abs() < 1e-12, "safe_v is safe / page_h");
        assert!((s.gutter_u() - 0.4 / 8.0).abs() < 1e-12, "gutter_u is gutter / page_w");

        // Every pair distinct, so a transposition is detectable at all.
        let all = [s.trim_u(), s.trim_v(), s.safe_u(), s.safe_v(), s.gutter_u()];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!((a - b).abs() > 1e-6, "fixture must keep {a} and {b} apart");
            }
        }
    }

    /// The gutter is no longer numerically tied to the bleed. Under Pixajoy
    /// they coincide, which is why this asserts on a spec where they do not.
    #[test]
    fn the_gutter_is_independent_of_the_bleed() {
        let s = odd_spec();
        assert!(
            (s.gutter_u() - s.trim_u()).abs() > 1e-6,
            "gutter_u must not be bleed / page_w"
        );
        assert!((PrintSpec::pixajoy().gutter_u() - PrintSpec::pixajoy().trim_u()).abs() < 1e-12,
            "under Pixajoy they still coincide, which is why odd_spec exists");
    }

    #[test]
    fn page_aspect_is_the_inch_ratio_not_the_normalised_one() {
        let s = PrintSpec::pixajoy();
        // Half the page wide and half tall: 1:1 normalised, but the page is
        // not square, so the inch ratio is the page's own aspect.
        let r = Rect::new(0.0, 0.0, 0.5, 0.5);
        let normalised = r.w / r.h;
        let real = s.page_aspect(&r);
        assert!((normalised - 1.0).abs() < 1e-9, "sanity: the fixture is 1:1 normalised");
        assert!((real - 11.197 / 8.894).abs() < 1e-9, "real was {real}");
        assert!(
            (real - normalised).abs() > 0.2,
            "the fixture must distinguish the two numbers"
        );
    }

    /// The one defaulted field. Every book and draft saved before the cover
    /// existed has a seven-key spec, and must load with Pixajoy's wrap
    /// rather than fail to open.
    #[test]
    fn a_spec_saved_before_the_cover_loads_with_pixajoys_wrap() {
        let legacy = r#"{"pageWIn":8.0,"pageHIn":10.0,"bleedIn":0.25,"gutterIn":0.4,
                         "safeMarginIn":0.05,"minDpi":150.0,"warnDpi":220.0}"#;
        let spec: PrintSpec = serde_json::from_str(legacy).expect("a legacy spec must load");
        assert_eq!(spec.cover_wrap_in(), 0.75);
        assert_eq!(spec.page_w_in(), 8.0, "the other fields are read, not defaulted");
        assert_eq!(PrintSpec::pixajoy().cover_wrap_in(), 0.75);
    }

    /// The shared fixture pins the legacy default for the webview's draft
    /// reader too, so the TS copy of 0.75 cannot drift from this one.
    #[test]
    fn the_legacy_print_spec_fixture_loads_as_it_says() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/wire/legacy-print-spec.json"
        ))
        .unwrap();
        let spec: PrintSpec = serde_json::from_value(fixture["legacy"].clone()).unwrap();
        assert_eq!(serde_json::to_value(spec).unwrap(), fixture["loadsAs"]);
    }

    #[test]
    fn print_spec_rejects_a_bad_cover_wrap() {
        let mut negative = raw();
        negative.cover_wrap_in = -0.01;
        assert!(matches!(
            PrintSpec::try_from(negative),
            Err(SpecError::Negative { field: SpecField::CoverWrapIn, .. })
        ));

        let mut nan = raw();
        nan.cover_wrap_in = f64::NAN;
        assert!(matches!(
            PrintSpec::try_from(nan),
            Err(SpecError::NotFinite { field: SpecField::CoverWrapIn })
        ));

        let mut at = raw();
        at.cover_wrap_in = MAX_WRAP_IN;
        assert!(PrintSpec::try_from(at).is_ok(), "the limit itself is allowed");
        let mut over = raw();
        over.cover_wrap_in = MAX_WRAP_IN + 1e-6;
        assert!(matches!(
            PrintSpec::try_from(over),
            Err(SpecError::TooLarge { field: SpecField::CoverWrapIn, .. })
        ));

        let mut zero = raw();
        zero.cover_wrap_in = 0.0;
        assert!(PrintSpec::try_from(zero).is_ok(), "a soft cover has no wrap");
    }

    /// The cover's visible rect insets the safe margin on the spine edge as
    /// well, so a margin wider than the fold must still leave cover width.
    #[test]
    fn print_spec_refuses_a_safe_margin_that_leaves_no_cover_width() {
        let mut r = raw();
        r.gutter_in = 0.0;
        r.page_w_in = r.bleed_in + 2.0 * r.safe_margin_in;
        assert!(matches!(
            PrintSpec::try_from(r),
            Err(SpecError::NoSafeArea { axis: Axis::Horizontal, .. })
        ));
        r.page_w_in += 1e-6;
        assert!(PrintSpec::try_from(r).is_ok());
    }

    /// Pixajoy: an 11 x 8.5 trim plus 0.75" of wrap on three edges.
    #[test]
    fn the_cover_panel_is_the_trim_plus_wrap_on_three_edges() {
        let p = PrintSpec::pixajoy();
        assert!((p.cover_panel_w_in() - 11.75).abs() < 1e-9, "{}", p.cover_panel_w_in());
        assert!((p.cover_panel_h_in() - 10.0).abs() < 1e-9, "{}", p.cover_panel_h_in());
        assert!((p.cover_aspect() - 1.175).abs() < 1e-9);

        // Portrait, and every number distinct, so a swapped axis shows.
        let o = odd_spec();
        let (tw, th, wrap) = (8.0 - 0.25, 10.0 - 0.5, o.cover_wrap_in());
        assert!((o.cover_panel_w_in() - (tw + wrap)).abs() < 1e-12);
        assert!((o.cover_panel_h_in() - (th + 2.0 * wrap)).abs() < 1e-12);
        assert!((o.cover_aspect() - (tw + wrap) / (th + 2.0 * wrap)).abs() < 1e-12);
    }

    /// The front's spine is its left edge, so its wrap is on the right; the
    /// back mirrors it. The safe margin applies on all four edges.
    #[test]
    fn the_cover_visible_rect_drops_the_wrap_on_the_outer_edges_only() {
        let o = odd_spec();
        let (pw, ph) = (o.cover_panel_w_in(), o.cover_panel_h_in());
        let (tw, th, wrap, safe) = (7.75, 9.5, o.cover_wrap_in(), 0.05);

        let front = o.cover_visible_rect(CoverSide::Front);
        assert!((front.x - safe / pw).abs() < 1e-12, "front x {}", front.x);
        assert!((front.right() - (tw - safe) / pw).abs() < 1e-12, "front right {}", front.right());
        assert!((front.y - (wrap + safe) / ph).abs() < 1e-12, "front y {}", front.y);
        assert!((front.bottom() - (wrap + th - safe) / ph).abs() < 1e-12);

        let back = o.cover_visible_rect(CoverSide::Back);
        assert!((back.x - (wrap + safe) / pw).abs() < 1e-12, "back x {}", back.x);
        assert!((back.right() - (1.0 - safe / pw)).abs() < 1e-12, "back right {}", back.right());
        assert_eq!((back.y, back.h), (front.y, front.h));
        assert!((back.w - front.w).abs() < 1e-12);
    }

    /// The board is the trim on the panel: flush with the spine edge, the
    /// wrap outside it on the other three. Everything outside it folds under.
    #[test]
    fn the_cover_board_rect_is_the_trim_with_the_wrap_outside_it() {
        let o = odd_spec();
        let (pw, ph) = (o.cover_panel_w_in(), o.cover_panel_h_in());
        let (tw, th, wrap) = (7.75, 9.5, o.cover_wrap_in());

        let front = o.cover_board_rect(CoverSide::Front);
        assert!(front.x.abs() < 1e-12, "front x {}", front.x);
        assert!((front.right() - tw / pw).abs() < 1e-12, "front right {}", front.right());
        assert!((front.y - wrap / ph).abs() < 1e-12, "front y {}", front.y);
        assert!((front.bottom() - (wrap + th) / ph).abs() < 1e-12, "front bottom {}", front.bottom());

        let back = o.cover_board_rect(CoverSide::Back);
        assert!((back.x - wrap / pw).abs() < 1e-12, "back x {}", back.x);
        assert!((back.right() - 1.0).abs() < 1e-12, "back right {}", back.right());
        assert_eq!((back.y, back.h), (front.y, front.h));

        for side in [CoverSide::Front, CoverSide::Back] {
            assert!(o.cover_board_rect(side).contains(&o.cover_visible_rect(side)), "{side:?}");
        }
    }
}

/// Today's default, abbreviated. Named for brevity at ~150 test call sites
/// that only need *a* valid spec; every test that could be fooled by
/// Pixajoy's coincidences (`gutter_in == bleed_in`, a landscape page) runs
/// `odd_spec` as well.
#[cfg(test)]
pub(crate) fn pixajoy_spec() -> PrintSpec {
    PrintSpec::pixajoy()
}

/// A second spec for tests anywhere in the crate, with every number distinct
/// from every other AND from Pixajoy's, so transposing two fields is
/// detectable.
///
/// It exists because every geometry test in this crate runs on the default
/// spec, where `gutter_u == trim_u` and the page happens to be landscape.
/// Such a test cannot tell a correct derivation from several wrong ones.
///
/// PORTRAIT on purpose: no fixture in this crate has ever had
/// `page_h_in > page_w_in`, so any code that quietly assumed landscape has
/// never been exercised.
#[cfg(test)]
pub(crate) fn odd_spec() -> PrintSpec {
    PrintSpec::try_from(RawPrintSpec {
        page_w_in: 8.0,
        page_h_in: 10.0,
        bleed_in: 0.25,
        gutter_in: 0.4,
        safe_margin_in: 0.05,
        min_dpi: 150.0,
        warn_dpi: 220.0,
        cover_wrap_in: 0.6,
    })
    .expect("odd_spec must be a valid spec")
}

/// Pixajoy's page with a different resolution band, for the two tests that
/// must vary `min_dpi`/`warn_dpi` while holding every inch measurement
/// fixed. Varying the page as well would change `effective_dpi` too, and a
/// fixture whose DPI moves for two reasons at once cannot attribute the
/// result to either.
#[cfg(test)]
pub(crate) fn spec_with_dpi(min_dpi: f64, warn_dpi: f64) -> PrintSpec {
    let p = PrintSpec::pixajoy();
    PrintSpec::try_from(RawPrintSpec {
        page_w_in: p.page_w_in(),
        page_h_in: p.page_h_in(),
        bleed_in: p.bleed_in(),
        gutter_in: p.gutter_in(),
        safe_margin_in: p.safe_margin_in(),
        min_dpi,
        warn_dpi,
        cover_wrap_in: p.cover_wrap_in(),
    })
    .expect("spec_with_dpi must be called with a valid band")
}
