# Phase 2 completion — design

**Date:** 2026-08-15
**Status:** approved, not yet planned
**Supersedes nothing.** Extends `2026-08-13-phase-2-layout-engine-design.md` and corrects
one mismatch in it — its §4.2 promises a gutter-saliency penalty its §4.3 weights table
omits. See §4.3 and §6.3 below.

Throughout, a bare `§n` refers to a section of *this* document. References to the Phase 2
layout-engine design are written out in full.

---

## 1. What this closes and why

Phase 2 is functionally complete: the app turns a folder into print-ready files. Two
separate ledgers record what it does *not* do well.

- **`docs/PROJECT-STATUS.md` § "Phase 2 open items"** — eleven items, each a real gap
  deliberately parked with a ruling rather than forgotten.
- **`docs/PROJECT-STATUS.md` § "Layout quality: the first real-photo run"** — four
  complaints from the only time a human has looked at a generated book. These are the
  first evidence the project has about whether the engine produces a *good* book as
  opposed to a *valid* one.

This design closes nine items across both. It does not add a new phase; it finishes this
one.

### In scope

| # | Item | Source |
|---|---|---|
| 1 | Slot kinds: the first and last slot are not spreads | complaint 1, open item 4's remainder |
| 2 | `face_quality` scoring term | complaint 2 |
| 3 | `spread_diversity` scoring term | complaint 3 |
| 4 | `hero_prominence` scoring term | complaint 4 (cheap half) |
| 5 | `gutter_saliency` penalty | open item 1 |
| 6 | Seed reaches `best_spread` | open item 3 |
| 7 | `from_features` refusals stop being silent | open item 11 |
| 8 | `preflight_core` becomes genuinely pure | open item 9 |
| 9 | Doc corrections | open items 6, 10 |

### Out of scope, with reasons

Each of these is deferred on evidence, not on effort.

- **RAW coverage end to end** (open item 5). macOS has no writable RAW type, so any
  fixture this repo could generate would be a renamed JPEG that tests the
  extension-matching path while *looking* like RAW coverage. Only closable at a real-photo
  run with actual camera files.
- **The two unmeasured constants** (open item 7) — `Exporter.cropRect`'s `1e-6` tolerance
  and `EXPORT_BATCH_SIZE`/`timeout_for`. Both are the kind of number only real data can
  justify.
- **`palette_harmony`'s Oklab recalibration** (open item 2). It stays parked and keeps its
  current weight. `spread_diversity` (§4.3) supersedes the need for it, and changing it
  now would move the golden fixture for no measurable gain. See §4.5.
- **The `ORDER BY position` guard** (open item 8). The clause is correct to keep and needs
  no code change; the honesty about what its test actually catches is already recorded.
- **Eyes-closed / mid-blink detection.** Derivable from landmark geometry already carried,
  but it is the same shape as the smile proxy, which had a proven 100% false-negative rate
  when finally measured. It must be measured against real faces before it is trusted.
- **Text rendering** (layout-engine design §5.4). Ruled out by the §3 decision below: banning size-1
  spreads removes the blank-page symptom without reversing §5.4.
- **The packer half of complaint 4.** Deferred by explicit decision — see §4.4.

### Already closed; the doc is stale

**Open item 6** states the UI never calls `open_project`, so Export is only offered for the
book generated in the current session. It does call it —
`app/composables/useBook.ts:162`, landed in `3e934e9`. This is a documentation fix, not
work. **Open item 10** already records that nothing is outstanding on culling.

---

## 2. The one thing that reframes complaint 1

**A blank page half at group size 1 is structural, not an authoring accident.**

A 1-photo spread template has exactly one slot. The validator forbids a slot from spanning
the fold (`TemplateError::SpansFold`, and `31afe4d` removed 21 templates for violating it).
Therefore one of that template's two halves must have zero slots, and the page it becomes
prints white.

All eight of the library's 1-photo templates are in that state — `03`, `04`, `05`, `06`,
`28`, `30`, `36`, `41` — and **there is no way to author a ninth that isn't.**

`PROJECT-STATUS.md` proposes "stop drawing zero-slot page-halves from `page_half_pool`" as
the cheapest fix. That is aimed at the wrong pool: `best_single` already refuses an empty
half (`!l.slots.is_empty()`), so `page_half_pool` is not the source. The blank pages come
from the **spread** pool, where `spreads_with(1)` returns only templates with an empty
half. The golden fixture demonstrates it: spread group sizes `[3,1,3,1,2,2,3,3,3]`, both
`1`s are `03-hero-left-text-right`, and pages 5 and 9 print blank.

