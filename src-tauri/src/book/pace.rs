//! Assembling a whole book, then sweeping it once for rhythm.
//!
//! Ordering is deliberate and load-bearing: cull before sizing (a book
//! cannot be sized before it is culled), pack before scoring (exact-count
//! templates mean group size decides eligibility), pace after scoring (a
//! rhythm cannot be varied before it exists).
//!
//! A book is 2 single pages + (N-2)/2 spreads. Page 1 is a right-hand page
//! facing the inside front cover and the last page is a left-hand page
//! facing the inside back cover; neither has a partner, so both draw from
//! the pool of page-halves rather than from whole spread templates.

use crate::book::crop::choose_crop;
use crate::book::cull::{cull, Overrides, Photo};
use crate::book::pack::{pack, Buildable, Capacity, Group, IncludeOverflow, SlotKind};
use crate::book::score::{best_spread, rejects, slot_aspect};
use crate::geometry::{Rect, Side};
use crate::templates::{EdgeTreatment, Library, PageLayout, SpreadTemplate, Weights};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The `template_id` of a page that carries no layout at all. A deliberately
/// blank page is a legitimate pacing device (spec 5.1), and the SKU fixes the
/// page count -- so where nothing can be laid out the page is emitted empty
/// rather than skipped, and the manifest still reconciles.
pub const BLANK_TEMPLATE_ID: &str = "blank";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Index into the ORIGINAL photo slice handed to `assemble`, so the
    /// manifest can name the source file without a second lookup table.
    pub photo_index: usize,
    /// Page-normalised destination rect.
    pub slot_rect: Rect,
    /// Crop window in the photo's own normalised coordinates.
    pub crop: Rect,
    /// Stacking order within the page, 1-based. Only matters where slots
    /// overlap, but it is also the order the user places the boxes in.
    pub z: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub number: u32,
    pub side: Side,
    /// The spread template's id for a spread page, `"<id>:left"` or
    /// `"<id>:right"` for a single page built from one half of that template,
    /// or `BLANK_TEMPLATE_ID`. The half is named rather than described
    /// (`"half:Left:2"`) because a manifest that cannot say WHICH template a
    /// page came from cannot reproduce the book.
    pub template_id: String,
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub pages: Vec<Page>,
    pub seed: u64,
    /// How many analysed photos did not make it into the book, for the
    /// confirmation the user sees before anything is rendered. Counts every
    /// cause: culled, trimmed to capacity, left unplaced by the packer, or
    /// overflowing a page half.
    pub dropped: usize,
}

/// Deterministic tie-breaker. The ONLY stochastic choice in the engine is
/// breaking an exact score tie; everything else is a total order over the
/// inputs. "Regenerate this spread" ADVANCES the seed rather than calling a
/// random source, which is what makes regeneration feel like browsing
/// alternatives instead of rolling dice -- and what makes golden files
/// possible at all.
pub(crate) fn tie_break(seed: u64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    // SplitMix64: tiny, dependency-free, and stable across platforms and
    // Rust versions -- `DefaultHasher` guarantees none of those.
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) % n as u64) as usize
}

/// Builds the placements for one page layout given the photos assigned to it.
/// Slot order is z order: 1-based, distinct, and the order the boxes are
/// stacked in the editor.
fn place(layout: &PageLayout, photo_indices: &[usize], photos: &[Photo]) -> Vec<Placement> {
    layout
        .slots
        .iter()
        .zip(photo_indices)
        .enumerate()
        .map(|(i, (slot, &photo_index))| Placement {
            photo_index,
            slot_rect: slot.rect,
            crop: choose_crop(&photos[photo_index], slot_aspect(slot)),
            z: i as u32 + 1,
        })
        .collect()
}

fn blank_page(side: Side) -> Page {
    Page { number: 0, side, template_id: BLANK_TEMPLATE_ID.to_string(), placements: Vec::new() }
}

fn half_id(template_id: &str, side: Side) -> String {
    match side {
        Side::Left => format!("{template_id}:left"),
        Side::Right => format!("{template_id}:right"),
    }
}

/// Every page-half paired with the template it came from.
/// `Library::page_half_pool` drops the parent id, which is enough for
/// capacity arithmetic but not for a manifest that has to name what it used.
fn half_pool(lib: &Library) -> Vec<(&str, &PageLayout)> {
    lib.spreads
        .iter()
        .flat_map(|t| [(t.id.as_str(), &t.left), (t.id.as_str(), &t.right)])
        .collect()
}

/// Aspect fit of a group against one page half, or `None` when a hard
/// constraint rejects it. Single pages go through the same face, gutter and
/// DPI rejections as spreads: a face cut in half on page 1 is exactly as
/// ruined as one on page 7.
///
/// Photos are zipped to slots in group order rather than permuted. A half has
/// at most a few slots and no spread-level terms to trade off, so the
/// permutation search `best_spread` runs is not worth its cost here.
fn single_fit(layout: &PageLayout, indices: &[usize], photos: &[Photo]) -> Option<f64> {
    let mut fit = 0.0;
    for (slot, &pi) in layout.slots.iter().zip(indices) {
        let photo = &photos[pi];
        let target = slot_aspect(slot);
        let crop = choose_crop(photo, target);
        if rejects(photo, &crop, slot, layout.side).is_some() {
            return None;
        }
        let actual = photo.aspect();
        fit += if target > actual { actual / target } else { target / actual };
    }
    Some(fit)
}

/// Picks the best page-half for a single page, and the photos it can hold.
///
/// Pages 1 and N have no partner page, so they draw from every half of every
/// template -- a half is by construction a valid page layout, so the single
/// pages get a library for free and one that matches the book's style.
///
/// Two constraints the exact-match version misses:
///
/// * the half must be authored for the SIDE it lands on. Bleed edges and the
///   gutter are side-specific, so a left half on a right-hand page bleeds off
///   the fold.
/// * the half must hold AT MOST the group, not exactly it. `pack` sizes every
///   group by SPREAD counts, and two of the slots it sizes for are single
///   pages that hold only a half's worth, so requiring `slots.len() == n`
///   leaves the opening page blank whenever the first group is oversized.
///
/// Sizes are tried LARGEST FIRST and the first size with a surviving
/// candidate wins. Committing to the largest size that fits and giving up
/// when every half at that size is rejected -- a clipped face, a face in the
/// gutter, too few pixels -- would leave the page blank while a smaller half
/// in the same library was perfectly printable. A blank page is a legitimate
/// pacing device but it must be the last resort, not the first failure.
fn best_single<'a>(
    pool: &[(&'a str, &'a PageLayout)],
    photos: &[Photo],
    group: &Group,
    side: Side,
    seed: u64,
) -> Option<(&'a str, &'a PageLayout, Vec<usize>)> {
    let n = group.photos.len();
    if n == 0 {
        return None;
    }
    let fits = |l: &PageLayout| l.side == side && !l.slots.is_empty() && l.slots.len() <= n;
    let sizes: BTreeSet<usize> =
        pool.iter().filter(|(_, l)| fits(l)).map(|(_, l)| l.slots.len()).collect();

    for capacity in sizes.into_iter().rev() {
        let indices = strongest(group, photos, capacity);
        let mut scored: Vec<(f64, &str, &PageLayout)> = pool
            .iter()
            .filter(|(_, l)| fits(l) && l.slots.len() == capacity)
            .filter_map(|(id, l)| single_fit(l, &indices, photos).map(|fit| (fit, *id, *l)))
            .collect();
        if scored.is_empty() {
            continue; // nothing at this size survived; try a smaller half
        }
        let best = scored.iter().fold(f64::NEG_INFINITY, |m, (fit, _, _)| m.max(*fit));
        // Exact equality is the definition of the tie the seed exists to
        // break; anything looser would let the seed override a real
        // preference.
        scored.retain(|(fit, _, _)| *fit == best);
        let (_, id, layout) = scored[tie_break(seed, scored.len())];
        return Some((id, layout, indices));
    }
    None
}

/// The `capacity` strongest photos of a group, in group order.
///
/// A single page holds less than the group `pack` sized for a spread, so some
/// photos are dropped. Which ones is a real decision, not a truncation: this
/// mirrors `pack`'s own worst-first ranking when it trims to capacity --
/// aesthetic percentile, then sharpness, then path for determinism -- and
/// drops from that end, so the page keeps the strongest photos rather than
/// whichever happen to sort first by filename.
fn strongest(group: &Group, photos: &[Photo], capacity: usize) -> Vec<usize> {
    if group.photos.len() <= capacity {
        return group.photos.clone();
    }
    let mut ranked = group.photos.clone();
    ranked.sort_by(|&a, &b| {
        photos[a]
            .aesthetic_pct
            .cmp(&photos[b].aesthetic_pct)
            .then(photos[a].sharpness_pct.cmp(&photos[b].sharpness_pct))
            .then(photos[a].path.cmp(&photos[b].path))
    });
    let cut: BTreeSet<usize> =
        ranked.into_iter().take(group.photos.len() - capacity).collect();
    group.photos.iter().copied().filter(|i| !cut.contains(i)).collect()
}

/// Builds one single page, blank when nothing can be laid out on it.
fn single_page(
    side: Side,
    group: Option<&Group>,
    pool: &[(&str, &PageLayout)],
    photos: &[Photo],
    seed: u64,
) -> Page {
    let Some((id, layout, indices)) =
        group.and_then(|g| best_single(pool, photos, g, side, seed))
    else {
        return blank_page(side);
    };
    Page {
        number: 0,
        side,
        template_id: half_id(id, side),
        placements: place(layout, &indices, photos),
    }
}

/// The two pages of one spread, or `None` when no eligible template survives
/// the hard constraints.
fn spread_pages<'a>(
    group: &Group,
    photos: &[Photo],
    lib: &'a Library,
    w: &Weights,
    previous: Option<&str>,
    seed: u64,
) -> Option<(&'a SpreadTemplate, [Page; 2])> {
    let refs: Vec<&Photo> = group.photos.iter().map(|&i| &photos[i]).collect();
    let eligible = lib.spreads_with(group.photos.len());
    let (template, assignment, _) = best_spread(&eligible, &refs, previous, w, seed)?;
    Some((template, rebuild(template, &assignment, &group.photos, photos)))
}

/// Lays `indices` (ordered as `assignment` indexes them) into a template's
/// two halves. Shared by assembly and repacing so a swapped template can
/// never keep the rects of the one it replaced.
fn rebuild(
    template: &SpreadTemplate,
    assignment: &[usize],
    indices: &[usize],
    photos: &[Photo],
) -> [Page; 2] {
    let left_n = template.left.slots.len();
    let left_idx: Vec<usize> = assignment[..left_n].iter().map(|&a| indices[a]).collect();
    let right_idx: Vec<usize> = assignment[left_n..].iter().map(|&a| indices[a]).collect();
    [
        Page {
            number: 0,
            side: Side::Left,
            template_id: template.id.clone(),
            placements: place(&template.left, &left_idx, photos),
        },
        Page {
            number: 0,
            side: Side::Right,
            template_id: template.id.clone(),
            placements: place(&template.right, &right_idx, photos),
        },
    ]
}

