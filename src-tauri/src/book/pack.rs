//! Cutting chapters into per-spread photo groups.
//!
//! Group sizes are constrained by what the library can actually BUILD:
//! templates are exact-count, so a group of five is unbuildable until
//! 5-photo templates are authored. The packer takes the buildable sizes as
//! input rather than assuming 1..=6, so the missing-5-up gap degrades into a
//! different split rather than an unfillable spread.

use crate::book::cull::{Override, Overrides, Photo};
use crate::templates::Library;

/// The user asked for more photos than the book they chose can hold.
///
/// Reported rather than resolved, and that is the whole point of the type.
/// Every other over-capacity case in this module is resolved by dropping the
/// weakest photos, because nobody asked for those in particular. An explicit
/// `Include` is different: silently discarding one is exactly the failure
/// user-controlled selection exists to prevent, so the engine refuses to
/// build the book and hands the numbers back for the user to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeOverflow {
    /// Page count of the book that cannot hold them, so the message can name
    /// the length the user actually chose.
    pub pages: u32,
    /// How many photos the user explicitly asked for.
    pub included: usize,
    /// How many photos a book of this length can hold at all.
    pub capacity: usize,
    /// `included - capacity`: how many they must let go of, or lengthen the
    /// book to fit.
    pub over: usize,
}

impl std::fmt::Display for IncludeOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "You marked {} photos to include, but a {}-page book holds {}. \
             Exclude {} of them, or choose a longer book.",
            self.included, self.pages, self.capacity, self.over
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capacity {
    pub pages: u32,
    pub singles: u32,
    pub spreads: u32,
    pub max_photos: usize,
}

impl Capacity {
    /// A book is 2 single pages facing the inside covers plus (N-2)/2
    /// spreads -- confirmed against Pixajoy's page navigator, which reads
    /// `Cover . 1 . 2-3 . 4-5 . ...`. It is NOT N/2 spreads.
    ///
    /// The two kinds of slot are measured against their OWN buildable sets --
    /// a spread against the library's spread counts less 1 (a 1-photo spread
    /// template prints a blank half), a single against `1 ..= largest_half`.
    /// `largest_half` is a parameter rather than read from a `Library` so
    /// every test in this module can run against plain lists, without loading
    /// template files from disk; `from_library` below takes both figures from
    /// the real library and should be preferred by any caller that has one.
    pub fn from_sizes(pages: u32, spread_sizes: &[usize], largest_half: usize) -> Capacity {
        Capacity::from_buildable(pages, &Buildable::from_sizes(spread_sizes, largest_half))
    }

    /// The accurate figure: both sets come from the loaded library.
    pub fn from_library(pages: u32, lib: &Library) -> Capacity {
        Capacity::from_buildable(pages, &Buildable::from_library(lib))
    }

    /// A book is 2 single pages plus (N-2)/2 spreads, each kind bounded by
    /// its own set. Written once so `from_sizes` and `from_library` cannot
    /// drift apart -- they differ only in where the `Buildable` came from.
    ///
    /// Public so a caller holding a `Buildable` it built itself -- the tests
    /// in this module do -- can measure the capacity of THAT set rather than
    /// of one reconstructed from a plain list, which is how the two used to
    /// disagree.
    pub fn from_buildable(pages: u32, b: &Buildable) -> Capacity {
        let singles = 2u32;
        let spreads = (pages.saturating_sub(2)) / 2;
        let (_, single_hi) = bounds_of(&b.single);
        let (_, spread_hi) = bounds_of(&b.spread);
        Capacity {
            pages,
            singles,
            spreads,
            max_photos: spreads as usize * spread_hi + singles as usize * single_hi,
        }
    }
}

/// Smallest and largest usable size in one set, both floored at 1 so a
/// malformed set cannot produce a zero divisor or a zero-sized group.
fn bounds_of(sizes: &[usize]) -> (usize, usize) {
    let usable = || sizes.iter().copied().filter(|&s| s > 0);
    (usable().min().unwrap_or(1), usable().max().unwrap_or(1))
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
/// Measured against `Capacity::from_sizes` with the largest page half taken
/// to be the largest buildable SPREAD, which OVER-estimates the two single
/// pages: a single page is one page-half and can never hold a whole spread's
/// worth. Callers holding a real `Library` should use `recommend_pages` below
/// instead; this variant exists so the recommendation is testable against a
/// plain list of buildable sizes, and the over-estimate is the price of that
/// independence.
pub fn recommend_pages_with(keeper_count: usize, buildable: &[usize]) -> u32 {
    let largest_half = buildable.iter().copied().max().unwrap_or(1);
    let twenty = Capacity::from_sizes(20, buildable, largest_half);
    if keeper_count <= twenty.max_photos {
        20
    } else {
        40
    }
}

/// Smallest SKU that fits the keepers, measured with `Capacity::from_library`
/// -- the MORE accurate capacity, where the two single pages are bounded by
/// what one page-half can hold rather than by the largest whole spread.
///
/// Deliberately not `recommend_pages_with(keeper_count, &buildable_sizes(lib))`:
/// that measures against the over-estimate, so for a keeper count in the gap
/// between the two figures (65 or 66 against the real library, whose
/// `from_library` 20-page capacity is 64 and whose over-estimate is 66) it
/// recommends a 20-page book that then silently drops photos a 40-page book
/// would have kept. `pace::assemble` packs against `from_library`, so this is
/// also the only figure that agrees with the book the recommendation leads to.
///
/// **"More accurate" is not "accurate."** That 64 is itself an overclaim by one:
/// the true 20-page ceiling is 63, because `Buildable::from_library`'s `single`
/// set is side-blind. See the warning block on that function and
/// `docs/PROJECT-STATUS.md` open item 14 — including why the obvious fix was
/// measured and rejected.
pub fn recommend_pages(keeper_count: usize, lib: &Library) -> u32 {
    if keeper_count <= Capacity::from_library(20, lib).max_photos {
        20
    } else {
        40
    }
}

/// Which kind of slot a group was cut to fill.
///
/// A book is 2 single PAGES facing the inside covers plus (N-2)/2 spreads,
/// and the two kinds do not hold the same number of photos: a single page is
/// one page-half, a spread is two. `pack` therefore has to size them
/// differently, and `pace::assemble` has to place them accordingly -- which
/// it cannot do from position alone, because when photos run out `pack`
/// produces fewer groups than there are slots and the last group it cut is
/// then NOT the closing single.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// The first or last page of the book: one page-half.
    Single,
    /// Any of the (N-2)/2 spreads between them: two page-halves.
    Spread,
}

/// Which kind of slot sits at `index`, in a book of `slots` slots.
///
/// Slot 0 and slot `slots - 1` are the two single pages facing the inside
/// covers; everything between them is a spread. Extracted rather than
/// written inline because more than one caller needs the answer, and two
/// copies of the rule deciding which slot is a single is the same
/// duplicated-authority defect `book::cull` already had to consolidate.
fn slot_kind_at(index: usize, slots: usize) -> SlotKind {
    if index == 0 || index + 1 == slots {
        SlotKind::Single
    } else {
        SlotKind::Spread
    }
}

