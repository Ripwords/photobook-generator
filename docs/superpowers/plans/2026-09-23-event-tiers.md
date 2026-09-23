# Event Tiers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every event a tier (Featured, Normal, Brief or Skipped). The app suggests each tier with a reason, the user can override it, and the photo budget is dealt out per event by tier instead of trimmed book-wide.

**Architecture:** A new pure module `book::events` holds three things:
- the tier types and the user's `EventTiers` map (hash-keyed, like `Overrides`);
- the per-event statistics, merit and `suggest`;
- `budget`, which replaces the book-wide trim in `pack::select`.

`pack` gains `pack_selected`, which takes the budget's selection and a set of Brief events to fold. `pace::assemble` becomes a wrapper around `assemble_with`, which runs the budget and checks the floor invariant on the finished book. Commands, persistence and the draft screen carry the map and show the result. A report example (`examples/book_report.rs`) measures the baseline before any tier code exists, and it measures the result afterwards.

**Tech Stack:** Rust (Tauri 2 backend, `src-tauri`), rusqlite, serde. Nuxt 4 / Vue 3 / Nuxt UI (`app/`). vitest, cargo test, oxlint, vue-tsc.

**Spec:** `docs/superpowers/specs/2026-09-23-event-tiers-design.md`. Read it before starting any task. Section numbers below (§N) refer to it.

## Global Constraints

- Images never leave the machine. The report reads the `features` cache (derived JSON), never pixels.
- macOS arm64 only. Bun, not npm. Conventional Commits. Never `git commit --no-verify`.
- `bun run sidecar` once before the first `cargo` command in a fresh checkout.
- Never use `any` in TypeScript. oxlint warnings are failures (`bun run lint`).
- Run `bun run check:build` **and** `bun run typecheck` before any commit that touches `app/**/*.vue`.
- Stage explicit paths. Never `git add -A`. Before the first commit, check `git config --get user.email`. If it is neither `contact@jjteoh.com` nor `teohjjteoh@gmail.com`, run `git config --local user.email "contact@jjteoh.com"`. Never pass `-c user.email`.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- TDD: write the test, watch it fail, implement, and watch it pass. **Mutation-check each load-bearing test.** Break the code on purpose, confirm the named test goes red, restore the code, and paste the evidence in the task's commit body. (CLAUDE.md, "On tests in this project".)
- The tier constants (§2, §3, §4) are exact:
  - `FEATURED_WEIGHT` 2.0, `NORMAL_WEIGHT` 1.0, `BRIEF_WEIGHT` 0.5.
  - `NORMAL_FLOOR` 2, Brief floor 1.
  - `W_ENGAGEMENT` 0.4, `W_QUALITY` 0.4, `W_NOVELTY` 0.2.
  - `UTILITY_SKIP` 0.8, `FEATURED_MARGIN` 1.3, `NORMAL_SLOT_SHARE` 0.6.
  - `NOVELTY_KM` 2.0, `SIMILAR_BELOW` 0.3.
- Per-project settings (§2.1): `featured_floor` defaults to **6** and accepts 2..=12. `brief_cap` defaults to **2** and accepts 1..=3.
- The TypeScript tier tokens are exactly `"featured" | "normal" | "brief" | "skipped"`. "Auto" is the absence of a key and never a stored token.
- A user-facing change updates `docs/manual.md` in the same commit.
- Print geometry is per book (`Book.spec`). Never add a module-level page constant.

## Deviations from the spec found while planning

1. **Budget fill rule, a new step 4b.**
   - The problem: say a book's Normal events run out of photos while their Brief neighbours are capped. The book would then underfill and print blank spreads, which `pack_sweep` forbids and today's engine never does.
   - The rule: when the D'Hondt deal (§5 step 4) has photos left and no event can take them, promote the highest-merit *suggested* Brief event to Normal (reason `Filled`) and keep dealing.
   - A user's own Brief is never promoted.
2. **Step 3 demotes Featured first.** Step 3 of §5 names only Normal → Brief. `suggest` can also produce a Featured event whose floor overflows the book (§4 says "step 3 applies"). So step 3 first demotes suggested Featured events to Normal, and only then demotes suggested Normal events to Brief. The reason for both is `Demoted`.
3. **Which projects the report uses.** §3.1/§9 name Bali, but the local database has no Bali project. Task 2 calibrates on Iceland (project 5), Japan (11) and Vietnam (4), the projects that exist.
4. **Undated only counts beside dated events.** §4 makes an undated event Brief. In a library where nothing is dated (scanned prints, or the engine test fixtures) that rule would make every event Brief, so `suggest` applies it only when at least one event is dated.

Tell the user about all four when you hand off.

## File structure

| File | Responsibility |
|---|---|
| `src-tauri/src/book/events.rs` (new) | Tier types, `EventTiers`, resolution, `EventStats`, merit and novelty, `suggest`, `plan`, `budget`, `gini`, and the mean-print helpers |
| `src-tauri/src/book/mod.rs` | `pub mod events;` |
| `src-tauri/src/book/pack.rs` | `event_order` (select's ranking split per event); `pack_selected`; the Brief fold in `merge_sub_spread_chapters` |
| `src-tauri/src/book/pace.rs` | `BookOptions` fields and validator; `assemble_with`; `BookError::TierFloorNotMet` |
| `src-tauri/src/commands.rs` | `load_project_photos` (pub, for the report); `with_chapters`; `recommend` per-event rows; `recommend_book`/`generate_book` args; `ProjectDetail.tiers` |
| `src-tauri/src/db.rs`, `src-tauri/src/project.rs` | the `project_event_tiers` table; `save_project_with`; `Project.tiers` |
| `src-tauri/examples/book_report.rs` (new) | baseline and after measurements, novelty calibration, scenarios, stability, sensitivity |
| `tests/fixtures/wire/event-tiers.json` (new), `book-recommendation.json`, `project-detail.json` | wire shapes pinned from both sides |
| `app/types/book.ts`, `app/types/features.ts` | `Tier`, `EventTiers`, `TierReason`, `EventRow`, `withEventTier`, `tierReasonText`, `BookOptions` fields |
| `app/composables/useAnalysisJobs.ts` | draft `tiers` and the options parse |
| `app/composables/useBook.ts` | sends `tiers`/`options` to `recommend_book`, and `tiers` to `generate_book` |
| `app/components/SelectPhotos.vue`, `ContactSheet.vue`, `GenerateBook.vue`, `EventTierControl.vue` (new), `EventsPanel.vue` (new) | UI |
| `docs/manual.md`, `docs/PROJECT-STATUS.md` | manual entries and measured figures |

---

### Task 1: Offline loader and the baseline report

**Files:**
- Modify: `src-tauri/src/commands.rs`. Add `pub fn load_project_photos` near `resolve_photos` (around line 1337), and make `cached_records` `pub(crate)` if the loader needs it.
- Create: `src-tauri/src/book/events.rs` (starts with `gini` only).
- Modify: `src-tauri/src/book/mod.rs`.
- Create: `src-tauri/examples/book_report.rs`.

**Interfaces:**
- Produces: `app_lib::commands::load_project_photos(db: &app_lib::db::Db, project_id: i64) -> Result<Vec<Photo>, String>`. It returns the project's photos in `photo_hashes` order, finalised exactly as a fresh analysis would be (percentiles, event clusters, near-dup clusters).
- Produces: `app_lib::book::events::gini(values: &[usize]) -> f64`.
- Produces: the `book_report` CLI. Later tasks add flags to it.

- [ ] **Step 1: Write the failing `gini` test.** Create `src-tauri/src/book/events.rs`:

```rust
//! Event tiers: which events of a trip a book shows, and how many photos
//! each one gets. See docs/superpowers/specs/2026-09-23-event-tiers-design.md.

/// Gini coefficient of a distribution of counts: 0 when every value is
/// equal, approaching 1 when one value holds everything. The report's
/// measure of how evenly a book spreads its photos across events.
pub fn gini(values: &[usize]) -> f64 {
    let _ = values;
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gini_is_zero_for_equal_counts_and_high_for_one_winner() {
        assert_eq!(gini(&[5, 5, 5, 5]), 0.0);
        assert!((gini(&[0, 0, 0, 12]) - 0.75).abs() < 1e-9, "{}", gini(&[0, 0, 0, 12]));
        assert_eq!(gini(&[]), 0.0);
        assert_eq!(gini(&[0, 0]), 0.0);
    }
}
```

Add `pub mod events;` to `src-tauri/src/book/mod.rs`, in alphabetical order after `pub mod edit;`.

- [ ] **Step 2: Run the test and confirm it fails.** Run `cd src-tauri && cargo test --lib book::events`. Expected: it panics with `not implemented`.

- [ ] **Step 3: Implement.**

```rust
pub fn gini(values: &[usize]) -> f64 {
    let n = values.len();
    let total: usize = values.iter().sum();
    if n == 0 || total == 0 {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    // G = (2 * sum(i * x_i)) / (n * sum(x)) - (n + 1) / n, with i from 1.
    let weighted: f64 = sorted.iter().enumerate().map(|(i, &x)| (i as f64 + 1.0) * x as f64).sum();
    2.0 * weighted / (n as f64 * total as f64) - (n as f64 + 1.0) / n as f64
}
```

- [ ] **Step 4: Run the test and confirm it passes.** Run the same command. Expected: PASS.

- [ ] **Step 5: Add the loader.** In `commands.rs`, next to `resolve_photos`, add the function below. First check the exact names: `Db::load_project` returns `Option<Project>` with `photo_hashes`, and `finalize_photos(Vec<Value>) -> Vec<Value>` and `photos_from_records(&[Value]) -> Result<Vec<Photo>, String>` are at commands.rs:365 and :1297. If `photos_from_records` takes `&Vec<Value>`, pass `&finalized`.

```rust
/// A saved project's photos, rebuilt from the features cache exactly as a
/// fresh analysis would finalise them. For offline tools such as
/// `examples/book_report.rs`: derived records only, no pixel is read.
pub fn load_project_photos(db: &Db, project_id: i64) -> Result<Vec<Photo>, String> {
    let project = db
        .load_project(project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no project {project_id}"))?;
    let records = cached_records(db, &project.photo_hashes)?;
    let finalized = finalize_photos(records);
    photos_from_records(&finalized)
}
```

- [ ] **Step 6: Write the report.** Create `src-tauri/examples/book_report.rs`. It must copy the database first: `Db::open` runs migrations, and the report must never write to the user's real database.

```rust
//! Measures how a book spreads its photos across the events of a trip.
//!
//! ```sh
//! bun run sidecar
//! cd src-tauri && cargo run --release --example book_report -- 4 5 11
//! cd src-tauri && cargo run --release --example book_report -- --places 5
//! ```
//!
//! Arguments are project ids in the app's database. `--db PATH` points at
//! another database. The database is COPIED to a temp dir first, because
//! `Db::open` migrates, and a report must never write to the user's data.
//! Only derived feature records are read; no image is opened.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use app_lib::book::cull::{Overrides, Photo};
use app_lib::book::events::gini;
use app_lib::book::pace::{assemble, Book};
use app_lib::commands::load_project_photos;
use app_lib::db::Db;
use app_lib::print_spec::PrintSpec;
use app_lib::templates::{Library, Weights};

const PAGES: [u32; 2] = [20, 40];
const SEEDS: [u64; 3] = [7, 1234, 99];

fn default_db() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME");
    Path::new(&home).join("Library/Application Support/com.jiajingteoh.photobook/photobook.sqlite")
}

fn copy_db(src: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("book_report_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let dst = dir.join("photobook.sqlite");
    std::fs::copy(src, &dst).expect("copy database");
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", src.display()));
        if side.exists() {
            std::fs::copy(&side, PathBuf::from(format!("{}{suffix}", dst.display()))).expect("copy wal");
        }
    }
    dst
}

/// Photos placed per event, over EVERY event in `photos` (0 included).
fn placed_per_event(book: &Book, photos: &[Photo]) -> BTreeMap<u32, usize> {
    let mut out: BTreeMap<u32, usize> = photos.iter().map(|p| (p.event_cluster, 0)).collect();
    let placed: BTreeSet<usize> =
        book.pages.iter().flat_map(|p| p.placements.iter().map(|pl| pl.photo_index)).collect();
    for i in placed {
        *out.entry(photos[i].event_cluster).or_default() += 1;
    }
    out
}

fn with_places(photos: &[Photo]) -> Vec<Photo> {
    photos
        .iter()
        .zip(app_lib::book::chapter::chapters(photos, true))
        .map(|(p, event_cluster)| Photo { event_cluster, ..p.clone() })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let mut db_path = default_db();
    let mut places = false;
    let mut projects: Vec<i64> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => db_path = PathBuf::from(args.next().expect("--db PATH")),
            "--places" => places = true,
            id => projects.push(id.parse().expect("project id")),
        }
    }
    let db = Db::open(&copy_db(&db_path)).expect("open copied database");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
    let lib = Library::load(&root).expect("library");
    let weights = Weights::load(&root.join("weights.json")).expect("weights");
    let spec = PrintSpec::pixajoy();

    println!("project,pages,seed,photos,events,events_at_zero,gini,min_chapter,blank_pages,dropped");
    for id in projects {
        let photos = load_project_photos(&db, id).expect("load project");
        let photos = if places { with_places(&photos) } else { photos };
        for pages in PAGES {
            for seed in SEEDS {
                let book = match assemble(&spec, &photos, pages, &lib, &weights, seed, &Overrides::new()) {
                    Ok(b) => b,
                    Err(e) => {
                        println!("{id},{pages},{seed},ERROR {e}");
                        continue;
                    }
                };
                let per = placed_per_event(&book, &photos);
                let counts: Vec<usize> = per.values().copied().collect();
                let zero = counts.iter().filter(|&&c| c == 0).count();
                let min_chapter = counts.iter().copied().filter(|&c| c > 0).min().unwrap_or(0);
                let blank = book.pages.iter().filter(|p| p.placements.is_empty()).count();
                println!(
                    "{id},{pages},{seed},{},{},{zero},{:.3},{min_chapter},{blank},{}",
                    photos.len(),
                    per.len(),
                    gini(&counts),
                    book.dropped
                );
            }
        }
    }
}
```

Check two things before building:
- `Photo` must implement `Clone`. It does, since `generate_and_save` clones it.
- `app_lib::print_spec::PrintSpec::pixajoy` must be `pub`. If the build says something is private, make that item `pub` and name it in the commit message.

- [ ] **Step 7: Run the baseline.** Run `cd src-tauri && cargo run --release --example book_report -- 4 5 11 > /tmp/baseline.csv; cat /tmp/baseline.csv`, then run it again with `--places`. Expected: one row per (project, pages, seed), with no `ERROR` rows.

- [ ] **Step 8: Gate.** Add a `### Baseline (measured <date from `date +%F`>)` subsection at the end of spec §9. Paste both CSVs into it as tables. **If `events_at_zero` is 0 on every row, stop and report to the user before Task 2.** §9 says the spec is worth less than it looks in that case.

- [ ] **Step 9: Commit.**

```bash
git add src-tauri/src/book/events.rs src-tauri/src/book/mod.rs src-tauri/src/commands.rs src-tauri/examples/book_report.rs docs/superpowers/specs/2026-09-23-event-tiers-design.md
git commit -m "feat(report): measure photos per event on saved projects

Baseline before event tiers: <paste the events_at_zero column summary>.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Novelty calibration, and whether "Look" survives

**Files:**
- Modify: `src-tauri/src/book/events.rs`. Add `mean_print`, `print_distance` and `auc`.
- Modify: `src-tauri/examples/book_report.rs`. Add a `--novelty` mode.
- Modify: the spec, §3.1.

**Interfaces:**
- Produces: `events::mean_print(prints: &[&[f32]]) -> Option<Vec<f32>>`. It returns `None` for an empty input or for mismatched lengths.
- Produces: `events::print_distance(a: &[f32], b: &[f32]) -> f64`, the L2 distance.
- Produces: `events::auc(same: &[f64], different: &[f64]) -> f64`. This is the probability that a random same-place distance is smaller than a random different-place distance. Ties count one half.
- Produces a decision recorded in the spec: **LOOK_KEPT** or **LOOK_DROPPED**. If kept, it also records `LOOK_SAME` and `LOOK_DIFFERENT`, the medians of the two groups. Task 4 reads this decision.

- [ ] **Step 1: Write the failing tests** in `events.rs` `mod tests`:

```rust
#[test]
fn mean_print_averages_componentwise_and_refuses_ragged_input() {
    let a = [1.0_f32, 0.0, 2.0];
    let b = [3.0_f32, 2.0, 0.0];
    assert_eq!(mean_print(&[&a, &b]), Some(vec![2.0, 1.0, 1.0]));
    assert_eq!(mean_print(&[]), None);
    assert_eq!(mean_print(&[&a, &[1.0_f32][..]]), None);
}

