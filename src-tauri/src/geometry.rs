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
}