Two further templates have an empty half by authoring choice rather than by structure, and
can be rebalanced: `20-four-up-windowpane` (4 left, 0 right) and
`45-three-up-mosaic-margin-left` (3 left, 0 right).

**Decision: ban size 1 in the spread region.** Middle spreads take 2..=6 photos. The two
true single pages keep size 1, where a single photo on a single page is exactly right.

---

## 3. Slot kinds — `pack` learns the first and last slot are different

This is the largest change and it closes two items at once.

### The problem

`pack` today takes one `buildable: &[usize]` and one flat slot budget
(`capacity.spreads + capacity.singles`). It sizes every group as though it were going on a
spread. `assemble` then hands group 0 to page 1 and the last group to page N — both of
which are *page halves*, not spreads.

That single-set assumption causes both of these:

- **The blank halves** of §2, because size 1 is buildable book-wide.
- **Open item 4's remaining item**, verbatim: "`pack` sizes every group by SPREAD counts,
  but two of the eleven slots are single pages holding only a page-half's worth, so a group
  landing on one is trimmed by `pace::strongest`. In the golden that costs one photo on the
  opening page."

### The change

`pack` gains **per-slot buildable sets** rather than one:

| Slot kind | Buildable sizes | Rationale |
|---|---|---|
| **Single** (first and last) | `1 ..= largest_half` | A single page is one page-half. `largest_half` is `lib.page_half_pool()`'s largest slot count — the figure `Capacity::from_library` already computes and `pack` currently ignores. |
| **Spread** (the middle) | `2 ..= largest_spread` | Size 1 removed per §2. |

Both bounds are **derived from the loaded library**, never hardcoded. The real library is
currently `1..=6` for spreads, so the spread set becomes `2..=6` today — but a future
template changes that figure, and `buildable_sizes(lib)` is already the single place that
answers the question.

`feasible_band`, `choose_group_size` and `size_bounds` take the set for the slot currently
being filled rather than a book-wide constant. The induction that makes "every slot filled
and every photo placed" hold now runs over a per-position band instead of a constant one —
the argument is unchanged in shape, only its bounds vary by position.

**The global slot index is already available.** `pack`'s density-swing computation reads
`groups.len()`, which is exactly the index of the slot about to be filled, counted across
chapters rather than within one. Slot kind is therefore
`Single` when `groups.len() == 0 || groups.len() == slots - 1`, else `Spread`.

### `Group` carries its slot kind; `assemble` stops guessing

`assemble` currently re-derives which groups are singles with `groups.first()` and
`groups.last()`. Once slot kinds differ, that re-derivation can **disagree with what `pack`
actually sized for**: when photos run out, `pack` produces fewer than `slots` groups, so
the last group it cut sits at an index below `slots - 1` and was sized as a *spread*, yet
`groups.last()` would place it on page N.

`Group` therefore gains an explicit `slot: SlotKind` field, and `assemble` places by that
tag rather than by position.

**Accepted consequence, to be pinned by a test:** when `pack` under-produces (fewer photos
than slots), no group is tagged for the closing single, so page N comes out blank. That is
the same honest outcome `assemble` already produces for a middle spread with no group left,
and it is preferable to placing a spread-sized group on a page half and letting
`pace::strongest` silently trim it — which is the defect this section exists to close.

### Side effects, accepted deliberately

- **The density axis loses its sparse end in the middle.** In this library density is a
  strict function of slot count (1 → sparse, 2..4 → medium, 6 → dense), so removing size 1
  removes `Sparse` from every spread. `repace` still has the energy and edge-treatment
  axes, both of which genuinely vary within a photo count. Rebalancing `20` and `45` (§2)
  so they use both pages adds real density variety back at sizes 3 and 4.
  `pace_spread_density_varies_across_a_book_rather_than_converging` must be re-examined
  against this, not merely re-baselined.
- **Minimum capacity rises.** A 20-page book goes from `11 × 1 = 11` photos to
  `2 × 1 + 9 × 2 = 20`. `recommend_page_count` and `Capacity::min_photos` shift with it.
  This is a correction, not a regression: the old figure recommended books that would print
  half-blank.
