### Task 14: Sidecar resilience — batching, timeout, respawn

**Files:**
- Modify: `src-tauri/src/sidecar.rs`
- Test: inline `#[cfg(test)]` module in `sidecar.rs`

**Interfaces:**
- Consumes: `Sidecar` from Task 3, `PhotoRecord` JSON from Task 9
- Produces:
  - `pub fn chunk_paths(paths: &[String], batch_size: usize) -> Vec<Vec<String>>`
  - `pub fn timeout_for(batch_len: usize) -> Duration` — `10s + 3s per photo`, generous enough for RAW
  - `Sidecar::analyze(&mut self, paths: Vec<String>) -> Result<Vec<serde_json::Value>, SidecarError>`
  - `SidecarPool::analyze_all(&mut self, app: &AppHandle, paths: &[String]) -> Vec<serde_json::Value>` — respawns on crash and emits synthetic `failed` records for a batch that dies twice, so the caller always receives one record per input path

RAW decode throughput has never been measured (spec §14). The timeout is deliberately generous, and a batch that exceeds it degrades to failure records rather than hanging the import.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/sidecar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("/photos/{i}.jpg")).collect()
    }

    #[test]
    fn chunks_paths_into_batches() {
        let batches = chunk_paths(&paths(25), 10);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 10);
        assert_eq!(batches[2].len(), 5);
    }

    #[test]
    fn chunking_preserves_every_path_in_order() {
        let all = paths(7);
        let flat: Vec<String> = chunk_paths(&all, 3).into_iter().flatten().collect();
        assert_eq!(flat, all);
    }

    #[test]
    fn empty_input_produces_no_batches() {
        assert!(chunk_paths(&[], 10).is_empty());
    }

    #[test]
    fn timeout_scales_with_batch_size() {
        assert!(timeout_for(20) > timeout_for(1));
    }

    #[test]
    fn timeout_has_a_floor_for_a_single_photo() {
        assert!(timeout_for(1) >= Duration::from_secs(10));
    }

    #[test]
    fn synthetic_failure_record_names_the_path() {
        let value = failure_record("/photos/bad.raw", "timed out");
        assert_eq!(value["status"], "failed");
        assert_eq!(value["path"], "/photos/bad.raw");
        assert_eq!(value["message"], "timed out");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `cannot find function 'chunk_paths' in this scope`

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/sidecar.rs` (above the test module):

```rust
/// Batches amortise sidecar round-trips while keeping any single timeout small
/// enough that one bad photo doesn't stall the whole import.
pub const BATCH_SIZE: usize = 16;

pub fn chunk_paths(paths: &[String], batch_size: usize) -> Vec<Vec<String>> {
    paths.chunks(batch_size.max(1)).map(<[String]>::to_vec).collect()
}

/// RAW decode is materially slower than JPEG and has never been benchmarked,
/// so this is deliberately generous.
pub fn timeout_for(batch_len: usize) -> Duration {
    Duration::from_secs(10) + Duration::from_secs(3) * batch_len as u32
}

pub fn failure_record(path: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "status": "failed", "path": path, "message": message })
}

impl Sidecar {
    pub fn analyze(&mut self, paths: Vec<String>) -> Result<Vec<serde_json::Value>, SidecarError> {
        let timeout = timeout_for(paths.len());
        match self.request(RequestKind::Analyze, Some(paths), timeout)? {
            ResponseResult::Analyzed(records) => Ok(records),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Pong { .. } => Err(SidecarError::Malformed("expected analyzed".into())),
        }
    }
}

pub struct SidecarPool {
    inner: Option<Sidecar>,
}

impl SidecarPool {
    pub fn new() -> Self {
        Self { inner: None }
    }

    fn ensure(&mut self, app: &AppHandle) -> Result<&mut Sidecar, SidecarError> {
        if self.inner.is_none() {
            self.inner = Some(Sidecar::spawn(app)?);
        }
        Ok(self.inner.as_mut().expect("just spawned"))
    }

    /// Always returns one record per input path. A batch that fails twice is
    /// converted to synthetic failure records so the caller can proceed.
    pub fn analyze_all(&mut self, app: &AppHandle, paths: &[String]) -> Vec<serde_json::Value> {
        let mut out = Vec::with_capacity(paths.len());

        for batch in chunk_paths(paths, BATCH_SIZE) {
            let mut records = None;

            for attempt in 0..2 {
                let result = self
                    .ensure(app)
                    .and_then(|sidecar| sidecar.analyze(batch.clone()));

                match result {
                    Ok(r) => {
                        records = Some(r);
                        break;
                    }
                    Err(err) => {
                        log::warn!("sidecar batch failed (attempt {attempt}): {err}");
                        // Drop the child so the next ensure() respawns it.
                        self.inner = None;
                    }
                }
            }

            match records {
                Some(r) if r.len() == batch.len() => out.extend(r),
                Some(r) => {
                    log::warn!("sidecar returned {} records for {} paths", r.len(), batch.len());
                    out.extend(batch.iter().map(|p| failure_record(p, "record count mismatch")));
                }
                None => out.extend(batch.iter().map(|p| failure_record(p, "sidecar failed twice"))),
            }
        }

        out
    }
}

impl Default for SidecarPool {
    fn default() -> Self {
        Self::new()
    }
}
```

Add the `Analyzed` variant to `src-tauri/src/protocol.rs`'s `ResponseResult`, as a **tuple variant** — Swift encodes `{"type":"analyzed","data":[...]}`, and with `#[serde(tag = "type", content = "data")]` the tuple form maps to that array exactly:

```rust
    Analyzed(Vec<serde_json::Value>),
```

Records stay as `serde_json::Value` rather than a typed struct: Rust only reads five fields (`status`, `hash`, `path`, `phash`, `aestheticScore`, `sharpness`), and keeping the rest opaque means adding a field in Swift needs no Rust change.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 6 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(sidecar): add batching, scaled timeouts, and crash respawn"
```

---

