//! The user's own layouts: one global library of page and spread layouts,
//! stored as normalised boxes in the app database so one layout fits every
//! print size, and offered to every book on top of the built-in templates.
//!
//! Three parts, kept apart on purpose:
//! - the VALIDATOR (`structural`, `print_rules`, `check`): what a layout must
//!   satisfy. Structural rules hold at any size and are enforced when a
//!   layout is saved. Print rules depend on the book's `PrintSpec`, so they
//!   are evaluated per book and a failure refuses the layout FOR THAT BOOK,
//!   with a reason, rather than at save time.
//! - the CONVERSION (`to_template`): a layout as the engine's `SpreadTemplate`
//!   or `PageLayout`, so it is scored exactly like a built-in.
//! - the STORE (`list`, `create`, `update`, ...): one row per layout.
//!
//! Whether a layout takes part in automatic layout is the `auto` flag, off
//! by default. `Library` (templates.rs) is where that flag is read.

use crate::book::edit::{bleed_edges, MIN_SLOT, OVERLAP_EPS};
use crate::db::Db;
use crate::geometry::{spread_to_page, BleedEdge, Rect, Side};
use crate::print_spec::PrintSpec;
use crate::templates::{
    edge_treatment, CustomTemplate, Density, Energy, PageLayout, Role, Slot, SpreadTemplate,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

/// Every user layout id starts with this, and `Library::load` refuses a
/// built-in id that does, so the two id spaces cannot collide. No `:` ever
/// follows it: a colon names a built-in page half (`<id>:left`).
pub const ID_PREFIX: &str = "user-";
/// `best_spread` tries every photo order, n! of them, so a layout's box
/// count is capped where that stays instant.
pub const MAX_BOXES: usize = 8;
pub const MAX_NAME_CHARS: usize = 60;
/// Float slack for boundary comparisons. Far below anything a person can
/// draw, far above rounding.
const EPS: f64 = 1e-6;
const FOLD_X: f64 = 0.5;

/// One photo box. `rect` is normalised on the layout's own canvas: the
/// spread for a spread layout, the page for a page layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutBox {
    pub rect: Rect,
    pub role: Role,
}

/// What a layout covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LayoutShape {
    /// Both pages of an opening, boxes on the spread canvas (fold at x=0.5).
    Spread { boxes: Vec<LayoutBox> },
    /// One page, boxes on the page canvas. `side` is the page it prints on:
    /// page 1 is a right page, the last page a left one.
    Page { side: Side, boxes: Vec<LayoutBox> },
}

impl LayoutShape {
    pub fn boxes(&self) -> &[LayoutBox] {
        match self {
            LayoutShape::Spread { boxes } | LayoutShape::Page { boxes, .. } => boxes,
        }
    }

    /// `None` for a spread.
    pub fn side(&self) -> Option<Side> {
        match self {
            LayoutShape::Spread { .. } => None,
            LayoutShape::Page { side, .. } => Some(*side),
        }
    }
}

/// One stored layout.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomLayout {
    pub id: String,
    pub name: String,
    pub shape: LayoutShape,
    /// "Use in auto layout". Off: only the layout menu and `SetTemplate`
    /// reach it. On: generate, Regenerate, Shuffle and AddPhoto may pick it.
    pub auto: bool,
    /// Unix seconds.
    pub created_at: i64,
    pub updated_at: i64,
}

/// Why a layout cannot be used. `slot` is a 0-based box index; messages
/// count from 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LayoutProblem {
    // --- structural: refused at save, whatever the print size ---
    NoBoxes,
    TooManyBoxes { count: usize, max: usize },
    NotFinite { slot: usize },
    /// Some part of the box is off the canvas.
    OffCanvas { slot: usize },
    /// Narrower or shorter than `MIN_SLOT` of a page.
    TooSmall { slot: usize },
    /// A spread box that spans the fold. Pixajoy boxes are page-local.
    CrossesFold { slot: usize },
    /// Two boxes on one page overlap by more than `OVERLAP_EPS` each way.
    Overlap { slot: usize, other: usize },
    // --- print rules: refused for one book's print size ---
    /// An outer edge stops inside the bleed, between the paper edge and the
    /// trim line, so the trim would shave an uneven sliver off it. Reaching
    /// the paper edge (full bleed) or stopping at or inside the trim is fine.
    EdgeInBleed { slot: usize, edge: BleedEdge, bleed_mm: f64 },
    /// The fold-side edge stops inside the gutter strip, where the binding
    /// swallows it. Running to the fold or stopping clear of the strip is fine.
    EdgeInGutter { slot: usize, gutter_mm: f64 },
    /// Less than `MIN_SLOT` of the box is left once trim and gutter are taken.
    MostlyHidden { slot: usize },
}

impl std::fmt::Display for LayoutProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pct = (MIN_SLOT * 100.0).round();
        match *self {
            Self::NoBoxes => write!(f, "A layout needs at least one photo box."),
            Self::TooManyBoxes { count, max } => {
                write!(f, "A layout can hold at most {max} photo boxes; this one has {count}.")
            }
            Self::NotFinite { slot } => {
                write!(f, "Box {} has a position or size that is not a number.", slot + 1)
            }
            Self::OffCanvas { slot } => write!(f, "Box {} runs off the page.", slot + 1),
            Self::TooSmall { slot } => write!(
                f,
                "Box {} is too small to print. Make it at least {pct}% of the page each way.",
                slot + 1
            ),
            Self::CrossesFold { slot } => write!(
                f,
                "Box {} crosses the fold. A box must stay on one page.",
                slot + 1
            ),
            Self::Overlap { slot, other } => {
                write!(f, "Box {} overlaps box {}.", slot + 1, other + 1)
            }
            Self::EdgeInBleed { slot, edge, bleed_mm } => write!(
                f,
                "Box {}'s {} edge stops inside the {bleed_mm} mm bleed, so the trim would cut it \
                 unevenly. Take it to the paper edge or pull it inside the trim line.",
                slot + 1,
                edge_name(edge)
            ),
            Self::EdgeInGutter { slot, gutter_mm } => write!(
                f,
                "Box {}'s edge by the fold stops inside the {gutter_mm} mm gutter, where the \
                 binding hides it. Take it to the fold or clear of the gutter.",
                slot + 1
            ),
            Self::MostlyHidden { slot } => write!(
                f,
                "Box {} is almost all in the trim or the gutter, so too little of it would print.",
                slot + 1
            ),
        }
    }
}

fn edge_name(edge: BleedEdge) -> &'static str {
    match edge {
        BleedEdge::Left => "left",
        BleedEdge::Right => "right",
        BleedEdge::Top => "top",
        BleedEdge::Bottom => "bottom",
    }
}

/// A problem as the webview receives it: the structured reason and its
/// readable message side by side, so the UI never re-words a rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    #[serde(flatten)]
    pub problem: LayoutProblem,
    pub message: String,
}

impl From<LayoutProblem> for Problem {
    fn from(problem: LayoutProblem) -> Self {
        Problem { message: problem.to_string(), problem }
    }
}

/// One box on its page, page-normalised.
struct Placed {
    slot: usize,
    side: Side,
    rect: Rect,
}

/// Every box on its page, or `None` for one that crosses the fold. Only
/// meaningful for boxes that already passed the finite check.
fn place_boxes(shape: &LayoutShape) -> Vec<Option<Placed>> {
    match shape {
        LayoutShape::Spread { boxes } => boxes
            .iter()
            .enumerate()
            .map(|(slot, b)| spread_to_page(&b.rect).map(|(side, rect)| Placed { slot, side, rect }))
            .collect(),
        LayoutShape::Page { side, boxes } => boxes
            .iter()
            .enumerate()
            .map(|(slot, b)| Some(Placed { slot, side: *side, rect: b.rect }))
            .collect(),
    }
}

fn finite(r: &Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite())
}

fn on_canvas(r: &Rect) -> bool {
    r.x >= -EPS && r.y >= -EPS && r.right() <= 1.0 + EPS && r.bottom() <= 1.0 + EPS
}

