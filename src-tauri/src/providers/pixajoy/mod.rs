//! Pixajoy (Taopix Online Designer). Each send creates a new project and
//! fills it in the same editor session; an existing project is never
//! reopened. Nothing here orders or adds to a cart.
//!
//! A send runs in phases so the caller can reuse earlier uploads: [`open`]
//! creates the project and its session, [`Opened::reuse`] adds earlier uploads
//! Pixajoy still has, [`Opened::upload`] sends the rest, and
//! [`Opened::lay_out`] saves the layout and returns the send record.

pub mod auth;
pub mod client;
pub mod plan;

use crate::providers::{PrintJob, ProviderId, SendRecord, Upload};
use client::{Client, Session};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const MY_PROJECTS_URL: &str = "https://www.pixajoy.com.my/dashboard/projects";
/// What an upload is: the whole photo, upright, sRGB JPEG. Bump it when
/// that changes, so cached asset ids of the old rendition are not reused.
pub const RENDITION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    Creating,
    Uploading { done: usize, total: usize },
    LayingOut,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub message: String,
    /// Set once the project exists, so the user knows a partly filled one
    /// may be in My Projects.
    pub project_name: Option<String>,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, project_name: None }
    }
}

/// A new project with its one editor session open and every box loaded.
pub struct Opened {
    pub projectref: String,
    pub project_name: String,
    pub session: Arc<Session>,
    layout: plan::Layout,
    page_count: u32,
}

pub async fn open(client: &Client, mbtk: &str, name: &str, book_pages: u32) -> Result<Opened, Failure> {
    let page_count = plan::page_count_for(book_pages as usize)?;
    let created = client.create_project(mbtk, name, page_count).await?;
    let named = |message: String| Failure { message, project_name: Some(created.project_name.clone()) };

    let session = Arc::new(Session::open(client, &created).await.map_err(named)?);
    let mut layout = plan::parse_load(&session.load().await.map_err(named)?).map_err(named)?;
    for slice in layout.unloaded_slices.clone() {
        let boxes = plan::parse_page_data(&session.page_data(slice).await.map_err(named)?).map_err(named)?;
        if boxes.is_empty() {
            return Err(named(format!("Pixajoy sent no boxes for pages {}-{}.", slice * 10, slice * 10 + 9)));
        }
        layout.boxes.extend(boxes);
    }
    Ok(Opened { projectref: created.projectref, project_name: created.project_name, session, layout, page_count })
}

impl Opened {
    pub fn fail(&self, message: String) -> Failure {
        Failure { message, project_name: Some(self.project_name.clone()) }
    }

    /// Adds earlier uploads to this project; returns the ids still alive.
    pub async fn reuse(&self, asset_ids: &[String]) -> HashSet<String> {
        self.session.reuse_assets(asset_ids).await
    }

    /// Uploads each file and returns the asset id per photo hash.
    pub async fn upload(
        &self,
        uploads: Vec<Upload>,
        mut progress: impl FnMut(Progress),
    ) -> Result<HashMap<String, String>, Failure> {
        progress(Progress::Uploading { done: 0, total: uploads.len() });
        client::upload_all(Arc::clone(&self.session), uploads, |done, total| {
            progress(Progress::Uploading { done, total })
        })
        .await
        .map_err(|e| self.fail(e))
    }

    /// Saves the layout. `assets` maps every photo hash the job shows to its
    /// asset id. The record names each photo box by the ref Pixajoy gave it.
    pub async fn lay_out(
        &self,
        job: &PrintJob,
        account: &str,
        assets: &HashMap<String, String>,
    ) -> Result<SendRecord, Failure> {
        let diff = plan::save_diff(&self.layout, job, self.page_count, assets).map_err(|e| self.fail(e))?;
        let reply = self.session.save(&diff.body).await.map_err(|e| self.fail(e))?;
        let boxes = plan::resolve_refs(diff.boxes, &reply).map_err(|e| self.fail(e))?;
        Ok(SendRecord {
            provider: ProviderId::Pixajoy,
            account: account.to_string(),
            projectref: self.projectref.clone(),
            project_name: self.project_name.clone(),
            page_count: self.page_count,
            boxes,
        })
    }
}

#[cfg(test)]
mod live {
    //! Talks to the real Pixajoy. Run once by hand:
    //! `PIXAJOY_TOKEN=... cargo test --lib providers::pixajoy::live -- --ignored --nocapture`
    use super::*;
    use crate::geometry::Rect;
    use crate::providers::{JobBox, JobCover, JobPage, JobPhoto, SentSlot};
    use serde_json::Value;
    use std::path::PathBuf;

    const FULL: Rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

    fn fixture(name: &str, width: u32, height: u32) -> Upload {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar/Fixtures").join(name);
        Upload { hash: name.into(), path: path.canonicalize().unwrap(), width, height }
    }

    fn photo(u: &Upload, crop: Rect) -> JobPhoto {
        JobPhoto { hash: u.hash.clone(), width: u.width, height: u.height, crop }
    }

    fn timestamp() -> String {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let (y, m, d) = crate::book::chapter_pages::civil(secs.div_euclid(86_400));
        let s = secs.rem_euclid(86_400);
        format!("{y}-{m:02}-{d:02} {:02}{:02}{:02}", s / 3600, s % 3600 / 60, s % 60)
    }

