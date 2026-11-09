//! The Tauri commands behind "Send to Pixajoy". Each photo is exported
//! whole, as JPEG, into a temporary directory removed when the send ends,
//! and only when Pixajoy does not already hold it from an earlier send.

use super::pixajoy::{self, auth, plan, Progress, MY_PROJECTS_URL};
use super::{job, CachedAsset, ProviderId, Upload};
use crate::commands::{book_photos, database_path, expected_photo_count, export_items, split_findings};
use crate::db::Db;
use crate::protocol::OutputFormat;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri::AppHandle;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SendEvent {
    Exporting { done: usize, total: usize },
    Creating,
    Uploading { done: usize, total: usize },
    LayingOut,
    Done { project_name: String, open_url: String },
    Failed { message: String, project_name: Option<String> },
}

impl From<Progress> for SendEvent {
    fn from(p: Progress) -> Self {
        match p {
            Progress::Creating => SendEvent::Creating,
            Progress::Uploading { done, total } => SendEvent::Uploading { done, total },
            Progress::LayingOut => SendEvent::LayingOut,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendPlan {
    pub book_pages: usize,
    pub page_count: u32,
}

/// The book's page count as Pixajoy will receive it, or why it cannot be sent.
pub fn send_plan(book: &crate::book::pace::Book) -> Result<SendPlan, String> {
    plan::check_size(&book.spec)?;
    Ok(SendPlan { book_pages: book.pages.len(), page_count: plan::page_count_for(book.pages.len())? })
}

#[tauri::command]
pub fn provider_status(provider: ProviderId) -> Result<auth::Status, String> {
    match provider {
        ProviderId::Pixajoy => auth::status(),
    }
}

#[tauri::command]
pub async fn provider_sign_in(app: AppHandle, provider: ProviderId) -> Result<auth::Status, String> {
    match provider {
        ProviderId::Pixajoy => auth::sign_in(&app).await,
    }
}

#[tauri::command]
pub fn provider_sign_out(provider: ProviderId) -> Result<(), String> {
    match provider {
        ProviderId::Pixajoy => auth::sign_out(),
    }
}

#[tauri::command]
pub fn provider_send_plan(app: AppHandle, project_id: i64, provider: ProviderId) -> Result<SendPlan, String> {
    let ProviderId::Pixajoy = provider;
    let db = Db::open(&database_path(&app)?).map_err(|e| e.to_string())?;
    let project = db
        .load_project(project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {project_id} no longer exists"))?;
    send_plan(&project.book)
}

/// Opens My Projects in the default browser. `open` rather than an opener
/// plugin: the app has none, and the URL is a constant.
#[tauri::command]
pub fn open_provider_projects(provider: ProviderId) -> Result<(), String> {
    let ProviderId::Pixajoy = provider;
    std::process::Command::new("open").arg(MY_PROJECTS_URL).status().map_err(|e| e.to_string()).and_then(|s| {
        if s.success() { Ok(()) } else { Err(format!("open exited with {s}")) }
    })
}

/// Removes the export directory on every exit path.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.0) {
            log::warn!("could not remove the send export directory: {err}");
        }
    }
}

fn emit(channel: &Channel<SendEvent>, event: SendEvent) {
    if let Err(err) = channel.send(event) {
        log::warn!("failed to send a provider event: {err}");
    }
}

/// Every failure arrives as a `failed` event; the command itself resolves.
#[tauri::command]
pub async fn send_to_provider(
    app: AppHandle,
    project_id: i64,
    provider: ProviderId,
    on_event: Channel<SendEvent>,
) -> Result<(), String> {
    let ProviderId::Pixajoy = provider;
    let event = match send(app, project_id, &on_event).await {
        Ok(project_name) => SendEvent::Done { project_name, open_url: MY_PROJECTS_URL.into() },
        Err(f) => SendEvent::Failed { message: f.message, project_name: f.project_name },
    };
    emit(&on_event, event);
    Ok(())
}

/// A book ready to send: its photos, which of them Pixajoy already has from
/// an earlier send, and the fresh exports of the rest.
struct Prepared {
    project_id: i64,
    name: String,
    book: crate::book::pace::Book,
    photos: Vec<crate::book::cull::Photo>,
    cached: HashMap<String, CachedAsset>,
    fresh: Vec<Upload>,
}

async fn send(app: AppHandle, project_id: i64, on_event: &Channel<SendEvent>) -> Result<String, pixajoy::Failure> {
    let mbtk = auth::token()?;
    let account = auth::account_of(&mbtk)?;
    let dir = TempDir(std::env::temp_dir().join(format!("photobookgen-send-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&dir.0).map_err(|e| e.to_string())?;
    let out = dir.0.to_string_lossy().into_owned();

    let prepared = {
        let (app, out, account, channel) = (app.clone(), out.clone(), account.clone(), on_event.clone());
        blocking(move || prepare(&app, project_id, &out, &account, |e| emit(&channel, e))).await?
    };

    let client = pixajoy::client::Client::new()?;
    emit(on_event, SendEvent::Creating);
    let opened = pixajoy::open(&client, &mbtk, &prepared.name, prepared.book.pages.len() as u32).await?;

    // Pixajoy may have deleted an earlier upload; those photos go up again.
    let cached_ids: Vec<String> = prepared.cached.values().map(|a| a.asset_id.clone()).collect();
    let live = opened.reuse(&cached_ids).await;
    let (cached, stale): (HashMap<_, _>, HashMap<_, _>) =
        prepared.cached.into_iter().partition(|(_, a)| live.contains(&a.asset_id));
    let mut uploads = prepared.fresh;
    if !stale.is_empty() {
        let stale_ids: Vec<String> = stale.values().map(|a| a.asset_id.clone()).collect();
        let hashes: Vec<String> = stale.into_keys().collect();
        let photos = prepared.photos.clone();
        let (app, out, account, channel) = (app.clone(), out.clone(), account.clone(), on_event.clone());
        let again = blocking(move || {
            open_db(&app)?
                .forget_provider_assets(ProviderId::Pixajoy, &account, &stale_ids)
                .map_err(|e| e.to_string())?;
            export_uploads(&app, &photos, &hashes, &out, |e| emit(&channel, e))
        })
        .await
        .map_err(|e| opened.fail(e))?;
        uploads.extend(again);
    }

    let mut sizes: HashMap<String, (u32, u32)> =
        cached.iter().map(|(h, a)| (h.clone(), (a.width, a.height))).collect();
    sizes.extend(uploads.iter().map(|u| (u.hash.clone(), (u.width, u.height))));
    let new_assets: Vec<(String, CachedAsset)> = {
        let dims: HashMap<String, (u32, u32)> = uploads.iter().map(|u| (u.hash.clone(), (u.width, u.height))).collect();
        let ids = opened.upload(uploads, |p| emit(on_event, p.into())).await?;
        ids.into_iter()
            .map(|(hash, asset_id)| {
                let (width, height) = dims[&hash];
                (hash, CachedAsset { asset_id, width, height })
            })
            .collect()
    };
    let mut assets: HashMap<String, String> = cached.into_iter().map(|(h, a)| (h, a.asset_id)).collect();
    assets.extend(new_assets.iter().map(|(h, a)| (h.clone(), a.asset_id.clone())));

    emit(on_event, SendEvent::LayingOut);
    let job = job::build_job(&prepared.name, &prepared.book, &prepared.photos, &sizes)
        .map_err(|e| opened.fail(e))?;
    let record = opened.lay_out(&job, &account, &assets).await?;

    // Recorded after the save: a failed send leaves no record, and its
    // uploads stay cached because Pixajoy keeps them in the account.
    let project_name = record.project_name.clone();
    let id = prepared.project_id;
    blocking(move || {
        let db = open_db(&app)?;
        for (hash, asset) in &new_assets {
            db.put_provider_asset(ProviderId::Pixajoy, &account, pixajoy::RENDITION, hash, asset)
                .map_err(|e| e.to_string())?;
        }
        db.record_send(id, &record).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| opened.fail(format!("The book was sent, but recording the send failed: {e}")))?;
    Ok(project_name)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn open_db(app: &AppHandle) -> Result<Db, String> {
    Db::open(&database_path(app)?).map_err(|e| e.to_string())
}

/// Checks the book can be sent, then exports every photo Pixajoy does not
/// already have from an earlier send to this account.
fn prepare(
    app: &AppHandle,
    project_id: i64,
    out: &str,
    account: &str,
    emit: impl Fn(SendEvent),
) -> Result<Prepared, String> {
    let db = open_db(app)?;
    let project = db
        .load_project(project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {project_id} no longer exists"))?;
    send_plan(&project.book)?;
    let photos = book_photos(&db, &project)?;
    let expected = expected_photo_count(&project.book);
    if photos.len() != expected {
        return Err(format!(
            "this book was assembled from {expected} photos but {} were found; \
             re-analyse the source folder and generate the book again",
            photos.len()
        ));
    }
    let (blocking, _) = split_findings(crate::book::preflight::preflight(&project.book, &photos, Path::new(out)));
    if let Some(first) = blocking.first() {
        return Err(format!("The book cannot be sent: {}", first.message));
    }

    let hashes = job::book_hashes(&project.book, &photos);
    let cached = db
        .provider_assets(ProviderId::Pixajoy, account, pixajoy::RENDITION, &hashes)
        .map_err(|e| e.to_string())?;
    let missing: Vec<String> = hashes.into_iter().filter(|h| !cached.contains_key(h)).collect();
    let fresh = export_uploads(app, &photos, &missing, out, &emit)?;
    Ok(Prepared { project_id, name: project.name, book: project.book, photos, cached, fresh })
}

fn export_uploads(
    app: &AppHandle,
    photos: &[crate::book::cull::Photo],
    hashes: &[String],
    out: &str,
    emit: impl Fn(SendEvent),
) -> Result<Vec<Upload>, String> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let items = job::upload_items(photos, hashes)?;
    emit(SendEvent::Exporting { done: 0, total: items.len() });
    let records = export_items(app, out, OutputFormat::Jpeg, &items, |done, total| {
        emit(SendEvent::Exporting { done, total })
    });
    job::uploads(&records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn send_events_match_the_ui_contract() {
        let cases = [
            (SendEvent::Exporting { done: 1, total: 4 }, json!({"kind":"exporting","done":1,"total":4})),
            (SendEvent::Creating, json!({"kind":"creating"})),
            (SendEvent::Uploading { done: 2, total: 3 }, json!({"kind":"uploading","done":2,"total":3})),
            (SendEvent::LayingOut, json!({"kind":"layingOut"})),
            (
                SendEvent::Done { project_name: "Trip".into(), open_url: "u".into() },
                json!({"kind":"done","projectName":"Trip","openUrl":"u"}),
            ),
            (
                SendEvent::Failed { message: "m".into(), project_name: None },
                json!({"kind":"failed","message":"m","projectName":null}),
            ),
        ];
        for (event, wire) in cases {
            assert_eq!(serde_json::to_value(&event).unwrap(), wire);
        }
    }

    #[test]
    fn the_send_plan_is_camel_case() {
        let wire = serde_json::to_value(SendPlan { book_pages: 7, page_count: 20 }).unwrap();
        assert_eq!(wire, json!({"bookPages":7,"pageCount":20}));
    }
}