/// The fewest photos a SPREAD slot may be cut to.
///
/// A spread is two page halves and the validator forbids a slot from spanning
/// the fold, so a spread laid out from one photo has that photo on one half
/// and nothing on the other -- a page that prints white. Two is the fewest
/// that can put something on both halves.
const SPREAD_MIN_PHOTOS: usize = 2;

/// The group sizes each kind of slot may be cut to.
///
/// One book-wide set is wrong in both directions. A SPREAD cannot take 1:
/// a 1-photo template has a single slot, the validator forbids a slot from
/// spanning the fold, so one of its halves is empty and prints white --
/// structurally, for every such template, with no authoring fix. A SINGLE
/// cannot take a whole spread's worth: it is one page-half, so sizing it
/// against the largest spread over-fills it and `pace::strongest` trims the
/// surplus away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buildable {
    /// Sizes for the first and last page: `1 ..= largest page half`.
    pub single: Vec<usize>,
    /// Sizes for the (N-2)/2 spreads: the library's spread counts, less 1.
    pub spread: Vec<usize>,
}

impl Buildable {
    /// Derived from the loaded library, never hardcoded, and BOTH sets are
    /// derived: `single` from the slot counts the page halves actually have,
    /// `spread` from the spread templates' photo counts less the sub-minimum
    /// ones. Neither is a contiguous range invented from a maximum -- a range
    /// claims the library can build every count below the largest, which is a
    /// claim only the library itself can make.
    ///
    /// # KNOWN DEFECT: `single` is SIDE-BLIND, and a fix was measured and rejected
    ///
    /// `page_half_pool()` pools BOTH sides, so `single` is the union of what
    /// left halves and right halves can build. But a page half is authored for
    /// the side it prints on -- bleed edges and the gutter are side-specific --
    /// so `pace::best_single` filters the pool by `l.side == side`, and
    /// `pace::assemble` places the opening single on the RIGHT and the closing
    /// single on the LEFT. The two slots therefore do NOT have the same set.
    ///
    /// On the real library: LEFT halves offer `{1,2,3,4}`, RIGHT `{1,2,3,4,5}`,
    /// so the union is `{1,2,3,4,5}`. Two consequences, both measured:
    ///
    /// * `pack` may cut a `(5, Single)` CLOSING group, which no left half can
    ///   build. `best_single` falls back to a 4-slot left half and
    ///   `pace::strongest` silently trims one photo.
    /// * `Capacity::from_buildable` reads `single_hi = 5` off this set, so
    ///   `Capacity::from_library(20, lib).max_photos` reports **64** when the
    ///   true ceiling is **63** (9 spreads x 6, plus a right-hand 5 opening and
    ///   a left-hand 4 closing). `commands::…dropped_photos` is
    ///   `keeper_count.saturating_sub(max_photos)`, so at 64 keepers the UI
    ///   reports 0 dropped for a book that actually drops 6.
    ///
    /// **Replacing the union with the INTERSECTION (`{1,2,3,4}`) was tried and
    /// measured NET NEGATIVE. Do not re-attempt it.** The intersection is not
    /// conservative, because the union is not uniformly an overclaim: 5 is a
    /// size the closing slot cannot take but the OPENING slot genuinely can.
    /// Measured on the real library, 20 pages, `pace::assemble`:
    ///
    /// * page 1 really does lay out five photos on
    ///   `25-six-up-mosaic-hero-five:right`. Under the intersection it drops to
    ///   four, losing a photo the library could print.
    /// * `max_photos` falls to 62 -- an UNDERclaim, since the true ceiling is
    ///   63 -- so `pack`'s capacity trim pre-emptively discards keepers at 63
    ///   and 64 that reached the page before.
    /// * across four synthetic chapter shapes x three seeds at 60..=64 keepers,
    ///   three of the four shapes lose 1-2 photos per book and none gains one.
    ///
    /// **The correct fix is a per-SIDE single set** -- `single_left` and
    /// `single_right`, each slot measured against the side it actually prints
    /// on, with `Capacity` summing `right_hi + left_hi` rather than
    /// `2 * single_hi`. That is a fourth change to `pack`'s contract and is
    /// deliberately deferred, not forgotten; see `docs/PROJECT-STATUS.md`
    /// known-open item 14, which carries the full 40..=64-keeper sweep.
    pub fn from_library(lib: &Library) -> Buildable {
        let halves: std::collections::BTreeSet<usize> =
            lib.page_half_pool().iter().map(|p| p.slots.len()).filter(|&n| n > 0).collect();
        Buildable {
            single: halves.into_iter().collect(),
            spread: buildable_sizes(lib)
                .into_iter()
                .filter(|&s| s >= SPREAD_MIN_PHOTOS)
                .collect(),
        }
    }

    /// As `from_library`, but from a plain list of spread sizes plus the
    /// largest page half -- so the callers that have no `Library` (chiefly
    /// `recommend_pages_with`) can still get a figure.
    ///
    /// This one DOES invent `1 ..= largest_half` for the singles, and that is
    /// its documented weakness: it presumes a page half exists at every count
    /// up to the largest, which only the library can confirm. Anything holding
    /// a real `Library` must use `from_library` above.
    pub fn from_sizes(spread_sizes: &[usize], largest_half: usize) -> Buildable {
        Buildable {
            single: (1..=largest_half.max(1)).collect(),
            spread: spread_sizes.iter().copied().filter(|&s| s >= SPREAD_MIN_PHOTOS).collect(),
        }
    }

    pub fn for_kind(&self, kind: SlotKind) -> &[usize] {
        match kind {
            SlotKind::Single => &self.single,
            SlotKind::Spread => &self.spread,
        }
    }

