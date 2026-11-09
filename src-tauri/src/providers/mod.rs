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
    pub image: JobImage,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JobCover {
    pub front: Option<JobImage>,
    pub back: Option<JobImage>,
}

/// An exported file: already cropped to its box, upright and sRGB, so a
/// provider only has to cover-fit it.
#[derive(Debug, Clone, PartialEq)]
pub struct JobImage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}