/// The rules that hold at every print size. A layout failing one is never
/// stored.
pub fn structural(shape: &LayoutShape) -> Vec<LayoutProblem> {
    let boxes = shape.boxes();
    let mut out = Vec::new();
    if boxes.is_empty() {
        out.push(LayoutProblem::NoBoxes);
    }
    if boxes.len() > MAX_BOXES {
        out.push(LayoutProblem::TooManyBoxes { count: boxes.len(), max: MAX_BOXES });
    }
    let mut bad = vec![false; boxes.len()];
    for (slot, b) in boxes.iter().enumerate() {
        if !finite(&b.rect) {
            out.push(LayoutProblem::NotFinite { slot });
            bad[slot] = true;
        } else if !on_canvas(&b.rect) {
            out.push(LayoutProblem::OffCanvas { slot });
            bad[slot] = true;
        }
    }
    let placed = place_boxes(shape);
    for (slot, p) in placed.iter().enumerate() {
        if bad[slot] {
            continue;
        }
        match p {
            None => {
                out.push(LayoutProblem::CrossesFold { slot });
                bad[slot] = true;
            }
            Some(p) if p.rect.w < MIN_SLOT - EPS || p.rect.h < MIN_SLOT - EPS => {
                out.push(LayoutProblem::TooSmall { slot });
            }
            Some(_) => {}
        }
    }
    let good: Vec<&Placed> =
        placed.iter().flatten().filter(|p| !bad[p.slot]).collect();
    for (i, a) in good.iter().enumerate() {
        for b in &good[i + 1..] {
            if a.side != b.side {
                continue;
            }
            let overlaps = a
                .rect
                .intersect(&b.rect)
                .is_some_and(|r| r.w > OVERLAP_EPS + EPS && r.h > OVERLAP_EPS + EPS);
            if overlaps {
                out.push(LayoutProblem::Overlap { slot: a.slot, other: b.slot });
            }
        }
    }
    out
}

fn mm(inches: f64) -> f64 {
    (inches * 25.4 * 10.0).round() / 10.0
}

/// True when `d`, an edge's distance in from the paper edge (or the fold),
/// lands strictly inside a band `z` wide: not flush, not at or past its far
/// side.
fn inside_band(d: f64, z: f64) -> bool {
    d > EPS && d < z - EPS
}

/// The rules that depend on the book's print size. Boxes that fail a
/// structural rule are skipped: the structural problem is the one to fix.
pub fn print_rules(shape: &LayoutShape, spec: &PrintSpec) -> Vec<LayoutProblem> {
    let boxes = shape.boxes();
    let (tu, tv, g) = (spec.trim_u(), spec.trim_v(), spec.gutter_u());
    let bleed_mm = mm(spec.bleed_in());
    let gutter_mm = mm(spec.gutter_in());
    let mut out = Vec::new();
    for p in place_boxes(shape).into_iter().flatten() {
        if !finite(&boxes[p.slot].rect) {
            continue;
        }
        let r = p.rect;
        let slot = p.slot;
        // Distance in from the outer paper edge and from the fold.
        let (outer, outer_edge, fold) = match p.side {
            Side::Left => (r.x, BleedEdge::Left, 1.0 - r.right()),
            Side::Right => (1.0 - r.right(), BleedEdge::Right, r.x),
        };
        for (d, edge) in [(outer, outer_edge), (r.y, BleedEdge::Top), (1.0 - r.bottom(), BleedEdge::Bottom)] {
            let z = if edge == outer_edge { tu } else { tv };
            if inside_band(d, z) {
                out.push(LayoutProblem::EdgeInBleed { slot, edge, bleed_mm });
            }
        }
        if inside_band(fold, g) {
            out.push(LayoutProblem::EdgeInGutter { slot, gutter_mm });
        }
        // The printable band across: inside the trim, clear of the gutter.
        let (lo, hi) = match p.side {
            Side::Left => (tu, 1.0 - g),
            Side::Right => (g, 1.0 - tu),
        };
        let visible_w = r.right().min(hi) - r.x.max(lo);
        let visible_h = r.bottom().min(1.0 - tv) - r.y.max(tv);
        if visible_w < MIN_SLOT - EPS || visible_h < MIN_SLOT - EPS {
            out.push(LayoutProblem::MostlyHidden { slot });
        }
    }
    out
}

/// Every problem with `shape` at `spec`, structural first. Empty means the
/// layout prints in a book at this size.
pub fn check(shape: &LayoutShape, spec: &PrintSpec) -> Vec<LayoutProblem> {
    let mut out = structural(shape);
    out.extend(print_rules(shape, spec));
    out
}

/// Each page's box rects, page-normalised: one list for a page layout, left
/// then right for a spread. A box across the fold is left out.
pub fn sketch(shape: &LayoutShape) -> Vec<Vec<Rect>> {
    let placed: Vec<Placed> = place_boxes(shape).into_iter().flatten().collect();
    let on = |side: Side| placed.iter().filter(|p| p.side == side).map(|p| p.rect).collect();
    match shape {
        LayoutShape::Spread { .. } => vec![on(Side::Left), on(Side::Right)],
        LayoutShape::Page { side, .. } => vec![on(*side)],
    }
}

/// A page's slots, bled where they reach the canvas and with `aspect_pref`
/// filled from the box's own real-world shape at `spec`. `aspect_pref` is
/// validator-only -- the scorer never reads it -- so this is bookkeeping,
/// not a preference.
fn page_layout(side: Side, rects: Vec<(Rect, Role)>, spec: &PrintSpec) -> PageLayout {
    let slots: Vec<Slot> = rects
        .into_iter()
        .map(|(rect, role)| {
            let a = spec.page_aspect(&rect);
            Slot { rect, role, bleed: bleed_edges(&rect, side), aspect_pref: (a, a) }
        })
        .collect();
    PageLayout { side, edge_treatment: edge_treatment(&slots), slots }
}

fn density(boxes: usize) -> Density {
    match boxes {
        0 | 1 => Density::Sparse,
        2..=4 => Density::Medium,
        _ => Density::Dense,
    }
}

/// `layout` as the engine's types at `spec`. Call only on a layout that
/// passed `structural`: a fold-crossing box is dropped here rather than
/// placed.
pub fn to_template(layout: &CustomLayout, spec: &PrintSpec) -> CustomTemplate {
    let boxes = layout.shape.boxes();
    let mut left = Vec::new();
    let mut right = Vec::new();
    for p in place_boxes(&layout.shape).into_iter().flatten() {
        let entry = (p.rect, boxes[p.slot].role);
        match p.side {
            Side::Left => left.push(entry),
            Side::Right => right.push(entry),
        }
    }
    match layout.shape {
        LayoutShape::Spread { .. } => CustomTemplate::Spread(SpreadTemplate {
            id: layout.id.clone(),
            left: page_layout(Side::Left, left, spec),
            right: page_layout(Side::Right, right, spec),
            density: density(boxes.len()),
            energy: Energy::Neutral,
        }),
        LayoutShape::Page { side, .. } => CustomTemplate::Page {
            id: layout.id.clone(),
            layout: page_layout(side, if side == Side::Left { left } else { right }, spec),
        },
    }
}

/// The spread-normalised x of a page-normalised rect, for `shape_of`.
fn to_spread(side: Side, r: &Rect) -> Rect {
    let origin = if side == Side::Left { 0.0 } else { FOLD_X };
    Rect::new(origin + r.x * FOLD_X, r.y, r.w * FOLD_X, r.h)
}

/// A built-in template as a user layout's shape: the starting point when a
/// user copies a built-in to edit, and how the validator is proven against
/// the authored library.
pub fn shape_of(t: &SpreadTemplate) -> LayoutShape {
    let boxes = [&t.left, &t.right]
        .into_iter()
        .flat_map(|page| {
            page.slots.iter().map(|s| LayoutBox { rect: to_spread(page.side, &s.rect), role: s.role })
        })
        .collect();
    LayoutShape::Spread { boxes }
}

/// A built-in template as the Layouts screen lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinLayout {
    pub id: String,
    pub shape: LayoutShape,
}

