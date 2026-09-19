//! Spread-level edits to an assembled book: regenerate, reject, choose a
//! template, lock, shuffle, swap two photos.
//!
//! Every edit is a pure function over a `Book`, the library, the photo slice
//! the book indexes and the weights -- the same inputs `pace::assemble`
//! took, so nothing here can produce a page `assemble` could not have. The
//! command layer loads those four, applies the edit, persists the book and
//! returns the new layout; nothing is decided in the webview.
//!
//! ## Openings, not pages
//!
//! The unit of editing is the OPENING: what the reader sees with the book
//! open. Page 1 alone facing the inside front cover, then each pair of facing
//! pages, then the last page alone. It is numbered the way the preview draws
//! it (`toSpreads` in `app/types/preview.ts`): `0` is page 1, `1 ..= S` the
//! spreads, `S + 1` the last page. Locking, rejecting and regenerating all
//! act on an opening, because a spread's two pages come from one template
//! and cannot be changed separately.
//!
//! ## "Regenerate" shows a DIFFERENT layout
//!
//! `best_spread` picks the highest-scoring template and the seed only breaks
//! exact ties, so re-running it with a fresh seed returns the same layout
//! almost every time. That is not what a user clicking "try another" wants.
//! Regenerating therefore excludes the CURRENT template and picks the best of
//! the rest, so every click shows a layout the user has not seen -- until the
//! alternatives run out, which is reported rather than silently repeating.
//! Rejecting is the same choice made permanent for that opening.
//!
//! ## Hard constraints still hold
//!
//! Choosing a template or swapping two photos goes through the same
//! `rejects` the scorer enforces: a face in the gutter, a face clipped by
//! the crop, a face outside the safe margin or a photo below `MIN_DPI`
//! refuses the edit with the reason. An edit cannot produce a book that
//! pre-flight would then block.

use crate::book::cover::{self, CoverPhoto, Rgb};
use crate::book::crop::choose_crop;
use crate::book::cull::Photo;
use crate::book::pace::{half_id, place, rebuild, single_fit, tie_break, Book, Page};
use crate::book::score::{best_spread, rejects, slot_aspect, Rejection};
use crate::geometry::{BleedEdge, CoverSide, Rect, Side};
use crate::templates::{Library, PageLayout, Role, Slot, SpreadTemplate, Weights};
use serde::{Deserialize, Serialize};

/// What the user has said about one opening. Absent from `Book::controls`
/// means all defaults, which is how every book saved before this existed
/// loads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningControls {
    /// A locked opening is skipped by `Shuffle` and refuses every other edit.
    #[serde(default)]
    pub locked: bool,
    /// Template ids (spread ids, or `"<id>:left"` / `"<id>:right"` for a
    /// single page) the user has rejected for this opening. Never chosen
    /// again for it by any edit.
    #[serde(default)]
    pub rejected: Vec<String>,
    /// How many times this opening has been regenerated. Mixed into the seed
    /// so consecutive regenerations break ties differently, and persisted so
    /// reopening the project and clicking again continues the sequence
    /// rather than restarting it.
    #[serde(default)]
    pub rerolls: u32,
}

/// One placement, named the way the preview names it: the printed page
/// number and the placement's z within that page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementRef {
    pub page: u32,
    pub z: u32,
}

impl std::fmt::Display for PlacementRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "page {} slot {}", self.page, self.z)
    }
}

/// An edit the webview asks for. One enum rather than six commands so every
/// write goes through one door and returns the same thing: the whole book.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BookEdit {
    /// Lay the opening's photos out on a different template.
    Regenerate {
        opening: usize,
    },
    /// Never show this opening's current template again, and regenerate.
    RejectTemplate {
        opening: usize,
    },
    /// Lay the opening's photos out on exactly this template.
    SetTemplate {
        opening: usize,
        #[serde(rename = "templateId")]
        template_id: String,
    },
    SetLocked {
        opening: usize,
        locked: bool,
    },
    /// Regenerate every unlocked opening that has an alternative.
    Shuffle,
    /// Exchange the photos in two slots, anywhere in the book.
    SwapPhotos {
        a: PlacementRef,
        b: PlacementRef,
    },
    /// Move or resize one placement's crop window by hand. `x`, `y` and `w`
    /// are in the photo's own normalised frame; the height is derived from
    /// the slot's aspect, so a hand crop can never distort.
    SetCrop {
        placement: PlacementRef,
        x: f64,
        y: f64,
        w: f64,
    },
    /// Move or resize the slot itself, in the page's normalised frame. The
    /// photo is re-cropped for the slot's new shape.
    SetSlot {
        placement: PlacementRef,
        rect: Rect,
    },
    /// Put photo `photo` (an index into the analysed set, placed or not) in
    /// this slot. A photo already in the book trades places with this one.
    ReplacePhoto {
        placement: PlacementRef,
        photo: usize,
    },
    /// Print this book at a different size. See `book::reprint`: the layout
    /// is kept and only the crops are recut, and unlike every other arm here
    /// it never refuses.
    ///
    /// Deliberately absent from `app/agent/tools.ts`. The chat agent edits
    /// layouts; it does not get to change what book the user is buying.
    SetPrintSpec {
        spec: crate::print_spec::PrintSpec,
    },
    /// Put photo `photo` (any analysed photo, in the book or not) on one
    /// side of the cover with its automatic crop, or clear that side.
    SetCoverPhoto {
        side: CoverSide,
        photo: Option<usize>,
    },
    /// `SetCrop` for a cover photo: the height is derived from the cover
    /// panel's aspect.
    SetCoverCrop {
        side: CoverSide,
        x: f64,
        y: f64,
        w: f64,
    },
    SetSpineColour {
        rgb: Rgb,
    },
}

/// What one photo would look like in one slot: the crop it would get, and
/// the hard constraint that refuses it there, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SlotCandidate {
    pub crop: Rect,
    pub refused: Option<Rejection>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    NoSuchOpening(usize),
    Locked(usize),
    /// The opening holds no photos, so there is nothing to lay out.
    Empty(usize),
    /// Every template that can hold this many photos has been shown or
    /// rejected.
    NoAlternative {
        opening: usize,
        photos: usize,
    },
    UnknownTemplate(String),
    /// The template exists but holds a different number of photos.
    WrongPhotoCount {
        template_id: String,
        holds: usize,
        photos: usize,
    },
    /// Every way of laying these photos out on this template breaks a hard
    /// constraint.
    TemplateRejects(String),
    NoSuchPlacement(PlacementRef),
    /// An index past the end of the analysed photos.
    NoSuchPhoto(usize),
    SamePlacement,
    SwapRejected {
        placement: PlacementRef,
        reason: Rejection,
    },
    /// A hand crop would break a hard constraint at this slot.
    CropRejected {
        placement: PlacementRef,
        reason: Rejection,
    },
    /// A hand crop falls outside the photo, or is too small to mean anything.
    CropOutOfBounds(PlacementRef),
    /// A moved or resized slot leaves the page, or is too small to print.
    SlotOutOfBounds(PlacementRef),
    /// A moved or resized slot lands on another slot of the same page.
    SlotOverlaps {
        placement: PlacementRef,
        other: PlacementRef,
    },
    /// The photo cannot be laid into the slot's new shape without breaking a
    /// hard constraint.
    SlotRejected {
        placement: PlacementRef,
        reason: Rejection,
    },
    /// A page names a template the library no longer has.
    MissingLayout(String),
    /// A cover photo or its hand crop would break a hard constraint there.
    CoverRejected {
        side: CoverSide,
        reason: Rejection,
    },
    CoverCropOutOfBounds(CoverSide),
    /// A cover crop was asked for on a side that has no photo.
    NoCoverPhoto(CoverSide),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchOpening(o) => write!(f, "this book has no opening {o}"),
            Self::Locked(_) => write!(f, "this spread is locked; unlock it to change it"),
            Self::Empty(_) => write!(
                f,
                "this page holds no photos, so there is nothing to lay out"
            ),
            Self::NoAlternative { photos, .. } => write!(
                f,
                "every layout for {photos} photo{} has been shown or rejected here; \
                 swap a photo in or out to open up different layouts",
                if *photos == 1 { "" } else { "s" }
            ),
            Self::UnknownTemplate(id) => write!(f, "no template is called {id:?}"),
            Self::WrongPhotoCount {
                template_id,
                holds,
                photos,
            } => write!(
                f,
                "{template_id} holds {holds} photos, but this opening has {photos}"
            ),
            Self::TemplateRejects(id) => write!(
                f,
                "{id} cannot hold these photos without cutting a face, putting one in the \
                 gutter or the margin, or printing below the book's lowest print resolution"
            ),
            Self::NoSuchPlacement(p) => write!(f, "there is no photo at {p}"),
            Self::NoSuchPhoto(i) => write!(f, "this book has no photo {i}"),
            Self::SamePlacement => write!(f, "choose two different photos to swap"),
            Self::SwapRejected { placement, reason } => write!(
                f,
                "the photo moving to {placement} would {}",
                match reason {
                    Rejection::FaceClipped => "have a face cut by the slot's crop",
                    Rejection::FaceInGutter => "put a face in the gutter",
                    Rejection::FaceInSafeMargin => "put a face outside the safe margin",
                    Rejection::TooLowResolution => "print below the book's lowest print resolution at that size",
                }
            ),
            Self::CropRejected { reason, .. } => write!(
                f,
                "that crop would {}",
                match reason {
                    Rejection::FaceClipped => "cut a face",
                    Rejection::FaceInGutter => "put a face in the gutter",
                    Rejection::FaceInSafeMargin => "put a face outside the safe margin",
                    Rejection::TooLowResolution => "print below the book's lowest print resolution",
                }
            ),
            Self::CropOutOfBounds(_) => {
                write!(f, "the crop window has to stay inside the photo")
            }
            Self::SlotOutOfBounds(_) => {
                write!(
                    f,
                    "a slot has to stay on the page and be at least 5% of it each way"
                )
            }
            Self::SlotOverlaps { other, .. } => {
                write!(f, "that would overlap the photo at {other}")
            }
            Self::SlotRejected { reason, .. } => write!(
                f,
                "at that size the photo would {}",
                match reason {
                    Rejection::FaceClipped => "have a face cut by the crop",
                    Rejection::FaceInGutter => "put a face in the gutter",
                    Rejection::FaceInSafeMargin => "put a face outside the safe margin",
                    Rejection::TooLowResolution => "print below the book's lowest print resolution",
                }
            ),
            Self::MissingLayout(id) => write!(
                f,
                "this page uses template {id:?}, which is no longer in the library"
            ),
            // `FaceInSafeMargin` is the only face refusal `cover::rejects`
            // gives for a face it can see, and on the cover that margin is
            // mostly the wrap, so the message names the fold under the board.
            Self::CoverRejected { side, reason } => write!(
                f,
                "on the {} that would {}",
                cover::side_name(*side),
                match reason {
                    Rejection::FaceClipped => "cut a face",
                    Rejection::FaceInGutter | Rejection::FaceInSafeMargin => {
                        "put a face where the cover folds under the board or too near its edge"
                    }
                    Rejection::TooLowResolution => {
                        "print below the book's lowest print resolution at the cover's size"
                    }
                }
            ),
            Self::CoverCropOutOfBounds(_) => {
                write!(f, "the crop window has to stay inside the photo")
            }
            Self::NoCoverPhoto(side) => {
                write!(f, "the {} has no photo to crop", cover::side_name(*side))
            }
        }
    }
}

