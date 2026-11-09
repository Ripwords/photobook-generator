//! Chapter pages: where each of a finished book's chapters begins and which
//! pages it covers. `book::chapter` decides which photos form a chapter; this
//! reads where those chapters landed in the laid-out book.
//!
//! Derived from the finished book on every read, so a regenerated opening or
//! a swapped photo can never leave a stale chapter behind. The book stores
//! nothing about chapters.

use crate::book::cull::Photo;
use crate::book::edit::{opening_count, opening_pages};
use crate::book::pace::Book;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    /// 1-based, in book order.
    pub number: usize,
    /// The event whose photo is read first on the chapter's opening.
    pub event: u32,
    /// Every event the chapter shows for the first time, `event` first.
    pub events: Vec<u32>,
    /// The opening the chapter starts on, and the indices into `Book::pages`
    /// it covers up to the next chapter.
    pub opening: usize,
    pub pages: Vec<usize>,
    /// Wall-clock capture times of the chapter's printed photos.
    pub start: Option<i64>,
    pub end: Option<i64>,
}

/// The book's chapters. A chapter starts on an opening whose first-read
/// photo belongs to an event no earlier opening has shown; an event that
/// first appears part-way through an opening joins the chapter already
/// running.
pub fn chapter_pages(book: &Book, photos: &[Photo]) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for o in 0..opening_count(book) {
        let pages = opening_pages(book, o).unwrap_or_default();
        let order = reading_order(book, &pages);
        if let Some(&first) = order.first() {
            let event = photos[first].event_cluster;
            if !seen.contains(&event) {
                out.push(Chapter {
                    number: out.len() + 1,
                    event,
                    events: Vec::new(),
                    opening: o,
                    pages: Vec::new(),
                    start: None,
                    end: None,
                });
            }
        }
        let Some(chapter) = out.last_mut() else { continue };
        chapter.pages.extend(&pages);
        for &i in &order {
            let event = photos[i].event_cluster;
            if seen.insert(event) {
                chapter.events.push(event);
            }
            if let Some(t) = photos[i].captured_at {
                chapter.start = Some(chapter.start.map_or(t, |s| s.min(t)));
                chapter.end = Some(chapter.end.map_or(t, |e| e.max(t)));
            }
        }
    }
    out
}