/// Every built-in template in library order, as a shape a user can copy.
pub fn builtins(lib: &crate::templates::Library) -> Vec<BuiltinLayout> {
    lib.spreads.iter().map(|t| BuiltinLayout { id: t.id.clone(), shape: shape_of(t) }).collect()
}

// --- the store ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CustomLayoutError {
    NotFound { id: String },
    EmptyName,
    NameTooLong { max: usize },
    /// The shape fails a structural rule. Print rules never refuse a save:
    /// they depend on the book.
    Invalid { problems: Vec<Problem> },
    Storage { message: String },
}

impl std::fmt::Display for CustomLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { .. } => write!(f, "That layout no longer exists."),
            Self::EmptyName => write!(f, "A layout needs a name."),
            Self::NameTooLong { max } => write!(f, "A layout name can be at most {max} characters."),
            Self::Invalid { problems } => {
                let messages: Vec<&str> = problems.iter().map(|p| p.message.as_str()).collect();
                write!(f, "{}", messages.join(" "))
            }
            Self::Storage { message } => write!(f, "Could not save layouts: {message}"),
        }
    }
}

impl From<rusqlite::Error> for CustomLayoutError {
    fn from(e: rusqlite::Error) -> Self {
        CustomLayoutError::Storage { message: e.to_string() }
    }
}

impl From<serde_json::Error> for CustomLayoutError {
    fn from(e: serde_json::Error) -> Self {
        CustomLayoutError::Storage { message: e.to_string() }
    }
}

fn valid_name(name: &str) -> Result<String, CustomLayoutError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CustomLayoutError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(CustomLayoutError::NameTooLong { max: MAX_NAME_CHARS });
    }
    Ok(name.to_string())
}

fn valid_shape(shape: &LayoutShape) -> Result<String, CustomLayoutError> {
    let problems = structural(shape);
    if !problems.is_empty() {
        return Err(CustomLayoutError::Invalid {
            problems: problems.into_iter().map(Problem::from).collect(),
        });
    }
    Ok(serde_json::to_string(shape)?)
}

/// `ID_PREFIX` and 32 random hex digits. Random rather than a counter so a
/// deleted layout's id is never handed to a new one: a book that still
/// names the old id must not silently change layout.
fn new_id() -> Result<String, CustomLayoutError> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| CustomLayoutError::Storage { message: e.to_string() })?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{ID_PREFIX}{hex}"))
}

/// Builds a `CustomLayout` from one row, or `None` for a row whose shape no
/// longer parses -- skipped rather than failing the whole library.
fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<CustomLayout>> {
    let shape_json: String = row.get(2)?;
    let Ok(shape) = serde_json::from_str::<LayoutShape>(&shape_json) else {
        return Ok(None);
    };
    Ok(Some(CustomLayout {
        id: row.get(0)?,
        name: row.get(1)?,
        shape,
        auto: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    }))
}

const COLUMNS: &str = "id, name, shape_json, auto, created_at, updated_at";

/// Every layout, oldest first.
pub fn list(db: &Db) -> Result<Vec<CustomLayout>, CustomLayoutError> {
    let mut stmt =
        db.conn.prepare(&format!("SELECT {COLUMNS} FROM custom_layouts ORDER BY rowid"))?;
    let rows = stmt.query_map([], from_row)?;
    let mut out = Vec::new();
    for row in rows {
        if let Some(layout) = row? {
            out.push(layout);
        }
    }
    Ok(out)
}

pub fn get(db: &Db, id: &str) -> Result<CustomLayout, CustomLayoutError> {
    db.conn
        .query_row(&format!("SELECT {COLUMNS} FROM custom_layouts WHERE id = ?1"), [id], from_row)
        .optional()?
        .flatten()
        .ok_or_else(|| CustomLayoutError::NotFound { id: id.to_string() })
}

fn insert(db: &Db, name: &str, shape_json: &str, auto: bool) -> Result<CustomLayout, CustomLayoutError> {
    let id = new_id()?;
    db.conn.execute(
        "INSERT INTO custom_layouts (id, name, shape_json, auto) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, name, shape_json, auto as i64],
    )?;
    get(db, &id)
}

/// A new layout, auto off.
pub fn create(db: &Db, name: &str, shape: &LayoutShape) -> Result<CustomLayout, CustomLayoutError> {
    let name = valid_name(name)?;
    let json = valid_shape(shape)?;
    insert(db, &name, &json, false)
}

fn touch(db: &Db, id: &str, sql: &str, value: &dyn rusqlite::ToSql) -> Result<CustomLayout, CustomLayoutError> {
    let changed = db.conn.execute(sql, rusqlite::params![value, id])?;
    if changed == 0 {
        return Err(CustomLayoutError::NotFound { id: id.to_string() });
    }
    get(db, id)
}

/// Replaces a layout's boxes. Books already laid out on it keep their rects
/// until the opening is laid out again.
pub fn update(db: &Db, id: &str, shape: &LayoutShape) -> Result<CustomLayout, CustomLayoutError> {
    let json = valid_shape(shape)?;
    touch(
        db,
        id,
        "UPDATE custom_layouts SET shape_json = ?1, updated_at = unixepoch() WHERE id = ?2",
        &json,
    )
}

pub fn rename(db: &Db, id: &str, name: &str) -> Result<CustomLayout, CustomLayoutError> {
    let name = valid_name(name)?;
    touch(db, id, "UPDATE custom_layouts SET name = ?1, updated_at = unixepoch() WHERE id = ?2", &name)
}

pub fn set_auto(db: &Db, id: &str, auto: bool) -> Result<CustomLayout, CustomLayoutError> {
    touch(
        db,
        id,
        "UPDATE custom_layouts SET auto = ?1, updated_at = unixepoch() WHERE id = ?2",
        &(auto as i64),
    )
}

/// A copy under a new id, named "<name> copy", auto off: a copy is a draft
/// to edit, and auto layout should not start using it unasked.
pub fn duplicate(db: &Db, id: &str) -> Result<CustomLayout, CustomLayoutError> {
    let original = get(db, id)?;
    let mut name: String = original.name.chars().take(MAX_NAME_CHARS - " copy".len()).collect();
    name.push_str(" copy");
    insert(db, &name, &serde_json::to_string(&original.shape)?, false)
}

