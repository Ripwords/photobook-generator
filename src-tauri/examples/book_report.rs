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
    let mut novelty = false;
    let mut projects: Vec<i64> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => db_path = PathBuf::from(args.next().expect("--db PATH")),
            "--places" => places = true,
            "--novelty" => novelty = true,
            id => projects.push(id.parse().expect("project id")),
        }
    }
    let db = Db::open(&copy_db(&db_path)).expect("open copied database");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
    let lib = Library::load(&root).expect("library");
    let weights = Weights::load(&root.join("weights.json")).expect("weights");
    let spec = PrintSpec::pixajoy();

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
        return;
    }

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
