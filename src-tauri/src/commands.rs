use crate::agent;
use crate::agent::keys::{self, KeyStatus, KeyStore, KeychainStore, Provider};
use crate::agent::request::{ModelEvent, ModelRequestError, ModelRequests, Outbound};
use crate::agent::view::{AgentView, SourcePhoto};
use crate::book::cull::{Overrides, Photo};
use crate::book::manifest::{manifest, Manifest};
use crate::book::pace::Book;
use crate::book::pack::{recommend_pages, Capacity};
use crate::book::preflight::{Finding, Severity};
use crate::export::build_items;
use crate::preview::BookLayout;
use crate::project::{ExportRecord, Project};
use crate::protocol::ExportItem;
use crate::templates::{Library, Weights};
use crate::{cluster, db::Db, ranking, sidecar::SidecarPool};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
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

/// Every supported image under any of `folders`, recursively, sorted by
/// path and listed once even when one folder lies inside another.
///
/// Recursive because real photo exports are nested -- a camera import is a
/// tree of dated folders, and a scan that stopped at the top level analysed
/// nothing and reported "no photos found" for a folder full of them. Several
/// roots because a book is often two or three folders' worth (a trip across
/// a phone and a camera), and the union is one population: every photo is
/// ranked against all the others the book draws from.
///
/// Two kinds of entry are skipped on purpose. Hidden directories (a leading
/// `.`), because `.Trashes`, `.Spotlight-V100` and their kind on a memory
/// card hold nothing the user meant to print. Directory SYMLINKS, because
/// following them can loop forever; a symlinked file still counts, as it
/// always did. An unreadable entry inside a folder is logged and skipped
/// rather than failing the whole scan; a root that cannot be read at all is
/// an error, because a missing folder must not read as an empty one.
pub(crate) fn collect_photo_paths(folders: &[String]) -> std::io::Result<Vec<String>> {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    log::warn!("skipping unreadable directory entry in {}: {err}", dir.display());
                    continue;
                }
            };
            let path = entry.path();
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            // `file_type` does not follow symlinks, which is the point: a
            // symlinked directory is neither descended nor listed.
            let is_real_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_real_dir {
                if !name.starts_with('.') {
                    if let Err(err) = walk(&path, out) {
                        log::warn!("skipping unreadable directory {}: {err}", path.display());
                    }
                }
            } else if path.is_file() && !is_apple_double(name) && supported_extension(name) {
                out.insert(path.to_string_lossy().into_owned());
            }
        }
        Ok(())
    }
    let mut paths = BTreeSet::new();
    for folder in folders {
        walk(Path::new(folder), &mut paths)?;
    }
    Ok(paths.into_iter().collect())
}

/// Trims a user-supplied project name and rejects an effectively-empty one.
/// Extracted so the validation is reachable from a unit test without a live
/// `AppHandle` -- `rename_project` needs one to open the database, but the
/// decision "was anything actually typed" does not depend on it at all.
pub(crate) fn normalize_project_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("a photobook needs a name".to_string());
    }
    Ok(trimmed.to_string())
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

/// The photo set one `analyze_folders` run produced, and which run it was.
///
/// `run_id` is the identity check that used to be missing: the override
/// toggle and the length chooser answer from this cache, and without the id
/// a second analysis would silently answer questions about the set that was
/// on screen from the set that replaced it -- keeper marks and the keeper
/// count become those of a different folder while the generated book stays
/// right, which is harder to notice, not easier. Every reader hands the id
/// its own summary carried, and a mismatch is refused, never answered.
#[derive(Default)]
pub struct AnalysedSet {
    /// `0` before any analysis has finished; never a real run's id.
    pub run_id: u64,
    pub photos: Vec<Photo>,
}

#[derive(Default)]
pub struct AppState {
    pub pool: Mutex<SidecarPool>,
    /// The photo set the contact sheet is currently showing, parsed once at
    /// the end of `analyze_folders`, together with the run that produced it.
    ///
    /// **This exists to keep the override toggle cheap.** Without it, every
    /// click has to ship the whole analysed array up to Rust so the verdict
    /// can be recomputed -- and those records carry faces (14-point lip
    /// contours), palettes, saliency boxes and EXIF, which measures around
    /// 2.5 KB per photo. A 1000-photo folder is then a ~10 MB upload per
    /// click, per command, which is not a slow feature, it is an unusable
    /// one. With the set cached here the toggle sends only the override map
    /// and gets back only the surviving paths.
    pub analysed: Mutex<AnalysedSet>,
    /// Minted once per `analyze_folders` call, before any work starts, so two
    /// overlapping runs get distinct ids and the later one wins the cache.
    runs: std::sync::atomic::AtomicU64,
}

