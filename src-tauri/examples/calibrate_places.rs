//! Calibrates `PLACE_SPLIT_KM` against a real photo library's GPS.
//!
//! ```sh
//! bun run sidecar
//! cd src-tauri && cargo run --release --example calibrate_places -- photos.csv [trips] [km...]
//! ```
//!
//! `photos.csv` has one line per photo, `epochSeconds,lat,lon,path`, with
//! empty fields where a photo has no capture time or no fix. Only derived
//! numbers go in; no image is read.
//!
//! The home base is the busiest 0.1 degree cell. A trip is a run of days whose
//! median fix is over `AWAY_KM` from home, bridging one day with no fixes.
//! Trips are ranked by how many distinct `CELL_KM` places they visit, and the
//! top `trips` (default 3) are printed as chapters at each threshold (default
//! 5, 10, 25, 50 km), after the time-only chapters for comparison.
//!
//! Two counts summarise each threshold. `same-place` counts adjacent chapters
//! in one time run whose centroids are under `SAME_PLACE_KM` apart: a split
//! inside one town. `spread` counts chapters with a located photo over
//! `SPREAD_KM` from their own centroid: a move the threshold missed.

use std::collections::HashMap;

use app_lib::book::chapter::{split_chapters, LatLon};

const AWAY_KM: f64 = 100.0;
const CELL_KM: f64 = 10.0;
const SAME_PLACE_KM: f64 = 10.0;
const SPREAD_KM: f64 = 40.0;
const TZ_OFFSET: i64 = 8 * 3600;
const GAP: i64 = 4 * 3600;

struct Shot {
    time: i64,
    place: Option<LatLon>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let csv = args.next().expect("usage: calibrate_places <photos.csv> [trips] [km...]");
    let trips: usize = args.next().map_or(3, |n| n.parse().expect("trips must be a number"));
    let mut thresholds: Vec<f64> = args.map(|k| k.parse().expect("km must be a number")).collect();
    if thresholds.is_empty() {
        thresholds = vec![5.0, 10.0, 25.0, 50.0];
    }

    let mut shots: Vec<Shot> = std::fs::read_to_string(&csv)
        .expect("readable csv")
        .lines()
        .filter_map(|line| {
            let mut f = line.splitn(4, ',');
            let time = f.next()?.parse().ok()?;
            let lat = f.next()?.parse().ok();
            let lon = f.next()?.parse().ok();
            let place = lat.zip(lon).and_then(|(a, b)| LatLon::new(a, b));
            Some(Shot { time, place })
        })
        .collect();
    shots.sort_by_key(|s| s.time);

    let home = busiest_cell(&shots);
    println!(
        "{} dated photos, {} located; home {:.3},{:.3}\n",
        shots.len(),
        shots.iter().filter(|s| s.place.is_some()).count(),
        home.lat(),
        home.lon()
    );

    let mut found = away_runs(&shots, home);
    found.sort_by_key(|t| std::cmp::Reverse(places_visited(t)));
    for trip in found.into_iter().take(trips) {
        print_trip(trip, home, &thresholds);
    }
}

fn busiest_cell(shots: &[Shot]) -> LatLon {
    let mut cells: HashMap<(i64, i64), usize> = HashMap::new();
    for p in shots.iter().filter_map(|s| s.place) {
        *cells.entry(((p.lat() * 10.0).round() as i64, (p.lon() * 10.0).round() as i64)).or_default() += 1;
    }
    let (&(lat, lon), _) = cells.iter().max_by_key(|&(k, n)| (*n, *k)).expect("some located photo");
    LatLon::new(lat as f64 / 10.0, lon as f64 / 10.0).expect("a real cell")
}

fn day(time: i64) -> i64 {
    (time + TZ_OFFSET).div_euclid(86_400)
}

fn away_runs(shots: &[Shot], home: LatLon) -> Vec<&[Shot]> {
    let mut by_day: Vec<(i64, usize, usize)> = Vec::new();
    for (i, s) in shots.iter().enumerate() {
        match by_day.last_mut() {
            Some((d, _, end)) if *d == day(s.time) => *end = i + 1,
            _ => by_day.push((day(s.time), i, i + 1)),
        }
    }
    let away = |&(_, start, end): &(i64, usize, usize)| {
        let mut km: Vec<f64> = shots[start..end].iter().filter_map(|s| s.place).map(|p| p.km_to(home)).collect();
        km.sort_by(f64::total_cmp);
        km.get(km.len() / 2).map(|&m| m > AWAY_KM)
    };

    let mut runs = Vec::new();
    let mut open: Option<(i64, usize, usize)> = None;
    for d in &by_day {
        match (away(d), open) {
            (Some(true), Some((last, start, _))) if d.0 - last <= 2 => open = Some((d.0, start, d.2)),
            (Some(true), _) => {
                if let Some((_, s, e)) = open {
                    runs.push(&shots[s..e]);
                }
                open = Some((d.0, d.1, d.2));
            }
            (None, Some((last, start, _))) if d.0 - last <= 1 => open = Some((last, start, d.2)),
            _ => {
                if let Some((_, s, e)) = open.take() {
                    runs.push(&shots[s..e]);
                }
            }
        }
    }
    if let Some((_, s, e)) = open {
        runs.push(&shots[s..e]);
    }
    runs
}

