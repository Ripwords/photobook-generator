//! Times the app's real analysis path (`analyze_folders`: Rust hashing, the
//! batched sidecar calls, finalize) over a folder, cold and warm.
//!
//! ```sh
//! bun run sidecar
//! cd src-tauri && cargo run --release --example analyze_bench -- ~/Pictures/Some\ Folder [runs]
//! ```
//!
//! `scripts/benchmark.sh` times the sidecar's stages one photo at a time;
//! this times what the user waits for. A cold run starts from an empty
//! feature cache and thumbnail directory; a warm run is every photo a cache
//! hit. The sidecar is spawned by a discarded first run, so cold runs time
//! analysis rather than process start. `HOME` points at a throwaway
//! directory, so the real app's cache is never touched.

use std::time::{Duration, Instant};

use app_lib::commands::analyze_folders;
use tauri::Manager;

fn main() {
    let mut args = std::env::args().skip(1);
    let folder = args.next().expect("usage: analyze_bench <folder> [runs]");
    let runs: usize = args.next().map_or(3, |n| n.parse().expect("runs must be a number"));

    let home = std::env::temp_dir().join(format!("pbg-analyze-bench-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("create bench HOME");
    // SAFETY: the first thing `main` does, before any other thread exists.
    unsafe {
        std::env::set_var("HOME", &home);
    }

    // `tauri-build` puts the sidecar beside the crate's own binaries, and the
    // shell plugin looks for it beside the running one, which for an example
    // is a directory lower.
    let exe_dir = std::env::current_exe().expect("current exe").parent().expect("exe dir").to_owned();
    let sidecar = exe_dir.join("photobook-engine");
    if !sidecar.exists() {
        std::fs::copy(exe_dir.join("../photobook-engine"), &sidecar)
            .expect("no sidecar beside target/<profile>; run `bun run sidecar` and a cargo build first");
    }

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = app_lib::builder().build(context).expect("build tauri app");
    let handle = app.handle().clone();
    let data_dir = handle.path().app_data_dir().expect("app data dir");

    let run = |label: &str| -> Duration {
        let started = Instant::now();
        let summary = tauri::async_runtime::block_on(analyze_folders(
            handle.clone(),
            vec![folder.clone()],
            tauri::ipc::Channel::new(|_| Ok(())),
        ))
        .expect("analysis failed");
        let elapsed = started.elapsed();
        println!(
            "{label}: {:.2?} for {} photos ({} cached, {} failed)",
            elapsed, summary.total, summary.cached, summary.failed
        );
        elapsed
    };
    let clear_cache = || {
        let _ = std::fs::remove_dir_all(&data_dir);
    };

    clear_cache();
    run("spawn (discarded)");

    let cold: Vec<Duration> = (1..=runs)
        .map(|i| {
            clear_cache();
            run(&format!("cold {i}"))
        })
        .collect();
    let warm: Vec<Duration> = (1..=runs).map(|i| run(&format!("warm {i}"))).collect();

    println!("cold median {:.2?}", median(cold));
    println!("warm median {:.2?}", median(warm));
    let _ = std::fs::remove_dir_all(&home);
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}
