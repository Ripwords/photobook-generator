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
