/// Number of differing bits between two 64-bit perceptual hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Single-link agglomerative clustering over Hamming distance, via union-find.
/// Returns a cluster id for each input index. O(n^2), which is fine at n <= 300.
pub fn near_duplicate_clusters(phashes: &[u64], max_distance: u32) -> Vec<usize> {
    let n = phashes.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut Vec<usize>, mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if hamming(phashes[i], phashes[j]) <= max_distance {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    let mut labels = std::collections::HashMap::new();
    (0..n)
        .map(|i| {
            let root = find(&mut parent, i);
            let next = labels.len();
            *labels.entry(root).or_insert(next)
        })
        .collect()
}

/// Splits a chronological sequence wherever the gap between consecutive
/// timestamps exceeds `gap_seconds`. Photos with no timestamp are grouped
/// together into one trailing cluster. Ids are returned in input order.
pub fn event_clusters(timestamps: &[Option<i64>], gap_seconds: i64) -> Vec<usize> {
    if timestamps.is_empty() {
        return Vec::new();
    }

    let mut dated: Vec<(usize, i64)> = timestamps
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.map(|t| (i, t)))
        .collect();
    dated.sort_by_key(|(_, t)| *t);

    let mut ids = vec![usize::MAX; timestamps.len()];
    let mut current = 0usize;
    let mut previous: Option<i64> = None;
    let mut any_dated = false;

    for (index, time) in dated {
        any_dated = true;
        if let Some(prev) = previous {
            if time - prev > gap_seconds {
                current += 1;
            }
        }
        ids[index] = current;
        previous = Some(time);
    }

    // Only reserve a distinct id for the undated group when dated groups
    // actually consumed ids 0..=current. With no dated entries at all,
    // `current` never advanced past its initial 0, so the undated group
    // takes id 0 rather than the unused id 1.
    let undated = if any_dated { current + 1 } else { 0 };
    for id in ids.iter_mut() {
        if *id == usize::MAX {
            *id = undated;
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamming_counts_differing_bits() {
        assert_eq!(hamming(0b1010, 0b1010), 0);
        assert_eq!(hamming(0b1010, 0b1011), 1);
        assert_eq!(hamming(0b0000, 0b1111), 4);
    }

    #[test]
    fn identical_hashes_land_in_one_cluster() {
        let ids = near_duplicate_clusters(&[42, 42, 42], 4);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[1], ids[2]);
    }

    #[test]
    fn distant_hashes_land_in_separate_clusters() {
        let ids = near_duplicate_clusters(&[0x0000_0000_0000_0000, 0xFFFF_FFFF_FFFF_FFFF], 4);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn clustering_is_transitive_within_threshold() {
        // a-b differ by 1 bit, b-c by 1 bit, a-c by 2. All one burst.
        let ids = near_duplicate_clusters(&[0b000, 0b001, 0b011], 1);
        assert_eq!(ids[0], ids[2]);
    }

    #[test]
    fn event_clusters_returns_ids_in_input_order_not_sorted_order() {
        // Input is deliberately NOT chronologically ordered: index 0 holds the
        // latest timestamp, index 3 the second-latest, etc. If the function
        // sorted internally and returned ids in that sorted order instead of
        // remapping back to input positions, ids[0] (last chronologically,
        // event B) would land wherever the earliest timestamp landed instead.
        let day = 86_400;
        let ids = event_clusters(
            &[
                Some(3 * day + 60), // index 0: event B (late)
                Some(0),            // index 1: event A (early)
                Some(60),           // index 2: event A (early)
                Some(3 * day),      // index 3: event B (late)
            ],
            day,
        );
        assert_eq!(ids[0], ids[3]); // both event B
        assert_eq!(ids[1], ids[2]); // both event A
        assert_ne!(ids[0], ids[1]); // different events
    }

    #[test]
    fn events_split_on_a_large_time_gap() {
        let day = 86_400;
        let ids = event_clusters(&[Some(0), Some(60), Some(3 * day), Some(3 * day + 60)], day);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[2], ids[3]);
        assert_ne!(ids[1], ids[2]);
    }

    #[test]
    fn photos_without_timestamps_get_their_own_cluster() {
        let ids = event_clusters(&[Some(0), None, Some(60)], 86_400);
        assert_eq!(ids[0], ids[2]);
        assert_ne!(ids[1], ids[0]);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(near_duplicate_clusters(&[], 4).is_empty());
        assert!(event_clusters(&[], 86_400).is_empty());
    }

    #[test]
    fn all_nil_timestamps_still_get_dense_ids_starting_at_zero() {
        // With no dated entries at all, `current` never advances, so the
        // undated group must take id 0 rather than a phantom id 1 with an
        // unused id 0 sitting below it.
        let ids = event_clusters(&[None, None, None], 86_400);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[1], ids[2]);
        assert_eq!(ids[0], 0); // not 1 — asserting equality alone would pass under the bug
    }

    #[test]
    fn single_dated_element_yields_one_id() {
        let ids = event_clusters(&[Some(1_000)], 86_400);
        assert_eq!(ids, vec![0]);
    }

    #[test]
    fn single_undated_element_yields_one_id() {
        let ids = event_clusters(&[None], 86_400);
        assert_eq!(ids, vec![0]);
    }

    #[test]
    fn tied_timestamps_do_not_split() {
        // Two photos at the identical instant must land in the same cluster.
        // Note: a gap of 0 is nowhere near `gap_seconds`, so this does not
        // exercise the strict `>` vs `>=` boundary — see
        // `gap_exactly_at_threshold_does_not_split` for that.
        let ids = event_clusters(&[Some(5), Some(5)], 86_400);
        assert_eq!(ids[0], ids[1]);
    }

    #[test]
    fn gap_exactly_at_threshold_does_not_split() {
        // The split condition is `time - prev > gap_seconds`. A gap exactly
        // equal to the threshold must NOT split (false under `>`), which
        // distinguishes the strict `>` from an off-by-one `>=` regression
        // (true under `>=`, which would incorrectly split this pair).
        let ids = event_clusters(&[Some(0), Some(86_400)], 86_400);
        assert_eq!(ids[0], ids[1]);
    }
}
