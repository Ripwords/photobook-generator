//! Proves `analyze_folder` actually streams -- not just that it accepts a
//! `Channel` parameter without crashing (`cache_hit.rs` and
//! `sidecar_worker_pool.rs` already cover that with a no-op channel).
//! Against the real sidecar and the real top-level `sidecar/Fixtures/`
//! photos, this asserts the wire-level ordering the whole feature depends
//! on: at least one `Batch` event, carrying real partial photos, arrives
//! strictly BEFORE the single `Done` event -- i.e. the frontend has
//! something to render before the run finishes, not just at the end.
//!
//! Needs a full `analyze_folder` command against a live `AppHandle` and the
//! real sidecar binary -- same reasoning and same `harness = false` setup as
//! `sidecar_worker_pool.rs` and `cache_hit.rs` (see those files' doc
//! comments for the full explanation of why `#[test]` cannot be used here).

use app_lib::commands::analyze_folders;
use tauri::ipc::{Channel, InvokeResponseBody};

fn main() {
    let fake_home = std::env::temp_dir().join(format!(
        "pbg-streaming-order-test-home-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&fake_home).expect("failed to create fake HOME dir");
    // SAFETY: nothing else in this process reads or writes HOME concurrently
    // -- this is the very first thing `main` does, before any other thread
    // exists (same justification as the other `harness = false` tests).
    unsafe {
        std::env::set_var("HOME", &fake_home);
    }

    // An isolated copy of the TOP-LEVEL fixtures only. `analyze_folder` now
    // walks subfolders, and `sidecar/Fixtures/hostile/` holds deliberately
    // broken images that are meant to fail -- this test is about event ordering,
    // not decode robustness, so they are left out.
    let source_dir = format!("{}/../sidecar/Fixtures", env!("CARGO_MANIFEST_DIR"));
    let fixture_copy = std::env::temp_dir().join(format!(
        "pbg-streaming-order-test-fixtures-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&fixture_copy).expect("failed to create fixture copy dir");
    for entry in std::fs::read_dir(&source_dir).expect("read source fixtures") {
        let entry = entry.expect("dir entry");
        if entry.path().is_file() {
            std::fs::copy(entry.path(), fixture_copy.join(entry.file_name())).expect("copy fixture");
        }
    }
    let fixture_dir = fixture_copy.to_string_lossy().into_owned();

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = app_lib::builder()
        .build(context)
        .expect("failed to build tauri app");
    let handle = app.handle().clone();

    // Collects the `kind` of every event, in arrival order, plus how many
    // partial photos each `batch` event carried.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, usize)>::new()));
    let seen_for_channel = seen.clone();
    let on_event: Channel<app_lib::commands::AnalysisEvent> = Channel::new(move |body| {
        let InvokeResponseBody::Json(text) = body else {
            panic!("expected a JSON channel payload, got {body:?}");
        };
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON event");
        let kind = value["kind"]
            .as_str()
            .expect("event has a kind")
            .to_string();
        let photo_count = value["photos"].as_array().map_or(0, Vec::len);
        seen_for_channel.lock().unwrap().push((kind, photo_count));
        Ok(())
    });

    let summary = tauri::async_runtime::block_on(analyze_folders(handle, vec![fixture_dir], on_event))
        .expect("analyze_folder run failed");

    let _ = std::fs::remove_dir_all(&fake_home);
    let _ = std::fs::remove_dir_all(&fixture_copy);

    let events = seen.lock().unwrap().clone();
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();

    assert_eq!(
        kinds.first(),
        Some(&"scanned"),
        "the very first event must be `scanned`, got {kinds:?}"
    );
    assert_eq!(
        kinds.last(),
        Some(&"done"),
        "the very last event must be `done`, got {kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "done").count(),
        1,
        "`done` must fire exactly once, got {kinds:?}"
    );

    let batch_events: Vec<&(String, usize)> = events.iter().filter(|(k, _)| k == "batch").collect();
    assert!(
        !batch_events.is_empty(),
        "expected at least one `batch` event before `done`, got {kinds:?}"
    );
    let streamed_photo_total: usize = batch_events.iter().map(|(_, n)| n).sum();
    assert!(
        streamed_photo_total > 0,
        "at least one `batch` event must carry real partial photos, not just counts"
    );

    // The property the whole feature is FOR: everything the final summary
    // reports as successfully analysed must already have been streamed via
    // `batch` events before `done` fired. If this were off, the frontend
    // would end up with tiles missing from the grid it built up during
    // `running`, silently contradicted by the final summary.
    assert_eq!(
        streamed_photo_total,
        summary.photos.len(),
        "every photo in the final summary must have been streamed via a `batch` event first"
    );

    println!(
        "ok: analyze_folder streamed {} batch event(s) totalling {streamed_photo_total} photos, \
         strictly before the single `done` event ({} photos in the final summary)",
        batch_events.len(),
        summary.photos.len()
    );
}
