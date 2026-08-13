### Task 15: The analyze_folder command and results UI

**Files:**
- Create: `src-tauri/src/commands.rs`, `app/types/features.ts`, `app/composables/useAnalysis.ts`
- Modify: `src-tauri/src/lib.rs`, `app/pages/index.vue`
- Test: `tests/features.test.ts`, inline `#[cfg(test)]` in `commands.rs`

**Interfaces:**
- Consumes: `Db`, `SidecarPool`, `cluster`, `ranking`
- Produces:
  - Rust command `analyze_folder(folder: String) -> Result<AnalysisSummary, String>`
  - `pub fn supported_extension(name: &str) -> bool`
  - `pub fn hash_file(path: &Path) -> std::io::Result<String>` — SHA-256 over raw bytes, no image decode
  - TypeScript `AnalyzedPhoto` and `AnalysisSummary` types, and `useAnalysis()` exposing `{ summary, running, error, pickFolderAndAnalyze }`

Rust hashes the files itself rather than waiting for the sidecar to report hashes. Hashing raw bytes needs no decoder, so the cache can be consulted *before* any sidecar work happens — which is the whole point of having one. The sidecar's own `hash` field is used only to key what it writes back, and the two agree because both are SHA-256 over the same bytes.

- [ ] **Step 1: Write the failing test**

`tests/features.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { isFailed, keepers, type AnalyzedPhoto } from "../app/types/features";

const photo = (over: Partial<AnalyzedPhoto> = {}): AnalyzedPhoto => ({
  status: "ok",
  path: "/p/a.jpg",
  hash: "h",
  width: 4032,
  height: 3024,
  isUtility: false,
  aestheticPct: 50,
  sharpnessPct: 50,
  faceCount: 0,
  smileFraction: null,
  sceneTags: [],
  nearDupCluster: 0,
  eventCluster: 0,
  ...over,
});

describe("feature helpers", () => {
  it("identifies failed records", () => {
    expect(isFailed({ status: "failed", path: "/p/b.jpg", message: "boom" })).toBe(true);
    expect(isFailed(photo())).toBe(false);
  });

  it("drops utility photos from keepers", () => {
    const result = keepers([photo(), photo({ isUtility: true, path: "/p/shot.png" })]);
    expect(result).toHaveLength(1);
  });

  it("keeps one photo per near-duplicate cluster", () => {
    const result = keepers([
      photo({ path: "/p/1.jpg", nearDupCluster: 7, sharpnessPct: 40 }),
      photo({ path: "/p/2.jpg", nearDupCluster: 7, sharpnessPct: 90 }),
      photo({ path: "/p/3.jpg", nearDupCluster: 8, sharpnessPct: 10 }),
    ]);
    expect(result).toHaveLength(2);
    expect(result.find((p) => p.nearDupCluster === 7)?.path).toBe("/p/2.jpg");
  });

  it("returns an empty array for no input", () => {
    expect(keepers([])).toEqual([]);
  });
});
```

Append to `src-tauri/src/commands.rs`:

```rust
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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `bun run test && cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — cannot resolve `../app/types/features`; `file not found for module 'commands'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/commands.rs` (above the test module):

```rust
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

    let db_path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("photobook.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    let db = Db::open(&db_path).map_err(|e| e.to_string())?;

    // Hash everything first (cheap, no decode), then send only cache misses to
    // the sidecar. Re-running on the same folder should do almost no work.
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
        pool.analyze_all(&app, &misses)
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

    // Cached and freshly-analysed photos are interleaved arbitrarily above;
    // restore folder order so the UI is stable across runs.
    ok.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));

    // Derive the book-relative signals Rust owns.
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
        // above 2^53. Clustering is done with it by this point, and the UI has
        // no use for it, so drop it rather than hand the webview a value that
        // is silently wrong for anyone who later reads it.
        if let Some(object) = features.as_object_mut() {
            object.remove("phash");
        }
    }

    Ok(AnalysisSummary { total: paths.len(), failed, cached, photos: ok })
}
```

Update `src-tauri/src/lib.rs`:

```rust
pub mod cluster;
pub mod commands;
pub mod db;
pub mod protocol;
pub mod ranking;
pub mod sidecar;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![commands::analyze_folder])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`app/types/features.ts`:

```ts
export interface AnalyzedPhoto {
  status: "ok";
  path: string;
  hash: string;
  width: number;
  height: number;
  isUtility: boolean;
  aestheticPct: number;
  sharpnessPct: number;
  faceCount: number;
  smileFraction: number | null;
  sceneTags: string[];
  nearDupCluster: number;
  eventCluster: number;
}

export interface FailedPhoto {
  status: "failed";
  path: string;
  message: string;
}

export type PhotoRecord = AnalyzedPhoto | FailedPhoto;

export interface AnalysisSummary {
  total: number;
  failed: number;
  cached: number;
  photos: AnalyzedPhoto[];
}