/// Removes a layout. Books that use it are NOT rewritten: their pages keep
/// the rects they were laid out with, and the id simply stops resolving --
/// see the deletion tests.
pub fn delete(db: &Db, id: &str) -> Result<(), CustomLayoutError> {
    let changed = db.conn.execute("DELETE FROM custom_layouts WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(CustomLayoutError::NotFound { id: id.to_string() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;

    fn b(x: f64, y: f64, w: f64, h: f64) -> LayoutBox {
        LayoutBox { rect: Rect::new(x, y, w, h), role: Role::Hero }
    }

    fn page(side: Side, boxes: Vec<LayoutBox>) -> LayoutShape {
        LayoutShape::Page { side, boxes }
    }

    fn spread(boxes: Vec<LayoutBox>) -> LayoutShape {
        LayoutShape::Spread { boxes }
    }

    fn kinds(problems: &[LayoutProblem]) -> Vec<LayoutProblem> {
        problems.to_vec()
    }

    /// A box comfortably inside the safe area of either page.
    fn safe_box() -> LayoutBox {
        b(0.2, 0.2, 0.5, 0.5)
    }

    // --- structural ---------------------------------------------------------

    #[test]
    fn custom_layouts_refuses_a_layout_with_no_boxes() {
        assert_eq!(structural(&spread(vec![])), vec![LayoutProblem::NoBoxes]);
        assert_eq!(structural(&page(Side::Right, vec![safe_box()])), vec![]);
    }

    #[test]
    fn custom_layouts_caps_the_box_count_at_eight() {
        // A 4 x 2 grid per page would be 16; eight narrow columns fit one page.
        let column = |i: usize| b(0.02 + 0.12 * i as f64, 0.2, 0.1, 0.5);
        let eight: Vec<LayoutBox> = (0..8).map(column).collect();
        assert_eq!(structural(&page(Side::Right, eight.clone())), vec![], "eight is the boundary");
        let mut nine = eight;
        nine.push(b(0.2, 0.75, 0.1, 0.1));
        assert_eq!(
            structural(&page(Side::Right, nine)),
            vec![LayoutProblem::TooManyBoxes { count: 9, max: 8 }]
        );
    }

    #[test]
    fn custom_layouts_refuses_a_box_that_is_not_a_number() {
        assert_eq!(
            structural(&page(Side::Right, vec![safe_box(), b(f64::NAN, 0.1, 0.2, 0.2)])),
            vec![LayoutProblem::NotFinite { slot: 1 }]
        );
        assert_eq!(
            structural(&page(Side::Right, vec![b(0.1, 0.1, f64::INFINITY, 0.2)])),
            vec![LayoutProblem::NotFinite { slot: 0 }]
        );
    }

    #[test]
    fn custom_layouts_refuses_a_box_off_the_canvas_and_accepts_one_filling_it() {
        assert_eq!(structural(&page(Side::Left, vec![b(0.0, 0.0, 1.0, 1.0)])), vec![]);
        assert_eq!(structural(&spread(vec![b(0.5, 0.0, 0.5, 1.0)])), vec![]);
        assert_eq!(
            structural(&page(Side::Left, vec![b(-0.01, 0.0, 0.5, 0.5)])),
            vec![LayoutProblem::OffCanvas { slot: 0 }]
        );
        assert_eq!(
            structural(&page(Side::Left, vec![b(0.2, 0.6, 0.5, 0.41)])),
            vec![LayoutProblem::OffCanvas { slot: 0 }]
        );
    }

    #[test]
    fn custom_layouts_refuses_a_box_below_five_percent_of_the_page() {
        assert_eq!(structural(&page(Side::Right, vec![b(0.2, 0.2, 0.05, 0.05)])), vec![]);
        assert_eq!(
            structural(&page(Side::Right, vec![b(0.2, 0.2, 0.0499, 0.3)])),
            vec![LayoutProblem::TooSmall { slot: 0 }]
        );
        assert_eq!(
            structural(&page(Side::Right, vec![b(0.2, 0.2, 0.3, 0.0499)])),
            vec![LayoutProblem::TooSmall { slot: 0 }]
        );
        // Measured on the PAGE: 0.025 of the spread is 0.05 of a page.
        assert_eq!(structural(&spread(vec![b(0.6, 0.2, 0.025, 0.3)])), vec![]);
        assert_eq!(
            structural(&spread(vec![b(0.6, 0.2, 0.0249, 0.3)])),
            vec![LayoutProblem::TooSmall { slot: 0 }]
        );
    }

    #[test]
    fn custom_layouts_refuses_a_box_across_the_fold_and_accepts_one_ending_on_it() {
        assert_eq!(structural(&spread(vec![b(0.1, 0.1, 0.4, 0.5)])), vec![], "ends exactly at the fold");
        assert_eq!(structural(&spread(vec![b(0.5, 0.1, 0.4, 0.5)])), vec![], "starts exactly at it");
        assert_eq!(
            structural(&spread(vec![b(0.1, 0.2, 0.3, 0.5), b(0.4, 0.1, 0.2, 0.5)])),
            vec![LayoutProblem::CrossesFold { slot: 1 }]
        );
    }

    #[test]
    fn custom_layouts_refuses_overlap_beyond_the_tolerance_on_one_page_only() {
        let a = b(0.1, 0.1, 0.4, 0.4);
        // Overlapping exactly OVERLAP_EPS each way is authored rounding.
        let edge = 0.5 - OVERLAP_EPS;
        let touching = b(edge, edge, 0.3, 0.3);
        assert_eq!(structural(&page(Side::Left, vec![a, touching])), vec![]);
        let into = b(0.49, 0.49, 0.3, 0.3);
        assert_eq!(
            structural(&page(Side::Left, vec![a, into])),
            vec![LayoutProblem::Overlap { slot: 0, other: 1 }]
        );
        // The same page-local rects on opposite pages of a spread never meet.
        assert_eq!(
            structural(&spread(vec![b(0.1, 0.1, 0.3, 0.4), b(0.6, 0.1, 0.3, 0.4)])),
            vec![]
        );
    }

    // --- print rules, at the Pixajoy size -----------------------------------

    #[test]
    fn custom_layouts_refuses_an_outer_edge_inside_the_bleed() {
        let spec = pixajoy_spec();
        let t = spec.trim_u();
        let tv = spec.trim_v();
        let bleed_mm = 5.0;
        // Right page: the outer edge is the right one.
        let at_trim = b(0.3, 0.3, 0.7 - t, 0.4);
        assert_eq!(print_rules(&page(Side::Right, vec![at_trim]), &spec), vec![], "at the trim line");
        let full = b(0.3, 0.3, 0.7, 0.4);
        assert_eq!(print_rules(&page(Side::Right, vec![full]), &spec), vec![], "full bleed");
        let inside = b(0.3, 0.3, 0.7 - t / 2.0, 0.4);
        assert_eq!(
            print_rules(&page(Side::Right, vec![inside]), &spec),
            vec![LayoutProblem::EdgeInBleed { slot: 0, edge: BleedEdge::Right, bleed_mm }]
        );
        // Left page: the outer edge is the left one.
        assert_eq!(print_rules(&page(Side::Left, vec![b(t, 0.3, 0.4, 0.4)]), &spec), vec![]);
        assert_eq!(
            print_rules(&page(Side::Left, vec![b(t / 2.0, 0.3, 0.4, 0.4)]), &spec),
            vec![LayoutProblem::EdgeInBleed { slot: 0, edge: BleedEdge::Left, bleed_mm }]
        );
        // Top and bottom, on the vertical trim inset.
        assert_eq!(print_rules(&page(Side::Left, vec![b(0.3, tv, 0.4, 1.0 - 2.0 * tv)]), &spec), vec![]);
        assert_eq!(
            print_rules(&page(Side::Left, vec![b(0.3, tv / 2.0, 0.4, 0.4)]), &spec),
            vec![LayoutProblem::EdgeInBleed { slot: 0, edge: BleedEdge::Top, bleed_mm }]
        );
        assert_eq!(
            print_rules(&page(Side::Left, vec![b(0.3, 0.3, 0.4, 0.7 - tv / 2.0)]), &spec),
            vec![LayoutProblem::EdgeInBleed { slot: 0, edge: BleedEdge::Bottom, bleed_mm }]
        );
    }

    #[test]
    fn custom_layouts_refuses_a_fold_edge_inside_the_gutter() {
        let spec = pixajoy_spec();
        let g = spec.gutter_u();
        let gutter_mm = 9.0;
        // Left page: the fold is its right edge.
        assert_eq!(print_rules(&page(Side::Left, vec![b(0.3, 0.3, 0.7 - g, 0.4)]), &spec), vec![], "clear of it");
        assert_eq!(print_rules(&page(Side::Left, vec![b(0.3, 0.3, 0.7, 0.4)]), &spec), vec![], "to the fold");
        assert_eq!(
            print_rules(&page(Side::Left, vec![b(0.3, 0.3, 0.7 - g / 2.0, 0.4)]), &spec),
            vec![LayoutProblem::EdgeInGutter { slot: 0, gutter_mm }]
        );
        // Right page: the fold is its left edge.
        assert_eq!(print_rules(&page(Side::Right, vec![b(g, 0.3, 0.4, 0.4)]), &spec), vec![]);
        assert_eq!(
            print_rules(&page(Side::Right, vec![b(g / 2.0, 0.3, 0.4, 0.4)]), &spec),
            vec![LayoutProblem::EdgeInGutter { slot: 0, gutter_mm }]
        );
        // The same rule on a spread, where the fold is x = 0.5.
        assert_eq!(
            print_rules(&spread(vec![b(0.2, 0.3, 0.3 - g / 4.0, 0.4)]), &spec),
            vec![LayoutProblem::EdgeInGutter { slot: 0, gutter_mm }]
        );
    }

    #[test]
    fn custom_layouts_refuses_a_box_mostly_lost_to_the_gutter() {
        let spec = pixajoy_spec();
        let g = spec.gutter_u();
        // Flush to the fold, so the gutter rule is satisfied, and exactly
        // MIN_SLOT of it is left clear of the gutter.
        assert_eq!(print_rules(&page(Side::Right, vec![b(0.0, 0.3, g + MIN_SLOT, 0.4)]), &spec), vec![]);
        assert_eq!(
            print_rules(&page(Side::Right, vec![b(0.0, 0.3, g + 0.049, 0.4)]), &spec),
            vec![LayoutProblem::MostlyHidden { slot: 0 }]
        );
        // And vertically, into the trim at the top.
        let tv = spec.trim_v();
        assert_eq!(print_rules(&page(Side::Right, vec![b(0.3, 0.0, 0.3, tv + MIN_SLOT)]), &spec), vec![]);
        assert_eq!(
            print_rules(&page(Side::Right, vec![b(0.3, 0.0, 0.3, tv + 0.049)]), &spec),
            vec![LayoutProblem::MostlyHidden { slot: 0 }]
        );
    }

    /// A rule that only exists at one print size is per book: the same
    /// layout passes at Pixajoy and fails where the bleed is wider.
    #[test]
    fn custom_layouts_print_rules_follow_the_book_spec() {
        let pixajoy = pixajoy_spec();
        let wider = crate::print_spec::odd_spec();
        assert!(wider.trim_u() > pixajoy.trim_u(), "fixture: odd_spec must bleed more");
        let shape = page(Side::Right, vec![b(0.3, 0.3, 0.7 - pixajoy.trim_u(), 0.4)]);
        assert_eq!(check(&shape, &pixajoy), vec![]);
        assert!(
            matches!(check(&shape, &wider)[..], [LayoutProblem::EdgeInBleed { edge: BleedEdge::Right, .. }]),
            "{:?}",
            check(&shape, &wider)
        );
    }

    /// The authored library is the reference: every built-in spread, read
    /// back as a user layout, must pass. A rule that refuses one of them is
    /// stricter than the templates Pixajoy already prints.
    #[test]
    fn custom_layouts_accepts_every_built_in_template() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = crate::templates::Library::load(&dir).unwrap();
        assert!(lib.spreads.len() >= 19);
        let spec = pixajoy_spec();
        for t in &lib.spreads {
            assert_eq!(kinds(&check(&shape_of(t), &spec)), vec![], "{}", t.id);
        }
    }

    #[test]
    fn custom_layouts_problems_carry_a_readable_message() {
        let p = Problem::from(LayoutProblem::Overlap { slot: 0, other: 2 });
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({"kind": "overlap", "slot": 0, "other": 2, "message": "Box 1 overlaps box 3."})
        );
        let bleed = LayoutProblem::EdgeInBleed { slot: 1, edge: BleedEdge::Top, bleed_mm: 5.0 };
        assert!(bleed.to_string().starts_with("Box 2's top edge stops inside the 5 mm bleed"), "{bleed}");
    }

    // --- the store ---

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn two_up() -> LayoutShape {
        spread(vec![b(0.0, 0.0, 0.5, 1.0), b(0.5, 0.0, 0.5, 1.0)])
    }

    #[test]
    fn custom_layouts_store_creates_with_auto_off_and_lists_oldest_first() {
        let db = db();
        let a = create(&db, "  Beach pair ", &two_up()).unwrap();
        let c = create(&db, "Cover", &page(Side::Right, vec![b(0.0, 0.0, 1.0, 1.0)])).unwrap();
        assert_eq!(a.name, "Beach pair", "names are trimmed");
        assert!(!a.auto && !c.auto, "auto is off until the user turns it on");
        assert_eq!(a.shape, two_up());
        assert_eq!(list(&db).unwrap(), vec![a.clone(), c.clone()]);
        assert_eq!(get(&db, &a.id).unwrap(), a);
    }

    #[test]
    fn custom_layouts_store_ids_are_prefixed_unique_and_never_a_half() {
        let db = db();
        let mut ids: Vec<String> =
            (0..50).map(|_| create(&db, "x", &two_up()).unwrap().id).collect();
        for id in &ids {
            assert!(id.starts_with(ID_PREFIX), "{id}");
            assert_eq!(id.len(), ID_PREFIX.len() + 32, "{id}");
            assert!(!id.contains(':'), "a ':' id would parse as a page half: {id}");
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 50);
        let built_in = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = crate::templates::Library::load(&built_in).unwrap();
        assert!(lib.spreads.iter().all(|t| !t.id.starts_with(ID_PREFIX)));
    }

    /// The Layouts screen lists the shipped library through `builtins`, so
    /// it must be every built-in, in library order, each one's boxes being
    /// `shape_of` that template: a skipped or reshaped entry would show the
    /// user a built-in that does not print as drawn.
    #[test]
    fn custom_layouts_builtins_are_every_shipped_template_as_its_shape() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = crate::templates::Library::load(&dir).unwrap();
        let listed = builtins(&lib);
        assert!(listed.len() > 5, "the shipped library has more than a handful");
        assert_eq!(
            listed.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            lib.spreads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>()
        );
        for (l, t) in listed.iter().zip(&lib.spreads) {
            assert_eq!(l.shape, shape_of(t), "{}", t.id);
            assert_eq!(l.shape.boxes().len(), t.photo_count(), "{}", t.id);
        }
    }

    #[test]
    fn custom_layouts_store_updates_renames_and_sets_auto() {
        let db = db();
        let a = create(&db, "A", &two_up()).unwrap();
        let shape = spread(vec![b(0.0, 0.0, 0.5, 1.0)]);
        assert_eq!(update(&db, &a.id, &shape).unwrap().shape, shape);
        assert_eq!(rename(&db, &a.id, "B").unwrap().name, "B");
        assert!(set_auto(&db, &a.id, true).unwrap().auto);
        let back = get(&db, &a.id).unwrap();
        assert_eq!((back.name.as_str(), back.auto, &back.shape), ("B", true, &shape));
        assert!(!set_auto(&db, &a.id, false).unwrap().auto);
    }

    #[test]
    fn custom_layouts_store_duplicates_under_a_new_id_with_auto_off() {
        let db = db();
        let a = create(&db, "A", &two_up()).unwrap();
        set_auto(&db, &a.id, true).unwrap();
        let copy = duplicate(&db, &a.id).unwrap();
        assert_ne!(copy.id, a.id);
        assert_eq!(copy.name, "A copy");
        assert_eq!(copy.shape, a.shape);
        assert!(!copy.auto);
        assert_eq!(list(&db).unwrap().len(), 2);

        let long = create(&db, &"n".repeat(MAX_NAME_CHARS), &two_up()).unwrap();
        let long_copy = duplicate(&db, &long.id).unwrap();
        assert_eq!(long_copy.name.chars().count(), MAX_NAME_CHARS);
        assert!(long_copy.name.ends_with(" copy"));
    }

    #[test]
    fn custom_layouts_store_deletes_and_reports_a_missing_id() {
        let db = db();
        let a = create(&db, "A", &two_up()).unwrap();
        let keep = create(&db, "B", &two_up()).unwrap();
        delete(&db, &a.id).unwrap();
        assert_eq!(list(&db).unwrap(), vec![keep]);
        let missing = CustomLayoutError::NotFound { id: a.id.clone() };
        assert_eq!(get(&db, &a.id), Err(missing.clone()));
        assert_eq!(delete(&db, &a.id), Err(missing.clone()));
        assert_eq!(update(&db, &a.id, &two_up()).map(|_| ()), Err(missing.clone()));
        assert_eq!(rename(&db, &a.id, "C").map(|_| ()), Err(missing.clone()));
        assert_eq!(set_auto(&db, &a.id, true).map(|_| ()), Err(missing.clone()));
        assert_eq!(duplicate(&db, &a.id).map(|_| ()), Err(missing));
    }

    #[test]
    fn custom_layouts_store_refuses_bad_names_at_the_limit() {
        let db = db();
        assert_eq!(create(&db, "   ", &two_up()), Err(CustomLayoutError::EmptyName));
        let at = "n".repeat(MAX_NAME_CHARS);
        assert!(create(&db, &at, &two_up()).is_ok());
        let over = "n".repeat(MAX_NAME_CHARS + 1);
        let too_long = CustomLayoutError::NameTooLong { max: MAX_NAME_CHARS };
        assert_eq!(create(&db, &over, &two_up()), Err(too_long.clone()));
        let a = create(&db, "A", &two_up()).unwrap();
        assert_eq!(rename(&db, &a.id, &over), Err(too_long));
        assert_eq!(rename(&db, &a.id, ""), Err(CustomLayoutError::EmptyName));
    }

    /// Only structural rules refuse a save; a print rule depends on the book.
    #[test]
    fn custom_layouts_store_refuses_only_structural_problems() {
        let db = db();
        let crossing = spread(vec![b(0.4, 0.2, 0.2, 0.5)]);
        match create(&db, "A", &crossing) {
            Err(CustomLayoutError::Invalid { problems }) => {
                assert_eq!(problems, vec![Problem::from(LayoutProblem::CrossesFold { slot: 0 })]);
            }
            other => panic!("{other:?}"),
        }
        let a = create(&db, "A", &two_up()).unwrap();
        assert!(matches!(update(&db, &a.id, &crossing), Err(CustomLayoutError::Invalid { .. })));
        assert_eq!(get(&db, &a.id).unwrap().shape, two_up(), "a refused update changes nothing");
        assert_eq!(list(&db).unwrap().len(), 1);

        // Inside the bleed at Pixajoy: a print problem, so it saves.
        let in_bleed = page(Side::Right, vec![b(0.3, 0.3, 0.69, 0.4)]);
        assert!(!print_rules(&in_bleed, &pixajoy_spec()).is_empty());
        assert!(create(&db, "B", &in_bleed).is_ok());
    }

    #[test]
    fn custom_layouts_store_skips_a_row_whose_shape_no_longer_parses() {
        let db = db();
        let a = create(&db, "A", &two_up()).unwrap();
        db.conn
            .execute(
                "INSERT INTO custom_layouts (id, name, shape_json) VALUES ('user-bad', 'bad', '{\"kind\":\"circle\"}')",
                [],
            )
            .unwrap();
        assert_eq!(list(&db).unwrap(), vec![a]);
    }

    // --- the wire ---

    /// The Rust half of `tests/fixtures/wire/custom-layouts.json`; the
    /// TypeScript half is `tests/customLayouts.test.ts`. Layouts and problems
    /// are literals (ids and timestamps are random and clock-bound); every
    /// error but `storage` comes from a real store call, so its wording is
    /// the store's own. Every float is an exact binary fraction, so
    /// `to_value` compares directly.
    #[test]
    fn custom_layouts_serialise_exactly_the_keys_the_webview_reads() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/wire/custom-layouts.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing wire fixture {path:?}: {e}"));
        let fixture: serde_json::Value =
            serde_json::from_str(&text).expect("wire fixture must be valid JSON");

        let support = |x, y, w, h| LayoutBox { rect: Rect::new(x, y, w, h), role: Role::Support };
        let layouts = vec![
            CustomLayout {
                id: "user-0123456789abcdef0123456789abcdef".into(),
                name: "Two halves".into(),
                shape: two_up(),
                auto: false,
                created_at: 1_762_560_000,
                updated_at: 1_762_560_000,
            },
            CustomLayout {
                id: "user-fedcba9876543210fedcba9876543210".into(),
                name: "Corner".into(),
                shape: page(Side::Right, vec![b(0.25, 0.25, 0.5, 0.5), support(0.75, 0.75, 0.25, 0.25)]),
                auto: true,
                created_at: 1_762_560_000,
                updated_at: 1_762_563_600,
            },
        ];
        let problems: Vec<Problem> = vec![
            LayoutProblem::NoBoxes,
            LayoutProblem::TooManyBoxes { count: 9, max: MAX_BOXES },
            LayoutProblem::NotFinite { slot: 0 },
            LayoutProblem::OffCanvas { slot: 1 },
            LayoutProblem::TooSmall { slot: 2 },
            LayoutProblem::CrossesFold { slot: 0 },
            LayoutProblem::Overlap { slot: 0, other: 1 },
            LayoutProblem::EdgeInBleed { slot: 1, edge: BleedEdge::Top, bleed_mm: 5.0 },
            LayoutProblem::EdgeInGutter { slot: 0, gutter_mm: 5.0 },
            LayoutProblem::MostlyHidden { slot: 3 },
        ]
        .into_iter()
        .map(Problem::from)
        .collect();
        let refused = crate::templates::RefusedLayout {
            template_id: "user-0123456789abcdef0123456789abcdef".into(),
            problems: vec![LayoutProblem::EdgeInBleed { slot: 0, edge: BleedEdge::Left, bleed_mm: 5.0 }.into()],
        };

        let db = db();
        let errors = vec![
            get(&db, "user-gone").unwrap_err(),
            create(&db, "  ", &two_up()).unwrap_err(),
            create(&db, &"x".repeat(MAX_NAME_CHARS + 1), &two_up()).unwrap_err(),
            create(&db, "Empty", &spread(vec![])).unwrap_err(),
            CustomLayoutError::Storage { message: "disk I/O error".into() },
        ];

        let value = serde_json::json!({
            "layouts": layouts,
            "problems": problems,
            "refused": refused,
            "errors": errors,
        });
        assert_eq!(value, fixture);

        // And the other direction: the shapes the webview sends back parse.
        for (sent, layout) in fixture["layouts"].as_array().unwrap().iter().zip(&layouts) {
            let shape: LayoutShape = serde_json::from_value(sent["shape"].clone()).unwrap();
            assert_eq!(shape, layout.shape);
        }
    }
}

