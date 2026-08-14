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
///
/// Measured against `Capacity::from_sizes`, which OVER-estimates the two
/// single pages -- see its doc comment. Callers holding a real `Library`
/// should use `recommend_pages` below instead; this variant exists so the
/// recommendation is testable against a plain list of buildable sizes.
pub fn recommend_pages_with(keeper_count: usize, buildable: &[usize]) -> u32 {
    let twenty = Capacity::from_sizes(20, buildable);
    if keeper_count <= twenty.max_photos {
        20
    } else {
        40
    }
}

/// Smallest SKU that fits the keepers, measured with `Capacity::from_library`
/// -- the ACCURATE capacity, where the two single pages are bounded by what
/// one page-half can hold rather than by the largest whole spread.
///
/// Deliberately not `recommend_pages_with(keeper_count, &buildable_sizes(lib))`:
/// that measures against the over-estimate, so for a keeper count in the gap
/// between the two figures (65 or 66 against the real library, whose accurate
/// 20-page capacity is 64 and whose over-estimate is 66) it recommends a
/// 20-page book that then silently drops photos a 40-page book would have
/// kept. `pace::assemble` packs against `from_library`, so this is also the
/// only figure that agrees with the book the recommendation leads to.
pub fn recommend_pages(keeper_count: usize, lib: &Library) -> u32 {
    if keeper_count <= Capacity::from_library(20, lib).max_photos {
        20
    } else {
        40
    }
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
/// Groups are sized against how many photos and slots REMAIN, not against
/// the largest size that fits, so the photos are spread over the whole book
/// instead of packed into the opening spreads. A slot ends up blank only
/// when there are genuinely fewer photos than slots -- never because an
/// earlier slot took more than its share. `apportion_slots` and
/// `choose_group_size` below carry the reasoning.
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
    let slots = capacity.spreads as usize + capacity.singles as usize;

    // Slots are a BOOK-WIDE budget; groups are cut per chapter. Apportion the
    // budget across chapters up front rather than letting each chapter take
    // what it likes from a shared counter -- otherwise the first chapter
    // drains the book and the last ones never appear.
    let counts: Vec<usize> = chapters.values().map(Vec::len).collect();
    let (smallest, largest) = size_bounds(buildable);
    let allowance = apportion_slots(&counts, slots, smallest, largest);

    for ((cluster, mut members), mut slots_left) in chapters.into_iter().zip(allowance) {
        // Chronological within the chapter is not knowable without capture
        // times here, so path order is used -- stable, and the same order
        // `finalize_photos` already established.
        members.sort_by(|&a, &b| photos[a].path.cmp(&photos[b].path));

        let mut i = 0;
        while i < members.len() && slots_left > 0 {
            let remaining = members.len() - i;
            // The deliberate density swing. Aiming every slot at exactly the
            // fair share converges the whole book on one group size, and in
            // this library density is a strict function of slot count
            // (1 -> sparse, 2..4 -> medium, 6 -> dense) -- so a book of
            // identically sized groups has ONE density, and `repace` cannot
            // vary an axis the packer has flattened. Alternating one photo
            // either side of the share restores the variation `repace`
            // operates on. It is a preference, not a licence: the feasible
            // band inside `choose_group_size` overrides it whenever obeying
            // it would strand a photo or leave a later slot unfillable.
            //
            // The phase runs on the whole book's group count rather than the
            // chapter's, so the rhythm carries across a chapter boundary
            // instead of resetting at every one. Starting SPARSE puts the
            // smaller group on the opening single page, which holds only a
            // page-half's worth anyway.
            let swing = if groups.len() % 2 == 0 { -1 } else { 1 };
            match choose_group_size(remaining, buildable, slots_left, swing) {
                Some(take) => {
                    groups.push(Group {
                        photos: members[i..i + take].to_vec(),
                        event_cluster: cluster,
                    });
                    i += take;
                    slots_left -= 1;
                }
                // No buildable size fits what's left (or nothing left ever
                // could, given `buildable`). The honest outcome is to leave
                // the remainder of this chapter OUT of the output rather
                // than fabricate a group whose size the library cannot
                // build -- those photos are simply absent from every
                // `Group`, i.e. dropped. Move on to the next chapter.
                None => break,
            }
        }
    }

    groups
}

