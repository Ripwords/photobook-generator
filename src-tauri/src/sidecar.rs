use crate::protocol::{ExportItem, ExportRequest, Request, RequestKind, Response, ResponseResult};
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
        //
        // Stdout and stderr share this one channel/loop (that's how the
        // shell plugin delivers both), so a burst of stderr diagnostics is
        // read out just as promptly as stdout lines -- nothing here waits on
        // a full pipe. Stderr lines are logged directly and never touch
        // `tx`/`lines`: that channel feeds response correlation in
        // `request`, and a stderr line landing there would be parsed as a
        // malformed response and could eat a request's timeout.
        let (tx, lines) = mpsc::channel();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).trim().to_string();
                        if !text.is_empty() && tx.send(text).is_err() {
                            break;
                        }
                    }
                    CommandEvent::Stderr(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).trim().to_string();
                        if !text.is_empty() {
                            log::warn!("sidecar stderr: {text}");
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            child: Some(child),
            lines,
            counter: 0,
        })
    }

    fn next_id(&mut self) -> String {
        self.counter += 1;
        self.counter.to_string()
    }

    /// `export` is spelled out as a parameter rather than defaulted to `None`
    /// inside: an `.export` request whose payload was silently dropped is
    /// answered by the sidecar with an error, so making every caller name it
    /// is what stops a future export path from forgetting it.
    pub fn request(
        &mut self,
        kind: RequestKind,
        paths: Option<Vec<String>>,
        thumbnail_dir: Option<String>,
        export: Option<ExportRequest>,
        timeout: Duration,
    ) -> Result<ResponseResult, SidecarError> {
        let id = self.next_id();
        let req = Request {
            id: id.clone(),
            kind,
            paths,
            thumbnail_dir,
            export,
        };
        let mut line =
            serde_json::to_string(&req).map_err(|e| SidecarError::Malformed(e.to_string()))?;
        line.push('\n');
        self.child
            .as_mut()
            .expect("Sidecar's child is only taken by Drop")
            .write(line.as_bytes())
            .map_err(|e| SidecarError::Spawn(e.to_string()))?;

        loop {
            let text = self
                .lines
                .recv_timeout(timeout)
                .map_err(|_| SidecarError::Timeout(timeout))?;
            let res: Response = serde_json::from_str(&text)
                .map_err(|e| SidecarError::Malformed(format!("{e}: {text}")))?;
            if res.id == id {
                return Ok(res.result);
            }
            // Ignore stale responses from a previous, timed-out request.
        }
    }

    pub fn analyze(
        &mut self,
        paths: Vec<String>,
        thumbnail_dir: &str,
    ) -> Result<Vec<serde_json::Value>, SidecarError> {
        let timeout = timeout_for(paths.len());
        let request_result = self.request(
            RequestKind::Analyze,
            Some(paths),
            Some(thumbnail_dir.to_string()),
            None,
            timeout,
        )?;
        match request_result {
            ResponseResult::Analyzed(records) => Ok(records),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Pong { .. }
            | ResponseResult::Benchmarked(_)
            | ResponseResult::Exported(_) => Err(SidecarError::Malformed("expected analyzed".into())),
        }
    }

    /// Runs the `benchmark` request kind: same pipeline as `analyze`, but
    /// the sidecar returns per-stage timings instead of features. Used by
    /// `scripts/benchmark.sh`; not wired into the app's UI, since this is a
    /// diagnostic tool, not a user-facing feature.
    pub fn benchmark(
        &mut self,
        paths: Vec<String>,
        thumbnail_dir: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, SidecarError> {
        let timeout = timeout_for(paths.len());
        let request_result = self.request(
            RequestKind::Benchmark,
            Some(paths),
            thumbnail_dir.map(str::to_string),
            None,
            timeout,
        )?;
        match request_result {
            ResponseResult::Benchmarked(records) => Ok(records),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Pong { .. } | ResponseResult::Analyzed(_) | ResponseResult::Exported(_) => {
                Err(SidecarError::Malformed("expected benchmarked".into()))
            }
        }
    }

    /// Crops and writes one batch of already-laid-out photos. Returns the raw
    /// `ExportRecord` JSON, one per item in input order -- the caller
    /// (`export_batches_with_progress`) is what turns a transport failure into
    /// synthetic per-item records, exactly as `analyze` does for analysis.
    ///
    /// Uses the same `timeout_for` scale as analysis: an export decodes the
    /// full-size source, crops, converts to sRGB and re-encodes, which is the
    /// same order of work per photo as analysing it.
    pub fn export(&mut self, req: ExportRequest) -> Result<Vec<serde_json::Value>, SidecarError> {
        let timeout = timeout_for(req.items.len());
        let request_result = self.request(RequestKind::Export, None, None, Some(req), timeout)?;
        match request_result {
            ResponseResult::Exported(records) => Ok(records),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Pong { .. }
            | ResponseResult::Analyzed(_)
            | ResponseResult::Benchmarked(_) => {
                Err(SidecarError::Malformed("expected exported".into()))
            }
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
    paths
        .chunks(batch_size.max(1))
        .map(<[String]>::to_vec)
        .collect()
}

/// Chunk sizes for `commands::gather_chunked`'s incremental folder gather:
/// starts small (4) so the very first photos surface as fast as possible
/// (only 4 files' worth of hashing before the first progress event), then
/// doubles each step until it reaches `BATCH_SIZE`, after which every
/// chunk is exactly `BATCH_SIZE` -- the same size the sidecar already
/// batches cache misses at internally, so once the ramp is warm a chunk
/// costs one sidecar round-trip. The last chunk is whatever remains, which
/// may be smaller than the target size.
///
/// This is the actual fix for the "0 / 283 frozen for 14 seconds" bug:
/// the old code hashed and cache-looked-up all 283 paths (SHA-256 over
/// every byte of every file, ~6.8GB for a folder of 24MB RAWs) in one pass
/// before the first sidecar batch, let alone the first UI update, ever
/// ran. Chunking the WHOLE pipeline -- hash, cache lookup, and sidecar
/// dispatch, not just reporting -- means the first progress event fires
/// after hashing only 4 files instead of the whole folder.
pub(crate) fn ramp_chunk_sizes(total: usize) -> Vec<usize> {
    let mut sizes = Vec::new();
    let mut remaining = total;
    let mut size = 4usize;
    while remaining > 0 {
        let take = size.min(remaining);
        sizes.push(take);
        remaining -= take;
        size = (size * 2).min(BATCH_SIZE);
    }
    sizes
}

/// Splits `paths` into chunks sized by `ramp_chunk_sizes`. Distinct from
/// `chunk_paths` above (which uniformly chunks the sidecar's OWN cache-miss
/// batches at a fixed size): this ramp chunks the WHOLE folder end to end --
/// hashing, cache lookup, and sidecar dispatch all happen per chunk in
/// `commands::gather_chunked` -- so the first tiles appear after hashing a
/// handful of files rather than the entire folder.
pub(crate) fn chunk_paths_ramped(paths: &[String]) -> Vec<Vec<String>> {
    let sizes = ramp_chunk_sizes(paths.len());
    let mut chunks = Vec::with_capacity(sizes.len());
    let mut offset = 0usize;
    for size in sizes {
        chunks.push(paths[offset..offset + size].to_vec());
        offset += size;
    }
    chunks
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
///
/// `on_batch` additionally fires once per batch with that batch's
/// *resolved* records -- exactly the slice about to be appended to the
/// running output -- so a caller can stream progress to the UI as each batch
/// of up to `batch_size` photos finishes, instead of waiting for the whole
/// folder. Pass `|_| {}` to opt out. `on_batch` sees the same records the
/// function's return value would carry for that batch (real records on
/// success, synthetic `failed` records on a double-error or a count
/// mismatch), in the same order, before they are appended to `out` -- so
/// summing what `on_batch` receives across calls always reconstructs the
/// eventual return value exactly.
pub(crate) fn analyze_batches_with_progress<F, P>(
    paths: &[String],
    batch_size: usize,
    mut call: F,
    mut on_batch: P,
) -> Vec<serde_json::Value>
where
    F: FnMut(&[String]) -> Result<Vec<serde_json::Value>, SidecarError>,
    P: FnMut(&[serde_json::Value]),
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

        let resolved: Vec<serde_json::Value> = match records {
            Some(r) if r.len() == batch.len() => r,
            Some(r) => {
                log::warn!(
                    "sidecar returned {} records for {} paths",
                    r.len(),
                    batch.len()
                );
                batch
                    .iter()
                    .map(|p| failure_record(p, "record count mismatch"))
                    .collect()
            }
            None => batch
                .iter()
                .map(|p| failure_record(p, "sidecar failed twice"))
                .collect(),
        };

        on_batch(&resolved);
        out.extend(resolved);
    }

    out
}

/// How many photos go into one `.export` request.
///
/// Smaller than `BATCH_SIZE` because an export is a full-size decode, crop,
/// colour conversion and re-encode per photo: a batch is the granularity of
/// both the progress the user sees and the timeout a stall costs, and eight
/// full-resolution photos is already tens of seconds of work.
///
/// Splitting one book across several requests is safe with respect to the
/// exporter's collision rule: Swift's claim set is per-REQUEST, and
/// `export::output_filename` is unique across the whole book by construction
/// (page number x z), so two items in different batches can never name the
/// same file.
pub const EXPORT_BATCH_SIZE: usize = 8;

/// Synthetic stand-in for an `ExportRecord` the sidecar never produced.
///
/// Deliberately shaped like Swift's `.failed` case -- `type`/`filename`/
/// `message` -- and NOT like `failure_record` above, which is analysis's
/// `status`/`path` shape. The two wire formats are genuinely different (see
/// `protocol::tests::deserializes_exported_response`), and a synthetic record
/// in the wrong shape would be silently invisible to every consumer that
/// matches on `type`.
pub fn export_failure_record(filename: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "type": "failed", "filename": filename, "message": message })
}

