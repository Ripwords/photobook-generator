//! `AgentView`: the only shape of a book a model is ever shown.
//!
//! Design §9.1 lists what may leave the machine and what may not. This file
//! is that table as code: the input carries paths, hashes, face boxes and
//! absolute times, and the output types have no field that could hold any of
//! them. Every read tool and every write tool's result is built here.

use crate::book::cull::{from_cached_features, Photo};
use crate::book::edit::{alternatives, opening_count, opening_pages};
use crate::book::pace::Book;
use crate::book::chapter::{split_chapters, PLACE_SPLIT_KM};
use crate::ranking::percentiles;
use crate::templates::Library;
use serde::Serialize;

/// One photo of a saved book's slice, with the raw scores its percentiles
/// are ranked from.
///
/// The raw scores sit beside `Photo` because the cached records a saved book
/// resolves from never carry `aestheticPct`/`sharpnessPct`:
/// `from_cached_features` zeroes them. Ranking a book therefore has to start
/// from the scores Swift wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePhoto {
    pub photo: Photo,
    pub aesthetic_score: f64,
    pub sharpness: f64,
}

impl SourcePhoto {
    /// Parses one cached feature record. `None` when it lacks a field the
    /// view needs; the caller fails the whole call, because a skipped record
    /// would renumber every photo id after it.
    pub fn from_record(record: &serde_json::Value) -> Option<Self> {
        Some(Self {
            photo: from_cached_features(record)?,
            aesthetic_score: record["aestheticScore"].as_f64()?,
            sharpness: record["sharpness"].as_f64()?,
        })
    }
}

/// Vision's scene tags, at most `MAX`, in Vision's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Tags(Vec<String>);

impl Tags {
    pub const MAX: usize = 3;

