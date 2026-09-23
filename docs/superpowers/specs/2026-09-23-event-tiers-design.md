# Event tiers: design

**Date:** 2026-09-23
**Status:** draft. The user answered the open questions on 2026-09-23 (§11). The spec has not been approved as a whole yet.

A book should show the events of a trip that are worth showing. It should not show every
event, and it should not favour whichever event had the most shooting. This spec gives
every event a **tier**. The app suggests each tier with a stated reason, the user can
override it, and the photo budget follows the tiers.

## 1. The problem today

`pack::select` (`src-tauri/src/book/pack.rs:537`) picks the book's photos from the whole
library before any chapter exists. `pack` then groups them by `event_cluster`
(`pack.rs:425`). `select` never reads the event. It ranks every moment's best photo by
aesthetic percentile and takes photos until it reaches `target_photos`. So:

- **A short or low-scoring event can get no photos.** On Iceland at 40 pages, 85 of 230
  moments place a photo, and nothing checks which events those moments belong to. A dinner
  or a night market shot in poor light loses to a landscape day.
- **An event's space follows how long you shot it, not how much it matters.** A 34-minute
  walk is about 17 moments. A 3-minute key moment is 1 or 2.
- **No event can be left out as a whole.** The third afternoon at the same pool, the
  airport, or a run of screenshots all compete like any other event. Leaving one out today
  means pressing **−** on each of its photos.
- **The user can't see any of this.** No screen shows events against what they place
  (see the review of 2026-09-23 and `PhotoTile.vue:53`).

Guaranteeing every event a place was considered and rejected. A long trip would spend its
pages on events that don't deserve them. The user ruled on 2026-09-23 that some events
should be left out automatically or by hand.

## 2. Tiers

| Tier | Photo floor | Share of the rest | Cap |
|---|---|---|---|
| `Featured` | `featured_floor` (**6**, set per project) | `FEATURED_WEIGHT` (2.0) × its share | usual per-moment cap |
| `Normal` | `NORMAL_FLOOR` (2) | 1.0 × its share | usual per-moment cap |
| `Brief` | 1 | `BRIEF_WEIGHT` (0.5) × its share, up to the cap | `brief_cap` (**2**, set per project) |
| `Skipped` | 0 | none | 0 |

- **Brief** takes one photo, and a second if the photos left over after the floors reach
  it. It never gets a chapter of its own: `merge_sub_spread_chapters` (`pack.rs:605`)
  already folds a one-photo chapter into the next one, and this spec extends it to fold a
  Brief chapter **of any size** the same way (into the next chapter, or the previous one
  when it is last). A 2-photo Brief event would otherwise meet the spread minimum of 2
  and take a whole spread. So a Brief event shares a spread with its neighbour and keeps
  its place in the story without taking a spread.
- **Skipped** places nothing, but its photos are still shown on the contact sheet, dimmed,
  with the reason.
- **An `Include` always wins.** A photo marked **+** is placed even when its event is
  Skipped, and it counts towards its event's floor. An `Exclude` is never placed. Tiers
  sit between the per-photo decisions and the engine's own choices. They never override
  a per-photo decision.

### 2.1 Per-project settings

The user ruled that the Featured floor and the Brief cap are set **per project**. They
join `BookOptions` (`pace.rs`, already saved on `Book` and on the draft, with
`#[serde(default)]`):

```rust
pub struct BookOptions {
    pub places: bool,
    #[serde(default = "default_featured_floor")] // 6
    pub featured_floor: u8,  // accepted 2..=12
    #[serde(default = "default_brief_cap")]      // 2
    pub brief_cap: u8,       // accepted 1..=3
}
```

A value outside those ranges is refused at the boundary, the same way `PrintSpec` is
built only through its validator (a `RawBookOptions` → `BookOptions` `try_from`). A book
saved before this change has neither key and loads with 6 and 2. The draft screen shows
both as steppers under **Split chapters by place**: "Featured events get at least **6**
photos" and "Brief events get at most **2**". Both re-run `recommend_book` when changed,
as the Places switch does.

