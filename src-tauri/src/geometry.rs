//! Page-local geometry for the layout engine.
//!
//! The authoring format is the SPREAD canvas (normalised [0,1] across two
//! pages); the engine's atom is the PAGE. Pixajoy-style picture boxes are
//! page-local and cannot cross the fold, so a rect that spans the fold has
//! no page representation at all -- `spread_to_page` returns `None` rather
//! than clamping, and the template validator rejects such slots before they
//! ever reach here.
//!
//! The inch measurements live on the book's `PrintSpec`, not here. Every
//! predicate below takes a `&PrintSpec`, and a `PrintSpec` cannot exist
//! unless it passed `TryFrom`. So this module assumes WITHOUT CHECKING that
//! page extents are finite and positive, that `trim_rect`, `safe_rect` and
//! `gutter_band` all have positive area on both axes, and that
//! `warn_dpi > min_dpi > 0` so `score::resolution_headroom` never divides by
//! zero. Do not re-check these here. If you want to, the check belongs in
//! `PrintSpec::try_from`.

use crate::print_spec::PrintSpec;
use serde::{Deserialize, Serialize};

/// The fold, normalised on the spread canvas. The one measurement that is
/// genuinely page-size independent: two equal pages put the fold in the
/// middle whatever they measure, which is why every `templates/*.json` stays
/// valid under any spec.
const FOLD_X: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Float tolerance shared by every predicate here. A rect landing exactly
/// on a band edge is inside it: the alternative is a slot that a template
/// authored flush to the trim line fails by one ulp.
const EPS: f64 = 1e-9;

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn area(&self) -> f64 {
        if self.w <= 0.0 || self.h <= 0.0 {
            0.0
        } else {
            self.w * self.h
        }
    }

    /// True when `inner` lies wholly within this rect, to within the
    /// float tolerance every predicate in this module shares.
    pub fn contains(&self, inner: &Rect) -> bool {
        inner.x >= self.x - EPS
            && inner.right() <= self.right() + EPS
            && inner.y >= self.y - EPS
            && inner.bottom() <= self.bottom() + EPS
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        if self.w <= 0.0 || self.h <= 0.0 || other.w <= 0.0 || other.h <= 0.0 {
            return None;
        }
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r <= x || b <= y {
            return None;
        }
        Some(Rect::new(x, y, r - x, b - y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BleedEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Converts a spread-normalised rect into `(side, page-normalised rect)`.
/// Returns `None` for a rect that spans the fold: such a rect cannot be
/// built in Pixajoy's editor and has no page representation.
pub fn spread_to_page(rect: &Rect) -> Option<(Side, Rect)> {
    if rect.x < FOLD_X - EPS && rect.right() > FOLD_X + EPS {
        return None;
    }
    let side = if rect.right() <= FOLD_X + EPS { Side::Left } else { Side::Right };
    let origin = match side {
        Side::Left => 0.0,
        Side::Right => FOLD_X,
    };
    Some((side, Rect::new((rect.x - origin) / FOLD_X, rect.y, rect.w / FOLD_X, rect.h)))
}

/// True when the rect lies wholly inside the trim rectangle of its page.
pub fn in_trim(spec: &PrintSpec, rect: &Rect, side: Side) -> bool {
    spec.trim_rect(side).contains(rect)
}

/// True when the rect lies wholly inside the SAFE rectangle of its page --
/// the trim rect inset by a further safe margin on every edge EXCEPT the
/// fold. `in_safe_margin` is therefore always a SUBSET of `in_trim`: passing
/// this implies passing `in_trim`, but not the reverse.
pub fn in_safe_margin(spec: &PrintSpec, rect: &Rect, side: Side) -> bool {
    spec.safe_rect(side).contains(rect)
}

/// True when the rect keeps clear of the gutter dead strip -- the band
/// nearest the fold, which curls into the binding. This is a CONTENT
/// predicate (faces, salient regions), never a slot rejection: a slot may
/// legitimately run flush to the fold.
///
/// Reads `PrintSpec::gutter_band`, the one definition of the strip, rather
/// than restating the inset. `gutter_overlap_area` is the graded
/// counterpart and reads the same band.
pub fn clear_of_gutter(spec: &PrintSpec, rect: &Rect, side: Side) -> bool {
    let band = spec.gutter_band(side);
    match side {
        Side::Left => rect.right() <= band.x + EPS,
        Side::Right => rect.x >= band.right() - EPS,
    }
}

/// Area of `rect` (page-normalised, on `side`) falling inside the gutter
/// band.
///
/// The graded counterpart to `clear_of_gutter`, which answers yes/no -- the
/// right shape for a rejection and the wrong one for a penalty.
pub fn gutter_overlap_area(spec: &PrintSpec, rect: &Rect, side: Side) -> f64 {
    rect.intersect(&spec.gutter_band(side)).map_or(0.0, |i| i.area())
}

/// True when every declared bleed edge actually reaches past the page
/// boundary. A slot that declares bleed but stops short leaves a white
/// sliver after trimming.
///
/// The fold edge cannot bleed: on a left page that is the RIGHT edge, on a
/// right page the LEFT edge. Declaring it is always an error.
pub fn bleeds_correctly(rect: &Rect, edges: &[BleedEdge], side: Side) -> bool {
    edges.iter().all(|edge| match (edge, side) {
        (BleedEdge::Right, Side::Left) | (BleedEdge::Left, Side::Right) => false,
        (BleedEdge::Left, Side::Left) => rect.x <= EPS,
        (BleedEdge::Right, Side::Right) => rect.right() >= 1.0 - EPS,
        (BleedEdge::Top, _) => rect.y <= EPS,
        (BleedEdge::Bottom, _) => rect.bottom() >= 1.0 - EPS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;

    // --- geometry: spread -> page conversion

    #[test]
    fn geometry_converts_a_left_page_rect_to_page_coordinates() {
        // Spans a quarter of the spread starting at the left edge -> the left
        // half of the LEFT page. Deliberately not symmetric about anything.
        let r = Rect::new(0.05, 0.1, 0.20, 0.6);
        let (side, page) = spread_to_page(&r).expect("does not span the fold");
        assert_eq!(side, Side::Left);
        assert!((page.x - 0.10).abs() < 1e-9, "x was {}", page.x);
        assert!((page.w - 0.40).abs() < 1e-9, "w was {}", page.w);
        assert!((page.y - 0.1).abs() < 1e-9, "y is unchanged");
        assert!((page.h - 0.6).abs() < 1e-9, "h is unchanged");
    }

    #[test]
    fn geometry_converts_a_right_page_rect_to_page_coordinates() {
        let r = Rect::new(0.55, 0.1, 0.20, 0.6);
        let (side, page) = spread_to_page(&r).expect("does not span the fold");
        assert_eq!(side, Side::Right);
        assert!((page.x - 0.10).abs() < 1e-9, "x was {}", page.x);
        assert!((page.w - 0.40).abs() < 1e-9, "w was {}", page.w);
    }

    #[test]
    fn geometry_rejects_a_rect_that_spans_the_fold() {
        assert!(spread_to_page(&Rect::new(0.4, 0.0, 0.2, 1.0)).is_none());
    }

    /// Flush TO the fold is legal on both sides — this is the whole reason
    /// the rule is "spans the fold" rather than "touches the dead band".
    #[test]
    fn geometry_accepts_a_rect_flush_to_the_fold_from_either_side() {
        let (side, page) = spread_to_page(&Rect::new(0.3, 0.0, 0.2, 1.0)).unwrap();
        assert_eq!(side, Side::Left);
        assert!((page.right() - 1.0).abs() < 1e-9);

        let (side, page) = spread_to_page(&Rect::new(0.5, 0.0, 0.2, 1.0)).unwrap();
        assert_eq!(side, Side::Right);
        assert!((page.x - 0.0).abs() < 1e-9);
    }

    // --- geometry: predicates

    #[test]
    fn geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_left_page() {
        // On a LEFT page the outer edge is x=0, so trim starts at pixajoy_spec().trim_u().
        assert!(!in_trim(&pixajoy_spec(), &Rect::new(0.0, 0.5, 0.1, 0.1), Side::Left));
        assert!(in_trim(&pixajoy_spec(), &Rect::new(pixajoy_spec().trim_u(), 0.5, 0.1, 0.1), Side::Left));
    }

    #[test]
    fn geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_right_page() {
        // On a RIGHT page the outer edge is x=1, so trim ends at 1 - pixajoy_spec().trim_u().
        assert!(!in_trim(&pixajoy_spec(), &Rect::new(0.95, 0.5, 0.1, 0.1), Side::Right));
        assert!(in_trim(&pixajoy_spec(), &Rect::new(0.8, 0.5, 1.0 - pixajoy_spec().trim_u() - 0.8, 0.1), Side::Right));
    }

    // --- geometry: safe margin

    /// The core property: `in_safe_margin` is a STRICT subset of `in_trim`.
    /// A rect sitting exactly at the trim boundary passes `in_trim` (no
    /// inset failure there) but must fail `in_safe_margin`, which insets a
    /// further 1/8" beyond it. This is the fixture that catches a
    /// `in_safe_margin` implemented as a bare delegation to `in_trim` -- the
    /// mutation the brief names explicitly.
    #[test]
    fn geometry_in_safe_margin_is_a_strict_subset_of_in_trim() {
        let rect = Rect::new(pixajoy_spec().trim_u(), 0.5, 0.05, 0.05);
        assert!(in_trim(&pixajoy_spec(), &rect, Side::Left), "sanity: sits right at the trim boundary");
        assert!(
            !in_safe_margin(&pixajoy_spec(), &rect, Side::Left),
            "the converse of 'inside safe margin implies inside trim' must not hold"
        );
    }

    /// Boundary AT the boundary, pinned two-sided, on the OUTER edge of a
    /// LEFT page (mirroring `geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_left_page`).
    #[test]
    fn geometry_in_safe_margin_insets_the_outer_edge_of_a_left_page_by_an_extra_eighth_inch() {
        let x0 = pixajoy_spec().trim_u() + pixajoy_spec().safe_u();
        assert!(!in_safe_margin(&pixajoy_spec(), &Rect::new(x0 - 1e-6, 0.5, 0.05, 0.05), Side::Left));
        assert!(in_safe_margin(&pixajoy_spec(), &Rect::new(x0, 0.5, 0.05, 0.05), Side::Left));
    }

    /// Same boundary, opposite edge, on a RIGHT page -- the asymmetry
    /// `in_trim` itself has between left and right pages must carry through.
    #[test]
    fn geometry_in_safe_margin_insets_the_outer_edge_of_a_right_page_by_an_extra_eighth_inch() {
        let x1 = 1.0 - pixajoy_spec().trim_u() - pixajoy_spec().safe_u();
        assert!(!in_safe_margin(&pixajoy_spec(), &Rect::new(x1 - 0.05 + 1e-6, 0.5, 0.05, 0.05), Side::Right));
        assert!(in_safe_margin(&pixajoy_spec(), &Rect::new(x1 - 0.05, 0.5, 0.05, 0.05), Side::Right));
    }

    /// The fold edge gets NO additional inset -- the gutter strip already
    /// governs content clearance there, and inserting a second inset would
    /// double-count it. A rect flush to the fold that is legal for `in_trim`
    /// (no inset there either) must stay legal for `in_safe_margin` too.
    #[test]
    fn geometry_in_safe_margin_does_not_inset_the_fold_edge() {
        let rect = Rect::new(0.9, pixajoy_spec().trim_v() + pixajoy_spec().safe_v(), 0.1, 0.05);
        assert!(in_trim(&pixajoy_spec(), &rect, Side::Left), "sanity: flush to the fold is legal for trim");
        assert!(
            in_safe_margin(&pixajoy_spec(), &rect, Side::Left),
            "the fold edge must not be inset a second time"
        );
    }

    /// Boundary test AT the boundary, per the plan's anti-pattern rules.
    #[test]
    fn geometry_clear_of_gutter_is_exact_at_the_strip_edge() {
        let strip_start = 1.0 - pixajoy_spec().gutter_u();
        assert!(clear_of_gutter(&pixajoy_spec(), &Rect::new(0.5, 0.4, strip_start - 0.5, 0.2), Side::Left));
        assert!(!clear_of_gutter(&pixajoy_spec(),
            &Rect::new(0.5, 0.4, strip_start - 0.5 + 1e-6, 0.2),
            Side::Left
        ));
    }

    #[test]
    fn geometry_clear_of_gutter_uses_the_opposite_edge_on_a_right_page() {
        assert!(clear_of_gutter(&pixajoy_spec(), &Rect::new(pixajoy_spec().gutter_u(), 0.4, 0.2, 0.2), Side::Right));
        assert!(!clear_of_gutter(&pixajoy_spec(), &Rect::new(pixajoy_spec().gutter_u() - 1e-6, 0.4, 0.2, 0.2), Side::Right));
    }

    #[test]
    fn geometry_bleeds_correctly_requires_reaching_past_the_declared_edge() {
        let flush = Rect::new(0.0, 0.0, 0.5, 1.0);
        assert!(bleeds_correctly(&flush, &[BleedEdge::Left], Side::Left));
        let short = Rect::new(0.01, 0.0, 0.5, 1.0);
        assert!(!bleeds_correctly(&short, &[BleedEdge::Left], Side::Left));
    }

    /// A left page has no bleed at the fold: declaring `right` bleed on a
    /// left page is meaningless and must not be satisfiable.
    #[test]
    fn geometry_bleeds_correctly_rejects_bleed_at_the_fold_edge() {
        let to_fold = Rect::new(0.5, 0.0, 0.5, 1.0);
        assert!(!bleeds_correctly(&to_fold, &[BleedEdge::Right], Side::Left));
    }

    #[test]
    fn geometry_intersect_returns_none_when_disjoint() {
        let a = Rect::new(0.0, 0.0, 0.2, 0.3);
        let b = Rect::new(0.5, 0.6, 0.2, 0.3);
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn geometry_intersect_returns_the_overlap() {
        let a = Rect::new(0.0, 0.0, 0.4, 0.6);
        let b = Rect::new(0.2, 0.3, 0.4, 0.6);
        let i = a.intersect(&b).unwrap();
        assert!((i.x - 0.2).abs() < 1e-9);
        assert!((i.w - 0.2).abs() < 1e-9);
        assert!((i.h - 0.3).abs() < 1e-9);
    }

    /// A zero-width box can "pass" a guard via NaN propagation, so guard
    /// tests use a NEGATIVE width — see the plan's anti-pattern rules.
    #[test]
    fn geometry_intersect_rejects_a_negative_width_rect() {
        let a = Rect::new(0.0, 0.0, -0.4, 0.6);
        let b = Rect::new(0.0, 0.0, 0.4, 0.6);
        assert!(a.intersect(&b).is_none());
    }

    use proptest::prelude::*;

    prop_compose! {
        /// Rects with strictly positive extent, sized so they can sit on one
        /// page. Deliberately excludes squares in normalised space being the
        /// ONLY case: w and h vary independently.
        fn any_page_rect()(
            x in 0.0f64..0.9,
            y in 0.0f64..0.9,
            w in 0.01f64..0.5,
            h in 0.01f64..0.5,
        ) -> Rect {
            Rect::new(x, y, w.min(1.0 - x), h.min(1.0 - y))
        }
    }

    proptest! {
        /// `in_trim` and `clear_of_gutter` are independent predicates: trim
        /// runs all the way to the fold (no inset there), while the gutter
        /// strip is measured inward FROM the fold -- so all four
        /// combinations of the two booleans are reachable on a real page.
        ///
        /// The brief's original version of this property always built a
        /// rect flush to the fold (`right() == 1.0`), which made the
        /// "beyond the gutter threshold" side of its `if`-guard constant
        /// (1.0 is always past `1.0 - pixajoy_spec().gutter_u()`) and its assertion a
        /// restatement of `clear_of_gutter`'s own Left-side branch on that
        /// fixed input -- it would pass for any implementation that agrees
        /// with itself, including a `clear_of_gutter` disconnected from
        /// `in_trim` entirely. This version pins the expected
        /// `(in_trim, clear_of_gutter)` pair for all four reachable
        /// quadrants, so a `clear_of_gutter` that always returns `true` (or
        /// one wired to the wrong edge) fails on quadrant 1 or 3, and an
        /// `in_trim` that always returns `true` fails on quadrant 2 or 3.
        #[test]
        fn geometry_prop_in_trim_and_clear_of_gutter_are_independent(
            quadrant in 0u8..4,
            jx in 0.0f64..0.01,
            jy in 0.0f64..0.01,
        ) {
            let (rect, expect_in_trim, expect_clear) = match quadrant {
                // Comfortably inside both the trim rect and clear of the
                // gutter.
                0 => (Rect::new(0.3 + jx, 0.3 + jy, 0.1, 0.1), true, true),
                // Flush to the fold (legal for trim -- there is no inset
                // there) but inside the gutter strip measured from the
                // fold: in_trim without clear_of_gutter.
                1 => (Rect::new(0.9 - jx, 0.3 + jy, 0.1 + jx, 0.1), true, false),
                // Over the outer-edge bleed (fails trim) but nowhere near
                // the fold: clear_of_gutter without in_trim.
                2 => (Rect::new(0.0, 0.3 + jy, 0.05 + jx, 0.1), false, true),
                // Over the top bleed AND inside the gutter strip: neither
                // predicate holds.
                _ => (Rect::new(0.95 - jx, 0.0, 0.05 + jx, 0.05), false, false),
            };
            prop_assert_eq!(in_trim(&pixajoy_spec(), &rect, Side::Left), expect_in_trim);
            prop_assert_eq!(clear_of_gutter(&pixajoy_spec(), &rect, Side::Left), expect_clear);
        }

        /// Round-tripping through the decomposition preserves width in
        /// inches: a page is exactly half the spread's width, so a rect's
        /// real-world width must be identical before and after.
        #[test]
        fn geometry_prop_decomposition_preserves_real_width(r in any_page_rect()) {
            let on_left = Rect::new(r.x * 0.5, r.y, r.w * 0.5, r.h);
            if let Some((side, page)) = spread_to_page(&on_left) {
                prop_assert_eq!(side, Side::Left);
                let before_in = on_left.w * (2.0 * pixajoy_spec().page_w_in());
                let after_in = page.w * pixajoy_spec().page_w_in();
                prop_assert!((before_in - after_in).abs() < 1e-9,
                    "{} vs {}", before_in, after_in);
            }
        }

        /// `intersect` is commutative and never yields more area than either
        /// input -- the two ways a naive min/max implementation goes wrong.
        #[test]
        fn geometry_prop_intersect_is_commutative_and_bounded(
            a in any_page_rect(), b in any_page_rect()
        ) {
            prop_assert_eq!(a.intersect(&b), b.intersect(&a));
            if let Some(i) = a.intersect(&b) {
                prop_assert!(i.area() <= a.area() + 1e-9);
                prop_assert!(i.area() <= b.area() + 1e-9);
            }
        }
    }
}
