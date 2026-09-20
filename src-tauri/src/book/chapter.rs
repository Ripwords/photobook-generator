//! A book's chapters: runs of photos taken close together in time and, with
//! Places on, in one place.
//!
//! Chapters are `Photo::event_cluster`. `finalize_photos` stamps the
//! time-only ones; `chapters` re-derives them for a book with Places on, and
//! every caller that shows or packs chapters goes through it, so the contact
//! sheet and the book agree.

use crate::book::cull::Photo;
use crate::cluster::EVENT_GAP_SECONDS;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A GPS fix. Built only through `LatLon::new`, so every value is a real
/// place on the globe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct LatLon {
    lat: f64,
    lon: f64,
}

impl LatLon {
    /// `None` for a coordinate that is not a place: non-finite, out of
    /// range, or exactly 0,0. A DJI Osmo Pocket with no GPS lock writes
    /// -0.0,-0.0 into every photo (seen on the Japan 2026 trip), and taking
    /// that as a place would put a chapter in the Gulf of Guinea.
    pub fn new(lat: f64, lon: f64) -> Option<Self> {
        let real = lat.is_finite()
            && lon.is_finite()
            && lat.abs() <= 90.0
            && lon.abs() <= 180.0
            && !(lat == 0.0 && lon == 0.0);
        real.then_some(Self { lat, lon })
    }

    pub fn lat(self) -> f64 {
        self.lat
    }

    pub fn lon(self) -> f64 {
        self.lon
    }

    /// Great-circle distance in kilometres (haversine).
    pub fn km_to(self, other: LatLon) -> f64 {
        let (p1, p2) = (self.lat.to_radians(), other.lat.to_radians());
        let dp = p2 - p1;
        let dl = (other.lon - self.lon).to_radians();
        let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_KM * a.sqrt().min(1.0).asin()
    }

    fn unit_vector(self) -> [f64; 3] {
        let (p, l) = (self.lat.to_radians(), self.lon.to_radians());
        [p.cos() * l.cos(), p.cos() * l.sin(), p.sin()]
    }
}

impl TryFrom<[f64; 2]> for LatLon {
    type Error = String;
    fn try_from([lat, lon]: [f64; 2]) -> Result<Self, String> {
        LatLon::new(lat, lon).ok_or_else(|| format!("{lat},{lon} is not a place"))
    }
}

impl From<LatLon> for [f64; 2] {
    fn from(p: LatLon) -> Self {
        [p.lat, p.lon]
    }
}

const EARTH_RADIUS_KM: f64 = 6371.0088;

/// How far a photo must be from its chapter's centroid, with the next located
/// photo just as far, to start a new chapter.
///
/// Calibrated with `examples/calibrate_places.rs` on a real library of 10,818
/// dated photos (9,725 located) against 5, 10, 25 and 50 km. 10 km shreds a
/// single city: Singapore became airport, city, airport; Tokyo became three
/// chapters in one evening; Taipei Main and Shilin, both in Taipei, split.
/// 50 km misses the moves the option exists for: Osaka and Kyoto (43 km)
/// fell into one chapter, as did Kyoto and Nara. 25 km keeps each of those
/// cities whole and separates each of those towns. At every threshold a
/// photo taken from a moving train can stand as a one-photo chapter.
pub const PLACE_SPLIT_KM: f64 = 25.0;

/// One chapter id per photo, in input order, like `event_clusters`.
///
/// With `places` off this is exactly `event_clusters(.., EVENT_GAP_SECONDS)`,
/// the chapters `finalize_photos` already stamped. With it on, see
/// `split_chapters`.
pub fn chapters(photos: &[Photo], places: bool) -> Vec<u32> {
    let times: Vec<Option<i64>> = photos.iter().map(|p| p.captured_at).collect();
    let locations: Vec<Option<LatLon>> = photos.iter().map(|p| p.location).collect();
    split_chapters(&times, &locations, places.then_some(PLACE_SPLIT_KM))
}

/// The place chapters of an analysed set, for the draft screen's contact
/// sheet, and how many photos had a location to split by.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceChapters {
    pub located: usize,
    pub chapters: std::collections::HashMap<String, u32>,
}

