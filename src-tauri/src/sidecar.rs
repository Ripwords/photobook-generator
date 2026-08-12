use crate::protocol::{Request, RequestKind, Response, ResponseResult};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("failed to spawn sidecar: {0}")]
    Spawn(String),
    #[error("sidecar timed out after {0:?}")]
    Timeout(Duration),
    #[error("sidecar closed its output stream")]
    Closed,
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("engine error: {0}")]
    Engine(String),
}

pub struct Sidecar {
    // `Option` so `Drop` can take ownership and call `CommandChild::kill(self)`,
    // which consumes by value; `Drop::drop` only ever gets `&mut self`.
    child: Option<CommandChild>,
    lines: Receiver<String>,
    counter: u64,
}

impl Sidecar {
    pub fn spawn(app: &AppHandle) -> Result<Self, SidecarError> {
        let (mut rx, child) = app
            .shell()
            .sidecar("photobook-engine")
            .map_err(|e| SidecarError::Spawn(e.to_string()))?
            .spawn()
            .map_err(|e| SidecarError::Spawn(e.to_string()))?;

        // The plugin's event channel has capacity 1; a dedicated drain thread
        // prevents the reader thread from blocking.
        let (tx, lines) = mpsc::channel();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let CommandEvent::Stdout(bytes) = event {
                    let text = String::from_utf8_lossy(&bytes).trim().to_string();
                    if !text.is_empty() && tx.send(text).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self { child: Some(child), lines, counter: 0 })
    }

    fn next_id(&mut self) -> String {
        self.counter += 1;
        self.counter.to_string()
    }

    pub fn request(&mut self, kind: RequestKind, paths: Option<Vec<String>>, timeout: Duration)
        -> Result<ResponseResult, SidecarError>
    {
        let id = self.next_id();
        let req = Request { id: id.clone(), kind, paths };
        let mut line = serde_json::to_string(&req).map_err(|e| SidecarError::Malformed(e.to_string()))?;
        line.push('\n');
        self.child
            .as_mut()
            .expect("Sidecar's child is only taken by Drop")
            .write(line.as_bytes())
            .map_err(|e| SidecarError::Spawn(e.to_string()))?;

        loop {
            let text = self.lines.recv_timeout(timeout).map_err(|_| SidecarError::Timeout(timeout))?;
            let res: Response = serde_json::from_str(&text)
                .map_err(|e| SidecarError::Malformed(format!("{e}: {text}")))?;
            if res.id == id {
                return Ok(res.result);
            }
            // Ignore stale responses from a previous, timed-out request.
        }
    }

    pub fn ping(&mut self) -> Result<String, SidecarError> {
        match self.request(RequestKind::Ping, None, Duration::from_secs(5))? {
            ResponseResult::Pong { version } => Ok(version),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Analyzed(_) => Err(SidecarError::Malformed("expected pong".into())),
        }
    }

