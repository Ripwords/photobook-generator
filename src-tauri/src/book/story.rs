//! What a photo does in a story, and how well a book tells one.
//!
//! A chapter that reads well opens wide (where are we?), meets the people,
//! looks closely at something, and settles at the end. `shot_role` names
//! which of those a photo can be; `story_metrics` measures how often a
//! finished book lands them. Both are pure, so the engine and the
//! `book_report` example share one definition.
//!
//! The tag sets come from the real library's tag counts (3998 photos).
//! `outdoor`, `sky` and `structure` are left out on purpose: they sit on
//! 57-76% of photos, so they say nothing about the shot.

use crate::book::cull::Photo;
use crate::book::pace::Book;
use std::collections::{BTreeMap, BTreeSet};

/// What a photo can do in a chapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShotRole {
    /// A wide scene that says where we are.
    Establishing,
    /// Someone you can recognise.
    People,
    /// A close look at one thing: food, a sign, a flower.
    Detail,
    /// Everything else.
    Moment,
}

impl ShotRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Establishing => "establishing",
            Self::People => "people",
            Self::Detail => "detail",
            Self::Moment => "moment",
        }
    }
}

/// A face this large is a portrait whatever the tags say. Measured: people
/// standing in a landscape sit around 0.015.
pub const PEOPLE_FACE_AREA: f64 = 0.02;
/// Below this, a face is a passer-by, even with a `people` tag.
pub const MIN_PEOPLE_FACE_AREA: f64 = 0.005;

const ESTABLISHING_TAGS: &[&str] = &[
    "land", "hill", "mountain", "water_body", "iceberg", "waterfall", "aurora", "glacier",
    "lake", "ocean", "cliff", "forest", "rainbow", "sunset_sunrise", "skyscraper", "haze",
    "beach", "shore", "coast", "desert", "canyon", "valley", "cityscape", "skyline", "volcano",
    "field",
];

const PEOPLE_TAGS: &[&str] = &["people", "adult", "child", "crowd"];

const DETAIL_TAGS: &[&str] = &[
    "food", "utensil", "tableware", "plate", "bowl", "drink", "drinking_glass", "baked_goods",
    "dessert", "bread", "fruit", "vegetable", "berry", "mushroom", "flower", "petunia", "moss",
    "spiderweb", "spider", "insect", "dragonfly", "arthropods", "arachnid", "sign",
    "billboards", "document", "art", "statue", "figurine", "toy", "stuffed_animals",
    "decoration", "bonsai", "decorative_plant", "seasonings", "herb", "spoon",
];

fn has_any(photo: &Photo, set: &[&str]) -> bool {
    photo.scene_tags.iter().any(|t| set.contains(&t.as_str()))
}

/// The role a photo can play. People win over everything (a face is the
/// subject), then Detail, unless the frame is also a landscape, then
/// Establishing.
pub fn shot_role(photo: &Photo) -> ShotRole {
    let face = photo.face_area_fraction;
    let people = !photo.faces.is_empty()
        && (face >= PEOPLE_FACE_AREA || (face >= MIN_PEOPLE_FACE_AREA && has_any(photo, PEOPLE_TAGS)));
    if people {
        return ShotRole::People;
    }
    let wide = has_any(photo, ESTABLISHING_TAGS);
    if has_any(photo, DETAIL_TAGS) && !wide {
        return ShotRole::Detail;
    }
    if wide {
        return ShotRole::Establishing;
    }
    ShotRole::Moment
}

/// Places after an event's best automatic pick that `cover_roles` may fill:
/// an event printed with three photos shows three different kinds of shot
/// when it has them.
pub const ROLE_WINDOW: usize = 2;

/// The roles a chapter owes, in the order they are promoted when there is
/// room for fewer than all.
const OWED: [ShotRole; 3] = [ShotRole::Establishing, ShotRole::People, ShotRole::Detail];

