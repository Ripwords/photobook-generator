//! Print providers: send a finished book to a printer's own editor.
//!
//! A [`PrintJob`] is the provider-neutral description of what to send, built
//! purely from the book and its exported files ([`job::build_job`]). Each
//! provider turns a job into its own API calls. Pixajoy is the only one.

pub mod commands;
pub mod job;
pub mod pixajoy;

use crate::geometry::Rect;
use crate::templates::Align;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The provider registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Pixajoy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrintJob {
    pub name: String,
    /// Interior pages in the book, numbered from 1.
    pub page_count: u32,
    pub pages: Vec<JobPage>,
    pub cover: JobCover,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobPage {
    pub number: u32,
    /// Bottom of the stack first.
    pub boxes: Vec<JobBox>,
    /// Printed above every photo on the page.
    pub texts: Vec<JobText>,
}

/// A chapter title, as `book::titles::TitleBox` places it.
#[derive(Debug, Clone, PartialEq)]
pub struct JobText {
    /// Page-normalised, like `JobBox::rect`.
    pub rect: Rect,
    pub text: String,
    pub align: Align,
    pub point_size: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobBox {
    /// Page-normalised, so it fits the provider's page whatever its size.
    pub rect: Rect,
    pub photo: JobPhoto,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JobCover {
    pub front: Option<JobPhoto>,
    pub back: Option<JobPhoto>,
}

/// One placement of a photo. Providers get each photo once, whole, and show
/// `crop` of it through their own zoom and pan, so recropping a box never
/// uploads anything.
#[derive(Debug, Clone, PartialEq)]
pub struct JobPhoto {
    /// The source file's content hash: the upload cache's key.
    pub hash: String,
    /// The upright pixel size of the uploaded rendition.
    pub width: u32,
    pub height: u32,
    /// Normalised to the upright photo, as `Placement::crop` is.
    pub crop: Rect,
}

/// A photo's upload: the whole upright photo as an sRGB JPEG, uncropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Upload {
    pub hash: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// What a provider holds for one photo already uploaded to one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedAsset {
    pub asset_id: String,
    pub width: u32,
    pub height: u32,
}

/// Where a sent photo landed in the provider's project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SentSlot {
    Page { number: u32, rect: Rect },
    Cover { side: crate::geometry::CoverSide },
}

/// One photo box as it was sent: the base a later sync compares the book and
/// the provider's project against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentBox {
    /// The provider's id for the box.
    pub reference: i64,
    pub slot: SentSlot,
    pub hash: String,
    pub asset_id: String,
    pub crop: Rect,
}

/// One send of one book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRecord {
    pub provider: ProviderId,
    pub account: String,
    pub projectref: String,
    pub project_name: String,
    pub page_count: u32,
    pub boxes: Vec<SentBox>,
}