    /// Bounds over BOTH sets, for the two places that need a book-wide
    /// figure rather than a per-slot one: `apportion_slots`, which divides
    /// slot COUNTS across chapters, and `Capacity`. Both use these as
    /// bounds, not as exact sizes, so the union is the honest answer.
    pub fn union_bounds(&self) -> (usize, usize) {
        let all = || self.single.iter().chain(self.spread.iter()).copied().filter(|&s| s > 0);
        (all().min().unwrap_or(1), all().max().unwrap_or(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Indices into the photo slice handed to `pack`.
    pub photos: Vec<usize>,
    /// The chapter this group was cut from -- but NOT reliably the cluster its
    /// photos were originally assigned. `merge_sub_spread_chapters` folds a
    /// chapter too small to fill a spread into the next one chronologically and
    /// re-keys the folded photos onto the LATER cluster id, so after a merge
    /// this names the chapter that absorbed them, not the one they came from.
    ///
    /// Inert today: the only non-test reader is the pass-through in
    /// `pace::assemble`. It stops being inert the moment Phase 3 derives a
    /// chapter TITLE or date range from it, which would then silently inherit
    /// the absorbing chapter's identity. Read the merge before relying on this.
    pub event_cluster: u32,
    /// The kind of slot this group was SIZED for. Set by `pack`; read by
    /// `pace::assemble` to decide where it goes.
    pub slot: SlotKind,
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
///
/// **A photo the user marked `Include` is never one of the dropped.** It is
/// protected from the capacity trim, its chapter is apportioned a slot ahead
/// of a chapter with nothing explicitly wanted in it, and a crowded chapter
/// strands an `Auto` photo rather than an explicit choice. When the `Include`
/// photos ALONE exceed what the book can hold there is no honest way to
/// choose between them, so this returns `Err(IncludeOverflow)` rather than
/// picking -- see that type.
pub fn pack(
    photos: &[Photo],
    capacity: &Capacity,
    buildable: &Buildable,
    overrides: &Overrides,
) -> Result<Vec<Group>, IncludeOverflow> {
    use std::collections::BTreeMap;

    if photos.is_empty() || (buildable.single.is_empty() && buildable.spread.is_empty()) {
        return Ok(Vec::new());
    }

    let wanted = |i: usize| overrides.get(&photos[i].hash) == Override::Include;

    // Checked before anything is cut. Every trim below protects the included
    // photos, so a book that cannot hold them all would otherwise fail by
    // dropping something else entirely -- or by running out of slots and
    // leaving the surplus unplaced with nothing to show for it.
    let included = (0..photos.len()).filter(|&i| wanted(i)).count();
    if included > capacity.max_photos {
        return Err(IncludeOverflow {
            pages: capacity.pages,
            included,
            capacity: capacity.max_photos,
            over: included - capacity.max_photos,
        });
    }

    // Chapters, keyed by cluster id so iteration is chronological regardless
    // of the input slice's order.
    let mut chapters: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, p) in photos.iter().enumerate() {
        chapters.entry(p.event_cluster).or_default().push(i);
    }

    // Worst-first: aesthetic, then sharpness, then path for determinism.
    // Shared by the book-wide trim and the per-chapter one below so both
    // discard by the same standard.
    let weakest_first = |a: &usize, b: &usize| {
        photos[*a]
            .aesthetic_pct
            .cmp(&photos[*b].aesthetic_pct)
            .then(photos[*a].sharpness_pct.cmp(&photos[*b].sharpness_pct))
            .then(photos[*a].path.cmp(&photos[*b].path))
    };

    // Trim to capacity by dropping the weakest photos overall.
    let total: usize = chapters.values().map(Vec::len).sum();
    if total > capacity.max_photos {
        // Only `Auto` photos are candidates. The check above guarantees there
        // are enough of them to get under capacity.
        let mut ranked: Vec<usize> = (0..photos.len()).filter(|&i| !wanted(i)).collect();
        ranked.sort_by(weakest_first);
        let drop_count = total - capacity.max_photos;
        let dropped: std::collections::BTreeSet<usize> =
            ranked.into_iter().take(drop_count).collect();
        for bucket in chapters.values_mut() {
            bucket.retain(|i| !dropped.contains(i));
        }
        chapters.retain(|_, v| !v.is_empty());
    }

    let chapters = merge_sub_spread_chapters(chapters, spread_minimum(buildable));

    let mut groups = Vec::new();
    let slots = capacity.spreads as usize + capacity.singles as usize;

    // Slots are a BOOK-WIDE budget; groups are cut per chapter. Apportion the
    // budget across chapters up front rather than letting each chapter take
    // what it likes from a shared counter -- otherwise the first chapter
    // drains the book and the last ones never appear.
    let counts: Vec<usize> = chapters.values().map(Vec::len).collect();
    let wants: Vec<usize> =
        chapters.values().map(|b| b.iter().filter(|&&i| wanted(i)).count()).collect();
    // The union bounds, deliberately: `apportion_slots` divides slot COUNTS
    // across chapters and uses these only as "fewest slots that hold all its
    // photos" and "most slots it could fill". A chapter CAN legitimately fill
    // a slot with one photo -- the two single pages take one -- so narrowing
    // the divisor to the spread minimum under-allots and strands the tail of a
    // chapter that would have fitted. Handing out a slot a chapter cannot fill
    // is instead caught where it belongs, in the feasible band, which knows
    // which kinds those slots actually are.
    let (smallest, largest) = buildable.union_bounds();
    let allowance = apportion_slots(&counts, &wants, slots, smallest, largest);

    for ((cluster, mut members), mut slots_left) in chapters.into_iter().zip(allowance) {
        // Chronological within the chapter is not knowable without capture
        // times here, so path order is used -- stable, and the same order
        // `finalize_photos` already established.
        members.sort_by(|&a, &b| photos[a].path.cmp(&photos[b].path));

        // A chapter apportioned fewer slots than its photos need strands
        // whatever the loop below does not reach, which is its TAIL in path
        // order -- and that tail is chosen by filename, so an explicit choice
        // is as likely to be in it as anything else. Where the chapter holds
        // one, the surplus is taken off the WEAKEST `Auto` members instead.
        //
        // Gated on the chapter actually holding an include, deliberately.
        // Unconditionally re-choosing which photo a starved chapter strands
        // would change books that have no overrides in them at all, and this
        // feature has no business doing that.
        let seatable = slots_left.saturating_mul(largest);
        if members.len() > seatable && members.iter().any(|&i| wanted(i)) {
            let mut droppable: Vec<usize> =
                members.iter().copied().filter(|&i| !wanted(i)).collect();
            droppable.sort_by(weakest_first);
            let dropped: std::collections::BTreeSet<usize> =
                droppable.into_iter().take(members.len() - seatable).collect();
            members.retain(|i| !dropped.contains(i));
        }

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
            // `groups.len()` is the GLOBAL slot index -- it counts across
            // chapters, not within one -- which is exactly what the swing
            // above already relies on. Slot 0 and slot `slots - 1` are the
            // two single pages; everything between them is a spread.
            let index = groups.len();
            let kind = slot_kind_at(index, slots);
            let look = lookahead(index, slots_left, slots, buildable);
            match choose_group_size(remaining, buildable.for_kind(kind), &look, slots_left, swing)
            {
                Some(take) => {
                    groups.push(Group {
                        photos: members[i..i + take].to_vec(),
                        event_cluster: cluster,
                        slot: kind,
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

    Ok(groups)
}

/// The fewest photos any spread in this library can be built from, floored at
/// 1 so an empty or malformed spread set cannot make every chapter "too
/// small" and collapse the book into one chapter.
fn spread_minimum(buildable: &Buildable) -> usize {
    buildable.spread.iter().copied().filter(|&s| s > 0).min().unwrap_or(1)
}

/// Folds every chapter holding fewer photos than a spread can be built from
/// into the next chapter chronologically (the last such run joins the final
/// chapter, there being no next one).
///
/// **Why this does not violate "a group never spans two chapters".** That rule
/// exists so a chapter boundary never falls in the MIDDLE of a spread, where
/// it reads as an accident rather than a decision. A chapter too small to
/// occupy a spread at all has no interior boundary to protect: there is no
/// arrangement in which it gets a spread of its own, so the only question is
/// whether its photos appear in the book beside their chronological
/// neighbours, or not at all.
///
/// Without this, they did not appear at all, and the failure was worse than a
/// silent drop in three compounding ways:
///
/// * the chapter was apportioned a slot it could never fill, so the slot was
///   spent and the book lost a spread as well as the photos;
/// * `pack` walks slots in order and a chapter that places nothing does not
///   advance the global slot index, so EVERY later small chapter faced the
///   same spread slot and failed identically -- twelve one-photo chapters into
///   eleven slots placed one photo in total;
/// * an `Include` on such a photo failed the whole book with
///   `IncludedNotPlaced`, whose advice ("choose a longer book") cannot help,
///   because a longer SKU adds spreads and never singles.
///
/// Merging removes the cause of all three rather than papering over any of
/// them. Chapters are visited in cluster order, which is chronological, and
/// the fold is forward, so a stray photo joins the chapter it precedes.
fn merge_sub_spread_chapters(
    chapters: std::collections::BTreeMap<u32, Vec<usize>>,
    spread_min: usize,
) -> std::collections::BTreeMap<u32, Vec<usize>> {
    if spread_min <= 1 {
        return chapters;
    }
    let first = match chapters.keys().next() {
        Some(&k) => k,
        None => return chapters,
    };
    let mut out: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    let mut carry: Vec<usize> = Vec::new();
    for (cluster, mut members) in chapters {
        members.append(&mut carry);
        if members.len() < spread_min {
            carry = members;
        } else {
            out.insert(cluster, members);
        }
    }
    if !carry.is_empty() {
        // Nothing followed the final run. It joins the last chapter that DID
        // reach the minimum; if no chapter did, the whole book is one chapter,
        // which the two single pages can still open and close.
        match out.iter_mut().next_back() {
            Some((_, last)) => last.append(&mut carry),
            None => {
                out.insert(first, carry);
            }
        }
    }
    out
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
///
/// `wants` is how many photos in each chapter the user explicitly marked
/// `Include`. It outranks every other consideration in both passes: a chapter
/// with no slot does not appear in the book at all, so a chapter holding an
/// explicit choice is represented before a longer one holding none, and it is
/// brought up to the slots THOSE photos need before any chapter is given a
/// slot for its automatic ones. With no overrides `wants` is all zeros and
/// both rules are inert, which is what keeps books with no decisions in them
/// packed exactly as before.
fn apportion_slots(
    counts: &[usize],
    wants: &[usize],
    slots: usize,
    smallest: usize,
    largest: usize,
) -> Vec<usize> {
    let n = counts.len();
    let mut out = vec![0usize; n];
    if n == 0 || slots == 0 {
        return out;
    }
    let caps: Vec<usize> = counts.iter().map(|&c| c / smallest.max(1)).collect();
    let needs: Vec<usize> = counts.iter().map(|&c| c.div_ceil(largest.max(1))).collect();
    let want_needs: Vec<usize> = wants.iter().map(|&c| c.div_ceil(largest.max(1))).collect();

    let mut longest_first: Vec<usize> = (0..n).collect();
    longest_first.sort_by(|&a, &b| {
        (wants[b] > 0).cmp(&(wants[a] > 0)).then(counts[b].cmp(&counts[a])).then(a.cmp(&b))
    });
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
        let short_wanted = |i: usize| usize::from(out[i] < want_needs[i]);
        let short = |i: usize| usize::from(out[i] < needs[i]);
        let pick = (0..n).filter(|&i| out[i] < caps[i]).max_by(|&a, &b| {
            short_wanted(a)
                .cmp(&short_wanted(b))
                .then(short(a).cmp(&short(b)))
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

/// What the slots AFTER this one can hold, and what they can be built from.
///
/// `least` and `most` are SUMS over the following slots of each one's own
/// smallest and largest buildable size -- not one slot's bounds multiplied by
/// the count. The distinction is load-bearing once the two kinds of slot have
/// different sets: a chapter ending `spread, spread, closing single` has
/// following capacities of 2..=3, 2..=3 and 1..=2, whose true total is 5..=8.
/// Taking the minimum (1) and maximum (3) across all of them and multiplying
/// gives 3..=9, which is wide enough to let a slot take two photos when it
/// had to take three -- and the chapter then arrives at the closing single
/// holding more than one page half can hold, and strands a photo. Measured:
/// that is exactly how an `Include` alone in its chapter was still being
/// dropped after the chapters were merged.
struct Lookahead {
    /// Every size any following slot can be built from, deduplicated. What the
    /// REMAINDER has to decompose into.
    sizes: Vec<usize>,
    /// Fewest photos the following slots can hold between them.
    least: usize,
    /// Most photos the following slots can hold between them.
    most: usize,
}

/// The `Lookahead` over the slots after `index`, given the book has `slots` of
/// them in total and this chapter holds `slots_left` including the current
/// one. Which kind sits where is `slot_kind_at`'s answer, not re-derived here:
/// one definition of "slot 0 and slot `slots - 1` are singles" is the whole
/// reason that function exists.
///
/// Empty when nothing follows, which is the honest answer: at the last slot
/// the band collapses onto `remaining`, and a non-zero remainder IS stranded.
fn lookahead(index: usize, slots_left: usize, slots: usize, buildable: &Buildable) -> Lookahead {
    let later = slots_left.saturating_sub(1);
    let mut sizes: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let (mut least, mut most) = (0usize, 0usize);
    for i in (index + 1)..=(index + later) {
        let set = buildable.for_kind(slot_kind_at(i, slots));
        sizes.extend(set.iter().copied().filter(|&s| s > 0));
        let (lo, hi) = bounds_of(set);
        least += lo;
        most += hi;
    }
    Lookahead { sizes: sizes.into_iter().collect(), least, most }
}

/// The least and most this slot may take without stranding a photo or
/// leaving a later slot with nothing to hold.
///
/// * take fewer than `lo` and the slots after it cannot hold the rest even at
///   their largest, so a photo is dropped;
/// * take more than `hi` and there are not enough photos left to give every
///   later slot even its smallest, so a slot comes out blank.
///
/// Both saturate at zero, which is the honest answer when there are simply
/// fewer photos than slots -- there the band collapses towards the smallest
/// group and the leftover slots are blank because the photos ran out, not
/// because an earlier slot was greedy.
///
/// The bounds come from the slots that FOLLOW rather than from this slot's own
/// set, because the slots that follow may be a different kind: a chapter's
/// last slot can be the book's closing single, whose set starts at 1, while
/// every slot before it is a spread whose set starts at 2. Using this slot's
/// own bounds for the lookahead mis-states the band at exactly the position
/// where it matters.
fn feasible_band(remaining: usize, look: &Lookahead) -> (usize, usize) {
    let lo = remaining.saturating_sub(look.most);
    let hi = remaining.saturating_sub(look.least);
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
/// 2. a remainder that is itself fully decomposable BY THE SLOTS THAT FOLLOW,
///    checked exhaustively (not a one-step lookahead) so a gapped buildable
///    set like {3,5} cannot slip an unreachable remainder past the check;
/// 3. `overshoot` -- HOW FAR outside the band, ranked only among candidates
///    that are all outside it already. Inert unless the band has collapsed
///    (a chapter apportioned more slots than its photos can fill), which is
///    the starved case it exists for; the inline comment at the sort key
///    below carries the measurement and explains why it is ordered after
///    `strands` rather than before;
/// 4. nearest to the aim;
/// 5. the larger size, so an aim sitting exactly between two buildable sizes
///    rounds up rather than trailing photos into a later slot.
///
/// The last slot needs no special case: at `slots_left == 1` the band
/// collapses to `[remaining, remaining]`, so the slot takes everything left
/// that it can build.
fn choose_group_size(
    remaining: usize,
    own_buildable: &[usize],
    look: &Lookahead,
    slots_left: usize,
    swing: i64,
) -> Option<usize> {
    if remaining == 0 || slots_left == 0 {
        return None;
    }

    // `(2r + s) / 2s` is `r / s` rounded half up, in integer arithmetic.
    let share = (2 * remaining + slots_left) / (2 * slots_left);
    // Both the band and the stranding guard below are questions about the
    // slots that FOLLOW this one -- they are what has to hold the remainder --
    // so both read `look` rather than this slot's own set.
    let (lo, hi) = feasible_band(remaining, look);
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
    own_buildable.iter().copied().filter(|&s| s > 0 && s <= remaining).min_by_key(|&size| {
        let rest = remaining - size;
        let outside = usize::from(size < lo || size > hi);
        let strands = usize::from(rest != 0 && !is_decomposable(rest, &look.sizes));
        // HOW FAR outside, ranked only among candidates that are all outside
        // already -- `outside` above has separated those from the ones inside,
        // and a candidate inside the band scores 0 here too, so this term is
        // inert except in the starved case it exists for.
        //
        // That case is a chapter apportioned more slots than its photos can
        // fill. The band then collapses to `hi == 0` ("take nothing"), no
        // buildable size satisfies it, and every candidate ties on `outside`.
        // Before this term the tie fell through to `aim`, which is
        // `share + swing` -- a TASTE preference -- and the swing could push
        // the choice UP at the exact moment photos were scarcest. Measured, on
        // the real library with 21 keepers in chapters of 3, 4, 6 and 8: the
        // last chapter held 8 with five slots to fill, took 2 then 3 then 2,
        // and arrived at a spread slot holding one photo. No spread can be
        // built from one -- that is the whole point of the size-1 ban -- so
        // the chapter broke off there and the photo was never placed, even
        // though the book's closing single page could have held it. Preferring
        // the candidate nearest the band takes 2 four times instead and seats
        // all eight.
        //
        // Ordered AFTER `strands`, deliberately. Ranking it before turns the
        // stranding guard off whenever the band has collapsed: measured, with
        // this term first, a chapter of 3 facing three spread slots takes 2
        // and strands the third photo, where `strands` had correctly made it
        // take 3.
        let overshoot = size.saturating_sub(hi).max(lo.saturating_sub(size));
        (outside, strands, overshoot, aim.abs_diff(size), std::cmp::Reverse(size))
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
            scene_tags: Vec::new(), captured_at: None,
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

    /// A `Buildable` from one plain list of sizes a fixture library declares
    /// it can build.
    ///
    /// Both sets are drawn from that list and NOTHING is invented: the singles
    /// are the declared sizes as they stand, the spreads are the ones that can
    /// fill both halves. Deliberately not `Buildable::from_sizes`, which fills
    /// the singles with `1 ..= largest` -- against a declared `{2,3}` that
    /// yields a single set containing 1, and `pack` was measured emitting a
    /// group of one photo under a library declared to build only 2s and 3s.
    /// A fixture must not be able to build a size its own declaration denies.
    fn buildable_for(sizes: &[usize]) -> Buildable {
        Buildable {
            single: sizes.iter().copied().filter(|&s| s > 0).collect(),
            spread: sizes.iter().copied().filter(|&s| s >= SPREAD_MIN_PHOTOS).collect(),
        }
    }

    /// The capacity of exactly the set `buildable_for` yields, so the two can
    /// never disagree about what the fixture library can build.
    fn capacity_for(pages: u32, sizes: &[usize]) -> Capacity {
        Capacity::from_buildable(pages, &buildable_for(sizes))
    }

    /// A `Lookahead` over `n` following slots that all draw from `sizes`.
    /// `later(&[], 0)` is "nothing follows", where the band collapses onto
    /// `remaining` and any remainder is stranded.
    fn later(sizes: &[usize], n: usize) -> Lookahead {
        let (lo, hi) = bounds_of(sizes);
        let usable: Vec<usize> = sizes.iter().copied().filter(|&s| s > 0).collect();
        Lookahead { sizes: usable, least: lo * n, most: hi * n }
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

    /// The paths in `groups`, so an assertion can name a photo rather than an
    /// index into a slice the reader has to reconstruct.
    fn placed_paths(photos: &[Photo], groups: &[Group]) -> Vec<String> {
        let mut out: Vec<String> =
            groups.iter().flat_map(|g| g.photos.iter().map(|&i| photos[i].path.clone())).collect();
        out.sort();
        out
    }

    fn include(paths: &[&str]) -> Overrides {
        paths.iter().map(|p| (format!("h{p}"), Override::Include)).collect()
    }

    // --- overrides: an explicit choice is never traded away silently ------

    /// **An `Include` is not droppable by capacity trimming.**
    ///
    /// 100 photos into a 20-page book that holds 33, so 67 must go. The two
    /// photos the user asked for are the two WORST in the set by the
    /// comparator that decides the trim (aesthetic 0 and 1), so nothing but
    /// the override can save them -- `pack_drops_the_lowest_ranked_photos_
    /// when_over_capacity` above pins that photo 0 is dropped without one.
    ///
    /// The placed COUNT is asserted too: protecting a photo must cost an
    /// `Auto` photo's place, not a slot.
    #[test]
    fn pack_never_drops_an_included_photo_when_trimming_to_capacity() {
        let photos: Vec<Photo> =
            (0..100).map(|i| photo(&format!("/p{i:03}.jpg"), 0, i as u8)).collect();
        let c = capacity_for(20, &sizes());
        let o = include(&["/p000.jpg", "/p001.jpg"]);

        let groups = pack(&photos, &c, &buildable_for(&sizes()), &o).expect("2 included photos fit in 33");

        let placed = placed_paths(&photos, &groups);
        assert!(placed.contains(&"/p000.jpg".to_string()), "the worst photo was asked for: {placed:?}");
        assert!(placed.contains(&"/p001.jpg".to_string()), "{placed:?}");
        assert_eq!(placed.len(), c.max_photos, "protecting a photo must not cost a slot");
        assert!(
            !placed.contains(&"/p002.jpg".to_string()),
            "an unasked-for weak photo must be the one that goes instead"
        );
    }

    /// **Included photos that do not fit are REPORTED, not dropped.**
    ///
    /// 40 photos, every one of them explicitly wanted, into a book that holds
    /// 33. There is no honest way to choose which seven of the user's own
    /// picks to discard, so the packer refuses and hands back the three
    /// numbers the user needs: how many they picked, how many fit, how many
    /// over.
    #[test]
    fn pack_reports_rather_than_drops_when_the_included_photos_alone_exceed_capacity() {
        let photos: Vec<Photo> =
            (0..40).map(|i| photo(&format!("/p{i:03}.jpg"), 0, i as u8)).collect();
        let c = capacity_for(20, &sizes());
        assert_eq!(c.max_photos, 33, "fixture: the book holds fewer than the 40 asked for");
        let o: Overrides =
            photos.iter().map(|p| (p.hash.clone(), Override::Include)).collect();

        let err = pack(&photos, &c, &buildable_for(&sizes()), &o)
            .expect_err("40 explicit choices cannot be silently cut to 33");

        assert_eq!(err, IncludeOverflow { pages: 20, included: 40, capacity: 33, over: 7 });
        assert!(err.to_string().contains("40"), "{err}");
        assert!(err.to_string().contains("20-page"), "{err}");
        assert!(err.to_string().contains("Exclude 7"), "{err}");
    }

    /// Exactly AT capacity is not over it. The boundary, at the boundary.
    #[test]
    fn pack_accepts_included_photos_that_exactly_fill_the_book() {
        let photos: Vec<Photo> =
            (0..33).map(|i| photo(&format!("/p{i:03}.jpg"), 0, i as u8)).collect();
        let c = capacity_for(20, &sizes());
        assert_eq!(c.max_photos, 33);
        let o: Overrides = photos.iter().map(|p| (p.hash.clone(), Override::Include)).collect();

        let groups = pack(&photos, &c, &buildable_for(&sizes()), &o).expect("33 into 33 must fit");

        assert_eq!(placed_paths(&photos, &groups).len(), 33);
    }

    /// Capacity trimming is not the only way a photo disappears: a chapter
    /// that is apportioned fewer slots than it needs strands the tail of
    /// itself, with no blank spread anywhere to show for it. An explicit
    /// choice must not be what gets stranded.
    ///
    /// Chapters of 31 and 2 with `buildable = {1,2,3}` sum to exactly the
    /// 33-photo capacity, so nothing is trimmed for capacity at all -- but
    /// `sum(need) = 11 + 1 = 12` exceeds the 11 slots, so chapter 1 is
    /// apportioned 10 slots for 31 photos and must strand one. `/c0p030.jpg`
    /// sorts last in its chapter, which is precisely the one the packer
    /// reaches after its slots run out.
    #[test]
    fn pack_strands_an_auto_photo_rather_than_an_included_one_in_a_crowded_chapter() {
        let mut photos: Vec<Photo> =
            (0..31).map(|i| photo(&format!("/c0p{i:03}.jpg"), 0, 50)).collect();
        photos.extend((0..2).map(|i| photo(&format!("/c1p{i:03}.jpg"), 1, 50)));
        let c = capacity_for(20, &sizes());
        assert_eq!(photos.len(), c.max_photos, "fixture: exactly at capacity, so nothing is trimmed");

        let without = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("no includes");
        let stranded = placed_paths(&photos, &without);
        assert!(
            !stranded.contains(&"/c0p030.jpg".to_string()),
            "fixture must actually strand this photo without an override, else the test is inert"
        );

        let groups = pack(&photos, &c, &buildable_for(&sizes()), &include(&["/c0p030.jpg"]))
            .expect("one include is well inside capacity");

        let placed = placed_paths(&photos, &groups);
        assert!(placed.contains(&"/c0p030.jpg".to_string()), "{placed:?}");
        assert!(
            placed.len() >= stranded.len(),
            "seating the explicit choice must cost an Auto photo's place, never a slot: \
             {} placed with the override, {} without",
            placed.len(),
            stranded.len()
        );
    }

    /// A chapter can be apportioned NO slot at all when there are more
    /// chapters than slots -- pass 1 hands one slot each, longest first, and
    /// runs out. Every photo in an unslotted chapter vanishes from the book.
    ///
    /// Twelve TWO-photo chapters into 11 slots: without an override the last
    /// chapter loses (ties go to the earlier chapter), so the photos the user
    /// asked for are exactly the ones the engine would have thrown away.
    ///
    /// Two photos per chapter rather than one, because a spread slot can no
    /// longer be cut to a single photo -- a one-photo chapter is now dropped
    /// wherever it lands but the closing single, which would make the fixture
    /// measure that instead of apportionment.
    #[test]
    fn pack_gives_a_slot_to_a_chapter_whose_photo_the_user_asked_for() {
        let photos: Vec<Photo> = (0..12)
            .flat_map(|i| {
                [photo(&format!("/p{i:02}a.jpg"), i, 50), photo(&format!("/p{i:02}b.jpg"), i, 50)]
            })
            .collect();
        let c = capacity_for(20, &sizes());

        let without = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("no includes");
        assert!(
            !placed_paths(&photos, &without).contains(&"/p11a.jpg".to_string()),
            "fixture must lose this chapter without an override, else the test is inert"
        );

        let groups = pack(&photos, &c, &buildable_for(&sizes()), &include(&["/p11a.jpg"])).expect("one include");

        assert!(
            placed_paths(&photos, &groups).contains(&"/p11a.jpg".to_string()),
            "the chapter holding an explicit choice must be represented: {:?}",
            placed_paths(&photos, &groups)
        );
    }

    /// An empty override map must leave the packer's output byte-identical to
    /// what it produced before overrides existed. Asserted against a fixture
    /// that exercises the trim, several chapters and the density swing at
    /// once.
    #[test]
    fn pack_with_no_overrides_packs_exactly_as_before() {
        let photos: Vec<Photo> = (0..60)
            .map(|i| photo(&format!("/p{i:02}.jpg"), (i % 4) as u32, (i * 7 % 100) as u8))
            .collect();
        let c = capacity_for(20, &full());
        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("no includes");

        assert_eq!(group_sizes(&groups), vec![4, 6, 5, 6, 4, 5, 4, 6, 5, 6, 6]);
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
        // The following slots are now explicit: `later(&[], 0)` where this is
        // the last slot (nothing follows, so the band collapses onto
        // `remaining`), and n slots drawing from {3,5} where more follow.
        assert_eq!(choose_group_size(2, &[3, 5], &later(&[], 0), 1, 0), None, "nothing in {{3,5}} covers 2");
        assert_eq!(
            choose_group_size(2, &[3, 5], &later(&[3, 5], 3), 4, -1),
            None,
            "nor with slots still to fill"
        );
        assert_eq!(
            choose_group_size(3, &[3, 5], &later(&[], 0), 1, 0),
            Some(3),
            "3 is covered, and must be"
        );
        // The decomposability guard itself, at the smallest input that shows
        // it working. Aim is 3 and 3 is buildable, so nearness-to-aim WANTS
        // 3 -- only the guard overrules it, because taking 3 of 5 strands a
        // remainder of 2 that {3,5} cannot build. Without the guard this
        // returns Some(3) and two photos vanish with nothing to show for it.
        // Asserted here rather than through `pack`, whose only gapped-set
        // fixture is decided by the band before the guard is ever consulted.
        assert_eq!(
            choose_group_size(5, &[3, 5], &later(&[3, 5], 1), 2, 0),
            Some(5),
            "taking 3 would strand a remainder of 2 that {{3,5}} cannot build"
        );
    }

    #[test]
    fn pack_never_chooses_a_zero_sized_group() {
        // `{0}` has no usable size at all, so its bounds floor at (1, 1) --
        // what `bounds_of` returns for it.
        assert_eq!(choose_group_size(4, &[0], &later(&[0], 1), 2, 0), None, "0 is not a group size");
        assert_eq!(
            choose_group_size(4, &[0, 3], &later(&[0, 3], 1), 2, 0),
            Some(3),
            "0 must not out-rank a real size"
        );
    }

    /// Photos in chapters, packed, keyed so the caller can count them.
    fn pack_chapters(chapter_sizes: &[usize], buildable: &[usize]) -> (Vec<Photo>, Vec<Group>) {
        let mut photos = Vec::new();
        for (c, &n) in chapter_sizes.iter().enumerate() {
            for i in 0..n {
                photos.push(photo(&format!("/c{c}p{i:03}.jpg"), c as u32, 50));
            }
        }
        let c = capacity_for(20, buildable);
        let groups = pack(&photos, &c, &buildable_for(buildable), &Overrides::new()).expect("the fixture must fit the included photos");
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
    /// No chapter here holds fewer than two photos: a spread slot cannot be
    /// cut to one (a 1-photo spread template prints a blank half), so a
    /// one-photo chapter landing on a spread is dropped BY DESIGN and would
    /// make this property untestable rather than false.
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
            vec![2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
            vec![2, 30, 2],
            vec![25, 25],
            vec![6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6],
            vec![13, 5, 2, 9, 22],
        ] {
            let total: usize = shape.iter().sum();
            let (photos, groups) = pack_chapters(&shape, &full);
            let placed: usize = groups.iter().map(|g| g.photos.len()).sum();
            assert!(
                total <= capacity_for(20, &full).max_photos,
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
        let c = capacity_for(20, &full());
        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("the fixture must fit the included photos");
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
        let c = capacity_for(20, &sizes());
        assert_eq!(c.singles, 2);
        assert_eq!(c.spreads, 9, "20 pages = 2 singles + 9 spreads");
        let c40 = capacity_for(40, &sizes());
        assert_eq!(c40.spreads, 19);
    }

    /// Boundary tests AT the boundaries, not near them.
    #[test]
    fn pack_recommends_twenty_pages_at_exactly_the_capacity_limit() {
        let twenty = capacity_for(20, &sizes());
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
        let over_estimate = capacity_for(20, &buildable_sizes(&lib)).max_photos;
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
        let c = capacity_for(20, &sizes());
        let groups = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("the fixture must fit the included photos");
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
        let c = capacity_for(20, &gapped);
        let groups = pack(&photos, &c, &buildable_for(&gapped), &Overrides::new()).expect("the fixture must fit the included photos");

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
        let c = capacity_for(20, &sizes());
        let groups = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("the fixture must fit the included photos");
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
        let c = capacity_for(20, &full());
        assert_eq!((c.spreads, c.singles), (9, 2), "the measured case: 9 spreads + 2 singles");
        let slots = c.spreads as usize + c.singles as usize;
        assert!(photos.len() > slots, "fixture: more photos than slots, so no slot may be blank");

        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("the fixture must fit the included photos");
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
        let c = capacity_for(20, &full());
        assert_eq!(photos.len(), c.max_photos, "fixture: exactly at capacity, so nothing is trimmed");

        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("the fixture must fit the included photos");
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
        let c = capacity_for(20, &buildable);
        let groups = pack(&photos, &c, &buildable_for(&buildable), &Overrides::new()).expect("the fixture must fit the included photos");

        assert_eq!(
            group_sizes(&groups).iter().sum::<usize>(),
            photos.len(),
            "fixture: all ten photos must be placed or the mixing has nowhere to show"
        );
        // The cut points are the whole argument here -- 2 and 3 are chosen so
        // no flat cut can land on the chapter boundary at 5 -- so a group of
        // any OTHER size falsifies the rationale before the mixing check even
        // runs. It was measured emitting [1, 2, 2, 2, 3] while the single set
        // was being invented as `1 ..= largest`.
        assert!(
            groups.iter().all(|g| buildable.contains(&g.photos.len())),
            "a group outside the declared set {buildable:?}: {:?}",
            group_sizes(&groups)
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
        let c = capacity_for(20, &full());
        let forward = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("the fixture must fit the included photos");

        let reversed: Vec<Photo> = photos.iter().rev().cloned().collect();
        let backward = pack(&reversed, &c, &buildable_for(&full()), &Overrides::new()).expect("the fixture must fit the included photos");

        assert!(!forward.is_empty());
        assert_eq!(
            by_path(&photos, &forward),
            by_path(&reversed, &backward),
            "permuting the input changed the book"
        );
    }

    /// Input in NON-chronological order -- a pre-sorted fixture cannot
    /// detect a missing sort (a real Phase 1 failure mode).
    ///
    /// TWO photos per chapter, not one. A spread slot cannot be cut to a
    /// single photo (a 1-photo spread template prints a blank half), so a
    /// one-photo chapter that lands on a spread is now dropped outright and
    /// the fixture would be measuring that rather than the ordering.
    #[test]
    fn pack_orders_groups_by_chapter_regardless_of_input_order() {
        let photos = vec![
            photo("/z1.jpg", 3, 50),
            photo("/a1.jpg", 1, 50),
            photo("/m1.jpg", 2, 50),
            photo("/z2.jpg", 3, 50),
            photo("/a2.jpg", 1, 50),
            photo("/m2.jpg", 2, 50),
        ];
        let c = capacity_for(20, &sizes());
        let groups = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("the fixture must fit the included photos");
        let clusters: Vec<u32> = groups.iter().map(|g| g.event_cluster).collect();
        assert_eq!(clusters, vec![1, 2, 3]);
    }

    /// The book's first and last slots are single PAGES facing the inside
    /// covers; everything between them is a spread. `pack` must say which is
    /// which, because `pace::assemble` cannot correctly re-derive it from
    /// position once the two kinds hold different numbers of photos.
    #[test]
    fn pack_tags_the_first_and_last_group_as_single_pages() {
        // 30 photos, one chapter, 20 pages -> 2 singles + 9 spreads = 11 slots.
        let photos: Vec<Photo> =
            (0..30).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
        let capacity = capacity_for(20, &[1, 2, 3, 4, 5, 6]);
        let groups = pack(
            &photos,
            &capacity,
            &buildable_for(&[1, 2, 3, 4, 5, 6]),
            &Overrides::new(),
        )
        .expect("packs");

        assert_eq!(groups.len(), 11, "11 slots should yield 11 groups");
        assert_eq!(groups[0].slot, SlotKind::Single, "group 0 fills the opening page");
        assert_eq!(groups[10].slot, SlotKind::Single, "group 10 fills the closing page");
        for (i, g) in groups.iter().enumerate().take(10).skip(1) {
            assert_eq!(g.slot, SlotKind::Spread, "group {i} fills a spread");
        }
    }

    /// **C1: a chapter too small to occupy a spread must not cost the user
    /// their photo, nor the book its assembly.**
    ///
    /// A spread slot cannot be cut to one photo. A chapter holding exactly one
    /// is therefore unplaceable wherever it lands but slot 0 or slot N-1, and
    /// `pack` walks slots in order -- so before the merge below, `/solo.jpg`
    /// came back unplaced and `pace::assemble` failed the whole book with
    /// "choose a longer book", advice that cannot help: a longer SKU adds
    /// spreads, never singles.
    ///
    /// Measured shape, from the review: 20 photos in cluster 0, one in
    /// cluster 1, 10 in cluster 2, with the singleton explicitly included.
    #[test]
    fn pack_places_a_singleton_chapter_the_user_asked_for() {
        let mut photos: Vec<Photo> =
            (0..20).map(|i| photo(&format!("/c0p{i:02}.jpg"), 0, 50)).collect();
        photos.push(photo("/solo.jpg", 1, 50));
        photos.extend((0..10).map(|i| photo(&format!("/c2p{i:02}.jpg"), 2, 50)));
        let c = capacity_for(20, &full());

        let groups = pack(&photos, &c, &buildable_for(&full()), &include(&["/solo.jpg"]))
            .expect("one include is well inside capacity");

        assert!(
            placed_paths(&photos, &groups).contains(&"/solo.jpg".to_string()),
            "a one-photo chapter the user asked for was dropped: {:?}",
            by_path(&photos, &groups)
        );
    }

    /// **The merge's last resort: a book with fewer photos than one spread.**
    ///
    /// Every chapter is then below the spread minimum, so every one of them
    /// carries forward and NO chapter is ever inserted into the output --
    /// `merge_sub_spread_chapters` reaches its final `carry` with nothing to
    /// append it to. The arm that handles that reinstates the photos as a
    /// chapter of their own; without it they are silently discarded and the
    /// book comes out empty, which is the worst outcome available for a
    /// one-photo book. Slot 0 is a single page and can hold exactly this.
    ///
    /// Probed: `placed=1 sizes=[1]` with the arm, `placed=0 sizes=[]` without.
    #[test]
    fn pack_places_the_only_photo_in_a_book_too_small_for_a_spread() {
        let photos = vec![photo("/only.jpg", 0, 50)];
        let c = capacity_for(20, &full());

        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("no includes");

        assert_eq!(
            placed_paths(&photos, &groups),
            vec!["/only.jpg".to_string()],
            "the only photo in the book was discarded: sizes {:?}",
            group_sizes(&groups)
        );
        assert_eq!(
            groups[0].slot,
            SlotKind::Single,
            "one photo belongs on the opening single page, the only slot that can hold it"
        );
    }

    /// **C1, the cascade.** Twelve one-photo chapters into 11 slots used to
    /// place ONE photo in total: the first chapter took slot 0 (a single, which
    /// can hold one), every later chapter faced slot 1 (a spread, which cannot),
    /// failed, and left `groups.len()` -- the global slot index -- exactly where
    /// it was, so the next chapter repeated the identical futile attempt.
    ///
    /// Merging sub-spread chapters removes the cause rather than papering over
    /// the symptom: twelve singletons become six chapters of two, and every
    /// photo is placed.
    #[test]
    fn pack_does_not_strand_every_chapter_when_one_cannot_fill_its_slot() {
        let photos: Vec<Photo> =
            (0..12).map(|i| photo(&format!("/p{i:02}.jpg"), i, 50)).collect();
        let c = capacity_for(20, &full());

        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("no includes");

        assert_eq!(
            placed_paths(&photos, &groups).len(),
            12,
            "12 photos over 11 slots must all be placed: {:?}",
            group_sizes(&groups)
        );
    }

    /// A 1-photo spread template has one slot, and the validator forbids a slot
    /// from spanning the fold -- so one of its two page halves is necessarily
    /// EMPTY and prints white. Size 1 therefore has no place in the spread
    /// region. The two single pages keep it: there, one photo on one page is
    /// the whole point.
    #[test]
    fn pack_never_cuts_a_one_photo_group_for_a_spread_slot() {
        // 13 photos into 11 slots forces small groups: under a book-wide
        // buildable set of 1..=6 the packer's fair share is ~1 and it cut
        // size-1 groups for spreads.
        let photos: Vec<Photo> =
            (0..13).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
        let buildable = Buildable::from_sizes(&[1, 2, 3, 4, 5, 6], 4);
        let capacity = Capacity::from_sizes(20, &[1, 2, 3, 4, 5, 6], 4);
        let groups = pack(&photos, &capacity, &buildable, &Overrides::new()).expect("packs");

        for g in &groups {
            if g.slot == SlotKind::Spread {
                assert!(
                    g.photos.len() >= 2,
                    "a spread slot was given {} photo(s); every 1-photo spread \
                     template prints one blank page",
                    g.photos.len()
                );
            }
        }
    }

    #[test]
    fn pack_drops_the_lowest_ranked_photos_when_over_capacity() {
        let photos: Vec<Photo> = (0..100)
            .map(|i| photo(&format!("/p{i:03}.jpg"), 0, i as u8))
            .collect();
        let c = capacity_for(20, &sizes());
        let groups = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("the fixture must fit the included photos");
        let used: usize = groups.iter().map(|g| g.photos.len()).sum();
        assert!(used <= c.max_photos, "used {used}, capacity {}", c.max_photos);
        // The best photo must survive; the worst must not.
        let kept: std::collections::BTreeSet<usize> =
            groups.iter().flat_map(|g| g.photos.iter().copied()).collect();
        assert!(kept.contains(&99), "highest aesthetic must be kept");
        assert!(!kept.contains(&0), "lowest aesthetic must be dropped");
    }
}
