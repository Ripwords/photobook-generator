/**
 * The read-only spread preview: turning the layout Rust already computed into
 * the handful of numbers the DOM needs to draw it.
 *
 * **This file recomputes nothing about the book.** Every rect here arrives
 * from `src-tauri/src/preview.rs`, which copies it straight off the `Book`
 * that `pace::assemble` produced and `project.rs` persisted, and every guide
 * constant is one `geometry.rs` shipped on the wire rather than one restated
 * here. That is deliberate: a preview whose job is to reveal defects -- a
 * crop that cuts a face, salient content sliding into the gutter, a page that
 * prints blank -- has to be showing the same geometry that was validated, or
 * it flatters the engine and the user is wrong at the printer.
 *
 * **Positioned DOM elements with CSS transforms, not a canvas.** Photos are
 * `<img>` with a transform for the crop; guides are absolutely positioned
 * overlays. Hit-testing, focus and accessibility come free and the geometry
 * is inspectable in devtools. Rendering is at roughly 1400 x 556 per spread;
 * print resolution is 6718 x 2668, which is 71.7 MB of canvas per spread and
 * well past what WKWebView will do.
 *
 * The functions live here rather than inside `BookPreview.vue` for the same
 * reason `applyAnalysisEvent` lives in `features.ts`: a component's template
 * is not unit-testable in this project, and the geometry is exactly where the
 * bugs would be. `tests/preview.test.ts` pins all of it against the same
 * `tests/fixtures/wire/book-layout.json` that Rust asserts itself against.
 */

/** Mirrors `geometry::Rect`, which serialises its fields unrenamed. */
export interface PreviewRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Mirrors `geometry::Side`, which serialises lowercase. */
export type PageSide = "left" | "right";

/** Mirrors `preview::PreviewPhoto`. */
export interface PreviewPhoto {
  path: string;
  hash: string;
  /** Oriented pixel dimensions -- the frame `crop` is normalised against. */
  width: number;
  height: number;
  /**
   * The sidecar's 400px contact-sheet JPEG, or `null` when writing it failed.
   * `null` renders as an empty box, never as a missing placement.
   */
  thumbnailPath: string | null;
}

/** Mirrors `preview::PreviewPlacement`. */
export interface PreviewPlacement {
  /** Indexes `BookLayout.photos`, NOT the position of this placement. */
  photoIndex: number;
  /** Page-normalised destination rect. */
  slotRect: PreviewRect;
  /** Crop window in the photo's own oriented, normalised frame. */
  crop: PreviewRect;
  z: number;
  /**
   * The basename the exporter writes this placement under, extensionless, so
   * something odd on screen can be traced to the file it produced and checked
   * against `manifest.json`. Built by `export::output_filename` in Rust --
   * never re-derived here, or the preview would name files the exporter does
   * not write. `null` only when the photo index is out of range.
   */
  filename: string | null;
}

/** Mirrors `preview::PreviewPage`. */
export interface PreviewPage {
  number: number;
  side: PageSide;
  templateId: string;
  /** True when this page prints with nothing on it -- see `preview.rs`. */
  blank: boolean;
  placements: PreviewPlacement[];
}

/** Mirrors `preview::PreviewGeometry` -- `geometry.rs`'s own constants. */
export interface PreviewGeometry {
  pageWIn: number;
  pageHIn: number;
  trimU: number;
  trimV: number;
  safeU: number;
  safeV: number;
  gutterU: number;
}

/** Mirrors `preview::BookLayout`, returned by the `book_layout` command. */
export interface BookLayout {
  projectId: number;
  seed: number;
  pageCount: number;
  placedPhotos: number;
  droppedPhotos: number;
  geometry: PreviewGeometry;
  /** Every analysed photo, in the order `photoIndex` indexes. */
  photos: PreviewPhoto[];
  pages: PreviewPage[];
  /** One entry per opening, in `toSpreads` order -- see `openingFor`. */
  openings: PreviewOpening[];
}

/**
 * Mirrors `preview::PreviewOpening`: what the user has said about one
 * opening and what the engine could still offer it. An opening is what the
 * reader sees with the book open -- page 1 alone, then each pair of facing
 * pages, then the last page alone -- and it is numbered the way `toSpreads`
 * orders them, so `openings[i]` describes `toSpreads(pages)[i]`.
 */
export interface PreviewOpening {
  index: number;
  /** Skipped by "shuffle"; refuses every other edit until unlocked. */
  locked: boolean;
  /** Templates the user has rejected here; never offered again. */
  rejected: string[];
  /**
   * Templates that could replace the current one. Empty when the opening
   * holds nothing, or when every template for this many photos has been
   * shown or rejected -- which is also when "regenerate" has nothing to do.
   */
  alternatives: string[];
}

