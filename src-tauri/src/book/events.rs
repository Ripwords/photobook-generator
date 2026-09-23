//! Event tiers: which events of a trip a book shows, and how many photos
//! each one gets. See docs/superpowers/specs/2026-09-23-event-tiers-design.md.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::book::pace::BookOptions;

pub const FEATURED_WEIGHT: f64 = 2.0;
pub const NORMAL_WEIGHT: f64 = 1.0;
pub const BRIEF_WEIGHT: f64 = 0.5;
pub const NORMAL_FLOOR: usize = 2;
const BRIEF_FLOOR: usize = 1;

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

#[cfg(test)]
mod tests {
    use super::*;

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
