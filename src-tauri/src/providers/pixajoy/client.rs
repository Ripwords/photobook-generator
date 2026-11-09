//! Pixajoy's HTTP calls, as recorded in `docs/providers/pixajoy.md`.
//!
//! Tokens and signed URLs never reach an error message or the log: every
//! reqwest error is stripped of its URL, and no request is logged.

use crate::providers::JobImage;
use base64::Engine;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::task::JoinSet;

/// The cart refuses a client without a browser User-Agent ("Your browser is
/// not supported").
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const SERVICE: &str = "https://service.pixajoy.com/api";
const CART: &str = "https://cart.pixajoy.com.my/";
const EDITOR: &str = "https://editor.pixajoy.com.my/api";
const IMAGES: &str = "https://images.pixajoy.com.my";
/// The device-detection string Pixajoy's website sends.
const DD: &str = "v1s19.2o154.28571428571428o384o210o3d0o0o1o771.4857142857143";
/// Pixajoy's brand key, from the editor's public `config.js`.
const PUBKEY: &str =
    "LgHOBjd/M9VL2eKwvcyYZV6SRrdQEEXTfIFibg4O4nLYHqZxARwC3EV49niR0XD+fWO9hoWENeqbef+1uC+Ppw==";
const BRAND: &str = "PIXAJOY";
/// "Photo Book Blank, Fit to Page", Imagewrap Hardcover 8.5" × 11" Landscape.
const COLLECTION: &str = "HNS_BLK02";
const PRODUCT: &str = "IHL8511";
/// Uploads in flight at once.
pub const UPLOAD_CONCURRENCY: usize = 4;
const STATUS_POLLS: u32 = 60;
const PROCESSED: i64 = 6;
const PROCESS_FAILED: [i64; 2] = [7, 8];

/// The step, the error and its causes ("Connection reset by peer" lives in
/// a source), without the URL, whose query can carry session ids.
fn net(what: &str) -> impl Fn(reqwest::Error) -> String + '_ {
    move |e| {
        let e = e.without_url();
        let mut message = format!("Could not reach Pixajoy ({what}): {e}");
        let mut cause = std::error::Error::source(&e);
        while let Some(c) = cause {
            message.push_str(&format!(": {c}"));
            cause = c.source();
        }
        message
    }
}

async fn checked(response: Response, what: &str) -> Result<Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let body: String = body.chars().take(200).collect();
    Err(format!("Pixajoy's {what} call failed ({status}): {body}"))
}

async fn json_of<T: DeserializeOwned>(response: Response, what: &str) -> Result<T, String> {
    let text = checked(response, what).await?.text().await.map_err(net(what))?;
    serde_json::from_str(&text).map_err(|e| format!("Pixajoy's {what} reply was not understood: {e}"))
}

pub struct Client {
    http: reqwest::Client,
}

/// A project `createProject2` made. Not `Debug`: `designurl` opens the
/// project's one editor session.
pub struct Created {
    pub projectref: String,
    pub project_name: String,
    designurl: String,
}

#[derive(Deserialize)]
struct CreateReply {
    result: i64,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    projectref: String,
    #[serde(default)]
    projectname: String,
    #[serde(default)]
    collectionname: String,
    #[serde(default)]
    layoutname: String,
    #[serde(default)]
    designurl: String,
}

impl Client {
    pub fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            // service.pixajoy.com speaks only TLS 1.2 with CBC suites, which rustls does not offer.
            .tls_backend_native()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http })
    }

    /// Creates the project with `pages` interior pages and registers it in
    /// the user's My Projects.
    pub async fn create_project(&self, mbtk: &str, name: &str, pages: u32) -> Result<Created, String> {
        #[derive(Deserialize)]
        struct Link {
            link: String,
        }
        let response = self
            .http
            .get(format!("{SERVICE}/token/GetTaopixCartLinkP"))
            .bearer_auth(mbtk)
            .send()
            .await
            .map_err(net("cart link"))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err("Your Pixajoy sign-in has expired. Sign in again.".into());
        }
        let link: Link = json_of(response, "cart link").await?;

        let url = format!(
            "{CART}?fsaction=OnlineAPI.createProject2&dd={DD}&{}&cpexpo=WIZARD:NDA|EDITOR:4QNEPKQEVVXF",
            link.link
        );
        let pages = format!("defaultpages={pages}");
        let form = [
            ("brandcode", BRAND),
            ("groupcode", BRAND),
            ("collectioncode", COLLECTION),
            ("productcode", PRODUCT),
            ("pagestyle", ""),
            ("projectname", name),
            ("txtfield", pages.as_str()),
        ];
        let response = self.http.post(url).form(&form).send().await.map_err(net("create project"))?;
        let created: CreateReply = json_of(response, "create project").await?;
        if created.result != 0 {
            return Err(format!(
                "Pixajoy did not create the project (result {}: {}).",
                created.result,
                created.message.unwrap_or_default()
            ));
        }

        let insert = json!({
            "webbrandcode": BRAND,
            "groupcode": BRAND,
            "collectioncode": COLLECTION,
            "collectionname": created.collectionname,
            "productcode": PRODUCT,
            "productname": created.layoutname,
            "projectref": created.projectref,
            "projectname": created.projectname,
            "thumbnail": "",
            "componentlist": "",
            "salesmethod": 1,
        });
        let response = self
            .http
            .post(format!("{SERVICE}/project/insert"))
            .bearer_auth(mbtk)
            .json(&insert)
            .send()
            .await
            .map_err(net("register project"))?;
        checked(response, "register project").await?;

        Ok(Created {
            projectref: created.projectref,
            project_name: created.projectname,
            designurl: created.designurl,
        })
    }
}