impl AppState {
    pub fn next_run_id(&self) -> u64 {
        self.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }
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
    /// Which analysis produced this set. Handed back with every command
    /// that answers from the cached photos -- see `AnalysedSet`.
    pub run_id: u64,
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
/// the task brief calls this out by name as the thing not to do.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AnalysisEvent {
    /// Fired once, right after the folder is scanned, before any analysis
    /// starts -- gives the frontend the denominator for a determinate
    /// progress bar.
    Scanned { total: usize },
    /// Fired once per resolved chunk of `sidecar::chunk_paths_ramped` (cache
    /// hits within that chunk are one event; that chunk's cache misses, once
    /// the sidecar resolves them, are another) -- see `gather_chunked`. The
    /// ramp starts small (4 paths) so the first event fires after hashing a
    /// handful of files rather than the whole folder.
    Batch {
        /// Only the photos that finished in THIS batch -- callers append
        /// these, never replace with them.
        photos: Vec<serde_json::Value>,
        /// Cumulative running totals as of THIS event, NOT per-batch
        /// deltas -- read the latest event's value directly (e.g. for
        /// "analysed X / Y"), never sum these fields across events. Summing
        /// double-counts every photo already reflected in an earlier
        /// `Batch`. Mirrored in `BatchEvent` in `app/types/features.ts`.
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
    let event_ids = cluster::event_clusters(&times, cluster::EVENT_GAP_SECONDS);

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

    let refused = stamp_kept(&mut ok);
    if refused > 0 {
        log::warn!(
            "{refused} analysed photo(s) were unparseable and are marked not-kept; \
             the book will not contain them"
        );
    }
    ok
}

/// Stamps each record with `kept`: the verdict of `book::cull::cull`, which
/// is the SINGLE authority on which photo survives.
///
/// Why the verdict travels on the wire rather than the rule being reproduced
/// in the webview: before this, TypeScript's `keepers()` re-derived the same
/// rule independently and the two ranked differently -- Rust breaks a
/// sharpness tie on face capture quality and only then on aesthetic,
/// TypeScript went straight from sharpness to aesthetic. The contact sheet
/// could therefore show a different set of survivors from the one the book
/// was built out of, in both count and identity. The webview cannot fix that
/// by copying the Rust rule more carefully, because `AnalyzedPhoto` does not
/// even carry `captureQuality` -- the tie-break input is not on the wire.
/// Sending the ANSWER instead of the inputs removes the second implementation
/// entirely; `keepers()` in `app/types/features.ts` is now a filter on this
/// flag and contains no ranking of its own.
///
/// Must run AFTER the loop above: `cull` reads `nearDupCluster`,
/// `sharpnessPct` and `aestheticPct`, none of which exist on the record until
/// that loop stamps them.
///
/// Keyed by `path`, which is unique per record here (`analyze_folder` walks
/// one directory, and `lookup_cache` rewrites every cached record's path to
/// the current one precisely so two byte-identical files never share a path).
/// `book::pace::assemble` already keys the same way.
///
/// A record `from_features` cannot parse (a missing `path`, `hash`, `width`
/// or `height`) is stamped `kept: false`, which is honest: the book builder
/// would skip that photo for the same reason.
///
/// Returns how many records `from_features` REFUSED. A refusal is not a
/// benign skip: the photo comes back `kept: false`, which is
/// indistinguishable from the engine having culled it, so the count has to
/// reach someone who can report it.
pub(crate) fn stamp_kept(ok: &mut [serde_json::Value]) -> usize {
    let parsed: Vec<crate::book::cull::Photo> =
        ok.iter().filter_map(crate::book::cull::from_features).collect();
    let refused = ok.len() - parsed.len();

    // No overrides: this runs while the analysis is still finishing, before
    // the contact sheet exists, so there is nothing the user can have decided
    // yet. Their decisions are applied afterwards by `kept_paths`, through
    // the same `cull`.
    let kept: std::collections::HashSet<String> =
        kept_paths(&parsed, &Overrides::new()).into_iter().collect();

    for features in ok.iter_mut() {
        let survives = features["path"].as_str().is_some_and(|p| kept.contains(p));
        features["kept"] = survives.into();
    }
    refused
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

/// Counts surviving photos for the completion notification.
///
/// Delegates to `book::cull::cull` rather than reimplementing the rule.
/// Before Phase 2 there were two implementations of "which photo survives"
/// -- this one and TypeScript's `keepers()` -- and nothing kept them in
/// agreement. The notification is now guaranteed to report the same number
/// the book is built from.
///
/// Returns `(keepers, refused)`. See `stamp_kept` on why the refusal count
/// is returned rather than swallowed: an undercounted notification is
/// indistinguishable from a folder with fewer good photos in it.
pub(crate) fn count_keepers(photos: &[serde_json::Value]) -> (usize, usize) {
    let parsed: Vec<crate::book::cull::Photo> =
        photos.iter().filter_map(crate::book::cull::from_features).collect();
    let refused = photos.len() - parsed.len();
    // No overrides: this counts the ENGINE's verdict for the completion
    // notification, which fires the moment analysis finishes -- before the
    // contact sheet has been shown, so before the user can have made a
    // decision about anything on it.
    (crate::book::cull::cull(&parsed, &Overrides::new()).len(), refused)
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

/// Outcome of `gather_chunked`: the fully-gathered `ok` records (cache hits
/// and freshly-analysed records interleaved in chunk-arrival order, NOT yet
/// sorted or derived -- `finalize_photos` still owns that, once, over the
/// complete set) plus the final running totals `analyze_folder` needs for
/// its `AnalysisSummary`.
pub(crate) struct GatherResult {
    pub ok: Vec<serde_json::Value>,
    pub cached: usize,
    pub failed: usize,
}

/// The chunked incremental gather that fixes the "0 / 283 frozen for ~14
/// seconds" stall: instead of hashing and cache-looking-up every path in the
/// folder (a SHA-256 pass over the entire folder's bytes) before the first
/// sidecar batch -- or the first UI update -- ever runs, this processes
/// `paths` in `sidecar::chunk_paths_ramped` chunks and, for EACH chunk in
/// turn: hashes + cache-looks-up just that chunk, streams its cache hits via
/// `on_progress`, hands its cache misses to `analyze`, then streams the
/// sidecar's results via `on_progress` too -- before moving to the next
/// chunk. The ramp starts at 4 paths, so the very first `on_progress` call
/// happens after hashing a handful of files rather than the whole folder;
/// later chunks grow to `sidecar::BATCH_SIZE` to amortise sidecar
/// round-trips once the user already has tiles on screen.
///
/// `analyze` is the sidecar call for one chunk's cache misses -- pluggable
/// so this whole function is unit-testable without a live AppHandle/sidecar,
/// the same reasoning as `sidecar::analyze_batches_with_progress` (which the
/// real caller's `analyze` closure delegates to via `SidecarPool::analyze_all`).
/// It must return exactly one record per input path, real or synthetic
/// `failed` -- a contract `analyze_batches_with_progress` already guarantees
/// for the real implementation.
///
/// Deliberately does NOT sort, cluster, or rank -- `ok` is handed to
/// `finalize_photos` afterwards to do that once, over the complete set,
/// exactly as before this change. Only the *gathering* became incremental;
/// a rank computed against a partial population would be wrong, which is
/// why `on_progress` only ever receives `partial_photo` projections that
/// carry no rank at all.
pub(crate) fn gather_chunked(
    db: &Db,
    paths: &[String],
    mut analyze: impl FnMut(&[String]) -> Vec<serde_json::Value>,
    mut on_progress: impl FnMut(Vec<serde_json::Value>, usize, usize, usize),
) -> Result<GatherResult, String> {
    let mut ok = Vec::with_capacity(paths.len());
    let mut cached = 0usize;
    let mut analysed = 0usize;
    let mut failed = 0usize;

    for chunk in crate::sidecar::chunk_paths_ramped(paths) {
        let CacheLookup {
            hits,
            misses,
            hash_failures,
        } = lookup_cache(db, &chunk)?;

        // Cache hits (and hash failures, discovered in the same pass) are
        // known the instant `lookup_cache` returns for THIS chunk -- no
        // sidecar round-trip needed -- so they stream immediately, ahead of
        // whatever the sidecar produces for this chunk's misses. A chunk
        // that turns out to be entirely cache hits (no misses at all) still
        // reaches this branch and still reports progress -- it just never
        // reaches the `analyze` call below.
        cached += hits.len();
        failed += hash_failures;
        if !hits.is_empty() || hash_failures > 0 {
            let photos = hits.iter().map(partial_photo).collect();
            on_progress(photos, analysed, cached, failed);
        }
        ok.extend(hits);

        if misses.is_empty() {
            continue;
        }

        let records = analyze(&misses);

        // Cache-write and collect the full-feature records `finalize_photos`
        // needs, in one pass over `records`; `batch_progress` (the same pure
        // ok/failed split `analyze_folder` used to call directly) derives
        // the partial-photo projections and failure count from the SAME
        // records right below, so the two views can never disagree about
        // which records counted as ok.
        let mut fresh = Vec::with_capacity(records.len());
        for record in &records {
            if record["status"] == "ok" {
                let features = record["features"].clone();
                if let (Some(hash), Some(path)) =
                    (features["hash"].as_str(), features["path"].as_str())
                {
                    // A failed cache write must not discard a photo the
                    // sidecar already spent potentially multi-minute
                    // analysis producing -- it just means this photo won't
                    // be a cache hit next run. Logged and swallowed, same
                    // pattern as `analyze_folder`'s notification failure.
                    if let Err(err) = db.put_features(hash, path, &features.to_string()) {
                        log::warn!("failed to write cache entry for {path}: {err}");
                    }
                }
                fresh.push(features);
            }
        }

        let (photos, batch_failed) = batch_progress(&records);
        analysed += photos.len();
        failed += batch_failed;
        on_progress(photos, analysed, cached, failed);
        ok.extend(fresh);
    }

    Ok(GatherResult { ok, cached, failed })
}

#[tauri::command]
pub async fn analyze_folders(
    app: AppHandle,
    folders: Vec<String>,
    on_event: Channel<AnalysisEvent>,
) -> Result<AnalysisSummary, String> {
    let started = Instant::now();
    if folders.is_empty() {
        return Err("choose at least one folder".into());
    }
    let run_id = app.state::<AppState>().next_run_id();

    let paths = collect_photo_paths(&folders).map_err(|e| e.to_string())?;

    // A send failure (e.g. the webview navigated away mid-run) must not fail
    // an analysis that would otherwise succeed -- logged and swallowed, same
    // pattern as the completion notification below.
    if let Err(err) = on_event.send(AnalysisEvent::Scanned { total: paths.len() }) {
        log::warn!("failed to send Scanned event: {err}");
    }

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let db_path = database_path(&app)?;

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

    // `gather_chunked` hashes, cache-looks-up, and dispatches to the sidecar
    // one small chunk of paths at a time (see `sidecar::chunk_paths_ramped`)
    // instead of hashing the whole folder before anything can stream --
    // that upfront hash pass is what used to leave the UI at "0 / 283" for
    // ~14 seconds on a folder of 24MB RAWs. `Sidecar::request` (called
    // inside `analyze_all`, itself called from `analyze`'s closure below)
    // does a *blocking* `std::sync::mpsc::Receiver::recv_timeout` while it
    // waits for the sidecar's stdout-drain task -- a tokio task spawned in
    // `Sidecar::spawn` -- to hand it the response line. Both that blocking
    // wait and the drain task need to run on the tauri/tokio async worker
    // pool. Running the WHOLE gather (hashing included, also blocking I/O)
    // inline here, on the same pool this `async fn` itself runs on, can
    // starve the drain task of a thread to run on: on a machine with few
    // worker threads, a request then resolves only once its OWN timeout
    // elapses, even though the sidecar already responded. See
    // `.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/thumbnails-report.md`
    // for the traced root cause. `spawn_blocking` moves the entire gather
    // onto tokio's separate blocking-thread pool, leaving the async worker
    // pool free for the drain task throughout the run, not just during the
    // sidecar calls.
    let GatherResult {
        ok: gathered,
        cached,
        failed,
    } = {
        let app_for_pool = app.clone();
        let on_event_for_pool = on_event.clone();
        let paths_for_pool = paths.clone();
        let db_path_for_pool = db_path.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<GatherResult, String> {
            let db = Db::open(&db_path_for_pool).map_err(|e| e.to_string())?;
            let state = app_for_pool.state::<AppState>();
            let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
            gather_chunked(
                &db,
                &paths_for_pool,
                |misses| pool.analyze_all(&app_for_pool, misses, &thumbnail_dir, |_| {}),
                |photos, analysed_now, cached_now, failed_now| {
                    if let Err(err) = on_event_for_pool.send(AnalysisEvent::Batch {
                        photos,
                        analysed: analysed_now,
                        cached: cached_now,
                        failed: failed_now,
                    }) {
                        log::warn!("failed to send Batch event: {err}");
                    }
                },
            )
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let ok = finalize_photos(gathered);

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
        let (keepers, refused) = count_keepers(&ok);
        if refused > 0 {
            log::warn!(
                "{refused} analysed photo(s) were unparseable and are excluded from the \
                 keeper count in this notification"
            );
        }
        let body = format!("{} photos analysed, {} keepers", ok.len(), keepers);
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

    // Cached BEFORE the summary is handed over, so the first override toggle
    // cannot race the analysis that produced the photos it names. A parse
    // failure here is not fatal: it costs the cheap override path, not the
    // analysis, and `photos_from_records` reports the same failure loudly at
    // the point a book is actually generated.
    match photos_from_records(&ok) {
        Ok(parsed) => match app.state::<AppState>().analysed.lock() {
            // A run that finished after a newer one started must not put the
            // older set back; the newer run's summary is what the screen has.
            Ok(mut cache) if cache.run_id < run_id => {
                *cache = AnalysedSet { run_id, photos: parsed }
            }
            Ok(_) => {}
            Err(err) => log::warn!("photo cache is poisoned, overrides will be slow: {err}"),
        },
        Err(err) => log::warn!("cannot cache the analysed photo set: {err}"),
    }

    let summary = AnalysisSummary {
        run_id,
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

// =====================================================================
// Phase 2: recommend a book, generate and save one, export it.
//
// Every type in this section crosses to the webview, where NOTHING checks
// it: `AnalysisSummary.photos` is untyped `serde_json::Value` and each
// command's return shape is hand-mirrored by an interface in
// `app/types/book.ts`. A field renamed on one side and not the other is a
// silent `undefined` at runtime, not a build error. Each one is therefore
// pinned by name against a committed fixture in `tests/fixtures/wire/` that
// `tests/book.test.ts` reads independently -- see that directory's README
// for why a Rust-to-Rust round-trip cannot do this job.
// =====================================================================

/// The page lengths the user can choose between: Pixajoy's two published
/// SKUs for this product. Offered as a list (rather than a `recommended` plus
/// a free-form number) because a page count that is not a real SKU cannot be
/// ordered, so an arbitrary override would be a way to build an unbuyable
/// book.
pub(crate) const PAGE_OPTIONS: [u32; 2] = [20, 40];

/// One page length the user can pick, and what picking it costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageOption {
    pub pages: u32,
    /// The most photos this length can hold, from `Capacity::from_library` --
    /// the accurate figure, with the two single pages bounded by a page-half
    /// rather than by a whole spread.
    pub capacity_photos: usize,
    /// How many keepers this length would leave out. The number the user is
    /// actually deciding on.
    pub dropped_photos: usize,
    /// How many photos the user explicitly marked `Include` that this length
    /// cannot hold. Non-zero means this length cannot be generated at all --
    /// see `book::pack::IncludeOverflow`.
    pub included_over_capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookRecommendation {
    /// Survivors of `book::cull::cull` -- utility images dropped, one photo
    /// per near-duplicate cluster. NOT the raw analysed count.
    pub keeper_count: usize,
    /// How many of these photos the user explicitly marked `Include`. Zero
    /// when they have made no decisions, which is every book generated before
    /// this feature existed.
    pub included_count: usize,
    pub recommended_pages: u32,
    pub options: Vec<PageOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedBook {
    /// The row `generate_book` wrote. Everything downstream (export, the
    /// project list, reopening) is keyed by this.
    pub project_id: i64,
    pub page_count: usize,
    pub placed_photos: usize,
    pub dropped_photos: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFailure {
    /// The output basename (no extension) the item would have been written
    /// under -- the only identifier a `.failed` `ExportRecord` carries.
    pub filename: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// True when pre-flight found a `Block` and NOTHING was written.
    pub blocked: bool,
    pub output_dir: String,
    /// Reported separately from `warnings` rather than as one list the UI
    /// filters: these two are not the same kind of thing (one stopped the
    /// export, the other did not), and a UI that has to re-derive which is
    /// which is a UI that can get it backwards.
    pub blocking: Vec<Finding>,
    pub warnings: Vec<Finding>,
    /// Absolute paths the sidecar actually wrote, with the extension IT
    /// chose from the source container.
    pub written: Vec<String>,
    pub failures: Vec<ExportFailure>,
    pub manifest_path: Option<String>,
    /// Why `manifest.json` could not be written, when it could not be.
    ///
    /// A manifest failure NEVER fails the export: by the time it is written
    /// every file is already on disk, and turning that into an error would
    /// tell the user the export failed while leaving nineteen files
    /// somewhere they were never told about. Reported alongside a result
    /// that still names every path written.
    pub manifest_error: Option<String>,
    /// `"jpg"`, `"png"`, `"mixed"`, or empty when nothing was written.
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub at: i64,
    pub output_dir: String,
    pub format: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListItem {
    pub id: i64,
    pub name: String,
    /// The first folder, kept for the list's one-line label and for every
    /// row saved before books could draw from several.
    pub source_folder: String,
    pub source_folders: Vec<String>,
    pub page_count: i64,
    pub photo_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// The most recent export, or `None` for a book that has never been
    /// exported -- the distinction the project list exists to show.
    pub last_export: Option<ExportSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub id: i64,
    pub name: String,
    pub source_folder: String,
    /// Every folder the book was analysed from, so "edit the selection" can
    /// re-analyse exactly the set it was generated over.
    pub source_folders: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub page_count: usize,
    pub photo_count: usize,
    pub dropped_photos: usize,
    pub seed: u64,
    /// The include/exclude decisions this book was generated with, so
    /// reopening a project restores them rather than quietly reverting every
    /// one to `Auto`.
    pub overrides: Overrides,
    pub exports: Vec<ExportSummary>,
}

/// Streamed over a `tauri::ipc::Channel` while `export_book` runs, the same
/// mechanism and for the same reason as `AnalysisEvent`: an export is a
/// full-size decode, crop, colour conversion and re-encode per photo, so a
/// sixty-photo book is minutes of work with nothing on screen otherwise.
///
/// `Started` fires only once pre-flight has PASSED -- a blocked export writes
/// nothing and has no progress to report, and showing a progress bar that
/// never advances would misreport it as a stall.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ExportEvent {
    Started { total: usize },
    /// Cumulative as of THIS event, not a delta -- same convention as
    /// `AnalysisEvent::Batch`'s running totals.
    Progress { completed: usize, total: usize },
}

/// Where the SQLite database lives. One authority, shared by every command,
/// so a second one can never open a different file.
pub(crate) fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("photobook.sqlite"))
}

/// The bundled template directory, with a source-tree fallback for `tauri
/// dev`.
///
/// `templates/` ships via `bundle.resources` (see `tauri.conf.json`), so the
/// packaged app resolves it under `resource_dir()`. The fallback is the
/// repository copy, resolved from `CARGO_MANIFEST_DIR` at compile time: in a
/// shipped `.app` that path does not exist on the user's machine, which is
/// exactly why it is guarded by an existence check rather than tried first.
fn template_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(resources) = app.path().resource_dir() {
        let bundled = resources.join("templates");
        if bundled.is_dir() {
            return Ok(bundled);
        }
    }
    let source_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
    if source_tree.is_dir() {
        return Ok(source_tree);
    }
    Err("template library not found in the app resources".into())
}

fn load_library(app: &AppHandle) -> Result<Library, String> {
    Library::load(&template_dir(app)?).map_err(|e| e.to_string())
}

/// Soft-term weights, hot-reloadable from `templates/weights.json`.
///
/// A missing or malformed weights file degrades to `Weights::default()` with
/// a logged warning rather than failing generation: the defaults are a
/// working set of weights, and refusing to build a book because a taste-tuning
/// file did not parse would be a worse outcome than building it with the
/// defaults.
fn load_weights(app: &AppHandle) -> Weights {
    let Ok(dir) = template_dir(app) else {
        return Weights::default();
    };
    match Weights::load(&dir.join("weights.json")) {
        Ok(weights) => weights,
        Err(err) => {
            log::warn!("falling back to default weights: {err}");
            Weights::default()
        }
    }
}

/// Parses the webview's analysed-photo records into engine photos, preserving
/// positional correspondence exactly.
///
/// Fails the whole call on the first record it cannot parse, rather than
/// skipping it. This is the opposite of `count_keepers`'s `filter_map`, and
/// deliberately so: `Placement::photo_index` indexes into the slice this
/// returns, so dropping one record silently shifts every later photo down an
/// index -- and the book would then export, and manifest, the wrong source
/// file for every placement after it, with no error raised anywhere.
pub(crate) fn photos_from_records(records: &[serde_json::Value]) -> Result<Vec<Photo>, String> {
    records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            crate::book::cull::from_features(record).ok_or_else(|| {
                format!(
                    "photo {i} ({}) is missing fields the layout engine needs",
                    record["path"].as_str().unwrap_or("<no path>")
                )
            })
        })
        .collect()
}

/// Rebuilds the exact photo slice a saved book was assembled against, from
/// the content hashes persisted with it.
///
/// This is what makes a project durable: exporting no longer depends on the
/// webview still holding the same array in the same order. The features
/// cache is keyed by exactly these hashes, and the fields export and
/// pre-flight need -- `path`, `hash`, `width`/`height`, `faces`,
/// `saliencyBox` -- are all in the stored record. The percentile and cluster
/// fields are not (they are whole-set derivations `finalize_photos` never
/// persists), which is fine: nothing downstream of assembly reads them --
/// pre-flight and the exporter neither cull, rank nor chapter.
///
/// Hence `from_cached_features` rather than `from_features`: this is the ONE
/// call site where those fields are legitimately missing, so it is the one
/// place that says so. Everywhere else an absent `nearDupCluster` collapses
/// the whole book to a single photo, and `from_features` now refuses the
/// record instead.
///
/// **All or nothing.** A single unresolvable hash fails the call. Returning
/// a shorter list would renumber every `photo_index` after the gap, and the
/// export would then write one photo's crop under another photo's name --
/// silently. The two ways a hash stops resolving are a cache that was
/// cleared and an `ANALYZER_VERSION` bump (`Db::get_features` filters on it,
/// so every row goes stale at once); both produce the same instruction,
/// because from the user's side they are the same problem.
pub(crate) fn resolve_photos(db: &Db, hashes: &[String]) -> Result<Vec<Photo>, String> {
    cached_records(db, hashes)?
        .iter()
        .map(|value| {
            crate::book::cull::from_cached_features(value).ok_or_else(|| {
                format!(
                    "A cached photo record is missing fields the layout engine needs -- {RESOLVE_REMEDY}."
                )
            })
        })
        .collect()
}

const RESOLVE_REMEDY: &str = "re-analyse the source folder, then generate the book again";

/// The raw cached feature records for a saved book's photo list, in order.
///
/// Split out of `resolve_photos` because the preview needs one field the
/// layout engine does not: `thumbnailPath`. `Photo` deliberately has no such
/// field -- nothing about assembling or exporting a book reads a thumbnail --
/// so rather than widening `Photo` for a display concern, both callers start
/// from the record and take what they need. See `resolve_photos` above for
/// why a single unresolvable hash fails the whole call.
fn cached_records(db: &Db, hashes: &[String]) -> Result<Vec<serde_json::Value>, String> {
    if hashes.is_empty() {
        return Err(format!(
            "This book was saved without its photo list (it predates that being recorded) -- {RESOLVE_REMEDY}."
        ));
    }

    let mut records = Vec::with_capacity(hashes.len());
    for hash in hashes {
        let json = db
            .get_features(hash)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "This book's photos are no longer in the analysis cache \
                     (either it was cleared, or the analyser has been updated since) -- {RESOLVE_REMEDY}."
                )
            })?;
        records.push(serde_json::from_str(&json).map_err(|e| e.to_string())?);
    }
    Ok(records)
}

/// The same photo slice `resolve_photos` returns, paired with each photo's
/// contact-sheet thumbnail, as the preview's `PreviewPhoto`s.
///
/// Order and length are preserved exactly, because
/// `Placement::photo_index` indexes into this list. A photo whose thumbnail
/// write failed comes back with `thumbnailPath: null` and is still present --
/// a missing thumbnail is an empty box in the preview, never a missing
/// placement, and dropping it would renumber every index after it.
pub(crate) fn resolve_preview_photos(
    db: &Db,
    hashes: &[String],
) -> Result<Vec<crate::preview::PreviewPhoto>, String> {
    let records = cached_records(db, hashes)?;
    records
        .iter()
        .map(|value| {
            let photo = crate::book::cull::from_cached_features(value).ok_or_else(|| {
                format!(
                    "A cached photo record is missing fields the layout engine needs -- {RESOLVE_REMEDY}."
                )
            })?;
            let thumbnail =
                value["thumbnailPath"].as_str().map(std::string::ToString::to_string);
            Ok(crate::preview::preview_photo(&photo, thumbnail))
        })
        .collect()
}

/// The recommended book length and what each length would cost.
pub(crate) fn recommend(
    photos: &[Photo],
    lib: &Library,
    overrides: &Overrides,
) -> BookRecommendation {
    let keeper_count = crate::book::cull::cull(photos, overrides).len();
    let included_count = overrides.included_in(photos);
    let options = PAGE_OPTIONS
        .iter()
        .map(|&pages| {
            let capacity = Capacity::from_library(pages, lib);
            PageOption {
                pages,
                capacity_photos: capacity.max_photos,
                // Saturating: a book with room to spare drops nothing, and an
                // unsigned wrap-around here would report a colossal number.
                dropped_photos: keeper_count.saturating_sub(capacity.max_photos),
                // The figure that decides whether this length can be built at
                // all. `dropped_photos` above is a cost the user accepts; this
                // one is a refusal, because the engine will not choose which
                // of their own picks to discard.
                included_over_capacity: included_count.saturating_sub(capacity.max_photos),
            }
        })
        .collect();
    BookRecommendation {
        keeper_count,
        included_count,
        recommended_pages: recommend_pages(keeper_count, lib),
        options,
    }
}

/// Splits pre-flight's findings into the ones that stop the export and the
/// ones that do not, preserving each list's original order.
pub(crate) fn split_findings(findings: Vec<Finding>) -> (Vec<Finding>, Vec<Finding>) {
    findings.into_iter().partition(|f| f.severity == Severity::Block)
}

fn placement_total(book: &Book) -> usize {
    book.pages.iter().map(|p| p.placements.len()).sum()
}

/// The length of the photo slice this book was assembled against.
///
/// That slice is not persisted with the book -- only the indices into it are
/// -- so re-exporting against a different set of photos would silently print
/// the wrong sources. `Book::dropped` is `photos.len() - distinct placed
/// indices` (`pace::assemble`), so the two together reconstruct the original
/// length, which is enough to REFUSE a mismatch. It does not detect a
/// same-length permutation; the UI never reorders `summary.photos`, and
/// `finalize_photos` sorts by path, so the ordering is stable within a run.
pub(crate) fn expected_photo_count(book: &Book) -> usize {
    let placed: BTreeSet<usize> = book
        .pages
        .iter()
        .flat_map(|p| p.placements.iter().map(|pl| pl.photo_index))
        .collect();
    book.dropped + placed.len()
}

/// What `generate_and_save` needs to know about the project it is about to
/// write, as one value rather than four positional arguments that are easy
/// to transpose (`name` and `source_folder` are both `&str`, and a swap
/// between them compiles).
pub(crate) struct NewProject<'a> {
    pub name: &'a str,
    /// Every folder the book draws from, first one first. See
    /// `Project::source_folders`.
    pub source_folders: Vec<String>,
    pub pages: u32,
    pub seed: u64,
}

/// Assembles a book and PERSISTS it, returning the new project's id.
///
/// The persistence is not an optimisation: a generated book the user cannot
/// find again after quitting is a book they have lost. Saving here, rather
/// than in a separate "save" command the UI has to remember to call, is what
/// makes that structurally impossible.
pub(crate) fn generate_and_save(
    db: &Db,
    meta: &NewProject<'_>,
    photos: &[Photo],
    lib: &Library,
    weights: &Weights,
    overrides: &Overrides,
) -> Result<GeneratedBook, String> {
    // `assemble` refuses rather than returning a book that lost a photo the
    // user explicitly asked for. Surfaced as a command error, with the
    // numbers they need to fix it -- see `book::pack::IncludeOverflow`.
    let book = crate::book::pace::assemble(photos, meta.pages, lib, weights, meta.seed, overrides)
        .map_err(|e| e.to_string())?;
    // The hash of EVERY photo the book was assembled against, in that
    // slice's order -- `Placement::photo_index` indexes it positionally.
    // This is what makes the project exportable after a restart; see
    // `Project::photo_hashes`.
    let photo_hashes: Vec<String> = photos.iter().map(|p| p.hash.clone()).collect();
    let project_id = db
        .save_project_in(meta.name, &meta.source_folders, &book, &photo_hashes, overrides)
        .map_err(|e| e.to_string())?;
    Ok(GeneratedBook {
        project_id,
        page_count: book.pages.len(),
        placed_photos: placement_total(&book),
        dropped_photos: book.dropped,
        seed: book.seed,
    })
}

