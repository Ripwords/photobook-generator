//! Event tiers: which events of a trip a book shows, and how many photos
//! each one gets. See docs/superpowers/specs/2026-09-23-event-tiers-design.md.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::book::chapter::{self, LatLon};
use crate::book::cull::Photo;
use crate::book::pace::BookOptions;
use crate::book::pack::{self, Capacity};

pub const FEATURED_WEIGHT: f64 = 2.0;
pub const NORMAL_WEIGHT: f64 = 1.0;
pub const BRIEF_WEIGHT: f64 = 0.5;
pub const NORMAL_FLOOR: usize = 2;
const BRIEF_FLOOR: usize = 1;

pub const W_ENGAGEMENT: f64 = 0.4;
pub const W_QUALITY: f64 = 0.4;
pub const W_NOVELTY: f64 = 0.2;
pub const UTILITY_SKIP: f64 = 0.8;
pub const FEATURED_MARGIN: f64 = 1.3;
pub const NORMAL_SLOT_SHARE: f64 = 0.6;
pub const NOVELTY_KM: f64 = 2.0;
pub const SIMILAR_BELOW: f64 = 0.3;
const QUALITY_TOP: usize = 3;
const TOP_TAGS: usize = 10;

/// How much of the book an event gets. Declared lowest first, so the derived
/// `Ord` reads "higher tier" and a tie in `resolve` can take the max.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Skipped,
    Brief,
    Normal,
    Featured,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Brief => "brief",
            Self::Normal => "normal",
            Self::Featured => "featured",
        }
    }

    /// `None` for an unknown token: a stored choice this build cannot honour
    /// fails the load rather than silently becoming the suggestion.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "skipped" => Some(Self::Skipped),
            "brief" => Some(Self::Brief),
            "normal" => Some(Self::Normal),
            "featured" => Some(Self::Featured),
            _ => None,
        }
    }

    pub fn weight(self) -> f64 {
        match self {
            Self::Featured => FEATURED_WEIGHT,
            Self::Normal => NORMAL_WEIGHT,
            Self::Brief => BRIEF_WEIGHT,
            Self::Skipped => 0.0,
        }
    }

    pub fn floor(self, options: &BookOptions) -> usize {
        match self {
            Self::Featured => options.featured_floor as usize,
            Self::Normal => NORMAL_FLOOR,
            Self::Brief => BRIEF_FLOOR,
            Self::Skipped => 0,
        }
    }
}

/// The user's tier choices, keyed by photo content hash (§6). Absent means
/// the event takes its suggestion; there is no stored "auto", the same rule
/// as `cull::Overrides`. Wire shape `{"<hash>": "featured"}`, pinned in
/// `tests/fixtures/wire/event-tiers.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventTiers(BTreeMap<String, Tier>);

impl EventTiers {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn get(&self, hash: &str) -> Option<Tier> {
        self.0.get(hash).copied()
    }

    pub fn set(&mut self, hash: impl Into<String>, tier: Option<Tier>) {
        let hash = hash.into();
        match tier {
            Some(t) => {
                self.0.insert(hash, t);
            }
            None => {
                self.0.remove(&hash);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, Tier)> {
        self.0.iter().map(|(h, &t)| (h.as_str(), t))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, Tier)> for EventTiers {
    fn from_iter<I: IntoIterator<Item = (String, Tier)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// An event's chosen tier: the one held by the most of its photos, ties to
/// the higher tier; `None` when none of its photos carries a choice (§6).
pub fn resolve<'a>(hashes: impl IntoIterator<Item = &'a str>, tiers: &EventTiers) -> Option<Tier> {
    let mut votes: BTreeMap<Tier, usize> = BTreeMap::new();
    for h in hashes {
        if let Some(t) = tiers.get(h) {
            *votes.entry(t).or_default() += 1;
        }
    }
    votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0))).map(|(t, _)| t)
}