/// User layouts inside the engine: what auto layout may pick, what the menu
/// offers, what a book refuses, and what survives deleting one.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::book::cull::{Overrides, PaletteColor, Photo};
    use crate::book::edit::{alternatives, apply, opening_count, opening_pages, refused, BookEdit, EditError};
    use crate::book::pace::{assemble, Book};
    use crate::print_spec::pixajoy_spec;
    use crate::templates::{Library, Weights};

    /// The frozen five: `03` (1 photo), `07` and `08` (2), `13` and `35` (3).
    fn frozen_library() -> Library {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
        Library::load(&dir).unwrap()
    }

    /// As `edit.rs`'s fixture: landscape, every third one portrait.
    fn photo(i: usize) -> Photo {
        let portrait = i % 3 == 2;
        Photo {
            path: format!("/e/p{i:02}.jpg"),
            hash: format!("h{i:02}"),
            width: if portrait { 3000 } else { 4000 },
            height: if portrait { 4000 } else { 3000 },
            is_utility: false,
            aesthetic_pct: ((i * 37) % 100) as u8,
            sharpness_pct: 50,
            near_dup_cluster: i as u32,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::<PaletteColor>::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0,
            clipped_high: 0.0,
            feature_print: None,
            location: None,
            utc_offset: None,
            warmth: None,
            horizon_tilt_deg: None,
        }
    }

    fn photos(n: usize) -> Vec<Photo> {
        (0..n).map(photo).collect()
    }

    fn bx(x: f64, y: f64, w: f64, h: f64) -> LayoutBox {
        LayoutBox { rect: Rect::new(x, y, w, h), role: Role::Hero }
    }

    fn layout(id: &str, shape: LayoutShape, auto: bool) -> CustomLayout {
        CustomLayout { id: id.into(), name: format!("{id} name"), shape, auto, created_at: 0, updated_at: 0 }
    }

    const PAIR: &str = "user-pair";
    const TRIO: &str = "user-trio";
    const SOLO: &str = "user-solo";

    /// Full-bleed, one photo per page: the shape the fixture's landscape
    /// photos fit best, so it wins whenever it is allowed to compete.
    fn pair(auto: bool) -> CustomLayout {
        layout(PAIR, LayoutShape::Spread { boxes: vec![bx(0.0, 0.0, 0.5, 1.0), bx(0.5, 0.0, 0.5, 1.0)] }, auto)
    }

    fn trio(auto: bool) -> CustomLayout {
        let boxes = vec![bx(0.0, 0.0, 0.5, 1.0), bx(0.5, 0.0, 0.5, 0.5), bx(0.5, 0.5, 0.5, 0.5)];
        layout(TRIO, LayoutShape::Spread { boxes }, auto)
    }

    /// A full-bleed page 1.
    fn solo(auto: bool) -> CustomLayout {
        layout(SOLO, LayoutShape::Page { side: Side::Right, boxes: vec![bx(0.0, 0.0, 1.0, 1.0)] }, auto)
    }

    fn lib_with(layouts: &[CustomLayout]) -> Library {
        frozen_library().with_custom(layouts, &pixajoy_spec())
    }

    fn all(auto: bool) -> Library {
        lib_with(&[pair(auto), trio(auto), solo(auto)])
    }

    fn generate(lib: &Library, seed: u64) -> Book {
        assemble(&pixajoy_spec(), &photos(25), 20, lib, &Weights::default(), seed, &Overrides::new()).unwrap()
    }

    fn uses_custom(book: &Book) -> Vec<String> {
        book.pages.iter().map(|p| p.template_id.clone()).filter(|id| id.starts_with(ID_PREFIX)).collect()
    }

    fn template_of(book: &Book, o: usize) -> String {
        book.pages[opening_pages(book, o).unwrap()[0]].template_id.clone()
    }

    fn photo_count(book: &Book, o: usize) -> usize {
        opening_pages(book, o).unwrap().iter().map(|&p| book.pages[p].placements.len()).sum()
    }

    fn spread_with(book: &Book, n: usize) -> usize {
        (1..opening_count(book) - 1).find(|&o| photo_count(book, o) == n).unwrap()
    }

    const SEEDS: [u64; 6] = [1, 7, 42, 1234, 9001, 31337];

    // --- generate ------------------------------------------------------------

    /// The same photos and seeds, the flag the only difference: on, the
    /// layouts win openings (so they score best there); off, they never do.
    #[test]
    fn custom_layouts_engine_generate_uses_a_layout_only_with_auto_on() {
        let on: Vec<String> = SEEDS.iter().flat_map(|&s| uses_custom(&generate(&all(true), s))).collect();
        assert!(on.iter().any(|id| id == PAIR), "the pair must win somewhere with auto on: {on:?}");
        assert!(on.iter().any(|id| id == SOLO), "the full-bleed page 1 must win with auto on: {on:?}");
        for s in SEEDS {
            assert_eq!(uses_custom(&generate(&all(false), s)), Vec::<String>::new(), "seed {s}");
        }
    }

    // --- regenerate, reject, shuffle -----------------------------------------

    /// Regenerating until nothing is left never reaches an auto-off layout,
    /// though the menu offers it the whole time.
    #[test]
    fn custom_layouts_engine_regenerate_never_picks_an_auto_off_layout() {
        let ps = photos(25);
        let w = Weights::default();
        for (auto, expect) in [(false, false), (true, true)] {
            let lib = all(auto);
            let mut b = generate(&frozen_library(), 1234);
            let o = spread_with(&b, 2);
            let mut seen = vec![template_of(&b, o)];
            for _ in 0..8 {
                assert!(alternatives(&b, &lib, o).contains(&PAIR.to_string()) || seen.contains(&PAIR.to_string()));
                match apply(&mut b, &BookEdit::Regenerate { opening: o }, &lib, &ps, &w) {
                    Ok(()) => seen.push(template_of(&b, o)),
                    Err(EditError::NoAlternative { .. }) => break,
                    Err(e) => panic!("{e:?}"),
                }
            }
            assert_eq!(seen.contains(&PAIR.to_string()), expect, "auto {auto}: {seen:?}");
        }
    }

    /// With every built-in rejected, auto on leaves the user layout as the
    /// best and only pick; auto off leaves nothing, and the book is unchanged.
    #[test]
    fn custom_layouts_engine_reject_reaches_a_layout_only_with_auto_on() {
        let ps = photos(25);
        let w = Weights::default();
        for auto in [false, true] {
            let lib = all(auto);
            let mut b = generate(&frozen_library(), 1234);
            let o = spread_with(&b, 2);
            let other = ["07-two-up-symmetric-margin", "08-two-up-symmetric-bleed-outer"]
                .into_iter()
                .find(|id| *id != template_of(&b, o))
                .unwrap();
            b.controls.entry(o).or_default().rejected.push(other.to_string());
            let before = b.clone();
            let result = apply(&mut b, &BookEdit::RejectTemplate { opening: o }, &lib, &ps, &w);
            if auto {
                result.unwrap();
                assert_eq!(template_of(&b, o), PAIR);
            } else {
                assert!(matches!(result, Err(EditError::NoAlternative { .. })), "{result:?}");
                assert_eq!(b, before);
                assert_eq!(alternatives(&b, &lib, o), vec![PAIR.to_string()], "the menu still offers it");
            }
        }
    }

    /// The menu still lists a user layout with auto off, so `alternatives`
    /// alone cannot say whether Regenerate and Reject have anything to pick.
    /// `regenerable` does: with only that layout left, it is false until the
    /// layout's auto is on.
    #[test]
    fn custom_layouts_engine_preview_says_regenerate_has_nothing_when_only_an_auto_off_layout_is_left() {
        for auto in [false, true] {
            let lib = all(auto);
            let mut b = generate(&frozen_library(), 1234);
            let o = spread_with(&b, 2);
            let other = ["07-two-up-symmetric-margin", "08-two-up-symmetric-bleed-outer"]
                .into_iter()
                .find(|id| *id != template_of(&b, o))
                .unwrap();
            b.controls.entry(o).or_default().rejected.push(other.to_string());
            let preview = crate::preview::book_layout(1, &b, Vec::new(), &lib, &[]);
            let opening = &preview.openings[o];
            assert_eq!(opening.alternatives, vec![PAIR.to_string()]);
            assert_eq!(opening.regenerable, auto, "auto = {auto}");
        }
        // And a built-in alternative alone is regenerable.
        let lib = all(false);
        let b = generate(&frozen_library(), 1234);
        let o = spread_with(&b, 2);
        assert!(crate::preview::book_layout(1, &b, Vec::new(), &lib, &[]).openings[o].regenerable);
    }

    #[test]
    fn custom_layouts_engine_shuffle_uses_a_layout_only_with_auto_on() {
        let ps = photos(25);
        let w = Weights::default();
        for auto in [false, true] {
            let lib = all(auto);
            let mut found = Vec::new();
            for s in SEEDS {
                let mut b = generate(&frozen_library(), s);
                for _ in 0..3 {
                    apply(&mut b, &BookEdit::Shuffle, &lib, &ps, &w).unwrap();
                    found.extend(uses_custom(&b));
                }
            }
            assert_eq!(!found.is_empty(), auto, "auto {auto}: {found:?}");
        }
    }

    // --- add photo -----------------------------------------------------------

    /// Adding a third photo with both built-in 3-up spreads rejected: auto on
    /// takes the user trio, auto off has nowhere to put it.
    #[test]
    fn custom_layouts_engine_add_photo_picks_a_layout_only_with_auto_on() {
        // One photo more than the book was made from: imported afterwards.
        let ps = photos(26);
        let spare = 25;
        let w = Weights::default();
        for auto in [false, true] {
            let lib = all(auto);
            let mut b = generate(&frozen_library(), 1234);
            let o = spread_with(&b, 2);
            let rejected = &mut b.controls.entry(o).or_default().rejected;
            rejected.extend(["13-three-up-hero-left-stack-right".to_string(), "35-portrait-triptych-hero-left".to_string()]);
            let before = b.clone();
            let result = apply(&mut b, &BookEdit::AddPhoto { opening: o, photo: spare }, &lib, &ps, &w);
            if auto {
                result.unwrap();
                assert_eq!(template_of(&b, o), TRIO);
                assert_eq!(photo_count(&b, o), 3);
            } else {
                assert!(matches!(result, Err(EditError::OpeningFull { .. })), "{result:?}");
                assert_eq!(b, before);
            }
        }
    }

    // --- the menu and SetTemplate --------------------------------------------

    #[test]
    fn custom_layouts_engine_menu_offers_an_auto_off_layout_and_set_template_uses_it() {
        let ps = photos(25);
        let w = Weights::default();
        let lib = all(false);
        let mut b = generate(&frozen_library(), 1234);
        let o = spread_with(&b, 2);
        assert!(alternatives(&b, &lib, o).contains(&PAIR.to_string()));
        assert!(!alternatives(&b, &lib, o).contains(&TRIO.to_string()), "wrong photo count");
        assert!(alternatives(&b, &lib, 0).contains(&SOLO.to_string()), "page 1 is a right page");
        let last = opening_count(&b) - 1;
        assert!(!alternatives(&b, &lib, last).contains(&SOLO.to_string()), "the last page is a left page");

        apply(&mut b, &BookEdit::SetTemplate { opening: o, template_id: PAIR.into() }, &lib, &ps, &w).unwrap();
        assert_eq!(template_of(&b, o), PAIR);
        let pages = opening_pages(&b, o).unwrap();
        let rects: Vec<Rect> = pages.iter().map(|&p| b.pages[p].placements[0].slot_rect).collect();
        assert_eq!(rects, vec![Rect::new(0.0, 0.0, 1.0, 1.0); 2], "the boxes as drawn, one per page");

        apply(&mut b, &BookEdit::SetTemplate { opening: 0, template_id: SOLO.into() }, &lib, &ps, &w).unwrap();
        assert_eq!(b.pages[0].template_id, SOLO);

        let preview = crate::preview::book_layout(1, &b, Vec::new(), &lib, &[]);
        assert_eq!(preview.sketches[PAIR], vec![vec![Rect::new(0.0, 0.0, 1.0, 1.0)]; 2]);
        assert_eq!(preview.layout_names[PAIR], "user-pair name");
        assert_eq!(preview.layout_names[SOLO], "user-solo name");
    }

    // --- refused for this book -----------------------------------------------

    /// A pair whose outer edges stop 2 mm in from the paper edge: it saves,
    /// but this book's 5 mm bleed refuses it, with the reason, on every
    /// 2-photo opening, and SetTemplate cannot reach it.
    #[test]
    fn custom_layouts_engine_lists_a_refused_layout_with_its_reason() {
        let ps = photos(25);
        let w = Weights::default();
        let spec = pixajoy_spec();
        let inset = spec.trim_u() / 2.0 * 0.5;
        let bad = layout(
            "user-bad",
            LayoutShape::Spread { boxes: vec![bx(inset, 0.0, 0.5 - inset, 1.0), bx(0.5, 0.0, 0.5 - inset, 1.0)] },
            true,
        );
        assert!(structural(&bad.shape).is_empty(), "it is a print problem, not a structural one");
        // And a page 1 whose outer edge stops in the bleed the same way.
        let bad_page = layout(
            "user-bad-page",
            LayoutShape::Page { side: Side::Right, boxes: vec![bx(0.0, 0.0, 1.0 - spec.trim_u() * 0.5, 1.0)] },
            true,
        );
        let lib = lib_with(&[bad, bad_page]);
        let mut b = generate(&frozen_library(), 1234);
        let o = spread_with(&b, 2);

        assert!(!alternatives(&b, &lib, o).contains(&"user-bad".to_string()));
        let r = refused(&b, &lib, o);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].template_id, "user-bad");
        let kinds: Vec<LayoutProblem> = r[0].problems.iter().map(|p| p.problem).collect();
        assert_eq!(
            kinds,
            vec![
                LayoutProblem::EdgeInBleed { slot: 0, edge: BleedEdge::Left, bleed_mm: 5.0 },
                LayoutProblem::EdgeInBleed { slot: 1, edge: BleedEdge::Right, bleed_mm: 5.0 },
            ]
        );
        assert!(r[0].problems[0].message.contains("5 mm bleed"), "{}", r[0].problems[0].message);
        assert!(refused(&b, &lib, spread_with(&b, 3)).is_empty(), "other photo counts are not told");
        let last = opening_count(&b) - 1;
        assert_eq!((photo_count(&b, 0), photo_count(&b, last)), (1, 1), "fixture: both single pages hold one");
        let first: Vec<String> = refused(&b, &lib, 0).into_iter().map(|r| r.template_id).collect();
        assert_eq!(first, vec!["user-bad-page"], "page 1 is told about the page layout only");
        assert!(refused(&b, &lib, last).is_empty(), "the last page is a left page");

        let before = b.clone();
        let result = apply(&mut b, &BookEdit::SetTemplate { opening: o, template_id: "user-bad".into() }, &lib, &ps, &w);
        assert!(matches!(result, Err(EditError::UnknownTemplate(_))), "{result:?}");
        assert_eq!(b, before);
        for s in SEEDS {
            assert!(uses_custom(&generate(&lib, s)).is_empty(), "auto on cannot override a refusal");
        }

        let preview = crate::preview::book_layout(1, &b, Vec::new(), &lib, &[]);
        assert_eq!(preview.openings[o].refused, r);
        assert_eq!(preview.sketches["user-bad"].len(), 2, "drawn dimmed in the menu");
        assert_eq!(preview.layout_names["user-bad"], "user-bad name");
    }

    // --- deletion ------------------------------------------------------------

    /// A book laid out on a layout the user then deletes keeps its pages as
    /// laid out, still previews, and can still be edited and re-laid.
    #[test]
    fn custom_layouts_engine_deleting_a_layout_leaves_books_working() {
        let ps = photos(25);
        let w = Weights::default();
        let mut b = generate(&frozen_library(), 1234);
        let o = spread_with(&b, 2);
        apply(&mut b, &BookEdit::SetTemplate { opening: o, template_id: PAIR.into() }, &all(false), &ps, &w).unwrap();
        let laid = b.clone();

        let gone = frozen_library().with_custom(&[], &pixajoy_spec());
        let preview = crate::preview::book_layout(1, &b, Vec::new(), &gone, &[]);
        let pages = opening_pages(&b, o).unwrap();
        for &p in &pages {
            assert_eq!(preview.pages[p].placements[0].slot_rect, Rect::new(0.0, 0.0, 1.0, 1.0));
        }
        assert!(!preview.sketches.contains_key(PAIR), "no sketch for an id that no longer resolves");
        assert!(!preview.openings[o].alternatives.is_empty(), "the built-ins are still offered");

        let number = b.pages[pages[0]].number;
        let placement = crate::book::edit::PlacementRef { page: number, z: 1 };
        apply(&mut b, &BookEdit::SetCrop { placement, x: 0.0, y: 0.0, w: 0.9 }, &gone, &ps, &w).unwrap();
        assert_eq!(b.pages[pages[0]].template_id, PAIR, "a crop edit leaves the layout alone");
        assert_eq!(b.pages[pages[0]].placements[0].slot_rect, laid.pages[pages[0]].placements[0].slot_rect);

        apply(&mut b, &BookEdit::Shuffle, &gone, &ps, &w).unwrap();
        apply(&mut b, &BookEdit::Regenerate { opening: o }, &gone, &ps, &w).unwrap();
        assert_ne!(template_of(&b, o), PAIR);
        assert!(!template_of(&b, o).starts_with(ID_PREFIX));
    }

    // --- changes reach the next command --------------------------------------

    #[test]
    fn custom_layouts_engine_the_library_follows_the_store() {
        let db = Db::open_in_memory().unwrap();
        let spec = pixajoy_spec();
        let shape = pair(false).shape;
        let a = create(&db, "A", &shape).unwrap();
        let lib = || frozen_library().with_custom(&list(&db).unwrap(), &spec);
        assert!(lib().spread(&a.id).is_some());
        assert!(!lib().in_auto(&a.id));
        set_auto(&db, &a.id, true).unwrap();
        assert!(lib().in_auto(&a.id));
        update(&db, &a.id, &trio(true).shape).unwrap();
        assert_eq!(lib().spread(&a.id).unwrap().photo_count(), 3);
        delete(&db, &a.id).unwrap();
        assert!(lib().spread(&a.id).is_none());
    }
}
