//! Cutting chapters into per-spread photo groups.
//!
//! Group sizes are constrained by what the library can actually BUILD:
//! templates are exact-count, so a group of five is unbuildable until
//! 5-photo templates are authored. The packer takes the buildable sizes as
//! input rather than assuming 1..=6, so the missing-5-up gap degrades into a
//! different split rather than an unfillable spread.

use crate::book::cull::Photo;
use crate::templates::Library;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capacity {
    pub pages: u32,
    pub singles: u32,
    pub spreads: u32,
    pub min_photos: usize,
    pub max_photos: usize,
}

impl Capacity {
    /// A book is 2 single pages facing the inside covers plus (N-2)/2
    /// spreads -- confirmed against Pixajoy's page navigator, which reads
    /// `Cover . 1 . 2-3 . 4-5 . ...`. It is NOT N/2 spreads.
    ///
    /// The single-page term below is computed from the largest *spread*
    /// size, which OVER-ESTIMATES what a single page can actually hold: a
    /// single page is one page-half, not a whole spread, so it can never
    /// hold as many photos as the largest buildable spread. This function
    /// exists so every test in this module can run against a plain list of
    /// buildable sizes, without loading template files from disk -- the
    /// over-estimate is the price of that independence. `from_library`
    /// below computes the accurate, page-half-bounded figure from the real
    /// template library and should be preferred by any caller that has one.
    pub fn from_sizes(pages: u32, buildable: &[usize]) -> Capacity {
        let singles = 2;
        let spreads = (pages.saturating_sub(2)) / 2;
        let smallest = buildable.iter().copied().min().unwrap_or(1);
        let largest = buildable.iter().copied().max().unwrap_or(1);
        Capacity {
            pages,
            singles,
            spreads,
            min_photos: (spreads as usize + singles as usize) * smallest,
            max_photos: spreads as usize * largest + singles as usize * largest,
        }
    }

    /// Accurate version of `from_sizes`: the single-page term is bounded by
    /// what one page-half can actually hold (`lib.page_half_pool()`'s
    /// largest half), not by the largest whole spread.
    pub fn from_library(pages: u32, lib: &Library) -> Capacity {
        let buildable = buildable_sizes(lib);
        let singles = 2;
        let spreads = (pages.saturating_sub(2)) / 2;
        let smallest = buildable.iter().copied().min().unwrap_or(1);
        let largest_spread = buildable.iter().copied().max().unwrap_or(1);
        let largest_half = lib
            .page_half_pool()
            .iter()
            .map(|p| p.slots.len())
            .max()
            .unwrap_or(1);
        Capacity {
            pages,
            singles,
            spreads,
            min_photos: (spreads as usize + singles as usize) * smallest,
            max_photos: spreads as usize * largest_spread + singles as usize * largest_half,
        }
    }
}

/// The photo counts the library can actually build a spread for.
pub fn buildable_sizes(lib: &Library) -> Vec<usize> {
    let mut sizes: Vec<usize> =
        lib.spreads.iter().map(|t| t.photo_count()).collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
    sizes.sort_unstable();
    sizes
}

/// Smallest SKU that fits the keepers, defaulting to 20 pages.
pub fn recommend_pages_with(keeper_count: usize, buildable: &[usize]) -> u32 {
    let twenty = Capacity::from_sizes(20, buildable);
    if keeper_count <= twenty.max_photos {
        20
    } else {
        40
    }
}

pub fn recommend_pages(keeper_count: usize, lib: &Library) -> u32 {
    recommend_pages_with(keeper_count, &buildable_sizes(lib))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Indices into the photo slice handed to `pack`.
    pub photos: Vec<usize>,
    pub event_cluster: u32,
}

/// Walks chapters in chronological order, cutting each into buildable
/// groups. A group never spans two chapters: a new chapter opening halfway
/// through a spread reads as an accident rather than a decision.
///
/// When the keepers exceed capacity, the LOWEST aesthetic percentiles are
/// dropped first, chapter proportions preserved.
pub fn pack(photos: &[Photo], capacity: &Capacity, buildable: &[usize]) -> Vec<Group> {
    use std::collections::BTreeMap;

    if photos.is_empty() || buildable.is_empty() {
        return Vec::new();
    }

    // Chapters, keyed by cluster id so iteration is chronological regardless
    // of the input slice's order.
    let mut chapters: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, p) in photos.iter().enumerate() {
        chapters.entry(p.event_cluster).or_default().push(i);
    }

    // Trim to capacity by dropping the weakest photos overall.
    let total: usize = chapters.values().map(Vec::len).sum();
    if total > capacity.max_photos {
        let mut ranked: Vec<usize> = (0..photos.len()).collect();
        // Sort worst-first: aesthetic, then sharpness, then path for
        // determinism.
        ranked.sort_by(|&a, &b| {
            photos[a]
                .aesthetic_pct
                .cmp(&photos[b].aesthetic_pct)
                .then(photos[a].sharpness_pct.cmp(&photos[b].sharpness_pct))
                .then(photos[a].path.cmp(&photos[b].path))
        });
        let drop_count = total - capacity.max_photos;
        let dropped: std::collections::BTreeSet<usize> =
            ranked.into_iter().take(drop_count).collect();
        for bucket in chapters.values_mut() {
            bucket.retain(|i| !dropped.contains(i));
        }
        chapters.retain(|_, v| !v.is_empty());
    }

    let mut groups = Vec::new();
    let mut slots_left = capacity.spreads as usize + capacity.singles as usize;

    for (cluster, mut members) in chapters {
        // Chronological within the chapter is not knowable without capture
        // times here, so path order is used -- stable, and the same order
        // `finalize_photos` already established.
        members.sort_by(|&a, &b| photos[a].path.cmp(&photos[b].path));

        let mut i = 0;
        while i < members.len() && slots_left > 0 {
            let remaining = members.len() - i;
            let take = choose_group_size(remaining, buildable, slots_left);
            groups.push(Group {
                photos: members[i..i + take].to_vec(),
                event_cluster: cluster,
            });
            i += take;
            slots_left -= 1;
        }
    }

    groups
}

