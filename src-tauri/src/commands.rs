use crate::{cluster, db::Db, ranking, sidecar::SidecarPool};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

const SUPPORTED: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];

pub fn supported_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| SUPPORTED.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// macOS writes a hidden `._<name>` "AppleDouble" sidecar next to a real
/// file on any volume that can't natively store a resource fork / extended
/// attributes (FAT-formatted cards, many network shares, some external
/// drives) -- so a folder of real photos routinely contains `._IMG_1234.JPG`
/// beside `IMG_1234.JPG`. These pass `supported_extension` (they end in a
/// real image extension) but are not images: ImageIO fails to decode them,
/// which surfaces as a spurious failure with no explanation to the user.
pub fn is_apple_double(name: &str) -> bool {
    name.starts_with("._")
}

/// SHA-256 over raw file bytes. No image decoding, so this is safe to run on
/// every file before deciding what needs analysis.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Default)]
pub struct AppState {
    pub pool: Mutex<SidecarPool>,
}

/// Wire-compatible counterpart of `AnalyzedPhoto`/`AnalysisSummary` in
/// `app/types/features.ts`. `photos` is untyped `serde_json::Value` on
/// purpose (Phase 2 adds fields here), so there is no compiler check linking
/// the two sides: a field rename on either side is a silent `undefined` at
/// runtime, not a build error. If you rename, add, or remove a field the
/// webview reads off `photos[i]`, update `AnalyzedPhoto` in
/// `app/types/features.ts` in the same change, and vice versa.
#[derive(Serialize, Clone)]
pub struct AnalysisSummary {
    pub total: usize,
    pub failed: usize,
    pub cached: usize,
    pub photos: Vec<serde_json::Value>,
}

/// Streamed to the webview over a `tauri::ipc::Channel` as `analyze_folder`
/// progresses, instead of the frontend blocking on one `AnalysisSummary` at
/// the end. A `Channel` (not the event system) is deliberate: Tauri's own
/// docs describe events as JSON-string-only and unsuited to high-throughput,
/// low-latency streaming, which is exactly what a folder of hundreds of
/// photos with thumbnails needs.
///
/// `Batch.photos` carries only per-photo *intrinsic* data (see
/// `partial_photo`) -- never a percentile or cluster id, because those are
/// whole-set derivations that don't exist until every photo has been seen.
/// Assigning one anyway from a partial population, then correcting it once
/// `Done` arrives, would render a rank that visibly changes under the user;
/// the task brief calls this out by name as the thing not to do. The
/// (`analysed`, `cached`, `failed`) counts on `Batch` are cumulative running
/// totals, not per-batch deltas, so the frontend can render "analysed X / Y"
/// directly off the latest event without summing anything itself.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AnalysisEvent {
    /// Fired once, right after the folder is scanned, before any analysis
    /// starts -- gives the frontend the denominator for a determinate
    /// progress bar.
    Scanned { total: usize },
    /// Fired once per resolved batch (cache hits are treated as one batch,
    /// then each `sidecar::BATCH_SIZE`-sized sidecar batch fires its own).
    Batch {
        photos: Vec<serde_json::Value>,
        analysed: usize,
        cached: usize,
        failed: usize,
    },
    /// Fired exactly once, at the very end, carrying the whole-set
    /// derivations (percentiles, cluster ids) that only exist once every
    /// photo has been analysed. The frontend merges these into the photos it
    /// already rendered from `Batch` events rather than replacing them, so a
    /// tile never flickers or reorders -- it just gains its rank.
    Done { summary: AnalysisSummary },
}

/// Projects a fully-decoded features record (either a fresh sidecar result's
/// `features` or a cache hit straight from `Db`) down to the fields
/// intrinsic to that ONE photo -- everything the UI can render immediately,
/// before the whole-set derivations in `finalize_photos` exist. Strips
/// `phash` for the same reason `finalize_photos` does (JS loses precision
/// above 2^53), and never includes `nearDupCluster`/`eventCluster`/
/// `aestheticPct`/`sharpnessPct` -- those only exist once every photo has
/// been analysed.
///
/// Optional fields (`smileFraction`, `thumbnailPath`) are copied only when
/// present in the source, matching Swift's `encodeIfPresent`-driven omission
/// of `nil` optionals from the wire JSON -- see the comment on
/// `AnalyzedPhoto.smileFraction` in `app/types/features.ts`. Building the
/// key unconditionally here would turn "absent" into an explicit JSON
/// `null`, which the frontend happens to also handle correctly today, but
/// would silently diverge from what `finalize_photos` produces for the same
/// photo once `Done` arrives.
pub(crate) fn partial_photo(features: &serde_json::Value) -> serde_json::Value {
    let mut partial = serde_json::Map::new();
    partial.insert("status".into(), "ok".into());
    for key in ["path", "hash", "width", "height", "isUtility", "sceneTags"] {
        partial.insert(key.into(), features[key].clone());
    }
    partial.insert(
        "faceCount".into(),
        features["faces"].as_array().map_or(0, Vec::len).into(),
    );
    for key in ["smileFraction", "thumbnailPath"] {
        if let Some(value) = features.get(key) {
            partial.insert(key.into(), value.clone());
        }
    }
    serde_json::Value::Object(partial)
}