    fn top(all: &[String]) -> Self {
        Self(all.iter().take(Self::MAX).cloned().collect())
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPhoto {
    /// The `photoIndex` `BookLayout` uses, so a tool call can name a photo.
    pub id: usize,
    /// Whole days since the book's earliest dated photo. `None` when undated.
    pub day: Option<u64>,
    /// Event cluster within this book's photos.
    pub event: usize,
    pub faces: usize,
    pub face_area: f64,
    pub tags: Tags,
    /// Percentile within this book's photos, not the analysed folder.
    pub aesthetic_pct: u8,
    pub sharpness_pct: u8,
    pub placed: bool,
}

/// One placement, named the way `edit::PlacementRef` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSlot {
    pub page: u32,
    pub z: u32,
    pub photo: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOpening {
    /// Numbered as `book::edit` numbers openings.
    pub index: usize,
    /// Printed page numbers, in reading order.
    pub pages: Vec<u32>,
    pub template_id: String,
    pub locked: bool,
    pub alternatives: Vec<String>,
    pub slots: Vec<AgentSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub page_count: usize,
    pub placed_photos: usize,
    pub dropped_photos: usize,
    pub openings: Vec<AgentOpening>,
    pub photos: Vec<AgentPhoto>,
}

/// The whole view of one saved book. `photos` is the slice
/// `Placement::photo_index` indexes into.
pub fn agent_view(book: &Book, lib: &Library, photos: &[SourcePhoto]) -> AgentView {
    let openings: Vec<AgentOpening> = (0..opening_count(book))
        .filter_map(|index| {
            let pages = opening_pages(book, index)?;
            Some(AgentOpening {
                index,
                pages: pages.iter().map(|&i| book.pages[i].number).collect(),
                template_id: book.pages[pages[0]].template_id.clone(),
                locked: book.controls.get(&index).is_some_and(|c| c.locked),
                alternatives: alternatives(book, lib, index),
                slots: pages
                    .iter()
                    .flat_map(|&i| {
                        let page = &book.pages[i];
                        page.placements.iter().map(|p| AgentSlot {
                            page: page.number,
                            z: p.z,
                            photo: p.photo_index,
                        })
                    })
                    .collect(),
            })
        })
        .collect();

    let placed: std::collections::BTreeSet<usize> = book
        .pages
        .iter()
        .flat_map(|p| p.placements.iter().map(|pl| pl.photo_index))
        .collect();
    let times: Vec<Option<i64>> = photos.iter().map(|p| p.photo.captured_at).collect();
    let earliest = times.iter().flatten().min().copied();
    let locations: Vec<_> = photos.iter().map(|p| p.photo.location).collect();
    let split_km = book.options.places.then_some(PLACE_SPLIT_KM);
    let events = split_chapters(&times, &locations, split_km);
    let aesthetic = percentiles(&photos.iter().map(|p| p.aesthetic_score).collect::<Vec<_>>());
    let sharpness = percentiles(&photos.iter().map(|p| p.sharpness).collect::<Vec<_>>());

    let agent_photos = photos
        .iter()
        .enumerate()
        .map(|(id, source)| AgentPhoto {
            id,
            // `t >= earliest`, so integer division is the floor.
            day: source
                .photo
                .captured_at
                .zip(earliest)
                .map(|(t, first)| ((t - first) / 86_400) as u64),
            event: events[id] as usize,
            faces: source.photo.faces.len(),
            face_area: source.photo.face_area_fraction,
            tags: Tags::top(&source.photo.scene_tags),
            aesthetic_pct: aesthetic[id],
            sharpness_pct: sharpness[id],
            placed: placed.contains(&id),
        })
        .collect();

    AgentView {
        page_count: book.pages.len(),
        placed_photos: book.pages.iter().map(|p| p.placements.len()).sum(),
        dropped_photos: book.dropped,
        openings,
        photos: agent_photos,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;
    use crate::book::cull::Overrides;
    use crate::book::pace::{assemble, Page, Placement, BLANK_TEMPLATE_ID};
    use crate::geometry::{Rect, Side};
    use crate::templates::Weights;
    use serde_json::json;

    const T0: i64 = 1_726_012_345;

    /// A cached feature record in the shape Swift writes it, every key
    /// present, so `SourcePhoto::from_record` parses what production parses.
    fn record(
        path: &str,
        hash: &str,
        (width, height): (u32, u32),
        captured_at: Option<i64>,
        tags: &[&str],
        (aesthetic, sharpness): (f64, f64),
    ) -> serde_json::Value {
        let mut exif = json!({
            "latitude": 51.501_364,
            "longitude": -0.141_889,
            "pixelWidth": width,
            "pixelHeight": height,
            "make": "Canon",
            "model": "EOS R5",
            "flashFired": false,
        });
        if let Some(t) = captured_at {
            exif["captureDate"] = json!(t as f64);
        }
        json!({
            "path": path,
            "hash": hash,
            "width": width,
            "height": height,
            "exif": exif,
            "isUtility": false,
            "aestheticScore": aesthetic,
            "sharpness": sharpness,
            "faces": [{ "box": [0.1234, 0.2345, 0.0913, 0.1177], "captureQuality": 0.6621 }],
            "faceAreaFraction": 0.0427,
            "smileFraction": 0.5,
            "saliencyBox": [0.6123, 0.0789, 0.2718, 0.3141],
            "horizonTiltDeg": 1.75,
            "sceneTags": tags,
            "clippedLow": 0.0, "clippedHigh": 0.0,
            "hasText": false,
            "palette": [{ "r": 0.8123, "g": 0.4456, "b": 0.2789, "weight": 0.5512 }],
            "warmth": 0.3319,
            "contrast": 0.4471,
            "phash": 12_345_678_901_234_u64,
            "thumbnailPath": "/Users/alice/Library/Caches/pbg/thumbs/c0ffee.jpg",
        })
    }

    fn source(value: &serde_json::Value) -> SourcePhoto {
        SourcePhoto::from_record(value).expect("fixture record parses")
    }

    fn placement(photo_index: usize, z: u32) -> Placement {
        Placement {
            photo_index,
            slot_rect: Rect::new(0.06, 0.08, 0.42, 0.84),
            crop: Rect::new(0.1, 0.0, 0.75, 1.0),
            z,
        }
    }

    fn page(number: u32, side: Side, template_id: &str, placements: Vec<Placement>) -> Page {
        Page {
            number,
            side,
            template_id: template_id.into(),
            placements,
        }
    }

    /// Page 1 alone, one two-page spread, the last page alone. Photo indices
    /// are out of order so a view that numbers slots positionally differs.
    pub(crate) fn small_book() -> Book {
        let mut book = Book {
            spec: pixajoy_spec(),
            cover: Default::default(),
            pages: vec![
                page(
                    1,
                    Side::Right,
                    "07-two-up-symmetric-margin:right",
                    vec![placement(3, 1)],
                ),
                page(
                    2,
                    Side::Left,
                    "07-two-up-symmetric-margin",
                    vec![placement(1, 1)],
                ),
                page(
                    3,
                    Side::Right,
                    "07-two-up-symmetric-margin",
                    vec![placement(0, 1)],
                ),
                page(4, Side::Left, BLANK_TEMPLATE_ID, Vec::new()),
            ],
            seed: 99,
            dropped: 1,
            controls: Default::default(),
            options: Default::default(),
        };
        book.controls.entry(0).or_default().locked = true;
        book
    }

    pub(crate) fn frozen_library() -> Library {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
        Library::load(&dir).expect("the frozen fixture library must decompose")
    }

    /// Four photos, never square, never the same shape twice. Photo 2 is the
    /// one the book left out and has nobody in it; photo 3 is undated.
    pub(crate) fn small_photos() -> Vec<SourcePhoto> {
        let mut empty = record(
            "/p/c.jpg",
            "hc",
            (6000, 4000),
            Some(T0 + 3_600),
            &[],
            (0.87, 40.0),
        );
        empty["faces"] = json!([]);
        empty["faceAreaFraction"] = json!(0.0);
        [
            record(
                "/p/a.jpg",
                "ha",
                (4032, 3024),
                Some(T0 + 129_600),
                &["beach", "sky", "people", "sand", "sea"],
                (0.61, 210.0),
            ),
            record(
                "/p/b.jpg",
                "hb",
                (3024, 4032),
                Some(T0),
                &["food"],
                (0.12, 950.0),
            ),
            empty,
            record(
                "/p/d.jpg",
                "hd",
                (4000, 6000),
                None,
                &["dog", "grass"],
                (0.33, 400.0),
            ),
        ]
        .iter()
        .map(source)
        .collect()
    }

    fn photo_by_id(view: &AgentView, id: usize) -> &AgentPhoto {
        view.photos
            .iter()
            .find(|p| p.id == id)
            .expect("every photo is in the view")
    }

    // --- the privacy chokepoint ---------------------------------------------

    /// The named regression: **something §9.1 withholds reaches the model.**
    /// A value search, not a key search: renaming a leaked field would pass a
    /// key search while the value still leaves the machine.
    ///
    /// Places is on, so the place-chapter path runs, and the secret photo
    /// carries a town name where one could plausibly arrive: a geocoded name
    /// stored on the record, or the IPTC city a phone writes into EXIF. The
    /// rounded cell `place_names` sends Apple is withheld too.
    #[test]
    fn agent_view_contains_no_path_hash_gps_timestamp_face_box_raw_score_or_place_name() {
        let mut secret = record(
            "/Users/alice/Pictures/Lisbon Trip/IMG_4821.HEIC",
            "9f2c41d7e8ab3605",
            (4284, 5712),
            Some(T0),
            &["beach", "sky", "people"],
            (0.873_142, 1_234.567),
        );
        secret["placeName"] = json!("Gion");
        secret["exif"]["city"] = json!("Kyoto");
        let other = record(
            "/p/b.jpg",
            "hb",
            (3024, 4032),
            Some(T0 + 129_600),
            &["food"],
            (0.12, 950.0),
        );
        let photos = vec![source(&secret), source(&other)];
        let book = Book {
            spec: pixajoy_spec(),
            cover: Default::default(),
            pages: vec![page(
                1,
                Side::Right,
                "07-two-up-symmetric-margin:right",
                vec![placement(0, 1)],
            )],
            seed: 1,
            dropped: 1,
            controls: Default::default(),
            options: crate::book::pace::BookOptions { places: true },
        };

        let text = serde_json::to_string(&agent_view(&book, &frozen_library(), &photos)).unwrap();

        let withheld = [
            "/Users/alice",
            "IMG_4821",
            "Lisbon",
            "HEIC",
            "9f2c41d7e8ab3605",
            "51.501364",
            "-0.141889",
            "0.141889",
            &T0.to_string(),
            &(T0 + 129_600).to_string(),
            "0.1234",
            "0.2345",
            "0.0913",
            "0.1177",
            "0.6621",
            "0.6123",
            "0.0789",
            "0.2718",
            "0.3141",
            "0.873142",
            "1234.567",
            "Canon",
            "EOS R5",
            "0.8123",
            "0.5512",
            "12345678901234",
            "c0ffee",
            "4284",
            "5712",
            "Gion",
            "Kyoto",
            "51.5",
            "-0.14",
        ];
        for value in withheld {
            assert!(
                !text.contains(value),
                "{value:?} leaked into the agent view: {text}"
            );
        }
        assert!(
            text.contains("0.0427"),
            "the fixture's face area is sent, so the search is live: {text}"
        );
    }

    // --- per-photo facts ------------------------------------------------------

    #[test]
    fn agent_view_counts_days_from_the_books_earliest_dated_photo() {
        let view = agent_view(&small_book(), &frozen_library(), &small_photos());

        assert_eq!(
            photo_by_id(&view, 1).day,
            Some(0),
            "the earliest dated photo is day 0"
        );
        assert_eq!(
            photo_by_id(&view, 2).day,
            Some(0),
            "one hour later is still day 0"
        );
        assert_eq!(
            photo_by_id(&view, 0).day,
            Some(1),
            "36 hours later is day 1"
        );
        assert_eq!(
            photo_by_id(&view, 3).day,
            None,
            "an undated photo has no day"
        );
    }

    #[test]
    fn agent_view_keeps_the_first_three_tags_in_visions_order() {
        let view = agent_view(&small_book(), &frozen_library(), &small_photos());

        assert_eq!(
            photo_by_id(&view, 0).tags.as_slice(),
            ["beach", "sky", "people"]
        );
        assert_eq!(photo_by_id(&view, 3).tags.as_slice(), ["dog", "grass"]);
        assert!(photo_by_id(&view, 2).tags.as_slice().is_empty());
    }

    /// A saved book's photos come back with `aesthetic_pct` and
    /// `sharpness_pct` zeroed, so reading them would rank every photo 0.
    #[test]
    fn agent_view_ranks_percentiles_from_raw_scores_within_the_book() {
        let photos = small_photos();
        assert!(
            photos.iter().all(|p| p.photo.aesthetic_pct == 0),
            "the cached path zeroes them"
        );

        let view = agent_view(&small_book(), &frozen_library(), &photos);
        let aesthetic: Vec<u8> = view.photos.iter().map(|p| p.aesthetic_pct).collect();
        let sharpness: Vec<u8> = view.photos.iter().map(|p| p.sharpness_pct).collect();

        assert_eq!(aesthetic, vec![75, 25, 100, 50]);
        assert_eq!(sharpness, vec![50, 100, 25, 75]);
    }

    #[test]
    fn agent_view_numbers_events_within_the_book_with_undated_photos_last() {
        let view = agent_view(&small_book(), &frozen_library(), &small_photos());
        let events: Vec<usize> = view.photos.iter().map(|p| p.event).collect();

        assert_eq!(events, vec![1, 0, 0, 2]);
    }

    #[test]
    fn agent_view_marks_which_photos_the_book_placed() {
        let view = agent_view(&small_book(), &frozen_library(), &small_photos());
        let placed: Vec<(usize, bool)> = view.photos.iter().map(|p| (p.id, p.placed)).collect();

        assert_eq!(placed, vec![(0, true), (1, true), (2, false), (3, true)]);
        assert_eq!(view.placed_photos, 3);
        assert_eq!(view.dropped_photos, 1);
        assert_eq!(view.page_count, 4);
    }

    #[test]
    fn agent_view_reports_faces_and_face_area() {
        let view = agent_view(&small_book(), &frozen_library(), &small_photos());

        assert_eq!(photo_by_id(&view, 0).faces, 1);
        assert_eq!(photo_by_id(&view, 0).face_area, 0.0427);
        assert_eq!(photo_by_id(&view, 2).faces, 0);
        assert_eq!(photo_by_id(&view, 2).face_area, 0.0);
    }

    /// A book laid out by place tells the agent the chapters it was laid out
    /// by. Four photos an hour apart are one time run; the last two are in
    /// Osaka, 43 km from Kyoto. Time alone is the guard.
    #[test]
    fn agent_view_events_follow_the_books_place_chapters() {
        let kyoto = crate::book::chapter::LatLon::new(35.0116, 135.7681);
        let osaka = crate::book::chapter::LatLon::new(34.6937, 135.5023);
        let photos: Vec<SourcePhoto> = (0..4)
            .map(|i| {
                let mut p = assembled_photo(i);
                p.photo.location = if i < 2 { kyoto } else { osaka };
                p
            })
            .collect();
        let events = |places: bool| {
            let mut book = small_book();
            book.options.places = places;
            agent_view(&book, &frozen_library(), &photos).photos.iter().map(|p| p.event).collect::<Vec<_>>()
        };

        assert_eq!(events(false), vec![0, 0, 0, 0]);
        assert_eq!(events(true), vec![0, 0, 1, 1]);
    }

    // --- openings ---------------------------------------------------------------

    fn assembled_photo(i: usize) -> SourcePhoto {
        let portrait = i % 3 == 2;
        let tags: Vec<String> = Vec::new();
        SourcePhoto {
            photo: Photo {
                path: format!("/e/p{i:02}.jpg"),
                hash: format!("h{i:02}"),
                width: if portrait { 3000 } else { 4000 },
                height: if portrait { 4000 } else { 3000 },
                is_utility: false,
                aesthetic_pct: ((i * 37) % 100) as u8,
                sharpness_pct: 50,
                near_dup_cluster: i as u32,
                event_cluster: 0,
                faces: Vec::new(),
                face_area_fraction: 0.0,
                saliency_box: None,
                palette: Vec::new(),
                capture_quality: None,
                scene_tags: tags,
                captured_at: Some(T0 + i as i64 * 600),
                clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
                location: None,
            },
            aesthetic_score: i as f64,
            sharpness: 1.0,
        }
    }

    /// The opening numbering is `book::edit`'s, which is what every edit
    /// command takes. A view numbered its own way would have the agent lock or
    /// regenerate the wrong spread.
    #[test]
    fn agent_view_openings_match_edit_opening_pages_for_a_20_page_book() {
        let lib = frozen_library();
        let photos: Vec<SourcePhoto> = (0..25).map(assembled_photo).collect();
        let engine: Vec<Photo> = photos.iter().map(|p| p.photo.clone()).collect();
        let book = assemble(&pixajoy_spec(),
            &engine,
            20,
            &lib,
            &Weights::default(),
            1234,
            &Overrides::new(),
        )
        .expect("no overrides");
        assert_eq!(book.pages.len(), 20);

        let view = agent_view(&book, &lib, &photos);

        assert_eq!(view.openings.len(), opening_count(&book));
        assert_eq!(view.openings.len(), 11, "page 1, nine spreads, page 20");
        for (o, opening) in view.openings.iter().enumerate() {
            let expected = opening_pages(&book, o).unwrap();
            let numbers: Vec<u32> = expected.iter().map(|&i| book.pages[i].number).collect();
            assert_eq!(opening.index, o);
            assert_eq!(opening.pages, numbers, "opening {o}");
            assert_eq!(
                opening.template_id, book.pages[expected[0]].template_id,
                "opening {o}"
            );
            assert_eq!(
                opening.alternatives,
                alternatives(&book, &lib, o),
                "opening {o}"
            );
            let slots: Vec<AgentSlot> = expected
                .iter()
                .flat_map(|&i| {
                    let page = &book.pages[i];
                    page.placements.iter().map(move |p| AgentSlot {
                        page: page.number,
                        z: p.z,
                        photo: p.photo_index,
                    })
                })
                .collect();
            assert_eq!(opening.slots, slots, "opening {o}");
        }
        assert_eq!(view.openings[0].pages, vec![1]);
        assert_eq!(view.openings[1].pages, vec![2, 3]);
        assert_eq!(view.openings[10].pages, vec![20]);
        assert!(
            view.openings.iter().any(|o| o.slots.len() >= 2),
            "the fixture needs a multi-slot opening"
        );
    }

    #[test]
    fn agent_view_carries_each_openings_lock() {
        let view = agent_view(&small_book(), &frozen_library(), &small_photos());
        let locked: Vec<bool> = view.openings.iter().map(|o| o.locked).collect();

        assert_eq!(locked, vec![true, false, false]);
    }

    // --- the wire ------------------------------------------------------------------

    /// The Rust half of the wire pin. `tests/agent-view.test.ts` reads the
    /// same file through `app/agent/view.ts`.
    #[test]
    fn agent_view_serialises_exactly_the_keys_the_agent_reads() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/wire/agent-view.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing wire fixture {path:?}: {e}"));
        let fixture: serde_json::Value =
            serde_json::from_str(&text).expect("wire fixture must be valid JSON");

        let value = serde_json::to_value(agent_view(
            &small_book(),
            &frozen_library(),
            &small_photos(),
        ))
        .unwrap();

        assert_eq!(value, fixture);
    }
}
