# Phase 2 Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close nine outstanding Phase 2 items — the four layout-quality complaints from the first real-photo run, and the open-items ledger entries that are not gated on real photographs.

**Architecture:** Two independent lines of work. The first teaches `book::pack` that a book's first and last slots are *page halves*, not spreads, which removes structurally-blank pages and closes the packer's known page-half defect — this is the only behaviour change in the plan. The second adds four new soft-scoring terms to `book::score`, each shipped at weight `0.0` so every commit is provably behaviour-neutral and tuning stays attributable to one term at a time.

**Tech Stack:** Rust (Tauri 2 backend), Swift sidecar (untouched here), Nuxt 4 webview (untouched here), `serde`/`serde_json`, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-15-phase-2-completion-design.md`

## Global Constraints

Copied verbatim from `CLAUDE.md` and `docs/PROJECT-STATUS.md`. Every task's requirements implicitly include this section.

- **`bun run sidecar` before any `cargo` command.** `tauri-build` validates the `externalBin` path at compile time. It is wired into the Tauri hooks but *not* into bare cargo invocations. Run it once at the start of the session.
- **Bun, not npm.** Conventional Commits. **Never `git commit --no-verify`.**
- **Never use `any` in TypeScript.** oxlint warnings are failures.
- **`bun run check:build` before every commit that touches `app/**/*.vue`.** No task in this plan touches a `.vue` file; if one ends up doing so, this applies.
- **Images never leave the machine.** Nothing in this plan sends anything anywhere.
- **arm64 only, macOS 15+.** Never configure a universal build.
- **Do not trust that a test tests what it says.** Break the thing deliberately and confirm the test notices. **Mutation-check anything load-bearing and paste the evidence into the commit message.** A test that passes with the feature deleted is worse than no test.
- **Non-square fixtures are mandatory** when testing anything aspect- or area-dependent. A square box makes `height/width == 1` and hides the whole class of bug.
- **Print geometry is normalised to the spread canvas** (22.394" × 8.894"). `aspect_pref` in `templates/*.json` is a real-world inch ratio and is validator-only — the scorer never reads it.
- **Commands:** `bun run test` (vitest), `bun run test:rust` (cargo test), `bun run test:swift`, `bun run lint`, `bun run check:build`.

**Baseline before starting:** `bun run sidecar && bun run test:rust` must be green. Record the test counts; the plan expects 308 Rust unit tests passing at the start.

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `src-tauri/src/book/pack.rs` | Slot kinds, per-slot buildable sets, group tagging | 1, 3 |
| `src-tauri/src/book/pace.rs` | Places groups by slot tag; seed reaches `best_spread` | 2, 5 |
| `src-tauri/src/book/score.rs` | Four new soft terms; `best_spread` takes a seed | 5, 10, 11, 12, 13 |
| `src-tauri/src/book/cull.rs` | `Photo` gains `scene_tags` and `captured_at` | 9 |
| `src-tauri/src/book/preflight.rs` | Existence check moves to the I/O shell | 7 |
| `src-tauri/src/templates.rs` | `Weights` gains four inert fields | 8 |
| `src-tauri/src/commands.rs` | Refusal counts surfaced from `stamp_kept`/`count_keepers` | 6 |
| `templates/weights.json` | The four new weights, all `0.0` | 8 |
| `templates/20-four-up-windowpane.json`, `templates/45-three-up-mosaic-margin-left.json` | Rebalanced to use both page halves | 4 |
| `docs/PROJECT-STATUS.md`, `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md` | Ledger and weights-table corrections | 14 |

---

## Task 1: Slot kinds — tag every group with the kind of slot it was cut for

**Files:**
- Modify: `src-tauri/src/book/pack.rs` (add `SlotKind`, add `Group.slot`, set it in `pack`)
- Modify: `src-tauri/src/book/pace.rs:366-379` (the remap that rebuilds `Group`s)
- Test: `src-tauri/src/book/pack.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `pub enum SlotKind { Single, Spread }` and `Group { photos: Vec<usize>, event_cluster: u32, slot: SlotKind }`, both in `crate::book::pack`. Task 2 reads `Group.slot`; Task 3 reads `SlotKind`.

**Why:** A book is `2 single pages + (N-2)/2 spreads`. `pack` currently cuts every group as though it were going on a spread, and `pace::assemble` re-derives which groups are singles by position. Tagging makes the packer's own decision explicit so Task 2 can stop guessing and Task 3 can size by kind. This task is behaviour-neutral: nothing reads the tag yet.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/book/pack.rs`:

```rust
/// The book's first and last slots are single PAGES facing the inside
/// covers; everything between them is a spread. `pack` must say which is
/// which, because `pace::assemble` cannot correctly re-derive it from
/// position once the two kinds hold different numbers of photos.
#[test]
fn pack_tags_the_first_and_last_group_as_single_pages() {
    // 30 photos, one chapter, 20 pages -> 2 singles + 9 spreads = 11 slots.
    let photos: Vec<Photo> =
        (0..30).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
    let capacity = Capacity::from_sizes(20, &[1, 2, 3, 4, 5, 6]);
    let groups =
        pack(&photos, &capacity, &[1, 2, 3, 4, 5, 6], &Overrides::new()).expect("packs");

    assert_eq!(groups.len(), 11, "11 slots should yield 11 groups");
    assert_eq!(groups[0].slot, SlotKind::Single, "group 0 fills the opening page");
    assert_eq!(groups[10].slot, SlotKind::Single, "group 10 fills the closing page");
    for (i, g) in groups.iter().enumerate().take(10).skip(1) {
        assert_eq!(g.slot, SlotKind::Spread, "group {i} fills a spread");
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `cd src-tauri && cargo test --lib pack_tags_the_first_and_last_group_as_single_pages`
Expected: FAIL to compile — `no field 'slot' on type 'Group'` and `cannot find type 'SlotKind'`.

- [ ] **Step 3: Add `SlotKind` and the `Group` field**

In `src-tauri/src/book/pack.rs`, immediately above `pub struct Group`:

```rust
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
```

Then add the field to `Group`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Indices into the photo slice handed to `pack`.
    pub photos: Vec<usize>,
    pub event_cluster: u32,
    /// The kind of slot this group was SIZED for. Set by `pack`; read by
    /// `pace::assemble` to decide where it goes.
    pub slot: SlotKind,
}
```

- [ ] **Step 4: Set the tag in `pack`**

In `pack`, replace the `groups.push(...)` at `src-tauri/src/book/pack.rs:302-305` with:

```rust
Some(take) => {
    // `groups.len()` is the GLOBAL slot index -- it counts across
    // chapters, not within one -- which is exactly what the swing
    // above already relies on. Slot 0 and slot `slots - 1` are the
    // two single pages; everything between them is a spread.
    let kind = if groups.len() == 0 || groups.len() + 1 == slots {
        SlotKind::Single
    } else {
        SlotKind::Spread
    };
    groups.push(Group {
        photos: members[i..i + take].to_vec(),
        event_cluster: cluster,
        slot: kind,
    });
    i += take;
    slots_left -= 1;
}
```

- [ ] **Step 5: Fix the other `Group` construction site**

In `src-tauri/src/book/pace.rs`, the remap inside `assemble` (around line 366) rebuilds every `Group` to re-index into `photos`. It must carry the tag through:

```rust
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
```

- [ ] **Step 6: Fix every remaining compile error from the new field**

Run: `cd src-tauri && cargo test --lib 2>&1 | grep -E "^error" | head -30`

Every `Group { photos: ..., event_cluster: ... }` literal in test modules needs `slot: SlotKind::Spread` added (a test fixture group is a middle spread unless the test is specifically about the opening or closing page). Add `use super::SlotKind;` or `use crate::book::pack::SlotKind;` to test modules as the compiler directs. Do not add `#[derive(Default)]` to `Group` to dodge this — an untagged group is a bug, and a default would hide it.

- [ ] **Step 7: Run the test and verify it passes**

Run: `cd src-tauri && cargo test --lib pack_tags_the_first_and_last_group_as_single_pages`
Expected: PASS.

- [ ] **Step 8: Verify the whole suite is still green and behaviour is unchanged**

Run: `cd src-tauri && cargo test`
Expected: all pass, including `pace_golden_twenty_page_book`. **The golden must NOT change in this task.** If it does, the tag is being read somewhere it should not be — stop and find out where.

- [ ] **Step 9: Mutation-check the test**

Temporarily change the tag expression in `pack` to always return `SlotKind::Spread`, re-run `cargo test --lib pack_tags_the_first_and_last_group_as_single_pages`, and confirm it FAILS. Then change it to always return `SlotKind::Single` and confirm it FAILS again. Restore the real expression. Paste both results into the commit message.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/book/pack.rs src-tauri/src/book/pace.rs
git commit -m "feat(pack): tag every group with the kind of slot it fills

A book is 2 single pages facing the inside covers plus (N-2)/2 spreads,
and the two kinds hold different numbers of photos. pack now records which
kind each group was cut for instead of leaving assemble to infer it from
position.

Behaviour-neutral: nothing reads the tag yet, and the golden is unchanged.

Mutation-checked: forcing the tag to always-Spread fails the new test;
forcing always-Single fails it too."
```

---

## Task 2: `assemble` places groups by their tag instead of by position

**Files:**
- Modify: `src-tauri/src/book/pace.rs:381-412` (inside `assemble`)
- Test: `src-tauri/src/book/pace.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `Group.slot` and `SlotKind` from Task 1.
- Produces: no new API. `assemble`'s signature is unchanged.

**Why:** `assemble` currently uses `groups.first()` for page 1 and `groups.last()` for page N. When `pack` produces fewer groups than there are slots — which happens whenever photos run out — the last group it cut sits below index `slots - 1` and was sized as a *spread*, yet `groups.last()` puts it on a page half, where `pace::strongest` silently trims it. Placing by tag removes the guess.

**Behaviour change, deliberate and pinned below:** in that under-production case, page N now comes out blank instead of holding a silently-trimmed spread group. This is the same honest outcome `assemble` already produces for a middle spread with no group left.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/book/pace.rs`:

```rust
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
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `cd src-tauri && cargo test --lib pace_leaves_the_closing_page_blank_when_the_packer_ran_out_of_groups`
Expected: FAIL — the closing page carries placements, because `groups.last()` put the final spread-sized group there.

- [ ] **Step 3: Place by tag**

In `src-tauri/src/book/pace.rs`, replace the three placement selections inside `assemble`. The opening page (was `groups.first()`):

```rust
// Placed by the packer's OWN tag, not by position. When photos run out,
// `pack` produces fewer groups than there are slots, and the last group it
// cut is then a middle spread -- placing it on a page half regardless is
// what used to let `strongest` silently trim it.
let opening = groups.first().filter(|g| g.slot == SlotKind::Single);
let closing = groups
    .last()
    .filter(|g| g.slot == SlotKind::Single && !std::ptr::eq(*g, &groups[0]));
let middle: Vec<&Group> =
    groups.iter().filter(|g| g.slot == SlotKind::Spread).collect();

// Page 1: a single, facing the inside front cover.
out.push(single_page(Side::Right, opening.copied(), &pool, photos, seed));
```

The middle loop keeps its shape but drops the `skip(1).take(len-2)` slicing, because `middle` is now selected by tag:

```rust
let mut previous: Option<String> = None;
for s in 0..spread_count {
    match middle.get(s).and_then(|g| spread_pages(g, photos, lib, w, previous.as_deref())) {
```

And the closing page (was the `groups.len() > 1` check):

```rust
// The final page: a single, facing the inside back cover.
out.push(single_page(Side::Left, closing.copied(), &pool, photos, seed));
```

Add `SlotKind` to the `use crate::book::pack::{...}` line at the top of `pace.rs`.

> **Note on `std::ptr::eq`:** it guards the one-group book, where `groups.first()` and `groups.last()` are the same group and it must not be placed twice. If the borrow checker makes this awkward, the equivalent and clearer form is to compare indices: compute `let n = groups.len();` and select `closing` as `(n > 1).then(|| &groups[n - 1]).filter(|g| g.slot == SlotKind::Single)`.

- [ ] **Step 4: Run the test and verify it passes**

Run: `cd src-tauri && cargo test --lib pace_leaves_the_closing_page_blank_when_the_packer_ran_out_of_groups`
Expected: PASS.

- [ ] **Step 5: Run the whole suite**

Run: `cd src-tauri && cargo test`
Expected: all pass. **The golden must NOT change** — it packs 30 photos into 11 slots and fills every one, so it never hits the under-production path. If the golden changes, the tag selection is wrong for the fully-packed case; stop and diagnose rather than re-baselining.

- [ ] **Step 6: Mutation-check**

Revert `opening`/`closing` to `groups.first()`/`groups.last()` (dropping the `.filter`), re-run the new test, and confirm it FAILS. Restore. Paste the result into the commit message.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/book/pace.rs
git commit -m "fix(pace): place groups by the packer's slot tag, not by position

groups.first()/groups.last() disagree with what pack sized for whenever
photos run out: the last group cut is then a middle spread, and putting it
on the closing page-half let pace::strongest trim photos off it silently.

Behaviour change, pinned by a new test: the closing page now comes out
blank in that case, which is the same honest outcome assemble already
produces for a middle spread with no group left. The golden is unchanged
-- it fills all 11 slots and never takes this path.

Mutation-checked: restoring groups.first()/groups.last() fails the new
test."
```

---

## Task 3: Per-slot-kind buildable sets — the behaviour change

**Files:**
- Modify: `src-tauri/src/book/pack.rs` (`Buildable`, `Capacity`, `feasible_band`, `choose_group_size`, `pack`)
- Modify: `src-tauri/src/book/pace.rs` (`assemble`'s call into `pack`)
- Test: `src-tauri/src/book/pack.rs`, `src-tauri/src/book/pace.rs`
- Re-baseline: `src-tauri/tests/fixtures/book-20.json`

**Interfaces:**
- Consumes: `SlotKind` from Task 1.
- Produces: `pub struct Buildable { pub single: Vec<usize>, pub spread: Vec<usize> }` with `Buildable::from_library(lib: &Library) -> Buildable`, `Buildable::from_sizes(spread_sizes: &[usize], largest_half: usize) -> Buildable`, and `Buildable::for_kind(&self, kind: SlotKind) -> &[usize]`. `pack`'s signature becomes `pack(photos: &[Photo], capacity: &Capacity, buildable: &Buildable, overrides: &Overrides) -> Result<Vec<Group>, IncludeOverflow>`.

**Why — the load-bearing fact:** A 1-photo spread template has exactly one slot, and the validator forbids a slot from spanning the fold, so **one of its two halves must be empty and prints white.** All eight of the library's 1-photo templates (`03 04 05 06 28 30 36 41`) are in that state and a ninth cannot be authored. Banning size 1 from the *spread* region is therefore the only fix that does not require rendering text. Single pages keep size 1, where one photo on one page is exactly right.

This task also closes open item 4's remaining item: singles were being sized against whole-spread counts and trimmed by `pace::strongest`.

- [ ] **Step 1: Write the failing test for the spread region**

Add to `mod tests` in `src-tauri/src/book/pack.rs`:

```rust
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
    let capacity = Capacity::from_sizes(20, &[1, 2, 3, 4, 5, 6]);
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
```

- [ ] **Step 2: Run it and verify it fails**

Run: `cd src-tauri && cargo test --lib pack_never_cuts_a_one_photo_group_for_a_spread_slot`
Expected: FAIL to compile (`Buildable` does not exist). After Step 3 introduces the type but before Step 5 wires it in, it must fail on the assertion instead — confirm you see the assertion failure, not just the compile error, or the test has proved nothing.

- [ ] **Step 3: Add the `Buildable` type**

In `src-tauri/src/book/pack.rs`, below `SlotKind`:

```rust
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
    /// Derived from the loaded library, never hardcoded. The real library is
    /// currently 1..=6 for spreads, so `spread` comes out 2..=6 today, but a
    /// future template changes that and `buildable_sizes` is the one place
    /// that answers the question.
    pub fn from_library(lib: &Library) -> Buildable {
        let largest_half =
            lib.page_half_pool().iter().map(|p| p.slots.len()).max().unwrap_or(1);
        Buildable::from_sizes(&buildable_sizes(lib), largest_half)
    }

    /// As `from_library`, but from a plain list of spread sizes plus the
    /// largest page half -- so the tests in this module can run without
    /// loading template files from disk.
    pub fn from_sizes(spread_sizes: &[usize], largest_half: usize) -> Buildable {
        Buildable {
            single: (1..=largest_half.max(1)).collect(),
            spread: spread_sizes.iter().copied().filter(|&s| s >= 2).collect(),
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
```

- [ ] **Step 4: Make `feasible_band` and `choose_group_size` take per-slot bounds**

The band looks ahead over the `slots_left - 1` slots that follow this one, and those may be a *different* kind from the current slot. The lookahead bounds must therefore come from the kinds of the slots that actually follow, not from the current slot's own set. Replace both functions:

```rust
/// The least and most this slot may take without stranding a photo or
/// leaving a later slot with nothing to hold.
///
/// * take fewer than `lo` and the `slots_left - 1` slots after it cannot
///   hold the rest even at `later_largest` each, so a photo is dropped;
/// * take more than `hi` and there are not enough photos left to give every
///   later slot even `later_smallest`, so a slot comes out blank.
///
/// The lookahead bounds are passed in rather than derived from this slot's
/// own buildable set, because the slots that FOLLOW may be a different kind:
/// a chapter's last slot can be the book's closing single, whose set starts
/// at 1, while every slot before it is a spread whose set starts at 2. Using
/// this slot's own bounds for the lookahead would mis-state the band by one
/// photo at exactly the position where it matters.
fn feasible_band(
    remaining: usize,
    later_smallest: usize,
    later_largest: usize,
    slots_left: usize,
) -> (usize, usize) {
    let later = slots_left.saturating_sub(1);
    let lo = remaining.saturating_sub(later_largest.saturating_mul(later));
    let hi = remaining.saturating_sub(later_smallest.saturating_mul(later));
    (lo.min(hi), hi)
}

/// The bounds over the slots that come AFTER `index`, given the book has
/// `slots` of them in total. Slot 0 and slot `slots - 1` are singles;
/// everything between is a spread.
fn lookahead_bounds(
    index: usize,
    slots_left: usize,
    slots: usize,
    buildable: &Buildable,
) -> (usize, usize) {
    let later = slots_left.saturating_sub(1);
    if later == 0 {
        // Nothing follows, so the band collapses to [remaining, remaining]
        // regardless of what these are.
        return (1, 1);
    }
    let kinds = (index + 1)..=(index + later);
    let mut smallest = usize::MAX;
    let mut largest = 0usize;
    for i in kinds {
        let set = if i == 0 || i + 1 == slots {
            &buildable.single
        } else {
            &buildable.spread
        };
        for &s in set.iter().filter(|&&s| s > 0) {
            smallest = smallest.min(s);
            largest = largest.max(s);
        }
    }
    (if smallest == usize::MAX { 1 } else { smallest }, largest.max(1))
}
```

And `choose_group_size` takes the current slot's own set plus the lookahead bounds:

```rust
fn choose_group_size(
    remaining: usize,
    own_buildable: &[usize],
    later_smallest: usize,
    later_largest: usize,
    slots_left: usize,
    swing: i64,
) -> Option<usize> {
    if remaining == 0 || slots_left == 0 {
        return None;
    }

    // `(2r + s) / 2s` is `r / s` rounded half up, in integer arithmetic.
    let share = (2 * remaining + slots_left) / (2 * slots_left);
    let (lo, hi) = feasible_band(remaining, later_smallest, later_largest, slots_left);
    // The aim is deliberately NOT clamped into the band. Clamping it would
    // enforce the band a second time, and a second enforcement of the same
    // rule is one no test can distinguish from the first.
    let aim = (share as i64 + swing).max(0) as usize;

    // `s > 0` is not cosmetic: the caller advances by the size returned, so a
    // zero-size group would loop forever rather than fail.
    own_buildable.iter().copied().filter(|&s| s > 0 && s <= remaining).min_by_key(|&size| {
        let rest = remaining - size;
        let outside = usize::from(size < lo || size > hi);
        // The remainder is checked against the LATER slots' sizes, which is
        // what will actually have to build it.
        let strands = usize::from(rest != 0 && !is_decomposable(rest, own_buildable));
        (outside, strands, aim.abs_diff(size), std::cmp::Reverse(size))
    })
}
```

- [ ] **Step 5: Wire it through `pack`**

Change `pack`'s signature to take `buildable: &Buildable`, and inside it:

```rust
if photos.is_empty() || (buildable.single.is_empty() && buildable.spread.is_empty()) {
    return Ok(Vec::new());
}
```

`size_bounds(buildable)` at the `apportion_slots` call site becomes `buildable.union_bounds()` — `apportion_slots` divides slot *counts* across chapters and uses these only as bounds on "fewest slots that hold all its photos" and "most slots it could fill", so the union is the honest figure. Leave `apportion_slots` itself unchanged.

Inside the per-chapter loop, replace the `choose_group_size` call:

```rust
let swing = if groups.len() % 2 == 0 { -1 } else { 1 };
let index = groups.len();
let kind = if index == 0 || index + 1 == slots {
    SlotKind::Single
} else {
    SlotKind::Spread
};
let (later_smallest, later_largest) =
    lookahead_bounds(index, slots_left, slots, buildable);
match choose_group_size(
    remaining,
    buildable.for_kind(kind),
    later_smallest,
    later_largest,
    slots_left,
    swing,
) {
    Some(take) => {
        groups.push(Group {
            photos: members[i..i + take].to_vec(),
            event_cluster: cluster,
            slot: kind,
        });
        i += take;
        slots_left -= 1;
    }
    None => break,
}
```

`slots` is already in scope as `capacity.spreads as usize + capacity.singles as usize`.

- [ ] **Step 6: Update `Capacity::from_sizes` and `from_library`**

Both compute `min_photos`/`max_photos` from one book-wide smallest/largest. They must now account for the two kinds separately:

```rust
pub fn from_library(pages: u32, lib: &Library) -> Capacity {
    let b = Buildable::from_library(lib);
    let singles = 2u32;
    let spreads = (pages.saturating_sub(2)) / 2;
    let (single_lo, single_hi) = bounds_of(&b.single);
    let (spread_lo, spread_hi) = bounds_of(&b.spread);
    Capacity {
        pages,
        singles,
        spreads,
        min_photos: spreads as usize * spread_lo + singles as usize * single_lo,
        max_photos: spreads as usize * spread_hi + singles as usize * single_hi,
    }
}
```

Add the small helper beside `size_bounds`:

```rust
/// Smallest and largest usable size in one set, both floored at 1 so a
/// malformed set cannot produce a zero divisor or a zero-sized group.
fn bounds_of(sizes: &[usize]) -> (usize, usize) {
    let usable = || sizes.iter().copied().filter(|&s| s > 0);
    (usable().min().unwrap_or(1), usable().max().unwrap_or(1))
}
```

`from_sizes(pages, buildable)` keeps its `&[usize]` parameter for the tests that pass a bare list, and builds a `Buildable` internally via `Buildable::from_sizes(buildable, largest_half)` — it needs a `largest_half` too, so change its signature to `from_sizes(pages: u32, spread_sizes: &[usize], largest_half: usize)`. Update its callers in `mod tests` and in `recommend_page_count` accordingly.

`size_bounds` is now unused; delete it rather than leaving a second, subtly different bounds function beside `bounds_of`.

- [ ] **Step 7: Update `pace::assemble`'s call**

```rust
let buildable = Buildable::from_library(lib);
let cap = Capacity::from_library(pages, lib);
let groups = pack(&kept, &cap, &buildable, overrides)
    .map_err(BookError::IncludedExceedCapacity)?;
```

Delete the now-unused `let sizes = buildable_sizes(lib);` line if nothing else in `assemble` reads it.

- [ ] **Step 8: Run the new test and the suite**

Run: `cd src-tauri && cargo test --lib pack_never_cuts_a_one_photo_group_for_a_spread_slot`
Expected: PASS.

Run: `cd src-tauri && cargo test`
Expected: several failures. Two categories, and they must be triaged differently:
- **Capacity/minimum-photos tests** — expected to change, because a 20-page book's minimum rises from `11 × 1 = 11` to `2 × 1 + 9 × 2 = 20`. Update the expected numbers.
- **`pace_spread_density_varies_across_a_book_rather_than_converging`** — this asserts on the library's declared `density`, and removing size 1 removes `Sparse` from every spread. **Do not simply re-baseline it.** Re-examine what it can still prove; Task 4 restores real density variety at sizes 3 and 4. If it cannot pass until Task 4, mark it `#[ignore]` with a comment naming Task 4, and un-ignore it there.

- [ ] **Step 9: Re-baseline the golden, and read the diff**

Run: `cd src-tauri && UPDATE_GOLDEN=1 cargo test --lib pace_golden_twenty_page_book`
Then: `git diff --stat src-tauri/tests/fixtures/book-20.json` and inspect the change.

Record, for the commit message: the number of placements before and after, the number of dropped photos before and after, the number of printed pages with zero placements before and after, and the set of distinct template ids before and after. The pre-change figures are in `docs/PROJECT-STATUS.md`: 24 placements, 6 dropped, 7 distinct templates, 0 blank spread slots but 2 blank printed pages (pages 5 and 9, both from `03-hero-left-text-right`).

**If blank printed pages did not go down, this task has not done its job** — stop and diagnose rather than committing the new golden.

- [ ] **Step 10: Mutation-check**

Change `Buildable::from_sizes`'s spread filter from `s >= 2` to `s >= 1`, re-run `cargo test --lib pack_never_cuts_a_one_photo_group_for_a_spread_slot`, and confirm it FAILS. Restore. Paste into the commit message.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/book/pack.rs src-tauri/src/book/pace.rs src-tauri/tests/fixtures/book-20.json
git commit -m "feat(pack): size the first and last slot as page halves, not spreads

A 1-photo spread template has one slot; the validator forbids a slot from
spanning the fold; so one of its halves is necessarily empty and prints
white. All eight 1-photo templates are in that state and a ninth cannot be
authored -- the blank page is structural, not an authoring accident.

pack now takes a Buildable carrying one size set per slot kind: singles get
1..=largest page half, spreads get the library's counts less 1. This also
closes the known defect where a single page was sized against a whole
spread and pace::strongest silently trimmed the surplus.

A 20-page book's minimum rises from 11 photos to 20, which is a correction
-- the old figure recommended books that printed half-blank.

Golden re-baselined; the diff is [FILL IN FROM STEP 9].

Mutation-checked: relaxing the spread filter to s >= 1 fails the new test."
```

---

## Task 4: Rebalance the two templates whose empty half is an authoring choice

**Files:**
- Modify: `templates/20-four-up-windowpane.json`
- Modify: `templates/45-three-up-mosaic-margin-left.json`
- Test: `tests/templates.test.ts`, and `src-tauri/src/book/pace.rs`'s density test from Task 3 Step 8

**Interfaces:**
- Consumes: nothing. Independent of Tasks 1-3 except that it restores what Task 3 removed.
- Produces: no code API. Restores `Sparse`/`Medium` density variety at spread sizes 3 and 4.

**Why:** Ten templates have a page half with zero photo slots. Eight are structural (size 1, Task 3). Two are not: `20-four-up-windowpane` puts all four slots on the left page, and `45-three-up-mosaic-margin-left` puts all three on the left. Both print a blank right page every time they are chosen, and both can simply be re-authored to use both halves. This is also what restores the density variety Task 3's ban removed.

- [ ] **Step 1: Confirm the current state**

Run:

```bash
python3 -c "
import json, glob, os
for f in sorted(glob.glob('templates/*.json')):
    if f.endswith('weights.json'): continue
    d = json.load(open(f))
    if 'slots' not in d: continue
    left = sum(1 for s in d['slots'] if s['rect'][0] + s['rect'][2] <= 0.5 + 1e-9)
    right = len(d['slots']) - left
    if left == 0 or right == 0:
        print(f'{os.path.basename(f):45s} L={left} R={right}')
"
```

Expected: ten lines. After this task, exactly eight — all of them 1-photo templates.

- [ ] **Step 2: Rebalance `20-four-up-windowpane.json`**

A windowpane is a 2×2 grid. Move it to span the spread as two panes per page: slots 1 and 3 on the left half, slots 2 and 4 on the right half, keeping the existing row heights and gutters. **No slot may cross `x = 0.5`** — the validator rejects it with `TemplateError::SpansFold`, so keep every rect entirely inside `[0, 0.5)` or entirely inside `(0.5, 1]`, clear of the gutter dead band at `0.491203..0.508797`.

Update `aspect_pref` on every slot you move: it is a **real-world inch ratio**, not the normalised rect ratio, and the canvas is 2.518:1. Compute it as `(rect.w * 22.394) / (rect.h * 8.894)` and give the declared range a tolerance around that number. Getting this wrong fails `tests/templates.test.ts` rather than mis-scoring a layout, so the test is the check.

- [ ] **Step 3: Rebalance `45-three-up-mosaic-margin-left.json`**

Three slots cannot split evenly, so put two on the left half and one on the right. Same fold and `aspect_pref` rules as Step 2. If the name no longer describes the layout, rename the file and its `id` field together — `pace::Page.template_id` stores the id and the manifest must be able to name what it used.

- [ ] **Step 4: Run the template validator**

Run: `bun run test -- templates`
Expected: PASS. If `aspect_pref` is out of range the failure names the slot; fix the declared range, not the rect.

- [ ] **Step 5: Run the ignored real-library test**

Run: `cd src-tauri && cargo test -- --ignored`
Expected: `templates_real_library_covers_every_group_size_from_one_to_six` still passes — moving slots between halves changes neither template's photo count.

- [ ] **Step 6: Re-check Step 1**

Run the Step 1 command again. Expected: exactly eight lines, all 1-photo templates.

- [ ] **Step 7: Restore the density test**

If Task 3 Step 8 marked `pace_spread_density_varies_across_a_book_rather_than_converging` as `#[ignore]`, remove the attribute now and run it.

Run: `cd src-tauri && cargo test --lib pace_spread_density_varies_across_a_book_rather_than_converging`
Expected: PASS. If it still fails, the density axis genuinely is flat without size 1 and the honest response is to say so in `PROJECT-STATUS.md` (Task 14) rather than weaken the assertion. **Do not change it to assert on photo counts** — 2, 3 and 4 are all `medium`, so a count-based assertion passes while the axis is still flat, which is exactly the decorative-test shape this project keeps finding.

- [ ] **Step 8: Run the full suite**

Run: `cd src-tauri && cargo test && bun run test`
Expected: all pass. The golden uses `frozen_library()`, a fixture library, so it is unaffected by real template edits.

- [ ] **Step 9: Commit**

```bash
git add templates/
git commit -m "fix(templates): use both page halves in the windowpane and mosaic

20-four-up-windowpane put all four slots on the left page and
45-three-up-mosaic-margin-left all three, so each printed a blank right
page every time it was chosen. Unlike the eight 1-photo templates, whose
empty half is structural, these two were an authoring choice.

Rebalancing them also restores the spread density variety that banning
size-1 spreads removed: both now offer a genuinely different density at
their photo count."
```

---

## Task 5: The seed reaches `best_spread`

**Files:**
- Modify: `src-tauri/src/book/score.rs:253-281` (`best_spread`)
- Modify: `src-tauri/src/book/pace.rs` (`spread_pages`, `repace`)
- Test: `src-tauri/src/book/pace.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `best_spread(templates: &[&SpreadTemplate], photos: &[&Photo], previous: Option<&str>, w: &Weights, seed: u64) -> Option<(&SpreadTemplate, Vec<usize>, f64)>`. Task 11 and Task 13 call it.

**Why:** `assemble(photos, pages, lib, w, seed)` passes `seed` only to `single_page` → `best_single` → `tie_break`. `best_spread` takes no seed at all and breaks ties on `t.id < bt.id`. The seed therefore influences only the first and last pages, never a middle spread — so Phase 3's "regenerate this spread advances the seed" is **unimplementable as the code stands**, because advancing the seed changes nothing. This is a Phase 3 blocker to be settled before Phase 3 is planned.

- [ ] **Step 1: Write the failing test**

The fixture library already engineers an exact tie for `best_single` — `f5`'s and `f7`'s right halves are deliberately identical. This test needs the same at spread level. Add to `mod tests` in `src-tauri/src/book/pace.rs`:

```rust
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
```

> **If this test cannot be made to fail-then-pass** because `fixture_library` has no two spread templates that tie exactly at the same photo count, add one. Copy an existing spread fixture, give it a new id, and make its slot rects produce an identical score — the same trick `f5`/`f7` already use for the single-page pool. Do **not** loosen the tie comparison to make a near-tie count; exact equality is the definition of the tie the seed exists to break, and anything looser lets the seed override a real preference.

- [ ] **Step 2: Run it and verify it fails**

Run: `cd src-tauri && cargo test --lib pace_uses_the_seed_to_break_a_tie_between_middle_spreads`
Expected: FAIL — every seed yields the same template id.

- [ ] **Step 3: Give `best_spread` a seed**

In `src-tauri/src/book/score.rs`, replace `best_spread`:

```rust
/// Enumerates every eligible template and every assignment, returning the
/// best. Brute force: at most 6! = 720 assignments over box arithmetic with
/// no pixels touched.
///
/// Exact ties are broken by the SEED, the same way `pace::best_single` does
/// it. Breaking them on template id instead was deterministic but seed-blind,
/// which left "regenerate this spread advances the seed" unimplementable for
/// every middle spread: advancing the seed changed nothing.
///
/// Exact equality is the definition of the tie the seed exists to break;
/// anything looser would let the seed override a real preference.
pub fn best_spread<'a>(
    templates: &[&'a SpreadTemplate],
    photos: &[&Photo],
    previous: Option<&str>,
    w: &Weights,
    seed: u64,
) -> Option<(&'a SpreadTemplate, Vec<usize>, f64)> {
    let mut best = f64::NEG_INFINITY;
    let mut tied: Vec<(&SpreadTemplate, Vec<usize>)> = Vec::new();

    for t in templates {
        if t.photo_count() != photos.len() {
            continue;
        }
        for assignment in permutations(photos.len()) {
            let Some(score) = score_spread(t, photos, &assignment, previous, w) else {
                continue;
            };
            if score > best {
                best = score;
                tied.clear();
                tied.push((t, assignment));
            } else if score == best {
                tied.push((t, assignment));
            }
        }
    }

    if tied.is_empty() {
        return None;
    }
    // Sorted by template id before indexing, so the candidate ORDER is a
    // total order over the inputs rather than enumeration order -- the seed
    // picks among candidates, it does not depend on how they were found.
    tied.sort_by(|(a, ai), (b, bi)| a.id.cmp(&b.id).then(ai.cmp(bi)));
    let pick = crate::book::pace::tie_break(seed, tied.len());
    let (template, assignment) = tied.swap_remove(pick);
    Some((template, assignment, best))
}
```

- [ ] **Step 4: Make `tie_break` visible to `score`**

In `src-tauri/src/book/pace.rs`, change `fn tie_break` to `pub(crate) fn tie_break`. It is the project's only stochastic choice and having two copies of it would be exactly the duplicated-authority problem `cull` already had to fix.

- [ ] **Step 5: Update the two callers**

In `src-tauri/src/book/pace.rs`, `spread_pages` gains a `seed: u64` parameter and forwards it:

```rust
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
```

Its call site inside `assemble`'s middle loop passes `seed`. `repace` also calls `best_spread`; give `repace` a `seed: u64` parameter, forward it, and update `assemble`'s `repace(&mut book, lib, photos, w)` call to `repace(&mut book, lib, photos, w, seed)`. Update `repace`'s callers in `mod tests` too.

- [ ] **Step 6: Run the new test**

Run: `cd src-tauri && cargo test --lib pace_uses_the_seed_to_break_a_tie_between_middle_spreads`
Expected: PASS.

- [ ] **Step 7: Run the suite**

Run: `cd src-tauri && cargo test`
Expected: all pass. `pace_is_deterministic_for_the_same_seed` must still pass — the seed picks among exact ties only, so the same seed still gives the same book. The golden may change if its library contains an exact tie; if it does, re-baseline with `UPDATE_GOLDEN=1` and say so in the commit message.

- [ ] **Step 8: Mutation-check**

Replace `crate::book::pace::tie_break(seed, tied.len())` with a constant `0`, re-run the new test, and confirm it FAILS. Restore. Paste into the commit message.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/book/score.rs src-tauri/src/book/pace.rs
git commit -m "feat(score): let the seed break ties between middle spreads

best_spread broke ties on template id, so the seed reached only the first
and last pages. Phase 3's 'regenerate this spread advances the seed' was
therefore unimplementable for every middle spread -- advancing the seed
changed nothing.

It now collects every candidate at the maximum score, sorts them by id for
a total order independent of enumeration order, and indexes with the same
tie_break the single pages already use. Exact equality only: anything
looser would let the seed override a real preference.

This unblocks Phase 3 without building any of it.

Mutation-checked: pinning the tie-break index to 0 fails the new test."
```

---

## Task 6: `from_features` refusals stop being silent

**Files:**
- Modify: `src-tauri/src/commands.rs:306-321` (`stamp_kept`), `:352-360` (`count_keepers`), and their callers
- Modify: `src-tauri/src/commands.rs:2444` (misattributed comment)
- Test: `src-tauri/src/commands.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `stamp_kept(ok: &mut [serde_json::Value]) -> usize` and `count_keepers(photos: &[serde_json::Value]) -> (usize, usize)`, both returning the count of records `from_features` refused.

**Why:** `from_features` refuses a record missing a field the book depends on — that is the point of it, and `photos_from_records`/`resolve_photos` surface the refusal loudly. But `stamp_kept` and `count_keepers` consume it through `filter_map`, so a malformed record is silently skipped: the photo comes back `kept: false`, or the completion notification undercounts, with no error anywhere. The `filter_map` shape predates the strictness and now has a larger blast radius.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/commands.rs`:

```rust
/// A record `from_features` refuses is currently skipped in silence here:
/// the photo comes back `kept: false` with nothing reported anywhere. That
/// is the invisible failure the strictness was added to prevent, so the
/// count of refusals must reach the caller.
#[test]
fn stamp_kept_reports_how_many_records_it_could_not_parse() {
    let mut records = vec![
        // A complete, parseable record.
        full_feature_record("/a.jpg", "ha"),
        // Missing `faces` entirely -- a missing KEY, not an empty array,
        // which `from_features` refuses because every gutter and
        // safe-margin rejection would otherwise go inert.
        serde_json::json!({
            "path": "/b.jpg", "hash": "hb", "width": 4000, "height": 3000,
            "isUtility": false, "aestheticPct": 50, "sharpnessPct": 50,
            "nearDupCluster": 1, "eventCluster": 0,
            "faceAreaFraction": 0.0, "palette": []
        }),
    ];

    let refused = stamp_kept(&mut records);

    assert_eq!(refused, 1, "one record was unparseable and must be reported");
    assert_eq!(records[1]["kept"], serde_json::json!(false));
}
```

> **`full_feature_record` must be a real, complete record.** If no such helper exists in `mod tests`, build one from the fixture shape already used around `src-tauri/src/commands.rs:2594` — it must carry `path hash width height isUtility aestheticPct sharpnessPct nearDupCluster eventCluster faces faceAreaFraction palette`. A fixture that is itself unparseable makes this test assert `refused == 2` and prove nothing about the parseable branch.

- [ ] **Step 2: Run it and verify it fails**

Run: `cd src-tauri && cargo test --lib stamp_kept_reports_how_many_records_it_could_not_parse`
Expected: FAIL to compile — `stamp_kept` returns `()`.

- [ ] **Step 3: Return the refusal count**

```rust
/// Returns how many records `from_features` REFUSED. A refusal is not a
/// benign skip: the photo comes back `kept: false`, which is
/// indistinguishable from the engine having culled it, so the count has to
/// reach someone who can report it.
pub(crate) fn stamp_kept(ok: &mut [serde_json::Value]) -> usize {
    let parsed: Vec<crate::book::cull::Photo> =
        ok.iter().filter_map(crate::book::cull::from_features).collect();
    let refused = ok.len() - parsed.len();

    // No overrides: this runs while the analysis is still finishing, before
    // the contact sheet exists, so there is nothing the user can have decided
    // yet. Their decisions are applied afterwards by `kept_paths`, through
    // the same `cull`.
    let kept: std::collections::HashSet<String> =
        kept_paths(&parsed, &Overrides::new()).into_iter().collect();

    for features in ok.iter_mut() {
        let survives = features["path"].as_str().is_some_and(|p| kept.contains(p));
        features["kept"] = survives.into();
    }
    refused
}
```

And `count_keepers`:

```rust
/// Returns `(keepers, refused)`. See `stamp_kept` on why the refusal count
/// is returned rather than swallowed: an undercounted notification is
/// indistinguishable from a folder with fewer good photos in it.
pub(crate) fn count_keepers(photos: &[serde_json::Value]) -> (usize, usize) {
    let parsed: Vec<crate::book::cull::Photo> =
        photos.iter().filter_map(crate::book::cull::from_features).collect();
    let refused = photos.len() - parsed.len();
    (crate::book::cull::cull(&parsed, &Overrides::new()).len(), refused)
}
```

- [ ] **Step 4: Surface it at the callers**

Find them: `cd src-tauri && grep -n "stamp_kept(\|count_keepers(" src/commands.rs`

At each call site, log the refusal when non-zero, using the same logging facility the file already uses (check the top of `commands.rs` for whether it is `log::warn!`, `eprintln!` or `tauri::...`; match it rather than introducing a new one):

```rust
let refused = stamp_kept(&mut ok);
if refused > 0 {
    log::warn!(
        "{refused} analysed photo(s) were unparseable and are marked not-kept; \
         the book will not contain them"
    );
}
```

Do the same for `count_keepers`'s `(keepers, refused)` destructuring.

- [ ] **Step 5: Fix the misattributed comment**

At `src-tauri/src/commands.rs:2444`, the doc says a record is "silently dropped by `from_features`'s `filter_map`". The `filter_map` belongs to the caller, not to `from_features`. Correct it to name the caller, and note that the drop is no longer silent.

- [ ] **Step 6: Run the test and the suite**

Run: `cd src-tauri && cargo test --lib stamp_kept_reports_how_many_records_it_could_not_parse && cargo test`
Expected: all pass.

- [ ] **Step 7: Mutation-check**

Change `let refused = ok.len() - parsed.len();` to `let refused = 0;`, re-run the new test, confirm it FAILS. Restore. Paste into the commit message.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "fix(commands): report the records from_features refused

from_features is strict on purpose, and photos_from_records/resolve_photos
surface a refusal loudly. stamp_kept and count_keepers consumed it through
filter_map, so there a malformed record was silently skipped -- the photo
came back kept: false, or the completion notification undercounted, with no
error anywhere. The filter_map shape predates the strictness and now has a
larger blast radius.

Both return the refusal count and their callers log it.

Mutation-checked: hardcoding the count to 0 fails the new test."
```

---

## Task 7: `preflight_core` becomes genuinely pure

**Files:**
- Modify: `src-tauri/src/book/preflight.rs:79-114` (the three shells), `:155-183` (the core)
- Test: `src-tauri/src/book/preflight.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `fn preflight_core(book: &Book, photos: &[Photo], bleed: &[Vec<BleedEdge>], available_bytes: u64, missing: &std::collections::BTreeSet<String>) -> Vec<Finding>`, private to the module.

**Why:** Its doc comment says "No I/O" and it performs one `Path::exists()` per placement. Minor in isolation, but it matters to Phase 3: the "pure core" is not actually callable in a hot loop or off-disk, which is the entire reason the pure-core split exists. Either the check moves out or the comment is corrected; the check moving out is what Phase 3 actually needs.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/book/preflight.rs`:

```rust
/// The pure core must decide from its arguments alone. Handed a photo whose
/// path does not exist on disk but which is NOT in the missing set, it must
/// raise no moved-source finding -- proving it consulted the argument
/// rather than the filesystem.
#[test]
fn preflight_core_reads_the_missing_set_not_the_filesystem() {
    let book = one_placement_book();
    let photos = vec![preflight_photo("/definitely/not/on/disk.jpg")];
    let bleed = vec![Vec::new(); 1];

    let findings =
        preflight_core(&book, &photos, &bleed, u64::MAX, &std::collections::BTreeSet::new());

    assert!(
        !findings.iter().any(|f| f.message.contains("no longer exists")),
        "the core touched the filesystem instead of reading the missing set: {findings:?}"
    );

    let missing: std::collections::BTreeSet<String> =
        ["/definitely/not/on/disk.jpg".to_string()].into_iter().collect();
    let findings = preflight_core(&book, &photos, &bleed, u64::MAX, &missing);
    assert!(
        findings.iter().any(|f| {
            f.severity == Severity::Block && f.message.contains("no longer exists")
        }),
        "a path in the missing set must still Block: {findings:?}"
    );
}
```

> **`one_placement_book` and `preflight_photo`** are fixture helpers. If `mod tests` in this file already has equivalents under different names, use those rather than adding duplicates — `cd src-tauri && grep -n "fn .*book()\|fn .*photo(" src/book/preflight.rs`. The photo must be **non-square** (e.g. 4000×3000) and resolve above 200 DPI in its slot, or the DPI Block fires and masks what this test is about.

- [ ] **Step 2: Run it and verify it fails**

Run: `cd src-tauri && cargo test --lib preflight_core_reads_the_missing_set_not_the_filesystem`
Expected: FAIL to compile — `preflight_core` takes four arguments.

- [ ] **Step 3: Move the check out**

Change the core's signature and its doc comment, and replace the existence check:

```rust
/// The pure core: every check in spec 5.6 over already-known data.
///
/// Genuinely no I/O. Both facts it cannot compute -- free disk space and
/// which source files have moved -- are ARGUMENTS, so Phase 3 can re-run
/// this on an edited book in a loop without touching the filesystem. That
/// is the whole reason the core/shell split exists; it previously did one
/// `Path::exists()` per placement and the doc comment claiming otherwise
/// was wrong.
fn preflight_core(
    book: &Book,
    photos: &[Photo],
    bleed: &[Vec<BleedEdge>],
    available_bytes: u64,
    missing: &std::collections::BTreeSet<String>,
) -> Vec<Finding> {
```

and inside the placement loop:

```rust
// Source file moved or deleted since analysis -- BLOCK. Decided by the
// caller, which is the only part of pre-flight that may read a disk.
if missing.contains(&photo.path) {
    findings.push(Finding {
        severity: Severity::Block,
        page: page.number,
        photo_path: photo.path.clone(),
        message: format!(
            "Source file {} no longer exists at its analysed path",
            photo.path
        ),
    });
}
```

Remove the now-unused `use std::path::Path` import only if nothing else in the file uses it — `available_bytes` takes a `&Path`, so it almost certainly stays.

- [ ] **Step 4: Compute the missing set in the shell**

Add beside `placement_count`:

```rust
/// The one filesystem read pre-flight's shell performs on behalf of the
/// core: which placed photos are no longer at their analysed path.
///
/// Built once over the distinct paths in the book rather than once per
/// placement, so a photo used twice is statted once.
fn missing_sources(book: &Book, photos: &[Photo]) -> std::collections::BTreeSet<String> {
    book.pages
        .iter()
        .flat_map(|p| p.placements.iter())
        .filter_map(|pl| photos.get(pl.photo_index))
        .map(|photo| photo.path.clone())
        .collect::<std::collections::BTreeSet<String>>()
        .into_iter()
        .filter(|path| !Path::new(path).exists())
        .collect()
}
```

Then update all three shells to pass it:

```rust
pub fn preflight_with_bleed(
    book: &Book,
    photos: &[Photo],
    output_dir: &Path,
    bleed: &[Vec<BleedEdge>],
) -> Vec<Finding> {
    let available = available_bytes(output_dir);
    let missing = missing_sources(book, photos);
    preflight_core(book, photos, bleed, available, &missing)
}

pub fn preflight_with_space(
    book: &Book,
    photos: &[Photo],
    _output_dir: &Path,
    available_bytes: u64,
) -> Vec<Finding> {
    let bleed = vec![Vec::new(); placement_count(book)];
    let missing = missing_sources(book, photos);
    preflight_core(book, photos, &bleed, available_bytes, &missing)
}
```

Update `preflight`'s doc comment, which currently says the exists-check lives "inside the pure core".

- [ ] **Step 5: Run the test and the suite**

Run: `cd src-tauri && cargo test --lib preflight && cargo test`
Expected: all pass, including `preflight_blocks_a_source_file_that_no_longer_exists` — it goes through a shell, so it still reads a real disk and still blocks.

- [ ] **Step 6: Mutation-check**

Delete the `missing.contains(...)` block entirely, re-run `preflight_blocks_a_source_file_that_no_longer_exists` and the new test, and confirm BOTH fail. Restore. Paste into the commit message — this proves the shell test still reaches the moved-file rule after the refactor, which is the thing most likely to break silently here.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/book/preflight.rs
git commit -m "refactor(preflight): make the pure core actually pure

Its doc comment said 'No I/O' and it did one Path::exists() per placement.
The check moves out to the shell, which passes in the set of missing paths,
so Phase 3 can re-run pre-flight on an edited book in a loop without
touching the filesystem -- the reason the core/shell split exists.

The shell also stats each distinct path once rather than once per
placement.

Mutation-checked: deleting the moved-source rule fails both the existing
shell test and the new core test."
```

---

## Task 8: `Weights` gains four inert fields

**Files:**
- Modify: `src-tauri/src/templates.rs:212-236` (`Weights` and its `Default`)
- Modify: `templates/weights.json`
- Test: `src-tauri/src/templates.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `Weights` gains `pub face_quality: f64`, `pub spread_diversity: f64`, `pub gutter_saliency: f64`, `pub hero_prominence: f64`. Tasks 10-13 each read one.

**Why:** Every new term ships at weight `0.0`. This project has no ground truth for "a better book" beyond the user's eye, so landing four uncalibrated terms live would move the golden wholesale and leave a green suite proving nothing. Inert keeps each commit provably behaviour-neutral and makes tuning attributable, one term at a time. `templates/weights.json` is already hot-reloadable, so raising a weight later needs no rebuild.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/templates.rs`:

```rust
/// The four terms added for Phase 2 completion ship INERT. A weights file
/// written before they existed must still load, with the new terms at zero
/// -- otherwise every saved project's weights file becomes unloadable.
#[test]
fn templates_weights_default_the_new_terms_to_zero_on_an_older_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("weights.json");
    std::fs::write(
        &path,
        r#"{"aspect_fit":1.0,"saliency_retention":0.8,"face_area_retention":1.2,
            "hero_match":0.6,"resolution_headroom":0.4,"palette_harmony":0.2,
            "variety":0.5}"#,
    )
    .expect("write");

    let w = Weights::load(&path).expect("an older weights file must still load");

    assert_eq!(w.aspect_fit, 1.0);
    assert_eq!(w.face_quality, 0.0);
    assert_eq!(w.spread_diversity, 0.0);
    assert_eq!(w.gutter_saliency, 0.0);
    assert_eq!(w.hero_prominence, 0.0);
}

/// A typo'd weight name must FAIL rather than read as 0.0. Silently
/// treating `pallete_harmony` as an unknown key and leaving the real term
/// at its default is exactly the invisible failure this project keeps
/// finding.
#[test]
fn templates_weights_reject_an_unknown_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("weights.json");
    std::fs::write(
        &path,
        r#"{"aspect_fit":1.0,"saliency_retention":0.8,"face_area_retention":1.2,
            "hero_match":0.6,"resolution_headroom":0.4,"pallete_harmony":0.2,
            "variety":0.5}"#,
    )
    .expect("write");

    assert!(
        Weights::load(&path).is_err(),
        "a misspelled weight name must be an error, not a silent zero"
    );
}
```

> `tempfile` is already a dev-dependency of this crate — the existing `templates_weights_fall_back_to_defaults_when_the_file_is_absent` test uses the same pattern. Match whatever that test does rather than adding a dependency.

- [ ] **Step 2: Run them and verify they fail**

Run: `cd src-tauri && cargo test --lib templates_weights_`
Expected: the first fails to compile (`no field 'face_quality'`); the second fails its assertion (the unknown key is currently ignored).

- [ ] **Step 3: Add the fields**

```rust
/// Soft-term weights, hot-reloadable from `templates/weights.json` so taste
/// is tunable without a rebuild. Hard constraints are NOT weighted and are
/// deliberately absent here.
///
/// `deny_unknown_fields` is load-bearing: without it a misspelled weight
/// name is ignored and the real term silently keeps its default, which is
/// indistinguishable from the file having been written correctly.
///
/// The four terms below it carry `#[serde(default)]` INDIVIDUALLY rather
/// than the struct carrying it wholesale: a weights file written before
/// they existed must still load, but a TRUNCATED file missing one of the
/// original seven must still fail loudly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub aspect_fit: f64,
    pub saliency_retention: f64,
    pub face_area_retention: f64,
    pub hero_match: f64,
    pub resolution_headroom: f64,
    pub palette_harmony: f64,
    pub variety: f64,
    /// Vision's own blur/exposure/pose score for the best face in the photo.
    /// Ships at 0.0 -- see the Phase 2 completion design, §4.0.
    #[serde(default)]
    pub face_quality: f64,
    /// How unlike each other the photos on one spread are. Ships at 0.0.
    #[serde(default)]
    pub spread_diversity: f64,
    /// Penalty for generic salient content falling in the gutter dead band.
    /// Ships at 0.0.
    #[serde(default)]
    pub gutter_saliency: f64,
    /// Rewards a dominant hero slot when the group holds a standout photo.
    /// Ships at 0.0.
    #[serde(default)]
    pub hero_prominence: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            aspect_fit: 1.0,
            saliency_retention: 0.8,
            face_area_retention: 1.2,
            hero_match: 0.6,
            resolution_headroom: 0.4,
            palette_harmony: 0.2,
            variety: 0.5,
            // Inert on purpose. `Weights::default()` must agree with the
            // shipped weights.json or the golden and the app disagree.
            face_quality: 0.0,
            spread_diversity: 0.0,
            gutter_saliency: 0.0,
            hero_prominence: 0.0,
        }
    }
}
```

- [ ] **Step 4: Update the shipped weights file**

`templates/weights.json`:

```json
{
  "aspect_fit": 1.0,
  "saliency_retention": 0.8,
  "face_area_retention": 1.2,
  "hero_match": 0.6,
  "resolution_headroom": 0.4,
  "palette_harmony": 0.2,
  "variety": 0.5,
  "face_quality": 0.0,
  "spread_diversity": 0.0,
  "gutter_saliency": 0.0,
  "hero_prominence": 0.0
}
```

- [ ] **Step 5: Run the tests and the suite**

Run: `cd src-tauri && cargo test --lib templates_weights_ && cargo test && bun run test`
Expected: all pass. **The golden must not change** — the new fields are zero and nothing reads them yet.

- [ ] **Step 6: Mutation-check**

Remove `#[serde(deny_unknown_fields)]`, re-run `templates_weights_reject_an_unknown_key`, confirm it FAILS. Restore. Then remove `#[serde(default)]` from `face_quality`, re-run `templates_weights_default_the_new_terms_to_zero_on_an_older_file`, confirm it FAILS. Restore. Paste both into the commit message.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/templates.rs templates/weights.json
git commit -m "feat(templates): add four soft-term weights, all inert

face_quality, spread_diversity, gutter_saliency and hero_prominence, each
at 0.0. This project has no ground truth for 'a better book' beyond the
user's eye, so four uncalibrated terms landing live would move the golden
wholesale and leave a green suite proving nothing. Inert keeps every commit
behaviour-neutral and makes tuning attributable one term at a time.

The new fields default individually so an older weights.json still loads,
while the original seven stay required so a truncated file still fails.
deny_unknown_fields stops a misspelled weight reading as a silent zero.

Mutation-checked: dropping deny_unknown_fields fails the typo test;
dropping serde(default) on face_quality fails the older-file test."
```

---

## Task 9: `Photo` gains `scene_tags` and `captured_at`

**Files:**
- Modify: `src-tauri/src/book/cull.rs` (`Photo`, `parse_features`)
- Test: `src-tauri/src/book/cull.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `Photo` gains `pub scene_tags: Vec<String>` and `pub captured_at: Option<i64>`. Task 11 reads both.

**Why:** `spread_diversity` needs signals `Photo` does not carry. **No wire change is needed:** `finalize_photos` strips only `phash` before the record reaches the webview, so `sceneTags` and `exif.captureDate` both survive the round trip that `generate_book` sends back.

**The strictness classes differ and getting them backwards is the trap.** `sceneTags` is always emitted by Swift, so it is **required** — an absent key is a pipeline bug. `exif.captureDate` is legitimately `null` on a photo with no EXIF date, so it is **optional**. Requiring `captureDate` would fail every book containing one scanned photo; defaulting `sceneTags` would silently degrade the diversity term to a constant.

`phash` is deliberately not used despite being the most direct similarity signal: it is stripped before the webview precisely because JavaScript loses precision above 2^53, and re-widening the wire is a larger change than this term justifies.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/book/cull.rs`:

```rust
/// `sceneTags` is always emitted by Swift, so its ABSENCE is a pipeline bug
/// and must be refused. `exif.captureDate` is legitimately null on a photo
/// with no EXIF date, so its absence must be tolerated. Getting these two
/// the wrong way round either fails every book containing one scanned
/// photo, or silently degrades the diversity term to a constant.
#[test]
fn cull_from_features_requires_scene_tags_but_tolerates_a_missing_capture_date() {
    let mut v = complete_record();
    v["sceneTags"] = serde_json::json!(["beach", "sunset"]);
    v["exif"] = serde_json::json!({ "captureDate": 1_700_000_000.0 });
    let photo = from_features(&v).expect("a complete record parses");
    assert_eq!(photo.scene_tags, vec!["beach".to_string(), "sunset".to_string()]);
    assert_eq!(photo.captured_at, Some(1_700_000_000));

    // A null capture date is a real answer, not a malformed record.
    let mut no_date = complete_record();
    no_date["sceneTags"] = serde_json::json!([]);
    no_date["exif"] = serde_json::json!({ "captureDate": serde_json::Value::Null });
    let photo = from_features(&no_date).expect("a photo with no EXIF date still parses");
    assert_eq!(photo.captured_at, None);
    assert!(photo.scene_tags.is_empty(), "an empty tag list is a real answer");

    // A MISSING sceneTags key is not.
    let mut no_tags = complete_record();
    no_tags.as_object_mut().expect("object").remove("sceneTags");
    assert!(
        from_features(&no_tags).is_none(),
        "a record with no sceneTags key must be refused"
    );
}
```

> **`complete_record`** must be a helper returning a fully-populated feature record including `sceneTags`. If `mod tests` in `cull.rs` has an equivalent under another name, use it. The record must carry every key `parse_features` reads, or the third assertion passes for the wrong reason.

- [ ] **Step 2: Run it and verify it fails**

Run: `cd src-tauri && cargo test --lib cull_from_features_requires_scene_tags_but_tolerates_a_missing_capture_date`
Expected: FAIL to compile — `no field 'scene_tags' on type 'Photo'`.

- [ ] **Step 3: Add the fields**

In `Photo`, after `capture_quality`:

```rust
    /// Vision's scene classification tags. Always emitted by Swift, so an
    /// absent KEY is a pipeline bug and is refused; an EMPTY list is a real
    /// answer (Vision recognised nothing) and is accepted.
    pub scene_tags: Vec<String>,
    /// EXIF capture time, seconds since the epoch. Legitimately `None` --
    /// a scan or an export often carries no EXIF date at all -- so absence
    /// is tolerated here, unlike `scene_tags`.
    pub captured_at: Option<i64>,
```

- [ ] **Step 4: Parse them**

In `parse_features`, before the `Some(Photo { ... })`:

```rust
    // Required: Swift always emits the key. `[]` is a real answer and is
    // accepted; a missing key is not.
    let scene_tags: Vec<String> = v["sceneTags"]
        .as_array()?
        .iter()
        .map(|t| t.as_str().map(str::to_string))
        .collect::<Option<Vec<String>>>()?;

    // Optional: `encodeIfPresent` omits it, and a photo with no EXIF date
    // is ordinary rather than malformed.
    let captured_at = v["exif"]["captureDate"].as_f64().map(|t| t as i64);
```

and add `scene_tags,` and `captured_at,` to the struct literal.

- [ ] **Step 5: Fix every `Photo` literal the new fields break**

Run: `cd src-tauri && cargo test --lib 2>&1 | grep -E "missing field" | head -20`

Add `scene_tags: Vec::new(), captured_at: None,` to each fixture. There are `Photo` literals in `score.rs`, `pace.rs`, `pack.rs`, `cull.rs` and `preflight.rs` test modules.

- [ ] **Step 6: Run the test and the suite**

Run: `cd src-tauri && cargo test`
Expected: all pass. **The golden must not change** — nothing reads the new fields yet.

Watch for a different failure class here: any existing fixture record that lacks `sceneTags` now fails to parse. If a test that used to pass now sees `None` from `from_features`, the fixture is incomplete — **fix the fixture, do not relax the parser.** A fixture missing a key the real pipeline always sends was already lying about what it tested.

- [ ] **Step 7: Mutation-check**

Change `v["sceneTags"].as_array()?` to `v["sceneTags"].as_array().unwrap_or(&Vec::new())`, re-run the new test, confirm the third assertion FAILS. Restore. Then change `captured_at` to use `?`, re-run, confirm the second assertion FAILS. Restore. Paste both into the commit message.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/book/cull.rs src-tauri/src/book
git commit -m "feat(cull): carry scene tags and capture time on Photo

Signals the spread-diversity term needs. No wire change: finalize_photos
strips only phash, so sceneTags and exif.captureDate already survive the
round trip generate_book sends back.

The two have different strictness classes and swapping them is the trap.
sceneTags is always emitted by Swift, so an absent key is a pipeline bug
and is refused; an empty list is a real answer. exif.captureDate is
legitimately null on a scan, so absence is tolerated -- requiring it would
fail every book containing one undated photo.

Mutation-checked: defaulting sceneTags to empty fails the refusal
assertion; requiring captureDate fails the null-date assertion."
```

---

## Task 10: `face_quality` — the expression complaint

**Files:**
- Modify: `src-tauri/src/book/score.rs` (new term, wired into `score_spread`)
- Test: `src-tauri/src/book/score.rs`

**Interfaces:**
- Consumes: `Weights.face_quality` from Task 8.
- Produces: `fn face_quality(photo: &Photo) -> f64`, private to `score`.

**Why:** The user's complaint was "some facial features are too candid/unprepared/unflattering". **The engine has no expression signal whatsoever and cannot get one** — Vision has no expression classifier at any macOS version, and the geometric smile proxy has a measured 100% false-negative rate and is deliberately unused. `Photo.capture_quality` is Vision's own blur/exposure/pose score and already exists; today it is only a tie-break inside `cull`, never a scoring term. Promoting it is the cheapest real improvement available and needs no new analysis.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/book/score.rs`:

```rust
/// A faceless photo is not a bad photo, so an absent capture quality scores
/// NEUTRALLY rather than zero -- the same convention saliency_retention and
/// face_area_retention already use for a missing signal.
#[test]
fn score_face_quality_is_neutral_without_a_face_and_ordered_with_one() {
    let mut none = photo(4000, 3000);
    none.capture_quality = None;
    assert_eq!(face_quality(&none), 0.5);

    let mut poor = photo(4000, 3000);
    poor.capture_quality = Some(0.1);
    let mut good = photo(4000, 3000);
    good.capture_quality = Some(0.9);

    assert!(
        face_quality(&good) > face_quality(&poor),
        "a well-captured face must outscore a poorly-captured one"
    );
    assert!((0.0..=1.0).contains(&face_quality(&good)));
    assert!((0.0..=1.0).contains(&face_quality(&poor)));
}

/// The term must actually reach the total. Weight 0.0 ships, so a test
/// using the shipped weights would pass with the function body deleted --
/// this one gives it a non-zero weight explicitly.
#[test]
fn score_face_quality_changes_the_spread_total_when_weighted() {
    let t = one_slot_template();
    let mut poor = photo(4000, 3000);
    poor.capture_quality = Some(0.1);
    let mut good = photo(4000, 3000);
    good.capture_quality = Some(0.9);

    let w = Weights { face_quality: 1.0, ..Weights::default() };
    let poor_score = score_spread(&t, &[&poor], &[0], None, &w).expect("scores");
    let good_score = score_spread(&t, &[&good], &[0], None, &w).expect("scores");
    assert!(
        good_score > poor_score,
        "face_quality never reached the total: {good_score} vs {poor_score}"
    );

    // And at the SHIPPED weight it changes nothing, which is what "inert"
    // means and what keeps the golden stable.
    let inert = Weights::default();
    assert_eq!(
        score_spread(&t, &[&poor], &[0], None, &inert),
        score_spread(&t, &[&good], &[0], None, &inert),
        "the term must be inert at its shipped weight of 0.0"
    );
}
```

> **`one_slot_template()`** is a 1-photo spread template fixture. If `mod tests` in `score.rs` has no such helper, build one from the existing `wide_slot()` helper plus `PageLayout`/`SpreadTemplate` literals already used in that module. Its slot must be **non-square** and the photo must resolve above 200 DPI in it, or `rejects` returns `TooLowResolution` and `score_spread` yields `None`.

- [ ] **Step 2: Run and verify failure**

Run: `cd src-tauri && cargo test --lib score_face_quality`
Expected: FAIL to compile — `face_quality` not found.

- [ ] **Step 3: Implement**

In `src-tauri/src/book/score.rs`, beside the other soft terms:

```rust
/// Vision's own capture-quality score for the best face in the photo:
/// blur, exposure and pose, in [0,1].
///
/// This is the closest thing the engine has to an EXPRESSION signal, and
/// the distance is worth stating. Vision has no expression classifier at
/// any macOS version, and the geometric smile proxy was measured against
/// real faces with a 100% false-negative rate, so it is computed and
/// deliberately unused. Capture quality does not know a smile from a
/// grimace -- it knows a sharp, well-exposed, front-facing face from a
/// blurred or turned one, which is a different thing that happens to
/// correlate with the complaint.
///
/// Neutral at 0.5 when there is no face or Vision returned no score: a
/// faceless photo is not a bad photo. Same convention as
/// `saliency_retention` and `face_area_retention`.
fn face_quality(photo: &Photo) -> f64 {
    photo.capture_quality.map_or(0.5, |q| q.clamp(0.0, 1.0))
}
```

Wire it into `score_spread`'s per-slot accumulation:

```rust
        total += w.aspect_fit * aspect_fit(photo, slot)
            + w.saliency_retention * saliency_retention(photo, &crop)
            + w.face_area_retention * face_area_retention(photo, &crop)
            + w.face_quality * face_quality(photo)
            + w.hero_match * hero_match(photo, slot, best_aesthetic)
            + w.resolution_headroom * resolution_headroom(photo, &crop, slot);
```

- [ ] **Step 4: Run the tests and the suite**

Run: `cd src-tauri && cargo test --lib score_face_quality && cargo test`
Expected: all pass. **The golden must not change.**

- [ ] **Step 5: Mutation-check**

Change `face_quality` to `fn face_quality(_photo: &Photo) -> f64 { 0.5 }`, re-run `cargo test --lib score_face_quality`, and confirm `score_face_quality_changes_the_spread_total_when_weighted` FAILS. Restore. Paste into the commit message — this is the proof that matters, because the term is inert at its shipped weight.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/book/score.rs
git commit -m "feat(score): weight Vision's face capture quality

The engine has no expression signal and cannot get one: Vision has no
expression classifier at any macOS version, and the geometric smile proxy
has a measured 100% false-negative rate. Capture quality -- blur, exposure,
pose -- already exists and was only a tie-break inside cull.

Inert at its shipped weight of 0.0.

Mutation-checked: stubbing the term to a constant 0.5 fails the
non-zero-weight test, which is the proof that matters for an inert term."
```

---

## Task 11: `spread_diversity` — the variety complaint

**Files:**
- Modify: `src-tauri/src/book/score.rs`
- Test: `src-tauri/src/book/score.rs`

**Interfaces:**
- Consumes: `Photo.scene_tags` and `Photo.captured_at` from Task 9; `Weights.spread_diversity` from Task 8.
- Produces: `fn spread_diversity(photos: &[&Photo]) -> f64`, private to `score`.

**Why:** Near-duplicate clustering is perceptual-hash based, so it only collapses near-identical frames; photos of one moment from slightly different angles all survive. **There is no within-spread diversity term at all.** Worse, `palette_harmony` *rewards* hue agreement, so the one aesthetic term that exists actively pushes toward the sameness being complained about.

- [ ] **Step 1: Write the failing test**

```rust
/// Three photos that share every signal must score LOWER than three that
/// share none. The fixture varies all three components at once, so the
/// assertion holds regardless of how they are weighted against each other.
#[test]
fn score_spread_diversity_separates_a_varied_spread_from_a_samey_one() {
    let mut same: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
    for p in &mut same {
        p.scene_tags = vec!["beach".into(), "sunset".into()];
        p.captured_at = Some(1_700_000_000);
        p.palette = vec![PaletteColor { r: 0.9, g: 0.4, b: 0.1, weight: 1.0 }];
    }

    let mut varied: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
    varied[0].scene_tags = vec!["beach".into()];
    varied[1].scene_tags = vec!["forest".into()];
    varied[2].scene_tags = vec!["city".into()];
    varied[0].captured_at = Some(1_700_000_000);
    varied[1].captured_at = Some(1_700_050_000);
    varied[2].captured_at = Some(1_700_100_000);
    varied[0].palette = vec![PaletteColor { r: 0.9, g: 0.1, b: 0.1, weight: 1.0 }];
    varied[1].palette = vec![PaletteColor { r: 0.1, g: 0.9, b: 0.1, weight: 1.0 }];
    varied[2].palette = vec![PaletteColor { r: 0.1, g: 0.1, b: 0.9, weight: 1.0 }];

    let same_refs: Vec<&Photo> = same.iter().collect();
    let varied_refs: Vec<&Photo> = varied.iter().collect();

    let s = spread_diversity(&same_refs);
    let v = spread_diversity(&varied_refs);
    assert!(v > s, "a varied spread must outscore a samey one: {v} vs {s}");
    assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&v));
}

/// Fewer than two photos, or no signal at all, is neutral rather than zero
/// -- a single photo on a spread is not "undiverse", the question simply
/// does not apply.
#[test]
fn score_spread_diversity_is_neutral_without_enough_to_compare() {
    let one = photo(4000, 3000);
    assert_eq!(spread_diversity(&[&one]), 0.5);
    assert_eq!(spread_diversity(&[]), 0.5);
}

/// The term must reach the spread total. Shipped weight is 0.0, so a test
/// against the shipped weights would pass with the body deleted.
#[test]
fn score_spread_diversity_changes_the_spread_total_when_weighted() {
    let t = two_slot_template();
    let mut a = photo(4000, 3000);
    let mut b = photo(4000, 3000);
    a.scene_tags = vec!["beach".into()];
    b.scene_tags = vec!["beach".into()];
    a.captured_at = Some(1_700_000_000);
    b.captured_at = Some(1_700_000_000);

    let mut c = photo(4000, 3000);
    let mut d = photo(4000, 3000);
    c.scene_tags = vec!["beach".into()];
    d.scene_tags = vec!["city".into()];
    c.captured_at = Some(1_700_000_000);
    d.captured_at = Some(1_700_100_000);

    let w = Weights { spread_diversity: 1.0, ..Weights::default() };
    let samey = score_spread(&t, &[&a, &b], &[0, 1], None, &w).expect("scores");
    let varied = score_spread(&t, &[&c, &d], &[0, 1], None, &w).expect("scores");
    assert!(varied > samey, "spread_diversity never reached the total");

    let inert = Weights::default();
    assert_eq!(
        score_spread(&t, &[&a, &b], &[0, 1], None, &inert),
        score_spread(&t, &[&c, &d], &[0, 1], None, &inert),
        "the term must be inert at its shipped weight of 0.0"
    );
}
```

> **`two_slot_template()`** is a 2-photo spread fixture, one slot per page half, both **non-square**, both resolving above 200 DPI for a 4000×3000 photo.

- [ ] **Step 2: Run and verify failure**

Run: `cd src-tauri && cargo test --lib score_spread_diversity`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
/// How unlike each other the photos on one spread are, in [0,1].
///
/// The complaint this answers: "there are too many images that are similar
/// in some pages". Near-duplicate clustering is perceptual-hash based, so it
/// only collapses near-IDENTICAL frames; two photos of the same moment from
/// slightly different angles are not near-duplicates and both survive.
///
/// Three components, each normalised to [0,1] and combined with EQUAL
/// weight. Equal weighting is a starting point, not a claim -- the three are
/// not commensurable and no measurement exists to weight them against each
/// other. Sub-weights are a tuning question; if one component turns out to
/// dominate, the fix is to measure it, not to guess a ratio.
///
/// `phash` would be the most direct signal and is deliberately absent: it is
/// stripped before the webview because JavaScript loses precision above
/// 2^53, and re-widening the wire is a larger change than this term
/// justifies.
///
/// Neutral at 0.5 for fewer than two photos: one photo on a spread is not
/// "undiverse", the question does not apply.
fn spread_diversity(photos: &[&Photo]) -> f64 {
    if photos.len() < 2 {
        return 0.5;
    }
    let mut total = 0.0;
    let mut pairs = 0.0;
    for i in 0..photos.len() {
        for j in (i + 1)..photos.len() {
            total += pair_distance(photos[i], photos[j]);
            pairs += 1.0;
        }
    }
    if pairs == 0.0 {
        return 0.5;
    }
    (total / pairs).clamp(0.0, 1.0)
}

/// Mean of the three pairwise distances, each in [0,1].
fn pair_distance(a: &Photo, b: &Photo) -> f64 {
    (scene_tag_distance(a, b) + palette_distance(a, b) + capture_gap_distance(a, b)) / 3.0
}

/// Jaccard distance over scene tags. Two photos with no tags at all are
/// neither similar nor different on this axis, so they score neutrally
/// rather than identical -- an empty-vs-empty comparison is an absent
/// signal, not agreement.
fn scene_tag_distance(a: &Photo, b: &Photo) -> f64 {
    use std::collections::BTreeSet;
    let sa: BTreeSet<&str> = a.scene_tags.iter().map(String::as_str).collect();
    let sb: BTreeSet<&str> = b.scene_tags.iter().map(String::as_str).collect();
    if sa.is_empty() && sb.is_empty() {
        return 0.5;
    }
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 {
        return 0.5;
    }
    1.0 - (sa.intersection(&sb).count() as f64 / union)
}

/// Euclidean distance between the dominant colours, normalised by the
/// longest possible distance in the unit RGB cube (sqrt(3)).
///
/// Plain sRGB, matching what `Metrics.palette` actually produces -- the
/// same crude space `palette_harmony` reads, and for the same reason: this
/// is not a perceptual distance and does not claim to be.
fn palette_distance(a: &Photo, b: &Photo) -> f64 {
    let (Some(ca), Some(cb)) = (a.palette.first(), b.palette.first()) else {
        return 0.5;
    };
    let d = ((ca.r - cb.r).powi(2) + (ca.g - cb.g).powi(2) + (ca.b - cb.b).powi(2)).sqrt();
    (d / 3.0_f64.sqrt()).clamp(0.0, 1.0)
}

/// Capture-time gap, saturating at one hour: photos minutes apart are the
/// same moment, photos hours apart are not. Beyond an hour the axis carries
/// no more information, and event clustering has already separated the
/// chapters.
fn capture_gap_distance(a: &Photo, b: &Photo) -> f64 {
    const SATURATE_SECONDS: f64 = 3600.0;
    let (Some(ta), Some(tb)) = (a.captured_at, b.captured_at) else {
        return 0.5;
    };
    let gap = (ta - tb).abs() as f64;
    (gap / SATURATE_SECONDS).clamp(0.0, 1.0)
}
```

Wire into `score_spread`'s spread-global section:

```rust
    // Spread-global terms, added once rather than per slot.
    total += w.palette_harmony * palette_harmony(photos);
    total += w.spread_diversity * spread_diversity(photos);
    total += w.variety * variety(&t.id, previous);
```

- [ ] **Step 4: Run the tests and the suite**

Run: `cd src-tauri && cargo test --lib score_spread_diversity && cargo test`
Expected: all pass. **The golden must not change.**

- [ ] **Step 5: Mutation-check**

Stub `spread_diversity` to `{ 0.5 }`, re-run, and confirm `score_spread_diversity_changes_the_spread_total_when_weighted` and `score_spread_diversity_separates_a_varied_spread_from_a_samey_one` both FAIL. Then stub each of the three component functions to a constant in turn and confirm the separation test still fails for at least two of them — if stubbing a component changes nothing, that component is not reaching the result. Restore. Paste into the commit message.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/book/score.rs
git commit -m "feat(score): add a within-spread diversity term

Near-duplicate clustering is phash-based and only collapses near-identical
frames, so two photos of one moment from different angles both survive and
nothing scored against that. Worse, palette_harmony REWARDS hue agreement,
so the one aesthetic term that existed pushed toward the sameness being
complained about.

Scene-tag Jaccard, palette distance and capture-time gap, equal-weighted.
Equal weighting is a starting point, not a claim: the three are not
commensurable and no measurement exists to weight them. phash is
deliberately unused -- it is stripped before the webview for precision
reasons and re-widening the wire costs more than this term is worth.

Inert at its shipped weight of 0.0.

Mutation-checked: stubbing the term to 0.5 fails both tests; stubbing each
component in turn confirms all three reach the result."
```

---

## Task 12: `gutter_saliency` — the spec's missing penalty

**Files:**
- Modify: `src-tauri/src/book/score.rs`
- Modify: `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md` (§4.3 table)
- Test: `src-tauri/src/book/score.rs`

**Interfaces:**
- Consumes: `Weights.gutter_saliency` from Task 8.
- Produces: `fn gutter_saliency(photo: &Photo, crop: &Rect, slot: &Slot, side: Side) -> f64`, private to `score`.

**Why:** The Phase 2 layout-engine design's §4.2 states the rule plainly: "a **face** in the dead strip is a rejection; generic salient content there is only a **penalty**." Only the rejection half exists. That document's §4.3 weights table lists no such term and `Weights` had no field for it. `score_does_not_reject_generic_saliency_in_the_gutter_strip` pins that generic saliency is deliberately not a rejection, but nothing implements the graded penalty. The ruling was "implement it or correct the spec"; we implement it.

- [ ] **Step 1: Write the failing test**

```rust
/// Generic salient content in the gutter is a PENALTY, never a rejection --
/// otherwise no slot could ever run flush to the fold. A saliency box
/// straddling the dead band must score below one clear of it, and both must
/// still produce a score at all.
#[test]
fn score_gutter_saliency_penalises_without_rejecting() {
    let slot = full_left_page_slot();
    let mut clear = photo(4000, 3000);
    // Well away from the fold, in the photo's own normalised coordinates.
    clear.saliency_box = Some(Rect::new(0.05, 0.30, 0.20, 0.30));

    let mut in_gutter = photo(4000, 3000);
    // Hard against the inner edge, which maps into the dead band.
    in_gutter.saliency_box = Some(Rect::new(0.75, 0.30, 0.20, 0.30));

    let crop_clear = choose_crop(&clear, slot_aspect(&slot));
    let crop_gutter = choose_crop(&in_gutter, slot_aspect(&slot));

    let g_clear = gutter_saliency(&clear, &crop_clear, &slot, Side::Left);
    let g_gutter = gutter_saliency(&in_gutter, &crop_gutter, &slot, Side::Left);

    assert!(
        g_gutter < g_clear,
        "saliency in the gutter must score lower: {g_gutter} vs {g_clear}"
    );
    assert!((0.0..=1.0).contains(&g_gutter) && (0.0..=1.0).contains(&g_clear));

    // And it must remain a penalty, not a rejection.
    assert!(
        rejects(&in_gutter, &crop_gutter, &slot, Side::Left).is_none(),
        "generic saliency in the gutter must not reject the candidate"
    );
}

/// A photo with no saliency box scores neutrally: absence of a signal is
/// not evidence of a bad placement, the same rule saliency_retention uses.
#[test]
fn score_gutter_saliency_is_neutral_without_a_saliency_box() {
    let slot = full_left_page_slot();
    let mut p = photo(4000, 3000);
    p.saliency_box = None;
    let crop = choose_crop(&p, slot_aspect(&slot));
    assert_eq!(gutter_saliency(&p, &crop, &slot, Side::Left), 1.0);
}

/// The term must reach the total; shipped weight is 0.0.
#[test]
fn score_gutter_saliency_changes_the_spread_total_when_weighted() {
    let t = one_slot_template();
    let mut clear = photo(4000, 3000);
    clear.saliency_box = Some(Rect::new(0.05, 0.30, 0.20, 0.30));
    let mut in_gutter = photo(4000, 3000);
    in_gutter.saliency_box = Some(Rect::new(0.75, 0.30, 0.20, 0.30));

    let w = Weights { gutter_saliency: 1.0, ..Weights::default() };
    let a = score_spread(&t, &[&clear], &[0], None, &w).expect("scores");
    let b = score_spread(&t, &[&in_gutter], &[0], None, &w).expect("scores");
    assert!(a > b, "gutter_saliency never reached the total: {a} vs {b}");

    let inert = Weights::default();
    let ia = score_spread(&t, &[&clear], &[0], None, &inert).expect("scores");
    let ib = score_spread(&t, &[&in_gutter], &[0], None, &inert).expect("scores");
    assert_eq!(ia, ib, "the term must be inert at its shipped weight of 0.0");
}
```

> **`full_left_page_slot()`** must be a slot running flush to the fold on the LEFT page — its rect must reach `x + w == 1.0` in page-normalised coordinates so the gutter dead band is actually inside it. A slot that stops short of the fold cannot exercise this term at all, and the test would pass vacuously. It must also be **non-square**.
>
> **The two saliency boxes must be positioned so the mapped rects genuinely straddle and clear the dead band.** After writing the test, print the mapped rect for each and confirm — a boundary test using a value nowhere near the boundary is the exact decorative shape this project keeps finding.
>
> **The band is expressed in PAGE-normalised coordinates, not spread ones.** `geometry::clear_of_gutter` reads `GUTTER_U` (`= BLEED_IN / PAGE_W_IN`) and tests `rect.right() <= 1.0 - GUTTER_U` on a left page, `rect.x >= GUTTER_U` on a right page. So the dead band as a rect is `[1.0 - GUTTER_U, 1.0]` on a left page and `[0.0, GUTTER_U]` on a right one. The `0.491203..0.508797` figures in `PROJECT-STATUS.md` are the SPREAD-canvas form of the same band and must not be used here — the two are different numbers for the same strip, which is precisely the unit-confusion trap that already cost this project a bug.

- [ ] **Step 2: Run and verify failure**

Run: `cd src-tauri && cargo test --lib score_gutter_saliency`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
/// Penalty for generic salient content falling in the gutter dead band, in
/// [0,1] where 1.0 is "none of it is in the crease".
///
/// The spec's split, and the reason this is not a rejection: a FACE in the
/// dead strip is a hard rejection, but generic salient content there is
/// only a penalty -- otherwise no slot could ever run flush to the fold and
/// half the library would be unusable.
///
/// Only the part of the saliency box that SURVIVES the crop is measured.
/// Unlike a face, a saliency box is never a hard constraint, so it can be
/// legitimately partial, and judging the whole declared box would penalise
/// a crop that already removed the offending part. Pre-flight's own
/// gutter-saliency WARN makes the same choice.
///
/// Neutral at 1.0 with no saliency box: absence of a signal is not evidence
/// of a bad placement.
fn gutter_saliency(photo: &Photo, crop: &Rect, slot: &Slot, side: Side) -> f64 {
    let Some(visible) = photo.saliency_box.and_then(|s| s.intersect(crop)) else {
        return 1.0;
    };
    let Some(mapped) = face_in_page(&visible, crop, slot) else {
        return 1.0;
    };
    let area = mapped.area();
    if area <= 0.0 {
        return 1.0;
    }
    // Graded, not binary: half a saliency box in the crease is half as bad
    // as all of it. `clear_of_gutter` answers yes/no, which is the right
    // shape for a rejection and the wrong one for a penalty.
    let in_band = gutter_overlap_area(&mapped, side);
    (1.0 - (in_band / area)).clamp(0.0, 1.0)
}
```

`gutter_overlap_area` must be added to `src-tauri/src/geometry.rs` beside `clear_of_gutter`, so the dead-band constants stay in one place:

```rust
/// The gutter dead band on `side`, as a page-normalised rect spanning the
/// full page height.
///
/// `clear_of_gutter` expresses the same strip as a comparison against
/// `GUTTER_U`. Both read that ONE constant: a second hand-typed copy of the
/// band is exactly the duplicated-authority problem culling already had to
/// fix, and the spread-canvas form of these numbers is different again.
pub fn gutter_band(side: Side) -> Rect {
    match side {
        Side::Left => Rect::new(1.0 - GUTTER_U, 0.0, GUTTER_U, 1.0),
        Side::Right => Rect::new(0.0, 0.0, GUTTER_U, 1.0),
    }
}

/// Area of `rect` (page-normalised, on `side`) falling inside that band.
///
/// The graded counterpart to `clear_of_gutter`, which answers yes/no --
/// the right shape for a rejection and the wrong one for a penalty.
pub fn gutter_overlap_area(rect: &Rect, side: Side) -> f64 {
    rect.intersect(&gutter_band(side)).map_or(0.0, |i| i.area())
}
```

- [ ] **Step 3a: Pin the two gutter helpers against each other**

Add to `mod tests` in `src-tauri/src/geometry.rs`:

```rust
/// The graded helper and the boolean one must describe the SAME strip. Two
/// definitions of the gutter that drift apart would let the scorer penalise
/// a rect pre-flight considers clear, on both sides of the fold.
#[test]
fn geometry_gutter_overlap_agrees_with_clear_of_gutter() {
    for side in [Side::Left, Side::Right] {
        // A tall, NARROW rect swept across the page: square or full-width
        // fixtures make the two agree trivially.
        for step in 0..200 {
            let x = step as f64 / 200.0;
            let rect = Rect::new(x, 0.25, 0.004, 0.5);
            let clear = clear_of_gutter(&rect, side);
            let overlap = gutter_overlap_area(&rect, side);
            assert_eq!(
                clear,
                overlap <= 1e-9,
                "{side:?} disagreed at x={x}: clear={clear}, overlap={overlap}"
            );
        }
    }
}
```

Run: `cd src-tauri && cargo test --lib geometry_gutter_overlap_agrees_with_clear_of_gutter`
Expected: PASS. If it fails at exactly one step, that is `clear_of_gutter`'s `EPS` and the band edge disagreeing by a hair — widen the assertion's tolerance to match `EPS`, do not move the band.

Wire into `score_spread`'s per-slot accumulation:

```rust
            + w.gutter_saliency * gutter_saliency(photo, &crop, slot, *side)
```

- [ ] **Step 4: Update the layout-engine design's §4.3 table**

In `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md`, add to the §4.3 soft-terms table:

```markdown
| Gutter saliency | Fraction of the surviving saliency box falling in the gutter dead band (§4.2's penalty half) |
```

This removes the §4.2-vs-§4.3 mismatch that open item 1 is about. Tasks 10, 11 and 13 add their rows in Task 14; add only this one here, since it is the row the spec was actually wrong about.

- [ ] **Step 5: Run the tests and the suite**

Run: `cd src-tauri && cargo test --lib score_gutter_saliency && cargo test`
Expected: all pass, including `score_does_not_reject_generic_saliency_in_the_gutter_strip`. **The golden must not change.**

- [ ] **Step 6: Mutation-check**

Stub `gutter_saliency` to `{ 1.0 }` and confirm `score_gutter_saliency_penalises_without_rejecting` and the weighted test FAIL. Then change `gutter_overlap_area` to return `0.0` unconditionally and confirm the same tests FAIL. Restore. Paste into the commit message.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/book/score.rs src-tauri/src/geometry.rs docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md
git commit -m "feat(score): penalise generic saliency in the gutter

The layout-engine design's §4.2 promised it -- 'a face in the dead strip is
a rejection; generic salient content there is only a penalty' -- and only
the rejection half was ever built. §4.3's weights table listed no such term
and Weights had no field for it. The spec now matches the scorer.

Graded rather than binary: half a saliency box in the crease is half as bad
as all of it, which is the shape a penalty needs and clear_of_gutter's
yes/no cannot give. Only the part surviving the crop is measured, matching
what pre-flight's own gutter WARN already does.

The band constants stay in geometry.rs; gutter_overlap_area reads the same
ones clear_of_gutter does rather than restating them.

Inert at its shipped weight of 0.0.

Mutation-checked: stubbing the term, and separately stubbing the overlap
area, each fail the penalty and weighted tests."
```

---

## Task 13: `hero_prominence` — the standout-photo complaint

**Files:**
- Modify: `src-tauri/src/book/score.rs`
- Test: `src-tauri/src/book/score.rs`

**Interfaces:**
- Consumes: `Weights.hero_prominence` from Task 8.
- Produces: `fn hero_prominence(t: &SpreadTemplate, photos: &[&Photo]) -> f64`, private to `score`.

**Why:** `hero_match` already rewards the highest-aesthetic photo landing in a `hero` slot, but **only within a group that has already been formed** — `pack` sizes groups from chapter boundaries and capacity alone and knows nothing about photo merit, so a standout image can be dealt into a six-up and get a sixth of a spread.

This term biases *template choice* for a given group. **The packer half is deferred by explicit decision:** letting merit influence group size is the real fix and a genuine change to `pack`'s contract, and Task 3 already changes that contract once. The agreed sequence is to ship this, measure it against a real book, and only then decide whether the packer change is still needed.

- [ ] **Step 1: Write the failing test**

```rust
/// When a group holds a standout photo, a template whose hero slot dominates
/// the spread should beat one whose slots are all the same size. When the
/// group is flat, the two should be indistinguishable -- the term is about
/// giving a STANDOUT room, not about preferring big slots generally.
#[test]
fn score_hero_prominence_prefers_a_dominant_hero_only_for_a_standout_group() {
    let dominant = hero_dominant_template();  // one large hero + two small
    let even = even_three_up_template();      // three equal, all Support

    let mut standout: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
    standout[0].aesthetic_pct = 99;
    standout[1].aesthetic_pct = 40;
    standout[2].aesthetic_pct = 38;

    let flat: Vec<Photo> = (0..3)
        .map(|_| {
            let mut p = photo(4000, 3000);
            p.aesthetic_pct = 50;
            p
        })
        .collect();

    let standout_refs: Vec<&Photo> = standout.iter().collect();
    let flat_refs: Vec<&Photo> = flat.iter().collect();

    assert!(
        hero_prominence(&dominant, &standout_refs) > hero_prominence(&even, &standout_refs),
        "a standout photo should pull toward a dominant hero slot"
    );
    assert_eq!(
        hero_prominence(&dominant, &flat_refs),
        hero_prominence(&even, &flat_refs),
        "with no standout, the term must not prefer either template"
    );
}

/// The term must reach the total; shipped weight is 0.0.
#[test]
fn score_hero_prominence_changes_the_spread_total_when_weighted() {
    let dominant = hero_dominant_template();
    let even = even_three_up_template();
    let mut standout: Vec<Photo> = (0..3).map(|_| photo(4000, 3000)).collect();
    standout[0].aesthetic_pct = 99;
    standout[1].aesthetic_pct = 40;
    standout[2].aesthetic_pct = 38;
    let refs: Vec<&Photo> = standout.iter().collect();

    let w = Weights { hero_prominence: 1.0, ..Weights::default() };
    let d = score_spread(&dominant, &refs, &[0, 1, 2], None, &w).expect("scores");
    let e = score_spread(&even, &refs, &[0, 1, 2], None, &w).expect("scores");
    assert!(d > e, "hero_prominence never reached the total: {d} vs {e}");

    // Inert at the shipped weight -- but hero_match still separates these
    // two templates, so compare the DELTA rather than asserting equality.
    let inert = Weights::default();
    let id = score_spread(&dominant, &refs, &[0, 1, 2], None, &inert).expect("scores");
    let ie = score_spread(&even, &refs, &[0, 1, 2], None, &inert).expect("scores");
    assert!(
        (d - e) > (id - ie),
        "weighting the term must widen the gap it is responsible for"
    );
}
```

> **`hero_dominant_template()` and `even_three_up_template()`** are both 3-photo spread fixtures. The first must have one `Role::Hero` slot occupying a clearly larger area than its two `Role::Support` siblings; the second must have three equal `Role::Support` slots. All slots **non-square**, all resolving above 200 DPI for a 4000×3000 photo, and no slot may span the fold.
>
> Note the second assertion in the weighted test compares deltas, not equality: `hero_match` is non-zero at the shipped weights and already distinguishes these two templates, so asserting `id == ie` would fail for a reason that has nothing to do with this term.

- [ ] **Step 2: Run and verify failure**

Run: `cd src-tauri && cargo test --lib score_hero_prominence`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
/// How much room the template gives its hero slot, scaled by how much this
/// group has a standout photo to put there. In [0,1].
///
/// The complaint: "some interesting features should be highlighted rather
/// than mixed with other images". `hero_match` already rewards the best
/// photo landing in a hero slot, but only WITHIN a group that has already
/// been formed -- `pack` knows nothing about merit, so a standout can be
/// dealt into a six-up and get a sixth of a spread.
///
/// This biases which template a formed group gets. It does NOT change how
/// groups are formed; letting merit influence group size is a real change to
/// `pack`'s contract and is deliberately deferred until this has been
/// measured against a real book.
///
/// Neutral at 0.5 when the group is flat, so the term is silent rather than
/// pushing toward big slots generally: it exists to give a standout room,
/// not to prefer dominant templates as a matter of taste.
fn hero_prominence(t: &SpreadTemplate, photos: &[&Photo]) -> f64 {
    if photos.len() < 2 {
        return 0.5;
    }
    let mut pcts: Vec<f64> = photos.iter().map(|p| p.aesthetic_pct as f64).collect();
    pcts.sort_by(|a, b| b.total_cmp(a));
    let rest = &pcts[1..];
    let rest_mean = rest.iter().sum::<f64>() / rest.len() as f64;
    // How far the best photo clears the others, as a fraction of the
    // percentile scale. Zero when the group is flat.
    let standout = ((pcts[0] - rest_mean) / 100.0).clamp(0.0, 1.0);
    if standout == 0.0 {
        return 0.5;
    }

    let slots: Vec<&Slot> =
        t.left.slots.iter().chain(t.right.slots.iter()).collect();
    let total: f64 = slots.iter().map(|s| s.rect.area()).sum();
    if total <= 0.0 {
        return 0.5;
    }
    let hero_area: f64 = slots
        .iter()
        .filter(|s| s.role == Role::Hero)
        .map(|s| s.rect.area())
        .sum();
    // An even split of n slots gives each 1/n; anything above that is
    // genuine dominance. Normalised so a single full-spread hero reads 1.0.
    let even_share = 1.0 / slots.len() as f64;
    let dominance = ((hero_area / total - even_share) / (1.0 - even_share)).clamp(0.0, 1.0);

    (0.5 + 0.5 * standout * dominance).clamp(0.0, 1.0)
}
```

Wire into `score_spread`'s spread-global section:

```rust
    total += w.hero_prominence * hero_prominence(t, photos);
```

- [ ] **Step 4: Run the tests and the suite**

Run: `cd src-tauri && cargo test --lib score_hero_prominence && cargo test`
Expected: all pass. **The golden must not change.**

- [ ] **Step 5: Mutation-check**

Stub `hero_prominence` to `{ 0.5 }` and confirm both new tests FAIL. Then remove the `standout` scaling (return `0.5 + 0.5 * dominance`) and confirm `score_hero_prominence_prefers_a_dominant_hero_only_for_a_standout_group`'s **second** assertion FAILS — this is the one that proves the term is about standouts rather than about big slots. Restore. Paste into the commit message.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/book/score.rs
git commit -m "feat(score): bias template choice toward a dominant hero slot

hero_match rewards the best photo landing in a hero slot, but only within a
group already formed -- pack sizes groups from chapter boundaries and
capacity alone and knows nothing about merit, so a standout could be dealt
into a six-up.

Scaled by how far the best photo clears the rest, so the term is silent on
a flat group rather than preferring big slots as a matter of taste.

The packer half of this complaint -- letting merit influence group size --
is deliberately deferred: it is a real change to pack's contract, and pack's
contract already changed once in this branch. Measure this first.

Inert at its shipped weight of 0.0.

Mutation-checked: stubbing the term fails both tests; removing the standout
scaling fails the flat-group assertion specifically."
```

---

## Task 14: Update the ledgers

**Files:**
- Modify: `docs/PROJECT-STATUS.md`
- Modify: `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md` (§4.3 table)

**Interfaces:**
- Consumes: the outcome of every preceding task.
- Produces: nothing consumed by code.

**Why:** `PROJECT-STATUS.md` is explicitly "the only surviving record" of these rulings. A closed item left open sends the next reader to re-derive it; an open item marked closed is worse.

- [ ] **Step 1: Close the items this branch actually closed**

In `docs/PROJECT-STATUS.md` § "Phase 2 open items", mark with the same `— FIXED 2026-08-15` convention item 4 already uses:

- **Item 1** (gutter-saliency penalty) — closed by Task 12. Note that the layout-engine design's §4.3 table now carries the row.
- **Item 3** (seed never reaches spread selection) — closed by Task 5. **Remove the "Phase 3 blocker" language**, and say so explicitly: the seed now reaches `best_spread`, so "regenerate this spread" is implementable.
- **Item 6** (persistence half-reachable) — **it was already closed** by `3e934e9`; the UI calls `open_project` at `app/composables/useBook.ts:162`. Correct the entry rather than claiming this branch fixed it.
- **Item 9** (`preflight_core` doc wrong about I/O) — closed by Task 7.
- **Item 11** (`from_features` silent on two paths) — closed by Task 6.
- **Item 4's "Still open, smaller"** paragraph — closed by Task 3. The singles are now sized as page halves.

- [ ] **Step 2: Record what did NOT change, and why**

Still open, with their existing rulings intact: **item 2** (`palette_harmony` inert — now with the note that `spread_diversity` supersedes the need, and that trading one against the other is the obvious first tuning move), **item 5** (RAW), **item 7** (two unmeasured constants), **item 8** (`ORDER BY position`).

- [ ] **Step 3: Rewrite the layout-quality section**

In § "Layout quality: the first real-photo run, 2026-08-14", record against each of the four complaints what now exists and that **it is inert**:

- Complaint 1 — the structural finding (a 1-photo spread must leave a half blank; all eight such templates, and no ninth is authorable) and that size 1 is now banned from the spread region. Record the new blank-printed-page count from Task 3 Step 9 against the old `8 / 4 / 5 / 1 / 2 / 1`.
- Complaint 2 — `face_quality` exists at weight 0.0. Restate that there is still **no expression signal** and that capture quality is a proxy for sharp/well-exposed/front-facing, not for expression.
- Complaint 3 — `spread_diversity` exists at weight 0.0.
- Complaint 4 — `hero_prominence` exists at weight 0.0, and the packer half is deferred pending measurement.

- [ ] **Step 4: Add the tuning section**

Add a new subsection stating plainly that **the four terms are implemented and dormant**, that `templates/weights.json` is hot-reloadable so raising a weight needs no rebuild, and that the honest description of this work until tuning happens is *"the machinery exists and is unproven"*, not *"the book is better"*. Leave a table with the four term names and a blank settled-weight column for the tuning session to fill in.

- [ ] **Step 5: Update the density note if Task 4 Step 7 could not restore it**

If `pace_spread_density_varies_across_a_book_rather_than_converging` still could not pass after the template rebalance, say so here — that the density axis is genuinely flat without size-1 spreads and what the options are — rather than leaving a weakened test as the only record.

- [ ] **Step 6: Add the remaining §4.3 rows**

In `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md`'s §4.3 soft-terms table, add the three rows Task 12 did not:

```markdown
| Face quality | Vision's blur/exposure/pose score for the best face — the nearest thing to an expression signal (§4.2a) |
| Spread diversity | How unlike each other the photos on one spread are: scene tags, palette, capture-time gap |
| Hero prominence | Rewards a dominant hero slot when the group holds a standout photo |
```

Add a line below the table noting all four ship at weight `0.0`.

- [ ] **Step 7: Update the top-of-file state**

`docs/PROJECT-STATUS.md`'s header says **Branch:** `feat/phase-2-layout-engine`. Update it to `feat/phase-2-completion`, update **Last updated**, and refresh the test counts by running `cd src-tauri && cargo test 2>&1 | tail -20` and `bun run test`.

- [ ] **Step 8: Verify everything is green before the final commit**

Run: `bun run sidecar && cd src-tauri && cargo test && cd .. && bun run test && bun run lint`
Expected: all pass, lint clean.

- [ ] **Step 9: Commit**

```bash
git add docs/
git commit -m "docs: record what Phase 2 completion closed, and what it did not

Closes open items 1, 3, 9, 11 and item 4's remaining page-half defect.
Corrects item 6, which 3e934e9 had already closed. Items 2, 5, 7 and 8 stay
open with their rulings intact.

Records the structural finding behind complaint 1 -- a 1-photo spread
template must leave one page half blank, all eight are in that state, and a
ninth cannot be authored -- and the new blank-printed-page counts.

States plainly that the four new scoring terms are implemented and DORMANT.
Until they are tuned on real photographs, the honest description of this
work is 'the machinery exists and is unproven', not 'the book is better'."
```

---

## Self-Review

**Spec coverage** — every section of the design maps to a task:

| Spec section | Task |
|---|---|
| §2 structural blank halves | 3 (ban), 4 (rebalance the two authored ones) |
| §3 slot kinds, `Group` tag, `assemble` places by tag | 1, 2, 3 |
| §3 `Capacity::from_sizes` split | 3 Step 6 |
| §4.0 inert policy, `serde` defaults, `deny_unknown_fields` | 8 |
| §4.1 `face_quality` | 10 |
| §4.2 `spread_diversity` (+ `Photo` fields) | 9, 11 |
| §4.3 `gutter_saliency` | 12 |
| §4.4 `hero_prominence` | 13 |
| §4.5 `palette_harmony` untouched | — (no task, by design; recorded in 14) |
| §5 seed to `best_spread` | 5 |
| §6.1 refusal counts | 6 |
| §6.2 `preflight_core` purity | 7 |
| §6.3 documentation | 14 (and 12 Step 4 for the row the spec was wrong about) |
| §7 testing rules | Every task's mutation-check step |
| §8 sequencing | Task order 1→14 |
| §9 what this does not prove | 14 Step 4 |

**Type consistency, checked across tasks:** `SlotKind` (Task 1) is used unchanged in Tasks 2 and 3. `Group.slot` (Task 1) is read in Task 2 and written in Task 3. `Buildable::for_kind` (Task 3) is the only accessor. `best_spread`'s new `seed` parameter (Task 5) is added before Tasks 11 and 13 touch `score_spread`, and neither of those changes `best_spread`'s signature again. `Weights`' four field names (Task 8) are used verbatim in Tasks 10-13 and in `templates/weights.json`. `Photo.scene_tags`/`captured_at` (Task 9) are read only in Task 11.

**Known ordering hazard, called out rather than hidden:** Task 3 changes `pack`'s signature and `Capacity::from_sizes`'s arity, so it touches every test in `pack.rs`. Tasks 5-13 do not touch `pack.rs`, so they will not conflict — but Task 4 depends on Task 3 having landed (it un-ignores a test Task 3 may have ignored). Keep 1→2→3→4 in order. Tasks 5, 6 and 7 are mutually independent and independent of 8-13. Tasks 10-13 all depend on Task 8; Task 11 additionally depends on Task 9.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-08-15-phase-2-completion.md`.