/// Reorders one event's ranked list so its first few picks cover the kinds
/// of shot the event has. `list` is `pack::EventOrder::list`: `included`
/// Includes, then the automatic picks best first, the first `distinct` of
/// them different pictures.
///
/// Only `list[included + 1..distinct]` moves, so the event's best pick, the
/// Includes and the deferred look-alikes stay where they were, and so does
/// `distinct`. A role missing from the first `included + 1 + ROLE_WINDOW`
/// places is filled from further down the distinct run, by its best photo
/// at or above `ROLE_FLOOR_PCT`; the window keeps its best picks in front
/// of the promoted ones. A budget still takes a prefix, so more room only
/// ever adds photos.
pub fn cover_roles(photos: &[Photo], list: &mut Vec<usize>, included: usize, distinct: usize) {
    let start = included + 1;
    let end = (start + ROLE_WINDOW).min(distinct);
    if end <= start || distinct <= end {
        return;
    }
    let present: BTreeSet<ShotRole> = list[..end].iter().map(|&i| shot_role(&photos[i])).collect();
    let mut promoted: Vec<usize> = Vec::new();
    for role in OWED {
        if promoted.len() == end - start {
            break;
        }
        if present.contains(&role) {
            continue;
        }
        let found = list[end..distinct]
            .iter()
            .copied()
            .find(|&i| shot_role(&photos[i]) == role && photos[i].aesthetic_pct >= ROLE_FLOOR_PCT);
        promoted.extend(found);
    }
    if promoted.is_empty() {
        return;
    }
    let keep = end - start - promoted.len();
    let mut middle: Vec<usize> = list[start..start + keep].to_vec();
    middle.extend(&promoted);
    middle.extend(list[start + keep..distinct].iter().copied().filter(|i| !promoted.contains(i)));
    list.splice(start..distinct, middle);
}

/// How well a photo closes a chapter, 0..1: a wide, warm, evenly exposed
/// frame settles; a clipped close-up does not. Night is not a signal --
/// half of one real library is aurora.
pub fn calm_score(photo: &Photo) -> f64 {
    let wide = if shot_role(photo) == ShotRole::Establishing { 1.0 } else { 0.0 };
    let warmth = photo.warmth.unwrap_or(0.5).clamp(0.0, 1.0);
    let clipped = (photo.clipped_low + photo.clipped_high).clamp(0.0, 1.0);
    0.5 * wide + 0.3 * warmth + 0.2 * (1.0 - clipped)
}

/// A photo counts toward what a chapter *could* show only if it is good
/// enough that the engine might reasonably pick it.
pub const ROLE_FLOOR_PCT: u8 = 40;

/// How a finished book reads as a story. Rates are over the chapters where
/// the question can be asked, `None` when there are none.
#[derive(Debug, Clone, PartialEq)]
pub struct StoryMetrics {
    /// Chapters printed: events, in the order they first appear.
    pub chapters: usize,
    /// Of chapters with 4+ photos that HAVE an establishing photo printed,
    /// the share that open on one.
    pub opener_hit: Option<f64>,
    /// Of chapters with 4+ photos, the share whose last photo is the
    /// calmest printed in that chapter.
    pub closer_hit: Option<f64>,
    /// Adjacent chapters (by first appearance) whose earliest instants run
    /// backwards. Must be 0.
    pub chrono_inv: usize,
    /// Neighbours in reading order from different events whose instants run
    /// backwards: a later chapter's photo printed before an earlier one's.
    pub event_inv: usize,
    /// Openings that show more than one event.
    pub mixed_spreads: usize,
    /// Mean over chapters of roles printed / roles available among that
    /// event's photos at or above `ROLE_FLOOR_PCT`.
    pub role_cov: Option<f64>,
}