/// Photo indices on `pages` in reading order: page by page, then left to
/// right, top to bottom. The same rule as `story::reading_order`.
fn reading_order(book: &Book, pages: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    for &p in pages {
        let mut placed: Vec<_> = book.pages[p].placements.iter().collect();
        placed.sort_by(|a, b| {
            (a.slot_rect.x, a.slot_rect.y)
                .partial_cmp(&(b.slot_rect.x, b.slot_rect.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.extend(placed.iter().map(|pl| pl.photo_index));
    }
    out
}

const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Days since 1970-01-01 to (year, month 1-12, day 1-31). Howard Hinnant's
/// `civil_from_days`; exact over the whole proleptic Gregorian calendar.
pub(crate) fn civil(days: i64) -> (i64, usize, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// "12 Mar 2026", "12–14 Mar 2026", "28 Mar – 2 Apr 2026" or
/// "30 Dec 2025 – 2 Jan 2026", from wall-clock seconds. The webview's
/// `dateRange` must agree; `tests/fixtures/wire/date-ranges.json` pins both.
pub fn date_range(start: i64, end: i64) -> String {
    let (start, end) = (start.min(end), start.max(end));
    let (y0, m0, d0) = civil(start.div_euclid(86_400));
    let (y1, m1, d1) = civil(end.div_euclid(86_400));
    let (n0, n1) = (MONTHS[m0 - 1], MONTHS[m1 - 1]);
    if (y0, m0, d0) == (y1, m1, d1) {
        format!("{d0} {n0} {y0}")
    } else if (y0, m0) == (y1, m1) {
        format!("{d0}\u{2013}{d1} {n0} {y0}")
    } else if y0 == y1 {
        format!("{d0} {n0} \u{2013} {d1} {n1} {y0}")
    } else {
        format!("{d0} {n0} {y0} \u{2013} {d1} {n1} {y1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::pace::{Page, Placement};
    use crate::geometry::{Rect, Side};
    use crate::print_spec::PrintSpec;

    const DAY: i64 = 86_400;
    /// 2026-03-12 00:00 wall clock.
    const MAR_12_2026: i64 = 20_524 * DAY;

    fn photo(event: u32, at: Option<i64>) -> Photo {
        Photo {
            path: format!("p{event}"), hash: "h".into(), width: 4000, height: 3000,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: event,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::new(), capture_quality: None,
            scene_tags: Vec::new(), captured_at: at,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None, utc_offset: None, warmth: None, horizon_tilt_deg: None,
        }
    }

    fn placed(photo_index: usize, slot_rect: Rect) -> Placement {
        Placement { photo_index, slot_rect, crop: Rect::new(0.0, 0.0, 1.0, 1.0), z: 1 }
    }

    /// A slot that fills the safe area of a Pixajoy page top to bottom.
    fn full(photo_index: usize) -> Placement {
        placed(photo_index, Rect::new(0.0, 0.0, 1.0, 1.0))
    }

    /// One page per entry, page 1 on the right; each entry is the photos on
    /// that page as full-page placements (a page with two shares the rect,
    /// which is fine for reading order: ties keep insertion order).
    fn book_of(pages: &[&[Placement]]) -> Book {
        Book {
            pages: pages
                .iter()
                .enumerate()
                .map(|(n, placements)| Page {
                    number: n as u32 + 1,
                    side: if n % 2 == 0 { Side::Right } else { Side::Left },
                    template_id: "none".into(),
                    placements: placements.to_vec(),
                })
                .collect(),
            seed: 1,
            dropped: 0,
            controls: Default::default(),
            spec: PrintSpec::pixajoy(),
            options: Default::default(),
            cover: Default::default(),
        }
    }

    #[test]
    fn chapter_pages_date_range_formats_each_span() {
        let d = MAR_12_2026;
        assert_eq!(date_range(d + 3600, d + 7200), "12 Mar 2026");
        assert_eq!(date_range(d, d + 2 * DAY + 50), "12\u{2013}14 Mar 2026");
        assert_eq!(date_range(d + 16 * DAY, d + 21 * DAY), "28 Mar \u{2013} 2 Apr 2026");
        assert_eq!(date_range(d - 72 * DAY, d - 69 * DAY), "30 Dec 2025 \u{2013} 2 Jan 2026");
        // Reversed arguments read the same as ordered ones.
        assert_eq!(date_range(d + 2 * DAY, d), "12\u{2013}14 Mar 2026");
        // Leap day, and a day before the epoch.
        assert_eq!(date_range(19_782 * DAY, 19_782 * DAY), "29 Feb 2024");
        assert_eq!(date_range(-1, -1), "31 Dec 1969");
    }

    /// Every date range the webview's `dateRange` must also produce, so the
    /// contact sheet and the editor's chapter heading can never disagree.
    #[test]
    fn chapter_pages_date_range_matches_the_shared_wire_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/wire/date-ranges.json");
        let cases: Vec<(i64, i64, String)> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(cases.len() >= 6);
        for (s, e, want) in cases {
            assert_eq!(date_range(s, e), want, "{s}..{e}");
        }
    }

    /// Openings: [p1] [p2 p3] [p4 p5] [p6]. Event 0 fills p1-p2, event 1
    /// starts mid-opening on p3 (so it joins chapter 1), event 2 opens p4.
    fn three_events() -> (Book, Vec<Photo>) {
        let d = MAR_12_2026;
        let photos = vec![
            photo(0, Some(d)),
            photo(0, Some(d + DAY)),
            photo(1, Some(d + 2 * DAY)),
            photo(2, Some(d + 20 * DAY)),
            photo(2, Some(d + 21 * DAY)),
            photo(2, None),
        ];
        let book =
            book_of(&[&[full(0)], &[full(1)], &[full(2)], &[full(3)], &[full(4)], &[full(5)]]);
        (book, photos)
    }

    #[test]
    fn chapter_pages_a_chapter_starts_only_where_an_opening_leads_with_a_new_event() {
        let (book, photos) = three_events();
        let cs = chapter_pages(&book, &photos);
        let got: Vec<_> = cs.iter().map(|c| (c.event, c.events.clone(), c.opening, c.pages.clone())).collect();
        assert_eq!(got, vec![(0, vec![0, 1], 0, vec![0, 1, 2]), (2, vec![2], 2, vec![3, 4, 5])]);
        let d = MAR_12_2026;
        assert_eq!((cs[0].start, cs[0].end), (Some(d), Some(d + 2 * DAY)));
        assert_eq!((cs[1].start, cs[1].end), (Some(d + 20 * DAY), Some(d + 21 * DAY)));
        assert_eq!(cs[1].number, 2);
    }

    #[test]
    fn chapter_pages_an_event_already_shown_does_not_start_a_chapter_again() {
        let (mut book, photos) = three_events();
        // Event 0 again leads the third opening; it was already shown.
        book.pages[3].placements = vec![full(1)];
        let cs = chapter_pages(&book, &photos);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].events, vec![0, 1, 2]);
    }
}
