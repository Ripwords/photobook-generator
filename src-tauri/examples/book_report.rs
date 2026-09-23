//! Measures how a book spreads its photos across the events of a trip.
//!
//! ```sh
//! bun run sidecar
//! cd src-tauri && cargo run --release --example book_report -- 9 10 12
//! cd src-tauri && cargo run --release --example book_report -- --places 10
//! ```
//!
//! `--novelty` switches to calibrating the Look similarity component
//! against GPS-derived same-place/different-place labels instead of
//! running the book loop:
//!
//! ```sh
//! cd src-tauri && cargo run --release --example book_report -- --novelty 9 10 12
//! cd src-tauri && cargo run --release --example book_report -- --novelty --places 9 10 12
//! ```
//!
//! The tier checks of spec §9, each exiting non-zero on a failure:
//!
//! ```sh
//! cd src-tauri && cargo run --release --example book_report -- --scenarios 9    # check 1
//! cd src-tauri && cargo run --release --example book_report -- --stability 9 10 12  # check 3
//! cd src-tauri && cargo run --release --example book_report -- --sensitivity 9  # check 4
//! ```
//!
//! `--scenarios` and `--sensitivity` build five synthetic trips out of the
//! first project's real cached records (re-timed and re-tagged copies) and
//! check the suggested tiers against answers that are not in dispute.
//!
//! Arguments are project ids in the app's database. `--db PATH` points at
//! another database. The database is COPIED to a temp dir first, because
//! `Db::open` migrates, and a report must never write to the user's data.
//! Only derived feature records are read; no image is opened.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use app_lib::book::chapter::LatLon;
use app_lib::book::cull::{cull, Overrides, Photo};
use app_lib::book::events::{self, gini, EventTiers, Suggestion, Tier, Tuning};
use app_lib::book::pace::{assemble_with, Book, BookOptions};
use app_lib::book::pack::Capacity;
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
    let mut mode = "books";
    let mut projects: Vec<i64> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => db_path = PathBuf::from(args.next().expect("--db PATH")),
            "--places" => places = true,
            "--novelty" => mode = "novelty",
            "--scenarios" => mode = "scenarios",
            "--stability" => mode = "stability",
            "--sensitivity" => mode = "sensitivity",
            id => projects.push(id.parse().expect("project id")),
        }
    }
    let db = Db::open(&copy_db(&db_path)).expect("open copied database");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
    let lib = Library::load(&root).expect("library");
    let weights = Weights::load(&root.join("weights.json")).expect("weights");
    let spec = PrintSpec::pixajoy();
    let load = |id: i64| {
        let photos = load_project_photos(&db, id).expect("load project");
        if places { with_places(&photos) } else { photos }
    };

    match mode {
        "novelty" => novelty(&projects, &load),
        "scenarios" => {
            let donor = load(*projects.first().expect("--scenarios PROJECT"));
            let scenarios = scenarios(&donor);
            let mut failed = 0;
            println!("scenario,pages,check,result,tiers");
            for sc in &scenarios {
                for r in run_scenario(sc, &lib, &Tuning::default()) {
                    failed += usize::from(!r.pass);
                    println!("{},{},{},{},{}", sc.name, r.pages, r.label, if r.pass { "PASS" } else { "FAIL" }, r.tiers);
                }
            }
            if failed > 0 {
                eprintln!("{failed} scenario check(s) FAILED");
                std::process::exit(1);
            }
        }
        "stability" => {
            let mut violations = 0;
            for &id in &projects {
                violations += stability(id, &load(id), &lib);
            }
            println!("stability violations: {violations}");
            if violations > 0 {
                std::process::exit(1);
            }
        }
        "sensitivity" => sensitivity(&scenarios(&load(*projects.first().expect("--sensitivity PROJECT"))), &lib),
        _ => books(&projects, &load, &spec, &lib, &weights),
    }
}

