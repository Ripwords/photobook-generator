### Task 12: Near-duplicate and event clustering

**Files:**
- Create: `src-tauri/src/cluster.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `cluster.rs`

**Interfaces:**
- Consumes: `phash: u64` and `capture_date` from the feature records
- Produces:
  - `pub fn hamming(a: u64, b: u64) -> u32`
  - `pub fn near_duplicate_clusters(phashes: &[u64], max_distance: u32) -> Vec<usize>` — returns a cluster id per input index
  - `pub fn event_clusters(timestamps: &[Option<i64>], gap_seconds: i64) -> Vec<usize>` — photos with no timestamp form their own trailing cluster

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/cluster.rs`:

```rust
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
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `file not found for module 'cluster'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/cluster.rs` (above the test module):

```rust
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
/// together into one trailing cluster.
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

    for (index, time) in dated {
        if let Some(prev) = previous {
            if time - prev > gap_seconds {
                current += 1;
            }
        }
        ids[index] = current;
        previous = Some(time);
    }

    let undated = current + 1;
    for id in ids.iter_mut() {
        if *id == usize::MAX {
            *id = undated;
        }
    }
    ids
}
```

Add `pub mod cluster;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 7 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(cluster): add near-duplicate and event clustering"
```

---

