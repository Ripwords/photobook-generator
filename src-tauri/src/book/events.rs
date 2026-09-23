//! Event tiers: which events of a trip a book shows, and how many photos
//! each one gets. See docs/superpowers/specs/2026-09-23-event-tiers-design.md.

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
}
