//! Page-local geometry for the layout engine.
//!
//! The authoring format is the SPREAD canvas (22.394" x 8.894", normalised
//! [0,1]); the engine's atom is the PAGE (11.197" x 8.894"). Pixajoy's
//! picture boxes are page-local and cannot cross the fold, so a rect that
//! spans the fold has no page representation at all -- `spread_to_page`
//! returns `None` rather than clamping, and the template validator rejects
//! such slots before they ever reach here.

use serde::{Deserialize, Serialize};

pub const SPREAD_W_IN: f64 = 22.394;
pub const SPREAD_H_IN: f64 = 8.894;
pub const PAGE_W_IN: f64 = 11.197;
pub const PAGE_H_IN: f64 = 8.894;

/// 11.197" and 8.894" at 300 DPI. Two pages sum to 6718 px, matching the
/// spread canvas exactly, so no rounding drift accumulates across a book.
pub const PAGE_W_PX: u32 = 3359;
pub const PAGE_H_PX: u32 = 2668;

/// 5 mm, on the three OUTER edges only. There is no bleed at the fold.
pub const BLEED_IN: f64 = 0.197;

/// Page-normalised trim inset on the outer vertical edge, and on top/bottom.
pub const TRIM_U: f64 = BLEED_IN / PAGE_W_IN;
pub const TRIM_V: f64 = BLEED_IN / PAGE_H_IN;

/// Page-normalised width of the gutter dead strip, measured inward from the
/// fold. Numerically equal to `TRIM_U` (both are 0.197" on an 11.197" page)
/// but conceptually unrelated -- one is a guillotine allowance, the other is
/// where the paper curls into the binding. Kept separate so changing one
/// does not silently change the other.
pub const GUTTER_U: f64 = BLEED_IN / PAGE_W_IN;

/// Pixajoy's published guidance: "leave a gap of 1/8th of an inch between
/// anything important that you don't want to cut off and the edge." This is
/// clearance INSIDE the trim line, on top of the trim inset itself -- a face
/// sitting flush against the trim line is still at real risk of being
/// guillotined off in production, trim tolerances being what they are.
pub const SAFE_MARGIN_IN: f64 = 0.125;

/// Page-normalised safe-margin inset, converted exactly as `TRIM_U` is.
pub const SAFE_U: f64 = SAFE_MARGIN_IN / PAGE_W_IN;
/// Page-normalised safe-margin inset, converted exactly as `TRIM_V` is.
pub const SAFE_V: f64 = SAFE_MARGIN_IN / PAGE_H_IN;