/// How many openings the book has: page 1, the spreads, the last page.
pub fn opening_count(book: &Book) -> usize {
    match book.pages.len() {
        0 => 0,
        1 => 1,
        n => 2 + (n - 2).div_ceil(2),
    }
}

/// The indices into `book.pages` that opening `o` shows, in reading order.
/// Mirrors `toSpreads` in the webview exactly, including an unpaired last
/// middle page in a degenerate odd-length book.
pub fn opening_pages(book: &Book, o: usize) -> Option<Vec<usize>> {
    let n = book.pages.len();
    let count = opening_count(book);
    if o >= count {
        return None;
    }
    if o == 0 {
        return Some(vec![0]);
    }
    if o + 1 == count {
        return Some(vec![n - 1]);
    }
    let left = 1 + 2 * (o - 1);
    let mut pages = vec![left];
    if left + 1 < n - 1 {
        pages.push(left + 1);
    }
    Some(pages)
}

/// The opening a page index belongs to.
fn opening_of(book: &Book, page_index: usize) -> usize {
    let n = book.pages.len();
    if page_index == 0 {
        0
    } else if page_index + 1 == n {
        opening_count(book) - 1
    } else {
        1 + (page_index - 1) / 2
    }
}

/// The photo indices on an opening, page by page, in z order.
fn opening_photos(book: &Book, pages: &[usize]) -> Vec<usize> {
    pages
        .iter()
        .flat_map(|&p| book.pages[p].placements.iter().map(|pl| pl.photo_index))
        .collect()
}

