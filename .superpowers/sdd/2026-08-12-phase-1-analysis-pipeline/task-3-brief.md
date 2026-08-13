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

