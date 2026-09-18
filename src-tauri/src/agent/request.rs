//! The one way a model request leaves the app. The webview names a provider,
//! a path and a JSON body; Rust picks the host, checks the path, and attaches
//! the key, so neither a host nor a key ever passes through JavaScript.

use crate::agent::keys::{KeyError, Provider};
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::sync::watch;

/// Streamed over the request's channel. Once `run` accepts a request, the
/// channel carries at most one `Head`, then `Chunk`s, then exactly one of
/// `End`, `Failed` or `Cancelled`. `Failed` can arrive without a `Head` when
/// the connection never produced a response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ModelEvent {
    /// `headers` is `[name, value]` pairs in arrival order, repeats kept, so
    /// the webview can pass it straight to `new Headers(...)`.
    Head {
        status: u16,
        headers: Vec<(String, String)>,
    },
    /// Raw body bytes as a JSON number array. A chunk boundary can split a
    /// UTF-8 character, so the bytes are never decoded here.
    Chunk {
        bytes: Vec<u8>,
    },
    End,
    Failed {
        message: String,
    },
    Cancelled,
}

/// Why `model_request` rejected a request before connecting to anything.
#[derive(Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ModelRequestError {
    #[error("{message}")]
    Refused { message: String },
    #[error("{message}")]
    MissingKey { provider: Provider, message: String },
    #[error("{message}")]
    Keychain { message: String },
}

impl From<KeyError> for ModelRequestError {
    fn from(e: KeyError) -> Self {
        let message = e.to_string();
        match e {
            KeyError::Missing(provider) => ModelRequestError::MissingKey { provider, message },
            KeyError::Store(_) => ModelRequestError::Keychain { message },
        }
    }
}

pub fn base_url(provider: Provider) -> String {
    match provider {
        Provider::DeepSeek => "https://api.deepseek.com",
        Provider::Jev => "https://api.typesafe.ai",
    }
    .to_string()
}

/// Compared for exact equality, so no suffix, query or fragment gets through.
pub fn allowed_path(provider: Provider) -> &'static str {
    match provider {
        Provider::DeepSeek => "/chat/completions",
        Provider::Jev => "/v1/systemone",
    }
}

/// A cancel that arrives before its request registers is kept as an entry
/// already set to `true`, because the two commands race on the async runtime.
type InFlight = Mutex<HashMap<String, Arc<watch::Sender<bool>>>>;

/// Shared by every `model_request`: one HTTP client, so the TLS connection to
/// a provider is reused across turns, and the requests in flight by id.
pub struct ModelRequests {
    client: reqwest::Client,
    in_flight: Arc<InFlight>,
}

enum Registration {
    Live(Guard),
    AlreadyCancelled,
}

/// Removes the id when the request finishes, however it finishes.
struct Guard {
    id: String,
    in_flight: Arc<InFlight>,
    cancelled: watch::Receiver<bool>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.in_flight.lock() {
            map.remove(&self.id);
        }
    }
}

impl ModelRequests {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            // DeepSeek keeps a connection open for up to 10 minutes before
            // inference starts, so only a peer silent for longer is dead.
            .read_timeout(Duration::from_secs(11 * 60))
            // A redirect could lead to a host outside the allowlist.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the rustls HTTP client builds");
        Self {
            client,
            in_flight: Arc::default(),
        }
    }

    pub fn cancel(&self, id: &str) {
        let mut map = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(id.to_string())
            .or_insert_with(|| Arc::new(watch::channel(false).0))
            .send_replace(true);
    }

    fn register(&self, id: &str) -> Result<Registration, ModelRequestError> {
        let mut map = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        match map.entry(id.to_string()) {
            Entry::Occupied(e) if *e.get().borrow() => {
                e.remove();
                Ok(Registration::AlreadyCancelled)
            }
            Entry::Occupied(_) => Err(ModelRequestError::Refused {
                message: format!("A model request with id {id:?} is already running."),
            }),
            Entry::Vacant(e) => {
                let sender = e.insert(Arc::new(watch::channel(false).0));
                Ok(Registration::Live(Guard {
                    id: id.to_string(),
                    in_flight: self.in_flight.clone(),
                    cancelled: sender.subscribe(),
                }))
            }
        }
    }

    #[cfg(test)]
    fn in_flight(&self) -> usize {
        self.in_flight.lock().unwrap().len()
    }
}

