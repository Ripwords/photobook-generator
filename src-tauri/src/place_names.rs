//! Town names for place chapters, from Apple's reverse geocoder.
//!
//! The one path by which anything derived from a photo leaves the Mac: a
//! chapter's centre, rounded to two decimals (about a kilometre), is sent to
//! Apple. `commands::place_names` is its only caller, and the webview asks
//! for names only while a book's Places option is on. Names are cached by
//! that rounded point, so reopening a draft asks Apple nothing.

use crate::book::chapter::LatLon;
use crate::db::Db;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Display;

/// A point rounded to two decimal places, stored as hundredths so it can be
/// a cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaceKey {
    pub lat_e2: i32,
    pub lon_e2: i32,
}

impl PlaceKey {
    pub fn of(p: LatLon) -> Self {
        Self { lat_e2: (p.lat() * 100.0).round() as i32, lon_e2: (p.lon() * 100.0).round() as i32 }
    }

    /// The rounded point itself, which is what gets geocoded, so a name is
    /// the same whichever centre in the cell asked first. `None` for the one
    /// cell `LatLon` refuses, 0,0.
    fn point(self) -> Option<LatLon> {
        LatLon::new(f64::from(self.lat_e2) / 100.0, f64::from(self.lon_e2) / 100.0)
    }
}

/// How many places one `geocode` request carries. Each lookup can take up to
/// the sidecar's per-lookup timeout, and the sidecar is shared with analysis
/// and export, so the lock is given back between batches.
pub const GEOCODE_BATCH: usize = 8;

