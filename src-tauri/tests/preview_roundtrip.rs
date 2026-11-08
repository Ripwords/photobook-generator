//! Drives the REAL sidecar with a `.preview` request, for the same reason
//! `export_roundtrip.rs` exists: Swift's `PreviewRequest` uses a synthesised
//! `Codable`, so a drifted key (`maxPixel` vs `max_pixel`) fails the decode
//! of the whole request, and each side's own tests, judged against a copy of
//! the contract, would stay green.

use app_lib::protocol::{
    PreviewItem, PreviewRequest, Request, RequestKind, Response, ResponseResult,
};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn real_sidecar_writes_a_preview_per_item_and_null_for_a_missing_file() {
    let binary = format!(
        "{}/binaries/photobook-engine-{}",
        env!("CARGO_MANIFEST_DIR"),
        env!("TAURI_ENV_TARGET_TRIPLE")
    );
    let dir = std::env::temp_dir().join(format!("preview-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let landscape = format!("{}/../sidecar/Fixtures/landscape.jpg", env!("CARGO_MANIFEST_DIR"));

    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {binary}: {e}"));
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let req = Request {
        id: "preview-1".into(),
        kind: RequestKind::Preview,
        paths: None,
        thumbnail_dir: None,
        export: None,
        coordinates: None,
        preview: Some(PreviewRequest {
            directory: dir.to_string_lossy().into_owned(),
            max_pixel: 600,
            items: vec![
                PreviewItem { path: "/Volumes/Gone/missing.ARW".into(), hash: "dd".into() },
                PreviewItem { path: landscape, hash: "ee".into() },
            ],
        }),
    };
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    stdin.write_all(line.as_bytes()).unwrap();
    stdin.flush().unwrap();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let result = BufReader::new(stdout).read_line(&mut out);
        let _ = tx.send(result.map(|_| out));
    });
    let text = rx.recv_timeout(Duration::from_secs(20)).expect("sidecar answered").unwrap();
    let _ = child.kill();
    let _ = child.wait();

    let res: Response = serde_json::from_str(&text).unwrap();
    assert_eq!(res.id, "preview-1");
    let expected = dir.join("ee.jpg");
    match res.result {
        ResponseResult::Previewed(paths) => {
            assert_eq!(paths, vec![None, Some(expected.to_string_lossy().into_owned())]);
        }
        other => panic!("expected previewed, got {other:?}"),
    }
    assert!(std::fs::metadata(&expected).unwrap().len() > 0);
    let _ = std::fs::remove_dir_all(&dir);
}
