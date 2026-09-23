//! Cutting chapters into per-spread photo groups.
//!
//! Group sizes are constrained by what the library can actually BUILD:
//! templates are exact-count, so a group of five is unbuildable until
//! 5-photo templates are authored. The packer takes the buildable sizes as
//! input rather than assuming 1..=6, so the missing-5-up gap degrades into a
//! different split rather than an unfillable spread.

use crate::book::cull::{Override, Overrides, Photo};
use crate::geometry::Side;
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
    /// How many photos `pack` aims to place: every spread at
    /// `TARGET_PER_SPREAD` (or its maximum, if that is smaller), the two
    /// single pages at their maximum. Below `max_photos` on purpose. Filling
    /// to the maximum leaves the packer no choice but a six-up on every
    /// spread, which is what the Iceland book measured: 20 of 20.
    pub target_photos: usize,
}

/// The average number of photos a spread aims for. `DENSITY_RHYTHM` swings
/// individual spreads either side of it.
pub const TARGET_PER_SPREAD: usize = 4;

/// A moment is every photo taken within this many seconds of the moment's
/// first photo. Pose and framing changes in a burst defeat pHash (distances of
/// 13 to 34 measured inside one 42-second burst, against a near-duplicate
/// threshold of 4), so capture time is the signal that sees them.
///
/// Anchored on the first photo rather than chained photo to photo: on the
/// Iceland library a chained 120-second gap joined 515 photos over 34 minutes
/// of steady shooting into one "moment", where the anchored window splits the
/// same 2783 photos into 230 moments of at most 72.
pub const MOMENT_GAP_SECONDS: i64 = 120;

/// The most photos one moment may place, unless the user included more.
pub const MAX_PER_MOMENT: usize = 2;

/// Offsets from the fair share, cycled slot by slot across the whole book.
/// With a share of 4 this reads 3, 6, 2, 5, 4: a mix of sparse and dense
/// spreads in which a six-up is one spread in five. Sums to zero so the
/// rhythm does not drift the book's average away from the share.
const DENSITY_RHYTHM: [i64; 5] = [-1, 2, -2, 1, 0];

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
        let slots = spreads as usize + singles as usize;
        // Summed slot by slot, each against the set of the side it prints on:
        // the opening page is a RIGHT half and the closing page a LEFT half,
        // and on the real library those hold 5 and 4. `2 x largest single`
        // read 64 here for a book whose true ceiling is 63, and the UI then
        // reported 0 dropped for a book that dropped photos.
        let most = |i: usize| b.bounds_at(i, slots).1;
        Capacity {
            pages,
            singles,
            spreads,
            max_photos: (0..slots).map(most).sum(),
            target_photos: (0..slots)
                .map(|i| match slot_kind_at(i, slots) {
                    SlotKind::Spread => most(i).min(TARGET_PER_SPREAD),
                    SlotKind::Single(_) => most(i),
                })
                .sum(),
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
/// between the two figures (64 to 66 against the real library, whose
/// `from_library` 20-page capacity is 63 and whose over-estimate is 66) it
/// recommends a 20-page book that then silently drops photos a 40-page book
/// would have kept. `pace::assemble` packs against `from_library`, so this is
/// also the only figure that agrees with the book the recommendation leads to.
///
/// On the real library the 20-page figure is 63: nine spreads of six, a
/// right-hand opening page of five and a left-hand closing page of four.
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
    /// The first or last page of the book: one page-half, on the side it
    /// prints on. Page 1 is a RIGHT half facing the inside front cover; the
    /// last page is a LEFT half facing the inside back cover. The side is
    /// carried because the two halves are authored separately and do not
    /// hold the same number of photos.
    Single(Side),
    /// Any of the (N-2)/2 spreads between them: two page-halves.
    Spread,
}

impl SlotKind {
    pub fn is_single(self) -> bool {
        matches!(self, SlotKind::Single(_))
    }
}

