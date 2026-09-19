//! The one test in this project that drives the REAL export wire end to end.
//!
//! Every other export test exercises one side against a fixture of the other:
//! `protocol.rs`'s unit tests assert the literal JSON bytes Rust emits,
//! `ExporterTests.swift` feeds Swift a hand-built `ExportRequest`, and
//! `export.rs` builds `ExportItem`s that no sidecar ever sees. None of them
//! can catch a *disagreement* between the two sides, because each one is
//! judged against a copy of the contract rather than against the other
//! implementation.
//!
//! That gap matters here specifically. Swift's `ExportItem` uses a
//! SYNTHESISED `Codable`, so a single drifted key name (`cropW` vs `crop_w`,
//! `sourcePath` vs `source_path`) throws `keyNotFound` while decoding the
//! whole `Request`, and the sidecar answers with one opaque error for the
//! entire batch. Rust's export path then reports "every photo failed" with
//! no cause attributed to any field. This test is the only place that
//! failure is reproducible before a user hits it.
//!
//! **The fixtures are deliberately non-square (1200x800), and the crop
//! windows deliberately asymmetric**, so the assertions below fail under a
//! swap as well as under a rename:
//!
//! - a `cropW`/`cropH` swap changes the output pixel dimensions;
//! - a `cropX`/`cropY` swap pushes item 1's window past the bottom edge
//!   (`0.7 + 0.85 > 1`), which the sidecar rejects as out-of-range, turning
//!   its `ok` record into a `failed` one;
//! - a `sourcePath` rename fails the decode outright.
//!
//! A square fixture or a centred crop would hide all three.
//!
//! Dimensions and container are read back from the WRITTEN FILE's own bytes
//! by the tiny parsers at the bottom of this file, not taken from the
//! sidecar's `ExportRecord`. The record is the sidecar's claim about what it
//! did; the file is what the user uploads to the printer. Both are asserted,
//! and they must agree.
//!
//! Uses the default test harness (like `sidecar_ping.rs`): this drives the
//! binary over a pipe and needs no `AppHandle`, so none of the
//! `harness = false` reasoning in `Cargo.toml` applies.

use app_lib::protocol::{ExportItem, ExportRequest, Request, RequestKind, Response, ResponseResult};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Both fixtures are this size. Every expected dimension below is derived
/// from it rather than hardcoded, so the arithmetic the sidecar performs is
/// mirrored explicitly and a fixture swap cannot silently invalidate the
/// expectations.
const FIXTURE_W: f64 = 1200.0;
const FIXTURE_H: f64 = 800.0;