/// Splits one already-resolved batch (guaranteed one record per input path
/// by `sidecar::analyze_batches_with_progress` -- real records on success,
/// synthetic `{"status":"failed",...}` on a double-error or count mismatch)
/// into the partial photos worth streaming to the UI and a count of how many
/// records in this batch failed. Pure and stateless on purpose: the caller
/// owns the running totals, so this function cannot itself accumulate the
/// wrong thing across batches -- see the accumulation tests below for the
/// property that actually matters (a failed batch must not shift a later
/// batch's photos).
pub(crate) fn batch_progress(records: &[serde_json::Value]) -> (Vec<serde_json::Value>, usize) {
    let mut photos = Vec::new();
    let mut failed = 0usize;
    for record in records {
        if record["status"] == "ok" {
            photos.push(partial_photo(&record["features"]));
        } else {
            failed += 1;
        }
    }
    (photos, failed)
}

/// Pure post-processing over the successfully-analysed (cached + freshly
/// sidecar-analysed) records: restores stable ordering, derives the
/// book-relative signals Rust owns (near-duplicate/event clusters, aesthetic
/// and sharpness percentiles), and strips `phash` before the result ever
/// reaches the webview.
///
/// Factored out of `analyze_folder` so it is unit-testable without a live
/// `AppHandle`/sidecar — the same reasoning as `sidecar::analyze_batches`.
/// This is also where the "zipped positionally" contract lives: sorting
/// MUST happen before the derived arrays (`dup_ids`, `event_ids`,
/// `aesthetic`, `sharpness`) are computed, because they are all read back
/// by index against `ok` afterwards. Computing them on pre-sort order and
/// reattaching post-sort would silently swap data between photos.
pub(crate) fn finalize_photos(mut ok: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    // Cached and freshly-analysed photos are interleaved arbitrarily by the
    // caller; restore folder order so the UI is stable across runs, and so
    // every derived array below is computed against one fixed ordering.
    ok.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));

    let phashes: Vec<u64> = ok
        .iter()
        .map(|f| f["phash"].as_u64().unwrap_or(0))
        .collect();
    let dup_ids = cluster::near_duplicate_clusters(&phashes, 4);

    let times: Vec<Option<i64>> = ok
        .iter()
        .map(|f| f["exif"]["captureDate"].as_f64().map(|t| t as i64))
        .collect();
    let event_ids = cluster::event_clusters(&times, 4 * 3600);

    let aesthetic = ranking::percentiles(
        &ok.iter()
            .map(|f| f["aestheticScore"].as_f64().unwrap_or(0.0))
            .collect::<Vec<_>>(),
    );
    let sharpness = ranking::percentiles(
        &ok.iter()
            .map(|f| f["sharpness"].as_f64().unwrap_or(0.0))
            .collect::<Vec<_>>(),
    );

    for (i, features) in ok.iter_mut().enumerate() {
        features["nearDupCluster"] = dup_ids[i].into();
        features["eventCluster"] = event_ids[i].into();
        features["aestheticPct"] = aesthetic[i].into();
        features["sharpnessPct"] = sharpness[i].into();
        features["faceCount"] = features["faces"].as_array().map_or(0, Vec::len).into();
        features["status"] = "ok".into();

        // phash is a full 64-bit value and JavaScript numbers lose precision
        // above 2^53. Clustering is done with it by this point, and the UI
        // has no use for it, so drop it rather than hand the webview a
        // value that is silently wrong for anyone who later reads it.
        if let Some(object) = features.as_object_mut() {
            object.remove("phash");
        }
    }

    ok
}

/// Minimum wall-clock time `analyze_folder` must take before its completion
/// is worth surfacing as a native notification. A fully-cached re-run
/// finishes in a couple of seconds; notifying after that is pure noise
/// because the user never had a reason to leave the screen. Named so the
/// threshold isn't a magic number scattered across the decision function and
/// its tests.
pub(crate) const NOTIFY_MIN_ELAPSED: Duration = Duration::from_secs(10);

/// Decides whether a completed `analyze_folder` run deserves a native
/// notification. Both conditions must hold: the run took long enough that
/// the user could plausibly have switched away (`elapsed >=
/// NOTIFY_MIN_ELAPSED`), and the window is not currently focused -- if the
/// user is watching the screen, the result is already visible and a
/// notification would be redundant.
///
/// Pure and extracted specifically so this decision is unit-testable
/// without a live `AppHandle`/`WebviewWindow` (which the actual `.show()`
/// call needs and which cannot be constructed in a unit test) -- the same
/// reasoning as `finalize_photos`/`lookup_cache`/`sidecar::analyze_batches`.
pub(crate) fn should_notify(elapsed: Duration, window_focused: bool) -> bool {
    !window_focused && elapsed >= NOTIFY_MIN_ELAPSED
}

/// Counts how many analysed photos would survive the frontend's culling:
/// drop utility shots (screenshots/documents), then keep only the sharpest
/// photo from each near-duplicate cluster (ties broken by aesthetic
/// percentile). This mirrors `keepers()` in `app/types/features.ts` exactly.
///
/// Duplicated rather than shared: the notification fires from Rust (inside
/// `analyze_folder`, which already knows when the work finished), and there
/// is no Rust/TS boundary to call the frontend's implementation through. If
/// `keepers()` in `features.ts` ever changes, mirror the change here too.
pub(crate) fn count_keepers(photos: &[serde_json::Value]) -> usize {
    let mut best: std::collections::HashMap<u64, (f64, f64)> = std::collections::HashMap::new();

    for photo in photos {
        if photo["isUtility"].as_bool().unwrap_or(false) {
            continue;
        }
        let cluster = photo["nearDupCluster"].as_u64().unwrap_or(0);
        let sharpness = photo["sharpnessPct"].as_f64().unwrap_or(0.0);
        let aesthetic = photo["aestheticPct"].as_f64().unwrap_or(0.0);

        best.entry(cluster)
            .and_modify(|incumbent| {
                if sharpness > incumbent.0 || (sharpness == incumbent.0 && aesthetic > incumbent.1)
                {
                    *incumbent = (sharpness, aesthetic);
                }
            })
            .or_insert((sharpness, aesthetic));
    }

    best.len()
}

