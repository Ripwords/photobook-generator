//! The Pixajoy `project/save` diff for a [`PrintJob`]. Pure: it reads the
//! parsed `project/load` (plus any lazily loaded page slices), the job and the
//! uploaded asset ids, and returns JSON. See `docs/providers/pixajoy.md`.

use crate::geometry::Rect;
use crate::print_spec::PrintSpec;
use crate::geometry::CoverSide;
use crate::providers::{JobPhoto, PrintJob, SentBox, SentSlot};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

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
    pub cover: Option<CoverCanvas>,
}

/// The cover spread in inches: back panel, spine, front panel, left to
/// right. A panel is one board plus the wrap on its outer edges, the area
/// PhotobookGen's cover photo fills.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverCanvas {
    pub width: f64,
    pub height: f64,
    pub panel_w: f64,
}

impl CoverCanvas {
    /// The panel on the left (back) or right (front) of the spread.
    fn panel(&self, front: bool) -> (f64, f64, f64, f64) {
        let x = if front { self.width - self.panel_w } else { 0.0 };
        (x, 0.0, self.panel_w, self.height)
    }
}

fn parse_cover(cover: &Value) -> Option<CoverCanvas> {
    let get = |k: &str| length(&cover[k]).filter(|v| *v > 0.0);
    Some(CoverCanvas {
        width: get("cover1width")?,
        height: get("cover1height")?,
        panel_w: get("cover1bleed")? + get("cover1paperwidth")?,
    })
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
    Ok(Layout {
        page_w,
        page_h,
        boxes,
        unloaded_slices: unloaded.into_iter().collect(),
        cover: parse_cover(&load["layout"]["cover"]),
    })
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

/// The `crop` window of a `pw × ph` pixel photo shown in a `bw × bh` inch
/// box. The window is scaled to cover the box and centred in it; when its
/// shape matches the box, as the book's crops do, it fills the box exactly.
/// The whole photo (`crop` of the unit square) is [`cover_fit`].
pub fn crop_fit(pw: u32, ph: u32, crop: Rect, bw: f64, bh: f64) -> Fit {
    let (pw, ph) = (pw as f64, ph as f64);
    let fit = (bw / pw).min(bh / ph);
    let scale = (bw / (crop.w * pw)).max(bh / (crop.h * ph));
    let centre = |start: f64, len: f64, px: f64, b: f64| b / 2.0 - (start + len / 2.0) * px * scale;
    Fit {
        magnification: 100.0 * scale / fit,
        scroll_left: centre(crop.x, crop.w, pw, bw),
        scroll_top: centre(crop.y, crop.h, ph, bh),
    }
}

/// A `project/save` body and the photo boxes it places. Added boxes carry
/// their temporary negative refs until [`resolve_refs`] reads the real ones
/// from the save's reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub body: Value,
    pub boxes: Vec<SentBox>,
}