fn fixture(name: &str) -> String {
    format!("{}/../sidecar/Fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn real_sidecar_crops_and_writes_files_matching_the_requested_windows() {
    let jpeg_source = fixture("export-quadrants.jpg");
    let png_source = fixture("export-quadrants.png");
    for source in [&jpeg_source, &png_source] {
        assert!(Path::new(source).exists(), "fixture missing at {source}");
    }

    // `TempDir` removes the tree on drop, including when an assertion below
    // panics, so a failing run leaves nothing behind in /tmp.
    let temp = tempfile::tempdir().expect("create temp dir");
    // A subdirectory that does NOT exist yet: the sidecar is contracted to
    // create its own output directory (`Exporter.export`), so leaving it
    // absent means this test also catches that being dropped.
    let output_dir = temp.path().join("export");
    let output_dir_str = output_dir.to_string_lossy().into_owned();

    // Item 1: a window in the lower-right region. `0.7 + 0.85 > 1` makes an
    // x/y swap out-of-range rather than merely wrong.
    let jpeg_item = ExportItem {
        source_path: jpeg_source.clone(),
        filename: "p04-z1-roundtrip".into(),
        crop_x: 0.7,
        crop_y: 0.1,
        crop_w: 0.25,
        crop_h: 0.85,
    };
    // Item 2: a different window again, so two records that got confused for
    // one another cannot both pass.
    let png_item = ExportItem {
        source_path: png_source.clone(),
        filename: "p04-z2-roundtrip".into(),
        crop_x: 0.1,
        crop_y: 0.05,
        crop_w: 0.5,
        crop_h: 0.6,
    };

    // The sidecar's own arithmetic, restated: origin and size are each
    // rounded to whole pixels, then the size is clamped to what is left
    // inside the image. Written out rather than reduced to constants so a
    // reader can check it against `Exporter.cropRect` line for line.
    let expect = |item: &ExportItem| -> (u32, u32) {
        let x = (item.crop_x * FIXTURE_W).round();
        let y = (item.crop_y * FIXTURE_H).round();
        let w = (item.crop_w * FIXTURE_W).round().min(FIXTURE_W - x).max(1.0);
        let h = (item.crop_h * FIXTURE_H).round().min(FIXTURE_H - y).max(1.0);
        (w as u32, h as u32)
    };
    let (jpeg_w, jpeg_h) = expect(&jpeg_item);
    let (png_w, png_h) = expect(&png_item);
    // Guards against an expectation that is accidentally square or
    // accidentally the full frame -- either would defeat the point of the
    // fixture choice documented at the top of this file.
    assert_ne!(jpeg_w, jpeg_h, "crop must be non-square to expose an axis swap");
    assert_ne!(png_w, png_h, "crop must be non-square to expose an axis swap");
    assert!(jpeg_w < FIXTURE_W as u32 && jpeg_h < FIXTURE_H as u32, "crop must be a real subset");

    let request = Request {
        id: "export-roundtrip-1".into(),
        kind: RequestKind::Export,
        paths: None,
        thumbnail_dir: None,
        export: Some(ExportRequest {
            output_dir: output_dir_str.clone(),
            items: vec![jpeg_item, png_item],
        }),
        coordinates: None,
    };

    let response = round_trip(&request);

    assert_eq!(response.id, "export-roundtrip-1");
    let records = match response.result {
        ResponseResult::Exported(records) => records,
        // An `Error` here is the exact degradation this test exists to
        // surface: a drifted field name fails Swift's synthesised decode for
        // the whole request. Print the sidecar's message, which names the
        // offending key.
        ResponseResult::Error { message } => panic!(
            "sidecar rejected the export request -- this is what a wire-format \
             disagreement between protocol.rs and Protocol.swift looks like: {message}"
        ),
        other => panic!("expected Exported, got {other:?}"),
    };

    assert_eq!(records.len(), 2, "one record per item, in input order: {records:?}");
    check_ok_record(&records[0], "jpg", jpeg_w, jpeg_h, &output_dir_str, "p04-z1-roundtrip");
    check_ok_record(&records[1], "png", png_w, png_h, &output_dir_str, "p04-z2-roundtrip");

    drop(temp);
}

/// Asserts one `ok` record against the file it claims to have written.
fn check_ok_record(
    record: &serde_json::Value,
    expected_ext: &str,
    expected_w: u32,
    expected_h: u32,
    output_dir: &str,
    stem: &str,
) {
    assert_eq!(
        record["type"], "ok",
        "expected an ok record, got {record} -- a `failed` record here means the sidecar \
         decoded the item but rejected its crop window, which is what an x/y swap looks like"
    );

    let path = record["path"].as_str().unwrap_or_else(|| panic!("record has no path: {record}"));
    // The sidecar owns the extension (it follows the SOURCE container, not
    // the requested filename), so this pins the format rule across the wire:
    // lossy source -> jpg, lossless source -> png.
    assert_eq!(
        path,
        format!("{output_dir}/{stem}.{expected_ext}"),
        "output path must be <outputDir>/<stem>.<extension chosen from the source container>"
    );

    let bytes = std::fs::read(path)
        .unwrap_or_else(|e| panic!("sidecar reported ok but nothing is at {path}: {e}"));
    assert!(!bytes.is_empty(), "wrote a zero-byte file at {path}");
    assert_eq!(
        record["bytes"].as_u64(),
        Some(bytes.len() as u64),
        "record's byte count must match the file actually on disk"
    );

    // Container read from the file's own magic bytes -- not from the
    // extension, which the assertion above already fixed, and not from the
    // record, which is the sidecar's own claim.
    let (actual_ext, actual_w, actual_h) = read_image_header(&bytes)
        .unwrap_or_else(|| panic!("could not parse an image header out of {path}"));
    assert_eq!(actual_ext, expected_ext, "file at {path} is not really a {expected_ext}");
    assert_eq!(
        (actual_w, actual_h),
        (expected_w, expected_h),
        "cropped file at {path} has the wrong pixel dimensions -- a cropW/cropH swap on the \
         wire lands exactly here"
    );
    // And the sidecar's own claim must agree with the file it wrote.
    assert_eq!(record["width"].as_u64(), Some(expected_w as u64), "{record}");
    assert_eq!(record["height"].as_u64(), Some(expected_h as u64), "{record}");
}

/// Sends one NDJSON line to the real sidecar binary and reads one back.
///
/// Same spawn/drain/timeout shape as `sidecar_ping.rs`: stderr is drained on
/// its own thread so a chatty sidecar cannot fill the pipe and block, and the
/// read is bounded so a hung sidecar fails in seconds instead of hanging CI.
fn round_trip(request: &Request) -> Response {
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

    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            line.clear();
        }
    });

    let mut line = serde_json::to_string(request).expect("serialize request");
    assert!(!line.contains('\n'), "an NDJSON request must be exactly one line");
    line.push('\n');
    stdin.write_all(line.as_bytes()).expect("write request to sidecar stdin");
    stdin.flush().expect("flush sidecar stdin");

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut out = String::new();
        let result = reader.read_line(&mut out);
        let _ = tx.send(result.map(|_| out));
    });

    // Generous: this decodes, crops, colour-converts and re-encodes two
    // 1200x800 images, and a cold binary pays dyld/ImageIO warm-up on top.
    let response_line = match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => {
            let _ = child.kill();
            panic!("failed reading sidecar stdout: {e}");
        }
        Err(_) => {
            let _ = child.kill();
            panic!("sidecar did not respond within 30s (binary: {binary_path})");
        }
    };

    let _ = child.kill();
    let _ = child.wait();

    serde_json::from_str(response_line.trim())
        .unwrap_or_else(|e| panic!("malformed response line {response_line:?}: {e}"))
}