/// Gini coefficient of a distribution of counts: 0 when every value is
/// equal, approaching 1 when one value holds everything. The report's
/// measure of how evenly a book spreads its photos across events.
pub fn gini(values: &[usize]) -> f64 {
    let n = values.len();
    let total: usize = values.iter().sum();
    if n == 0 || total == 0 {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    // G = (2 * sum(i * x_i)) / (n * sum(x)) - (n + 1) / n, with i from 1.
    let weighted: f64 = sorted.iter().enumerate().map(|(i, &x)| (i as f64 + 1.0) * x as f64).sum();
    2.0 * weighted / (n as f64 * total as f64) - (n as f64 + 1.0) / n as f64
}

/// The componentwise mean of several feature prints: an event's "look".
pub fn mean_print(prints: &[&[f32]]) -> Option<Vec<f32>> {
    let first = prints.first()?;
    if prints.iter().any(|p| p.len() != first.len()) {
        return None;
    }
    let n = prints.len() as f32;
    Some((0..first.len()).map(|k| prints.iter().map(|p| p[k]).sum::<f32>() / n).collect())
}

/// Euclidean (L2) distance between two prints.
pub fn print_distance(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| ((x - y) as f64).powi(2)).sum::<f64>().sqrt()
}

/// Area under the ROC curve for "a smaller distance means the same place":
/// the probability that a random same-place distance is smaller than a
/// random different-place distance. Ties count one half. `0.5` (chance)
/// when either group is empty.
pub fn auc(same: &[f64], different: &[f64]) -> f64 {
    if same.is_empty() || different.is_empty() {
        return 0.5;
    }
    let mut wins = 0.0;
    for s in same {
        for d in different {
            wins += if s < d { 1.0 } else if s == d { 0.5 } else { 0.0 };
        }
    }
    wins / (same.len() * different.len()) as f64
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventStats {
    pub event: u32,
    pub photos: usize,
    pub keepers: usize,
    /// Distinct moments among the keepers (`pack::moments`).
    pub moments: usize,
    /// Mean `aesthetic_pct` of the best `QUALITY_TOP` keepers, / 100.
    pub quality: f64,
    /// Share of ALL the event's photos flagged `is_utility`.
    pub utility: f64,
    /// No photo in the event has a capture time.
    pub undated: bool,
    pub centroid: Option<LatLon>,
    /// The `TOP_TAGS` most frequent scene tags among the keepers.
    pub tags: BTreeSet<String>,
    /// Mean feature print of the keepers that have one.
    pub print: Option<Vec<f32>>,
}

/// Per-event figures, sorted by event id. `kept` is `cull`'s output for
/// `photos`; an event whose photos were all culled has `keepers == 0`.
pub fn stats(photos: &[Photo], kept: &[Photo]) -> Vec<EventStats> {
    let ids: Vec<u32> = photos.iter().map(|p| p.event_cluster).collect();
    let locations: Vec<Option<LatLon>> = photos.iter().map(|p| p.location).collect();
    let centres = chapter::centroids(&locations, &ids);

    let mut out: BTreeMap<u32, EventStats> = BTreeMap::new();
    for p in photos {
        let s = out.entry(p.event_cluster).or_insert_with(|| EventStats {
            event: p.event_cluster,
            photos: 0,
            keepers: 0,
            moments: 0,
            quality: 0.0,
            utility: 0.0,
            undated: true,
            centroid: centres.get(&p.event_cluster).copied(),
            tags: BTreeSet::new(),
            print: None,
        });
        s.photos += 1;
        if p.is_utility {
            s.utility += 1.0;
        }
        if p.captured_at.is_some() {
            s.undated = false;
        }
    }

    let moment = pack::moments(kept);
    let mut members: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, p) in kept.iter().enumerate() {
        members.entry(p.event_cluster).or_default().push(i);
    }
    for (event, idx) in members {
        let Some(s) = out.get_mut(&event) else { continue };
        s.keepers = idx.len();
        s.moments = idx.iter().map(|&i| moment[i]).collect::<BTreeSet<_>>().len();
        let mut aes: Vec<u8> = idx.iter().map(|&i| kept[i].aesthetic_pct).collect();
        aes.sort_unstable_by(|a, b| b.cmp(a));
        let top = &aes[..aes.len().min(QUALITY_TOP)];
        s.quality = top.iter().map(|&a| a as f64).sum::<f64>() / top.len() as f64 / 100.0;
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for &i in &idx {
            for t in &kept[i].scene_tags {
                *counts.entry(t.as_str()).or_default() += 1;
            }
        }
        let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        s.tags = ranked.into_iter().take(TOP_TAGS).map(|(t, _)| t.to_string()).collect();
        let prints: Vec<&[f32]> = idx.iter().filter_map(|&i| kept[i].feature_print.as_deref()).collect();
        s.print = mean_print(&prints);
    }
    for s in out.values_mut() {
        s.utility /= s.photos as f64;
    }
    out.into_values().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reason {
    Utility { share: f64 },
    NothingKept,
    Undated,
    Ranked { rank: usize, of: usize },
    Standout,
    OutOfRoom { rank: usize, of: usize },
    SimilarTo { event: u32 },
    /// `budget` lowered a suggested tier so the floors fit the book (§5 step 3).
    Demoted,
    /// `budget` raised a suggested Brief so the book does not underfill.
    Filled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion {
    pub event: u32,
    pub tier: Tier,
    pub reason: Reason,
    pub merit: f64,
}

/// How many events the page length can give a chapter of their own (§4).
pub fn normal_room(eligible: usize, capacity: &Capacity) -> usize {
    let slots = capacity.spreads as usize + capacity.singles as usize;
    eligible.min((slots as f64 * NORMAL_SLOT_SHARE).floor() as usize)
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// How alike two events are, 0..1: the highest of place, scene and look (§3.1).
fn similarity(a: &EventStats, b: &EventStats) -> f64 {
    let gps = match (a.centroid, b.centroid) {
        (Some(x), Some(y)) => 1.0 - (x.km_to(y) / NOVELTY_KM).min(1.0),
        _ => 0.0,
    };
    gps.max(jaccard(&a.tags, &b.tags)).max(look_similarity(a, b))
}

/// LOOK_DROPPED by Task 2: always 0. If Task 2 recorded LOOK_KEPT, replace
/// with `1 - clamp((d - LOOK_SAME) / (LOOK_DIFFERENT - LOOK_SAME), 0, 1)`
/// over `print_distance` of the two mean prints, and add a test.
fn look_similarity(_a: &EventStats, _b: &EventStats) -> f64 {
    0.0
}

pub fn suggest(stats: &[EventStats], capacity: &Capacity) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let mut eligible: Vec<&EventStats> = Vec::new();
    // "Undated" only means something beside dated events; in a library with
    // no dates at all it would make every event Brief.
    let any_dated = stats.iter().any(|s| !s.undated);
    for s in stats {
        let fixed = |tier, reason| Suggestion { event: s.event, tier, reason, merit: 0.0 };
        if s.utility > UTILITY_SKIP {
            out.push(fixed(Tier::Skipped, Reason::Utility { share: s.utility }));
        } else if s.keepers == 0 {
            out.push(fixed(Tier::Skipped, Reason::NothingKept));
        } else if s.undated && any_dated {
            out.push(fixed(Tier::Brief, Reason::Undated));
        } else {
            eligible.push(s);
        }
    }

    let most = eligible.iter().map(|s| s.moments).max().unwrap_or(0);
    let engagement = |s: &EventStats| {
        if most == 0 { 0.0 } else { (1.0 + s.moments as f64).ln() / (1.0 + most as f64).ln() }
    };
    let base = |s: &EventStats| (W_ENGAGEMENT * engagement(s) + W_QUALITY * s.quality) * (1.0 - s.utility);

    // Walk in base-merit order; each event's novelty is against those above it.
    let mut walk = eligible.clone();
    walk.sort_by(|a, b| base(b).total_cmp(&base(a)).then(a.event.cmp(&b.event)));
    struct Scored<'a> {
        s: &'a EventStats,
        merit: f64,
        novelty: f64,
        nearest: Option<u32>,
        base_rank: usize,
    }
    let mut scored: Vec<Scored> = walk
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let (sim, nearest) = walk[..k]
                .iter()
                .map(|o| (similarity(s, o), Some(o.event)))
                .fold((0.0, None), |best, cur| if cur.0 > best.0 { cur } else { best });
            let novelty = 1.0 - sim;
            let merit = (W_ENGAGEMENT * engagement(s) + W_QUALITY * s.quality + W_NOVELTY * novelty)
                * (1.0 - s.utility);
            Scored { s, merit, novelty, nearest, base_rank: k }
        })
        .collect();
    scored.sort_by(|a, b| b.merit.total_cmp(&a.merit).then(a.s.event.cmp(&b.s.event)));

    let of = scored.len();
    let room = normal_room(of, capacity);
    let mut top: Vec<f64> = scored[..room].iter().map(|x| x.merit).collect();
    top.sort_by(f64::total_cmp);
    let median = if top.is_empty() {
        0.0
    } else if top.len() % 2 == 1 {
        top[top.len() / 2]
    } else {
        (top[top.len() / 2 - 1] + top[top.len() / 2]) / 2.0
    };
    let featured_limit = (room / 6).max(1);
    let mut featured = 0;
    for (rank, x) in scored.iter().enumerate() {
        let (tier, reason) = if rank < room {
            if featured < featured_limit && x.merit >= FEATURED_MARGIN * median {
                featured += 1;
                (Tier::Featured, Reason::Standout)
            } else {
                (Tier::Normal, Reason::Ranked { rank: rank + 1, of })
            }
        } else if x.novelty < SIMILAR_BELOW && x.base_rank < room {
            (Tier::Brief, Reason::SimilarTo { event: x.nearest.unwrap_or(x.s.event) })
        } else {
            (Tier::Brief, Reason::OutOfRoom { rank: rank + 1, of })
        };
        out.push(Suggestion { event: x.s.event, tier, reason, merit: x.merit });
    }
    out.sort_by_key(|s| s.event);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::chapter::LatLon;
    use crate::book::cull::Photo;
    use crate::book::pack::Capacity;

    /// Photo `n` of event `event`: moment `n / 2` (two frames a moment, 60 s
    /// apart; moments 10 min apart), aesthetic `aes` minus a small per-photo
    /// spread so percentiles genuinely differ.
    fn shot(event: u32, day: i64, n: usize, aes: u8) -> Photo {
        Photo {
            path: format!("/e{event}/p{n:03}.jpg"),
            hash: format!("e{event}-{n}"),
            width: 4000,
            height: 3000,
            is_utility: false,
            aesthetic_pct: aes.saturating_sub((n % 7) as u8),
            sharpness_pct: 50,
            near_dup_cluster: event * 10_000 + n as u32,
            event_cluster: event,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: Some(day * 86_400 + (n / 2) as i64 * 600 + (n % 2) as i64 * 60),
            clipped_low: 0.0,
            clipped_high: 0.0,
            feature_print: None,
            location: None,
        }
    }

    fn event(event: u32, day: i64, count: usize, aes: u8) -> Vec<Photo> {
        (0..count).map(|n| shot(event, day, n, aes)).collect()
    }

    fn located(mut photos: Vec<Photo>, lat: f64, lon: f64, tags: &[&str]) -> Vec<Photo> {
        for p in &mut photos {
            p.location = LatLon::new(lat, lon);
            p.scene_tags = tags.iter().map(|t| t.to_string()).collect();
        }
        photos
    }

    fn capacity(spreads: u32, target: usize) -> Capacity {
        Capacity { pages: spreads * 2 + 2, singles: 2, spreads, max_photos: target * 2, target_photos: target }
    }

    fn keepers(photos: &[Photo]) -> Vec<Photo> {
        photos.iter().filter(|p| !p.is_utility).cloned().collect()
    }

    #[test]
    fn stats_count_moments_among_keepers_and_utility_over_all_photos() {
        let mut photos = event(0, 0, 10, 80); // 5 moments
        for p in photos.iter_mut().take(4) {
            p.is_utility = true;
        }
        let s = stats(&photos, &keepers(&photos));
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].photos, s[0].keepers, s[0].moments), (10, 6, 3));
        assert!((s[0].utility - 0.4).abs() < 1e-9);
        assert!(!s[0].undated);
    }

    #[test]
    fn a_mostly_screenshot_event_is_suggested_skipped() {
        let mut shots = event(1, 1, 40, 70);
        for p in shots.iter_mut().take(36) {
            p.is_utility = true;
        }
        let mut photos = event(0, 0, 30, 80);
        photos.extend(shots);
        photos.extend(event(2, 2, 30, 80));
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        assert_eq!(s[1].tier, Tier::Skipped);
        assert!(matches!(s[1].reason, Reason::Utility { share } if share > 0.8));
    }

    #[test]
    fn an_undated_event_is_suggested_brief() {
        let mut undated = event(1, 0, 30, 90);
        for p in &mut undated {
            p.captured_at = None;
        }
        let mut photos = event(0, 0, 20, 60);
        photos.extend(undated);
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        assert_eq!((s[1].tier, s[1].reason), (Tier::Brief, Reason::Undated));
    }

    #[test]
    fn a_library_with_no_dates_at_all_is_ranked_not_all_brief() {
        // Scanned prints, or the test fixtures: nothing is dated, so "undated"
        // says nothing about one event against another.
        let mut photos = event(0, 0, 20, 60);
        photos.extend(event(1, 1, 20, 80));
        for p in &mut photos {
            p.captured_at = None;
        }
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        assert!(s.iter().all(|x| x.reason != Reason::Undated), "{s:?}");
        assert!(s.iter().all(|x| x.tier >= Tier::Normal), "{s:?}");
    }

    #[test]
    fn normal_room_is_sixty_percent_of_the_slots() {
        assert_eq!(normal_room(50, &capacity(9, 45)), 6); // 11 slots
        assert_eq!(normal_room(50, &capacity(19, 85)), 12); // 21 slots
        assert_eq!(normal_room(3, &capacity(19, 85)), 3);
    }

    #[test]
    fn events_beyond_the_room_are_brief_and_a_standout_is_featured() {
        // 9 ordinary events and one with 3x the moments and top aesthetics, 11 slots -> room 6.
        let mut photos = Vec::new();
        for e in 0..9 {
            photos.extend(located(event(e, e as i64, 12, 40 + e as u8), 10.0 + e as f64, 100.0, &["tag"]));
        }
        photos.extend(located(event(9, 9, 36, 95), 50.0, 50.0, &["summit"]));
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        let count = |t: Tier| s.iter().filter(|x| x.tier == t).count();
        assert_eq!(s[9].tier, Tier::Featured, "{:?}", s[9]);
        assert_eq!(s[9].reason, Reason::Standout);
        assert_eq!(count(Tier::Featured) + count(Tier::Normal), 6);
        assert_eq!(count(Tier::Brief), 4);
        assert!(s.iter().filter(|x| x.tier == Tier::Brief).all(|x| matches!(
            x.reason,
            Reason::OutOfRoom { .. } | Reason::SimilarTo { .. }
        )));
    }

    #[test]
    fn of_two_look_alike_events_the_lower_is_brief_when_room_is_tight() {
        // Two pool afternoons 300 m apart with the same tags, plus distinct
        // events enough to make room tight (room = floor(3 * 0.6) = 1 with 1 spread + 2 singles).
        let pool = |e: u32, day: i64, aes: u8| located(event(e, day, 12, aes), 8.0, 115.0, &["pool", "water"]);
        let mut photos = pool(0, 0, 80);
        photos.extend(pool(1, 2, 78));
        photos.extend(located(event(2, 1, 8, 70), 8.5, 115.5, &["temple"]));
        let mut pool_1 = photos[12..24].to_vec();
        for p in &mut pool_1 {
            p.location = LatLon::new(8.0027, 115.0); // ~300 m north
        }
        photos.splice(12..24, pool_1);
        // 2 spreads + 2 singles = 4 slots -> room floor(2.4) = 2.
        // Merits (engagement = ln(1+m)/ln(1+6); quality = top-3 aesthetic / 100):
        //   pool 0:  0.4*1.000 + 0.4*0.793 + 0.2*1 (first, nothing above) = 0.917
        //   temple:  0.4*0.827 + 0.4*0.693 + 0.2*1 (78 km, no shared tag) = 0.808
        //   pool 1:  0.4*1.000 + 0.4*0.773 + 0.2*0 (300 m, same tags)     = 0.709
        // Without novelty pool 1 scores 0.909 and takes the second place from the temple.
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(2, 16));
        assert_eq!(s[1].tier, Tier::Brief, "{s:?}");
        assert_eq!(s[1].reason, Reason::SimilarTo { event: 0 });
        assert_ne!(s[2].tier, Tier::Brief, "the temple takes the room novelty freed");
    }

    #[test]
    fn gini_is_zero_for_equal_counts_and_high_for_one_winner() {
        assert_eq!(gini(&[5, 5, 5, 5]), 0.0);
        assert!((gini(&[0, 0, 0, 12]) - 0.75).abs() < 1e-9, "{}", gini(&[0, 0, 0, 12]));
        assert_eq!(gini(&[]), 0.0);
        assert_eq!(gini(&[0, 0]), 0.0);
    }

    #[test]
    fn mean_print_averages_componentwise_and_refuses_ragged_input() {
        let a = [1.0_f32, 0.0, 2.0];
        let b = [3.0_f32, 2.0, 0.0];
        assert_eq!(mean_print(&[&a, &b]), Some(vec![2.0, 1.0, 1.0]));
        assert_eq!(mean_print(&[]), None);
        assert_eq!(mean_print(&[&a, &[1.0_f32][..]]), None);
    }

    #[test]
    fn print_distance_is_euclidean() {
        assert!((print_distance(&[0.0, 0.0], &[3.0, 4.0]) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn auc_is_one_for_perfect_separation_and_half_for_none() {
        assert_eq!(auc(&[0.1, 0.2], &[0.5, 0.9]), 1.0);
        assert_eq!(auc(&[0.5], &[0.5]), 0.5);
        assert_eq!(auc(&[0.9], &[0.1]), 0.0);
    }

    /// The brief's `auc(&[0.5], &[0.5]) == 0.5` test is a single pair, so a
    /// tie-mishandling bug there can't be told apart from an all-pairs bug.
    /// This test mixes one exact tie with three clean wins: `same = [0.5,
    /// 0.1]`, `different = [0.5, 0.6]` gives pairs (0.5,0.5)=tie,
    /// (0.5,0.6)=win, (0.1,0.5)=win, (0.1,0.6)=win. Scoring the tie as 0.5
    /// gives 3.5/4 = 0.875; scoring it as 1.0 gives 1.0; scoring it as 0.0
    /// gives 0.75. All three are distinct, so this fails under either wrong
    /// treatment.
    #[test]
    fn auc_counts_a_tied_distance_as_half_a_win_not_a_whole_or_none() {
        assert_eq!(auc(&[0.5, 0.1], &[0.5, 0.6]), 0.875);
    }

    // --- Tier, EventTiers and resolution ----------------------------------

    use crate::book::pace::BookOptions;

    #[test]
    fn tier_resolution_follows_the_majority_of_photos() {
        let mut tiers = EventTiers::new();
        for h in ["a", "b", "c", "d", "e", "f", "g"] {
            tiers.set(h, Some(Tier::Brief));
        }
        for h in ["h", "i", "j"] {
            tiers.set(h, Some(Tier::Featured));
        }
        let event = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        assert_eq!(resolve(event.iter().copied(), &tiers), Some(Tier::Brief));
        // The FIRST photo is Featured here; a first-photo rule would answer Featured.
        let reordered = ["h", "a", "b", "c", "d", "e", "f", "g", "i", "j"];
        assert_eq!(resolve(reordered.iter().copied(), &tiers), Some(Tier::Brief));
    }

    #[test]
    fn tier_resolution_ties_go_to_the_higher_tier_and_unset_is_none() {
        let tiers: EventTiers =
            [("a".to_string(), Tier::Skipped), ("b".to_string(), Tier::Normal)].into_iter().collect();
        assert_eq!(resolve(["a", "b"], &tiers), Some(Tier::Normal));
        assert_eq!(resolve(["x", "y"], &tiers), None);
    }

    #[test]
    fn tier_floors_and_weights_follow_the_spec() {
        let o = BookOptions { featured_floor: 9, ..BookOptions::default() };
        assert_eq!(
            [Tier::Featured, Tier::Normal, Tier::Brief, Tier::Skipped].map(|t| t.floor(&o)),
            [9, 2, 1, 0]
        );
        assert_eq!(
            [Tier::Featured, Tier::Normal, Tier::Brief, Tier::Skipped].map(Tier::weight),
            [2.0, 1.0, 0.5, 0.0]
        );
    }

    #[test]
    fn event_tiers_set_none_removes_the_choice() {
        let mut t = EventTiers::new();
        t.set("a", Some(Tier::Featured));
        t.set("a", None);
        assert!(t.is_empty());
    }

    /// Pins the FULL rank, not just one pair: a transposition of two adjacent
    /// variants (e.g. `Skipped, Normal, Brief, Featured`) changes `Ord`
    /// exactly the way `tier_resolution_ties_go_to_the_higher_tier_and_unset_is_none`
    /// cannot see, because that test only ever compares Skipped against
    /// Normal. Sorting every variant and comparing against the declared order
    /// catches ANY adjacent-pair swap, not only that one.
    #[test]
    fn tier_rank_is_skipped_then_brief_then_normal_then_featured() {
        let mut all = [Tier::Featured, Tier::Skipped, Tier::Normal, Tier::Brief];
        all.sort();
        assert_eq!(all, [Tier::Skipped, Tier::Brief, Tier::Normal, Tier::Featured]);
    }

    #[test]
    fn tier_resolution_ties_go_to_the_higher_tier_for_every_adjacent_pair() {
        let brief_normal: EventTiers =
            [("a".to_string(), Tier::Brief), ("b".to_string(), Tier::Normal)].into_iter().collect();
        assert_eq!(resolve(["a", "b"], &brief_normal), Some(Tier::Normal));

        let normal_featured: EventTiers =
            [("a".to_string(), Tier::Normal), ("b".to_string(), Tier::Featured)].into_iter().collect();
        assert_eq!(resolve(["a", "b"], &normal_featured), Some(Tier::Featured));

        let skipped_brief: EventTiers =
            [("a".to_string(), Tier::Skipped), ("b".to_string(), Tier::Brief)].into_iter().collect();
        assert_eq!(resolve(["a", "b"], &skipped_brief), Some(Tier::Brief));
    }

    /// An untiered hash casts no vote: one Brief photo plus three photos with
    /// no stored choice still resolves to Brief, not to a tie or to `None`.
    #[test]
    fn tier_resolution_ignores_untiered_hashes_when_counting_votes() {
        let tiers: EventTiers = [("a".to_string(), Tier::Brief)].into_iter().collect();
        assert_eq!(resolve(["a", "x", "y", "z"], &tiers), Some(Tier::Brief));
    }
}