/** Mirrors `edit::PlacementRef`: a slot named by printed page number and z. */
export interface PlacementRef {
  page: number;
  z: number;
}

/**
 * Mirrors `edit::BookEdit`, the one argument of the `edit_book` command.
 * `kind` is the serde tag; every field name is the camelCase Rust accepts,
 * pinned by `tests/fixtures/wire/book-edits.json` on both sides.
 */
export type BookEdit =
  | { kind: "regenerate"; opening: number }
  | { kind: "rejectTemplate"; opening: number }
  | { kind: "setTemplate"; opening: number; templateId: string }
  | { kind: "setLocked"; opening: number; locked: boolean }
  | { kind: "shuffle" }
  | { kind: "swapPhotos"; a: PlacementRef; b: PlacementRef };

/**
 * One printed opening: two facing pages with the fold between them, or ONE
 * page facing an inside cover.
 *
 * A book is 2 single pages + (N-2)/2 spreads. Page 1 is a right-hand page
 * facing the inside front cover; the last page is a left-hand page facing the
 * inside back cover. Neither has a partner, and rendering either as half of a
 * spread would show a fold where the book has a cover.
 */
export interface PreviewSpread {
  key: string;
  left: PreviewPage | null;
  right: PreviewPage | null;
  label: string;
}

/** CSS for an absolutely positioned box inside the page. */
export interface BoxStyle {
  left: string;
  top: string;
  width: string;
  height: string;
}

/** CSS for the `<img>` inside a slot. */
export interface CropStyle {
  width: string;
  height: string;
  /**
   * Always `"none"`. Tailwind's preflight sets `img { max-width: 100% }`,
   * which would clamp an enlarged image back to the slot width and silently
   * show MORE of the photo than the engine cropped -- the one regression this
   * whole file exists to prevent, arriving through CSS rather than through
   * arithmetic. It lives in the style object so a template cannot forget it.
   */
  maxWidth: string;
  transform: string;
}

/** One drawable slot: which photo, where on the page, and which part of it. */
export interface PreviewSlot {
  /** Unique within the book: page number and stacking order, never the hash. */
  key: string;
  z: number;
  /** `null` when the payload carries no photo at that index. */
  photo: PreviewPhoto | null;
  slot: BoxStyle;
  crop: CropStyle;
  /** The exporter's own basename for this placement -- see `PreviewPlacement`. */
  filename: string | null;
}

/**
 * Percentages, rounded to 4 decimal places.
 *
 * Rounding is not cosmetic here: `0.62 * 100` is `62.00000000000001` in
 * IEEE-754, and a style string carrying that reads as a bug in devtools. Four
 * places is roughly 0.03 px on a 700 px page -- far below one device pixel,
 * so nothing about where a box lands changes.
 */
function pct(value: number): string {
  return `${Math.round(value * 10_000) / 10_000}`;
}

/**
 * A positive extent, or 1 as a fallback.
 *
 * A crop with a non-positive extent cannot happen -- `book::crop::choose_crop`
 * never produces one -- but dividing by it would yield `Infinity` and a slot
 * that renders as nothing at all, which reads on screen as "the engine placed
 * no photo here". Falling back to the full frame is visibly wrong instead of
 * invisibly wrong.
 */
function extent(value: number): number {
  return value > 0 ? value : 1;
}

/**
 * The CSS that draws the engine's crop, and the most important few numbers in
 * the preview.
 *
 * The cropped window has to fill the slot, so the whole image is drawn at
 * `1/crop.w` by `1/crop.h` of the slot's size and then slid so the window's
 * top-left corner sits at the slot's. `transform: translate()` resolves its
 * percentages against the ELEMENT's own border box -- which, after the sizing
 * above, is the full image -- so the offsets are exactly the normalised
 * `crop.x` and `crop.y`, with no second conversion to get wrong.
 *
 * Undistorted precisely when the crop's real-world aspect equals the slot's,
 * which is what `choose_crop` guarantees (it only ever moves the window, never
 * reshapes it). A visible stretch here would therefore be a real disagreement
 * between the crop and the slot, not a rendering artefact -- which is the
 * behaviour worth having.
 */
export function cropStyle(crop: PreviewRect): CropStyle {
  return {
    width: `${pct(100 / extent(crop.w))}%`,
    height: `${pct(100 / extent(crop.h))}%`,
    maxWidth: "none",
    transform: `translate(-${pct(crop.x * 100)}%, -${pct(crop.y * 100)}%)`,
  };
}