- **`Capacity::from_sizes` needs the same split.** It exists so the pack tests can run
  against a plain list of sizes without loading template files, and it already
  over-estimates the two singles by measuring them against the largest whole *spread*.
  Under slot kinds that over-estimate gets worse in one direction and wrong in the other:
  its `min_photos` would keep using the book-wide smallest size (now 1, from the singles)
  for every slot including spreads, understating the true minimum. It takes the same
  per-slot-kind bounds, or the tests that use it stop describing the packer they test.

---

## 4. Four new scoring terms, all shipped inert

### 4.0 The calibration policy

Every new term lands **fully implemented and unit-tested, with weight `0.0`** in
`templates/weights.json`.

This is a deliberate response to a project-specific risk. The work adds or changes four
scoring terms at once; every one is uncalibrated; and this project has no ground truth for
"a better book" beyond the user's eye. Landing them live would move the golden fixture
wholesale and leave a green test suite saying nothing about whether the output improved.

Shipping inert means:

- the golden fixture and every existing test stay byte-identical;
- each commit is **provably behaviour-neutral**, so a regression during this work is
  attributable to the slot-kind change of §3 and nothing else;
- tuning happens afterwards, on the user's own photographs, one term at a time, with the
  settled weights recorded back into `PROJECT-STATUS.md`.