impl PlaceChapters {
    pub fn of(photos: &[Photo]) -> Self {
        let ids = chapters(photos, true);
        Self {
            located: photos.iter().filter(|p| p.location.is_some()).count(),
            chapters: photos.iter().map(|p| p.path.clone()).zip(ids).collect(),
        }
    }
}

/// Walks photos in capture order and starts a new chapter on a time gap over
/// `EVENT_GAP_SECONDS`, or, when `split_km` is set, where a photo is more
/// than `split_km` from its chapter's running centroid and the next located
/// photo before the next time gap is too. The second condition stops one
/// stray GPS fix from cutting a chapter; a far photo with no located photo
/// after it in the same run cannot be told from a stray, so it stays.
///
/// A photo with no location never splits a chapter and never moves its
/// centroid; it joins the chapter it falls in by time. Undated photos form
/// one trailing chapter, as in `event_clusters`.
pub fn split_chapters(
    times: &[Option<i64>],
    locations: &[Option<LatLon>],
    split_km: Option<f64>,
) -> Vec<u32> {
    let mut dated: Vec<(usize, i64)> =
        times.iter().enumerate().filter_map(|(i, t)| t.map(|t| (i, t))).collect();
    dated.sort_by_key(|&(_, t)| t);
    let gap_before = |k: usize| k > 0 && dated[k].1 - dated[k - 1].1 > EVENT_GAP_SECONDS;

    let mut ids = vec![None; times.len()];
    let mut current = 0u32;
    let mut centroid = Centroid::default();
    for (k, &(i, _)) in dated.iter().enumerate() {
        let moved = || {
            let (km, here, centre) = (split_km?, locations[i]?, centroid.point()?);
            let next = (k + 1..dated.len())
                .take_while(|&n| !gap_before(n))
                .find_map(|n| locations[dated[n].0])?;
            Some(here.km_to(centre) > km && next.km_to(centre) > km)
        };
        if gap_before(k) || moved() == Some(true) {
            current += 1;
            centroid = Centroid::default();
        }
        if let Some(here) = locations[i] {
            centroid.add(here);
        }
        ids[i] = Some(current);
    }

    let undated = if dated.is_empty() { 0 } else { current + 1 };
    ids.into_iter().map(|id| id.unwrap_or(undated)).collect()
}

/// The centre of each chapter with a located photo, by chapter id. `ids`
/// is one chapter id per photo, as `chapters` returns.
pub fn centroids(locations: &[Option<LatLon>], ids: &[u32]) -> BTreeMap<u32, LatLon> {
    let mut sums: BTreeMap<u32, Centroid> = BTreeMap::new();
    for (&id, location) in ids.iter().zip(locations) {
        if let Some(p) = *location {
            sums.entry(id).or_default().add(p);
        }
    }
    sums.into_iter().filter_map(|(id, c)| Some((id, c.point()?))).collect()
}

/// The mean of a chapter's fixes, taken on the unit sphere so a chapter
/// either side of the antimeridian does not average to longitude 0.
#[derive(Default)]
struct Centroid {
    sum: [f64; 3],
}

impl Centroid {
    fn add(&mut self, p: LatLon) {
        for (s, v) in self.sum.iter_mut().zip(p.unit_vector()) {
            *s += v;
        }
    }