export function isFailed(record: PhotoRecord): record is FailedPhoto {
  return record.status === "failed";
}

/**
 * Drops utility images, then keeps the best photo from each near-duplicate
 * cluster, ranked by sharpness then aesthetic percentile.
 */
export function keepers(photos: AnalyzedPhoto[]): AnalyzedPhoto[] {
  const best = new Map<number, AnalyzedPhoto>();

  for (const photo of photos) {
    if (photo.isUtility) continue;
    const incumbent = best.get(photo.nearDupCluster);
    if (
      !incumbent ||
      photo.sharpnessPct > incumbent.sharpnessPct ||
      (photo.sharpnessPct === incumbent.sharpnessPct &&
        photo.aestheticPct > incumbent.aestheticPct)
    ) {
      best.set(photo.nearDupCluster, photo);
    }
  }

  return [...best.values()];
}
```

`app/composables/useAnalysis.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AnalysisSummary } from "~/types/features";

export function useAnalysis() {
  const summary = ref<AnalysisSummary | null>(null);
  const running = ref(false);
  const error = ref<string | null>(null);

  async function pickFolderAndAnalyze() {
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== "string") return;

    running.value = true;
    error.value = null;
    try {
      summary.value = await invoke<AnalysisSummary>("analyze_folder", { folder });
    } catch (e) {
      error.value = String(e);
    } finally {
      running.value = false;
    }
  }

  return { summary, running, error, pickFolderAndAnalyze };
}
```

`app/pages/index.vue`:

```vue
<script setup lang="ts">
import { keepers } from "~/types/features";

const { summary, running, error, pickFolderAndAnalyze } = useAnalysis();
const kept = computed(() => (summary.value ? keepers(summary.value.photos) : []));
</script>

<template>
  <main class="p-8 space-y-6">
    <div class="flex items-center gap-4">
      <h1 class="text-2xl font-semibold">Photobook Generator</h1>
      <UButton :loading="running" @click="pickFolderAndAnalyze">
        Choose photo folder
      </UButton>
    </div>

    <UAlert v-if="error" color="error" :title="error" />

    <div v-if="summary" class="space-y-4">
      <div class="flex gap-6 text-sm">
        <span>{{ summary.total }} scanned</span>
        <span>{{ summary.photos.length }} analysed</span>
        <span>{{ summary.cached }} from cache</span>
        <span>{{ summary.failed }} failed</span>
        <span class="font-medium">{{ kept.length }} keepers</span>
      </div>

      <UTable
        :rows="kept"
        :columns="[
          { key: 'path', label: 'Photo' },
          { key: 'aestheticPct', label: 'Aesthetic' },
          { key: 'sharpnessPct', label: 'Sharpness' },
          { key: 'faceCount', label: 'Faces' },
          { key: 'eventCluster', label: 'Event' },
        ]"
      />
    </div>
  </main>
</template>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `bun run test && cargo test --manifest-path src-tauri/Cargo.toml && bun run lint`
Expected: PASS — 4 TS tests, 5 Rust tests in this module, no lint output

Then the real end-to-end check:

```bash
bun run sidecar && bun run dev
```

Click "Choose photo folder", select a folder of **your own photos including HEIC and RAW**, and confirm a keepers table appears.

Three things to verify by hand, all of which the automated tests cannot cover:

1. **Time the first run and record it** — this is the RAW throughput measurement flagged in spec §14. Note the photo count, the format mix, and the elapsed time.
2. **Run it a second time on the same folder.** "from cache" must equal the analysed count and the run should complete in roughly the time it takes to hash the files. If it doesn't, the cache is not working and the sidecar hash and the Rust hash have diverged.
3. **Spot-check three portrait photos** in the table. Their `width` must be smaller than their `height`. If any portrait photo reports landscape dimensions, EXIF orientation is not being applied and every layout downstream will be wrong.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add analyze_folder command and results view"
```

---

## Phase 1 exit criteria

- [ ] `bun run test`, `cargo test`, and `swift test` all pass
- [ ] `bun run lint` produces no output
- [ ] The hostile fixture corpus runs to completion with exit code 0
- [ ] A folder of real photos including HEIC and RAW analyses end-to-end
- [ ] **RAW throughput measured and recorded** in the spec's §14 table
- [ ] `bun run build` produces a signed, launchable `.app` with the sidecar embedded

## Deferred to a later plan

- **SigLIP 2 zero-shot mood axes.** Deliberately excluded. It needs a Core ML conversion, a designed evidence vocabulary, and measurement against ~200 of your own photos before it earns a place. Everything above is useful without it, and spec §6.3 already flags it as the weakest signal in the pipeline.
- **Palette harmony scoring** (Matsuda templates) — belongs with the layout engine that consumes it, in phase 2.
- **GPS-based location clustering** — currently only time-gap clustering is implemented. Add when chapters need it.
