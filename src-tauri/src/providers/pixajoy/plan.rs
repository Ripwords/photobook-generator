//! The Pixajoy `project/save` diff for a [`PrintJob`]. Pure: it reads the
//! parsed `project/load` (plus any lazily loaded page slices), the job and the
//! uploaded asset ids, and returns JSON. See `docs/providers/pixajoy.md`.

use crate::geometry::Rect;
use crate::print_spec::PrintSpec;
use crate::providers::{JobImage, PrintJob};
use crate::templates::Align;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// IHL8511's page limits. Fixed before the project exists, because the page
/// count is sent with `createProject2` and cannot change afterwards.
pub const MIN_PAGES: u32 = 20;
pub const MAX_PAGES: u32 = 150;

/// The cover's page number in Taopix.
const COVER_PAGE: u32 = 100_000;
/// Taopix lengths are inches × 10¹⁴.
const UNIT: f64 = 1e14;
/// `project/load` carries items for the first slice only; later pages come
/// from `project/pageData/<slice>`, `SLICE` page numbers at a time.
const SLICE: u32 = 10;
/// Arial, the font of the cover text box the titles are cloned from.
const FONT: &str = "<fi0*0400*ARIALMT\tArialMT\tArial\tArial\tArial (TT)><fn0*0400*ARIALMT>";
/// Titles print black: `TitleBox` has no colour.
const BLACK: u32 = 0;
/// Taopix's transparent `backcolour`.
const TRANSPARENT: i64 = -1;
/// The `itemstate` of a page record whose items came with the load.
const LOADED: i64 = 2;
/// How far a book's page shape may stray from Pixajoy's landscape page.
/// Books saved under the old 11.197 x 8.894 default are 2 % off Taopix's
/// page, and the cover fit absorbs that.
const ASPECT_TOLERANCE: f64 = 0.05;

/// How many pages the Pixajoy project gets for a book of `book_pages`.
pub fn page_count_for(book_pages: usize) -> Result<u32, String> {
    if book_pages > MAX_PAGES as usize {
        return Err(format!("Pixajoy books have at most {MAX_PAGES} pages. This book has {book_pages}."));
    }
    let n = (book_pages as u32).max(MIN_PAGES);
    Ok(n + n % 2)
}