fn books(projects: &[i64], load: &dyn Fn(i64) -> Vec<Photo>, spec: &PrintSpec, lib: &Library, weights: &Weights) {
    println!(
        "project,pages,seed,photos,events,events_at_zero,gini,min_chapter,blank_pages,dropped,featured,normal,brief,skipped,floor_misses"
    );
    let overrides = Overrides::new();
    let options = BookOptions::default();
    for &id in projects {
        let photos = load(id);
        let kept = cull(&photos, &overrides);
        for pages in PAGES {
            // The same plan and budget `assemble_with` deals from, at the same
            // capacity, so the tier columns describe the book beside them.
            let cap = Capacity::from_library(pages, lib);
            let plan = events::plan(&photos, &kept, &EventTiers::new(), &cap);
            let budget = events::budget(&kept, plan, &cap, &overrides, &options);
            let tier_count = |t: Tier| budget.plan.iter().filter(|p| p.tier == t).count();
            for seed in SEEDS {
                let book = match assemble_with(
                    spec,
                    &photos,
                    pages,
                    lib,
                    weights,
                    seed,
                    &overrides,
                    &EventTiers::new(),
                    options,
                ) {
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
                // Measured on the PRINTED pages, so a photo the layout stage
                // drops after the packed-group check (R10) shows here.
                let floor_misses = budget
                    .plan
                    .iter()
                    .filter(|p| matches!(p.tier, Tier::Featured | Tier::Normal) && budget.overflow.is_none())
                    .filter(|p| per.get(&p.event).copied().unwrap_or(0) < budget.floors.get(&p.event).copied().unwrap_or(0))
                    .count();
                println!(
                    "{id},{pages},{seed},{},{},{zero},{:.3},{min_chapter},{blank},{},{},{},{},{},{floor_misses}",
                    photos.len(),
                    per.len(),
                    gini(&counts),
                    book.dropped,
                    tier_count(Tier::Featured),
                    tier_count(Tier::Normal),
                    tier_count(Tier::Brief),
                    tier_count(Tier::Skipped),
                );
            }
        }
    }
}

fn novelty(projects: &[i64], load: &dyn Fn(i64) -> Vec<Photo>) {
    let (mut same_all, mut diff_all) = (Vec::new(), Vec::new());
    for &id in projects {
        let photos = load(id);
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
            .filter_map(|(e, ps)| Some((*e, *centres.get(e)?, events::mean_print(ps)?)))
            .collect();
        let (mut same, mut diff) = (Vec::new(), Vec::new());
        for i in 0..events.len() {
            for j in i + 1..events.len() {
                let km = events[i].1.km_to(events[j].1);
                let d = events::print_distance(&events[i].2, &events[j].2);
                if km < 0.5 {
                    same.push(d)
                } else if km > 20.0 {
                    diff.push(d)
                }
            }
        }
        report_novelty(&format!("project {id}"), &same, &diff);
        same_all.extend(same);
        diff_all.extend(diff);
    }
    report_novelty("pooled", &same_all, &diff_all);
}

// ---------------------------------------------------------------------------
// §9 check 1: scenario fixtures with known answers.

/// One expectation of a scenario at one page length, over the suggested
/// tiers keyed by event id.
struct Check {
    pages: u32,
    label: &'static str,
    pass: fn(&BTreeMap<u32, Tier>) -> bool,
}

struct Scenario {
    name: &'static str,
    photos: Vec<Photo>,
    checks: Vec<Check>,
}

struct Outcome {
    pages: u32,
    label: &'static str,
    pass: bool,
    tiers: String,
}

/// Synthetic event ids the checks below name.
const SUBJECT: u32 = 100;
const POOL: [u32; 3] = [100, 101, 102];

/// A day in the synthetic trips: 2023-11-14 09:00 UTC plus `day` days.
fn day_start(day: i64) -> i64 {
    1_700_000_000 - 1_700_000_000 % 86_400 + day * 86_400 + 9 * 3_600
}

/// Copies of real photos as synthetic event `id`, starting at `start`. Their
/// own spacing in time is kept, so moments stay real-shaped. Paths, hashes
/// and near-duplicate clusters are made unique per copy, so a photo reused in
/// two synthetic events never contests itself in `cull`.
fn copy_event(tag: &str, src: &[&Photo], id: u32, start: i64) -> Vec<Photo> {
    let first = src.iter().filter_map(|p| p.captured_at).min().unwrap_or(0);
    src.iter()
        .enumerate()
        .map(|(i, p)| {
            assert!(p.near_dup_cluster < 100_000, "near_dup_cluster {} too large to offset", p.near_dup_cluster);
            Photo {
                path: format!("/scenario/{tag}/e{id}/{i:04}"),
                hash: format!("{tag}-{id}-{i}"),
                event_cluster: id,
                near_dup_cluster: id * 100_000 + p.near_dup_cluster,
                captured_at: Some(start + p.captured_at.unwrap_or(first) - first),
                location: None,
                ..(*p).clone()
            }
        })
        .collect()
}

fn located(mut photos: Vec<Photo>, lat: f64, lon: f64) -> Vec<Photo> {
    for p in &mut photos {
        p.location = LatLon::new(lat, lon);
    }
    photos
}

/// The five trips of §9 check 1, built from `donor`'s real cached records.
/// Every choice of source event is made by a rule on the real figures, stated
/// beside it, rather than by event id, so the fixture does not quietly depend
/// on which events a re-analysis happens to number how.
fn scenarios(donor: &[Photo]) -> Vec<Scenario> {
    let kept = cull(donor, &Overrides::new());
    let st = events::stats(donor, &kept);
    let mut by_event: BTreeMap<u32, Vec<&Photo>> = BTreeMap::new();
    for p in donor {
        by_event.entry(p.event_cluster).or_default().push(p);
    }
    for v in by_event.values_mut() {
        v.sort_by_key(|p| (p.captured_at, p.path.clone()));
    }
    // Dated events, most cull keepers first.
    let mut ranked: Vec<&events::EventStats> = st.iter().filter(|s| !s.undated && s.keepers > 0).collect();
    ranked.sort_by(|a, b| b.keepers.cmp(&a.keepers).then(a.event.cmp(&b.event)));
    let src = |rank: usize| by_event[&ranked[rank].event].as_slice();
    let first = |rank: usize, n: usize| &src(rank)[..n.min(src(rank).len())];
    let mut out = Vec::new();

    // 1. screenshots: 40 real photos (the first 40 of the 4th-largest event)
    //    re-timed into one event with `is_utility` on 36 of them, between
    //    three ordinary days (the three largest events).
    {
        let mut shots = copy_event("screenshots", first(3, 40), SUBJECT, day_start(1));
        for (i, p) in shots.iter_mut().enumerate() {
            p.is_utility = i < 36;
        }
        let mut photos = copy_event("screenshots", src(0), 1, day_start(0));
        photos.extend(shots);
        photos.extend(copy_event("screenshots", src(1), 2, day_start(2)));
        photos.extend(copy_event("screenshots", src(2), 3, day_start(3)));
        let skipped: fn(&BTreeMap<u32, Tier>) -> bool = |t| t[&SUBJECT] == Tier::Skipped;
        out.push(Scenario {
            name: "screenshots",
            photos,
            checks: PAGES.iter().map(|&pages| Check { pages, label: "event 100 Skipped", pass: skipped }).collect(),
        });
    }

    // 2. pool days: the 4th-largest of the seven largest events (their
    //    median) repeated on three days, same tags by construction and
    //    placed within 500 m of each other, plus the other six of the seven
    //    as distinct events, each at its own place 55 km from the next.
    {
        let mut photos = Vec::new();
        for (k, &e) in POOL.iter().enumerate() {
            photos.extend(located(
                copy_event("pool", src(3), e, day_start(2 * k as i64)),
                64.1466 + 0.002 * k as f64, // ~220 m apart: pool days span ~450 m
                -21.9426,
            ));
        }
        for (k, rank) in [0, 1, 2, 4, 5, 6].into_iter().enumerate() {
            photos.extend(located(
                copy_event("pool", src(rank), 1 + k as u32, day_start(2 * k as i64 + 1)),
                63.5 + 0.5 * k as f64,
                -19.0,
            ));
        }
        let one_pool: fn(&BTreeMap<u32, Tier>) -> bool = |t| {
            let up = POOL.iter().filter(|e| matches!(t[e], Tier::Normal | Tier::Featured)).count();
            let brief = POOL.iter().filter(|e| t[e] == Tier::Brief).count();
            up == 1 && brief == 2
        };
        out.push(Scenario {
            name: "pool days",
            photos,
            checks: vec![Check { pages: 20, label: "one pool day Normal/Featured, two Brief", pass: one_pool }],
        });
    }

    // 3. dinner: the 8 lowest-aesthetic non-utility photos of the library as
    //    one evening between two 300-photo days (the first 300 photos of
    //    the two events of at least 300 photos with the best quality).
    {
        let mut big: Vec<&events::EventStats> = ranked.iter().copied().filter(|s| s.photos >= 300).collect();
        big.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(a.event.cmp(&b.event)));
        assert!(big.len() >= 2, "the donor needs two events of 300 photos for the dinner scenario");
        let mut dull: Vec<&Photo> = donor.iter().filter(|p| !p.is_utility && p.captured_at.is_some()).collect();
        dull.sort_by_key(|p| (p.aesthetic_pct, p.path.clone()));
        let mut dinner = copy_event("dinner", &dull[..8], SUBJECT, day_start(1) + 10 * 3_600);
        // Four moments of two frames, a minute apart, ten minutes between
        // moments; each frame a distinct shot, so all eight are keepers.
        for (i, p) in dinner.iter_mut().enumerate() {
            p.captured_at = Some(day_start(1) + 10 * 3_600 + (i / 2) as i64 * 600 + (i % 2) as i64 * 60);
            p.near_dup_cluster = SUBJECT * 100_000 + i as u32;
        }
        let mut photos = copy_event("dinner", &by_event[&big[0].event][..300], 1, day_start(0));
        photos.extend(dinner);
        photos.extend(copy_event("dinner", &by_event[&big[1].event][..300], 2, day_start(2)));
        out.push(Scenario {
            name: "dinner",
            photos,
            checks: vec![
                Check { pages: 20, label: "event 100 at least Brief", pass: |t| t[&SUBJECT] >= Tier::Brief },
                Check { pages: 40, label: "event 100 Normal", pass: |t| t[&SUBJECT] == Tier::Normal },
            ],
        });
    }

    // 4. undated: 30 non-utility photos of the 5th-largest event with their
    //    capture time removed, beside three dated days (the three largest).
    {
        let mut undated: Vec<Photo> =
            copy_event("undated", &src(4).iter().copied().filter(|p| !p.is_utility).take(30).collect::<Vec<_>>(), SUBJECT, 0);
        for p in &mut undated {
            p.captured_at = None;
        }
        assert_eq!(undated.len(), 30);
        let mut photos = copy_event("undated", src(0), 1, day_start(0));
        photos.extend(copy_event("undated", src(1), 2, day_start(1)));
        photos.extend(copy_event("undated", src(2), 3, day_start(2)));
        photos.extend(undated);
        let brief: fn(&BTreeMap<u32, Tier>) -> bool = |t| t[&SUBJECT] == Tier::Brief;
        out.push(Scenario {
            name: "undated",
            photos,
            checks: PAGES.iter().map(|&pages| Check { pages, label: "event 100 Brief", pass: brief }).collect(),
        });
    }

    // 5. standout: the best-quality event of at least 20 moments, beside
    //    every dated event with at most half its moments -- so it has 2x the
    //    moments of any other and the top aesthetics, both from real data.
    {
        let mut candidates: Vec<&events::EventStats> = ranked.iter().copied().filter(|s| s.moments >= 20).collect();
        candidates.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(b.moments.cmp(&a.moments)));
        let star = candidates[0];
        let others: Vec<&events::EventStats> =
            ranked.iter().copied().filter(|s| s.event != star.event && 2 * s.moments <= star.moments).collect();
        let mut photos = copy_event("standout", &by_event[&star.event], SUBJECT, day_start(0));
        for (k, o) in others.iter().enumerate() {
            photos.extend(copy_event("standout", &by_event[&o.event], 1 + k as u32, day_start(1 + k as i64)));
        }
        let featured: fn(&BTreeMap<u32, Tier>) -> bool = |t| t[&SUBJECT] == Tier::Featured;
        out.push(Scenario {
            name: "standout",
            photos,
            checks: PAGES.iter().map(|&pages| Check { pages, label: "event 100 Featured", pass: featured }).collect(),
        });
    }
    out
}