/// A seed for one regeneration of one opening, distinct per opening and per
/// click so the tie-break does not resolve the same way everywhere.
fn opening_seed(book_seed: u64, opening: usize, rerolls: u32) -> u64 {
    book_seed
        ^ (opening as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (rerolls as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
}

/// Whether an opening is a single page (page 1 or the last page).
fn is_single(book: &Book, o: usize) -> bool {
    o == 0 || o + 1 == opening_count(book)
}

/// The template a spread opening drew from, for the variety term.
fn previous_template(book: &Book, o: usize) -> Option<String> {
    if o < 2 {
        return None;
    }
    let pages = opening_pages(book, o - 1)?;
    let id = &book.pages[pages[0]].template_id;
    (!id.contains(':') && id != crate::book::pace::BLANK_TEMPLATE_ID).then(|| id.clone())
}

/// Every half of every template, with the id the manifest names it by.
fn halves(lib: &Library) -> Vec<(String, &PageLayout)> {
    lib.spreads
        .iter()
        .flat_map(|t| {
            [
                (half_id(&t.id, Side::Left), &t.left),
                (half_id(&t.id, Side::Right), &t.right),
            ]
        })
        .collect()
}

/// The templates that could replace the current one on opening `o`, in
/// library order: same photo count, right side for a single page, not the
/// current one, not rejected. What the "change template" menu offers.
pub fn alternatives(book: &Book, lib: &Library, o: usize) -> Vec<String> {
    let Some(pages) = opening_pages(book, o) else {
        return Vec::new();
    };
    let n = opening_photos(book, &pages).len();
    if n == 0 {
        return Vec::new();
    }
    let current = &book.pages[pages[0]].template_id;
    let rejected = book
        .controls
        .get(&o)
        .map(|c| c.rejected.as_slice())
        .unwrap_or(&[]);
    let allowed = |id: &str| id != current && !rejected.iter().any(|r| r == id);
    if is_single(book, o) {
        let side = book.pages[pages[0]].side;
        halves(lib)
            .into_iter()
            .filter(|(id, l)| l.side == side && l.slots.len() == n && allowed(id))
            .map(|(id, _)| id)
            .collect()
    } else {
        lib.spreads_with(n)
            .into_iter()
            .filter(|t| allowed(&t.id))
            .map(|t| t.id.clone())
            .collect()
    }
}

/// Applies one edit. On `Err` the book is unchanged.
pub fn apply(
    book: &mut Book,
    edit: &BookEdit,
    lib: &Library,
    photos: &[Photo],
    w: &Weights,
) -> Result<(), EditError> {
    match edit {
        BookEdit::Regenerate { opening } => regenerate(book, lib, photos, w, *opening),
        BookEdit::RejectTemplate { opening } => reject(book, lib, photos, w, *opening),
        BookEdit::SetTemplate {
            opening,
            template_id,
        } => set_template(book, lib, photos, w, *opening, template_id),
        BookEdit::SetLocked { opening, locked } => {
            opening_pages(book, *opening).ok_or(EditError::NoSuchOpening(*opening))?;
            book.controls.entry(*opening).or_default().locked = *locked;
            Ok(())
        }
        BookEdit::Shuffle => {
            for o in 0..opening_count(book) {
                match regenerate(book, lib, photos, w, o) {
                    Ok(()) | Err(EditError::Locked(_) | EditError::Empty(_)) => {}
                    Err(EditError::NoAlternative { .. }) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }
        BookEdit::SwapPhotos { a, b } => swap(book, lib, photos, *a, *b),
        BookEdit::SetCrop { placement, x, y, w } => {
            set_crop(book, lib, photos, *placement, *x, *y, *w)
        }
        BookEdit::SetSlot { placement, rect } => set_slot(book, lib, photos, *placement, *rect),
        BookEdit::ReplacePhoto { placement, photo } => {
            replace(book, lib, photos, *placement, *photo)
        }
        // No `lib` and no `w`: a size change re-cuts crops, it does not
        // re-pick a template or re-score anything. The findings `reprint`
        // computes are dropped here on purpose -- this door returns the whole
        // book, and the panel has already seen them from `check_print_spec`.
        BookEdit::SetPrintSpec { spec } => {
            *book = crate::book::reprint::reprint(book, photos, spec).book;
            Ok(())
        }
        BookEdit::SetCoverPhoto { side, photo } => set_cover_photo(book, photos, *side, *photo),
        BookEdit::SetCoverCrop { side, x, y, w } => {
            set_cover_crop(book, photos, *side, *x, *y, *w)
        }
        BookEdit::SetSpineColour { rgb } => {
            book.cover.spine = *rgb;
            Ok(())
        }
    }
}

fn unlocked(book: &Book, o: usize) -> Result<Vec<usize>, EditError> {
    let pages = opening_pages(book, o).ok_or(EditError::NoSuchOpening(o))?;
    if book.controls.get(&o).is_some_and(|c| c.locked) {
        return Err(EditError::Locked(o));
    }
    Ok(pages)
}

/// Lays the opening's photos out on the best template it has not shown yet.
fn regenerate(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    w: &Weights,
    o: usize,
) -> Result<(), EditError> {
    let pages = unlocked(book, o)?;
    let indices = opening_photos(book, &pages);
    if indices.is_empty() {
        return Err(EditError::Empty(o));
    }
    let candidates = alternatives(book, lib, o);
    if candidates.is_empty() {
        return Err(EditError::NoAlternative {
            opening: o,
            photos: indices.len(),
        });
    }
    let rerolls = book.controls.get(&o).map(|c| c.rerolls).unwrap_or(0) + 1;
    let seed = opening_seed(book.seed, o, rerolls);
    let laid = lay_out(book, lib, photos, w, o, &pages, &indices, &candidates, seed)?;
    commit(book, &pages, laid);
    book.controls.entry(o).or_default().rerolls = rerolls;
    Ok(())
}

/// Rejects the current template for good, then regenerates. Checked before
/// anything is recorded, so an opening with nowhere else to go keeps its
/// template rather than losing it and failing.
fn reject(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    w: &Weights,
    o: usize,
) -> Result<(), EditError> {
    let pages = unlocked(book, o)?;
    let indices = opening_photos(book, &pages);
    if indices.is_empty() {
        return Err(EditError::Empty(o));
    }
    if alternatives(book, lib, o).is_empty() {
        return Err(EditError::NoAlternative {
            opening: o,
            photos: indices.len(),
        });
    }
    let current = book.pages[pages[0]].template_id.clone();
    let controls = book.controls.entry(o).or_default();
    if !controls.rejected.contains(&current) {
        controls.rejected.push(current);
    }
    regenerate(book, lib, photos, w, o)
}

/// Lays the opening out on exactly `template_id`, and forgives a rejection
/// of it: choosing a template by name is a stronger statement than having
/// rejected it once.
fn set_template(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    w: &Weights,
    o: usize,
    template_id: &str,
) -> Result<(), EditError> {
    let pages = unlocked(book, o)?;
    let indices = opening_photos(book, &pages);
    if indices.is_empty() {
        return Err(EditError::Empty(o));
    }
    let holds = if is_single(book, o) {
        let side = book.pages[pages[0]].side;
        halves(lib)
            .into_iter()
            .find(|(id, _)| id == template_id)
            .map(|(_, l)| {
                if l.side == side {
                    l.slots.len()
                } else {
                    usize::MAX
                }
            })
    } else {
        lib.spreads
            .iter()
            .find(|t| t.id == template_id)
            .map(|t| t.photo_count())
    };
    let holds = holds.ok_or_else(|| EditError::UnknownTemplate(template_id.to_string()))?;
    if holds != indices.len() {
        return Err(EditError::WrongPhotoCount {
            template_id: template_id.to_string(),
            holds,
            photos: indices.len(),
        });
    }
    let seed = opening_seed(book.seed, o, 0);
    let laid = lay_out(
        book,
        lib,
        photos,
        w,
        o,
        &pages,
        &indices,
        &[template_id.to_string()],
        seed,
    )?;
    commit(book, &pages, laid);
    if let Some(c) = book.controls.get_mut(&o) {
        c.rejected.retain(|r| r != template_id);
    }
    Ok(())
}

/// The new pages for an opening, chosen from `candidates`. Spread candidates
/// go through `best_spread`, which also chooses the photo-to-slot
/// assignment; single-page candidates are ranked by `single_fit` with the
/// photos in their current order. `None` from either means every candidate
/// breaks a hard constraint.
#[allow(clippy::too_many_arguments)]
fn lay_out(
    book: &Book,
    lib: &Library,
    photos: &[Photo],
    w: &Weights,
    o: usize,
    pages: &[usize],
    indices: &[usize],
    candidates: &[String],
    seed: u64,
) -> Result<Vec<Page>, EditError> {
    if is_single(book, o) {
        let side = book.pages[pages[0]].side;
        let mut scored: Vec<(f64, String, &PageLayout)> = halves(lib)
            .into_iter()
            .filter(|(id, l)| l.side == side && candidates.contains(id))
            .filter_map(|(id, l)| {
                single_fit(&book.spec, l, indices, photos).map(|fit| (fit, id, l))
            })
            .collect();
        if scored.is_empty() {
            return Err(rejected_all(candidates));
        }
        let best = scored
            .iter()
            .fold(f64::NEG_INFINITY, |m, (fit, _, _)| m.max(*fit));
        scored.retain(|(fit, _, _)| *fit == best);
        let (_, id, layout) = scored.swap_remove(tie_break(seed, scored.len()));
        return Ok(vec![Page {
            number: 0,
            side,
            template_id: id,
            placements: place(&book.spec, layout, indices, photos),
        }]);
    }
    let eligible: Vec<&SpreadTemplate> = lib
        .spreads
        .iter()
        .filter(|t| candidates.contains(&t.id))
        .collect();
    let refs: Vec<&Photo> = indices.iter().map(|&i| &photos[i]).collect();
    let previous = previous_template(book, o);
    let (template, assignment, _) =
        best_spread(&book.spec, &eligible, &refs, previous.as_deref(), w, seed)
            .ok_or_else(|| rejected_all(candidates))?;
    Ok(rebuild(&book.spec, template, &assignment, indices, photos).to_vec())
}

fn rejected_all(candidates: &[String]) -> EditError {
    EditError::TemplateRejects(candidates.join(", "))
}

/// Writes new pages over an opening, keeping the printed page numbers. A
/// degenerate opening with one middle page takes the left page of the pair.
fn commit(book: &mut Book, pages: &[usize], laid: Vec<Page>) {
    for (&p, page) in pages.iter().zip(laid) {
        let number = book.pages[p].number;
        book.pages[p] = Page { number, ..page };
    }
}

/// The slot behind a placement, resolved from the page's template so the
/// crop and the hard constraints see the same rectangle the scorer did.
fn slot_for<'a>(
    book: &Book,
    lib: &'a Library,
    page_index: usize,
) -> Result<&'a PageLayout, EditError> {
    let page = &book.pages[page_index];
    let (id, side) = match page.template_id.rsplit_once(':') {
        Some((id, "left")) => (id, Side::Left),
        Some((id, "right")) => (id, Side::Right),
        _ => (page.template_id.as_str(), page.side),
    };
    let template = lib
        .spreads
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| EditError::MissingLayout(page.template_id.clone()))?;
    Ok(match side {
        Side::Left => &template.left,
        Side::Right => &template.right,
    })
}

/// The slot a placement prints in, as the constraints see it: the
/// PLACEMENT's own rect (a slot the user has moved no longer matches the
/// template's), with the template's role where the template is still in the
/// library and `Support` where it is not, and bleed edges read off the rect
/// itself -- an edge on the canvas boundary bleeds, the fold edge never does.
/// Never fails: a page whose template has gone is still a page of rects.
fn placement_slot(
    book: &Book,
    lib: &Library,
    page_index: usize,
    slot_index: usize,
) -> (Slot, Side) {
    let page = &book.pages[page_index];
    let rect = page.placements[slot_index].slot_rect;
    let role = slot_for(book, lib, page_index)
        .ok()
        .and_then(|layout| layout.slots.get(slot_index))
        .map(|s| s.role)
        .unwrap_or(Role::Support);
    (
        Slot {
            rect,
            role,
            bleed: bleed_edges(&rect, page.side),
            aspect_pref: (book.spec.page_aspect(&rect), book.spec.page_aspect(&rect)),
        },
        page.side,
    )
}

/// The edges of a page-normalised rect that reach the canvas boundary and so
/// print as bleed. The fold edge is not a boundary: the paper continues.
fn bleed_edges(rect: &Rect, side: Side) -> Vec<BleedEdge> {
    const EPS: f64 = 1e-9;
    let mut edges = Vec::new();
    if rect.x <= EPS && side == Side::Left {
        edges.push(BleedEdge::Left);
    }
    if rect.right() >= 1.0 - EPS && side == Side::Right {
        edges.push(BleedEdge::Right);
    }
    if rect.y <= EPS {
        edges.push(BleedEdge::Top);
    }
    if rect.bottom() >= 1.0 - EPS {
        edges.push(BleedEdge::Bottom);
    }
    edges
}

fn locate(book: &Book, r: PlacementRef) -> Result<(usize, usize), EditError> {
    let page_index = book
        .pages
        .iter()
        .position(|p| p.number == r.page)
        .ok_or(EditError::NoSuchPlacement(r))?;
    let slot_index = book.pages[page_index]
        .placements
        .iter()
        .position(|pl| pl.z == r.z)
        .ok_or(EditError::NoSuchPlacement(r))?;
    Ok((page_index, slot_index))
}

/// Exchanges the photos in two slots, re-cropping each for its new slot and
/// refusing if either would break a hard constraint. Both are checked before
/// either is written.
fn swap(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    a: PlacementRef,
    b: PlacementRef,
) -> Result<(), EditError> {
    if a == b {
        return Err(EditError::SamePlacement);
    }
    let (pa, ia) = locate(book, a)?;
    let (pb, ib) = locate(book, b)?;
    for p in [pa, pb] {
        let o = opening_of(book, p);
        if book.controls.get(&o).is_some_and(|c| c.locked) {
            return Err(EditError::Locked(o));
        }
    }
    let photo_a = book.pages[pa].placements[ia].photo_index;
    let photo_b = book.pages[pb].placements[ib].photo_index;

    // `z` is 1-based slot order (`pace::place`), so the layout slot behind a
    // placement is `z - 1` -- looked up by z rather than by position in the
    // placements list, which would drift if a placement were ever removed.
    let fit = |page_index: usize, slot_index: usize, photo_index: usize, at: PlacementRef| {
        let (slot, side) = placement_slot(book, lib, page_index, slot_index);
        let photo = &photos[photo_index];
        let crop = choose_crop(photo, slot_aspect(&book.spec, &slot));
        match rejects(&book.spec, photo, &crop, &slot, side) {
            Some(reason) => Err(EditError::SwapRejected {
                placement: at,
                reason,
            }),
            None => Ok(crop),
        }
    };
    let crop_a = fit(pa, ia, photo_b, a)?;
    let crop_b = fit(pb, ib, photo_a, b)?;

    book.pages[pa].placements[ia].photo_index = photo_b;
    book.pages[pa].placements[ia].crop = crop_a;
    book.pages[pb].placements[ib].photo_index = photo_a;
    book.pages[pb].placements[ib].crop = crop_b;
    Ok(())
}

/// Puts `photo` in the slot at `at`. A photo already placed elsewhere is
/// swapped in, so the book never holds one photo twice; a left-out photo is
/// re-cropped for the slot and checked against the hard constraints, and the
/// photo it displaces leaves the book.
fn replace(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    at: PlacementRef,
    photo: usize,
) -> Result<(), EditError> {
    if photo >= photos.len() {
        return Err(EditError::NoSuchPhoto(photo));
    }
    let placed = book.pages.iter().find_map(|p| {
        p.placements.iter().find(|pl| pl.photo_index == photo).map(|pl| PlacementRef {
            page: p.number,
            z: pl.z,
        })
    });
    if let Some(other) = placed {
        return swap(book, lib, photos, at, other);
    }
    let (pi, si) = locate(book, at)?;
    let o = opening_of(book, pi);
    if book.controls.get(&o).is_some_and(|c| c.locked) {
        return Err(EditError::Locked(o));
    }
    let (slot, side) = placement_slot(book, lib, pi, si);
    let crop = choose_crop(&photos[photo], slot_aspect(&book.spec, &slot));
    if let Some(reason) = rejects(&book.spec, &photos[photo], &crop, &slot, side) {
        return Err(EditError::SwapRejected {
            placement: at,
            reason,
        });
    }
    let placement = &mut book.pages[pi].placements[si];
    placement.photo_index = photo;
    placement.crop = crop;
    Ok(())
}

/// How every analysed photo would sit in the slot at `at`, in `photos`
/// order, so the picker can show each one cropped as it would print and say
/// up front which ones `replace` would refuse.
pub fn slot_candidates(
    book: &Book,
    lib: &Library,
    photos: &[Photo],
    at: PlacementRef,
) -> Result<Vec<SlotCandidate>, EditError> {
    let (pi, si) = locate(book, at)?;
    let (slot, side) = placement_slot(book, lib, pi, si);
    let aspect = slot_aspect(&book.spec, &slot);
    Ok(photos
        .iter()
        .map(|photo| {
            let crop = choose_crop(photo, aspect);
            SlotCandidate {
                refused: rejects(&book.spec, photo, &crop, &slot, side),
                crop,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;
    use crate::book::cull::{Face, Overrides, PaletteColor};
    use crate::book::pace::assemble;

    /// The frozen five: `03` (1 photo), `07` and `08` (2), `13` and `35` (3).
    /// A 2-photo spread therefore has exactly ONE alternative, which is what
    /// makes "regenerate shows a different template, then runs out" exact.
    fn frozen_library() -> Library {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
        Library::load(&dir).expect("the frozen fixture library must decompose")
    }

    /// Landscape and portrait alternate, so a swap between two slots of
    /// different shape produces a DIFFERENT crop and the recrop is observable.
    fn photo(i: usize) -> Photo {
        let portrait = i % 3 == 2;
        Photo {
            path: format!("/e/p{i:02}.jpg"),
            hash: format!("h{i:02}"),
            width: if portrait { 3000 } else { 4000 },
            height: if portrait { 4000 } else { 3000 },
            is_utility: false,
            aesthetic_pct: ((i * 37) % 100) as u8,
            sharpness_pct: 50,
            near_dup_cluster: i as u32,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::<PaletteColor>::new(),
            capture_quality: None,
            scene_tags: Vec::new(),
            captured_at: None,
            clipped_low: 0.0, clipped_high: 0.0, feature_print: None,
            location: None,
        }
    }

    fn photos(n: usize) -> Vec<Photo> {
        (0..n).map(photo).collect()
    }

    /// A 20-page book from 25 photos on the frozen library: the density swing
    /// cuts both 2-up and 3-up spreads, so both alternative sets are
    /// exercised. `spread_with` asserts the mix rather than assuming it.
    fn book(photos: &[Photo]) -> Book {
        assemble(&pixajoy_spec(),
            photos,
            20,
            &frozen_library(),
            &Weights::default(),
            1234,
            &Overrides::new(),
        )
        .expect("no overrides")
    }

    fn template_of(book: &Book, o: usize) -> String {
        let pages = opening_pages(book, o).unwrap();
        book.pages[pages[0]].template_id.clone()
    }

    fn photos_of(book: &Book, o: usize) -> Vec<usize> {
        let pages = opening_pages(book, o).unwrap();
        let mut v = opening_photos(book, &pages);
        v.sort_unstable();
        v
    }

    /// The first spread opening holding exactly `n` photos.
    fn spread_with(book: &Book, n: usize) -> usize {
        (1..opening_count(book) - 1)
            .find(|&o| photos_of(book, o).len() == n)
            .unwrap_or_else(|| panic!("fixture needs a {n}-photo spread"))
    }

    // --- openings mirror the preview ---------------------------------------

    #[test]
    fn edit_numbers_openings_the_way_the_preview_draws_them() {
        let b = book(&photos(25));
        assert_eq!(b.pages.len(), 20);
        assert_eq!(opening_count(&b), 11, "page 1, nine spreads, page 20");
        assert_eq!(opening_pages(&b, 0), Some(vec![0]));
        assert_eq!(opening_pages(&b, 1), Some(vec![1, 2]));
        assert_eq!(opening_pages(&b, 9), Some(vec![17, 18]));
        assert_eq!(opening_pages(&b, 10), Some(vec![19]));
        assert_eq!(opening_pages(&b, 11), None);
        for o in 0..11 {
            for &p in &opening_pages(&b, o).unwrap() {
                assert_eq!(opening_of(&b, p), o, "page index {p}");
            }
        }
    }

    /// A degenerate odd-length book: `toSpreads` leaves the last middle page
    /// unpartnered rather than dropping it, and so does this.
    #[test]
    fn edit_keeps_an_unpaired_middle_page_in_an_odd_book() {
        let mut b = book(&photos(25));
        b.pages.truncate(5);
        assert_eq!(opening_count(&b), 4);
        assert_eq!(opening_pages(&b, 1), Some(vec![1, 2]));
        assert_eq!(
            opening_pages(&b, 2),
            Some(vec![3]),
            "page index 3 has no partner"
        );
        assert_eq!(opening_pages(&b, 3), Some(vec![4]));
    }

    // --- regenerate ---------------------------------------------------------

    /// The whole point: a regenerated spread shows a template the user has
    /// NOT just seen, with the same photos on it.
    #[test]
    fn edit_regenerate_changes_the_template_and_keeps_the_photos() {
        let ps = photos(25);
        let mut b = book(&ps);
        let o = spread_with(&b, 2);
        let before_template = template_of(&b, o);
        let before_photos = photos_of(&b, o);
        let lib = frozen_library();

        apply(
            &mut b,
            &BookEdit::Regenerate { opening: o },
            &lib,
            &ps,
            &Weights::default(),
        )
        .unwrap();

        assert_ne!(
            template_of(&b, o),
            before_template,
            "regenerate must show a different layout"
        );
        assert_eq!(
            photos_of(&b, o),
            before_photos,
            "the same photos stay on the spread"
        );
        assert_eq!(b.controls[&o].rerolls, 1);
        let pages = opening_pages(&b, o).unwrap();
        assert_eq!(
            b.pages[pages[0]].number as usize,
            pages[0] + 1,
            "page numbers survive"
        );
        assert_eq!(
            b.pages[pages[0]].template_id, b.pages[pages[1]].template_id,
            "both halves from one template"
        );
    }

    /// With one alternative, the second regenerate goes back to the first
    /// template -- browsing, not dice -- and the reroll count keeps climbing.
    #[test]
    fn edit_regenerate_cycles_through_the_alternatives() {
        let ps = photos(25);
        let mut b = book(&ps);
        let o = spread_with(&b, 2);
        let first = template_of(&b, o);
        let lib = frozen_library();
        let w = Weights::default();

        apply(&mut b, &BookEdit::Regenerate { opening: o }, &lib, &ps, &w).unwrap();
        let second = template_of(&b, o);
        apply(&mut b, &BookEdit::Regenerate { opening: o }, &lib, &ps, &w).unwrap();

        assert_ne!(first, second);
        assert_eq!(template_of(&b, o), first);
        assert_eq!(b.controls[&o].rerolls, 2);
    }

    #[test]
    fn edit_regenerate_lays_out_the_single_pages_too() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let before = template_of(&b, 0);
        assert!(
            before.ends_with(":right"),
            "page 1 is a right half: {before}"
        );
        let n = photos_of(&b, 0).len();
        let others = alternatives(&b, &lib, 0);
        assert!(
            !others.is_empty(),
            "fixture: page 1 needs an alternative half for {n} photos"
        );

        apply(
            &mut b,
            &BookEdit::Regenerate { opening: 0 },
            &lib,
            &ps,
            &Weights::default(),
        )
        .unwrap();

        assert!(others.contains(&template_of(&b, 0)));
        assert_eq!(photos_of(&b, 0).len(), n);
        assert_eq!(b.pages[0].side, Side::Right);
    }

    #[test]
    fn edit_refuses_to_regenerate_a_locked_opening_and_leaves_it_untouched() {
        let ps = photos(25);
        let mut b = book(&ps);
        let o = spread_with(&b, 2);
        let lib = frozen_library();
        apply(
            &mut b,
            &BookEdit::SetLocked {
                opening: o,
                locked: true,
            },
            &lib,
            &ps,
            &Weights::default(),
        )
        .unwrap();
        let before = b.clone();

        let err = apply(
            &mut b,
            &BookEdit::Regenerate { opening: o },
            &lib,
            &ps,
            &Weights::default(),
        );

        assert_eq!(err, Err(EditError::Locked(o)));
        assert_eq!(b, before);
    }

    #[test]
    fn edit_reports_an_opening_that_does_not_exist() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::Regenerate { opening: 11 },
                &lib,
                &ps,
                &Weights::default()
            ),
            Err(EditError::NoSuchOpening(11))
        );
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::SetLocked {
                    opening: 11,
                    locked: true
                },
                &lib,
                &ps,
                &Weights::default()
            ),
            Err(EditError::NoSuchOpening(11))
        );
        assert!(
            b.controls.is_empty(),
            "a refused lock must not create a controls entry"
        );
    }

    // --- reject -------------------------------------------------------------

    /// Reject records the template AND moves off it; a second reject with no
    /// alternative left is refused and the book -- including the rejected
    /// list -- is exactly as it was.
    #[test]
    fn edit_reject_records_the_template_then_runs_out_of_alternatives_without_changing_anything() {
        let ps = photos(25);
        let mut b = book(&ps);
        let o = spread_with(&b, 2);
        let first = template_of(&b, o);
        let lib = frozen_library();
        let w = Weights::default();

        apply(
            &mut b,
            &BookEdit::RejectTemplate { opening: o },
            &lib,
            &ps,
            &w,
        )
        .unwrap();
        let second = template_of(&b, o);
        assert_ne!(second, first);
        assert_eq!(b.controls[&o].rejected, vec![first.clone()]);
        assert!(
            alternatives(&b, &lib, o).is_empty(),
            "07 and 08 are the only 2-ups"
        );

        let before = b.clone();
        let err = apply(
            &mut b,
            &BookEdit::RejectTemplate { opening: o },
            &lib,
            &ps,
            &w,
        );
        assert_eq!(
            err,
            Err(EditError::NoAlternative {
                opening: o,
                photos: 2
            })
        );
        assert_eq!(
            b, before,
            "a refused reject must change nothing, not even the rejected list"
        );

        let err = apply(&mut b, &BookEdit::Regenerate { opening: o }, &lib, &ps, &w);
        assert_eq!(
            err,
            Err(EditError::NoAlternative {
                opening: o,
                photos: 2
            })
        );
    }

    // --- set template ---------------------------------------------------------

    #[test]
    fn edit_set_template_lays_out_exactly_the_named_template_and_forgives_its_rejection() {
        let ps = photos(25);
        let mut b = book(&ps);
        let o = spread_with(&b, 2);
        let first = template_of(&b, o);
        let lib = frozen_library();
        let w = Weights::default();
        apply(
            &mut b,
            &BookEdit::RejectTemplate { opening: o },
            &lib,
            &ps,
            &w,
        )
        .unwrap();
        assert!(b.controls[&o].rejected.contains(&first));

        apply(
            &mut b,
            &BookEdit::SetTemplate {
                opening: o,
                template_id: first.clone(),
            },
            &lib,
            &ps,
            &w,
        )
        .unwrap();

        assert_eq!(template_of(&b, o), first);
        assert!(
            !b.controls[&o].rejected.contains(&first),
            "choosing it by name forgives the rejection"
        );
    }

    #[test]
    fn edit_set_template_refuses_a_template_for_a_different_photo_count_or_an_unknown_one() {
        let ps = photos(25);
        let mut b = book(&ps);
        let o = spread_with(&b, 2);
        let lib = frozen_library();
        let w = Weights::default();
        let before = b.clone();

        let err = apply(
            &mut b,
            &BookEdit::SetTemplate {
                opening: o,
                template_id: "13-three-up-hero-left-stack-right".into(),
            },
            &lib,
            &ps,
            &w,
        );
        assert_eq!(
            err,
            Err(EditError::WrongPhotoCount {
                template_id: "13-three-up-hero-left-stack-right".into(),
                holds: 3,
                photos: 2
            })
        );
        let err = apply(
            &mut b,
            &BookEdit::SetTemplate {
                opening: o,
                template_id: "nope".into(),
            },
            &lib,
            &ps,
            &w,
        );
        assert_eq!(err, Err(EditError::UnknownTemplate("nope".into())));
        assert_eq!(b, before);
    }

    #[test]
    fn edit_set_template_on_a_single_page_takes_a_half_of_the_right_side() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w = Weights::default();
        let n = photos_of(&b, 0).len();
        let wanted = alternatives(&b, &lib, 0)
            .into_iter()
            .next()
            .expect("an alternative half");
        assert!(wanted.ends_with(":right"));
        let wrong_side = wanted.replace(":right", ":left");

        let err = apply(
            &mut b,
            &BookEdit::SetTemplate {
                opening: 0,
                template_id: wrong_side.clone(),
            },
            &lib,
            &ps,
            &w,
        );
        assert!(
            matches!(err, Err(EditError::WrongPhotoCount { .. })),
            "a left half cannot print on page 1: {err:?}"
        );

        apply(
            &mut b,
            &BookEdit::SetTemplate {
                opening: 0,
                template_id: wanted.clone(),
            },
            &lib,
            &ps,
            &w,
        )
        .unwrap();
        assert_eq!(template_of(&b, 0), wanted);
        assert_eq!(photos_of(&b, 0).len(), n);
    }

    // --- shuffle ----------------------------------------------------------------

    #[test]
    fn edit_shuffle_regenerates_every_unlocked_opening_and_skips_the_locked_one() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w = Weights::default();
        let locked = spread_with(&b, 2);
        apply(
            &mut b,
            &BookEdit::SetLocked {
                opening: locked,
                locked: true,
            },
            &lib,
            &ps,
            &w,
        )
        .unwrap();
        let before: Vec<String> = (0..opening_count(&b)).map(|o| template_of(&b, o)).collect();
        let before_photos: Vec<Vec<usize>> =
            (0..opening_count(&b)).map(|o| photos_of(&b, o)).collect();
        let had_alternative: Vec<bool> = (0..opening_count(&b))
            .map(|o| !alternatives(&b, &lib, o).is_empty())
            .collect();
        assert!(
            had_alternative.iter().filter(|&&h| h).count() > 2,
            "fixture: several openings can change"
        );

        apply(&mut b, &BookEdit::Shuffle, &lib, &ps, &w).unwrap();

        for o in 0..opening_count(&b) {
            assert_eq!(
                photos_of(&b, o),
                before_photos[o],
                "shuffle never moves a photo"
            );
            if o == locked {
                assert_eq!(
                    template_of(&b, o),
                    before[o],
                    "the locked spread keeps its template"
                );
            } else if had_alternative[o] {
                assert_ne!(
                    template_of(&b, o),
                    before[o],
                    "opening {o} should have changed"
                );
            }
        }
    }

    // --- swap -------------------------------------------------------------------

    #[test]
    fn edit_swap_exchanges_two_photos_and_recrops_each_for_its_new_slot() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        // Two placements whose photos have different shapes, so at least one
        // crop must change.
        let refs: Vec<(PlacementRef, usize, Rect)> = b
            .pages
            .iter()
            .flat_map(|p| {
                p.placements.iter().map(move |pl| {
                    (
                        PlacementRef {
                            page: p.number,
                            z: pl.z,
                        },
                        pl.photo_index,
                        pl.crop,
                    )
                })
            })
            .collect();
        let (a, ia, crop_a) = refs
            .iter()
            .find(|(_, i, _)| ps[*i].width > ps[*i].height)
            .copied()
            .unwrap();
        let (bb, ib, crop_b) = refs
            .iter()
            .find(|(_, i, _)| ps[*i].width < ps[*i].height)
            .copied()
            .unwrap();

        apply(
            &mut b,
            &BookEdit::SwapPhotos { a, b: bb },
            &lib,
            &ps,
            &Weights::default(),
        )
        .unwrap();

        let at = |r: PlacementRef| {
            let page = b.pages.iter().find(|p| p.number == r.page).unwrap();
            page.placements
                .iter()
                .find(|pl| pl.z == r.z)
                .unwrap()
                .clone()
        };
        assert_eq!(at(a).photo_index, ib);
        assert_eq!(at(bb).photo_index, ia);
        assert!(
            at(a).crop != crop_a || at(bb).crop != crop_b,
            "a landscape and a portrait swapped slots, so a crop must have been recomputed"
        );
    }

    #[test]
    fn edit_swap_refuses_the_same_slot_a_missing_slot_and_a_locked_opening() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w = Weights::default();
        let a = PlacementRef { page: 2, z: 1 };
        let before = b.clone();

        assert_eq!(
            apply(&mut b, &BookEdit::SwapPhotos { a, b: a }, &lib, &ps, &w),
            Err(EditError::SamePlacement)
        );
        let missing = PlacementRef { page: 2, z: 9 };
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::SwapPhotos { a, b: missing },
                &lib,
                &ps,
                &w
            ),
            Err(EditError::NoSuchPlacement(missing))
        );
        apply(
            &mut b,
            &BookEdit::SetLocked {
                opening: 1,
                locked: true,
            },
            &lib,
            &ps,
            &w,
        )
        .unwrap();
        let elsewhere = PlacementRef { page: 4, z: 1 };
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::SwapPhotos { a, b: elsewhere },
                &lib,
                &ps,
                &w
            ),
            Err(EditError::Locked(1))
        );
        b.controls.clear();
        assert_eq!(b, before);
    }

    /// The hard constraints hold under a swap: a photo too small to print at
    /// the destination slot's size is refused with the reason, and nothing
    /// moves.
    #[test]
    fn edit_swap_refuses_a_photo_that_would_break_a_hard_constraint_at_its_new_slot() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        // Shrink one PLACED photo far below 200 DPI at any slot. The book was
        // assembled before the shrink, so it is still in place.
        let victim = b.pages[1].placements[0].photo_index;
        ps[victim].width = 300;
        ps[victim].height = 200;
        let a = PlacementRef { page: 2, z: 1 };
        let target = PlacementRef { page: 4, z: 1 };
        let before = b.clone();

        let err = apply(
            &mut b,
            &BookEdit::SwapPhotos { a, b: target },
            &lib,
            &ps,
            &Weights::default(),
        );

        assert_eq!(
            err,
            Err(EditError::SwapRejected {
                placement: target,
                reason: Rejection::TooLowResolution
            })
        );
        assert_eq!(b, before);
    }

    #[test]
    fn edit_swap_refuses_a_face_the_destination_crop_would_cut() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        // A face spanning the whole frame cannot survive any crop that is
        // not the full frame. Put it on a landscape photo and send it to a
        // portrait-shaped slot (page 1's hero on `35:right` is 0.64 x 0.42 --
        // find any slot whose crop for this photo is not full-frame).
        let a = PlacementRef { page: 2, z: 1 };
        let victim = b.pages[1].placements[0].photo_index;
        ps[victim].faces = vec![Face {
            box_: Rect::new(0.02, 0.02, 0.96, 0.96),
            capture_quality: None,
        }];
        let target = b
            .pages
            .iter()
            .flat_map(|p| {
                p.placements.iter().map(move |pl| PlacementRef {
                    page: p.number,
                    z: pl.z,
                })
            })
            .find(|&r| {
                r != a && {
                    let (pi, _) = locate(&b, r).unwrap();
                    let layout = slot_for(&b, &lib, pi).unwrap();
                    let slot = &layout.slots[(r.z - 1) as usize];
                    let crop = choose_crop(&ps[victim], slot_aspect(&pixajoy_spec(), slot));
                    rejects(&pixajoy_spec(), &ps[victim], &crop, slot, layout.side) == Some(Rejection::FaceClipped)
                }
            })
            .expect("fixture: some slot must clip a full-frame face");
        let before = b.clone();

        let err = apply(
            &mut b,
            &BookEdit::SwapPhotos { a, b: target },
            &lib,
            &ps,
            &Weights::default(),
        );

        assert_eq!(
            err,
            Err(EditError::SwapRejected {
                placement: target,
                reason: Rejection::FaceClipped
            })
        );
        assert_eq!(b, before);
    }

    // --- replace ----------------------------------------------------------------

    /// A photo the book was never given, appended after assembly so it is
    /// guaranteed unplaced. Portrait, where `at` holds a landscape photo, so
    /// a crop copied over from the old photo would be visibly wrong.
    fn with_left_out(ps: &mut Vec<Photo>) -> usize {
        let mut extra = photo(ps.len());
        extra.width = 3000;
        extra.height = 4000;
        extra.hash = "left-out".into();
        ps.push(extra);
        ps.len() - 1
    }

    fn placement(b: &Book, r: PlacementRef) -> crate::book::pace::Placement {
        let (pi, si) = locate(b, r).unwrap();
        b.pages[pi].placements[si].clone()
    }

    /// A landscape placement, so replacing it with a portrait photo must
    /// change the crop.
    fn landscape_placement(b: &Book, ps: &[Photo]) -> PlacementRef {
        b.pages
            .iter()
            .flat_map(|p| {
                p.placements.iter().map(move |pl| (p.number, pl.z, pl.photo_index))
            })
            .find(|&(_, _, i)| ps[i].width > ps[i].height)
            .map(|(page, z, _)| PlacementRef { page, z })
            .expect("fixture: some placed photo is landscape")
    }

    #[test]
    fn edit_replace_puts_a_left_out_photo_in_the_slot_cropped_for_that_slot() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let extra = with_left_out(&mut ps);
        let at = landscape_placement(&b, &ps);
        let old = placement(&b, at);
        let (pi, si) = locate(&b, at).unwrap();
        let (slot, _) = placement_slot(&b, &lib, pi, si);
        let expected = choose_crop(&ps[extra], slot_aspect(&pixajoy_spec(), &slot));

        apply(
            &mut b,
            &BookEdit::ReplacePhoto {
                placement: at,
                photo: extra,
            },
            &lib,
            &ps,
            &Weights::default(),
        )
        .unwrap();

        let now = placement(&b, at);
        assert_eq!(now.photo_index, extra);
        assert_eq!(now.crop, expected);
        assert_ne!(now.crop, old.crop, "a portrait replaced a landscape, so the crop must change");
        assert!(
            !b.pages.iter().flat_map(|p| &p.placements).any(|pl| pl.photo_index == old.photo_index),
            "the replaced photo leaves the book"
        );
    }

    /// Choosing a photo that is already in the book is a swap, so the book
    /// never holds the same photo twice.
    #[test]
    fn edit_replace_with_a_placed_photo_swaps_the_two() {
        let ps = photos(25);
        let b = book(&ps);
        let lib = frozen_library();
        let w = Weights::default();
        let a = PlacementRef { page: 2, z: 1 };
        let other = PlacementRef { page: 4, z: 1 };
        let other_photo = placement(&b, other).photo_index;

        let mut replaced = b.clone();
        apply(
            &mut replaced,
            &BookEdit::ReplacePhoto {
                placement: a,
                photo: other_photo,
            },
            &lib,
            &ps,
            &w,
        )
        .unwrap();
        let mut swapped = b.clone();
        apply(&mut swapped, &BookEdit::SwapPhotos { a, b: other }, &lib, &ps, &w).unwrap();

        assert_ne!(replaced, b);
        assert_eq!(replaced, swapped);
    }

    #[test]
    fn edit_replace_refuses_a_locked_opening_an_unknown_photo_and_the_photo_already_there() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w = Weights::default();
        let extra = with_left_out(&mut ps);
        let at = PlacementRef { page: 2, z: 1 };
        let before = b.clone();

        assert_eq!(
            apply(
                &mut b,
                &BookEdit::ReplacePhoto {
                    placement: at,
                    photo: ps.len()
                },
                &lib,
                &ps,
                &w
            ),
            Err(EditError::NoSuchPhoto(ps.len()))
        );
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::ReplacePhoto {
                    placement: at,
                    photo: placement(&before, at).photo_index
                },
                &lib,
                &ps,
                &w
            ),
            Err(EditError::SamePlacement)
        );
        b.controls.entry(opening_of(&b, 1)).or_default().locked = true;
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::ReplacePhoto {
                    placement: at,
                    photo: extra
                },
                &lib,
                &ps,
                &w
            ),
            Err(EditError::Locked(1))
        );
        b.controls.clear();
        assert_eq!(b, before);
    }

    #[test]
    fn edit_replace_refuses_a_left_out_photo_that_would_break_a_hard_constraint() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let extra = with_left_out(&mut ps);
        ps[extra].width = 300;
        ps[extra].height = 400;
        let at = PlacementRef { page: 2, z: 1 };
        let before = b.clone();

        assert_eq!(
            apply(
                &mut b,
                &BookEdit::ReplacePhoto {
                    placement: at,
                    photo: extra
                },
                &lib,
                &ps,
                &Weights::default()
            ),
            Err(EditError::SwapRejected {
                placement: at,
                reason: Rejection::TooLowResolution
            })
        );
        assert_eq!(b, before);
    }

    /// `SetPrintSpec` really goes through the one door, on a real 20-page
    /// book rather than `reprint`'s two-placement fixture.
    ///
    /// The mutation is the arm returning `Ok(())` without calling `reprint`,
    /// which every test in `book::reprint` survives because none of them
    /// reaches `apply`. A size change that silently did nothing would leave
    /// the panel showing the new numbers over the old book.
    ///
    /// It asserts the whole book moved, not just that the call succeeded: the
    /// new spec is stored, every crop is recut, and the layout it was recut
    /// for is untouched.
    #[test]
    fn edit_set_print_spec_reprints_the_whole_book_through_the_one_door() {
        let ps = photos(25);
        let mut b = book(&ps);
        let before = b.clone();
        let lib = frozen_library();

        let r = apply(
            &mut b,
            &BookEdit::SetPrintSpec { spec: crate::print_spec::odd_spec() },
            &lib,
            &ps,
            &Weights::default(),
        );

        assert_eq!(r, Ok(()), "a size change is never refused -- see book::reprint");
        assert_eq!(b.spec, crate::print_spec::odd_spec());
        assert_eq!(b.seed, before.seed);
        assert_eq!(b.pages.len(), before.pages.len());

        let mut recut = 0;
        for (page, orig) in b.pages.iter().zip(&before.pages) {
            assert_eq!(page.template_id, orig.template_id, "a resize must not re-pick a template");
            for (pl, was) in page.placements.iter().zip(&orig.placements) {
                assert_eq!(pl.slot_rect, was.slot_rect, "a resize must not move a slot");
                assert_eq!(pl.photo_index, was.photo_index);
                if pl.crop != was.crop {
                    recut += 1;
                }
            }
        }
        // Every one of them, not merely "some": 8.0 x 10.0 is portrait where
        // Pixajoy is landscape, so no slot in the book keeps its printed shape.
        let total: usize = b.pages.iter().map(|p| p.placements.len()).sum();
        assert!(total > 0);
        assert_eq!(recut, total, "{recut} of {total} placements were recut");
    }

    /// One answer per photo, in the order `photo_index` counts: the crop the
    /// slot would give it and whether a hard constraint refuses it. Landscape
    /// and portrait alternate, so an answer at the wrong index has the wrong
    /// crop.
    #[test]
    fn edit_slot_candidates_answer_for_every_photo_in_photo_order() {
        let mut ps = photos(25);
        let b = book(&ps);
        let lib = frozen_library();
        ps[7].width = 300;
        ps[7].height = 200;
        let at = PlacementRef { page: 2, z: 1 };
        let (pi, si) = locate(&b, at).unwrap();
        let (slot, _) = placement_slot(&b, &lib, pi, si);

        let candidates = slot_candidates(&b, &lib, &ps, at).unwrap();

        assert_eq!(candidates.len(), ps.len());
        for (i, c) in candidates.iter().enumerate() {
            assert_eq!(c.crop, choose_crop(&ps[i], slot_aspect(&pixajoy_spec(), &slot)), "photo {i}");
        }
        assert_eq!(candidates[7].refused, Some(Rejection::TooLowResolution));
        assert_eq!(candidates[8].refused, None);
        assert_ne!(candidates[7].crop, candidates[8].crop, "fixture: neighbours differ in shape");
        assert_eq!(
            slot_candidates(&b, &lib, &ps, PlacementRef { page: 2, z: 9 }),
            Err(EditError::NoSuchPlacement(PlacementRef { page: 2, z: 9 }))
        );
    }

    /// What `slotCandidates` in `app/types/preview.ts` reads.
    #[test]
    fn edit_slot_candidate_serialises_camel_case_reasons() {
        let refused = SlotCandidate {
            crop: Rect::new(0.0, 0.125, 1.0, 0.75),
            refused: Some(Rejection::FaceInSafeMargin),
        };
        assert_eq!(
            serde_json::to_value(&refused).unwrap(),
            serde_json::json!({"crop": {"x": 0.0, "y": 0.125, "w": 1.0, "h": 0.75}, "refused": "faceInSafeMargin"})
        );
        let fine = SlotCandidate { refused: None, ..refused };
        assert_eq!(serde_json::to_value(&fine).unwrap()["refused"], serde_json::Value::Null);
    }

    // --- crop -----------------------------------------------------------------

    /// The window moves where the user put it, keeps the slot's shape whatever
    /// height the webview thinks, and a window that would leave the photo is
    /// refused with the book untouched.
    #[test]
    fn edit_set_crop_moves_the_window_and_derives_its_height_from_the_slot() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w_ = Weights::default();
        let at = PlacementRef { page: 2, z: 1 };
        let (pi, si) = locate(&b, at).unwrap();
        let before = b.pages[pi].placements[si].crop;
        assert!(
            before.w < 1.0 || before.h < 1.0,
            "fixture: the crop must have room to move"
        );
        let w = before.w * 0.8;

        apply(
            &mut b,
            &BookEdit::SetCrop {
                placement: at,
                x: 0.1,
                y: 0.05,
                w,
            },
            &lib,
            &ps,
            &w_,
        )
        .unwrap();

        let after = b.pages[pi].placements[si].crop;
        assert_eq!((after.x, after.y, after.w), (0.1, 0.05, w));
        let ratio = |r: Rect| r.w / r.h;
        assert!(
            (ratio(after) - ratio(before)).abs() < 1e-9,
            "the shape is the slot's: {after:?} vs {before:?}"
        );

        let snapshot = b.clone();
        let err = apply(
            &mut b,
            &BookEdit::SetCrop {
                placement: at,
                x: 0.5,
                y: 0.0,
                w: 0.8,
            },
            &lib,
            &ps,
            &w_,
        );
        assert_eq!(err, Err(EditError::CropOutOfBounds(at)));
        let err = apply(
            &mut b,
            &BookEdit::SetCrop {
                placement: at,
                x: 0.0,
                y: 0.0,
                w: 0.001,
            },
            &lib,
            &ps,
            &w_,
        );
        assert_eq!(err, Err(EditError::CropOutOfBounds(at)));
        assert_eq!(b, snapshot);
    }

    #[test]
    fn edit_set_crop_refuses_a_window_that_cuts_a_face_or_is_locked() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w_ = Weights::default();
        let at = PlacementRef { page: 2, z: 1 };
        let (pi, si) = locate(&b, at).unwrap();
        let victim = b.pages[pi].placements[si].photo_index;
        ps[victim].faces = vec![Face {
            box_: Rect::new(0.4, 0.4, 0.2, 0.2),
            capture_quality: None,
        }];
        let snapshot = b.clone();

        // A window in the top-left corner, a quarter of the photo wide, cannot
        // contain a face centred at (0.5, 0.5).
        let err = apply(
            &mut b,
            &BookEdit::SetCrop {
                placement: at,
                x: 0.0,
                y: 0.0,
                w: 0.25,
            },
            &lib,
            &ps,
            &w_,
        );
        assert!(
            matches!(
                err,
                Err(EditError::CropRejected {
                    reason: Rejection::FaceClipped | Rejection::TooLowResolution,
                    ..
                })
            ),
            "{err:?}"
        );
        assert_eq!(b, snapshot);

        apply(
            &mut b,
            &BookEdit::SetLocked {
                opening: 1,
                locked: true,
            },
            &lib,
            &ps,
            &w_,
        )
        .unwrap();
        let err = apply(
            &mut b,
            &BookEdit::SetCrop {
                placement: at,
                x: 0.3,
                y: 0.3,
                w: 0.4,
            },
            &lib,
            &ps,
            &w_,
        );
        assert_eq!(err, Err(EditError::Locked(1)));
    }

    // --- slot -----------------------------------------------------------------

    /// The slot moves and resizes where the user put it and the photo is
    /// re-cropped for its new shape; a rect off the page, a sliver, or one on
    /// top of a neighbour is refused with the book untouched.
    #[test]
    fn edit_set_slot_moves_the_slot_and_recrops_the_photo_for_its_new_shape() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w_ = Weights::default();
        let (page_number, first, second) = b
            .pages
            .iter()
            .find(|p| p.placements.len() >= 2)
            .map(|p| (p.number, p.placements[0].clone(), p.placements[1].clone()))
            .expect("fixture: a page with two placements");
        let at = PlacementRef {
            page: page_number,
            z: first.z,
        };
        // Half the width at the same height: a different SHAPE, so the crop
        // has to change (halving both would keep the aspect and the crop).
        let target = Rect::new(
            first.slot_rect.x,
            first.slot_rect.y,
            first.slot_rect.w * 0.5,
            first.slot_rect.h,
        );

        apply(
            &mut b,
            &BookEdit::SetSlot {
                placement: at,
                rect: target,
            },
            &lib,
            &ps,
            &w_,
        )
        .unwrap();

        let (pi, si) = locate(&b, at).unwrap();
        let moved = &b.pages[pi].placements[si];
        assert_eq!(moved.slot_rect, target);
        assert_ne!(
            moved.crop, first.crop,
            "a different shape needs a different crop"
        );
        let photo = &ps[moved.photo_index];
        let crop_aspect =
            (moved.crop.w * photo.width as f64) / (moved.crop.h * photo.height as f64);
        assert!(
            (crop_aspect - b.spec.page_aspect(&target)).abs() < 1e-9,
            "the crop has the slot's real aspect"
        );

        let snapshot = b.clone();
        for bad in [
            Rect::new(0.9, 0.1, 0.3, 0.3),
            Rect::new(0.1, 0.1, 0.01, 0.3),
            Rect::new(-0.2, 0.1, 0.3, 0.3),
            Rect::new(f64::NAN, 0.1, 0.3, 0.3),
        ] {
            assert_eq!(
                apply(
                    &mut b,
                    &BookEdit::SetSlot {
                        placement: at,
                        rect: bad
                    },
                    &lib,
                    &ps,
                    &w_
                ),
                Err(EditError::SlotOutOfBounds(at)),
                "{bad:?}"
            );
        }
        let onto = second.slot_rect;
        assert_eq!(
            apply(
                &mut b,
                &BookEdit::SetSlot {
                    placement: at,
                    rect: onto
                },
                &lib,
                &ps,
                &w_
            ),
            Err(EditError::SlotOverlaps {
                placement: at,
                other: PlacementRef {
                    page: page_number,
                    z: second.z
                }
            })
        );
        assert_eq!(b, snapshot);
    }

    /// A slot the user moved is what later edits see: a swap into it crops for
    /// ITS rect, not the template's, and a slot dragged to the canvas edge
    /// bleeds there. Also: a tiny slot refuses a photo below 200 DPI.
    #[test]
    fn edit_moved_slot_governs_later_edits_and_reads_bleed_off_its_own_edges() {
        let mut ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let w_ = Weights::default();
        let at = PlacementRef { page: 2, z: 1 };
        let (pi, si) = locate(&b, at).unwrap();
        let side = b.pages[pi].side;

        let flush = match side {
            Side::Left => Rect::new(0.0, 0.0, 0.4, 0.5),
            Side::Right => Rect::new(0.6, 0.0, 0.4, 0.5),
        };
        let others: Vec<PlacementRef> = b.pages[pi]
            .placements
            .iter()
            .skip(1)
            .map(|p| PlacementRef { page: 2, z: p.z })
            .collect();
        for (k, other) in others.iter().enumerate() {
            let parked = Rect::new(0.45, 0.6 + 0.1 * k as f64, 0.1, 0.08);
            apply(
                &mut b,
                &BookEdit::SetSlot {
                    placement: *other,
                    rect: parked,
                },
                &lib,
                &ps,
                &w_,
            )
            .unwrap();
        }
        apply(
            &mut b,
            &BookEdit::SetSlot {
                placement: at,
                rect: flush,
            },
            &lib,
            &ps,
            &w_,
        )
        .unwrap();
        let (slot, _) = placement_slot(&b, &lib, pi, si);
        let expected_outer = if side == Side::Left {
            BleedEdge::Left
        } else {
            BleedEdge::Right
        };
        assert_eq!(slot.bleed, vec![expected_outer, BleedEdge::Top]);

        let other = PlacementRef { page: 4, z: 1 };
        apply(
            &mut b,
            &BookEdit::SwapPhotos { a: at, b: other },
            &lib,
            &ps,
            &w_,
        )
        .unwrap();
        let (pi2, si2) = locate(&b, at).unwrap();
        let swapped = b.pages[pi2].placements[si2].clone();
        let photo = &ps[swapped.photo_index];
        let crop_aspect =
            (swapped.crop.w * photo.width as f64) / (swapped.crop.h * photo.height as f64);
        assert!((crop_aspect - b.spec.page_aspect(&flush)).abs() < 1e-9);

        apply(
            &mut b,
            &BookEdit::SetCrop {
                placement: at,
                x: 0.0,
                y: 0.0,
                w: swapped.crop.w * 0.9,
            },
            &lib,
            &ps,
            &w_,
        )
        .unwrap();

        ps[swapped.photo_index].width = 300;
        ps[swapped.photo_index].height = 200;
        let err = apply(
            &mut b,
            &BookEdit::SetSlot {
                placement: at,
                rect: Rect::new(flush.x, 0.1, 0.4, 0.5),
            },
            &lib,
            &ps,
            &w_,
        );
        assert_eq!(
            err,
            Err(EditError::SlotRejected {
                placement: at,
                reason: Rejection::TooLowResolution
            })
        );
    }

    // --- wire ---------------------------------------------------------------------

    /// The literal shapes the webview sends, read from the wire fixture
    /// `tests/preview.test.ts` builds the SAME values from the TypeScript
    /// type -- so a renamed tag or field fails one suite against a fixture
    /// that did not move. Asserted from text, not a round trip, because a
    /// round trip through the same enum cannot detect a rename.
    #[test]
    fn edit_deserialises_the_camel_case_tagged_shape_the_webview_sends() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/wire/book-edits.json");
        let text = std::fs::read_to_string(&path).expect("wire fixture");
        let parsed: Vec<BookEdit> = serde_json::from_str(&text).expect("every fixture edit parses");
        assert_eq!(
            parsed,
            vec![
                BookEdit::Regenerate { opening: 3 },
                BookEdit::RejectTemplate { opening: 0 },
                BookEdit::SetTemplate {
                    opening: 2,
                    template_id: "07-two-up-symmetric-margin".into()
                },
                BookEdit::SetLocked {
                    opening: 1,
                    locked: true
                },
                BookEdit::Shuffle,
                BookEdit::SwapPhotos {
                    a: PlacementRef { page: 2, z: 1 },
                    b: PlacementRef { page: 5, z: 2 }
                },
                BookEdit::SetCrop {
                    placement: PlacementRef { page: 3, z: 2 },
                    x: 0.125,
                    y: 0.0,
                    w: 0.75
                },
                BookEdit::SetSlot {
                    placement: PlacementRef { page: 3, z: 2 },
                    rect: Rect::new(0.1, 0.2, 0.3, 0.4),
                },
                BookEdit::ReplacePhoto {
                    placement: PlacementRef { page: 3, z: 1 },
                    photo: 12,
                },
                // `odd_spec`'s numbers, every one distinct from every other,
                // so a transposed field in the panel's payload lands here
                // rather than as a quietly wrong book.
                BookEdit::SetPrintSpec { spec: crate::print_spec::odd_spec() },
                BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(7) },
                BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: None },
                BookEdit::SetCoverCrop { side: CoverSide::Back, x: 0.125, y: 0.0625, w: 0.5 },
                BookEdit::SetSpineColour { rgb: Rgb { r: 0x1a, g: 0x2b, b: 0x3c } },
            ]
        );
        // The panel sends a spec, and it goes through `TryFrom<RawPrintSpec>`
        // on the way in like every other route into a `PrintSpec`. A page
        // that leaves no safe area is refused at the wire, not inside
        // `reprint`, which has no way to report it -- `apply`'s arm returns
        // `Ok(())` unconditionally on purpose.
        assert!(
            serde_json::from_str::<BookEdit>(
                r#"{"kind":"setPrintSpec","spec":{"pageWIn":0.5,"pageHIn":10.0,"bleedIn":0.25,
                     "gutterIn":0.4,"safeMarginIn":0.05,"minDpi":150.0,"warnDpi":220.0}}"#
            )
            .is_err(),
            "an impossible geometry must not deserialise into an edit"
        );
        assert!(serde_json::from_str::<BookEdit>(r#"{"kind":"burn"}"#).is_err());
        assert!(
            serde_json::from_str::<BookEdit>(
                r#"{"kind":"setTemplate","opening":2,"template_id":"x"}"#
            )
            .is_err(),
            "snake_case is not the wire"
        );
    }

    /// A book saved before controls existed has no `controls` key and must
    /// load; a book with controls must round-trip them; a book with none must
    /// serialise without the key, so the golden did not move.
    #[test]
    fn edit_controls_are_optional_on_the_wire_and_omitted_when_empty() {
        let ps = photos(25);
        let mut b = book(&ps);
        let plain = serde_json::to_string(&b).unwrap();
        assert!(
            !plain.contains("\"controls\""),
            "an unedited book must not grow a key"
        );
        // Crops are irrational f64s and serde_json's default parser lands up
        // to one ULP off on the way back, so the comparison is on the fields
        // this test is about, not the whole book.
        let reloaded: Book = serde_json::from_str(&plain).unwrap();
        assert!(reloaded.controls.is_empty());
        assert_eq!(reloaded.pages.len(), b.pages.len());

        b.controls.insert(
            3,
            OpeningControls {
                locked: true,
                rejected: vec!["08-two-up-symmetric-bleed-outer".into()],
                rerolls: 2,
            },
        );
        let edited = serde_json::to_string(&b).unwrap();
        assert!(edited.contains(r#""controls":{"3":{"locked":true,"rejected":["08-two-up-symmetric-bleed-outer"],"rerolls":2}}"#), "{edited}");
        let reloaded: Book = serde_json::from_str(&edited).unwrap();
        assert_eq!(reloaded.controls, b.controls);
    }

    #[test]
    fn edit_alternatives_exclude_the_current_and_rejected_templates() {
        let ps = photos(25);
        let mut b = book(&ps);
        let lib = frozen_library();
        let o = spread_with(&b, 3);
        let current = template_of(&b, o);
        let alts = alternatives(&b, &lib, o);
        assert!(!alts.contains(&current));
        assert_eq!(alts.len(), 1, "13 and 35 are the only 3-ups: {alts:?}");
        b.controls
            .entry(o)
            .or_default()
            .rejected
            .push(alts[0].clone());
        assert!(alternatives(&b, &lib, o).is_empty());
        assert!(
            alternatives(&b, &lib, 99).is_empty(),
            "no such opening, no alternatives"
        );
    }

    // --- the cover ----------------------------------------------------------

    fn edit(b: &mut Book, ps: &[Photo], e: BookEdit) -> Result<(), EditError> {
        apply(b, &e, &frozen_library(), ps, &Weights::default())
    }

    fn cover_crop(ps: &[Photo], i: usize) -> Rect {
        choose_crop(&ps[i], pixajoy_spec().cover_aspect())
    }

    /// Photo 2 is portrait and photo 1 landscape, so the two automatic crops
    /// differ and a crop copied from the wrong photo shows.
    #[test]
    fn set_cover_photo_puts_any_photo_on_that_side_with_its_automatic_crop() {
        let ps = photos(25);
        let mut b = book(&ps);
        let front = b.cover.front;
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: Some(2) }).unwrap();
        assert_eq!(b.cover.back, Some(CoverPhoto { photo_index: 2, crop: cover_crop(&ps, 2) }));
        assert_eq!(b.cover.front, front, "the other side is untouched");

        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(1) }).unwrap();
        assert_eq!(b.cover.front, Some(CoverPhoto { photo_index: 1, crop: cover_crop(&ps, 1) }));
        assert_ne!(cover_crop(&ps, 1), cover_crop(&ps, 2), "fixture: the crops must differ");
    }

    #[test]
    fn set_cover_photo_none_clears_only_that_side() {
        let ps = photos(25);
        let mut b = book(&ps);
        let front = b.cover.front;
        assert!(front.is_some() && b.cover.back.is_some(), "fixture: both sides filled");
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: None }).unwrap();
        assert_eq!(b.cover.back, None);
        assert_eq!(b.cover.front, front);
    }

    #[test]
    fn set_cover_photo_refuses_a_photo_the_book_does_not_have() {
        let ps = photos(25);
        let mut b = book(&ps);
        let before = b.clone();
        let err = edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(25) });
        assert_eq!(err, Err(EditError::NoSuchPhoto(25)));
        assert_eq!(b, before);
    }

    /// The face is at the top of a landscape photo, whose cover crop is the
    /// full height, so it lands in the top wrap. Too few pixels is the other
    /// refusal a photo can earn on the cover by itself.
    #[test]
    fn set_cover_photo_refuses_a_face_in_the_wrap_or_too_few_pixels() {
        let mut ps = photos(25);
        ps[3].faces = vec![Face { box_: Rect::new(0.45, 0.01, 0.08, 0.05), capture_quality: None }];
        ps[4].width = 1000;
        ps[4].height = 750;
        let mut b = book(&ps);
        let before = b.clone();
        assert_eq!(
            edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: Some(3) }),
            Err(EditError::CoverRejected { side: CoverSide::Back, reason: Rejection::FaceInSafeMargin })
        );
        assert_eq!(
            edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(4) }),
            Err(EditError::CoverRejected { side: CoverSide::Front, reason: Rejection::TooLowResolution })
        );
        assert_eq!(b, before, "a refused edit changes nothing");
    }

    /// Landscape 4:3 onto Pixajoy's 1.175 panel: `h = w * 4/3 / 1.175`.
    #[test]
    fn set_cover_crop_derives_the_height_from_the_panel() {
        let ps = photos(25);
        let mut b = book(&ps);
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(0) }).unwrap();
        edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.1, y: 0.05, w: 0.8 })
            .unwrap();
        let crop = b.cover.front.unwrap().crop;
        let h = 0.8 * (4000.0 / 3000.0) / pixajoy_spec().cover_aspect();
        assert_eq!((crop.x, crop.y, crop.w), (0.1, 0.05, 0.8));
        assert!((crop.h - h).abs() < 1e-12, "h {} != {h}", crop.h);
        assert_eq!(b.cover.front.unwrap().photo_index, 0);
    }

    #[test]
    fn set_cover_crop_keeps_the_window_inside_the_photo() {
        let ps = photos(25);
        let mut b = book(&ps);
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Back, photo: Some(0) }).unwrap();
        let before = b.clone();
        // w 0.8 gives h ~0.908, so y 0.1 runs off the bottom.
        for (x, y, w) in [(0.3, 0.0, 0.8), (0.0, 0.1, 0.8), (-0.01, 0.0, 0.5), (0.0, 0.0, 0.01), (f64::NAN, 0.0, 0.5)] {
            assert_eq!(
                edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Back, x, y, w }),
                Err(EditError::CoverCropOutOfBounds(CoverSide::Back)),
                "{x} {y} {w}"
            );
        }
        assert_eq!(b, before);
    }

    #[test]
    fn set_cover_crop_needs_a_photo_on_that_side() {
        let ps = photos(25);
        let mut b = book(&ps);
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: None }).unwrap();
        assert_eq!(
            edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.0, y: 0.0, w: 0.5 }),
            Err(EditError::NoCoverPhoto(CoverSide::Front))
        );
    }

    /// The face at 0.20-0.25 of the photo: a window starting at 0.19 puts it
    /// 0.01 below the panel's top edge, in the wrap; one starting at 0.10
    /// shows it. Width 0.6 is 204 DPI over the 11.75" panel; 0.3 is
    /// 102.
    #[test]
    fn set_cover_crop_refuses_a_face_in_the_wrap_or_too_few_pixels() {
        let mut ps = photos(25);
        ps[0].faces = vec![Face { box_: Rect::new(0.40, 0.20, 0.05, 0.05), capture_quality: None }];
        let mut b = book(&ps);
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(0) }).unwrap();
        let before = b.clone();
        assert_eq!(
            edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.2, y: 0.19, w: 0.6 }),
            Err(EditError::CoverRejected { side: CoverSide::Front, reason: Rejection::FaceInSafeMargin })
        );
        assert_eq!(
            edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.3, y: 0.1, w: 0.3 }),
            Err(EditError::CoverRejected { side: CoverSide::Front, reason: Rejection::TooLowResolution })
        );
        assert_eq!(b, before);
        edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.2, y: 0.1, w: 0.6 }).unwrap();
        assert_eq!(b.cover.front.unwrap().crop.y, 0.1);
    }

    #[test]
    fn set_spine_colour_sets_it() {
        let ps = photos(25);
        let mut b = book(&ps);
        let rgb = Rgb::try_from("#12ab34".to_string()).unwrap();
        edit(&mut b, &ps, BookEdit::SetSpineColour { rgb }).unwrap();
        assert_eq!(b.cover.spine, rgb);
    }

    /// A size change recuts the cover for the new panel and keeps its
    /// photos; one that leaves the panel's shape alone keeps a hand crop.
    #[test]
    fn set_print_spec_recuts_the_cover_only_when_its_shape_changes() {
        let ps = photos(25);
        let mut b = book(&ps);
        edit(&mut b, &ps, BookEdit::SetCoverPhoto { side: CoverSide::Front, photo: Some(0) }).unwrap();
        edit(&mut b, &ps, BookEdit::SetCoverCrop { side: CoverSide::Front, x: 0.1, y: 0.05, w: 0.8 })
            .unwrap();
        let hand = b.cover.front.unwrap().crop;

        let p = pixajoy_spec();
        let same_shape = crate::print_spec::PrintSpec::try_from(crate::print_spec::RawPrintSpec {
            page_w_in: p.page_w_in(),
            page_h_in: p.page_h_in(),
            bleed_in: p.bleed_in(),
            gutter_in: p.gutter_in(),
            safe_margin_in: p.safe_margin_in(),
            min_dpi: 150.0,
            warn_dpi: 250.0,
            cover_wrap_in: p.cover_wrap_in(),
        })
        .unwrap();
        edit(&mut b, &ps, BookEdit::SetPrintSpec { spec: same_shape }).unwrap();
        assert_eq!(b.cover.front.unwrap().crop, hand, "a DPI change keeps a hand crop");

        let odd = crate::print_spec::odd_spec();
        let back = b.cover.back.unwrap().photo_index;
        edit(&mut b, &ps, BookEdit::SetPrintSpec { spec: odd }).unwrap();
        assert_eq!(b.cover.front.unwrap().crop, choose_crop(&ps[0], odd.cover_aspect()));
        assert_eq!(b.cover.back.unwrap().crop, choose_crop(&ps[back], odd.cover_aspect()));
        assert_eq!(b.cover.back.unwrap().photo_index, back);
    }

    #[test]
    fn a_cover_refusal_names_the_side_and_the_reason() {
        let e = EditError::CoverRejected { side: CoverSide::Back, reason: Rejection::FaceInSafeMargin };
        let text = e.to_string();
        assert!(text.contains("back cover") && text.contains("fold"), "{text}");
        assert!(EditError::NoCoverPhoto(CoverSide::Front).to_string().contains("front cover"));
    }
}

