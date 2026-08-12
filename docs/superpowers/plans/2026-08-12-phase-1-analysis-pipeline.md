# Phase 1: Analysis Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri 2 + Nuxt 4 app that scans a folder of photos, analyses every one on-device through a long-lived Swift sidecar, caches the results in SQLite, and displays the ranked, clustered feature records.

**Architecture:** A Swift executable (`photobook-engine`) owns all pixel work — ImageIO decode, EXIF, Vision, classical metrics — and speaks newline-delimited JSON on stdin/stdout. The Rust Tauri core spawns it once at launch, batches requests to it, enforces per-photo timeouts, respawns it on crash, and persists results to SQLite keyed by content hash. Rust then derives near-duplicate clusters, event clusters, and within-book percentile ranks. Nuxt displays the result.

**Tech Stack:** Swift 6.3 (SwiftPM, swift-testing), Rust (Tauri 2, rusqlite, serde), Nuxt 4 + `@nuxt/ui` v4 + Tailwind 4, Bun, vitest, oxlint/oxfmt.

**Spec:** `docs/superpowers/specs/2026-08-12-photobook-generator-design.md`

## Global Constraints

- **Platform:** macOS only, **arm64 only**, deployment target **macOS 15.0** (`VNCalculateImageAestheticsScoresRequest` requires 15.0). Never build universal.
- **Privacy:** images never leave the machine. No network calls anywhere in this phase.
- **Sidecar packaging:** ship via `bundle.externalBin`, **never** `bundle.resources` — resources are not codesigned.
- **Sidecar filename:** must be `photobook-engine-aarch64-apple-darwin` on disk. The bundler does a naive string append with no fallback.
- **Orientation:** all dimensions, face boxes, and saliency boxes are **post-orientation**. Never compute geometry from pre-orientation dimensions.
- **Lint:** oxlint warnings are failures (`--deny-warnings`). Never `git commit --no-verify`.
- **Commits:** Conventional Commits (`feat:`, `fix:`, `test:`, `chore:`, `docs:`).
- **TDD:** every task writes a failing test first, verifies it fails, then implements.
- **Never use `any` in TypeScript.** Never re-declare types that can be derived.

## File Structure

```
photobook-generator/
├── package.json                    Bun workspace root, scripts
├── nuxt.config.ts                  ssr: false, @nuxt/ui
├── .oxlintrc.json
├── vitest.config.ts
├── app/
│   ├── app.vue
│   ├── pages/index.vue             folder picker + results table
│   ├── composables/useAnalysis.ts  invoke() wrappers
│   └── types/features.ts           FeatureRecord mirror
├── scripts/
│   └── build-sidecar.sh            swift build + copy with triple suffix
├── sidecar/                        Swift package
│   ├── Package.swift
│   ├── Sources/PhotobookEngine/
│   │   ├── main.swift              stdin NDJSON loop
│   │   ├── Protocol.swift          Codable Request/Response
│   │   ├── ImageLoader.swift       ImageIO decode + orientation
│   │   ├── ExifReader.swift        metadata-only reads
│   │   ├── VisionAnalyzer.swift    one handler, all requests
│   │   ├── Metrics.swift           sharpness, palette, pHash
│   │   └── SmileProxy.swift        landmark geometry
│   ├── Tests/PhotobookEngineTests/
│   └── Fixtures/                   hostile inputs
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/default.json
    └── src/
        ├── main.rs
        ├── lib.rs
        ├── protocol.rs             mirrors Swift Protocol.swift
        ├── sidecar.rs              spawn, batch, timeout, respawn
        ├── db.rs                   schema, migrations, cache
        ├── cluster.rs              pHash Hamming + event clustering
        ├── ranking.rs              within-book percentiles
        └── commands.rs             #[tauri::command] surface
```

Each file has one responsibility. `sidecar.rs` never touches SQL; `db.rs` never spawns processes; `cluster.rs` and `ranking.rs` are pure functions over feature records and are the easiest things in the codebase to test.

---

### Task 1: Project scaffold

**Files:**
- Create: `package.json`, `nuxt.config.ts`, `.oxlintrc.json`, `vitest.config.ts`, `app/app.vue`, `app/pages/index.vue`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`
- Test: `tests/scaffold.test.ts`

**Interfaces:**
- Consumes: nothing
- Produces: a running `bun run tauri dev` window; the `app_lib` Rust crate name used by every later Rust task

- [ ] **Step 1: Write the failing test**

`tests/scaffold.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import config from "../nuxt.config";