fn run_scenario(sc: &Scenario, lib: &Library, t: &Tuning) -> Vec<Outcome> {
    let kept = cull(&sc.photos, &Overrides::new());
    let st = events::stats(&sc.photos, &kept);
    sc.checks
        .iter()
        .map(|c| {
            let s: Vec<Suggestion> = events::suggest_with(&st, &Capacity::from_library(c.pages, lib), t);
            let tiers: BTreeMap<u32, Tier> = s.iter().map(|x| (x.event, x.tier)).collect();
            let shown: Vec<String> = s
                .iter()
                .zip(&st)
                .map(|(x, e)| format!("e{}={}:{:.3}(m{} q{:.2})", x.event, x.tier.as_str(), x.merit, e.moments, e.quality))
                .collect();
            Outcome { pages: c.pages, label: c.label, pass: (c.pass)(&tiers), tiers: shown.join(" ") }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// §9 check 3: stability.

/// Suggested and effective (after `budget`) tier of every event, with no
/// user choices, at `pages`.
fn tiers_of(photos: &[Photo], pages: u32, lib: &Library) -> [BTreeMap<u32, Tier>; 2] {
    let overrides = Overrides::new();
    let kept = cull(photos, &overrides);
    let cap = Capacity::from_library(pages, lib);
    let plan = events::plan(photos, &kept, &EventTiers::new(), &cap);
    let suggested = plan.iter().map(|p| (p.event, p.suggested)).collect();
    let budget = events::budget(&kept, plan, &cap, &overrides, &BookOptions::default());
    [suggested, budget.plan.iter().map(|p| (p.event, p.tier)).collect()]
}

const KINDS: [&str; 2] = ["suggested", "effective"];

/// Prints every violation of §9 check 3 on one library and returns how many.
fn stability(id: i64, photos: &[Photo], lib: &Library) -> usize {
    let mut violations = 0;
    let mut violation = |what: String| {
        println!("project {id}: VIOLATION {what}");
        violations += 1;
    };
    let at: BTreeMap<u32, [BTreeMap<u32, Tier>; 2]> = PAGES.iter().map(|&p| (p, tiers_of(photos, p, lib))).collect();

    // Seeds: nothing in `plan` or `budget` takes a seed, so this holds by
    // construction today. It is re-derived per seed anyway, as a guard for the
    // day a seed reaches the tier path.
    for seed in SEEDS {
        for &pages in &PAGES {
            if tiers_of(photos, pages, lib) != at[&pages] {
                violation(format!("seed {seed} at {pages} pages gives different tiers"));
            }
        }
    }

    // Removing one photo moves at most its own event's tier. 20 photos,
    // every len/20-th, chosen deterministically.
    let (mut removed, mut own_moved) = (0, 0);
    for k in 0..20 {
        let idx = k * photos.len() / 20;
        let own = photos[idx].event_cluster;
        let without: Vec<Photo> = photos.iter().enumerate().filter(|&(i, _)| i != idx).map(|(_, p)| p.clone()).collect();
        for &pages in &PAGES {
            let after = tiers_of(&without, pages, lib);
            for (kind, (before, after)) in KINDS.iter().zip(at[&pages].iter().zip(&after)) {
                for (e, t) in before {
                    // An event whose only photo was removed is gone, not moved.
                    let Some(now) = after.get(e) else { continue };
                    if *e == own && now != t {
                        own_moved += 1;
                    }
                    if *e != own && now != t {
                        violation(format!(
                            "removing photo {idx} (event {own}) at {pages} pages moves {kind} event {e} {} -> {}",
                            t.as_str(),
                            now.as_str()
                        ));
                    }
                }
            }
        }
        removed += 1;
    }

    // 20 -> 40 pages never lowers a tier.
    let mut lowered = 0;
    for (kind, (short, long)) in KINDS.iter().zip(at[&20].iter().zip(&at[&40])) {
        for (e, t) in short {
            if long[e] < *t {
                lowered += 1;
                violation(format!("{kind} event {e} is {} at 20 pages but {} at 40", t.as_str(), long[e].as_str()));
            }
        }
    }
    let summary = |m: &BTreeMap<u32, Tier>| {
        let n = |t: Tier| m.values().filter(|&&x| x == t).count();
        format!("F{} N{} B{} S{}", n(Tier::Featured), n(Tier::Normal), n(Tier::Brief), n(Tier::Skipped))
    };
    println!(
        "project {id}: {} events; suggested 20p {} 40p {}; effective 20p {} 40p {}; {removed} removals checked ({own_moved} moved their own event); {lowered} lowered at 40",
        at[&20][0].len(),
        summary(&at[&20][0]),
        summary(&at[&40][0]),
        summary(&at[&20][1]),
        summary(&at[&40][1]),
    );
    violations
}

// ---------------------------------------------------------------------------
// §9 check 4: sensitivity.

type Field = (&'static str, for<'a> fn(&'a mut Tuning) -> &'a mut f64);

const FIELDS: [Field; 8] = [
    ("w_engagement", |t| &mut t.w_engagement),
    ("w_quality", |t| &mut t.w_quality),
    ("w_novelty", |t| &mut t.w_novelty),
    ("utility_skip", |t| &mut t.utility_skip),
    ("featured_margin", |t| &mut t.featured_margin),
    ("normal_slot_share", |t| &mut t.normal_slot_share),
    ("novelty_km", |t| &mut t.novelty_km),
    ("similar_below", |t| &mut t.similar_below),
];
const FACTORS: [f64; 4] = [0.5, 0.9, 1.1, 1.5];

fn sensitivity(scenarios: &[Scenario], lib: &Library) {
    let verdicts = |t: &Tuning| -> Vec<(String, bool)> {
        scenarios
            .iter()
            .flat_map(|sc| {
                run_scenario(sc, lib, t).into_iter().map(move |r| (format!("{}@{}", sc.name, r.pages), r.pass))
            })
            .collect()
    };
    let base = verdicts(&Tuning::default());
    println!(
        "baseline: {}",
        base.iter().map(|(n, p)| format!("{n}={}", if *p { "PASS" } else { "FAIL" })).collect::<Vec<_>>().join(" ")
    );
    println!("field,factor,value,flips");
    let mut summary = Vec::new();
    for (name, field) in FIELDS {
        let mut flipped: BTreeMap<u64, usize> = BTreeMap::new();
        for f in FACTORS {
            let mut t = Tuning::default();
            *field(&mut t) *= f;
            let value = *field(&mut t);
            let flips: Vec<String> = verdicts(&t)
                .into_iter()
                .zip(&base)
                .filter(|((_, now), (_, was))| now != was)
                .map(|((n, now), _)| format!("{n}->{}", if now { "PASS" } else { "FAIL" }))
                .collect();
            flipped.insert(f.to_bits(), flips.len());
            println!("{name},{f},{value:.3},{}", if flips.is_empty() { "-".to_string() } else { flips.join(" ") });
        }
        let at = |f: f64| flipped[&f.to_bits()];
        let verdict = if at(0.9) + at(1.1) > 0 {
            "fragile"
        } else if at(0.5) + at(1.5) == 0 {
            "not load-bearing"
        } else {
            "ok"
        };
        summary.push(format!("{name}: {verdict}"));
    }
    println!("summary: {}", summary.join("; "));
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn report_novelty(label: &str, same: &[f64], diff: &[f64]) {
    let (mut s, mut d) = (same.to_vec(), diff.to_vec());
    println!(
        "{label}: same n={} p10/50/90={:.3}/{:.3}/{:.3}  different n={} p10/50/90={:.3}/{:.3}/{:.3}  auc={:.3}",
        s.len(),
        pct(&mut s, 0.1),
        pct(&mut s, 0.5),
        pct(&mut s, 0.9),
        d.len(),
        pct(&mut d, 0.1),
        pct(&mut d, 0.5),
        pct(&mut d, 0.9),
        app_lib::book::events::auc(same, diff)
    );
}