/// Maps each output basename the sidecar actually wrote to the extension it
/// wrote it under.
///
/// Keyed on the file STEM of the returned path, which is by construction the
/// `filename` the item was sent with (`Exporter.outputPath` appends the
/// extension to the stem it was given).
fn written_extensions(records: &[serde_json::Value]) -> BTreeMap<String, String> {
    records
        .iter()
        .filter(|r| r["type"] == "ok")
        .filter_map(|r| {
            let path = Path::new(r["path"].as_str()?);
            let stem = path.file_stem()?.to_str()?.to_string();
            // The extension is OPTIONAL on purpose. A record's presence means
            // the file was written; requiring an extension to keep the entry
            // would drop a real file out of the manifest because its name
            // happened to carry no container. Unknown is recorded as unknown.
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            Some((stem, extension))
        })
        .collect()
}

fn written_paths(records: &[serde_json::Value]) -> Vec<String> {
    records
        .iter()
        .filter(|r| r["type"] == "ok")
        .filter_map(|r| r["path"].as_str().map(str::to_string))
        .collect()
}

fn export_failures(records: &[serde_json::Value]) -> Vec<ExportFailure> {
    records
        .iter()
        .filter(|r| r["type"] != "ok")
        .map(|r| ExportFailure {
            filename: r["filename"].as_str().unwrap_or_default().to_string(),
            message: r["message"].as_str().unwrap_or("export failed").to_string(),
        })
        .collect()
}

/// Rewrites the manifest to describe what was ACTUALLY written.
///
/// `book::manifest::manifest` fills `format` from
/// `export::predicted_format`, which reads the source's file EXTENSION. The
/// Swift exporter decides the real container from the file itself, via
/// `CGImageSourceGetType` -- so a `.jpg` that is secretly a PNG makes the two
/// disagree, and the manifest, which is the record of what the user
/// uploaded, would name a file that does not exist. The returned
/// `ExportRecord.ok.path` carries the true extension; this reconciles against
/// it.
///
/// An entry with no successful record is REMOVED: it names no file, so
/// listing it (with any format at all) would be a claim about a file nobody
/// wrote. Pages themselves are never removed, preserving `manifest`'s own
/// rule that `page_count` always reconciles against the SKU. Returns how many
/// entries were dropped.
pub(crate) fn reconcile_manifest(m: &mut Manifest, records: &[serde_json::Value]) -> usize {
    let written = written_extensions(records);
    let mut dropped = 0usize;
    for page in &mut m.pages {
        page.photos.retain_mut(|photo| match written.get(&photo.filename) {
            Some(extension) => {
                photo.format.clone_from(extension);
                true
            }
            None => {
                dropped += 1;
                false
            }
        });
    }
    dropped
}

/// One label for the containers an export produced, for the project's export
/// history. `"mixed"` rather than picking a winner: a book of camera JPEGs
/// and scanned PNGs genuinely has both, and naming one of them would be
/// wrong about the other.
fn export_format_label(paths: &[String]) -> String {
    let extensions: BTreeSet<String> = paths
        .iter()
        .filter_map(|p| Path::new(p).extension()?.to_str().map(str::to_ascii_lowercase))
        .collect();
    match extensions.len() {
        0 => String::new(),
        1 => extensions.into_iter().next().expect("exactly one"),
        _ => "mixed".into(),
    }
}

/// Pre-flight first, then write -- with both side effects injected.
///
/// `send` (the sidecar round-trip) and `write_manifest` (the filesystem) are
/// parameters rather than calls, so the ordering guarantee this function
/// exists to make -- *nothing is written when a finding blocks* -- is
/// testable without a live `AppHandle`, by handing it closures that panic if
/// reached. That is the same "extract the part that can be tested" pattern
/// `finalize_photos`, `lookup_cache`, `gather_chunked` and
/// `analyze_batches_with_progress` already follow.
pub(crate) fn run_export<S, W>(
    book: &Book,
    photos: &[Photo],
    project_id: i64,
    output_dir: &str,
    findings: Vec<Finding>,
    send: S,
    write_manifest: W,
) -> Result<ExportResult, String>
where
    S: FnOnce(&[ExportItem]) -> Vec<serde_json::Value>,
    W: FnOnce(&Manifest) -> Result<String, String>,
{
    // Checked before anything else: `build_items` and `manifest` both index
    // `photos` by `photo_index` directly, so a slice that is not the one the
    // book was built against panics rather than misbehaves.
    let expected = expected_photo_count(book);
    if photos.len() != expected {
        return Err(format!(
            "this book was assembled from {expected} photos but {} were supplied; \
             re-analyse the source folder and generate the book again",
            photos.len()
        ));
    }

    let (blocking, warnings) = split_findings(findings);
    if !blocking.is_empty() {
        return Ok(ExportResult {
            blocked: true,
            output_dir: output_dir.to_string(),
            blocking,
            warnings,
            written: Vec::new(),
            failures: Vec::new(),
            manifest_path: None,
            manifest_error: None,
            format: String::new(),
        });
    }

    let items = build_items(book, photos);
    let records = send(&items);

    let written = written_paths(&records);
    let failures = export_failures(&records);

    // Built and reconciled AFTER the export, from the real returned paths --
    // see `reconcile_manifest`.
    let mut manifest = manifest(book, photos, project_id);
    let unwritten = reconcile_manifest(&mut manifest, &records);
    if unwritten > 0 {
        log::warn!("{unwritten} placement(s) failed to export and were left out of the manifest");
    }
    // Deliberately not `?`: the sidecar has already written every file, and
    // a failure here (read-only volume, full disk) must not discard an
    // export that succeeded. Reported instead, so the user still learns
    // where their files are.
    let (manifest_path, manifest_error) = match write_manifest(&manifest) {
        Ok(path) => (Some(path), None),
        Err(err) => {
            log::warn!("export succeeded but the manifest could not be written: {err}");
            (None, Some(err))
        }
    };

    Ok(ExportResult {
        blocked: false,
        output_dir: output_dir.to_string(),
        blocking,
        warnings,
        format: export_format_label(&written),
        written,
        failures,
        manifest_path,
        manifest_error,
    })
}

fn write_manifest_file(output_dir: &Path, manifest: &Manifest) -> Result<String, String> {
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let path = output_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// The photos that survive culling under the user's own decisions, by path.
///
/// **This exists so that no second copy of the culling rule appears in the
/// webview.** The overrides are made on the contact sheet, long after
/// `analyze_folder` stamped its verdict, and the sheet has to show their
/// effect immediately -- a photo the user includes must gain the keeper
/// marker, and the keeper count must move. The webview could compute that
/// itself in two lines, and that is precisely the mistake Phase 2 spent a
/// task undoing: `book::cull::cull` is the single authority, so the toggle
/// goes DOWN to Rust and the answer comes back. `keepers()` in TypeScript
/// stays a filter on the `kept` flag, with no rule of its own.
///
/// **It returns the verdict, not the records.** An earlier version took the
/// whole analysed array and handed it back re-stamped, which is ~2.5 KB per
/// photo in each direction -- around 20 MB of round trip per click on a
/// 1000-photo folder. The photo set is cached in `AppState` at the end of
/// `analyze_folder` instead, so the request is the override map alone and
/// the response is a list of paths.
///
/// **Paths, not hashes.** A content hash is NOT unique within one analysis --
/// two byte-identical files in a folder share one -- so a hash-keyed verdict
/// would mark both copies kept when `cull` kept only one of them. `path` is
/// unique per record here (`analyze_folder` walks one directory, and
/// `lookup_cache` rewrites every cached record's path to the current one
/// precisely so two identical files never share a path), which is why
/// `stamp_kept` and `pace::assemble` both key on it too. The OVERRIDES are
/// hash-keyed, because a decision is about the photograph; the VERDICT is
/// path-keyed, because it is about the file.
pub(crate) fn kept_paths(photos: &[Photo], overrides: &Overrides) -> Vec<String> {
    crate::book::cull::cull(photos, overrides).into_iter().map(|p| p.path).collect()
}

/// The analysed photo set cached by `analyze_folders` for run `run_id`, or
/// an error naming the remedy -- including when the cache holds a DIFFERENT
/// run, which is the collision that used to be answered silently.
fn cached_photos(app: &AppHandle, run_id: u64) -> Result<Vec<Photo>, String> {
    let state = app.state::<AppState>();
    let cache = state
        .analysed
        .lock()
        .map_err(|e| format!("the analysed photo set is unreadable: {e}"))?;
    select_run(&cache, run_id).map(<[Photo]>::to_vec)
}

/// The identity check itself, kept pure so it can be tested without an
/// `AppHandle`: the cached set answers only for the run that produced it.
pub(crate) fn select_run(cache: &AnalysedSet, run_id: u64) -> Result<&[Photo], String> {
    if cache.run_id == 0 || cache.photos.is_empty() {
        return Err("No analysed photos are loaded -- analyse a folder first.".into());
    }
    if cache.run_id != run_id {
        return Err(
            "The photos on screen come from a different analysis than the one loaded here. \
             Analyse the folder again."
                .into(),
        );
    }
    Ok(&cache.photos)
}

#[tauri::command]
pub async fn apply_photo_overrides(
    app: AppHandle,
    run_id: u64,
    overrides: Overrides,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(kept_paths(&cached_photos(&app, run_id)?, &overrides))
    })
        .await
        .map_err(|e| e.to_string())?
}

/// How long a book would be, and what each length costs.
///
/// Reads the photo set cached by `analyze_folders` rather than taking it from
/// the webview: this is re-run on every override toggle, and re-uploading
/// several megabytes of feature records to answer "how many keepers now?" is
/// what made the toggle unusable on a real folder. `generate_book` still
/// takes the array, because it runs once and is the call that must not depend
/// on a cache being warm.
#[tauri::command]
pub async fn recommend_book(
    app: AppHandle,
    run_id: u64,
    overrides: Option<Overrides>,
) -> Result<BookRecommendation, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = load_library(&app)?;
        let parsed = cached_photos(&app, run_id)?;
        Ok(recommend(&parsed, &lib, &overrides.unwrap_or_default()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Assembles a book and saves it as a project, returning its id.
#[tauri::command]
pub async fn generate_book(
    app: AppHandle,
    photos: Vec<serde_json::Value>,
    pages: Option<u32>,
    name: String,
    source_folders: Vec<String>,
    seed: Option<u64>,
    // `overrides` carries the user's own include/exclude decisions, held in
    // webview state from the moment they are made on the contact sheet until
    // this call -- which is what finally gives them somewhere durable to
    // live. `None` from a caller that has none means an empty map.
    overrides: Option<Overrides>,
) -> Result<GeneratedBook, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = load_library(&app)?;
        let weights = load_weights(&app);
        let parsed = photos_from_records(&photos)?;
        let overrides = overrides.unwrap_or_default();
        let pages =
            pages.unwrap_or_else(|| recommend_pages(crate::book::cull::cull(&parsed, &overrides).len(), &lib));
        // A seed the caller did not pin is taken from the clock, so
        // "generate again" genuinely re-rolls the tie-breaks instead of
        // rebuilding the identical book; it is then persisted with the book,
        // so re-opening one is reproducible.
        let seed = seed.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        });
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let meta = NewProject { name: &name, source_folders, pages, seed };
        generate_and_save(&db, &meta, &parsed, &lib, &weights, &overrides)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pre-flights a saved book and, if nothing blocks, exports it.
