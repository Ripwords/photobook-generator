/// Converts raw scores to 0-100 percentile ranks within this population.
/// Equal values receive equal ranks. Order matches the input, not sorted order.
///
/// NaN inputs receive a percentile of 0 (an absence of rank, not a rank of
/// their own) and do not perturb the percentiles of the other values in the
/// call — those are computed as if the NaN entries were not present at all.
pub fn percentiles(values: &[f64]) -> Vec<u8> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }

    // Only non-NaN values participate in the sort and in the population
    // count. NaN compares unordered against everything, so including it
    // would make the sort order incoherent and corrupt `partition_point`'s
    // binary search for every other value, not just the NaN entry.
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n_valid = sorted.len();

    values
        .iter()
        .map(|v| {
            if v.is_nan() || n_valid == 0 {
                return 0;
            }
            // Count of values <= v, so the maximum always lands on 100.
            let count = sorted.partition_point(|s| s <= v);
            ((count as f64 / n_valid as f64) * 100.0).round() as u8
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

    /// NaN entries themselves receive percentile 0 — an absence of rank,
    /// not a rank of their own.
    #[test]
    fn nan_entries_receive_zero() {
        let p = percentiles(&[5.0, f64::NAN, 1.0]);
        assert_eq!(p[1], 0);

        let all_nan = percentiles(&[f64::NAN, f64::NAN]);
        assert_eq!(all_nan, vec![0, 0]);
    }

    /// The property that actually matters: a NaN mixed into the input must
    /// not perturb the percentiles of the *other* values. Each non-NaN value
    /// must get exactly the percentile it would have received had the NaN
    /// simply been absent from the call. A test asserting only "NaN gets 0"
    /// would pass even if this property were still broken.
    #[test]
    fn nan_does_not_perturb_the_percentiles_of_other_values() {
        let with_nan = percentiles(&[5.0, f64::NAN, 1.0, 3.0]);
        let without_nan = percentiles(&[5.0, 1.0, 3.0]);

        assert_eq!(with_nan[0], without_nan[0], "5.0 (max)");
        assert_eq!(with_nan[1], 0, "the NaN entry itself");
        assert_eq!(with_nan[2], without_nan[1], "1.0 (min)");
        assert_eq!(with_nan[3], without_nan[2], "3.0 (middle)");
    }
}
