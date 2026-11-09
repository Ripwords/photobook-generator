//! Builds a [`PrintJob`] from a book, and the whole-photo exports a
//! provider uploads.

use super::{JobBox, JobCover, JobPage, JobPhoto, JobText, PrintJob, Upload};
use crate::book::cull::Photo;
use crate::book::pace::Book;
use crate::book::titles::Chapter;
use crate::geometry::{CoverSide, Rect};
use crate::protocol::ExportItem;
use std::collections::HashMap;
use std::path::PathBuf;

const UPLOAD_PREFIX: &str = "photo-";
const WHOLE: Rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

/// Every photo the book shows, pages then cover, each once by content hash.
pub fn book_hashes(book: &Book, photos: &[Photo]) -> Vec<String> {
    let pages = book.pages.iter().flat_map(|p| p.placements.iter().map(|pl| pl.photo_index));
    let cover = [CoverSide::Front, CoverSide::Back].into_iter().filter_map(|s| book.cover.side(s).map(|c| c.photo_index));
    let mut hashes: Vec<String> = Vec::new();
    for i in pages.chain(cover) {
        let hash = &photos[i].hash;
        if !hashes.contains(hash) {
            hashes.push(hash.clone());
        }
    }
    hashes
}

/// One export per hash: the whole photo, upright, named by its hash so the
/// record maps back to it.
pub fn upload_items(photos: &[Photo], hashes: &[String]) -> Result<Vec<ExportItem>, String> {
    hashes
        .iter()
        .map(|hash| {
            let photo = photos.iter().find(|p| &p.hash == hash).ok_or_else(|| format!("Photo {hash} is not in the book."))?;
            Ok(ExportItem {
                source_path: photo.path.clone(),
                filename: format!("{UPLOAD_PREFIX}{hash}"),
                crop_x: WHOLE.x,
                crop_y: WHOLE.y,
                crop_w: WHOLE.w,
                crop_h: WHOLE.h,
            })
        })
        .collect()
}

/// The sidecar's records for [`upload_items`]. Any failed record fails the
/// whole send: a provider project with a silently missing photo is worse
/// than no project.
pub fn uploads(records: &[serde_json::Value]) -> Result<Vec<Upload>, String> {
    records.iter().map(upload).collect()
}

fn upload(r: &serde_json::Value) -> Result<Upload, String> {
    if r["type"] != "ok" {
        let name = r["filename"].as_str().unwrap_or("a photo");
        let why = r["message"].as_str().unwrap_or("export failed");
        return Err(format!("{name} could not be exported: {why}"));
    }
    let path = PathBuf::from(r["path"].as_str().ok_or("An export record has no path.")?);
    let stem = path.file_stem().and_then(|s| s.to_str()).ok_or("An export record has no file name.")?;
    let hash = stem.strip_prefix(UPLOAD_PREFIX).ok_or_else(|| format!("{stem} is not a photo upload."))?.to_string();
    let size = |k: &str| r[k].as_u64().map(|n| n as u32).ok_or_else(|| format!("{stem} has no {k}."));
    Ok(Upload { width: size("width")?, height: size("height")?, hash, path })
}