Every event has a **suggested** tier and an **effective** tier. The effective tier is the
user's choice if there is one, and otherwise the suggestion.

## 3. Merit: how the app ranks events

Merit ranks events against each other within one book. It is not an absolute grade. The
components below are computed per event (`EventId` = the chapter id from
`book::chapter::chapters`, so the Places switch is respected), each scaled to 0..1:

| Term | Definition | Why |
|---|---|---|
| `engagement` | `ln(1 + moments) / ln(1 + most moments of any event)`, using `pack::moments` | Counts distinct things photographed, not burst length. The log flattens the difference between a 400-photo event and a 60-photo one. |
| `quality` | Mean `aesthetic_pct` of the event's best 3 cull keepers, ÷ 100 | Whether the event holds good pictures, not just many. |
| `novelty` | `1 − similarity` to the most similar event ranked above it | So the third pool afternoon ranks below the first two. See §3.1. |
| `utility` | Fraction of the event's photos with `is_utility` | Screenshots, receipts and documents. |

```
merit = (W_ENGAGEMENT·engagement + W_QUALITY·quality + W_NOVELTY·novelty) × (1 − utility)
```

The weights start at 0.4 / 0.4 / 0.2 and are **chosen, not measured**. §9 is how they get
measured. They are named constants in a new module, `book::events`, not entries in
`templates/weights.json`, which holds template scoring terms only.

### 3.1 Novelty

Events are walked in order of merit computed without novelty. Each one is compared with
the events already ranked above it, and `similarity` is the highest of:

- **GPS:** `1 − min(1, km / NOVELTY_KM)` between chapter centroids
  (`chapter::centroids`), when both events have locations. `NOVELTY_KM` = 2.
- **Scene:** Jaccard similarity of the two events' top-10 scene tags.
- **Look:** a similarity built from the distance between the events' mean feature prints.

**Risk: feature prints may not separate events.** The calibration of 2026-09-19 measured
*pairwise* print distances of p50 0.93–0.99 inside an event against 1.02–1.03 across
events. Those ranges overlap almost completely. Mean prints have never been measured. The
plan's first task measures mean-print distance between pairs of events labelled by GPS,
with no human needed: same place (centroids within 500 m) against different places
(more than 20 km apart), on Bali and Iceland. If it doesn't separate them, the
**Look** component is dropped rather than tuned. It is a guess until then.

## 4. Suggested tiers

`book::events::suggest(events, capacity) -> Vec<Suggestion>` is pure and runs every time
the photos, overrides or page length change. Rules, in order:

1. **Skipped**, when `utility > UTILITY_SKIP` (0.8), or when no photo in the event is a
   cull keeper. Reason: `Utility { share }` or `NothingKept`.
2. **Brief**, for the undated chapter (`captured_at` is `None` for all its photos).
   Reason: `Undated`. Undated photos have no position in the trip, so they are never
   treated as a real event by default.
3. The remaining events are ranked by merit, and then:
   - **Normal** for the top `normal_room` events. Reason: `Ranked { rank, of }`.
   - **Featured** for events in that set whose merit is at least `FEATURED_MARGIN` (1.3)
     × the median Normal merit, up to `normal_room / 6` (at least 1 when any event
     qualifies). Reason: `Standout`. Promoting an event to Featured must not push the
     floors past `target_photos` (step 3 of §5 applies): with a floor of 6, a 20-page book
     (target 45) seats 1 Featured and 5 Normal events at 16 photos of floor.
   - **Brief** for the rest. Reason: `OutOfRoom { rank, of }`, or `SimilarTo { event }`
     when novelty below 0.3 is what pushed it out.

`normal_room` is how many events the page length can give a chapter of their own:
`min(eligible events, floor(slots × NORMAL_SLOT_SHARE))`, where `slots` = spreads +
2 single pages and `NORMAL_SLOT_SHARE` = 0.6. So a 20-page book (11 slots) seats 6
Normal-or-Featured events and a 40-page book (21 slots) seats 12. The remaining slots give
the better events depth. This constant is chosen and is tuned in §9.