/// The smallest crop width accepted, as a fraction of the photo. Below this
/// the window is a handful of pixels and `rejects` would refuse it on DPI
/// anyway; refusing earlier gives a clearer message.
const MIN_CROP_W: f64 = 0.02;
const CROP_EPS: f64 = 1e-6;

/// Moves or resizes a placement's crop by hand.
///
/// The height is DERIVED from the slot's aspect and the photo's, exactly as
/// `choose_crop` derives it, so the window keeps the slot's shape whatever the
/// webview sends. The window must lie inside the photo, and the same hard
/// constraints the scorer enforces still hold: a face the new window would cut,
/// or a window too small to print at 200 DPI, is refused with the reason.
///
/// A hand crop is a property of THIS placement of THIS photo in THIS slot.
/// Regenerating the opening or swapping the photo away recomputes the crop
/// from scratch, because the slot it was chosen for no longer exists; that is
/// deliberate and documented in the manual.
fn set_crop(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    at: PlacementRef,
    x: f64,
    y: f64,
    w: f64,
) -> Result<(), EditError> {
    let (page_index, slot_index) = locate(book, at)?;
    let o = opening_of(book, page_index);
    if book.controls.get(&o).is_some_and(|c| c.locked) {
        return Err(EditError::Locked(o));
    }
    let (slot, side) = placement_slot(book, lib, page_index, slot_index);
    let photo = &photos[book.pages[page_index].placements[slot_index].photo_index];
    let h = w * photo.aspect() / slot_aspect(&book.spec, &slot);
    let crop = hand_crop(x, y, w, h).ok_or(EditError::CropOutOfBounds(at))?;
    if let Some(reason) = rejects(&book.spec, photo, &crop, &slot, side) {
        return Err(EditError::CropRejected {
            placement: at,
            reason,
        });
    }
    book.pages[page_index].placements[slot_index].crop = crop;
    Ok(())
}