/// The one-time session ref inside a `designurl`: its `ref` query parameter
/// is base64 of `?ref=<session ref>&theme=…`.
pub fn session_ref(designurl: &str) -> Result<String, String> {
    let bad = || "Pixajoy's editor link was not understood.".to_string();
    let url = reqwest::Url::parse(designurl).map_err(|_| bad())?;
    let outer = url.query_pairs().find(|(k, _)| k == "ref").ok_or_else(bad)?.1.into_owned();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(outer.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(outer.as_bytes()))
        .map_err(|_| bad())?;
    let inner = String::from_utf8(decoded).map_err(|_| bad())?;
    let inner = reqwest::Url::parse(&format!("https://x/{inner}")).map_err(|_| bad())?;
    let session = inner.query_pairs().find(|(k, _)| k == "ref").ok_or_else(bad)?.1.into_owned();
    Ok(session)
}

#[derive(Deserialize)]
struct Tokens {
    token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
}

/// One editor session on one project. The session ref authenticates once,
/// so every load, upload and save happens through this value.
pub struct Session {
    http: reqwest::Client,
    tokens: Mutex<Tokens>,
}

impl Session {
    pub async fn open(client: &Client, created: &Created) -> Result<Self, String> {
        let form = reqwest::multipart::Form::new()
            .text("pubkey", PUBKEY)
            .text("ref", session_ref(&created.designurl)?)
            .text("designerId", uuid::Uuid::new_v4().to_string());
        let response = client
            .http
            .post(format!("{EDITOR}/authenticate"))
            .multipart(form)
            .send()
            .await
            .map_err(net("editor sign-in"))?;
        let tokens: Tokens = json_of(response, "editor sign-in").await?;
        let session = Self { http: client.http.clone(), tokens: Mutex::new(tokens) };
        session.get("system/initialise").await?;
        session.get("project/initialise").await?;
        Ok(session)
    }

    fn token(&self) -> String {
        self.tokens.lock().expect("token lock").token.clone()
    }

    async fn reauthenticate(&self) -> Result<(), String> {
        let refresh = self.tokens.lock().expect("token lock").refresh_token.clone();
        let response = self
            .http
            .post(format!("{EDITOR}/reauthenticate"))
            .bearer_auth(refresh)
            .json(&json!({}))
            .send()
            .await
            .map_err(net("editor refresh"))?;
        let tokens: Tokens = json_of(response, "editor refresh").await?;
        *self.tokens.lock().expect("token lock") = tokens;
        Ok(())
    }

    /// Sends, and on a 401 refreshes the editor token once and resends.
    async fn call(&self, path: &str, build: impl Fn(&str) -> RequestBuilder) -> Result<Value, String> {
        let response = build(&self.token()).send().await.map_err(net(path))?;
        let response = if response.status() == StatusCode::UNAUTHORIZED {
            self.reauthenticate().await?;
            build(&self.token()).send().await.map_err(net(path))?
        } else {
            response
        };
        json_of(response, path).await
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        let url = format!("{EDITOR}/{path}");
        self.call(path, |t| self.http.get(&url).bearer_auth(t).header("accept", "application/json"))
            .await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let url = format!("{EDITOR}/{path}");
        self.call(path, |t| {
            self.http.post(&url).bearer_auth(t).header("accept", "application/json").json(body)
        })
        .await
    }

    pub async fn load(&self) -> Result<Value, String> {
        self.get("project/load").await
    }

    pub async fn page_data(&self, slice: u32) -> Result<Value, String> {
        self.get(&format!("project/pageData/{slice}")).await
    }

    pub async fn save(&self, diff: &Value) -> Result<Value, String> {
        self.post("project/save", diff).await
    }

