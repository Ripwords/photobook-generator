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

use crate::book::crop::choose_crop;
use crate::book::cull::Photo;
use crate::book::pace::{half_id, place, rebuild, single_fit, tie_break, Book, Page};
use crate::book::score::{best_spread, rejects, slot_aspect, Rejection};
use crate::geometry::Side;
use crate::templates::{Library, PageLayout, Slot, SpreadTemplate, Weights};
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
    /// A page names a template the library no longer has.
    MissingLayout(String),
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
                 gutter or the margin, or printing below 200 DPI"
            ),
            Self::NoSuchPlacement(p) => write!(f, "there is no photo at {p}"),
            Self::SamePlacement => write!(f, "choose two different photos to swap"),
            Self::SwapRejected { placement, reason } => write!(
                f,
                "the photo moving to {placement} would {}",
                match reason {
                    Rejection::FaceClipped => "have a face cut by the slot's crop",
                    Rejection::FaceInGutter => "put a face in the gutter",
                    Rejection::FaceInSafeMargin => "put a face outside the safe margin",
                    Rejection::TooLowResolution => "print below 200 DPI at that size",
                }
            ),
            Self::CropRejected { reason, .. } => write!(
                f,
                "that crop would {}",
                match reason {
                    Rejection::FaceClipped => "cut a face",
                    Rejection::FaceInGutter => "put a face in the gutter",
                    Rejection::FaceInSafeMargin => "put a face outside the safe margin",
                    Rejection::TooLowResolution => "print below 200 DPI",
                }
            ),
            Self::CropOutOfBounds(_) => {
                write!(f, "the crop window has to stay inside the photo")
            }
            Self::MissingLayout(id) => write!(
                f,
                "this page uses template {id:?}, which is no longer in the library"
            ),
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
            .filter_map(|(id, l)| single_fit(l, indices, photos).map(|fit| (fit, id, l)))
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
            placements: place(layout, indices, photos),
        }]);
    }
    let eligible: Vec<&SpreadTemplate> = lib
        .spreads
        .iter()
        .filter(|t| candidates.contains(&t.id))
        .collect();
    let refs: Vec<&Photo> = indices.iter().map(|&i| &photos[i]).collect();
    let previous = previous_template(book, o);
    let (template, assignment, _) = best_spread(&eligible, &refs, previous.as_deref(), w, seed)
        .ok_or_else(|| rejected_all(candidates))?;
    Ok(rebuild(template, &assignment, indices, photos).to_vec())
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
    let fit = |page_index: usize, photo_index: usize, at: PlacementRef| {
        let layout = slot_for(book, lib, page_index)?;
        let slot: &Slot = layout
            .slots
            .get((at.z as usize).saturating_sub(1))
            .ok_or_else(|| EditError::MissingLayout(book.pages[page_index].template_id.clone()))?;
        let photo = &photos[photo_index];
        let crop = choose_crop(photo, slot_aspect(slot));
        match rejects(photo, &crop, slot, layout.side) {
            Some(reason) => Err(EditError::SwapRejected {
                placement: at,
                reason,
            }),
            None => Ok(crop),
        }
    };
    let crop_a = fit(pa, photo_b, a)?;
    let crop_b = fit(pb, photo_a, b)?;

    book.pages[pa].placements[ia].photo_index = photo_b;
    book.pages[pa].placements[ia].crop = crop_a;
    book.pages[pb].placements[ib].photo_index = photo_a;
    book.pages[pb].placements[ib].crop = crop_b;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::{Face, Overrides, PaletteColor};
    use crate::book::pace::assemble;
    use crate::geometry::Rect;

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
        }
    }

    fn photos(n: usize) -> Vec<Photo> {
        (0..n).map(photo).collect()
    }

    /// A 20-page book from 25 photos on the frozen library: the density swing
    /// cuts both 2-up and 3-up spreads, so both alternative sets are
    /// exercised. `spread_with` asserts the mix rather than assuming it.
    fn book(photos: &[Photo]) -> Book {
        assemble(
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
                    let crop = choose_crop(&ps[victim], slot_aspect(slot));
                    rejects(&ps[victim], &crop, slot, layout.side) == Some(Rejection::FaceClipped)
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
            ]
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
    let layout = slot_for(book, lib, page_index)?;
    let slot: &Slot = layout
        .slots
        .get((at.z as usize).saturating_sub(1))
        .ok_or_else(|| EditError::MissingLayout(book.pages[page_index].template_id.clone()))?;
    let photo = &photos[book.pages[page_index].placements[slot_index].photo_index];
    let h = w * photo.aspect() / slot_aspect(slot);
    let inside = |v: f64| v.is_finite() && v >= -CROP_EPS;
    if !(inside(x) && inside(y) && w.is_finite() && w >= MIN_CROP_W && h.is_finite())
        || x + w > 1.0 + CROP_EPS
        || y + h > 1.0 + CROP_EPS
    {
        return Err(EditError::CropOutOfBounds(at));
    }
    let crop =
        crate::geometry::Rect::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0), w.min(1.0), h.min(1.0));
    if let Some(reason) = rejects(photo, &crop, slot, layout.side) {
        return Err(EditError::CropRejected {
            placement: at,
            reason,
        });
    }
    book.pages[page_index].placements[slot_index].crop = crop;
    Ok(())
}