    /// Uploads only what `cache` lacks or Pixajoy no longer has, the way the
    /// app's send does, and returns how many files went up.
    async fn send(
        client: &Client,
        token: &str,
        job: &PrintJob,
        uploads: &[Upload],
        cache: &mut HashMap<String, String>,
    ) -> (Opened, SendRecord, usize) {
        let opened = open(client, token, &job.name, job.page_count)
            .await
            .unwrap_or_else(|f| panic!("open failed: {} (project {:?})", f.message, f.project_name));
        let cached: Vec<String> = cache.values().cloned().collect();
        let live = opened.reuse(&cached).await;
        cache.retain(|_, id| live.contains(id));
        let missing: Vec<Upload> = uploads.iter().filter(|u| !cache.contains_key(&u.hash)).cloned().collect();
        let sent = missing.len();
        cache.extend(opened.upload(missing, |_| {}).await.unwrap_or_else(|f| panic!("upload failed: {}", f.message)));
        let record = opened
            .lay_out(job, &auth::account_of(token).unwrap(), cache)
            .await
            .unwrap_or_else(|f| panic!("lay out failed: {} (project {:?})", f.message, f.project_name));
        (opened, record, sent)
    }

    #[tokio::test]
    #[ignore = "creates two real Pixajoy projects; needs PIXAJOY_TOKEN"]
    async fn sends_a_small_book_and_resends_it_without_uploading_again() {
        let token = std::env::var("PIXAJOY_TOKEN").expect("PIXAJOY_TOKEN");
        let quadrants = fixture("export-quadrants.jpg", 1200, 800);
        let landscape = fixture("landscape.jpg", 1200, 800);
        let large = fixture("large-no-thumbnail.jpg", 2000, 1333);
        let p3 = fixture("export-p3.jpg", 1200, 800);
        let job = |name: String| PrintJob {
            name,
            page_count: 3,
            pages: vec![
                JobPage {
                    number: 1,
                    // The top-left quadrant only: the editor should show one colour.
                    boxes: vec![JobBox { rect: FULL, photo: photo(&quadrants, Rect::new(0.0, 0.0, 0.5, 0.5)) }],
                },
                JobPage {
                    number: 2,
                    boxes: vec![
                        JobBox { rect: Rect::new(0.05, 0.07, 0.43, 0.86), photo: photo(&landscape, FULL) },
                        JobBox { rect: Rect::new(0.52, 0.07, 0.43, 0.41), photo: photo(&large, FULL) },
                        JobBox { rect: Rect::new(0.52, 0.52, 0.43, 0.41), photo: photo(&p3, FULL) },
                    ],
                },
                JobPage {
                    number: 3,
                    boxes: vec![JobBox { rect: Rect::new(0.1, 0.1, 0.8, 0.8), photo: photo(&large, FULL) }],
                },
            ],
            cover: JobCover { front: Some(photo(&landscape, FULL)), back: None },
        };
        let uploads = [quadrants.clone(), landscape.clone(), large.clone(), p3.clone()];
        let client = Client::new().unwrap();
        let mut cache = HashMap::new();

        let (opened, record, sent) =
            send(&client, &token, &job(format!("PhotobookGen test {}", timestamp())), &uploads, &mut cache).await;
        println!("projectref {} name {}", opened.projectref, opened.project_name);
        assert_eq!(sent, 4);
        assert_eq!(record.boxes.len(), 6, "five page boxes and the cover front");
        assert!(record.boxes.iter().all(|b| b.reference > 0));
        assert!(record.boxes.iter().any(|b| b.slot == SentSlot::Cover { side: crate::geometry::CoverSide::Front }));

        let mut after = plan::parse_load(&opened.session.load().await.unwrap()).unwrap();
        for slice in after.unloaded_slices.clone() {
            after.boxes.extend(plan::parse_page_data(&opened.session.page_data(slice).await.unwrap()).unwrap());
        }
        let pictures = |page: u32| -> Vec<&plan::LoadBox> {
            after.boxes.iter().filter(|b| b.picture && b.page == page).collect()
        };
        let filled = |b: &&plan::LoadBox| !b.fields["hirespath"].as_str().unwrap_or("").is_empty();
        assert_eq!(pictures(1).len(), 1);
        assert_eq!(pictures(2).len(), 3);
        assert_eq!(pictures(3).len(), 1);
        for page in 1..=3 {
            assert!(pictures(page).iter().all(filled), "page {page} has an empty box");
        }
        for page in 4..=20 {
            assert!(pictures(page).is_empty(), "padding page {page} kept a box");
        }
        let refs: HashSet<i64> = after.boxes.iter().filter(|b| b.picture).map(|b| b.reference).collect();
        assert!(record.boxes.iter().all(|b| refs.contains(&b.reference)), "a recorded ref is not in the project");
        let page1 = pictures(1)[0];
        println!(
            "page 1 quadrant crop: magnification {} scroll ({}, {})",
            page1.fields["magnification"], page1.fields["boxscrollleft"], page1.fields["boxscrolltop"]
        );
        assert!(after.boxes.iter().all(|b| b.fields.get("ref").and_then(Value::as_i64).unwrap_or(0) > 0));

        let (again, record2, sent2) =
            send(&client, &token, &job(format!("PhotobookGen test {} reuse", timestamp())), &uploads, &mut cache).await;
        println!("second projectref {} name {}", again.projectref, again.project_name);
        assert_eq!(sent2, 0, "the second send reused every upload");
        assert!(again.reuse(&["1".into()]).await.is_empty(), "an asset that is not the account's is refused");
        let mut reloaded = plan::parse_load(&again.session.load().await.unwrap()).unwrap();
        for slice in reloaded.unloaded_slices.clone() {
            reloaded.boxes.extend(plan::parse_page_data(&again.session.page_data(slice).await.unwrap()).unwrap());
        }
        let ids: HashSet<&str> = reloaded.boxes.iter().filter_map(|b| b.fields["hirespath"].as_str()).collect();
        assert!(record2.boxes.iter().all(|b| ids.contains(b.asset_id.as_str())), "a reused asset did not stick");
    }
}
