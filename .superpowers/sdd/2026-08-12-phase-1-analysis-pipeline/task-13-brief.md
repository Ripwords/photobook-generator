### Task 13: Within-book percentile ranking

**Files:**
- Create: `src-tauri/src/ranking.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `ranking.rs`

**Interfaces:**
- Consumes: raw scores from feature records
- Produces: `pub fn percentiles(values: &[f64]) -> Vec<u8>` — each value's rank as 0–100 within this book's own population. Ties receive the same percentile. `None` values are handled by the caller filtering first.

Raw aesthetic and sharpness scores are not comparable across books or subject matter. Percentile-within-book converts a shaky absolute signal into a defensible relative one.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/ranking.rs`:

```rust
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
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `file not found for module 'ranking'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/ranking.rs` (above the test module):

```rust
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
```

Add `pub mod ranking;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 6 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(ranking): add within-book percentile ranking"
```

---

