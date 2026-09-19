/// Number of differing bits between two 64-bit perceptual hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Euclidean distance between two Vision feature prints, the same value as
/// Apple's `VNFeaturePrintObservation.computeDistance` for revision 2 (checked
/// on real photos). `None` when the lengths differ: such prints come from
/// different models and are not comparable.
pub fn feature_distance(a: &[f32], b: &[f32]) -> Option<f32> {
    (a.len() == b.len())
        .then(|| a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt())
}

/// A pHash match: at most this many of the 64 bits differ.
pub const PHASH_MAX_HAMMING: u32 = 4;

/// Feature-print distance at or below which two photos are the same shot
/// taken twice. Calibrated on Bali 2025 (283 photos) and a 300-photo slice of
/// Iceland 2025: every sampled pair up to 0.35 was the same subject, spot and
/// framing; from 0.35 to 0.45 about half were a different pose, framing or
/// orientation. See PROJECT-STATUS.md, "similarity calibration".
pub const SIMILAR_DISTANCE: f32 = 0.35;

/// A pHash match is refused when both photos have prints further apart than
/// this. pHash collapses flat grey skies and mist into near-identical hashes:
/// on Iceland it joined a statue, a waterfall and a sea view shot 28 h apart.
/// Every sampled pHash pair past 0.6 was two different pictures.
pub const PHASH_VETO_DISTANCE: f32 = 0.5;

/// No similarity cluster spans more than this, first dated photo to last.
/// Real look-alike pairs were never more than 110 s apart; a time-lapse is
/// all look-alikes and would otherwise collapse into one photo.
pub const SIMILAR_SPAN_SECONDS: i64 = 120;

/// Groups photos that are the same shot taken more than once.
///
/// Two photos are *linked* when their feature prints are within
/// `max_distance`, or when their pHashes match and their prints (if both
/// exist) are within `PHASH_VETO_DISTANCE`. A photo with no print links by
/// pHash alone.
///
/// Clusters grow by complete linkage, not single linkage: two clusters merge
/// only when every photo in one is linked to every photo in the other, and
/// the merged cluster spans at most `max_span_seconds` of dated photos.
/// Single linkage chains A~B~C into one cluster even when A and C are
/// different pictures, and on real bursts that walked a whole scene into
/// one keeper. Undated photos do not constrain the span.
///
/// Returns a cluster id per input index, dense and in first-seen order.
pub fn similar_clusters(
    phashes: &[u64],
    prints: &[Option<Vec<f32>>],
    times: &[Option<i64>],
    max_distance: f32,
    max_span_seconds: i64,
) -> Vec<usize> {
    let n = phashes.len();
    let distance = |i: usize, j: usize| match (&prints[i], &prints[j]) {
        (Some(a), Some(b)) => feature_distance(a, b),
        _ => None,
    };

    let mut links: Vec<(bool, f32, usize, usize)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            // Complete linkage then keeps every cluster within the span.
            if let (Some(a), Some(b)) = (times[i], times[j]) {
                if (a - b).abs() > max_span_seconds {
                    continue;
                }
            }
            let phash_match = hamming(phashes[i], phashes[j]) <= PHASH_MAX_HAMMING;
            match distance(i, j) {
                Some(d) if d <= max_distance || (phash_match && d <= PHASH_VETO_DISTANCE) => {
                    links.push((false, d, i, j))
                }
                None if phash_match => links.push((true, 0.0, i, j)),
                _ => {}
            }
        }
    }
    links.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then((a.2, a.3).cmp(&(b.2, b.3))));
    let linked: std::collections::HashSet<(usize, usize)> =
        links.iter().map(|&(_, _, i, j)| (i, j)).collect();
    let is_linked = |a: usize, b: usize| linked.contains(&(a.min(b), a.max(b)));

    let mut cluster_of: Vec<usize> = (0..n).collect();
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    for &(_, _, i, j) in &links {
        let (a, b) = (cluster_of[i], cluster_of[j]);
        if a == b {
            continue;
        }
        if !members[a].iter().all(|&x| members[b].iter().all(|&y| is_linked(x, y))) {
            continue;
        }
        let moved = std::mem::take(&mut members[b]);
        for &k in &moved {
            cluster_of[k] = a;
        }
        members[a].extend(moved);
    }

    let mut dense = std::collections::HashMap::new();
    cluster_of
        .iter()
        .map(|&c| {
            let next = dense.len();
            *dense.entry(c).or_insert(next)
        })
        .collect()
}