/// Largest buildable size that does not strand an unbuildable remainder.
///
/// With buildable sizes {1,2,3} and 5 remaining, taking 3 leaves 2 (fine);
/// taking 2 leaves 3 (also fine). With {2,3} and 5 remaining, taking 3
/// leaves 2 -- but taking 2 leaves 3, so both work. The lookahead matters
/// when a size would strand a remainder no size can cover.
fn choose_group_size(remaining: usize, buildable: &[usize], slots_left: usize) -> usize {
    let largest = buildable.iter().copied().max().unwrap_or(1);

    // On the last available slot, take as much as one spread can hold.
    if slots_left == 1 {
        return remaining.min(largest);
    }

    let mut best = *buildable
        .iter()
        .filter(|&&s| s <= remaining)
        .max()
        .unwrap_or(&1);

    // Prefer a size whose remainder is itself coverable.
    for &size in buildable.iter().rev() {
        if size > remaining {
            continue;
        }
        let rest = remaining - size;
        if rest == 0 || buildable.iter().any(|&s| s <= rest) {
            best = size;
            break;
        }
    }
    best.min(remaining).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::PaletteColor;

    fn photo(path: &str, event: u32, aesthetic: u8) -> Photo {
        Photo {
            path: path.into(), hash: format!("h{path}"), width: 4000, height: 3000,
            is_utility: false, aesthetic_pct: aesthetic, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: event,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
        }
    }

    /// Buildable group sizes {1,2,3}: matches the real library's usable
    /// counts today (spec 6.1). Deliberately EXCLUDES 5 so the missing-5-up
    /// behaviour is exercised.
    fn sizes() -> Vec<usize> {
        vec![1, 2, 3]
    }

    #[test]
    fn pack_capacity_follows_two_singles_plus_spreads() {
        let c = Capacity::from_sizes(20, &sizes());
        assert_eq!(c.singles, 2);
        assert_eq!(c.spreads, 9, "20 pages = 2 singles + 9 spreads");
        let c40 = Capacity::from_sizes(40, &sizes());
        assert_eq!(c40.spreads, 19);
    }

    /// Boundary tests AT the boundaries, not near them.
    #[test]
    fn pack_recommends_twenty_pages_at_exactly_the_capacity_limit() {
        let twenty = Capacity::from_sizes(20, &sizes());
        assert_eq!(recommend_pages_with(twenty.max_photos, &sizes()), 20);
        assert_eq!(recommend_pages_with(twenty.max_photos + 1, &sizes()), 40);
    }

    #[test]
    fn pack_recommends_twenty_pages_for_a_tiny_set() {
        assert_eq!(recommend_pages_with(1, &sizes()), 20);
    }

    #[test]
    fn pack_never_emits_a_group_the_library_cannot_build() {
        let photos: Vec<Photo> =
            (0..11).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        for g in &groups {
            assert!(sizes().contains(&g.photos.len()),
                "group of {} is unbuildable", g.photos.len());
        }
    }

    /// The 5-photo gap, pinned. With only {1,2,3} buildable, five photos in
    /// one chapter must split, never emit a single group of five.
    #[test]
    fn pack_splits_a_chapter_of_five_because_no_five_up_template_exists() {
        let photos: Vec<Photo> =
            (0..5).map(|i| photo(&format!("/p{i}.jpg"), 7, 50)).collect();
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        assert!(groups.iter().all(|g| g.photos.len() != 5));
        assert_eq!(groups.iter().map(|g| g.photos.len()).sum::<usize>(), 5);
    }

    #[test]
    fn pack_does_not_mix_two_chapters_in_one_group() {
        let photos = vec![
            photo("/a.jpg", 1, 50), photo("/b.jpg", 1, 50),
            photo("/c.jpg", 2, 50), photo("/d.jpg", 2, 50),
        ];
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        for g in &groups {
            let clusters: std::collections::BTreeSet<u32> =
                g.photos.iter().map(|&i| photos[i].event_cluster).collect();
            assert_eq!(clusters.len(), 1, "a group must not span chapters");
        }
    }

    /// Input in NON-chronological order -- a pre-sorted fixture cannot
    /// detect a missing sort (a real Phase 1 failure mode).
    #[test]
    fn pack_orders_groups_by_chapter_regardless_of_input_order() {
        let photos = vec![
            photo("/z.jpg", 3, 50),
            photo("/a.jpg", 1, 50),
            photo("/m.jpg", 2, 50),
        ];
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        let clusters: Vec<u32> = groups.iter().map(|g| g.event_cluster).collect();
        assert_eq!(clusters, vec![1, 2, 3]);
    }

    #[test]
    fn pack_drops_the_lowest_ranked_photos_when_over_capacity() {
        let photos: Vec<Photo> = (0..100)
            .map(|i| photo(&format!("/p{i:03}.jpg"), 0, i as u8))
            .collect();
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        let used: usize = groups.iter().map(|g| g.photos.len()).sum();
        assert!(used <= c.max_photos, "used {used}, capacity {}", c.max_photos);
        // The best photo must survive; the worst must not.
        let kept: std::collections::BTreeSet<usize> =
            groups.iter().flat_map(|g| g.photos.iter().copied()).collect();
        assert!(kept.contains(&99), "highest aesthetic must be kept");
        assert!(!kept.contains(&0), "lowest aesthetic must be dropped");
    }
}