/// A hand-set window, or `None` when it leaves the photo or is too small.
/// Within `CROP_EPS` of an edge is snapped onto it: a drag to the edge
/// arrives as a float a hair past it.
fn hand_crop(x: f64, y: f64, w: f64, h: f64) -> Option<Rect> {
    let inside = |v: f64| v.is_finite() && v >= -CROP_EPS;
    let ok = inside(x)
        && inside(y)
        && w.is_finite()
        && w >= MIN_CROP_W
        && h.is_finite()
        && x + w <= 1.0 + CROP_EPS
        && y + h <= 1.0 + CROP_EPS;
    ok.then(|| Rect::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0), w.min(1.0), h.min(1.0)))
}

/// A cover photo is not a placement, so it needs no slot, no lock and no
/// swap: the panel is fixed and the photo may repeat one in the book.
fn set_cover_photo(
    book: &mut Book,
    photos: &[Photo],
    side: CoverSide,
    photo: Option<usize>,
) -> Result<(), EditError> {
    let next = match photo {
        None => None,
        Some(i) => {
            let p = photos.get(i).ok_or(EditError::NoSuchPhoto(i))?;
            let crop = cover::crop_for(&book.spec, p, side)
                .map_err(|reason| EditError::CoverRejected { side, reason })?;
            Some(CoverPhoto { photo_index: i, crop })
        }
    };
    *book.cover.side_mut(side) = next;
    Ok(())
}