/// Why a book could not be built as asked. Both variants exist for the same
/// reason: the user made an explicit choice the engine cannot honour, and
/// reporting that is the only honest outcome -- silently discarding one is
/// precisely the failure user-controlled selection exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    /// More photos were marked `Include` than a book of this length can hold.
    /// Detected before anything is laid out.
    IncludedExceedCapacity(IncludeOverflow),
    /// Every `Include` fits the book's capacity, but the layout engine could
    /// not seat these ones: a chapter apportioned too few slots, a group that
    /// overflowed a page half, or a template that rejected the photo outright.
    ///
    /// This is the catch-all post-condition, deliberately checked against the
    /// FINISHED book rather than at each of the places a photo can be lost.
    /// There are four such places and they are not all reachable by a test, so
    /// a per-site guard would be a claim this module cannot back; asserting
    /// over the result is one check that covers all of them, present and
    /// future.
    IncludedNotPlaced { paths: Vec<String> },
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncludedExceedCapacity(overflow) => overflow.fmt(f),
            Self::IncludedNotPlaced { paths } => write!(
                f,
                "{} photo(s) you marked to include could not be placed in this book: {}. \
                 Exclude them, or choose a longer book.",
                paths.len(),
                paths.join(", ")
            ),
        }
    }
}

/// Full pipeline: cull -> size -> pack -> score -> place -> pace.
///
/// Always emits exactly `pages` pages. A group that cannot be laid out yields
/// a page with zero placements rather than a shorter book: the SKU fixes the
/// page count, and a 19-page book cannot be uploaded against a 20-page
/// product.
///
/// `overrides` carries the user's own include/exclude decisions all the way
/// through: `cull` honours them, `pack` refuses to trim an `Include` away,
/// and the post-condition at the bottom of this function refuses to RETURN a
/// book that lost one by any other route.
pub fn assemble(
    photos: &[Photo],
    pages: u32,
    lib: &Library,
    w: &Weights,
    seed: u64,
    overrides: &Overrides,
) -> Result<Book, BookError> {
    let kept = cull(photos, overrides);
    // `from_library`, not `from_sizes`: the latter over-estimates what the two
    // single pages hold by measuring them against the largest whole SPREAD.
    let buildable = Buildable::from_library(lib);
    let cap = Capacity::from_library(pages, lib);
    let groups =
        pack(&kept, &cap, &buildable, overrides).map_err(BookError::IncludedExceedCapacity)?;

    // `pack` indexes into `kept`; the manifest wants indices into `photos`.
    // Keyed on PATH, not hash: a content hash is not unique within a run --
    // two byte-identical files in one scan both reach the sidecar -- so a
    // hash-keyed remap silently maps the second onto the first's index and the
    // manifest names the wrong source file. Built once rather than by a linear
    // scan per photo.
    let mut by_path: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, p) in photos.iter().enumerate() {
        by_path.entry(p.path.as_str()).or_insert(i);
    }
    let groups: Vec<Group> = groups
        .iter()
        .map(|g| Group {
            // `kept` is a subset of `photos`, so the lookup cannot miss; a
            // photo that somehow did would be left out and counted dropped
            // rather than panicking mid-book.
            photos: g
                .photos
                .iter()
                .filter_map(|&i| by_path.get(kept[i].path.as_str()).copied())
                .collect(),
            event_cluster: g.event_cluster,
            slot: g.slot,
        })
        .collect();

    let pool = half_pool(lib);
    let spread_count = (pages.saturating_sub(2) / 2) as usize;
    let mut out: Vec<Page> = Vec::with_capacity(pages as usize);

    // Placed by the packer's OWN tag, not by position. When photos run out,
    // `pack` produces fewer groups than there are slots, and the last group it
    // cut is then a middle spread -- placing it on a page half regardless is
    // what used to let `strongest` silently trim it.
    let n = groups.len();
    let opening = groups.first().filter(|g| g.slot == SlotKind::Single);
    let closing = (n > 1)
        .then(|| &groups[n - 1])
        .filter(|g| g.slot == SlotKind::Single);
    let middle: Vec<&Group> =
        groups.iter().filter(|g| g.slot == SlotKind::Spread).collect();

    // Page 1: a single, facing the inside front cover.
    out.push(single_page(Side::Right, opening, &pool, photos, seed));

    // The middle: true spreads, two pages each.
    let mut previous: Option<String> = None;
    for s in 0..spread_count {
        match middle.get(s).and_then(|g| spread_pages(g, photos, lib, w, previous.as_deref(), seed)) {
            Some((template, [left, right])) => {
                previous = Some(template.id.clone());
                out.push(left);
                out.push(right);
            }
            // No group left, or nothing eligible survived the hard
            // constraints. Either way the pages are still emitted.
            None => {
                out.push(blank_page(Side::Left));
                out.push(blank_page(Side::Right));
            }
        }
    }

    // The final page: a single, facing the inside back cover.
    out.push(single_page(Side::Left, closing, &pool, photos, seed));

    // Odd or degenerate page counts are not real SKUs, but the promise is
    // exactly `pages` pages, so honour it either way.
    out.truncate(pages as usize);
    while out.len() < pages as usize {
        out.push(blank_page(if out.len() % 2 == 0 { Side::Right } else { Side::Left }));
    }

    // Numbered last and in one place: every page above is built with a
    // placeholder number, so there is no path on which a blank page leaves a
    // gap in the sequence.
    for (i, page) in out.iter_mut().enumerate() {
        page.number = i as u32 + 1;
    }

    let mut book = Book { pages: out, seed, dropped: 0 };
    repace(&mut book, lib, photos, w, seed);

    // Counted from what actually survived into the book, after repacing, so
    // no accounting can drift out of step with the pages.
    let placed: BTreeSet<usize> = book
        .pages
        .iter()
        .flat_map(|p| p.placements.iter().map(|pl| pl.photo_index))
        .collect();
    book.dropped = photos.len().saturating_sub(placed.len());

    // The post-condition. Everything above tries to seat every explicit
    // choice; this is what makes "an Include is never silently dropped" a
    // property of the OUTPUT rather than a hope about four separate code
    // paths. Ordered by path so the message is stable.
    let mut missing: Vec<String> = photos
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            overrides.get(&p.hash) == crate::book::cull::Override::Include && !placed.contains(i)
        })
        .map(|(_, p)| p.path.clone())
        .collect();
    if !missing.is_empty() {
        missing.sort();
        return Err(BookError::IncludedNotPlaced { paths: missing });
    }

    Ok(book)
}

/// The three pacing axes. `density` and `energy` are authored per template;
/// edge treatment is derived from the per-slot bleed arrays, which is what
/// keeps it from requiring an edit to every existing template file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Density,
    Energy,
    Edge,
}

const AXES: [Axis; 3] = [Axis::Density, Axis::Energy, Axis::Edge];

/// A spread reads as edge-to-edge when EITHER of its pages does.
fn spread_edge(t: &SpreadTemplate) -> EdgeTreatment {
    if t.left.edge_treatment == EdgeTreatment::Bleed
        || t.right.edge_treatment == EdgeTreatment::Bleed
    {
        EdgeTreatment::Bleed
    } else {
        EdgeTreatment::Margin
    }
}

fn same_on(axis: Axis, a: &SpreadTemplate, b: &SpreadTemplate) -> bool {
    match axis {
        Axis::Density => a.density == b.density,
        Axis::Energy => a.energy == b.energy,
        Axis::Edge => spread_edge(a) == spread_edge(b),
    }
}

/// The template of spread `s`, where spread `s` occupies pages `1 + 2s` and
/// `2 + 2s`. `None` for a blank spread or a single page.
fn spread_template<'a>(book: &Book, lib: &'a Library, s: usize) -> Option<&'a SpreadTemplate> {
    let page = book.pages.get(1 + 2 * s)?;
    lib.spreads.iter().find(|t| t.id == page.template_id)
}

