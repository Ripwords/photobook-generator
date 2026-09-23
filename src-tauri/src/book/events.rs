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
}