/// The fold, normalised on the spread canvas.
const FOLD_X: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

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

    /// Real-world width/height ratio on a canvas of the given inch
    /// dimensions. NEVER compare `aspect_pref` against `w / h` -- the
    /// canvases are not square, so the normalised ratio and the inch ratio
    /// are different numbers (2.518x apart on the spread canvas).
    pub fn aspect_in(&self, canvas_w_in: f64, canvas_h_in: f64) -> f64 {
        (self.w * canvas_w_in) / (self.h * canvas_h_in)
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
    const EPS: f64 = 1e-9;
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
/// The fold edge has no trim inset -- the paper is continuous there.
pub fn in_trim(rect: &Rect, side: Side) -> bool {
    const EPS: f64 = 1e-9;
    let (x0, x1) = match side {
        Side::Left => (TRIM_U, 1.0),
        Side::Right => (0.0, 1.0 - TRIM_U),
    };
    rect.x >= x0 - EPS
        && rect.right() <= x1 + EPS
        && rect.y >= TRIM_V - EPS
        && rect.bottom() <= 1.0 - TRIM_V + EPS
}

/// True when the rect lies wholly inside the SAFE rectangle of its page --
/// `in_trim` inset by a further `SAFE_MARGIN_IN` (Pixajoy's published 1/8")
/// on every edge EXCEPT the fold. There is no bleed and no trim inset at the
/// fold either (the paper is continuous there), and the gutter dead strip
/// already governs content clearance on that edge, so inserting a second
/// inset there would double-count it. `in_safe_margin` is therefore always a
/// SUBSET of `in_trim`: passing this implies passing `in_trim`, but not the
/// reverse.
pub fn in_safe_margin(rect: &Rect, side: Side) -> bool {
    const EPS: f64 = 1e-9;
    let (x0, x1) = match side {
        Side::Left => (TRIM_U + SAFE_U, 1.0),
        Side::Right => (0.0, 1.0 - TRIM_U - SAFE_U),
    };
    rect.x >= x0 - EPS
        && rect.right() <= x1 + EPS
        && rect.y >= TRIM_V + SAFE_V - EPS
        && rect.bottom() <= 1.0 - TRIM_V - SAFE_V + EPS
}

/// True when the rect keeps clear of the gutter dead strip -- the 0.197"
/// nearest the fold, which curls into the binding. This is a CONTENT
/// predicate (faces, salient regions), never a slot rejection: a slot may
/// legitimately run flush to the fold.
pub fn clear_of_gutter(rect: &Rect, side: Side) -> bool {
    const EPS: f64 = 1e-9;
    match side {
        Side::Left => rect.right() <= 1.0 - GUTTER_U + EPS,
        Side::Right => rect.x >= GUTTER_U - EPS,
    }
}

/// True when every declared bleed edge actually reaches past the page
/// boundary. A slot that declares bleed but stops short leaves a white
/// sliver after trimming.
///
/// The fold edge cannot bleed: on a left page that is the RIGHT edge, on a
/// right page the LEFT edge. Declaring it is always an error.
pub fn bleeds_correctly(rect: &Rect, edges: &[BleedEdge], side: Side) -> bool {
    const EPS: f64 = 1e-9;
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
        // On a LEFT page the outer edge is x=0, so trim starts at TRIM_U.
        assert!(!in_trim(&Rect::new(0.0, 0.5, 0.1, 0.1), Side::Left));
        assert!(in_trim(&Rect::new(TRIM_U, 0.5, 0.1, 0.1), Side::Left));
    }

    #[test]
    fn geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_right_page() {
        // On a RIGHT page the outer edge is x=1, so trim ends at 1 - TRIM_U.
        assert!(!in_trim(&Rect::new(0.95, 0.5, 0.1, 0.1), Side::Right));
        assert!(in_trim(&Rect::new(0.8, 0.5, 1.0 - TRIM_U - 0.8, 0.1), Side::Right));
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
        let rect = Rect::new(TRIM_U, 0.5, 0.05, 0.05);
        assert!(in_trim(&rect, Side::Left), "sanity: sits right at the trim boundary");
        assert!(
            !in_safe_margin(&rect, Side::Left),
            "the converse of 'inside safe margin implies inside trim' must not hold"
        );
    }

    /// Boundary AT the boundary, pinned two-sided, on the OUTER edge of a
    /// LEFT page (mirroring `geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_left_page`).
    #[test]
    fn geometry_in_safe_margin_insets_the_outer_edge_of_a_left_page_by_an_extra_eighth_inch() {
        let x0 = TRIM_U + SAFE_U;
        assert!(!in_safe_margin(&Rect::new(x0 - 1e-6, 0.5, 0.05, 0.05), Side::Left));
        assert!(in_safe_margin(&Rect::new(x0, 0.5, 0.05, 0.05), Side::Left));
    }

    /// Same boundary, opposite edge, on a RIGHT page -- the asymmetry
    /// `in_trim` itself has between left and right pages must carry through.
    #[test]
    fn geometry_in_safe_margin_insets_the_outer_edge_of_a_right_page_by_an_extra_eighth_inch() {
        let x1 = 1.0 - TRIM_U - SAFE_U;
        assert!(!in_safe_margin(&Rect::new(x1 - 0.05 + 1e-6, 0.5, 0.05, 0.05), Side::Right));
        assert!(in_safe_margin(&Rect::new(x1 - 0.05, 0.5, 0.05, 0.05), Side::Right));
    }

    /// The fold edge gets NO additional inset -- the gutter strip already
    /// governs content clearance there, and inserting a second inset would
    /// double-count it. A rect flush to the fold that is legal for `in_trim`
    /// (no inset there either) must stay legal for `in_safe_margin` too.
    #[test]
    fn geometry_in_safe_margin_does_not_inset_the_fold_edge() {
        let rect = Rect::new(0.9, TRIM_V + SAFE_V, 0.1, 0.05);
        assert!(in_trim(&rect, Side::Left), "sanity: flush to the fold is legal for trim");
        assert!(
            in_safe_margin(&rect, Side::Left),
            "the fold edge must not be inset a second time"
        );
    }

    /// Boundary test AT the boundary, per the plan's anti-pattern rules.
    #[test]
    fn geometry_clear_of_gutter_is_exact_at_the_strip_edge() {
        let strip_start = 1.0 - GUTTER_U;
        assert!(clear_of_gutter(&Rect::new(0.5, 0.4, strip_start - 0.5, 0.2), Side::Left));
        assert!(!clear_of_gutter(
            &Rect::new(0.5, 0.4, strip_start - 0.5 + 1e-6, 0.2),
            Side::Left
        ));
    }

    #[test]
    fn geometry_clear_of_gutter_uses_the_opposite_edge_on_a_right_page() {
        assert!(clear_of_gutter(&Rect::new(GUTTER_U, 0.4, 0.2, 0.2), Side::Right));
        assert!(!clear_of_gutter(&Rect::new(GUTTER_U - 1e-6, 0.4, 0.2, 0.2), Side::Right));
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

    // --- geometry: aspect

    /// The `aspect_pref` trap, pinned. A slot half the canvas wide and half
    /// tall is 1:1 normalised but 2.518:1 in inches. A fixture where those
    /// two numbers coincide would pass under the bug.
    #[test]
    fn geometry_aspect_in_uses_real_inches_not_the_normalised_ratio() {
        let r = Rect::new(0.0, 0.0, 0.5, 0.5);
        let normalised = r.w / r.h;
        let real = r.aspect_in(SPREAD_W_IN, SPREAD_H_IN);
        assert!((normalised - 1.0).abs() < 1e-9, "sanity: normalised is 1:1");
        assert!((real - 2.518).abs() < 0.001, "real was {real}");
        assert!((real - normalised).abs() > 1.0, "the fixture must distinguish the two");
    }

    #[test]
    fn geometry_page_pixels_sum_to_the_spread_width() {
        assert_eq!(PAGE_W_PX * 2, 6718);
        assert_eq!(PAGE_H_PX, 2668);
    }

    // --- geometry: intersection

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
        /// (1.0 is always past `1.0 - GUTTER_U`) and its assertion a
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
            prop_assert_eq!(in_trim(&rect, Side::Left), expect_in_trim);
            prop_assert_eq!(clear_of_gutter(&rect, Side::Left), expect_clear);
        }

        /// Round-tripping through the decomposition preserves width in
        /// inches: a page is exactly half the spread's width, so a rect's
        /// real-world width must be identical before and after.
        #[test]
        fn geometry_prop_decomposition_preserves_real_width(r in any_page_rect()) {
            let on_left = Rect::new(r.x * 0.5, r.y, r.w * 0.5, r.h);
            if let Some((side, page)) = spread_to_page(&on_left) {
                prop_assert_eq!(side, Side::Left);
                let before_in = on_left.w * SPREAD_W_IN;
                let after_in = page.w * PAGE_W_IN;
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