fn set_cover_crop(
    book: &mut Book,
    photos: &[Photo],
    side: CoverSide,
    x: f64,
    y: f64,
    w: f64,
) -> Result<(), EditError> {
    let current = book.cover.side(side).ok_or(EditError::NoCoverPhoto(side))?;
    let photo =
        photos.get(current.photo_index).ok_or(EditError::NoSuchPhoto(current.photo_index))?;
    let h = w * photo.aspect() / book.spec.cover_aspect();
    let crop = hand_crop(x, y, w, h).ok_or(EditError::CoverCropOutOfBounds(side))?;
    if let Some(reason) = cover::rejects(&book.spec, photo, &crop, side) {
        return Err(EditError::CoverRejected { side, reason });
    }
    if let Some(c) = book.cover.side_mut(side) {
        c.crop = crop;
    }
    Ok(())
}

/// The smallest slot accepted, as a fraction of the page each way. Smaller
/// than this is a sliver nobody meant to print.
const MIN_SLOT: f64 = 0.05;
/// Two slots may touch; they may not overlap by more than this, the same
/// tolerance the template validator allows for authored rounding.
const OVERLAP_EPS: f64 = 0.0005;

/// Moves or resizes a slot on the page and re-crops its photo for the new
/// shape.
///
/// The rect stays on the page -- it may reach the canvas edge, which prints
/// as bleed, but not pass it -- and may not overlap another slot on the same
/// page, which the template validator forbids for authored layouts and the
/// editor forbids for the same reason. The photo's crop is recomputed with
/// `choose_crop` for the slot's new aspect and then checked against the same
/// hard constraints as every other edit, with the slot's bleed read off the
/// new rect. A hand crop made before the slot moved is replaced: it was
/// chosen for a shape that no longer exists.
fn set_slot(
    book: &mut Book,
    lib: &Library,
    photos: &[Photo],
    at: PlacementRef,
    rect: Rect,
) -> Result<(), EditError> {
    let (page_index, slot_index) = locate(book, at)?;
    let o = opening_of(book, page_index);
    if book.controls.get(&o).is_some_and(|c| c.locked) {
        return Err(EditError::Locked(o));
    }
    let finite = [rect.x, rect.y, rect.w, rect.h]
        .iter()
        .all(|v| v.is_finite());
    if !finite
        || rect.w < MIN_SLOT
        || rect.h < MIN_SLOT
        || rect.x < -CROP_EPS
        || rect.y < -CROP_EPS
        || rect.right() > 1.0 + CROP_EPS
        || rect.bottom() > 1.0 + CROP_EPS
    {
        return Err(EditError::SlotOutOfBounds(at));
    }
    let rect = Rect::new(
        rect.x.max(0.0),
        rect.y.max(0.0),
        rect.w.min(1.0),
        rect.h.min(1.0),
    );
    let page = &book.pages[page_index];
    for (i, other) in page.placements.iter().enumerate() {
        if i == slot_index {
            continue;
        }
        let overlaps = rect
            .intersect(&other.slot_rect)
            .is_some_and(|r| r.w > OVERLAP_EPS && r.h > OVERLAP_EPS);
        if overlaps {
            return Err(EditError::SlotOverlaps {
                placement: at,
                other: PlacementRef {
                    page: page.number,
                    z: other.z,
                },
            });
        }
    }
    let role = slot_for(book, lib, page_index)
        .ok()
        .and_then(|layout| layout.slots.get(slot_index))
        .map(|s| s.role)
        .unwrap_or(Role::Support);
    let slot = Slot {
        rect,
        role,
        bleed: bleed_edges(&rect, page.side),
        aspect_pref: (book.spec.page_aspect(&rect), book.spec.page_aspect(&rect)),
    };
    let photo = &photos[page.placements[slot_index].photo_index];
    let crop = choose_crop(photo, slot_aspect(&book.spec, &slot));
    if let Some(reason) = rejects(&book.spec, photo, &crop, &slot, page.side) {
        return Err(EditError::SlotRejected {
            placement: at,
            reason,
        });
    }
    let placement = &mut book.pages[page_index].placements[slot_index];
    placement.slot_rect = rect;
    placement.crop = crop;
    Ok(())
}
