//! Builds a [`PrintJob`] from a book and the files its export wrote.

use super::{JobBox, JobCover, JobImage, JobPage, JobText, PrintJob};
use crate::book::cull::Photo;
use crate::book::pace::Book;
use crate::book::titles::Chapter;
use crate::export::{cover_filename, output_filename};
use crate::geometry::CoverSide;
use std::collections::HashMap;
use std::path::PathBuf;

/// The sidecar's export records keyed by the item filename they answer (the
/// written path's stem). Any failed record fails the whole job: a provider
/// project with a silently missing photo is worse than no project.
pub fn exported_files(records: &[serde_json::Value]) -> Result<HashMap<String, JobImage>, String> {
    records.iter().map(exported_file).collect()
}

fn exported_file(r: &serde_json::Value) -> Result<(String, JobImage), String> {
    if r["type"] != "ok" {
        let name = r["filename"].as_str().unwrap_or("a photo");
        let why = r["message"].as_str().unwrap_or("export failed");
        return Err(format!("{name} could not be exported: {why}"));
    }
    let path = PathBuf::from(r["path"].as_str().ok_or("An export record has no path.")?);
    let stem = path.file_stem().and_then(|s| s.to_str()).ok_or("An export record has no file name.")?.to_string();
    let size = |k: &str| r[k].as_u64().map(|n| n as u32).ok_or_else(|| format!("{stem} has no {k}."));
    let image = JobImage { width: size("width")?, height: size("height")?, path };
    Ok((stem, image))
}

pub fn build_job(
    name: &str,
    book: &Book,
    photos: &[Photo],
    files: &HashMap<String, JobImage>,
    chapters: &[Chapter],
) -> Result<PrintJob, String> {
    let mut texts: HashMap<usize, Vec<JobText>> = HashMap::new();
    for c in chapters {
        if let Some(t) = &c.title_box {
            let text = JobText { rect: t.rect, text: c.title.clone(), align: t.align, point_size: t.point_size };
            texts.entry(t.page).or_default().push(text);
        }
    }
    let file = |filename: String| {
        files.get(&filename).cloned().ok_or_else(|| format!("{filename} was not exported."))
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
                .map(|pl| {
                    let image = file(output_filename(page.number, pl.z, &photos[pl.photo_index].hash))?;
                    Ok(JobBox { rect: pl.slot_rect, image })
                })
                .collect::<Result<_, String>>()?;
            Ok(JobPage { number: page.number, boxes, texts: texts.remove(&i).unwrap_or_default() })
        })
        .collect::<Result<_, String>>()?;
    let side = |side: CoverSide| {
        book.cover.side(side).map(|c| file(cover_filename(side, &photos[c.photo_index].hash))).transpose()
    };
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

    fn image(path: &str, width: u32, height: u32) -> JobImage {
        JobImage { path: PathBuf::from(path), width, height }
    }

    #[test]
    fn exported_files_are_keyed_by_the_written_file_stem() {
        let files = exported_files(&[ok("/tmp/x/p01-z1-aaaaaaaa.jpg", 1200, 800)]).unwrap();
        assert_eq!(files["p01-z1-aaaaaaaa"], image("/tmp/x/p01-z1-aaaaaaaa.jpg", 1200, 800));
    }

    #[test]
    fn a_failed_export_record_fails_the_job_and_names_the_file() {
        let err = exported_files(&[
            ok("/tmp/x/p01-z1-aaaaaaaa.jpg", 1200, 800),
            json!({ "type": "failed", "filename": "p02-z1-bbbbbbbb", "message": "unreadable" }),
        ])
        .unwrap_err();
        assert!(err.contains("p02-z1-bbbbbbbb") && err.contains("unreadable"), "{err}");
    }

    #[test]
    fn each_placement_gets_its_own_exported_file_and_slot_in_z_order() {
        let photos = vec![photo("aaaaaaaa11"), photo("bbbbbbbb22")];
        let low = Rect::new(0.0, 0.0, 1.0, 1.0);
        let high = Rect::new(0.6, 0.6, 0.3, 0.3);
        // Stored out of z order, so a builder that keeps storage order fails.
        let b = book(
            vec![page(1, vec![]), page(2, vec![placement(1, 2, high), placement(0, 1, low)])],
            None,
        );
        let files = exported_files(&[
            ok("/t/p02-z1-aaaaaaaa.jpg", 1200, 800),
            ok("/t/p02-z2-bbbbbbbb.jpg", 600, 600),
        ])
        .unwrap();

        let job = build_job("Trip", &b, &photos, &files, &[]).unwrap();

        assert_eq!(job.name, "Trip");
        assert_eq!(job.page_count, 2);
        assert_eq!(job.pages[0], JobPage { number: 1, boxes: vec![], texts: vec![] });
        assert_eq!(
            job.pages[1].boxes,
            vec![
                JobBox { rect: low, image: image("/t/p02-z1-aaaaaaaa.jpg", 1200, 800) },
                JobBox { rect: high, image: image("/t/p02-z2-bbbbbbbb.jpg", 600, 600) },
            ]
        );
        assert_eq!(job.cover, JobCover::default());
    }

    #[test]
    fn the_cover_front_comes_from_its_own_exported_file() {
        let photos = vec![photo("aaaaaaaa11"), photo("cccccccc33")];
        let b = book(vec![page(1, vec![placement(0, 1, Rect::new(0.0, 0.0, 1.0, 1.0))])], Some(1));
        let files = exported_files(&[
            ok("/t/p01-z1-aaaaaaaa.jpg", 1200, 800),
            ok("/t/cover-front-cccccccc.jpg", 900, 700),
        ])
        .unwrap();

        let job = build_job("Trip", &b, &photos, &files, &[]).unwrap();

        assert_eq!(job.cover.front, Some(image("/t/cover-front-cccccccc.jpg", 900, 700)));
        assert_eq!(job.cover.back, None);
    }

    #[test]
    fn a_placement_with_no_exported_file_is_an_error() {
        let photos = vec![photo("aaaaaaaa11")];
        let b = book(vec![page(1, vec![placement(0, 1, Rect::new(0.0, 0.0, 1.0, 1.0))])], None);
        let err = build_job("Trip", &b, &photos, &HashMap::new(), &[]).unwrap_err();
        assert!(err.contains("p01-z1-aaaaaaaa"), "{err}");
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