/// Result of consulting the cache for a batch of candidate paths.
pub(crate) struct CacheLookup {
    /// Already-analysed `features` JSON, pulled straight from `Db`.
    pub hits: Vec<serde_json::Value>,
    /// Paths with no usable cache entry; still need the sidecar.
    pub misses: Vec<String>,
    /// Files that could not even be hashed (e.g. unreadable). Counted
    /// separately so they land in the caller's `failed` count rather than
    /// silently vanishing from `total`.
    pub hash_failures: usize,
}

/// Hashes every path, consults `db`, and splits the input into cache hits and
/// misses. This is the hash -> cache-lookup -> miss-list -> count block that
/// regressed once before (a design that wrote to the cache and never read
/// it), extracted so it is unit-testable without a live `AppHandle`/sidecar
/// -- the same reasoning as `finalize_photos`/`sidecar::analyze_batches`.
pub(crate) fn lookup_cache(db: &Db, paths: &[String]) -> Result<CacheLookup, String> {
    let mut hits = Vec::new();
    let mut misses = Vec::new();
    let mut hash_failures = 0usize;

    for path in paths {
        match hash_file(Path::new(path)) {
            Ok(hash) => match db.get_features(&hash).map_err(|e| e.to_string())? {
                Some(json) => match serde_json::from_str::<serde_json::Value>(&json) {
                    // The cache is keyed by content hash, but the stored
                    // `path` is whatever it was when the file was FIRST
                    // analysed. Two byte-identical files (e.g. "IMG_1234.jpg"
                    // and "IMG_1234 copy.jpg") both hash the same and must
                    // not both come back with the same stale path -- that
                    // produces duplicate `:key="photo.path"` entries in the
                    // UI grid. A renamed folder hits the same bug for every
                    // cached photo at once. Overwrite with the CURRENT path
                    // on every hit, never trust the cached one.
                    Ok(mut features) => {
                        features["path"] = path.clone().into();
                        hits.push(features);
                    }
                    Err(_) => misses.push(path.clone()),
                },
                None => misses.push(path.clone()),
            },
            Err(err) => {
                log::warn!("cannot hash {path}: {err}");
                hash_failures += 1;
            }
        }
    }

    Ok(CacheLookup {
        hits,
        misses,
        hash_failures,
    })
}