/** Any page-normalised rect as an absolutely positioned box. */
export function rectStyle(rect: PreviewRect): BoxStyle {
  return {
    left: `${pct(rect.x * 100)}%`,
    top: `${pct(rect.y * 100)}%`,
    width: `${pct(rect.w * 100)}%`,
    height: `${pct(rect.h * 100)}%`,
  };
}

/**
 * The trim rectangle: what survives the guillotine. Mirrors
 * `geometry::in_trim` -- the outer vertical edge and top/bottom are inset by
 * the bleed, and **the fold edge is not inset at all**, because the paper is
 * continuous there.
 */
export function trimRect(geometry: PreviewGeometry, side: PageSide): PreviewRect {
  const x = side === "left" ? geometry.trimU : 0;
  return {
    x,
    w: 1 - geometry.trimU,
    y: geometry.trimV,
    h: 1 - 2 * geometry.trimV,
  };
}

/**
 * The safe area: Pixajoy's published 1/8" clearance inside the trim, on every
 * edge except the fold -- where the gutter band already governs clearance and
 * a second inset would double-count it. Mirrors `geometry::in_safe_margin`,
 * and is therefore a STRICT subset of `trimRect`.
 */
export function safeRect(geometry: PreviewGeometry, side: PageSide): PreviewRect {
  const inset = geometry.trimU + geometry.safeU;
  return {
    x: side === "left" ? inset : 0,
    w: 1 - inset,
    y: geometry.trimV + geometry.safeV,
    h: 1 - 2 * (geometry.trimV + geometry.safeV),
  };
}

/**
 * The gutter dead band: the strip nearest the fold that curls into the
 * binding. Mirrors `geometry::clear_of_gutter`, so it is measured inward FROM
 * THE FOLD -- the inner edge of each page, which is `x = 1` on a left page and
 * `x = 0` on a right one, never the outer edge.
 */
export function gutterRect(geometry: PreviewGeometry, side: PageSide): PreviewRect {
  return {
    x: side === "left" ? 1 - geometry.gutterU : 0,
    w: geometry.gutterU,
    y: 0,
    h: 1,
  };
}

function spread(left: PreviewPage | null, right: PreviewPage | null): PreviewSpread {
  const numbers = [left?.number, right?.number].filter((n) => n !== undefined);
  return {
    key: `spread-${numbers.join("-")}`,
    left,
    right,
    label: numbers.length > 1 ? `Pages ${numbers[0]}–${numbers[1]}` : `Page ${numbers[0]}`,
  };
}

/**
 * The book's pages grouped into printed openings.
 *
 * Page 1 alone, then the middle in facing pairs, then the last page alone --
 * matching `pace::assemble`, which builds exactly that and gives page 1
 * `Side::Right` and the last page `Side::Left`.
 *
 * **Every page appears exactly once**, blank ones included: ten of the 36
 * templates leave a full page empty (mostly text-zone layouts nothing renders
 * yet), so skipping them would show a tidier book than the one that prints. An
 * odd-length middle -- not a real SKU, but reachable through a degenerate page
 * count -- leaves its last page unpartnered rather than dropping it.
 */
export function toSpreads(pages: PreviewPage[]): PreviewSpread[] {
  if (pages.length === 0) return [];

  const spreads: PreviewSpread[] = [];
  const first = pages[0];
  if (first) spreads.push(spread(null, first));

  const middle = pages.slice(1, -1);
  for (let i = 0; i < middle.length; i += 2) {
    spreads.push(spread(middle[i] ?? null, middle[i + 1] ?? null));
  }

  if (pages.length > 1) {
    const last = pages[pages.length - 1];
    if (last) spreads.push(spread(last, null));
  }
  return spreads;
}

/**
 * The photo at an index of the layout's own photo list, or `null`.
 *
 * `photoIndex` indexes the WHOLE analysed slice -- the same contract
 * `book::manifest` and `export::build_items` read -- so this must never be
 * confused with a placement's position on its page. The two differ on every
 * page whose photos were not assigned in order, and the symptom is the
 * preview showing one photograph where another one prints.
 */
/**
 * Which side of the fold a page is, for choosing its guides.
 *
 * A page's side is a PROPERTY OF THE PAGE, never of the half of the opening it
 * happens to be drawn in. The two agree for every real SKU -- `pace::assemble`
 * builds R, (L,R)..., L -- but the padding path at `pace.rs:418` does not: it
 * appends by parity, so the last page of a degenerate 5-page book is a
 * right-hand page even though `toSpreads` renders it in the left half.
 *
 * Inferring the side from position there would inset the trim and draw the
 * gutter band on the wrong edges, i.e. the preview misrepresenting the
 * physical book -- the one thing it must never do.
 *
 * The half is used only for `null`, which is an inside cover rather than a
 * page, and so has no side of its own.
 */