Suggestions depend on the page length. Switching from 20 to 40 pages promotes Brief
events to Normal, and the draft screen says so (§7).

## 5. Budget

A new step, `book::events::budget(...)`, replaces the book-wide trim inside `select`.
`select` keeps its within-moment logic and is called once per event with that event's
quota.

1. **Includes first.** Every `Include` photo is taken and counts towards its event's
   floor. `IncludeOverflow` is unchanged.
2. **Floors.** Each event gets `min(floor(tier), available)`, where the Featured floor
   is `options.featured_floor`. Here `available` is the
   event's cull keepers after `Exclude`s, capped at `MAX_PER_MOMENT` per moment.
3. **Floors that don't fit** (floors add up to more than `target_photos`):
   - Demote the lowest-merit *suggested* Normal events to Brief until the floors fit.
   - A user's choice is never demoted. If the user's own Featured and Normal floors still
     don't fit, cut every floor to 1 and report `TierOverflow { needed, capacity }`. It
     is a warning in `BookRecommendation`, not an error: the book is still generated.
4. **Remainder.** Deal out the photos left after the floors to Featured, Normal and Brief
   events by D'Hondt highest averages, with a vote of `tier_weight × sqrt(moments)`
   (`tier_weight` is 2.0, 1.0 or 0.5). An event drops out of the deal when it has used up
   `available`, and a Brief event also drops out when it reaches `options.brief_cap`. The square root is what stops a
   400-photo event from swamping the book.
5. **Inside an event**, the photos are ranked **once** into an ordered list:
   `Include`s, then each moment's best photo, then each moment's second, stopping at
   `MAX_PER_MOMENT` per moment. This is today's `select` order, restricted to one event.
   A quota takes a **prefix** of that list. This is the nested "curation levels" model
   Apple describes for Memories (WWDC17 session 505): one ranking per event, with each
   density level a prefix of the next. It gives the stability property in §9 for free,
   because more pages or a higher tier can only add photos to an event, never swap
   them.

After that, `pack` is unchanged. Chapters are built from what was selected. Brief events
(1 photo) fold into their neighbour through `merge_sub_spread_chapters`, and
`apportion_slots` shares out the slots.

**Invariant, and the test that guards it:** every Featured and Normal event places at
least `min(floor, available)` photos in the finished book, or `TierOverflow` was
reported. It is checked on the output of `assemble`, like `IncludedNotPlaced`, because
there are several places after `select` where a photo can still be lost.

## 6. Storage

Event ids are positions (`cluster::event_clusters`, `chapter::chapters`). Adding a folder,
turning Places on, or a future clock fix renumbers them. So **a tier choice is stored on
the event's photos, by content hash**, as include/exclude already are.

- The UI sets a tier on an event by sending `{hash: tier}` for every photo in it.
- `EventTiers = BTreeMap<String, Tier>`, with only user choices stored and no `Auto` row,
  the same rule as `Overrides`.
- **Resolving an event's tier:** among its photos' stored tiers, the one held by the most
  photos wins, and ties go to the higher tier. If no photo has a stored tier, the event
  takes the suggestion. So a photo added to an event later inherits the event's tier, and
  two events merged by re-clustering keep the choice made for most of their photos.
- **Drafts:** add a `tiers` field to the draft JSON beside `overrides`
  (`useAnalysisJobs.ts:142`). Parse it the same way, and fail the load on an unknown
  token.
- **Projects:** a new table `project_event_tiers (project_id, hash, tier)`, written in the
  same transaction as `project_photo_overrides` and returned on `ProjectDetail.tiers`.
- **Wire:** `recommend_book` and `generate_book` take `tiers` beside `overrides`. The
  cached photos stay in Rust, as they already do, and only the map is sent.

## 7. What the user sees