///
/// Takes NO photos from the webview. The photo slice is rebuilt from the
/// content hashes persisted with the project (`resolve_photos`), which is
/// what lets a book be exported after a restart -- and, just as importantly,
/// removes the hazard of the webview handing back a same-length-but-
/// different array after a re-analysis, which would have exported the old
/// book's crops against the new photos with no error anywhere.
///
/// The whole body runs inside `spawn_blocking`: `Sidecar::request` does a
/// BLOCKING `recv_timeout` while waiting for the sidecar's stdout-drain task
/// -- itself a tokio task -- to hand it the response. Running that on the
/// async worker pool starves the drain task of a thread on a machine with
/// few workers, and the request then resolves only when its own timeout
/// elapses, even though the sidecar already answered. This is a traced,
/// documented bug from Phase 1, not a precaution.
#[tauri::command]
pub async fn export_book(
    app: AppHandle,
    project_id: i64,
    output_dir: String,
    on_event: Channel<ExportEvent>,
) -> Result<ExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let project = db
            .load_project(project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project {project_id} no longer exists"))?;
        let parsed = resolve_photos(&db, &project.photo_hashes)?;
        let output = PathBuf::from(&output_dir);

        // Pre-flight reads the real world: it checks every source file still
        // exists and how much space the volume has. It runs BEFORE anything
        // is written, which is the entire point of it.
        //
        // `preflight`, not `preflight_with_bleed`: a persisted `Placement`
        // carries its slot rect but not the originating template's bleed
        // array, so there is no bleed declaration to check here and that one
        // check is trivially satisfied. Every other check runs in full. A
        // caller that reloads the template library and re-derives the per
        // placement bleed edges could call `preflight_with_bleed` instead --
        // that is a Phase 3 concern, once the preview needs the template
        // anyway.
        let findings = crate::book::preflight::preflight(&project.book, &parsed, &output);

        let total = placement_total(&project.book);
        let mut completed = 0usize;

        let result = run_export(
            &project.book,
            &parsed,
            project_id,
            &output_dir,
            findings,
            |items| {
                if let Err(err) = on_event.send(ExportEvent::Started { total }) {
                    log::warn!("failed to send export Started event: {err}");
                }
                let state = app.state::<AppState>();
                let mut pool = match state.pool.lock() {
                    Ok(pool) => pool,
                    Err(err) => {
                        log::warn!("sidecar pool is poisoned: {err}");
                        return items
                            .iter()
                            .map(|i| {
                                crate::sidecar::export_failure_record(&i.filename, "sidecar unavailable")
                            })
                            .collect();
                    }
                };
                pool.export_all(&app, &output_dir, items, |resolved| {
                    completed += resolved.len();
                    if let Err(err) =
                        on_event.send(ExportEvent::Progress { completed, total })
                    {
                        log::warn!("failed to send export Progress event: {err}");
                    }
                })
            },
            |manifest| write_manifest_file(&output, manifest),
        )?;

        // Recorded only for an export that actually wrote something: an
        // export history entry for a run that produced no files would show
        // the user a book as "exported" when nothing landed on disk.
        if !result.written.is_empty() {
            let record = ExportRecord {
                at: 0, // stamped by SQLite; see ExportRecord's doc comment.
                output_dir: result.output_dir.clone(),
                format: result.format.clone(),
                file_count: result.written.len(),
            };
            if let Err(err) = db.record_export(project_id, &record) {
                // The files are already written; failing the command now
                // would tell the user the export failed when it did not.
                log::warn!("failed to record export against project {project_id}: {err}");
            }
        }

        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Every saved project, newest-updated first, each with its most recent
/// export.
///
/// `Db::list_projects` deliberately does not parse `book_json`, but it also
/// does not carry export history, so each row's exports are fetched with a
/// `load_project`. That parses the book per row, which is wasteful in
/// principle and irrelevant in practice at the scale a person's project list
/// actually reaches; worth a dedicated query only if it ever stops being
/// instant.
#[tauri::command]
pub async fn list_projects(app: AppHandle) -> Result<Vec<ProjectListItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let summaries = db.list_projects().map_err(|e| e.to_string())?;
        let mut items = Vec::with_capacity(summaries.len());
        for summary in summaries {
            // Degrades rather than propagates: one project whose `book_json`
            // no longer parses must not make the whole list unreadable, which
            // would leave the user with no way to see (or delete) any of their
            // other books.
            let last_export = match db.load_project(summary.id) {
                Ok(project) => project,
                Err(err) => {
                    log::warn!("cannot read project {}: {err}", summary.id);
                    None
                }
            }
            .and_then(|project| project.exports.last().cloned())
                .map(|record| ExportSummary {
                    at: record.at,
                    output_dir: record.output_dir,
                    format: record.format,
                    file_count: record.file_count,
                });
            items.push(ProjectListItem {
                id: summary.id,
                name: summary.name,
                source_folder: summary.source_folder,
                source_folders: summary.source_folders,
                page_count: summary.page_count,
                photo_count: summary.photo_count,
                created_at: summary.created_at,
                updated_at: summary.updated_at,
                last_export,
            });
        }
        Ok(items)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A loaded `Project` as the webview's `ProjectDetail`.
///
/// Extracted from the command body rather than written inline for the reason
/// this codebase extracts `finalize_photos`, `lookup_cache` and `percentiles`:
/// `open_project` needs a live `AppHandle`, so nothing inside it is reachable
/// from a unit test. The mapping is not clerical -- `overrides` is the user's
/// own photo selection, and dropping it here would silently revert every
/// decision they made to `Auto` while the reopened project still looked
/// entirely correct.
pub(crate) fn project_detail(project: Project) -> ProjectDetail {
    ProjectDetail {
        id: project.id,
        name: project.name,
        source_folder: project.source_folder,
        source_folders: project.source_folders,
        created_at: project.created_at,
        updated_at: project.updated_at,
        page_count: project.book.pages.len(),
        photo_count: placement_total(&project.book),
        dropped_photos: project.book.dropped,
        seed: project.book.seed,
        overrides: project.overrides,
        exports: project
            .exports
            .into_iter()
            .map(|record| ExportSummary {
                at: record.at,
                output_dir: record.output_dir,
                format: record.format,
                file_count: record.file_count,
            })
            .collect(),
    }
}

/// One saved project in full, for reopening it.
#[tauri::command]
pub async fn open_project(app: AppHandle, id: i64) -> Result<ProjectDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let project = db
            .load_project(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project {id} no longer exists"))?;
        Ok(project_detail(project))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The saved book's layout, for the read-only preview.
///
/// Keyed by project id and read from the database rather than taken from the
/// webview, so it works identically for a book generated moments ago (which
/// `generate_book` has already saved) and one reopened from disk after a
/// restart. That symmetry is most of the feature's value: a reopened project
/// carries its full layout, and until now nothing could show it.
///
/// Deliberately a SEPARATE command from `open_project` rather than a field on
/// `ProjectDetail`: a freshly generated book never goes through
/// `open_project` at all, so a layout hung off that type would be missing for
/// exactly the case the user is looking at while they test.
#[tauri::command]
pub async fn book_layout(app: AppHandle, project_id: i64) -> Result<BookLayout, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let project = db
            .load_project(project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project {project_id} no longer exists"))?;
        let photos = resolve_preview_photos(&db, &project.photo_hashes)?;
        let lib = load_library(&app)?;
        Ok(crate::preview::book_layout(project.id, &project.book, photos, &lib))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The saved book as the agent may see it -- see `agent::view`.
///
/// Reads the cached records directly rather than going through
/// `resolve_photos`, because the view ranks from the raw `aestheticScore` and
/// `sharpness` that `Photo` does not keep.
#[tauri::command]
pub async fn agent_view(app: AppHandle, project_id: i64) -> Result<AgentView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let project = db
            .load_project(project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project {project_id} no longer exists"))?;
        let photos = cached_records(&db, &project.photo_hashes)?
            .iter()
            .map(|record| {
                SourcePhoto::from_record(record).ok_or_else(|| {
                    format!(
                        "A cached photo record is missing fields the agent needs -- {RESOLVE_REMEDY}."
                    )
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let lib = load_library(&app)?;
        Ok(crate::agent::view::agent_view(&project.book, &lib, &photos))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stores a provider's API key in the keychain. Blank keys are refused and
/// the key is trimmed first.
#[tauri::command]
pub async fn set_api_key(provider: Provider, key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || keys::set_key(&KeychainStore, provider, &key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_api_key(provider: Provider) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || KeychainStore.clear(provider))
        .await
        .map_err(|e| e.to_string())?
}

/// Whether each provider has a usable key. The only key read the webview
/// can trigger, and it answers with bools.
#[tauri::command]
pub async fn api_key_status() -> Result<KeyStatus, String> {
    tauri::async_runtime::spawn_blocking(|| keys::status(&KeychainStore, keys::env_fallback))
        .await
        .map_err(|e| e.to_string())?
}

/// Sends one model request and streams its response over `on_event`. The
/// webview names only the provider, the path and the body; see
/// `agent::request::run` for what is checked before anything connects.
#[tauri::command]
pub async fn model_request(
    requests: tauri::State<'_, ModelRequests>,
    id: String,
    provider: Provider,
    path: String,
    body: String,
    on_event: Channel<ModelEvent>,
) -> Result<(), ModelRequestError> {
    let request = Outbound { id, provider, path, body };
    agent::request::run(&requests, request, agent::request::base_url, keys::api_key, &on_event)
        .await
}

/// Stops the request with this id, which then ends with `Cancelled`. Safe to
/// send before `model_request` has started.
#[tauri::command]
pub fn cancel_model_request(requests: tauri::State<'_, ModelRequests>, id: String) {
    requests.cancel(&id);
}

/// Applies one spread-level edit to a saved book, persists it, and returns
/// the whole new layout -- never `{ok: true}`. The webview replaces what it
/// shows with what came back, so a partial or refused edit can never leave
/// the screen and the saved book disagreeing.
///
/// The photo slice is rebuilt from the hashes persisted with the project,
/// exactly as `export_book` does, so an edit after a restart sees the same
/// photos the book was assembled against.
#[tauri::command]
pub async fn edit_book(
    app: AppHandle,
    project_id: i64,
    edit: crate::book::edit::BookEdit,
) -> Result<BookLayout, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let mut project = db
            .load_project(project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project {project_id} no longer exists"))?;
        let lib = load_library(&app)?;
        let weights = load_weights(&app);
        let parsed = resolve_photos(&db, &project.photo_hashes)?;
        crate::book::edit::apply(&mut project.book, &edit, &lib, &parsed, &weights)
            .map_err(|e| e.to_string())?;
        let changed = db.update_project_book(project_id, &project.book).map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err(format!("project {project_id} no longer exists"));
        }
        let photos = resolve_preview_photos(&db, &project.photo_hashes)?;
        Ok(crate::preview::book_layout(project.id, &project.book, photos, &lib))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Removes a saved project: its row, its export history, its photo list and
/// the user's include/exclude decisions -- see `Db::delete_project`.
///
/// Deliberately does NOT touch anything outside those four tables. The
/// `features` analysis cache is keyed by content hash and shared across every
/// project, not owned by this one -- it is the expensive thing (a full Apple
/// Vision pass over every photo), and `Db::delete_project` never reaches into
/// it. Any files this project exported live in a folder the user chose, and
/// may already be uploaded to a printer -- removing a book from the app must
/// not reach onto their disk, and nothing here does either.
#[tauri::command]
pub async fn delete_project(app: AppHandle, id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        db.delete_project(id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Renames a saved project. The name is set once at generate time from the
/// source folder (see `defaultProjectName` in the webview); this is the only
/// way to change it afterwards.
///
/// Fails with a clear message rather than a silent no-op for both an empty
/// name and an id that no longer exists -- `Db::rename_project`'s affected-row
/// count is what tells the two apart from "renamed successfully".
#[tauri::command]
pub async fn rename_project(app: AppHandle, id: i64, name: String) -> Result<(), String> {
    let name = normalize_project_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
        let affected = db.rename_project(id, &name).map_err(|e| e.to_string())?;
        if affected == 0 {
            return Err(format!("project {id} no longer exists"));
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens Finder with the exported files selected.
///
/// Shells out to macOS's own `open -R` rather than adding the shell plugin's
/// `open` permission to the webview: this app is macOS-only by constraint,
/// and the command is spawned (never waited on) so a slow Finder cannot
/// block the caller. A failure to reveal is reported, not swallowed -- the
/// user pressed a button and deserves to know it did nothing.
#[tauri::command]
pub async fn reveal_in_finder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not reveal {path} in Finder: {e}"))
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
    fn normalize_project_name_trims_surrounding_whitespace() {
        assert_eq!(normalize_project_name("  Kyoto Trip  ").unwrap(), "Kyoto Trip");
    }

    #[test]
    fn normalize_project_name_rejects_an_empty_or_whitespace_only_name() {
        assert!(normalize_project_name("").is_err());
        assert!(normalize_project_name("   ").is_err());
    }

    #[test]
    fn normalize_project_name_keeps_internal_whitespace() {
        assert_eq!(normalize_project_name("Kyoto  Trip").unwrap(), "Kyoto  Trip");
    }

    /// **Deliberately no length cap.** `save_project`'s own `name: &str`
    /// never enforced one either (SQLite's `TEXT` column has no practical
    /// limit), so `rename_project` refusing a long name would be a NEW,
    /// stricter rule invented for rename alone rather than something the app
    /// already asks of a project's name. If a cap is ever wanted, it belongs
    /// here AND in `save_project`'s path, decided together -- this test pins
    /// today's actual (permissive) behaviour so that decision is visible
    /// rather than accidental.
    #[test]
    fn normalize_project_name_does_not_truncate_a_very_long_name() {
        let long_name = "A".repeat(5_000);
        let padded = format!("  {long_name}  ");
        let result = normalize_project_name(&padded).unwrap();
        assert_eq!(result.len(), 5_000, "must not be truncated");
        assert_eq!(result, long_name);
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

    /// The scan walks nested folders, skips hidden directories and AppleDouble
    /// files, ignores non-images, and returns a sorted list -- asserted on a
    /// real temporary tree rather than a mock, so the recursion, the hidden-
    /// directory rule and the file filter are all exercised on disk.
    #[test]
    fn collect_photo_paths_walks_nested_folders_and_skips_hidden_and_apple_double() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("2024-05-01/raw")).unwrap();
        std::fs::create_dir_all(root.join(".Trashes")).unwrap();
        for rel in [
            "top.jpg",
            "2024-05-01/a.HEIC",
            "2024-05-01/raw/b.arw",
            "2024-05-01/._a.HEIC",
            "2024-05-01/notes.txt",
            ".Trashes/hidden.jpg",
        ] {
            std::fs::write(root.join(rel), b"x").unwrap();
        }

        let found = collect_photo_paths(&[root.to_string_lossy().into_owned()]).unwrap();

        let rel: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&root.to_string_lossy().into_owned()).unwrap().trim_start_matches('/').to_string())
            .collect();
        assert_eq!(rel, vec!["2024-05-01/a.HEIC", "2024-05-01/raw/b.arw", "top.jpg"]);
    }

    /// A directory symlink is not followed: a link back to an ancestor would
    /// otherwise recurse until the path length limit, and the scan would never
    /// report anything. The file behind the link is still counted once, via
    /// its real path.
    #[test]
    fn collect_photo_paths_does_not_follow_a_directory_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::write(root.join("real/one.jpg"), b"x").unwrap();
        std::os::unix::fs::symlink(root, root.join("real/loop")).unwrap();

        let found = collect_photo_paths(&[root.to_string_lossy().into_owned()]).unwrap();

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("real/one.jpg"), "{found:?}");
    }

    /// A folder that does not exist is an error the user sees, not an empty
    /// scan that reads as "no photos here".
    #[test]
    fn collect_photo_paths_reports_a_missing_folder() {
        assert!(collect_photo_paths(&["/nonexistent/pbg-scan-test".to_string()]).is_err());
    }

    /// Several folders are one population: listed once each even when one
    /// root sits inside another, and sorted across roots by path. A missing
    /// SECOND folder still fails the scan -- a folder the user chose and
    /// cannot be read must not quietly contribute nothing.
    #[test]
    fn collect_photo_paths_unions_several_folders_without_repeating_a_nested_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("a/inner")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        for rel in ["a/one.jpg", "a/inner/two.jpg", "b/three.jpg"] {
            std::fs::write(root.join(rel), b"x").unwrap();
        }
        let folder = |rel: &str| root.join(rel).to_string_lossy().into_owned();

        let found = collect_photo_paths(&[folder("b"), folder("a"), folder("a/inner")]).unwrap();

        let rel: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&root.to_string_lossy().into_owned()).unwrap().trim_start_matches('/').to_string())
            .collect();
        assert_eq!(rel, vec!["a/inner/two.jpg", "a/one.jpg", "b/three.jpg"]);
        assert!(collect_photo_paths(&[folder("a"), folder("missing")]).is_err());
    }

    /// The cached set answers ONLY the run that produced it. Before the id
    /// existed, analysing a second folder made the override toggle judge the
    /// first folder's screen against the second folder's photos, with no
    /// error anywhere -- the keeper marks simply became another folder's.
    #[test]
    fn select_run_refuses_a_run_other_than_the_one_cached() {
        let photo = Photo {
            path: "/a.jpg".into(),
            hash: "h".into(),
            width: 4000,
            height: 3000,
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 0,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
        };
        let cache = AnalysedSet { run_id: 4, photos: vec![photo] };

        assert_eq!(select_run(&cache, 4).map(<[Photo]>::len), Ok(1));
        let stale = select_run(&cache, 3).unwrap_err();
        assert!(stale.contains("different analysis"), "{stale}");
        let empty = select_run(&AnalysedSet::default(), 0).unwrap_err();
        assert!(empty.contains("analyse a folder first"), "{empty}");
    }

    #[test]
    fn run_ids_are_minted_in_order_and_never_zero() {
        let state = AppState::default();
        assert_eq!(state.next_run_id(), 1);
        assert_eq!(state.next_run_id(), 2);
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

    // --- `gather_chunked`: the incremental hash -> cache -> sidecar ->
    // progress pipeline that replaces hashing the whole folder up front
    // (see `sidecar::ramp_chunk_sizes`'s doc comment for the bug this
    // fixes). Uses a real (in-memory) `Db` and real temp files, same style
    // as `lookup_cache`'s own tests above, with a fake `analyze` closure
    // standing in for the sidecar -- the same pattern
    // `sidecar::analyze_batches_with_progress`'s tests use for `call`.

    fn gather_ok_record(path: &str, hash: &str, aesthetic: f64, capture: f64) -> serde_json::Value {
        serde_json::json!({
            "status": "ok",
            "features": {
                "path": path,
                "hash": hash,
                "aestheticScore": aesthetic,
                "sharpness": 10.0,
                "phash": 1u64,
                "exif": { "captureDate": capture },
                "faces": [],
            }
        })
    }

    /// A chunk that resolves to zero misses (every path in it is a cache
    /// hit) must still report progress -- otherwise a folder that is
    /// entirely a cache-warm re-run would stream nothing until the very
    /// last chunk happened to contain a miss, or nothing at all if the
    /// whole folder were cached.
    #[test]
    fn gather_chunked_reports_progress_for_a_chunk_that_is_entirely_cache_hits() {
        let db = Db::open_in_memory().unwrap();
        let path = write_temp_file("gather-all-cached.jpg", b"gather all cached bytes");
        let hash = hash_file(Path::new(&path)).unwrap();
        db.put_features(
            &hash,
            &path,
            &serde_json::json!({"path": path, "status": "ok"}).to_string(),
        )
        .unwrap();

        let mut progress_calls = 0;
        let result = gather_chunked(
            &db,
            std::slice::from_ref(&path),
            |_misses| panic!("a chunk with no misses must never call analyze"),
            |photos, _analysed, _cached, _failed| {
                progress_calls += 1;
                assert_eq!(photos.len(), 1, "the cache-hit chunk must stream its photo");
            },
        )
        .unwrap();

        assert_eq!(progress_calls, 1, "a fully-cached chunk must still emit progress");
        assert_eq!(result.cached, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(result.ok.len(), 1);
    }

    #[test]
    fn gather_chunked_counts_reconcile_against_the_input() {
        let db = Db::open_in_memory().unwrap();

        let cached_path = write_temp_file("gather-counted-cached.jpg", b"gather counted cached");
        let hash = hash_file(Path::new(&cached_path)).unwrap();
        db.put_features(
            &hash,
            &cached_path,
            &serde_json::json!({"path": cached_path, "status": "ok"}).to_string(),
        )
        .unwrap();

        let miss_path = write_temp_file("gather-counted-miss.jpg", b"gather counted miss");
        let failing_miss_path =
            write_temp_file("gather-counted-failing-miss.jpg", b"gather counted failing miss");
        let missing_path = "/nonexistent/pbg-cache-test/gather-gone.jpg".to_string();

        let input = vec![
            cached_path,
            miss_path,
            failing_miss_path.clone(),
            missing_path,
        ];

        let result = gather_chunked(
            &db,
            &input,
            |misses| {
                misses
                    .iter()
                    .map(|p| {
                        if *p == failing_miss_path {
                            crate::sidecar::failure_record(p, "boom")
                        } else {
                            gather_ok_record(p, "freshhash", 0.5, 0.0)
                        }
                    })
                    .collect()
            },
            |_photos, _analysed, _cached, _failed| {},
        )
        .unwrap();

        assert_eq!(
            result.ok.len() + result.failed,
            input.len(),
            "every input path must end up counted as either a successful record or a failure"
        );
        assert_eq!(result.cached, 1);
        assert_eq!(result.failed, 2, "one hash failure, one sidecar failure");
        assert_eq!(result.ok.len(), 2, "one cache hit, one fresh sidecar success");
    }

    /// The invariant the task brief calls out by name: positional
    /// correspondence must survive from the chunked gather all the way
    /// through `finalize_photos`. Paths are handed to `gather_chunked` in
    /// DESCENDING order -- the opposite of the ascending order
    /// `finalize_photos` sorts into -- and split across TWO ramp chunks (5
    /// paths -> [4, 1], see `sidecar::ramp_chunk_sizes`), with a mix of
    /// cache hits and sidecar misses in EACH chunk, so a bug that let
    /// derived data ride along with gather/append order instead of each
    /// record's own `path` field would swap data between photos.
    ///
    /// Mutation-checked: commenting out `finalize_photos`'s `ok.sort_by`
    /// call makes this test fail (along with the two pre-existing
    /// sort-order tests below it) -- see
    /// `.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/chunked-analysis-report.md`
    /// for the transcript.
    #[test]
    fn positional_correspondence_survives_chunked_gather_into_finalize_photos() {
        let db = Db::open_in_memory().unwrap();

        // Cache hits: pre-populate the db for "e" and "d" with known
        // aesthetic/capture values. `lookup_cache` overwrites the stale
        // cached `path` with the current one on every hit -- see
        // `a_cache_hit_carries_the_current_path_not_the_stale_cached_one`.
        let path_e = write_temp_file("gather-positional-e.jpg", b"content e");
        let path_d = write_temp_file("gather-positional-d.jpg", b"content d");
        let path_c = write_temp_file("gather-positional-c.jpg", b"content c");
        let path_b = write_temp_file("gather-positional-b.jpg", b"content b");
        let path_a = write_temp_file("gather-positional-a.jpg", b"content a");

        let day = 86_400.0;
        db.put_features(
            &hash_file(Path::new(&path_e)).unwrap(),
            &path_e,
            &gather_ok_record(&path_e, "hash-e", 0.9, 4.0 * day)["features"].to_string(),
        )
        .unwrap();
        db.put_features(
            &hash_file(Path::new(&path_d)).unwrap(),
            &path_d,
            &gather_ok_record(&path_d, "hash-d", 0.7, 3.0 * day)["features"].to_string(),
        )
        .unwrap();

        // Descending path order, split by the ramp into
        // chunk1=[e,d,c,b] (cache hits e,d; misses c,b) and chunk2=[a] (a
        // miss) -- the opposite of the ascending order finalize_photos
        // sorts into.
        let input = vec![
            path_e.clone(),
            path_d.clone(),
            path_c.clone(),
            path_b.clone(),
            path_a.clone(),
        ];

        let result = gather_chunked(
            &db,
            &input,
            |misses| {
                misses
                    .iter()
                    .map(|p| {
                        let (aesthetic, capture) = if *p == path_c {
                            (0.5, 2.0 * day)
                        } else if *p == path_b {
                            (0.3, 1.0 * day)
                        } else if *p == path_a {
                            (0.1, 0.0)
                        } else {
                            panic!("unexpected miss: {p}")
                        };
                        gather_ok_record(p, "freshhash", aesthetic, capture)
                    })
                    .collect()
            },
            |_photos, _analysed, _cached, _failed| {},
        )
        .unwrap();

        assert_eq!(result.ok.len(), 5);

        let finalized = finalize_photos(result.ok);
        let paths: Vec<&str> = finalized
            .iter()
            .map(|p| p["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                path_a.as_str(),
                path_b.as_str(),
                path_c.as_str(),
                path_d.as_str(),
                path_e.as_str(),
            ],
            "finalize_photos must sort into ascending path order regardless of gather/chunk order"
        );

        let by_path = |p: &str| finalized.iter().find(|f| f["path"] == p).unwrap();

        // aestheticPct must rank by the RAW aestheticScore each path was
        // given above (e: 0.9 highest ... a: 0.05 lowest), not by
        // gather/chunk order.
        let pct = |p: &str| by_path(p)["aestheticPct"].as_u64().unwrap();
        assert!(pct(&path_a) < pct(&path_b));
        assert!(pct(&path_b) < pct(&path_c));
        assert!(pct(&path_c) < pct(&path_d));
        assert!(pct(&path_d) < pct(&path_e));

        // eventCluster must rank by the RAW captureDate each path was given
        // above (a: earliest ... e: latest), not by gather/chunk order.
        let cluster = |p: &str| by_path(p)["eventCluster"].as_u64().unwrap();
        assert_eq!(cluster(&path_a), 0);
        assert_eq!(cluster(&path_b), 1);
        assert_eq!(cluster(&path_c), 2);
        assert_eq!(cluster(&path_d), 3);
        assert_eq!(cluster(&path_e), 4);
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

    // --- `stamp_kept`: Rust's culling verdict, sent to the webview so the
    // contact sheet and the printed book cannot disagree.

    /// A features record complete enough for `book::cull::from_features` to
    /// parse. `feat` above deliberately omits `hash`/`width`/`height`, which
    /// `from_features` requires, so culling tests need their own fixture.
    fn cullable(
        path: &str,
        phash: u64,
        aesthetic: f64,
        sharpness: f64,
        capture_quality: Option<f64>,
        is_utility: bool,
    ) -> serde_json::Value {
        let faces = match capture_quality {
            Some(q) => serde_json::json!([{ "box": [0.1, 0.1, 0.2, 0.2], "captureQuality": q }]),
            None => serde_json::json!([]),
        };
        serde_json::json!({
            "path": path,
            "hash": format!("hash-{path}"),
            "width": 4032,
            "height": 3024,
            "isUtility": is_utility,
            "phash": phash,
            "aestheticScore": aesthetic,
            "sharpness": sharpness,
            "exif": { "captureDate": null },
            "faces": faces,
            // Swift emits these unconditionally and `from_features` now
            // requires the keys, so a fixture standing in for a real record
            // must carry them even where the assertions never read them.
            "faceAreaFraction": 0.0,
            "palette": [],
            "sceneTags": [],
        })
    }

    /// **This is the regression the reconciliation exists for.**
    ///
    /// Two frames of one burst (identical phash -> one near-duplicate
    /// cluster), tied on raw sharpness, where the two ranking rules that used
    /// to coexist give OPPOSITE answers:
    ///
    /// - Rust (`book::cull::cull`, the authority): sharpness ties, so face
    ///   capture quality decides -> `/p/b.jpg` (0.9 beats 0.2).
    /// - The deleted TypeScript `keepers()`: sharpness ties, so aesthetic
    ///   decides -> `/p/a.jpg` (0.9 beats 0.1).
    ///
    /// The screen used to show one and the book contained the other. A
    /// fixture where the two rules happened to agree could not detect that at
    /// all, which is why the capture quality and the aesthetic are pointed in
    /// deliberately opposite directions here.
    #[test]
    fn stamps_kept_using_the_capture_quality_tie_break_the_webview_could_not_apply() {
        let finalized = finalize_photos(vec![
            cullable("/p/a.jpg", 100, 0.9, 5.0, Some(0.2), false),
            cullable("/p/b.jpg", 100, 0.1, 5.0, Some(0.9), false),
        ]);

        // Preconditions, asserted rather than assumed: the fixture only
        // exercises the tie-break if the first two keys really do tie.
        assert_eq!(
            finalized[0]["nearDupCluster"], finalized[1]["nearDupCluster"],
            "fixture must put both photos in one near-duplicate cluster"
        );
        assert_eq!(
            finalized[0]["sharpnessPct"], finalized[1]["sharpnessPct"],
            "fixture must tie on sharpness so the tie-break is what decides"
        );
        assert!(
            finalized[0]["aestheticPct"].as_u64() > finalized[1]["aestheticPct"].as_u64(),
            "fixture must point aesthetic at the LOSER, or it cannot tell the rules apart"
        );

        assert_eq!(finalized[0]["path"], "/p/a.jpg");
        assert_eq!(finalized[1]["path"], "/p/b.jpg");
        assert_eq!(finalized[0]["kept"], false, "higher aesthetic must NOT win the tie");
        assert_eq!(finalized[1]["kept"], true, "higher capture quality wins the tie");
    }

    /// **The toggle reaches Rust, and the verdict comes back from it.**
    ///
    /// `kept_paths` is the whole body of `apply_photo_overrides` (the command
    /// only adds the `AppState` lookup and `spawn_blocking`), and it is the
    /// single thing standing between this feature and a second copy of the
    /// culling rule in the webview.
    ///
    /// Every arm at once, from ONE call: an excluded winner drops out of the
    /// verdict, an included loser appears in it, and an untouched photo keeps
    /// whatever the engine gave it. A fixture exercising one arm cannot tell
    /// "the overrides were applied" from "the engine was re-run".
    #[test]
    fn kept_paths_answers_from_the_users_own_overrides_rather_than_the_engine_alone() {
        let records = finalize_photos(vec![
            cullable("/p/a.jpg", 100, 0.9, 9.0, None, false), // wins its burst
            cullable("/p/b.jpg", 100, 0.1, 1.0, None, false), // loses its burst
            cullable("/p/c.jpg", 4095, 0.5, 5.0, None, false), // uncontested
        ]);
        let photos = photos_from_records(&records).unwrap();
        assert_eq!(
            kept_paths(&photos, &Overrides::new()),
            vec!["/p/a.jpg", "/p/c.jpg"],
            "fixture: without an override a wins, b loses, c is uncontested"
        );

        let overrides: Overrides = [
            ("hash-/p/a.jpg".to_string(), crate::book::cull::Override::Exclude),
            ("hash-/p/b.jpg".to_string(), crate::book::cull::Override::Include),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            kept_paths(&photos, &overrides),
            vec!["/p/b.jpg", "/p/c.jpg"],
            "the excluded winner must go, the included loser must arrive, c is unaffected"
        );
    }

    /// Clearing a decision must restore the engine's own verdict, not freeze
    /// whatever the last override run produced -- "back to automatic" has to
    /// actually undo.
    #[test]
    fn kept_paths_returns_to_the_engine_verdict_when_an_override_is_cleared() {
        let records = finalize_photos(vec![
            cullable("/p/a.jpg", 100, 0.9, 9.0, None, false),
            cullable("/p/b.jpg", 100, 0.1, 1.0, None, false),
        ]);
        let photos = photos_from_records(&records).unwrap();
        let mut overrides = Overrides::new();
        overrides.set("hash-/p/a.jpg", crate::book::cull::Override::Exclude);
        assert_eq!(kept_paths(&photos, &overrides), vec!["/p/b.jpg"], "precondition");

        overrides.set("hash-/p/a.jpg", crate::book::cull::Override::Auto);

        assert_eq!(kept_paths(&photos, &overrides), vec!["/p/a.jpg"]);
    }

    /// **The verdict is keyed by PATH, not by content hash.**
    ///
    /// Two byte-identical files in one folder share a hash, `cull` keeps only
    /// one of them (same phash -> same near-duplicate cluster), and a
    /// hash-keyed verdict would then mark BOTH copies kept on the contact
    /// sheet -- the sheet would show a photo surviving that the book does not
    /// contain.
    ///
    /// **Asserted on CONTENT, not on length.** An earlier version of this
    /// test asserted only `kept.len() == 1`, which is true whichever field is
    /// extracted -- the cluster contest already collapses the pair to one
    /// winning `Photo`, so `.map(|p| p.hash)` passed it. It could not
    /// distinguish the two keyings, which is the single thing it exists to
    /// prove. The fixture now makes path and hash textually distinguishable
    /// for every photo and pins the whole returned vector:
    ///
    /// * path-keyed (correct): `["/p/copy.jpg", "/p/third.jpg"]`
    /// * hash-keyed (the bug):  `["identical-bytes", "hash-/p/third.jpg"]`
    ///
    /// The third photo is not decoration: with only the identical pair, a
    /// hash-keyed result is a one-element vector whose single value differs
    /// from the correct one only in spelling, and any assertion loose enough
    /// to tolerate "either survivor may win the tie" would tolerate the hash
    /// too. The third photo's hash and path differ from each other AND from
    /// the pair's, so the vector as a whole is unambiguous.
    #[test]
    fn kept_paths_names_the_surviving_file_by_path_not_by_content_hash() {
        let records = finalize_photos(vec![
            cullable("/p/orig.jpg", 100, 0.9, 9.0, None, false),
            cullable("/p/copy.jpg", 100, 0.9, 9.0, None, false),
            cullable("/p/third.jpg", 4095, 0.5, 5.0, None, false),
        ]);
        let mut photos = photos_from_records(&records).unwrap();
        // Byte-identical files: one content hash, two paths. `cullable`
        // derives the hash from the path, so sameness is forced here.
        for photo in &mut photos {
            if photo.path != "/p/third.jpg" {
                photo.hash = "identical-bytes".into();
            }
        }
        // The fixture is only capable of telling the two keyings apart if no
        // photo's path equals its hash.
        assert!(
            photos.iter().all(|p| p.path != p.hash),
            "fixture: path and hash must be textually distinguishable"
        );

        let kept = kept_paths(&photos, &Overrides::new());

        assert_eq!(
            kept,
            vec!["/p/copy.jpg", "/p/third.jpg"],
            "the verdict must name FILES; a hash-keyed one would say \
             [\"identical-bytes\", \"hash-/p/third.jpg\"] and mark both copies kept"
        );
    }

    #[test]
    fn stamps_kept_false_on_utility_images() {
        let finalized = finalize_photos(vec![
            cullable("/p/a.jpg", 100, 0.5, 5.0, None, true),
            cullable("/p/b.jpg", 999, 0.5, 5.0, None, false),
        ]);
        assert_eq!(finalized[0]["kept"], false);
        assert_eq!(finalized[1]["kept"], true);
    }

    /// The notification's count and the screen's kept set are now the same
    /// number by construction. Both derive from `book::cull::cull` over the
    /// same finalized records, so this pins that they cannot drift apart --
    /// the property the old two-implementation arrangement could not offer.
    #[test]
    fn kept_stamps_agree_with_the_notification_keeper_count() {
        let finalized = finalize_photos(vec![
            cullable("/p/a.jpg", 100, 0.9, 5.0, None, false),
            cullable("/p/b.jpg", 100, 0.1, 9.0, None, false), // same burst, sharper -> wins
            cullable("/p/c.jpg", 999, 0.5, 5.0, None, true),  // utility -> dropped
            cullable("/p/d.jpg", 4095, 0.5, 5.0, None, false),
        ]);
        let stamped = finalized.iter().filter(|f| f["kept"] == true).count();

        let (keepers, refused) = count_keepers(&finalized);
        assert_eq!(refused, 0, "fixture: every record here is parseable");
        assert_eq!(stamped, keepers);
        // A fixture where nothing is culled would make the equality above
        // hold trivially (every photo kept, on both sides).
        assert!(
            stamped > 0 && stamped < finalized.len(),
            "fixture must actually cull something: {stamped} of {}",
            finalized.len()
        );
    }

    /// A record `from_features` cannot parse still gets the key, stamped
    /// false -- `keepers()` in the webview reads it unconditionally, so a
    /// missing key would arrive as `undefined` and silently drop the photo
    /// with no record of why. `feat` omits `hash`/`width`/`height` on
    /// purpose, which is exactly such a record.
    #[test]
    fn stamps_kept_on_every_record_including_ones_culling_cannot_parse() {
        let finalized = finalize_photos(vec![feat("/p/a.jpg", 42, 0.5, 10.0, None, 0)]);
        assert_eq!(
            finalized[0]["kept"], false,
            "an unparseable record is not kept, and says so explicitly"
        );
    }

    /// A genuinely complete, `from_features`-parseable record, keyed by
    /// caller-supplied `path`/`hash` so two calls in one test don't collide.
    /// Carries every field `book::cull::from_features` requires --
    /// `path`/`hash`/`width`/`height`, `isUtility`, both percentiles, both
    /// cluster ids, and the `faces`/`faceAreaFraction`/`palette` keys --
    /// plus `sceneTags`, which real records always carry even though
    /// `from_features` never reads it. Distinct from `kept_candidate` above,
    /// which is shaped for culling-outcome tests (utility/cluster/tie-break
    /// knobs) rather than for pinning down "this record parses".
    fn full_feature_record(path: &str, hash: &str) -> serde_json::Value {
        serde_json::json!({
            "path": path,
            "hash": hash,
            "width": 4032,
            "height": 3024,
            "isUtility": false,
            "aestheticPct": 50,
            "sharpnessPct": 50,
            "nearDupCluster": 0,
            "eventCluster": 0,
            "faces": [],
            "faceAreaFraction": 0.0,
            "palette": [],
            "sceneTags": [],
        })
    }

    /// A record `from_features` refuses is currently skipped in silence here:
    /// the photo comes back `kept: false` with nothing reported anywhere. That
    /// is the invisible failure the strictness was added to prevent, so the
    /// count of refusals must reach the caller.
    #[test]
    fn stamp_kept_reports_how_many_records_it_could_not_parse() {
        let mut records = vec![
            // A complete, parseable record.
            full_feature_record("/a.jpg", "ha"),
            // Missing `faces` entirely -- a missing KEY, not an empty array,
            // which `from_features` refuses because every gutter and
            // safe-margin rejection would otherwise go inert.
            serde_json::json!({
                "path": "/b.jpg", "hash": "hb", "width": 4000, "height": 3000,
                "isUtility": false, "aestheticPct": 50, "sharpnessPct": 50,
                "nearDupCluster": 1, "eventCluster": 0,
                "faceAreaFraction": 0.0, "palette": []
            }),
        ];

        let refused = stamp_kept(&mut records);

        assert_eq!(refused, 1, "one record was unparseable and must be reported");
        assert_eq!(records[1]["kept"], serde_json::json!(false));
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

    // --- `count_keepers`: delegates to `book::cull::cull`, the single
    // authority on which photo survives a near-duplicate cluster, so the
    // notification body's count can never disagree with the book.

    /// Builds a full-shaped feature record, not the minimal
    /// `{isUtility, nearDupCluster, sharpnessPct, aestheticPct}` this fixture
    /// used before `count_keepers` delegated to `book::cull`.
    /// `book::cull::from_features` requires every field whose absence would
    /// change the printed book -- `path`/`hash`/`width`/`height`,
    /// `isUtility`, both percentiles, both cluster ids, and the `faces`,
    /// `faceAreaFraction` and `palette` keys (every real record from
    /// `finalize_photos` has them; only a hand-built test fixture could omit
    /// one). A record missing any of them is dropped by `count_keepers`'s own
    /// `filter_map` over `from_features` -- no longer silently, since
    /// `count_keepers` now returns the refusal count alongside the keeper
    /// count -- but an incomplete fixture would still under-count keepers
    /// regardless of the values below. A counter keeps `path`/`hash` unique
    /// per call so distinct photos in one test don't collide.
    /// Percentiles are passed as INTEGERS, not floats. `from_features` reads
    /// `sharpnessPct`/`aestheticPct` with `as_u64()`, and `serde_json` stores
    /// a float literal as an F64 variant whose `as_u64()` is `None` -- so a
    /// fixture written as `10.0` silently reaches `cull` as percentile 0 for
    /// EVERY photo, making the "sharper, same cluster -> wins" comments below
    /// untrue of the data. The tests still pass because they assert counts,
    /// which per-cluster deduplication alone decides, but the fixture must
    /// not lie about what it is feeding in.
    fn kept_candidate(
        is_utility: bool,
        cluster: u64,
        sharpness: u64,
        aesthetic: u64,
    ) -> serde_json::Value {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        serde_json::json!({
            "path": format!("/p{n}.jpg"),
            "hash": format!("h{n}"),
            "width": 4032,
            "height": 3024,
            "isUtility": is_utility,
            "nearDupCluster": cluster,
            // Every candidate in one chapter: `count_keepers` culls, and
            // culling does not read the event cluster -- but the record must
            // still carry it, because a real one always does.
            "eventCluster": 0,
            "sharpnessPct": sharpness,
            "aestheticPct": aesthetic,
            "faces": [],
            "faceAreaFraction": 0.0,
            "palette": [],
            "sceneTags": [],
        })
    }

    #[test]
    fn empty_input_has_no_keepers() {
        assert_eq!(count_keepers(&[]), (0, 0));
    }

    #[test]
    fn utility_photos_are_never_keepers() {
        let photos = vec![kept_candidate(true, 0, 100, 100)];
        assert_eq!(count_keepers(&photos), (0, 0));
    }

    #[test]
    fn keeps_one_photo_per_near_duplicate_cluster() {
        let photos = vec![
            kept_candidate(false, 0, 10, 0),
            kept_candidate(false, 0, 20, 0), // sharper, same cluster -> wins
            kept_candidate(false, 1, 5, 0),  // different cluster -> also kept
        ];
        assert_eq!(count_keepers(&photos), (2, 0));
    }

    #[test]
    fn ties_on_sharpness_are_broken_by_aesthetic_percentile() {
        let photos = vec![
            kept_candidate(false, 0, 50, 10),
            kept_candidate(false, 0, 50, 90), // same sharpness, higher aesthetic
        ];
        // Still one keeper per cluster regardless of which one wins; the
        // count itself doesn't reveal the tie-break, but this at least
        // pins that a tie doesn't produce two keepers.
        assert_eq!(count_keepers(&photos), (1, 0));
    }

    /// A record `count_keepers` cannot parse must not vanish uncounted: the
    /// completion notification would otherwise undercount with no trace of
    /// why. `full_feature_record` supplies the parseable baseline; this test
    /// adds one record missing `faces` (a missing KEY, not `[]`, which
    /// `from_features` refuses).
    #[test]
    fn count_keepers_reports_how_many_records_it_could_not_parse() {
        let mut photos = vec![full_feature_record("/a.jpg", "ha")];
        photos.push(serde_json::json!({
            "path": "/b.jpg", "hash": "hb", "width": 4000, "height": 3000,
            "isUtility": false, "aestheticPct": 50, "sharpnessPct": 50,
            "nearDupCluster": 1, "eventCluster": 0,
            "faceAreaFraction": 0.0, "palette": []
        }));

        let (keepers, refused) = count_keepers(&photos);

        assert_eq!(refused, 1, "one record was unparseable and must be reported");
        assert_eq!(keepers, 1, "the parseable record is still counted");
    }

    // ================================================================
    // Phase 2: recommend / generate / export / projects
    // ================================================================

    use crate::book::cull::Photo;
    use crate::book::manifest::Manifest;
    use crate::book::pace::{Book, Page, Placement};
    use crate::book::preflight::{Finding, Severity};
    use crate::geometry::{Rect, Side};
    use crate::protocol::ExportItem;
    use crate::templates::Library;

    /// The frozen five-template library `book::pace`'s goldens already use.
    /// Deliberately NOT the real `templates/` directory: authoring a new
    /// template must not churn a command-level expectation.
    fn fixture_library() -> Library {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
        Library::load(&dir).expect("the frozen fixture library must decompose")
    }

    /// One record in the shape `finalize_photos` hands the webview -- which is
    /// exactly the shape the webview hands back to `generate_book`.
    fn photo_record(i: usize, is_utility: bool, dup: u32, event: u32) -> serde_json::Value {
        serde_json::json!({
            "status": "ok",
            "path": format!("/photos/p{i:03}.jpg"),
            "hash": format!("hash{i:04}"),
            "width": 4032,
            "height": 3024,
            "isUtility": is_utility,
            "aestheticPct": (i * 7) % 100,
            "sharpnessPct": (i * 13) % 100,
            "nearDupCluster": dup,
            "eventCluster": event,
            "faces": [],
            "faceAreaFraction": 0.0,
            "palette": [],
            "sceneTags": [],
        })
    }

    /// `n` distinct, non-utility, non-duplicate photos spread over a few
    /// chapters -- so `cull` keeps every one of them and the keeper count is
    /// `n`, making capacity arithmetic in these tests unambiguous.
    fn distinct_records(n: usize) -> Vec<serde_json::Value> {
        (0..n).map(|i| photo_record(i, false, i as u32, (i / 4) as u32)).collect()
    }

    /// As `distinct_records`, but each photo's HASH is numbered backwards
    /// against its position, so sorted-by-hash order is the exact reverse of
    /// slice order.
    ///
    /// Load-bearing, not decorative: `distinct_records`'s hashes ascend with
    /// position, so a persistence layer that lost the ordering and re-sorted
    /// by hash would round-trip it unchanged and every ordering assertion
    /// built on it would pass under the bug. (Verified: the sort-by-hash
    /// mutation was inert against `distinct_records` and fails against
    /// this.)
    fn records_with_hashes_out_of_slice_order(n: usize) -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| {
                let mut record = photo_record(i, false, i as u32, (i / 4) as u32);
                record["hash"] = format!("hash{:04}", n - 1 - i).into();
                record
            })
            .collect()
    }

    fn engine_photo(path: &str, hash: &str) -> Photo {
        Photo {
            path: path.into(),
            hash: hash.into(),
            width: 4032,
            height: 3024,
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 0,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
        }
    }

    /// Two pages, two placements on the first and one on the second -- so a
    /// bug that only ever looks at page one, or only at placement one, is
    /// visible.
    fn two_page_book() -> (Book, Vec<Photo>) {
        let photos = vec![
            engine_photo("/photos/a.jpg", "haaa1111"),
            engine_photo("/photos/b.jpg", "hbbb2222"),
            engine_photo("/photos/c.jpg", "hccc3333"),
        ];
        let placement = |photo_index: usize, z: u32| Placement {
            photo_index,
            slot_rect: Rect::new(0.1, 0.1, 0.4, 0.4),
            crop: Rect::new(0.0, 0.0, 1.0, 1.0),
            z,
        };
        let book = Book {
            controls: Default::default(),
            seed: 99,
            dropped: 0,
            pages: vec![
                Page {
                    number: 1,
                    side: Side::Right,
                    template_id: "t".into(),
                    placements: vec![placement(0, 1), placement(1, 2)],
                },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "t".into(),
                    placements: vec![placement(2, 1)],
                },
            ],
        };
        (book, photos)
    }

    fn ok_record(filename: &str, extension: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "ok",
            "path": format!("/out/{filename}.{extension}"),
            "width": 3000,
            "height": 2000,
            "bytes": 1234,
        })
    }

    fn failed_record(filename: &str, message: &str) -> serde_json::Value {
        serde_json::json!({ "type": "failed", "filename": filename, "message": message })
    }

    fn finding(severity: Severity, page: u32, message: &str) -> Finding {
        Finding {
            severity,
            page,
            photo_path: "/photos/a.jpg".into(),
            message: message.into(),
        }
    }

    /// Writes the manifest into a real directory, so a test can assert on the
    /// bytes that actually landed on disk rather than on an in-memory struct
    /// the writer might never have been handed.
    fn manifest_writer(dir: &std::path::Path) -> impl FnOnce(&Manifest) -> Result<String, String> + '_ {
        move |m| {
            let path = dir.join("manifest.json");
            std::fs::write(&path, serde_json::to_string_pretty(m).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            Ok(path.to_string_lossy().into_owned())
        }
    }

    // --- `photos_from_records`: the index-preserving parse ----------------

    #[test]
    fn photos_from_records_preserves_input_order() {
        let records = distinct_records(3);
        let photos = photos_from_records(&records).unwrap();
        assert_eq!(photos.len(), 3);
        assert_eq!(photos[0].path, "/photos/p000.jpg");
        assert_eq!(photos[1].path, "/photos/p001.jpg");
        assert_eq!(photos[2].path, "/photos/p002.jpg");
    }

    /// The load-bearing property: `Placement::photo_index` indexes into this
    /// exact slice, so a record that cannot be parsed must FAIL the whole
    /// call rather than be skipped. Skipping (the `filter_map` shape
    /// `count_keepers` legitimately uses, where only a count matters) would
    /// shift every later photo down one index, and the book would then export
    /// and manifest the wrong source file for every placement after the
    /// broken record -- silently, with no error anywhere.
    #[test]
    fn photos_from_records_fails_rather_than_skipping_an_unparseable_record() {
        let mut records = distinct_records(3);
        records[1]["hash"] = serde_json::Value::Null;

        let result = photos_from_records(&records);

        let message = result.expect_err("a record missing a required field must fail the call");
        assert!(
            message.contains("/photos/p001.jpg") || message.contains('1'),
            "the error must identify which record failed: {message}"
        );
    }

    // --- `recommend`: page length and what it costs -----------------------

    #[test]
    fn recommend_counts_keepers_after_culling_not_raw_photos() {
        let lib = fixture_library();
        let records = vec![
            photo_record(0, false, 0, 0),
            photo_record(1, true, 1, 0),  // utility -> culled
            photo_record(2, false, 2, 0), // distinct cluster -> kept
            photo_record(3, false, 2, 0), // same cluster as #2 -> one survives
        ];
        let photos = photos_from_records(&records).unwrap();

        let rec = recommend(&photos, &lib, &Overrides::new());

        assert_eq!(rec.keeper_count, 2, "one utility dropped, one near-duplicate collapsed");
    }

    #[test]
    fn recommend_reports_how_many_keepers_each_page_length_would_drop() {
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(200)).unwrap();

        let rec = recommend(&photos, &lib, &Overrides::new());

        assert_eq!(rec.keeper_count, 200);
        for option in &rec.options {
            let capacity = crate::book::pack::Capacity::from_library(option.pages, &lib);
            assert_eq!(option.capacity_photos, capacity.max_photos);
            assert_eq!(
                option.dropped_photos,
                200 - capacity.max_photos,
                "the drop count must be keepers minus what the SKU can actually hold"
            );
        }
    }

    /// A book with room to spare must report zero dropped, not a negative
    /// number wrapped around into a colossal `usize`.
    #[test]
    fn recommend_reports_no_drops_when_every_keeper_fits() {
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(3)).unwrap();

        let rec = recommend(&photos, &lib, &Overrides::new());

        assert!(rec.options.iter().all(|o| o.dropped_photos == 0), "{:?}", rec.options);
    }

    /// The recommendation must move to the larger SKU once the keepers stop
    /// fitting the smaller one -- and stay on the smaller one when they do
    /// fit. Both directions, because a constant would pass either alone.
    #[test]
    fn recommend_moves_to_the_larger_sku_only_once_the_keepers_overflow_the_smaller_one() {
        let lib = fixture_library();
        let twenty = crate::book::pack::Capacity::from_library(20, &lib).max_photos;

        let fits = photos_from_records(&distinct_records(twenty)).unwrap();
        assert_eq!(recommend(&fits, &lib, &Overrides::new()).recommended_pages, 20);

        let overflows = photos_from_records(&distinct_records(twenty + 1)).unwrap();
        assert_eq!(recommend(&overflows, &lib, &Overrides::new()).recommended_pages, 40);
    }

    #[test]
    fn recommend_offers_every_page_length_the_user_can_choose_between() {
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(5)).unwrap();
        let rec = recommend(&photos, &lib, &Overrides::new());
        let offered: Vec<u32> = rec.options.iter().map(|o| o.pages).collect();
        assert_eq!(offered, PAGE_OPTIONS.to_vec());
        assert!(
            offered.contains(&rec.recommended_pages),
            "the recommended length must be one the user can actually pick: {offered:?}"
        );
    }

    /// The recommendation is what the user reads BEFORE pressing generate, so
    /// it has to tell them a length they cannot build is a length they cannot
    /// build -- otherwise their only feedback is a failed generation.
    ///
    /// `includedCount` counts over the PHOTO SET, not over the override map:
    /// a decision about a photo that is not in this folder must not inflate
    /// the number on screen. The fixture includes one such stray hash.
    #[test]
    fn recommend_reports_how_many_photos_the_user_asked_for_and_where_they_do_not_fit() {
        let lib = fixture_library();
        let twenty = crate::book::pack::Capacity::from_library(20, &lib).max_photos;
        let forty = crate::book::pack::Capacity::from_library(40, &lib).max_photos;
        assert!(forty > twenty + 3, "fixture: the two lengths must differ enough to distinguish");
        let records = distinct_records(twenty + 3);
        let photos = photos_from_records(&records).unwrap();
        let mut overrides: Overrides = photos
            .iter()
            .map(|p| (p.hash.clone(), crate::book::cull::Override::Include))
            .collect();
        overrides.set("a-hash-from-another-folder", crate::book::cull::Override::Include);

        let rec = recommend(&photos, &lib, &overrides);

        assert_eq!(rec.included_count, twenty + 3, "the stray hash must not be counted");
        let twenty_option = rec.options.iter().find(|o| o.pages == 20).unwrap();
        let forty_option = rec.options.iter().find(|o| o.pages == 40).unwrap();
        assert_eq!(twenty_option.included_over_capacity, 3, "20 pages cannot hold them");
        assert_eq!(forty_option.included_over_capacity, 0, "40 pages can");
    }

    /// With no decisions made, both new figures are zero -- the shape every
    /// book generated before this feature existed reports.
    #[test]
    fn recommend_reports_no_included_photos_when_the_user_has_decided_nothing() {
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(5)).unwrap();

        let rec = recommend(&photos, &lib, &Overrides::new());

        assert_eq!(rec.included_count, 0);
        assert!(rec.options.iter().all(|o| o.included_over_capacity == 0), "{:?}", rec.options);
    }

    // --- `split_findings`: Blocks and Warns are not interchangeable -------

    #[test]
    fn split_findings_separates_blocks_from_warnings_without_reclassifying_either() {
        let findings = vec![
            finding(Severity::Warn, 1, "soft"),
            finding(Severity::Block, 2, "too low"),
            finding(Severity::Warn, 3, "gutter"),
        ];

        let (blocking, warnings) = split_findings(findings);

        assert_eq!(blocking.len(), 1, "exactly one Block: {blocking:?}");
        assert_eq!(blocking[0].page, 2);
        assert!(blocking.iter().all(|f| f.severity == Severity::Block));
        assert_eq!(warnings.len(), 2, "exactly two Warns: {warnings:?}");
        assert!(warnings.iter().all(|f| f.severity == Severity::Warn));
        assert_eq!(
            warnings.iter().map(|f| f.page).collect::<Vec<_>>(),
            vec![1, 3],
            "order within a severity must survive the split"
        );
    }

    // --- `generate_and_save`: generation the user cannot lose -------------

    /// The regression this task names by name: a `generate_book` that
    /// assembles a book and hands it back without writing it means quitting
    /// the app loses it. The assertion is not "a function was called" but
    /// "the row is readable back out of the database".
    #[test]
    fn generating_a_book_persists_it_so_quitting_cannot_lose_it() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(12)).unwrap();

        let meta = NewProject { name: "Japan 2026", source_folders: vec!["/photos".into()], pages: 20, seed: 7 };
        let generated = generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &Overrides::new()).unwrap();

        let loaded = db
            .load_project(generated.project_id)
            .unwrap()
            .expect("the project must be readable back out of the database");
        assert_eq!(loaded.name, "Japan 2026");
        assert_eq!(loaded.source_folder, "/photos");
        assert_eq!(loaded.book.pages.len(), 20);
        assert_eq!(loaded.book.seed, 7);
        assert_eq!(
            loaded.book.pages.iter().map(|p| p.placements.len()).sum::<usize>(),
            generated.placed_photos,
            "the persisted book must be the same book whose counts were reported"
        );
    }

    /// **Overrides survive the save, and come back on reopen.**
    ///
    /// The full round trip the user actually experiences: decisions made on
    /// the contact sheet, carried into `generate_book`, and read back out of
    /// the database by the same path `open_project` uses. Without this they
    /// would be lost exactly the way the analysis used to be -- and
    /// invisibly, because the reopened project would simply show the engine's
    /// own verdict and look entirely correct.
    ///
    /// Both states are asserted, plus an untouched photo: a round trip that
    /// stored one state, or collapsed both into the same one, passes a
    /// single-state fixture.
    #[test]
    fn generating_a_book_persists_the_users_own_include_and_exclude_decisions() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let records = distinct_records(12);
        let photos = photos_from_records(&records).unwrap();
        let mut overrides = Overrides::new();
        overrides.set(photos[3].hash.clone(), crate::book::cull::Override::Include);
        overrides.set(photos[9].hash.clone(), crate::book::cull::Override::Exclude);

        let meta = NewProject { name: "Japan 2026", source_folders: vec!["/photos".into()], pages: 20, seed: 7 };
        let generated =
            generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &overrides).unwrap();

        let loaded = db.load_project(generated.project_id).unwrap().expect("saved project");
        assert_eq!(loaded.overrides, overrides, "reopening must restore what the user chose");
        assert_eq!(
            loaded.overrides.get(&photos[3].hash),
            crate::book::cull::Override::Include
        );
        assert_eq!(
            loaded.overrides.get(&photos[9].hash),
            crate::book::cull::Override::Exclude
        );
        assert_eq!(
            loaded.overrides.get(&photos[0].hash),
            crate::book::cull::Override::Auto,
            "an untouched photo must come back untouched"
        );
    }

    /// **Reopening a project hands the user's own decisions back to the
    /// webview.**
    ///
    /// `open_project` needs a live `AppHandle`, so this drives the mapping it
    /// delegates to, against a project read back out of a real database --
    /// the full save-and-reopen path minus the IPC hop. Dropping `overrides`
    /// in that mapping is invisible without this: the reopened project shows
    /// the engine's own verdict, which is a perfectly plausible book.
    #[test]
    fn reopening_a_project_returns_the_users_own_include_and_exclude_decisions() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(12)).unwrap();
        let mut overrides = Overrides::new();
        overrides.set(photos[3].hash.clone(), crate::book::cull::Override::Include);
        overrides.set(photos[9].hash.clone(), crate::book::cull::Override::Exclude);
        let meta = NewProject { name: "Japan 2026", source_folders: vec!["/photos".into()], pages: 20, seed: 7 };
        let generated =
            generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &overrides).unwrap();

        let detail = project_detail(db.load_project(generated.project_id).unwrap().unwrap());

        assert_eq!(detail.id, generated.project_id);
        assert_eq!(detail.overrides, overrides, "the decisions must survive the reopen");
        assert_eq!(
            detail.overrides.get(&photos[3].hash),
            crate::book::cull::Override::Include
        );
        assert_eq!(
            detail.overrides.get(&photos[9].hash),
            crate::book::cull::Override::Exclude
        );
    }

    /// Generation REFUSES rather than quietly building a book that is missing
    /// photos the user explicitly asked for. The message carries the three
    /// numbers they need to act: how many they picked, how many fit, how many
    /// over.
    #[test]
    fn generating_a_book_too_short_for_the_included_photos_fails_with_the_numbers_to_fix_it() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let capacity = crate::book::pack::Capacity::from_library(20, &lib).max_photos;
        let records = distinct_records(capacity + 4);
        let photos = photos_from_records(&records).unwrap();
        let overrides: Overrides = photos
            .iter()
            .map(|p| (p.hash.clone(), crate::book::cull::Override::Include))
            .collect();

        let meta = NewProject { name: "Too many", source_folders: vec!["/photos".into()], pages: 20, seed: 7 };
        let err = generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &overrides)
            .expect_err("a book that cannot hold every explicit choice must not be built");

        assert!(err.contains(&(capacity + 4).to_string()), "{err}");
        assert!(err.contains(&capacity.to_string()), "{err}");
        assert!(err.contains("Exclude 4"), "{err}");
        assert_eq!(
            db.list_projects().unwrap().len(),
            0,
            "and nothing may be saved: a refused generation must leave no project behind"
        );
    }

    #[test]
    fn a_generated_project_shows_up_in_the_project_list_under_the_id_it_returned() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(12)).unwrap();

        let meta = NewProject { name: "Japan 2026", source_folders: vec!["/photos".into()], pages: 20, seed: 7 };
        let generated = generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &Overrides::new()).unwrap();

        let listed = db.list_projects().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, generated.project_id);
        assert_eq!(listed[0].page_count, 20);
    }

    #[test]
    fn generate_reports_the_page_count_the_book_actually_has() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(30)).unwrap();

        let meta = NewProject { name: "b", source_folders: vec!["/photos".into()], pages: 40, seed: 3 };
        let generated = generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &Overrides::new()).unwrap();

        assert_eq!(generated.page_count, 40);
        assert_eq!(generated.seed, 3);
        assert_eq!(
            generated.placed_photos + generated.dropped_photos,
            30,
            "every input photo is either placed or dropped"
        );
    }

    // --- `expected_photo_count`: the slice a book was built against -------

    /// `Placement::photo_index` indexes into the photo slice `assemble` was
    /// given, and that slice is not persisted with the book -- so re-exporting
    /// against a DIFFERENT set of photos would silently print the wrong
    /// sources. `Book::dropped` plus the number of distinct placed indices
    /// reconstructs the original slice length, which is enough to refuse the
    /// mismatch instead of exporting nonsense.
    #[test]
    fn expected_photo_count_reconstructs_the_slice_the_book_was_built_against() {
        let lib = fixture_library();
        let photos = photos_from_records(&distinct_records(12)).unwrap();
        let book = crate::book::pace::assemble(&photos, 20, &lib, &Weights::default(), 5, &Overrides::new()).expect("no overrides");

        assert_eq!(expected_photo_count(&book), 12);
    }

    #[test]
    fn export_refuses_a_photo_slice_that_is_not_the_one_the_book_was_built_from() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();
        let too_few = photos[..2].to_vec();

        let result = run_export(
            &book,
            &too_few,
            1,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |_| panic!("a mismatched photo slice must never reach the sidecar"),
            manifest_writer(dir.path()),
        );

        assert!(result.is_err(), "a book whose placements outrun the photo slice must not export");
    }

    // --- `run_export`: pre-flight first, then write ----------------------

    /// The whole point of pre-flight: a Block is reported BEFORE anything is
    /// written, not discovered after nineteen files have landed. Both side
    /// effects panic if reached, so this cannot pass by accident.
    #[test]
    fn a_blocking_finding_stops_the_export_before_a_single_file_is_written() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();
        let findings = vec![
            finding(Severity::Warn, 1, "soft"),
            finding(Severity::Block, 2, "below the 200 DPI floor"),
        ];

        let result = run_export(
            &book,
            &photos,
            1,
            &dir.path().to_string_lossy(),
            findings,
            |_| panic!("the sidecar must never be reached when a finding blocks"),
            |_| panic!("the manifest must never be written when a finding blocks"),
        )
        .unwrap();

        assert!(result.blocked);
        assert_eq!(result.blocking.len(), 1);
        assert_eq!(result.warnings.len(), 1, "warnings are still reported alongside the block");
        assert!(result.written.is_empty());
        assert_eq!(result.manifest_path, None);
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "the output directory must be untouched"
        );
    }

    #[test]
    fn warnings_alone_do_not_stop_the_export() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();
        let mut sent = 0usize;

        let result = run_export(
            &book,
            &photos,
            1,
            &dir.path().to_string_lossy(),
            vec![finding(Severity::Warn, 1, "soft")],
            |items| {
                sent = items.len();
                items.iter().map(|i| ok_record(&i.filename, "jpg")).collect()
            },
            manifest_writer(dir.path()),
        )
        .unwrap();

        assert!(!result.blocked);
        assert_eq!(sent, 3, "every placement in the book must be sent");
        assert_eq!(result.written.len(), 3);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.blocking.is_empty());
        assert!(result.manifest_path.is_some());
    }

    #[test]
    fn export_reports_the_paths_actually_written_and_the_items_that_failed() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            1,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| {
                vec![
                    ok_record(&items[0].filename, "jpg"),
                    failed_record(&items[1].filename, "could not decode: /photos/b.jpg"),
                    ok_record(&items[2].filename, "jpg"),
                ]
            },
            manifest_writer(dir.path()),
        )
        .unwrap();

        assert_eq!(result.written.len(), 2);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].filename, "p01-z2-hbbb2222");
        assert_eq!(result.failures[0].message, "could not decode: /photos/b.jpg");
    }

    // --- the manifest must describe what was written, not what was guessed

    /// The ruling carried into this task: `export::predicted_format` guesses
    /// the container from the file EXTENSION, while the Swift exporter picks
    /// it from the actual container via `CGImageSourceGetType`. A `.jpg` file
    /// that is really a PNG makes the two disagree, and the manifest -- the
    /// record of what the user uploaded -- would name a file that does not
    /// exist. Every source here is named `.jpg` (so the prediction is `jpg`)
    /// while every returned path is `.png`, so a manifest still trusting the
    /// prediction is unambiguously distinguishable from one reconciled
    /// against reality.
    #[test]
    fn the_manifest_records_the_container_actually_written_not_the_one_predicted() {
        let (book, photos) = two_page_book();
        assert!(
            photos.iter().all(|p| p.path.ends_with(".jpg")),
            "fixture must make the prediction and the reality disagree"
        );
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            42,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| items.iter().map(|i| ok_record(&i.filename, "png")).collect(),
            manifest_writer(dir.path()),
        )
        .unwrap();

        let written: Manifest = serde_json::from_str(
            &std::fs::read_to_string(result.manifest_path.unwrap()).unwrap(),
        )
        .unwrap();
        let formats: Vec<&str> = written
            .pages
            .iter()
            .flat_map(|p| p.photos.iter().map(|ph| ph.format.as_str()))
            .collect();
        assert_eq!(
            formats,
            vec!["png", "png", "png"],
            "the manifest must carry the extension the sidecar actually wrote"
        );
        assert_eq!(result.format, "png", "and so must the recorded export");
    }

    /// A photo the sidecar never wrote must not appear in the manifest at
    /// all: the manifest is the record of the files that exist, and an entry
    /// naming a file nobody wrote is exactly the "records a format that
    /// differs from what was written" failure in its worst form -- there is
    /// no file to have a format. The page itself still appears, per
    /// `manifest`'s own never-drop-a-page rule.
    #[test]
    fn the_manifest_drops_an_entry_the_sidecar_never_wrote() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            42,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| {
                vec![
                    ok_record(&items[0].filename, "jpg"),
                    failed_record(&items[1].filename, "could not decode"),
                    ok_record(&items[2].filename, "jpg"),
                ]
            },
            manifest_writer(dir.path()),
        )
        .unwrap();

        let written: Manifest = serde_json::from_str(
            &std::fs::read_to_string(result.manifest_path.unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(written.page_count, 2, "the page count still reconciles against the book");
        assert_eq!(written.pages.len(), 2, "no page is ever dropped");
        let filenames: Vec<&str> = written
            .pages
            .iter()
            .flat_map(|p| p.photos.iter().map(|ph| ph.filename.as_str()))
            .collect();
        assert_eq!(
            filenames,
            vec!["p01-z1-haaa1111", "p02-z1-hccc3333"],
            "the failed item must not be listed as if it had been written"
        );
    }

    #[test]
    fn a_mixed_format_export_is_recorded_as_mixed_rather_than_one_of_the_two() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            1,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| {
                vec![
                    ok_record(&items[0].filename, "jpg"),
                    ok_record(&items[1].filename, "png"),
                    ok_record(&items[2].filename, "jpg"),
                ]
            },
            manifest_writer(dir.path()),
        )
        .unwrap();

        assert_eq!(result.format, "mixed");
    }

    // --- wire fixtures: the Rust half of the pin -------------------------
    //
    // Every type below crosses to the webview, where nothing checks it: a
    // rename on either side is a silent `undefined` at runtime, not a build
    // error (see `docs/PROJECT-STATUS.md`'s known-debts list). These assert
    // the serialised value EXACTLY equals a fixture that `tests/book.test.ts`
    // independently feeds through the real UI functions -- so a rename here
    // fails this test, and "fixing" the fixture fails the TypeScript one.
    // A Rust-to-Rust round-trip cannot do that, because both of its sides
    // move together.

    fn wire_fixture(name: &str) -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/wire")
            .join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing wire fixture {path:?}: {e}"));
        serde_json::from_str(&text).expect("wire fixture must be valid JSON")
    }

    #[test]
    fn book_recommendation_serialises_exactly_the_keys_the_webview_reads() {
        let value = BookRecommendation {
            keeper_count: 26,
            included_count: 3,
            recommended_pages: 20,
            options: vec![
                PageOption {
                    pages: 20,
                    capacity_photos: 24,
                    dropped_photos: 2,
                    included_over_capacity: 0,
                },
                PageOption {
                    pages: 40,
                    capacity_photos: 54,
                    dropped_photos: 0,
                    included_over_capacity: 0,
                },
            ],
        };
        assert_eq!(
            serde_json::to_value(&value).unwrap(),
            wire_fixture("book-recommendation.json")
        );
    }

    /// `Overrides` crosses this boundary in BOTH directions -- up to Rust on
    /// every override toggle and on generation, back down inside
    /// `ProjectDetail` when a project is reopened -- so both directions are
    /// pinned against the same committed fixture.
    ///
    /// A key-name change is not the hazard here (the keys are content
    /// hashes); the STATE SPELLING is. Rust writes it from
    /// `#[serde(rename_all = "lowercase")]`, SQLite from `Override::as_str`,
    /// and TypeScript from its own union type, and nothing but this fixture
    /// makes those three agree.
    #[test]
    fn photo_overrides_cross_the_wire_as_the_states_the_webview_writes() {
        let fixture = wire_fixture("photo-overrides.json");
        let value: Overrides = [
            ("a1b2c3d4".to_string(), crate::book::cull::Override::Include),
            ("e5f6a7b8".to_string(), crate::book::cull::Override::Exclude),
        ]
        .into_iter()
        .collect();

        assert_eq!(serde_json::to_value(&value).unwrap(), fixture, "Rust -> webview");
        assert_eq!(
            serde_json::from_value::<Overrides>(fixture).unwrap(),
            value,
            "webview -> Rust"
        );
        // The same spelling SQLite stores, asserted against the same fixture
        // rather than against a second literal that could drift from it.
        assert_eq!(crate::book::cull::Override::Include.as_str(), "include");
        assert_eq!(crate::book::cull::Override::Exclude.as_str(), "exclude");
    }

    #[test]
    fn generated_book_serialises_exactly_the_keys_the_webview_reads() {
        let value = GeneratedBook {
            project_id: 7,
            page_count: 20,
            placed_photos: 24,
            dropped_photos: 2,
            seed: 424242,
        };
        assert_eq!(serde_json::to_value(&value).unwrap(), wire_fixture("generated-book.json"));
    }

    #[test]
    fn export_result_serialises_exactly_the_keys_the_webview_reads() {
        let value = ExportResult {
            blocked: false,
            output_dir: "/Users/jj/Desktop/photobook-export".into(),
            blocking: Vec::new(),
            warnings: vec![Finding {
                severity: Severity::Warn,
                page: 4,
                photo_path: "/photos/IMG_0042.jpg".into(),
                message: "Photo resolves at 250 DPI in this slot, below the 300 DPI target".into(),
            }],
            written: vec!["/Users/jj/Desktop/photobook-export/p04-z1-abcd1234.jpg".into()],
            failures: vec![ExportFailure {
                filename: "p04-z2-ef567890".into(),
                message: "could not decode: /photos/bad.raw".into(),
            }],
            manifest_path: Some("/Users/jj/Desktop/photobook-export/manifest.json".into()),
            manifest_error: None,
            format: "jpg".into(),
        };
        assert_eq!(serde_json::to_value(&value).unwrap(), wire_fixture("export-result.json"));
    }

    /// The blocked shape is pinned separately: it is the one the UI renders
    /// its "nothing was written" branch from, and `manifestPath: null` /
    /// `written: []` are exactly the fields a careless refactor would omit
    /// rather than emit empty.
    #[test]
    fn a_blocked_export_result_serialises_exactly_the_keys_the_webview_reads() {
        let value = ExportResult {
            blocked: true,
            output_dir: "/Users/jj/Desktop/photobook-export".into(),
            blocking: vec![Finding {
                severity: Severity::Block,
                page: 4,
                photo_path: "/photos/IMG_0042.jpg".into(),
                message: "Photo resolves at 120 DPI in this slot, below the 200 DPI floor".into(),
            }],
            warnings: vec![Finding {
                severity: Severity::Warn,
                page: 6,
                photo_path: "/photos/IMG_0099.jpg".into(),
                message: "Salient content falls inside the gutter dead strip".into(),
            }],
            written: Vec::new(),
            failures: Vec::new(),
            manifest_path: None,
            manifest_error: None,
            format: String::new(),
        };
        assert_eq!(
            serde_json::to_value(&value).unwrap(),
            wire_fixture("export-result-blocked.json")
        );
    }

    #[test]
    fn export_events_serialise_exactly_the_keys_the_webview_reads() {
        let events =
            vec![ExportEvent::Started { total: 24 }, ExportEvent::Progress { completed: 8, total: 24 }];
        assert_eq!(serde_json::to_value(&events).unwrap(), wire_fixture("export-events.json"));
    }

    #[test]
    fn project_list_items_serialise_exactly_the_keys_the_webview_reads() {
        let items = vec![
            ProjectListItem {
                id: 7,
                name: "Japan 2026".into(),
                source_folder: "/Users/jj/Pictures/Japan".into(),
                source_folders: vec!["/Users/jj/Pictures/Japan".into()],
                page_count: 20,
                photo_count: 24,
                created_at: 1_755_100_000,
                updated_at: 1_755_103_600,
                last_export: Some(ExportSummary {
                    at: 1_755_103_600,
                    output_dir: "/Users/jj/Desktop/photobook-export".into(),
                    format: "jpg".into(),
                    file_count: 24,
                }),
            },
            ProjectListItem {
                id: 8,
                name: "Kyoto draft".into(),
                source_folder: "/Users/jj/Pictures/Kyoto".into(),
                source_folders: vec!["/Users/jj/Pictures/Kyoto".into()],
                page_count: 40,
                photo_count: 54,
                created_at: 1_755_200_000,
                updated_at: 1_755_200_000,
                last_export: None,
            },
        ];
        assert_eq!(serde_json::to_value(&items).unwrap(), wire_fixture("project-list.json"));
    }

    #[test]
    fn project_detail_serialises_exactly_the_keys_the_webview_reads() {
        let detail = ProjectDetail {
            id: 7,
            name: "Japan 2026".into(),
            source_folder: "/Users/jj/Pictures/Japan".into(),
            source_folders: vec!["/Users/jj/Pictures/Japan".into()],
            created_at: 1_755_100_000,
            updated_at: 1_755_103_600,
            page_count: 20,
            photo_count: 24,
            dropped_photos: 2,
            seed: 424242,
            overrides: [
                ("a1b2c3d4".to_string(), crate::book::cull::Override::Include),
                ("e5f6a7b8".to_string(), crate::book::cull::Override::Exclude),
            ]
            .into_iter()
            .collect(),
            exports: vec![ExportSummary {
                at: 1_755_103_600,
                output_dir: "/Users/jj/Desktop/photobook-export".into(),
                format: "jpg".into(),
                file_count: 24,
            }],
        };
        assert_eq!(serde_json::to_value(&detail).unwrap(), wire_fixture("project-detail.json"));
    }

    /// `Finding` is re-exported to the webview verbatim from
    /// `book::preflight`, so its camelCase spelling and its lowercase
    /// severity are part of THIS boundary too, not just pre-flight's internal
    /// concern. Pinned here as literal wire bytes rather than by round-trip.
    #[test]
    fn a_finding_reaches_the_webview_as_camel_case_with_a_lowercase_severity() {
        let value = serde_json::to_string(&finding(Severity::Block, 4, "boom")).unwrap();
        assert!(value.contains(r#""severity":"block""#), "{value}");
        assert!(value.contains(r#""photoPath":"/photos/a.jpg""#), "{value}");
        assert!(!value.contains("photo_path"), "{value}");
        let warn = serde_json::to_string(&finding(Severity::Warn, 4, "boom")).unwrap();
        assert!(warn.contains(r#""severity":"warn""#), "{warn}");
    }

    // --- a project is durable across a restart (spec 5.4a) --------------

    /// Writes each record into the features cache under its own hash, the
    /// way a real analysis run does, so a project saved against them can be
    /// resolved back later without the webview.
    fn cache_records(db: &Db, records: &[serde_json::Value]) {
        for record in records {
            db.put_features(
                record["hash"].as_str().unwrap(),
                record["path"].as_str().unwrap(),
                &record.to_string(),
            )
            .unwrap();
        }
    }

    /// The preview's photo list is `resolve_photos` plus one field the
    /// layout engine never reads.
    ///
    /// Two things it must not get wrong, and both are silent when it does.
    /// **Order**: `photo_index` indexes this list positionally, so returning
    /// it in hash order (which SQLite would happily do) puts a different
    /// photograph in every slot. The hashes here are numbered BACKWARDS
    /// against the order asked for, so sorted order is the exact reverse.
    /// **A missing thumbnail**: the sidecar degrades a failed thumbnail write
    /// to a null path, and dropping that photo would renumber every index
    /// after it -- so it comes back present, with `thumbnail_path: None`.
    #[test]
    fn resolve_preview_photos_keeps_the_asked_for_order_and_a_photo_whose_thumbnail_is_missing() {
        let db = Db::open_in_memory().unwrap();
        let records: Vec<serde_json::Value> = (0..3)
            .map(|i| {
                let mut record = photo_record(i, false, i as u32, 0);
                record["hash"] = format!("hash{:04}", 2 - i).into();
                // The middle photo's thumbnail write failed.
                if i != 1 {
                    record["thumbnailPath"] =
                        format!("/thumbs/hash{:04}.jpg", 2 - i).into();
                }
                record
            })
            .collect();
        cache_records(&db, &records);
        let asked_for: Vec<String> =
            records.iter().map(|r| r["hash"].as_str().unwrap().to_string()).collect();
        assert_eq!(asked_for, vec!["hash0002", "hash0001", "hash0000"], "sanity: descending");

        let photos = resolve_preview_photos(&db, &asked_for).unwrap();

        assert_eq!(
            photos.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            vec!["/photos/p000.jpg", "/photos/p001.jpg", "/photos/p002.jpg"],
            "the order asked for, not the order the hashes sort in"
        );
        assert_eq!(
            photos.iter().map(|p| p.thumbnail_path.as_deref()).collect::<Vec<_>>(),
            vec![Some("/thumbs/hash0002.jpg"), None, Some("/thumbs/hash0000.jpg")]
        );
        assert_eq!(photos[0].width, 4032, "oriented dimensions come through");
        assert_eq!(photos[0].height, 3024);
    }

    /// A hash the cache no longer holds fails the whole call rather than
    /// returning a shorter list -- the same all-or-nothing rule
    /// `resolve_photos` has, and for the same reason: a gap renumbers every
    /// `photo_index` after it, so the preview would show the wrong photo in
    /// every subsequent slot with no error anywhere.
    #[test]
    fn resolve_preview_photos_refuses_a_hash_the_cache_no_longer_holds() {
        let db = Db::open_in_memory().unwrap();
        cache_records(&db, &distinct_records(2));

        let err = resolve_preview_photos(
            &db,
            &["hash0000".to_string(), "gone".to_string(), "hash0001".to_string()],
        )
        .unwrap_err();

        assert!(err.contains("no longer in the analysis cache"), "{err}");
    }

    /// **The property the review asked for**: a project saved, the app
    /// restarted, and the project reopened must export exactly what the
    /// original export did, without re-analysing the folder.
    ///
    /// "Byte-identical files" is asserted through the three inputs that
    /// wholly determine each output file -- source path, crop window, and
    /// output filename -- because the exporter is a pure function of those
    /// (decode the source, crop that window, write that name; see
    /// `Exporter.exportOne`). Running the real sidecar twice would test the
    /// sidecar's determinism, not this module's.
    ///
    /// The restart is real, not simulated: the database is a FILE, the first
    /// handle is dropped, and a second `Db::open` reads it back with no
    /// in-memory state carried over. The webview's photo array is never
    /// consulted on the second pass -- `resolve_photos` is given nothing but
    /// the hashes that were persisted.
    #[test]
    fn a_project_exports_identically_after_a_restart_without_re_analysing() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("photobook.sqlite");
        let lib = fixture_library();
        let records = records_with_hashes_out_of_slice_order(12);

        let before_restart = {
            let db = Db::open(&db_path).unwrap();
            cache_records(&db, &records);
            let photos = photos_from_records(&records).unwrap();
            let meta =
                NewProject { name: "Japan", source_folders: vec!["/photos".into()], pages: 20, seed: 11 };
            let generated =
                generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &Overrides::new()).unwrap();
            let book = db.load_project(generated.project_id).unwrap().unwrap().book;
            (generated.project_id, crate::export::build_items(&book, &photos))
        }; // every handle dropped here -- this is the "quit".

        let (project_id, original_items) = before_restart;
        assert!(!original_items.is_empty(), "sanity: the book must place something");

        // --- restart ---
        let db = Db::open(&db_path).unwrap();
        let project = db.load_project(project_id).unwrap().expect("the project must survive");
        let resolved = resolve_photos(&db, &project.photo_hashes)
            .expect("a saved project must resolve its photos from the cache alone");
        let items = crate::export::build_items(&project.book, &resolved);

        assert_eq!(
            items, original_items,
            "the reopened project must export the same sources, crops and filenames"
        );
    }

    #[test]
    fn generating_a_book_stores_one_hash_per_photo_in_slice_order() {
        let db = Db::open_in_memory().unwrap();
        let lib = fixture_library();
        let records = records_with_hashes_out_of_slice_order(12);
        let photos = photos_from_records(&records).unwrap();
        let meta = NewProject { name: "b", source_folders: vec!["/photos".into()], pages: 20, seed: 2 };

        let generated = generate_and_save(&db, &meta, &photos, &lib, &Weights::default(), &Overrides::new()).unwrap();

        let expected: Vec<String> = photos.iter().map(|p| p.hash.clone()).collect();
        let mut sorted = expected.clone();
        sorted.sort();
        assert_ne!(
            sorted, expected,
            "fixture must not be pre-sorted, or a re-sorting bug is invisible here"
        );

        let stored = db.load_project(generated.project_id).unwrap().unwrap().photo_hashes;
        assert_eq!(
            stored, expected,
            "the stored list must be every photo the book was assembled against, in that order"
        );
    }

    /// The caveat the review named explicitly: `Db::get_features` filters on
    /// `ANALYZER_VERSION`, so bumping it makes every stored hash
    /// unresolvable. That must be a clear instruction to re-analyse -- never
    /// a panic, and never a partial photo list that would export the wrong
    /// sources for every placement after the first gap.
    #[test]
    fn a_project_cached_under_an_older_analyzer_version_asks_for_a_re_analysis() {
        let db = Db::open_in_memory().unwrap();
        let records = distinct_records(3);
        cache_records(&db, &records);
        let hashes: Vec<String> = records
            .iter()
            .map(|r| r["hash"].as_str().unwrap().to_string())
            .collect();
        assert!(resolve_photos(&db, &hashes).is_ok(), "sanity: resolvable at the current version");

        // Exactly what an ANALYZER_VERSION bump does to these rows.
        db.conn
            .execute(
                "UPDATE features SET analyzer_version = ?1",
                rusqlite::params![crate::db::ANALYZER_VERSION - 1],
            )
            .unwrap();

        let message = resolve_photos(&db, &hashes)
            .expect_err("stale-version rows must not silently resolve to a shorter list");
        assert!(
            message.contains("re-analyse") || message.contains("re-analyze"),
            "the error must tell the user what to do: {message}"
        );
    }

    #[test]
    fn a_project_whose_photos_left_the_cache_asks_for_a_re_analysis() {
        let db = Db::open_in_memory().unwrap();
        let records = distinct_records(3);
        cache_records(&db, &records);
        let mut hashes: Vec<String> = records
            .iter()
            .map(|r| r["hash"].as_str().unwrap().to_string())
            .collect();
        hashes.push("hash-that-was-never-analysed".into());

        let message = resolve_photos(&db, &hashes).expect_err("a missing photo must fail loudly");
        assert!(message.contains("re-analyse"), "{message}");
    }

    /// A project saved before `project_photos` existed. `load_project`
    /// returns an empty list rather than failing (so the book still appears
    /// in the project list); resolving it is where the user is told what to
    /// do about it.
    #[test]
    fn a_project_saved_before_photo_lists_existed_asks_for_a_re_analysis() {
        let db = Db::open_in_memory().unwrap();
        let message = resolve_photos(&db, &[]).expect_err("an empty photo list cannot export");
        assert!(message.contains("re-analyse"), "{message}");
    }

    // --- a successful export is not reported as a failure ---------------

    /// The files are already on disk by the time the manifest is written. A
    /// manifest that cannot be written (read-only volume, full disk) must
    /// NOT discard the export: the user has nineteen files somewhere and
    /// needs to be told where, not shown "something went wrong" with no path
    /// back to them.
    #[test]
    fn a_manifest_that_cannot_be_written_does_not_discard_a_successful_export() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            1,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| items.iter().map(|i| ok_record(&i.filename, "jpg")).collect(),
            |_| Err("Read-only file system (os error 30)".into()),
        )
        .expect("a manifest failure must not fail the export");

        assert!(!result.blocked);
        assert_eq!(result.written.len(), 3, "the files that were written are still reported");
        assert_eq!(result.manifest_path, None);
        let error = result
            .manifest_error
            .as_ref()
            .expect("the manifest failure must be surfaced, not swallowed");
        assert!(error.contains("Read-only"), "the real cause must reach the user: {error}");
    }

    #[test]
    fn a_successful_export_reports_no_manifest_error() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            1,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| items.iter().map(|i| ok_record(&i.filename, "jpg")).collect(),
            manifest_writer(dir.path()),
        )
        .unwrap();

        assert_eq!(result.manifest_error, None);
        assert!(result.manifest_path.is_some());
    }

    /// Reconciliation keys on the file stem and reads the extension off the
    /// returned path. A path with no extension at all must keep its manifest
    /// entry -- the file WAS written -- rather than be dropped as if it never
    /// existed. Latent today (Swift always appends one) but it fails in the
    /// direction that loses a real file from the record.
    #[test]
    fn an_output_path_without_an_extension_keeps_its_manifest_entry() {
        let (book, photos) = two_page_book();
        let dir = tempfile::tempdir().unwrap();

        let result = run_export(
            &book,
            &photos,
            42,
            &dir.path().to_string_lossy(),
            Vec::new(),
            |items| {
                items
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "type": "ok",
                            "path": format!("/out/{}", i.filename), // no extension
                            "width": 3000, "height": 2000, "bytes": 1234,
                        })
                    })
                    .collect()
            },
            manifest_writer(dir.path()),
        )
        .unwrap();

        let written: Manifest = serde_json::from_str(
            &std::fs::read_to_string(result.manifest_path.unwrap()).unwrap(),
        )
        .unwrap();
        let entries: Vec<&str> = written
            .pages
            .iter()
            .flat_map(|p| p.photos.iter().map(|ph| ph.filename.as_str()))
            .collect();
        assert_eq!(
            entries,
            vec!["p01-z1-haaa1111", "p01-z2-hbbb2222", "p02-z1-hccc3333"],
            "a written file must stay in the manifest even when its path names no container"
        );
        assert!(
            written.pages[0].photos.iter().all(|p| p.format.is_empty()),
            "and its container is recorded as unknown rather than guessed"
        );
    }

    /// A `Vec<ExportItem>` is built from the book and handed straight to the
    /// sidecar, so the count the UI is told to expect must be the count that
    /// is actually sent -- otherwise the progress bar's denominator is a
    /// different number from its numerator's source.
    #[test]
    fn the_progress_total_is_the_number_of_items_actually_sent() {
        let (book, photos) = two_page_book();
        let items: Vec<ExportItem> = crate::export::build_items(&book, &photos);
        assert_eq!(items.len(), placement_total(&book));
    }
}
