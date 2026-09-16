//! Whole-library sweep of `pace::assemble`: how many photos reach the page,
//! how many are lost, and how many printed pages come out white, across the
//! keeper counts a 20-page book is actually generated at.
//!
//! This is the instrument every packer change is measured with. It exists
//! because `pack`'s contract has now been changed four times and every change
//! surfaced a defect that only a sweep like this caught -- never a unit test.
//! Run it with output to see the table:
//!
//! ```sh
//! cargo test --manifest-path src-tauri/Cargo.toml --test pack_sweep -- --nocapture
//! ```
//!
//! The assertions at the bottom are the contract the sweep guards. They are
//! deliberately over the WHOLE range, not a single count, because both known
//! packer defects (`docs/PROJECT-STATUS.md` items 12 and 14) were invisible at
//! the counts the fixtures happened to use.

use app_lib::book::cull::{Overrides, Photo};
use app_lib::book::pace::{assemble, Book, BLANK_TEMPLATE_ID};
use app_lib::book::pack::{Buildable, Capacity};
use app_lib::templates::{Library, Weights};
use std::path::Path;

const PAGES: u32 = 20;
const SEEDS: [u64; 3] = [7, 1234, 99];

fn real_library() -> Library {
    Library::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates")).expect("library")
}

fn real_weights() -> Weights {
    Weights::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates/weights.json"))
        .expect("weights")
}

/// One keeper: no utility flag, its own near-dup cluster, no faces and no
/// saliency so no template can hard-reject it. Aspects alternate so the
/// scorer has something to prefer; aesthetics are distinct so the capacity
/// trim is total.
fn photo(i: usize, event: u32) -> Photo {
    let portrait = i % 5 == 3;
    Photo {
        path: format!("/sweep/p{i:03}.jpg"),
        hash: format!("h{i:03}"),
        width: if portrait { 3000 } else { 4000 },
        height: if portrait { 4000 } else { 3000 },
        is_utility: false,
        aesthetic_pct: ((i * 37) % 100) as u8,
        sharpness_pct: ((i * 53) % 100) as u8,
        near_dup_cluster: i as u32,
        event_cluster: event,
        faces: Vec::new(),
        face_area_fraction: 0.0,
        saliency_box: None,
        palette: Vec::new(),
        capture_quality: None,
        scene_tags: Vec::new(),
        captured_at: Some(1_700_000_000 + (event as i64) * 86_400 + (i as i64) * 60),
    }
}

/// How `n` keepers are divided into chapters. Each shape is a real failure
/// mode seen in the field, not a random partition.
#[derive(Clone, Copy)]
enum Shape {
    /// Everything shot in one sitting.
    One,
    /// A holiday: one chapter per day, eight photos a day, the tail shorter.
    Eights,
    /// Uneven days: 3, 4, 6, 8, 12, then repeat.
    Uneven,
    /// Many tiny events of two or three photos each.
    Tiny,
}

impl Shape {
    const ALL: [Shape; 4] = [Shape::One, Shape::Eights, Shape::Uneven, Shape::Tiny];

    fn name(self) -> &'static str {
        match self {
            Shape::One => "one",
            Shape::Eights => "eights",
            Shape::Uneven => "uneven",
            Shape::Tiny => "tiny",
        }
    }

    fn photos(self, n: usize) -> Vec<Photo> {
        let mut out = Vec::with_capacity(n);
        let mut event = 0u32;
        let mut in_chapter = 0usize;
        let mut chapter_len = self.chapter_len(0);
        for i in 0..n {
            if in_chapter == chapter_len {
                event += 1;
                in_chapter = 0;
                chapter_len = self.chapter_len(event as usize);
            }
            out.push(photo(i, event));
            in_chapter += 1;
        }
        out
    }

    fn chapter_len(self, chapter: usize) -> usize {
        match self {
            Shape::One => usize::MAX,
            Shape::Eights => 8,
            Shape::Uneven => [3, 4, 6, 8, 12][chapter % 5],
            Shape::Tiny => [2, 3][chapter % 2],
        }
    }
}