/// Refuses a book whose pages are not Pixajoy's landscape shape: its
/// layouts would be stretched onto the product's page.
pub fn check_size(spec: &PrintSpec) -> Result<(), String> {
    let want = PrintSpec::pixajoy();
    let aspect = |s: &PrintSpec| s.page_w_in() / s.page_h_in();
    if (aspect(spec) / aspect(&want) - 1.0).abs() > ASPECT_TOLERANCE {
        return Err(format!(
            "This book's pages are {:.2} × {:.2} in. Pixajoy's landscape book is {:.2} × {:.2} in.",
            spec.page_w_in(),
            spec.page_h_in(),
            want.page_w_in(),
            want.page_h_in()
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadBox {
    pub reference: i64,
    pub page: u32,
    pub picture: bool,
    pub left: f64,
    pub width: f64,
    pub height: f64,
    pub zindex: i64,
    pub fields: Map<String, Value>,
}

/// The parts of `project/load` the plan needs, in inches.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub page_w: f64,
    pub page_h: f64,
    pub boxes: Vec<LoadBox>,
    /// `project/pageData` slices whose boxes the load did not include.
    pub unloaded_slices: Vec<u32>,
}

pub fn parse_load(load: &Value) -> Result<Layout, String> {
    let page_w = length(&load["layout"]["width"]).ok_or("Pixajoy's layout has no page width.")?;
    let page_h = length(&load["layout"]["height"]).ok_or("Pixajoy's layout has no page height.")?;
    let records = load["project"].as_object().ok_or("Pixajoy's project has no pages.")?;
    let mut boxes = Vec::new();
    let mut unloaded = BTreeSet::new();
    for record in records.values() {
        boxes.extend(parse_items(&record["items"])?);
        let page = record["pagenumber"].as_u64().ok_or("A Pixajoy page has no number.")? as u32;
        if record["itemstate"].as_i64() != Some(LOADED) && page < COVER_PAGE {
            unloaded.insert(page / SLICE);
        }
    }
    Ok(Layout { page_w, page_h, boxes, unloaded_slices: unloaded.into_iter().collect() })
}

/// A Taopix length string, in inches.
fn length(v: &Value) -> Option<f64> {
    v.as_str()?.parse::<f64>().ok().map(|n| n / UNIT)
}

/// One record's `items`: an object keyed by ref, or `[]` when empty.
fn parse_items(items: &Value) -> Result<Vec<LoadBox>, String> {
    let Some(items) = items.as_object() else { return Ok(Vec::new()) };
    items.values().map(parse_box).collect()
}

fn parse_box(v: &Value) -> Result<LoadBox, String> {
    let fields = v.as_object().ok_or("A Pixajoy box is not an object.")?;
    let reference = v["ref"].as_i64().ok_or("A Pixajoy box has no ref.")?;
    let bad = |what: &str| format!("Pixajoy box {reference} has no {what}.");
    Ok(LoadBox {
        reference,
        page: v["pagenumber"].as_u64().ok_or_else(|| bad("page"))? as u32,
        picture: v["type"].as_i64() == Some(0),
        left: length(&v["boxleft"]).ok_or_else(|| bad("left"))?,
        width: length(&v["boxwidth"]).ok_or_else(|| bad("width"))?,
        height: length(&v["boxheight"]).ok_or_else(|| bad("height"))?,
        zindex: v["zindex"].as_i64().unwrap_or(0),
        fields: fields.clone(),
    })
}

/// The boxes in one `project/pageData/<slice>` response.
pub fn parse_page_data(data: &Value) -> Result<Vec<LoadBox>, String> {
    match &data["pageItems"] {
        Value::Object(pages) => {
            let mut boxes = Vec::new();
            for items in pages.values() {
                boxes.extend(parse_items(items)?);
            }
            Ok(boxes)
        }
        Value::Array(none) if none.is_empty() => Ok(Vec::new()),
        _ => {
            let keys: Vec<&str> = data.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
            Err(format!("Pixajoy sent no page items (the reply had {keys:?})."))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub magnification: f64,
    pub scroll_left: f64,
    pub scroll_top: f64,
}

/// A `pw × ph` pixel photo centred to cover a `bw × bh` inch box.
pub fn cover_fit(pw: u32, ph: u32, bw: f64, bh: f64) -> Fit {
    let (pw, ph) = (pw as f64, ph as f64);
    let fit = (bw / pw).min(bh / ph);
    let fill = (bw / pw).max(bh / ph);
    Fit {
        magnification: 100.0 * fill / fit,
        scroll_left: -(pw * fill - bw) * 0.5,
        scroll_top: -(ph * fill - bh) * 0.5,
    }
}

/// A Taopix `styledtext.data` string: Arial at `point_size`, `colour` as
/// 0xRRGGBB, aligned, then the text prefixed by its length. The length is in
/// UTF-16 code units because the editor that reads it is JavaScript.
pub fn styled_text_data(text: &str, align: Align, point_size: f64, colour: u32) -> String {
    let pa = match align {
        Align::Left => 0,
        Align::Center => 1,
        Align::Right => 2,
    };
    let ts = (point_size * 100.0).round() as i64;
    let len = text.encode_utf16().count();
    format!("{FONT}<ts{ts}><tc{colour}><pa{pa}>{len} <..>{text}")
}

/// The `project/save` body. `assets` maps each job image's path to its
/// uploaded asset id.
pub fn save_diff(
    layout: &Layout,
    job: &PrintJob,
    page_count: u32,
    assets: &HashMap<PathBuf, String>,
) -> Result<Value, String> {
    let interior = |b: &&LoadBox| b.picture && b.page < COVER_PAGE;
    let deleted: Map<String, Value> =
        layout.boxes.iter().filter(interior).map(|b| (b.reference.to_string(), json!(b.reference))).collect();

    let clone = |b: &LoadBox| {
        let mut t = b.fields.clone();
        for k in ["ref", "recordid", "boxtype"] {
            t.remove(k);
        }
        t
    };
    let template = layout.boxes.iter().find(interior).map(clone);
    // The cover's "Insert Your Text Here" box: the one text box a user may
    // delete, so the only one not carrying a protected label.
    let text_template = layout
        .boxes
        .iter()
        .find(|b| b.page == COVER_PAGE && b.fields["type"] == 1 && b.fields["protectfromdelete"] == false)
        .map(clone);
    let top = layout.boxes.iter().map(|b| b.zindex).max().unwrap_or(0);
    let mut added = Map::new();
    let mut k = 0i64;
    for page in &job.pages {
        if page.number < 1 || page.number > page_count {
            return Err(format!("Page {} is outside the {page_count}-page Pixajoy project.", page.number));
        }
        // Each box is placed on the page strip, stacked above everything
        // before it: photos bottom first, then the titles.
        let mut place = |mut item: Map<String, Value>, rect: &Rect| {
            k += 1;
            let (x, y) = ((page.number as f64 + rect.x) * layout.page_w, rect.y * layout.page_h);
            let (w, h) = (rect.w * layout.page_w, rect.h * layout.page_h);
            let at = |v: f64| json!(((v * UNIT).round() as i64).to_string());
            item.insert("ref".into(), json!(-k));
            item.insert("pagenumber".into(), json!(page.number));
            item.insert("zindex".into(), json!(top + k));
            for (key, v) in [
                ("boxleft", x),
                ("boxtop", y),
                ("boxwidth", w),
                ("boxheight", h),
                ("boxlinex1", x),
                ("boxliney1", y),
                ("boxlinex2", x + w),
                ("boxliney2", y + h),
            ] {
                item.insert(key.into(), at(v));
            }
            (-k, item, w, h)
        };
        for b in &page.boxes {
            let item = template.clone().ok_or("Pixajoy's project has no picture box to copy.")?;
            let (r, mut item, w, h) = place(item, &b.rect);
            item.extend(fill(&b.image, w, h, assets)?);
            added.insert(r.to_string(), Value::Object(item));
        }
        for t in &page.texts {
            let item = text_template.clone().ok_or("Pixajoy's project has no cover text box to copy.")?;
            let (r, mut item, _, _) = place(item, &t.rect);
            item.insert("backcolour".into(), json!(TRANSPARENT));
            item.insert("labelshort".into(), json!({}));
            let styled = item
                .get_mut("styledtext")
                .and_then(Value::as_object_mut)
                .ok_or("Pixajoy's cover text box has no styled text.")?;
            styled.insert("data".into(), json!(styled_text_data(&t.text, t.align, t.point_size, BLACK)));
            added.insert(r.to_string(), Value::Object(item));
        }
    }

    let mut cover: Vec<&LoadBox> = layout.boxes.iter().filter(|b| b.picture && b.page == COVER_PAGE).collect();
    cover.sort_by(|a, b| a.left.total_cmp(&b.left));
    let mut changed = Map::new();
    for (image, slot, side) in [(&job.cover.back, cover.first(), "back"), (&job.cover.front, cover.last(), "front")] {
        let Some(image) = image else { continue };
        let slot = slot.filter(|_| cover.len() == 2).ok_or(format!("Pixajoy's cover has no {side} picture box."))?;
        changed.insert(slot.reference.to_string(), Value::Object(fill(image, slot.width, slot.height, assets)?));
    }

    let empty = json!({ "deleted": {}, "added": {}, "changed": {} });
    Ok(json!({
        "pages": empty,
        "items": { "deleted": deleted, "added": added, "changed": changed },
        "events": empty,
        "itemsToPages": {},
        "projectName": job.name,
        "autoSave": false,
    }))
}

/// The fields that put `image` into a `bw × bh` inch box, centred to cover it.
fn fill(image: &JobImage, bw: f64, bh: f64, assets: &HashMap<PathBuf, String>) -> Result<Map<String, Value>, String> {
    let asset = assets
        .get(&image.path)
        .ok_or_else(|| format!("{} was not uploaded to Pixajoy.", image.path.display()))?;
    let fit = cover_fit(image.width, image.height, bw, bh);
    Ok(Map::from_iter([
        ("hiresfiletype".into(), json!(106)),
        ("hirespath".into(), json!(asset)),
        ("hireswidth".into(), json!(image.width)),
        ("hiresheight".into(), json!(image.height)),
        ("rotatedhireswidth".into(), json!(image.width)),
        ("rotatedhiresheight".into(), json!(image.height)),
        ("magnification".into(), json!(fit.magnification)),
        ("boxscrollleft".into(), json!(fit.scroll_left)),
        ("boxscrolltop".into(), json!(fit.scroll_top)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;
    use crate::providers::{JobBox, JobCover, JobPage, JobText};

    /// The cover's one unprotected text box in the fixture.
    const TEXT_TEMPLATE: i64 = 82447;

    #[test]
    fn styled_text_counts_utf16_units_and_codes_size_colour_and_alignment() {
        // Saved through the real editor and round-tripped exactly.
        assert_eq!(
            styled_text_data("Align one: Kyoto, März 2026", Align::Center, 36.0, 0),
            format!("{FONT}<ts3600><tc0><pa1>27 <..>Align one: Kyoto, März 2026")
        );
        assert_eq!(styled_text_data("Hi", Align::Left, 14.5, 65280), format!("{FONT}<ts1450><tc65280><pa0>2 <..>Hi"));
        // Six chars, nine bytes, seven UTF-16 units.
        assert_eq!(
            styled_text_data("Fuji 🗻", Align::Right, 20.0, 0xFF0000),
            format!("{FONT}<ts2000><tc16711680><pa2>7 <..>Fuji 🗻")
        );
    }

    fn titled(text: JobText) -> PrintJob {
        let photo = JobBox { rect: Rect::new(0.0, 0.0, 1.0, 1.0), image: image("/t/a.jpg", 1200, 800) };
        job(vec![JobPage { number: 2, boxes: vec![photo], texts: vec![text] }], JobCover::default())
    }

    fn kyoto() -> JobText {
        JobText { rect: Rect::new(0.1, 0.05, 0.8, 0.1), text: "Kyoto".into(), align: Align::Center, point_size: 24.0 }
    }

    #[test]
    fn a_title_is_the_cover_text_box_cloned_onto_its_page_above_its_photos() {
        let l = layout();
        let diff = save_diff(&l, &titled(kyoto()), 20, &assets(&["/t/a.jpg"])).unwrap();
        let added = diff["items"]["added"].as_object().unwrap();
        assert_eq!(added.len(), 2);
        let (photo, t) = (&added["-1"], &added["-2"]);
        let template = &l.boxes.iter().find(|b| b.reference == TEXT_TEMPLATE).unwrap().fields;

        assert_eq!(t["ref"], json!(-2));
        assert_eq!(t["type"], json!(1));
        assert_eq!(t["pagenumber"], json!(2));
        assert_eq!(t["backcolour"], json!(-1));
        assert_eq!(t["labelshort"], json!({}));
        assert!(t.get("recordid").is_none() && t.get("boxtype").is_none());
        assert!(t["zindex"].as_i64().unwrap() > photo["zindex"].as_i64().unwrap());

        let left = 2.1 * PAGE_W;
        assert!(close(inches(&t["boxleft"]), left, 1e-9), "{}", t["boxleft"]);
        assert!(close(inches(&t["boxtop"]), 0.05 * PAGE_H, 1e-9));
        assert!(close(inches(&t["boxwidth"]), 0.8 * PAGE_W, 1e-9));
        assert!(close(inches(&t["boxheight"]), 0.1 * PAGE_H, 1e-9));
        assert!(close(inches(&t["boxlinex2"]), left + 0.8 * PAGE_W, 1e-9));

        let mut want = template["styledtext"].clone();
        want["data"] = json!(styled_text_data("Kyoto", Align::Center, 24.0, 0));
        assert_eq!(t["styledtext"], want);
        assert!(t.get("hirespath").is_none_or(|v| v == &template["hirespath"]));
    }

    #[test]
    fn a_title_with_no_cover_text_box_to_copy_is_an_error() {
        let mut l = layout();
        l.boxes.retain(|b| b.reference != TEXT_TEMPLATE);
        let err = save_diff(&l, &titled(kyoto()), 20, &assets(&["/t/a.jpg"])).unwrap_err();
        assert!(err.contains("text box"), "{err}");
    }

    const LOAD: &str = include_str!("../../../tests/fixtures/pixajoy/load.json");
    const PAGE_W: f64 = 11.22047244094486;
    const PAGE_H: f64 = 8.74015748031494;

    fn layout() -> Layout {
        parse_load(&serde_json::from_str(LOAD).unwrap()).unwrap()
    }

    fn inches(v: &Value) -> f64 {
        v.as_str().unwrap().parse::<f64>().unwrap() / UNIT
    }

    fn image(path: &str, width: u32, height: u32) -> JobImage {
        JobImage { path: PathBuf::from(path), width, height }
    }

    fn job(pages: Vec<JobPage>, cover: JobCover) -> PrintJob {
        PrintJob { name: "Trip".into(), page_count: pages.len() as u32, pages, cover }
    }

    fn assets(paths: &[&str]) -> HashMap<PathBuf, String> {
        paths.iter().enumerate().map(|(i, p)| (PathBuf::from(p), format!("{}", 900 + i))).collect()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn page_count_pads_short_books_to_twenty() {
        assert_eq!(page_count_for(0), Ok(20));
        assert_eq!(page_count_for(3), Ok(20));
        assert_eq!(page_count_for(19), Ok(20));
        assert_eq!(page_count_for(20), Ok(20));
    }

    #[test]
    fn page_count_rounds_an_odd_count_up_to_even() {
        assert_eq!(page_count_for(21), Ok(22));
        assert_eq!(page_count_for(22), Ok(22));
        assert_eq!(page_count_for(149), Ok(150));
    }

    #[test]
    fn page_count_refuses_more_than_150_pages() {
        assert_eq!(page_count_for(150), Ok(150));
        let err = page_count_for(151).unwrap_err();
        assert!(err.contains("150") && err.contains("151"), "{err}");
    }

    #[test]
    fn the_pixajoy_size_fits_and_a_portrait_book_does_not() {
        assert_eq!(check_size(&pixajoy_spec()), Ok(()));
        let saved_before_ihl8511: PrintSpec = serde_json::from_value(json!({
            "pageWIn": 11.197, "pageHIn": 8.894, "bleedIn": 0.197, "gutterIn": 0.197,
            "safeMarginIn": 0.125, "minDpi": 200.0, "warnDpi": 300.0, "coverWrapIn": 0.75
        }))
        .unwrap();
        assert_eq!(check_size(&saved_before_ihl8511), Ok(()));
        let portrait: PrintSpec = serde_json::from_value(json!({
            "pageWIn": 8.5, "pageHIn": 11.0, "bleedIn": 0.19685, "gutterIn": 0.3937,
            "safeMarginIn": 0.25, "minDpi": 150.0, "warnDpi": 240.0, "coverWrapIn": 0.6
        }))
        .unwrap();
        assert!(check_size(&portrait).is_err());
    }

    #[test]
    fn the_api_doc_check_case() {
        let fit = cover_fit(1200, 800, 11.2205, 8.7402);
        assert!(close(fit.magnification, 116.842, 0.001), "{fit:?}");
        assert!(close(fit.scroll_left, -0.9449, 0.0001), "{fit:?}");
        assert!(close(fit.scroll_top, 0.0, 1e-9), "{fit:?}");
    }

    #[test]
    fn a_tall_photo_in_a_wide_box_scrolls_vertically() {
        // 800 × 1600 in a 4 × 2 box: fill 0.005 in/px, fit 0.00125.
        let fit = cover_fit(800, 1600, 4.0, 2.0);
        assert!(close(fit.magnification, 400.0, 1e-9), "{fit:?}");
        assert!(close(fit.scroll_left, 0.0, 1e-9), "{fit:?}");
        assert!(close(fit.scroll_top, -(8.0 - 2.0) / 2.0, 1e-9), "{fit:?}");
    }

    #[test]
    fn the_load_gives_the_page_size_and_the_unloaded_slices() {
        let l = layout();
        assert!(close(l.page_w, PAGE_W, 1e-9) && close(l.page_h, PAGE_H, 1e-9));
        // Records 10..=40 came back with `itemstate` 0 and no items.
        assert_eq!(l.unloaded_slices, vec![1, 2, 3, 4]);
        assert!(l.boxes.iter().any(|b| b.page == 9));
        assert!(!l.boxes.iter().any(|b| (10..COVER_PAGE).contains(&b.page)));
    }

    #[test]
    fn page_data_boxes_are_read_from_every_page_entry() {
        let l = layout();
        let mut box10 = l.boxes.iter().find(|b| b.page == 3).unwrap().fields.clone();
        box10.insert("ref".into(), json!(99_010));
        box10.insert("pagenumber".into(), json!(10));
        let mut box11 = box10.clone();
        box11.insert("ref".into(), json!(99_011));
        box11.insert("pagenumber".into(), json!(11));
        // The editor destructures `{ data: { pageItems } }` from an axios
        // response, so in the body itself `pageItems` is top level.
        let data = json!({ "pageItems": {
            "10": { "99010": box10, "99011": box11 },
            "12": []
        }, "textCache": {} });

        let mut refs: Vec<(i64, u32)> =
            parse_page_data(&data).unwrap().iter().map(|b| (b.reference, b.page)).collect();
        refs.sort();
        assert_eq!(refs, vec![(99_010, 10), (99_011, 11)]);
    }

    #[test]
    fn a_slice_with_no_boxes_left_is_an_empty_array() {
        // PHP encodes an empty map as `[]`, as it does for a page's `items`.
        assert_eq!(parse_page_data(&json!({ "pageItems": [], "textCache": [] })), Ok(vec![]));
        let err = parse_page_data(&json!({ "error": "x" })).unwrap_err();
        assert!(err.contains("error"), "names what came back: {err}");
    }

    #[test]
    fn every_default_interior_picture_box_is_deleted_including_lazily_loaded_ones() {
        let mut l = layout();
        let mut late = l.boxes.iter().find(|b| b.page == 3).unwrap().clone();
        late.reference = 99_015;
        late.page = 15;
        l.boxes.push(late);

        let diff = save_diff(&l, &job(vec![], JobCover::default()), 20, &HashMap::new()).unwrap();

        let deleted = diff["items"]["deleted"].as_object().unwrap();
        let interior: BTreeSet<i64> = l
            .boxes
            .iter()
            .filter(|b| b.picture && b.page < COVER_PAGE)
            .map(|b| b.reference)
            .collect();
        assert_eq!(interior.len(), 10, "pages 1-9 from the load plus page 15");
        let got: BTreeSet<i64> = deleted.keys().map(|k| k.parse().unwrap()).collect();
        assert_eq!(got, interior);
        for (k, v) in deleted {
            assert_eq!(v.as_i64().unwrap().to_string(), *k);
        }
    }

    #[test]
    fn the_cover_text_boxes_are_never_touched() {
        let l = layout();
        let photos = ["/t/front.jpg", "/t/back.jpg"];
        let cover = JobCover { front: Some(image(photos[0], 900, 700)), back: Some(image(photos[1], 700, 700)) };
        let diff = save_diff(&l, &job(vec![], cover), 20, &assets(&photos)).unwrap();

        let text: BTreeSet<String> = l
            .boxes
            .iter()
            .filter(|b| b.page == COVER_PAGE && !b.picture)
            .map(|b| b.reference.to_string())
            .collect();
        assert_eq!(text.len(), 7);
        for section in ["deleted", "changed"] {
            for k in diff["items"][section].as_object().unwrap().keys() {
                assert!(!text.contains(k), "{section} touches cover text box {k}");
            }
        }
    }

    #[test]
    fn interior_boxes_sit_on_the_page_strip_at_their_page() {
        let l = layout();
        let rect = Rect::new(0.1, 0.2, 0.5, 0.25);
        let pages = vec![
            JobPage { number: 1, boxes: vec![JobBox { rect, image: image("/t/a.jpg", 1200, 800) }], texts: vec![] },
            JobPage { number: 2, boxes: vec![JobBox { rect, image: image("/t/a.jpg", 1200, 800) }], texts: vec![] },
            JobPage { number: 3, boxes: vec![JobBox { rect, image: image("/t/a.jpg", 1200, 800) }], texts: vec![] },
        ];
        let diff = save_diff(&l, &job(pages, JobCover::default()), 20, &assets(&["/t/a.jpg"])).unwrap();
        let added = diff["items"]["added"].as_object().unwrap();
        assert_eq!(added.len(), 3);

        for (n, k) in [(1u32, "-1"), (2, "-2"), (3, "-3")] {
            let b = &added[k];
            let left = n as f64 * PAGE_W + 0.1 * PAGE_W;
            assert_eq!(b["pagenumber"], json!(n));
            assert!(close(inches(&b["boxleft"]), left, 1e-9), "page {n}: {}", b["boxleft"]);
            assert!(close(inches(&b["boxtop"]), 0.2 * PAGE_H, 1e-9));
            assert!(close(inches(&b["boxwidth"]), 0.5 * PAGE_W, 1e-9));
            assert!(close(inches(&b["boxheight"]), 0.25 * PAGE_H, 1e-9));
            assert_eq!(b["boxlinex1"], b["boxleft"]);
            assert_eq!(b["boxliney1"], b["boxtop"]);
            assert!(close(inches(&b["boxlinex2"]), left + 0.5 * PAGE_W, 1e-9));
            assert!(close(inches(&b["boxliney2"]), 0.45 * PAGE_H, 1e-9));
        }
    }

    #[test]
    fn added_boxes_are_cloned_defaults_with_unique_negative_refs_and_a_fill() {
        let l = layout();
        let full = Rect::new(0.0, 0.0, 1.0, 1.0);
        let pages = vec![
            JobPage { number: 1, boxes: vec![], texts: vec![] },
            JobPage {
                number: 2,
                boxes: vec![
                    JobBox { rect: full, image: image("/t/a.jpg", 1200, 800) },
                    JobBox { rect: Rect::new(0.5, 0.5, 0.25, 0.25), image: image("/t/b.jpg", 600, 600) },
                ],
                texts: vec![],
            },
            JobPage { number: 3, boxes: vec![JobBox { rect: full, image: image("/t/a.jpg", 1200, 800) }], texts: vec![] },
        ];
        let ids = assets(&["/t/a.jpg", "/t/b.jpg"]);
        let diff = save_diff(&l, &job(pages, JobCover::default()), 20, &ids).unwrap();
        let added = diff["items"]["added"].as_object().unwrap();

        let refs: BTreeSet<i64> = added.values().map(|b| b["ref"].as_i64().unwrap()).collect();
        assert_eq!(refs, BTreeSet::from([-3, -2, -1]));
        for (k, b) in added {
            assert_eq!(b["ref"].as_i64().unwrap().to_string(), *k);
            assert!(b.get("recordid").is_none() && b.get("boxtype").is_none(), "{k} kept a server field");
            assert_eq!(b["type"], json!(0));
            assert_eq!(b["hiresfiletype"], json!(106));
        }
        // Stacking follows the job's order, above everything already there.
        let max_z = l.boxes.iter().map(|b| b.zindex).max().unwrap();
        let z = |k: &str| added[k]["zindex"].as_i64().unwrap();
        assert!(z("-1") > max_z && z("-2") > z("-1"));

        let b = &added["-2"];
        assert_eq!(b["hirespath"], json!(ids[&PathBuf::from("/t/b.jpg")]));
        assert_eq!((b["hireswidth"].clone(), b["hiresheight"].clone()), (json!(600), json!(600)));
        assert_eq!(b["rotatedhireswidth"], json!(600));
        let fit = cover_fit(600, 600, 0.25 * PAGE_W, 0.25 * PAGE_H);
        assert!(close(b["magnification"].as_f64().unwrap(), fit.magnification, 1e-9));
        assert!(close(b["boxscrolltop"].as_f64().unwrap(), fit.scroll_top, 1e-9));
        assert_eq!(added["-1"]["pagenumber"], json!(2));
        assert_eq!(added["-2"]["pagenumber"], json!(2));
        assert_eq!(added["-3"]["pagenumber"], json!(3));
    }

    #[test]
    fn the_cover_front_is_the_rightmost_picture_box_and_the_back_the_other() {
        let l = layout();
        let photos = ["/t/front.jpg", "/t/back.jpg"];
        let ids = assets(&photos);
        let cover = JobCover { front: Some(image(photos[0], 900, 700)), back: Some(image(photos[1], 700, 700)) };
        let diff = save_diff(&l, &job(vec![], cover), 20, &ids).unwrap();
        let changed = diff["items"]["changed"].as_object().unwrap();

        assert_eq!(changed.len(), 2);
        let front = &changed["82448"];
        assert_eq!(front["hirespath"], json!(ids[&PathBuf::from(photos[0])]));
        let fit = cover_fit(900, 700, 8.63893583502664, 6.47920187627016);
        assert!(close(front["magnification"].as_f64().unwrap(), fit.magnification, 1e-9));
        assert!(close(front["boxscrolltop"].as_f64().unwrap(), fit.scroll_top, 1e-9));
        assert!(front.get("boxleft").is_none(), "the cover box keeps its place");
        assert_eq!(changed["82449"]["hirespath"], json!(ids[&PathBuf::from(photos[1])]));
    }

    #[test]
    fn an_empty_cover_side_leaves_its_box_alone() {
        let l = layout();
        let cover = JobCover { front: None, back: Some(image("/t/back.jpg", 700, 700)) };
        let diff = save_diff(&l, &job(vec![], cover), 20, &assets(&["/t/back.jpg"])).unwrap();
        let changed = diff["items"]["changed"].as_object().unwrap();
        assert_eq!(changed.keys().collect::<Vec<_>>(), vec!["82449"]);
    }

    #[test]
    fn a_page_beyond_the_project_is_an_error() {
        let l = layout();
        let pages = vec![JobPage {
            number: 21,
            boxes: vec![JobBox { rect: Rect::new(0.0, 0.0, 1.0, 1.0), image: image("/t/a.jpg", 10, 10) }],
            texts: vec![],
        }];
        assert!(save_diff(&l, &job(pages, JobCover::default()), 20, &assets(&["/t/a.jpg"])).is_err());
    }

    #[test]
    fn an_image_with_no_uploaded_asset_is_an_error() {
        let l = layout();
        let pages = vec![JobPage {
            number: 1,
            boxes: vec![JobBox { rect: Rect::new(0.0, 0.0, 1.0, 1.0), image: image("/t/a.jpg", 10, 10) }],
            texts: vec![],
        }];
        assert!(save_diff(&l, &job(pages, JobCover::default()), 20, &HashMap::new()).is_err());
    }

    #[test]
    fn the_diff_has_the_save_envelope() {
        let diff = save_diff(&layout(), &job(vec![], JobCover::default()), 20, &HashMap::new()).unwrap();
        assert_eq!(diff["autoSave"], json!(false));
        assert_eq!(diff["projectName"], json!("Trip"));
        for k in ["pages", "events"] {
            assert_eq!(diff[k], json!({ "deleted": {}, "added": {}, "changed": {} }));
        }
        assert_eq!(diff["itemsToPages"], json!({}));
    }
}
