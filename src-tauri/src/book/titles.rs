//! Chapter titles: where a book's chapters begin, what each is called, and
//! where on the page the title prints.
//!
//! Everything here is DERIVED from the finished book on every read. The book
//! stores only what the user typed (`Book::titles`), so a regenerated
//! opening, a reprint at another size or a moved slot can never leave a
//! stale title box behind.
//!
//! Export never composites (see PROJECT-STATUS, "export design was
//! revised"), so a title is not drawn into any image. It travels in
//! `manifest.json` as a text box -- text, page, rect, alignment, point size
//! -- for the user to type into Pixajoy's text tool.

use crate::book::cull::Photo;
use crate::book::edit::{opening_count, opening_pages};
use crate::book::pace::Book;
use crate::geometry::{Rect, Side};
use crate::templates::{Align, Library};
use std::collections::{BTreeMap, BTreeSet};

/// The longest title the editor accepts, in characters. A title is a place
/// or a few words, and one long enough to wrap twice no longer fits a band.
pub const MAX_TITLE_CHARS: usize = 60;

/// The shortest free band, in inches, a title may print in without sitting
/// on a photo: one line at the smallest point size (14 pt is 0.19"), with
/// air around it. Half an inch was tried first and measured too strict: the
/// symmetric margin layouts leave 0.39" and every chapter opening on one
/// printed its title on a photo.
pub const MIN_BAND_IN: f64 = 0.35;

/// The narrowest free column beside the photos, in inches, a title may take
/// when no band is left: room for a short place name at the smallest point
/// size. Portrait pairs and diptychs leave two inches or more.
pub const MIN_COLUMN_IN: f64 = 1.5;

/// The tallest band a title takes. A blank page is all free space, and a
/// title centred in eight inches of it reads as a mistake, not a heading.
pub const MAX_BAND_IN: f64 = 1.0;

pub const MIN_POINT: f64 = 14.0;
pub const MAX_POINT: f64 = 28.0;

/// Where a chapter's title prints. `page` indexes `Book::pages`; `rect` is
/// page-normalised like a slot.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleBox {
    pub page: usize,
    pub rect: Rect,
    pub align: Align,
    pub point_size: f64,
    /// No clear space was found, so the title sits on a photo. The editor
    /// warns, and the user decides whether to move a slot or keep it.
    pub overlaps_photo: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    /// 1-based, in book order.
    pub number: usize,
    /// The event whose photo is read first on the chapter's opening. Titles
    /// are keyed by it.
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
    pub auto_title: String,
    /// What prints: the user's text when `custom`, otherwise `auto_title`.
    /// Empty means the user chose no title, and `title_box` is `None`.
    pub title: String,
    pub custom: bool,
    pub title_box: Option<TitleBox>,
}

/// The book's chapters. A chapter starts on an opening whose first-read
/// photo belongs to an event no earlier opening has shown; an event that
/// first appears part-way through an opening joins the chapter already
/// running, so two events sharing a spread never stack two titles on it.
///
/// `names` maps an event to its place name and is read only when the book
/// was generated with Places on; pass the `place_names` cache, never a live
/// geocode.
pub fn chapters(
    book: &Book,
    photos: &[Photo],
    lib: &Library,
    names: &BTreeMap<u32, String>,
) -> Vec<Chapter> {
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
                    auto_title: String::new(),
                    title: String::new(),
                    custom: false,
                    title_box: None,
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

    for c in &mut out {
        c.auto_title = auto_title(book, names, c);
        // Any of the chapter's events: a swap inside the chapter can change
        // which event reads first, and the user's title must not fall off.
        match c.events.iter().find_map(|e| book.titles.get(e)) {
            Some(t) => {
                c.title = t.clone();
                c.custom = true;
            }
            None => c.title = c.auto_title.clone(),
        }
        if !c.title.is_empty() {
            let first = opening_pages(book, c.opening).unwrap_or_default();
            c.title_box = title_box(book, lib, &first);
        }
    }
    out
}