impl Default for ModelRequests {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Outbound {
    pub id: String,
    pub provider: Provider,
    pub path: String,
    pub body: String,
}

/// The webview stopped listening, so there is no one to stream to.
struct Gone;

/// Checks the path, then reads the key, then connects: a refused path never
/// touches the keychain or the network. Errors returned here all happen
/// before a connection; everything after is reported on `events`.
pub async fn run(
    requests: &ModelRequests,
    request: Outbound,
    base: impl FnOnce(Provider) -> String,
    key: impl FnOnce(Provider) -> Result<String, KeyError> + Send + 'static,
    events: &Channel<ModelEvent>,
) -> Result<(), ModelRequestError> {
    let Outbound {
        id,
        provider,
        path,
        body,
    } = request;
    if path != allowed_path(provider) {
        return Err(ModelRequestError::Refused {
            message: format!(
                "{path:?} is not a {} endpoint this app calls.",
                provider.label()
            ),
        });
    }
    let key = tokio::task::spawn_blocking(move || key(provider))
        .await
        .map_err(|e| ModelRequestError::Keychain {
            message: e.to_string(),
        })??;

    let mut guard = match requests.register(&id)? {
        Registration::Live(guard) => guard,
        Registration::AlreadyCancelled => {
            let _ = events.send(ModelEvent::Cancelled);
            return Ok(());
        }
    };
    let call = requests
        .client
        .post(format!("{}{path}", base(provider)))
        .bearer_auth(key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body);

    let terminal = tokio::select! {
        _ = guard.cancelled.wait_for(|cancelled| *cancelled) => ModelEvent::Cancelled,
        streamed = stream(call, events) => match streamed {
            Ok(Ok(())) => ModelEvent::End,
            Ok(Err(e)) => ModelEvent::Failed { message: describe(&e) },
            Err(Gone) => return Ok(()),
        },
    };
    let _ = events.send(terminal);
    Ok(())
}

async fn stream(
    call: reqwest::RequestBuilder,
    events: &Channel<ModelEvent>,
) -> Result<Result<(), reqwest::Error>, Gone> {
    let mut response = match call.send().await {
        Ok(response) => response,
        Err(e) => return Ok(Err(e)),
    };
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let emit = |event| events.send(event).map_err(|_| Gone);
    emit(ModelEvent::Head {
        status: response.status().as_u16(),
        headers,
    })?;
    loop {
        match response.chunk().await {
            Ok(Some(bytes)) => emit(ModelEvent::Chunk {
                bytes: bytes.to_vec(),
            })?,
            Ok(None) => return Ok(Ok(())),
            Err(e) => return Ok(Err(e)),
        }
    }
}

/// reqwest's top-level message omits the cause, such as "connection refused".
fn describe(e: &dyn std::error::Error) -> String {
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::keys::{resolve_key, KeyStore, MemoryStore};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tauri::ipc::InvokeResponseBody;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;

    const LIMIT: Duration = Duration::from_secs(5);

    #[derive(Clone)]
    enum Step {
        Write(Vec<u8>),
        Pause(u64),
        /// Keeps the connection open until the client closes it.
        Hold,
    }

    fn write(s: &str) -> Step {
        Step::Write(s.as_bytes().to_vec())
    }

    /// A loopback HTTP/1.1 server that plays one script per connection and
    /// records what it saw.
    struct Server {
        port: u16,
        accepts: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<String>>>,
        client_closed: mpsc::UnboundedReceiver<()>,
    }

    impl Server {
        async fn start(script: Vec<Step>) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let accepts = Arc::new(AtomicUsize::new(0));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let (closed_tx, client_closed) = mpsc::unbounded_channel();
            let (a, r) = (accepts.clone(), requests.clone());
            tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    a.fetch_add(1, Ordering::SeqCst);
                    let (script, r, closed_tx) = (script.clone(), r.clone(), closed_tx.clone());
                    tokio::spawn(async move {
                        serve(stream, script, r, closed_tx).await;
                    });
                }
            });
            Server {
                port,
                accepts,
                requests,
                client_closed,
            }
        }

        fn base(&self) -> impl FnOnce(Provider) -> String {
            let port = self.port;
            move |_| format!("http://127.0.0.1:{port}")
        }

        fn accepts(&self) -> usize {
            self.accepts.load(Ordering::SeqCst)
        }

        fn request(&self) -> String {
            self.requests.lock().unwrap()[0].clone()
        }
    }

    async fn serve(
        mut stream: TcpStream,
        script: Vec<Step>,
        requests: Arc<Mutex<Vec<String>>>,
        closed: mpsc::UnboundedSender<()>,
    ) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream.read(&mut tmp).await.unwrap();
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        let (name, value) = l.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= end + 4 + length {
                    requests.lock().unwrap().push(text);
                    break;
                }
            }
        }
        for step in script {
            match step {
                Step::Write(bytes) => {
                    if stream.write_all(&bytes).await.is_err() {
                        return;
                    }
                    let _ = stream.flush().await;
                }
                Step::Pause(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
                Step::Hold => {
                    while stream.read(&mut tmp).await.map(|n| n > 0).unwrap_or(false) {}
                    let _ = closed.send(());
                    return;
                }
            }
        }
    }

    fn chunked(parts: &[&[u8]]) -> Vec<Step> {
        let mut steps = vec![write(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n",
        )];
        for part in parts {
            let mut frame = format!("{:x}\r\n", part.len()).into_bytes();
            frame.extend_from_slice(part);
            frame.extend_from_slice(b"\r\n");
            steps.push(Step::Write(frame));
            steps.push(Step::Pause(60));
        }
        steps.push(write("0\r\n\r\n"));
        steps
    }

    /// A channel that records every event as the JSON the webview receives.
    fn recorder() -> (
        Channel<ModelEvent>,
        Arc<Mutex<Vec<Value>>>,
        mpsc::UnboundedReceiver<Value>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let s = seen.clone();
        let channel = Channel::new(move |body| {
            let InvokeResponseBody::Json(json) = body else {
                panic!("model events are JSON");
            };
            let value: Value = serde_json::from_str(&json).unwrap();
            s.lock().unwrap().push(value.clone());
            let _ = tx.send(value);
            Ok(())
        });
        (channel, seen, rx)
    }

    fn kinds(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .map(|e| e["kind"].as_str().unwrap().to_string())
            .collect()
    }

    fn body_of(events: &[Value]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter(|e| e["kind"] == "chunk")
            .map(|e| serde_json::from_value(e["bytes"].clone()).unwrap())
            .collect()
    }

    fn outbound(id: &str, provider: Provider, path: &str) -> Outbound {
        Outbound {
            id: id.into(),
            provider,
            path: path.into(),
            body: r#"{"model":"deepseek-flash"}"#.into(),
        }
    }

    fn keys() -> impl FnOnce(Provider) -> Result<String, KeyError> + Send + 'static {
        let store = MemoryStore::default();
        store.set(Provider::DeepSeek, "sk-deepseek").unwrap();
        store.set(Provider::Jev, "sk-jev").unwrap();
        move |p| resolve_key(&store, p, |_| None)
    }

    async fn within<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(LIMIT, f).await.expect("timed out")
    }

    fn ok_json(body: &str) -> Vec<Step> {
        vec![write(&format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        ))]
    }

    #[test]
    fn production_hosts_and_paths() {
        assert_eq!(base_url(Provider::DeepSeek), "https://api.deepseek.com");
        assert_eq!(base_url(Provider::Jev), "https://api.typesafe.ai");
        assert_eq!(allowed_path(Provider::DeepSeek), "/chat/completions");
        assert_eq!(allowed_path(Provider::Jev), "/v1/systemone");
    }

    #[tokio::test]
    async fn auth_header_comes_from_the_store_for_the_named_provider() {
        for (provider, path, key, other) in [
            (
                Provider::DeepSeek,
                "/chat/completions",
                "sk-deepseek",
                "sk-jev",
            ),
            (Provider::Jev, "/v1/systemone", "sk-jev", "sk-deepseek"),
        ] {
            let server = Server::start(ok_json("{}")).await;
            let requests = ModelRequests::new();
            let (channel, _, _) = recorder();
            within(run(
                &requests,
                outbound("a", provider, path),
                server.base(),
                keys(),
                &channel,
            ))
            .await
            .unwrap();

            let seen = server.request();
            let lower = seen.to_ascii_lowercase();
            assert!(
                seen.starts_with(&format!("POST {path} HTTP/1.1\r\n")),
                "{seen}"
            );
            assert!(
                seen.contains(&format!("authorization: Bearer {key}\r\n")),
                "{seen}"
            );
            assert!(!seen.contains(other), "{seen}");
            assert!(
                lower.contains("content-type: application/json\r\n"),
                "{seen}"
            );
            assert!(
                seen.ends_with("\r\n\r\n{\"model\":\"deepseek-flash\"}"),
                "{seen}"
            );
        }
    }

    #[tokio::test]
    async fn a_path_outside_the_allowlist_is_refused_before_the_key_or_a_connection() {
        let server = Server::start(ok_json("{}")).await;
        let requests = ModelRequests::new();
        let lookups = Arc::new(AtomicUsize::new(0));
        for path in [
            "/v1/systemone",
            "/chat/completions/",
            "/chat/completions?stream=true",
            "/chat/completions#x",
            "/chat/completionsx",
            "/chat/Completions",
            " /chat/completions",
            "/",
            "",
            "//evil.example/chat/completions",
            "/v1/../chat/completions",
            "@evil.example/chat/completions",
            "https://evil.example/chat/completions",
        ] {
            let (channel, seen, _) = recorder();
            let l = lookups.clone();
            let key = move |_| {
                l.fetch_add(1, Ordering::SeqCst);
                Ok("sk-deepseek".to_string())
            };
            let err = within(run(
                &requests,
                outbound("a", Provider::DeepSeek, path),
                server.base(),
                key,
                &channel,
            ))
            .await
            .unwrap_err();
            assert!(
                matches!(err, ModelRequestError::Refused { .. }),
                "{path:?}: {err:?}"
            );
            assert!(seen.lock().unwrap().is_empty(), "{path:?} emitted events");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(lookups.load(Ordering::SeqCst), 0, "a key was read");
        assert_eq!(server.accepts(), 0, "a connection was made");

        let (channel, _, _) = recorder();
        within(run(
            &requests,
            outbound("b", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();
        assert_eq!(server.accepts(), 1, "the server does count accepts");
    }

    #[tokio::test]
    async fn a_missing_key_is_a_typed_error_and_nothing_connects() {
        let server = Server::start(ok_json("{}")).await;
        let requests = ModelRequests::new();
        let (channel, seen, _) = recorder();
        let empty = MemoryStore::default();
        let err = within(run(
            &requests,
            outbound("a", Provider::Jev, "/v1/systemone"),
            server.base(),
            move |p| resolve_key(&empty, p, |_| None),
            &channel,
        ))
        .await
        .unwrap_err();
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({
                "kind": "missingKey",
                "provider": "jev",
                "message": "No Jev API key is set. Add it in Settings.",
            })
        );
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(server.accepts(), 0);
    }

    #[test]
    fn other_errors_serialise_with_their_kind() {
        let refused = ModelRequestError::Refused {
            message: "no".into(),
        };
        assert_eq!(
            serde_json::to_value(&refused).unwrap(),
            json!({ "kind": "refused", "message": "no" })
        );
        let keychain: ModelRequestError = KeyError::Store("locked".into()).into();
        assert_eq!(
            serde_json::to_value(&keychain).unwrap(),
            json!({ "kind": "keychain", "message": "locked" })
        );
    }

    #[tokio::test]
    async fn a_chunked_body_arrives_as_ordered_chunk_events() {
        let parts: [&[u8]; 3] = [
            b"data: one\n\n",
            b"data: caf\xc3",
            b"\xa9\n\ndata: [DONE]\n\n",
        ];
        let server = Server::start(chunked(&parts)).await;
        let requests = ModelRequests::new();
        let (channel, seen, _) = recorder();
        within(run(
            &requests,
            outbound("a", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();

        let events = seen.lock().unwrap().clone();
        assert_eq!(kinds(&events), ["head", "chunk", "chunk", "chunk", "end"]);
        assert_eq!(events[0]["status"], 200);
        assert_eq!(body_of(&events), parts.map(<[u8]>::to_vec));
        assert_eq!(
            events[1],
            json!({ "kind": "chunk", "bytes": b"data: one\n\n".to_vec() })
        );
        assert_eq!(events[4], json!({ "kind": "end" }));
    }

    #[tokio::test]
    async fn a_429_passes_through_with_its_retry_after() {
        let body = r#"{"error":{"message":"rate limited"}}"#;
        let server = Server::start(vec![write(&format!(
            "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\n\
             retry-after: 7\r\nx-ratelimit-remaining: 0\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        ))])
        .await;
        let requests = ModelRequests::new();
        let (channel, seen, _) = recorder();
        within(run(
            &requests,
            outbound("a", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();

        let events = seen.lock().unwrap().clone();
        assert_eq!(events[0]["kind"], "head");
        assert_eq!(events[0]["status"], 429);
        let headers: Vec<(String, String)> =
            serde_json::from_value(events[0]["headers"].clone()).unwrap();
        for pair in [
            ("retry-after", "7"),
            ("content-type", "application/json"),
            ("x-ratelimit-remaining", "0"),
        ] {
            assert!(
                headers.contains(&(pair.0.to_string(), pair.1.to_string())),
                "{pair:?} missing from {headers:?}"
            );
        }
        assert_eq!(body_of(&events).concat(), body.as_bytes());
        assert_eq!(events.last().unwrap()["kind"], "end");
    }

    #[tokio::test]
    async fn a_redirect_is_passed_back_not_followed() {
        let elsewhere = Server::start(ok_json("{}")).await;
        let server = Server::start(vec![write(&format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://127.0.0.1:{}/chat/completions\r\n\
             content-length: 0\r\n\r\n",
            elsewhere.port
        ))])
        .await;
        let requests = ModelRequests::new();
        let (channel, seen, _) = recorder();
        within(run(
            &requests,
            outbound("a", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();
        let events = seen.lock().unwrap().clone();
        assert_eq!(events[0]["status"], 307);
        assert_eq!(elsewhere.accepts(), 0, "the redirect was followed");
    }

    #[tokio::test]
    async fn a_refused_connection_ends_in_failed_without_a_head() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let requests = ModelRequests::new();
        let (channel, seen, _) = recorder();
        within(run(
            &requests,
            outbound("a", Provider::DeepSeek, "/chat/completions"),
            move |_| format!("http://127.0.0.1:{port}"),
            keys(),
            &channel,
        ))
        .await
        .unwrap();
        let events = seen.lock().unwrap().clone();
        assert_eq!(kinds(&events), ["failed"]);
        assert!(!events[0]["message"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_stops_the_stream_and_ends_in_cancelled() {
        let mut script = chunked(&[b"data: one\n\n"]);
        script.truncate(2);
        script.push(Step::Hold);
        let mut server = Server::start(script).await;
        let requests = ModelRequests::new();
        let (channel, seen, mut rx) = recorder();

        let request = run(
            &requests,
            outbound("req-1", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        );
        let cancel = async {
            while within(rx.recv()).await.unwrap()["kind"] != "chunk" {}
            requests.cancel("req-1");
        };
        let (result, ()) = within(async { tokio::join!(request, cancel) }).await;
        result.unwrap();

        let events = seen.lock().unwrap().clone();
        assert_eq!(kinds(&events), ["head", "chunk", "cancelled"]);
        within(server.client_closed.recv())
            .await
            .expect("the connection was not closed");
    }

    #[tokio::test]
    async fn cancel_that_arrives_before_the_request_still_cancels_it() {
        let server = Server::start(ok_json("{}")).await;
        let requests = ModelRequests::new();
        requests.cancel("early");
        let (channel, seen, _) = recorder();
        within(run(
            &requests,
            outbound("early", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();
        assert_eq!(kinds(&seen.lock().unwrap()), ["cancelled"]);
        assert_eq!(server.accepts(), 0);

        let (channel, seen, _) = recorder();
        within(run(
            &requests,
            outbound("early", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();
        assert_eq!(
            kinds(&seen.lock().unwrap()).last().unwrap(),
            "end",
            "the early cancel lingered"
        );
    }

    #[tokio::test]
    async fn an_id_already_in_flight_is_refused() {
        let mut script = chunked(&[b"data: one\n\n"]);
        script.truncate(2);
        script.push(Step::Hold);
        let server = Server::start(script).await;
        let requests = ModelRequests::new();
        let (first_channel, _, mut rx) = recorder();
        let first = run(
            &requests,
            outbound("dup", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &first_channel,
        );
        let second = async {
            while within(rx.recv()).await.unwrap()["kind"] != "chunk" {}
            let (channel, seen, _) = recorder();
            let err = run(
                &requests,
                outbound("dup", Provider::DeepSeek, "/chat/completions"),
                server.base(),
                keys(),
                &channel,
            )
            .await
            .unwrap_err();
            assert!(seen.lock().unwrap().is_empty());
            requests.cancel("dup");
            err
        };
        let (first, err) = within(async { tokio::join!(first, second) }).await;
        first.unwrap();
        assert!(matches!(err, ModelRequestError::Refused { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_finished_request_leaves_nothing_in_flight() {
        let server = Server::start(ok_json("{}")).await;
        let requests = ModelRequests::new();
        let (channel, _, _) = recorder();
        within(run(
            &requests,
            outbound("done", Provider::DeepSeek, "/chat/completions"),
            server.base(),
            keys(),
            &channel,
        ))
        .await
        .unwrap();
        assert_eq!(requests.in_flight(), 0);
    }
}