/// The gap that starts a new event: `finalize_photos` clusters a folder with
/// it, and the agent view re-clusters a book's photos with the same value.
pub const EVENT_GAP_SECONDS: i64 = 4 * 3600;

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
    #[test]
    fn feature_distance_is_euclidean() {
        assert_eq!(feature_distance(&[0.0, 0.0], &[3.0, 4.0]), Some(5.0));
        assert_eq!(feature_distance(&[1.0, -2.0], &[1.0, -2.0]), Some(0.0));
    }

    #[test]
    fn feature_distance_refuses_prints_of_different_lengths() {
        assert_eq!(feature_distance(&[0.0, 0.0], &[3.0, 4.0, 0.0]), None);
    }

    use super::*;

    #[test]
    fn hamming_counts_differing_bits() {
        assert_eq!(hamming(0b1010, 0b1010), 0);
        assert_eq!(hamming(0b1010, 0b1011), 1);
        assert_eq!(hamming(0b0000, 0b1111), 4);
    }

    fn print(x: f32) -> Option<Vec<f32>> {
        Some(vec![x, 0.0])
    }

    /// Distinct hashes (64 bits apart pairwise are not possible, but these
    /// are all > PHASH_MAX_HAMMING apart), so only prints can link them.
    const UNRELATED: [u64; 4] = [0, 0xFFFF, 0xFFFF_0000, 0xFFFF_0000_0000];

    fn clusters(prints: &[Option<Vec<f32>>], times: &[Option<i64>]) -> Vec<usize> {
        similar_clusters(&UNRELATED[..prints.len()], prints, times, 0.3, 120)
    }

    #[test]
    fn similar_prints_join_and_distant_ones_do_not() {
        let ids = clusters(&[print(0.0), print(0.2), print(0.9)], &[Some(0), Some(1), Some(2)]);
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
    }

    /// A~B and B~C are both within 0.3, A~C is 0.5. Single linkage would put
    /// all three in one cluster; complete linkage must not.
    #[test]
    fn similar_clusters_do_not_chain_through_a_middle_photo() {
        let ids = clusters(&[print(0.0), print(0.25), print(0.5)], &[Some(0), Some(1), Some(2)]);
        assert_eq!(ids[0], ids[1], "the first link merges");
        assert_ne!(ids[0], ids[2], "A and C are different pictures");
    }

    /// Every pair is within the threshold, but the three together span 200 s.
    #[test]
    fn similar_clusters_refuse_to_span_more_than_the_cap() {
        let ids = clusters(&[print(0.0), print(0.0), print(0.0)], &[Some(0), Some(100), Some(200)]);
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
        let apart = clusters(&[print(0.0), print(0.0)], &[Some(0), Some(121)]);
        assert_ne!(apart[0], apart[1], "one pair past the cap stays apart");
    }

    #[test]
    fn undated_photos_join_on_their_prints_alone() {
        let ids = clusters(&[print(0.0), print(0.1), print(0.0)], &[None, None, Some(5_000)]);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[0], ids[2]);
    }

    #[test]
    fn photos_without_prints_join_only_by_phash() {
        let ids = similar_clusters(
            &[7, 7, 7, 0xFFFF_FFFF],
            &[None, None, print(0.0), None],
            &[Some(0), Some(1), Some(2), Some(3)],
            0.3,
            120,
        );
        assert_eq!(ids[0], ids[1], "two print-less photos with one pHash");
        assert_eq!(ids[0], ids[2], "a print-less photo and a printed one with one pHash");
        assert_ne!(ids[0], ids[3], "a print-less photo with a different pHash");
    }

    /// Identical hashes, prints 0.4 apart (past the 0.3 threshold but within
    /// the veto) join; prints 0.6 apart are two different pictures that pHash
    /// happens to confuse, and stay apart.
    #[test]
    fn a_phash_match_is_vetoed_by_prints_that_disagree() {
        let times = [Some(0), Some(1)];
        let near = similar_clusters(&[7, 7], &[print(0.0), print(0.4)], &times, 0.3, 120);
        assert_eq!(near[0], near[1]);
        let far = similar_clusters(&[7, 7], &[print(0.0), print(0.6)], &times, 0.3, 120);
        assert_ne!(far[0], far[1]);
    }

    #[test]
    fn similar_cluster_ids_are_dense_in_first_seen_order() {
        let ids = clusters(&[print(0.9), print(0.9), print(0.0), print(0.1)], &[Some(0); 4]);
        assert_eq!(ids, vec![0, 0, 1, 1]);
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
        assert!(similar_clusters(&[], &[], &[], 0.3, 120).is_empty());
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
