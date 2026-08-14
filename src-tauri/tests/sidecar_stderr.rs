//! Proves `Sidecar::spawn`'s drain loop forwards the child's stderr to the
//! `log` crate instead of silently dropping it.
//!
//! Before this fix, the drain loop in `sidecar.rs` matched only
//! `CommandEvent::Stdout`, so every diagnostic the Swift sidecar writes to
//! stderr -- including the warning `SmileProxy.swift` writes when Vision's
//! `outerLips` region returns fewer points than its floor, which is the
//! designated way to detect that `smile_fraction` silently comes back `nil`
//! for the whole library -- vanished into nothing. See
//! `docs/PROJECT-STATUS.md`'s "Vision's `outerLips` point count" entry and
//! `SmileProxy.swift`'s `minOuterLipPoints` doc comment.
//!
//! This drives the REAL `Sidecar::spawn` + `Sidecar::request` against the
//! real built sidecar binary, through a real `AppHandle` built the same way
//! production does (`app_lib::builder()`, which registers `tauri-plugin-log`
//! exactly as `run()` does) -- so this test observes the exact thing a user
//! would see in `~/Library/Logs/com.jiajingteoh.photobook/PhotobookGen.log`,
//! with `HOME` redirected to a throwaway temp dir so it doesn't touch the
//! real log file. `harness = false` is required for the same reason as
//! `sidecar_worker_pool.rs`/`cache_hit.rs`: `Sidecar::spawn` needs a real
//! `Wry` `AppHandle`, and building one must happen on the process's actual
//! main thread, which the default `#[test]` harness does not guarantee.
//!
//! The trigger is `ThumbnailWriter.write` failing (via
//! `Analyzer.analyzeOne`), not `SmileProxy`'s `outerLips` warning: the
//! latter needs a real face landmark under the point-count floor, which no
//! fixture can reliably produce (Vision's face detector cannot be reliably
//! tripped by a synthetic image -- see PROJECT-STATUS.md's "whole face code
//! path" note). Pointing `thumbnailDir` at a path that already exists as a
//! plain file makes `FileManager.createDirectory` throw deterministically,
//! which `Analyzer.analyzeOne` catches and reports to stderr as
//! `"PhotobookEngine: thumbnail write failed for ...: ..."` -- a real
//! diagnostic write on the exact same stderr stream, just easier to trigger
//! on demand than the outerLips one.
//!
//! Also covers the `sidecar_ping.rs` re-assessment the task called for: that
//! test already drains its own stderr pipe on a background thread rather
//! than leaving it unread, so a chatty sidecar cannot fill the pipe and
//! block the child there -- nothing to fix in that file.

use app_lib::protocol::RequestKind;
use app_lib::sidecar::Sidecar;
use std::time::Duration;
use tauri::Manager;

fn main() {
    // Redirect HOME so tauri-plugin-log's rotating file lands in a
    // throwaway temp directory instead of the user's real
    // `~/Library/Logs/com.jiajingteoh.photobook/`, same technique as
    // `sidecar_worker_pool.rs` and `cache_hit.rs`.
    let fake_home =
        std::env::temp_dir().join(format!("pbg-stderr-test-home-{}", std::process::id()));
    std::fs::create_dir_all(&fake_home).expect("failed to create fake HOME dir");
    // SAFETY: nothing else in this process reads or writes HOME concurrently
    // -- this is the very first thing `main` does, before any other thread
    // exists (same justification as the other harness=false tests).
    unsafe {
        std::env::set_var("HOME", &fake_home);
    }

    let fixture_path =
        format!("{}/../sidecar/Fixtures/landscape.jpg", env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::path::Path::new(&fixture_path).exists(),
        "fixture image missing at {fixture_path}"
    );

    // A regular file, not a directory, at the path handed to the sidecar as
    // `thumbnailDir`. `FileManager.createDirectory` throws when the target
    // path exists and is not itself a directory -- exactly the write
    // failure `Analyzer.analyzeOne` catches and reports to stderr.
    let bogus_thumbnail_dir = std::env::temp_dir().join(format!(
        "pbg-stderr-test-not-a-directory-{}",
        std::process::id()
    ));
    std::fs::write(&bogus_thumbnail_dir, b"not a directory")
        .expect("create the file that blocks the thumbnail directory");
    let bogus_thumbnail_dir_str = bogus_thumbnail_dir.to_string_lossy().into_owned();

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();

    // The same plugin chain `run()` uses in production, including
    // `tauri-plugin-log` -- so this exercises the real logging path a user's
    // log file would show, not a hand-rolled stand-in.
    let app = app_lib::builder()
        .build(context)
        .expect("failed to build tauri app");
    let handle = app.handle().clone();

    let mut sidecar = Sidecar::spawn(&handle).expect("failed to spawn sidecar");
    let result = sidecar.request(
        RequestKind::Analyze,
        Some(vec![fixture_path]),
        Some(bogus_thumbnail_dir_str.clone()),
        None,
        Duration::from_secs(30),
    );
    // The request itself must still succeed -- a failed thumbnail write
    // degrades to a nil thumbnail path for that one photo, not a failed
    // request. If this doesn't hold, the trigger stopped matching the
    // documented behavior and the test needs a different one, not a
    // loosened assertion below.
    result.expect("analyze request should succeed even though the thumbnail write fails");
    drop(sidecar); // flush/close the child before reading the log file back.

    let log_dir = handle
        .path()
        .app_log_dir()
        .expect("resolve app_log_dir under the fake HOME");
    let mut combined_log = String::new();
    if let Ok(entries) = std::fs::read_dir(&log_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("log") {
                combined_log.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
    }

    let _ = std::fs::remove_dir_all(&fake_home);
    let _ = std::fs::remove_file(&bogus_thumbnail_dir);

    assert!(
        combined_log.contains("sidecar stderr:") && combined_log.contains("thumbnail write failed"),
        "expected the app log under {log_dir:?} to contain a 'sidecar stderr:'-prefixed line \
         about the thumbnail write failure for {bogus_thumbnail_dir_str}, but it did not \
         (log content follows):\n{combined_log}"
    );

    println!("ok: sidecar stderr diagnostic reached the app log via log::warn!");
}