/// The pure batching core of `SidecarPool::export_all`, factored out so it is
/// testable with a fake `call` instead of a live `AppHandle` -- the same
/// reasoning as `analyze_batches_with_progress`.
///
/// Guarantees exactly one record per input item, in input order: a batch that
/// errors twice, or that answers with the wrong number of records, is
/// entirely replaced with synthetic `failed` records rather than partially
/// trusted. The manifest reconciliation downstream drops any entry with no
/// successful record, so a silently-missing record would otherwise leave the
/// manifest claiming a file nobody wrote.
///
/// Retrying once is safe here even though a retry re-exports photos the first
/// attempt may already have written: Swift's collision guard is per-request,
/// so re-running a batch overwrites its own output rather than failing on it
/// (see `Exporter.export`'s doc comment). A retry costs one batch of decode
/// work, never the whole book, because batches are `EXPORT_BATCH_SIZE` wide.
pub(crate) fn export_batches_with_progress<F, P>(
    items: &[ExportItem],
    batch_size: usize,
    mut call: F,
    mut on_batch: P,
) -> Vec<serde_json::Value>
where
    F: FnMut(&[ExportItem]) -> Result<Vec<serde_json::Value>, SidecarError>,
    P: FnMut(&[serde_json::Value]),
{
    let mut out = Vec::with_capacity(items.len());

    for batch in items.chunks(batch_size.max(1)) {
        let mut records = None;

        for attempt in 0..2 {
            match call(batch) {
                Ok(r) => {
                    records = Some(r);
                    break;
                }
                Err(err) => log::warn!("export batch failed (attempt {attempt}): {err}"),
            }
        }

        let resolved: Vec<serde_json::Value> = match records {
            Some(r) if r.len() == batch.len() => r,
            Some(r) => {
                log::warn!("sidecar returned {} export records for {} items", r.len(), batch.len());
                batch
                    .iter()
                    .map(|i| export_failure_record(&i.filename, "record count mismatch"))
                    .collect()
            }
            None => batch
                .iter()
                .map(|i| export_failure_record(&i.filename, "sidecar failed twice"))
                .collect(),
        };

        on_batch(&resolved);
        out.extend(resolved);
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
    /// `on_batch` fires once per `BATCH_SIZE`-sized batch as it resolves, so
    /// a caller with a live `Channel` can stream progress to the UI instead
    /// of waiting for the whole folder -- see `analyze_batches_with_progress`
    /// for the exact contract of what it receives and when.
    ///
    /// Thin wrapper around `analyze_batches_with_progress`: owns the one side
    /// effect that needs a real `AppHandle` (spawning/respawning the child),
    /// which is why this method itself is not unit tested — see
    /// task-14-report.md.
    pub fn analyze_all<P>(
        &mut self,
        app: &AppHandle,
        paths: &[String],
        thumbnail_dir: &str,
        on_batch: P,
    ) -> Vec<serde_json::Value>
    where
        P: FnMut(&[serde_json::Value]),
    {
        analyze_batches_with_progress(
            paths,
            BATCH_SIZE,
            |batch| {
                let result = self
                    .ensure(app)
                    .and_then(|sidecar| sidecar.analyze(batch.to_vec(), thumbnail_dir));
                if result.is_err() {
                    // Drop the child so the next ensure() respawns it.
                    self.inner = None;
                }
                result
            },
            on_batch,
        )
    }

    /// Crops and writes every item, in `EXPORT_BATCH_SIZE` batches, always
    /// returning one record per item. `on_batch` fires as each batch resolves
    /// so the caller can stream export progress the same way `analyze_all`
    /// streams analysis progress.
    ///
    /// Thin wrapper around `export_batches_with_progress`: owns the one side
    /// effect that needs a real `AppHandle` (spawning/respawning the child),
    /// which is why this method itself is not unit tested -- same reasoning
    /// as `analyze_all`.
    pub fn export_all<P>(
        &mut self,
        app: &AppHandle,
        output_dir: &str,
        items: &[ExportItem],
        on_batch: P,
    ) -> Vec<serde_json::Value>
    where
        P: FnMut(&[serde_json::Value]),
    {
        export_batches_with_progress(
            items,
            EXPORT_BATCH_SIZE,
            |batch| {
                let request =
                    ExportRequest { output_dir: output_dir.to_string(), items: batch.to_vec() };
                let result =
                    self.ensure(app).and_then(|sidecar| sidecar.export(request));
                if result.is_err() {
                    // Drop the child so the next ensure() respawns it.
                    self.inner = None;
                }
                result
            },
            on_batch,
        )
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

    // --- `ramp_chunk_sizes` / `chunk_paths_ramped`: the chunking that
    // replaces "hash the whole folder before the first sidecar batch runs"
    // with "hash, cache-lookup, and sidecar-dispatch one small chunk at a
    // time". The brief calls for something like 4, 8, 16, 16, 16... with
    // `BATCH_SIZE` as the steady-state size, in a named tested function.

    #[test]
    fn ramp_starts_at_four_doubles_to_batch_size_then_holds() {
        // 4 + 8 + 16*k... : first two steps double, then every step is
        // pinned at BATCH_SIZE (16) except possibly the final, partial,
        // leftover chunk.
        let sizes = ramp_chunk_sizes(283);
        assert_eq!(&sizes[..4], &[4, 8, 16, 16], "ramp shape: 4, 8, then 16 steady-state");
        let (last, steady_state) = sizes.split_last().expect("283 produces at least one chunk");
        assert!(
            steady_state[2..].iter().all(|&s| s == BATCH_SIZE),
            "every chunk from the third up to (but excluding) the last must be exactly BATCH_SIZE"
        );
        assert!(
            *last <= BATCH_SIZE,
            "the final leftover chunk must be no larger than BATCH_SIZE"
        );
    }

    #[test]
    fn ramp_covers_every_path_exactly_once_in_order() {
        for total in [0, 1, 3, 4, 5, 12, 13, 16, 17, 40, 283] {
            let sizes = ramp_chunk_sizes(total);
            assert_eq!(
                sizes.iter().sum::<usize>(),
                total,
                "ramp for total={total} must sum to the total"
            );
        }
    }

    #[test]
    fn ramp_of_zero_is_empty() {
        assert!(ramp_chunk_sizes(0).is_empty());
    }

    #[test]
    fn ramp_never_exceeds_batch_size_per_chunk() {
        let sizes = ramp_chunk_sizes(500);
        assert!(sizes.iter().all(|&s| s <= BATCH_SIZE));
    }

    #[test]
    fn ramp_final_chunk_is_the_remainder_even_if_smaller_than_the_target() {
        // 4 + 8 = 12, so total=17 leaves a final chunk of 5 (< BATCH_SIZE).
        assert_eq!(ramp_chunk_sizes(17), vec![4, 8, 5]);
    }

    #[test]
    fn chunk_paths_ramped_reconstructs_the_original_paths_in_order() {
        let all = paths(283);
        let chunks = chunk_paths_ramped(&all);
        // Sanity: chunk sizes match the ramp exactly.
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            ramp_chunk_sizes(283)
        );
        let flat: Vec<String> = chunks.into_iter().flatten().collect();
        assert_eq!(flat, all, "chunking must not drop, duplicate, or reorder any path");
    }

    #[test]
    fn chunk_paths_ramped_of_empty_input_is_empty() {
        assert!(chunk_paths_ramped(&[]).is_empty());
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
        let out = analyze_batches_with_progress(
            &ps,
            2,
            |batch| Ok(batch.iter().map(|p| ok_record(p)).collect()),
            |_resolved| {},
        );

        assert_eq!(out.len(), ps.len());
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(
                out[i],
                ok_record(p),
                "record at offset {i} does not match input path {p}"
            );
        }
    }

    #[test]
    fn analyze_batches_retries_once_then_succeeds() {
        let ps = paths(3);
        let mut invocations = 0;

        let out = analyze_batches_with_progress(
            &ps,
            10,
            |batch| {
                invocations += 1;
                if invocations == 1 {
                    Err(SidecarError::Closed)
                } else {
                    Ok(batch.iter().map(|p| ok_record(p)).collect())
                }
            },
            |_resolved| {},
        );

        assert_eq!(
            invocations, 2,
            "expected exactly one retry after the first failure"
        );
        assert_eq!(out.len(), 3);
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i], ok_record(p));
        }
    }

    #[test]
    fn analyze_batches_synthesizes_one_failure_record_per_path_when_call_always_errors() {
        let ps = paths(3);
        let mut invocations = 0;

        let out = analyze_batches_with_progress(
            &ps,
            10,
            |_batch| {
                invocations += 1;
                Err(SidecarError::Closed)
            },
            |_resolved| {},
        );

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

        let out = analyze_batches_with_progress(
            &ps,
            10,
            |batch| {
                Ok(batch
                    .iter()
                    .take(batch.len() - 1)
                    .map(|p| ok_record(p))
                    .collect())
            },
            |_resolved| {},
        );

        assert_eq!(out.len(), 3, "must still be one record per input path");
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(out[i]["status"], "failed");
            assert_eq!(out[i]["path"], *p);
        }
    }

    #[test]
    fn analyze_batches_keeps_successful_batches_at_correct_offsets_around_a_failed_one() {
        let ps = paths(7); // batches of 3: [0,1,2] [3,4,5] [6], middle one always fails
        let out = analyze_batches_with_progress(
            &ps,
            3,
            |batch| {
                if batch.iter().any(|p| p == "/photos/3.jpg") {
                    Err(SidecarError::Closed)
                } else {
                    Ok(batch.iter().map(|p| ok_record(p)).collect())
                }
            },
            |_resolved| {},
        );

        assert_eq!(out.len(), ps.len());
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(
                out[i]["path"], *p,
                "record at offset {i} does not sit opposite its own input path"
            );
            let expected_status = if (3..=5).contains(&i) { "failed" } else { "ok" };
            assert_eq!(
                out[i]["status"], expected_status,
                "wrong status at offset {i}"
            );
        }
    }

    // --- analyze_batches_with_progress: the streaming addition. `on_batch`
    // must see exactly the resolved records for its own batch, in order, and
    // concatenating everything it receives across calls must reconstruct the
    // same return value `analyze_batches` would have produced -- so a caller
    // streaming those records to the UI never shows a photo under the wrong
    // batch or drops one silently.

    #[test]
    fn on_batch_fires_once_per_batch_with_that_batchs_own_records() {
        let ps = paths(5); // batches of 2: [0,1] [2,3] [4]
        let mut seen: Vec<Vec<String>> = Vec::new();

        let out = analyze_batches_with_progress(
            &ps,
            2,
            |batch| Ok(batch.iter().map(|p| ok_record(p)).collect()),
            |resolved| {
                seen.push(
                    resolved
                        .iter()
                        .map(|r| r["path"].as_str().unwrap().to_string())
                        .collect(),
                );
            },
        );

        assert_eq!(
            seen,
            vec![
                vec!["/photos/0.jpg".to_string(), "/photos/1.jpg".to_string()],
                vec!["/photos/2.jpg".to_string(), "/photos/3.jpg".to_string()],
                vec!["/photos/4.jpg".to_string()],
            ]
        );
        assert_eq!(out.len(), 5);
    }

    /// The concatenation-equivalence property stated in the doc comment:
    /// flattening everything `on_batch` receives, in call order, must equal
    /// the function's own return value record-for-record. This is the
    /// property a streaming caller relies on -- it accumulates only what
    /// `on_batch` gives it and must end up with the same data the final
    /// summary is built from.
    #[test]
    fn on_batch_records_concatenate_to_exactly_the_return_value() {
        let ps = paths(7);
        let mut accumulated: Vec<serde_json::Value> = Vec::new();

        let out = analyze_batches_with_progress(
            &ps,
            3,
            |batch| {
                if batch.iter().any(|p| p == "/photos/3.jpg") {
                    Err(SidecarError::Closed)
                } else {
                    Ok(batch.iter().map(|p| ok_record(p)).collect())
                }
            },
            |resolved| accumulated.extend(resolved.iter().cloned()),
        );

        assert_eq!(
            accumulated, out,
            "on_batch's accumulated records must equal the return value exactly"
        );
    }

    /// Mutation-style guard for the invariant that matters most to the UI: a
    /// batch that fails must not shift the paths attached to a LATER batch's
    /// streamed records. Batch 1 (paths 3-5) always fails; batch 2 (path 6)
    /// always succeeds. If an implementation accidentally dropped the failed
    /// batch's records instead of synthesizing failures for it (shifting
    /// everything after it back by one batch), batch 2's streamed record
    /// would carry path "3.jpg" or similar instead of its own "6.jpg".
    #[test]
    fn a_failed_batch_does_not_shift_a_later_batchs_streamed_paths() {
        let ps = paths(7); // batches of 3: [0,1,2] [3,4,5] [6]
        let mut batches_seen: Vec<Vec<(String, bool)>> = Vec::new();

        analyze_batches_with_progress(
            &ps,
            3,
            |batch| {
                if batch.iter().any(|p| p == "/photos/3.jpg") {
                    Err(SidecarError::Closed)
                } else {
                    Ok(batch.iter().map(|p| ok_record(p)).collect())
                }
            },
            |resolved| {
                batches_seen.push(
                    resolved
                        .iter()
                        .map(|r| (r["path"].as_str().unwrap().to_string(), r["status"] == "ok"))
                        .collect(),
                );
            },
        );

        assert_eq!(
            batches_seen.len(),
            3,
            "one on_batch call per batch, including the failed one"
        );
        assert_eq!(
            batches_seen[2],
            vec![("/photos/6.jpg".to_string(), true)],
            "the third (successful) batch's streamed record must still carry its own path, not one \
             shifted in from the failed second batch"
        );
        assert!(
            batches_seen[1].iter().all(|(_, ok)| !ok),
            "the second batch's records must be the synthetic failures"
        );
    }

    // --- export batching: the same "one record per item, no matter what"
    // guarantee `analyze_batches_with_progress` gives, over `ExportItem`s and
    // the `type`-discriminated `ExportRecord` wire shape (NOT `status`/`path`,
    // which is analysis's shape -- see `protocol::tests::
    // deserializes_exported_response`).

    fn item(filename: &str) -> ExportItem {
        ExportItem {
            source_path: format!("/photos/{filename}.jpg"),
            filename: filename.into(),
            crop_x: 0.0,
            crop_y: 0.0,
            crop_w: 1.0,
            crop_h: 1.0,
        }
    }

    fn items(n: usize) -> Vec<ExportItem> {
        (0..n).map(|i| item(&format!("p{i:02}-z1-hash{i:04}"))).collect()
    }

    fn ok_export_record(it: &ExportItem) -> serde_json::Value {
        serde_json::json!({
            "type": "ok",
            "path": format!("/out/{}.jpg", it.filename),
            "width": 3000,
            "height": 2000,
            "bytes": 1234,
        })
    }

    #[test]
    fn export_failure_record_names_the_filename_not_the_source_path() {
        let value = export_failure_record("p04-z1-abcd1234", "sidecar failed twice");
        assert_eq!(value["type"], "failed");
        assert_eq!(value["filename"], "p04-z1-abcd1234");
        assert_eq!(value["message"], "sidecar failed twice");
        assert!(
            value.get("status").is_none(),
            "export records discriminate on `type`, never `status` -- see Protocol.swift"
        );
    }

    #[test]
    fn export_batches_returns_one_record_per_item_in_input_order() {
        let all = items(7);
        let records = export_batches_with_progress(
            &all,
            3,
            |batch| Ok(batch.iter().map(ok_export_record).collect()),
            |_| {},
        );
        assert_eq!(records.len(), 7);
        let paths: Vec<&str> = records.iter().map(|r| r["path"].as_str().unwrap()).collect();
        let expected: Vec<String> = all.iter().map(|i| format!("/out/{}.jpg", i.filename)).collect();
        assert_eq!(paths, expected.iter().map(String::as_str).collect::<Vec<_>>());
    }

    /// A batch the sidecar never answers must still produce one `failed`
    /// record per item in it -- otherwise the manifest reconciliation
    /// downstream would silently keep the items it never heard about, and the
    /// UI would report fewer failures than files actually missing.
    #[test]
    fn a_batch_that_fails_twice_becomes_one_synthetic_failure_per_item() {
        let all = items(5);
        let mut attempts = 0usize;
        let records = export_batches_with_progress(
            &all,
            5,
            |_batch| {
                attempts += 1;
                Err(SidecarError::Closed)
            },
            |_| {},
        );
        assert_eq!(attempts, 2, "one retry, then give up -- never an unbounded loop");
        assert_eq!(records.len(), 5);
        assert!(records.iter().all(|r| r["type"] == "failed"));
        assert_eq!(records[0]["filename"], all[0].filename);
        assert_eq!(records[4]["filename"], all[4].filename);
    }

    /// The count-mismatch guard: a sidecar that answers with the wrong number
    /// of records must have the WHOLE batch replaced, not be partially
    /// trusted. Reconciliation matches records to manifest entries by
    /// filename, but a short array still means some item's outcome is
    /// unknown, and reporting an unknown as a success is the failure mode
    /// that ships a book with a missing page.
    #[test]
    fn a_short_response_replaces_the_whole_batch_with_failures() {
        let all = items(4);
        let records = export_batches_with_progress(
            &all,
            4,
            |batch| Ok(batch.iter().take(2).map(ok_export_record).collect()),
            |_| {},
        );
        assert_eq!(records.len(), 4);
        assert!(
            records.iter().all(|r| r["type"] == "failed"),
            "a short response must not be partially trusted: {records:?}"
        );
    }

    #[test]
    fn export_batches_reports_progress_once_per_batch_including_a_failed_one() {
        let all = items(7);
        let mut seen: Vec<usize> = Vec::new();
        export_batches_with_progress(
            &all,
            3, // batches: 3, 3, 1
            |batch| {
                if batch.iter().any(|i| i.filename.starts_with("p03")) {
                    Err(SidecarError::Closed)
                } else {
                    Ok(batch.iter().map(ok_export_record).collect())
                }
            },
            |resolved| seen.push(resolved.len()),
        );
        assert_eq!(seen, vec![3, 3, 1], "one call per batch, failed batches included");
    }

    #[test]
    fn exporting_no_items_calls_the_sidecar_not_at_all() {
        let records = export_batches_with_progress(
            &[],
            EXPORT_BATCH_SIZE,
            |_| panic!("an empty export must never reach the sidecar"),
            |_| panic!("an empty export must never report progress"),
        );
        assert!(records.is_empty());
    }
}