export function pageSide(page: PreviewPage | null, half: PageSide): PageSide {
  return page?.side ?? half;
}

/**
 * The distinct templates an opening drew from, in left-then-right order.
 *
 * Deduplicated because a spread's two halves normally come from ONE template
 * decomposed at the fold, and naming it twice would read as a repeat. A
 * genuinely repeating template across consecutive spreads is one of the
 * defects this preview exists to reveal, and it is only inferable from layout
 * shape unless the id is on screen.
 */
export function spreadTemplates(opening: PreviewSpread | undefined): string[] {
  const ids: string[] = [];
  for (const page of [opening?.left, opening?.right]) {
    if (page && !ids.includes(page.templateId)) ids.push(page.templateId);
  }
  return ids;
}

export function photoFor(layout: BookLayout, photoIndex: number): PreviewPhoto | null {
  return layout.photos[photoIndex] ?? null;
}

/** Everything needed to draw one page's slots, in stacking order. */
export function pageSlots(layout: BookLayout, page: PreviewPage): PreviewSlot[] {
  return page.placements.map((placement) => ({
    key: `p${page.number}-z${placement.z}`,
    z: placement.z,
    photo: photoFor(layout, placement.photoIndex),
    slot: rectStyle(placement.slotRect),
    crop: cropStyle(placement.crop),
    filename: placement.filename,
  }));
}

/**
 * The photos no page placed -- what "N left out" actually refers to.
 *
 * Derived from which indices the pages reference rather than from a flag,
 * because there is no flag: the engine reports only a COUNT (`Book::dropped`,
 * covering culling, capacity trimming, unplaced groups and page-half
 * overflow), and the preview carries the full photo list precisely so the
 * count can be turned back into pictures.
 */
export function leftOutPhotos(layout: BookLayout): PreviewPhoto[] {
  const placed = new Set(
    layout.pages.flatMap((page) => page.placements.map((placement) => placement.photoIndex)),
  );
  return layout.photos.filter((_, index) => !placed.has(index));
}

/** No decisions and nothing to offer: what a payload without controls means. */
const UNCONTROLLED: Omit<PreviewOpening, "index"> = { locked: false, rejected: [], alternatives: [] };

/**
 * The controls for the opening at `index` of `toSpreads(layout.pages)`.
 *
 * Falls back to "unlocked, nothing to offer" rather than throwing when the
 * payload carries no entry, so a preview drawn from an older payload still
 * renders every page; the buttons simply have nothing to do.
 */
export function openingFor(layout: BookLayout, index: number): PreviewOpening {
  return layout.openings.find((opening) => opening.index === index) ?? { index, ...UNCONTROLLED };
}

/** Whether "regenerate", "reject" and "change layout" can do anything here. */
export function canRelayout(opening: PreviewOpening): boolean {
  return !opening.locked && opening.alternatives.length > 0;
}

export function samePlacement(a: PlacementRef, b: PlacementRef): boolean {
  return a.page === b.page && a.z === b.z;
}

/** What a click on a photo does while swapping: the next selection, and the edit to send if any. */
export interface SwapStep {
  selected: PlacementRef | null;
  edit: BookEdit | null;
}

/**
 * The swap gesture as a pure function: the first click selects a photo, a
 * second click on the SAME photo deselects it, and a click on any other photo
 * -- on any page of the book -- swaps the two and clears the selection.
 *
 * Nothing about which swaps are allowed lives here. Rust refuses a swap that
 * would cut a face, put one in the gutter or the margin, or print below 200
 * DPI, and returns the untouched book with the reason; the webview shows the
 * reason and the book it was given.
 */
export function nextSwapStep(selected: PlacementRef | null, clicked: PlacementRef): SwapStep {
  if (selected === null) return { selected: clicked, edit: null };
  if (samePlacement(selected, clicked)) return { selected: null, edit: null };
  return { selected: null, edit: { kind: "swapPhotos", a: selected, b: clicked } };
}

/**
 * A template id as a menu label: `07-two-up-symmetric-margin` reads
 * "two-up symmetric margin", and a page half `...:right` says which half. The
 * number is the library's own sort key, not something a user chooses by.
 */
export function templateLabel(id: string): string {
  const [base, side] = id.split(":");
  const words = (base ?? id).replace(/^\d+-/, "").replaceAll("-", " ");
  return side ? `${words} (${side} half)` : words;
}