#[test]
fn print_distance_is_euclidean() {
    assert!((print_distance(&[0.0, 0.0], &[3.0, 4.0]) - 5.0).abs() < 1e-9);
}

#[test]
fn auc_is_one_for_perfect_separation_and_half_for_none() {
    assert_eq!(auc(&[0.1, 0.2], &[0.5, 0.9]), 1.0);
    assert_eq!(auc(&[0.5], &[0.5]), 0.5);
    assert_eq!(auc(&[0.9], &[0.1]), 0.0);
}
```

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cd src-tauri && cargo test --lib book::events`. Expected: compile errors, because the functions are not defined yet.

- [ ] **Step 3: Implement.**

```rust
/// The componentwise mean of several feature prints: an event's "look".
pub fn mean_print(prints: &[&[f32]]) -> Option<Vec<f32>> {
    let first = prints.first()?;
    if prints.iter().any(|p| p.len() != first.len()) {
        return None;
    }
    let n = prints.len() as f32;
    Some((0..first.len()).map(|k| prints.iter().map(|p| p[k]).sum::<f32>() / n).collect())
}

pub fn print_distance(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| ((x - y) as f64).powi(2)).sum::<f64>().sqrt()
}

/// Area under the ROC curve for "a smaller distance means the same place".
pub fn auc(same: &[f64], different: &[f64]) -> f64 {
    if same.is_empty() || different.is_empty() {
        return 0.5;
    }
    let mut wins = 0.0;
    for s in same {
        for d in different {
            wins += if s < d { 1.0 } else if s == d { 0.5 } else { 0.0 };
        }
    }
    wins / (same.len() * different.len()) as f64
}
```

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Add `--novelty` to the report.** When the flag is set, skip the book loop. For each project:
  1. Group the photos by `event_cluster`.
  2. For each event, take the chapter centroid (`chapter::centroids`) and the `mean_print` of its photos that have `feature_print: Some`.
  3. For every pair of events that both have a centroid and a mean print, compute `km = a.km_to(b)` and `d = print_distance`.
  4. Classify each pair as same (`km < 0.5`), different (`km > 20.0`) or ignored.

  Print per project, then pooled:
  - the pair counts;
  - the p10/p50/p90 of `d` for each class;
  - the `auc`.

  Run it with `--places` as well: time-only events rarely sit within 500 m of each other, and place chapters do.

```rust
// inside main, after parsing a `--novelty` flag into `novelty: bool`:
if novelty {
    let (mut same_all, mut diff_all) = (Vec::new(), Vec::new());
    for &id in &projects {
        let photos = load_project_photos(&db, id).expect("load project");
        let photos = if places { with_places(&photos) } else { photos };
        let ids: Vec<u32> = photos.iter().map(|p| p.event_cluster).collect();
        let locs: Vec<_> = photos.iter().map(|p| p.location).collect();
        let centres = app_lib::book::chapter::centroids(&locs, &ids);
        let mut prints: BTreeMap<u32, Vec<&[f32]>> = BTreeMap::new();
        for p in &photos {
            if let Some(fp) = &p.feature_print {
                prints.entry(p.event_cluster).or_default().push(fp.as_slice());
            }
        }
        let events: Vec<(u32, _, Vec<f32>)> = prints
            .iter()
            .filter_map(|(e, ps)| Some((*e, *centres.get(e)?, app_lib::book::events::mean_print(ps)?)))
            .collect();
        let (mut same, mut diff) = (Vec::new(), Vec::new());
        for i in 0..events.len() {
            for j in i + 1..events.len() {
                let km = events[i].1.km_to(events[j].1);
                let d = app_lib::book::events::print_distance(&events[i].2, &events[j].2);
                if km < 0.5 { same.push(d) } else if km > 20.0 { diff.push(d) }
            }
        }
        report_novelty(&format!("project {id}"), &same, &diff);
        same_all.extend(same);
        diff_all.extend(diff);
    }
    report_novelty("pooled", &same_all, &diff_all);
    return;
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() { return f64::NAN; }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn report_novelty(label: &str, same: &[f64], diff: &[f64]) {
    let (mut s, mut d) = (same.to_vec(), diff.to_vec());
    println!(
        "{label}: same n={} p10/50/90={:.3}/{:.3}/{:.3}  different n={} p10/50/90={:.3}/{:.3}/{:.3}  auc={:.3}",
        s.len(), pct(&mut s, 0.1), pct(&mut s, 0.5), pct(&mut s, 0.9),
        d.len(), pct(&mut d, 0.1), pct(&mut d, 0.5), pct(&mut d, 0.9),
        app_lib::book::events::auc(same, diff)
    );
}
```

- [ ] **Step 6: Run it and decide.** Run `cargo run --release --example book_report -- --novelty --places 4 5 11`, then again without `--places`.

  **Decision rule, fixed in advance:**
  - If the pooled `auc ≥ 0.80` and there are at least 10 pairs in each class, the decision is **LOOK_KEPT**. Record `LOOK_SAME` = the same-place p50 and `LOOK_DIFFERENT` = the different-place p50.
  - Otherwise the decision is **LOOK_DROPPED**.

  Do not move the threshold after seeing the number. Write the output and the decision into spec §3.1, replacing the "Risk" paragraph's last sentence with the measured result.

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/book/events.rs src-tauri/examples/book_report.rs docs/superpowers/specs/2026-09-23-event-tiers-design.md
git commit -m "feat(report): calibrate event look similarity against GPS labels

<LOOK_KEPT or LOOK_DROPPED, with pooled auc and pair counts>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Per-project `featured_floor` and `brief_cap` on `BookOptions`

**Files:**
- Modify: `src-tauri/src/book/pace.rs:101-113`.
- Modify: the struct literals `BookOptions { places: … }` at `src-tauri/src/preview.rs:468`, `src-tauri/src/commands.rs:5630`, `src-tauri/src/agent/view.rs:397` and `src-tauri/src/book/pace.rs:2464`. Each becomes `BookOptions { places: …, ..BookOptions::default() }`.
- Modify: `app/types/book.ts:32-48`.
- Modify: `app/composables/useAnalysisJobs.ts:124` (`parseOptions`).
- Test: `pace.rs` `mod tests`; `tests/jobs.test.ts`.

**Interfaces:**
- Produces:
  ```rust
  pub struct BookOptions { pub places: bool, pub featured_floor: u8, pub brief_cap: u8 }
  pub const DEFAULT_FEATURED_FLOOR: u8 = 6;
  pub const DEFAULT_BRIEF_CAP: u8 = 2;
  pub const FEATURED_FLOOR_RANGE: RangeInclusive<u8> = 2..=12;
  pub const BRIEF_CAP_RANGE: RangeInclusive<u8> = 1..=3;
  ```
  The wire keys are `places`, `featuredFloor` and `briefCap`. `BookOptions` stays `Copy`. It has a manual `Default` impl (false, 6, 2), and it deserialises only through `RawBookOptions`.
- Produces: TS `BookOptions { places: boolean; featuredFloor: number; briefCap: number }` and `DEFAULT_BOOK_OPTIONS = { places: false, featuredFloor: 6, briefCap: 2 }`.

- [ ] **Step 1: Write the failing Rust tests** in `pace.rs` tests:

```rust
#[test]
fn book_options_saved_before_tiers_load_with_the_default_floor_and_cap() {
    let o: BookOptions = serde_json::from_str(r#"{"places":true}"#).unwrap();
    assert_eq!(o, BookOptions { places: true, featured_floor: 6, brief_cap: 2 });
}

#[test]
fn book_options_refuse_a_floor_or_cap_outside_their_range() {
    for bad in [r#"{"featuredFloor":1}"#, r#"{"featuredFloor":13}"#, r#"{"briefCap":0}"#, r#"{"briefCap":4}"#] {
        assert!(serde_json::from_str::<BookOptions>(bad).is_err(), "{bad} must be refused");
    }
    let edge: BookOptions = serde_json::from_str(r#"{"featuredFloor":12,"briefCap":3}"#).unwrap();
    assert_eq!((edge.featured_floor, edge.brief_cap), (12, 3));
}

#[test]
fn default_book_options_still_serialise_to_nothing_on_a_book() {
    assert!(BookOptions::default().is_default());
    assert!(!BookOptions { featured_floor: 9, ..BookOptions::default() }.is_default());
}
```

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cd src-tauri && cargo test --lib book::pace::tests::book_options`. Expected: a compile error, because the fields do not exist yet.

- [ ] **Step 3: Implement.** Replace `pace.rs:101-113` with:

```rust
pub const DEFAULT_FEATURED_FLOOR: u8 = 6;
pub const DEFAULT_BRIEF_CAP: u8 = 2;
pub const FEATURED_FLOOR_RANGE: std::ops::RangeInclusive<u8> = 2..=12;
pub const BRIEF_CAP_RANGE: std::ops::RangeInclusive<u8> = 1..=3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "RawBookOptions")]
pub struct BookOptions {
    /// Chapters split where the photos move between towns as well as at a
    /// gap in time. See `book::chapter`.
    pub places: bool,
    /// The fewest photos a Featured event places. Set per project; see
    /// `book::events`.
    pub featured_floor: u8,
    /// The most photos a Brief event places. Set per project.
    pub brief_cap: u8,
}

impl Default for BookOptions {
    fn default() -> Self {
        Self { places: false, featured_floor: DEFAULT_FEATURED_FLOOR, brief_cap: DEFAULT_BRIEF_CAP }
    }
}

/// The wire shape, before validation. A book or draft saved before tiers
/// existed has neither key and gets the defaults.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBookOptions {
    #[serde(default)]
    places: bool,
    #[serde(default = "default_featured_floor")]
    featured_floor: u8,
    #[serde(default = "default_brief_cap")]
    brief_cap: u8,
}

fn default_featured_floor() -> u8 {
    DEFAULT_FEATURED_FLOOR
}

fn default_brief_cap() -> u8 {
    DEFAULT_BRIEF_CAP
}

impl TryFrom<RawBookOptions> for BookOptions {
    type Error = String;
    fn try_from(raw: RawBookOptions) -> Result<Self, String> {
        if !FEATURED_FLOOR_RANGE.contains(&raw.featured_floor) {
            return Err(format!("featuredFloor {} is outside {:?}", raw.featured_floor, FEATURED_FLOOR_RANGE));
        }
        if !BRIEF_CAP_RANGE.contains(&raw.brief_cap) {
            return Err(format!("briefCap {} is outside {:?}", raw.brief_cap, BRIEF_CAP_RANGE));
        }
        Ok(Self { places: raw.places, featured_floor: raw.featured_floor, brief_cap: raw.brief_cap })
    }
}

impl BookOptions {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
```

Then fix the four struct literals listed under **Files**.

- [ ] **Step 4: Run the tests and confirm they pass.** Then run all of `bun run test:rust`. Expected: PASS. The golden in `tests/fixtures/book-20.json` does not move, because the default options still skip serialisation.

- [ ] **Step 5: Write the failing TS test** in `tests/jobs.test.ts`. Keep the existing import of `parseSavedDraft`.

```ts
describe("parseSavedDraft options", () => {
  const base = { id: 1, name: "Trip", folders: ["/a"] };
  it("gives a draft saved before tiers the default floor and cap", () => {
    const draft = parseSavedDraft(JSON.stringify({ ...base, options: { places: true } }));
    expect(draft?.options).toEqual({ places: true, featuredFloor: 6, briefCap: 2 });
  });
  it("replaces an out-of-range floor or cap with its default", () => {
    const draft = parseSavedDraft(JSON.stringify({ ...base, options: { featuredFloor: 40, briefCap: 0 } }));
    expect(draft?.options).toEqual({ places: false, featuredFloor: 6, briefCap: 2 });
  });
});
```

- [ ] **Step 6: Run the TS test and confirm it fails.** Run `bun run test tests/jobs.test.ts`. Expected: FAIL.

- [ ] **Step 7: Implement.** In `app/types/book.ts`:

```ts
export interface BookOptions {
  /** Chapters split where the photos move between towns, not only at a gap in time. */
  places: boolean;
  /** The fewest photos a Featured event places. Rust accepts 2..=12. */
  featuredFloor: number;
  /** The most photos a Brief event places. Rust accepts 1..=3. */
  briefCap: number;
}

export const FEATURED_FLOOR_RANGE = { min: 2, max: 12 } as const;
export const BRIEF_CAP_RANGE = { min: 1, max: 3 } as const;

export const DEFAULT_BOOK_OPTIONS: Readonly<BookOptions> = Object.freeze({
  places: false,
  featuredFloor: 6,
  briefCap: 2,
});
```

In `useAnalysisJobs.ts`, extend the import from `~/types/book` with `BRIEF_CAP_RANGE`, `DEFAULT_BOOK_OPTIONS` and `FEATURED_FLOOR_RANGE`, then replace `parseOptions` with:

```ts
function inRange(value: unknown, range: { min: number; max: number }, fallback: number): number {
  return typeof value === "number" && Number.isInteger(value) && value >= range.min && value <= range.max
    ? value
    : fallback;
}

function parseOptions(value: unknown): BookOptions {
  if (!isRecord(value)) return { ...DEFAULT_BOOK_OPTIONS };
  return {
    places: value.places === true,
    featuredFloor: inRange(value.featuredFloor, FEATURED_FLOOR_RANGE, DEFAULT_BOOK_OPTIONS.featuredFloor),
    briefCap: inRange(value.briefCap, BRIEF_CAP_RANGE, DEFAULT_BOOK_OPTIONS.briefCap),
  };
}
```

Run `grep -rn "places: " app tests` and fix every other `BookOptions` literal the type checker now rejects.

- [ ] **Step 8: Run the checks.** Run `bun run test && bun run typecheck && bun run lint`. Expected: all pass.

- [ ] **Step 9: Mutation-check.**
  - Change `2..=12` to `1..=12`. `book_options_refuse_a_floor_or_cap_outside_their_range` must go red.
  - Change `default_featured_floor` to return 5. `book_options_saved_before_tiers…` must go red.
  - Restore both and paste the evidence into the commit body.

- [ ] **Step 10: Commit.**

```bash
git add src-tauri/src/book/pace.rs src-tauri/src/preview.rs src-tauri/src/commands.rs src-tauri/src/agent/view.rs app/types/book.ts app/composables/useAnalysisJobs.ts tests/jobs.test.ts
git commit -m "feat(options): per-project Featured floor and Brief cap

<mutation evidence>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

(This commit has no manual entry. The steppers that expose these settings arrive in Task 11 together with their manual entry.)

---

### Task 4: `Tier`, `EventTiers` and resolution

**Files:**
- Modify: `src-tauri/src/book/events.rs`.
- Create: `tests/fixtures/wire/event-tiers.json`.
- Modify: `app/types/features.ts`.
- Test: `events.rs` tests; the wire test in `src-tauri/src/commands.rs` next to the `photo-overrides.json` test at :6201; `tests/book.test.ts`.

**Interfaces:**
- Produces (Rust, in `book::events`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum Tier { Skipped, Brief, Normal, Featured }   // declaration order IS rank: Ord gives "higher tier"
  impl Tier {
      pub fn as_str(self) -> &'static str;
      pub fn from_token(token: &str) -> Option<Tier>;
      pub fn weight(self) -> f64;                               // 2.0 / 1.0 / 0.5 / 0.0
      pub fn floor(self, options: &BookOptions) -> usize;       // featured_floor / 2 / 1 / 0
  }
  pub struct EventTiers(BTreeMap<String, Tier>);   // #[serde(transparent)], Default, Clone, PartialEq
  impl EventTiers {
      pub fn new() -> Self;
      pub fn get(&self, hash: &str) -> Option<Tier>;
      pub fn set(&mut self, hash: impl Into<String>, tier: Option<Tier>);  // None removes
      pub fn iter(&self) -> impl Iterator<Item = (&str, Tier)>;
      pub fn is_empty(&self) -> bool;
  }
  impl FromIterator<(String, Tier)> for EventTiers;
  pub fn resolve<'a>(hashes: impl IntoIterator<Item = &'a str>, tiers: &EventTiers) -> Option<Tier>;
  ```
- Produces (TS, in `app/types/features.ts`):
  ```ts
  export type Tier = "featured" | "normal" | "brief" | "skipped";
  export type EventTiers = Record<string, Tier>;
  export const TIERS: readonly Tier[];
  export function withEventTier(tiers: EventTiers, hashes: readonly string[], tier: Tier | "auto"): EventTiers;
  ```

- [ ] **Step 1: Write the failing Rust tests** in `events.rs` tests:

```rust
use crate::book::pace::BookOptions;

#[test]
fn tier_resolution_follows_the_majority_of_photos() {
    let mut tiers = EventTiers::new();
    for h in ["a", "b", "c", "d", "e", "f", "g"] {
        tiers.set(h, Some(Tier::Brief));
    }
    for h in ["h", "i", "j"] {
        tiers.set(h, Some(Tier::Featured));
    }
    let event = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
    assert_eq!(resolve(event.iter().copied(), &tiers), Some(Tier::Brief));
    // The FIRST photo is Featured here; a first-photo rule would answer Featured.
    let reordered = ["h", "a", "b", "c", "d", "e", "f", "g", "i", "j"];
    assert_eq!(resolve(reordered.iter().copied(), &tiers), Some(Tier::Brief));
}

#[test]
fn tier_resolution_ties_go_to_the_higher_tier_and_unset_is_none() {
    let tiers: EventTiers =
        [("a".to_string(), Tier::Skipped), ("b".to_string(), Tier::Normal)].into_iter().collect();
    assert_eq!(resolve(["a", "b"], &tiers), Some(Tier::Normal));
    assert_eq!(resolve(["x", "y"], &tiers), None);
}

#[test]
fn tier_floors_and_weights_follow_the_spec() {
    let o = BookOptions { featured_floor: 9, ..BookOptions::default() };
    assert_eq!(
        [Tier::Featured, Tier::Normal, Tier::Brief, Tier::Skipped].map(|t| t.floor(&o)),
        [9, 2, 1, 0]
    );
    assert_eq!(
        [Tier::Featured, Tier::Normal, Tier::Brief, Tier::Skipped].map(Tier::weight),
        [2.0, 1.0, 0.5, 0.0]
    );
}

#[test]
fn event_tiers_set_none_removes_the_choice() {
    let mut t = EventTiers::new();
    t.set("a", Some(Tier::Featured));
    t.set("a", None);
    assert!(t.is_empty());
}
```

- [ ] **Step 2: Run the tests and confirm they fail.** Expected: compile errors.

- [ ] **Step 3: Implement** at the top of `events.rs`, below the module doc:

```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::book::pace::BookOptions;

pub const FEATURED_WEIGHT: f64 = 2.0;
pub const NORMAL_WEIGHT: f64 = 1.0;
pub const BRIEF_WEIGHT: f64 = 0.5;
pub const NORMAL_FLOOR: usize = 2;
const BRIEF_FLOOR: usize = 1;

/// How much of the book an event gets. Declared lowest first, so the derived
/// `Ord` reads "higher tier" and a tie in `resolve` can take the max.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Skipped,
    Brief,
    Normal,
    Featured,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Brief => "brief",
            Self::Normal => "normal",
            Self::Featured => "featured",
        }
    }

    /// `None` for an unknown token: a stored choice this build cannot honour
    /// fails the load rather than silently becoming the suggestion.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "skipped" => Some(Self::Skipped),
            "brief" => Some(Self::Brief),
            "normal" => Some(Self::Normal),
            "featured" => Some(Self::Featured),
            _ => None,
        }
    }

    pub fn weight(self) -> f64 {
        match self {
            Self::Featured => FEATURED_WEIGHT,
            Self::Normal => NORMAL_WEIGHT,
            Self::Brief => BRIEF_WEIGHT,
            Self::Skipped => 0.0,
        }
    }

    pub fn floor(self, options: &BookOptions) -> usize {
        match self {
            Self::Featured => options.featured_floor as usize,
            Self::Normal => NORMAL_FLOOR,
            Self::Brief => BRIEF_FLOOR,
            Self::Skipped => 0,
        }
    }
}