fn places_visited(trip: &[Shot]) -> usize {
    let mut seen: Vec<LatLon> = Vec::new();
    for p in trip.iter().filter_map(|s| s.place) {
        if seen.iter().all(|q| q.km_to(p) > CELL_KM) {
            seen.push(p);
        }
    }
    seen.len()
}

struct Chapter {
    start: i64,
    end: i64,
    count: usize,
    located: Vec<LatLon>,
}

impl Chapter {
    fn centroid(&self) -> Option<(f64, f64)> {
        let n = self.located.len() as f64;
        (n > 0.0).then(|| {
            let lat = self.located.iter().map(|p| p.lat()).sum::<f64>() / n;
            let lon = self.located.iter().map(|p| p.lon()).sum::<f64>() / n;
            (lat, lon)
        })
    }

    fn spread_km(&self) -> f64 {
        self.centroid()
            .and_then(|(a, b)| LatLon::new(a, b))
            .map_or(0.0, |c| self.located.iter().map(|p| p.km_to(c)).fold(0.0, f64::max))
    }
}

fn group(trip: &[Shot], split_km: Option<f64>) -> Vec<Chapter> {
    let times: Vec<Option<i64>> = trip.iter().map(|s| Some(s.time)).collect();
    let places: Vec<Option<LatLon>> = trip.iter().map(|s| s.place).collect();
    let ids = split_chapters(&times, &places, split_km);
    let mut chapters: Vec<Chapter> = Vec::new();
    for (s, &id) in trip.iter().zip(&ids) {
        if chapters.len() <= id as usize {
            chapters.push(Chapter { start: s.time, end: s.time, count: 0, located: Vec::new() });
        }
        let c = &mut chapters[id as usize];
        c.end = s.time;
        c.count += 1;
        c.located.extend(s.place);
    }
    chapters
}

fn stamp(time: i64) -> String {
    let local = time + TZ_OFFSET;
    let (y, m, d) = civil(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    format!("{y}-{m:02}-{d:02} {:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Hinnant's algorithm).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn print_trip(trip: &[Shot], home: LatLon, thresholds: &[f64]) {
    let far = trip.iter().filter_map(|s| s.place).map(|p| p.km_to(home)).fold(0.0, f64::max);
    println!(
        "=== trip {} to {}: {} photos, {} located, {} places, up to {:.0} km from home ===",
        stamp(trip[0].time),
        stamp(trip[trip.len() - 1].time),
        trip.len(),
        trip.iter().filter(|s| s.place.is_some()).count(),
        places_visited(trip),
        far
    );
    for split_km in std::iter::once(None).chain(thresholds.iter().copied().map(Some)) {
        let chapters = group(trip, split_km);
        let same_place = chapters
            .windows(2)
            .filter(|w| w[1].start - w[0].end <= GAP)
            .filter(|w| match (w[0].centroid(), w[1].centroid()) {
                (Some(a), Some(b)) => LatLon::new(a.0, a.1)
                    .zip(LatLon::new(b.0, b.1))
                    .is_some_and(|(a, b)| a.km_to(b) < SAME_PLACE_KM),
                _ => false,
            })
            .count();
        let spread = chapters.iter().filter(|c| c.spread_km() > SPREAD_KM).count();
        let label = split_km.map_or("time only".to_string(), |k| format!("{k} km"));
        println!("\n--- {label}: {} chapters, same-place {same_place}, spread {spread}", chapters.len());
        println!("{:<17} {:<17} {:>5} {:>5} {:>18} {:>8}", "start", "end", "n", "gps", "centroid", "spread");
        for c in &chapters {
            let centroid = c.centroid().map_or("-".to_string(), |(a, b)| format!("{a:.3},{b:.3}"));
            println!(
                "{:<17} {:<17} {:>5} {:>5} {:>18} {:>7.1}k",
                stamp(c.start),
                stamp(c.end),
                c.count,
                c.located.len(),
                centroid,
                c.spread_km()
            );
        }
    }
    println!();
}