fn auto_title(book: &Book, names: &BTreeMap<u32, String>, c: &Chapter) -> String {
    if book.options.places {
        if let Some(name) = names.get(&c.event).filter(|n| !n.trim().is_empty()) {
            return name.clone();
        }
    }
    match (c.start, c.end) {
        (Some(s), Some(e)) => date_range(s, e),
        _ => format!("Chapter {}", c.number),
    }
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
fn civil(days: i64) -> (i64, usize, u32) {
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

fn point_size(h_in: f64) -> f64 {
    (h_in * 72.0 * 0.6).clamp(MIN_POINT, MAX_POINT)
}

fn covers_a_photo(book: &Book, page: usize, rect: &Rect) -> bool {
    book.pages[page]
        .placements
        .iter()
        .any(|pl| pl.slot_rect.intersect(rect).is_some_and(|r| r.area() > 1e-9))
}

/// Where a title prints on the opening made of `pages`, in order of
/// preference:
/// 1. a text zone the template left for it, while no photo has been moved
///    onto it;
/// 2. the tallest free band of at least `MIN_BAND_IN` across the top or
///    bottom of a page, inside the safe area and clear of the gutter;
/// 3. the widest free column of at least `MIN_COLUMN_IN` beside the
///    photos, its top `MAX_BAND_IN` of it, on the same terms;
/// 4. a `MIN_BAND_IN` band across the top of the first page, flagged
///    `overlaps_photo` when it sits on a picture.
pub fn title_box(book: &Book, lib: &Library, pages: &[usize]) -> Option<TitleBox> {
    let &first = pages.first()?;
    let spec = &book.spec;
    let h_in = spec.page_h_in();

    for &p in pages {
        let page = &book.pages[p];
        let Some(layout) = lib.half(&page.template_id, page.side) else { continue };
        if let Some(z) = layout.text.iter().find(|z| !covers_a_photo(book, p, &z.rect)) {
            return Some(TitleBox {
                page: p,
                rect: z.rect,
                align: z.align,
                point_size: point_size(z.rect.h * h_in),
                overlaps_photo: false,
            });
        }
    }

    let usable = |side: Side| {
        let safe = spec.safe_rect(side);
        let g = spec.gutter_u();
        match side {
            Side::Left => Rect::new(safe.x, safe.y, (1.0 - g - safe.x).max(0.0), safe.h),
            Side::Right => Rect::new(g, safe.y, (safe.right() - g).max(0.0), safe.h),
        }
    };
    let cap = MAX_BAND_IN / h_in;
    let mut best: Option<(f64, TitleBox)> = None;
    for &p in pages {
        let page = &book.pages[p];
        let area = usable(page.side);
        let across: Vec<&Rect> = page
            .placements
            .iter()
            .map(|pl| &pl.slot_rect)
            .filter(|r| r.x < area.right() && r.right() > area.x)
            .collect();
        let top = across.iter().map(|r| r.y).fold(area.bottom(), f64::min) - area.y;
        let bottom = area.bottom() - across.iter().map(|r| r.bottom()).fold(area.y, f64::max);
        for (free, at_top) in [(top, true), (bottom, false)] {
            if free * h_in + 1e-9 < MIN_BAND_IN || best.as_ref().is_some_and(|(b, _)| free <= *b) {
                continue;
            }
            let h = free.min(cap);
            let y = if at_top { area.y } else { area.bottom() - h };
            let rect = Rect::new(area.x, y, area.w, h);
            best = Some((
                free,
                TitleBox {
                    page: p,
                    rect,
                    align: Align::Left,
                    point_size: point_size(h * h_in),
                    overlaps_photo: false,
                },
            ));
        }
    }
    if let Some((_, b)) = best {
        return Some(b);
    }

    let w_in = spec.page_w_in();
    let mut best: Option<(f64, TitleBox)> = None;
    for &p in pages {
        let page = &book.pages[p];
        let area = usable(page.side);
        let beside: Vec<&Rect> = page
            .placements
            .iter()
            .map(|pl| &pl.slot_rect)
            .filter(|r| r.y < area.bottom() && r.bottom() > area.y)
            .collect();
        let left = beside.iter().map(|r| r.x).fold(area.right(), f64::min) - area.x;
        let right = area.right() - beside.iter().map(|r| r.right()).fold(area.x, f64::max);
        for (free, at_left) in [(left, true), (right, false)] {
            if free * w_in + 1e-9 < MIN_COLUMN_IN || best.as_ref().is_some_and(|(b, _)| free <= *b) {
                continue;
            }
            let x = if at_left { area.x } else { area.right() - free };
            let h = area.h.min(cap);
            // Sized to the column's width, not its height: a column is
            // tall, and a title as large as the band rule allows would
            // break a short place name over three lines.
            let point = point_size(free * w_in / 5.0);
            best = Some((
                free,
                TitleBox {
                    page: p,
                    rect: Rect::new(x, area.y, free, h),
                    align: Align::Left,
                    point_size: point,
                    overlaps_photo: false,
                },
            ));
        }
    }
    if let Some((_, b)) = best {
        return Some(b);
    }

    let area = usable(book.pages[first].side);
    let rect = Rect::new(area.x, area.y, area.w, MIN_BAND_IN / h_in);
    Some(TitleBox {
        page: first,
        rect,
        align: Align::Left,
        point_size: point_size(MIN_BAND_IN),
        overlaps_photo: covers_a_photo(book, first, &rect),
    })
}

/// Why a title edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    /// No chapter is keyed by this event.
    NoSuchChapter(u32),
    TooLong { chars: usize },
}

/// Set (`Some`) or reset to automatic (`None`) the title of the chapter keyed
/// by `event`. The text is trimmed; an empty string means "print no title".
pub fn set_title(
    book: &mut Book,
    photos: &[Photo],
    lib: &Library,
    event: u32,
    title: Option<&str>,
) -> Result<(), TitleError> {
    let chapters = chapters(book, photos, lib, &BTreeMap::new());
    let chapter = chapters.iter().find(|c| c.event == event).ok_or(TitleError::NoSuchChapter(event))?;
    let text = title.map(str::trim);
    if let Some(t) = text {
        let chars = t.chars().count();
        if chars > MAX_TITLE_CHARS {
            return Err(TitleError::TooLong { chars });
        }
    }
    for e in &chapter.events {
        book.titles.remove(e);
    }
    if let Some(t) = text {
        book.titles.insert(event, t.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::pace::{Page, Placement};
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
            titles: Default::default(),
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

    fn empty_lib() -> Library {
        Library { spreads: Vec::new() }
    }

    fn real_lib() -> Library {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        Library::load(&dir).unwrap()
    }

    #[test]
    fn titles_date_range_formats_each_span() {
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
    /// contact sheet and the printed title can never disagree.
    #[test]
    fn titles_date_range_matches_the_shared_wire_fixture() {
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
    fn titles_a_chapter_starts_only_where_an_opening_leads_with_a_new_event() {
        let (book, photos) = three_events();
        let cs = chapters(&book, &photos, &empty_lib(), &BTreeMap::new());
        let got: Vec<_> = cs.iter().map(|c| (c.event, c.events.clone(), c.opening, c.pages.clone())).collect();
        assert_eq!(got, vec![(0, vec![0, 1], 0, vec![0, 1, 2]), (2, vec![2], 2, vec![3, 4, 5])]);
        assert_eq!(cs[0].auto_title, "12\u{2013}14 Mar 2026");
        assert_eq!(cs[1].auto_title, "1\u{2013}2 Apr 2026");
        assert_eq!(cs[1].number, 2);
    }

    #[test]
    fn titles_an_event_already_shown_does_not_start_a_chapter_again() {
        let (mut book, photos) = three_events();
        // Event 0 again leads the third opening; it was already titled.
        book.pages[3].placements = vec![full(1)];
        let cs = chapters(&book, &photos, &empty_lib(), &BTreeMap::new());
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].events, vec![0, 1, 2]);
    }

    #[test]
    fn titles_fall_back_to_a_place_then_dates_then_a_number() {
        let (mut book, mut photos) = three_events();
        let names: BTreeMap<u32, String> = [(2, "Vík".to_string())].into();
        // Places off: the name is ignored.
        let cs = chapters(&book, &photos, &empty_lib(), &names);
        assert_eq!(cs[1].auto_title, "1\u{2013}2 Apr 2026");
        book.options.places = true;
        let cs = chapters(&book, &photos, &empty_lib(), &names);
        assert_eq!((cs[0].auto_title.as_str(), cs[1].auto_title.as_str()), ("12\u{2013}14 Mar 2026", "Vík"));
        for p in &mut photos {
            p.captured_at = None;
        }
        let cs = chapters(&book, &photos, &empty_lib(), &names);
        assert_eq!(cs[0].auto_title, "Chapter 1");
    }

    #[test]
    fn titles_the_users_text_wins_and_survives_a_change_of_lead() {
        let (mut book, photos) = three_events();
        set_title(&mut book, &photos, &empty_lib(), 0, Some("  Reykjavík  ")).unwrap();
        let cs = chapters(&book, &photos, &empty_lib(), &BTreeMap::new());
        assert_eq!((cs[0].title.as_str(), cs[0].custom), ("Reykjavík", true));
        assert_eq!(cs[0].auto_title, "12\u{2013}14 Mar 2026");
        // A swap inside the chapter's first page puts event 1 first: the
        // title stays with the chapter.
        let (l, r) = (Rect::new(0.05, 0.1, 0.4, 0.8), Rect::new(0.55, 0.1, 0.4, 0.8));
        book.pages[0].placements = vec![placed(0, l), placed(2, r)];
        book.pages[2].placements = Vec::new();
        assert_eq!(chapters(&book, &photos, &empty_lib(), &BTreeMap::new())[0].event, 0);
        book.pages[0].placements = vec![placed(0, r), placed(2, l)];
        let cs = chapters(&book, &photos, &empty_lib(), &BTreeMap::new());
        assert_eq!(cs[0].event, 1);
        assert_eq!(cs[0].title, "Reykjavík");
        // Setting it under the new lead replaces rather than duplicates.
        set_title(&mut book, &photos, &empty_lib(), 1, Some("Golden Circle")).unwrap();
        assert_eq!(book.titles, [(1, "Golden Circle".to_string())].into());
        set_title(&mut book, &photos, &empty_lib(), 1, None).unwrap();
        assert!(book.titles.is_empty());
    }

    #[test]
    fn titles_an_empty_title_prints_nothing() {
        let (mut book, photos) = three_events();
        set_title(&mut book, &photos, &empty_lib(), 2, Some("   ")).unwrap();
        let cs = chapters(&book, &photos, &empty_lib(), &BTreeMap::new());
        assert_eq!((cs[1].title.as_str(), cs[1].custom), ("", true));
        assert_eq!(cs[1].title_box, None);
        assert!(cs[0].title_box.is_some());
    }

    #[test]
    fn titles_set_title_refuses_an_unknown_chapter_or_an_overlong_title() {
        let (mut book, photos) = three_events();
        assert_eq!(set_title(&mut book, &photos, &empty_lib(), 1, Some("x")), Err(TitleError::NoSuchChapter(1)));
        let long = "é".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(
            set_title(&mut book, &photos, &empty_lib(), 0, Some(&long)),
            Err(TitleError::TooLong { chars: MAX_TITLE_CHARS + 1 })
        );
        // Exactly the cap, counted in characters rather than bytes, is fine.
        set_title(&mut book, &photos, &empty_lib(), 0, Some(&"é".repeat(MAX_TITLE_CHARS))).unwrap();
        assert!(book.titles.contains_key(&0));
    }

    #[test]
    fn titles_box_takes_the_templates_text_zone() {
        let lib = real_lib();
        let t = lib.spreads.iter().find(|t| t.id == "03-hero-left-text-right").unwrap();
        let zone = t.right.text[0].clone();
        let mut book = book_of(&[&[], &[placed(0, t.left.slots[0].rect)], &[], &[]]);
        book.pages[1].template_id = t.id.clone();
        book.pages[2].template_id = t.id.clone();
        let b = title_box(&book, &lib, &[1, 2]).unwrap();
        assert_eq!((b.page, b.rect, b.align, b.overlaps_photo), (2, zone.rect, zone.align, false));
        // 0.2 of an 8.894" page is 1.78" tall: the largest point size.
        assert_eq!(b.point_size, MAX_POINT);

        // A photo moved onto the zone sends the title elsewhere.
        book.pages[2].placements = vec![placed(0, zone.rect)];
        let b = title_box(&book, &lib, &[1, 2]).unwrap();
        assert_ne!(b.rect, zone.rect);
    }

    #[test]
    fn titles_box_takes_the_tallest_free_band_clear_of_the_gutter() {
        // Left page: a photo from 0.30 down to the bottom edge, so the top is
        // free. Right page: a photo from the top to 0.85, a smaller band.
        let book = book_of(&[
            &[],
            &[placed(0, Rect::new(0.0, 0.30, 1.0, 0.70))],
            &[placed(1, Rect::new(0.0, 0.0, 1.0, 0.85))],
            &[],
        ]);
        let spec = &book.spec;
        let b = title_box(&book, &empty_lib(), &[1, 2]).unwrap();
        assert_eq!(b.page, 1);
        assert!(!b.overlaps_photo);
        let safe = spec.safe_rect(Side::Left);
        assert!((b.rect.y - safe.y).abs() < 1e-9, "{b:?}");
        assert!((b.rect.h - MAX_BAND_IN / spec.page_h_in()).abs() < 1e-9, "capped at 1 inch: {b:?}");
        assert!(b.rect.bottom() <= 0.30 + 1e-9);
        assert!(b.rect.right() <= 1.0 - spec.gutter_u() + 1e-9, "clear of the gutter: {b:?}");
        assert!(spec.safe_rect(Side::Left).contains(&b.rect));
        assert_eq!(b.point_size, MAX_POINT);

        // Right page wins once its band is the taller one, and the band sits
        // at the bottom, starting past the gutter.
        let book = book_of(&[
            &[],
            &[placed(0, Rect::new(0.0, 0.08, 1.0, 0.92))],
            &[placed(1, Rect::new(0.0, 0.0, 1.0, 0.75))],
            &[],
        ]);
        let b = title_box(&book, &empty_lib(), &[1, 2]).unwrap();
        assert_eq!(b.page, 2);
        let safe = spec.safe_rect(Side::Right);
        assert!((b.rect.bottom() - safe.bottom()).abs() < 1e-9, "{b:?}");
        assert!(b.rect.y >= 0.75 - 1e-9);
        assert!(b.rect.x >= spec.gutter_u() - 1e-9, "{b:?}");
    }

    #[test]
    fn titles_box_sits_on_a_photo_only_when_there_is_no_room() {
        let book = book_of(&[&[], &[full(0)], &[full(1)], &[]]);
        let b = title_box(&book, &empty_lib(), &[1, 2]).unwrap();
        assert_eq!(b.page, 1);
        assert!(b.overlaps_photo);
        assert!((b.rect.h * book.spec.page_h_in() - MIN_BAND_IN).abs() < 1e-9);
        // A band just under the floor is still too small.
        let gap = (MIN_BAND_IN - 0.01) / book.spec.page_h_in();
        let top = book.spec.safe_rect(Side::Left).y;
        let book = book_of(&[&[], &[placed(0, Rect::new(0.0, top + gap, 1.0, 1.0 - top - gap))], &[full(1)], &[]]);
        assert!(title_box(&book, &empty_lib(), &[1, 2]).unwrap().overlaps_photo);
    }

    /// The book's own pages, both halves of `id`, one photo per slot.
    fn spread_of(lib: &Library, id: &str) -> Book {
        let t = lib.spreads.iter().find(|t| t.id == id).unwrap();
        let slots = |h: &crate::templates::PageLayout| -> Vec<Placement> {
            h.slots.iter().enumerate().map(|(i, s)| placed(i, s.rect)).collect()
        };
        let (left, right) = (slots(&t.left), slots(&t.right));
        let mut book = book_of(&[&[], &left, &right, &[]]);
        book.pages[1].template_id = t.id.clone();
        book.pages[2].template_id = t.id.clone();
        book
    }

    fn clear_of_photos(book: &Book, b: &TitleBox) -> bool {
        !covers_a_photo(book, b.page, &b.rect)
    }

    /// Measured on the real library: the symmetric margin layout leaves
    /// 0.39" above its photo inside the safe area, and a half-inch floor
    /// put every chapter opening on it onto the photo. 14 pt type is under
    /// a fifth of an inch, so that margin is room enough.
    #[test]
    fn titles_box_fits_the_margin_a_margin_template_leaves() {
        let lib = real_lib();
        let book = spread_of(&lib, "07-two-up-symmetric-margin");
        let b = title_box(&book, &lib, &[1, 2]).unwrap();
        assert!(!b.overlaps_photo, "{b:?}");
        assert!(clear_of_photos(&book, &b));
        assert!(book.spec.safe_rect(book.pages[b.page].side).contains(&b.rect), "{b:?}");
        assert!(b.point_size >= MIN_POINT);
    }

    /// A portrait pair leaves no band top or bottom but two inches beside
    /// each photo. The title takes that column rather than a photo.
    #[test]
    fn titles_box_takes_a_free_side_column_when_no_band_is_left() {
        let lib = real_lib();
        for id in ["34-portrait-diptych", "11-two-up-portrait-pair"] {
            let book = spread_of(&lib, id);
            let spec = &book.spec;
            let b = title_box(&book, &lib, &[1, 2]).unwrap();
            assert!(!b.overlaps_photo, "{id}: {b:?}");
            assert!(clear_of_photos(&book, &b), "{id}: {b:?}");
            let side = book.pages[b.page].side;
            assert!(spec.safe_rect(side).contains(&b.rect), "{id}: {b:?}");
            match side {
                Side::Left => assert!(b.rect.right() <= 1.0 - spec.gutter_u() + 1e-9, "{id}: {b:?}"),
                Side::Right => assert!(b.rect.x >= spec.gutter_u() - 1e-9, "{id}: {b:?}"),
            }
            assert!(b.rect.w * spec.page_w_in() + 1e-9 >= MIN_COLUMN_IN, "{id}: {b:?}");
            assert!(b.rect.h * spec.page_h_in() <= MAX_BAND_IN + 1e-9, "{id}: {b:?}");
        }
    }

    /// A band, where there is one, beats a column: a heading across the top
    /// reads as a heading, one in a side strip reads as a caption.
    #[test]
    fn titles_box_prefers_a_band_to_a_column() {
        let top = 0.25;
        let book = book_of(&[&[], &[placed(0, Rect::new(0.0, top, 0.5, 1.0 - top))], &[full(1)], &[]]);
        let b = title_box(&book, &empty_lib(), &[1, 2]).unwrap();
        assert!(!b.overlaps_photo);
        assert!(b.rect.bottom() <= top + 1e-9, "{b:?}");
        let safe = book.spec.safe_rect(Side::Left);
        assert!((b.rect.x - safe.x).abs() < 1e-9 && b.rect.w > 0.5, "a band across the page, not the column beside the photo: {b:?}");
    }

    #[test]
    fn titles_box_point_size_follows_the_band() {
        assert_eq!(point_size(0.1), MIN_POINT);
        assert!((point_size(0.5) - 21.6).abs() < 1e-9);
        assert_eq!(point_size(2.0), MAX_POINT);
    }
}
