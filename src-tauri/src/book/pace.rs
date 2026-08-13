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
use crate::book::cull::{cull, Photo};
use crate::book::pack::{buildable_sizes, pack, Capacity, Group};
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
fn tie_break(seed: u64, n: usize) -> usize {
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
///   The largest half that fits is taken and the remainder is dropped.
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
    let capacity = pool.iter().filter(|(_, l)| fits(l)).map(|(_, l)| l.slots.len()).max()?;
    let indices: Vec<usize> = group.photos[..capacity].to_vec();

    let mut scored: Vec<(f64, &str, &PageLayout)> = pool
        .iter()
        .filter(|(_, l)| fits(l) && l.slots.len() == capacity)
        .filter_map(|(id, l)| single_fit(l, &indices, photos).map(|fit| (fit, *id, *l)))
        .collect();
    if scored.is_empty() {
        return None;
    }
    let best = scored.iter().fold(f64::NEG_INFINITY, |m, (fit, _, _)| m.max(*fit));
    // Exact equality is the definition of the tie the seed exists to break;
    // anything looser would let the seed override a real preference.
    scored.retain(|(fit, _, _)| *fit == best);
    let (_, id, layout) = scored[tie_break(seed, scored.len())];
    Some((id, layout, indices))
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
) -> Option<(&'a SpreadTemplate, [Page; 2])> {
    let refs: Vec<&Photo> = group.photos.iter().map(|&i| &photos[i]).collect();
    let eligible = lib.spreads_with(group.photos.len());
    let (template, assignment, _) = best_spread(&eligible, &refs, previous, w)?;
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

/// Full pipeline: cull -> size -> pack -> score -> place -> pace.
///
/// Always emits exactly `pages` pages. A group that cannot be laid out yields
/// a page with zero placements rather than a shorter book: the SKU fixes the
/// page count, and a 19-page book cannot be uploaded against a 20-page
/// product.
pub fn assemble(photos: &[Photo], pages: u32, lib: &Library, w: &Weights, seed: u64) -> Book {
    let kept = cull(photos);
    let sizes = buildable_sizes(lib);
    // `from_library`, not `from_sizes`: the latter over-estimates what the two
    // single pages hold by measuring them against the largest whole SPREAD.
    let cap = Capacity::from_library(pages, lib);
    let groups = pack(&kept, &cap, &sizes);

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
        })
        .collect();

    let pool = half_pool(lib);
    let spread_count = (pages.saturating_sub(2) / 2) as usize;
    let mut out: Vec<Page> = Vec::with_capacity(pages as usize);

    // Page 1: a single, facing the inside front cover.
    out.push(single_page(Side::Right, groups.first(), &pool, photos, seed));

    // The middle: true spreads, two pages each. The first and last groups are
    // spoken for by the single pages.
    let middle: Vec<&Group> =
        groups.iter().skip(1).take(groups.len().saturating_sub(2)).collect();
    let mut previous: Option<String> = None;
    for s in 0..spread_count {
        match middle.get(s).and_then(|g| spread_pages(g, photos, lib, w, previous.as_deref())) {
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

    // The final page: a single, facing the inside back cover. Only when there
    // is a group the opening page did not already take.
    let last = if groups.len() > 1 { groups.last() } else { None };
    out.push(single_page(Side::Left, last, &pool, photos, seed));

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
    repace(&mut book, lib, photos, w);

    // Counted from what actually survived into the book, after repacing, so
    // no accounting can drift out of step with the pages.
    let placed: BTreeSet<usize> = book
        .pages
        .iter()
        .flat_map(|p| p.placements.iter().map(|pl| pl.photo_index))
        .collect();
    book.dropped = photos.len().saturating_sub(placed.len());
    book
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
pub fn repace(book: &mut Book, lib: &Library, photos: &[Photo], w: &Weights) {
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
        let Some((template, assignment, _)) = best_spread(&candidates, &refs, Some(previous), w)
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

    // --- structure -------------------------------------------------------

    #[test]
    fn pace_assembles_the_correct_page_count_for_twenty_pages() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 42);
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
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 42);
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
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 42);
        for page in &book.pages {
            let expected = if page.number % 2 == 1 { Side::Right } else { Side::Left };
            assert_eq!(page.side, expected, "page {} sits on the wrong side", page.number);
        }
    }

    /// R2: the SKU fixes the page count. Every layout here is rejected on
    /// resolution (300px photos in ~9" slots is ~30 DPI against a 150 floor),
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
        let book = assemble(&photos, 20, &lib, &Weights::default(), 42);
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
    /// One chapter here is unprintable (300px photos, ~30 DPI against a 150
    /// floor); every other chapter lays out. 36 fixture photos fill all 11
    /// slots exactly, so the only blanks in a correct book are that chapter's.
    #[test]
    fn pace_leaves_a_failed_spread_blank_in_place_rather_than_shifting_the_book() {
        let lib = fixture_library();
        let mut photos = fixture_photos(36);
        for p in photos.iter_mut().filter(|p| p.event_cluster == 3) {
            p.width = 300;
            p.height = 225;
        }
        let book = assemble(&photos, 20, &lib, &Weights::default(), 5);
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
        let intact = assemble(&fixture_photos(36), 20, &lib, &Weights::default(), 5);
        assert!(
            intact.pages.iter().all(|p| p.template_id != BLANK_TEMPLATE_ID),
            "fixture check: 36 photos fill all 11 slots, so only the failure blanks"
        );
    }

    // --- determinism -----------------------------------------------------

    #[test]
    fn pace_is_deterministic_for_the_same_seed() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let a = assemble(&photos, 20, &lib, &Weights::default(), 7);
        let b = assemble(&photos, 20, &lib, &Weights::default(), 7);
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
                assemble(&photos, 20, &lib, &Weights::default(), seed).pages[0].template_id.clone()
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
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
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
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
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
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
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
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
        for page in &book.pages {
            let zs: BTreeSet<u32> = page.placements.iter().map(|p| p.z).collect();
            assert_eq!(zs.len(), page.placements.len(), "z values must be distinct");
        }
    }

    /// R2 again, at the other end: `pack` sizes every group by SPREAD counts,
    /// so the group handed to a single page is routinely larger than any page
    /// half. Requiring an exact match leaves the opening page blank; taking
    /// the largest half that fits places what it can and drops the rest.
    #[test]
    fn pace_fills_a_single_page_from_a_group_larger_than_any_page_half() {
        let lib = fixture_library();
        let largest_half =
            lib.page_half_pool().iter().map(|p| p.slots.len()).max().unwrap();
        assert_eq!(largest_half, 2, "fixture: no half holds three photos");
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
        // The opening group holds three photos (chapter 1 is p001..p003).
        assert_eq!(
            book.pages[0].placements.len(),
            2,
            "the opening page must hold what the largest half can, not nothing"
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
        let book = assemble(&photos, 20, &lib, &Weights::default(), 5);
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
        let book = assemble(&photos, 20, &lib, &Weights::default(), 5);
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
    /// 3/4/6/8/4. With buildable sizes {1,2,3} and 11 slots (9 spreads + 2
    /// singles) the packer cuts them into 10 groups of 3,3,1,3,3,3,3,2,3,1.
    /// The opening single takes the first group of 3 but the largest page half
    /// holds 2, so one photo there is dropped; the remaining 8 middle groups
    /// fill 8 of the 9 spreads and the last group of 1 fills the closing page.
    /// Placed: 2 + 21 + 1 = 24. Dropped: 5 utility + 1 = 6.
    #[test]
    fn pace_accounts_for_every_photo_it_did_not_place() {
        let lib = fixture_library();
        let photos = fixture_photos(30);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 11);
        let placed = placed_indices(&book);
        let unique: BTreeSet<usize> = placed.iter().copied().collect();
        assert_eq!(unique.len(), placed.len(), "a photo must not be placed twice");
        assert_eq!(placed.len(), 24, "placed count");
        assert_eq!(book.dropped, 6, "5 utility + 1 that overflowed the opening page");
        assert_eq!(book.dropped, photos.len() - unique.len(), "dropped must reconcile");
    }

    /// The over-capacity path: 500 photos cannot fit 9 spreads of at most 3
    /// plus 2 singles of at most 2.
    #[test]
    fn pace_records_how_many_photos_were_dropped() {
        let lib = fixture_library();
        let photos = fixture_photos(500);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 1);
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
        let mut book = assemble(&photos, 20, &flat, &w, 9);
        let before: Vec<String> = book.pages.iter().map(|p| p.template_id.clone()).collect();
        assert!(
            before.iter().filter(|id| *id == "f2-two-up-margin").count() >= 6,
            "fixture must start out flat: {before:?}"
        );

        repace(&mut book, &fixture_library(), &photos, &w);
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
        let mut book = assemble(&photos, 20, &flat, &w, 9);
        repace(&mut book, &lib, &photos, &w);

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
        let mut book = assemble(&photos, 20, &flat, &w, 9);
        repace(&mut book, &fixture_library(), &photos, &w);
        for s in 0..(book.pages.len() - 2) / 2 {
            let left = &book.pages[1 + 2 * s];
            let right = &book.pages[2 + 2 * s];
            assert_eq!(
                left.template_id, right.template_id,
                "spread {s} straddles two templates"
            );
        }
    }

    // --- golden -----------------------------------------------------------

    #[test]
    fn pace_golden_twenty_page_book() {
        let lib = frozen_library();
        let book = assemble(&fixture_photos(30), 20, &lib, &Weights::default(), 1234);
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