struct Outcome {
    placed: usize,
    blank_pages: usize,
    /// Pages naming a real template and holding nothing -- must always be 0.
    hollow_pages: usize,
}

fn measure(book: &Book) -> Outcome {
    let placed: std::collections::BTreeSet<usize> = book
        .pages
        .iter()
        .flat_map(|p| p.placements.iter().map(|pl| pl.photo_index))
        .collect();
    Outcome {
        placed: placed.len(),
        blank_pages: book
            .pages
            .iter()
            .filter(|p| p.template_id == BLANK_TEMPLATE_ID)
            .count(),
        hollow_pages: book
            .pages
            .iter()
            .filter(|p| p.template_id != BLANK_TEMPLATE_ID && p.placements.is_empty())
            .count(),
    }
}

#[test]
fn pack_sweep_places_every_keeper_the_book_can_hold_and_blanks_only_when_photos_run_out() {
    let lib = real_library();
    let w = real_weights();
    let cap = Capacity::from_library(PAGES, &lib);
    let buildable = Buildable::from_library(&lib);
    let slots = (cap.spreads + cap.singles) as usize;
    // The fewest photos it takes to give every slot its smallest group.
    let min_full: usize = (0..slots).map(|i| buildable.bounds_at(i, slots).0).sum();

    let mut total_lost = 0usize;
    let mut total_blank = 0usize;
    let mut total_hollow = 0usize;
    let mut lost_under_capacity: Vec<String> = Vec::new();
    let mut blank_while_full: Vec<String> = Vec::new();

    println!(
        "{:>8} {:>7} {:>5} | {:>6} {:>4} {:>5} {:>6}",
        "shape", "keepers", "seed", "placed", "lost", "blank", "hollow"
    );
    for shape in Shape::ALL {
        for n in 10..=cap.max_photos + 2 {
            for seed in SEEDS {
                let photos = shape.photos(n);
                let book = assemble(&photos, PAGES, &lib, &w, seed, &Overrides::default())
                    .expect("no overrides, so nothing can refuse");
                let o = measure(&book);
                let lost = n.saturating_sub(o.placed);
                total_lost += lost;
                total_blank += o.blank_pages;
                total_hollow += o.hollow_pages;
                if lost > 0 && n <= cap.max_photos {
                    lost_under_capacity
                        .push(format!("{}/{n}/seed{seed}: lost {lost}", shape.name()));
                }
                if o.blank_pages > 0 && n >= min_full {
                    blank_while_full.push(format!(
                        "{}/{n}/seed{seed}: {} blank pages",
                        shape.name(),
                        o.blank_pages
                    ));
                }
                println!(
                    "{:>8} {:>7} {:>5} | {:>6} {:>4} {:>5} {:>6}",
                    shape.name(),
                    n,
                    seed,
                    o.placed,
                    lost,
                    o.blank_pages,
                    o.hollow_pages
                );
            }
        }
    }
    println!(
        "capacity {} | total lost {total_lost} | total blank pages {total_blank} | hollow {total_hollow}",
        cap.max_photos
    );

    assert_eq!(
        total_hollow, 0,
        "a page naming a real template must hold a photo"
    );
    assert!(
        lost_under_capacity.is_empty(),
        "photos lost below the book's own stated capacity ({}):\n{}",
        cap.max_photos,
        lost_under_capacity.join("\n")
    );
    assert!(
        blank_while_full.is_empty(),
        "blank pages with enough photos to fill every slot:\n{}",
        blank_while_full.join("\n")
    );
    // Measured, not derived: the blank pages left when there are fewer
    // photos than slots, over this sweep. Every one of them is a page the
    // photos could not fill. A change that lowers the figure should lower
    // this bound with it; one that raises it has front-loaded a short book.
    assert!(
        total_blank <= 840,
        "blank pages rose to {total_blank}: a short book is being front-loaded"
    );
}