#[tauri::command]
pub async fn analyze_folder(
    app: AppHandle,
    folder: String,
    on_event: Channel<AnalysisEvent>,
) -> Result<AnalysisSummary, String> {
    let started = Instant::now();

    let mut paths: Vec<String> = std::fs::read_dir(&folder)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| match entry {
            Ok(e) => Some(e),
            Err(err) => {
                log::warn!("skipping unreadable directory entry in {folder}: {err}");
                None
            }
        })
        .map(|entry| entry.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !is_apple_double(n) && supported_extension(n))
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    paths.sort();

    // A send failure (e.g. the webview navigated away mid-run) must not fail
    // an analysis that would otherwise succeed -- logged and swallowed, same
    // pattern as the completion notification below.
    if let Err(err) = on_event.send(AnalysisEvent::Scanned { total: paths.len() }) {
        log::warn!("failed to send Scanned event: {err}");
    }

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let db_path = app_data_dir.join("photobook.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    let db = Db::open(&db_path).map_err(|e| e.to_string())?;

    // The webview cannot decode RAW/HEIC originals at all and loading
    // hundreds of full-size decoded images is not viable, so the sidecar
    // writes small JPEG thumbnails here during analysis. Rust owns
    // `app_data_dir`, so the directory is computed and created here rather
    // than hardcoded on the Swift side.
    let thumbnail_dir = app_data_dir.join("thumbnails");
    // A missing thumbnail is a blank grid tile, not a lost photo (see the
    // doc comment on `PhotoFeatures.thumbnailPath` in Analyzer.swift) -- so
    // failing to even create the directory must not abort a potentially
    // multi-minute analysis that would otherwise succeed. Logged and
    // swallowed rather than propagated with `?`; the sidecar's own
    // `ThumbnailWriter.write` makes an independent attempt per photo and
    // degrades each one to a nil `thumbnailPath` on failure.
    if let Err(err) = std::fs::create_dir_all(&thumbnail_dir) {
        log::warn!("failed to create thumbnail directory {thumbnail_dir:?}: {err}");
    }
    let thumbnail_dir = thumbnail_dir.to_string_lossy().into_owned();

    // Hash everything first (cheap, no decode), then send only cache misses to
    // the sidecar. Re-running on the same folder should do almost no work.
    // This ordering -- hash, then consult the cache, THEN call the sidecar --
    // is the entire point of having a cache at all.
    let CacheLookup {
        hits,
        misses,
        hash_failures,
    } = lookup_cache(&db, &paths)?;
    let mut ok: Vec<serde_json::Value> = hits;
    let cached = ok.len();
    let mut failed = hash_failures;

    // Cache hits (and hash failures, discovered in the same pass) are known
    // the instant `lookup_cache` returns -- no sidecar round-trip needed --
    // so they stream as one immediate batch, ahead of anything the sidecar
    // produces. `analysed` is 0 here: nothing has come back from the sidecar
    // yet.
    if !ok.is_empty() || hash_failures > 0 {
        let photos = ok.iter().map(partial_photo).collect();
        if let Err(err) = on_event.send(AnalysisEvent::Batch {
            photos,
            analysed: 0,
            cached,
            failed,
        }) {
            log::warn!("failed to send cache-hit Batch event: {err}");
        }
    }

    // `Sidecar::request` (called inside `analyze_all`) does a *blocking*
    // `std::sync::mpsc::Receiver::recv_timeout` while it waits for the
    // sidecar's stdout-drain task -- a tokio task spawned in `Sidecar::spawn`
    // -- to hand it the response line. Both that blocking wait and the drain
    // task need to run on the tauri/tokio async worker pool. Calling it
    // inline here (this fn runs as a task on that same pool, since it's an
    // async `#[tauri::command]`) can starve the drain task of a thread to
    // run on: on a machine with few worker threads, the request then
    // resolves only once its OWN timeout elapses, even though the sidecar
    // already responded. See
    // `.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/thumbnails-report.md`
    // for the traced root cause. `spawn_blocking` moves the blocking wait
    // onto tokio's separate blocking-thread pool, leaving the async worker
    // pool free for the drain task.
    let records = {
        let app_for_pool = app.clone();
        let on_event_for_pool = on_event.clone();
        // Running totals for the sidecar's share of the work, seeded from
        // what the cache pass already found. `on_batch` closes over these
        // (FnMut) and fires once per `sidecar::BATCH_SIZE`-sized batch, via
        // `batch_progress` -- the pure ok/failed split -- so the closure
        // itself does nothing but accumulate and send.
        let mut analysed_so_far = 0usize;
        let mut failed_so_far = failed;
        tauri::async_runtime::spawn_blocking(move || -> Result<Vec<serde_json::Value>, String> {
            let state = app_for_pool.state::<AppState>();
            let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
            Ok(
                pool.analyze_all(&app_for_pool, &misses, &thumbnail_dir, |batch| {
                    let (photos, batch_failed) = batch_progress(batch);
                    analysed_so_far += photos.len();
                    failed_so_far += batch_failed;
                    if let Err(err) = on_event_for_pool.send(AnalysisEvent::Batch {
                        photos,
                        analysed: analysed_so_far,
                        cached,
                        failed: failed_so_far,
                    }) {
                        log::warn!("failed to send sidecar-batch Batch event: {err}");
                    }
                }),
            )
        })
        .await
        .map_err(|e| e.to_string())??
    };

    for record in records {
        if record["status"] == "ok" {
            let features = record["features"].clone();
            if let (Some(hash), Some(path)) = (features["hash"].as_str(), features["path"].as_str())
            {
                // A failed cache write must not discard a photo that the
                // sidecar already spent potentially multi-minute analysis
                // producing -- it just means this photo won't be a cache hit
                // next run. Logged and swallowed, matching the notification
                // failure path below.
                if let Err(err) = db.put_features(hash, path, &features.to_string()) {
                    log::warn!("failed to write cache entry for {path}: {err}");
                }
            }
            ok.push(features);
        } else {
            failed += 1;
        }
    }

    let ok = finalize_photos(ok);

    // The window handle and the notification call both need a live
    // AppHandle and cannot be unit-tested; `should_notify` (the decision of
    // *whether* to fire) is the pure part and is covered separately. A
    // denied permission or any other plugin error must not fail the
    // analysis that already succeeded, so this is logged and swallowed
    // rather than propagated with `?`.
    let window_focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(true); // Can't determine focus -> assume focused, so we err toward silence rather than a spurious notification.

    if should_notify(started.elapsed(), window_focused) {
        let body = format!(
            "{} photos analysed, {} keepers",
            ok.len(),
            count_keepers(&ok)
        );
        if let Err(err) = app
            .notification()
            .builder()
            .title("Analysis complete")
            .body(body)
            .show()
        {
            log::warn!("failed to show completion notification: {err}");
        }
    }

    let summary = AnalysisSummary {
        total: paths.len(),
        failed,
        cached,
        photos: ok,
    };

    // The one and only place the whole-set derivations (percentiles, cluster
    // ids) reach the webview. Everything streamed via `Batch` above was
    // intentionally missing them; this is what lets the frontend merge
    // rank/cluster data into tiles it already rendered instead of the tile
    // itself changing underneath the user.
    if let Err(err) = on_event.send(AnalysisEvent::Done {
        summary: summary.clone(),
    }) {
        log::warn!("failed to send Done event: {err}");
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_photo_extensions() {
        for name in [
            "a.jpg", "b.JPEG", "c.heic", "d.png", "e.CR2", "f.nef", "g.arw", "h.dng",
        ] {
            assert!(supported_extension(name), "{name} should be supported");
        }
    }

    #[test]
    fn rejects_non_photo_files() {
        for name in ["notes.txt", "movie.mov", "archive.zip", "noextension"] {
            assert!(!supported_extension(name), "{name} should be rejected");
        }
    }

    #[test]
    fn recognizes_apple_double_sidecar_files() {
        for name in ["._IMG_1234.JPG", "._photo.heic", "._.DS_Store"] {
            assert!(
                is_apple_double(name),
                "{name} should be recognized as an AppleDouble sidecar"
            );
        }
    }

    #[test]
    fn does_not_flag_ordinary_photos_as_apple_double() {
        for name in ["IMG_1234.JPG", "a._weird.jpg", "photo.heic"] {
            assert!(
                !is_apple_double(name),
                "{name} should not be flagged as an AppleDouble sidecar"
            );
        }
    }

    #[test]
    fn hashes_identical_bytes_identically() {
        let dir = std::env::temp_dir().join("pbg-hash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        std::fs::write(&a, b"same bytes").unwrap();
        std::fs::write(&b, b"same bytes").unwrap();
        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn hashes_different_bytes_differently() {
        let dir = std::env::temp_dir().join("pbg-hash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("c.bin");
        let b = dir.join("d.bin");
        std::fs::write(&a, b"one").unwrap();
        std::fs::write(&b, b"two").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn hashing_a_missing_file_is_an_error_not_a_panic() {
        assert!(hash_file(std::path::Path::new("/nonexistent/nope.bin")).is_err());
    }

    /// Regression guard for I4: `hash_file` here and Swift's `contentHash`
    /// (`sidecar/Sources/PhotobookEngine/Analyzer.swift`) must produce
    /// byte-identical hex strings over the same file -- Rust's hash is the
    /// cache lookup key, Swift's is the write-back key AND the thumbnail
    /// filename. A divergence (hex case, chunk size, algorithm) makes every
    /// run a 100% cache miss and orphans every thumbnail, silently: no
    /// error, every existing test in both languages stays green, because
    /// nothing compared the two literal outputs against each other. Pinned
    /// against a fixture committed to the repo and the SAME literal string
    /// as `AnalyzerTests.swift`'s
    /// `analyzerHashMatchesThePinnedRustLiteral` -- the two tests can only
    /// both pass if the implementations genuinely agree.
    #[test]
    fn hash_matches_the_literal_pinned_against_swifts_content_hash() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sidecar/Fixtures/landscape.jpg");
        let hash = hash_file(&fixture).unwrap();
        assert_eq!(
            hash, "08c8f73e189ba397ff2097fe192831e8f266f98538233e9b12b5790e65720159",
            "hash_file's output for sidecar/Fixtures/landscape.jpg must match Swift's \
             contentHash over the same bytes -- see analyzerHashMatchesThePinnedRustLiteral"
        );
    }

    // --- `lookup_cache`: the hash -> cache-lookup -> miss-list -> count
    // block, extracted specifically because this is the exact logic that
    // regressed once before (a design that wrote to the cache and never read
    // it). `Db::open_in_memory` (already used by db.rs's own tests) makes
    // this testable without a live `AppHandle`/sidecar.

    fn write_temp_file(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join("pbg-cache-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_cached_hash_is_a_hit_not_a_miss() {
        let db = Db::open_in_memory().unwrap();
        let path = write_temp_file("cached.jpg", b"cached bytes");
        let hash = hash_file(Path::new(&path)).unwrap();
        db.put_features(&hash, &path, r#"{"path":"cached.jpg","status":"ok"}"#)
            .unwrap();

        let result = lookup_cache(&db, &[path]).unwrap();

        assert_eq!(
            result.hits.len(),
            1,
            "a cached hash must be returned as a hit"
        );
        assert!(
            result.misses.is_empty(),
            "a cached hash must not appear in the miss list"
        );
        assert_eq!(result.hash_failures, 0);
    }

    #[test]
    fn an_uncached_hash_is_a_miss() {
        let db = Db::open_in_memory().unwrap();
        let path = write_temp_file("uncached.jpg", b"uncached bytes");

        let result = lookup_cache(&db, std::slice::from_ref(&path)).unwrap();

        assert!(
            result.hits.is_empty(),
            "an uncached hash must not be reported as a hit"
        );
        assert_eq!(result.misses, vec![path]);
        assert_eq!(result.hash_failures, 0);
    }

    #[test]
    fn a_hash_failure_is_counted_and_does_not_silently_vanish() {
        let db = Db::open_in_memory().unwrap();
        let missing = "/nonexistent/pbg-cache-test/gone.jpg".to_string();

        let result = lookup_cache(&db, &[missing]).unwrap();

        assert!(result.hits.is_empty());
        assert!(
            result.misses.is_empty(),
            "a hash failure must be counted as a failure, not silently retried as a miss"
        );
        assert_eq!(result.hash_failures, 1);
    }

    /// Regression test for C2: the cache is keyed by content hash, but the
    /// JSON blob stored under that hash carries whatever `path` the file had
    /// when it was FIRST analysed. `path_a` here simulates that original
    /// analysis; `path_b` is a byte-identical file at a different path (a
    /// duplicate filename, or the same file after a folder rename) that
    /// hashes identically and therefore hits the cache. The returned
    /// record's `path` must be `path_b` -- the path THIS call was asked
    /// about -- never the stale `path_a` baked into the cached row.
    #[test]
    fn a_cache_hit_carries_the_current_path_not_the_stale_cached_one() {
        let db = Db::open_in_memory().unwrap();
        let path_a = write_temp_file("stale-path-original.jpg", b"byte-identical content");
        let hash = hash_file(Path::new(&path_a)).unwrap();
        db.put_features(
            &hash,
            &path_a,
            &serde_json::json!({"path": path_a, "status": "ok"}).to_string(),
        )
        .unwrap();

        let path_b = write_temp_file("stale-path-duplicate.jpg", b"byte-identical content");
        let result = lookup_cache(&db, std::slice::from_ref(&path_b)).unwrap();

        assert_eq!(
            result.hits.len(),
            1,
            "byte-identical content must be a cache hit"
        );
        assert_eq!(
            result.hits[0]["path"].as_str().unwrap(),
            path_b,
            "a cache hit must carry the path passed in THIS call, not the stale path baked into the cached row"
        );
    }

    #[test]
    fn every_input_path_is_accounted_for_exactly_once() {
        let db = Db::open_in_memory().unwrap();

        let cached_path = write_temp_file("counted-cached.jpg", b"counted cached bytes");
        let hash = hash_file(Path::new(&cached_path)).unwrap();
        db.put_features(&hash, &cached_path, r#"{"path":"x","status":"ok"}"#)
            .unwrap();

        let miss_path = write_temp_file("counted-miss.jpg", b"counted miss bytes");
        let missing_path = "/nonexistent/pbg-cache-test/gone-too.jpg".to_string();

        let input = vec![cached_path, miss_path, missing_path];
        let result = lookup_cache(&db, &input).unwrap();

        assert_eq!(
            result.hits.len() + result.misses.len() + result.hash_failures,
            input.len(),
            "hits + misses + hash_failures must equal the input count"
        );
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.misses.len(), 1);
        assert_eq!(result.hash_failures, 1);
    }

    // --- Additional tests beyond the brief: `finalize_photos`, which the
    // brief's reference implementation inlines directly into `analyze_folder`
    // (untestable without a live AppHandle/sidecar). Extracted here so the
    // ordering and phash-stripping contracts from the task brief can be
    // pinned directly, the same way `sidecar::analyze_batches` was pulled
    // out of `SidecarPool::analyze_all` in Task 14.

    fn feat(
        path: &str,
        phash: u64,
        aesthetic: f64,
        sharpness: f64,
        capture: Option<f64>,
        face_count: usize,
    ) -> serde_json::Value {
        serde_json::json!({
            "path": path,
            "phash": phash,
            "aestheticScore": aesthetic,
            "sharpness": sharpness,
            "exif": { "captureDate": capture },
            "faces": vec![serde_json::json!({}); face_count],
        })
    }

    #[test]
    fn strips_phash_from_output() {
        let ok = vec![feat("/p/a.jpg", 42, 0.5, 10.0, None, 0)];
        let result = finalize_photos(ok);
        assert!(
            result[0].get("phash").is_none(),
            "phash must not reach the webview"
        );
    }

    #[test]
    fn sorts_output_by_path_regardless_of_input_order() {
        let ok = vec![
            feat("/p/b.jpg", 1, 0.1, 1.0, None, 0),
            feat("/p/a.jpg", 1, 0.1, 1.0, None, 0),
        ];
        let result = finalize_photos(ok);
        assert_eq!(result[0]["path"], "/p/a.jpg");
        assert_eq!(result[1]["path"], "/p/b.jpg");
    }

    #[test]
    fn face_count_is_derived_from_the_faces_array_length() {
        let ok = vec![feat("/p/a.jpg", 1, 0.1, 1.0, None, 3)];
        let result = finalize_photos(ok);
        assert_eq!(result[0]["faceCount"], 3);
    }

    /// The core of contract #1 from the task brief: `event_clusters` and
    /// `percentiles` are zipped positionally against `ok`. If an
    /// implementation computed these derived arrays BEFORE sorting `ok` by
    /// path but then read them back by index AFTER sorting, data would
    /// silently swap between photos. Input here is deliberately reverse-path
    /// order with the higher aesthetic score on the photo that sorts LAST,
    /// so a pre/post-sort index mismatch inverts the comparison below.
    #[test]
    fn attributes_percentile_data_to_the_correct_photo_after_sorting() {
        let ok = vec![
            feat("/p/b.jpg", 1, 0.9, 5.0, None, 2), // higher score, sorts second
            feat("/p/a.jpg", 1, 0.1, 5.0, None, 0), // lower score, sorts first
        ];
        let result = finalize_photos(ok);

        assert_eq!(result[0]["path"], "/p/a.jpg");
        assert_eq!(result[1]["path"], "/p/b.jpg");
        assert!(
            result[0]["aestheticPct"].as_u64().unwrap()
                < result[1]["aestheticPct"].as_u64().unwrap(),
            "a.jpg (lower raw score) must rank below b.jpg (higher raw score) after sorting"
        );
        assert_eq!(result[1]["faceCount"], 2);
        assert_eq!(result[0]["faceCount"], 0);
    }

    /// Same mismatch risk as above, for `event_clusters` specifically (the
    /// brief calls this out by name as a distinct contract from
    /// `percentiles`). Reverse-path order again, with timestamps far enough
    /// apart that a positional swap would be visible as the wrong photo
    /// landing in event cluster 0.
    #[test]
    fn attributes_event_cluster_to_the_correct_photo_after_sorting() {
        let day = 86_400.0;
        let ok = vec![
            feat("/p/late.jpg", 1, 0.5, 5.0, Some(10.0 * day), 0),
            feat("/p/early.jpg", 1, 0.5, 5.0, Some(0.0), 0),
        ];
        let result = finalize_photos(ok);

        assert_eq!(result[0]["path"], "/p/early.jpg");
        assert_eq!(result[1]["path"], "/p/late.jpg");
        assert_ne!(result[0]["eventCluster"], result[1]["eventCluster"]);
        assert_eq!(
            result[0]["eventCluster"], 0,
            "chronologically-first photo gets the lower id"
        );
        assert_eq!(result[1]["eventCluster"], 1);
    }

    // --- `partial_photo`: projects a full features record down to the
    // fields intrinsic to one photo, for streaming ahead of the whole-set
    // derivations.

    fn full_features(over: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "path": "/p/a.jpg",
            "hash": "abc123",
            "width": 4032,
            "height": 3024,
            "isUtility": false,
            "sceneTags": ["beach", "sunset"],
            "faces": [serde_json::json!({}), serde_json::json!({})],
            "aestheticScore": 0.7,
            "sharpness": 12.0,
            "phash": 42u64,
            "exif": { "captureDate": null },
        });
        for (key, value) in over.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    #[test]
    fn partial_photo_carries_intrinsic_fields() {
        let features = full_features(
            serde_json::json!({ "thumbnailPath": "/thumbs/a.jpg", "smileFraction": 0.5 }),
        );
        let partial = partial_photo(&features);

        assert_eq!(partial["status"], "ok");
        assert_eq!(partial["path"], "/p/a.jpg");
        assert_eq!(partial["hash"], "abc123");
        assert_eq!(partial["width"], 4032);
        assert_eq!(partial["height"], 3024);
        assert_eq!(partial["isUtility"], false);
        assert_eq!(partial["sceneTags"], serde_json::json!(["beach", "sunset"]));
        assert_eq!(partial["thumbnailPath"], "/thumbs/a.jpg");
        assert_eq!(partial["smileFraction"], 0.5);
    }

    #[test]
    fn partial_photo_derives_face_count_from_the_faces_array_length() {
        let features = full_features(serde_json::json!({}));
        let partial = partial_photo(&features);
        assert_eq!(partial["faceCount"], 2);
    }

    #[test]
    fn partial_photo_strips_phash() {
        let features = full_features(serde_json::json!({}));
        let partial = partial_photo(&features);
        assert!(
            partial.get("phash").is_none(),
            "phash must not reach the webview, even in a partial record"
        );
    }

    /// The invariant the task brief calls out by name: a partial record must
    /// never carry a percentile or cluster id, even if the source somehow
    /// already had one (e.g. a future bug re-feeding an already-finalized
    /// record through `partial_photo`). Those are whole-set derivations that
    /// don't exist until every photo has been analysed, and showing one that
    /// will later change is exactly what the brief says not to do.
    #[test]
    fn partial_photo_never_carries_whole_set_derivations() {
        let features = full_features(serde_json::json!({
            "nearDupCluster": 3,
            "eventCluster": 1,
            "aestheticPct": 90,
            "sharpnessPct": 80,
        }));
        let partial = partial_photo(&features);

        for key in [
            "nearDupCluster",
            "eventCluster",
            "aestheticPct",
            "sharpnessPct",
        ] {
            assert!(
                partial.get(key).is_none(),
                "partial_photo must never carry {key}"
            );
        }
    }

    /// Swift's synthesized `Codable` omits a `nil` optional from the wire
    /// JSON entirely (`encodeIfPresent`), so the key is genuinely ABSENT from
    /// `features`, not present with a JSON `null`. `partial_photo` must
    /// preserve that absence rather than manufacturing an explicit `null` --
    /// see the comment on `AnalyzedPhoto.smileFraction` in
    /// `app/types/features.ts`.
    #[test]
    fn partial_photo_omits_smile_fraction_when_absent_rather_than_nulling_it() {
        let features = full_features(serde_json::json!({}));
        assert!(
            features.get("smileFraction").is_none(),
            "sanity: the fixture omits it"
        );

        let partial = partial_photo(&features);
        assert!(
            partial.get("smileFraction").is_none(),
            "an absent smileFraction must stay absent, not become an explicit null"
        );
    }

    // --- `batch_progress`: splits one resolved batch into the partial
    // photos worth streaming and a failure count, stateless so the caller
    // owns (and cannot corrupt) the running totals across batches.

    fn ok_wire_record(path: &str) -> serde_json::Value {
        serde_json::json!({ "status": "ok", "features": full_features(serde_json::json!({ "path": path })) })
    }

    fn failed_wire_record(path: &str) -> serde_json::Value {
        serde_json::json!({ "status": "failed", "path": path, "message": "boom" })
    }

    #[test]
    fn batch_progress_splits_ok_and_failed_records() {
        let records = vec![
            ok_wire_record("/p/a.jpg"),
            failed_wire_record("/p/b.jpg"),
            ok_wire_record("/p/c.jpg"),
        ];
        let (photos, failed) = batch_progress(&records);

        assert_eq!(photos.len(), 2);
        assert_eq!(photos[0]["path"], "/p/a.jpg");
        assert_eq!(photos[1]["path"], "/p/c.jpg");
        assert_eq!(failed, 1);
    }

    #[test]
    fn batch_progress_on_an_empty_batch_is_empty() {
        let (photos, failed) = batch_progress(&[]);
        assert!(photos.is_empty());
        assert_eq!(failed, 0);
    }

    #[test]
    fn batch_progress_on_an_all_failed_batch_streams_no_photos() {
        let records = vec![
            failed_wire_record("/p/a.jpg"),
            failed_wire_record("/p/b.jpg"),
        ];
        let (photos, failed) = batch_progress(&records);
        assert!(photos.is_empty());
        assert_eq!(failed, 2);
    }

    // --- Streaming accumulation across multiple sidecar batches: wires
    // `sidecar::analyze_batches_with_progress` (already covers the
    // retry/backfill/offset guarantees on its own) together with
    // `batch_progress` the same way `analyze_folder`'s spawn_blocking closure
    // does, and checks the two properties the task brief asks for
    // explicitly: partial records accumulate in input order, and a failed
    // batch does not shift a later batch's photos.

    #[test]
    fn streamed_partial_photos_accumulate_in_input_order_across_batches() {
        let paths: Vec<String> = (0..7).map(|i| format!("/p/{i}.jpg")).collect();
        let mut streamed: Vec<serde_json::Value> = Vec::new();

        crate::sidecar::analyze_batches_with_progress(
            &paths,
            3, // batches: [0,1,2] [3,4,5] [6]
            |batch| Ok(batch.iter().map(|p| ok_wire_record(p)).collect()),
            |resolved| {
                let (photos, _failed) = batch_progress(resolved);
                streamed.extend(photos);
            },
        );

        let streamed_paths: Vec<&str> = streamed
            .iter()
            .map(|p| p["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            streamed_paths,
            paths.iter().map(String::as_str).collect::<Vec<_>>(),
            "streamed partial photos must arrive in the same order as the input paths"
        );
    }

    /// Mutation-checked: batch 2 (paths 3,4,5) always fails and streams zero
    /// photos. Batch 3 (path 6) must still stream a partial photo for its
    /// OWN path, not one shifted in from an earlier or later batch, and
    /// batch 1's photos must be unaffected by batch 2's failure. This is the
    /// exact "a failed batch does not shift subsequent photos' data"
    /// property from the brief -- verified below to actually fail under a
    /// broken accumulation (see the mutation note in the streaming report).
    #[test]
    fn a_failed_batch_does_not_shift_or_corrupt_later_streamed_photos() {
        let paths: Vec<String> = (0..7).map(|i| format!("/p/{i}.jpg")).collect();
        let mut streamed: Vec<serde_json::Value> = Vec::new();
        let mut total_failed = 0usize;

        crate::sidecar::analyze_batches_with_progress(
            &paths,
            3, // batches: [0,1,2] [3,4,5] [6]
            |batch| {
                if batch.iter().any(|p| p == "/p/3.jpg") {
                    Err(crate::sidecar::SidecarError::Closed)
                } else {
                    Ok(batch.iter().map(|p| ok_wire_record(p)).collect())
                }
            },
            |resolved| {
                let (photos, failed) = batch_progress(resolved);
                streamed.extend(photos);
                total_failed += failed;
            },
        );

        let streamed_paths: Vec<&str> = streamed
            .iter()
            .map(|p| p["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            streamed_paths,
            vec!["/p/0.jpg", "/p/1.jpg", "/p/2.jpg", "/p/6.jpg"],
            "the failed middle batch must contribute no photos, and the last batch's photo \
             must still carry its own path"
        );
        assert_eq!(
            total_failed, 3,
            "the three paths in the failed batch must all be counted as failed"
        );
    }

    // --- `should_notify`: the pure decision of whether a completed
    // `analyze_folder` run deserves a native notification. The actual
    // `.show()` call needs a live AppHandle/WebviewWindow and is not
    // unit-testable; this is the part that is, extracted for exactly that
    // reason (same as `finalize_photos`/`lookup_cache`).

    #[test]
    fn does_not_notify_when_unfocused_but_under_the_threshold() {
        assert!(!should_notify(
            NOTIFY_MIN_ELAPSED - Duration::from_millis(1),
            false
        ));
    }

    /// The boundary itself: `should_notify` uses `elapsed >=
    /// NOTIFY_MIN_ELAPSED`, so a run that takes EXACTLY the threshold must
    /// notify. A value nowhere near the boundary (e.g. 60s) would still pass
    /// under an accidental `>` instead of `>=`; only a value pinned to the
    /// threshold itself distinguishes the two.
    #[test]
    fn notifies_when_unfocused_at_exactly_the_threshold() {
        assert!(should_notify(NOTIFY_MIN_ELAPSED, false));
    }

    #[test]
    fn notifies_when_unfocused_just_over_the_threshold() {
        assert!(should_notify(
            NOTIFY_MIN_ELAPSED + Duration::from_millis(1),
            false
        ));
    }

    #[test]
    fn never_notifies_when_the_window_is_focused_no_matter_how_long_it_took() {
        assert!(!should_notify(NOTIFY_MIN_ELAPSED * 100, true));
    }

    // --- `count_keepers`: mirrors `keepers()` in `app/types/features.ts`
    // (drop utility shots, keep the sharpest -- tie-broken by aesthetic --
    // photo per near-duplicate cluster), so the notification body can report
    // a keeper count without a JS/Rust boundary to call through.

    fn kept_candidate(
        is_utility: bool,
        cluster: u64,
        sharpness: f64,
        aesthetic: f64,
    ) -> serde_json::Value {
        serde_json::json!({
            "isUtility": is_utility,
            "nearDupCluster": cluster,
            "sharpnessPct": sharpness,
            "aestheticPct": aesthetic,
        })
    }

    #[test]
    fn empty_input_has_no_keepers() {
        assert_eq!(count_keepers(&[]), 0);
    }

    #[test]
    fn utility_photos_are_never_keepers() {
        let photos = vec![kept_candidate(true, 0, 100.0, 100.0)];
        assert_eq!(count_keepers(&photos), 0);
    }

    #[test]
    fn keeps_one_photo_per_near_duplicate_cluster() {
        let photos = vec![
            kept_candidate(false, 0, 10.0, 0.0),
            kept_candidate(false, 0, 20.0, 0.0), // sharper, same cluster -> wins
            kept_candidate(false, 1, 5.0, 0.0),  // different cluster -> also kept
        ];
        assert_eq!(count_keepers(&photos), 2);
    }

    #[test]
    fn ties_on_sharpness_are_broken_by_aesthetic_percentile() {
        let photos = vec![
            kept_candidate(false, 0, 50.0, 10.0),
            kept_candidate(false, 0, 50.0, 90.0), // same sharpness, higher aesthetic
        ];
        // Still one keeper per cluster regardless of which one wins; the
        // count itself doesn't reveal the tie-break, but this at least
        // pins that a tie doesn't produce two keepers.
        assert_eq!(count_keepers(&photos), 1);
    }
}