/// The user's tier choices, keyed by photo content hash (§6). Absent means
/// the event takes its suggestion; there is no stored "auto", the same rule
/// as `cull::Overrides`. Wire shape `{"<hash>": "featured"}`, pinned in
/// `tests/fixtures/wire/event-tiers.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventTiers(BTreeMap<String, Tier>);

impl EventTiers {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn get(&self, hash: &str) -> Option<Tier> {
        self.0.get(hash).copied()
    }

    pub fn set(&mut self, hash: impl Into<String>, tier: Option<Tier>) {
        let hash = hash.into();
        match tier {
            Some(t) => {
                self.0.insert(hash, t);
            }
            None => {
                self.0.remove(&hash);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, Tier)> {
        self.0.iter().map(|(h, &t)| (h.as_str(), t))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, Tier)> for EventTiers {
    fn from_iter<I: IntoIterator<Item = (String, Tier)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// An event's chosen tier: the one held by the most of its photos, ties to
/// the higher tier; `None` when none of its photos carries a choice (§6).
pub fn resolve<'a>(hashes: impl IntoIterator<Item = &'a str>, tiers: &EventTiers) -> Option<Tier> {
    let mut votes: BTreeMap<Tier, usize> = BTreeMap::new();
    for h in hashes {
        if let Some(t) = tiers.get(h) {
            *votes.entry(t).or_default() += 1;
        }
    }
    votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0))).map(|(t, _)| t)
}
```

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Pin the wire shape.** Create `tests/fixtures/wire/event-tiers.json`:

```json
{ "h-featured": "featured", "h-normal": "normal", "h-brief": "brief", "h-skipped": "skipped" }
```

Add a Rust test next to the `photo-overrides.json` test (commands.rs:6201), using the same `wire_fixture` helper:

```rust
#[test]
fn event_tiers_match_the_wire_fixture() {
    let fixture = wire_fixture("event-tiers.json");
    let parsed: crate::book::events::EventTiers = serde_json::from_value(fixture.clone()).unwrap();
    assert_eq!(parsed.get("h-featured"), Some(crate::book::events::Tier::Featured));
    assert_eq!(serde_json::to_value(&parsed).unwrap(), fixture);
    assert!(serde_json::from_str::<crate::book::events::EventTiers>(r#"{"h":"auto"}"#).is_err());
}
```

Add a TS test in `tests/book.test.ts`, using the file's existing `fixture` helper:

```ts
import { TIERS, withEventTier, type EventTiers } from "~/types/features";

describe("event tiers", () => {
  it("matches the wire fixture's tokens", () => {
    const tiers = fixture<EventTiers>("event-tiers.json");
    expect(Object.values(tiers).sort()).toEqual([...TIERS].sort());
  });
  it("sets and clears a tier on every photo of an event", () => {
    const set = withEventTier({ z: "brief" }, ["a", "b"], "featured");
    expect(set).toEqual({ z: "brief", a: "featured", b: "featured" });
    expect(withEventTier(set, ["a", "b"], "auto")).toEqual({ z: "brief" });
  });
});
```

- [ ] **Step 6: Implement TS.** Add to `app/types/features.ts`:

```ts
/** Mirrors `book::events::Tier`. "Auto" is the absence of a key, never a token. */
export type Tier = "featured" | "normal" | "brief" | "skipped";
export const TIERS: readonly Tier[] = ["featured", "normal", "brief", "skipped"];
/** Mirrors `book::events::EventTiers`: hash -> the user's tier for that photo's event. */
export type EventTiers = Record<string, Tier>;

/** The map with every one of an event's photos set to `tier` (or cleared), as a NEW object. */
export function withEventTier(tiers: EventTiers, hashes: readonly string[], tier: Tier | "auto"): EventTiers {
  const next = { ...tiers };
  for (const hash of hashes) {
    if (tier === "auto") delete next[hash];
    else next[hash] = tier;
  }
  return next;
}
```

- [ ] **Step 7: Run the checks.** Run `bun run test && bun run test:rust && bun run lint`. Expected: all pass.

- [ ] **Step 8: Mutation-check.** Replace `resolve`'s body with "the first photo that has a tier". `tier_resolution_follows_the_majority_of_photos` must go red. Restore it and paste the evidence.

- [ ] **Step 9: Commit.**

```bash
git add src-tauri/src/book/events.rs src-tauri/src/commands.rs tests/fixtures/wire/event-tiers.json app/types/features.ts tests/book.test.ts
git commit -m "feat(events): tier type, hash-keyed choices and majority resolution

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Event statistics, merit, novelty and `suggest`

**Files:**
- Modify: `src-tauri/src/book/events.rs`.
- Modify: `src-tauri/src/book/pack.rs`, if `moments` is not `pub`. It is `pub` at :506, so no change is expected.

**Interfaces:**
- Consumes: `pack::moments(&[Photo]) -> Vec<u32>`; `chapter::centroids(&[Option<LatLon>], &[u32]) -> BTreeMap<u32, LatLon>`; `LatLon::km_to`; `Capacity { spreads, singles, target_photos, .. }`; `Tier` (Task 4); `mean_print`/`print_distance` (Task 2); the LOOK decision (Task 2).
- Produces:
  ```rust
  pub struct EventStats { pub event: u32, pub photos: usize, pub keepers: usize, pub moments: usize,
      pub quality: f64, pub utility: f64, pub undated: bool, pub centroid: Option<LatLon>,
      pub tags: BTreeSet<String>, pub print: Option<Vec<f32>> }
  pub fn stats(photos: &[Photo], kept: &[Photo]) -> Vec<EventStats>;   // sorted by event
  #[serde(tag = "kind", rename_all = "camelCase")]
  pub enum Reason { Utility { share: f64 }, NothingKept, Undated, Ranked { rank: usize, of: usize },
      Standout, OutOfRoom { rank: usize, of: usize }, SimilarTo { event: u32 }, Demoted, Filled }
  pub struct Suggestion { pub event: u32, pub tier: Tier, pub reason: Reason, pub merit: f64 }
  pub fn normal_room(eligible: usize, capacity: &Capacity) -> usize;
  pub fn suggest(stats: &[EventStats], capacity: &Capacity) -> Vec<Suggestion>;  // sorted by event
  ```
  `Reason` derives `Debug, Clone, Copy, PartialEq, Serialize`. `Demoted` and `Filled` are set only by `budget` (Task 6).

**Test fixture helper.** Put this in `events.rs` `mod tests`. It builds a real-shaped `Photo`: several moments per event, aesthetic that varies, and distinct near-dup clusters. Check the fields against `cull.rs:30` and add any that are missing.

```rust
use crate::book::chapter::LatLon;
use crate::book::cull::Photo;
use crate::book::pack::Capacity;

/// Photo `n` of event `event`: moment `n / 2` (two frames a moment, 60 s
/// apart; moments 10 min apart), aesthetic `aes` minus a small per-photo
/// spread so percentiles genuinely differ.
fn shot(event: u32, day: i64, n: usize, aes: u8) -> Photo {
    Photo {
        path: format!("/e{event}/p{n:03}.jpg"),
        hash: format!("e{event}-{n}"),
        width: 4000,
        height: 3000,
        is_utility: false,
        aesthetic_pct: aes.saturating_sub((n % 7) as u8),
        sharpness_pct: 50,
        near_dup_cluster: event * 10_000 + n as u32,
        event_cluster: event,
        faces: Vec::new(),
        face_area_fraction: 0.0,
        saliency_box: None,
        palette: Vec::new(),
        capture_quality: None,
        scene_tags: Vec::new(),
        captured_at: Some(day * 86_400 + (n / 2) as i64 * 600 + (n % 2) as i64 * 60),
        clipped_low: 0.0,
        clipped_high: 0.0,
        feature_print: None,
        location: None,
    }
}

fn event(event: u32, day: i64, count: usize, aes: u8) -> Vec<Photo> {
    (0..count).map(|n| shot(event, day, n, aes)).collect()
}

fn located(mut photos: Vec<Photo>, lat: f64, lon: f64, tags: &[&str]) -> Vec<Photo> {
    for p in &mut photos {
        p.location = LatLon::new(lat, lon);
        p.scene_tags = tags.iter().map(|t| t.to_string()).collect();
    }
    photos
}

fn capacity(spreads: u32, target: usize) -> Capacity {
    Capacity { pages: spreads * 2 + 2, singles: 2, spreads, max_photos: target * 2, target_photos: target }
}
```

The tests treat `kept` as the photos with `is_utility == false`. That matches `cull` with near-dup clusters of one, and it keeps the tests independent of `cull`.

- [ ] **Step 1: Write the failing tests.**

```rust
fn keepers(photos: &[Photo]) -> Vec<Photo> {
    photos.iter().filter(|p| !p.is_utility).cloned().collect()
}

#[test]
fn stats_count_moments_among_keepers_and_utility_over_all_photos() {
    let mut photos = event(0, 0, 10, 80); // 5 moments
    for p in photos.iter_mut().take(4) {
        p.is_utility = true;
    }
    let s = stats(&photos, &keepers(&photos));
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].photos, s[0].keepers, s[0].moments), (10, 6, 3));
    assert!((s[0].utility - 0.4).abs() < 1e-9);
    assert!(!s[0].undated);
}

#[test]
fn a_mostly_screenshot_event_is_suggested_skipped() {
    let mut shots = event(1, 1, 40, 70);
    for p in shots.iter_mut().take(36) {
        p.is_utility = true;
    }
    let mut photos = event(0, 0, 30, 80);
    photos.extend(shots);
    photos.extend(event(2, 2, 30, 80));
    let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
    assert_eq!(s[1].tier, Tier::Skipped);
    assert!(matches!(s[1].reason, Reason::Utility { share } if share > 0.8));
}

#[test]
fn an_undated_event_is_suggested_brief() {
    let mut undated = event(1, 0, 30, 90);
    for p in &mut undated {
        p.captured_at = None;
    }
    let mut photos = event(0, 0, 20, 60);
    photos.extend(undated);
    let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
    assert_eq!((s[1].tier, s[1].reason), (Tier::Brief, Reason::Undated));
}

#[test]
fn a_library_with_no_dates_at_all_is_ranked_not_all_brief() {
    // Scanned prints, or the test fixtures: nothing is dated, so "undated"
    // says nothing about one event against another.
    let mut photos = event(0, 0, 20, 60);
    photos.extend(event(1, 1, 20, 80));
    for p in &mut photos {
        p.captured_at = None;
    }
    let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
    assert!(s.iter().all(|x| x.reason != Reason::Undated), "{s:?}");
    assert!(s.iter().all(|x| x.tier >= Tier::Normal), "{s:?}");
}

#[test]
fn normal_room_is_sixty_percent_of_the_slots() {
    assert_eq!(normal_room(50, &capacity(9, 45)), 6); // 11 slots
    assert_eq!(normal_room(50, &capacity(19, 85)), 12); // 21 slots
    assert_eq!(normal_room(3, &capacity(19, 85)), 3);
}

#[test]
fn events_beyond_the_room_are_brief_and_a_standout_is_featured() {
    // 9 ordinary events and one with 3x the moments and top aesthetics, 11 slots -> room 6.
    let mut photos = Vec::new();
    for e in 0..9 {
        photos.extend(located(event(e, e as i64, 12, 40 + e as u8), 10.0 + e as f64, 100.0, &["tag"]));
    }
    photos.extend(located(event(9, 9, 36, 95), 50.0, 50.0, &["summit"]));
    let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
    let count = |t: Tier| s.iter().filter(|x| x.tier == t).count();
    assert_eq!(s[9].tier, Tier::Featured, "{:?}", s[9]);
    assert_eq!(s[9].reason, Reason::Standout);
    assert_eq!(count(Tier::Featured) + count(Tier::Normal), 6);
    assert_eq!(count(Tier::Brief), 4);
    assert!(s.iter().filter(|x| x.tier == Tier::Brief).all(|x| matches!(
        x.reason,
        Reason::OutOfRoom { .. } | Reason::SimilarTo { .. }
    )));
}

#[test]
fn of_two_look_alike_events_the_lower_is_brief_when_room_is_tight() {
    // Two pool afternoons 300 m apart with the same tags, plus distinct
    // events enough to make room tight (room = floor(3 * 0.6) = 1 with 1 spread + 2 singles).
    let pool = |e: u32, day: i64, aes: u8| located(event(e, day, 12, aes), 8.0, 115.0, &["pool", "water"]);
    let mut photos = pool(0, 0, 80);
    photos.extend(pool(1, 2, 78));
    photos.extend(located(event(2, 1, 8, 70), 8.5, 115.5, &["temple"]));
    let mut pool_1 = photos[12..24].to_vec();
    for p in &mut pool_1 {
        p.location = LatLon::new(8.0027, 115.0); // ~300 m north
    }
    photos.splice(12..24, pool_1);
    // 2 spreads + 2 singles = 4 slots -> room floor(2.4) = 2.
    // Merits (engagement = ln(1+m)/ln(1+6); quality = top-3 aesthetic / 100):
    //   pool 0:  0.4*1.000 + 0.4*0.793 + 0.2*1 (first, nothing above) = 0.917
    //   temple:  0.4*0.827 + 0.4*0.693 + 0.2*1 (78 km, no shared tag) = 0.808
    //   pool 1:  0.4*1.000 + 0.4*0.773 + 0.2*0 (300 m, same tags)     = 0.709
    // Without novelty pool 1 scores 0.909 and takes the second place from the temple.
    let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(2, 16));
    assert_eq!(s[1].tier, Tier::Brief, "{s:?}");
    assert_eq!(s[1].reason, Reason::SimilarTo { event: 0 });
    assert_ne!(s[2].tier, Tier::Brief, "the temple takes the room novelty freed");
}
```

The comment's numbers assume the moment rule in `pack::moments` puts two frames 60 s apart in one moment and splits frames 600 s apart. Before relying on those numbers, print `stats(..)` once and confirm `moments` is 6 / 4 / 6. If it is not, fix the fixture's timing, not the constants. Step 6 confirms the test goes red with novelty removed.

- [ ] **Step 2: Run the tests and confirm they fail.** Expected: compile errors.

- [ ] **Step 3: Implement.** Add to `events.rs`:

```rust
use std::collections::BTreeSet;

use crate::book::chapter::{self, LatLon};
use crate::book::cull::Photo;
use crate::book::pack::{self, Capacity};

pub const W_ENGAGEMENT: f64 = 0.4;
pub const W_QUALITY: f64 = 0.4;
pub const W_NOVELTY: f64 = 0.2;
pub const UTILITY_SKIP: f64 = 0.8;
pub const FEATURED_MARGIN: f64 = 1.3;
pub const NORMAL_SLOT_SHARE: f64 = 0.6;
pub const NOVELTY_KM: f64 = 2.0;
pub const SIMILAR_BELOW: f64 = 0.3;
const QUALITY_TOP: usize = 3;
const TOP_TAGS: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct EventStats {
    pub event: u32,
    pub photos: usize,
    pub keepers: usize,
    /// Distinct moments among the keepers (`pack::moments`).
    pub moments: usize,
    /// Mean `aesthetic_pct` of the best `QUALITY_TOP` keepers, / 100.
    pub quality: f64,
    /// Share of ALL the event's photos flagged `is_utility`.
    pub utility: f64,
    /// No photo in the event has a capture time.
    pub undated: bool,
    pub centroid: Option<LatLon>,
    /// The `TOP_TAGS` most frequent scene tags among the keepers.
    pub tags: BTreeSet<String>,
    /// Mean feature print of the keepers that have one.
    pub print: Option<Vec<f32>>,
}

/// Per-event figures, sorted by event id. `kept` is `cull`'s output for
/// `photos`; an event whose photos were all culled has `keepers == 0`.
pub fn stats(photos: &[Photo], kept: &[Photo]) -> Vec<EventStats> {
    let ids: Vec<u32> = photos.iter().map(|p| p.event_cluster).collect();
    let locations: Vec<Option<LatLon>> = photos.iter().map(|p| p.location).collect();
    let centres = chapter::centroids(&locations, &ids);

    let mut out: BTreeMap<u32, EventStats> = BTreeMap::new();
    for p in photos {
        let s = out.entry(p.event_cluster).or_insert_with(|| EventStats {
            event: p.event_cluster,
            photos: 0,
            keepers: 0,
            moments: 0,
            quality: 0.0,
            utility: 0.0,
            undated: true,
            centroid: centres.get(&p.event_cluster).copied(),
            tags: BTreeSet::new(),
            print: None,
        });
        s.photos += 1;
        if p.is_utility {
            s.utility += 1.0;
        }
        if p.captured_at.is_some() {
            s.undated = false;
        }
    }

    let moment = pack::moments(kept);
    let mut members: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, p) in kept.iter().enumerate() {
        members.entry(p.event_cluster).or_default().push(i);
    }
    for (event, idx) in members {
        let Some(s) = out.get_mut(&event) else { continue };
        s.keepers = idx.len();
        s.moments = idx.iter().map(|&i| moment[i]).collect::<BTreeSet<_>>().len();
        let mut aes: Vec<u8> = idx.iter().map(|&i| kept[i].aesthetic_pct).collect();
        aes.sort_unstable_by(|a, b| b.cmp(a));
        let top = &aes[..aes.len().min(QUALITY_TOP)];
        s.quality = top.iter().map(|&a| a as f64).sum::<f64>() / top.len() as f64 / 100.0;
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for &i in &idx {
            for t in &kept[i].scene_tags {
                *counts.entry(t.as_str()).or_default() += 1;
            }
        }
        let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        s.tags = ranked.into_iter().take(TOP_TAGS).map(|(t, _)| t.to_string()).collect();
        let prints: Vec<&[f32]> = idx.iter().filter_map(|&i| kept[i].feature_print.as_deref()).collect();
        s.print = mean_print(&prints);
    }
    for s in out.values_mut() {
        s.utility /= s.photos as f64;
    }
    out.into_values().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reason {
    Utility { share: f64 },
    NothingKept,
    Undated,
    Ranked { rank: usize, of: usize },
    Standout,
    OutOfRoom { rank: usize, of: usize },
    SimilarTo { event: u32 },
    /// `budget` lowered a suggested tier so the floors fit the book (§5 step 3).
    Demoted,
    /// `budget` raised a suggested Brief so the book does not underfill.
    Filled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion {
    pub event: u32,
    pub tier: Tier,
    pub reason: Reason,
    pub merit: f64,
}

/// How many events the page length can give a chapter of their own (§4).
pub fn normal_room(eligible: usize, capacity: &Capacity) -> usize {
    let slots = capacity.spreads as usize + capacity.singles as usize;
    eligible.min((slots as f64 * NORMAL_SLOT_SHARE).floor() as usize)
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// How alike two events are, 0..1: the highest of place, scene and look (§3.1).
fn similarity(a: &EventStats, b: &EventStats) -> f64 {
    let gps = match (a.centroid, b.centroid) {
        (Some(x), Some(y)) => 1.0 - (x.km_to(y) / NOVELTY_KM).min(1.0),
        _ => 0.0,
    };
    gps.max(jaccard(&a.tags, &b.tags)).max(look_similarity(a, b))
}

/// LOOK_DROPPED by Task 2: always 0. If Task 2 recorded LOOK_KEPT, replace
/// with `1 - clamp((d - LOOK_SAME) / (LOOK_DIFFERENT - LOOK_SAME), 0, 1)`
/// over `print_distance` of the two mean prints, and add a test.
fn look_similarity(_a: &EventStats, _b: &EventStats) -> f64 {
    0.0
}

pub fn suggest(stats: &[EventStats], capacity: &Capacity) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let mut eligible: Vec<&EventStats> = Vec::new();
    // "Undated" only means something beside dated events; in a library with
    // no dates at all it would make every event Brief.
    let any_dated = stats.iter().any(|s| !s.undated);
    for s in stats {
        let fixed = |tier, reason| Suggestion { event: s.event, tier, reason, merit: 0.0 };
        if s.utility > UTILITY_SKIP {
            out.push(fixed(Tier::Skipped, Reason::Utility { share: s.utility }));
        } else if s.keepers == 0 {
            out.push(fixed(Tier::Skipped, Reason::NothingKept));
        } else if s.undated && any_dated {
            out.push(fixed(Tier::Brief, Reason::Undated));
        } else {
            eligible.push(s);
        }
    }

    let most = eligible.iter().map(|s| s.moments).max().unwrap_or(0);
    let engagement = |s: &EventStats| {
        if most == 0 { 0.0 } else { (1.0 + s.moments as f64).ln() / (1.0 + most as f64).ln() }
    };
    let base = |s: &EventStats| (W_ENGAGEMENT * engagement(s) + W_QUALITY * s.quality) * (1.0 - s.utility);

    // Walk in base-merit order; each event's novelty is against those above it.
    let mut walk = eligible.clone();
    walk.sort_by(|a, b| base(b).total_cmp(&base(a)).then(a.event.cmp(&b.event)));
    struct Scored<'a> { s: &'a EventStats, merit: f64, novelty: f64, nearest: Option<u32>, base_rank: usize }
    let mut scored: Vec<Scored> = walk
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let (sim, nearest) = walk[..k]
                .iter()
                .map(|o| (similarity(s, o), Some(o.event)))
                .fold((0.0, None), |best, cur| if cur.0 > best.0 { cur } else { best });
            let novelty = 1.0 - sim;
            let merit = (W_ENGAGEMENT * engagement(s) + W_QUALITY * s.quality + W_NOVELTY * novelty)
                * (1.0 - s.utility);
            Scored { s, merit, novelty, nearest, base_rank: k }
        })
        .collect();
    scored.sort_by(|a, b| b.merit.total_cmp(&a.merit).then(a.s.event.cmp(&b.s.event)));

    let of = scored.len();
    let room = normal_room(of, capacity);
    let mut top: Vec<f64> = scored[..room].iter().map(|x| x.merit).collect();
    top.sort_by(f64::total_cmp);
    let median = if top.is_empty() {
        0.0
    } else if top.len() % 2 == 1 {
        top[top.len() / 2]
    } else {
        (top[top.len() / 2 - 1] + top[top.len() / 2]) / 2.0
    };
    let featured_limit = (room / 6).max(1);
    let mut featured = 0;
    for (rank, x) in scored.iter().enumerate() {
        let (tier, reason) = if rank < room {
            if featured < featured_limit && x.merit >= FEATURED_MARGIN * median {
                featured += 1;
                (Tier::Featured, Reason::Standout)
            } else {
                (Tier::Normal, Reason::Ranked { rank: rank + 1, of })
            }
        } else if x.novelty < SIMILAR_BELOW && x.base_rank < room {
            (Tier::Brief, Reason::SimilarTo { event: x.nearest.unwrap_or(x.s.event) })
        } else {
            (Tier::Brief, Reason::OutOfRoom { rank: rank + 1, of })
        };
        out.push(Suggestion { event: x.s.event, tier, reason, merit: x.merit });
    }
    out.sort_by_key(|s| s.event);
    out
}
```

- [ ] **Step 4: Run the tests and confirm they pass.** Run `cd src-tauri && cargo test --lib book::events`. If a fixture assertion fails, fix the **fixture's arithmetic** (per the note in Step 1), never the constants. The constants are fixed by the spec until Task 12.

- [ ] **Step 5: Run clippy on the file.** Run `cd src-tauri && cargo clippy --all-targets -- -D warnings`. Expected: clean.

- [ ] **Step 6: Mutation-check** each of the following, one at a time. Each must turn the named test red.

| Mutant | Must go red |
|---|---|
| Delete the `s.utility > UTILITY_SKIP` branch | `a_mostly_screenshot_event_is_suggested_skipped` |
| `&& any_dated` removed | `a_library_with_no_dates_at_all_is_ranked_not_all_brief` |
| `novelty = 1.0` always | `of_two_look_alike_events_the_lower_is_brief_when_room_is_tight` |
| `featured_limit = 0` | `events_beyond_the_room_are_brief_and_a_standout_is_featured` |
| `NORMAL_SLOT_SHARE = 1.0` | `normal_room_is_sixty_percent_of_the_slots` |

- [ ] **Step 7: Commit** with the mutation evidence in the body.

```bash
git add src-tauri/src/book/events.rs
git commit -m "feat(events): per-event merit, novelty and suggested tiers

<mutation evidence>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Per-event ranking and `budget`

**Files:**
- Modify: `src-tauri/src/book/pack.rs:537-583`. Split `select`'s ranking into `ranked_auto` and add `event_order`. `select` keeps its exact behaviour.
- Modify: `src-tauri/src/book/events.rs`. Add `EventPlan`, `plan`, `TierOverflow`, `Budget` and `budget`.

**Interfaces:**
- Produces, in `pack`:
  ```rust
  pub struct EventOrder { pub list: Vec<usize>, pub included: usize }
  /// Per event: its Includes, then every moment's best, then every moment's second
  /// (at most MAX_PER_MOMENT per moment) -- today's `select` order restricted to the event.
  pub fn event_order(photos: &[Photo], overrides: &Overrides) -> BTreeMap<u32, EventOrder>;
  ```
- Produces, in `events`:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
  pub struct EventPlan { pub event: u32, pub tier: Tier, pub suggested: Tier, pub chosen: bool,
      pub reason: Reason, pub merit: f64, pub moments: usize, pub kept: usize, pub photos: usize }
  pub fn plan(photos: &[Photo], kept: &[Photo], tiers: &EventTiers, capacity: &Capacity) -> Vec<EventPlan>;
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "camelCase")]
  pub struct TierOverflow { pub needed: usize, pub capacity: usize }
  pub struct Budget { pub selected: Vec<usize>, pub plan: Vec<EventPlan>,
      pub floors: BTreeMap<u32, usize>, pub overflow: Option<TierOverflow> }
  impl Budget { pub fn brief_events(&self) -> BTreeSet<u32>; pub fn selected_in(&self, kept: &[Photo], event: u32) -> usize; }
  pub fn budget(kept: &[Photo], plan: Vec<EventPlan>, capacity: &Capacity,
      overrides: &Overrides, options: &BookOptions) -> Budget;
  ```
  `selected` holds sorted indices into `kept`.

- [ ] **Step 1: Write the failing `event_order` test** in `pack.rs` tests. Use that module's existing photo helper. If its signature differs, adapt the calls and keep the assertions.

```rust
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
```

- [ ] **Step 2: Run the test and confirm it fails.** Expected: a compile error, because `event_order` does not exist.

- [ ] **Step 3: Implement** in `pack.rs`, replacing `select`'s body:

```rust
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
}

/// Each event's photos ranked ONCE (spec §5 step 5): its Includes, then
/// `ranked_auto` restricted to the event. A quota takes a prefix, so more
/// room only ever adds photos to an event, never swaps them.
pub fn event_order(photos: &[Photo], overrides: &Overrides) -> std::collections::BTreeMap<u32, EventOrder> {
    let mut out: std::collections::BTreeMap<u32, EventOrder> = std::collections::BTreeMap::new();
    for (i, p) in photos.iter().enumerate() {
        let entry = out.entry(p.event_cluster).or_insert_with(|| EventOrder { list: Vec::new(), included: 0 });
        if overrides.get(&p.hash) == Override::Include {
            entry.list.push(i);
            entry.included += 1;
        }
    }
    for i in ranked_auto(photos, overrides) {
        if let Some(entry) = out.get_mut(&photos[i].event_cluster) {
            entry.list.push(i);
        }
    }
    out
}
```

- [ ] **Step 4: Run all of `pack`'s tests.** Run `cargo test --lib book::pack`. Expected: every existing `select` test still passes, and the new test passes.

- [ ] **Step 5: Write the failing `budget` tests** in `events.rs` tests. Reuse the Task 5 helpers, and add this one:

```rust
use crate::book::cull::{Override, Overrides};

fn plan_of(photos: &[Photo], tiers: &EventTiers, cap: &Capacity) -> Vec<EventPlan> {
    plan(photos, &keepers(photos), tiers, cap)
}

fn placed(b: &Budget, kept: &[Photo], event: u32) -> usize {
    b.selected_in(kept, event)
}

#[test]
fn budget_gives_a_small_dull_normal_event_its_floor_beside_a_huge_bright_one() {
    let mut photos = event(0, 0, 400, 95);
    photos.extend(event(1, 1, 6, 20));
    let kept = keepers(&photos);
    let tiers: EventTiers = photos[400..].iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
    let cap = capacity(9, 45);
    let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
    assert!(placed(&b, &kept, 1) >= 2, "got {}", placed(&b, &kept, 1));
    assert_eq!(b.selected.len(), 45);
}

#[test]
fn the_remainder_vote_uses_the_square_root_of_moments() {
    // 400 moments against 25: sqrt gives a 4:1 vote, a linear vote 16:1.
    let mut photos = event(0, 0, 800, 70);
    photos.extend(event(1, 1, 50, 70));
    let kept = keepers(&photos);
    let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
    let cap = capacity(19, 85);
    let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
    let (big, small) = (placed(&b, &kept, 0), placed(&b, &kept, 1));
    assert!(big <= 5 * small, "big {big} small {small}: more than 5x means the vote is not sqrt");
}

#[test]
fn a_skipped_event_places_nothing_but_its_includes() {
    let mut photos = event(0, 0, 30, 60);
    photos.extend(event(1, 1, 30, 99));
    let kept = keepers(&photos);
    let tiers: EventTiers = photos[30..].iter().map(|p| (p.hash.clone(), Tier::Skipped)).collect();
    let cap = capacity(9, 45);
    let none = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
    assert_eq!(placed(&none, &kept, 1), 0);
    let mut overrides = Overrides::new();
    overrides.set(photos[40].hash.clone(), Override::Include);
    let one = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &overrides, &BookOptions::default());
    let idx = kept.iter().position(|p| p.hash == photos[40].hash).unwrap();
    assert!(one.selected.contains(&idx), "the Include in a Skipped event is selected");
    assert_eq!(placed(&one, &kept, 1), 1);
}

#[test]
fn a_brief_event_stops_at_the_brief_cap_with_room_to_spare() {
    let mut photos = event(0, 0, 8, 60);
    photos.extend(event(1, 1, 20, 99)); // 10 moments
    let kept = keepers(&photos);
    let mut tiers: EventTiers = photos[8..].iter().map(|p| (p.hash.clone(), Tier::Brief)).collect();
    for p in &photos[..8] {
        tiers.set(p.hash.clone(), Some(Tier::Normal));
    }
    let cap = capacity(19, 85);
    for brief_cap in 1..=3u8 {
        let o = BookOptions { brief_cap, ..BookOptions::default() };
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &o);
        assert_eq!(placed(&b, &kept, 1), brief_cap as usize);
    }
}

#[test]
fn a_featured_event_gets_the_projects_featured_floor() {
    let mut photos = event(0, 0, 200, 95);
    photos.extend(event(1, 1, 30, 10));
    let kept = keepers(&photos);
    let tiers: EventTiers = photos[200..].iter().map(|p| (p.hash.clone(), Tier::Featured)).collect();
    let cap = capacity(9, 45);
    let o = BookOptions { featured_floor: 9, ..BookOptions::default() };
    let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &o);
    assert!(placed(&b, &kept, 1) >= 9, "got {}", placed(&b, &kept, 1));
}

#[test]
fn floors_that_do_not_fit_demote_suggestions_but_never_the_users_choice() {
    // 12 Normal events at floor 2 = 24 > target 20. Event 0 is the user's.
    let mut photos = Vec::new();
    for e in 0..12u32 {
        photos.extend(event(e, e as i64, 8, 50 + e as u8));
    }
    let kept = keepers(&photos);
    let tiers: EventTiers = photos[..8].iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
    let cap = capacity(20, 20); // room = 13, so all 12 are suggested Normal/Featured
    let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
    assert!(b.overflow.is_none());
    assert_eq!(b.plan[0].tier, Tier::Normal, "the user's choice is kept");
    assert!(b.plan.iter().any(|p| p.reason == Reason::Demoted));
    assert!(b.floors.values().sum::<usize>() <= 20);
}

#[test]
fn users_floors_that_cannot_fit_report_tier_overflow_and_drop_to_one() {
    let mut photos = Vec::new();
    for e in 0..6u32 {
        photos.extend(event(e, e as i64, 20, 60));
    }
    let kept = keepers(&photos);
    let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Featured)).collect();
    let cap = capacity(9, 20); // 6 x 6 = 36 > 20
    let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
    assert_eq!(b.overflow, Some(TierOverflow { needed: 36, capacity: 20 }));
    assert!(b.floors.values().all(|&f| f == 1));
    assert_eq!(b.selected.len(), 20, "the rest is still dealt out");
}

#[test]
fn a_book_whose_normal_events_run_dry_promotes_a_suggested_brief_rather_than_underfill() {
    // 2 events of 4 photos, 10 of 30 each: room 1 at 3 slots, the rest Brief-capped.
    let mut photos = Vec::new();
    for e in 0..12u32 {
        photos.extend(event(e, e as i64, if e < 2 { 4 } else { 30 }, 90 - e as u8));
    }
    let kept = keepers(&photos);
    let cap = capacity(1, 40); // 3 slots -> room 1
    let b = budget(&kept, plan_of(&photos, &EventTiers::new(), &cap), &cap, &Overrides::new(), &BookOptions::default());
    assert_eq!(b.selected.len(), 40);
    assert!(b.plan.iter().any(|p| p.reason == Reason::Filled));
}

#[test]
fn more_room_only_adds_photos_to_an_event() {
    let mut photos = event(0, 0, 60, 80);
    photos.extend(event(1, 1, 40, 70));
    let kept = keepers(&photos);
    let small = capacity(9, 45);
    let large = capacity(19, 85);
    let a = budget(&kept, plan_of(&photos, &EventTiers::new(), &small), &small, &Overrides::new(), &BookOptions::default());
    let b = budget(&kept, plan_of(&photos, &EventTiers::new(), &large), &large, &Overrides::new(), &BookOptions::default());
    assert!(a.selected.iter().all(|i| b.selected.contains(i)), "a prefix, never a swap");
}
```

- [ ] **Step 6: Run the tests and confirm they fail.** Expected: compile errors.

- [ ] **Step 7: Implement.** Add to `events.rs`:

```rust
use crate::book::cull::Overrides;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPlan {
    pub event: u32,
    /// Effective: the user's choice, else the suggestion, after `budget`'s adjustments.
    pub tier: Tier,
    pub suggested: Tier,
    pub chosen: bool,
    pub reason: Reason,
    pub merit: f64,
    pub moments: usize,
    pub kept: usize,
    pub photos: usize,
}

/// Every event's effective tier, sorted by event id.
pub fn plan(photos: &[Photo], kept: &[Photo], tiers: &EventTiers, capacity: &Capacity) -> Vec<EventPlan> {
    let stats = stats(photos, kept);
    let suggestions = suggest(&stats, capacity);
    let mut hashes: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for p in photos {
        hashes.entry(p.event_cluster).or_default().push(p.hash.as_str());
    }
    suggestions
        .into_iter()
        .zip(&stats)
        .map(|(s, st)| {
            debug_assert_eq!(s.event, st.event);
            let chosen = resolve(hashes.get(&s.event).into_iter().flatten().copied(), tiers);
            EventPlan {
                event: s.event,
                tier: chosen.unwrap_or(s.tier),
                suggested: s.tier,
                chosen: chosen.is_some(),
                reason: s.reason,
                merit: s.merit,
                moments: st.moments,
                kept: st.keepers,
                photos: st.photos,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierOverflow {
    pub needed: usize,
    pub capacity: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    /// Sorted indices into `kept`.
    pub selected: Vec<usize>,
    pub plan: Vec<EventPlan>,
    /// What each event is guaranteed: `min(floor, available)`, never below its Includes.
    pub floors: BTreeMap<u32, usize>,
    pub overflow: Option<TierOverflow>,
}

impl Budget {
    pub fn brief_events(&self) -> BTreeSet<u32> {
        self.plan.iter().filter(|p| p.tier == Tier::Brief).map(|p| p.event).collect()
    }

    pub fn selected_in(&self, kept: &[Photo], event: u32) -> usize {
        self.selected.iter().filter(|&&i| kept[i].event_cluster == event).count()
    }
}

/// Deals the book's `target_photos` out to events by tier (§5).
pub fn budget(
    kept: &[Photo],
    mut plan: Vec<EventPlan>,
    capacity: &Capacity,
    overrides: &Overrides,
    options: &BookOptions,
) -> Budget {
    let order = pack::event_order(kept, overrides);
    let target = capacity.target_photos;
    let avail = |e: u32| order.get(&e).map_or(0, |o| o.list.len());
    let included = |e: u32| order.get(&e).map_or(0, |o| o.included);
    let floor_of = |p: &EventPlan| p.tier.floor(options).min(avail(p.event)).max(included(p.event));
    let total = |plan: &[EventPlan]| plan.iter().map(floor_of).sum::<usize>();

    // Step 3: floors that do not fit demote SUGGESTIONS, lowest merit first.
    for (from, to) in [(Tier::Featured, Tier::Normal), (Tier::Normal, Tier::Brief)] {
        while total(&plan) > target {
            let Some(p) = plan
                .iter_mut()
                .filter(|p| !p.chosen && p.tier == from)
                .min_by(|a, b| a.merit.total_cmp(&b.merit).then(b.event.cmp(&a.event)))
            else {
                break;
            };
            p.tier = to;
            p.reason = Reason::Demoted;
        }
    }
    let mut floors: BTreeMap<u32, usize> = plan.iter().map(|p| (p.event, floor_of(p))).collect();
    let needed: usize = floors.values().sum();
    let mut overflow = None;
    if needed > target {
        overflow = Some(TierOverflow { needed, capacity: target });
        for p in &plan {
            let one = if p.tier == Tier::Skipped { 0 } else { 1.min(avail(p.event)) };
            floors.insert(p.event, one.max(included(p.event)));
        }
    }

    // Step 4: D'Hondt over the remainder, vote = weight x sqrt(moments).
    // Step 4b: when nothing can take a photo, promote the best suggested Brief.
    let mut quota = floors.clone();
    let mut left = target.saturating_sub(quota.values().sum());
    while left > 0 {
        let open = |p: &&EventPlan| {
            let q = quota[&p.event];
            p.tier != Tier::Skipped
                && q < avail(p.event)
                && (p.tier != Tier::Brief || q < options.brief_cap as usize)
        };
        let score = |p: &EventPlan| p.tier.weight() * (p.moments as f64).sqrt() / (quota[&p.event] + 1) as f64;
        let pick = plan
            .iter()
            .filter(open)
            .max_by(|a, b| score(a).total_cmp(&score(b)).then(b.event.cmp(&a.event)))
            .map(|p| p.event);
        match pick {
            Some(e) => {
                if let Some(q) = quota.get_mut(&e) {
                    *q += 1;
                }
                left -= 1;
            }
            None => {
                let promote = plan
                    .iter_mut()
                    .filter(|p| !p.chosen && p.tier == Tier::Brief && avail(p.event) > quota[&p.event])
                    .max_by(|a, b| a.merit.total_cmp(&b.merit).then(b.event.cmp(&a.event)));
                match promote {
                    Some(p) => {
                        p.tier = Tier::Normal;
                        p.reason = Reason::Filled;
                    }
                    None => break,
                }
            }
        }
    }

    let mut selected: Vec<usize> = order
        .iter()
        .flat_map(|(e, o)| o.list.iter().take(quota.get(e).copied().unwrap_or(0)).copied())
        .collect();
    selected.sort_unstable();
    Budget { selected, plan, floors, overflow }
}
```

If the borrow checker rejects `open`/`score` because both capture `quota` while `quota` is mutated in the `Some` arm, compute `pick` in its own block so the closures are dropped before the mutation. Do not clone `quota` for every seat.

- [ ] **Step 8: Run the tests and confirm they pass.** Run `cargo test --lib book::events book::pack`. If a fixture assertion fails, check the fixture's arithmetic first. For example, in `the_remainder_vote_uses_the_square_root_of_moments` both events have enough photos, and 85 − 4 = 81 photos are dealt at a 4:1 vote, which gives about 65:16. The bound of 5× passes under sqrt and fails under the linear vote (16:1, which gives about 76:5).

- [ ] **Step 9: Mutation-check.** Paste the evidence for each row.

| Mutant | Must go red |
|---|---|
| `floor_of` returns `included(p.event)` only (floors ignored) | `budget_gives_a_small_dull_normal_event_its_floor…` |
| The vote uses `p.moments as f64` without `.sqrt()` | `the_remainder_vote_uses_the_square_root_of_moments` |
| `p.tier != Tier::Skipped` removed from `open` | `a_skipped_event_places_nothing_but_its_includes` |
| `.max(included(p.event))` removed from `floor_of` **and** from the overflow branch | `a_skipped_event_places_nothing_but_its_includes` (the Include case) |
| The `brief_cap` clause removed from `open` | `a_brief_event_stops_at_the_brief_cap…` |
| `Tier::Featured => NORMAL_FLOOR * 3` in `Tier::floor` (a constant instead of options) | `a_featured_event_gets_the_projects_featured_floor` |
| `!p.chosen &&` removed from the demotion filter | `floors_that_do_not_fit_demote_suggestions_but_never_the_users_choice` |
| The 4b promotion replaced by `None => break` | `a_book_whose_normal_events_run_dry…` |

- [ ] **Step 10: Commit.**

```bash
git add src-tauri/src/book/pack.rs src-tauri/src/book/events.rs
git commit -m "feat(events): deal the photo budget per event by tier

<mutation evidence>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: `pack_selected`, the Brief fold, `assemble_with` and the floor invariant

**Files:**
- Modify: `src-tauri/src/book/pack.rs:391-501` (`pack`) and `:605-639` (`merge_sub_spread_chapters`).
- Modify: `src-tauri/src/book/pace.rs:419-434` (`BookError`) and `:462-601` (`assemble`).
- Modify: `src-tauri/tests/pack_sweep.rs`.
- Possibly modify: `src-tauri/tests/fixtures/book-20.json` (see Step 8).

**Interfaces:**
- Consumes: `events::{plan, budget, Budget, EventTiers}`.
- Produces, in `pack`:
  ```rust
  pub fn pack_selected(photos: &[Photo], selected: &[usize], brief: &BTreeSet<u32>, capacity: &Capacity,
      buildable: &Buildable, overrides: &Overrides) -> Result<Vec<Group>, IncludeOverflow>;
  ```
  `pack` becomes `pack_selected(photos, &select(..), &BTreeSet::new(), ..)`.
- Produces, in `pace`:
  ```rust
  pub fn assemble_with(spec: &PrintSpec, photos: &[Photo], pages: u32, lib: &Library, w: &Weights, seed: u64,
      overrides: &Overrides, tiers: &EventTiers, options: BookOptions) -> Result<Book, BookError>;
  // `assemble(..)` = `assemble_with(.., overrides, &EventTiers::new(), BookOptions::default())`
  pub struct FloorMiss { pub event: u32, pub floor: usize, pub placed: usize }
  BookError::TierFloorNotMet { misses: Vec<FloorMiss> }
  ```
  `assemble_with` sets `book.options = options`. `photos[i].event_cluster` is the chapter id, and the caller stamps place chapters first, as `generate_and_save` already does.

- [ ] **Step 1: Write the failing tests.** In `pack.rs` tests:

```rust
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
}
```

Update the existing callers of `merge_sub_spread_chapters` in `pack.rs` tests to pass `&BTreeSet::new()`.

In the `pace.rs` tests, the helpers are `fixture_photos(n)` (pace.rs:962), `real_library()` (:943), `pixajoy_spec()` and `Weights::default()`. Add:

```rust
use crate::book::events::{EventPlan, EventTiers, Reason, Tier};
use std::collections::{BTreeMap, BTreeSet};

/// `fixture_photos`' varied aspects and faces, regrouped into the given
/// (event, count, aesthetic) events: dated, two frames a moment, nothing culled.
fn tiered_photos(events: &[(u32, usize, u8)]) -> Vec<Photo> {
    let total: usize = events.iter().map(|e| e.1).sum();
    let mut base = fixture_photos(total).into_iter();
    let mut out = Vec::new();
    for &(event, count, aes) in events {
        for n in 0..count {
            let mut p = base.next().expect("enough fixture photos");
            p.is_utility = false;
            p.event_cluster = event;
            p.aesthetic_pct = aes.saturating_sub((n % 7) as u8);
            p.captured_at = Some(event as i64 * 86_400 + (n / 2) as i64 * 600 + (n % 2) as i64 * 60);
            out.push(p);
        }
    }
    out
}

fn tiers_for(photos: &[Photo], event: u32, tier: Tier) -> EventTiers {
    photos.iter().filter(|p| p.event_cluster == event).map(|p| (p.hash.clone(), tier)).collect()
}

fn placed_in_event(book: &Book, photos: &[Photo], event: u32) -> usize {
    placed_indices(book).into_iter().filter(|&i| photos[i].event_cluster == event).count()
}

#[test]
fn assemble_with_places_a_normal_events_floor_beside_a_dominant_event() {
    // The book-wide trim ranks by aesthetic and places none of the dull event.
    let photos = tiered_photos(&[(0, 200, 95), (1, 6, 8)]);
    let tiers = tiers_for(&photos, 1, Tier::Normal);
    let book = assemble_with(&pixajoy_spec(), &photos, 20, &real_library(), &Weights::default(), 7,
        &Overrides::new(), &tiers, BookOptions::default()).unwrap();
    let placed = placed_in_event(&book, &photos, 1);
    assert!(placed >= 2, "placed {placed}");
}

#[test]
fn a_two_photo_brief_event_never_owns_a_spread() {
    let photos = tiered_photos(&[(0, 60, 80), (1, 12, 90), (2, 60, 80)]);
    let tiers = tiers_for(&photos, 1, Tier::Brief);
    let book = assemble_with(&pixajoy_spec(), &photos, 20, &real_library(), &Weights::default(), 7,
        &Overrides::new(), &tiers, BookOptions::default()).unwrap();
    assert_eq!(placed_in_event(&book, &photos, 1), 2, "the Brief cap");
    for page in &book.pages {
        let events: BTreeSet<u32> =
            page.placements.iter().map(|pl| photos[pl.photo_index].event_cluster).collect();
        assert_ne!(events, BTreeSet::from([1]), "page {} holds only the Brief event", page.number);
    }
}

#[test]
fn the_output_invariant_reports_a_normal_event_below_its_floor() {
    let row = |event, tier| EventPlan {
        event, tier, suggested: tier, chosen: false, reason: Reason::Ranked { rank: 1, of: 3 },
        merit: 0.5, moments: 4, kept: 8, photos: 8,
    };
    let plan = vec![row(3, Tier::Normal), row(4, Tier::Brief), row(5, Tier::Featured)];
    let floors = BTreeMap::from([(3, 2), (4, 1), (5, 6)]);
    let placed = BTreeMap::from([(3, 1), (5, 6)]); // Brief 4 placed 0: not a floor miss
    assert_eq!(check_floors(&placed, &floors, &plan), vec![FloorMiss { event: 3, floor: 2, placed: 1 }]);
}
```

If `a_two_photo_brief_event_never_owns_a_spread` stays green under the Step 10 fold mutant, the fixture is not reaching the fold. Shrink the neighbours (for example to 20 photos each) until the mutant turns it red, and record the change. `check_floors` is a pure function extracted for exactly this test (CLAUDE.md, "Extract pure functions"):

```rust
pub(crate) fn check_floors(placed: &BTreeMap<u32, usize>, floors: &BTreeMap<u32, usize>, plan: &[EventPlan]) -> Vec<FloorMiss>
```

`FloorMiss` derives `Debug, Clone, PartialEq, Eq`.

- [ ] **Step 2: Run the tests and confirm they fail.** Expected: compile errors.

- [ ] **Step 3: Implement the fold.** In `merge_sub_spread_chapters`, add the parameter `brief: &std::collections::BTreeSet<u32>`, change the early return to `if spread_min <= 1 && brief.is_empty()`, and change the fold test to:

```rust
if members.len() < spread_min || brief.contains(&cluster) {
    carry = members;
}
```

Extend the doc comment with one paragraph: a Brief event folds whatever its size, because a 2-photo Brief chapter meets the spread minimum and would otherwise take a whole spread (spec §2).

**Last-chapter case:** the existing tail logic joins the carry to the last chapter in `out`, which is the previous chapter. That is correct.

- [ ] **Step 4: Implement `pack_selected`.** Rename `pack`'s body to `pack_selected` and add the parameters `selected: &[usize], brief: &BTreeSet<u32>`. Replace:

```rust
for i in select(photos, capacity.target_photos, overrides) {
```

with:

```rust
for &i in selected {
```

and pass `brief` to `merge_sub_spread_chapters`. Then:

```rust
pub fn pack(photos: &[Photo], capacity: &Capacity, buildable: &Buildable, overrides: &Overrides)
    -> Result<Vec<Group>, IncludeOverflow> {
    let selected = select(photos, capacity.target_photos, overrides);
    pack_selected(photos, &selected, &std::collections::BTreeSet::new(), capacity, buildable, overrides)
}
```

- [ ] **Step 5: Implement `assemble_with` and the invariant.** In `pace.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorMiss {
    pub event: u32,
    pub floor: usize,
    pub placed: usize,
}
```

Add the `BookError` variant:

```rust
    /// A Featured or Normal event placed fewer than `min(floor, available)`
    /// photos and no `TierOverflow` excused it (spec §5, "Invariant"). Checked
    /// on the finished book for the same reason as `IncludedNotPlaced`.
    TierFloorNotMet { misses: Vec<FloorMiss> },
```

Add the `Display` arm:

```rust
            Self::TierFloorNotMet { misses } => write!(
                f,
                "{} event(s) got fewer photos than their tier promises (event {}: {} of {}). \
                 Choose a longer book or lower an event's tier.",
                misses.len(),
                misses[0].event,
                misses[0].placed,
                misses[0].floor
            ),
```

Add the pure checker:

```rust
pub(crate) fn check_floors(
    placed: &BTreeMap<u32, usize>,
    floors: &BTreeMap<u32, usize>,
    plan: &[crate::book::events::EventPlan],
) -> Vec<FloorMiss> {
    use crate::book::events::Tier;
    plan.iter()
        .filter(|p| matches!(p.tier, Tier::Featured | Tier::Normal))
        .filter_map(|p| {
            let floor = floors.get(&p.event).copied().unwrap_or(0);
            let got = placed.get(&p.event).copied().unwrap_or(0);
            (got < floor).then_some(FloorMiss { event: p.event, floor, placed: got })
        })
        .collect()
}
```

Change `assemble`'s signature line to `assemble_with` with the two extra parameters, and add a new `assemble` wrapper:

```rust
pub fn assemble(
    spec: &PrintSpec, photos: &[Photo], pages: u32, lib: &Library, w: &Weights, seed: u64, overrides: &Overrides,
) -> Result<Book, BookError> {
    assemble_with(spec, photos, pages, lib, w, seed, overrides, &crate::book::events::EventTiers::new(), BookOptions::default())
}
```

Inside `assemble_with`, replace the `pack(...)` call with:

```rust
    let plan = crate::book::events::plan(photos, &kept, tiers, &cap);
    let budget = crate::book::events::budget(&kept, plan, &cap, overrides, &options);
    let groups = pack_selected(&kept, &budget.selected, &budget.brief_events(), &cap, &buildable, overrides)
        .map_err(BookError::IncludedExceedCapacity)?;
```

Replace `options: BookOptions::default()` in the `Book` literal with `options`. After the `IncludedNotPlaced` check, add:

```rust
    if budget.overflow.is_none() {
        let mut per_event: BTreeMap<u32, usize> = BTreeMap::new();
        for &i in &placed {
            *per_event.entry(photos[i].event_cluster).or_default() += 1;
        }
        let misses = check_floors(&per_event, &budget.floors, &budget.plan);
        if !misses.is_empty() {
            return Err(BookError::TierFloorNotMet { misses });
        }
    }
```

Add `#[allow(clippy::too_many_arguments)]` on `assemble_with`, matching any existing use in the crate. If clippy rejects the attribute style, bundle `(overrides, tiers, options)` into a `pub struct Choices<'a>` instead, and use that everywhere this plan names the three arguments.

- [ ] **Step 6: Run the new tests.** Run `cargo test --lib book::pace book::pack`. Expected: the new tests pass.

- [ ] **Step 7: Run the whole Rust suite.** Run `bun run test:rust`. Existing `assemble` tests now go through tiers. Classify every failure before changing anything:
  - **Expected movement:** a golden or a layout assertion that changed because photos moved between events, for example `tests/fixtures/book-20.json`.
  - **A real bug:** a new `TierFloorNotMet`, a blank page that was not there before, or an `IncludedNotPlaced`.

  Fix real bugs in the engine. **Never loosen an assertion** to fix one. For expected movement, first run the Task 1 report (Step 9) and confirm events-at-zero did not get worse. Only then regenerate a golden, with that file's documented regeneration command, and list every regenerated file in the commit body with one line on why it moved.

- [ ] **Step 8: Extend `pack_sweep`.** In `src-tauri/tests/pack_sweep.rs`, add an assertion in the sweep loop that `assemble` never returns `BookError::TierFloorNotMet`. Its existing blank-page and dropped-photo assertions stay as they are. Run `cargo test --test pack_sweep`. Expected: PASS.

- [ ] **Step 9: Re-run the report.** Run `cargo run --release --example book_report -- 4 5 11` and again with `--places`. Paste the results under the baseline in spec §9 as "After Task 7". `events_at_zero` must not rise for any project, and `blank_pages` must not rise. If either rises, stop and re-plan. Do not push on.

- [ ] **Step 10: Mutation-check.**

| Mutant | Must go red |
|---|---|
| `|| brief.contains(&cluster)` removed | `a_two_photo_brief_chapter_folds_into_its_neighbour` and `a_two_photo_brief_event_never_owns_a_spread` |
| `check_floors` returns `Vec::new()` | `the_output_invariant_reports_a_normal_event_below_its_floor` |
| `assemble_with` calls `pack(&kept, ..)` (the book-wide trim) | `assemble_with_places_a_normal_events_floor_beside_a_dominant_event` |

- [ ] **Step 11: Commit.**

```bash
git add src-tauri/src/book/pack.rs src-tauri/src/book/pace.rs src-tauri/tests/pack_sweep.rs docs/superpowers/specs/2026-09-23-event-tiers-design.md <each regenerated golden, by path>
git commit -m "feat(layout): build books from the per-event budget

<report before/after, regenerated goldens with reasons, mutation evidence>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Commands: recommend per event, and take `tiers` and `options`

**Files:**
- Modify: `src-tauri/src/commands.rs`:
  - `PageOption`/`BookRecommendation` at :1079/:1095;
  - `recommend` at :1433;
  - `generate_and_save` at :1520;
  - `recommend_book` at :1879;
  - `generate_book` at :1938.
- Modify: `tests/fixtures/wire/book-recommendation.json`.
- Modify: `app/types/book.ts`.
- Test: the `recommend` tests in commands.rs (`grep -n "fn .*recommend" src-tauri/src/commands.rs`); `tests/book.test.ts`.

**Interfaces:**
- Produces:
  ```rust
  pub(crate) fn with_chapters(photos: &[Photo], places: bool) -> std::borrow::Cow<'_, [Photo]>;
  #[derive(Serialize)] #[serde(rename_all = "camelCase")]
  pub struct EventRow { #[serde(flatten)] pub plan: EventPlan, pub selected: usize }
  #[derive(Serialize, Default)] #[serde(rename_all = "camelCase")]
  pub struct TierCounts { pub featured: usize, pub normal: usize, pub brief: usize, pub skipped: usize }
  PageOption { pages, capacity_photos, dropped_photos, included_over_capacity,
      events: Vec<EventRow>, events_by_tier: TierCounts, selected_paths: Vec<String>,
      tier_overflow: Option<TierOverflow> }
  pub(crate) fn recommend(photos: &[Photo], lib: &Library, overrides: &Overrides, tiers: &EventTiers, options: BookOptions) -> BookRecommendation;
  recommend_book(app, run_id, overrides: Option<Overrides>, tiers: Option<EventTiers>, options: Option<BookOptions>)
  generate_book(.., overrides, spec, options, tiers: Option<EventTiers>)
  generate_and_save(db, meta, photos, lib, weights, overrides, tiers: &EventTiers)
  ```
- Produces (TS, `app/types/book.ts`):
  ```ts
  export type TierReason =
    | { kind: "utility"; share: number } | { kind: "nothingKept" } | { kind: "undated" }
    | { kind: "ranked"; rank: number; of: number } | { kind: "standout" }
    | { kind: "outOfRoom"; rank: number; of: number } | { kind: "similarTo"; event: number }
    | { kind: "demoted" } | { kind: "filled" };
  export interface EventRow { event: number; tier: Tier; suggested: Tier; chosen: boolean; reason: TierReason;
    merit: number; moments: number; kept: number; photos: number; selected: number }
  export interface TierCounts { featured: number; normal: number; brief: number; skipped: number }
  export interface TierOverflow { needed: number; capacity: number }
  // PageOption gains: events: EventRow[]; eventsByTier: TierCounts; selectedPaths: string[]; tierOverflow: TierOverflow | null
  export function tierReasonText(reason: TierReason, title: (event: number) => string): string;
  ```

- [ ] **Step 1: Write the failing Rust test** next to the existing `recommend` tests (commands.rs:5427). They use `photo_record(i, is_utility, dup, event)` and `photos_from_records`, and their records carry no capture time. That is fine now that an all-undated library is ranked (Task 5).

```rust
fn shipped_library() -> Library {
    Library::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates")).expect("shipped library")
}

fn three_events() -> Vec<Photo> {
    let records: Vec<_> = (0..106)
        .map(|i| photo_record(i, false, i as u32, if i < 60 { 0 } else if i < 66 { 1 } else { 2 }))
        .collect();
    photos_from_records(&records).unwrap()
}

#[test]
fn recommend_reports_each_event_and_selects_by_tier() {
    let photos = three_events();
    let mut tiers = EventTiers::new();
    for p in photos.iter().filter(|p| p.event_cluster == 2) {
        tiers.set(p.hash.clone(), Some(Tier::Skipped));
    }
    let rec = recommend(&photos, &shipped_library(), &Overrides::new(), &tiers, BookOptions::default());
    let twenty = rec.options.iter().find(|o| o.pages == 20).unwrap();
    assert_eq!(twenty.events.len(), 3);
    let skipped = twenty.events.iter().find(|r| r.plan.event == 2).unwrap();
    assert_eq!((skipped.plan.tier, skipped.plan.chosen, skipped.selected), (Tier::Skipped, true, 0));
    assert_eq!(twenty.events_by_tier.skipped, 1);
    assert_eq!(twenty.selected_paths.len(), rec.keeper_count - twenty.dropped_photos);
    assert!(twenty.selected_paths.iter().all(|p| photos.iter().any(|q| &q.path == p && q.event_cluster != 2)));
}

#[test]
fn recommend_respects_the_places_option_for_event_ids() {
    // One time event: 40 photos a minute apart, the first 20 in Reykjavik,
    // the last 20 in Vík (~180 km). Places on splits it into two chapters.
    let lib = shipped_library();
    let records: Vec<_> = (0..40).map(|i| photo_record(i, false, i as u32, 0)).collect();
    let mut photos = photos_from_records(&records).unwrap();
    for (i, p) in photos.iter_mut().enumerate() {
        p.captured_at = Some(1_700_000_000 + i as i64 * 60);
        p.location = if i < 20 { LatLon::new(64.14, -21.94) } else { LatLon::new(63.42, -19.01) };
    }
    let rec_off = recommend(&photos, &lib, &Overrides::new(), &EventTiers::new(), BookOptions::default());
    let rec_on = recommend(&photos, &lib, &Overrides::new(), &EventTiers::new(),
        BookOptions { places: true, ..BookOptions::default() });
    assert_eq!(rec_off.options[0].events.len(), 1);
    assert_eq!(rec_on.options[0].events.len(), 2);
}
```

- [ ] **Step 2: Run the tests and confirm they fail.** Expected: compile errors.

- [ ] **Step 3: Implement.**

```rust
/// Photos with `event_cluster` restamped to place chapters when Places is on,
/// borrowed untouched otherwise. The one place this choice is made, shared by
/// `recommend` and `generate_and_save` so the two can never count events differently.
pub(crate) fn with_chapters(photos: &[Photo], places: bool) -> std::borrow::Cow<'_, [Photo]> {
    if !places {
        return std::borrow::Cow::Borrowed(photos);
    }
    std::borrow::Cow::Owned(
        photos
            .iter()
            .zip(crate::book::chapter::chapters(photos, true))
            .map(|(p, event_cluster)| Photo { event_cluster, ..p.clone() })
            .collect(),
    )
}
```

Replace the `by_place` block in `generate_and_save` with `let photos = with_chapters(photos, meta.options.places);`. Pass `&photos` onward, call `assemble_with(.., overrides, tiers, meta.options)`, and delete the now-redundant `book.options = meta.options;`.

Rewrite `recommend`:

```rust
pub(crate) fn recommend(
    photos: &[Photo],
    lib: &Library,
    overrides: &Overrides,
    tiers: &EventTiers,
    options: BookOptions,
) -> BookRecommendation {
    let photos = with_chapters(photos, options.places);
    let culled = crate::book::cull::cull(&photos, overrides);
    let keeper_count = culled.len();
    let included_count = overrides.included_in(&photos);
    let options_out = PAGE_OPTIONS
        .iter()
        .map(|&pages| {
            let capacity = Capacity::from_library(pages, lib);
            let plan = crate::book::events::plan(&photos, &culled, tiers, &capacity);
            let budget = crate::book::events::budget(&culled, plan, &capacity, overrides, &options);
            let mut by_tier = TierCounts::default();
            let events = budget
                .plan
                .iter()
                .map(|p| {
                    match p.tier {
                        Tier::Featured => by_tier.featured += 1,
                        Tier::Normal => by_tier.normal += 1,
                        Tier::Brief => by_tier.brief += 1,
                        Tier::Skipped => by_tier.skipped += 1,
                    }
                    EventRow { plan: p.clone(), selected: budget.selected_in(&culled, p.event) }
                })
                .collect();
            PageOption {
                pages,
                capacity_photos: capacity.target_photos,
                dropped_photos: keeper_count - budget.selected.len(),
                included_over_capacity: included_count.saturating_sub(capacity.max_photos),
                events,
                events_by_tier: by_tier,
                selected_paths: budget.selected.iter().map(|&i| culled[i].path.clone()).collect(),
                tier_overflow: budget.overflow,
            }
        })
        .collect();
    BookRecommendation {
        keeper_count,
        included_count,
        recommended_pages: recommend_pages(keeper_count, lib),
        options: options_out,
    }
}
```

Keep the existing doc comments on `dropped_photos` and `included_over_capacity`. Update `recommend_book` and `generate_book` to take `tiers: Option<EventTiers>` (and `options` for `recommend_book`), each with a one-line comment in the style of the existing arguments, and pass `&tiers.unwrap_or_default()`. Fix every other caller of `recommend`/`generate_and_save` the compiler names, using `&EventTiers::new()` in tests.

- [ ] **Step 4: Run the tests and confirm they pass.** Run `bun run test:rust`.

- [ ] **Step 5: Update the wire fixture.** Update `tests/fixtures/wire/book-recommendation.json` so that each option has `events` (one row, all fields), `eventsByTier`, `selectedPaths` and `tierOverflow: null`. Run the Rust wire test for it and make the Rust side agree. Then add the TS types above and write a TS test in `tests/book.test.ts`:

```ts
describe("tierReasonText", () => {
  const title = (e: number) => `Event ${e + 1}`;
  it.each([
    [{ kind: "utility", share: 0.92 }, "Mostly screenshots and documents (92%)"],
    [{ kind: "nothingKept" }, "No photo here survived culling"],
    [{ kind: "undated" }, "These photos have no date"],
    [{ kind: "ranked", rank: 3, of: 19 }, "Ranked 3 of 19"],
    [{ kind: "standout" }, "Stands out from the rest of the trip"],
    [{ kind: "outOfRoom", rank: 14, of: 19 }, "Ranked 14 of 19, beyond what this length has room for"],
    [{ kind: "similarTo", event: 0 }, "Similar to “Event 1”"],
    [{ kind: "demoted" }, "Lowered so every event's minimum fits this length"],
    [{ kind: "filled" }, "Raised so the book is not left with empty pages"],
  ] as const)("%j", (reason, text) => {
    expect(tierReasonText(reason, title)).toBe(text);
  });
  it("matches the wire fixture's first event row", () => {
    const rec = fixture<BookRecommendation>("book-recommendation.json");
    expect(typeof tierReasonText(rec.options[0]!.events[0]!.reason, title)).toBe("string");
  });
});
```

Implement `tierReasonText` with a `switch` on `reason.kind` that returns exactly those strings, with `Math.round(share * 100)` for the percentage.

- [ ] **Step 6: Run the checks.** Run `bun run test && bun run test:rust && bun run lint && bun run typecheck`. Expected: all pass.

- [ ] **Step 7: Mutation-check.** Make `with_chapters` ignore `places` (always `Borrowed`). `recommend_respects_the_places_option_for_event_ids` must go red.

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/commands.rs tests/fixtures/wire/book-recommendation.json app/types/book.ts tests/book.test.ts
git commit -m "feat(recommend): per-event tiers and selected photos for each length

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Persist tiers with the project

**Files:**
- Modify: `src-tauri/src/db.rs`:
  - the schema at :155;
  - `save_project_in` at :460-505;
  - `load_project` at :510-560;
  - `purge_project` at :887. `delete_project` is a soft delete and must not touch the tiers.
- Modify: `src-tauri/src/project.rs` (the `Project` struct).
- Modify: `src-tauri/src/commands.rs` (`ProjectDetail` at :1191 and its construction at :2170; `generate_and_save`'s save call).
- Modify: `tests/fixtures/wire/project-detail.json` and `app/types/book.ts` (`ProjectDetail.tiers: EventTiers`).

**Interfaces:**
- Produces: `Db::save_project_with(&self, name, source_folders: &[String], book, photo_hashes, overrides, tiers: &EventTiers) -> rusqlite::Result<i64>`. `save_project_in` delegates to it with `&EventTiers::new()`.
- Produces: `Project.tiers: EventTiers`, `ProjectDetail.tiers: EventTiers`, and on the TS side `ProjectDetail.tiers: EventTiers`.

- [ ] **Step 1: Write the failing tests** in the `db.rs` tests, modelled on the override round-trip tests near :1808-1861:

```rust
#[test]
fn project_tiers_round_trip_and_an_unknown_token_fails_the_load() {
    let db = Db::open_in_memory().unwrap();
    let tiers: EventTiers =
        [("h1".to_string(), Tier::Featured), ("h2".to_string(), Tier::Skipped)].into_iter().collect();
    let id = db
        .save_project_with("T", &["/a".to_string()], &fixture_book(), &fixture_hashes(), &Overrides::new(), &tiers)
        .unwrap();
    assert_eq!(db.load_project(id).unwrap().unwrap().tiers, tiers);
    db.conn.execute("UPDATE project_event_tiers SET tier = 'hero' WHERE hash = 'h1'", []).unwrap();
    assert!(db.load_project(id).is_err(), "an unknown tier must fail the load, never become Auto");
}

/// `delete_project` is a soft delete (the library's undo): the tiers must
/// survive it and come back with `restore_project`, and go only on purge.
#[test]
fn tiers_survive_a_restore_and_go_with_a_purge() {
    let db = Db::open_in_memory().unwrap();
    let tiers: EventTiers = [("h1".to_string(), Tier::Brief)].into_iter().collect();
    let id = db
        .save_project_with("T", &["/a".to_string()], &fixture_book(), &fixture_hashes(), &Overrides::new(), &tiers)
        .unwrap();
    db.delete_project(id).unwrap();
    assert_eq!(db.restore_project(id).unwrap(), 1);
    assert_eq!(db.load_project(id).unwrap().unwrap().tiers, tiers);
    db.delete_project(id).unwrap();
    db.purge_project(id).unwrap();
    let rows: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM project_event_tiers WHERE project_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.**
  - Schema, after `project_photo_overrides`:
    ```sql
    CREATE TABLE IF NOT EXISTS project_event_tiers (
        project_id INTEGER NOT NULL REFERENCES projects(id),
        hash       TEXT NOT NULL,
        tier       TEXT NOT NULL,
        PRIMARY KEY (project_id, hash)
    );
    ```
  - Insert in the same transaction as overrides:
    ```rust
    {
        let mut stmt = tx.prepare(
            "INSERT INTO project_event_tiers (project_id, hash, tier) VALUES (?1, ?2, ?3)",
        )?;
        for (hash, tier) in tiers.iter() {
            stmt.execute(rusqlite::params![id, hash, tier.as_str()])?;
        }
    }
    ```
  - Load them after the overrides, with the same error shape: an unknown token becomes `FromSqlConversionFailure`, and the comment explains why it fails rather than degrading.
  - Add `DELETE FROM project_event_tiers WHERE project_id = ?1` in `purge_project` (db.rs:887), before the `projects` row goes: `PRAGMA foreign_keys` is on and nothing cascades. Update its doc comment's list of what it removes.
  - Add `pub tiers: EventTiers` to `Project`, and `tiers` to `ProjectDetail`, filled from `project.tiers`.
  - `generate_and_save` calls `save_project_with(.., overrides, tiers)`.

- [ ] **Step 4: Update the fixture and the TS type.** Update `project-detail.json` with `"tiers": {"h1": "featured"}` and add `tiers: EventTiers` to TS `ProjectDetail`. Run `bun run test && bun run test:rust && bun run typecheck`. Expected: all pass.

- [ ] **Step 5: Mutation-check.** Make the load map an unknown token to `Tier::Normal`. `project_tiers_round_trip_and_an_unknown_token_fails_the_load` must go red. Remove the purge `DELETE`. `tiers_survive_a_restore_and_go_with_a_purge` must go red (a foreign-key error, or a count of 1).

- [ ] **Step 6: Commit.**

```bash
git add src-tauri/src/db.rs src-tauri/src/project.rs src-tauri/src/commands.rs tests/fixtures/wire/project-detail.json app/types/book.ts
git commit -m "feat(projects): save event tier choices with the book

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: Draft state, wire calls and dimming by "placed at this length"

**Files:**
- Modify: `app/composables/useAnalysisJobs.ts`:
  - `SavedDraft`/`AnalysisJob` gain `tiers: EventTiers`;
  - `parseSavedDraft` at :134;
  - the draft save, wherever `overrides` is serialised in this file;
  - the reopen path that restores `overrides` from `ProjectDetail`.
- Modify: `app/composables/useBook.ts:139` and `:152`. Add a `tiers` ref parameter beside `overrides`, and send `tiers` and `options`.
- Modify: `app/pages/index.vue:118-132`. Restore `tiers` like `overrides`.
- Modify: `app/components/SelectPhotos.vue`:
  - a `tiers` computed on the job, like `overrides` at :59;
  - `selectedPaths` from the recommendation;
  - `:is-kept` at :376.
- Modify: `app/components/GenerateBook.vue`. Add a `tiers` prop, pass it to `useBook`, and emit the chosen option.
- Modify: `docs/manual.md:98`.
- Test: `tests/jobs.test.ts`, `tests/book.test.ts`.

**Interfaces:**
- Consumes: `EventTiers`, `withEventTier` (Task 4); `PageOption.selectedPaths`/`events` (Task 8).
- Produces: `GenerateBook` emits `"option", PageOption | undefined` whenever `chosenOption` changes. `SelectPhotos` holds `chosenOption: Ref<PageOption | undefined>`.
- Produces: the pure helper `app/types/book.ts: isPlaced(photo: { path: string; kept: boolean }, option: PageOption | undefined): boolean`. It returns `option.selectedPaths` membership when an option exists, and otherwise `photo.kept`.

- [ ] **Step 1: Write the failing tests.** In `tests/jobs.test.ts`:

```ts
describe("parseSavedDraft tiers", () => {
  const base = { id: 1, name: "Trip", folders: ["/a"] };
  it("restores the user's tier choices", () => {
    const draft = parseSavedDraft(JSON.stringify({ ...base, tiers: { h1: "featured", h2: "skipped" } }));
    expect(draft?.tiers).toEqual({ h1: "featured", h2: "skipped" });
  });
  it("gives a draft saved before tiers an empty map", () => {
    expect(parseSavedDraft(JSON.stringify(base))?.tiers).toEqual({});
  });
  it("fails the load on an unknown tier token (spec §6)", () => {
    expect(parseSavedDraft(JSON.stringify({ ...base, tiers: { h1: "hero" } }))).toBeNull();
  });
});
```

In `tests/book.test.ts`:

```ts
describe("isPlaced", () => {
  const option = { selectedPaths: ["/a.jpg"] } as Pick<PageOption, "selectedPaths"> as PageOption;
  it("dims by the chosen length once there is one", () => {
    expect(isPlaced({ path: "/a.jpg", kept: true }, option)).toBe(true);
    expect(isPlaced({ path: "/b.jpg", kept: true }, option)).toBe(false);
  });
  it("falls back to the cull verdict before a recommendation arrives", () => {
    expect(isPlaced({ path: "/b.jpg", kept: true }, undefined)).toBe(true);
  });
});
```

The `as Pick<…> as PageOption` cast in that test needs care. If the checker rejects it, build a complete `PageOption` literal instead. Never use `any`.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `bun run test tests/jobs.test.ts tests/book.test.ts`.

- [ ] **Step 3: Implement.**
  - In `parseSavedDraft`, destructure `tiers`, then:
    ```ts
    const chosenTiers: EventTiers = {};
    if (isRecord(tiers)) {
      for (const [hash, tier] of Object.entries(tiers)) {
        if (typeof tier !== "string" || !(TIERS as readonly string[]).includes(tier)) return null;
        chosenTiers[hash] = tier as Tier;
      }
    }
    ```
    Then return `tiers: chosenTiers`. The `tier as Tier` cast is safe because of the `includes` check. If the linter flags it, write a type guard `isTier(value: unknown): value is Tier` in `features.ts` and use that instead.
  - Wherever the draft is saved, add `tiers: job.tiers` beside `overrides`.
  - `useBook`: add a `tiers: Ref<EventTiers>` parameter after `overrides`. Send `tiers: tiers.value, options` in `recommend_book`. `refreshRecommendation` must receive `options`, so change it to `refreshRecommendation(options: BookOptions)` and update its callers. Send `tiers: tiers.value` in `generate_book`.
  - In GenerateBook's watcher that re-runs the recommendation (the `() => overrides` watch at :163-171), add the tiers and `options.value`, so any change re-recommends.
  - `isPlaced` goes in `app/types/book.ts`:
    ```ts
    export function isPlaced(photo: { path: string; kept: boolean }, option: PageOption | undefined): boolean {
      return option ? option.selectedPaths.includes(photo.path) : photo.kept;
    }
    ```
    The linear `includes` is fine at ≤ ~100 paths. If profiling says otherwise, precompute a `Set` in `SelectPhotos` and pass it in.
  - `SelectPhotos.vue`:
    - `const chosenOption = ref<PageOption>()`;
    - `<GenerateBook … :tiers @option="chosenOption = $event" />`;
    - `:is-kept="isPlaced(photo, chosenOption)"`.
  - `index.vue`: restore `tiers` from the draft/project exactly as `overrides` is restored (`restoreTiers: payload.tiers`), and follow `restoreOverrides` through to where it lands on the job.
  - Update PhotoTile's `isKept` doc comment (PhotoTile.vue:31 and :49-53), so it no longer claims the dimming is the cull result.

- [ ] **Step 4: Update the manual.** Replace the sentence at `docs/manual.md:98`, "The contact sheet shows every photo with the ones the book would leave out dimmed.", with:

  > The contact sheet dims every photo the book would leave out **at the length you have chosen**. A longer book places more, so switching from 20 to 40 pages lights more of them up.

- [ ] **Step 5: Run the checks.** Run `bun run test && bun run lint && bun run typecheck && bun run check:build`. Expected: all pass.

- [ ] **Step 6: Look at it.** Run `bun run dev`, open the Iceland draft, and switch between 20 and 40 pages. The number of undimmed tiles must change, and a photo that was undimmed at 20 pages must stay undimmed at 40 (the prefix property). Take a screenshot. Check the right edge and the last row of the sheet for anything clipped, and say in the commit body what you checked.

- [ ] **Step 7: Commit.**

```bash
git add app/composables/useAnalysisJobs.ts app/composables/useBook.ts app/pages/index.vue app/components/SelectPhotos.vue app/components/GenerateBook.vue app/components/PhotoTile.vue app/types/book.ts app/types/features.ts tests/jobs.test.ts tests/book.test.ts docs/manual.md
git commit -m "feat(draft): carry tier choices and dim by what the chosen length places

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: The tier control, the events panel, the steppers, and confirming Update

**Files:**
- Create: `app/components/EventTierControl.vue`.
- Create: `app/components/EventsPanel.vue`.
- Modify: `app/components/ContactSheet.vue`. Add an optional `header` slot for extra header content.
- Modify: `app/components/SelectPhotos.vue`. Fill that slot with `EventTierControl`, and dim a Skipped event.
- Modify: `app/components/GenerateBook.vue`:
  - add the steppers under **Split chapters by place**;
  - put `EventsPanel` above the page-length choice;
  - extend the page-length labels;
  - add the Update confirmation.
- Modify: `docs/manual.md`, `docs/screenshots/` (if neighbouring entries have screenshots).
- Test: `tests/book.test.ts` (pure helpers only; vitest never imports `.vue`).

**Interfaces:**
- Consumes: `EventRow`, `tierReasonText`, `withEventTier`, `PageOption.eventsByTier`.
- Produces:
  - `EventTierControl` props: `{ row: EventRow | undefined; title: string }`. It emits `"set", Tier | "auto"`.
  - `EventsPanel` props: `{ rows: EventRow[]; title: (event: number) => string }`. It emits `"reveal", number`.
  - Pure helper in `app/types/book.ts`: `pageOptionLabel(option: PageOption): string`, which returns `"40 pages · 12 of 19 events, 85 photos"`.

- [ ] **Step 1: Write the failing test for the label helper.**

```ts
describe("pageOptionLabel", () => {
  it("counts the events that get their own place, and the photos", () => {
    const option = {
      pages: 40, capacityPhotos: 85, droppedPhotos: 0, includedOverCapacity: 0,
      events: [], selectedPaths: Array.from({ length: 85 }, (_, i) => `/p${i}`), tierOverflow: null,
      eventsByTier: { featured: 2, normal: 10, brief: 5, skipped: 2 },
    } satisfies PageOption;
    expect(pageOptionLabel(option)).toBe("40 pages · 12 of 19 events, 85 photos");
  });
});
```

- [ ] **Step 2: Run the test and confirm it fails.** Then implement:

```ts
export function pageOptionLabel(option: PageOption): string {
  const t = option.eventsByTier;
  const own = t.featured + t.normal;
  const all = own + t.brief + t.skipped;
  return `${option.pages} pages · ${own} of ${all} events, ${option.selectedPaths.length} photos`;
}
```

Use it for the `label` in `pageItems` (GenerateBook.vue:125-127).

- [ ] **Step 3: Build `EventTierControl.vue`.** It is monochrome (memory: ui-style-monochrome), and tiers are shown by type and weight, never colour.

```vue
<script setup lang="ts">
import { tierReasonText, type EventRow } from "~/types/book";
import type { Tier } from "~/types/features";

const { row, title } = defineProps<{ row: EventRow | undefined; title: string }>();
const emit = defineEmits<{ set: [tier: Tier | "auto"] }>();

const LABELS: Record<Tier, string> = { featured: "Featured", normal: "Normal", brief: "Brief", skipped: "Skip" };
const current = computed<Tier | "auto">(() => (row?.chosen ? row.tier : "auto"));
const items = computed(() => [
  { label: "Featured", value: "featured" },
  { label: row && !row.chosen ? `Auto: ${LABELS[row.tier]}` : "Auto", value: "auto" },
  { label: "Brief", value: "brief" },
  { label: "Skip", value: "skipped" },
]);
const reason = computed(() => (row ? tierReasonText(row.reason, () => title) : ""));
</script>

<template>
  <UTooltip :text="reason" :disabled="!row">
    <UTabs
      :items
      :model-value="current"
      size="xs"
      color="neutral"
      variant="link"
      :content="false"
      :aria-label="`Tier for ${title}`"
      @update:model-value="(v) => emit('set', v as Tier | 'auto')"
    />
  </UTooltip>
</template>
```

Two details to check:
- `similarTo` names another event, so pass the real title function down rather than `() => title`. Add a `titleOf: (event: number) => string` prop if needed.
- Check the `UTabs` API in the installed Nuxt UI version (`grep -rn "UTabs" app` for existing use). Use the component the codebase already uses for a segmented choice. Replace the `as` cast with a type guard if oxlint flags it.

- [ ] **Step 4: Put the control in the header.** In `ContactSheet.vue`, add a slot to the header `<h2>` after the count span:

```vue
<span class="ml-auto shrink-0"><slot name="header" :event="row.event" /></span>
```

Add `event: number` to `SheetEventRow` in `app/types/sheet.ts`, filled from `group.eventCluster` in `sheetRows`. Update `tests/sheet.test.ts` for the new field. Extend `defineSlots` with `header(props: { event: number }): unknown`.

In `SelectPhotos.vue`:

```vue
<template #header="{ event }">
  <EventTierControl
    :row="chosenOption?.events.find((r) => r.event === event)"
    :title="titleOf(event)"
    @set="(tier) => setEventTier(event, tier)"
  />
</template>
```

with:

```ts
function setEventTier(event: number, tier: Tier | "auto") {
  const hashes = eventGroups.value.find((g) => g.eventCluster === event)?.photos.map((p) => p.hash) ?? [];
  tiers.value = withEventTier(tiers.value, hashes, tier);
}
```

- [ ] **Step 5: Build `EventsPanel.vue`.** It shows one row per event, as in spec §7:

```vue
<script setup lang="ts">
import { tierReasonText, type EventRow } from "~/types/book";

const { rows, title } = defineProps<{ rows: EventRow[]; title: (event: number) => string }>();
const emit = defineEmits<{ reveal: [event: number] }>();
const LABELS = { featured: "Featured", normal: "Normal", brief: "Brief", skipped: "Skip" } as const;
</script>

<template>
  <div class="flex flex-col gap-1 text-sm">
    <h3 class="font-semibold text-highlighted">Events</h3>
    <button
      v-for="row in rows"
      :key="row.event"
      type="button"
      class="grid grid-cols-[1fr_auto] gap-x-3 rounded px-2 py-1 text-left hover:bg-elevated"
      @click="emit('reveal', row.event)"
    >
      <span class="min-w-0 truncate" :class="row.tier === 'featured' ? 'font-semibold' : ''">{{ title(row.event) }}</span>
      <span class="text-muted">
        {{ LABELS[row.tier] }} <span class="text-dimmed">({{ row.chosen ? "you" : "auto" }})</span>
      </span>
      <span class="col-span-2 text-xs text-muted tabular-nums">
        <template v-if="row.tier === 'skipped' || row.selected === 0">{{ tierReasonText(row.reason, title) }}</template>
        <template v-else>{{ row.kept }} kept → {{ row.selected }} placed</template>
      </span>
    </button>
  </div>
</template>
```

Mount it in `GenerateBook.vue` above the page-length control, with `:rows="chosenOption?.events ?? []"`. The `reveal` event is emitted up to `SelectPhotos`, which scrolls the sheet to that event's header row. To find the row, use `sheetRows`' key `event-${id}` and the virtualizer's `scrollToIndex`, which needs an exposed method on `ContactSheet` (`defineExpose({ revealEvent })`). The title function is the same one the sheet uses: the place name when Places is on, otherwise `Event N`.

If `chosenOption.tierOverflow` is set, show under the panel: "These tiers need **{needed}** photos but {pages} pages hold {capacity}; every event gets at least one." Use text styling only.

**Say what 20 → 40 changes:** when the chosen length changes, compare the old and new `events` and, if any tier rose, show "At {pages} pages, {n} more events get their own pages." for that render.

- [ ] **Step 6: Add the steppers.** Put two `UInputNumber`s under the Places switch, each `:min`/`:max` from `FEATURED_FLOOR_RANGE`/`BRIEF_CAP_RANGE`:
  - "Featured events get at least `[6]` photos"
  - "Brief events get at most `[2]` photos"

  Each writes `options.value = { ...options.value, featuredFloor: n }` (or `briefCap`). The watcher from Task 10 already re-recommends on `options`.

- [ ] **Step 7: Confirm before Update.** In `GenerateBook.vue`, make the Update button open a `UModal`. Follow the `DeleteBookDialog.vue` pattern. Title it "Update “{name}”?" and use this body: "Updating builds the book again from these photos and settings. Any changes you made in the editor, such as swapped photos, layouts and captions, are replaced, and the export history is cleared." Its buttons are **Cancel** and **Update**, and only **Update** calls `onGenerate(true)`. Check that last clause against what `generate_book` + `deleteProject` actually discard. Read `onGenerate` (GenerateBook.vue:182-200), and state only what is true.

- [ ] **Step 8: Update the manual.** Match the neighbouring entries' style in `docs/manual.md`:
  - **Event tiers.** Add a subsection after the contact-sheet paragraph. It covers:
    - the four tiers and what each gets (Featured ≥ your floor, Normal ≥ 2, Brief 1 up to your cap, Skip none);
    - that the app suggests a tier with a reason, and that the header control changes it;
    - that **+** on a photo beats Skip;
    - that choices are saved with the draft and the book.
  - **Events panel.** Add the panel, and the page-length label "12 of 19 events".
  - **Steppers.** Add the two steppers under **Split chapters by place**.
  - **Update.** Add that Update now asks first, and what it replaces.
  - **Screenshots.** Add a screenshot of the panel if the entries around it have screenshots (`ls docs/screenshots`).
  - **Contradictions.** Read the manual's existing claims about chapters and "left out" in the draft section, and fix any that tiers make untrue.

- [ ] **Step 9: Run the checks.** Run `bun run test && bun run lint && bun run typecheck && bun run check:build`. Expected: all pass.

- [ ] **Step 10: Look at it.** Run `bun run dev` on Iceland and do each of the following:
  - Set one event to Skip. Its tiles dim, the panel says "Skip (you)", and the photo count drops.
  - Press **+** on one of its photos. That tile lights up.
  - Set a Featured event with a floor of 9. The panel shows ≥ 9 placed.
  - Press Update. The dialog appears, and Cancel changes nothing.

  Screenshot the header control at the narrowest window width. Check that the control does not push the count off the right edge, and that the last event's panel row is not clipped. Say what you checked.

- [ ] **Step 11: Commit.**

```bash
git add app/components/EventTierControl.vue app/components/EventsPanel.vue app/components/ContactSheet.vue app/components/SelectPhotos.vue app/components/GenerateBook.vue app/types/book.ts app/types/sheet.ts tests/book.test.ts tests/sheet.test.ts docs/manual.md <screenshots by path>
git commit -m "feat(draft): event tier control, events panel and a confirmation before Update

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 12: Algorithmic checks, sensitivity and the status record

**Files:**
- Modify: `src-tauri/examples/book_report.rs`. Add the `--scenarios`, `--stability` and `--sensitivity` modes, plus tier columns in the default mode.
- Modify: `src-tauri/src/book/events.rs`. Make the constants overridable for the sweep through a `pub struct Tuning` with `Default` equal to today's constants. `suggest_with(stats, capacity, &Tuning)` holds the logic, and `suggest` calls it with `Tuning::default()`.
- Modify: `docs/PROJECT-STATUS.md` and spec §9.

**Interfaces:**
- Produces:
  ```rust
  pub struct Tuning { pub w_engagement: f64, pub w_quality: f64, pub w_novelty: f64,
      pub utility_skip: f64, pub featured_margin: f64, pub normal_slot_share: f64,
      pub novelty_km: f64, pub similar_below: f64 }
  pub fn suggest_with(stats: &[EventStats], capacity: &Capacity, t: &Tuning) -> Vec<Suggestion>;
  ```

- [ ] **Step 1: Write the failing test for the tuning refactor.**

```rust
#[test]
fn suggest_is_suggest_with_default_tuning() {
    // Reuse the fixture from events_beyond_the_room_are_brief_and_a_standout_is_featured.
    let st = stats(&photos, &keepers(&photos));
    assert_eq!(suggest(&st, &cap), suggest_with(&st, &cap, &Tuning::default()));
    let mut t = Tuning::default();
    t.normal_slot_share = 0.3;
    assert_ne!(suggest(&st, &cap), suggest_with(&st, &cap, &t), "the tuning is actually read");
}
```

- [ ] **Step 2: Run the test and confirm it fails.** Implement: move `suggest`'s body into `suggest_with`, and replace every constant it reads (including those read inside `similarity` and `normal_room`) with the `t.` field. Pass `t` down, keep the `pub const`s as the `Default` values, and make `suggest` a one-line call. Run the test and confirm it passes.

- [ ] **Step 3: Add the scenario fixtures** to the report (§9 check 1). Build them from **real cached records**:
  1. Load project 5 (Iceland).
  2. Take real `Photo`s from it.
  3. Re-time and re-tag copies into five synthetic trips, each with its expected tiers:

  | Scenario | Construction | Expect |
  |---|---|---|
  | screenshots | 40 real photos re-timed into one event with `is_utility = true` on 36, plus 3 normal events | Skipped |
  | pool days | 3 events on 3 days, the same tags, GPS within 500 m (copy one event's location), plus 6 distinct events, 20 pages | exactly one pool event Normal/Featured, the others Brief |
  | dinner | 8 low-aesthetic photos between two 300-photo high-aesthetic days | ≥ Brief at 20, Normal at 40 |
  | undated | 30 photos with `captured_at = None` | Brief |
  | standout | one event with 2× the moments of any other and the top aesthetics | Featured |

  Print `PASS`/`FAIL` per scenario and length, and exit non-zero on any `FAIL`.

- [ ] **Step 4: Add stability (§9 check 3).** For each real project:
  - Tiers are identical across the 3 seeds. This is trivially true, since `suggest` takes no seed. Assert it anyway, as a guard against a future change.
  - Removing each of 20 photos, chosen deterministically (every `len/20`-th), changes the tier of at most the removed photo's own event.
  - 20 → 40 pages never lowers a suggested tier.

  Print the violations, and exit non-zero on any.

- [ ] **Step 5: Add sensitivity (§9 check 4).** For each `Tuning` field and each factor in {0.5, 0.9, 1.1, 1.5}, re-run the scenarios and print which ones flip. Summarise per field:
  - "not load-bearing": no flip at ±50%;
  - "fragile": a flip at ±10%;
  - "ok": neither.

- [ ] **Step 6: Add tier columns to the default mode.** The default mode now calls `assemble_with` with the empty map. Add `featured,normal,brief,skipped,floor_misses` columns, computed with `events::plan` + `events::budget` at the same capacity.

- [ ] **Step 7: Run everything and act on it.** Run `cargo run --release --example book_report -- --scenarios 5`, then `--stability 4 5 11`, then `--sensitivity 5`, then the default mode on `4 5 11` with and without `--places`.
  - If a scenario FAILs, fix the **engine** or re-derive a constant **only** from the sensitivity output, and record which one and why in spec §9. Do not tune a constant to make one fixture pass unless the sensitivity sweep supports the new value.
  - A "not load-bearing" constant is removed, or fixed to a round value, in the same commit, with the evidence.

- [ ] **Step 8: Record the results.** Add to `docs/PROJECT-STATUS.md`, in the style of its existing measured sections:
  - baseline vs after, per project: events at 0, Gini, blank pages;
  - scenario results;
  - stability violations (expected 0);
  - the sensitivity summary;
  - "Hand-tiering (secondary, §9): not yet done";
  - the four planning deviations listed at the top of this plan.

  Update the spec's **Status** line to "implemented; constants measured on <date>".

- [ ] **Step 9: Run the final gate.** Run `bun run test && bun run test:rust && bun run test:swift && bun run lint && bun run typecheck && bun run check:build`. Expected: all pass.

- [ ] **Step 10: Commit.**

```bash
git add src-tauri/src/book/events.rs src-tauri/examples/book_report.rs docs/PROJECT-STATUS.md docs/superpowers/specs/2026-09-23-event-tiers-design.md
git commit -m "test(events): scenario, stability and sensitivity checks for tier suggestions

<summary of results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

## Spec coverage

| Spec | Task |
|---|---|
| §1 problem, §9 step 1 report and baseline | 1 |
| §2 tiers, floors, weights, Brief cap, Skipped, Include wins | 4, 6 |
| §2 Brief fold of any size | 7 |
| §2.1 per-project floor/cap, validator, steppers | 3, 11 |
| §3 merit, §3.1 novelty and Look risk | 2, 5 |
| §4 suggest and reasons, normal_room, Featured margin | 5 |
| §5 budget steps 1-5, prefix ranking, invariant | 6, 7 |
| §6 hash storage, resolution, drafts, projects, wire | 4, 9, 10 |
| §7 header control, events panel, events_by_tier, dimming fix, Update confirm | 8, 10, 11 |
| §9 checks 1-4, mutation table | 12, with the mutation rows spread across 3-9 |
| §11 rulings | Global Constraints, 3, 6, 10 |

Where each row of the spec's §9 mutation table is checked:
- budget floors: Task 6
- sqrt: Task 6
- Skipped: Task 6
- brief_cap: Task 6
- 2-photo Brief fold: Task 7
- featured_floor from options: Task 6
- Include in Skipped: Task 6
- majority resolution: Task 4
- utility: Task 5
- novelty: Task 5
- output invariant: Task 7