/// Which kind of slot sits at `index`, in a book of `slots` slots.
///
/// Slot 0 is the right-hand opening page and slot `slots - 1` the left-hand
/// closing page; everything between them is a spread. Extracted rather than
/// written inline because more than one caller needs the answer, and two
/// copies of the rule deciding which slot is a single is the same
/// duplicated-authority defect `book::cull` already had to consolidate.
fn slot_kind_at(index: usize, slots: usize) -> SlotKind {
    if index == 0 {
        SlotKind::Single(Side::Right)
    } else if index + 1 == slots {
        SlotKind::Single(Side::Left)
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
    /// Sizes for the closing page, a LEFT half: the slot counts of the
    /// library's left halves.
    pub single_left: Vec<usize>,
    /// Sizes for the opening page, a RIGHT half: the slot counts of the
    /// library's right halves.
    pub single_right: Vec<usize>,
    /// Sizes for the (N-2)/2 spreads: the library's spread counts, less 1.
    pub spread: Vec<usize>,
}

impl Buildable {
    /// Derived from the loaded library, never hardcoded, and every set is
    /// derived: the two single sets from the slot counts the page halves of
    /// EACH SIDE actually have, `spread` from the spread templates' photo
    /// counts less the sub-minimum ones. None is a contiguous range invented
    /// from a maximum -- a range claims the library can build every count
    /// below the largest, which is a claim only the library itself can make.
    ///
    /// Per side, deliberately. A page half is authored for the side it prints
    /// on -- bleed edges and the gutter are side-specific -- so
    /// `pace::best_single` filters the pool by `l.side == side`, and the
    /// opening page is a RIGHT half while the closing page is a LEFT half. On
    /// the real library LEFT halves offer `{1,2,3,4}` and RIGHT `{1,2,3,4,5}`.
    /// One pooled set was wrong both ways: its union let `pack` cut a
    /// 5-photo closing group no left half could build (so `pace::strongest`
    /// silently trimmed one), and its intersection was measured to lose the
    /// genuinely buildable 5-photo opening page. Only a per-side set gets
    /// both slots right, and only it makes `Capacity` read the true ceiling.
    pub fn from_library(lib: &Library) -> Buildable {
        let sizes = |side: Side| -> Vec<usize> {
            lib.page_half_pool()
                .iter()
                .filter(|p| p.side == side)
                .map(|p| p.slots.len())
                .filter(|&n| n > 0)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        Buildable {
            single_left: sizes(Side::Left),
            single_right: sizes(Side::Right),
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
    /// This one DOES invent `1 ..= largest_half` for both singles, and that
    /// is its documented weakness: it presumes a page half exists at every
    /// count up to the largest, on both sides, which only the library can
    /// confirm. Anything holding a real `Library` must use `from_library`.
    pub fn from_sizes(spread_sizes: &[usize], largest_half: usize) -> Buildable {
        let singles: Vec<usize> = (1..=largest_half.max(1)).collect();
        Buildable {
            single_left: singles.clone(),
            single_right: singles,
            spread: spread_sizes.iter().copied().filter(|&s| s >= SPREAD_MIN_PHOTOS).collect(),
        }
    }

    pub fn for_kind(&self, kind: SlotKind) -> &[usize] {
        match kind {
            SlotKind::Single(Side::Left) => &self.single_left,
            SlotKind::Single(Side::Right) => &self.single_right,
            SlotKind::Spread => &self.spread,
        }
    }

    /// Smallest and largest group the slot at `index` can be cut to, in a
    /// book of `slots` slots. The one place slot position is turned into a
    /// photo count; `Capacity`, `apportion_slots`, `lookahead` and the
    /// stranding guard in `pack` all read it rather than re-deriving which
    /// slot is which.
    pub fn bounds_at(&self, index: usize, slots: usize) -> (usize, usize) {
        bounds_of(self.for_kind(slot_kind_at(index, slots)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Indices into the photo slice handed to `pack`.
    pub photos: Vec<usize>,
    /// The chapter this group was cut from -- but NOT reliably the cluster its
    /// photos were originally assigned. `merge_sub_spread_chapters` folds a
    /// chapter too small to fill a spread into the next one chronologically,
    /// and `merge_chapters_the_slots_cannot_seat` folds the smallest chapter
    /// into a neighbour when the slots cannot seat them all; both re-key the
    /// folded photos onto the ABSORBING cluster id, so after a merge this names
    /// the chapter that absorbed them, not the one they came from.
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
/// through a spread reads as an accident rather than a decision. The one
/// exception is deliberate and bounded: when the chapters between them need
/// more slots than the book has, the smallest is folded into a neighbour
/// first, because a chapter boundary is the engine's own guess and a lost
/// photograph is not -- see `merge_chapters_the_slots_cannot_seat`.
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
/// protected from the capacity trim, and the trim is the only place this
/// function discards a photo: once the keepers fit the book, chapters are
/// folded together until the slots can seat every one of them
/// (`merge_chapters_the_slots_cannot_seat`). When the `Include` photos ALONE
/// exceed what the book can hold there is no honest way to choose between
/// them, so this returns `Err(IncludeOverflow)` rather than picking -- see
/// that type.
pub fn pack(
    photos: &[Photo],
    capacity: &Capacity,
    buildable: &Buildable,
    overrides: &Overrides,
) -> Result<Vec<Group>, IncludeOverflow> {
    let selected = select(photos, capacity.target_photos, overrides);
    pack_selected(photos, &selected, &std::collections::BTreeSet::new(), capacity, buildable, overrides)
}

/// `pack` with the selection made by the caller: `selected` indexes `photos`
/// and is laid out as given, and every event in `brief` folds into its
/// neighbour (`merge_sub_spread_chapters`). `pack` is this with the book-wide
/// `select` and no Brief events; `assemble_with` passes the per-event
/// `events::budget`.
pub fn pack_selected(
    photos: &[Photo],
    selected: &[usize],
    brief: &std::collections::BTreeSet<u32>,
    capacity: &Capacity,
    buildable: &Buildable,
    overrides: &Overrides,
) -> Result<Vec<Group>, IncludeOverflow> {
    use std::collections::BTreeMap;

    if photos.is_empty()
        || (buildable.single_left.is_empty()
            && buildable.single_right.is_empty()
            && buildable.spread.is_empty())
    {
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
    for &i in selected {
        chapters.entry(photos[i].event_cluster).or_default().push(i);
    }

    let slots = capacity.spreads as usize + capacity.singles as usize;
    let chapters = merge_sub_spread_chapters(chapters, spread_minimum(buildable), brief);
    let chapters = merge_chapters_the_slots_cannot_seat(chapters, slots, buildable);

    let mut groups = Vec::new();

    // Slots are a BOOK-WIDE budget; groups are cut per chapter. Apportion the
    // budget across chapters up front rather than letting each chapter take
    // what it likes from a shared counter -- otherwise the first chapter
    // drains the book and the last ones never appear.
    let counts: Vec<usize> = chapters.values().map(Vec::len).collect();
    let allowance = apportion_slots(&counts, slots, buildable);

    for ((cluster, mut members), mut slots_left) in chapters.into_iter().zip(allowance) {
        // Capture order, so a group reads as a stretch of the trip. Path
        // breaks ties and orders photos with no capture time.
        members.sort_by(|&a, &b| {
            photos[a].captured_at.cmp(&photos[b].captured_at).then(photos[a].path.cmp(&photos[b].path))
        });

        let mut i = 0;
        while i < members.len() && slots_left > 0 {
            let remaining = members.len() - i;
            // The deliberate density swing. Aiming every slot at exactly the
            // fair share converges the whole book on one group size, and in
            // this library density is a strict function of slot count
            // (1 -> sparse, 2..4 -> medium, 6 -> dense) -- so a book of
            // identically sized groups has ONE density, and `repace` cannot
            // vary an axis the packer has flattened. `DENSITY_RHYTHM` swings
            // each slot either side of the share, restoring the variation
            // `repace` operates on. It is a preference, not a licence: the feasible
            // band inside `choose_group_size` overrides it whenever obeying
            // it would strand a photo or leave a later slot unfillable.
            //
            // The phase runs on the whole book's group count rather than the
            // chapter's, so the rhythm carries across a chapter boundary
            // instead of resetting at every one. Starting SPARSE puts the
            // smaller group on the opening single page, which holds only a
            // page-half's worth anyway.
            let swing = DENSITY_RHYTHM[groups.len() % DENSITY_RHYTHM.len()];
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

/// The moment each photo belongs to, indexed like `photos`. Photos are walked
/// in capture order, and one taken more than `MOMENT_GAP_SECONDS` after the
/// current moment's first photo starts a new moment. A photo with no capture
/// time is a moment of its own.
pub fn moments(photos: &[Photo]) -> Vec<u32> {
    let mut timed: Vec<(i64, usize)> =
        photos.iter().enumerate().filter_map(|(i, p)| p.captured_at.map(|t| (t, i))).collect();
    timed.sort_unstable();
    let mut out = vec![0u32; photos.len()];
    let mut next = 0u32;
    let mut start: Option<i64> = None;
    for (t, i) in timed {
        if start.is_some_and(|s| t - s > MOMENT_GAP_SECONDS) {
            next += 1;
            start = None;
        }
        start.get_or_insert(t);
        out[i] = next;
    }
    for (i, p) in photos.iter().enumerate() {
        if p.captured_at.is_none() {
            next += 1;
            out[i] = next;
        }
    }
    out
}

/// Every photo the user did not include that `select` may take, best first:
/// every moment's best before any moment's second, at most `MAX_PER_MOMENT`
/// per moment. "Best" is aesthetic, then sharpness, then path.
fn ranked_auto(photos: &[Photo], overrides: &Overrides) -> Vec<usize> {
    let wanted = |i: usize| overrides.get(&photos[i].hash) == Override::Include;
    let moment = moments(photos);
    let mut auto: Vec<usize> = (0..photos.len()).filter(|&i| !wanted(i)).collect();
    auto.sort_by(|&a, &b| {
        photos[b]
            .aesthetic_pct
            .cmp(&photos[a].aesthetic_pct)
            .then(photos[b].sharpness_pct.cmp(&photos[a].sharpness_pct))
            .then(photos[b].path.cmp(&photos[a].path))
    });

    // (place within its moment, place overall, photo): sorting on this puts
    // every moment's first pick ahead of every moment's second.
    let mut taken: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut ranked: Vec<(usize, usize, usize)> = Vec::new();
    for (order, i) in auto.into_iter().enumerate() {
        let place = taken.entry(moment[i]).or_default();
        if *place < MAX_PER_MOMENT {
            ranked.push((*place, order, i));
        }
        *place += 1;
    }
    ranked.sort_unstable();
    ranked.into_iter().map(|(_, _, i)| i).collect()
}

/// The photos `pack` places, as sorted indices into `photos`: every
/// `Include`, then up to `target` in all, taken so that every moment's best
/// photo comes before any moment's second. Each moment places at most
/// `MAX_PER_MOMENT` photos the user did not include, however much room is
/// left, so a burst of one pose cannot fill a spread.
pub fn select(photos: &[Photo], target: usize, overrides: &Overrides) -> Vec<usize> {
    let wanted = |i: usize| overrides.get(&photos[i].hash) == Override::Include;
    let mut kept: Vec<usize> = (0..photos.len()).filter(|&i| wanted(i)).collect();
    let room = target.saturating_sub(kept.len());
    kept.extend(ranked_auto(photos, overrides).into_iter().take(room));
    kept.sort_unstable();
    kept
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventOrder {
    pub list: Vec<usize>,
    pub included: usize,
    /// How many of `list`'s leading photos are different pictures: its
    /// Includes and every auto pick `defer_look_alikes` did not move back.
    pub distinct: usize,
}

/// Each event's photos ranked ONCE (spec §5 step 5): its Includes, then
/// `ranked_auto` restricted to the event, with look-alikes moved behind the
/// rest (`defer_look_alikes`). A quota takes a prefix, so more
/// room only ever adds photos to an event, never swaps them.
///
/// `moments` (and so `MAX_PER_MOMENT`) is computed over the whole `photos`
/// slice passed in here, not per event: events are bucketed by
/// `event_cluster` AFTER one library-wide `ranked_auto` pass. Two events
/// whose capture times overlap -- e.g. one event's photos span long enough
/// to run chronologically alongside a neighbour's -- compete for the same
/// per-moment slots, not separate ones.
pub fn event_order(
    photos: &[Photo],
    overrides: &Overrides,
) -> std::collections::BTreeMap<u32, EventOrder> {
    let mut out: std::collections::BTreeMap<u32, EventOrder> = std::collections::BTreeMap::new();
    for (i, p) in photos.iter().enumerate() {
        let entry =
            out.entry(p.event_cluster).or_insert_with(|| EventOrder { list: Vec::new(), included: 0, distinct: 0 });
        if overrides.get(&p.hash) == Override::Include {
            entry.list.push(i);
            entry.included += 1;
            entry.distinct += 1;
        }
    }
    let mut auto: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for i in ranked_auto(photos, overrides) {
        auto.entry(photos[i].event_cluster).or_default().push(i);
    }
    for (event, ranked) in auto {
        if let Some(entry) = out.get_mut(&event) {
            let (list, fresh) = defer_look_alikes(photos, &entry.list, ranked);
            entry.list.extend(list);
            entry.distinct += fresh;
        }
    }
    out
}

/// `ranked` reordered so a photo that looks like one already ahead of it (in
/// `chosen` or earlier in `ranked`) waits behind every photo that does not.
///
/// The cull only merges look-alikes taken within `SIMILAR_SPAN_SECONDS`, and a
/// moment ends after `MOMENT_GAP_SECONDS`, so the same shot retaken every
/// few minutes -- a time-lapse, a pose repeated along a walk -- is a keeper
/// and a moment each time. An event with room for several photos then
/// printed that shot several times over a different picture ranked below it. Deferred, not dropped: an event with nothing
/// else still fills its quota with them. Also returns how many lead the
/// list as different pictures.
fn defer_look_alikes(photos: &[Photo], chosen: &[usize], ranked: Vec<usize>) -> (Vec<usize>, usize) {
    let print = |i: usize| photos[i].feature_print.as_deref();
    let mut ahead: Vec<&[f32]> = chosen.iter().filter_map(|&i| print(i)).collect();
    let (mut first, mut later) = (Vec::with_capacity(ranked.len()), Vec::new());
    for i in ranked {
        let Some(p) = print(i) else {
            first.push(i);
            continue;
        };
        let alike = ahead
            .iter()
            .any(|a| crate::cluster::feature_distance(a, p).is_some_and(|d| d <= crate::cluster::SIMILAR_DISTANCE));
        if alike {
            later.push(i);
        } else {
            ahead.push(p);
            first.push(i);
        }
    }
    let fresh = first.len();
    first.extend(later);
    (first, fresh)
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
///
/// A Brief event (`brief`) folds whatever its size. Its cap is small enough
/// that its photos can meet the spread minimum -- two photos build a spread --
/// so without this a Brief event would take a whole spread of its own, which
/// is what Brief exists to prevent (spec §2).
fn merge_sub_spread_chapters(
    chapters: std::collections::BTreeMap<u32, Vec<usize>>,
    spread_min: usize,
    brief: &std::collections::BTreeSet<u32>,
) -> std::collections::BTreeMap<u32, Vec<usize>> {
    if spread_min <= 1 && brief.is_empty() {
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
        if members.len() < spread_min || brief.contains(&cluster) {
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

/// The slot bounds a chapter would occupy if it were given `k` slots.
///
/// Which slots those are follows from the chapter's position, because
/// `pack` hands slots out in chapter order:
///
/// * the FIRST chapter's run starts at slot 0, the right-hand opening page;
/// * the LAST chapter's run ends at the left-hand closing page -- but only
///   when the book fills (`full`). When the photos run out before the last
///   slot, its run ends on a spread and the closing page is never reached;
/// * every other chapter sits wholly in the spread region.
///
/// Position matters because the kinds differ: a single page holds one photo
/// at the least where a spread holds two, and on the real library the
/// opening page holds five at the most, the closing page four, a spread six.
/// Sizing a chapter against a book-wide floor of one was how a chapter of
/// eight photos came to be handed five spread slots that between them need
/// ten -- `docs/PROJECT-STATUS.md` open item 12.
fn chapter_slot_bounds(
    chapter: usize,
    chapters: usize,
    k: usize,
    slots: usize,
    full: bool,
    b: &Buildable,
) -> Vec<(usize, usize)> {
    let k = k.min(slots);
    if chapter == 0 {
        (0..k).map(|i| b.bounds_at(i, slots)).collect()
    } else if chapter + 1 == chapters && full {
        (slots - k..slots).map(|i| b.bounds_at(i, slots)).collect()
    } else {
        // Spreads only. Index 1 is a spread in any book of three or more
        // slots; the degenerate smaller books have no middle chapter.
        let spread = bounds_of(&b.spread);
        vec![spread; k]
    }
}

/// The MOST slots a chapter of `count` photos can fill, every one of them
/// with at least its smallest group. One more and a slot comes out blank.
fn most_slots(
    count: usize,
    chapter: usize,
    chapters: usize,
    slots: usize,
    full: bool,
    b: &Buildable,
) -> usize {
    (1..=slots)
        .take_while(|&k| {
            chapter_slot_bounds(chapter, chapters, k, slots, full, b)
                .iter()
                .map(|&(lo, _)| lo)
                .sum::<usize>()
                <= count
        })
        .count()
}

/// The FEWEST slots that hold `count` photos, each at its largest. One fewer
/// and a photo has nowhere to go.
fn fewest_slots(
    count: usize,
    chapter: usize,
    chapters: usize,
    slots: usize,
    full: bool,
    b: &Buildable,
) -> usize {
    if count == 0 {
        return 0;
    }
    (1..=slots)
        .find(|&k| {
            chapter_slot_bounds(chapter, chapters, k, slots, full, b)
                .iter()
                .map(|&(_, hi)| hi)
                .sum::<usize>()
                >= count
        })
        .unwrap_or(slots)
}

/// Folds chapters together until the slots can seat every photo and, when
/// there are enough photos to fill the book, until every slot can be filled.
///
/// "A group never spans two chapters" protects the reader from a chapter
/// boundary falling in the middle of a spread. It is a rule about how the
/// book READS, and it was being paid for twice over:
///
/// * **in photos.** 26 chapters of two or three into 11 slots kept 11 of
///   them and silently dropped the other 15 chapters whole; eight chapters of
///   eight into the same 11 slots -- 64 photos, one over a 63-photo book --
///   each needed two slots, got one, and stranded their tails.
/// * **in blank pages.** 20 photos in chapters of 8, 8 and 4 can fill only
///   ten of the eleven slots while every chapter keeps its own spreads, so
///   the closing page printed white with a photo count that fills the book
///   exactly. The first thing the first real-photo run complained about was
///   pages left blank for no visible reason.
///
/// The clusters are the engine's own time-gap guess, not something the user
/// drew, and both a lost photograph and an unexplained white page are worse
/// than a chapter boundary landing mid-spread. So two adjacent chapters are
/// folded into one -- adjacent only, so chronology is preserved -- while
/// either holds:
///
/// 1. the chapters between them NEED more slots than the book has (a photo
///    would be stranded), choosing the fold that leaves the fewest slots
///    needed in total;
/// 2. the chapters between them CAN FILL fewer slots than the book has while
///    the photos could fill every one of them (a slot would print white),
///    choosing the fold that leaves the most slots fillable.
///
/// Ties go to the smaller resulting chapter, then the earlier one. Folding
/// the smallest chapter into its smaller neighbour was tried first and
/// over-folded badly: twelve chapters of two under a largest group of three
/// fold pairwise into fours that need two slots each -- no saving at all --
/// and it kept folding down to five chapters, when folding one two into ONE
/// neighbour already fits. Nothing is folded when the slots already suffice,
/// which keeps every book that fitted before packed exactly as before.
fn merge_chapters_the_slots_cannot_seat(
    mut chapters: std::collections::BTreeMap<u32, Vec<usize>>,
    slots: usize,
    b: &Buildable,
) -> std::collections::BTreeMap<u32, Vec<usize>> {
    // Both figures are measured against a FULL book throughout, deliberately:
    // a book whose chapters need more slots than exist is full by definition,
    // and the second rule only fires when the photos could fill it.
    let total_need = |counts: &[usize]| -> usize {
        let n = counts.len();
        (0..n).map(|j| fewest_slots(counts[j], j, n, slots, true, b)).sum()
    };
    let total_cap = |counts: &[usize]| -> usize {
        let n = counts.len();
        (0..n).map(|j| most_slots(counts[j], j, n, slots, true, b)).sum()
    };
    let fewest_to_fill: usize = (0..slots).map(|i| b.bounds_at(i, slots).0).sum();
    loop {
        let n = chapters.len();
        let counts: Vec<usize> = chapters.values().map(Vec::len).collect();
        let total: usize = counts.iter().sum();
        let stranding = total_need(&counts) > slots;
        let blanking = total >= fewest_to_fill && total_cap(&counts) < slots;
        if n <= 1 || !(stranding || blanking) {
            return chapters;
        }
        let best = (0..n - 1)
            .map(|j| {
                let mut folded = counts.clone();
                folded[j] += folded.remove(j + 1);
                let score = if stranding {
                    total_need(&folded)
                } else {
                    slots.saturating_sub(total_cap(&folded))
                };
                (score, folded[j], j)
            })
            .min()
            .map(|(_, _, j)| j);
        let Some(j) = best else {
            return chapters;
        };
        let keys: Vec<u32> = chapters.keys().copied().collect();
        let mut moved = chapters.remove(&keys[j + 1]).unwrap_or_default();
        if let Some(target) = chapters.get_mut(&keys[j]) {
            target.append(&mut moved);
        }
    }
}

/// How many slots each chapter may cut groups from, given the whole book's
/// budget. `counts` is per chapter in chronological (cluster id) order; the
/// return is the same length and order.
///
/// Each chapter has two figures that bound its share, both measured against
/// the KINDS of slot it would actually occupy (`chapter_slot_bounds`):
///
/// * **`need`** (`fewest_slots`) -- the fewest slots that can hold all its
///   photos. Below this the chapter silently drops photos, with no blank
///   spread anywhere to show for it. That is the same failure this whole
///   module exists to remove, so `need` is a FLOOR, not a preference.
/// * **`cap`** (`most_slots`) -- the most slots it could possibly fill.
///   Above this the surplus slots can only come out blank.
///
/// Whether the book FILLS decides which slots the last chapter ends on, and
/// that is settled first: if the chapters' caps measured against a full book
/// cover every slot, every slot is handed out and the last chapter really
/// does close on the left-hand page. If they do not, the photos run out
/// before the closing page and the last chapter is measured against spreads
/// alone -- otherwise it is credited with a one-photo closing slot it will
/// never reach, handed one slot too many, and strands a photo at a spread it
/// cannot build.
///
/// Three passes, all deterministic and all independent of the input slice's
/// ordering (they see only per-chapter counts, in cluster order):
///
/// 1. **One slot each, longest chapter first.** A chapter with no slot does
///    not appear in the book at all -- a group never spans chapters, so
///    there is no way for its photos to ride along in someone else's group.
///    `merge_chapters_the_slots_cannot_seat` has already made sure there are
///    no more chapters than slots, so this pass always completes; the
///    ordering is kept so the rule is total if that ever stops being true.
/// 2. **Every chapter up to its `need`**, before any chapter is given a slot
///    it merely *wants*. The merge above guarantees `sum(need) <= slots`, so
///    every chapter reaches its need and no photo is stranded for want of a
///    slot.
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
/// The user's `Include` decisions do not appear here, and that is not an
/// omission. `merge_chapters_the_slots_cannot_seat` runs first and guarantees
/// `sum(need) <= slots`, so pass 2 brings EVERY chapter up to its need and no
/// chapter -- with or without an explicit choice in it -- is ever starved of
/// a slot. An earlier version ranked chapters holding an `Include` first in
/// both passes; with starvation impossible that ranking could never change
/// an outcome, and it was removed rather than kept as a guard no test could
/// reach. `pace::assemble`'s post-condition remains the check that an
/// `Include` reached the page.
fn apportion_slots(counts: &[usize], slots: usize, b: &Buildable) -> Vec<usize> {
    let n = counts.len();
    let mut out = vec![0usize; n];
    if n == 0 || slots == 0 {
        return out;
    }
    let caps_if = |full: bool| -> Vec<usize> {
        (0..n).map(|j| most_slots(counts[j], j, n, slots, full, b)).collect()
    };
    let mut caps = caps_if(true);
    let full = caps.iter().sum::<usize>() >= slots;
    if !full {
        caps = caps_if(false);
    }
    let needs: Vec<usize> =
        (0..n).map(|j| fewest_slots(counts[j], j, n, slots, full, b)).collect();

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
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None,
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
        let singles: Vec<usize> = sizes.iter().copied().filter(|&s| s > 0).collect();
        Buildable {
            single_left: singles.clone(),
            single_right: singles,
            spread: sizes.iter().copied().filter(|&s| s >= SPREAD_MIN_PHOTOS).collect(),
        }
    }

    /// The capacity of exactly the set `buildable_for` yields, so the two can
    /// never disagree about what the fixture library can build.
    fn capacity_for(pages: u32, sizes: &[usize]) -> Capacity {
        Capacity::from_buildable(pages, &buildable_for(sizes))
    }

    /// `capacity_for` with the target raised to the maximum, for tests about
    /// how the packer shares out slots when every slot must be filled to the
    /// top. The density target would otherwise trim their fixtures first.
    fn full_density(pages: u32, sizes: &[usize]) -> Capacity {
        let c = capacity_for(pages, sizes);
        Capacity { target_photos: c.max_photos, ..c }
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

    /// **Chapters that between them need more slots than the book has are
    /// folded together, not starved.**
    ///
    /// Chapters of 31 and 2 with `buildable = {1,2,3}` sum to exactly the
    /// 33-photo capacity, so nothing is trimmed -- but chapter 0 alone needs
    /// all 11 slots (right half of 3, nine spreads of 3, left half of 3), and
    /// chapter 1 needs one more. Apportioning 10 to chapter 0 stranded
    /// `/c0p030.jpg`, the last in path order, with no blank page to show for
    /// it. Folding the two-photo chapter into its neighbour seats all 33.
    ///
    /// Mutation: with `merge_chapters_the_slots_cannot_seat` returning its
    /// input unchanged, 32 are placed and `/c0p030.jpg` is the one missing.
    #[test]
    fn pack_folds_a_chapter_in_rather_than_stranding_a_photo_when_the_slots_run_short() {
        let mut photos: Vec<Photo> =
            (0..31).map(|i| photo(&format!("/c0p{i:03}.jpg"), 0, 50)).collect();
        photos.extend((0..2).map(|i| photo(&format!("/c1p{i:03}.jpg"), 1, 50)));
        let c = capacity_for(20, &sizes());
        assert_eq!(photos.len(), c.max_photos, "fixture: exactly at capacity, so nothing is trimmed");

        let groups = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("no includes");

        let placed = placed_paths(&photos, &groups);
        assert_eq!(placed.len(), 33, "every photo the book can hold is seated: {:?}", group_sizes(&groups));
        assert!(placed.contains(&"/c0p030.jpg".to_string()), "{placed:?}");
    }

    /// **More chapters than slots loses no chapter.** Twelve two-photo
    /// chapters into 11 slots used to drop the twelfth whole -- pass 1 of
    /// `apportion_slots` handed one slot each, longest first, and ran out.
    /// The smallest chapter is now folded into a chronological neighbour, so
    /// all 24 photos are placed and the folded pair land in ONE group beside
    /// the chapter that absorbed them rather than scattered.
    ///
    /// Mutation: with the fold disabled, 22 are placed and `/p11a.jpg`,
    /// `/p11b.jpg` are the ones missing.
    #[test]
    fn pack_folds_chapters_together_rather_than_dropping_one_when_they_outnumber_the_slots() {
        let photos: Vec<Photo> = (0..12)
            .flat_map(|i| {
                [photo(&format!("/p{i:02}a.jpg"), i, 50), photo(&format!("/p{i:02}b.jpg"), i, 50)]
            })
            .collect();
        let c = capacity_for(20, &sizes());

        let groups = pack(&photos, &c, &buildable_for(&sizes()), &Overrides::new()).expect("no includes");

        assert_eq!(placed_paths(&photos, &groups).len(), 24, "{:?}", by_path(&photos, &groups));
        assert_eq!(groups.len(), 11, "one group per slot");
        let folded = groups
            .iter()
            .find(|g| g.photos.iter().any(|&i| photos[i].path == "/p00a.jpg"))
            .expect("the first chapter is placed");
        let mut members: Vec<&str> = folded.photos.iter().map(|&i| photos[i].path.as_str()).collect();
        members.sort_unstable();
        assert_eq!(
            members,
            vec!["/p00a.jpg", "/p00b.jpg", "/p01a.jpg"],
            "the smallest chapter folds into its chronological neighbour, and the two are cut as one chapter"
        );
    }

    /// Overrides reach `pack` through exactly one door: the capacity trim,
    /// which refuses to discard an `Include`. Everything after the trim --
    /// merging, apportionment, cutting -- sees only per-chapter counts, so an
    /// override that changes nothing about WHICH photos survive the trim
    /// cannot change the groups. Pinned by packing the same 40 photos with an
    /// empty map and with every one of them marked `Include`. 40 is under the
    /// density target of 48, so the trim keeps every photo either way.
    #[test]
    fn pack_cuts_the_same_groups_whether_or_not_the_survivors_are_marked_include() {
        let photos: Vec<Photo> = (0..40)
            .map(|i| photo(&format!("/p{i:02}.jpg"), (i % 4) as u32, (i * 7 % 100) as u8))
            .collect();
        let c = capacity_for(20, &full());
        let all: Overrides = photos.iter().map(|p| (p.hash.clone(), Override::Include)).collect();

        let plain = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("no includes");
        let marked = pack(&photos, &c, &buildable_for(&full()), &all).expect("40 fit in 66");

        assert_eq!(by_path(&photos, &plain), by_path(&photos, &marked));
        assert_eq!(placed_paths(&photos, &plain).len(), 40);
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
        let c = full_density(20, buildable);
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
        // 30 photos over 11 slots is 2.7 each. `DENSITY_RHYTHM` reaches two
        // photos above that, so 5 is the ceiling -- and no slot may run at
        // the library's maximum, which is what the greedy packer did with all
        // five of its groups.
        assert!(
            s.iter().all(|&n| n <= 5),
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
        let c = full_density(20, &full());
        assert_eq!(photos.len(), c.max_photos, "fixture: exactly at capacity, so nothing is trimmed");

        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).expect("the fixture must fit the included photos");
        let placed = placed_paths(&photos, &groups);
        assert_eq!(placed.len(), 66, "a chapter vanished from the book: sizes {:?}", group_sizes(&groups));
        assert!(placed.contains(&"/b0.jpg".to_string()) && placed.contains(&"/c2.jpg".to_string()));
        assert!(
            groups.iter().filter(|g| g.event_cluster == 1).count() < groups.len(),
            "the first chapter must not take every slot: {:?}",
            by_path(&photos, &groups)
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
        assert_eq!(groups[0].slot, SlotKind::Single(Side::Right), "group 0 fills the opening page");
        assert_eq!(groups[10].slot, SlotKind::Single(Side::Left), "group 10 fills the closing page");
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

    /// A Brief event folds into its neighbour whatever its size: its two
    /// photos meet the spread minimum, so without the fold it takes a whole
    /// spread of its own (spec §2).
    #[test]
    fn a_two_photo_brief_chapter_folds_into_its_neighbour() {
        use std::collections::{BTreeMap, BTreeSet};
        let chapters: BTreeMap<u32, Vec<usize>> =
            [(0, vec![0, 1, 2, 3]), (1, vec![4, 5]), (2, vec![6, 7, 8, 9])].into_iter().collect();
        let brief: BTreeSet<u32> = [1].into_iter().collect();
        let out = merge_sub_spread_chapters(chapters.clone(), 2, &brief);
        assert_eq!(out.keys().copied().collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(out[&2].len(), 6, "the Brief pair joins the NEXT chapter");
        // The last chapter Brief joins the previous one.
        let brief_last: BTreeSet<u32> = [2].into_iter().collect();
        let out = merge_sub_spread_chapters(chapters.clone(), 2, &brief_last);
        assert_eq!(out[&1].len(), 6);
        // Nothing Brief: unchanged, as before.
        assert_eq!(merge_sub_spread_chapters(chapters.clone(), 2, &BTreeSet::new()), chapters);
        // A library whose spreads start at one photo folds nothing by size,
        // and still folds a Brief event.
        let out = merge_sub_spread_chapters(chapters.clone(), 1, &brief);
        assert_eq!(out.keys().copied().collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(merge_sub_spread_chapters(chapters.clone(), 1, &BTreeSet::new()), chapters);
        // A Brief FIRST chapter folds forward too.
        let out = merge_sub_spread_chapters(chapters.clone(), 2, &BTreeSet::from([0]));
        assert_eq!(out.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(out[&1], vec![4, 5, 0, 1, 2, 3], "the Brief first chapter joins chapter 1");
        // Consecutive Brief chapters keep the earlier carry: both land in 2.
        let out = merge_sub_spread_chapters(chapters.clone(), 2, &BTreeSet::from([0, 1]));
        assert_eq!(out.keys().copied().collect::<Vec<_>>(), vec![2]);
        assert_eq!(out[&2].len(), 10);
        // Every chapter Brief: one chapter holding every photo.
        let out = merge_sub_spread_chapters(chapters.clone(), 2, &BTreeSet::from([0, 1, 2]));
        assert_eq!(out.len(), 1);
        assert_eq!(out.values().map(Vec::len).sum::<usize>(), 10);
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
            SlotKind::Single(Side::Right),
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

    // --- variety: moments, not just bursts -----------------------------------

    fn timed(path: &str, aesthetic: u8, at: Option<i64>) -> Photo {
        Photo { captured_at: at, ..photo(path, 0, aesthetic) }
    }

    fn kept_paths(photos: &[Photo], kept: &[usize]) -> Vec<String> {
        let mut out: Vec<String> = kept.iter().map(|&i| photos[i].path.clone()).collect();
        out.sort();
        out
    }

    /// A moment ends exactly `MOMENT_GAP_SECONDS` after its FIRST photo: 120 s
    /// after it is still the same moment, 121 s is the next, even though that
    /// photo is only 1 s after the one before it. A chained gap would keep
    /// all four together. The input is out of time order, so a grouping that
    /// walked the slice as given would split the burst.
    #[test]
    fn pack_moments_end_one_second_past_the_window_from_their_first_photo() {
        let photos = vec![
            timed("/c.jpg", 50, Some(1_120)),
            timed("/a.jpg", 50, Some(1_000)),
            timed("/d.jpg", 50, Some(1_121)),
            timed("/b.jpg", 50, Some(1_042)),
            timed("/e.jpg", 50, None),
            timed("/f.jpg", 50, None),
            timed("/g.jpg", 50, Some(1_241)),
        ];
        let m = moments(&photos);
        let (c, a, d, b, e, f, g) = (m[0], m[1], m[2], m[3], m[4], m[5], m[6]);
        assert_eq!((a, b), (c, c), "1000, 1042 and 1120 are one moment: {m:?}");
        assert_ne!(d, c, "1121 is 121 s after 1000, so it starts a new moment: {m:?}");
        assert_eq!(g, d, "1241 is 120 s after 1121, the new moment's first photo: {m:?}");
        assert!(e != f && ![a, d].contains(&e) && ![a, d].contains(&f),
            "a photo with no capture time is a moment of its own: {m:?}");
    }

    /// Three moments of six frames each. Moment A's frames all out-score
    /// moment C's best, so an aesthetic-only trim keeps A and B and loses C.
    /// Coverage first keeps one of each, and each is its moment's best.
    #[test]
    fn pack_selects_one_photo_from_every_moment_before_a_second_from_any() {
        let mut photos = Vec::new();
        for (m, base) in [("a", 90u8), ("b", 80), ("c", 10)] {
            let start = match m { "a" => 0, "b" => 3_600, _ => 7_200 };
            for i in 0..6u8 {
                photos.push(timed(&format!("/{m}{i}.jpg"), base + i, Some(start + i64::from(i))));
            }
        }
        let kept = select(&photos, 3, &Overrides::new());
        assert_eq!(kept_paths(&photos, &kept), ["/a5.jpg", "/b5.jpg", "/c5.jpg"]);

        let four = select(&photos, 4, &Overrides::new());
        assert_eq!(
            kept_paths(&photos, &four),
            ["/a4.jpg", "/a5.jpg", "/b5.jpg", "/c5.jpg"],
            "the fourth pick is the best second frame, from the best moment"
        );
    }

    /// Six frames of one moment in a book with room for all of them: only
    /// `MAX_PER_MOMENT` are placed, the best ones. An explicit `Include` on
    /// the worst frame is placed as well and does not cost a frame its place.
    #[test]
    fn pack_caps_the_photos_one_moment_can_place_even_with_room_to_spare() {
        let photos: Vec<Photo> =
            (0..6u8).map(|i| timed(&format!("/m{i}.jpg"), 50 + i, Some(i64::from(i) * 5))).collect();
        let kept = select(&photos, 100, &Overrides::new());
        assert_eq!(kept_paths(&photos, &kept), ["/m4.jpg", "/m5.jpg"]);

        let kept = select(&photos, 100, &include(&["/m0.jpg"]));
        assert_eq!(kept_paths(&photos, &kept), ["/m0.jpg", "/m4.jpg", "/m5.jpg"]);

        let c = capacity_for(20, &full());
        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).unwrap();
        assert_eq!(placed_paths(&photos, &groups), ["/m4.jpg", "/m5.jpg"], "pack applies the cap");
    }

    /// 90 distinct photos into a 20-page book. The book is filled to the
    /// density target, not to the last slot, and the spreads mix sparse and
    /// dense instead of all being six-ups.
    #[test]
    fn pack_aims_below_the_maximum_and_mixes_sparse_and_dense_spreads() {
        let photos: Vec<Photo> =
            (0..90).map(|i| photo(&format!("/p{i:02}.jpg"), 0, (i % 100) as u8)).collect();
        let c = capacity_for(20, &full());
        // The fixture's single pages hold 6, the real library's 5 and 4, so
        // this reads 66 and 48 where the real 20-page book reads 63 and 45.
        assert_eq!((c.max_photos, c.target_photos), (66, 48));
        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).unwrap();
        let s = group_sizes(&groups);
        assert_eq!(s.iter().sum::<usize>(), 48, "{s:?}");
        let spreads: Vec<usize> = groups
            .iter()
            .filter(|g| matches!(g.slot, SlotKind::Spread))
            .map(|g| g.photos.len())
            .collect();
        assert_eq!(spreads.len(), 9, "{s:?}");
        assert!(spreads.contains(&6), "a six-up still appears: {spreads:?}");
        assert!(spreads.iter().any(|&n| n <= 3), "so does a sparse spread: {spreads:?}");
        assert!(
            spreads.iter().filter(|&&n| n == 6).count() * 2 < spreads.len(),
            "six-ups are fewer than half the spreads: {spreads:?}"
        );
    }

    /// Within a group, photos run in capture order even when their file
    /// names sort the other way.
    #[test]
    fn pack_orders_a_chapter_by_capture_time_not_file_name() {
        let photos: Vec<Photo> = (0..4)
            .map(|i| timed(&format!("/{}.jpg", ["d", "c", "b", "a"][i]), 50, Some(i as i64 * 3_600)))
            .collect();
        let c = capacity_for(20, &full());
        let groups = pack(&photos, &c, &buildable_for(&full()), &Overrides::new()).unwrap();
        let order: Vec<&str> = groups
            .iter()
            .flat_map(|g| g.photos.iter().map(|&i| photos[i].path.as_str()))
            .collect();
        assert_eq!(order, ["/d.jpg", "/c.jpg", "/b.jpg", "/a.jpg"]);
    }

    #[test]
    fn event_order_is_selects_order_split_by_event() {
        // Two events, several moments each, varied aesthetics.
        // Event 0 = indices 0..6, event 1 = 6..12; two frames per moment
        // (60 s apart), moments 600 s apart; aesthetic varies so ranks differ.
        let photos: Vec<Photo> = (0..12)
            .map(|i| {
                let mut p = photo(&format!("/p{i:02}.jpg"), (i / 6) as u32, ((i * 37) % 100) as u8);
                p.near_dup_cluster = i as u32;
                p.captured_at = Some((i / 2) as i64 * 600 + (i % 2) as i64 * 60);
                p
            })
            .collect();
        let overrides = Overrides::new();
        let order = event_order(&photos, &overrides);
        let all = select(&photos, photos.len(), &overrides);
        for (event, o) in &order {
            let mut expected: Vec<usize> = all.iter().copied().filter(|&i| photos[i].event_cluster == *event).collect();
            let mut got = o.list.clone();
            expected.sort_unstable();
            got.sort_unstable();
            assert_eq!(got, expected, "event {event} holds the same photos");
        }
        // Moment-first: every moment's best precedes any moment's second.
        let moment = moments(&photos);
        for o in order.values() {
            let firsts = o.list.iter().take_while(|&&i| {
                o.list.iter().position(|&j| moment[j] == moment[i]) == o.list.iter().position(|&j| j == i)
            }).count();
            let distinct = o.list.iter().map(|&i| moment[i]).collect::<std::collections::BTreeSet<_>>().len();
            assert_eq!(firsts, distinct);
        }
    }

    /// A timed photo whose feature print is one-hot on `axis`, nudged by
    /// `offset` along the next axis: two prints on one axis are exactly their
    /// offsets apart, and prints on different axes are more than 1 apart.
    fn printed(path: &str, aesthetic: u8, at: i64, axis: usize, offset: f32) -> Photo {
        let mut print = vec![0.0f32; 8];
        print[axis] = 1.0;
        print[axis + 1] = offset;
        Photo { feature_print: Some(print), ..timed(path, aesthetic, Some(at)) }
    }

    /// Iceland 2025: four frames of one star field, 3 to 24 minutes apart and
    /// 0.11 to 0.31 apart in feature print, each far enough from the others in
    /// time to be a moment -- and a cull cluster -- of its own. The event
    /// ranked them all ahead of a different picture, so they printed side by
    /// side on one page.
    ///
    /// A look-alike goes behind every photo that is not one, not out: with
    /// room for all three it is still placed.
    #[test]
    fn event_order_puts_a_look_alike_behind_a_different_picture() {
        let photos = vec![
            printed("/portrait.jpg", 90, 0, 0, 0.0),
            printed("/portrait-again.jpg", 80, 600, 0, 0.2),
            printed("/lagoon.jpg", 40, 1_200, 3, 0.0),
        ];
        let order = event_order(&photos, &Overrides::new());
        let list: Vec<&str> = order[&0].list.iter().map(|&i| photos[i].path.as_str()).collect();
        assert_eq!(list, ["/portrait.jpg", "/lagoon.jpg", "/portrait-again.jpg"]);
    }

    /// An `Include` counts as already chosen: the best auto pick that looks
    /// like it waits behind a different picture. A pair just past
    /// `SIMILAR_DISTANCE` is not a look-alike and keeps its rank.
    #[test]
    fn event_order_measures_look_alikes_against_includes_and_at_the_threshold() {
        let near = crate::cluster::SIMILAR_DISTANCE;
        let photos = vec![
            printed("/kept.jpg", 10, 0, 0, 0.0),
            printed("/like-kept.jpg", 90, 600, 0, near),
            printed("/other.jpg", 40, 1_200, 3, 0.0),
            printed("/just-past.jpg", 30, 1_800, 3, near + 0.01),
        ];
        let order = event_order(&photos, &include(&["/kept.jpg"]));
        let list: Vec<&str> = order[&0].list.iter().map(|&i| photos[i].path.as_str()).collect();
        assert_eq!(list, ["/kept.jpg", "/other.jpg", "/just-past.jpg", "/like-kept.jpg"]);
    }
}