    /// Uploads one file and waits for Pixajoy to process it. Returns the
    /// asset id a box's `hirespath` names.
    pub async fn upload(&self, image: &JobImage) -> Result<String, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Asset {
            id: Value,
            volume_id: Value,
            file_name: String,
            folder_path: String,
            signature: String,
        }
        let bytes = tokio::fs::read(&image.path).await.map_err(|e| e.to_string())?;
        let size = bytes.len();
        let name = image.path.file_name().and_then(|n| n.to_str()).unwrap_or("photo.jpg");
        let created = self
            .post(
                "assets/create",
                &json!({ "assets": [{
                    "fileSizeBytes": size,
                    "name": name,
                    "fileType": 1,
                    "uploadid": upload_id(),
                    "width": image.width,
                    "height": image.height,
                    "batchRef": upload_id()[..15],
                    "colourComponent": 0,
                    "colourMode": 3,
                    "fileDateModified": iso_now(),
                    "rotation": 0,
                    "assetType": 106,
                    "metaData": { "dateTimeOriginal": "0000-00-00 00:00:00", "latitude": "", "longitude": "" },
                    "status": 0,
                    "aithumb": null,
                    "thumbnail": false,
                }] }),
            )
            .await?;
        let mut assets: Vec<Asset> = serde_json::from_value(created)
            .map_err(|e| format!("Pixajoy's assets/create reply was not understood: {e}"))?;
        let asset = assets.pop().ok_or("Pixajoy created no asset for the upload.")?;

        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(asset.file_name.clone())
            .mime_str("image/jpeg")
            .map_err(|e| e.to_string())?;
        let started = Instant::now();
        let response = self
            .http
            .post(format!("{IMAGES}{}/{}{}", asset.folder_path, asset.file_name, asset.signature))
            .multipart(reqwest::multipart::Form::new().part("asset", part))
            .send()
            .await
            .map_err(net("upload"))?;
        checked(response, "photo upload").await?;

        self.post(
            "assets/queue",
            &json!({
                "asset": {
                    "assetId": asset.id,
                    "jobType": "1",
                    "uploadDuration": started.elapsed().as_secs_f64(),
                    "fileSizeBytes": size,
                    "vid": asset.volume_id,
                    "volumeType": 1,
                    "assetType": 106,
                    "scaled": false,
                },
                "queue": "process",
            }),
        )
        .await?;

        for _ in 0..STATUS_POLLS {
            let status = self.post("assets/status", &json!({ "assets": [asset.id] })).await?;
            match status["assets"][0]["status"].as_i64() {
                Some(PROCESSED) => return Ok(id_string(&asset.id)),
                Some(s) if PROCESS_FAILED.contains(&s) => {
                    return Err(format!("Pixajoy could not process {name} (status {s})."))
                }
                _ => tokio::time::sleep(Duration::from_secs(1)).await,
            }
        }
        Err(format!("Pixajoy took too long to process {name}."))
    }
}

/// Uploads each distinct image once, `UPLOAD_CONCURRENCY` at a time, and
/// returns the asset id per path. `on_done(done, total)` follows each upload.
pub async fn upload_all(
    session: Arc<Session>,
    images: Vec<JobImage>,
    mut on_done: impl FnMut(usize, usize),
) -> Result<HashMap<PathBuf, String>, String> {
    let mut unique: Vec<JobImage> = Vec::new();
    for image in images {
        if !unique.iter().any(|u| u.path == image.path) {
            unique.push(image);
        }
    }
    let total = unique.len();
    let mut queue = unique.into_iter();
    let mut running = JoinSet::new();
    let mut ids = HashMap::with_capacity(total);
    loop {
        while running.len() < UPLOAD_CONCURRENCY {
            let Some(image) = queue.next() else { break };
            let session = Arc::clone(&session);
            running.spawn(async move {
                let id = session.upload(&image).await?;
                Ok::<_, String>((image.path, id))
            });
        }
        let Some(joined) = running.join_next().await else { break };
        let (path, id) = joined.map_err(|e| e.to_string())??;
        ids.insert(path, id);
        on_done(ids.len(), total);
    }
    Ok(ids)
}

fn id_string(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Any 21 characters, per the API.
fn upload_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..21].to_string()
}

fn iso_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, d) = crate::book::titles::civil(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", s / 3600, s % 3600 / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_ref_is_read_from_inside_the_base64_ref() {
        let inner = base64::engine::general_purpose::STANDARD.encode("?ref=SESSION123&theme=pixajoy");
        let url = format!("https://editor.pixajoy.com.my/ui/pixajoy/?ref={}&l=en", urlencode(&inner));
        assert_eq!(session_ref(&url).unwrap(), "SESSION123");
    }

    #[test]
    fn a_designurl_without_a_ref_is_an_error() {
        assert!(session_ref("https://editor.pixajoy.com.my/ui/pixajoy/").is_err());
    }

    #[test]
    fn upload_ids_are_21_characters() {
        assert_eq!(upload_id().len(), 21);
    }

    fn urlencode(s: &str) -> String {
        s.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D")
    }
}
