# Cover, places and people: design

**Date:** 2026-09-20
**Status:** approved in conversation 2026-09-20; §4 (people) is gated on the face-identity
experiment in §4.1.

Closes the three entries `docs/PROJECT-STATUS.md` lists under "Also explicitly deferred":
the cover, GPS location clustering and same-person grouping.

## 1. Rulings (from the user, 2026-09-20)

| Question | Ruling |
|---|---|
| Which features are optional | Places and people are per-book toggles, **off by default**. The cover always exists and may be left empty. |
| What the cover is | A **front photo and a back photo**, chosen separately. The spine is a plain colour. |
| What people grouping does | All three: person chips on the contact sheet (filter, include or exclude a person's photos), book assembly balances people, and the user can name people. |
| Place names | Named with Apple's `CLGeocoder`. Coordinates, never pixels, go to Apple, and only when the Places toggle is on. |

## 2. Book options

One struct, saved on the book and on the draft, read by every step that changes behaviour.

`BookOptions { places: bool, people: bool }` sits on `Book` as `options`, with
`#[serde(default)]`. A book saved before this has neither key and loads with both off,
which is exactly how it was made. It is passed to `recommend_book` and `generate_book`
beside `spec`, and saved on `SavedDraft` beside `spec`.

The draft screen's inspector (`GenerateBook.vue`) shows two switches under Print size:
**Split chapters by place** and **Group by people**. Each switch is disabled with a reason
when the analysed set has nothing for it (no photo carries GPS; nobody recurs).

## 3. Places

### 3.1 Data

`Photo` gains `location: Option<LatLon>`, read from `exif.latitude`/`exif.longitude`.
The sidecar already emits both. A photo with no GPS has `None`, which is ordinary, not
malformed. No sidecar or analyzer-version change is needed.

### 3.2 Chapters

Chapters stay `event_cluster`. `finalize_photos` keeps stamping the time-only events it
stamps today. With Places on, `book::chapter::chapters(photos, rule)` re-derives them
before cull and pack. It is pure, and every caller (`recommend_book`, `generate_book`,
the contact sheet) goes through it, so the sheet's chapter headers and the book agree.

The rule: walk photos in capture order. Start a new chapter on a time gap over
`EVENT_GAP_SECONDS` (unchanged), **or** when the photo is more than `PLACE_SPLIT_KM` from
its chapter's running centroid **and** the next located photo is too. The second
condition stops one stray GPS fix from cutting a chapter. A photo with no location
never splits a chapter; it joins the one it falls in by time. `PLACE_SPLIT_KM` is
calibrated on real GPS photos before it is committed (see the plan). A calibration that
cannot be done on real data is reported, not guessed.

### 3.3 Names

A new sidecar request kind, `geocode`, takes a chapter centroid list and answers one
locality name per entry (`CLGeocoder.reverseGeocodeLocation`, `locality` falling back to
`administrativeArea`, then `country`). Rust caches answers in SQLite keyed by the
coordinate rounded to 2 decimal places (about 1 km), so re-opening a draft asks nothing.
A failure (offline, rate limited) leaves the name blank. It never fails the chapters.

Names show on the contact sheet's chapter headers. They are **not** sent to the chat
agent. `agent/view.rs` keeps its opaque "location B" labels, per design §9.1.

## 4. People

### 4.1 Gate: can we tell people apart on-device?

Apple's public Vision API has no face-recognition model, and InsightFace/ArcFace is ruled
out on licence. Candidate: `VNGenerateImageFeaturePrintRequest` on an aligned face crop.
An experiment on the real Iceland, Bali, Japan and Vietnam photos measures same-person
versus different-person distances against a hand-labelled ground truth. **§4.2 onward is
built only if that separates people.** If it does not, people grouping is reported as
blocked, with the numbers, and the toggle is not shipped.

### 4.2 Data

The sidecar adds `identity` (the face-crop print, base64, same encoding as
`featurePrint`) to each `FaceObservation`, for faces that pass the experiment's quality
and size filter. This changes the analysis record, so `ANALYZER_VERSION` bumps and the
cache re-analyses.

`finalize_photos` clusters every face in the run into people
(`cluster::people`, pure) and stamps `people: [personId]` on each photo. Only a person
seen in `PERSON_MIN_PHOTOS` or more photos gets an id; a stranger in one shot is nobody.
The summary gains `persons: [{ id, photoCount, face: { hash, box } }]`, where `face`
is the member face used for the chip thumbnail.

### 4.3 Names

Names are per book, stored on the draft and on `Book` as `person_names`. A person id is
only stable for one analysed set, so a name is keyed by a member face
(`hash` + face index). After re-analysis, the name follows the new person that contains
that face.

### 4.4 Contact sheet

With People on, a row of chips sits above the sheet: the face thumbnail (the photo's
thumbnail, CSS-cropped to the face box), the name or "Person N", and the photo count.
Clicking a chip filters the sheet to that person. Its menu offers **Include all**,
**Exclude all** and **Name…**. Include and exclude write the same overrides the tile
toggles write.

### 4.5 Balance

With People on, `pack::select` takes each person's best photo before any moment's second
pick, so every recurring person appears. It also caps any one person at
`PERSON_MAX_SHARE` of the photos it picks automatically. Explicit `Include`s are exempt,
as they are from every other trim.

## 5. Cover

### 5.1 Geometry

`PrintSpec` gains `cover_wrap_in`, the band of the cover photo that wraps round the board,
defaulting to 0.75". It is validated like the other fields and shown in the Print size
panel. A cover panel is one trim page plus the wrap on its three outer edges. Its
aspect is `(trim_w + wrap) / (trim_h + 2 × wrap)`. The spine is not part of either photo,
so its width does not affect the crops. The preview draws it at a nominal width.

### 5.2 Model

`Book` gains `cover: Cover { front: Option<CoverPhoto>, back: Option<CoverPhoto>,
spine: Rgb }`, `#[serde(default)]`. `CoverPhoto { photo_index, crop }` works like a
`Placement` without a slot. An old book loads with an empty cover.

`assemble` fills it. The front is the kept photo with the best aesthetic score whose
cover crop keeps every face out of the wrap band. The back is the next best from a
different chapter. The spine is the front photo's dominant palette colour. The cover
photos may also appear inside; a book cover usually repeats an inside photo.

### 5.3 Edits

`BookEdit::SetCoverPhoto { side, photo }` (with `photo: None` to clear),
`BookEdit::SetCoverCrop { side, x, y, w }` and `BookEdit::SetSpineColour { rgb }`.
The first two re-run the face-in-wrap check `score::rejects` already does for the
safe margin. `BookEdit::SetPrintSpec` recuts cover crops the way `reprint` recuts
page crops. The chat agent gets none of these.

### 5.4 Preview and export

`BookLayout` gains `cover`. `BookPreview.vue` shows the cover before page 1 as back,
spine, front, with the wrap band shaded. Clicking a panel opens `ReplacePhotoDialog`
against the cover slot.

Export writes `cover-front` and `cover-back` through the same `ExportItem` path as the
interior. Pre-flight checks their resolution against `min_dpi` over the panel's size.
The manifest records the spine colour as hex.

## 6. Privacy

- No new data reaches the chat agent. `agent_view_contains_no_…` is extended to assert
  no place name and no person name or id appears in its output.
- The geocoder is the one new outbound call. It carries centroid coordinates only, only
  with Places on, and the manual says so.

## 7. Verification

Each part is verified on real photos, not fixtures alone:

- Places: the chapter split on a real GPS-tagged trip, with the chapters listed and read.
- People: the experiment's labelled distances, then the chips on a real folder, looked at.
- Cover: an exported book's `cover-front`/`cover-back` opened and checked at the edges,
  and the preview screenshotted in light and dark.

Tests are mutation-checked per `CLAUDE.md`.

## 8. Out of scope

- A wrap-around single cover photo, text on the cover or spine, and a spine-width formula.
  The user chose front and back photos, and Pixajoy publishes no spine formula.
- Place names in the printed book. Pages carry no text yet.
- Merging events that return to the same place on different days.