/// The `project/save` diff. `assets` maps each photo's hash to its uploaded
/// asset id.
pub fn save_diff(
    layout: &Layout,
    job: &PrintJob,
    page_count: u32,
    assets: &HashMap<String, String>,
) -> Result<Diff, String> {
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
    let top = layout.boxes.iter().map(|b| b.zindex).max().unwrap_or(0);
    let mut added = Map::new();
    let mut sent = Vec::new();
    let mut k = 0i64;
    for page in &job.pages {
        if page.number < 1 || page.number > page_count {
            return Err(format!("Page {} is outside the {page_count}-page Pixajoy project.", page.number));
        }
        // Each box is placed on the page strip, stacked above everything
        // before it, bottom first.
        for b in &page.boxes {
            let mut item = template.clone().ok_or("Pixajoy's project has no picture box to copy.")?;
            k += 1;
            let r = -k;
            let (x, y) = ((page.number as f64 + b.rect.x) * layout.page_w, b.rect.y * layout.page_h);
            let (w, h) = (b.rect.w * layout.page_w, b.rect.h * layout.page_h);
            item.insert("ref".into(), json!(r));
            item.insert("pagenumber".into(), json!(page.number));
            item.insert("zindex".into(), json!(top + k));
            item.extend(geometry(x, y, w, h));
            let (fields, asset_id) = fill(&b.photo, w, h, assets)?;
            item.extend(fields);
            added.insert(r.to_string(), Value::Object(item));
            let slot = SentSlot::Page { number: page.number, rect: b.rect };
            sent.push(SentBox { reference: r, slot, hash: b.photo.hash.clone(), asset_id, crop: b.photo.crop });
        }
    }

    let mut cover: Vec<&LoadBox> = layout.boxes.iter().filter(|b| b.picture && b.page == COVER_PAGE).collect();
    cover.sort_by(|a, b| a.left.total_cmp(&b.left));
    // The template's two cover boxes (a framed front, a small square back)
    // become full panels, the shape the book's cover was cropped to.
    let mut changed = Map::new();
    for (photo, slot, side) in
        [(&job.cover.back, cover.first(), CoverSide::Back), (&job.cover.front, cover.last(), CoverSide::Front)]
    {
        let Some(photo) = photo else { continue };
        let front = side == CoverSide::Front;
        let name = if front { "front" } else { "back" };
        let slot = slot.filter(|_| cover.len() == 2).ok_or(format!("Pixajoy's cover has no {name} picture box."))?;
        let canvas = layout.cover.ok_or("Pixajoy's layout has no cover size.")?;
        let (x, y, w, h) = canvas.panel(front);
        let mut item = geometry(x, y, w, h);
        let (fields, asset_id) = fill(photo, w, h, assets)?;
        item.extend(fields);
        changed.insert(slot.reference.to_string(), Value::Object(item));
        let place = SentSlot::Cover { side };
        sent.push(SentBox { reference: slot.reference, slot: place, hash: photo.hash.clone(), asset_id, crop: photo.crop });
    }

    let empty = json!({ "deleted": {}, "added": {}, "changed": {} });
    let body = json!({
        "pages": empty,
        "items": { "deleted": deleted, "added": added, "changed": changed },
        "events": empty,
        "itemsToPages": {},
        "projectName": job.name,
        "autoSave": false,
    });
    Ok(Diff { body, boxes: sent })
}

/// Swaps each added box's temporary ref for the one Pixajoy gave it. The
/// save's reply lists them as `pageItems: [{ oldRef, newRef, id }]`.
pub fn resolve_refs(boxes: Vec<SentBox>, reply: &Value) -> Result<Vec<SentBox>, String> {
    let renamed: HashMap<i64, i64> = reply["pageItems"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["oldRef"].as_i64()?, p["newRef"].as_i64()?)))
        .collect();
    boxes
        .into_iter()
        .map(|mut b| {
            if b.reference < 0 {
                b.reference = *renamed
                    .get(&b.reference)
                    .ok_or_else(|| format!("Pixajoy's save gave no id for new box {}.", b.reference))?;
            }
            Ok(b)
        })
        .collect()
}

/// A box's position and size, with its line coordinates set to the same
/// rectangle.
fn geometry(x: f64, y: f64, w: f64, h: f64) -> Map<String, Value> {
    let at = |v: f64| json!(((v * UNIT).round() as i64).to_string());
    [
        ("boxleft", x),
        ("boxtop", y),
        ("boxwidth", w),
        ("boxheight", h),
        ("boxlinex1", x),
        ("boxliney1", y),
        ("boxlinex2", x + w),
        ("boxliney2", y + h),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), at(v)))
    .collect()
}

