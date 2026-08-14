//! Integration test that drives the real built Swift sidecar binary directly
//! (not through `tauri_plugin_shell`/`AppHandle`) to verify the cross-language
//! NDJSON wire format actually agrees end-to-end: Rust's
//! `#[serde(tag = "type", content = "data")]` encoding/decoding must match
//! Swift's hand-rolled `Codable` implementation in `Protocol.swift`.
//!
//! `tauri-build`'s build script validates at compile time that the sidecar
//! binary exists at `src-tauri/binaries/photobook-engine-<triple>` (required
//! by the `bundle.externalBin` entry in `tauri.conf.json`), so by the time
//! this test binary is compiled the binary is guaranteed to be present.

use app_lib::protocol::{Request, RequestKind, Response, ResponseResult};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn sidecar_binary_responds_to_ping_with_matching_id_and_version() {
    // Resolve via CARGO_MANIFEST_DIR (the src-tauri crate root) rather than a
    // relative path, so the test works regardless of the working directory
    // the test runner uses. Resolve the triple the same way tauri-build did
    // when it validated the resource path at compile time, rather than
    // hardcoding it.
    let triple = env!("TAURI_ENV_TARGET_TRIPLE");
    let binary_path =
        format!("{}/binaries/photobook-engine-{}", env!("CARGO_MANIFEST_DIR"), triple);

    let mut child = Command::new(&binary_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn sidecar binary at {binary_path}: {e}"));

    let mut stdin = child.stdin.take().expect("child stdin was piped");
    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");

    // Drain stderr on its own thread so a chatty sidecar can't fill the pipe
    // buffer and block the child on write, which would otherwise stall it
    // before it ever gets to read stdin or write a response.
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            line.clear();
        }
    });

    let req = Request {
        id: "integration-1".into(),
        kind: RequestKind::Ping,
        paths: None,
        thumbnail_dir: None,
        export: None,
    };
    let mut line = serde_json::to_string(&req).expect("serialize request");
    line.push('\n');
    stdin.write_all(line.as_bytes()).expect("write request to sidecar stdin");
    stdin.flush().expect("flush sidecar stdin");

    // Read one line on a background thread and join it through a channel so
    // a hung or crashed sidecar fails this test in seconds via
    // recv_timeout, instead of hanging CI forever on a blocking read.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut out = String::new();
        let result = reader.read_line(&mut out);
        let _ = tx.send(result.map(|_| out));
    });

    let response_line = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => {
            let _ = child.kill();
            panic!("failed reading sidecar stdout: {e}");
        }
        Err(_) => {
            let _ = child.kill();
            panic!("sidecar did not respond within 5s (binary: {binary_path})");
        }
    };

    let response: Response = serde_json::from_str(response_line.trim())
        .unwrap_or_else(|e| panic!("malformed response line {response_line:?}: {e}"));

    assert_eq!(response.id, "integration-1");
    match response.result {
        ResponseResult::Pong { version } => assert!(!version.is_empty(), "expected non-empty version"),
        other => panic!("expected Pong, got {other:?}"),
    }

    let _ = child.kill();
    let _ = child.wait();
}