/// Sweeps the assembled book once and breaks flat stretches.
///
/// Runs AFTER scoring because a rhythm cannot be varied before it exists. All
/// three axes are checked together, not density alone: in the authored
/// library density is currently a function of photo count (1 -> sparse,
/// 2..4 -> medium, 6 -> dense), so a density-only sweep can never fire against
/// it, while energy and edge treatment genuinely do vary within a count.
///
/// Takes the photos and weights because a swap RE-LAYS the spread through
/// `best_spread`. Relabelling `template_id` and keeping the old placements
/// would leave a page naming one template while carrying another's rects,
/// which is unrenderable -- and it would skip the hard constraints, which the
/// new template's slots have not been checked against.
pub fn repace(book: &mut Book, lib: &Library, photos: &[Photo], w: &Weights, seed: u64) {
    let spread_count = book.pages.len().saturating_sub(2) / 2;

    for s in 2..spread_count {
        let (Some(a), Some(b), Some(c)) = (
            spread_template(book, lib, s - 2),
            spread_template(book, lib, s - 1),
            spread_template(book, lib, s),
        ) else {
            continue;
        };
        let flat: Vec<Axis> =
            AXES.into_iter().filter(|&ax| same_on(ax, a, b) && same_on(ax, b, c)).collect();
        if flat.is_empty() {
            continue;
        }

        // The middle spread of the run is the one that gets swapped: changing
        // an end just moves the flat stretch along by one.
        let middle = s - 1;
        let indices: Vec<usize> = book.pages[1 + 2 * middle]
            .placements
            .iter()
            .chain(book.pages[2 + 2 * middle].placements.iter())
            .map(|p| p.photo_index)
            .collect();
        if indices.is_empty() {
            continue;
        }

        let breaks = |t: &SpreadTemplate| flat.iter().filter(|&&ax| !same_on(ax, t, b)).count();
        let alternatives: Vec<&SpreadTemplate> =
            lib.spreads_with(indices.len()).into_iter().filter(|t| t.id != b.id).collect();
        let Some(most) = alternatives.iter().map(|t| breaks(t)).max().filter(|&m| m > 0) else {
            continue;
        };
        let candidates: Vec<&SpreadTemplate> =
            alternatives.into_iter().filter(|t| breaks(t) == most).collect();

        let refs: Vec<&Photo> = indices.iter().map(|&i| &photos[i]).collect();
        let previous = a.id.as_str();
        let Some((template, assignment, _)) =
            best_spread(&candidates, &refs, Some(previous), w, seed)
        else {
            continue;
        };

        let [left, right] = rebuild(template, &assignment, &indices, photos);
        let numbers = (book.pages[1 + 2 * middle].number, book.pages[2 + 2 * middle].number);
        book.pages[1 + 2 * middle] = Page { number: numbers.0, ..left };
        book.pages[2 + 2 * middle] = Page { number: numbers.1, ..right };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::Override;
    use crate::book::cull::{Face, PaletteColor};

    // --- fixture library -------------------------------------------------
    //
    // Authored in SPREAD coordinates and loaded through `Library::load`, so
    // the fixtures exercise the real decomposition rather than hand-written
    // page rects that could silently disagree with it.
    //
    // Slot rects are ASYMMETRIC wherever a left/right mixup would otherwise
    // be invisible. The one deliberately identical pair (`f5`'s right half and
    // `f7`'s right half) exists so that an exact scoring tie is reachable at
    // all -- without it the seed would have nothing to break and every
    // determinism test would pass under an implementation that ignored it.
    // `f8` is `f3`'s geometric clone for the same reason, one level up: a
    // tie between two whole SPREAD templates, not just two page halves.
    //
    // Density, energy and edge treatment all vary WITHIN a photo count. In
    // the real library they do not: density is currently a function of photo
    // count (1 -> sparse, 2..4 -> medium, 6 -> dense), so a fixture copied
    // from it could never exercise a density swap.

    const F1_ONE_UP: &str = r#"{
        "id": "f1-one-up-left",
        "slots": [
            {"rect":[0.0,0.0,0.46,1.0],"role":"hero","bleed":["left","top","bottom"],
             "aspect_pref":[1.1,1.5]}
        ],
        "text_zones": [],
        "min_photos": 1, "max_photos": 1,
        "density": "sparse", "energy": "calm"
    }"#;

    const F2_TWO_UP_MARGIN: &str = r#"{
        "id": "f2-two-up-margin",
        "slots": [
            {"rect":[0.04,0.08,0.42,0.84],"role":"support","bleed":[],"aspect_pref":[1.05,1.5]},
            {"rect":[0.54,0.08,0.42,0.84],"role":"support","bleed":[],"aspect_pref":[1.05,1.5]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "medium", "energy": "calm"
    }"#;

    const F3_TWO_UP_BLEED: &str = r#"{
        "id": "f3-two-up-bleed",
        "slots": [
            {"rect":[0.0,0.0,0.46,1.0],"role":"hero","bleed":["left","top","bottom"],
             "aspect_pref":[1.1,1.5]},
            {"rect":[0.56,0.1,0.38,0.8],"role":"support","bleed":[],"aspect_pref":[1.0,1.4]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "medium", "energy": "lively"
    }"#;

    /// A geometric CLONE of `f3-two-up-bleed`: identical slot rects, roles,
    /// bleed edges and aspect preferences, in the identical left-then-right
    /// order, so `ordered_slots` produces the same terms in the same order
    /// for any assignment and the two templates' scores are not merely equal
    /// but computed by the identical sequence of floating-point operations --
    /// a genuine bit-exact tie, not one that happens to round the same way.
    /// Only the id (and, to avoid asserting a degenerate two-template pool
    /// elsewhere, the density label) differs.
    ///
    /// This is the spread-level counterpart of the `f5`/`f7` trick above: the
    /// fixture library had no exact tie between two SPREAD templates until
    /// this one was added -- verified by running
    /// `pace_uses_the_seed_to_break_a_tie_between_middle_spreads` before this
    /// template existed, which failed with every seed choosing
    /// `f3-two-up-bleed` outright.
    const F8_TWO_UP_BLEED_CLONE: &str = r#"{
        "id": "f8-two-up-bleed-clone",
        "slots": [
            {"rect":[0.0,0.0,0.46,1.0],"role":"hero","bleed":["left","top","bottom"],
             "aspect_pref":[1.1,1.5]},
            {"rect":[0.56,0.1,0.38,0.8],"role":"support","bleed":[],"aspect_pref":[1.0,1.4]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "medium", "energy": "lively"
    }"#;

    const F4_TWO_UP_DENSE: &str = r#"{
        "id": "f4-two-up-dense",
        "slots": [
            {"rect":[0.06,0.12,0.38,0.76],"role":"support","bleed":[],"aspect_pref":[1.0,1.4]},
            {"rect":[0.5,0.0,0.5,1.0],"role":"hero","bleed":["right","top","bottom"],
             "aspect_pref":[1.1,1.5]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "dense", "energy": "neutral"
    }"#;

    const F5_THREE_UP_HERO_LEFT: &str = r#"{
        "id": "f5-three-up-hero-left",
        "slots": [
            {"rect":[0.0,0.0,0.46,1.0],"role":"hero","bleed":["left","top","bottom"],
             "aspect_pref":[1.1,1.5]},
            {"rect":[0.56,0.06,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]},
            {"rect":[0.56,0.52,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]}
        ],
        "text_zones": [],
        "min_photos": 3, "max_photos": 3,
        "density": "medium", "energy": "neutral"
    }"#;

    const F6_THREE_UP_HERO_RIGHT: &str = r#"{
        "id": "f6-three-up-hero-right",
        "slots": [
            {"rect":[0.54,0.0,0.46,1.0],"role":"hero","bleed":["right","top","bottom"],
             "aspect_pref":[1.1,1.5]},
            {"rect":[0.04,0.06,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]},
            {"rect":[0.04,0.52,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]}
        ],
        "text_zones": [],
        "min_photos": 3, "max_photos": 3,
        "density": "medium", "energy": "lively"
    }"#;

    /// Same right half as `f5`, different hero and different density, so two
    /// RIGHT-hand halves score exactly equal on the opening page.
    const F7_THREE_UP_HERO_LEFT_ALT: &str = r#"{
        "id": "f7-three-up-hero-left-alt",
        "slots": [
            {"rect":[0.02,0.06,0.42,0.88],"role":"hero","bleed":[],"aspect_pref":[1.1,1.5]},
            {"rect":[0.56,0.06,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]},
            {"rect":[0.56,0.52,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]}
        ],
        "text_zones": [],
        "min_photos": 3, "max_photos": 3,
        "density": "sparse", "energy": "lively"
    }"#;

    /// Two templates whose declared `density` is a one-to-one function of
    /// their slot count -- 2 -> Sparse, 3 -> Medium -- so that "the assembled
    /// book shows two densities" is provably the same claim as "the packer
    /// built two different group sizes". Both fill both page halves, so no
    /// choice between them can print white.
    ///
    /// The buckets are not invented: the real library declares `Sparse` at
    /// size 2 (`42-two-up-fold-flush-bleed`,
    /// `43-two-up-bleed-left-inset-right`) and `Medium` at size 3 (all six).
    ///
    /// Buildable spread sizes are `{2, 3}` -- deliberately the SAME set as
    /// `frozen_library()`, which holds a 1-photo template that `pack` may no
    /// longer cut a spread group for, plus 2-photo and 3-photo ones. The only
    /// difference from the frozen library is the density LABEL on the 2-up,
    /// which is the whole point: on both libraries `assemble` produces spreads
    /// of `[2,2,2,2,2,2,3,3,3]`, so the packer was never converging. The
    /// frozen library simply labels sizes 2 and 3 both `Medium` and cannot
    /// express the variety the packer already has.
    ///
    /// A third, `Dense` size-4 template was tried and left out on evidence,
    /// not to buy a green test. Adding one makes `union_bounds().1` 4 instead
    /// of 3, and measured on the same 30 photos and seed that changes the
    /// assembled book from 24 placed / 6 dropped / spreads `[2,2,2,2,2,2,3,3,3]`
    /// to 22 placed / 8 dropped / spreads `[2,2,2,2,2,2,2,2,2]` -- a library
    /// that can build BIGGER spreads places FEWER photos and converges. That
    /// is an apportionment question in `pack`, not a template one, and it is
    /// recorded here rather than hidden behind a fixture chosen to avoid it.
    const D2_TWO_UP_SPARSE: &str = r#"{
        "id": "d2-two-up-sparse",
        "slots": [
            {"rect":[0.06,0.12,0.36,0.76],"role":"support","bleed":[],"aspect_pref":[1.03,1.36]},
            {"rect":[0.58,0.12,0.36,0.76],"role":"support","bleed":[],"aspect_pref":[1.03,1.36]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "sparse", "energy": "calm"
    }"#;

    const D3_THREE_UP_MEDIUM: &str = r#"{
        "id": "d3-three-up-medium",
        "slots": [
            {"rect":[0.04,0.06,0.42,0.88],"role":"hero","bleed":[],"aspect_pref":[1.03,1.37]},
            {"rect":[0.56,0.06,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]},
            {"rect":[0.56,0.52,0.4,0.42],"role":"support","bleed":[],"aspect_pref":[2.0,2.8]}
        ],
        "text_zones": [],
        "min_photos": 3, "max_photos": 3,
        "density": "medium", "energy": "neutral"
    }"#;

    fn library_from(files: &[(&str, &str)]) -> Library {
        let dir = tempfile::tempdir().unwrap();
        for (name, json) in files {
            std::fs::write(dir.path().join(name), json).unwrap();
        }
        Library::load(dir.path()).expect("fixture templates must decompose")
    }

    fn fixture_library() -> Library {
        library_from(&[
            ("f1.json", F1_ONE_UP),
            ("f2.json", F2_TWO_UP_MARGIN),
            ("f3.json", F3_TWO_UP_BLEED),
            ("f4.json", F4_TWO_UP_DENSE),
            ("f5.json", F5_THREE_UP_HERO_LEFT),
            ("f6.json", F6_THREE_UP_HERO_RIGHT),
            ("f7.json", F7_THREE_UP_HERO_LEFT_ALT),
            ("f8.json", F8_TWO_UP_BLEED_CLONE),
        ])
    }

    fn density_by_size_library() -> Library {
        library_from(&[
            ("d2.json", D2_TWO_UP_SPARSE),
            ("d3.json", D3_THREE_UP_MEDIUM),
        ])
    }

    /// The frozen five, copied from the real library. Goldens read THIS, never
    /// `templates/`: Task 13 authors 15-20 more templates and every golden
    /// would churn on that commit.
    fn frozen_library() -> Library {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
        Library::load(&dir).expect("the frozen fixture library must decompose")
    }

    /// The library that actually ships. Goldens must never read this (see
    /// `frozen_library` above), but a property that is the whole point of a
    /// change has to be asserted against the real thing or it is only ever
    /// asserted about a fixture nobody prints.
    fn real_library() -> Library {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        Library::load(&dir).expect("the real template library must decompose")
    }

    // --- fixture photos --------------------------------------------------

    /// Photos with THREE different aspect ratios (4:3, 3:2, 3:4) -- never all
    /// square, which hides every aspect bug -- across several chapters of
    /// UNEQUAL length, with a face on every third photo and a saliency box
    /// that surrounds it.
    ///
    /// Every seventh photo is `is_utility`, so `cull` removes it and the
    /// surviving indices no longer line up with the input slice. Without that
    /// the index remap in `assemble` would be the identity function and no
    /// test could tell a correct remap from a missing one.
    ///
    /// `near_dup_cluster` is unique per photo: sharing one would make `cull`
    /// collapse the whole fixture to a single keeper.
    fn fixture_photos(n: usize) -> Vec<Photo> {
        (0..n)
            .map(|i| {
                let (width, height) = match i % 3 {
                    0 => (4032, 3024), // 4:3
                    1 => (3600, 2400), // 3:2
                    _ => (3024, 4032), // 3:4
                };
                // Centred enough that no crop clips it and no slot pushes it
                // into the gutter; the point of the faces is that the crop
                // and DPI paths run on real inputs, not that they reject.
                let faces = if i % 3 == 0 {
                    vec![Face {
                        box_: Rect::new(0.40, 0.30, 0.10, 0.14),
                        capture_quality: Some(0.4 + (i % 5) as f64 / 10.0),
                    }]
                } else {
                    Vec::new()
                };
                Photo {
                    path: format!("/photos/p{i:03}.jpg"),
                    hash: format!("hash-{i:03}"),
                    width,
                    height,
                    is_utility: i % 7 == 0,
                    aesthetic_pct: ((i * 37) % 100) as u8,
                    sharpness_pct: ((i * 53) % 100) as u8,
                    near_dup_cluster: i as u32,
                    event_cluster: (i as f64).sqrt() as u32,
                    face_area_fraction: if faces.is_empty() { 0.0 } else { 0.014 },
                    faces,
                    saliency_box: Some(Rect::new(0.30 + (i % 4) as f64 * 0.02, 0.20, 0.30, 0.40)),
                    palette: vec![PaletteColor {
                        r: 0.2 + (i % 5) as f64 * 0.15,
                        g: 0.3,
                        b: 0.7 - (i % 5) as f64 * 0.1,
                        weight: 1.0,
                    }],
                    capture_quality: if i % 3 == 0 { Some(0.4) } else { None },
                    scene_tags: Vec::new(),
                    captured_at: None,
                }
            })
            .collect()
    }

    /// Resolves the page layout a page's `template_id` names, for both spread
    /// pages (`"f2-two-up-margin"`) and single pages (`"f2-two-up-margin:left"`).
    fn named_layout<'a>(lib: &'a Library, page: &Page) -> Option<&'a PageLayout> {
        let (id, side) = match page.template_id.split_once(':') {
            Some((id, "left")) => (id, Side::Left),
            Some((id, "right")) => (id, Side::Right),
            Some(_) => return None,
            None => (page.template_id.as_str(), page.side),
        };
        let t = lib.spreads.iter().find(|t| t.id == id)?;
        Some(if side == Side::Left { &t.left } else { &t.right })
    }

    fn placed_indices(book: &Book) -> Vec<usize> {
        book.pages
            .iter()
            .flat_map(|p| p.placements.iter().map(|pl| pl.photo_index))
            .collect()
    }

    /// Paths of every photo that reached a page, so an assertion can name a
    /// file rather than an index the reader has to resolve.
    fn placed_paths(book: &Book, photos: &[Photo]) -> Vec<String> {
        let mut out: Vec<String> =
            placed_indices(book).into_iter().map(|i| photos[i].path.clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    fn overrides(pairs: &[(&str, Override)]) -> Overrides {
        pairs.iter().map(|(h, s)| ((*h).to_string(), *s)).collect()
    }

    // --- the user's own choices, end to end through the whole pipeline ----

    /// **An excluded photo never appears in the finished book.**
    ///
    /// Asserted on the assembled `Book` rather than on `cull`'s return value:
    /// that is the artefact that gets printed, and the whole point of the
    /// feature is what comes out at the end. The un-overridden run is
    /// asserted first, so the test cannot pass because the photo was never
    /// going to be in the book anyway.
    #[test]
    fn pace_never_places_an_excluded_photo() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let before = assemble(&photos, 20, &lib, &Weights::default(), 42, &Overrides::new())
            .expect("no overrides");
        assert!(
            placed_paths(&before, &photos).contains(&"/photos/p005.jpg".to_string()),
            "fixture: this photo must be in the book without an override, or the test is inert"
        );

        let after = assemble(
            &photos,
            20,
            &lib,
            &Weights::default(),
            42,
            &overrides(&[("hash-005", Override::Exclude)]),
        )
        .expect("excluding a photo cannot overflow anything");

        assert!(
            !placed_paths(&after, &photos).contains(&"/photos/p005.jpg".to_string()),
            "an excluded photo reached the printed book: {:?}",
            placed_paths(&after, &photos)
        );
    }

    /// **An included photo that lost its near-duplicate cluster still reaches
    /// the book.**
    ///
    /// The fixture gives photos 3, 4 and 5 one shared cluster -- THREE, not
    /// two, because a two-photo cluster cannot distinguish "kept the included
    /// one" from "kept both". Photo 3 is the sharpest and wins the cluster
    /// automatically; photo 5 is the worst and can only get in by being asked
    /// for; photo 4 is the control that must stay out.
    #[test]
    fn pace_places_an_included_photo_that_lost_its_near_duplicate_cluster() {
        let lib = fixture_library();
        let mut photos = fixture_photos(24);
        for (i, sharp) in [(3usize, 90u8), (4, 60), (5, 10)] {
            photos[i].near_dup_cluster = 99;
            photos[i].sharpness_pct = sharp;
        }
        let before = assemble(&photos, 20, &lib, &Weights::default(), 42, &Overrides::new())
            .expect("no overrides");
        let was = placed_paths(&before, &photos);
        assert!(was.contains(&"/photos/p003.jpg".to_string()), "fixture: 3 wins the burst");
        assert!(
            !was.contains(&"/photos/p005.jpg".to_string()),
            "fixture: 5 must lose the burst without an override, or the test is inert"
        );

        let after = assemble(
            &photos,
            20,
            &lib,
            &Weights::default(),
            42,
            &overrides(&[("hash-005", Override::Include)]),
        )
        .expect("one extra photo is well inside capacity");

        let now = placed_paths(&after, &photos);
        assert!(now.contains(&"/photos/p005.jpg".to_string()), "the explicit choice is absent: {now:?}");
        assert!(now.contains(&"/photos/p003.jpg".to_string()), "and must not displace the winner");
        assert!(!now.contains(&"/photos/p004.jpg".to_string()), "the cluster rule still governs the rest");
    }

    /// Two frames from the SAME burst, both asked for, both printed.
    #[test]
    fn pace_places_both_frames_when_two_from_one_burst_are_included() {
        let lib = fixture_library();
        let mut photos = fixture_photos(24);
        for (i, sharp) in [(3usize, 90u8), (4, 60), (5, 10)] {
            photos[i].near_dup_cluster = 99;
            photos[i].sharpness_pct = sharp;
        }

        let book = assemble(
            &photos,
            20,
            &lib,
            &Weights::default(),
            42,
            &overrides(&[("hash-004", Override::Include), ("hash-005", Override::Include)]),
        )
        .expect("two extra photos are well inside capacity");

        let now = placed_paths(&book, &photos);
        for path in ["/photos/p003.jpg", "/photos/p004.jpg", "/photos/p005.jpg"] {
            assert!(now.contains(&path.to_string()), "{path} missing from {now:?}");
        }
    }

    /// **Included photos that cannot all fit are reported, not silently cut.**
    ///
    /// Every photo in the fixture is asked for, against a book whose capacity
    /// is read from the library rather than hardcoded -- so the fixture
    /// cannot silently stop overflowing if the template set changes.
    #[test]
    fn pace_refuses_to_build_a_book_too_short_for_every_photo_the_user_asked_for() {
        let lib = fixture_library();
        let capacity = Capacity::from_library(20, &lib).max_photos;
        let photos = fixture_photos(capacity + 5);
        let all: Overrides =
            photos.iter().map(|p| (p.hash.clone(), Override::Include)).collect();

        let err = assemble(&photos, 20, &lib, &Weights::default(), 42, &all)
            .expect_err("every photo asked for, more than the book holds");

        assert_eq!(
            err,
            BookError::IncludedExceedCapacity(IncludeOverflow {
                pages: 20,
                included: capacity + 5,
                capacity,
                over: 5,
            })
        );
    }

    /// The post-condition, on the one route that is still reachable: a photo
    /// the templates cannot lay out at all.
    ///
    /// This photo is 300x225, which resolves at roughly 30 DPI in any slot in
    /// the library and is hard-rejected by every one of them. `pack` seats it
    /// happily -- it is a group member like any other -- and it then falls
    /// out at layout time. Without the post-condition it would simply be
    /// absent from the finished book, which is exactly the silent discard
    /// this feature exists to prevent, so `assemble` refuses to return that
    /// book and names the file.
    #[test]
    fn pace_refuses_a_book_that_lost_an_included_photo_to_the_layout_engine() {
        let lib = fixture_library();
        let mut photos = fixture_photos(24);
        photos[5].width = 300;
        photos[5].height = 225;

        let err = assemble(
            &photos,
            20,
            &lib,
            &Weights::default(),
            42,
            &overrides(&[("hash-005", Override::Include)]),
        )
        .expect_err("a photo no template can lay out must be reported, not dropped");

        assert_eq!(err, BookError::IncludedNotPlaced { paths: vec!["/photos/p005.jpg".into()] });
        assert!(err.to_string().contains("/photos/p005.jpg"), "{err}");
    }

    /// **Capacity trimming would have dropped this photo, and the book still
    /// contains it.**
    ///
    /// The end-to-end counterpart of
    /// `pack_never_drops_an_included_photo_when_trimming_to_capacity`: that
    /// one asserts on `pack`'s groups, this one on the printed pages, which
    /// is the artefact the property is actually about. There are more photos
    /// than the book holds, the included photo is the WORST in the set by the
    /// comparator the trim uses (aesthetic 0, sharpness 0), and the
    /// un-overridden run is asserted to drop it first -- so nothing but the
    /// override can put it on a page.
    #[test]
    fn pace_keeps_an_included_photo_capacity_trimming_would_have_dropped() {
        let lib = fixture_library();
        let capacity = Capacity::from_library(20, &lib).max_photos;
        let mut photos = fixture_photos(capacity + 12);
        // The weakest photo in the set, by both keys the trim ranks on.
        photos[5].aesthetic_pct = 0;
        photos[5].sharpness_pct = 0;
        photos[5].is_utility = false;
        photos[5].near_dup_cluster = 5000;

        let before = assemble(&photos, 20, &lib, &Weights::default(), 42, &Overrides::new())
            .expect("no overrides");
        assert!(
            before.dropped > 0,
            "fixture must actually overflow the book, or nothing is trimmed"
        );
        assert!(
            !placed_paths(&before, &photos).contains(&"/photos/p005.jpg".to_string()),
            "fixture: the trim must drop this photo without an override, or the test is inert"
        );

        let after = assemble(
            &photos,
            20,
            &lib,
            &Weights::default(),
            42,
            &overrides(&[("hash-005", Override::Include)]),
        )
        .expect("one included photo is far inside capacity");

        assert!(
            placed_paths(&after, &photos).contains(&"/photos/p005.jpg".to_string()),
            "the photo the user asked for was trimmed away: {:?}",
            placed_paths(&after, &photos)
        );
    }

    /// The post-condition must name EVERY photo it could not place, not the
    /// first one it noticed. A message listing one file when two are missing
    /// sends the user round the exclude-and-retry loop once per photo.
    ///
    /// Two unplaceable includes, and the list is asserted in full and in
    /// path order -- a fixture with one missing photo cannot tell a complete
    /// list from a `find()`.
    #[test]
    fn pace_names_every_included_photo_it_could_not_place_not_just_the_first() {
        let lib = fixture_library();
        let mut photos = fixture_photos(24);
        for i in [5usize, 11] {
            photos[i].width = 300;
            photos[i].height = 225;
        }

        let err = assemble(
            &photos,
            20,
            &lib,
            &Weights::default(),
            42,
            &overrides(&[("hash-005", Override::Include), ("hash-011", Override::Include)]),
        )
        .expect_err("neither photo can be laid out");

        assert_eq!(
            err,
            BookError::IncludedNotPlaced {
                paths: vec!["/photos/p005.jpg".into(), "/photos/p011.jpg".into()]
            }
        );
    }

    /// An empty override map must leave the assembled book byte-identical to
    /// what the engine produced before overrides existed. Compared as a whole
    /// `Book`, so a change to any placement, crop or template choice fails.
    #[test]
    fn pace_with_no_overrides_assembles_exactly_the_same_book() {
        let lib = fixture_library();
        let photos = fixture_photos(30);
        let a = assemble(&photos, 20, &lib, &Weights::default(), 1234, &Overrides::new()).unwrap();
        let b = assemble(&photos, 20, &lib, &Weights::default(), 1234, &Overrides::default()).unwrap();
        assert_eq!(a, b);
        assert!(a.pages.iter().any(|p| !p.placements.is_empty()), "sanity: not an empty book");
    }

    // --- structure -------------------------------------------------------

    #[test]
    fn pace_assembles_the_correct_page_count_for_twenty_pages() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 42, &Overrides::new()).expect("the fixture must place every included photo");
        assert_eq!(book.pages.len(), 20, "2 singles + 9 spreads = 20 pages");
        assert_eq!(book.pages[0].side, Side::Right, "page 1 faces the inside front cover");
        assert_eq!(
            book.pages.last().unwrap().side,
            Side::Left,
            "the final page faces the inside back cover"
        );
    }

    #[test]
    fn pace_numbers_pages_consecutively_from_one() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 42, &Overrides::new()).expect("the fixture must place every included photo");
        let numbers: Vec<u32> = book.pages.iter().map(|p| p.number).collect();
        assert_eq!(numbers, (1..=20).collect::<Vec<u32>>());
    }

    /// Every odd page number is a right-hand page and every even one a left --
    /// the physical consequence of `1 . 2-3 . 4-5 . ... . N`. The page-count
    /// test only pins the two ends; this pins the 18 pages between them, where
    /// a swapped side would otherwise put a left-page layout on the right of
    /// the fold for the whole book.
    #[test]
    fn pace_alternates_sides_so_odd_pages_are_right_hand_pages() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 42, &Overrides::new()).expect("the fixture must place every included photo");
        for page in &book.pages {
            let expected = if page.number % 2 == 1 { Side::Right } else { Side::Left };
            assert_eq!(page.side, expected, "page {} sits on the wrong side", page.number);
        }
    }

    /// R2: the SKU fixes the page count. Every layout here is rejected on
    /// resolution (300px photos in ~9" slots is ~30 DPI against a 200 floor),
    /// so a `continue`-on-failure assembler emits an empty book instead of a
    /// blank-but-complete one.
    #[test]
    fn pace_emits_the_full_page_count_even_when_every_layout_is_rejected() {
        let lib = fixture_library();
        let mut photos = fixture_photos(24);
        for p in &mut photos {
            p.width = 300;
            p.height = 225;
        }
        let book = assemble(&photos, 20, &lib, &Weights::default(), 42, &Overrides::new()).expect("the fixture must place every included photo");
        assert_eq!(book.pages.len(), 20, "a 19-page book cannot be uploaded as a 20-page SKU");
        assert!(
            book.pages.iter().all(|p| p.placements.is_empty()),
            "nothing is placeable at 30 DPI"
        );
        assert_eq!(book.dropped, photos.len(), "every photo is dropped, not silently lost");
        assert!(book.pages.iter().all(|p| p.template_id == BLANK_TEMPLATE_ID));
    }

    /// R2 has a second half the page count alone cannot see: the blank must
    /// appear WHERE the failed group sits. Skipping it instead shifts every
    /// later spread two pages earlier and pushes the closing single page out
    /// of the last position -- and because the book is padded back up to the
    /// SKU length, the page count stays 20 either way.
    ///
    /// One chapter here is unprintable (300px photos, ~30 DPI against a 200
    /// floor); every other chapter lays out. 36 fixture photos fill all 11
    /// slots exactly, so the only blanks in a correct book are that chapter's.
    /// Chapter 3 is apportioned two of the eleven slots and its groups land on
    /// spreads 2 and 3, i.e. pages 6-9.
    #[test]
    fn pace_leaves_a_failed_spread_blank_in_place_rather_than_shifting_the_book() {
        let lib = fixture_library();
        let mut photos = fixture_photos(36);
        for p in photos.iter_mut().filter(|p| p.event_cluster == 3) {
            p.width = 300;
            p.height = 225;
        }
        let book = assemble(&photos, 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        assert_eq!(book.pages.len(), 20);

        let blanks: Vec<u32> = book
            .pages
            .iter()
            .filter(|p| p.template_id == BLANK_TEMPLATE_ID)
            .map(|p| p.number)
            .collect();
        assert_eq!(blanks, vec![6, 7, 8, 9], "the blanks belong to the failed chapter");
        assert!(
            !book.pages.last().unwrap().placements.is_empty(),
            "the closing single page was pushed out of the last position"
        );
        let intact = assemble(&fixture_photos(36), 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        assert!(
            intact.pages.iter().all(|p| p.template_id != BLANK_TEMPLATE_ID),
            "fixture check: 36 photos fill all 11 slots, so only the failure blanks"
        );
    }

    /// When there are fewer photos than slots, `pack` produces fewer groups
    /// than the book has slots, so the last group it cut was sized for a
    /// SPREAD. Placing it on the closing page-half regardless means
    /// `pace::strongest` silently trims photos off it; the honest outcome is a
    /// blank closing page, which is what a reader can actually see and act on.
    #[test]
    fn pace_leaves_the_closing_page_blank_when_the_packer_ran_out_of_groups() {
        let lib = fixture_library();
        // Far fewer photos than the 11 slots a 20-page book has, so `pack`
        // cannot fill every slot and the final group it cuts is a middle
        // spread, not the closing single.
        let photos = fixture_photos(4);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 7, &Overrides::new())
            .expect("the fixture must place every included photo");

        let last = book.pages.last().expect("a 20-page book has a last page");
        assert_eq!(last.number, 20);
        assert!(
            last.placements.is_empty(),
            "the closing page must be blank rather than hold a spread-sized group, \
             but it carried {} placements",
            last.placements.len()
        );
    }

    // --- determinism -----------------------------------------------------

    #[test]
    fn pace_is_deterministic_for_the_same_seed() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let a = assemble(&photos, 20, &lib, &Weights::default(), 7, &Overrides::new()).expect("the fixture must place every included photo");
        let b = assemble(&photos, 20, &lib, &Weights::default(), 7, &Overrides::new()).expect("the fixture must place every included photo");
        assert_eq!(a.pages, b.pages);
    }

    /// The test above passes under an implementation that ignores the seed
    /// entirely, so it proves nothing on its own. This one fails unless the
    /// seed actually reaches a tie-break: `f5` and `f7` have identical RIGHT
    /// halves, so they score EXACTLY equal on the opening page (a right-hand
    /// page) and only the seed can separate them.
    #[test]
    fn pace_uses_the_seed_to_break_an_exact_scoring_tie() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let openings: BTreeSet<String> = (0..16)
            .map(|seed| {
                assemble(&photos, 20, &lib, &Weights::default(), seed, &Overrides::new()).expect("the fixture must place every included photo").pages[0].template_id.clone()
            })
            .collect();
        assert!(
            openings.len() > 1,
            "the seed never changed the outcome of a genuine tie: {openings:?}"
        );
    }

    /// The tie-break must be a pure function of the seed -- not of hash
    /// iteration order, address layout or time -- or golden files are
    /// impossible.
    #[test]
    fn pace_tie_break_is_a_stable_function_of_the_seed() {
        assert_eq!(tie_break(0, 1), 0, "a single candidate is not a choice");
        assert_eq!(tie_break(12345, 0), 0, "an empty candidate set must not divide by zero");
        let a: Vec<usize> = (0..8).map(|s| tie_break(s, 5)).collect();
        let b: Vec<usize> = (0..8).map(|s| tie_break(s, 5)).collect();
        assert_eq!(a, b);
        assert!(a.iter().all(|&i| i < 5));
        assert!(a.iter().collect::<BTreeSet<_>>().len() > 1, "every seed mapped to one index");
    }

    // --- placement -------------------------------------------------------

    /// The brief checked the x-axis only, so a slot escaping the top or
    /// bottom of the page would have gone unnoticed.
    #[test]
    fn pace_places_every_photo_within_its_page() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        for page in &book.pages {
            for pl in &page.placements {
                assert!(
                    pl.slot_rect.x >= -1e-9 && pl.slot_rect.right() <= 1.0 + 1e-9,
                    "page {} slot escapes the canvas horizontally: {:?}",
                    page.number,
                    pl.slot_rect
                );
                assert!(
                    pl.slot_rect.y >= -1e-9 && pl.slot_rect.bottom() <= 1.0 + 1e-9,
                    "page {} slot escapes the canvas vertically: {:?}",
                    page.number,
                    pl.slot_rect
                );
                assert!(
                    pl.crop.x >= -1e-9
                        && pl.crop.y >= -1e-9
                        && pl.crop.right() <= 1.0 + 1e-9
                        && pl.crop.bottom() <= 1.0 + 1e-9,
                    "page {} crop escapes the photo: {:?}",
                    page.number,
                    pl.crop
                );
            }
        }
    }

    /// Every placement must sit in a slot of the layout its page NAMES, in
    /// slot order. This is what makes `template_id` reproducible from the
    /// manifest: a page that reports one template while carrying another's
    /// rects is unrenderable, and it is exactly what a template swap that
    /// forgets to rebuild its placements produces.
    #[test]
    fn pace_places_into_slots_of_the_layout_the_page_names() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        for page in &book.pages {
            if page.template_id == BLANK_TEMPLATE_ID {
                assert!(page.placements.is_empty(), "a blank page holds nothing");
                continue;
            }
            let layout = named_layout(&lib, page)
                .unwrap_or_else(|| panic!("page {} names an unknown layout", page.number));
            assert!(
                page.placements.len() <= layout.slots.len(),
                "page {} places {} photos into {} slots",
                page.number,
                page.placements.len(),
                layout.slots.len()
            );
            for (i, pl) in page.placements.iter().enumerate() {
                assert_eq!(
                    pl.slot_rect, layout.slots[i].rect,
                    "page {} placement {i} is not slot {i} of {}",
                    page.number, page.template_id
                );
            }
        }
    }

    /// A spread page must fill every slot its template declares; only single
    /// pages and blanks may come up short.
    #[test]
    fn pace_fills_every_slot_of_a_spread_template() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        for page in &book.pages {
            if page.template_id == BLANK_TEMPLATE_ID || page.template_id.contains(':') {
                continue;
            }
            let layout = named_layout(&lib, page).unwrap();
            assert_eq!(
                page.placements.len(),
                layout.slots.len(),
                "page {} leaves a slot of {} empty",
                page.number,
                page.template_id
            );
        }
    }

    #[test]
    fn pace_assigns_distinct_z_order_within_a_page() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        for page in &book.pages {
            let zs: BTreeSet<u32> = page.placements.iter().map(|p| p.z).collect();
            assert_eq!(zs.len(), page.placements.len(), "z values must be distinct");
        }
    }

    /// R2 again, at the other end: when a group is larger than any page half,
    /// requiring an exact match leaves the page blank; taking the largest half
    /// that fits places what it can and drops the rest.
    ///
    /// Driven through `single_page` DIRECTLY, and that change of level is the
    /// point rather than a convenience. `pack` used to size every group by
    /// SPREAD counts and hand the opening page a group no half could hold --
    /// which is the defect `Buildable` removed, by sizing a single at
    /// `1 ..= largest page half`. So the overflow can no longer arrive from
    /// the packer, and the first assertion below PINS that: if a future change
    /// lets `pack` over-fill a single again, this test says so instead of
    /// quietly going back to being an end-to-end test of a bug.
    ///
    /// `single_page` still owes the contract, because the half it actually
    /// chooses can be smaller than the largest one (every larger half may be
    /// rejected by a face or DPI constraint), so a group CAN still overflow
    /// the half it lands on. That is what is asserted here.
    #[test]
    fn pace_fills_a_single_page_from_a_group_larger_than_any_page_half() {
        let lib = fixture_library();
        let largest_half =
            lib.page_half_pool().iter().map(|p| p.slots.len()).max().unwrap();
        assert_eq!(largest_half, 2, "fixture: no half holds three photos");

        let photos = fixture_photos(36);
        let kept = cull(&photos, &Overrides::new());
        let groups = pack(
            &kept,
            &Capacity::from_library(20, &lib),
            &Buildable::from_library(&lib),
            &Overrides::new(),
        )
        .expect("no includes in the fixture");
        let singles: Vec<&Group> =
            groups.iter().filter(|g| g.slot == SlotKind::Single).collect();
        assert!(
            !singles.is_empty(),
            "fixture must produce at least one single-page group, or the pin below is vacuous"
        );
        for g in singles {
            assert!(
                g.photos.len() <= largest_half,
                "pack over-filled a single page again: {} photos into a half of {largest_half}",
                g.photos.len()
            );
        }

        // Three photos onto a half that holds at most two.
        let group = Group { photos: vec![0, 1, 2], event_cluster: 0, slot: SlotKind::Single };
        let page = single_page(Side::Right, Some(&group), &half_pool(&lib), &photos, 5);
        assert_ne!(page.template_id, BLANK_TEMPLATE_ID, "a blank page is the wrong answer here");
        assert_eq!(
            page.placements.len(),
            largest_half,
            "the page must hold what the largest half can, not nothing"
        );
    }

    /// A photo whose two faces sit far apart vertically. Every 2-slot half in
    /// the fixture library is ~2.4:1, so its crop window is too short to hold
    /// both and clips one -- a hard rejection. The 1-slot halves are ~1.26:1,
    /// keep full height, and accept the same photo.
    fn split_face_photo(i: usize) -> Photo {
        let face = |y: f64| Face {
            box_: Rect::new(0.42, y, 0.10, 0.12),
            capture_quality: Some(0.5),
        };
        Photo {
            path: format!("/photos/s{i:03}.jpg"),
            hash: format!("split-{i:03}"),
            width: 4032,
            height: 3024,
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 100 + i as u32,
            event_cluster: 0,
            faces: vec![face(0.15), face(0.72)],
            face_area_fraction: 0.024,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: Some(0.5),
            scene_tags: Vec::new(),
            captured_at: None,
        }
    }

    /// R2's remaining hole: committing to the largest half that FITS and then
    /// discovering every half at that size is rejected leaves the page blank,
    /// even though a smaller half in the same library would have been
    /// accepted. A blank page must be the last resort, not the first failure.
    #[test]
    fn pace_falls_back_to_a_smaller_half_when_the_largest_is_rejected() {
        let lib = fixture_library();
        let photos = vec![split_face_photo(0), split_face_photo(1)];
        let book = assemble(&photos, 20, &lib, &Weights::default(), 3, &Overrides::new()).expect("the fixture must place every included photo");

        // Precondition: the group is two photos and the largest right-hand
        // half holds exactly two, so the fixed-capacity version stops there.
        let largest_right = lib
            .page_half_pool()
            .iter()
            .filter(|l| l.side == Side::Right)
            .map(|l| l.slots.len())
            .max()
            .unwrap();
        assert_eq!(largest_right, 2, "fixture: the largest right half holds two");

        assert_eq!(
            book.pages[0].placements.len(),
            1,
            "every 2-slot half clips a face here, so the page must fall back to a 1-up"
        );
        assert_ne!(book.pages[0].template_id, BLANK_TEMPLATE_ID);
    }

    /// When a group overflows its page half, the photo left out must be the
    /// WEAKEST, by the same ranking `pack` uses when it trims to capacity --
    /// not whichever one happens to sit last in path order. Photo 1 is made
    /// the weakest of its group here precisely so that positional truncation
    /// (which would keep photos 1 and 2) and ranking (which keeps 2 and 3)
    /// disagree.
    ///
    /// Asserted on `strongest` DIRECTLY. `pack` now sizes a single page at
    /// `1 ..= largest page half`, so it no longer hands one a group that
    /// overflows every half and the trim cannot be reached end to end from
    /// the opening page any more. It is still reachable -- the half actually
    /// CHOSEN can be smaller than the largest, when a face or DPI constraint
    /// rejects the bigger ones -- so the ranking is a contract `strongest`
    /// still owes its caller, kept honest here rather than left as an arm no
    /// test enters.
    #[test]
    fn pace_drops_the_weakest_photo_when_a_group_overflows_its_page_half() {
        let mut photos = fixture_photos(36);
        assert_eq!(photos[2].aesthetic_pct, 74, "fixture: photo 2 is the strongest");
        assert_eq!(photos[3].aesthetic_pct, 11);
        photos[1].aesthetic_pct = 5; // now the weakest of the group

        let group = Group { photos: vec![1, 2, 3], event_cluster: 0, slot: SlotKind::Single };
        assert_eq!(
            strongest(&group, &photos, 2),
            vec![2, 3],
            "the weakest photo of the group must be the one dropped, not the last in path order"
        );
        assert_eq!(
            strongest(&group, &photos, 3),
            vec![1, 2, 3],
            "a group that fits must come through untouched"
        );
    }

    // --- the index remap --------------------------------------------------

    /// `photo_index` indexes the ORIGINAL slice, so it must never name a photo
    /// `cull` removed. An identity remap (or one built over the culled slice)
    /// lands on utility photos here, because every seventh fixture photo is
    /// one.
    #[test]
    fn pace_placement_indices_name_photos_that_survived_culling() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        assert!(!placed_indices(&book).is_empty());
        for i in placed_indices(&book) {
            assert!(i < photos.len(), "index {i} is out of range");
            assert!(!photos[i].is_utility, "photo {i} was culled but still placed");
        }
    }

    /// R3: a content hash is NOT unique within a run -- two byte-identical
    /// files in one scan both reach the sidecar with the same hash. Keying the
    /// remap on `hash` maps the second one onto the first one's index, and the
    /// manifest then names the wrong source file. Paths are unique.
    #[test]
    fn pace_remaps_indices_by_path_when_two_photos_share_a_hash() {
        let lib = fixture_library();
        let mut photos = fixture_photos(24);
        let duplicate = photos[1].hash.clone();
        for p in photos.iter_mut().filter(|p| !p.is_utility) {
            p.hash = duplicate.clone();
        }
        let book = assemble(&photos, 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("the fixture must place every included photo");
        let placed = placed_indices(&book);
        let unique: BTreeSet<usize> = placed.iter().copied().collect();
        assert_eq!(unique.len(), placed.len(), "a hash-keyed remap collapses onto one index");
        assert!(unique.len() > 1, "the book placed nothing to check");
        // The crop is computed from the photo the index names, so a wrong
        // index also produces a crop for the wrong aspect ratio.
        for page in &book.pages {
            if let Some(layout) = named_layout(&lib, page) {
                for (i, pl) in page.placements.iter().enumerate() {
                    let target = slot_aspect(&layout.slots[i]);
                    let expected = choose_crop(&photos[pl.photo_index], target);
                    assert_eq!(pl.crop, expected, "page {} crop is for another photo", page.number);
                }
            }
        }
    }

    // --- dropped accounting ----------------------------------------------

    /// Hand-derived, not merely positive. 30 fixture photos: `cull` drops the
    /// five utility ones (indices 0, 7, 14, 21, 28), leaving chapters of
    /// 3/4/6/8/4. With buildable sizes {1,2,3} the packer apportions the 11
    /// slots (9 spreads + 2 singles) as 1/2/3/3/2 -- every chapter at or above
    /// the `ceil(count / 3)` it needs to seat all its photos -- and cuts each
    /// to its own share: 3 | 3,1 | 3,1,2 | 2,3,3 | 3,1. That is 11 groups
    /// holding all 25 keepers, so the packer itself drops nothing.
    ///
    /// The opening single takes the first group of 3 but the largest page half
    /// holds 2, so one photo is dropped there; the nine middle groups place 21
    /// on the nine spreads; the closing single takes the last group of 1 and
    /// places it whole. Placed: 2 + 21 + 1 = 24. Dropped: the 5 utility photos
    /// plus the 1 that overflowed the opening page = 6.
    #[test]
    fn pace_accounts_for_every_photo_it_did_not_place() {
        let lib = fixture_library();
        let photos = fixture_photos(30);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 11, &Overrides::new()).expect("the fixture must place every included photo");
        let placed = placed_indices(&book);
        let unique: BTreeSet<usize> = placed.iter().copied().collect();
        assert_eq!(unique.len(), placed.len(), "a photo must not be placed twice");
        assert_eq!(placed.len(), 24, "placed count");
        assert_eq!(book.dropped, 6, "5 utility + 1 that overflowed the opening page");
        assert_eq!(book.dropped, photos.len() - unique.len(), "dropped must reconcile");

        // Every spread slot carries a group: no photo is stranded behind a
        // blank spread that an earlier slot took more than its share to create.
        assert!(
            book.pages.iter().all(|p| p.template_id != BLANK_TEMPLATE_ID),
            "25 keepers over 11 slots must leave no blank page"
        );
    }

    /// **Every keeper the book has room for is placed.** This is the invariant
    /// `pack`'s induction argument exists to establish, and it is asserted here
    /// on the REAL library because that is where it broke.
    ///
    /// Banning size 1 from the spread region made a chapter's remainder of one
    /// unbuildable at a spread slot. `pack` walks a chapter's slots in order and
    /// stops at the first it cannot fill, so such a remainder ended the chapter
    /// -- even when the book's closing single page, which holds one, was still
    /// free. Measured before the fix in `choose_group_size`: over this sweep,
    /// 471 of 474 keepers were placed, all three losses at 25 photos, where the
    /// last chapter of 8 took 2 then 3 then 2 and arrived at a spread slot
    /// holding one photo.
    ///
    /// Asserted per book rather than as a total, so a failure names the book.
    /// `pace_records_how_many_photos_were_dropped` covers the over-capacity
    /// case, where dropping is correct.
    ///
    /// # This sweep's range is 10..=51 keepers, and the invariant is FALSE above it
    ///
    /// The six counts cull to 10, 17, 21, 25, 34 and 51 keepers (158 per seed,
    /// 474 over the three). Do not read the green as "every keeper is always
    /// placed": swept independently at 40..=64 keepers on the same real library,
    /// this engine loses up to **6 of 64**, with zero blank pages throughout.
    /// The per-count table and both mechanisms are in `docs/PROJECT-STATUS.md`
    /// open item 14. Widening the range here would simply turn this test red;
    /// it is left at the range it can honestly assert, and the failure above it
    /// is tracked as an open item rather than hidden behind a narrow sweep
    /// nobody documented the edges of.
    #[test]
    fn pace_places_every_keeper_across_the_real_library_sweep() {
        let lib = real_library();
        for n in [12usize, 20, 25, 30, 40, 60] {
            for seed in [1u64, 7, 1234] {
                let photos = fixture_photos(n);
                let keepers = cull(&photos, &Overrides::new()).len();
                let book =
                    assemble(&photos, 20, &lib, &Weights::default(), seed, &Overrides::new())
                        .expect("the real library must place every included photo");
                let placed: usize = book.pages.iter().map(|p| p.placements.len()).sum();
                assert_eq!(
                    placed, keepers,
                    "{n} photos, seed {seed}: {keepers} keepers survived `cull` but only \
                     {placed} were placed -- {} photo(s) the book had room for are simply \
                     absent from it",
                    keepers - placed
                );
            }
        }
    }

    /// **I2: the headline outcome of the per-slot-kind change, asserted.**
    ///
    /// A page that names a REAL template and holds nothing prints white. That
    /// is the defect this whole change exists to remove, and until this test
    /// existed the property lived only in a re-baselineable golden -- so the
    /// next `UPDATE_GOLDEN=1` would have accepted its return in silence.
    ///
    /// A `BLANK_TEMPLATE_ID` check does NOT cover this and never did: the two
    /// blank pages this change removed were `03-hero-left-text-right`, a real
    /// 1-photo spread template whose other half is empty by construction. The
    /// assertion is therefore on the CONJUNCTION -- a real id AND no
    /// placements -- which is the only shape that catches it.
    ///
    /// Scoped to the fixture and frozen libraries. The real-library version
    /// lives in `pace_no_printed_page_names_a_real_template_and_holds_nothing_in_the_real_library`
    /// below: it could not pass until Task 4 rebalanced
    /// `20-four-up-windowpane` and `45-three-up-mosaic-hero-right`, the two
    /// MULTI-photo templates whose empty page half was an authoring choice.
    #[test]
    fn pace_no_printed_page_names_a_real_template_and_holds_nothing() {
        for (name, lib) in [("frozen", frozen_library()), ("fixture", fixture_library())] {
            for n in [12, 20, 25, 30, 40, 60] {
                for seed in [1, 7, 1234] {
                    let book =
                        assemble(&fixture_photos(n), 20, &lib, &Weights::default(), seed, &Overrides::new())
                            .expect("the fixture must place every included photo");
                    for (i, page) in book.pages.iter().enumerate() {
                        assert!(
                            page.template_id == BLANK_TEMPLATE_ID || !page.placements.is_empty(),
                            "{name} lib, {n} photos, seed {seed}: page {} names `{}` and holds \
                             nothing -- it prints white",
                            i + 1,
                            page.template_id
                        );
                    }
                }
            }
        }
    }

    /// The same property, against the library that actually ships.
    ///
    /// This is the guard for the whole point of Tasks 3 and 4, and until now
    /// it existed only against fixtures -- which is exactly the shape that
    /// lets a defect live on in the product while every test stays green. The
    /// fixture libraries hold no template with an empty page half at all, so
    /// the fixture-scoped version above cannot fail for that reason no matter
    /// what `templates/` contains.
    ///
    /// Two things have to hold for this to pass, and they are the two changes:
    /// `pack` must not cut a 1-photo group for a spread slot (Task 3 -- the
    /// eight 1-photo spread templates leave one page half empty by
    /// construction, so choosing one prints white), and no MULTI-photo
    /// template may leave a half empty (Task 4 -- `20-four-up-windowpane` and
    /// `45-three-up-mosaic-hero-right` did, by authoring choice). Reverting
    /// either change turns this red.
    ///
    /// Not `#[ignore]`d: the real-library tests in `templates.rs` are plain
    /// `#[test]`s, having been deliberately un-ignored because `bun run
    /// test:rust` passes no `--ignored` and there is no CI, so an ignored
    /// guard on the authored library runs nowhere at all.
    #[test]
    fn pace_no_printed_page_names_a_real_template_and_holds_nothing_in_the_real_library() {
        let lib = real_library();
        for n in [12, 20, 25, 30, 40, 60] {
            for seed in [1, 7, 1234] {
                let book =
                    assemble(&fixture_photos(n), 20, &lib, &Weights::default(), seed, &Overrides::new())
                        .expect("the real library must place every included photo");
                for (i, page) in book.pages.iter().enumerate() {
                    assert!(
                        page.template_id == BLANK_TEMPLATE_ID || !page.placements.is_empty(),
                        "real lib, {n} photos, seed {seed}: page {} names `{}` and holds \
                         nothing -- it prints white",
                        i + 1,
                        page.template_id
                    );
                }
            }
        }
    }

    /// **C1 at the assembly level.** A chapter of one photo cannot fill a
    /// spread, and `pack` walks slots in order, so before sub-spread chapters
    /// were merged an `Include` on such a photo failed the ENTIRE book with
    /// `IncludedNotPlaced` -- whose advice, "choose a longer book", cannot
    /// help, because a longer SKU adds spreads and never singles. The user was
    /// given an instruction that could not work.
    #[test]
    fn pace_assembles_a_book_whose_included_photo_is_alone_in_its_chapter() {
        let lib = fixture_library();
        let mut photos = fixture_photos(31);
        // 20 photos in cluster 0, one alone in cluster 1, 10 in cluster 2.
        for (i, p) in photos.iter_mut().enumerate() {
            p.is_utility = false;
            p.event_cluster = match i {
                0..=19 => 0,
                20 => 1,
                _ => 2,
            };
        }
        photos[20].path = "/photos/solo.jpg".into();
        let solo = photos[20].hash.clone();
        let overrides: Overrides = [(solo, Override::Include)].into_iter().collect();

        let book = assemble(&photos, 20, &lib, &Weights::default(), 5, &overrides)
            .expect("a lone included photo must not fail the whole book");

        assert!(
            placed_indices(&book).contains(&20),
            "the included photo alone in its chapter was not placed"
        );
    }

    /// If the packer converges every group on the same size then every spread
    /// has the same density, and `repace` cannot vary an axis the packer has
    /// already flattened -- however well `repace` is written. So the variation
    /// has to exist in the assembled book, not merely in `pack`'s own unit
    /// tests.
    ///
    /// Asserted on the `density` the library itself declares for each spread's
    /// chosen template, NOT on the raw photo counts. Counts are the wrong
    /// measure: in the real library 2, 3 and 4 are all reachable as `Medium`,
    /// so a book of 2s and 3s can have two distinct counts and exactly one
    /// density, and a count-based assertion passes while the axis is still
    /// flat.
    ///
    /// **Measured on a library built for this test, where density is a
    /// one-to-one function of slot count (2 -> Sparse, 3 -> Medium).** That
    /// bijection is asserted below rather than assumed,
    /// because the whole test rests on it: it is what makes "two densities
    /// appeared" equivalent to "two group sizes appeared", so the assertion
    /// cannot be satisfied by a book that converged on one size and merely
    /// picked two differently-labelled templates at it.
    ///
    /// It used to run on `frozen_library()`, and Task 3 had to `#[ignore]` it
    /// there. That library holds five templates -- one 1-photo `Sparse` and
    /// four `Medium` at sizes 2 and 3 -- so once `pack` stopped cutting
    /// 1-photo groups for spread slots, every buildable spread was `Medium`
    /// and the axis was flat by construction. Rebalancing `20-four-up-windowpane`
    /// and `45-three-up-mosaic-hero-right` could not fix that: both are
    /// `Medium`, and neither is in the frozen library, which is a fixture
    /// directory and not `templates/`. Measured on the alternative: copying a
    /// real `Sparse` 2-up (`43-two-up-bleed-left-inset-right`) into the frozen
    /// fixtures does make this pass, but it re-baselines
    /// `pace_golden_twenty_page_book`, and it would let the assertion be met
    /// by two size-2 spreads with different labels -- weaker than what is
    /// asserted here. The packer was never the problem: on the frozen library
    /// it already produced spreads of both size 2 and size 3.
    ///
    /// Filling every slot is the harder constraint and is asserted first, so
    /// this can never be satisfied by buying variety with a blank spread.
    ///
    /// **Mutation-checked.** Deleting the deliberate density swing in `pack`
    /// (`swing = 0`) turns this red with `densities {"Sparse"} from photo
    /// counts [2,2,2,2,2,2,2,2,2]`. It only does so because of the fixture
    /// below: on `fixture_photos(30)` the same mutation left the test GREEN,
    /// because 25 keepers in odd-sized chapters make a size-3 group
    /// arithmetically unavoidable, so the assertion held whatever the packer
    /// preferred. An assertion that cannot fail for the reason it names is
    /// worse than none, so the fixture was rebuilt until the mutation bit.
    #[test]
    fn pace_spread_density_varies_across_a_book_rather_than_converging() {
        let lib = density_by_size_library();

        let mut density_of_size: BTreeMap<usize, String> = BTreeMap::new();
        for t in &lib.spreads {
            let d = format!("{:?}", t.density);
            if let Some(prev) = density_of_size.insert(t.photo_count(), d.clone()) {
                assert_eq!(
                    prev, d,
                    "two size-{} templates declare different densities, so this library \
                     can no longer prove size variety from density variety",
                    t.photo_count()
                );
            }
        }
        let labels: BTreeSet<&String> = density_of_size.values().collect();
        assert_eq!(
            labels.len(),
            density_of_size.len(),
            "density must be one-to-one with slot count for this test to mean anything: \
             {density_of_size:?}"
        );

        // 22 keepers over 11 slots, in chapters of 8, 8 and 6 -- every one of
        // them EVEN. A book of nothing but 2s is therefore exactly buildable
        // and strands nobody, so the `strands` guard inside `choose_group_size`
        // has no reason to force a 3. That is the whole point of the fixture:
        // with `fixture_photos(30)` the keeper count is 25 and odd chapters
        // make a size-3 group arithmetically unavoidable, so this assertion
        // held no matter what the packer preferred -- measured, by deleting
        // the density swing in `pack` and by forcing `choose_group_size` to
        // take the smallest feasible size every time. Both left the test
        // green. On this fixture, deleting the swing turns it red.
        let mut photos = fixture_photos(26);
        let mut keeper = 0usize;
        for p in photos.iter_mut() {
            p.event_cluster = if p.is_utility {
                1
            } else {
                let c = match keeper {
                    0..=7 => 1,
                    8..=15 => 2,
                    _ => 3,
                };
                keeper += 1;
                c
            };
        }
        assert_eq!(keeper, 22, "fixture: 26 photos less the utility ones");

        let book = assemble(&photos, 20, &lib, &Weights::default(), 1234, &Overrides::new()).expect("the fixture must place every included photo");
        let spreads = (book.pages.len() - 2) / 2;
        let per_spread: Vec<usize> = (0..spreads)
            .map(|s| book.pages[1 + 2 * s].placements.len() + book.pages[2 + 2 * s].placements.len())
            .collect();

        assert!(
            book.pages.iter().all(|p| p.template_id != BLANK_TEMPLATE_ID),
            "variety must not cost a spread: {per_spread:?}"
        );

        let densities: BTreeSet<String> = (0..spreads)
            .filter_map(|s| spread_template(&book, &lib, s))
            .map(|t| format!("{:?}", t.density))
            .collect();
        assert!(
            densities.len() >= 2,
            "every spread has the same density, so `repace` has no density variation left \
             to work with: densities {densities:?} from photo counts {per_spread:?}"
        );
    }

    /// The over-capacity path: 500 photos cannot fit 9 spreads of at most 3
    /// plus 2 singles of at most 2.
    #[test]
    fn pace_records_how_many_photos_were_dropped() {
        let lib = fixture_library();
        let photos = fixture_photos(500);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 1, &Overrides::new()).expect("the fixture must place every included photo");
        let placed = placed_indices(&book);
        let unique: BTreeSet<usize> = placed.iter().copied().collect();
        assert_eq!(unique.len(), placed.len(), "a photo must not be placed twice");
        assert!(
            placed.len() <= 9 * 3 + 2 * 2,
            "placed {} exceeds what 20 pages can hold",
            placed.len()
        );
        assert_eq!(book.dropped, photos.len() - unique.len());
        assert!(book.dropped > 460, "a 500-photo set cannot fit 20 pages");
    }

    // --- pacing -----------------------------------------------------------

    /// `repace` must break a stretch that is flat on density, energy or edge
    /// treatment. The book is assembled against a ONE-template library, so
    /// every spread is identical on all three axes; repacing against the full
    /// library has to change something.
    #[test]
    fn pace_repace_breaks_a_run_of_three_identical_spreads() {
        let flat = library_from(&[("f2.json", F2_TWO_UP_MARGIN)]);
        let photos = fixture_photos(24);
        let w = Weights::default();
        let mut book = assemble(&photos, 20, &flat, &w, 9, &Overrides::new()).expect("the fixture must place every included photo");
        let before: Vec<String> = book.pages.iter().map(|p| p.template_id.clone()).collect();
        assert!(
            before.iter().filter(|id| *id == "f2-two-up-margin").count() >= 6,
            "fixture must start out flat: {before:?}"
        );

        repace(&mut book, &fixture_library(), &photos, &w, 9);
        let after: Vec<String> = book.pages.iter().map(|p| p.template_id.clone()).collect();
        assert_ne!(before, after, "repace left a fully flat book untouched");

        let lib = fixture_library();
        let spreads: Vec<Option<&SpreadTemplate>> = (0..(book.pages.len() - 2) / 2)
            .map(|s| {
                let id = &book.pages[1 + 2 * s].template_id;
                lib.spreads.iter().find(|t| &t.id == id)
            })
            .collect();
        for window in spreads.windows(3) {
            let [a, b, c] = [window[0], window[1], window[2]];
            let (Some(a), Some(b), Some(c)) = (a, b, c) else { continue };
            assert!(
                !(a.density == b.density && b.density == c.density)
                    || !(a.energy == b.energy && b.energy == c.energy),
                "three consecutive spreads are still flat on every authored axis"
            );
        }
    }

    /// The swap must rebuild the placements, not just relabel the page. A
    /// page whose `template_id` says one thing while its rects say another
    /// renders the wrong book, and `pace_places_into_slots_of_the_layout_the_page_names`
    /// only sees books that went through `assemble`.
    #[test]
    fn pace_repace_rebuilds_placements_for_the_template_it_swaps_in() {
        let flat = library_from(&[("f2.json", F2_TWO_UP_MARGIN)]);
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let w = Weights::default();
        let mut book = assemble(&photos, 20, &flat, &w, 9, &Overrides::new()).expect("the fixture must place every included photo");
        repace(&mut book, &lib, &photos, &w, 9);

        let mut swapped = 0;
        for page in &book.pages {
            if page.template_id == BLANK_TEMPLATE_ID {
                continue;
            }
            let layout = named_layout(&lib, page).expect("a named layout");
            for (i, pl) in page.placements.iter().enumerate() {
                assert_eq!(
                    pl.slot_rect, layout.slots[i].rect,
                    "page {} kept slot rects from a template it no longer names",
                    page.number
                );
            }
            if page.template_id != "f2-two-up-margin" && !page.template_id.contains(':') {
                swapped += 1;
            }
        }
        assert!(swapped >= 2, "expected at least one spread (two pages) to be swapped");
    }

    /// Both pages of a spread must name the same template: half a spread from
    /// one template and half from another is not a spread.
    #[test]
    fn pace_keeps_both_pages_of_a_spread_on_one_template() {
        let flat = library_from(&[("f2.json", F2_TWO_UP_MARGIN)]);
        let photos = fixture_photos(24);
        let w = Weights::default();
        let mut book = assemble(&photos, 20, &flat, &w, 9, &Overrides::new()).expect("the fixture must place every included photo");
        repace(&mut book, &fixture_library(), &photos, &w, 9);
        for s in 0..(book.pages.len() - 2) / 2 {
            let left = &book.pages[1 + 2 * s];
            let right = &book.pages[2 + 2 * s];
            assert_eq!(
                left.template_id, right.template_id,
                "spread {s} straddles two templates"
            );
        }
    }

    /// `best_spread` broke ties on template id alone, so the seed reached only
    /// the first and last pages and never a middle spread. "Regenerate this
    /// spread" is unimplementable until it does.
    ///
    /// The assertion is that SOME seed produces a different middle spread --
    /// not that a particular one does. A test pinning one seed to one template
    /// id would pass under an implementation that ignores the seed and simply
    /// happens to agree on that value.
    #[test]
    fn pace_uses_the_seed_to_break_a_tie_between_middle_spreads() {
        let lib = fixture_library();
        let photos = fixture_photos(30);
        let chosen: Vec<String> = [1u64, 9, 1234, 77, 5150]
            .into_iter()
            .map(|seed| {
                let book = assemble(&photos, 20, &lib, &Weights::default(), seed, &Overrides::new())
                    .expect("the fixture must place every included photo");
                // Page 1 is a single; the first MIDDLE spread is page index 1.
                book.pages[1].template_id.clone()
            })
            .collect();
        assert!(
            chosen.iter().collect::<std::collections::BTreeSet<_>>().len() > 1,
            "the seed never changed a middle spread: {chosen:?}"
        );
    }

    // --- golden -----------------------------------------------------------

    #[test]
    fn pace_golden_twenty_page_book() {
        let lib = frozen_library();
        let book = assemble(&fixture_photos(30), 20, &lib, &Weights::default(), 1234, &Overrides::new()).expect("the fixture must place every included photo");
        let actual = serde_json::to_string_pretty(&book).unwrap();
        let golden_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/book-20.json");
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            std::fs::write(&golden_path, &actual).unwrap();
        }
        let expected =
            std::fs::read_to_string(&golden_path).expect("run with UPDATE_GOLDEN=1 first");
        assert_eq!(actual, expected);
    }
}