/// Photo indices in reading order: page by page, then left to right, top
/// to bottom within a page.
pub fn reading_order(book: &Book) -> Vec<usize> {
    let mut out = Vec::new();
    for page in &book.pages {
        let mut placed: Vec<_> = page.placements.iter().collect();
        placed.sort_by(|a, b| {
            (a.slot_rect.x, a.slot_rect.y)
                .partial_cmp(&(b.slot_rect.x, b.slot_rect.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.extend(placed.iter().map(|p| p.photo_index));
    }
    out
}

fn rate(hits: usize, of: usize) -> Option<f64> {
    (of > 0).then(|| hits as f64 / of as f64)
}

/// Measure a book. `photos` is the list `Placement::photo_index` points into.
pub fn story_metrics(book: &Book, photos: &[Photo]) -> StoryMetrics {
    let order = reading_order(book);
    // Chapters in the order they first appear, each with its photos in
    // reading order. Grouped by event rather than split into runs, so two
    // events sharing a page do not read as four chapters.
    let mut runs: Vec<(u32, Vec<usize>)> = Vec::new();
    for &i in &order {
        let event = photos[i].event_cluster;
        match runs.iter_mut().find(|(e, _)| *e == event) {
            Some((_, members)) => members.push(i),
            None => runs.push((event, vec![i])),
        }
    }

    let (mut open_of, mut open_hit, mut close_of, mut close_hit) = (0, 0, 0, 0);
    let mut coverage = Vec::new();
    for (event, members) in &runs {
        if members.len() >= 4 {
            if members.iter().any(|&i| shot_role(&photos[i]) == ShotRole::Establishing) {
                open_of += 1;
                open_hit += usize::from(shot_role(&photos[members[0]]) == ShotRole::Establishing);
            }
            close_of += 1;
            let best = members.iter().map(|&i| calm_score(&photos[i])).fold(f64::MIN, f64::max);
            let last = calm_score(&photos[*members.last().expect("non-empty run")]);
            close_hit += usize::from(last >= best - 1e-9);
        }
        let available: BTreeSet<ShotRole> = photos
            .iter()
            .filter(|p| p.event_cluster == *event && p.aesthetic_pct >= ROLE_FLOOR_PCT)
            .map(shot_role)
            .collect();
        if !available.is_empty() {
            let printed: BTreeSet<ShotRole> = members.iter().map(|&i| shot_role(&photos[i])).collect();
            coverage.push(printed.intersection(&available).count() as f64 / available.len() as f64);
        }
    }

    let starts: Vec<i64> = runs
        .iter()
        .filter_map(|(_, m)| m.iter().filter_map(|&i| photos[i].instant()).min())
        .collect();
    let chrono_inv = starts.windows(2).filter(|w| w[1] < w[0]).count();
    let event_inv = order
        .windows(2)
        .filter(|w| photos[w[0]].event_cluster != photos[w[1]].event_cluster)
        .filter(|w| matches!((photos[w[0]].instant(), photos[w[1]].instant()), (Some(a), Some(b)) if b < a))
        .count();

    let mut by_opening: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
    for o in 0..crate::book::edit::opening_count(book) {
        let pages = crate::book::edit::opening_pages(book, o).unwrap_or_default();
        let events = by_opening.entry(o).or_default();
        for p in pages {
            events.extend(book.pages[p].placements.iter().map(|pl| photos[pl.photo_index].event_cluster));
        }
    }
    let mixed_spreads = by_opening.values().filter(|e| e.len() > 1).count();

    StoryMetrics {
        chapters: runs.len(),
        opener_hit: rate(open_hit, open_of),
        closer_hit: rate(close_hit, close_of),
        chrono_inv,
        event_inv,
        mixed_spreads,
        role_cov: (!coverage.is_empty()).then(|| coverage.iter().sum::<f64>() / coverage.len() as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::Face;
    use crate::geometry::Rect;

    fn photo(tags: &[&str]) -> Photo {
        Photo {
            path: "p".into(), hash: "h".into(), width: 4000, height: 3000,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: 0,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::new(), capture_quality: None,
            scene_tags: tags.iter().map(|t| t.to_string()).collect(), captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None, utc_offset: None, warmth: None, horizon_tilt_deg: None,
        }
    }

    fn faced(tags: &[&str], area: f64) -> Photo {
        let mut p = photo(tags);
        p.faces = vec![Face { box_: Rect { x: 0.4, y: 0.4, w: 0.1, h: 0.1 }, capture_quality: None }];
        p.face_area_fraction = area;
        p
    }

    #[test]
    fn a_big_face_is_people_whatever_the_tags() {
        assert_eq!(shot_role(&faced(&["mountain", "food"], 0.03)), ShotRole::People);
    }

    #[test]
    fn a_small_face_needs_a_people_tag_and_a_passer_by_is_not_people() {
        // 0.012: above the passer-by floor, below the portrait threshold.
        assert_eq!(shot_role(&faced(&["people", "mountain"], 0.012)), ShotRole::People);
        assert_eq!(shot_role(&faced(&["mountain"], 0.012)), ShotRole::Establishing);
        assert_eq!(shot_role(&faced(&["people", "mountain"], 0.003)), ShotRole::Establishing);
    }

    #[test]
    fn a_people_tag_without_a_face_is_not_people() {
        // People photographed from behind: 1473 photos carry the tag.
        assert_eq!(shot_role(&photo(&["people", "adult", "land"])), ShotRole::Establishing);
    }

    #[test]
    fn detail_needs_a_detail_tag_and_no_landscape() {
        assert_eq!(shot_role(&photo(&["food", "tableware"])), ShotRole::Detail);
        assert_eq!(shot_role(&photo(&["statue", "mountain"])), ShotRole::Establishing);
    }

    #[test]
    fn the_ubiquitous_tags_say_nothing() {
        assert_eq!(shot_role(&photo(&["outdoor", "sky", "structure"])), ShotRole::Moment);
        assert_eq!(shot_role(&photo(&[])), ShotRole::Moment);
    }

    #[test]
    fn calm_prefers_wide_warm_and_even() {
        let mut wide = photo(&["lake"]);
        wide.warmth = Some(0.7);
        let mut clipped = wide.clone();
        clipped.clipped_high = 0.4;
        let mut cool = wide.clone();
        cool.warmth = Some(0.3);
        let close = photo(&["food"]);
        assert!(calm_score(&wide) > calm_score(&clipped));
        assert!(calm_score(&wide) > calm_score(&cool));
        assert!(calm_score(&cool) > calm_score(&close));
    }

    mod covering {
        use super::*;

        fn set(roles: &[&[&str]]) -> Vec<Photo> {
            roles.iter().map(|t| photo(t)).collect()
        }

        #[test]
        fn a_missing_role_is_promoted_into_the_window_behind_the_best_picks() {
            // 0 best, 1 and 2 moments, 3 another moment, 4 a lake.
            let photos = set(&[&["outdoor"], &["outdoor"], &["outdoor"], &["outdoor"], &["lake"]]);
            let mut list = vec![0, 1, 2, 3, 4];
            cover_roles(&photos, &mut list, 0, 5);
            assert_eq!(list, vec![0, 1, 4, 2, 3]);
        }

        #[test]
        fn two_missing_roles_fill_the_window_in_owed_order() {
            let mut photos = set(&[&["outdoor"], &["outdoor"], &["outdoor"], &["food"], &["lake"]]);
            photos.push(faced(&["people"], 0.05));
            let mut list = vec![0, 1, 2, 3, 4, 5];
            cover_roles(&photos, &mut list, 0, 6);
            // Establishing then People win the two places; Detail waits.
            assert_eq!(list, vec![0, 4, 5, 1, 2, 3]);
        }

        #[test]
        fn the_best_pick_includes_and_look_alikes_never_move() {
            let mut photos = set(&[&["food"], &["outdoor"], &["outdoor"], &["outdoor"], &["outdoor"], &["outdoor"], &["mountain"]]);
            photos[5] = faced(&["people"], 0.05);
            // 0 is an Include, 1 the best auto pick; 6, the only wide shot,
            // is a deferred look-alike and so is not owed a place.
            let mut list = vec![0, 1, 2, 3, 4, 5, 6];
            cover_roles(&photos, &mut list, 1, 6);
            assert_eq!(&list[..2], &[0, 1]);
            assert_eq!(list[6], 6);
            assert_eq!(list, vec![0, 1, 2, 5, 3, 4, 6]);
        }

        #[test]
        fn a_role_already_shown_or_too_weak_is_not_promoted() {
            let mut photos = set(&[&["lake"], &["outdoor"], &["outdoor"], &["outdoor"], &["food"]]);
            photos[4].aesthetic_pct = ROLE_FLOOR_PCT - 1;
            photos.push(photo(&["mountain"]));
            let mut list = vec![0, 1, 2, 3, 4, 5];
            cover_roles(&photos, &mut list, 0, 6);
            assert_eq!(list, vec![0, 1, 2, 3, 4, 5]);
        }

        #[test]
        fn a_list_too_short_to_have_a_tail_is_left_alone() {
            let photos = set(&[&["outdoor"], &["outdoor"], &["lake"]]);
            let mut list = vec![0, 1, 2];
            cover_roles(&photos, &mut list, 0, 3);
            assert_eq!(list, vec![0, 1, 2]);
        }
    }

    mod metrics {
        use super::*;
        use crate::book::pace::{Page, Placement};
        use crate::geometry::Side;
        use crate::print_spec::PrintSpec;

        fn timed(tags: &[&str], event: u32, at: i64) -> Photo {
            let mut p = photo(tags);
            p.event_cluster = event;
            p.captured_at = Some(at);
            p
        }

        /// One photo per page, pages in the order given.
        fn book_of(order: &[usize]) -> Book {
            let pages = order
                .iter()
                .enumerate()
                .map(|(n, &i)| Page {
                    number: n as u32 + 1,
                    side: if n % 2 == 0 { Side::Right } else { Side::Left },
                    template_id: "t".into(),
                    placements: vec![Placement {
                        photo_index: i,
                        slot_rect: Rect { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
                        crop: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
                        z: 0,
                    }],
                })
                .collect();
            Book {
                pages,
                seed: 1,
                dropped: 0,
                controls: Default::default(),
                spec: PrintSpec::pixajoy(),
                options: Default::default(),
                cover: Default::default(),
            }
        }

        fn chapter() -> Vec<Photo> {
            vec![
                timed(&["lake"], 0, 10),     // 0 establishing, calm
                timed(&["food"], 0, 20),     // 1 detail
                timed(&["outdoor"], 0, 30),  // 2 moment
                timed(&["outdoor"], 0, 40),  // 3 moment
            ]
        }

        #[test]
        fn an_establishing_opener_and_a_calm_closer_hit() {
            let photos = chapter();
            let m = story_metrics(&book_of(&[0, 1, 2, 3]), &photos);
            assert_eq!(m.opener_hit, Some(1.0));
            assert_eq!(m.closer_hit, Some(0.0)); // the lake is the calmest, and it opens
            let m = story_metrics(&book_of(&[1, 2, 3, 0]), &photos);
            assert_eq!(m.opener_hit, Some(0.0));
            assert_eq!(m.closer_hit, Some(1.0));
        }

        #[test]
        fn a_short_chapter_is_not_asked() {
            let photos = chapter();
            let m = story_metrics(&book_of(&[1, 2, 0]), &photos);
            assert_eq!((m.opener_hit, m.closer_hit), (None, None));
        }

        #[test]
        fn chapters_that_run_backwards_are_counted() {
            let mut photos = chapter();
            photos.push(timed(&["outdoor"], 1, 5));
            assert_eq!(story_metrics(&book_of(&[0, 1, 2, 3, 4]), &photos).chrono_inv, 1);
            assert_eq!(story_metrics(&book_of(&[4, 0, 1, 2, 3]), &photos).chrono_inv, 0);
        }

        #[test]
        fn two_events_sharing_pages_are_still_two_chapters() {
            let mut photos = chapter();
            photos.push(timed(&["outdoor"], 1, 50));
            // 0 0 1 0: event 0's last photo printed after event 1's first.
            let m = story_metrics(&book_of(&[0, 1, 4, 2, 3]), &photos);
            assert_eq!((m.chapters, m.chrono_inv, m.event_inv), (2, 0, 1));
            let m = story_metrics(&book_of(&[0, 1, 2, 3, 4]), &photos);
            assert_eq!(m.event_inv, 0);
        }

        #[test]
        fn a_spread_with_two_events_is_mixed() {
            let mut photos = chapter();
            photos.push(timed(&["outdoor"], 1, 50));
            // Pages: [0] | [1,2] | [3,4] | ... the opening of pages 4 and 5
            // holds event 0's last photo and event 1's.
            let book = book_of(&[0, 1, 2, 3, 4, 2]);
            assert_eq!(story_metrics(&book, &photos).mixed_spreads, 1);
        }

        #[test]
        fn coverage_counts_only_roles_worth_printing() {
            let mut photos = chapter();
            // The chapter's only portrait, and too weak to be owed a place.
            let mut weak = faced(&["people"], 0.05);
            weak.captured_at = Some(25);
            weak.aesthetic_pct = 10;
            photos.push(weak);
            // Printed: establishing + moment. Available at >=40: establishing,
            // detail, moment -- not people.
            let m = story_metrics(&book_of(&[0, 2]), &photos);
            assert!((m.role_cov.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        }
    }
}
