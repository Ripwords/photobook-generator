//! Pixajoy (Taopix Online Designer). Each send creates a new project and
//! fills it in the same editor session; an existing project is never
//! reopened. Nothing here orders or adds to a cart.

pub mod auth;
pub mod client;
pub mod plan;

use crate::providers::PrintJob;
use client::{Client, Session};
use std::sync::Arc;

pub const MY_PROJECTS_URL: &str = "https://www.pixajoy.com.my/dashboard/projects";

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

pub struct Sent {
    pub projectref: String,
    pub project_name: String,
    pub session: Arc<Session>,
}

pub async fn send(
    client: &Client,
    mbtk: &str,
    job: &PrintJob,
    mut progress: impl FnMut(Progress),
) -> Result<Sent, Failure> {
    let page_count = plan::page_count_for(job.page_count as usize)?;
    progress(Progress::Creating);
    let created = client.create_project(mbtk, &job.name, page_count).await?;
    let project_name = created.project_name.clone();
    let named = |message: String| Failure { message, project_name: Some(project_name.clone()) };

    let session = Arc::new(Session::open(client, &created).await.map_err(named)?);
    let mut layout = plan::parse_load(&session.load().await.map_err(named)?).map_err(named)?;
    for slice in layout.unloaded_slices.clone() {
        let boxes = plan::parse_page_data(&session.page_data(slice).await.map_err(named)?).map_err(named)?;
        if boxes.is_empty() {
            return Err(named(format!("Pixajoy sent no boxes for pages {}-{}.", slice * 10, slice * 10 + 9)));
        }
        layout.boxes.extend(boxes);
    }

    let images: Vec<_> = job
        .pages
        .iter()
        .flat_map(|p| p.boxes.iter().map(|b| b.image.clone()))
        .chain(job.cover.front.clone())
        .chain(job.cover.back.clone())
        .collect();
    progress(Progress::Uploading { done: 0, total: images.len() });
    let assets = client::upload_all(Arc::clone(&session), images, |done, total| {
        progress(Progress::Uploading { done, total })
    })
    .await
    .map_err(named)?;

    progress(Progress::LayingOut);
    let diff = plan::save_diff(&layout, job, page_count, &assets).map_err(named)?;
    session.save(&diff).await.map_err(named)?;

    Ok(Sent { projectref: created.projectref, project_name: created.project_name, session })
}

#[cfg(test)]
mod live {
    //! Talks to the real Pixajoy. Run once by hand:
    //! `PIXAJOY_TOKEN=... cargo test --lib providers::pixajoy::live -- --ignored --nocapture`
    use super::*;
    use crate::geometry::Rect;
    use crate::providers::{JobBox, JobCover, JobImage, JobPage, JobText};
    use crate::templates::Align;

    /// Non-ASCII for the UTF-16 length, and `<`/`>` because the styled text
    /// format uses them for its own tags.
    const TITLE: &str = "Kyoto, März <3 & >";
    use serde_json::Value;
    use std::path::PathBuf;

    fn fixture(name: &str, width: u32, height: u32) -> JobImage {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar/Fixtures").join(name);
        JobImage { path: path.canonicalize().unwrap(), width, height }
    }

    fn timestamp() -> String {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let (y, m, d) = crate::book::titles::civil(secs.div_euclid(86_400));
        let s = secs.rem_euclid(86_400);
        format!("{y}-{m:02}-{d:02} {:02}{:02}{:02}", s / 3600, s % 3600 / 60, s % 60)
    }

    #[tokio::test]
    #[ignore = "creates a real Pixajoy project; needs PIXAJOY_TOKEN"]
    async fn sends_a_small_book_to_pixajoy() {
        let token = std::env::var("PIXAJOY_TOKEN").expect("PIXAJOY_TOKEN");
        let quadrants = fixture("export-quadrants.jpg", 1200, 800);
        let landscape = fixture("landscape.jpg", 1200, 800);
        let large = fixture("large-no-thumbnail.jpg", 2000, 1333);
        let p3 = fixture("export-p3.jpg", 1200, 800);
        let full = Rect::new(0.0, 0.0, 1.0, 1.0);
        let job = PrintJob {
            name: format!("PhotobookGen test {}", timestamp()),
            page_count: 3,
            pages: vec![
                JobPage { number: 1, boxes: vec![JobBox { rect: full, image: quadrants.clone() }], texts: vec![] },
                JobPage {
                    number: 2,
                    boxes: vec![
                        JobBox { rect: Rect::new(0.05, 0.07, 0.43, 0.86), image: landscape.clone() },
                        JobBox { rect: Rect::new(0.52, 0.07, 0.43, 0.41), image: large.clone() },
                        JobBox { rect: Rect::new(0.52, 0.52, 0.43, 0.41), image: p3.clone() },
                    ],
                    texts: vec![JobText {
                        rect: Rect::new(0.05, 0.93, 0.9, 0.06),
                        text: TITLE.into(),
                        align: Align::Center,
                        point_size: 20.0,
                    }],
                },
                JobPage { number: 3, boxes: vec![JobBox { rect: Rect::new(0.1, 0.1, 0.8, 0.8), image: large }], texts: vec![] },
            ],
            cover: JobCover { front: Some(landscape), back: None },
        };

        let mut seen = Vec::new();
        let sent = send(&Client::new().unwrap(), &token, &job, |p| seen.push(p))
            .await
            .unwrap_or_else(|f| panic!("send failed: {} (project {:?})", f.message, f.project_name));
        println!("projectref {} name {}", sent.projectref, sent.project_name);
        assert_eq!(seen.last(), Some(&Progress::LayingOut));

        let mut after = plan::parse_load(&sent.session.load().await.unwrap()).unwrap();
        for slice in after.unloaded_slices.clone() {
            after.boxes.extend(plan::parse_page_data(&sent.session.page_data(slice).await.unwrap()).unwrap());
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
        let texts: Vec<&str> = after
            .boxes
            .iter()
            .filter(|b| b.page == 2 && b.fields["type"] == 1)
            .filter_map(|b| b.fields["styledtext"]["data"].as_str())
            .collect();
        println!("page 2 text after reload: {texts:?}");
        assert_eq!(texts.len(), 1);
        assert!(texts[0].ends_with(&format!("<..>{TITLE}")), "title came back as {:?}", texts[0]);
        let cover = pictures(100_000);
        let front = cover.iter().max_by(|a, b| a.left.total_cmp(&b.left)).unwrap();
        assert!(filled(front), "cover front empty");
        let page2: Vec<(f64, f64)> = pictures(2).iter().map(|b| (b.left / after.page_w - 2.0, b.width / after.page_w)).collect();
        println!("page 2 boxes (x, w) normalised: {page2:?}");
        println!("cover front magnification {}", front.fields["magnification"]);
        assert!(after.boxes.iter().all(|b| b.fields.get("ref").and_then(Value::as_i64).unwrap_or(0) > 0));
    }
}
