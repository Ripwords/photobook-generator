//! The Tauri commands behind "Send to Pixajoy". The export reuses
//! `run_export` with JPEG output into a temporary directory that is removed
//! when the send ends, whatever the outcome.

use super::pixajoy::{self, auth, plan, Progress, MY_PROJECTS_URL};
use super::{job, PrintJob, ProviderId};
use crate::commands::{
    book_chapters, book_library, book_photos, database_path, export_items, run_export, write_manifest_file,
};
use crate::db::Db;
use crate::protocol::OutputFormat;
use serde::Serialize;
use std::path::PathBuf;
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

async fn send(app: AppHandle, project_id: i64, on_event: &Channel<SendEvent>) -> Result<String, pixajoy::Failure> {
    let mbtk = auth::token()?;
    let dir = TempDir(std::env::temp_dir().join(format!("photobookgen-send-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&dir.0).map_err(|e| e.to_string())?;

    let out = dir.0.clone();
    let channel = on_event.clone();
    let job = tauri::async_runtime::spawn_blocking(move || {
        export_job(&app, project_id, &out, |event| emit(&channel, event))
    })
    .await
    .map_err(|e| e.to_string())??;

    let client = pixajoy::client::Client::new()?;
    let sent = pixajoy::send(&client, &mbtk, &job, |p| emit(on_event, p.into())).await?;
    Ok(sent.project_name)
}

fn export_job(
    app: &AppHandle,
    project_id: i64,
    out: &std::path::Path,
    emit: impl Fn(SendEvent),
) -> Result<PrintJob, String> {
    let db = Db::open(&database_path(app)?).map_err(|e| e.to_string())?;
    let project = db
        .load_project(project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {project_id} no longer exists"))?;
    send_plan(&project.book)?;
    let photos = book_photos(&db, &project)?;
    let findings = crate::book::preflight::preflight(&project.book, &photos, out);
    let output_dir = out.to_string_lossy().into_owned();

    let mut records = Vec::new();
    let result = run_export(
        &project.book,
        &photos,
        project_id,
        &output_dir,
        findings,
        |items| {
            emit(SendEvent::Exporting { done: 0, total: items.len() });
            records = export_items(app, &output_dir, OutputFormat::Jpeg, items, |done, total| {
                emit(SendEvent::Exporting { done, total })
            });
            records.clone()
        },
        |manifest| write_manifest_file(out, manifest),
    )?;
    if result.blocked {
        let first = result.blocking.first().map(|f| f.message.as_str()).unwrap_or("pre-flight failed");
        return Err(format!("The book cannot be exported: {first}"));
    }
    // The same chapters, with the same cached place names, the editor shows.
    let chapters = book_chapters(&db, &project.book, &photos, &book_library(app, &db, &project.book.spec)?);
    job::build_job(&project.name, &project.book, &photos, &job::exported_files(&records)?, &chapters)
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