**The contact sheet's event header** (`ContactSheet.vue`, the event row) gets a
four-way control: **Featured · Auto · Brief · Skip**. **Auto** shows the suggestion in
place ("Auto: Normal"), and hovering shows the reason sentence. A Skipped event's photos
are dimmed with the reason. The monochrome style applies, so tiers are shown with type and
weight, not colour.

**An events panel on the draft screen** (`GenerateBook.vue` inspector), one row per event:

```
Ubud · 12–13 Mar          Normal (auto)    38 kept → 6 placed
Airport · 11 Mar          Skip (auto)      mostly screenshots
Pool, day 3 · 15 Mar      Brief (auto)     similar to "Pool, day 1"
Mt Batur sunrise · 14 Mar Featured (you)   24 kept → 9 placed
```

The title is the place name when Places is on, and otherwise "Event N". Clicking a row
scrolls the contact sheet to that event. The panel sits above the page-length choice and
updates with it.

**Page-length options** (`PageOption`) gain `events_by_tier`, so the choice reads
"40 pages · 12 of 19 events, 85 photos" rather than just a photo count.

**The dimming bug is fixed here.** Tiers make the gap between "kept" and "placed" much
bigger, so the contact sheet has to dim by *placed at this length*, not by the cull result.
`recommend_book` returns the selected paths for the chosen length, not just the count.
`manual.md:98` changes to match.

**In the editor**, tiers are read-only. Changing one needs **Update**, which regenerates
the book. Update already throws away every edit without saying so (review item 12). This
change adds the missing confirmation before Update, and a manual entry for it, because
tiers make Update much more common.

## 8. Out of scope

- **Placing a hero for a Featured event.** Featured means more photos in this spec. Making
  its best photo large is a change to how `pack` sizes groups, which has broken things
  twice before. It gets its own spec, measured with the §9 report.
- **Fixing clocks and time zones** (`ExifReader.swift:23` reads times as UTC). Wrong clocks
  produce wrong events, and tiers inherit that. It is a separate spec.
- **Splitting events within a day** without Places. That is a separate spec. The
  likely method is PhotoTOC's adaptive gap (Platt 2003): a boundary wherever a gap is much
  larger than the average of the gaps around it, with the 4 h gap kept as an upper bound.