// --- independent header readers ---------------------------------------------
//
// Deliberately hand-rolled rather than shelled out to `sips`. `sips` is
// ImageIO, which is the same library that WROTE these files -- if ImageIO
// misencoded a dimension it would misreport it identically, and the check
// would agree with the bug. Parsing the container bytes directly is a genuinely
// independent reading. Both parsers are minimal on purpose: they only need to
// handle files this project's own exporter produces.

/// Returns `(extension, width, height)` read from the container header, or
/// `None` when the bytes are neither a PNG nor a JPEG.
fn read_image_header(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    if let Some((w, h)) = png_dimensions(bytes) {
        return Some(("png", w, h));
    }
    jpeg_dimensions(bytes).map(|(w, h)| ("jpg", w, h))
}

/// PNG: 8-byte signature, then an IHDR chunk whose data begins at offset 16
/// with width and height as big-endian u32s.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((be_u32(bytes, 16)?, be_u32(bytes, 20)?))
}

/// JPEG: walk the marker segments from SOI until a Start-Of-Frame, whose
/// payload carries height then width as big-endian u16s after a one-byte
/// sample precision.
///
/// `C4` (Huffman tables), `C8` (reserved) and `CC` (arithmetic coding
/// conditioning) sit inside the `C0..=CF` range but are NOT frame headers;
/// treating them as one would read table bytes as dimensions.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    loop {
        // Segments may be preceded by any number of 0xFF fill bytes.
        while i < bytes.len() && bytes[i] != 0xFF {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        let marker = *bytes.get(i)?;
        i += 1;
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            // marker payload: [len:2][precision:1][height:2][width:2]
            let height = be_u16(bytes, i + 3)?;
            let width = be_u16(bytes, i + 5)?;
            return Some((width as u32, height as u32));
        }
        if marker == 0xD9 || marker == 0xDA {
            return None; // end of image, or start of scan with no frame seen
        }
        let length = be_u16(bytes, i)? as usize;
        if length < 2 {
            return None;
        }
        i += length;
    }
}

fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}