/// The smallest and largest group the library can build, both floored at 1
/// so a malformed `buildable` cannot produce a zero divisor or a zero-sized
/// group.
fn size_bounds(buildable: &[usize]) -> (usize, usize) {
    let usable = || buildable.iter().copied().filter(|&s| s > 0);
    (usable().min().unwrap_or(1), usable().max().unwrap_or(1))
}

/// How many spread slots each chapter may cut groups from, given the whole
/// book's budget. `counts` is per chapter in chronological (cluster id)
/// order; the return is the same length and order.
///
/// Each chapter has two figures that bound its share:
///
/// * **`need = count / largest`, rounded up** -- the fewest slots that can
///   hold all its photos. Below this the chapter silently drops photos, with
///   no blank spread anywhere to show for it. That is the same failure this
///   whole module exists to remove, so `need` is a FLOOR, not a preference.
/// * **`cap = count / smallest`** -- the most slots it could possibly fill.
///   Above this the surplus slots can only come out blank.
///
/// Three passes, all deterministic and all independent of the input slice's
/// ordering (they see only per-chapter counts, in cluster order):
///
/// 1. **One slot each, longest chapter first.** A chapter with no slot does
///    not appear in the book at all -- a group never spans chapters, so
///    there is no way for its photos to ride along in someone else's group.
///    Losing a chapter outright is a worse outcome than a crowded one, so
///    representation is bought before proportionality. When there are more
///    chapters than slots not every chapter can be represented; the longest
///    ones win, which is the same rule the over-capacity trim already uses
///    (keep the most, drop the least).
/// 2. **Every chapter up to its `need`**, before any chapter is given a slot
///    it merely *wants*. Whenever the budget can seat every photo, it does.
/// 3. **Highest averages (D'Hondt) for whatever is left over.** Each
///    remaining slot goes to whichever chapter would otherwise be the most
///    crowded -- the one with the largest `photos / (slots + 1)`. Repeated,
///    that greedily minimises the worst photos-per-spread figure in the
///    book, which is exactly "no chapter is starved".
///
/// Passes 2 and 3 are one loop: being short of `need` simply outranks every
/// D'Hondt average. When the budget cannot seat everyone (`sum(need) >
/// slots`) that ordering also decides who goes short -- the most crowded
/// chapters are served first.
///
/// Ties go to the earlier chapter, which is arbitrary but total; nothing
/// here may depend on iteration order.
fn apportion_slots(counts: &[usize], slots: usize, smallest: usize, largest: usize) -> Vec<usize> {
    let n = counts.len();
    let mut out = vec![0usize; n];
    if n == 0 || slots == 0 {
        return out;
    }
    let caps: Vec<usize> = counts.iter().map(|&c| c / smallest.max(1)).collect();
    let needs: Vec<usize> = counts.iter().map(|&c| c.div_ceil(largest.max(1))).collect();

    let mut longest_first: Vec<usize> = (0..n).collect();
    longest_first.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(a.cmp(&b)));
    let mut left = slots;
    for &i in &longest_first {
        if left == 0 {
            break;
        }
        if caps[i] > 0 {
            out[i] = 1;
            left -= 1;
        }
    }

    for _ in 0..left {
        // `counts[a] / (out[a] + 1)` compared without dividing, so the
        // comparison is exact rather than float-rounded. `max_by` yields the
        // LAST maximum, so the tie-break is inverted to leave the earliest
        // chapter as the strict maximum.
        let short = |i: usize| usize::from(out[i] < needs[i]);
        let pick = (0..n).filter(|&i| out[i] < caps[i]).max_by(|&a, &b| {
            short(a)
                .cmp(&short(b))
                .then((counts[a] * (out[b] + 1)).cmp(&(counts[b] * (out[a] + 1))))
                .then(b.cmp(&a))
        });
        match pick {
            Some(i) => out[i] += 1,
            None => break,
        }
    }
    out
}