- **Choosing for variety inside an event.** Moments are ranked by aesthetic score today.
  The published model for doing better is a greedy selection that trades quality against
  diversity (Google patent US8983193B1 uses a determinantal point process; Sinha et al.
  WWW'11 add coverage). It only changes how the list in §5 step 5 is ordered, so it slots
  in behind this spec without changing the budget. It gets its own spec.
- **Agent tools.** The agent view can show each event's tier, but there is no
  `set_event_tier` tool in this spec.
- **An absolute quality floor for single photos.** A related idea, specified separately.

## 9. Measuring it

**Step 1 is a report, built before any tier code.** `src-tauri/examples/book_report.rs`
loads analysed photos from the features cache (derived data only; pixels are never read)
and assembles books across seeds and page lengths. For each book it prints:

- events with 0 photos placed; photos per event against its moments; the fewest photos
  in any chapter;
- blank pages and dropped photos, the figures `pack_sweep` already checks;
- after this change: events per tier, and every event that places less than its floor.

Run it on today's engine first. The number of events at 0 photos on Iceland (11 events),
Bali (8) and Vietnam is the baseline this change has to beat. If it is already close to
zero, this spec is worth less than it looks, and that should be known before building.

**Checking the suggestions without a person (primary).** The user ruled that tuning is
algorithmic first and hand-tiering second. Four checks, all run by the report and all
repeatable:

1. **Scenario fixtures with known answers.** Built from real cached feature records
   re-timed and re-tagged into trips whose correct tiers are not in dispute, not from
   invented photos:
   - a 40-photo event that is 90% screenshots (expect Skipped);
   - three pool afternoons with the same tags and GPS within 500 m on three days (expect
     one Normal and the others Brief);
   - an 8-photo low-light dinner between two 300-photo landscape days (expect at least
     Brief, and Normal at 40 pages);
   - an undated folder of 30 (expect Brief);
   - a single standout event with twice the moments and the top aesthetic scores (expect
     Featured).
2. **Coverage on the real libraries.** Events placing 0 photos, the Gini coefficient of
   photos per event, and each Featured or Normal event's share against its moment share.
   Compared against the baseline from step 1.
3. **Stability.** The same library at 5 seeds gives identical tiers (suggestions don't
   depend on the seed). Adding or removing one photo changes the tier of at most the
   event it belongs to. Switching 20 → 40 pages only ever promotes events, never demotes.
4. **Sensitivity.** Sweep each weight and threshold in §3 and §4 by ±50% and record
   which scenario fixtures flip. A constant whose ±50% flips no fixture is not
   load-bearing, and is either removed or fixed to a round value. A constant that flips
   one at ±10% is fragile, and the report says so.

The constants in §3 and §4 are settled when all four pass.

**Checking against the user (secondary).** Afterwards, and not blocking the ship, the user
tiers Bali and Iceland by hand without seeing the suggestions. The exact-match and
within-one-tier rates are recorded in PROJECT-STATUS as the first real-world figure. A
hand-Featured event suggested as Skipped is a bug to fix. A one-tier disagreement is a
tuning note.

**Mutation checks** (per CLAUDE.md, "On tests in this project"). Each must turn exactly
the named test red:

| Mutant | Test |
|---|---|
| `budget` ignores floors (goes back to the book-wide trim) | a 6-photo low-aesthetic Normal event beside a 400-photo high-aesthetic event places at least 2 |
| The D'Hondt vote uses `moments` without `sqrt` | a 400-moment event does not take more than `k` times the share of a 25-moment one |
| Skipped places photos | a Skipped event places 0 when it holds no `Include` |
| Brief ignores `brief_cap` | a Brief event with 10 moments and room to spare places at most `brief_cap` |
| A 2-photo Brief chapter is not folded | a 2-photo Brief event does not own a spread |
| `featured_floor` read from the constant instead of the options | a book with `featured_floor = 9` places at least 9 from its Featured event |
| An `Include` in a Skipped event is dropped | the included photo is placed |
| Tier resolution uses the first photo instead of the majority | a 10-photo event with 7 Brief and 3 Featured resolves to Brief |
| Suggestion ignores `utility` | a 90% screenshot event is suggested Skipped |
| Novelty is removed | of two events with identical tags and GPS within 1 km, the lower one is suggested Brief when room is tight |
| The output invariant is deleted | the `assemble` post-condition catches a Normal event losing its floor |

The fixtures must hold real-shaped data: several moments per event, and aesthetic
percentiles that actually differ. A fixture where every photo is its own moment hides the
bug, as `pack_sweep` shows.

## 10. Prior work this draws on

Researched 2026-09-23. No public source gives Apple's or any photobook service's
event-scoring formula, so the merit terms in §3 are our own and are measured in §9.

- Apple Photos Memories: nested curation levels, relevance weights 0..1, one key asset
  per memory (WWDC17 session 505); screenshots and receipts hidden from the Days view.
- Sinha, Mehrotra and Jain, WWW'11: a summary of a personal collection should maximise
  quality, diversity and **coverage**. The tiers are coverage at event level, and
  novelty (§3.1) is its diversity term.
- Loui and Savakis, IEEE TMM 2003: events, then quality screening, then the album. This
  is the pipeline this app already has.
- CEWE's assistant and Shutterfly's guidance: the page count sets the density (1–4
  photos a page). That matches taking `normal_room` from the page length.

## 11. Rulings (from the user, 2026-09-23)

| Question | Ruling |
|---|---|
| How many photos a Brief event gets | **Up to 2** (at least 1). |
| Featured floor | **6.** |
| Tunable? | Both the Featured floor and the Brief cap are **settings per project** (§2.1). |
| A **+** photo in a Skipped event | **Placed.** A per-photo decision beats the event's tier. |
| Undated photos | Start as **Brief**. |
| Hand-tiering | Willing, but **secondary**. Tune algorithmically first (§9). |