/// A name for each chapter whose centre has one, from the cache or, for
/// places not yet cached, from `geocode`. Only real names are cached, so a
/// lookup that failed is asked again next time. A `geocode` error ends the
/// asking; chapters it would have named are left without a name.
pub fn name_chapters<E: Display>(
    db: &Db,
    centroids: &BTreeMap<u32, LatLon>,
    mut geocode: impl FnMut(Vec<LatLon>) -> Result<Vec<Option<String>>, E>,
) -> HashMap<u32, String> {
    let keys: BTreeMap<u32, PlaceKey> = centroids.iter().map(|(&id, &p)| (id, PlaceKey::of(p))).collect();
    let mut names: HashMap<PlaceKey, String> = HashMap::new();
    for &key in keys.values().collect::<BTreeSet<_>>() {
        match db.place_name(key) {
            Ok(Some(name)) => {
                names.insert(key, name);
            }
            Ok(None) => {}
            Err(e) => log::warn!("place name cache read failed: {e}"),
        }
    }

    let missing: Vec<(PlaceKey, LatLon)> = keys
        .values()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| !names.contains_key(key))
        .filter_map(|&key| Some((key, key.point()?)))
        .collect();
    for batch in missing.chunks(GEOCODE_BATCH) {
        let answers = match geocode(batch.iter().map(|&(_, p)| p).collect()) {
            Ok(answers) => answers,
            Err(e) => {
                log::warn!("geocode failed: {e}");
                break;
            }
        };
        for (&(key, _), answer) in batch.iter().zip(answers) {
            let Some(name) = answer else { continue };
            if let Err(e) = db.put_place_name(key, &name) {
                log::warn!("place name cache write failed: {e}");
            }
            names.insert(key, name);
        }
    }

    keys.into_iter().filter_map(|(id, key)| Some((id, names.get(&key)?.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).unwrap()
    }

    const KYOTO: (f64, f64) = (35.0116, 135.7681);
    const OSAKA: (f64, f64) = (34.6937, 135.5023);
    const REYKJAVIK: (f64, f64) = (64.1466, -21.9426);

    /// Answers like Apple would, by the rounded point it is sent, and
    /// records every batch it was asked.
    struct FakeApple {
        known: HashMap<PlaceKey, &'static str>,
        asked: Vec<Vec<PlaceKey>>,
    }

    impl FakeApple {
        fn knowing(places: &[((f64, f64), &'static str)]) -> Self {
            Self {
                known: places.iter().map(|&((lat, lon), name)| (PlaceKey::of(at(lat, lon)), name)).collect(),
                asked: Vec::new(),
            }
        }

        fn geocode(&mut self, points: Vec<LatLon>) -> Result<Vec<Option<String>>, String> {
            let keys: Vec<PlaceKey> = points.iter().map(|&p| PlaceKey::of(p)).collect();
            self.asked.push(keys.clone());
            Ok(keys.iter().map(|k| self.known.get(k).map(|n| n.to_string())).collect())
        }

        fn asked(&self) -> Vec<PlaceKey> {
            self.asked.concat()
        }
    }

    fn chapters(places: &[(u32, (f64, f64))]) -> BTreeMap<u32, LatLon> {
        places.iter().map(|&(id, (lat, lon))| (id, at(lat, lon))).collect()
    }

    fn key(p: (f64, f64)) -> PlaceKey {
        PlaceKey::of(at(p.0, p.1))
    }

    #[test]
    fn place_key_rounds_to_hundredths_either_side_of_zero() {
        assert_eq!(PlaceKey::of(at(35.0149, 135.7651)), PlaceKey { lat_e2: 3501, lon_e2: 13577 });
        assert_eq!(PlaceKey::of(at(-33.8688, -21.9426)), PlaceKey { lat_e2: -3387, lon_e2: -2194 });
    }

    #[test]
    fn names_each_chapter_by_its_own_centre() {
        let db = Db::open_in_memory().unwrap();
        let mut apple = FakeApple::knowing(&[(KYOTO, "Kyoto"), (OSAKA, "Osaka"), (REYKJAVIK, "Reykjavík")]);
        let names = name_chapters(&db, &chapters(&[(4, REYKJAVIK), (0, OSAKA), (7, KYOTO)]), |p| apple.geocode(p));
        assert_eq!(
            names,
            HashMap::from([(0, "Osaka".into()), (4, "Reykjavík".into()), (7, "Kyoto".into())])
        );
    }

    /// The cell's own point is what Apple sees, not the centre: two decimals
    /// is all that leaves the Mac.
    #[test]
    fn sends_apple_the_rounded_point_not_the_centre() {
        let db = Db::open_in_memory().unwrap();
        let mut sent = Vec::new();
        name_chapters(&db, &chapters(&[(0, KYOTO)]), |points| {
            sent.extend(points.iter().map(|p| (p.lat(), p.lon())));
            Ok::<_, String>(vec![None; points.len()])
        });
        assert_eq!(sent, vec![(35.01, 135.77)]);
    }

    #[test]
    fn a_cached_place_is_not_asked_again() {
        let db = Db::open_in_memory().unwrap();
        let mut first = FakeApple::knowing(&[(KYOTO, "Kyoto")]);
        name_chapters(&db, &chapters(&[(0, KYOTO)]), |p| first.geocode(p));

        let mut second = FakeApple::knowing(&[(OSAKA, "Osaka")]);
        let names = name_chapters(&db, &chapters(&[(0, KYOTO), (1, OSAKA)]), |p| second.geocode(p));

        assert_eq!(second.asked(), vec![key(OSAKA)]);
        assert_eq!(names, HashMap::from([(0, "Kyoto".into()), (1, "Osaka".into())]));
    }

    #[test]
    fn a_place_apple_could_not_name_is_asked_again_next_time() {
        let db = Db::open_in_memory().unwrap();
        let mut offline = FakeApple::knowing(&[]);
        let names = name_chapters(&db, &chapters(&[(0, KYOTO)]), |p| offline.geocode(p));
        assert!(names.is_empty());

        let mut online = FakeApple::knowing(&[(KYOTO, "Kyoto")]);
        let names = name_chapters(&db, &chapters(&[(0, KYOTO)]), |p| online.geocode(p));
        assert_eq!(online.asked(), vec![key(KYOTO)]);
        assert_eq!(names, HashMap::from([(0, "Kyoto".into())]));
    }

    #[test]
    fn chapters_in_one_cell_share_one_lookup() {
        let db = Db::open_in_memory().unwrap();
        let mut apple = FakeApple::knowing(&[(KYOTO, "Kyoto")]);
        let names =
            name_chapters(&db, &chapters(&[(0, KYOTO), (1, OSAKA), (2, (35.0149, 135.7651))]), |p| apple.geocode(p));
        assert_eq!(apple.asked().iter().filter(|&&k| k == key(KYOTO)).count(), 1);
        assert_eq!(names.get(&0), Some(&"Kyoto".to_string()));
        assert_eq!(names.get(&2), Some(&"Kyoto".to_string()));
        assert_eq!(names.get(&1), None);
    }

    /// Twenty places go out as batches of `GEOCODE_BATCH`. The second batch
    /// fails: nothing more is asked, the first batch's names stand, and the
    /// places never named are the ones asked next time.
    #[test]
    fn a_sidecar_error_stops_the_asking_and_keeps_the_names_already_found() {
        let db = Db::open_in_memory().unwrap();
        let places: Vec<(u32, (f64, f64))> = (0..20).map(|i| (i, (10.0 + f64::from(i), 20.0))).collect();
        let mut calls = Vec::new();
        let names = name_chapters(&db, &chapters(&places), |points| {
            calls.push(points.len());
            if calls.len() == 2 {
                return Err("sidecar exited".to_string());
            }
            Ok(points.iter().map(|p| Some(format!("Town {}", p.lat()))).collect())
        });
        assert_eq!(calls, vec![GEOCODE_BATCH, GEOCODE_BATCH]);
        assert_eq!(names.len(), GEOCODE_BATCH);
        assert_eq!(names.get(&3), Some(&"Town 13".to_string()));

        let mut retried = FakeApple::knowing(&[]);
        name_chapters(&db, &chapters(&places), |p| retried.geocode(p));
        let expected: Vec<PlaceKey> = (8..20).map(|i| key((10.0 + f64::from(i), 20.0))).collect();
        assert_eq!(retried.asked(), expected);
    }
}