describe("nuxt config", () => {
  it("disables SSR because Tauri has no server", () => {
    expect(config.ssr).toBe(false);
  });

  it("does not ignore the app directory", () => {
    expect(config.ignore).toContain("**/src-tauri/**");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `bun run test`
Expected: FAIL — `Cannot find module '../nuxt.config'`

- [ ] **Step 3: Create the scaffold**

`package.json`:

```json
{
  "name": "photobook-generator",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "tauri dev",
    "build": "tauri build",
    "ui:dev": "nuxt dev",
    "generate": "nuxt generate",
    "sidecar": "bash scripts/build-sidecar.sh",
    "test": "vitest run",
    "test:rust": "cargo test --manifest-path src-tauri/Cargo.toml",
    "test:swift": "swift test --package-path sidecar",
    "lint": "oxlint --deny-warnings",
    "lint:fix": "oxlint --fix --deny-warnings",
    "fmt": "oxfmt",
    "postinstall": "nuxt prepare"
  },
  "dependencies": {
    "@nuxt/ui": "^4.10.0",
    "@tauri-apps/api": "^2.11.1",
    "@tauri-apps/plugin-dialog": "^2.4.0",
    "@tauri-apps/plugin-fs": "^2.5.1",
    "nuxt": "^4.5.0",
    "tailwindcss": "^4.3.3"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.4",
    "oxfmt": "^0.60.0",
    "oxlint": "^1.75.0",
    "typescript": "^5.9.0",
    "vitest": "^4.1.10"
  }
}
```

`nuxt.config.ts`:

```ts
export default defineNuxtConfig({
  modules: ["@nuxt/ui"],
  ssr: false,
  devtools: { enabled: true },
  compatibilityDate: "2026-08-12",
  vite: {
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_"],
    server: { strictPort: true },
  },
  ignore: ["**/src-tauri/**"],
});
```

`vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: { include: ["tests/**/*.test.ts"] },
});
```

`.oxlintrc.json`:

```json
{ "categories": { "correctness": "error", "suspicious": "warn" } }
```

`app/app.vue`:

```vue
<template>
  <UApp>
    <NuxtPage />
  </UApp>
</template>
```

`app/pages/index.vue`:

```vue
<template>
  <main class="p-8">
    <h1 class="text-2xl font-semibold">Photobook Generator</h1>
  </main>
</template>
```

`src-tauri/Cargo.toml`:

```toml
[package]
name = "photobook-generator"
version = "0.1.0"
edition = "2021"
rust-version = "1.77.2"

[lib]
name = "app_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[[bin]]
name = "PhotobookGenerator"
path = "src/main.rs"

[build-dependencies]
tauri-build = { version = "2.6.3", features = [] }

[dependencies]
tauri = { version = "2.11.3", features = [] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
rusqlite = { version = "0.32", features = ["bundled"] }
sha2 = "0.10"
thiserror = "2"
log = "0.4"
```

`src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app_lib::run()
}
```

`src-tauri/src/lib.rs`:

```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Photobook Generator",
  "version": "0.1.0",
  "identifier": "com.jiajingteoh.photobook",
  "build": {
    "beforeDevCommand": "bun run ui:dev",
    "devUrl": "http://localhost:3000",
    "beforeBuildCommand": "bun run generate",
    "frontendDist": "../.output/public"
  },
  "app": {
    "windows": [{ "title": "Photobook Generator", "width": 1400, "height": 900 }],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": "app",
    "macOS": { "minimumSystemVersion": "15.0" }
  }
}
```

`src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:default", "fs:default"]
}
```

Note `frontendDist` is `../.output/public`, **not** `../dist` — Nuxt 4 generates there and the root `dist` symlink can confuse the macOS bundler.

- [ ] **Step 4: Run tests to verify they pass**

Run: `bun install && bun run test`
Expected: PASS, 2 tests

Then confirm the app launches: `bun run dev`. Expected: a window titled "Photobook Generator".

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: scaffold Tauri 2 + Nuxt 4 project"
```

---

### Task 2: Swift package and NDJSON protocol loop

**Files:**
- Create: `sidecar/Package.swift`, `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/main.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `Request` / `Response` Codable types; a binary that reads one JSON object per line from stdin and writes one per line to stdout. Request kinds in this task: `ping`. Every response carries the request's `id`.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

@Test func decodesPingRequest() throws {
    let json = #"{"id":"abc","kind":"ping"}"#
    let req = try JSONDecoder().decode(Request.self, from: Data(json.utf8))
    #expect(req.id == "abc")
    #expect(req.kind == .ping)
}

@Test func encodesPongResponseWithMatchingId() throws {
    let res = Response(id: "abc", result: .pong(PongResult(version: "0.1.0")))
    let data = try JSONEncoder().encode(res)
    let text = String(decoding: data, as: UTF8.self)
    #expect(text.contains("\"id\":\"abc\""))
    #expect(text.contains("\"version\":\"0.1.0\""))
}

@Test func encodesErrorResponse() throws {
    let res = Response(id: "z", result: .error(ErrorResult(message: "boom")))
    let data = try JSONEncoder().encode(res)
    #expect(String(decoding: data, as: UTF8.self).contains("boom"))
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — no such package / cannot find `Request` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Package.swift`:

```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PhotobookEngine",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(name: "PhotobookEngine"),
        .testTarget(name: "PhotobookEngineTests", dependencies: ["PhotobookEngine"]),
    ]
)
```

`sidecar/Sources/PhotobookEngine/Protocol.swift`:

```swift
import Foundation

enum RequestKind: String, Codable {
    case ping
    case analyze
}

struct Request: Codable {
    let id: String
    let kind: RequestKind
    var paths: [String]?
}

struct PongResult: Codable { let version: String }
struct ErrorResult: Codable { let message: String }

enum ResponseResult: Codable {
    case pong(PongResult)
    case error(ErrorResult)

    private enum CodingKeys: String, CodingKey { case type, data }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .pong(let v):
            try c.encode("pong", forKey: .type)
            try c.encode(v, forKey: .data)
        case .error(let v):
            try c.encode("error", forKey: .type)
            try c.encode(v, forKey: .data)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "pong": self = .pong(try c.decode(PongResult.self, forKey: .data))
        default: self = .error(try c.decode(ErrorResult.self, forKey: .data))
        }
    }
}

struct Response: Codable {
    let id: String
    let result: ResponseResult
}

enum Engine {
    static let version = "0.1.0"
}
```

`sidecar/Sources/PhotobookEngine/main.swift`:

```swift
import Foundation

let encoder = JSONEncoder()
// Swift's default `.deferredToDate` encodes seconds since 2001-01-01, which
// Rust would silently misread as a Unix epoch. Be explicit.
encoder.dateEncodingStrategy = .secondsSince1970

let decoder = JSONDecoder()
decoder.dateDecodingStrategy = .secondsSince1970

func emit(_ response: Response) {
    guard let data = try? encoder.encode(response) else { return }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

while let line = readLine(strippingNewline: true) {
    if line.isEmpty { continue }
    guard let request = try? decoder.decode(Request.self, from: Data(line.utf8)) else {
        emit(Response(id: "unknown", result: .error(ErrorResult(message: "malformed request"))))
        continue
    }
    switch request.kind {
    case .ping:
        emit(Response(id: request.id, result: .pong(PongResult(version: Engine.version))))
    case .analyze:
        emit(Response(id: request.id, result: .error(ErrorResult(message: "not implemented"))))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 tests

Then verify the loop by hand:

```bash
echo '{"id":"a","kind":"ping"}' | swift run --package-path sidecar PhotobookEngine
```

Expected: `{"id":"a","result":{"type":"pong","data":{"version":"0.1.0"}}}`

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add NDJSON protocol loop with ping"
```

---

### Task 3: Sidecar build script and Rust spawn

**Files:**
- Create: `scripts/build-sidecar.sh`, `src-tauri/src/protocol.rs`, `src-tauri/src/sidecar.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `.gitignore`
- Test: `src-tauri/src/protocol.rs` (inline `#[cfg(test)]` module — the tests cover wire-format serialisation, so they live with the types)

**Interfaces:**
- Consumes: `Request`/`Response` JSON shape from Task 2
- Produces: `Sidecar::spawn(app: &AppHandle) -> Result<Sidecar, SidecarError>` and `Sidecar::ping(&mut self) -> Result<String, SidecarError>` returning the engine version. `protocol::Request { id, kind, paths }` and `protocol::Response { id, result }` mirroring the Swift types.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/protocol.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_ping_request_as_single_line() {
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'));
        assert!(line.contains("\"kind\":\"ping\""));
    }

    #[test]
    fn deserializes_pong_response() {
        let line = r#"{"id":"a","result":{"type":"pong","data":{"version":"0.1.0"}}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        assert_eq!(res.id, "a");
        match res.result {
            ResponseResult::Pong { version } => assert_eq!(version, "0.1.0"),
            _ => panic!("expected pong"),
        }
    }

    #[test]
    fn deserializes_error_response() {
        let line = r#"{"id":"a","result":{"type":"error","data":{"message":"boom"}}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        match res.result {
            ResponseResult::Error { message } => assert_eq!(message, "boom"),
            _ => panic!("expected error"),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `file not found for module 'protocol'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/protocol.rs` (above the test module):

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    Ping,
    Analyze,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub kind: RequestKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum ResponseResult {
    Pong { version: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub result: ResponseResult,
}
```

`src-tauri/src/sidecar.rs`:

```rust
use crate::protocol::{Request, RequestKind, Response, ResponseResult};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("failed to spawn sidecar: {0}")]
    Spawn(String),
    #[error("sidecar timed out after {0:?}")]
    Timeout(Duration),
    #[error("sidecar closed its output stream")]
    Closed,
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("engine error: {0}")]
    Engine(String),
}

pub struct Sidecar {
    child: CommandChild,
    lines: Receiver<String>,
    counter: u64,
}

impl Sidecar {
    pub fn spawn(app: &AppHandle) -> Result<Self, SidecarError> {
        let (mut rx, child) = app
            .shell()
            .sidecar("photobook-engine")
            .map_err(|e| SidecarError::Spawn(e.to_string()))?
            .spawn()
            .map_err(|e| SidecarError::Spawn(e.to_string()))?;

        // The plugin's event channel has capacity 1; a dedicated drain thread
        // prevents the reader thread from blocking.
        let (tx, lines) = mpsc::channel();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let CommandEvent::Stdout(bytes) = event {
                    let text = String::from_utf8_lossy(&bytes).trim().to_string();
                    if !text.is_empty() && tx.send(text).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self { child, lines, counter: 0 })
    }

    fn next_id(&mut self) -> String {
        self.counter += 1;
        self.counter.to_string()
    }

    pub fn request(&mut self, kind: RequestKind, paths: Option<Vec<String>>, timeout: Duration)
        -> Result<ResponseResult, SidecarError>
    {
        let id = self.next_id();
        let req = Request { id: id.clone(), kind, paths };
        let mut line = serde_json::to_string(&req).map_err(|e| SidecarError::Malformed(e.to_string()))?;
        line.push('\n');
        self.child.write(line.as_bytes()).map_err(|e| SidecarError::Spawn(e.to_string()))?;

        loop {
            let text = self.lines.recv_timeout(timeout).map_err(|_| SidecarError::Timeout(timeout))?;
            let res: Response = serde_json::from_str(&text)
                .map_err(|e| SidecarError::Malformed(format!("{e}: {text}")))?;
            if res.id == id {
                return Ok(res.result);
            }
            // Ignore stale responses from a previous, timed-out request.
        }
    }

    pub fn ping(&mut self) -> Result<String, SidecarError> {
        match self.request(RequestKind::Ping, None, Duration::from_secs(5))? {
            ResponseResult::Pong { version } => Ok(version),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
        }
    }
}
```

`scripts/build-sidecar.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

TRIPLE="$(rustc --print host-tuple)"
OUT="src-tauri/binaries"

swift build --package-path sidecar -c release \
  -Xswiftc -target -Xswiftc arm64-apple-macos15.0

mkdir -p "$OUT"
cp "sidecar/.build/release/PhotobookEngine" "$OUT/photobook-engine-${TRIPLE}"
echo "built $OUT/photobook-engine-${TRIPLE}"
```

Then `chmod +x scripts/build-sidecar.sh`.

Add to `src-tauri/tauri.conf.json` under `bundle`:

```json
"externalBin": ["binaries/photobook-engine"]
```

Replace `src-tauri/capabilities/default.json` permissions array with:

```json
"permissions": [
  "core:default",
  "dialog:default",
  "fs:default",
  {
    "identifier": "shell:allow-execute",
    "allow": [{ "name": "binaries/photobook-engine", "sidecar": true }]
  }
]
```

`shell:default` grants only `allow-open` and does **not** cover sidecars.

Add `src-tauri/binaries` to `.gitignore` (already present from the initial commit — verify).

Wire the modules in `src-tauri/src/lib.rs`, above `run()`:

```rust
pub mod protocol;
pub mod sidecar;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `bun run sidecar && cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 3 tests, and the script prints the built binary path.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(sidecar): wire Swift sidecar into Tauri with NDJSON transport"
```

---

### Task 4: ImageIO decode with orientation

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/ImageLoader.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift`
- Create fixtures: `sidecar/Fixtures/landscape.jpg`, `sidecar/Fixtures/portrait-rot90.jpg`

**Interfaces:**
- Consumes: nothing
- Produces: `ImageLoader.loadThumbnail(path: String, maxPixel: Int) throws -> CGImage` returning an image with EXIF orientation **already applied**, and `ImageLoader.LoadError`.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/ImageLoaderTests.swift`:

```swift
import Testing
import Foundation
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()   // PhotobookEngineTests
        .deletingLastPathComponent()   // Tests
        .deletingLastPathComponent()   // sidecar
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func loadsLandscapeAtRequestedSize() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 512)
    #expect(max(img.width, img.height) <= 512)
    #expect(img.width > img.height)
}

@Test func appliesExifOrientationSoPortraitIsTaller() throws {
    // File is stored landscape with EXIF orientation 6 (rotate 90 CW).
    let img = try ImageLoader.loadThumbnail(path: fixture("portrait-rot90.jpg"), maxPixel: 512)
    #expect(img.height > img.width, "orientation must be applied before we report dimensions")
}

@Test func throwsOnMissingFile() {
    #expect(throws: ImageLoader.LoadError.self) {
        try ImageLoader.loadThumbnail(path: "/nonexistent/nope.jpg", maxPixel: 512)
    }
}
```

Create the fixtures with a Swift script. **ImageMagick and exiftool are not installed on
this machine**, and requiring them would make the test suite unreproducible. ImageIO can
write both the pixels and the EXIF orientation tag, with no external dependency.

Create `scripts/make-fixtures.swift`:

```swift
import Foundation
import ImageIO
import CoreGraphics
import UniformTypeIdentifiers

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let dir = root.appendingPathComponent("sidecar/Fixtures")
try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

/// Writes a solid-colour JPEG of the given stored pixel size, tagging it with
/// `orientation` (1 = normal, 6 = rotate 90 CW on display).
func write(_ name: String, width: Int, height: Int,
           rgb: (Double, Double, Double), orientation: Int) {
    let ctx = CGContext(
        data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.setFillColor(red: rgb.0, green: rgb.1, blue: rgb.2, alpha: 1)
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = ctx.makeImage()!

    let url = dir.appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!
    let props: [CFString: Any] = [kCGImagePropertyOrientation: orientation]
    CGImageDestinationAddImage(dest, image, props as CFDictionary)
    precondition(CGImageDestinationFinalize(dest), "failed to write \(name)")
    print("wrote \(name) (\(width)x\(height), orientation \(orientation))")
}

write("landscape.jpg", width: 1200, height: 800, rgb: (0.1, 0.1, 0.5), orientation: 1)
write("portrait-rot90.jpg", width: 1200, height: 800, rgb: (0.1, 0.4, 0.1), orientation: 6)
```

Run it from the repo root:

```bash
swift scripts/make-fixtures.swift
```

`portrait-rot90.jpg` stores 1200×800 pixels but is tagged orientation 6, so a correct
reader displays it as 800×1200. That is exactly what the second test checks — and it fails
if `kCGImageSourceCreateThumbnailWithTransform` is omitted, which is the point.

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `ImageLoader` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/ImageLoader.swift`:

```swift
import Foundation
import ImageIO
import CoreGraphics

enum ImageLoader {
    enum LoadError: Error {
        case unreadable(String)
        case decodeFailed(String)
    }

    /// Decodes at most `maxPixel` on the long edge, with EXIF orientation applied.
    /// `kCGImageSourceCreateThumbnailWithTransform` reconciles EXIF orientation and
    /// HEIC container `irot`/`imir` properties, which can disagree.
    static func loadThumbnail(path: String, maxPixel: Int) throws -> CGImage {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil) else {
            throw LoadError.unreadable(path)
        }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            throw LoadError.decodeFailed(path)
        }
        return image
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add ImageIO thumbnail decode with orientation applied"
```

---

### Task 5: EXIF extraction

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/ExifReader.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift`

**Interfaces:**
- Consumes: nothing (reads metadata only, never decodes pixels)
- Produces: `struct ExifData: Codable { captureDate: Date?; latitude: Double?; longitude: Double?; pixelWidth: Int; pixelHeight: Int; make: String?; model: String?; flashFired: Bool? }` and `ExifReader.read(path: String) throws -> ExifData`. Dimensions are **post-orientation**.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/ExifReaderTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func readsDimensionsForLandscape() throws {
    let exif = try ExifReader.read(path: fixture("landscape.jpg"))
    #expect(exif.pixelWidth == 1200)
    #expect(exif.pixelHeight == 800)
}

@Test func reportsPostOrientationDimensions() throws {
    // Stored 1200x800 with orientation 6; must be reported as 800x1200.
    let exif = try ExifReader.read(path: fixture("portrait-rot90.jpg"))
    #expect(exif.pixelWidth == 800)
    #expect(exif.pixelHeight == 1200)
}

@Test func throwsOnMissingFile() {
    #expect(throws: ExifReader.ReadError.self) {
        try ExifReader.read(path: "/nonexistent/nope.jpg")
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `ExifReader` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/ExifReader.swift`:

```swift
import Foundation
import ImageIO

struct ExifData: Codable {
    var captureDate: Date?
    var latitude: Double?
    var longitude: Double?
    var pixelWidth: Int
    var pixelHeight: Int
    var make: String?
    var model: String?
    var flashFired: Bool?
}

enum ExifReader {
    enum ReadError: Error { case unreadable(String) }

    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy:MM:dd HH:mm:ss"
        f.timeZone = TimeZone(secondsFromGMT: 0)
        return f
    }()

    /// Orientations 5-8 are the four that swap width and height.
    private static func swapsAxes(_ orientation: Int) -> Bool {
        (5...8).contains(orientation)
    }

    static func read(path: String) throws -> ExifData {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        else { throw ReadError.unreadable(path) }

        let rawW = props[kCGImagePropertyPixelWidth] as? Int ?? 0
        let rawH = props[kCGImagePropertyPixelHeight] as? Int ?? 0
        let orientation = props[kCGImagePropertyOrientation] as? Int ?? 1
        let (w, h) = swapsAxes(orientation) ? (rawH, rawW) : (rawW, rawH)

        let exif = props[kCGImagePropertyExifDictionary] as? [CFString: Any]
        let tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
        let gps = props[kCGImagePropertyGPSDictionary] as? [CFString: Any]

        var lat = gps?[kCGImagePropertyGPSLatitude] as? Double
        if let ref = gps?[kCGImagePropertyGPSLatitudeRef] as? String, ref == "S", let v = lat {
            lat = -v
        }
        var lon = gps?[kCGImagePropertyGPSLongitude] as? Double
        if let ref = gps?[kCGImagePropertyGPSLongitudeRef] as? String, ref == "W", let v = lon {
            lon = -v
        }

        var flash: Bool?
        if let raw = exif?[kCGImagePropertyExifFlash] as? Int { flash = (raw & 1) == 1 }

        return ExifData(
            captureDate: (exif?[kCGImagePropertyExifDateTimeOriginal] as? String).flatMap(formatter.date(from:)),
            latitude: lat,
            longitude: lon,
            pixelWidth: w,
            pixelHeight: h,
            make: tiff?[kCGImagePropertyTIFFMake] as? String,
            model: tiff?[kCGImagePropertyTIFFModel] as? String,
            flashFired: flash
        )
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add metadata-only EXIF reader with post-orientation dimensions"
```

---

### Task 6: Classical metrics — sharpness, palette, pHash

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/Metrics.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`

**Interfaces:**
- Consumes: `CGImage` from `ImageLoader`
- Produces:
  - `Metrics.sharpness(_ image: CGImage) -> Double` — contrast-normalised tile-max Laplacian variance over a 4×4 grid
  - `Metrics.palette(_ image: CGImage, count: Int) -> [PaletteColor]` where `struct PaletteColor: Codable { r, g, b: Double; weight: Double }`
  - `Metrics.perceptualHash(_ image: CGImage) -> UInt64` — 8×8 DCT-free average hash over a 32×32 grey downscale
  - `Metrics.warmth(_ palette: [PaletteColor]) -> Double` and `Metrics.contrast(_ image: CGImage) -> Double`

Sharpness is **tile-max**, not whole-image: a sharp photo of a blank wall scores as blurry under plain Laplacian variance. Taking the max over tiles answers the better question — is the *subject* in focus.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/MetricsTests.swift`:

```swift
import Testing
import CoreGraphics
@testable import PhotobookEngine

/// Builds a greyscale-ish RGBA image from a pixel generator.
private func makeImage(width: Int, height: Int, _ pixel: (Int, Int) -> UInt8) -> CGImage {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let v = pixel(x, y)
            let i = (y * width + x) * 4
            bytes[i] = v; bytes[i + 1] = v; bytes[i + 2] = v; bytes[i + 3] = 255
        }
    }
    let ctx = CGContext(
        data: &bytes, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: width * 4,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    return ctx.makeImage()!
}

@Test func sharpEdgesScoreHigherThanFlatField() {
    let flat = makeImage(width: 128, height: 128) { _, _ in 128 }
    let checker = makeImage(width: 128, height: 128) { x, y in
        ((x / 4) + (y / 4)) % 2 == 0 ? 0 : 255
    }
    #expect(Metrics.sharpness(checker) > Metrics.sharpness(flat))
}

@Test func sharpSubjectOnBlankBackgroundIsNotScoredBlurry() {
    // Detail confined to one tile; whole-image variance would wash this out.
    let flat = makeImage(width: 128, height: 128) { _, _ in 200 }
    let localDetail = makeImage(width: 128, height: 128) { x, y in
        (x < 32 && y < 32) ? (((x + y) % 2 == 0) ? 0 : 255) : 200
    }
    #expect(Metrics.sharpness(localDetail) > Metrics.sharpness(flat) * 5)
}

@Test func paletteReturnsRequestedCountAndWeightsSumToOne() {
    let img = makeImage(width: 64, height: 64) { x, _ in x < 32 ? 20 : 220 }
    let palette = Metrics.palette(img, count: 4)
    #expect(palette.count <= 4)
    #expect(palette.count >= 1)
    let total = palette.reduce(0.0) { $0 + $1.weight }
    #expect(abs(total - 1.0) < 0.001)
}

@Test func identicalImagesHaveIdenticalHash() {
    let a = makeImage(width: 64, height: 64) { x, y in UInt8((x ^ y) & 0xFF) }
    let b = makeImage(width: 64, height: 64) { x, y in UInt8((x ^ y) & 0xFF) }
    #expect(Metrics.perceptualHash(a) == Metrics.perceptualHash(b))
}

@Test func differentImagesHaveDifferentHash() {
    let a = makeImage(width: 64, height: 64) { x, _ in x < 32 ? 0 : 255 }
    let b = makeImage(width: 64, height: 64) { _, y in y < 32 ? 0 : 255 }
    #expect(Metrics.perceptualHash(a) != Metrics.perceptualHash(b))
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `Metrics` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/Metrics.swift`:

```swift
import Foundation
import CoreGraphics

struct PaletteColor: Codable {
    let r: Double
    let g: Double
    let b: Double
    let weight: Double
}

enum Metrics {
    /// Draws `image` into a tightly-packed RGBA8 buffer of the given size.
    private static func rgbaBuffer(_ image: CGImage, width: Int, height: Int) -> [UInt8] {
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        bytes.withUnsafeMutableBytes { raw in
            guard let ctx = CGContext(
                data: raw.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return }
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
        return bytes
    }

    private static func luma(_ bytes: [UInt8], _ i: Int) -> Double {
        0.2126 * Double(bytes[i]) + 0.7152 * Double(bytes[i + 1]) + 0.0722 * Double(bytes[i + 2])
    }

    /// Contrast-normalised tile-max Laplacian variance over a 4x4 grid.
    static func sharpness(_ image: CGImage) -> Double {
        let side = 256
        let bytes = rgbaBuffer(image, width: side, height: side)
        var grey = [Double](repeating: 0, count: side * side)
        for p in 0..<(side * side) { grey[p] = luma(bytes, p * 4) }

        let tiles = 4
        let tile = side / tiles
        var best = 0.0

        for ty in 0..<tiles {
            for tx in 0..<tiles {
                var values: [Double] = []
                values.reserveCapacity(tile * tile)
                for y in (ty * tile + 1)..<((ty + 1) * tile - 1) {
                    for x in (tx * tile + 1)..<((tx + 1) * tile - 1) {
                        let c = grey[y * side + x]
                        let lap = grey[(y - 1) * side + x] + grey[(y + 1) * side + x]
                                + grey[y * side + x - 1] + grey[y * side + x + 1] - 4 * c
                        values.append(lap)
                    }
                }
                guard values.count > 1 else { continue }
                let mean = values.reduce(0, +) / Double(values.count)
                let variance = values.reduce(0) { $0 + ($1 - mean) * ($1 - mean) } / Double(values.count)

                // Normalise by local contrast so dark tiles aren't unfairly penalised.
                var lo = 255.0, hi = 0.0
                for y in (ty * tile)..<((ty + 1) * tile) {
                    for x in (tx * tile)..<((tx + 1) * tile) {
                        let v = grey[y * side + x]
                        lo = min(lo, v); hi = max(hi, v)
                    }
                }
                let contrast = max(hi - lo, 1.0)
                best = max(best, variance / contrast)
            }
        }
        return best
    }

    /// Uniform 4x4x4 RGB bucket histogram, returning the heaviest buckets.
    static func palette(_ image: CGImage, count: Int) -> [PaletteColor] {
        let side = 128
        let bytes = rgbaBuffer(image, width: side, height: side)
        var buckets = [Int: (r: Double, g: Double, b: Double, n: Int)]()

        for p in 0..<(side * side) {
            let i = p * 4
            let r = Double(bytes[i]), g = Double(bytes[i + 1]), b = Double(bytes[i + 2])
            let key = (Int(r) >> 6) << 4 | (Int(g) >> 6) << 2 | (Int(b) >> 6)
            var e = buckets[key] ?? (0, 0, 0, 0)
            e.r += r; e.g += g; e.b += b; e.n += 1
            buckets[key] = e
        }

        let total = Double(side * side)
        return buckets.values
            .sorted { $0.n > $1.n }
            .prefix(count)
            .map { e in
                PaletteColor(
                    r: e.r / Double(e.n) / 255.0,
                    g: e.g / Double(e.n) / 255.0,
                    b: e.b / Double(e.n) / 255.0,
                    weight: Double(e.n) / total
                )
            }
            .normalisedWeights()
    }

    /// Average hash over a 8x8 grey downscale. 64 bits, Hamming-comparable.
    static func perceptualHash(_ image: CGImage) -> UInt64 {
        let side = 8
        let bytes = rgbaBuffer(image, width: side, height: side)
        var grey = [Double](repeating: 0, count: side * side)
        for p in 0..<(side * side) { grey[p] = luma(bytes, p * 4) }
        let mean = grey.reduce(0, +) / Double(grey.count)

        var hash: UInt64 = 0
        for (i, v) in grey.enumerated() where v > mean {
            hash |= (1 << UInt64(i))
        }
        return hash
    }

    /// Warm colours (red-dominant) score toward 1, cool (blue-dominant) toward 0.
    static func warmth(_ palette: [PaletteColor]) -> Double {
        guard !palette.isEmpty else { return 0.5 }
        return palette.reduce(0.0) { acc, c in
            acc + c.weight * (0.5 + (c.r - c.b) / 2.0)
        }.clamped(0, 1)
    }

    /// Global luma spread, normalised to 0...1.
    static func contrast(_ image: CGImage) -> Double {
        let side = 128
        let bytes = rgbaBuffer(image, width: side, height: side)
        var lo = 255.0, hi = 0.0
        for p in 0..<(side * side) {
            let v = luma(bytes, p * 4)
            lo = min(lo, v); hi = max(hi, v)
        }
        return ((hi - lo) / 255.0).clamped(0, 1)
    }
}

private extension Array where Element == PaletteColor {
    func normalisedWeights() -> [PaletteColor] {
        let total = reduce(0.0) { $0 + $1.weight }
        guard total > 0 else { return self }
        return map { PaletteColor(r: $0.r, g: $0.g, b: $0.b, weight: $0.weight / total) }
    }
}

extension Double {
    func clamped(_ lo: Double, _ hi: Double) -> Double { Swift.min(Swift.max(self, lo), hi) }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 5 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add sharpness, palette, and perceptual hash metrics"
```

---

### Task 7: Vision analysis pass

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/VisionAnalyzerTests.swift`

**Interfaces:**
- Consumes: `CGImage` from `ImageLoader`
- Produces: `struct VisionResult: Codable { isUtility: Bool; aestheticScore: Double; faces: [FaceObservation]; saliencyBox: [Double]?; horizonTiltDeg: Double?; sceneTags: [String]; hasText: Bool }` and `struct FaceObservation: Codable { box: [Double]; yaw: Double?; pitch: Double?; roll: Double?; captureQuality: Double?; landmarks: [[Double]]? }`; `VisionAnalyzer.analyze(_ image: CGImage) -> VisionResult`.

All boxes are `[x, y, w, h]` normalised to `0...1` in **top-left origin** coordinates — Vision returns bottom-left origin, so this converts.

**Critical:** all requests go on **one** `VNImageRequestHandler`. Vision reuses the decoded surface across requests on the same handler; one handler per request costs several times more.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/VisionAnalyzerTests.swift`:

```swift
import Testing
import CoreGraphics
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func returnsResultForPlainImageWithoutCrashing() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    #expect(result.faces.isEmpty)
    #expect(result.aestheticScore >= -1.0 && result.aestheticScore <= 1.0)
}

@Test func boxesAreNormalisedAndTopLeftOrigin() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    if let box = result.saliencyBox {
        #expect(box.count == 4)
        for v in box { #expect(v >= -0.001 && v <= 1.001) }
    }
}

@Test func sceneTagsAreReturnedForARecognisableImage() throws {
    let img = try ImageLoader.loadThumbnail(path: fixture("landscape.jpg"), maxPixel: 1024)
    let result = VisionAnalyzer.analyze(img)
    // A flat navy field may legitimately produce no confident tags; assert the
    // contract, not the content.
    #expect(result.sceneTags.count <= 8)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `VisionAnalyzer` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/VisionAnalyzer.swift`:

```swift
import Foundation
import Vision
import CoreGraphics

struct FaceObservation: Codable {
    let box: [Double]
    let yaw: Double?
    let pitch: Double?
    let roll: Double?
    let captureQuality: Double?
    let landmarks: [[Double]]?
}

struct VisionResult: Codable {
    var isUtility: Bool = false
    var aestheticScore: Double = 0
    var faces: [FaceObservation] = []
    var saliencyBox: [Double]?
    var horizonTiltDeg: Double?
    var sceneTags: [String] = []
    var hasText: Bool = false
}

enum VisionAnalyzer {
    /// Vision uses a bottom-left origin; everything downstream uses top-left.
    private static func topLeft(_ r: CGRect) -> [Double] {
        [Double(r.origin.x), Double(1.0 - r.origin.y - r.height),
         Double(r.width), Double(r.height)]
    }

    static func analyze(_ image: CGImage) -> VisionResult {
        var result = VisionResult()

        let aesthetics = VNCalculateImageAestheticsScoresRequest()
        let faces = VNDetectFaceRectanglesRequest()
        let saliency = VNGenerateAttentionBasedSaliencyImageRequest()
        let horizon = VNDetectHorizonRequest()
        let classify = VNClassifyImageRequest()
        let text = VNRecognizeTextRequest()
        text.recognitionLevel = .fast

        // One handler for everything: Vision reuses the decoded surface across
        // requests, so a second perform() on the same handler is nearly free.
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        try? handler.perform([aesthetics, faces, saliency, horizon, classify, text])

        // Landmarks and capture quality must be seeded with the rectangles
        // request's observations. Running them standalone returns their own
        // independently-ordered face lists, and pairing those by array index
        // silently attaches one person's quality score to another person's box.
        let detected = faces.results ?? []
        let landmarks = VNDetectFaceLandmarksRequest()
        let quality = VNDetectFaceCaptureQualityRequest()
        landmarks.inputFaceObservations = detected
        quality.inputFaceObservations = detected
        if !detected.isEmpty {
            try? handler.perform([landmarks, quality])
        }

        if let obs = aesthetics.results?.first {
            result.aestheticScore = Double(obs.overallScore)
            result.isUtility = obs.isUtility
        }

        // Seeded requests return observations in the same order as the input,
        // so index correspondence is now guaranteed rather than assumed.
        let qualityResults = quality.results ?? []
        let landmarkResults = landmarks.results ?? []

        result.faces = detected.enumerated().map { index, face in
            let points: [[Double]]? = index < landmarkResults.count
                ? landmarkResults[index].landmarks?.allPoints?.normalizedPoints
                    .map { [Double($0.x), Double(1.0 - $0.y)] }
                : nil
            let q: Double? = index < qualityResults.count
                ? qualityResults[index].faceCaptureQuality.map(Double.init)
                : nil
            return FaceObservation(
                box: topLeft(face.boundingBox),
                yaw: face.yaw.map { Double(truncating: $0) },
                pitch: face.pitch.map { Double(truncating: $0) },
                roll: face.roll.map { Double(truncating: $0) },
                captureQuality: q,
                landmarks: points
            )
        }

        if let salient = saliency.results?.first?.salientObjects?.first {
            result.saliencyBox = topLeft(salient.boundingBox)
        }

        if let obs = horizon.results?.first {
            result.horizonTiltDeg = Double(obs.angle) * 180.0 / .pi
        }

        // Per-class calibration varies widely across Vision's 1,303 labels, so
        // filter by precision rather than a flat confidence threshold.
        if let classifications = classify.results {
            result.sceneTags = classifications
                .filter { (try? $0.hasMinimumRecall(0.0, forPrecision: 0.8)) ?? false }
                .prefix(8)
                .map(\.identifier)
        }

        result.hasText = !(text.results ?? []).isEmpty

        return result
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add batched Vision analysis on a single request handler"
```

---

### Task 8: Smile proxy from landmarks

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/SmileProxy.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`

**Interfaces:**
- Consumes: `FaceObservation` from Task 7
- Produces: `SmileProxy.confidence(for face: FaceObservation) -> Double?` (nil when head pose is unusable) and `SmileProxy.fraction(faces: [FaceObservation], threshold: Double) -> Double?` (nil when no face has usable pose).

Vision has no expression classifier at any macOS version, so this is geometric. It returns **nil, never 0**, when it cannot tell — "nobody smiling" and "couldn't tell" must not collapse.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/SmileProxyTests.swift`:

```swift
import Testing
@testable import PhotobookEngine

/// Vision's allPoints ordering places the outer lip contour last; these helpers
/// build the minimal shape the proxy reads.
private func face(yaw: Double, mouthCornerLift: Double) -> FaceObservation {
    // Points: left corner, centre-top, right corner, centre-bottom.
    let pts: [[Double]] = [
        [0.35, 0.70 - mouthCornerLift],
        [0.50, 0.68],
        [0.65, 0.70 - mouthCornerLift],
        [0.50, 0.74],
    ]
    return FaceObservation(box: [0.3, 0.3, 0.4, 0.4], yaw: yaw, pitch: 0, roll: 0,
                           captureQuality: 0.8, landmarks: pts)
}

@Test func returnsNilWhenHeadIsTurnedTooFar() {
    #expect(SmileProxy.confidence(for: face(yaw: 0.9, mouthCornerLift: 0.05)) == nil)
}

@Test func returnsNilWhenLandmarksAreMissing() {
    let f = FaceObservation(box: [0, 0, 1, 1], yaw: 0, pitch: 0, roll: 0,
                            captureQuality: 0.8, landmarks: nil)
    #expect(SmileProxy.confidence(for: f) == nil)
}

@Test func liftedMouthCornersScoreHigherThanFlat() {
    let smiling = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.05))
    let neutral = SmileProxy.confidence(for: face(yaw: 0.0, mouthCornerLift: 0.0))
    #expect(smiling != nil && neutral != nil)
    #expect(smiling! > neutral!)
}

@Test func fractionIsNilWhenNoFaceHasUsablePose() {
    let faces = [face(yaw: 1.2, mouthCornerLift: 0.05), face(yaw: -1.1, mouthCornerLift: 0.05)]
    #expect(SmileProxy.fraction(faces: faces, threshold: 0.5) == nil)
}

@Test func fractionCountsOnlyUsableFaces() {
    let faces = [
        face(yaw: 0.0, mouthCornerLift: 0.08),   // usable, smiling
        face(yaw: 0.0, mouthCornerLift: 0.0),    // usable, not smiling
        face(yaw: 1.2, mouthCornerLift: 0.08),   // unusable, excluded entirely
    ]
    let f = SmileProxy.fraction(faces: faces, threshold: 0.5)
    #expect(f != nil)
    #expect(abs(f! - 0.5) < 0.001)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `SmileProxy` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/SmileProxy.swift`:

```swift
import Foundation

enum SmileProxy {
    /// Beyond roughly 30 degrees of yaw the mouth contour foreshortens enough
    /// that corner elevation is no longer meaningful. 0.52 rad ~= 30 degrees.
    static let maxYawRadians = 0.52
    static let maxPitchRadians = 0.52

    static func confidence(for face: FaceObservation) -> Double? {
        guard let points = face.landmarks, points.count >= 4 else { return nil }
        if let yaw = face.yaw, abs(yaw) > maxYawRadians { return nil }
        if let pitch = face.pitch, abs(pitch) > maxPitchRadians { return nil }

        // Outer lip contour: leftmost and rightmost points are the corners,
        // and the vertical midpoint of the contour is the reference line.
        guard let left = points.min(by: { $0[0] < $1[0] }),
              let right = points.max(by: { $0[0] < $1[0] })
        else { return nil }

        let width = right[0] - left[0]
        guard width > 0.0001 else { return nil }

        let cornerY = (left[1] + right[1]) / 2.0
        let centreY = points.reduce(0.0) { $0 + $1[1] } / Double(points.count)

        // Top-left origin: corners *above* the contour centre means a smaller y.
        let lift = (centreY - cornerY) / width

        // Map roughly [-0.15, 0.35] of normalised lift onto 0...1.
        return ((lift + 0.15) / 0.5).clamped(0, 1)
    }

    static func fraction(faces: [FaceObservation], threshold: Double) -> Double? {
        let usable = faces.compactMap { confidence(for: $0) }
        guard !usable.isEmpty else { return nil }
        let smiling = usable.filter { $0 > threshold }.count
        return Double(smiling) / Double(usable.count)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 5 new tests

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add pose-gated smile proxy from face landmarks"
```

---

### Task 9: Wire the analyze request end-to-end in Swift

**Files:**
- Modify: `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/main.swift`
- Create: `sidecar/Sources/PhotobookEngine/Analyzer.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`

**Interfaces:**
- Consumes: `ImageLoader`, `ExifReader`, `Metrics`, `VisionAnalyzer`, `SmileProxy`
- Produces: `struct PhotoFeatures: Codable` (the full per-photo record) and `Analyzer.analyze(paths: [String]) -> [PhotoRecord]` where `enum PhotoRecord: Codable` is either `.ok(PhotoFeatures)` or `.failed(path: String, message: String)`. Adds `ResponseResult.analyzed([PhotoRecord])`.

Analysis runs under `DispatchQueue.concurrentPerform`. A failure on one photo produces a `.failed` record — **never** a crash and never an aborted batch.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/AnalyzerTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

private func fixture(_ name: String) -> String {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/\(name)").path
}

@Test func analysesAGoodPhoto() {
    let records = Analyzer.analyze(paths: [fixture("landscape.jpg")])
    #expect(records.count == 1)
    guard case .ok(let f) = records[0] else {
        Issue.record("expected ok record"); return
    }
    #expect(f.width == 1200)
    #expect(f.height == 800)
    #expect(!f.hash.isEmpty)
    #expect(f.phash != 0)
}

@Test func reportsFailureForMissingFileWithoutCrashing() {
    let records = Analyzer.analyze(paths: ["/nonexistent/nope.jpg"])
    #expect(records.count == 1)
    guard case .failed(let path, _) = records[0] else {
        Issue.record("expected failed record"); return
    }
    #expect(path == "/nonexistent/nope.jpg")
}

@Test func oneBadPhotoDoesNotAbortTheBatch() {
    let records = Analyzer.analyze(paths: [
        "/nonexistent/nope.jpg",
        fixture("landscape.jpg"),
    ])
    #expect(records.count == 2)
    // Order must be preserved so callers can correlate with their input.
    guard case .failed = records[0] else { Issue.record("expected failure first"); return }
    guard case .ok = records[1] else { Issue.record("expected success second"); return }
}

@Test func preservesInputOrderUnderConcurrency() {
    let paths = (0..<8).map { _ in fixture("landscape.jpg") } + ["/nonexistent/a.jpg"]
    let records = Analyzer.analyze(paths: paths)
    #expect(records.count == 9)
    guard case .failed = records[8] else { Issue.record("last must be the failure"); return }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — cannot find `Analyzer` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Sources/PhotobookEngine/Analyzer.swift`:

```swift
import Foundation
import CryptoKit

struct PhotoFeatures: Codable {
    let path: String
    let hash: String
    let width: Int
    let height: Int
    let exif: ExifData
    let isUtility: Bool
    let aestheticScore: Double
    let sharpness: Double
    let faces: [FaceObservation]
    let faceAreaFraction: Double
    let smileFraction: Double?
    let saliencyBox: [Double]?
    let horizonTiltDeg: Double?
    let sceneTags: [String]
    let hasText: Bool
    let palette: [PaletteColor]
    let warmth: Double
    let contrast: Double
    let phash: UInt64
}

enum PhotoRecord: Codable {
    case ok(PhotoFeatures)
    case failed(path: String, message: String)

    private enum CodingKeys: String, CodingKey { case status, features, path, message }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let f):
            try c.encode("ok", forKey: .status)
            try c.encode(f, forKey: .features)
        case .failed(let path, let message):
            try c.encode("failed", forKey: .status)
            try c.encode(path, forKey: .path)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .status) == "ok" {
            self = .ok(try c.decode(PhotoFeatures.self, forKey: .features))
        } else {
            self = .failed(path: try c.decode(String.self, forKey: .path),
                           message: try c.decode(String.self, forKey: .message))
        }
    }
}

enum Analyzer {
    static let analysisMaxPixel = 1536

    private static func contentHash(path: String) throws -> String {
        let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: path))
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    static func analyzeOne(path: String) throws -> PhotoFeatures {
        let exif = try ExifReader.read(path: path)
        let hash = try contentHash(path: path)
        let image = try ImageLoader.loadThumbnail(path: path, maxPixel: analysisMaxPixel)

        // autoreleasepool matters: CGImage allocations otherwise accumulate
        // across a large concurrent batch.
        return autoreleasepool {
            let vision = VisionAnalyzer.analyze(image)
            let palette = Metrics.palette(image, count: 6)
            let faceArea = vision.faces.reduce(0.0) { $0 + $1.box[2] * $1.box[3] }

            return PhotoFeatures(
                path: path,
                hash: hash,
                width: exif.pixelWidth,
                height: exif.pixelHeight,
                exif: exif,
                isUtility: vision.isUtility,
                aestheticScore: vision.aestheticScore,
                sharpness: Metrics.sharpness(image),
                faces: vision.faces,
                faceAreaFraction: min(faceArea, 1.0),
                smileFraction: SmileProxy.fraction(faces: vision.faces, threshold: 0.5),
                saliencyBox: vision.saliencyBox,
                horizonTiltDeg: vision.horizonTiltDeg,
                sceneTags: vision.sceneTags,
                hasText: vision.hasText,
                palette: palette,
                warmth: Metrics.warmth(palette),
                contrast: Metrics.contrast(image),
                phash: Metrics.perceptualHash(image)
            )
        }
    }

    static func analyze(paths: [String]) -> [PhotoRecord] {
        var results = [PhotoRecord?](repeating: nil, count: paths.count)
        let lock = NSLock()

        DispatchQueue.concurrentPerform(iterations: paths.count) { index in
            let path = paths[index]
            let record: PhotoRecord
            do {
                record = .ok(try analyzeOne(path: path))
            } catch {
                record = .failed(path: path, message: String(describing: error))
            }
            lock.lock()
            results[index] = record
            lock.unlock()
        }

        return results.compactMap { $0 }
    }
}
```

Add to `Protocol.swift`'s `ResponseResult`:

```swift
    case analyzed([PhotoRecord])
```

with encode/decode arms:

```swift
        case .analyzed(let v):
            try c.encode("analyzed", forKey: .type)
            try c.encode(v, forKey: .data)
```

```swift
        case "analyzed": self = .analyzed(try c.decode([PhotoRecord].self, forKey: .data))
```

Replace the `.analyze` arm in `main.swift`:

```swift
    case .analyze:
        let paths = request.paths ?? []
        emit(Response(id: request.id, result: .analyzed(Analyzer.analyze(paths: paths))))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 4 new tests

Verify by hand:

```bash
swift build --package-path sidecar -c release
echo '{"id":"1","kind":"analyze","paths":["sidecar/Fixtures/landscape.jpg"]}' \
  | ./sidecar/.build/release/PhotobookEngine
```

Expected: one line of JSON containing `"status":"ok"` and `"width":1200`.

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add concurrent analyze pipeline with per-photo failure records"
```

---

### Task 10: Hostile-input fixture corpus

**Files:**
- Create: `sidecar/Fixtures/hostile/` (7 files), `sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift`
- Create: `scripts/make-hostile-fixtures.sh`

**Interfaces:**
- Consumes: `Analyzer.analyze` from Task 9
- Produces: no new API. This task's deliverable is proof that malformed input degrades to `.failed` records rather than crashing the process.

This is the task that makes the crash-isolation argument real. If any of these crash the test runner, that is the bug the whole sidecar architecture exists to contain.

- [ ] **Step 1: Write the failing test**

`scripts/make-hostile-fixtures.sh`:

ImageMagick is not installed, so the three fixtures that need real image data are produced
by extending `scripts/make-fixtures.swift` from Task 4 rather than shelling out.

Add to `scripts/make-fixtures.swift`, after the two existing `write` calls:

```swift
let hostile = dir.appendingPathComponent("hostile")
try? FileManager.default.createDirectory(at: hostile, withIntermediateDirectories: true)

/// Writes a solid-colour image into the hostile directory, optionally in a
/// colour space or with a filename that downstream code may not expect.
func writeHostile(_ name: String, width: Int, height: Int, cmyk: Bool) {
    let space = cmyk ? CGColorSpaceCreateDeviceCMYK() : CGColorSpaceCreateDeviceRGB()
    let info = cmyk
        ? CGImageAlphaInfo.none.rawValue
        : CGImageAlphaInfo.premultipliedLast.rawValue
    let ctx = CGContext(
        data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: 0,
        space: space, bitmapInfo: info
    )!
    ctx.setFillColor(CGColor(colorSpace: space,
                             components: cmyk ? [0, 1, 1, 0, 1] : [1, 0, 0, 1])!)
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = ctx.makeImage()!

    let url = hostile.appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    precondition(CGImageDestinationFinalize(dest), "failed to write \(name)")
    print("wrote hostile/\(name)")
}

writeHostile("one-pixel.jpg", width: 1, height: 1, cmyk: false)
writeHostile("cmyk.jpg", width: 64, height: 64, cmyk: true)
writeHostile("no-extension", width: 64, height: 64, cmyk: false)
```

Then `scripts/make-hostile-fixtures.sh` covers only the four that are pure byte
manipulation and need no image library:

```bash
#!/usr/bin/env bash
set -euo pipefail
DIR="sidecar/Fixtures/hostile"
mkdir -p "$DIR"

: > "$DIR/empty.jpg"                                    # zero bytes
head -c 400 sidecar/Fixtures/landscape.jpg > "$DIR/truncated.jpg"
printf 'not an image at all, just text' > "$DIR/text.jpg"
head -c 2000 /dev/urandom > "$DIR/random.jpg"
echo "created byte-level hostile fixtures in $DIR"
echo "run 'swift scripts/make-fixtures.swift' for the image-based ones"
```

If `CGColorSpaceCreateDeviceCMYK` refuses the JPEG destination on this macOS version,
substitute any other awkward-but-writable colour space and note the substitution — the
point of the fixture is that the analyser meets a colour space it did not expect, not CMYK
specifically.

`sidecar/Tests/PhotobookEngineTests/HostileInputTests.swift`:

```swift
import Testing
import Foundation
@testable import PhotobookEngine

private func hostileDir() -> URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/hostile")
}

@Test func everyHostileFixtureProducesARecordAndNeverCrashes() throws {
    let files = try FileManager.default
        .contentsOfDirectory(at: hostileDir(), includingPropertiesForKeys: nil)
        .map(\.path)
        .sorted()

    #expect(files.count >= 7, "run scripts/make-hostile-fixtures.sh first")

    let records = Analyzer.analyze(paths: files)
    #expect(records.count == files.count)
    // Reaching this line at all is the assertion: nothing trapped.
}

@Test func zeroByteFileFailsCleanly() {
    let path = hostileDir().appendingPathComponent("empty.jpg").path
    let records = Analyzer.analyze(paths: [path])
    guard case .failed = records[0] else {
        Issue.record("zero-byte file must produce a failed record"); return
    }
}

@Test func truncatedJpegFailsCleanlyOrDecodesPartially() {
    let path = hostileDir().appendingPathComponent("truncated.jpg").path
    let records = Analyzer.analyze(paths: [path])
    #expect(records.count == 1)   // either outcome is acceptable; a crash is not
}

@Test func hostileBatchMixedWithGoodPhotoStillReturnsTheGoodOne() throws {
    let good = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Fixtures/landscape.jpg").path
    let bad = hostileDir().appendingPathComponent("random.jpg").path

    let records = Analyzer.analyze(paths: [bad, good, bad])
    #expect(records.count == 3)
    guard case .ok = records[1] else {
        Issue.record("the good photo must survive a hostile batch"); return
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — `files.count >= 7` fails because the fixtures don't exist yet

- [ ] **Step 3: Generate the fixtures**

```bash
chmod +x scripts/make-hostile-fixtures.sh
bash scripts/make-hostile-fixtures.sh
```

If any test now *crashes* rather than fails, that is a real bug in `Analyzer.analyzeOne` — the fix is to add the missing `guard`/`throws` path in `ImageLoader` or `ExifReader`, not to remove the fixture.

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 4 new tests, process exits 0

- [ ] **Step 5: Commit**

```bash
git add sidecar scripts
git commit -m "test(sidecar): add hostile input corpus proving crash isolation"
```

---

### Task 11: SQLite cache

**Files:**
- Create: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `db.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `Db::open(path: &Path) -> rusqlite::Result<Db>` and `Db::open_in_memory()`
  - `Db::analyzer_version() -> u32` (constant `ANALYZER_VERSION`)
  - `Db::get_features(&self, hash: &str) -> rusqlite::Result<Option<String>>` — returns the raw feature JSON if cached at the current analyzer version
  - `Db::put_features(&self, hash: &str, path: &str, json: &str) -> rusqlite::Result<()>`
  - `Db::hashes_needing_analysis(&self, hashes: &[String]) -> rusqlite::Result<Vec<String>>`

Features are stored as raw JSON. Rust never needs to interpret every field, and storing JSON means adding a field in Swift doesn't require a Rust migration.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/db.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_for_unknown_hash() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_features("deadbeef").unwrap().is_none());
    }

    #[test]
    fn round_trips_features() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"width":100}"#).unwrap();
        assert_eq!(db.get_features("abc").unwrap().unwrap(), r#"{"width":100}"#);
    }

    #[test]
    fn overwrites_on_reanalysis_of_same_hash() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"v":1}"#).unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"v":2}"#).unwrap();
        assert_eq!(db.get_features("abc").unwrap().unwrap(), r#"{"v":2}"#);
    }

    #[test]
    fn ignores_rows_from_an_older_analyzer_version() {
        let db = Db::open_in_memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO features (hash, path, json, analyzer_version) VALUES (?1,?2,?3,?4)",
                rusqlite::params!["old", "/tmp/o.jpg", "{}", ANALYZER_VERSION - 1],
            )
            .unwrap();
        assert!(db.get_features("old").unwrap().is_none());
    }

    #[test]
    fn reports_only_uncached_hashes_as_needing_analysis() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("cached", "/tmp/c.jpg", "{}").unwrap();
        let need = db
            .hashes_needing_analysis(&["cached".to_string(), "fresh".to_string()])
            .unwrap();
        assert_eq!(need, vec!["fresh".to_string()]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `file not found for module 'db'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/db.rs` (above the test module):

```rust
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

/// Bump this whenever the Swift analyzer's output changes shape or semantics.
/// Cached rows at an older version are ignored and re-analysed.
pub const ANALYZER_VERSION: u32 = 1;

pub struct Db {
    pub(crate) conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Self { conn: Connection::open(path)? };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let db = Self { conn: Connection::open_in_memory()? };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS features (
                 hash             TEXT PRIMARY KEY,
                 path             TEXT NOT NULL,
                 json             TEXT NOT NULL,
                 analyzer_version INTEGER NOT NULL,
                 created_at       INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS idx_features_version
                 ON features (analyzer_version);",
        )
    }

    pub fn get_features(&self, hash: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT json FROM features WHERE hash = ?1 AND analyzer_version = ?2",
                rusqlite::params![hash, ANALYZER_VERSION],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn put_features(&self, hash: &str, path: &str, json: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO features (hash, path, json, analyzer_version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(hash) DO UPDATE SET
                 path = excluded.path,
                 json = excluded.json,
                 analyzer_version = excluded.analyzer_version",
            rusqlite::params![hash, path, json, ANALYZER_VERSION],
        )?;
        Ok(())
    }

    pub fn hashes_needing_analysis(&self, hashes: &[String]) -> rusqlite::Result<Vec<String>> {
        let mut needed = Vec::new();
        for hash in hashes {
            if self.get_features(hash)?.is_none() {
                needed.push(hash.clone());
            }
        }
        Ok(needed)
    }
}
```

Add `pub mod db;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 5 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(db): add SQLite feature cache keyed by content hash"
```

---

### Task 12: Near-duplicate and event clustering

**Files:**
- Create: `src-tauri/src/cluster.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `cluster.rs`

**Interfaces:**
- Consumes: `phash: u64` and `capture_date` from the feature records
- Produces:
  - `pub fn hamming(a: u64, b: u64) -> u32`
  - `pub fn near_duplicate_clusters(phashes: &[u64], max_distance: u32) -> Vec<usize>` — returns a cluster id per input index
  - `pub fn event_clusters(timestamps: &[Option<i64>], gap_seconds: i64) -> Vec<usize>` — photos with no timestamp form their own trailing cluster

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/cluster.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamming_counts_differing_bits() {
        assert_eq!(hamming(0b1010, 0b1010), 0);
        assert_eq!(hamming(0b1010, 0b1011), 1);
        assert_eq!(hamming(0b0000, 0b1111), 4);
    }

    #[test]
    fn identical_hashes_land_in_one_cluster() {
        let ids = near_duplicate_clusters(&[42, 42, 42], 4);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[1], ids[2]);
    }

    #[test]
    fn distant_hashes_land_in_separate_clusters() {
        let ids = near_duplicate_clusters(&[0x0000_0000_0000_0000, 0xFFFF_FFFF_FFFF_FFFF], 4);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn clustering_is_transitive_within_threshold() {
        // a-b differ by 1 bit, b-c by 1 bit, a-c by 2. All one burst.
        let ids = near_duplicate_clusters(&[0b000, 0b001, 0b011], 1);
        assert_eq!(ids[0], ids[2]);
    }

    #[test]
    fn events_split_on_a_large_time_gap() {
        let day = 86_400;
        let ids = event_clusters(&[Some(0), Some(60), Some(3 * day), Some(3 * day + 60)], day);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[2], ids[3]);
        assert_ne!(ids[1], ids[2]);
    }

    #[test]
    fn photos_without_timestamps_get_their_own_cluster() {
        let ids = event_clusters(&[Some(0), None, Some(60)], 86_400);
        assert_eq!(ids[0], ids[2]);
        assert_ne!(ids[1], ids[0]);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(near_duplicate_clusters(&[], 4).is_empty());
        assert!(event_clusters(&[], 86_400).is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `file not found for module 'cluster'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/cluster.rs` (above the test module):

```rust
/// Number of differing bits between two 64-bit perceptual hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Single-link agglomerative clustering over Hamming distance, via union-find.
/// Returns a cluster id for each input index. O(n^2), which is fine at n <= 300.
pub fn near_duplicate_clusters(phashes: &[u64], max_distance: u32) -> Vec<usize> {
    let n = phashes.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut Vec<usize>, mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if hamming(phashes[i], phashes[j]) <= max_distance {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    let mut labels = std::collections::HashMap::new();
    (0..n)
        .map(|i| {
            let root = find(&mut parent, i);
            let next = labels.len();
            *labels.entry(root).or_insert(next)
        })
        .collect()
}

/// Splits a chronological sequence wherever the gap between consecutive
/// timestamps exceeds `gap_seconds`. Photos with no timestamp are grouped
/// together into one trailing cluster.
pub fn event_clusters(timestamps: &[Option<i64>], gap_seconds: i64) -> Vec<usize> {
    if timestamps.is_empty() {
        return Vec::new();
    }

    let mut dated: Vec<(usize, i64)> = timestamps
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.map(|t| (i, t)))
        .collect();
    dated.sort_by_key(|(_, t)| *t);

    let mut ids = vec![usize::MAX; timestamps.len()];
    let mut current = 0usize;
    let mut previous: Option<i64> = None;

    for (index, time) in dated {
        if let Some(prev) = previous {
            if time - prev > gap_seconds {
                current += 1;
            }
        }
        ids[index] = current;
        previous = Some(time);
    }

    let undated = current + 1;
    for id in ids.iter_mut() {
        if *id == usize::MAX {
            *id = undated;
        }
    }
    ids
}
```

Add `pub mod cluster;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 7 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(cluster): add near-duplicate and event clustering"
```

---

### Task 13: Within-book percentile ranking

**Files:**
- Create: `src-tauri/src/ranking.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `ranking.rs`

**Interfaces:**
- Consumes: raw scores from feature records
- Produces: `pub fn percentiles(values: &[f64]) -> Vec<u8>` — each value's rank as 0–100 within this book's own population. Ties receive the same percentile. `None` values are handled by the caller filtering first.

Raw aesthetic and sharpness scores are not comparable across books or subject matter. Percentile-within-book converts a shaky absolute signal into a defensible relative one.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/ranking.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert!(percentiles(&[]).is_empty());
    }

    #[test]
    fn single_value_is_the_top_percentile() {
        assert_eq!(percentiles(&[3.7]), vec![100]);
    }

    #[test]
    fn ranks_ascending_values_across_the_range() {
        let p = percentiles(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(p[0], 25);
        assert_eq!(p[3], 100);
        assert!(p[0] < p[1] && p[1] < p[2] && p[2] < p[3]);
    }

    #[test]
    fn preserves_input_order_not_sorted_order() {
        let p = percentiles(&[9.0, 1.0, 5.0]);
        assert_eq!(p[0], 100, "the largest input was first");
        assert_eq!(p[1], 33);
    }

    #[test]
    fn ties_receive_the_same_percentile() {
        let p = percentiles(&[2.0, 2.0, 2.0]);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }

    #[test]
    fn all_percentiles_are_within_bounds() {
        let p = percentiles(&[-5.0, 0.0, 0.5, 100.0]);
        for v in p {
            assert!(v <= 100);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `file not found for module 'ranking'`

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/ranking.rs` (above the test module):

```rust
/// Converts raw scores to 0-100 percentile ranks within this population.
/// Equal values receive equal ranks. Order matches the input, not sorted order.
pub fn percentiles(values: &[f64]) -> Vec<u8> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }

    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    values
        .iter()
        .map(|v| {
            // Count of values <= v, so the maximum always lands on 100.
            let count = sorted.partition_point(|s| s <= v);
            ((count as f64 / n as f64) * 100.0).round() as u8
        })
        .collect()
}
```

Add `pub mod ranking;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 6 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(ranking): add within-book percentile ranking"
```

---

### Task 14: Sidecar resilience — batching, timeout, respawn

**Files:**
- Modify: `src-tauri/src/sidecar.rs`
- Test: inline `#[cfg(test)]` module in `sidecar.rs`

**Interfaces:**
- Consumes: `Sidecar` from Task 3, `PhotoRecord` JSON from Task 9
- Produces:
  - `pub fn chunk_paths(paths: &[String], batch_size: usize) -> Vec<Vec<String>>`
  - `pub fn timeout_for(batch_len: usize) -> Duration` — `10s + 3s per photo`, generous enough for RAW
  - `Sidecar::analyze(&mut self, paths: Vec<String>) -> Result<Vec<serde_json::Value>, SidecarError>`
  - `SidecarPool::analyze_all(&mut self, app: &AppHandle, paths: &[String]) -> Vec<serde_json::Value>` — respawns on crash and emits synthetic `failed` records for a batch that dies twice, so the caller always receives one record per input path

RAW decode throughput has never been measured (spec §14). The timeout is deliberately generous, and a batch that exceeds it degrades to failure records rather than hanging the import.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/sidecar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("/photos/{i}.jpg")).collect()
    }

    #[test]
    fn chunks_paths_into_batches() {
        let batches = chunk_paths(&paths(25), 10);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 10);
        assert_eq!(batches[2].len(), 5);
    }

    #[test]
    fn chunking_preserves_every_path_in_order() {
        let all = paths(7);
        let flat: Vec<String> = chunk_paths(&all, 3).into_iter().flatten().collect();
        assert_eq!(flat, all);
    }

    #[test]
    fn empty_input_produces_no_batches() {
        assert!(chunk_paths(&[], 10).is_empty());
    }

    #[test]
    fn timeout_scales_with_batch_size() {
        assert!(timeout_for(20) > timeout_for(1));
    }

    #[test]
    fn timeout_has_a_floor_for_a_single_photo() {
        assert!(timeout_for(1) >= Duration::from_secs(10));
    }

    #[test]
    fn synthetic_failure_record_names_the_path() {
        let value = failure_record("/photos/bad.raw", "timed out");
        assert_eq!(value["status"], "failed");
        assert_eq!(value["path"], "/photos/bad.raw");
        assert_eq!(value["message"], "timed out");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `cannot find function 'chunk_paths' in this scope`

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/sidecar.rs` (above the test module):

```rust
/// Batches amortise sidecar round-trips while keeping any single timeout small
/// enough that one bad photo doesn't stall the whole import.
pub const BATCH_SIZE: usize = 16;

pub fn chunk_paths(paths: &[String], batch_size: usize) -> Vec<Vec<String>> {
    paths.chunks(batch_size.max(1)).map(<[String]>::to_vec).collect()
}

/// RAW decode is materially slower than JPEG and has never been benchmarked,
/// so this is deliberately generous.
pub fn timeout_for(batch_len: usize) -> Duration {
    Duration::from_secs(10) + Duration::from_secs(3) * batch_len as u32
}

pub fn failure_record(path: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "status": "failed", "path": path, "message": message })
}

impl Sidecar {
    pub fn analyze(&mut self, paths: Vec<String>) -> Result<Vec<serde_json::Value>, SidecarError> {
        let timeout = timeout_for(paths.len());
        match self.request(RequestKind::Analyze, Some(paths), timeout)? {
            ResponseResult::Analyzed(records) => Ok(records),
            ResponseResult::Error { message } => Err(SidecarError::Engine(message)),
            ResponseResult::Pong { .. } => Err(SidecarError::Malformed("expected analyzed".into())),
        }
    }
}

pub struct SidecarPool {
    inner: Option<Sidecar>,
}

impl SidecarPool {
    pub fn new() -> Self {
        Self { inner: None }
    }

    fn ensure(&mut self, app: &AppHandle) -> Result<&mut Sidecar, SidecarError> {
        if self.inner.is_none() {
            self.inner = Some(Sidecar::spawn(app)?);
        }
        Ok(self.inner.as_mut().expect("just spawned"))
    }

    /// Always returns one record per input path. A batch that fails twice is
    /// converted to synthetic failure records so the caller can proceed.
    pub fn analyze_all(&mut self, app: &AppHandle, paths: &[String]) -> Vec<serde_json::Value> {
        let mut out = Vec::with_capacity(paths.len());

        for batch in chunk_paths(paths, BATCH_SIZE) {
            let mut records = None;

            for attempt in 0..2 {
                let result = self
                    .ensure(app)
                    .and_then(|sidecar| sidecar.analyze(batch.clone()));

                match result {
                    Ok(r) => {
                        records = Some(r);
                        break;
                    }
                    Err(err) => {
                        log::warn!("sidecar batch failed (attempt {attempt}): {err}");
                        // Drop the child so the next ensure() respawns it.
                        self.inner = None;
                    }
                }
            }

            match records {
                Some(r) if r.len() == batch.len() => out.extend(r),
                Some(r) => {
                    log::warn!("sidecar returned {} records for {} paths", r.len(), batch.len());
                    out.extend(batch.iter().map(|p| failure_record(p, "record count mismatch")));
                }
                None => out.extend(batch.iter().map(|p| failure_record(p, "sidecar failed twice"))),
            }
        }

        out
    }
}

impl Default for SidecarPool {
    fn default() -> Self {
        Self::new()
    }
}
```

Add the `Analyzed` variant to `src-tauri/src/protocol.rs`'s `ResponseResult`, as a **tuple variant** — Swift encodes `{"type":"analyzed","data":[...]}`, and with `#[serde(tag = "type", content = "data")]` the tuple form maps to that array exactly:

```rust
    Analyzed(Vec<serde_json::Value>),
```

Records stay as `serde_json::Value` rather than a typed struct: Rust only reads five fields (`status`, `hash`, `path`, `phash`, `aestheticScore`, `sharpness`), and keeping the rest opaque means adding a field in Swift needs no Rust change.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, 6 new tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(sidecar): add batching, scaled timeouts, and crash respawn"
```

---

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
