//! Regression test for C2: "cache hits carry a stale `path`". `lookup_cache`
//! used to return the cached `features` JSON verbatim, including whatever
//! `path` the file had when it was FIRST analysed. This has an
//! `commands.rs`-level unit test
//! (`a_cache_hit_carries_the_current_path_not_the_stale_cached_one`, which
//! pins the CURRENT-path guarantee directly and precisely with no live
//! `AppHandle` needed) -- this file covers the other half named in the
//! brief: a full, real `analyze_folder` run over the same folder twice must
//! actually populate `summary.cached`. The only prior end-to-end test
//! (`sidecar_worker_pool.rs`) redirects `HOME` to a FRESH temp dir every
//! run, so the cache-hit branch of `lookup_cache` had never executed
//! end-to-end before either of these two tests existed.
//!
//! Needs a full `analyze_folder` command against a live `AppHandle` and the
//! real sidecar binary -- same reasoning and same `harness = false` setup as
//! `sidecar_worker_pool.rs` (see that file's doc comment for the full
//! explanation of why `#[test]` cannot be used here).

use app_lib::commands::analyze_folders;

fn main() {
    // Redirect HOME so the sqlite cache and thumbnails dir land in a
    // throwaway temp directory, same as sidecar_worker_pool.rs -- but,
    // unlike that test, this one deliberately reuses the SAME fake HOME
    // across two calls to `analyze_folder`, because the whole point is
    // observing the cache survive between runs.
    let fake_home =
        std::env::temp_dir().join(format!("pbg-cache-hit-test-home-{}", std::process::id()));
    std::fs::create_dir_all(&fake_home).expect("failed to create fake HOME dir");
    // SAFETY: nothing else in this process reads or writes HOME concurrently
    // -- this is the very first thing `main` does, before any other thread
    // exists (same justification as sidecar_worker_pool.rs).
    unsafe {
        std::env::set_var("HOME", &fake_home);
    }

    // An isolated copy of the fixtures, not `sidecar/Fixtures` directly, so
    // this test cannot race `sidecar_worker_pool.rs`'s own `analyze_folder`
    // call over the exact same source files if `cargo test` ever runs test
    // binaries in parallel.
    let source_dir = format!("{}/../sidecar/Fixtures", env!("CARGO_MANIFEST_DIR"));
    let fixture_dir = std::env::temp_dir().join(format!(
        "pbg-cache-hit-test-fixtures-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&fixture_dir).expect("failed to create fixture copy dir");
    for entry in std::fs::read_dir(&source_dir).expect("read source fixtures") {
        let entry = entry.expect("dir entry");
        if entry.path().is_file() {
            std::fs::copy(entry.path(), fixture_dir.join(entry.file_name())).expect("copy fixture");
        }
    }
    let fixture_dir_str = fixture_dir.to_string_lossy().into_owned();

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();

    let app = app_lib::builder()
        .build(context)
        .expect("failed to build tauri app");
    let handle = app.handle().clone();

    // A no-op streaming channel: this test only cares about the final
    // `AnalysisSummary`, not the progress events `analyze_folder` now emits
    // alongside it.
    let first = tauri::async_runtime::block_on(analyze_folders(
        handle.clone(),
        vec![fixture_dir_str.clone()],
        tauri::ipc::Channel::new(|_| Ok(())),
    ))
    .expect("first analyze_folder run failed");
    assert!(
        first.total > 0,
        "expected at least one supported photo in sidecar/Fixtures"
    );
    assert_eq!(
        first.cached, 0,
        "first run over an empty cache must have zero cache hits"
    );

    let second = tauri::async_runtime::block_on(analyze_folders(
        handle,
        vec![fixture_dir_str],
        tauri::ipc::Channel::new(|_| Ok(())),
    ))
    .expect("second analyze_folder run failed");
    assert_eq!(
        second.total, first.total,
        "the folder did not change between runs"
    );
    assert!(
        second.cached > 0,
        "second run over the SAME folder must hit the cache -- got {} cached of {} total \
         (this is the exact end-to-end path the stale-path bug lived on, and the only \
         prior end-to-end test never exercised it because it uses a fresh HOME every run)",
        second.cached,
        second.total
    );

    // Every returned photo's path must be the CURRENT path passed to THIS
    // run, not a path resurrected from a stale cache row -- the precise
    // regression this test and `a_cache_hit_carries_the_current_path_not_the_stale_cached_one`
    // (in commands.rs, which uses two DIFFERENT paths with identical bytes
    // to isolate this exact guarantee) both exist to cover.
    let fixture_dir_prefix = fixture_dir.to_string_lossy().into_owned();
    for photo in &second.photos {
        let path = photo
            .get("path")
            .and_then(|p| p.as_str())
            .expect("every ok photo record must have a path");
        assert!(
            path.starts_with(&fixture_dir_prefix),
            "cached photo path {path} does not point into the current fixture_dir {fixture_dir_prefix} \
             -- looks like a stale path leaked through from the cache"
        );
    }

    let _ = std::fs::remove_dir_all(&fake_home);
    let _ = std::fs::remove_dir_all(&fixture_dir);

    println!(
        "ok: second analyze_folder run over the same folder reported {} cached of {} total, \
         all with fresh paths",
        second.cached, second.total
    );
}
