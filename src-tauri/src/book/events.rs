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

/// The chosen constants of §3 and §4 in one value, so the report's
/// sensitivity sweep (§9 check 4) can vary them one at a time. `Default` is
/// exactly the `pub const`s above, and `suggest` always uses it: the app has
/// no path that tunes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    pub w_engagement: f64,
    pub w_quality: f64,
    pub w_novelty: f64,
    pub utility_skip: f64,
    pub featured_margin: f64,
    pub normal_slot_share: f64,
    pub novelty_km: f64,
    pub similar_below: f64,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            w_engagement: W_ENGAGEMENT,
            w_quality: W_QUALITY,
            w_novelty: W_NOVELTY,
            utility_skip: UTILITY_SKIP,
            featured_margin: FEATURED_MARGIN,
            normal_slot_share: NORMAL_SLOT_SHARE,
            novelty_km: NOVELTY_KM,
            similar_below: SIMILAR_BELOW,
        }
    }
}
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
///
/// `moments` comes from `pack::moments` over the WHOLE `kept` slice, not
/// per event, and that function makes an undated photo its own moment. So
/// in a library with no dates at all, every keeper is its own moment and
/// `engagement` (§3) degenerates to counting keepers -- burst length, not
/// distinct things photographed.
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
pub fn normal_room(eligible: usize, capacity: &Capacity, t: &Tuning) -> usize {
    let slots = capacity.spreads as usize + capacity.singles as usize;
    eligible.min((slots as f64 * t.normal_slot_share).floor() as usize)
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// How alike two events are, 0..1: the highest of place, scene and look (§3.1).
fn similarity(a: &EventStats, b: &EventStats, t: &Tuning) -> f64 {
    let gps = match (a.centroid, b.centroid) {
        (Some(x), Some(y)) => 1.0 - (x.km_to(y) / t.novelty_km).min(1.0),
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

/// Suggests a tier and reason for every event in `stats`, sorted by event
/// id (§4). `stats` must carry one entry per event id -- `stats()` above
/// guarantees this by construction -- since a repeated id would double an
/// event's vote in `normal_room`, the median and the novelty walk.
pub fn suggest(stats: &[EventStats], capacity: &Capacity) -> Vec<Suggestion> {
    suggest_with(stats, capacity, &Tuning::default())
}

/// `suggest` under explicit constants, for the report's sensitivity sweep.
pub fn suggest_with(stats: &[EventStats], capacity: &Capacity, t: &Tuning) -> Vec<Suggestion> {
    debug_assert!(
        stats.iter().map(|s| s.event).collect::<BTreeSet<_>>().len() == stats.len(),
        "suggest: event ids must be unique"
    );
    let mut out: Vec<Suggestion> = Vec::new();
    let mut eligible: Vec<&EventStats> = Vec::new();
    // "Undated" only means something beside dated events; in a library with
    // no dates at all it would make every event Brief.
    let any_dated = stats.iter().any(|s| !s.undated);
    for s in stats {
        let fixed = |tier, reason| Suggestion { event: s.event, tier, reason, merit: 0.0 };
        if s.utility > t.utility_skip {
            out.push(fixed(Tier::Skipped, Reason::Utility { share: s.utility }));
        } else if s.keepers == 0 {
            out.push(fixed(Tier::Skipped, Reason::NothingKept));
        } else if s.undated && any_dated {
            out.push(fixed(Tier::Brief, Reason::Undated));
        } else {
            eligible.push(s);
        }
    }

    // Normalised against the OTHER eligible events only: a Skipped or Brief
    // (undated) event's moment count must not deflate everyone else's
    // engagement, since those events never reach the ranking below.
    let most = eligible.iter().map(|s| s.moments).max().unwrap_or(0);
    let engagement = |s: &EventStats| {
        if most == 0 { 0.0 } else { (1.0 + s.moments as f64).ln() / (1.0 + most as f64).ln() }
    };
    let base = |s: &EventStats| (t.w_engagement * engagement(s) + t.w_quality * s.quality) * (1.0 - s.utility);

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
                .map(|o| (similarity(s, o, t), Some(o.event)))
                .fold((0.0, None), |best, cur| if cur.0 > best.0 { cur } else { best });
            let novelty = 1.0 - sim;
            let merit = (t.w_engagement * engagement(s) + t.w_quality * s.quality + t.w_novelty * novelty)
                * (1.0 - s.utility);
            Scored { s, merit, novelty, nearest, base_rank: k }
        })
        .collect();
    scored.sort_by(|a, b| b.merit.total_cmp(&a.merit).then(a.s.event.cmp(&b.s.event)));

    let of = scored.len();
    let room = normal_room(of, capacity, t);
    let featured_limit = (room / 6).max(1);
    // The Featured bar is the median merit of the events that STAY Normal,
    // not the whole top-`room` set: a candidate must clear a bar set by its
    // peers, not by a bar it could itself inflate by being counted in the
    // median. `scored` is sorted by merit descending, so ranks
    // `0..featured_limit` are the only ranks any event can be promoted from
    // (the cap enforces that); `featured_limit..room` is guaranteed to stay
    // Normal and is what the median is measured over. When `room <=
    // featured_limit` there is no such slice, so nothing is Featured --
    // there is nothing left to stand out from.
    let normal: &[Scored] = if room > featured_limit { &scored[featured_limit..room] } else { &[] };
    let mut top: Vec<f64> = normal.iter().map(|x| x.merit).collect();
    top.sort_by(f64::total_cmp);
    let median = if top.is_empty() {
        0.0
    } else if top.len() % 2 == 1 {
        top[top.len() / 2]
    } else {
        (top[top.len() / 2 - 1] + top[top.len() / 2]) / 2.0
    };
    let mut featured = 0;
    for (rank, x) in scored.iter().enumerate() {
        let (tier, reason) = if rank < room {
            if !normal.is_empty() && featured < featured_limit && x.merit >= t.featured_margin * median {
                featured += 1;
                (Tier::Featured, Reason::Standout)
            } else {
                (Tier::Normal, Reason::Ranked { rank: rank + 1, of })
            }
        } else if x.novelty < t.similar_below && x.base_rank < room {
            // A proxy for "novelty is what pushed it out": an event with
            // near-zero novelty that would have taken a normal/featured slot
            // by BASE merit (before novelty was applied) lost its place to
            // its near-duplicate, so it is named as a duplicate rather than
            // merely reported as out of room.
            (Tier::Brief, Reason::SimilarTo { event: x.nearest.unwrap_or(x.s.event) })
        } else {
            (Tier::Brief, Reason::OutOfRoom { rank: rank + 1, of })
        };
        out.push(Suggestion { event: x.s.event, tier, reason, merit: x.merit });
    }
    out.sort_by_key(|s| s.event);
    out
}

use crate::book::cull::Overrides;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPlan {
    pub event: u32,
    /// Effective: the user's choice, else the suggestion, after `budget`'s adjustments.
    pub tier: Tier,
    pub suggested: Tier,
    pub chosen: bool,
    pub reason: Reason,
    pub merit: f64,
    pub moments: usize,
    pub kept: usize,
    pub photos: usize,
}

/// Every event's effective tier, sorted by event id.
pub fn plan(photos: &[Photo], kept: &[Photo], tiers: &EventTiers, capacity: &Capacity) -> Vec<EventPlan> {
    let stats = stats(photos, kept);
    let suggestions = suggest(&stats, capacity);
    let mut hashes: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for p in photos {
        hashes.entry(p.event_cluster).or_default().push(p.hash.as_str());
    }
    suggestions
        .into_iter()
        .zip(&stats)
        .map(|(s, st)| {
            debug_assert_eq!(s.event, st.event);
            let chosen = resolve(hashes.get(&s.event).into_iter().flatten().copied(), tiers);
            EventPlan {
                event: s.event,
                tier: chosen.unwrap_or(s.tier),
                suggested: s.tier,
                chosen: chosen.is_some(),
                reason: s.reason,
                merit: s.merit,
                moments: st.moments,
                kept: st.keepers,
                photos: st.photos,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierOverflow {
    pub needed: usize,
    pub capacity: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    /// Sorted indices into `kept`.
    pub selected: Vec<usize>,
    pub plan: Vec<EventPlan>,
    /// What each event is guaranteed: `min(floor, available)`, never below its Includes.
    pub floors: BTreeMap<u32, usize>,
    pub overflow: Option<TierOverflow>,
}

impl Budget {
    pub fn brief_events(&self) -> BTreeSet<u32> {
        self.plan.iter().filter(|p| p.tier == Tier::Brief).map(|p| p.event).collect()
    }

    pub fn selected_in(&self, kept: &[Photo], event: u32) -> usize {
        self.selected.iter().filter(|&&i| kept[i].event_cluster == event).count()
    }
}

/// Deals the book's `target_photos` out to events by tier (§5).
///
/// Availability (`avail`, via `pack::event_order`) and floors are per event,
/// but the photo ranking underneath is library-wide: `moments` is computed
/// once over every kept photo, not per event, so two events that overlap in
/// capture time share the same `MAX_PER_MOMENT` slots rather than each
/// getting their own.
pub fn budget(
    kept: &[Photo],
    mut plan: Vec<EventPlan>,
    capacity: &Capacity,
    overrides: &Overrides,
    options: &BookOptions,
) -> Budget {
    let order = pack::event_order(kept, overrides);
    let target = capacity.target_photos;
    let avail = |e: u32| order.get(&e).map_or(0, |o| o.list.len());
    let distinct = |e: u32| order.get(&e).map_or(0, |o| o.distinct);
    let included = |e: u32| order.get(&e).map_or(0, |o| o.included);
    // A floor guarantees different pictures: an event of one shot retaken
    // twenty times is owed that shot once, not six copies of it.
    let floor_at = |p: &EventPlan, tier: Tier| tier.floor(options).min(distinct(p.event)).max(included(p.event));
    let floor_of = |p: &EventPlan| floor_at(p, p.tier);
    let total = |plan: &[EventPlan]| plan.iter().map(floor_of).sum::<usize>();

    // Step 3: floors that do not fit demote SUGGESTIONS, lowest merit first:
    // Featured -> Normal, then Normal -> Brief, then (R9) Brief -> Skipped.
    // A Skipped event still places its Includes -- `floor_of` maxes with
    // `included`, and an Include always wins regardless of tier. A
    // candidate is only eligible when the new tier's floor is strictly
    // less than its current one: an event whose floor is already held up
    // by its own Includes (e.g. a Brief event with one Include has floor 1
    // either as Brief or as Skipped) frees nothing by being demoted, so it
    // is skipped in favour of the next-lowest-merit event that actually
    // shrinks the total.
    for (from, to) in [(Tier::Featured, Tier::Normal), (Tier::Normal, Tier::Brief), (Tier::Brief, Tier::Skipped)] {
        while total(&plan) > target {
            let Some(p) = plan
                .iter_mut()
                .filter(|p| !p.chosen && p.tier == from && floor_at(p, to) < floor_at(p, from))
                .min_by(|a, b| a.merit.total_cmp(&b.merit).then(b.event.cmp(&a.event)))
            else {
                break;
            };
            p.tier = to;
            p.reason = Reason::Demoted;
        }
    }
    // By now only the user's OWN floors (`chosen` tiers) and Includes can
    // still overflow the target -- every suggested event has already been
    // demoted as far as Skipped.
    let mut floors: BTreeMap<u32, usize> = plan.iter().map(|p| (p.event, floor_of(p))).collect();
    let needed: usize = floors.values().sum();
    let mut overflow = None;
    if needed > target {
        overflow = Some(TierOverflow { needed, capacity: target });
        for p in &plan {
            let one = if p.tier == Tier::Skipped { 0 } else { 1.min(avail(p.event)) };
            floors.insert(p.event, one.max(included(p.event)));
        }
    }

    // Step 4: D'Hondt over the remainder, vote = weight x sqrt(moments).
    // Step 4b: when nothing can take a photo, promote the best suggested Brief.
    // Both run first over each event's different pictures only, then again
    // over its look-alikes, so no event prints a second copy of a shot while
    // another still has a picture the book has not shown.
    let mut quota = floors.clone();
    let mut left = target.saturating_sub(quota.values().sum());
    let mut copies = false;
    while left > 0 {
        let limit = |e: u32| if copies { avail(e) } else { distinct(e) };
        let open = |p: &&EventPlan| {
            let q = quota[&p.event];
            p.tier != Tier::Skipped
                && q < limit(p.event)
                && (p.tier != Tier::Brief || q < options.brief_cap as usize)
        };
        let score = |p: &EventPlan| p.tier.weight() * (p.moments as f64).sqrt() / (quota[&p.event] + 1) as f64;
        let pick = plan
            .iter()
            .filter(open)
            .max_by(|a, b| score(a).total_cmp(&score(b)).then(b.event.cmp(&a.event)))
            .map(|p| p.event);
        match pick {
            Some(e) => {
                if let Some(q) = quota.get_mut(&e) {
                    *q += 1;
                }
                left -= 1;
            }
            None => {
                let promote = plan
                    .iter_mut()
                    .filter(|p| !p.chosen && p.tier == Tier::Brief && limit(p.event) > quota[&p.event])
                    .max_by(|a, b| a.merit.total_cmp(&b.merit).then(b.event.cmp(&a.event)));
                match promote {
                    Some(p) => {
                        p.tier = Tier::Normal;
                        p.reason = Reason::Filled;
                    }
                    None if !copies => copies = true,
                    None => break,
                }
            }
        }
    }

    let mut selected: Vec<usize> = order
        .iter()
        .flat_map(|(e, o)| o.list.iter().take(quota.get(e).copied().unwrap_or(0)).copied())
        .collect();
    selected.sort_unstable();
    Budget { selected, plan, floors, overflow }
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

    /// An ordinary (non-utility) event that the culler dropped every photo
    /// of -- e.g. all near-duplicates of a worse shot elsewhere -- gets
    /// `NothingKept`/Skipped, distinctly from the `Utility` skip reason.
    #[test]
    fn an_event_with_no_cull_keepers_is_suggested_skipped() {
        let mut photos = event(0, 0, 20, 80);
        photos.extend(event(1, 1, 20, 85));
        let kept: Vec<Photo> = keepers(&photos).into_iter().filter(|p| p.event_cluster != 1).collect();
        let s = suggest(&stats(&photos, &kept), &capacity(9, 45));
        assert_eq!(s[1].tier, Tier::Skipped, "{s:?}");
        assert_eq!(s[1].reason, Reason::NothingKept);
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
        assert_eq!(normal_room(50, &capacity(9, 45), &Tuning::default()), 6); // 11 slots
        assert_eq!(normal_room(50, &capacity(19, 85), &Tuning::default()), 12); // 21 slots
        assert_eq!(normal_room(3, &capacity(19, 85), &Tuning::default()), 3);
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
        assert_eq!(count(Tier::Featured), 1, "{s:?}");
        assert_eq!(count(Tier::Featured) + count(Tier::Normal), 6);
        assert_eq!(count(Tier::Brief), 4);
        assert!(s.iter().filter(|x| x.tier == Tier::Brief).all(|x| matches!(
            x.reason,
            Reason::OutOfRoom { .. } | Reason::SimilarTo { .. }
        )));
        // e0-e3 share "tag" and are more than NOVELTY_KM apart from every
        // other event, so novelty is 0 for all of them -- but each ranked
        // below `room` by BASE merit too (base_rank 6..9), so they are out
        // of room on their own account, not standing in for a look-alike.
        for x in &s[0..4] {
            assert!(matches!(x.reason, Reason::OutOfRoom { .. }), "{x:?}");
        }
    }

    #[test]
    fn suggest_is_suggest_with_default_tuning() {
        // The fixture of events_beyond_the_room_are_brief_and_a_standout_is_featured.
        let mut photos = Vec::new();
        for e in 0..9 {
            photos.extend(located(event(e, e as i64, 12, 40 + e as u8), 10.0 + e as f64, 100.0, &["tag"]));
        }
        photos.extend(located(event(9, 9, 36, 95), 50.0, 50.0, &["summit"]));
        let st = stats(&photos, &keepers(&photos));
        let cap = capacity(9, 45);
        assert_eq!(suggest(&st, &cap), suggest_with(&st, &cap, &Tuning::default()));
        let t = Tuning { normal_slot_share: 0.3, ..Tuning::default() };
        assert_ne!(suggest(&st, &cap), suggest_with(&st, &cap, &t), "the tuning is actually read");
    }

    /// Each field of `Tuning`, pushed to an extreme, changes the output: none
    /// of them is silently read from its constant instead.
    #[test]
    fn every_tuning_field_is_read() {
        let mut standout = Vec::new();
        for e in 0..9 {
            standout.extend(located(event(e, e as i64, 12, 40 + e as u8), 10.0 + e as f64, 100.0, &["tag"]));
        }
        standout.extend(located(event(9, 9, 36, 95), 50.0, 50.0, &["summit"]));
        let standout = stats(&standout, &keepers(&standout));
        // Two same-place pool days and a distinct event, room 2: the lower pool
        // day is Brief as SimilarTo the first.
        let mut pool = located(event(0, 0, 12, 80), 8.0, 115.0, &["pool"]);
        pool.extend(located(event(1, 2, 12, 78), 8.0, 115.0, &["pool"]));
        pool.extend(located(event(2, 4, 12, 70), 30.0, 10.0, &["temple"]));
        let pool = stats(&pool, &keepers(&pool));
        let changes = |edit: fn(&mut Tuning)| {
            let mut t = Tuning::default();
            edit(&mut t);
            suggest_with(&standout, &capacity(9, 45), &t) != suggest(&standout, &capacity(9, 45))
                || suggest_with(&pool, &capacity(2, 16), &t) != suggest(&pool, &capacity(2, 16))
        };
        assert!(changes(|t| t.w_engagement = 0.0), "w_engagement");
        assert!(changes(|t| t.w_quality = 0.0), "w_quality");
        assert!(changes(|t| t.w_novelty = 0.0), "w_novelty");
        assert!(changes(|t| t.utility_skip = -1.0), "utility_skip");
        assert!(changes(|t| t.featured_margin = 10.0), "featured_margin");
        assert!(changes(|t| t.normal_slot_share = 0.3), "normal_slot_share");
        assert!(changes(|t| t.novelty_km = 1e6), "novelty_km");
        assert!(changes(|t| t.similar_below = 0.0), "similar_below");
    }

    #[test]
    fn tuning_default_is_the_spec_constants() {
        let t = Tuning::default();
        assert_eq!(
            [t.w_engagement, t.w_quality, t.w_novelty, t.utility_skip, t.featured_margin, t.normal_slot_share, t.novelty_km, t.similar_below],
            [0.4, 0.4, 0.2, 0.8, 1.3, 0.6, 2.0, 0.3]
        );
    }

    /// Same shape as the standout test above, but the "standout" only edges
    /// out the Normal median rather than clearing 1.3x it, so it must stay
    /// Normal: R-a measures the Featured bar against the Normal events, and
    /// an event that merely ties or slightly beats its peers is not a
    /// standout.
    #[test]
    fn an_event_that_does_not_clear_the_featured_margin_stays_normal() {
        // 9 identical events, each with its own tag and > NOVELTY_KM from
        // every other, so novelty is 1 for all ten and plays no favourites.
        // A 10th ("almost") has a slim edge in moments and quality -- real,
        // but nowhere near 1.3x the other nine's merit -- so it must stay
        // Normal rather than being called a standout.
        let mut photos = Vec::new();
        for e in 0..9 {
            let tag = format!("tag{e}");
            photos.extend(located(event(e, e as i64, 12, 50), 10.0 + e as f64 * 10.0, 100.0, &[&tag]));
        }
        photos.extend(located(event(9, 9, 14, 55), 80.0, 50.0, &["summit"]));
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        let count = |t: Tier| s.iter().filter(|x| x.tier == t).count();
        assert_eq!(count(Tier::Featured), 0, "{s:?}");
        assert_eq!(s[9].tier, Tier::Normal, "{:?}", s[9]);
        assert!(matches!(s[9].reason, Reason::Ranked { .. }), "{:?}", s[9]);
    }

    /// R-a: the Featured bar is 1.3x the median merit of the events that
    /// STAY Normal (`scored[featured_limit..room]`), not the median of the
    /// whole top-`room` set including the candidate itself
    /// (`scored[..room]`). Two events, room 2 (via `capacity(9, 45)` with
    /// only 2 eligible events: 11 slots -> floor(11*0.6) = 6, capped to the
    /// 2 eligible), `featured_limit = max(1, 2/6) = 1`, so `normal` is just
    /// event 1 -- its merit alone is the median under the fix.
    ///
    /// Event 0 (candidate): 6 moments (12 photos), aesthetic 90 -- engagement
    /// 1.0, quality (top-3 mean of [90,90,89,...]/100) ~0.8967, novelty 1.0
    /// (walks first, nothing above it): merit = 0.4*1.0 + 0.4*0.8967 +
    /// 0.2*1.0 ~ 0.9587.
    /// Event 1 (peer): 1 moment (2 photos), aesthetic 50 -- engagement
    /// ln(2)/ln(7) ~ 0.3562, quality (mean of [50,49]/100) 0.495, novelty
    /// 1.0 (no GPS/tag overlap with event 0): merit = 0.4*0.3562 +
    /// 0.4*0.495 + 0.2*1.0 ~ 0.5405.
    ///
    /// Fixed code: median = event 1's merit alone (~0.5405); 1.3 * 0.5405 ~
    /// 0.7026 <= event 0's ~0.9587 -> Featured.
    /// Mutant R (`&scored[..room]`, i.e. median of BOTH events): median =
    /// (0.9587 + 0.5405) / 2 ~ 0.7496; 1.3 * 0.7496 ~ 0.9745 > event 0's
    /// ~0.9587 -> NOT Featured. This test must catch that regression.
    #[test]
    fn a_candidate_that_clears_the_peer_median_but_not_the_self_inclusive_one_is_featured() {
        let candidate = located(event(0, 0, 12, 90), 10.0, 100.0, &["tagA"]);
        let peer = located(event(1, 1, 2, 50), 80.0, 50.0, &["tagB"]);
        let mut photos = candidate;
        photos.extend(peer);
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        assert_eq!(s[0].tier, Tier::Featured, "{s:?}");
        assert_eq!(s[0].reason, Reason::Standout);
    }

    /// R-a's `!normal.is_empty()` guard: when `room <= featured_limit`
    /// there are no events left to measure a Normal median from
    /// (`normal` is empty, so `median` defaults to 0.0), and NOTHING may be
    /// Featured -- not even a clear standout -- because there is nothing to
    /// stand out from. `capacity(0, n)`: 0 spreads + 2 singles = 2 slots,
    /// floor(2*0.6) = 1, so `room = 1` and `featured_limit = max(1, 1/6) =
    /// 1`; `room > featured_limit` is false, so `normal` is empty.
    ///
    /// Without the guard, `median` would be 0.0 and `x.merit >= 1.3 * 0.0`
    /// is true for any positive merit, so the top-ranked event would be
    /// wrongly promoted to Featured on an empty comparison set.
    #[test]
    fn a_room_of_one_never_features_even_a_clear_standout() {
        let standout = located(event(0, 0, 20, 95), 10.0, 100.0, &["tagA"]);
        let mut photos = standout;
        photos.extend(located(event(1, 1, 4, 40), 80.0, 50.0, &["tagB"]));
        photos.extend(located(event(2, 2, 4, 40), 20.0, 150.0, &["tagC"]));
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(0, 45));
        let count = |t: Tier| s.iter().filter(|x| x.tier == t).count();
        assert_eq!(count(Tier::Featured), 0, "{s:?}");
        assert_eq!(s[0].tier, Tier::Normal, "{:?}", s[0]);
        assert!(matches!(s[0].reason, Reason::Ranked { .. }), "{:?}", s[0]);
    }

    /// Two events, 12 photos each, all kept, same aesthetic pattern -- the
    /// only difference is how many distinct moments they group into. §3
    /// defines engagement from moments, not photo or keeper count, so the
    /// event with more moments must rank higher even though every count
    /// (photos, keepers) tied.
    #[test]
    fn more_moments_ranks_above_more_photos_at_equal_keeper_count() {
        let many_moments = located(event(0, 0, 12, 60), 10.0, 100.0, &["tagA"]);
        let mut few_moments = located(event(1, 1, 12, 60), 80.0, 50.0, &["tagB"]);
        // Collapse the 6 normal moments (600 s apart) into a 330 s burst.
        // `pack::moments` starts a new moment once a photo lands more than
        // MOMENT_GAP_SECONDS (120 s) past the CURRENT moment's first photo,
        // so 12 photos 30 s apart (0, 30, .., 330 s) split into 3 moments
        // (0-120 s, 150-270 s, 300-330 s) -- same 12 photos, same 12
        // keepers, 3 moments instead of 6.
        for (n, p) in few_moments.iter_mut().enumerate() {
            p.captured_at = Some(86_400 + n as i64 * 30);
        }
        let mut photos = many_moments;
        photos.extend(few_moments);
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        let merit = |e: u32| s.iter().find(|x| x.event == e).unwrap().merit;
        assert!(merit(0) > merit(1), "{s:?}");
    }

    /// Two events with the same photo/keeper count and moments, but a
    /// quality distribution where the top-3 mean and the whole-keeper mean
    /// disagree on which is better: event 0's best three keepers (90) beat
    /// event 1's flat 60, but event 0's other two keepers (10) are bad
    /// enough to pull its ALL-keeper mean (58) below event 1's (60). §3
    /// scores quality from the top `QUALITY_TOP` keepers only, so event 0
    /// must still rank above event 1.
    #[test]
    fn quality_ranks_by_the_top_keepers_not_the_whole_event_mean() {
        let mut best_few = located(event(0, 0, 5, 90), 10.0, 100.0, &["tagA"]);
        for (n, aes) in [90u8, 90, 90, 10, 10].into_iter().enumerate() {
            best_few[n].aesthetic_pct = aes;
        }
        let mut flat = located(event(1, 1, 5, 60), 80.0, 50.0, &["tagB"]);
        for p in &mut flat {
            p.aesthetic_pct = 60;
        }
        let mut photos = best_few;
        photos.extend(flat);
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        let merit = |e: u32| s.iter().find(|x| x.event == e).unwrap().merit;
        assert!(merit(0) > merit(1), "{s:?}");
    }

    /// UTILITY_SKIP is a share, not a round number: 0.775 stays eligible,
    /// 0.825 -- just over the 0.8 bar -- is Skipped.
    #[test]
    fn utility_share_boundary_below_and_above_the_skip_threshold() {
        let mut below = event(0, 0, 40, 80);
        for p in below.iter_mut().take(31) {
            p.is_utility = true; // 31 / 40 = 0.775
        }
        let mut above = event(1, 1, 40, 80);
        for p in above.iter_mut().take(33) {
            p.is_utility = true; // 33 / 40 = 0.825
        }
        let mut photos = below;
        photos.extend(above);
        let s = suggest(&stats(&photos, &keepers(&photos)), &capacity(9, 45));
        assert_ne!(s[0].tier, Tier::Skipped, "{:?}", s[0]);
        assert_eq!(s[1].tier, Tier::Skipped, "{:?}", s[1]);
        assert!(
            matches!(s[1].reason, Reason::Utility { share } if (share - 0.825).abs() < 1e-9),
            "{:?}",
            s[1]
        );
    }

    #[test]
    fn of_two_look_alike_events_the_lower_is_brief_when_room_is_tight() {
        // Two pool afternoons 300 m apart with the same tags, plus a distinct
        // event, with a capacity(2, 16) fixture tight enough that room = 2
        // (see below) has no space for both pool events.
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

    /// `from_token`'s first real caller is `Db::load_project` (task 9), which
    /// trusts it to refuse anything that is not exactly one of the four wire
    /// tokens -- an unknown token must fail the load rather than silently
    /// becoming a tier. Round-trips every token through `as_str` and checks
    /// four near-misses: `"auto"` (never a stored token -- absence IS auto),
    /// the empty string, a capitalised token (tokens are lowercase, case
    /// matters), and a plausible-looking word that is not one of the four.
    #[test]
    fn tier_from_token_round_trips_the_four_tokens_and_refuses_everything_else() {
        for tier in [Tier::Skipped, Tier::Brief, Tier::Normal, Tier::Featured] {
            assert_eq!(Tier::from_token(tier.as_str()), Some(tier), "{tier:?}");
        }
        for bad in ["auto", "", "Featured", "hero"] {
            assert_eq!(Tier::from_token(bad), None, "{bad:?}");
        }
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

    // --- plan and budget -----------------------------------------------

    use crate::book::cull::{Override, Overrides};

    fn plan_of(photos: &[Photo], tiers: &EventTiers, cap: &Capacity) -> Vec<EventPlan> {
        plan(photos, &keepers(photos), tiers, cap)
    }

    fn placed(b: &Budget, kept: &[Photo], event: u32) -> usize {
        b.selected_in(kept, event)
    }

    #[test]
    fn budget_gives_a_small_dull_normal_event_its_floor_beside_a_huge_bright_one() {
        // Event 0's 400 photos, 2 per moment 600 s apart, span ~33 hours --
        // longer than one day -- so event 1 sits on day 2, not day 1, or its
        // "later" timestamps actually fall chronologically INSIDE event 0's
        // span. `pack::moments` (and so `event_order`'s per-moment cap) is
        // computed over the whole kept slice regardless of event, so photos
        // from two events that share a moment window compete for the same
        // MAX_PER_MOMENT slots -- and event 0's far brighter photos would
        // win every one of them, starving event 1 down to zero available
        // before its floor is ever applied.
        let mut photos = event(0, 0, 400, 95);
        photos.extend(event(1, 2, 6, 20));
        let kept = keepers(&photos);
        let tiers: EventTiers = photos[400..].iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
        let cap = capacity(9, 45);
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert!(placed(&b, &kept, 1) >= 2, "got {}", placed(&b, &kept, 1));
        assert_eq!(b.selected.len(), 45);
    }

    #[test]
    fn the_remainder_vote_uses_the_square_root_of_moments() {
        // 400 moments against 25: sqrt gives a 4:1 vote, a linear vote 16:1.
        // Event 0's 800 photos span ~66.5 hours (2.77 days), so event 1 sits
        // on day 3 -- otherwise its "later" timestamps land chronologically
        // inside event 0's span and the two compete for the same global
        // per-moment cap (see the comment on the previous test).
        let mut photos = event(0, 0, 800, 70);
        photos.extend(event(1, 3, 50, 70));
        let kept = keepers(&photos);
        let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
        let cap = capacity(19, 85);
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        let (big, small) = (placed(&b, &kept, 0), placed(&b, &kept, 1));
        assert!(big <= 5 * small, "big {big} small {small}: more than 5x means the vote is not sqrt");
    }

    #[test]
    fn a_skipped_event_places_nothing_but_its_includes() {
        let mut photos = event(0, 0, 30, 60);
        photos.extend(event(1, 1, 30, 99));
        let kept = keepers(&photos);
        let tiers: EventTiers = photos[30..].iter().map(|p| (p.hash.clone(), Tier::Skipped)).collect();
        let cap = capacity(9, 45);
        let none = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!(placed(&none, &kept, 1), 0);
        let mut overrides = Overrides::new();
        overrides.set(photos[40].hash.clone(), Override::Include);
        let one = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &overrides, &BookOptions::default());
        let idx = kept.iter().position(|p| p.hash == photos[40].hash).unwrap();
        assert!(one.selected.contains(&idx), "the Include in a Skipped event is selected");
        assert_eq!(placed(&one, &kept, 1), 1);
    }

    /// Iceland 2025's night sky: 18 frames of one star field and 2 of the
    /// aurora, a time-lapse too long for the cull to merge. Its many moments
    /// out-voted a smaller event, so 5 of its frames were dealt -- three the
    /// same stars -- while the other event had pictures to spare.
    ///
    /// Event 0 is ten frames of ONE picture over five moments; event 1 is four
    /// different pictures over two. Six photos: event 0's single picture and
    /// all four of event 1's before a second copy of anything, which still
    /// fills the book.
    #[test]
    fn budget_deals_different_pictures_before_a_look_alike() {
        let print = |axis: usize| {
            let mut p = vec![0.0f32; 8];
            p[axis] = 1.0;
            Some(p)
        };
        let mut photos = event(0, 0, 10, 80);
        for p in &mut photos {
            p.feature_print = print(0);
        }
        let mut other = event(1, 1, 4, 80);
        for (n, p) in other.iter_mut().enumerate() {
            p.feature_print = print(n + 1);
        }
        photos.extend(other);
        let kept = keepers(&photos);
        let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
        let cap = capacity(1, 6);
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!((placed(&b, &kept, 0), placed(&b, &kept, 1)), (2, 4));
        assert_eq!(b.floors[&0], 1, "a floor guarantees different pictures, not copies of one");
    }

    #[test]
    fn a_brief_event_stops_at_the_brief_cap_with_room_to_spare() {
        let mut photos = event(0, 0, 8, 60);
        photos.extend(event(1, 1, 20, 99)); // 10 moments
        let kept = keepers(&photos);
        let mut tiers: EventTiers = photos[8..].iter().map(|p| (p.hash.clone(), Tier::Brief)).collect();
        for p in &photos[..8] {
            tiers.set(p.hash.clone(), Some(Tier::Normal));
        }
        let cap = capacity(19, 85);
        for brief_cap in 1..=3u8 {
            let o = BookOptions { brief_cap, ..BookOptions::default() };
            let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &o);
            assert_eq!(placed(&b, &kept, 1), brief_cap as usize);
        }
    }

    #[test]
    fn a_featured_event_gets_the_projects_featured_floor() {
        let mut photos = event(0, 0, 200, 95);
        photos.extend(event(1, 1, 30, 10));
        let kept = keepers(&photos);
        let tiers: EventTiers = photos[200..].iter().map(|p| (p.hash.clone(), Tier::Featured)).collect();
        let cap = capacity(9, 45);
        let o = BookOptions { featured_floor: 9, ..BookOptions::default() };
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &o);
        assert!(placed(&b, &kept, 1) >= 9, "got {}", placed(&b, &kept, 1));
        // Assert the floor itself, not just the post-D'Hondt count: event 1's
        // Featured weight (2.0, highest of any tier) dominates the remainder
        // vote regardless of its floor, so `placed >= 9` alone still holds
        // even if `floor()` silently ignored `options.featured_floor`.
        assert_eq!(b.floors[&1], 9, "the floor itself must come from options.featured_floor");
    }

    #[test]
    fn floors_that_do_not_fit_demote_suggestions_but_never_the_users_choice() {
        // 12 Normal events at floor 2 = 24 > target 20. Event 0 is the user's.
        let mut photos = Vec::new();
        for e in 0..12u32 {
            photos.extend(event(e, e as i64, 8, 50 + e as u8));
        }
        let kept = keepers(&photos);
        let tiers: EventTiers = photos[..8].iter().map(|p| (p.hash.clone(), Tier::Normal)).collect();
        let cap = capacity(20, 20); // room = 13, so all 12 are suggested Normal/Featured
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert!(b.overflow.is_none());
        assert_eq!(b.plan[0].tier, Tier::Normal, "the user's choice is kept");
        assert!(b.plan.iter().any(|p| p.reason == Reason::Demoted));
        assert!(b.floors.values().sum::<usize>() <= 20);
    }

    #[test]
    fn users_floors_that_cannot_fit_report_tier_overflow_and_drop_to_one() {
        let mut photos = Vec::new();
        for e in 0..6u32 {
            photos.extend(event(e, e as i64, 20, 60));
        }
        let kept = keepers(&photos);
        let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Featured)).collect();
        let cap = capacity(9, 20); // 6 x 6 = 36 > 20
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!(b.overflow, Some(TierOverflow { needed: 36, capacity: 20 }));
        assert!(b.floors.values().all(|&f| f == 1));
        assert_eq!(b.selected.len(), 20, "the rest is still dealt out");
    }

    #[test]
    fn a_book_whose_normal_events_run_dry_promotes_a_suggested_brief_rather_than_underfill() {
        // 12 events of 4 photos each (2 moments apiece, so every event ties
        // on engagement and merit ranks purely by aesthetic): room 1 at 3
        // slots picks exactly one Normal event, the rest sit Brief and cap
        // at `brief_cap` (2). Brief-capped total is 2 + 11*2 = 24, and even
        // the Normal event's own pool is only 4 photos, so the un-promoted
        // ceiling is 4 + 11*2 = 26 -- short of the 40-photo target. Filling
        // the rest requires rule 4b to promote suggested-Brief events to
        // Normal (lifting their cap from `brief_cap` to their own pool of 4)
        // until the target is met, rather than leaving the book underfull.
        let mut photos = Vec::new();
        for e in 0..12u32 {
            photos.extend(event(e, e as i64, 4, 90 - e as u8));
        }
        let kept = keepers(&photos);
        let cap = capacity(1, 40); // 3 slots -> room 1
        let b = budget(&kept, plan_of(&photos, &EventTiers::new(), &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!(b.selected.len(), 40);
        assert!(b.plan.iter().any(|p| p.reason == Reason::Filled));
    }

    #[test]
    fn more_room_only_adds_photos_to_an_event() {
        let mut photos = event(0, 0, 60, 80);
        photos.extend(event(1, 1, 40, 70));
        let kept = keepers(&photos);
        let small = capacity(9, 45);
        let large = capacity(19, 85);
        let a = budget(&kept, plan_of(&photos, &EventTiers::new(), &small), &small, &Overrides::new(), &BookOptions::default());
        let b = budget(&kept, plan_of(&photos, &EventTiers::new(), &large), &large, &Overrides::new(), &BookOptions::default());
        assert!(a.selected.iter().all(|i| b.selected.contains(i)), "a prefix, never a swap");
    }

    #[test]
    fn floors_that_do_not_fit_demote_featured_before_normal() {
        // A hand-built plan (bypassing suggest, so the tiers are exact):
        // one Featured event (floor 6) and one Normal event (floor 2),
        // both unchosen, with a target that only fits one demotion step
        // (6 + 2 = 8 > 5, but 2 + 2 = 4 <= 5). Demoting Featured -> Normal
        // first satisfies the target after a single step and never touches
        // the Normal event. Demoting Normal -> Brief first does not (7 still
        // > 5), so it falls through to also demote the Featured event,
        // leaving the Normal event wrongly knocked down to Brief.
        let mut photos = event(0, 0, 10, 90);
        photos.extend(event(1, 1, 10, 80));
        let kept = keepers(&photos);
        let cap = capacity(1, 5);
        let plan = vec![
            EventPlan {
                event: 0,
                tier: Tier::Featured,
                suggested: Tier::Featured,
                chosen: false,
                reason: Reason::Standout,
                merit: 1.0,
                moments: 5,
                kept: 10,
                photos: 10,
            },
            EventPlan {
                event: 1,
                tier: Tier::Normal,
                suggested: Tier::Normal,
                chosen: false,
                reason: Reason::Ranked { rank: 1, of: 2 },
                merit: 0.5,
                moments: 5,
                kept: 10,
                photos: 10,
            },
        ];
        let b = budget(&kept, plan, &cap, &Overrides::new(), &BookOptions::default());
        let tier_of = |e: u32| b.plan.iter().find(|p| p.event == e).unwrap().tier;
        assert_eq!(tier_of(0), Tier::Normal, "the Featured event is the one demoted");
        assert_eq!(tier_of(1), Tier::Normal, "a Normal event must not be demoted while a Featured one still can be");
    }

    #[test]
    fn the_remainder_vote_uses_the_true_dhondt_divisor_quota_plus_one() {
        // Two hand-built Normal events (real avail 10 each, so avail never
        // constrains either), differing only in a *fake* `moments` field
        // (1 vs 2) chosen so that D'Hondt's genuine divisor sequence
        // (weight*sqrt(moments) / (quota+1)) and an off-by-one variant
        // (.../quota) provably disagree after exactly 2 remainder seats:
        // continuing the true divisor from floor 2 each, event 1 (sqrt 2)
        // outscores event 0 (sqrt 1) on both remaining seats (0.471 then
        // 0.354, against 0.333 flat), so it takes both: (2, 4). The
        // off-by-one divisor gives event 0 a first-seat score of 0.5 (vs
        // 0.707), event 1 wins seat one (-> 3), but then event 0's second
        // score rises to 0.5 while event 1's falls to 0.471, so event 0
        // takes the second seat instead: (3, 3). The two rules must not
        // agree here, or this mutation is not actually pinned by the test.
        let mut photos = event(0, 0, 10, 90);
        photos.extend(event(1, 1, 10, 80));
        let kept = keepers(&photos);
        let cap = capacity(1, 6); // 2 (floor) + 2 (floor) + 2 remainder seats
        let plan = vec![
            EventPlan {
                event: 0,
                tier: Tier::Normal,
                suggested: Tier::Normal,
                chosen: false,
                reason: Reason::Ranked { rank: 1, of: 2 },
                merit: 0.6,
                moments: 1,
                kept: 10,
                photos: 10,
            },
            EventPlan {
                event: 1,
                tier: Tier::Normal,
                suggested: Tier::Normal,
                chosen: false,
                reason: Reason::Ranked { rank: 2, of: 2 },
                merit: 0.5,
                moments: 2,
                kept: 10,
                photos: 10,
            },
        ];
        let b = budget(&kept, plan, &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!(b.selected.len(), 6);
        assert_eq!(placed(&b, &kept, 0), 2, "event 0's own divisor never overtakes event 1's");
        assert_eq!(placed(&b, &kept, 1), 4, "both remainder seats go to the higher-moments event");
    }

    #[test]
    fn r9a_no_choices_no_includes_never_exceeds_the_target() {
        // 30 events of 4 photos (2 moments) each, no user tiers and no
        // Includes. Suggested floors alone (Normal 2, Brief 1) sum well
        // past the 20-photo target, and none of them is `chosen`, so
        // without demoting unchosen Brief events all the way to Skipped,
        // `selected` overflows the target -- the bug this ruling fixes: 30
        // events x 4 photos at target 20 used to select 30.
        let mut photos = Vec::new();
        for e in 0..30u32 {
            photos.extend(event(e, e as i64, 4, 90 - e as u8));
        }
        let kept = keepers(&photos);
        let cap = capacity(9, 20);
        let b = budget(&kept, plan_of(&photos, &EventTiers::new(), &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert!(b.selected.len() <= 20, "got {}", b.selected.len());
        assert!(b.overflow.is_none());
        assert!(
            b.plan.iter().any(|p| p.tier == Tier::Skipped && p.reason == Reason::Demoted),
            "the lowest-merit events must be demoted all the way to Skipped: {:?}",
            b.plan.iter().map(|p| (p.event, p.tier, p.reason)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn r9b_zero_capacity_with_no_choices_selects_nothing() {
        let mut photos = Vec::new();
        for e in 0..5u32 {
            photos.extend(event(e, e as i64, 4, 90 - e as u8));
        }
        let kept = keepers(&photos);
        let cap = capacity(0, 0);
        let b = budget(&kept, plan_of(&photos, &EventTiers::new(), &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!(b.selected.len(), 0);
        assert!(b.overflow.is_none());
    }

    #[test]
    fn r9c_a_chosen_brief_on_every_event_is_never_demoted_and_can_overflow() {
        // Every event's tier is the user's own choice (Brief), so the new
        // Brief -> Skipped step must never touch them: `!p.chosen` excludes
        // every event from all three demotion steps, and the book reports
        // overflow instead of silently demoting the user's own floors.
        let mut photos = Vec::new();
        for e in 0..21u32 {
            photos.extend(event(e, e as i64, 4, 90 - e as u8));
        }
        let kept = keepers(&photos);
        let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Brief)).collect();
        let cap = capacity(9, 20); // 21 x floor 1 = 21 > 20
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &Overrides::new(), &BookOptions::default());
        assert!(b.plan.iter().all(|p| p.tier == Tier::Brief), "a chosen tier is never demoted");
        assert!(b.overflow.is_some());
    }

    #[test]
    fn floors_are_honored_even_when_plain_dhondt_never_catches_up() {
        // A hand-built plan: a Featured event with almost no moments (fake
        // `moments: 1`, so its D'Hondt vote is tiny) beside a Normal event
        // with an enormous fake `moments: 1000` vote. `quota` MUST start
        // from each event's already-computed floor, not from something
        // weaker (e.g. Includes alone): seeded from Includes only, the
        // Featured event would need to WIN a remainder seat by score to
        // ever reach its floor, and the huge competitor's score dominates
        // every one of the 10 remainder seats this fixture hands out, so a
        // wrongly-seeded quota would leave the Featured event under floor.
        let mut photos = event(0, 0, 20, 90); // Featured, avail 20
        photos.extend(event(1, 1, 50, 80)); // Normal, avail 50
        let kept = keepers(&photos);
        let cap = capacity(1, 18); // floor 6 + floor 2 + 10 remainder seats
        let plan = vec![
            EventPlan {
                event: 0,
                tier: Tier::Featured,
                suggested: Tier::Featured,
                chosen: false,
                reason: Reason::Standout,
                merit: 1.0,
                moments: 1,
                kept: 20,
                photos: 20,
            },
            EventPlan {
                event: 1,
                tier: Tier::Normal,
                suggested: Tier::Normal,
                chosen: false,
                reason: Reason::Ranked { rank: 1, of: 2 },
                merit: 0.9,
                moments: 1000,
                kept: 50,
                photos: 50,
            },
        ];
        let b = budget(&kept, plan, &cap, &Overrides::new(), &BookOptions::default());
        assert_eq!(b.floors[&0], 6);
        assert!(placed(&b, &kept, 0) >= b.floors[&0], "got {}", placed(&b, &kept, 0));
    }

    #[test]
    fn a_brief_events_includes_are_never_capped_below_brief_cap() {
        let photos = event(0, 0, 10, 60); // 5 moments, avail 10
        let kept = keepers(&photos);
        let mut overrides = Overrides::new();
        for p in &photos[..3] {
            overrides.set(p.hash.clone(), Override::Include);
        }
        let tiers: EventTiers = photos.iter().map(|p| (p.hash.clone(), Tier::Brief)).collect();
        let cap = capacity(9, 45);
        let o = BookOptions { brief_cap: 2, ..BookOptions::default() };
        let b = budget(&kept, plan_of(&photos, &tiers, &cap), &cap, &overrides, &o);
        assert_eq!(b.floors[&0], 3, "the floor must cover all 3 Includes, past brief_cap 2");
        for p in &photos[..3] {
            let idx = kept.iter().position(|k| k.hash == p.hash).unwrap();
            assert!(b.selected.contains(&idx), "every Include must be selected even past brief_cap");
        }
    }

    #[test]
    fn demotion_skips_a_candidate_whose_floor_would_not_shrink() {
        // 25 unchosen Brief events (floor 1 each = 25 > target 20). Events
        // 15..25, the 10 LOWEST-merit ones, each hold one Include, so
        // demoting them to Skipped frees nothing -- `floor_of` still maxes
        // with `included` at 1 either way. Events 10..15, the next 5
        // lowest-merit ones, hold no Include, so demoting them genuinely
        // frees a photo each -- exactly enough to reach the target. A
        // demotion candidate is only eligible when its floor at the new
        // tier is strictly less than its floor at the old one, so the fix
        // must skip straight past every Include-holding event and demote
        // exactly events 10..15, leaving both the Include-holders and the
        // higher-merit events 0..10 untouched.
        let mut photos = Vec::new();
        for e in 0..25u32 {
            photos.extend(event(e, e as i64, 4, 50));
        }
        let kept = keepers(&photos);
        let mut overrides = Overrides::new();
        for e in 15..25u32 {
            let hash = photos.iter().find(|p| p.event_cluster == e).unwrap().hash.clone();
            overrides.set(hash, Override::Include);
        }
        let cap = capacity(9, 20);
        let plan: Vec<EventPlan> = (0..25u32)
            .map(|e| EventPlan {
                event: e,
                tier: Tier::Brief,
                suggested: Tier::Brief,
                chosen: false,
                reason: Reason::OutOfRoom { rank: e as usize + 1, of: 25 },
                merit: 100.0 - e as f64, // event 0 highest merit, event 24 lowest
                moments: 2,
                kept: 4,
                photos: 4,
            })
            .collect();
        let b = budget(&kept, plan, &cap, &overrides, &BookOptions::default());
        let tier_of = |e: u32| b.plan.iter().find(|p| p.event == e).unwrap().tier;
        let reason_of = |e: u32| b.plan.iter().find(|p| p.event == e).unwrap().reason;

        for e in 10..15u32 {
            assert_eq!(tier_of(e), Tier::Skipped, "event {e} (no Include, lowest eligible merit) must be cut");
            assert_eq!(reason_of(e), Reason::Demoted);
        }
        for e in 15..25u32 {
            assert_eq!(tier_of(e), Tier::Brief, "event {e} holds an Include; demoting it frees nothing");
        }
        for e in 0..10u32 {
            assert_eq!(tier_of(e), Tier::Brief, "higher-merit events must not be touched");
        }
        assert_eq!(b.floors.values().sum::<usize>(), 20);
    }

    #[test]
    fn demotion_order_is_lowest_merit_first_ties_to_the_later_id() {
        // 5 unchosen Brief events, no Includes, floor 1 each = 5 > target
        // 3, so exactly 2 demotions are needed. Merits: 10, 20, 20, 30, 40
        // (events 0..5) -- event 0 is uniquely lowest and must go first;
        // events 1 and 2 tie at the next-lowest merit, and only ONE of
        // them is needed for the second demotion, so the tie-break ("ties
        // go to the later event id") is the sole thing deciding which:
        // event 2. A mutant that demoted highest-merit first would instead
        // cut events 3 and 4, and a mutant that broke the tie toward the
        // earlier id would cut event 1 instead of event 2 -- either is
        // caught by asserting the exact Skipped set.
        let mut photos = Vec::new();
        for e in 0..5u32 {
            photos.extend(event(e, e as i64, 4, 50));
        }
        let kept = keepers(&photos);
        let cap = capacity(9, 3);
        let merits = [10.0, 20.0, 20.0, 30.0, 40.0];
        let plan: Vec<EventPlan> = (0..5u32)
            .map(|e| EventPlan {
                event: e,
                tier: Tier::Brief,
                suggested: Tier::Brief,
                chosen: false,
                reason: Reason::OutOfRoom { rank: e as usize + 1, of: 5 },
                merit: merits[e as usize],
                moments: 2,
                kept: 4,
                photos: 4,
            })
            .collect();
        let b = budget(&kept, plan, &cap, &Overrides::new(), &BookOptions::default());
        let tier_of = |e: u32| b.plan.iter().find(|p| p.event == e).unwrap().tier;
        assert_eq!(tier_of(0), Tier::Skipped, "event 0 is uniquely lowest merit");
        assert_eq!(tier_of(2), Tier::Skipped, "of the merit-20 tie, the later id (2) is cut");
        assert_eq!(tier_of(1), Tier::Brief, "of the merit-20 tie, the earlier id (1) survives");
        assert_eq!(tier_of(3), Tier::Brief);
        assert_eq!(tier_of(4), Tier::Brief);
    }
}