`templates/weights.json` is already hot-reloadable by design ("so taste is tunable without
a rebuild", layout-engine design §4.3), so raising a weight needs no rebuild.

**`Weights` deserialisation must change with it.** The struct derives `Deserialize` with no
field defaults, so adding four required fields would make `Weights::load` reject the
existing file. The new fields get `#[serde(default)]` individually — so an older
`weights.json` still loads and the new terms stay inert — while the seven existing fields
stay required, so a truncated file still fails loudly. The struct also gains
`#[serde(deny_unknown_fields)]`: without it, a typo'd weight name would silently read as
`0.0`, which is precisely the invisible failure this project keeps finding. `Weights::default()`
sets the four new fields to `0.0` so it agrees with the shipped file.

### 4.1 `face_quality` — complaint 2

**Per-slot term.** Reads `Photo.capture_quality`, Vision's own score for blur, exposure and
pose on the best face in the photo.

The engine has **no expression signal whatsoever** and cannot get one: Vision has no
expression classifier at any macOS version, and the geometric smile proxy has a measured
100% false-negative rate and is deliberately unused. `capture_quality` is the signal that
already exists and is underused — today it is only a tie-break inside `cull`, never a
scoring term. Promoting it is the cheapest real improvement available for "some facial
features are too candid/unprepared/unflattering" and needs no new analysis.

Returns `0.5` when `capture_quality` is `None` — a faceless photo is not a bad photo. This
matches the convention `saliency_retention` and `face_area_retention` already use for an
absent signal.

### 4.2 `spread_diversity` — complaint 3

**Spread-global term**, added once per spread like `palette_harmony`, not per slot.

Near-duplicate clustering is perceptual-hash based, so it only collapses near-identical
frames; photos of one moment from slightly different angles all survive, and there is no
within-spread diversity term at all.

Computed as the mean pairwise distance across the group over three components, each
normalised to `[0,1]` and combined with **equal weight**:

- **Scene-tag distance** — Jaccard distance over `sceneTags`.
- **Palette distance** — over the dominant colours already carried.
- **Capture-time gap** — saturating, so photos minutes apart read as similar and photos
  hours apart do not.

Equal weighting is a starting point, not a claim. The three components are not
commensurable and no measurement exists to weight them against each other; sub-weights are
a tuning question for step 4 of §8, and if one component turns out to dominate, the honest
fix is to measure it rather than to guess a ratio. A group of fewer than two photos, or one
where every component is absent, returns `0.5` — the neutral answer this scorer already
uses for a missing signal.

**No wire change is needed.** `finalize_photos` strips only `phash` before the record
reaches the webview, so `sceneTags` and `exif.captureDate` both survive the round trip that
`generate_book` sends back. `Photo` gains `scene_tags: Vec<String>` and
`captured_at: Option<i64>`, parsed in `from_features`.

Their strictness classes differ and this matters: `sceneTags` is always emitted by Swift,
so it is **required** — an absent key is a pipeline bug. `exif.captureDate` is legitimately
`null` on a photo with no EXIF date, so it is **optional**. Getting this backwards would
either fail every book or silently degrade the term to a constant.

`phash` is deliberately not used, despite being the most direct similarity signal: it is
stripped before the webview precisely because JavaScript loses precision above 2^53, and
re-widening the wire to carry it is a larger change than this term justifies.

### 4.3 `gutter_saliency` — open item 1

**Per-slot term.** The Phase 2 layout-engine design's §4.2 states the rule plainly: "a
**face** in the dead strip is a rejection; generic salient content there is only a
**penalty**." Only the rejection half exists. That document's §4.3 weights table lists no
such term, and `Weights` has no field for it —
`score_does_not_reject_generic_saliency_in_the_gutter_strip` pins that generic saliency is
deliberately not a rejection, but nothing implements the graded penalty.

The ruling was "implement it or correct the spec — do not leave the spec claiming a term
the scorer does not have." **We implement it.**

Measures the fraction of the saliency box that lands inside the gutter dead band once
mapped into page coordinates, and scores `1.0 - that_fraction`. The mapping helper already
exists: `score::face_in_page` does exactly this for face boxes and is reused rather than
reimplemented.

This also removes the mismatch: the layout-engine design's §4.3 table gains the row.

### 4.4 `hero_prominence` — complaint 4, cheap half

**Spread-global term.** `hero_match` already rewards the highest-aesthetic photo landing in
a `hero` slot, but only *within a group that has already been formed* — `pack` sizes groups
from chapter boundaries and capacity alone and knows nothing about photo merit, so a
standout image can be dealt into a six-up and get a sixth of a spread.

When the group's top aesthetic percentile clears the rest by a margin, this term rewards
templates whose `hero` slot holds a large share of the spread's total slot area. It biases
*template choice* for a given group; it does not change how groups are formed.

**The packer half is deferred by decision.** Letting merit influence group size in `pack` is
the real fix, and it is a genuine change to `pack`'s contract — `feasible_band` and
`apportion_slots` are what make "every slot filled, every photo placed" hold by induction,
and merit would have to bend that band without breaking it. Section 3 already changes that
contract once. The agreed sequence is: ship the scorer bias, measure it against a real
book, and only then decide whether the packer change is still needed.

### 4.5 What `palette_harmony` does, and why it is not touched

It stays exactly as it is, at weight `0.2`.

It is already recorded as effectively inert (open item 2): it computes hue as
`atan2(b, r)` over sRGB rather than Oklab, its circular-variance floor is about `0.707`, and
at weight `0.2` it contributes roughly 1% of a spread's score.

It is also the term that **rewards hue agreement between the photos on a spread** — which
means the one aesthetic term that exists actively pushes toward the sameness complaint 3 is
about. That makes it the obvious thing to trade against `spread_diversity` during tuning.
Changing it now would move the golden for no measurable gain, so the trade is a tuning
decision, not an implementation one.

---

## 5. The seed reaches `best_spread` — open item 3

`best_spread` breaks ties on `t.id < bt.id`. That is deterministic and stable across
machines, which is right, but it is **seed-blind**: `assemble(photos, pages, lib, w, seed)`
passes `seed` only to `single_page` → `best_single` → `tie_break`, so the seed influences
only the first and last pages and never a middle spread.

The layout-engine design's "regenerate this spread advances the seed" is consequently unimplementable for
middle spreads as the code stands — advancing the seed changes nothing. `Book.seed` is
persisted, so the data model is ready; the plumbing is not. The ruling was that this is a
**Phase 3 blocker** to be decided before planning Phase 3, not during.

**Decision: thread the seed through.** `best_spread` gains a `seed: u64` parameter and
adopts the shape `best_single` already uses — collect every candidate at the maximum score,
then index with `tie_break(seed, n)`.

Exact equality is the definition of the tie the seed exists to break; anything looser would
let the seed override a real preference. `best_single`'s comment says this and the same
reasoning applies unchanged.

Two callers update: `pace::spread_pages` and `pace::repace`.

This unblocks Phase 3 without building any of it.

---

## 6. Small correctness items

### 6.1 `from_features` refusals stop being silent — open item 11

`from_features` refuses a feature record missing a field the book depends on. That is the
point of it, and `photos_from_records`/`resolve_photos` surface the refusal loudly. But
`stamp_kept` and `count_keepers` consume it through `filter_map`, so there a malformed
record is silently skipped: the photo comes back `kept: false`, or the completion
notification undercounts, with no error anywhere.

The `filter_map` shape predates the strictness — it was harmless when `from_features`
defaulted everything and could only fail on a truly unparseable record. It now has a larger
blast radius.

Both functions return a refusal count alongside their result, and the callers surface it. A
photo silently marked `kept: false` is exactly the invisible failure the strictness was
added to prevent.

Also fixes the misattributed comment at `commands.rs:2444`, which says a record is "silently
dropped by `from_features`'s `filter_map`" — the `filter_map` belongs to the caller, not to
`from_features`.

### 6.2 `preflight_core` becomes genuinely pure — open item 9

Its doc comment says "No I/O" and it performs one `Path::exists()` per placement, for the
moved-source-file Block check.

The existence check moves out to the I/O shell, which passes in the set of missing paths.
The doc comment then becomes true, and Phase 3 gets a core it can actually re-run in a hot
loop on an edited book without touching the filesystem — which is the reason the pure-core
split exists at all.

### 6.3 Documentation

`PROJECT-STATUS.md` is updated for: open item 6 already being closed by `3e934e9`; open
items 1, 3, 9 and 11 being closed by this work; open item 4's remaining item being closed by
§3; and the settled weights once tuning happens. The layout-quality section records which
complaint each term addresses and what weight was settled on.

The Phase 2 layout-engine design's §4.3 gains rows for the four new terms, which removes the
§4.2/§4.3 mismatch that open item 1 is about.

---

## 7. Testing

This project's standing rule is that a test which passes with the feature deleted is worse
than no test, because it reads as protection. Phase 1 shipped eight such tests; Phase 2
found thirteen more. **Every load-bearing test added here gets a mutation proof pasted into
its commit message.**

Three are hard to write honestly and are called out now, before they are written:

**Slot kinds (§3).** The regression must fail with a *specific* wrong output, not merely
"some book". Pin it as: no middle-spread page has zero placements, over a fixture where the
old rule provably produced one. A fixture whose groups happen never to be size 1 passes
under both rules and proves nothing.

**Inert terms (§4).** A term at weight `0.0` is trivially "passing" — the whole class of
decorative test this project keeps finding. Each term is therefore tested **twice**: the
pure function directly, and through `score_spread` with a non-default `Weights` fixture
giving it a non-zero weight. A test exercising only the shipped weights would pass with the
function body deleted.

**The seed in `best_spread` (§5).** Needs two templates with genuinely identical scores. The
`pace.rs` fixture library already engineers exactly this — `f5`'s and `f7`'s right halves
are deliberately identical so a tie is reachable at all — and the new test needs the same
treatment at spread level. A fixture where one template wins on merit tests nothing, and
would pass under an implementation that ignores the seed entirely.

Two further rules from the project's list apply directly here and are easy to violate:

- **Non-square fixtures are mandatory** for `hero_prominence` and `gutter_saliency`, both of
  which are area- and aspect-dependent. A square box makes `height/width == 1` and hides the
  whole class of bug.
- **A boundary test must use a value near the boundary.** `gutter_saliency` has an obvious
  one — a saliency box just inside versus just outside the dead band — and an obvious way
  to get it wrong.

---

## 8. Sequencing

Section 3 changes behaviour; section 4 does not. Ordering them so the behaviour-changing
work lands first and alone is what makes the rest attributable.

1. **§3 slot kinds** — the only behaviour change. The golden fixture is re-baselined here,
   once, with the before/after diff in the commit message.
2. **§5 seed**, **§6.1 refusals**, **§6.2 preflight purity** — independent of each other and
   of §4; each behaviour-neutral except the seed, which changes only which of two exactly
   tied templates wins.
3. **§4 the four terms** — each its own commit, each provably behaviour-neutral at weight
   `0.0`.
4. **Tuning** — on the user's own photographs, one term at a time, weights recorded back
   into `PROJECT-STATUS.md`.

Step 4 is not optional and is not something an agent can do: it needs a human looking at a
book. Until it happens, the four terms are implemented and dormant, and the honest
description of this work is "the machinery exists and is unproven", not "the book is
better".

---

## 9. What this does not prove

**Nobody has still generated a real book from real photos and uploaded a page to Pixajoy.**
That remains the outstanding verification, it is unchanged by this design, and none of the
work here substitutes for it. Section 3 changes what the packer produces; only a human
looking at printed output can say whether it produces something good.