    fn point(&self) -> Option<LatLon> {
        let [x, y, z] = self.sum;
        let norm = (x * x + y * y + z * z).sqrt();
        (norm > 1e-9).then(|| LatLon {
            lat: (z / norm).asin().to_degrees(),
            lon: y.atan2(x).to_degrees(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::event_clusters;
    use proptest::prelude::*;

    fn at(lat: f64, lon: f64) -> Option<LatLon> {
        Some(LatLon::new(lat, lon).unwrap())
    }

    /// `km` east of `origin` along its parallel.
    fn east(origin: (f64, f64), km: f64) -> Option<LatLon> {
        let degrees = km / (EARTH_RADIUS_KM * origin.0.to_radians().cos()) * 180.0 / std::f64::consts::PI;
        at(origin.0, origin.1 + degrees)
    }

    const KYOTO: (f64, f64) = (35.0116, 135.7681);
    const OSAKA: (f64, f64) = (34.6937, 135.5023);
    const HOUR: i64 = 3600;

    fn split(stops: &[(Option<i64>, Option<LatLon>)], km: f64) -> Vec<u32> {
        let (times, locations): (Vec<_>, Vec<_>) = stops.iter().copied().unzip();
        split_chapters(&times, &locations, Some(km))
    }

    #[test]
    fn centroids_average_each_chapters_own_fixes() {
        let ids = [1, 0, 2, 1, 0];
        let locations = [at(35.0, 135.0), at(10.0, 20.0), None, at(35.2, 135.4), at(10.4, 20.2)];
        let centres = centroids(&locations, &ids);
        assert_eq!(centres.keys().copied().collect::<Vec<_>>(), vec![0, 1], "chapter 2 has no fix");
        let (c0, c1) = (centres[&0], centres[&1]);
        assert!((c0.lat() - 10.2).abs() < 0.01 && (c0.lon() - 20.1).abs() < 0.01, "{c0:?}");
        assert!((c1.lat() - 35.1).abs() < 0.01 && (c1.lon() - 135.2).abs() < 0.01, "{c1:?}");
    }

    /// Fiji straddles the antimeridian. A plain mean of 179.9 and -179.9 is
    /// longitude 0, the far side of the planet.
    #[test]
    fn centroids_of_a_chapter_across_the_antimeridian_stay_there() {
        let centres = centroids(&[at(-17.0, 179.9), at(-17.0, -179.9)], &[0, 0]);
        assert!(centres[&0].lon().abs() > 179.9, "{:?}", centres[&0]);
    }

    #[test]
    fn chapter_haversine_matches_a_known_distance() {
        let paris = LatLon::new(48.8566, 2.3522).unwrap();
        let london = LatLon::new(51.5074, -0.1278).unwrap();
        assert!((paris.km_to(london) - 343.6).abs() < 1.0, "{}", paris.km_to(london));
        let kyoto = at(KYOTO.0, KYOTO.1).unwrap();
        assert!((kyoto.km_to(at(OSAKA.0, OSAKA.1).unwrap()) - 42.8).abs() < 1.0);
    }

    #[test]
    fn chapter_latlon_refuses_null_island_and_impossible_coordinates() {
        assert_eq!(LatLon::new(-0.0, -0.0), None, "DJI's no-fix value");
        assert_eq!(LatLon::new(0.0, 0.0), None);
        assert_eq!(LatLon::new(f64::NAN, 1.0), None);
        assert_eq!(LatLon::new(1.0, f64::INFINITY), None);
        assert_eq!(LatLon::new(91.0, 1.0), None);
        assert_eq!(LatLon::new(1.0, -181.0), None);
        assert!(LatLon::new(0.0, 101.7).is_some(), "the equator is a place");
        assert!(LatLon::new(51.5, 0.0).is_some(), "so is Greenwich");
    }

    proptest! {
        /// Places off must not change a single chapter the book already had.
        #[test]
        fn chapter_places_off_is_exactly_event_clusters(
            stops in prop::collection::vec(
                (prop::option::weighted(0.85, 0i64..20 * 86_400), prop::option::of((-60.0f64..60.0, -170.0f64..170.0))),
                0..40,
            )
        ) {
            let times: Vec<Option<i64>> = stops.iter().map(|s| s.0).collect();
            let locations: Vec<Option<LatLon>> =
                stops.iter().map(|s| s.1.and_then(|(a, b)| LatLon::new(a, b))).collect();
            let expected: Vec<u32> =
                event_clusters(&times, EVENT_GAP_SECONDS).into_iter().map(|id| id as u32).collect();
            prop_assert_eq!(split_chapters(&times, &locations, None), expected);
        }
    }

    /// Kyoto in the morning, Osaka in the afternoon, one time run. Input order
    /// is shuffled so the ids have to come back in input order.
    #[test]
    fn chapter_a_move_between_towns_in_one_day_splits() {
        let k = at(KYOTO.0, KYOTO.1);
        let o = at(OSAKA.0, OSAKA.1);
        let ids = split(
            &[
                (Some(13 * HOUR), o),
                (Some(9 * HOUR), k),
                (Some(14 * HOUR), o),
                (Some(10 * HOUR), k),
                (Some(12 * HOUR), o),
                (Some(11 * HOUR), k),
            ],
            25.0,
        );
        assert_eq!(ids, vec![1, 0, 1, 0, 1, 0]);
    }

    #[test]
    fn chapter_one_stray_fix_does_not_split() {
        let k = at(KYOTO.0, KYOTO.1);
        let ids = split(
            &[
                (Some(0), k),
                (Some(600), k),
                (Some(1200), at(OSAKA.0, OSAKA.1)),
                (Some(1800), k),
                (Some(2400), k),
            ],
            25.0,
        );
        assert_eq!(ids, vec![0; 5]);
    }

    /// Unlocated photos sit between the move and its confirmation; they join
    /// whichever chapter they fall in by time and do not count as "next".
    #[test]
    fn chapter_unlocated_photos_never_split_and_join_by_time() {
        let k = at(KYOTO.0, KYOTO.1);
        let o = at(OSAKA.0, OSAKA.1);
        let ids = split(
            &[
                (Some(0), k),
                (Some(100), None),
                (Some(200), k),
                (Some(300), o),
                (Some(400), None),
                (Some(500), None),
                (Some(600), o),
            ],
            25.0,
        );
        assert_eq!(ids, vec![0, 0, 0, 1, 1, 1, 1]);
    }

    /// Consecutive photos each 15 km apart never trip a 25 km threshold
    /// against their neighbour, but the chapter has walked 60 km from where
    /// it started. The centroid notices; a photo-to-photo rule would not.
    #[test]
    fn chapter_splits_against_the_running_centroid_not_the_previous_photo() {
        let stops: Vec<_> = (0..6).map(|i| (Some(i * 600), east(KYOTO, 15.0 * i as f64))).collect();
        let ids = split(&stops, 25.0);
        assert!(ids.iter().any(|&id| id > 0), "a 75 km walk stayed one chapter: {ids:?}");
        assert_eq!(ids[0], ids[1], "15 km from the start is not a new place");
    }

    /// Day two starts somewhere new after a night's gap. The gap already
    /// starts the chapter; day two must then measure against its own photos,
    /// not day one's.
    #[test]
    fn chapter_a_time_gap_starts_a_fresh_centroid() {
        let k = at(KYOTO.0, KYOTO.1);
        let o = at(OSAKA.0, OSAKA.1);
        let day = 86_400;
        let ids = split(
            &[(Some(0), k), (Some(600), k), (Some(day), o), (Some(day + 600), o), (Some(day + 1200), o)],
            25.0,
        );
        assert_eq!(ids, vec![0, 0, 1, 1, 1]);
    }

    /// The last photo of a day is far away and the next located photo is
    /// after the night's gap. There is nothing in the same run to confirm the
    /// move, so it is treated like a stray.
    #[test]
    fn chapter_confirmation_does_not_look_past_a_time_gap() {
        let k = at(KYOTO.0, KYOTO.1);
        let o = at(OSAKA.0, OSAKA.1);
        let day = 86_400;
        let ids = split(&[(Some(0), k), (Some(600), k), (Some(1200), o), (Some(day), o)], 25.0);
        assert_eq!(ids, vec![0, 0, 0, 1]);
    }

    /// Fiji straddles the antimeridian. Two photos 22 km apart on either side
    /// of it average to longitude 0 with a naive mean.
    #[test]
    fn chapter_centroid_survives_the_antimeridian() {
        let ids = split(
            &[
                (Some(0), at(-16.8, 179.9)),
                (Some(600), at(-16.8, -179.9)),
                (Some(1200), at(-16.8, 179.9)),
                (Some(1800), at(-16.8, -179.9)),
            ],
            25.0,
        );
        assert_eq!(ids, vec![0; 4]);
    }

    #[test]
    fn chapter_undated_photos_trail_as_one_chapter() {
        let k = at(KYOTO.0, KYOTO.1);
        let o = at(OSAKA.0, OSAKA.1);
        let ids = split(&[(None, o), (Some(0), k), (Some(600), k), (None, k), (Some(1200), o), (Some(1800), o)], 25.0);
        assert_eq!(ids, vec![2, 0, 0, 2, 1, 1]);
        assert_eq!(split(&[(None, k), (None, o)], 25.0), vec![0, 0]);
    }
}