/// `sizes` holds the upright pixel size of every photo's upload, by hash.
pub fn build_job(
    name: &str,
    book: &Book,
    photos: &[Photo],
    sizes: &HashMap<String, (u32, u32)>,
    chapters: &[Chapter],
) -> Result<PrintJob, String> {
    let mut texts: HashMap<usize, Vec<JobText>> = HashMap::new();
    for c in chapters {
        if let Some(t) = &c.title_box {
            let text = JobText { rect: t.rect, text: c.title.clone(), align: t.align, point_size: t.point_size };
            texts.entry(t.page).or_default().push(text);
        }
    }
    let photo = |index: usize, crop: Rect| {
        let hash = &photos[index].hash;
        let &(width, height) = sizes.get(hash).ok_or_else(|| format!("Photo {hash} has no upload."))?;
        Ok::<_, String>(JobPhoto { hash: hash.clone(), width, height, crop })
    };
    let pages = book
        .pages
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let mut placements: Vec<_> = page.placements.iter().collect();
            placements.sort_by_key(|pl| pl.z);
            let boxes = placements
                .into_iter()
                .map(|pl| Ok(JobBox { rect: pl.slot_rect, photo: photo(pl.photo_index, pl.crop)? }))
                .collect::<Result<_, String>>()?;
            Ok(JobPage { number: page.number, boxes, texts: texts.remove(&i).unwrap_or_default() })
        })
        .collect::<Result<_, String>>()?;
    let side = |side: CoverSide| book.cover.side(side).map(|c| photo(c.photo_index, c.crop)).transpose();
    Ok(PrintJob {
        name: name.to_string(),
        page_count: book.pages.len() as u32,
        pages,
        cover: JobCover { front: side(CoverSide::Front)?, back: side(CoverSide::Back)? },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cover::{Cover, CoverPhoto};
    use crate::book::pace::{Page, Placement};
    use crate::book::titles::{Chapter, TitleBox};
    use crate::templates::Align;
    use crate::geometry::{Rect, Side};
    use crate::print_spec::pixajoy_spec;
    use serde_json::json;

    fn photo(hash: &str) -> Photo {
        Photo {
            path: format!("/src/{hash}.heic"),
            hash: hash.into(),
            width: 4032,
            height: 3024,
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 0,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None,
            utc_offset: None,
            warmth: None,
            horizon_tilt_deg: None,
        }
    }

    fn placement(photo_index: usize, z: u32, slot: Rect) -> Placement {
        Placement { photo_index, slot_rect: slot, crop: Rect::new(0.0, 0.0, 1.0, 1.0), z }
    }

    fn page(number: u32, placements: Vec<Placement>) -> Page {
        let side = if number % 2 == 1 { Side::Right } else { Side::Left };
        Page { number, side, template_id: "t".into(), placements }
    }

    fn book(pages: Vec<Page>, front: Option<usize>) -> Book {
        Book {
            titles: Default::default(),
            spec: pixajoy_spec(),
            options: Default::default(),
            cover: Cover {
                front: front.map(|photo_index| CoverPhoto { photo_index, crop: Rect::new(0.0, 0.0, 1.0, 1.0) }),
                back: None,
                spine: Default::default(),
            },
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages,
        }
    }

    fn ok(path: &str, width: u32, height: u32) -> serde_json::Value {
        json!({ "type": "ok", "path": path, "width": width, "height": height, "bytes": 10 })
    }

    const FULL: Rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

    fn sizes(entries: &[(&str, u32, u32)]) -> HashMap<String, (u32, u32)> {
        entries.iter().map(|&(h, w, ht)| (h.to_string(), (w, ht))).collect()
    }

    #[test]
    fn each_photo_is_uploaded_once_whole_however_often_the_book_shows_it() {
        let photos = vec![photo("aa"), photo("bb"), photo("cc"), photo("dd")];
        let half = Rect::new(0.0, 0.0, 0.5, 1.0);
        // "aa" twice on the pages and again on the back; "dd" is never shown.
        let mut b = book(
            vec![page(1, vec![placement(0, 1, half)]), page(2, vec![placement(1, 1, FULL), placement(0, 2, FULL)])],
            Some(2),
        );
        b.cover.back = Some(CoverPhoto { photo_index: 0, crop: half });
        let hashes = book_hashes(&b, &photos);
        assert_eq!(hashes, vec!["aa", "bb", "cc"]);

        let items = upload_items(&photos, &hashes[1..]).unwrap();
        assert_eq!(items.iter().map(|i| (i.source_path.as_str(), i.filename.as_str())).collect::<Vec<_>>(), vec![
            ("/src/bb.heic", "photo-bb"),
            ("/src/cc.heic", "photo-cc")
        ]);
        assert!(items.iter().all(|i| (i.crop_x, i.crop_y, i.crop_w, i.crop_h) == (0.0, 0.0, 1.0, 1.0)));
    }

    #[test]
    fn an_upload_is_read_back_by_the_hash_in_its_name() {
        let got = uploads(&[ok("/tmp/x/photo-aa11.jpg", 4032, 3024)]).unwrap();
        assert_eq!(got, vec![Upload { hash: "aa11".into(), path: "/tmp/x/photo-aa11.jpg".into(), width: 4032, height: 3024 }]);
        assert!(uploads(&[ok("/tmp/x/p01-z1-aa.jpg", 1, 1)]).is_err(), "not an upload's name");
    }

    #[test]
    fn a_failed_export_record_fails_the_send_and_names_the_file() {
        let err = uploads(&[
            ok("/tmp/x/photo-aa.jpg", 1200, 800),
            json!({ "type": "failed", "filename": "photo-bb", "message": "unreadable" }),
        ])
        .unwrap_err();
        assert!(err.contains("photo-bb") && err.contains("unreadable"), "{err}");
    }

    #[test]
    fn each_placement_shows_its_photo_and_crop_in_z_order() {
        let photos = vec![photo("aaaaaaaa11"), photo("bbbbbbbb22")];
        let low = Rect::new(0.0, 0.0, 1.0, 1.0);
        let high = Rect::new(0.6, 0.6, 0.3, 0.3);
        let crop = Rect::new(0.1, 0.2, 0.3, 0.4);
        // Stored out of z order, so a builder that keeps storage order fails.
        let b = book(
            vec![page(1, vec![]), page(2, vec![placement(1, 2, high), Placement { crop, ..placement(0, 1, low) }])],
            None,
        );
        let s = sizes(&[("aaaaaaaa11", 4032, 3024), ("bbbbbbbb22", 600, 600)]);

        let job = build_job("Trip", &b, &photos, &s, &[]).unwrap();

        assert_eq!(job.name, "Trip");
        assert_eq!(job.page_count, 2);
        assert_eq!(job.pages[0], JobPage { number: 1, boxes: vec![], texts: vec![] });
        let photo = |hash: &str, width, height, crop| JobPhoto { hash: hash.into(), width, height, crop };
        assert_eq!(
            job.pages[1].boxes,
            vec![
                JobBox { rect: low, photo: photo("aaaaaaaa11", 4032, 3024, crop) },
                JobBox { rect: high, photo: photo("bbbbbbbb22", 600, 600, FULL) },
            ]
        );
        assert_eq!(job.cover, JobCover::default());
    }

    #[test]
    fn the_cover_front_shows_its_own_photo_and_crop() {
        let photos = vec![photo("aaaaaaaa11"), photo("cccccccc33")];
        let crop = Rect::new(0.2, 0.0, 0.6, 1.0);
        let mut b = book(vec![page(1, vec![placement(0, 1, FULL)])], Some(1));
        b.cover.front = Some(CoverPhoto { photo_index: 1, crop });
        let s = sizes(&[("aaaaaaaa11", 1200, 800), ("cccccccc33", 900, 700)]);

        let job = build_job("Trip", &b, &photos, &s, &[]).unwrap();

        assert_eq!(job.cover.front, Some(JobPhoto { hash: "cccccccc33".into(), width: 900, height: 700, crop }));
        assert_eq!(job.cover.back, None);
    }

    #[test]
    fn a_photo_with_no_upload_is_an_error() {
        let photos = vec![photo("aaaaaaaa11")];
        let b = book(vec![page(1, vec![placement(0, 1, FULL)])], None);
        let err = build_job("Trip", &b, &photos, &HashMap::new(), &[]).unwrap_err();
        assert!(err.contains("aaaaaaaa11"), "{err}");
    }

    fn chapter(number: usize, title: &str, title_box: Option<TitleBox>) -> Chapter {
        Chapter {
            number,
            event: number as u32,
            events: vec![number as u32],
            opening: number - 1,
            pages: Vec::new(),
            start: None,
            end: None,
            auto_title: title.into(),
            title: title.into(),
            custom: false,
            title_box,
        }
    }

    #[test]
    fn each_printed_chapter_title_lands_on_its_book_page() {
        let photos = vec![photo("aaaaaaaa11")];
        let b = book(vec![page(1, vec![]), page(2, vec![]), page(3, vec![])], None);
        let rect = Rect::new(0.1, 0.05, 0.8, 0.1);
        // `TitleBox::page` indexes `Book::pages`; the job wants page numbers.
        let kyoto = TitleBox { page: 2, rect, align: Align::Right, point_size: 22.0, overlaps_photo: false };
        let chapters = [chapter(1, "Untitled", None), chapter(2, "Kyoto", Some(kyoto))];

        let job = build_job("Trip", &b, &photos, &HashMap::new(), &chapters).unwrap();

        assert!(job.pages[0].texts.is_empty() && job.pages[1].texts.is_empty());
        assert_eq!(
            job.pages[2].texts,
            vec![JobText { rect, text: "Kyoto".into(), align: Align::Right, point_size: 22.0 }]
        );
    }
}