/// The least and most this slot may take without stranding a photo or
/// leaving a later slot with nothing to hold.
///
/// * take fewer than `lo` and the `slots_left - 1` slots after it cannot
///   hold the rest even at `largest` each, so a photo is dropped;
/// * take more than `hi` and there are not enough photos left to give every
///   later slot even `smallest`, so a slot comes out blank.
///
/// Both saturate at zero, which is the honest answer when there are simply
/// fewer photos than slots -- there the band collapses towards the smallest
/// group and the leftover slots are blank because the photos ran out, not
/// because an earlier slot was greedy.
///
/// For a CONTIGUOUS `buildable` (which the real library is, 1..=6) the band
/// is never empty when `smallest * slots_left <= remaining <= largest *
/// slots_left`, and staying inside it is what makes "every slot filled and
/// every photo placed" hold by induction all the way down the chapter.
fn feasible_band(remaining: usize, buildable: &[usize], slots_left: usize) -> (usize, usize) {
    let (smallest, largest) = size_bounds(buildable);
    let later = slots_left.saturating_sub(1);
    let lo = remaining.saturating_sub(largest.saturating_mul(later));
    let hi = remaining.saturating_sub(smallest.saturating_mul(later));
    (lo.min(hi), hi)
}

/// A buildable size for `remaining`, aimed at this chapter's FAIR SHARE of
/// what is left -- `remaining / slots_left`, rounded half up -- rather than
/// at the largest size that fits. Returns `None` only when no buildable size
/// fits `remaining` at all; the caller must then leave the remainder
/// unplaced, never fabricate a size outside `buildable`.
///
/// Taking the largest was defensible while the library covered only 1, 2 and
/// 3 photos per spread, guarded by a lookahead asking whether the remainder
/// stayed decomposable. Once the library covered 1..=6 that guard became
/// inert -- 1 is buildable, so EVERY remainder is decomposable -- and 30
/// photos into 9 spreads packed as 6,6,6,6,6 with four spreads left blank.
/// Sizing against the remaining slots instead lands near 3 per spread and
/// fills all of them.
///
/// `swing` nudges the aim one photo either side of the share so the book's
/// group sizes -- and therefore its densities -- vary instead of converging
/// on a single value. It is only ever an aim: `feasible_band` overrides it.
///
/// Ordering of the candidates, in priority order:
///
/// 1. inside the feasible band, so neither a photo nor a slot is lost to a
///    stylistic preference -- this outranks everything below it;
/// 2. a remainder that is itself fully decomposable, checked exhaustively
///    (not a one-step lookahead) so a gapped buildable set like {3,5} cannot
///    slip an unreachable remainder past the check;
/// 3. nearest to the aim;
/// 4. the larger size, so an aim sitting exactly between two buildable sizes
///    rounds up rather than trailing photos into a later slot.
///
/// The last slot needs no special case: at `slots_left == 1` the band
/// collapses to `[remaining, remaining]`, so the slot takes everything left
/// that it can build.
fn choose_group_size(
    remaining: usize,
    buildable: &[usize],
    slots_left: usize,
    swing: i64,
) -> Option<usize> {
    if remaining == 0 || slots_left == 0 {
        return None;
    }

    // `(2r + s) / 2s` is `r / s` rounded half up, in integer arithmetic.
    let share = (2 * remaining + slots_left) / (2 * slots_left);
    let (lo, hi) = feasible_band(remaining, buildable, slots_left);
    // The aim is deliberately NOT clamped into the band. Clamping it would
    // enforce the band a second time, and a second enforcement of the same
    // rule is one no test can distinguish from the first -- measured: with
    // the clamp in place, deleting the band check below changes no test's
    // result at all. One guard, and it is the one below.
    let aim = (share as i64 + swing).max(0) as usize;

    // `s > 0` is not cosmetic: the caller advances by the size returned, so a
    // zero-size group would loop forever rather than fail. The validator
    // forbids an empty template, so this only ever fires on a malformed
    // `buildable`, and reporting the chapter unplaceable beats hanging.
    buildable.iter().copied().filter(|&s| s > 0 && s <= remaining).min_by_key(|&size| {
        let rest = remaining - size;
        let outside = usize::from(size < lo || size > hi);
        let strands = usize::from(rest != 0 && !is_decomposable(rest, buildable));
        (outside, strands, aim.abs_diff(size), std::cmp::Reverse(size))
    })
}