    pub fn analyze(&mut self, paths: Vec<String>) -> Result<Vec<serde_json::Value>, SidecarError> {
        let timeout = timeout_for(paths.len());
        match self.request(RequestKind::Analyze, Some(paths), timeout)? {
            ResponseResult::Analyzed(records) => Ok(records),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Pong { .. } => Err(SidecarError::Malformed("expected analyzed".into())),
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        // Best-effort: if the child already exited or the kill signal fails,
        // there's nothing more we can do from a destructor.
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
    }
}

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

/// The pure retry-and-backfill core of `SidecarPool::analyze_all`, factored
/// out so it can be tested with a fake `call` instead of a real `AppHandle`.
///
/// Chunks `paths`, invokes `call` up to twice per batch, and guarantees the
/// output has exactly one record per input path in input order: a batch that
/// errors twice, or that returns the wrong number of records even on
/// success, is entirely replaced with synthetic `failed` records rather than
/// partially trusted. Task 15 zips this output positionally against `paths`,
/// so a short or misordered result would silently attribute one photo's
/// analysis to another.
pub(crate) fn analyze_batches<F>(
    paths: &[String],
    batch_size: usize,
    mut call: F,
) -> Vec<serde_json::Value>
where
    F: FnMut(&[String]) -> Result<Vec<serde_json::Value>, SidecarError>,
{
    let mut out = Vec::with_capacity(paths.len());

    for batch in chunk_paths(paths, batch_size) {
        let mut records = None;

        for attempt in 0..2 {
            match call(&batch) {
                Ok(r) => {
                    records = Some(r);
                    break;
                }
                Err(err) => {
                    log::warn!("sidecar batch failed (attempt {attempt}): {err}");
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
    ///
    /// Thin wrapper around `analyze_batches`: owns the one side effect that
    /// needs a real `AppHandle` (spawning/respawning the child), which is why
    /// this method itself is not unit tested — see task-14-report.md.
    pub fn analyze_all(&mut self, app: &AppHandle, paths: &[String]) -> Vec<serde_json::Value> {
        analyze_batches(paths, BATCH_SIZE, |batch| {
            let result = self.ensure(app).and_then(|sidecar| sidecar.analyze(batch.to_vec()));
            if result.is_err() {
                // Drop the child so the next ensure() respawns it.
                self.inner = None;
            }
            result
        })
    }
}

impl Default for SidecarPool {
    fn default() -> Self {
        Self::new()
    }
}

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

    // --- Additional tests beyond the brief: see task-14-report.md "Mutation
    // evidence" for what the brief's own tests above would still pass under.

    /// `timeout_scales_with_batch_size` only checks `>`, and the floor test
    /// only checks `>=` at n=1, so a mutant that scales by e.g. 1ms/photo
    /// instead of 3s/photo, or that adds a different flat floor, would pass
    /// both. Pin the exact "10s + 3s per photo" formula from the brief.
    #[test]
    fn timeout_matches_exact_formula() {
        assert_eq!(timeout_for(0), Duration::from_secs(10));
        assert_eq!(timeout_for(1), Duration::from_secs(13));
        assert_eq!(timeout_for(16), Duration::from_secs(10 + 3 * 16));
    }

    /// None of the brief's tests call `chunk_paths` with `batch_size: 0`, so
    /// the `.max(1)` guard in the reference implementation (which prevents a
    /// panic from `[T]::chunks(0)`) is exercised by nothing. Deleting that
    /// guard would still pass every test above.
    #[test]
    fn zero_batch_size_does_not_panic() {
        let batches = chunk_paths(&paths(3), 0);
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| b.len() == 1));
    }

    // --- analyze_batches: the one-record-per-path guarantee, tested with
    // fake `call` closures instead of a real AppHandle/Sidecar. Records are
    // tagged with their own path so offsets, not just counts, can be
    // checked.

    fn ok_record(path: &str) -> serde_json::Value {
        serde_json::json!({ "path": path, "status": "ok" })
    }

    #[test]
    fn analyze_batches_passes_through_records_unchanged_on_success() {
        let ps = paths(5);
        let out = analyze_batches(&ps, 2, |batch| {
            Ok(batch.iter().map(|p| ok_record(p)).collect())
        });

        assert_eq!(out.len(), ps.len());
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i], ok_record(p), "record at offset {i} does not match input path {p}");
        }
    }

    #[test]
    fn analyze_batches_retries_once_then_succeeds() {
        let ps = paths(3);
        let mut invocations = 0;

        let out = analyze_batches(&ps, 10, |batch| {
            invocations += 1;
            if invocations == 1 {
                Err(SidecarError::Closed)
            } else {
                Ok(batch.iter().map(|p| ok_record(p)).collect())
            }
        });

        assert_eq!(invocations, 2, "expected exactly one retry after the first failure");
        assert_eq!(out.len(), 3);
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i], ok_record(p));
        }
    }

    #[test]
    fn analyze_batches_synthesizes_one_failure_record_per_path_when_call_always_errors() {
        let ps = paths(3);
        let mut invocations = 0;

        let out = analyze_batches(&ps, 10, |_batch| {
            invocations += 1;
            Err(SidecarError::Closed)
        });

        assert_eq!(invocations, 2, "expected exactly two attempts, no more");
        assert_eq!(out.len(), 3);
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i]["status"], "failed");
            assert_eq!(out[i]["path"], *p);
        }
    }

    /// The subtle case: `call` succeeds but returns the wrong number of
    /// records (2 for a 3-path batch). The result must still be exactly 3
    /// records, one per input path — not the short array extended verbatim.
    /// A short array here is precisely the silent misattribution Task 15's
    /// positional zip depends on this guarantee to prevent.
    #[test]
    fn analyze_batches_backfills_when_call_returns_too_few_records() {
        let ps = paths(3);

        let out = analyze_batches(&ps, 10, |batch| {
            Ok(batch.iter().take(batch.len() - 1).map(|p| ok_record(p)).collect())
        });

        assert_eq!(out.len(), 3, "must still be one record per input path");
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i]["status"], "failed");
            assert_eq!(out[i]["path"], *p);
        }
    }

    #[test]
    fn analyze_batches_keeps_successful_batches_at_correct_offsets_around_a_failed_one() {
        let ps = paths(7); // batches of 3: [0,1,2] [3,4,5] [6], middle one always fails
        let out = analyze_batches(&ps, 3, |batch| {
            if batch.iter().any(|p| p == "/photos/3.jpg") {
                Err(SidecarError::Closed)
            } else {
                Ok(batch.iter().map(|p| ok_record(p)).collect())
            }
        });

        assert_eq!(out.len(), ps.len());
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i]["path"], *p, "record at offset {i} does not sit opposite its own input path");
            let expected_status = if (3..=5).contains(&i) { "failed" } else { "ok" };
            assert_eq!(out[i]["status"], expected_status, "wrong status at offset {i}");
        }
    }
}