/// The fields that show `photo`'s crop in a `bw × bh` inch box, and the
/// asset they name.
fn fill(photo: &JobPhoto, bw: f64, bh: f64, assets: &HashMap<String, String>) -> Result<(Map<String, Value>, String), String> {
    let asset = assets.get(&photo.hash).ok_or_else(|| format!("Photo {} was not uploaded to Pixajoy.", photo.hash))?;
    let fit = crop_fit(photo.width, photo.height, photo.crop, bw, bh);
    let fields = Map::from_iter([
        ("hiresfiletype".into(), json!(106)),
        ("hirespath".into(), json!(asset)),
        ("hireswidth".into(), json!(photo.width)),
        ("hiresheight".into(), json!(photo.height)),
        ("rotatedhireswidth".into(), json!(photo.width)),
        ("rotatedhiresheight".into(), json!(photo.height)),
        ("magnification".into(), json!(fit.magnification)),
        ("boxscrollleft".into(), json!(fit.scroll_left)),
        ("boxscrolltop".into(), json!(fit.scroll_top)),
    ]);
    Ok((fields, asset.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;
    use crate::providers::{JobBox, JobCover, JobPage, JobPhoto};

    const LOAD: &str = include_str!("../../../tests/fixtures/pixajoy/load.json");
    const PAGE_W: f64 = 11.22047244094486;
    const PAGE_H: f64 = 8.74015748031494;

    fn layout() -> Layout {
        parse_load(&serde_json::from_str(LOAD).unwrap()).unwrap()
    }

    fn inches(v: &Value) -> f64 {
        v.as_str().unwrap().parse::<f64>().unwrap() / UNIT
    }

    const FULL: Rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

    fn image(hash: &str, width: u32, height: u32) -> JobPhoto {
        JobPhoto { hash: hash.into(), width, height, crop: FULL }
    }

    fn job(pages: Vec<JobPage>, cover: JobCover) -> PrintJob {
        PrintJob { name: "Trip".into(), page_count: pages.len() as u32, pages, cover }
    }

    fn assets(hashes: &[&str]) -> HashMap<String, String> {
        hashes.iter().enumerate().map(|(i, h)| (h.to_string(), format!("{}", 900 + i))).collect()
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

        let diff = save_diff(&l, &job(vec![], JobCover::default()), 20, &HashMap::new()).unwrap().body;

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
        let diff = save_diff(&l, &job(vec![], cover), 20, &assets(&photos)).unwrap().body;

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
            JobPage { number: 1, boxes: vec![JobBox { rect, photo: image("/t/a.jpg", 1200, 800) }] },
            JobPage { number: 2, boxes: vec![JobBox { rect, photo: image("/t/a.jpg", 1200, 800) }] },
            JobPage { number: 3, boxes: vec![JobBox { rect, photo: image("/t/a.jpg", 1200, 800) }] },
        ];
        let diff = save_diff(&l, &job(pages, JobCover::default()), 20, &assets(&["/t/a.jpg"])).unwrap().body;
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
            JobPage { number: 1, boxes: vec![] },
            JobPage {
                number: 2,
                boxes: vec![
                    JobBox { rect: full, photo: image("/t/a.jpg", 1200, 800) },
                    JobBox { rect: Rect::new(0.5, 0.5, 0.25, 0.25), photo: image("/t/b.jpg", 600, 600) },
                ],
            },
            JobPage { number: 3, boxes: vec![JobBox { rect: full, photo: image("/t/a.jpg", 1200, 800) }] },
        ];
        let ids = assets(&["/t/a.jpg", "/t/b.jpg"]);
        let diff = save_diff(&l, &job(pages, JobCover::default()), 20, &ids).unwrap().body;
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
        assert_eq!(b["hirespath"], json!(ids["/t/b.jpg"]));
        assert_eq!((b["hireswidth"].clone(), b["hiresheight"].clone()), (json!(600), json!(600)));
        assert_eq!(b["rotatedhireswidth"], json!(600));
        let fit = cover_fit(600, 600, 0.25 * PAGE_W, 0.25 * PAGE_H);
        assert!(close(b["magnification"].as_f64().unwrap(), fit.magnification, 1e-9));
        assert!(close(b["boxscrolltop"].as_f64().unwrap(), fit.scroll_top, 1e-9));
        assert_eq!(added["-1"]["pagenumber"], json!(2));
        assert_eq!(added["-2"]["pagenumber"], json!(2));
        assert_eq!(added["-3"]["pagenumber"], json!(3));
    }

    /// Pixajoy's IHL8511 cover at 40 pages, from the fixture: a 0.807 in
    /// wrap round two 11.280 in boards and a 0.354 in spine.
    const COVER_W: f64 = 24.52755905511807;
    const COVER_H: f64 = 10.47244094488187;
    const PANEL_W: f64 = 0.80708661417322 + 11.27952755905511;

    fn rect_of(b: &Value) -> [f64; 4] {
        ["boxleft", "boxtop", "boxwidth", "boxheight"].map(|k| inches(&b[k]))
    }

    #[test]
    fn the_cover_canvas_comes_from_the_load() {
        let c = layout().cover.unwrap();
        assert!(close(c.width, COVER_W, 1e-9) && close(c.height, COVER_H, 1e-9));
        assert!(close(c.panel_w, PANEL_W, 1e-9));
    }

    #[test]
    fn each_cover_photo_fills_its_whole_panel_wrap_included() {
        let l = layout();
        let photos = ["/t/front.jpg", "/t/back.jpg"];
        let ids = assets(&photos);
        let cover = JobCover { front: Some(image(photos[0], 1200, 1040)), back: Some(image(photos[1], 700, 700)) };
        let diff = save_diff(&l, &job(vec![], cover), 20, &ids).unwrap().body;
        let changed = diff["items"]["changed"].as_object().unwrap();
        assert_eq!(changed.len(), 2);

        // The rightmost template box is the front; it moves onto the right panel.
        let front = &changed["82448"];
        assert_eq!(front["hirespath"], json!(ids[photos[0]]));
        let want = [COVER_W - PANEL_W, 0.0, PANEL_W, COVER_H];
        for (got, want) in rect_of(front).iter().zip(want) {
            assert!(close(*got, want, 1e-9), "front {:?} != {want:?}", rect_of(front));
        }
        assert!(close(inches(&front["boxlinex2"]), COVER_W, 1e-9));
        let fit = cover_fit(1200, 1040, PANEL_W, COVER_H);
        assert!(close(front["magnification"].as_f64().unwrap(), fit.magnification, 1e-9));
        assert!(close(front["boxscrollleft"].as_f64().unwrap(), fit.scroll_left, 1e-9));

        let back = &changed["82449"];
        assert_eq!(back["hirespath"], json!(ids[photos[1]]));
        for (got, want) in rect_of(back).iter().zip([0.0, 0.0, PANEL_W, COVER_H]) {
            assert!(close(*got, want, 1e-9), "back {:?}", rect_of(back));
        }
    }

    #[test]
    fn a_cover_photo_without_the_cover_canvas_is_an_error() {
        let mut l = layout();
        l.cover = None;
        let cover = JobCover { front: Some(image("/t/front.jpg", 900, 700)), back: None };
        let err = save_diff(&l, &job(vec![], cover), 20, &assets(&["/t/front.jpg"])).unwrap_err();
        assert!(err.contains("cover"), "{err}");
    }

    #[test]
    fn an_empty_cover_side_leaves_its_box_alone() {
        let l = layout();
        let cover = JobCover { front: None, back: Some(image("/t/back.jpg", 700, 700)) };
        let diff = save_diff(&l, &job(vec![], cover), 20, &assets(&["/t/back.jpg"])).unwrap().body;
        let changed = diff["items"]["changed"].as_object().unwrap();
        assert_eq!(changed.keys().collect::<Vec<_>>(), vec!["82449"]);
    }

    #[test]
    fn a_page_beyond_the_project_is_an_error() {
        let l = layout();
        let pages = vec![JobPage {
            number: 21,
            boxes: vec![JobBox { rect: Rect::new(0.0, 0.0, 1.0, 1.0), photo: image("/t/a.jpg", 10, 10) }],
        }];
        assert!(save_diff(&l, &job(pages, JobCover::default()), 20, &assets(&["/t/a.jpg"])).is_err());
    }

    #[test]
    fn an_image_with_no_uploaded_asset_is_an_error() {
        let l = layout();
        let pages = vec![JobPage {
            number: 1,
            boxes: vec![JobBox { rect: Rect::new(0.0, 0.0, 1.0, 1.0), photo: image("/t/a.jpg", 10, 10) }],
        }];
        assert!(save_diff(&l, &job(pages, JobCover::default()), 20, &HashMap::new()).is_err());
    }

    #[test]
    fn the_diff_has_the_save_envelope() {
        let diff = save_diff(&layout(), &job(vec![], JobCover::default()), 20, &HashMap::new()).unwrap().body;
        assert_eq!(diff["autoSave"], json!(false));
        assert_eq!(diff["projectName"], json!("Trip"));
        for k in ["pages", "events"] {
            assert_eq!(diff[k], json!({ "deleted": {}, "added": {}, "changed": {} }));
        }
        assert_eq!(diff["itemsToPages"], json!({}));
    }

    #[test]
    fn the_whole_photo_is_the_centred_cover_fit() {
        let (a, b) = (crop_fit(1200, 800, FULL, PAGE_W, PAGE_H), cover_fit(1200, 800, PAGE_W, PAGE_H));
        assert!(close(a.magnification, 116.842, 0.001), "{a:?}");
        for (x, y) in [(a.magnification, b.magnification), (a.scroll_left, b.scroll_left), (a.scroll_top, b.scroll_top)] {
            assert!(close(x, y, 1e-9), "{a:?} != {b:?}");
        }
    }

    #[test]
    fn a_crop_zooms_in_and_pans_to_its_window() {
        // Each half of a 2:1 photo fills a square box at twice the fit scale.
        let left = crop_fit(2000, 1000, Rect::new(0.0, 0.0, 0.5, 1.0), 5.0, 5.0);
        assert!(close(left.magnification, 200.0, 1e-9), "{left:?}");
        assert!(close(left.scroll_left, 0.0, 1e-9) && close(left.scroll_top, 0.0, 1e-9), "{left:?}");
        let right = crop_fit(2000, 1000, Rect::new(0.5, 0.0, 0.5, 1.0), 5.0, 5.0);
        assert!(close(right.scroll_left, -5.0, 1e-9), "{right:?}");
        // The middle quarter of a square photo: 2x, panned a quarter in each way.
        let middle = crop_fit(1000, 1000, Rect::new(0.25, 0.25, 0.5, 0.5), 4.0, 4.0);
        assert!(close(middle.magnification, 200.0, 1e-9), "{middle:?}");
        assert!(close(middle.scroll_left, -2.0, 1e-9) && close(middle.scroll_top, -2.0, 1e-9), "{middle:?}");
    }

    #[test]
    fn a_box_shows_its_photos_crop_and_the_send_records_every_photo_box() {
        let l = layout();
        let crop = Rect::new(0.5, 0.0, 0.5, 1.0);
        let rect = Rect::new(0.1, 0.1, 0.4, 0.4);
        let cropped = JobPhoto { hash: "a".into(), width: 2000, height: 1000, crop };
        let pages = vec![JobPage { number: 2, boxes: vec![JobBox { rect, photo: cropped }], }];
        let cover = JobCover { front: Some(image("f", 1200, 1040)), back: None };
        let ids = assets(&["a", "f"]);
        let diff = save_diff(&l, &job(pages, cover), 20, &ids).unwrap();

        let b = &diff.body["items"]["added"]["-1"];
        let fit = crop_fit(2000, 1000, crop, 0.4 * PAGE_W, 0.4 * PAGE_H);
        assert!(close(b["magnification"].as_f64().unwrap(), fit.magnification, 1e-9));
        assert!(close(b["boxscrollleft"].as_f64().unwrap(), fit.scroll_left, 1e-9));
        assert_ne!(fit, crop_fit(2000, 1000, FULL, 0.4 * PAGE_W, 0.4 * PAGE_H), "the crop must matter");

        assert_eq!(
            diff.boxes,
            vec![
                SentBox { reference: -1, slot: SentSlot::Page { number: 2, rect }, hash: "a".into(), asset_id: ids["a"].clone(), crop },
                SentBox {
                    reference: 82448,
                    slot: SentSlot::Cover { side: CoverSide::Front },
                    hash: "f".into(),
                    asset_id: ids["f"].clone(),
                    crop: FULL,
                },
            ]
        );
    }

    #[test]
    fn new_boxes_take_the_refs_the_save_gave_them() {
        let sent = |reference: i64| SentBox {
            reference,
            slot: SentSlot::Cover { side: CoverSide::Back },
            hash: "h".into(),
            asset_id: "1".into(),
            crop: FULL,
        };
        let reply = json!({ "pageItems": [{ "oldRef": -2, "newRef": 501, "id": 9 }, { "oldRef": -1, "newRef": 500, "id": 8 }] });
        let resolved = resolve_refs(vec![sent(-1), sent(-2), sent(82448)], &reply).unwrap();
        assert_eq!(resolved.iter().map(|b| b.reference).collect::<Vec<_>>(), vec![500, 501, 82448]);
        assert!(resolve_refs(vec![sent(-3)], &reply).is_err());
    }
}