/// Whether `n` can be written as a sum of (repeated) values from
/// `buildable`. `0` is trivially decomposable (the empty sum).
fn is_decomposable(n: usize, buildable: &[usize]) -> bool {
    let mut reachable = vec![false; n + 1];
    reachable[0] = true;
    for i in 1..=n {
        reachable[i] = buildable.iter().any(|&s| s <= i && reachable[i - s]);
    }
    reachable[n]
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

    /// What the library actually covers now: every group size 1..=6.
    /// Distinct from `sizes()` because full coverage is precisely what broke
    /// the old greedy packer -- with 1 buildable, its decomposability guard
    /// was true for every remainder, so the largest size always won.
    fn full() -> Vec<usize> {
        vec![1, 2, 3, 4, 5, 6]
    }

    fn group_sizes(groups: &[Group]) -> Vec<usize> {
        groups.iter().map(|g| g.photos.len()).collect()
    }

    /// Groups keyed by content rather than by index into the input slice, so
    /// two runs over differently-ordered slices are comparable at all.
    fn by_path(photos: &[Photo], groups: &[Group]) -> Vec<(u32, Vec<String>)> {
        groups
            .iter()
            .map(|g| {
                (g.event_cluster, g.photos.iter().map(|&i| photos[i].path.clone()).collect())
            })
            .collect()
    }

    /// A zero in `buildable` is malformed input (the validator forbids an
    /// empty template), but `pack` advances by whatever size it is handed, so
    /// returning 0 would spin forever instead of failing. Asserted on
    /// `choose_group_size` directly rather than through `pack`, because the
    /// regression it guards is a HANG -- a test that drove `pack` would never
    /// report, it would just never finish.
    /// The remainder that no buildable size covers must come back as `None`,
    /// so `pack` leaves those photos out rather than inventing a group the
    /// library cannot build.
    ///
    /// Asserted on `choose_group_size` DIRECTLY, not through `pack`, and that
    /// is not a shortcut. The feasible band now stops a chapter from ever
    /// eating into the photos its later slots need, so within `pack` the
    /// chapter runs out of SLOTS before it can run out of buildable sizes and
    /// the `None` arm is no longer reachable from there -- it is a contract
    /// this function owes its caller, kept honest here rather than left as an
    /// arm no test enters.
    #[test]
    fn pack_reports_no_size_rather_than_one_the_library_cannot_build() {
        assert_eq!(choose_group_size(2, &[3, 5], 1, 0), None, "nothing in {{3,5}} covers 2");
        assert_eq!(choose_group_size(2, &[3, 5], 4, -1), None, "nor with slots still to fill");
        assert_eq!(choose_group_size(3, &[3, 5], 1, 0), Some(3), "3 is covered, and must be");
    }

    #[test]
    fn pack_never_chooses_a_zero_sized_group() {
        assert_eq!(choose_group_size(4, &[0], 2, 0), None, "0 is not a group size");
        assert_eq!(choose_group_size(4, &[0, 3], 2, 0), Some(3), "0 must not out-rank a real size");
    }

    /// Photos in chapters, packed, keyed so the caller can count them.
    fn pack_chapters(chapter_sizes: &[usize], buildable: &[usize]) -> (Vec<Photo>, Vec<Group>) {
        let mut photos = Vec::new();
        for (c, &n) in chapter_sizes.iter().enumerate() {
            for i in 0..n {
                photos.push(photo(&format!("/c{c}p{i:03}.jpg"), c as u32, 50));
            }
        }
        let c = Capacity::from_sizes(20, buildable);
        let groups = pack(&photos, &c, buildable);
        (photos, groups)
    }

    /// **No photo is dropped while any slot could still take it.**
    ///
    /// The slot budget is apportioned across chapters before any chapter is
    /// cut, so it is possible to hand one chapter more slots than it needs
    /// while another goes short -- and the shortfall vanishes with no blank
    /// spread anywhere to show for it, which is the same class of defect as
    /// front-loading arriving by a different route.
    ///
    /// The first row is the measured counterexample: chapters of 6/8/12/16/8
    /// over 11 slots with `buildable = 1..=6`. Apportioning on the upper
    /// bound alone gives chapter 4 four slots for 16 photos when it needs 3,
    /// and chapter 5 one slot for 8 photos when it needs 2 -- two photos
    /// disappear. Every row here fits inside capacity, so "what the slots
    /// could hold" is every photo.
    #[test]
    fn pack_places_every_photo_while_a_slot_could_still_take_it() {
        let full = full();
        for shape in [
            vec![6, 8, 12, 16, 8],
            vec![16, 8, 12, 6, 8],
            vec![30],
            vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
            vec![2, 30, 2],
            vec![25, 25],
            vec![6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6],
            vec![13, 5, 1, 9, 22],
        ] {
            let total: usize = shape.iter().sum();
            let (photos, groups) = pack_chapters(&shape, &full);
            let placed: usize = groups.iter().map(|g| g.photos.len()).sum();
            assert!(
                total <= Capacity::from_sizes(20, &full).max_photos,
                "fixture {shape:?} must fit inside capacity or the property does not apply"
            );
            assert_eq!(
                placed,
                photos.len(),
                "{shape:?}: dropped {} photo(s) with slots still available; sizes {:?}",
                photos.len() - placed,
                group_sizes(&groups)
            );
            assert!(
                groups.len() <= 11,
                "{shape:?}: {} groups for 11 slots",
                groups.len()
            );
        }
    }

    /// **Where the photo budget allows it, group sizes vary across a book
    /// rather than converging on one value.**
    ///
    /// Density is a strict function of slot count across this library
    /// (1 -> sparse, 2..4 -> medium, 6 -> dense), so a packer that aims every
    /// slot at exactly the fair share gives every spread the same density and
    /// `pace::repace` cannot vary an axis the packer has already flattened.
    ///
    /// `max - min >= 2` rather than "more than one distinct size": aiming at
    /// the bare share already yields two adjacent values (the floor and the
    /// ceiling of 30/11, i.e. 2 and 3), which is rounding, not variation, and
    /// does not cross a density boundary. A spread of 2 is what proves the
    /// swing is real.
    ///
    /// Filling every slot is the harder constraint and is asserted FIRST, so
    /// this test can never be satisfied by buying variety with a blank slot
    /// or a dropped photo.
    #[test]
    fn pack_varies_group_size_across_a_book_instead_of_converging_on_the_average() {
        let photos: Vec<Photo> =
            (0..30).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
        let c = Capacity::from_sizes(20, &full());
        let groups = pack(&photos, &c, &full());
        let s = group_sizes(&groups);

        assert_eq!(groups.len(), 11, "variety must not cost a slot: {s:?}");
        assert_eq!(s.iter().sum::<usize>(), 30, "variety must not cost a photo: {s:?}");

        let (min, max) = (*s.iter().min().unwrap(), *s.iter().max().unwrap());
        assert!(
            max - min >= 2,
            "group sizes converged on the average instead of varying: {s:?}"
        );
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

    /// `recommend_pages` must measure against the ACCURATE capacity
    /// (`from_library`), not the single-page over-estimate `from_sizes`
    /// returns -- otherwise it recommends a 20-page book for a keeper count
    /// that a 20-page book cannot actually hold, and the user is shown
    /// "0 dropped" for a book that drops photos.
    ///
    /// The fixture library's two figures differ (31 accurate vs 33
    /// over-estimated), so a keeper count between them distinguishes the two
    /// rules; both are asserted from the library itself rather than
    /// hardcoded, and the test asserts they genuinely differ first, so it
    /// cannot silently degenerate into a tautology if the fixture changes.
    #[test]
    fn pack_recommends_against_the_accurate_capacity_not_the_single_page_over_estimate() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
        let lib = Library::load(&dir).expect("the frozen fixture library must decompose");
        let accurate = Capacity::from_library(20, &lib).max_photos;
        let over_estimate = Capacity::from_sizes(20, &buildable_sizes(&lib)).max_photos;
        assert!(
            over_estimate > accurate,
            "fixture must distinguish the two capacity rules ({over_estimate} vs {accurate})"
        );

        assert_eq!(recommend_pages(accurate, &lib), 20, "exactly at the accurate capacity");
        assert_eq!(
            recommend_pages(accurate + 1, &lib),
            40,
            "one past the accurate capacity must move to the larger SKU, even though the \
             over-estimate would still claim it fits"
        );
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

    /// With {1,2,3}, 1 is always buildable, so this fixture alone cannot
    /// distinguish a correct implementation from one that emits nothing but
    /// singles -- it only catches a size leaking OUTSIDE the set entirely
    /// (e.g. 5). A GAPPED buildable set is needed to reach genuine
    /// stranding: with {3,5} and a chapter of 7, taking 5 leaves 2 (not
    /// buildable) and taking 3 leaves 4 (not buildable either) -- there is
    /// no way to place all 7 photos without emitting an invalid size, so
    /// the only correct behaviour is to place what a real template covers
    /// and leave the rest OUT of the output entirely, never to fabricate a
    /// group of, say, 1 or 2 that no template can build.
    ///
    /// The mutation it catches is a packer that computes a size
    /// ARITHMETICALLY and emits it rather than choosing one the library
    /// actually offers: the last slot's fair share here is 4, and 4 is not in
    /// {3,5}. `pack_reports_no_size_rather_than_one_the_library_cannot_build`
    /// above covers the other half of the same contract -- the `None` return
    /// -- which `pack` itself can no longer reach.
    #[test]
    fn pack_leaves_a_remainder_unplaced_rather_than_fabricate_an_unbuildable_size() {
        let gapped = vec![3, 5];
        let photos: Vec<Photo> =
            (0..7).map(|i| photo(&format!("/p{i}.jpg"), 0, 50)).collect();
        let c = Capacity::from_sizes(20, &gapped);
        let groups = pack(&photos, &c, &gapped);

        for g in &groups {
            assert!(gapped.contains(&g.photos.len()),
                "group of {} is not in the buildable set {:?}", g.photos.len(), gapped);
        }

        let placed: usize = groups.iter().map(|g| g.photos.len()).sum();
        assert!(placed < photos.len(),
            "7 photos cannot be fully covered by {{3,5}} without an invalid \
             size, so some must be left unplaced (placed {placed})");
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

    /// THE measured regression, pinned exactly: 30 photos into a 20-page book
    /// -- 9 spread slots plus 2 single pages -- with every group size 1..=6
    /// buildable. The greedy packer took the largest fitting size every time
    /// (`is_decomposable` is trivially true for every remainder once 1 is
    /// buildable), emitting 6,6,6,6,6 and leaving four spreads blank. There
    /// are more photos than slots here, so a blank slot cannot be excused by
    /// "genuinely fewer photos than slots": every slot must carry a group.
    #[test]
    fn pack_fills_every_slot_rather_than_front_loading_the_first_spreads() {
        let photos: Vec<Photo> =
            (0..30).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
        let c = Capacity::from_sizes(20, &full());
        assert_eq!((c.spreads, c.singles), (9, 2), "the measured case: 9 spreads + 2 singles");
        let slots = c.spreads as usize + c.singles as usize;
        assert!(photos.len() > slots, "fixture: more photos than slots, so no slot may be blank");

        let groups = pack(&photos, &c, &full());
        let s = group_sizes(&groups);
        assert_eq!(groups.len(), slots, "{} slots but only {} groups: {s:?}", slots, groups.len());
        assert_eq!(s.iter().sum::<usize>(), 30, "every photo must be placed: {s:?}");
        // 30 photos over 11 slots is 2.7 each. The deliberate density swing
        // is one photo either side of that, so 4 is the ceiling -- and no
        // slot may run at the library's maximum, which is what the greedy
        // packer did with all five of its groups.
        assert!(
            s.iter().all(|&n| n <= 4),
            "a slot took more than its share of 30/11 plus the swing: {s:?}"
        );
    }

    /// Slots are a book-wide budget but groups are cut per chapter, so a
    /// packer that hands each chapter whatever is left when it reaches it
    /// starves the last chapters entirely. Here chapter 1 alone can absorb
    /// every slot at the largest buildable size (60 = 10 x 6), so the greedy
    /// packer leaves chapter 3 with no slot at all and its photos never
    /// appear in the book -- with no blank spread to show for it.
    #[test]
    fn pack_gives_every_chapter_a_slot_rather_than_letting_the_first_take_them_all() {
        let mut photos: Vec<Photo> =
            (0..60).map(|i| photo(&format!("/a{i:02}.jpg"), 1, 50)).collect();
        photos.extend((0..3).map(|i| photo(&format!("/b{i}.jpg"), 2, 50)));
        photos.extend((0..3).map(|i| photo(&format!("/c{i}.jpg"), 3, 50)));
        let c = Capacity::from_sizes(20, &full());
        assert_eq!(photos.len(), c.max_photos, "fixture: exactly at capacity, so nothing is trimmed");

        let groups = pack(&photos, &c, &full());
        let chapters: std::collections::BTreeSet<u32> =
            groups.iter().map(|g| g.event_cluster).collect();
        assert_eq!(
            chapters,
            [1, 2, 3].into_iter().collect(),
            "a chapter vanished from the book: sizes {:?}",
            group_sizes(&groups)
        );
    }

    /// Two chapters of five with buildable {2,3}. Correct packing cuts each
    /// chapter into 3+2; a packer that ignores chapter boundaries and cuts
    /// the flat run of ten into 2,2,2,2,2 puts photos 4 and 5 -- one from
    /// each chapter -- in the same group. Sizes 2 and 3 are chosen so no
    /// flat cut point can coincide with the chapter boundary at 5.
    #[test]
    fn pack_does_not_mix_two_chapters_in_one_group() {
        let mut photos: Vec<Photo> =
            (0..5).map(|i| photo(&format!("/a{i}.jpg"), 1, 50)).collect();
        photos.extend((0..5).map(|i| photo(&format!("/b{i}.jpg"), 2, 50)));
        let buildable = vec![2, 3];
        let c = Capacity::from_sizes(20, &buildable);
        let groups = pack(&photos, &c, &buildable);

        assert_eq!(
            group_sizes(&groups).iter().sum::<usize>(),
            photos.len(),
            "fixture: all ten photos must be placed or the mixing has nowhere to show"
        );
        for g in &groups {
            let clusters: std::collections::BTreeSet<u32> =
                g.photos.iter().map(|&i| photos[i].event_cluster).collect();
            assert_eq!(clusters.len(), 1, "a group must not span chapters");
        }
    }

    /// Determinism against the ORDERING of the input, not just against a
    /// repeated call: chapters come from a `BTreeMap` keyed on cluster id and
    /// members are sorted by path, so reversing the slice must produce the
    /// same book. Compared by PATH, since the indices legitimately differ.
    #[test]
    fn pack_produces_the_same_groups_when_the_input_slice_is_permuted() {
        let photos: Vec<Photo> = (0..30)
            .map(|i| photo(&format!("/p{i:02}.jpg"), (i % 4) as u32, (i * 7 % 100) as u8))
            .collect();
        let c = Capacity::from_sizes(20, &full());
        let forward = pack(&photos, &c, &full());

        let reversed: Vec<Photo> = photos.iter().rev().cloned().collect();
        let backward = pack(&reversed, &c, &full());

        assert!(!forward.is_empty());
        assert_eq!(
            by_path(&photos, &forward),
            by_path(&reversed, &backward),
            "permuting the input changed the book"
        );
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
