use crate::{cluster, db::Db, ranking, sidecar::SidecarPool};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

const SUPPORTED: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "raf", "orf",
];

pub fn supported_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| SUPPORTED.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
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

#[derive(Serialize)]
pub struct AnalysisSummary {
    pub total: usize,
    pub failed: usize,
    pub cached: usize,
    pub photos: Vec<serde_json::Value>,
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

    let phashes: Vec<u64> = ok.iter().map(|f| f["phash"].as_u64().unwrap_or(0)).collect();
    let dup_ids = cluster::near_duplicate_clusters(&phashes, 4);

    let times: Vec<Option<i64>> = ok
        .iter()
        .map(|f| f["exif"]["captureDate"].as_f64().map(|t| t as i64))
        .collect();
    let event_ids = cluster::event_clusters(&times, 4 * 3600);

    let aesthetic = ranking::percentiles(
        &ok.iter().map(|f| f["aestheticScore"].as_f64().unwrap_or(0.0)).collect::<Vec<_>>(),
    );
    let sharpness = ranking::percentiles(
        &ok.iter().map(|f| f["sharpness"].as_f64().unwrap_or(0.0)).collect::<Vec<_>>(),
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

#[tauri::command]
pub async fn analyze_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
) -> Result<AnalysisSummary, String> {
    let mut paths: Vec<String> = std::fs::read_dir(&folder)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.is_file())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(supported_extension))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    paths.sort();

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
    std::fs::create_dir_all(&thumbnail_dir).map_err(|e| e.to_string())?;
    let thumbnail_dir = thumbnail_dir.to_string_lossy().into_owned();

    // Hash everything first (cheap, no decode), then send only cache misses to
    // the sidecar. Re-running on the same folder should do almost no work.
    // This ordering -- hash, then consult the cache, THEN call the sidecar --
    // is the entire point of having a cache at all.
    let mut ok: Vec<serde_json::Value> = Vec::new();
    let mut failed = 0usize;
    let mut cached = 0usize;
    let mut misses: Vec<String> = Vec::new();

    for path in &paths {
        match hash_file(Path::new(path)) {
            Ok(hash) => match db.get_features(&hash).map_err(|e| e.to_string())? {
                Some(json) => match serde_json::from_str(&json) {
                    Ok(features) => {
                        cached += 1;
                        ok.push(features);
                    }
                    Err(_) => misses.push(path.clone()),
                },
                None => misses.push(path.clone()),
            },
            Err(err) => {
                log::warn!("cannot hash {path}: {err}");
                failed += 1;
            }
        }
    }

    let records = {
        let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
        pool.analyze_all(&app, &misses, &thumbnail_dir)
    };

    for record in records {
        if record["status"] == "ok" {
            let features = record["features"].clone();
            if let (Some(hash), Some(path)) = (features["hash"].as_str(), features["path"].as_str())
            {
                db.put_features(hash, path, &features.to_string())
                    .map_err(|e| e.to_string())?;
            }
            ok.push(features);
        } else {
            failed += 1;
        }
    }

    let ok = finalize_photos(ok);

    Ok(AnalysisSummary { total: paths.len(), failed, cached, photos: ok })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_photo_extensions() {
        for name in ["a.jpg", "b.JPEG", "c.heic", "d.png", "e.CR2", "f.nef", "g.arw", "h.dng"] {
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
        assert!(result[0].get("phash").is_none(), "phash must not reach the webview");
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
            result[0]["aestheticPct"].as_u64().unwrap() < result[1]["aestheticPct"].as_u64().unwrap(),
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
        assert_eq!(result[0]["eventCluster"], 0, "chronologically-first photo gets the lower id");
        assert_eq!(result[1]["eventCluster"], 1);
    }
}
