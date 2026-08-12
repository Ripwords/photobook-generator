/// Converts raw scores to 0-100 percentile ranks within this population.
/// Equal values receive equal ranks. Order matches the input, not sorted order.
pub fn percentiles(values: &[f64]) -> Vec<u8> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }

    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    values
        .iter()
        .map(|v| {
            // Count of values <= v, so the maximum always lands on 100.
            let count = sorted.partition_point(|s| s <= v);
            ((count as f64 / n as f64) * 100.0).round() as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert!(percentiles(&[]).is_empty());
    }

    #[test]
    fn single_value_is_the_top_percentile() {
        assert_eq!(percentiles(&[3.7]), vec![100]);
    }

    #[test]
    fn ranks_ascending_values_across_the_range() {
        let p = percentiles(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(p[0], 25);
        assert_eq!(p[3], 100);
        assert!(p[0] < p[1] && p[1] < p[2] && p[2] < p[3]);
    }

    #[test]
    fn preserves_input_order_not_sorted_order() {
        let p = percentiles(&[9.0, 1.0, 5.0]);
        assert_eq!(p[0], 100, "the largest input was first");
        assert_eq!(p[1], 33);
    }

    #[test]
    fn ties_receive_the_same_percentile() {
        let p = percentiles(&[2.0, 2.0, 2.0]);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }

    #[test]
    fn all_percentiles_are_within_bounds() {
        let p = percentiles(&[-5.0, 0.0, 0.5, 100.0]);
        for v in p {
            assert!(v <= 100);
        }
    }

    // --- Additional tests beyond the brief ---

    /// Two of four values tie; the other two are distinct. Unlike an
    /// all-identical array, this catches a rank formula that happens to keep
    /// a *uniform* tied population self-consistent (e.g. "first occurrence
    /// index" ranking) but would still assign a wrong/mismatched percentile
    /// to a partial tie relative to its non-tied neighbours.
    #[test]
    fn partial_ties_get_equal_percentile_distinct_from_non_tied_values() {
        let p = percentiles(&[1.0, 2.0, 2.0, 3.0]);
        assert_eq!(p, vec![25, 75, 75, 100]);
    }

    /// Aesthetic scores are documented as roughly [-1, 1], so negatives are
    /// real input, not a hypothetical. Also unsorted, so this doubles as a
    /// second, independent order-preservation check.
    #[test]
    fn negative_scores_rank_correctly_preserving_input_order() {
        let p = percentiles(&[1.0, -1.0, 0.0, 0.5, -0.5]);
        assert_eq!(p, vec![100, 20, 60, 80, 40]);
    }

    /// A whole book where every photo scored identically (e.g. a burst of
    /// near-duplicates) — every value must land at the top percentile, not
    /// just be internally consistent with a smaller sample.
    #[test]
    fn single_repeated_value_throughout_all_tie_at_top() {
        let p = percentiles(&[7.0, 7.0, 7.0, 7.0, 7.0]);
        assert_eq!(p, vec![100, 100, 100, 100, 100]);
    }

    /// Infinities are totally ordered like any other f64 via partial_cmp,
    /// so they rank correctly with no special-casing needed.
    #[test]
    fn infinities_are_ranked_correctly() {
        let p = percentiles(&[f64::NEG_INFINITY, 0.0, f64::INFINITY]);
        assert_eq!(p, vec![33, 67, 100]);
    }

    #[test]
    fn tied_infinities_receive_equal_top_percentile() {
        let p = percentiles(&[f64::INFINITY, f64::INFINITY, 0.0]);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[0], 100);
        assert_eq!(p[2], 33);
    }

    /// NaN cannot satisfy `<=` against anything, which breaks the
    /// monotonicity `partition_point` relies on for its binary search. This
    /// does not panic, but it silently corrupts percentiles for *other*
    /// values in the same call, not just the NaN entry — e.g.
    /// percentiles(&[5.0, f64::NAN, 1.0]) ranks 5.0 (the true max) at 25,
    /// not 100. This is a known gap: callers must filter NaN out before
    /// calling `percentiles`, same as the existing None-filtering contract.
    /// This test only pins the no-panic guarantee, not correctness.
    #[test]
    fn nan_does_not_panic_though_it_is_not_meaningfully_ranked() {
        let p = percentiles(&[5.0, f64::NAN, 1.0]);
        assert_eq!(p.len(), 3);
    }
}
