//! Regression test for the worker-pool starvation deadlock documented in
//! `.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/thumbnails-report.md`:
//!
//! `Sidecar::request` does a *blocking* `std::sync::mpsc::Receiver::recv_timeout`
//! while it waits for the sidecar's stdout-drain task -- a tokio task spawned
//! in `Sidecar::spawn` -- to hand it the response line. Before the fix,
//! `analyze_folder` called this inline from its own body, which runs as a
//! task on the tauri/tokio async worker pool (it's an async
//! `#[tauri::command]`). On a machine with few worker threads that starves
//! the drain task of a thread to run on, so the blocking wait resolves only
//! once its OWN timeout elapses -- `analyze_folder` times out even though
//! the sidecar responded successfully.
//!
//! This test calls the real, unmodified `analyze_folder` command function
//! (not a reimplementation of its logic) against the real built sidecar
//! binary, spawned through `tauri_plugin_shell`/`AppHandle` exactly as
//! production does, on a deliberately single-worker-thread tokio runtime --
//! reproducing a low-core machine.
//!
//! It gives itself a real out-of-band watchdog: a raw `std::thread` plus a
//! `std::sync::mpsc::Receiver::recv_timeout`, independent of the (possibly
//! starved) constrained runtime under test, so a genuine hang fails this
//! test instead of hanging CI. A watchdog scheduled onto the same starved
//! pool cannot fire -- that mistake was made and caught once already in
//! this project (see the report above).
//!
//! ## Why this file has no `#[test]` (`harness = false` in `Cargo.toml`)
//!
//! `analyze_folder`, `Sidecar::spawn`, and `ShellExt::shell` are all typed
//! against the concrete default `AppHandle` (= `AppHandle<tauri::Wry>`), not
//! generic over `tauri::Runtime`. That means `tauri::test::mock_builder`'s
//! `MockRuntime`-backed `AppHandle` doesn't type-check as an argument to
//! `analyze_folder` -- a real `Wry` app is required to call the real
//! function. Building a real `Wry` app touches AppKit/tao and must happen on
//! the process's actual main thread; the default `#[test]` harness runs
//! each test on a worker thread instead, which would panic. `harness =
//! false` makes this whole file a plain `fn main()`, run directly by `cargo
//! test` on the real main thread, sidestepping that restriction. The window
//! list is cleared from the config before `.build()` so this doesn't
//! actually flash a window on screen.

use app_lib::commands::analyze_folder;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() {
    // Constrain tauri's global async runtime to exactly one worker thread,
    // mirroring a low-core production machine. This MUST happen before
    // anything else touches `tauri::async_runtime`: it lazily creates a
    // `num_cpus`-sized runtime on first use, held in a process-wide
    // `OnceLock`, and can only be configured once per process.
    let constrained = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("failed to build a constrained single-worker-thread tokio runtime");
    tauri::async_runtime::set(constrained.handle().clone());

    // Redirect HOME so `app_data_dir()` (and the sqlite cache / thumbnails
    // dir `analyze_folder` creates under it) lands in a throwaway temp
    // directory instead of the real user's `~/Library/Application Support`,
    // which could collide with a real running instance of this same app.
    let fake_home =
        std::env::temp_dir().join(format!("pbg-worker-pool-test-home-{}", std::process::id()));
    std::fs::create_dir_all(&fake_home).expect("failed to create fake HOME dir");
    // SAFETY: nothing else in this process reads or writes HOME concurrently
    // -- this is the very first thing `main` does, before any other thread
    // exists.
    unsafe {
        std::env::set_var("HOME", &fake_home);
    }

    // Real config (identifier, `bundle.externalBin`) is required so
    // `app.shell().sidecar("photobook-engine")` resolves to the actual built
    // binary the same way production does -- but the configured app window
    // is cleared so `.build()` doesn't try to put anything on screen.
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();

    // `app_lib::builder()` is the same plugin/state chain `run()` uses in
    // production (including `tauri-plugin-log`), so this test exercises the
    // real startup path rather than a hand-trimmed stand-in.
    let app = app_lib::builder().build(context).expect("failed to build tauri app");
    let handle = app.handle().clone();

    // Only the top-level fixture files are used (`std::fs::read_dir` is not
    // recursive), so the adversarial fixtures under
    // `sidecar/Fixtures/hostile/` are not exercised here -- this test is
    // about the worker pool, not decode robustness.
    let fixture_dir = format!("{}/../sidecar/Fixtures", env!("CARGO_MANIFEST_DIR"));

    // Watchdog: drive the reproduction on its own OS thread, independent of
    // the constrained runtime, and report back over a channel with a
    // timeout. Spawning the reproduction as a task ON the constrained
    // runtime (rather than just calling it inline on this watchdog thread)
    // is essential -- that is what puts `analyze_folder` on the worker pool
    // the way Tauri's own async command dispatch would.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let join =
            tauri::async_runtime::spawn(async move { analyze_folder(handle, fixture_dir).await });
        let result = tauri::async_runtime::block_on(join);
        let _ = tx.send((started.elapsed(), result));
    });

    let outcome = rx.recv_timeout(Duration::from_secs(60));
    let _ = std::fs::remove_dir_all(&fake_home);

    match outcome {
        Ok((elapsed, Ok(Ok(summary)))) => {
            assert!(
                summary.total > 0,
                "expected at least one supported photo in sidecar/Fixtures"
            );
            assert_eq!(summary.failed, 0, "no photo in sidecar/Fixtures should fail to analyze");
            // The per-batch timeout floor is 13s (10s + 3s/photo, see
            // `sidecar::timeout_for`), and a starved batch is retried once
            // more before giving up (see `sidecar::analyze_batches`) -- so a
            // starved run resolves only after roughly 2x that. A healthy run
            // over 3 small fixture photos should complete in a small
            // fraction of it.
            assert!(
                elapsed < Duration::from_secs(20),
                "analyze_folder took {elapsed:?} on a single-worker-thread \
                 runtime -- that is the drain-task starvation symptom (the \
                 sidecar request resolves only once its own timeout \
                 elapses), even though it nominally 'succeeded' here"
            );
            println!("ok: analyze_folder completed in {elapsed:?} on a single-worker-thread runtime");
        }
        Ok((elapsed, Ok(Err(e)))) => {
            panic!("analyze_folder returned an error after {elapsed:?}: {e}")
        }
        Ok((elapsed, Err(join_err))) => {
            panic!("analyze_folder task panicked after {elapsed:?}: {join_err}")
        }
        Err(_) => panic!(
            "analyze_folder did not complete within 60s on a single-worker-\
             thread tokio runtime -- the async worker pool is starved (the \
             stdout-drain task spawned in Sidecar::spawn cannot be scheduled \
             while Sidecar::request blocks a worker thread on \
             recv_timeout). This is the exact deadlock documented in \
             thumbnails-report.md."
        ),
    }
}
