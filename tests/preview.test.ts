import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  canRelayout,
  layoutSketch,
  chapterOpening,
  pageTitle,
  viewerStep,
  escapeDeselects,
  titleEdit,
  cropStyle,
  cropZoomed,
  cropZoom,
  cropSubject,
  cropAtZoom,
  windowMoved,
  MAX_CROP_ZOOM,
  leftOutPhotos,
  openingFor,
  insideCover,
  pageSide,
  pageSlots,
  photoFor,
  rectStyle,
  refusalText,
  replaceCandidates,
  withImportedPhotos,
  pageGuides,
  pressIsDrag,
  SLOT_DRAG_THRESHOLD_PX,
  DRAG_THRESHOLD_PX,
  pickAspect,
  pickBlock,
  pickEdit,
  setCoverCropEdit,
  setCropEdit,
  setSlotEdit,
  canStep,
  stepLabel,
  slotMoved,
  slotResized,
  nearestGuide,
  snapLines,
  NO_HOLD,
  spreadTemplates,
  templateLabel,
  layoutLabel,
  toSpreads,
  wheelZoomFactor,
  type BookEdit,
  type BookLayout,
  type PreviewGeometry,
  type PickTarget,
  type PreviewPage,
  type PreviewPhoto,
  type Rejection,
  type SlotCandidate,
  type SnapGuides,
} from "../app/types/preview";

/**
 * The TypeScript half of the preview's wire pin, plus the geometry the
 * preview does for itself.
 *
 * `book-layout.json` is asserted EXACTLY by
 * `preview::tests::book_layout_serialises_exactly_the_keys_the_preview_reads`
 * on the Rust side; everything below feeds that same file through the real
 * functions `BookPreview.vue` calls. A rename on either side fails one of the
 * two suites against a fixture that did not move.
 *
 * The component itself has no test harness in this project -- vitest never
 * renders a `.vue` file here -- so every decision worth pinning lives in
 * `app/types/preview.ts` as a pure function and is pinned here.
 */
const layout: BookLayout = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/book-layout.json", import.meta.url)), "utf8"),
) as BookLayout;

const geometry: PreviewGeometry = layout.geometry;

/** Page numbers, so a spread's contents read at a glance in a failure. */
function shape(pages: (PreviewPage | null)[]): (number | null)[] {
  return pages.map((page) => page?.number ?? null);
}

describe("the wire fixture", () => {
  it("carries the cover: both panels, one empty, and the spine", () => {
    expect(layout.cover.aspect).toBe(1.175);
    expect(layout.cover.spine).toBe("#1a2b3c");
    expect(layout.cover.front.photo).toEqual({
      photoIndex: 4,
      crop: { x: 0, y: 0.125, w: 1, h: 0.567 },
      filename: "cover-front-hash-e",
    });
    expect(layout.cover.back.photo).toBeNull();
    expect(layout.cover.back.visible.x).toBeGreaterThan(layout.cover.front.visible.x);
  });

  it("carries every key the preview reads, including a page that holds nothing", () => {
    expect(layout.projectId).toBe(7);
    expect(layout.pageCount).toBe(4);
    expect(layout.placedPhotos).toBe(3);
    expect(layout.droppedPhotos).toBe(2);
    expect(layout.options).toStrictEqual({ places: true, featuredFloor: 6, briefCap: 2 });
    expect(layout.pages).toHaveLength(4);
    expect(layout.photos).toHaveLength(5);
    // The text-zone half of a real template holds no photo, and is not
    // blank because the chapter title prints there.
    expect(layout.pages[2]?.templateId).toBe("03-hero-left-text-right");
    expect(layout.pages[2]?.placements).toHaveLength(0);
    expect(layout.pages[2]?.blank).toBe(false);
    expect(layout.chapters).toEqual([
      {
        number: 1,
        event: 4,
        events: [4, 6],
        firstPage: 2,
        lastPage: 4,
        start: 1773273600,
        end: 1773446450,
        autoTitle: "12–14 Mar 2026",
        title: "Reykjavík",
        custom: true,
        titleBox: {
          page: 3,
          rect: { x: 0.125, y: 0.25, w: 0.75, h: 0.25 },
          align: "center",
          pointSize: 24,
          overlapsPhoto: false,
        },
      },
    ]);
    // A page that got no group at all still prints white.
    expect(layout.pages[3]?.blank).toBe(true);
  });

  /**
   * T24. The spec and the guides are two different kinds of number on one
   * payload, and the failure this guards is reading the wrong one: the panel
   * editing a normalised inset, or the preview drawing a raw inch value.
   *
   * So it asserts both halves and the arithmetic between them. The left
   * trim's x is the bleed as a fraction of the page width, which is a
   * relation neither side can satisfy by accident -- dropping `spec` yields
   * `undefined`, and swapping the two structs' contents fails the ratio.
   */
  it("carries the book's own spec in inches alongside the guides it implies", () => {
    expect(layout.spec).toEqual({
      pageWIn: 11.197,
      pageHIn: 8.894,
      bleedIn: 0.197,
      gutterIn: 0.197,
      safeMarginIn: 0.125,
      minDpi: 200,
      warnDpi: 300,
      coverWrapIn: 0.75,
    });

    const { pageWIn, bleedIn, gutterIn, safeMarginIn } = layout.spec;
    expect(geometry.left.trim.x).toBeCloseTo(bleedIn / pageWIn, 15);
    expect(geometry.left.gutter.w).toBeCloseTo(gutterIn / pageWIn, 15);
    // The safe margin sits INSIDE the trim, so its distance from the page
    // edge is bleed plus margin. A Rust-side "fix" that drops either term
    // moves every safe guide and fails here.
    expect(geometry.left.safe.x).toBeCloseTo((bleedIn + safeMarginIn) / pageWIn, 15);
  });

  it("carries an off-centre crop that is not the full frame", () => {
    // If this fixture were a full-frame crop, "crop applied" and "crop
    // ignored" would render identically and every assertion below would be
    // decorative.
    const crop = layout.pages[1]?.placements[1]?.crop;
    expect(crop).toEqual({ x: 0.2, y: 0.1, w: 0.5, h: 0.4 });
  });
});

describe("toSpreads", () => {
  /**
   * The named regression: **page 1 or the last page rendered as half of a
   * spread.** A book is 2 single pages + (N-2)/2 spreads -- page 1 faces the
   * inside front cover and the last page faces the inside back cover, so
   * neither has a partner.
   */
  it("gives page 1 and the last page a spread of their own", () => {
    const spreads = toSpreads(layout.pages);

    expect(spreads.map((s) => shape([s.left, s.right]))).toEqual([
      [null, 1],
      [2, 3],
      [4, null],
    ]);
  });

  it("puts the single pages on the side the engine assigned them", () => {
    const spreads = toSpreads(layout.pages);

    // Page 1 is a RIGHT-hand page; the last is a LEFT-hand page. A pairing
    // that got this backwards would put the fold on the wrong side of both.
    expect(spreads[0]?.right?.side).toBe("right");
    expect(spreads[0]?.left).toBeNull();
    expect(spreads[2]?.left?.side).toBe("left");
    expect(spreads[2]?.right).toBeNull();
  });

  /**
   * The named regression: **a blank page is skipped instead of rendered
   * blank.** Every page of the book must appear exactly once across the
   * spreads, empty ones included.
   */
  it("keeps every page exactly once, including the blank ones", () => {
    const spreads = toSpreads(layout.pages);
    const seen = spreads.flatMap((s) => [s.left, s.right]).filter((p) => p !== null);

    expect(seen.map((p) => p.number)).toEqual([1, 2, 3, 4]);
    // p3 holds no photo but prints the chapter title, so only p4 is blank.
    expect(seen.filter((p) => p.blank).map((p) => p.number)).toEqual([4]);
  });

  it("labels a spread by both its page numbers and a single by its one", () => {
    const spreads = toSpreads(layout.pages);

    expect(spreads.map((s) => s.label)).toEqual(["Page 1", "Pages 2–3", "Page 4"]);
  });

  it("scales to a real 20-page book: 2 singles and 9 spreads", () => {
    const pages: PreviewPage[] = Array.from({ length: 20 }, (_, i) => ({
      number: i + 1,
      // Page 1 is right, then left/right alternating, last is left.
      side: i === 0 || i % 2 === 0 ? "right" : "left",
      templateId: "t",
      blank: false,
      placements: [],
    }));

    const spreads = toSpreads(pages);

    expect(spreads).toHaveLength(11);
    expect(shape([spreads[0]?.left ?? null, spreads[0]?.right ?? null])).toEqual([null, 1]);
    expect(shape([spreads[1]?.left ?? null, spreads[1]?.right ?? null])).toEqual([2, 3]);
    expect(shape([spreads[9]?.left ?? null, spreads[9]?.right ?? null])).toEqual([18, 19]);
    expect(shape([spreads[10]?.left ?? null, spreads[10]?.right ?? null])).toEqual([20, null]);
    // Nothing lost and nothing shown twice.
    expect(spreads.flatMap((s) => [s.left, s.right]).filter((p) => p !== null)).toHaveLength(20);
  });

  it("never drops a page from a degenerate odd-length book", () => {
    const pages: PreviewPage[] = [1, 2, 3, 4, 5].map((number) => ({
      number,
      side: number % 2 === 1 ? "right" : "left",
      templateId: "t",
      blank: false,
      placements: [],
    }));

    const seen = toSpreads(pages)
      .flatMap((s) => [s.left, s.right])
      .filter((p) => p !== null);

    expect(seen.map((p) => p.number)).toEqual([1, 2, 3, 4, 5]);
  });

  it("handles a one-page and an empty book without inventing a partner", () => {
    expect(toSpreads([])).toEqual([]);
    const one = toSpreads([
      { number: 1, side: "right", templateId: "t", blank: true, placements: [] },
    ]);
    expect(one).toHaveLength(1);
    expect(one[0]?.left).toBeNull();
    expect(one[0]?.right?.number).toBe(1);
  });
});

describe("cropStyle", () => {
  /**
   * The named regression: **the preview shows the whole photo rather than the
   * engine's crop.** This is the single most important number on the screen;
   * showing the uncropped photo hides exactly the defects the preview exists
   * to reveal.
   *
   * The maths: the slot box is `crop.w` x `crop.h` of the displayed image, so
   * the image is drawn at `1/crop.w` x `1/crop.h` of the slot and translated
   * by `crop.x`/`crop.y` OF ITS OWN (already enlarged) size -- which is why
   * the translate percentages are the raw normalised offsets.
   */
  it("scales the photo so the crop window fills the slot, and offsets to it", () => {
    expect(cropStyle({ x: 0.2, y: 0.1, w: 0.5, h: 0.4 })).toEqual({
      width: "200%",
      height: "250%",
      maxWidth: "none",
      transform: "translate(-20%, -10%)",
    });
  });

  it("does not translate a crop that starts at the origin", () => {
    expect(cropStyle({ x: 0, y: 0.125, w: 1, h: 0.75 })).toEqual({
      width: "100%",
      height: "133.3333%",
      maxWidth: "none",
      transform: "translate(-0%, -12.5%)",
    });
  });

  /**
   * Tailwind's preflight sets `img { max-width: 100% }`, which would clamp an
   * enlarged image back to the slot width and silently show MORE of the photo
   * than the crop -- the exact regression above, arriving through CSS rather
   * than through arithmetic. The override belongs in the style object, where
   * it cannot be forgotten by a template.
   */
  it("overrides the max-width the CSS reset would clamp the crop with", () => {
    expect(cropStyle({ x: 0.3125, y: 0, w: 0.6875, h: 1 }).maxWidth).toBe("none");
  });

  it("falls back to the full frame rather than dividing by a degenerate crop", () => {
    // A negative extent, not a zero one: a zero-width box can "pass" a guard
    // via NaN propagation, which is a documented trap in this project.
    expect(cropStyle({ x: 0, y: 0, w: -0.5, h: 0.4 })).toEqual({
      width: "100%",
      height: "250%",
      maxWidth: "none",
      transform: "translate(-0%, -0%)",
    });
  });
});

describe("pageSide", () => {
  /**
   * A page's side is a PROPERTY OF THE PAGE, not of which half of the opening
   * it happens to be drawn in. The two agree for every real SKU --
   * `pace::assemble` builds R, (L,R)..., L -- but the padding path at
   * `pace.rs:418` does not: a degenerate odd page count appends pages by
   * parity, so the final page of a 5-page book is a RIGHT-hand page even
   * though `toSpreads` renders it in the left half.
   *
   * Inferring the side from position there would draw the trim inset and the
   * gutter band on the wrong edges -- the preview misrepresenting the
   * physical book, which is the one thing it must never do.
   */
  it("reads the page's own side, not the half it is drawn in", () => {
    const rightHandPageInTheLeftHalf: PreviewPage = {
      number: 5,
      side: "right",
      templateId: "t",
      blank: false,
      placements: [],
    };

    expect(pageSide(rightHandPageInTheLeftHalf, "left")).toBe("right");
  });

  it("falls back to the half only for the inside cover, which is not a page", () => {
    expect(pageSide(null, "left")).toBe("left");
    expect(pageSide(null, "right")).toBe("right");
  });

  /**
   * Page 1 is a right-hand page, so the empty half BESIDE it is the inside
   * FRONT cover; the last page is a left-hand page facing the inside BACK
   * cover. Read off `toSpreads`' own output rather than asserted per half, so
   * the test fails if either the label or the spread order is wrong.
   */
  it("names the inside cover each lone page actually faces", () => {
    const pages: PreviewPage[] = [1, 2, 3, 4].map((number) => ({
      number,
      side: number % 2 === 1 ? "right" : "left",
      templateId: "t",
      blank: false,
      placements: [],
    }));
    const spreads = toSpreads(pages);
    const first = spreads[0];
    const last = spreads[spreads.length - 1];

    expect(first?.left).toBeNull();
    expect(insideCover("left")).toBe("front");
    expect(last?.right).toBeNull();
    expect(insideCover("right")).toBe("back");
  });

  /**
   * The regression end to end, on the fixture `pace.rs`'s padding path
   * actually produces: five pages numbered by parity. Page 5 lands in a left
   * half and must still be drawn as the right-hand page it is.
   */
  it("keeps a padded odd-length book's last page on its own side", () => {
    const pages: PreviewPage[] = [1, 2, 3, 4, 5].map((number) => ({
      number,
      // `pace.rs:418` pads by parity: pages 1, 3, 5 are right-hand.
      side: number % 2 === 1 ? "right" : "left",
      templateId: "t",
      blank: false,
      placements: [],
    }));

    const spreads = toSpreads(pages);
    const last = spreads[spreads.length - 1];

    expect(last?.left?.number).toBe(5);
    expect(pageSide(last?.left ?? null, "left")).toBe("right");
    // And the guides that follow from it are the RIGHT page's.
    const guides = geometry[pageSide(last?.left ?? null, "left")];
    expect(guides.gutter.x).toBeCloseTo(0, 12);
    expect(guides.trim.x).toBeCloseTo(0, 12);
  });
});

describe("spreadTemplates", () => {
  /**
   * A repeating template is one of the defects this preview exists to reveal,
   * and it is only inferable from layout shape unless the id is on screen.
   */
  it("names the distinct templates a spread used", () => {
    const spreads = toSpreads(layout.pages);

    // Pages 2 and 3 are two halves of ONE template: named once, not twice.
    expect(spreadTemplates(spreads[1])).toEqual(["03-hero-left-text-right"]);
    expect(spreadTemplates(spreads[0])).toEqual(["12-quad-right:right"]);
    expect(spreadTemplates(spreads[2])).toEqual(["blank"]);
  });

  it("names both when the two halves came from different templates", () => {
    const spread = toSpreads([
      { number: 1, side: "right", templateId: "a", blank: false, placements: [] },
      { number: 2, side: "left", templateId: "b", blank: false, placements: [] },
      { number: 3, side: "right", templateId: "c", blank: false, placements: [] },
      { number: 4, side: "left", templateId: "d", blank: false, placements: [] },
    ])[1];

    expect(spreadTemplates(spread)).toEqual(["b", "c"]);
  });
});

describe("the guides", () => {
  /**
   * The named regression: **a guide drawn at the wrong place.** Rust derives
   * the rects `geometry::in_trim`, `in_safe_margin` and `clear_of_gutter`
   * test against and ships them per side; these pin that what arrives is
   * sided correctly, so the preview can draw them without re-deriving.
   */
  it("insets the trim on the outer edge but never at the fold", () => {
    const left = geometry.left.trim;
    const trimU = 0.197 / 11.197;
    const trimV = 0.197 / 8.894;
    // A LEFT page's outer edge is x=0; its fold edge is x=1.
    expect(left.x).toBeCloseTo(trimU, 12);
    expect(left.x + left.w).toBeCloseTo(1, 12);

    const right = geometry.right.trim;
    // A RIGHT page is the mirror: fold at x=0, outer edge at x=1.
    expect(right.x).toBeCloseTo(0, 12);
    expect(right.x + right.w).toBeCloseTo(1 - trimU, 12);

    for (const rect of [left, right]) {
      expect(rect.y).toBeCloseTo(trimV, 12);
      expect(rect.y + rect.h).toBeCloseTo(1 - trimV, 12);
    }
  });

  /**
   * `in_safe_margin` is a STRICT subset of `in_trim` -- the same property
   * `geometry_in_safe_margin_is_a_strict_subset_of_in_trim` pins in Rust. A
   * safe guide wired to the trim inset would draw two lines on top of each
   * other and look entirely plausible doing it.
   */
  it("insets the safe margin strictly inside the trim, except at the fold", () => {
    const trim = geometry.left.trim;
    const safe = geometry.left.safe;

    expect(safe.x).toBeGreaterThan(trim.x);
    expect(safe.y).toBeGreaterThan(trim.y);
    expect(safe.y + safe.h).toBeLessThan(trim.y + trim.h);
    // The fold edge is NOT inset a second time -- the gutter band governs it.
    expect(safe.x + safe.w).toBeCloseTo(trim.x + trim.w, 12);
    expect(safe.x).toBeCloseTo((0.197 + 0.125) / 11.197, 12);

    const rightSafe = geometry.right.safe;
    expect(rightSafe.x).toBeCloseTo(0, 12);
    expect(rightSafe.x + rightSafe.w).toBeCloseTo(1 - (0.197 + 0.125) / 11.197, 12);
  });

  /**
   * The gutter dead band is measured inward FROM THE FOLD, so it sits on the
   * INNER edge of each page: x=1 on a left page, x=0 on a right one. Putting
   * it on the outer edge is the mistake this pins.
   */
  it("puts the gutter band against the fold on both pages, never the outer edge", () => {
    const gutterU = 0.197 / 11.197;
    const left = geometry.left.gutter;
    expect(left.x).toBeCloseTo(1 - gutterU, 12);
    expect(left.x + left.w).toBeCloseTo(1, 12);

    const right = geometry.right.gutter;
    expect(right.x).toBeCloseTo(0, 12);
    expect(right.x + right.w).toBeCloseTo(gutterU, 12);

    // Full page height on both: the band is a strip, not a box.
    for (const rect of [left, right]) {
      expect(rect.y).toBe(0);
      expect(rect.h).toBe(1);
    }
  });

  it("uses the engine's real Pixajoy figures, not round numbers", () => {
    // 5mm bleed on an 11.197" page, and Pixajoy's published 1/8" safe margin.
    expect(geometry.left.safe.y).toBeCloseTo((0.197 + 0.125) / 8.894, 12);
    expect(geometry.pageWIn).toBe(11.197);
    expect(geometry.pageHIn).toBe(8.894);
  });

  /**
   * T23. The guides are drawn from whatever the payload carries, never
   * rebuilt from a constant. A book printed at a different size ships
   * different rects, and every consumer must follow them. `odd` is an 8 x 10
   * portrait page (bleed 0.25, fold 0.4, safe 0.05) so no number coincides
   * with Pixajoy's.
   */
  it("draws the guides the payload carries, not Pixajoy's", () => {
    const odd: PreviewGeometry = {
      pageWIn: 8,
      pageHIn: 10,
      left: {
        trim: { x: 0.25 / 8, y: 0.025, w: 1 - 0.25 / 8, h: 0.95 },
        safe: { x: 0.3 / 8, y: 0.03, w: 1 - 0.3 / 8, h: 0.94 },
        gutter: { x: 1 - 0.4 / 8, y: 0, w: 0.4 / 8, h: 1 },
      },
      right: {
        trim: { x: 0, y: 0.025, w: 1 - 0.25 / 8, h: 0.95 },
        safe: { x: 0, y: 0.03, w: 1 - 0.3 / 8, h: 0.94 },
        gutter: { x: 0, y: 0, w: 0.4 / 8, h: 1 },
      },
    };

    const g = pageGuides(odd, "left", []);
    expect(g.edges.xs.some((x) => Math.abs(x - 0.25 / 8) < 1e-9)).toBe(true);
    expect(g.edges.xs.some((x) => Math.abs(x - (1 - 0.4 / 8)) < 1e-9)).toBe(true);
    expect(g.edges.ys.some((y) => Math.abs(y - 0.03) < 1e-9)).toBe(true);
    // The centre follows the payload too: this page's trim centre, not 0.5.
    expect(g.centres.xs).toEqual([0.25 / 8 + (1 - 0.25 / 8) / 2]);
    // Nothing of Pixajoy's leaks through.
    expect(g.edges.xs.some((x) => Math.abs(x - 0.197 / 11.197) < 1e-6)).toBe(false);

    const r = pageGuides(odd, "right", []);
    expect(r.edges.xs.some((x) => Math.abs(x - 0.4 / 8) < 1e-9)).toBe(true);
    expect(r.edges.xs.some((x) => Math.abs(x - (1 - 0.25 / 8)) < 1e-9)).toBe(true);
    expect(rectStyle(odd.left.gutter)).not.toEqual(rectStyle(geometry.left.gutter));
  });

  it("renders a guide rect as page percentages", () => {
    expect(rectStyle({ x: 0.25, y: 0.5, w: 0.125, h: 0.2 })).toEqual({
      left: "25%",
      top: "50%",
      width: "12.5%",
      height: "20%",
    });
  });
});

describe("pageSlots", () => {
  /**
   * The named regression: **the preview disagrees with the manifest about
   * which photo is in which slot.** `photoIndex` indexes the whole analysed
   * slice, and the fixture's indices are deliberately out of order (3, then
   * 1, then 0) -- so a lookup that walked placements positionally would show
   * `/a.jpg` where `/b.jpg` prints.
   */
  it("resolves each slot's photo by photoIndex, not by position on the page", () => {
    const page = layout.pages[1];
    if (!page) throw new Error("fixture page 2 is missing");

    const boxes = pageSlots(layout, page);

    expect(boxes.map((box) => box.photo?.path)).toEqual(["/b.jpg", "/a.jpg"]);
    expect(boxes.map((box) => box.photo?.hash)).toEqual(["hash-b", "hash-a"]);
    expect(boxes.map((box) => box.z)).toEqual([1, 2]);
  });

  it("gives every slot its own box and its own crop", () => {
    const page = layout.pages[1];
    if (!page) throw new Error("fixture page 2 is missing");

    const boxes = pageSlots(layout, page);

    expect(boxes[0]?.slot).toEqual({
      left: "6%",
      top: "8%",
      width: "50%",
      height: "62%",
    });
    expect(boxes[1]?.slot).toEqual({
      left: "60%",
      top: "31%",
      width: "34%",
      height: "45%",
    });
    expect(boxes[0]?.crop.transform).toBe("translate(-0%, -12.5%)");
    expect(boxes[1]?.crop.transform).toBe("translate(-20%, -10%)");
  });

  it("returns nothing for a page that prints blank", () => {
    const page = layout.pages[2];
    if (!page) throw new Error("fixture page 3 is missing");
    expect(pageSlots(layout, page)).toEqual([]);
  });

  it("keys each slot uniquely by page and z, not by photo", () => {
    const keys = layout.pages.flatMap((page) => pageSlots(layout, page).map((box) => box.key));
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys).toEqual(["p1-z1", "p2-z1", "p2-z2"]);
  });

  it("keeps the slot when the thumbnail is missing rather than dropping it", () => {
    // A failed thumbnail write must never look like a missing placement: the
    // layout is real either way.
    const page: PreviewPage = {
      number: 9,
      side: "left",
      templateId: "t",
      blank: false,
      placements: [
        { photoIndex: 2, slotRect: { x: 0.1, y: 0.2, w: 0.3, h: 0.4 }, crop: { x: 0, y: 0, w: 1, h: 1 }, z: 1 },
      ],
    };

    const boxes = pageSlots(layout, page);

    expect(boxes).toHaveLength(1);
    expect(boxes[0]?.photo?.path).toBe("/c.jpg");
    expect(boxes[0]?.photo?.thumbnailPath).toBeNull();
  });

  it("does not invent a photo for an index the payload does not carry", () => {
    const page: PreviewPage = {
      number: 9,
      side: "left",
      templateId: "t",
      blank: false,
      placements: [
        { photoIndex: 99, slotRect: { x: 0, y: 0, w: 1, h: 1 }, crop: { x: 0, y: 0, w: 1, h: 1 }, z: 1 },
      ],
    };

    expect(pageSlots(layout, page)[0]?.photo).toBeNull();
    expect(photoFor(layout, 99)).toBeNull();
  });
});

describe("leftOutPhotos", () => {
  it("names the photos no page placed, and agrees with the engine's count", () => {
    const left = leftOutPhotos(layout);

    expect(left.map((photo) => photo.path)).toEqual(["/c.jpg", "/e.jpg"]);
    expect(left).toHaveLength(layout.droppedPhotos);
  });

  it("carries each photo's index in the layout's photo list, which a drag from the tray sends", () => {
    for (const photo of leftOutPhotos(layout)) {
      expect(layout.photos[photo.index]?.path).toBe(photo.path);
    }
    expect(leftOutPhotos(layout).map((photo) => photo.index)).toEqual([2, 4]);
  });
});

describe("openings", () => {
  it("carries one entry per opening, in toSpreads order", () => {
    // The Rust side numbers openings the way `toSpreads` draws them; if the
    // two ever disagree, the buttons act on a different spread than the one
    // they sit beside.
    expect(layout.openings.map((o) => o.index)).toEqual(toSpreads(layout.pages).map((_, i) => i));
  });

  it("reads an opening's controls off the payload, and falls back to nothing to offer", () => {
    expect(openingFor(layout, 1)).toEqual({ index: 1, locked: false, rejected: [], alternatives: [], refused: [] });
    expect(openingFor(layout, 42)).toEqual({ index: 42, locked: false, rejected: [], alternatives: [], refused: [] });
  });

  it("can only re-lay an unlocked opening that has somewhere else to go", () => {
    expect(canRelayout({ index: 0, locked: false, rejected: [], alternatives: ["a"], refused: [] })).toBe(true);
    expect(canRelayout({ index: 0, locked: true, rejected: [], alternatives: ["a"], refused: [] })).toBe(false);
    expect(canRelayout({ index: 0, locked: false, rejected: ["b"], alternatives: [], refused: [] })).toBe(false);
  });
});

describe("layoutSketch", () => {
  const spread = [
    [{ x: 0.1, y: 0.2, w: 0.8, h: 0.6 }],
    [
      { x: 0, y: 0, w: 0.5, h: 1 },
      { x: 0.5, y: 0.5, w: 0.5, h: 0.5 },
    ],
  ];

  it("lays the pages side by side at the page's real shape", () => {
    const sketch = layoutSketch(spread, 1.25);
    expect(sketch.viewBox).toBe("0 0 250 100");
    expect(sketch.pages).toEqual([
      { x: 0, y: 0, w: 125, h: 100 },
      { x: 125, y: 0, w: 125, h: 100 },
    ]);
  });

  it("scales each page's slots into that page, not across the spread", () => {
    const { slots } = layoutSketch(spread, 1.25);
    expect(slots).toEqual([
      { x: 12.5, y: 20, w: 100, h: 60 },
      { x: 125, y: 0, w: 62.5, h: 100 },
      { x: 187.5, y: 50, w: 62.5, h: 50 },
    ]);
  });

  it("draws a single page alone, and a portrait page taller than wide", () => {
    const sketch = layoutSketch([[{ x: 0, y: 0, w: 1, h: 1 }]], 0.8);
    expect(sketch.viewBox).toBe("0 0 80 100");
    expect(sketch.slots).toEqual([{ x: 0, y: 0, w: 80, h: 100 }]);
  });
});

describe("templateLabel", () => {
  it("drops the library's sort number and reads the slug as words", () => {
    expect(templateLabel("07-two-up-symmetric-margin")).toBe("two-up symmetric margin".replaceAll("-", " "));
  });

  it("names the half for a single-page layout", () => {
    expect(templateLabel("35-portrait-triptych-hero-left:right")).toBe(
      "portrait triptych hero left (right half)",
    );
  });
});

describe("layoutLabel", () => {
  it("reads the fixture's layoutNames, which an empty library leaves empty", () => {
    expect(layout.layoutNames).toEqual({});
    expect(layoutLabel(layout, "07-two-up-symmetric-margin")).toBe("two up symmetric margin");
  });

  it("names a user layout by the name the user gave it", () => {
    const named = { ...layout, layoutNames: { "user-0123": "Beach pair" } };
    expect(layoutLabel(named, "user-0123")).toBe("Beach pair");
    expect(layoutLabel(named, "07-two-up-symmetric-margin")).toBe("two up symmetric margin");
  });
});

describe("the edit wire", () => {
  /**
   * `book-edits.json` is what `edit::tests::edit_deserialises_the_camel_case_
   * tagged_shape_the_webview_sends` parses on the Rust side. Building the
   * same values through the TypeScript type here means a renamed tag or field
   * on either side fails one suite against a fixture that did not move.
   */
  it("matches the shapes Rust parses, field for field", () => {
    const fixture: unknown = JSON.parse(
      readFileSync(fileURLToPath(new URL("./fixtures/wire/book-edits.json", import.meta.url)), "utf8"),
    );
    const typed: BookEdit[] = [
      { kind: "regenerate", opening: 3 },
      { kind: "rejectTemplate", opening: 0 },
      { kind: "setTemplate", opening: 2, templateId: "07-two-up-symmetric-margin" },
      { kind: "setLocked", opening: 1, locked: true },
      { kind: "shuffle" },
      { kind: "swapPhotos", a: { page: 2, z: 1 }, b: { page: 5, z: 2 } },
      { kind: "setCrop", placement: { page: 3, z: 2 }, x: 0.125, y: 0, w: 0.75 },
      { kind: "setSlot", placement: { page: 3, z: 2 }, rect: { x: 0.1, y: 0.2, w: 0.3, h: 0.4 } },
      { kind: "replacePhoto", placement: { page: 3, z: 1 }, photo: 12 },
      {
        kind: "setPrintSpec",
        spec: {
          pageWIn: 8,
          pageHIn: 10,
          bleedIn: 0.25,
          gutterIn: 0.4,
          safeMarginIn: 0.05,
          minDpi: 150,
          warnDpi: 220,
          coverWrapIn: 0.6,
        },
      },
      { kind: "setCoverPhoto", side: "front", photo: 7 },
      { kind: "setCoverPhoto", side: "back", photo: null },
      { kind: "setCoverCrop", side: "back", x: 0.125, y: 0.0625, w: 0.5 },
      { kind: "setSpineColour", rgb: "#1a2b3c" },
      { kind: "setChapterTitle", event: 4, title: "Reykjavík" },
      { kind: "setChapterTitle", event: 9, title: null },
      { kind: "addPhoto", opening: 10, photo: 31 },
      { kind: "movePhoto", from: { page: 4, z: 2 }, opening: 7 },
    ];
    expect(fixture).toEqual(typed);
  });
});

describe("hand cropping", () => {
  // A wide window on a tall photo: room to move on both axes.
  const crop = { x: 0.2, y: 0.1, w: 0.5, h: 0.25 };

  it("zooms about the centre and keeps the shape", () => {
    const zoomed = cropZoomed(crop, 2);
    expect(zoomed.w).toBeCloseTo(0.25, 12);
    expect(zoomed.h).toBeCloseTo(0.125, 12);
    expect(zoomed.x + zoomed.w / 2).toBeCloseTo(0.45, 12);
    expect(zoomed.y + zoomed.h / 2).toBeCloseTo(0.225, 12);
  });

  it("never grows past the photo, sliding back inside when it must", () => {
    const wide = cropZoomed(crop, 0.25);
    expect(wide.w).toBe(1);
    expect(wide.h).toBeCloseTo(0.5, 12);
    expect(wide.x).toBe(0);
    expect(wide.y).toBeGreaterThanOrEqual(0);
    expect(wide.y + wide.h).toBeLessThanOrEqual(1 + 1e-12);
  });

  it("zooms in on a scroll up and out on a scroll down", () => {
    const zoomIn = wheelZoomFactor(-40);
    const zoomOut = wheelZoomFactor(40);
    expect(zoomIn).toBeGreaterThan(1);
    expect(zoomOut).toBeLessThan(1);
    expect(zoomOut).toBeGreaterThan(0);
  });

  it("never shrinks below two percent, and ignores a nonsense factor", () => {
    expect(cropZoomed(crop, 1000).w).toBeCloseTo(0.02, 12);
    expect(cropZoomed(crop, 0)).toEqual(crop);
    expect(cropZoomed(crop, Number.NaN)).toEqual(crop);
  });

  it("sends x, y and w only -- Rust derives the height from the slot", () => {
    expect(setCropEdit({ page: 4, z: 1 }, { x: 0.1, y: 0.2, w: 0.3, h: 0.9 })).toEqual({
      kind: "setCrop",
      placement: { page: 4, z: 1 },
      x: 0.1,
      y: 0.2,
      w: 0.3,
    });
  });
});

describe("slot editing", () => {
  const rect = { x: 0.2, y: 0.2, w: 0.3, h: 0.4 };
  const guides: SnapGuides = {
    edges: { xs: [0, 0.5, 1], ys: [0, 0.5, 1] },
    centres: { xs: [0.5], ys: [0.5] },
  };

  it("finds the nearest guide within the threshold, or none", () => {
    expect(nearestGuide(0.49, [0, 0.5, 1], 0.02, 0, 1)).toBe(0.5);
    expect(nearestGuide(0.45, [0, 0.5, 1], 0.02, 0, 1)).toBeNull();
    expect(nearestGuide(0.26, [0.25, 0.3], 0.05, 0, 1)).toBe(0.25);
  });

  /**
   * A guide the value cannot legally reach is not a candidate at all, rather
   * than a snap the caller then clamps away. Clamping would leave the box on
   * no line while `held` claimed one, and it would hide the admissible guide
   * sitting just behind the unreachable one.
   */
  it("offers only a guide the value may legally reach", () => {
    expect(nearestGuide(0.49, [0.5, 0.475], 0.02, 0, 1)).toBe(0.5);
    expect(nearestGuide(0.49, [0.5, 0.475], 0.02, 0, 0.48)).toBe(0.475);
    expect(nearestGuide(0.49, [0.5, 0.475], 0.02, 0, 0.47)).toBeNull();
    expect(nearestGuide(0.49, [0.5, 0.475], 0.02, 0.48, 1)).toBe(0.5);
  });

  it("moves a slot, snapping whichever edge is nearest a guide and keeping its size", () => {
    const moved = slotMoved(rect, { dx: -0.01, dy: 0 }, guides, 0.02);
    expect(moved.rect).toEqual({ x: 0.2, y: 0.2, w: 0.3, h: 0.4 });
    const free = slotMoved(rect, { dx: 0.1, dy: 0.05 }, guides, 0.02);
    expect(free.rect.x).toBeCloseTo(0.3, 12);
    expect(free.rect.y).toBeCloseTo(0.25, 12);
    expect([free.rect.w, free.rect.h]).toEqual([0.3, 0.4]);
  });

  it("keeps a moved slot on the page", () => {
    expect(slotMoved(rect, { dx: 5, dy: 5 }, guides, 0).rect).toEqual({ x: 0.7, y: 0.6, w: 0.3, h: 0.4 });
    expect(slotMoved(rect, { dx: -5, dy: -5 }, guides, 0).rect).toEqual({ x: 0, y: 0, w: 0.3, h: 0.4 });
  });

  /**
   * The third anchor a move has and a resize does not. The box's centre lands
   * at 0.492 under the pointer, inside the threshold of the 0.5 centre guide,
   * and neither edge is near anything, so the centre pulls the whole box.
   */
  it("pulls a moved slot's centre onto a centre guide", () => {
    const box = { x: 0.2, y: 0.1, w: 0.3, h: 0.3 };
    const out = slotMoved(box, { dx: 0.142, dy: 0 }, guides, 0.02);
    expect(out.rect.x).toBeCloseTo(0.35, 12);
    expect(out.rect.x + out.rect.w / 2).toBeCloseTo(0.5, 12);
    expect(out.held.x).toBe(0.5);
    expect(out.held.y).toBeNull();
  });

  /**
   * `edit_label` (src-tauri/src/book/history.rs:72) calls a `SetSlot` a move
   * only when the size matches within 1e-9. A width recomputed as
   * `right - left` drifts by ~5e-17 after the centre snap above, which
   * `toBeCloseTo` cannot see and which relabels every move "Undo resize".
   */
  it("keeps a moved slot's size bit-identical, not merely close", () => {
    const box = { x: 0.2, y: 0.1, w: 0.3, h: 0.3 };
    const out = slotMoved(box, { dx: 0.142, dy: 0 }, guides, 0.02);
    expect(Object.is(out.rect.w, box.w)).toBe(true);
    expect(Object.is(out.rect.h, box.h)).toBe(true);
  });

  it("holds nothing when no anchor is near a line", () => {
    const out = slotMoved({ x: 0.2, y: 0.1, w: 0.3, h: 0.3 }, { dx: 0.05, dy: 0.05 }, guides, 0.02);
    expect(out.held).toEqual(NO_HOLD);
  });

  /**
   * A zero correction is a hit, not a miss. Discarding it -- which the old
   * `snapEither` did, to stop an unsnapped edge beating a snapped one --
   * reports a box parked exactly on its guide as held by nothing, and the
   * indicator goes dark under a box that is still snapped.
   */
  it("reports a box already exactly on a line as held by it", () => {
    const out = slotMoved({ x: 0.5, y: 0.1, w: 0.3, h: 0.3 }, { dx: 0, dy: 0 }, guides, 0.02);
    expect(out.rect.x).toBe(0.5);
    expect(out.held.x).toBe(0.5);
    expect(out.held.y).toBeNull();
  });

  it("resizes from a corner with the opposite corner fixed", () => {
    const se = slotResized(rect, "se", { dx: 0.1, dy: 0.1 }, guides, 0, 0.05).rect;
    expect(se.x).toBe(0.2);
    expect(se.y).toBe(0.2);
    expect(se.w).toBeCloseTo(0.4, 12);
    expect(se.h).toBeCloseTo(0.5, 12);
    const nw = slotResized(rect, "nw", { dx: -0.1, dy: -0.1 }, guides, 0, 0.05).rect;
    expect(nw.x).toBeCloseTo(0.1, 12);
    expect(nw.y).toBeCloseTo(0.1, 12);
    expect(nw.x + nw.w).toBeCloseTo(0.5, 12);
    expect(nw.y + nw.h).toBeCloseTo(0.6, 12);
  });

  it("never resizes below the minimum or past the page, and snaps the moving edge", () => {
    const tiny = slotResized(rect, "se", { dx: -5, dy: -5 }, guides, 0, 0.05).rect;
    expect(tiny.w).toBeCloseTo(0.05, 12);
    expect(tiny.h).toBeCloseTo(0.05, 12);
    const huge = slotResized(rect, "se", { dx: 5, dy: 5 }, guides, 0, 0.05).rect;
    expect(huge.x + huge.w).toBe(1);
    expect(huge.y + huge.h).toBe(1);
    const snapped = slotResized(rect, "ne", { dx: -0.01, dy: 0 }, guides, 0.02, 0.05);
    expect(snapped.rect.x + snapped.rect.w).toBeCloseTo(0.5, 12);
    expect(snapped.held.x).toBe(0.5);
  });

  /**
   * A resize is the user steering one EDGE. Dragging the right edge to 0.79
   * puts the box's centre at 0.495, well inside the threshold of the 0.5
   * centre guide -- and nothing may happen, because honouring it would move
   * the edge to 0.81, twice the correction and nowhere near the pointer.
   */
  it("never pulls a resized slot's centre onto a centre guide", () => {
    const out = slotResized({ x: 0.2, y: 0.1, w: 0.3, h: 0.3 }, "se", { dx: 0.29, dy: 0 }, guides, 0.02, 0.05);
    expect(out.rect.x + out.rect.w).toBeCloseTo(0.79, 12);
    expect(out.rect.x + out.rect.w / 2).toBeCloseTo(0.495, 12);
    expect(out.held.x).toBeNull();
  });

  it("locks the aspect ratio when asked, driving from the axis that moved more", () => {
    // 0.3 x 0.4, so a locked resize keeps w/h at 0.75 whatever the pointer does.
    const wide = slotResized(rect, "se", { dx: 0.15, dy: 0.01 }, guides, 0, 0.05, true).rect;
    expect(wide.w / wide.h).toBeCloseTo(0.75, 12);
    expect(wide.w).toBeCloseTo(0.45, 12);
    expect(wide.h).toBeCloseTo(0.6, 12);
    const tall = slotResized(rect, "se", { dx: 0.01, dy: 0.2 }, guides, 0, 0.05, true).rect;
    expect(tall.w / tall.h).toBeCloseTo(0.75, 12);
    expect(tall.h).toBeCloseTo(0.6, 12);
  });

  it("keeps the opposite corner fixed under a locked resize", () => {
    const nw = slotResized(rect, "nw", { dx: -0.15, dy: -0.01 }, guides, 0, 0.05, true).rect;
    expect(nw.x + nw.w).toBeCloseTo(0.5, 12);
    expect(nw.y + nw.h).toBeCloseTo(0.6, 12);
    expect(nw.w / nw.h).toBeCloseTo(0.75, 12);
    const ne = slotResized(rect, "ne", { dx: 0.15, dy: -0.01 }, guides, 0, 0.05, true).rect;
    expect(ne.x).toBeCloseTo(0.2, 12);
    expect(ne.y + ne.h).toBeCloseTo(0.6, 12);
    expect(ne.w / ne.h).toBeCloseTo(0.75, 12);
  });

  it("holds the ratio at both limits: the minimum size and the page edge", () => {
    const tiny = slotResized(rect, "se", { dx: -5, dy: -5 }, guides, 0, 0.05, true).rect;
    expect(tiny.w / tiny.h).toBeCloseTo(0.75, 12);
    expect(Math.min(tiny.w, tiny.h)).toBeCloseTo(0.05, 12);
    const huge = slotResized(rect, "se", { dx: 5, dy: 5 }, guides, 0, 0.05, true).rect;
    expect(huge.w / huge.h).toBeCloseTo(0.75, 12);
    expect(huge.x + huge.w).toBeLessThanOrEqual(1 + 1e-12);
    expect(huge.y + huge.h).toBeLessThanOrEqual(1 + 1e-12);
    // It fills the page in whichever direction runs out first, and no further.
    expect(Math.max(huge.x + huge.w, huge.y + huge.h)).toBeCloseTo(1, 12);

    // A slot WIDER than it is tall runs out on its height first: shrunk to
    // 0.05 of width it would be 0.025 tall, half the minimum. The floor has
    // to scale with the ratio, and a tall slot like `rect` never shows it --
    // for anything narrower than square the two floors are the same number,
    // so the case above passes with the scaling dropped entirely.
    const squat = { x: 0.2, y: 0.2, w: 0.4, h: 0.2 };
    const squashed = slotResized(squat, "se", { dx: -5, dy: -5 }, guides, 0, 0.05, true).rect;
    expect(squashed.w / squashed.h).toBeCloseTo(2, 12);
    expect(squashed.h).toBeCloseTo(0.05, 12);
    expect(squashed.w).toBeCloseTo(0.1, 12);
  });

  it("ignores snapping while the ratio is locked, because a snapped edge breaks it", () => {
    // dx alone would pull the right edge onto the 0.5 guide and leave h at 0.4.
    const locked = slotResized(rect, "se", { dx: -0.01, dy: 0 }, guides, 0.02, 0.05, true);
    expect(locked.rect.w).toBeCloseTo(0.29, 12);
    expect(locked.rect.x + locked.rect.w).toBeCloseTo(0.49, 12);
    expect(locked.rect.h).toBeCloseTo(0.29 / 0.75, 12);
    const free = slotResized(rect, "se", { dx: -0.01, dy: 0 }, guides, 0.02, 0.05);
    expect(free.rect.x + free.rect.w).toBeCloseTo(0.5, 12);
    expect(free.rect.h).toBeCloseTo(0.4, 12);
  });

  it("holds nothing while the ratio is locked, so the indicator goes dark with shift", () => {
    const locked = slotResized(rect, "se", { dx: -0.01, dy: 0 }, guides, 0.02, 0.05, true);
    expect(locked.held).toEqual(NO_HOLD);
    const free = slotResized(rect, "se", { dx: -0.01, dy: 0 }, guides, 0.02, 0.05);
    expect(free.held.x).toBe(0.5);
  });

  it("builds page guides from the engine's own geometry plus the other slots", () => {
    const g = pageGuides(geometry, "left", [{ x: 0.1, y: 0.3, w: 0.2, h: 0.2 }]);
    expect(g.edges.xs).toContain(0);
    expect(g.edges.xs).toContain(1);
    expect(g.edges.xs.some((x) => Math.abs(x - geometry.left.trim.x) < 1e-6)).toBe(true);
    expect(g.edges.xs.some((x) => Math.abs(x - geometry.left.gutter.x) < 1e-6)).toBe(true);
    expect(g.edges.xs).toContain(0.1);
    expect(g.edges.xs.some((x) => Math.abs(x - 0.3) < 1e-6)).toBe(true);
    expect(g.edges.ys).toContain(0.3);
    expect(g.edges.ys.some((y) => Math.abs(y - 0.5) < 1e-6)).toBe(true);
    expect(g.edges.xs.toSorted((a, b) => a - b)).toEqual(g.edges.xs);
  });

  /**
   * Exactly one horizontal centre, the trim centre. A left page is asymmetric
   * because the fold carries no bleed, so its canvas centre (0.5) and safe
   * centre (0.514379) sit within one 0.012 threshold of the trim centre
   * (0.508797); offering all three would make one snap three ways.
   */
  it("offers one page centre per axis, the trim centre, plus the other slots'", () => {
    const g = pageGuides(geometry, "left", [{ x: 0.1, y: 0.3, w: 0.2, h: 0.2 }]);
    const cx = geometry.left.trim.x + geometry.left.trim.w / 2;
    expect(cx).toBeCloseTo(0.508797, 6);
    expect(g.centres.xs).toEqual([0.2, Math.round(cx * 1e6) / 1e6]);
    expect(g.centres.ys).toEqual([0.4, 0.5]);
    // The trim centre is a target for an edge too; the other slots' are not.
    expect(g.edges.xs.some((x) => Math.abs(x - cx) < 1e-6)).toBe(true);
    expect(g.edges.xs.some((x) => Math.abs(x - 0.2) < 1e-6)).toBe(false);
  });

  it("draws a held line as a zero-thickness box across the page", () => {
    expect(snapLines({ x: 0.25, y: null })).toEqual([
      { axis: "x", style: { left: "25%", top: "0%", width: "0%", height: "100%" } },
    ]);
    expect(snapLines({ x: null, y: 0.5 })).toEqual([
      { axis: "y", style: { left: "0%", top: "50%", width: "100%", height: "0%" } },
    ]);
    expect(snapLines(NO_HOLD)).toEqual([]);
    expect(snapLines({ x: 0.25, y: 0.5 }).map((line) => line.axis)).toEqual(["x", "y"]);
  });

  it("sends the whole rect for a slot edit", () => {
    expect(setSlotEdit({ page: 2, z: 1 }, rect)).toEqual({ kind: "setSlot", placement: { page: 2, z: 1 }, rect });
  });
});

function indices(rows: { index: number }[]): number[] {
  return rows.map((row) => row.index);
}

describe("choosing a photo for a slot", () => {
  // The fixture places photos 3 (page 1), 1 and 0 (page 2); 2 and 4 are left
  // out. Scores 62, 18, 91, 40, 77; photo 2 has no capture time.
  const target: PickTarget = { kind: "slot", placement: { page: 2, z: 1 } };
  const candidates: SlotCandidate[] = layout.photos.map((_, i) => ({
    crop: { x: i / 10, y: 0, w: 0.5, h: 1 },
    refused: i === 4 ? "faceInGutter" : null,
  }));

  it("offers the left-out photos, the placed ones, or all", () => {
    expect(indices(replaceCandidates(layout, candidates, target, "leftOut", "best")).toSorted()).toEqual([2, 4]);
    expect(indices(replaceCandidates(layout, candidates, target, "inBook", "best")).toSorted()).toEqual([0, 1, 3]);
    expect(indices(replaceCandidates(layout, candidates, target, "all", "best"))).toHaveLength(5);
  });

  it("sorts best first, or by time taken with undated photos last", () => {
    expect(indices(replaceCandidates(layout, candidates, target, "all", "best"))).toEqual([2, 4, 0, 3, 1]);
    expect(indices(replaceCandidates(layout, candidates, target, "all", "taken"))).toEqual([1, 4, 0, 3, 2]);
  });

  it("pairs each photo with its own crop and refusal, and says where a placed one is", () => {
    const rows = replaceCandidates(layout, candidates, target, "all", "best");
    const row = (index: number) => rows.find((r) => r.index === index);
    expect(row(4)?.crop.x).toBeCloseTo(0.4, 12);
    expect(row(4)?.refused).toBe("faceInGutter");
    expect(row(2)?.refused).toBeNull();
    expect(row(2)?.placedAt).toBeNull();
    expect(row(3)?.placedAt).toEqual({ page: 1, z: 1 });
    expect(row(0)?.placedAt).toEqual({ page: 2, z: 2 });
    expect(row(1)?.current).toBe(true);
    expect(rows.filter((r) => r.current)).toHaveLength(1);
  });

  it("marks a photo on a locked page, which cannot be swapped", () => {
    const locked: BookLayout = {
      ...layout,
      openings: layout.openings.map((o) => (o.index === 0 ? { ...o, locked: true } : o)),
    };
    const rows = replaceCandidates(locked, candidates, target, "all", "best");
    expect(rows.filter((r) => r.locked).map((r) => r.index)).toEqual([3]);
  });

  it("frames each tile at the slot's printed shape and replaces the photo there", () => {
    expect(pickAspect(layout, target)).toBeCloseTo((0.5 * 11.197) / (0.62 * 8.894), 12);
    expect(pickEdit(target, 2)).toEqual({ kind: "replacePhoto", placement: { page: 2, z: 1 }, photo: 2 });
  });

  it("puts every refusal into words", () => {
    const reasons: Rejection[] = ["faceClipped", "faceInGutter", "faceInSafeMargin", "tooLowResolution"];
    const texts = reasons.map((r) => refusalText(r, "slot"));
    expect(new Set(texts).size).toBe(reasons.length);
    for (const text of texts) expect(text.length).toBeGreaterThan(10);
  });
});

describe("adding a photo to a blank page", () => {
  // Page 1 is the one this target fills; the fixture places 3 there and 1, 0
  // on page 2. Photos 2 and 4 are left out.
  const target: PickTarget = { kind: "opening", opening: 0, page: 1 };
  const candidates: SlotCandidate[] = layout.photos.map(() => ({
    crop: { x: 0, y: 0, w: 1, h: 0.9 },
    refused: null,
  }));
  const rows = replaceCandidates(layout, candidates, target, "all", "best");
  const block = (index: number) => pickBlock(rows.find((r) => r.index === index)!, target);

  it("sends an AddPhoto for the opening, not a swap", () => {
    expect(pickEdit(target, 2)).toEqual({ kind: "addPhoto", opening: 0, photo: 2 });
  });

  /** Adding a photo the book already holds would print it twice, so Rust refuses it; the tile says so first. */
  it("blocks every photo already in the book, and only those", () => {
    expect([0, 1, 3].map(block)).toEqual(["Already used", "Already used", "Already used"]);
    expect([2, 4].map(block)).toEqual([null, null]);
  });

  it("has no current photo and no locks, since nothing trades places", () => {
    const locked: BookLayout = {
      ...layout,
      openings: layout.openings.map((o) => ({ ...o, locked: true })),
    };
    const lockedRows = replaceCandidates(locked, candidates, target, "all", "best");
    expect(lockedRows.filter((r) => r.current || r.locked)).toEqual([]);
  });

  it("frames each tile at the whole page's printed shape", () => {
    expect(pickAspect(layout, target)).toBeCloseTo(layout.geometry.pageWIn / layout.geometry.pageHIn, 12);
  });
});

describe("pickBlock on a slot", () => {
  const target: PickTarget = { kind: "slot", placement: { page: 2, z: 1 } };
  const candidates: SlotCandidate[] = layout.photos.map((_, i) => ({
    crop: { x: 0, y: 0, w: 0.5, h: 1 },
    refused: i === 4 ? "faceInGutter" : null,
  }));
  const locked: BookLayout = {
    ...layout,
    openings: layout.openings.map((o) => (o.index === 0 ? { ...o, locked: true } : o)),
  };
  const rows = replaceCandidates(locked, candidates, target, "all", "best");
  const block = (index: number) => pickBlock(rows.find((r) => r.index === index)!, target);

  it("lets a placed photo swap in, and blocks the current, the locked and the refused", () => {
    expect(block(0)).toBeNull();
    expect(block(1)).toBe("In this slot now");
    expect(block(3)).toBe("On a locked page");
    expect(block(4)).toBe(refusalText("faceInGutter", "slot"));
    expect(block(2)).toBeNull();
  });
});

describe("choosing a photo for the cover", () => {
  // The fixture's front cover is photo 4, which no page places; the back is empty.
  const front: PickTarget = { kind: "cover", side: "front" };
  const back: PickTarget = { kind: "cover", side: "back" };
  const candidates: SlotCandidate[] = layout.photos.map((_, i) => ({
    crop: { x: 0, y: i / 20, w: 1, h: 0.6 },
    refused: i === 2 ? "faceInSafeMargin" : null,
  }));

  it("marks the photo on that side as current, and nothing on an empty side", () => {
    expect(replaceCandidates(layout, candidates, front, "all", "best").filter((r) => r.current).map((r) => r.index)).toEqual([4]);
    expect(replaceCandidates(layout, candidates, back, "all", "best").filter((r) => r.current)).toEqual([]);
  });

  /** A cover photo may also be on a page, so a lock on that page does not stand in the way. */
  it("never marks a photo locked, since the cover takes a copy rather than a swap", () => {
    const locked: BookLayout = {
      ...layout,
      openings: layout.openings.map((o) => (o.index === 0 ? { ...o, locked: true } : o)),
    };
    const rows = replaceCandidates(locked, candidates, front, "all", "best");
    expect(rows.find((r) => r.index === 3)?.placedAt).toEqual({ page: 1, z: 1 });
    expect(rows.filter((r) => r.locked)).toEqual([]);
  });

  it("frames each tile at the cover panel's shape and sets that side's photo", () => {
    expect(pickAspect(layout, front)).toBe(layout.cover.aspect);
    expect(pickEdit(back, 2)).toEqual({ kind: "setCoverPhoto", side: "back", photo: 2 });
  });

  /** On the cover the margin a face strays into is mostly the wrap, as Rust's own refusal says. */
  it("words a face refusal as the fold under the board", () => {
    expect(refusalText("faceInSafeMargin", "cover")).toBe("A face would fold under the board or sit too near its edge");
    expect(refusalText("faceInSafeMargin", "slot")).toBe("A face would sit in the trim margin");
    expect(refusalText("tooLowResolution", "cover")).toBe(refusalText("tooLowResolution", "slot"));
  });

  it("saves a dragged cover crop as that side's crop", () => {
    expect(setCoverCropEdit("back", { x: 0.1, y: 0.2, w: 0.5, h: 0.4 })).toEqual({
      kind: "setCoverCrop",
      side: "back",
      x: 0.1,
      y: 0.2,
      w: 0.5,
    });
  });
});

describe("click versus drag", () => {
  it("calls a press a click until it has travelled the threshold", () => {
    expect(pressIsDrag(0, 0)).toBe(false);
    expect(pressIsDrag(3, 0)).toBe(false);
    expect(pressIsDrag(6, 6)).toBe(false);
    expect(pressIsDrag(10, 0)).toBe(true);
    expect(pressIsDrag(-10, 0)).toBe(true);
    expect(pressIsDrag(0, 40)).toBe(true);
  });

  it("does not care which way the press travelled", () => {
    expect(pressIsDrag(1, 0)).toBe(false);
    expect(pressIsDrag(-20, 0)).toBe(true);
    expect(pressIsDrag(0, -20)).toBe(true);
  });

  it("takes the threshold from the caller, defaulting to the generous one", () => {
    expect(pressIsDrag(4, 0, SLOT_DRAG_THRESHOLD_PX)).toBe(true);
    expect(pressIsDrag(4, 0)).toBe(false);
    expect(pressIsDrag(4, 0, DRAG_THRESHOLD_PX)).toBe(false);
  });

  // A slot that cannot be nudged by less than ten pixels is the complaint.
  // The floor exists only to stop tremor during a press committing an edit,
  // which is a far smaller job than telling a click from a drag.
  it("keeps a floor under the slot threshold rather than dropping it", () => {
    expect(SLOT_DRAG_THRESHOLD_PX).toBeGreaterThan(0);
    expect(SLOT_DRAG_THRESHOLD_PX).toBeLessThan(DRAG_THRESHOLD_PX);
    expect(pressIsDrag(1, 0, SLOT_DRAG_THRESHOLD_PX)).toBe(false);
  });
});

/**
 * The two gestures on a slot want different thresholds because a press that
 * never travels means different things in each.
 *
 * In crop mode a click selects the photo for a swap, so the threshold has to
 * be generous enough to absorb the tremor in a real trackpad click -- at 3px
 * a click on the cover registered as a tiny crop drag and the picker never
 * opened. In layout mode `onSlotPointerUp` has no click branch at all: a
 * press that does not travel discards its live rect and does nothing. So
 * there the ten pixels protected nothing and cost the user every nudge
 * smaller than ten pixels, since the move applies the whole offset from the
 * origin the moment the threshold is crossed.
 *
 * Pinned as source for the same reason as the block above: a pointer gesture
 * is out of reach of a unit test.
 */
describe("a slot nudge and a crop drag do not share a threshold", () => {
  const text = readFileSync(
    new URL("../app/components/BookPreviewPage.vue", import.meta.url),
    "utf8",
  );
  const body = (name: string) =>
    text.match(new RegExp(`function ${name}\\([\\s\\S]*?\\n}`))?.[0] ?? "";

  it("moves a slot on the smaller threshold", () => {
    expect(body("onSlotPointerMove")).toContain("pressIsDrag(dxPx, dyPx, SLOT_DRAG_THRESHOLD_PX)");
  });

  it("leaves the photo drag on the generous default", () => {
    const move = body("onPointerMove");
    expect(move).toMatch(/pressIsDrag\([^)]*\)\)/);
    expect(move).not.toContain("SLOT_DRAG_THRESHOLD_PX");
  });

  it("still has no click branch in layout mode, which is why this is safe", () => {
    // If a click on a slot ever comes to mean something, the small threshold
    // stops being free and this test is the place that says so.
    expect(body("onSlotPointerUp")).not.toMatch(/emit\("(select|swap)"/);
  });
});

/**
 * The snap indicator is lit only while a gesture is running. `held` has one
 * setter, in `onSlotPointerMove`, which is reachable only while `slotDrag` is
 * set, and a reset beside each of the two places a drag ends. Leave one out
 * and the hairline stays painted over a page nobody is dragging.
 *
 * A pointer gesture is out of reach of a unit test, so this pins it as source
 * the way the block above does.
 */
describe("the snap indicator goes dark when the gesture ends", () => {
  const text = readFileSync(
    new URL("../app/components/BookPreviewPage.vue", import.meta.url),
    "utf8",
  );
  const body = (name: string) =>
    text.match(new RegExp(`function ${name}\\([\\s\\S]*?\\n}`))?.[0] ?? "";

  it.each(["onSlotPointerUp", "onSlotPointerCancel"])("%s clears the held lines", (name) => {
    expect(body(name)).toContain("slotDrag = null;");
    expect(body(name)).toContain("held.value = NO_HOLD;");
  });

  it("sets the held lines only where the drag is live", () => {
    expect([...text.matchAll(/held\.value = (?!NO_HOLD)/g)]).toHaveLength(1);
    expect(body("onSlotPointerMove")).toContain("held.value = next.held;");
  });
});

/**
 * On a page, a press that becomes a drag is handed to the book's photo drag
 * (`usePhotoDrag`), which owns the release from then on. The page must forget
 * the press BEFORE handing it over: otherwise the page's own pointerup would
 * still read it as a click and select the photo the user just dropped
 * somewhere else. A pointer gesture is out of reach of a unit test, so this
 * pins it as source, like the blocks above.
 */
describe("a page photo drag is handed over, not also clicked", () => {
  const text = readFileSync(new URL("../app/components/BookPreviewPage.vue", import.meta.url), "utf8");
  const body = (name: string) => text.match(new RegExp(`function ${name}\\([\\s\\S]*?\\n}`))?.[0] ?? "";

  it("clears the press before starting the drag", () => {
    const move = body("onPointerMove");
    const cleared = move.indexOf("press = null;");
    const started = move.indexOf("photoDrag?.start(");
    expect(cleared).toBeGreaterThan(-1);
    expect(started).toBeGreaterThan(cleared);
  });

  it("selects on release only from a press still held", () => {
    const up = body("onPointerUp");
    expect(up).toMatch(/if \(!press\) return;/);
    expect(up).toContain('emit("select"');
  });
});

/**
 * An inset ring is a box-shadow, and a box-shadow paints UNDER the element's
 * children, so a ring on the slot itself is hidden by the photo filling it.
 * That is how the selection ring went unseen since it was written, and how the
 * swap target and the post-edit flash first did too: every one has to be drawn
 * on an overlay that comes after the photo.
 */
describe("slot highlights paint over the photo, not under it", () => {
  const text = readFileSync(new URL("../app/components/BookPreviewPage.vue", import.meta.url), "utf8");
  const slotOpen = text.match(/<component\s[\s\S]*?:data-placement[\s\S]*?\n {6}>/)?.[0] ?? "";

  it("keeps rings and the flash off the slot element itself", () => {
    expect(slotOpen).toContain(":data-placement");
    expect(slotOpen).not.toContain("ring-inset");
    expect(slotOpen).not.toContain("slot-flash");
  });

  it("draws them after the photo", () => {
    const photo = text.indexOf(':src="box.src"');
    expect(photo).toBeGreaterThan(-1);
    expect(text.indexOf("ring-inset", photo)).toBeGreaterThan(photo);
    expect(text.indexOf("'slot-flash'", photo)).toBeGreaterThan(photo);
  });
});

/**
 * The Undo button and the Cmd-Z that does the same thing have to agree about
 * when stepping is allowed, and they did not: the button was disabled while a
 * command was in flight and the shortcut fired regardless, so holding Cmd-Z
 * during a save sent overlapping `step_book` calls and the book on screen was
 * whichever reply landed last, not where the cursor ended up. One predicate
 * now answers for both.
 */
describe("canStep", () => {
  const both: HistoryStatus = { undo: "resize", redo: "crop" };

  it("allows a step the timeline has, when nothing else is running", () => {
    expect(canStep("undo", both, false)).toBe(true);
    expect(canStep("redo", both, false)).toBe(true);
  });

  it("refuses the end of the timeline", () => {
    expect(canStep("undo", { undo: null, redo: "crop" }, false)).toBe(false);
    expect(canStep("redo", { undo: "resize", redo: null }, false)).toBe(false);
  });

  it("refuses while a command is in flight, whichever way", () => {
    expect(canStep("undo", both, true)).toBe(false);
    expect(canStep("redo", both, true)).toBe(false);
  });
});

describe("the editor's undo and redo controls all ask canStep", () => {
  const text = readFileSync(new URL("../app/components/BookEditor.vue", import.meta.url), "utf8");

  it("sends every step through the one guarded caller", () => {
    // Two mentions and no more: the `useBook` destructure, and the call inside
    // `step`. A button or shortcut wired straight to `stepBook` is the drift
    // that let the shortcut forget `busy` in the first place.
    expect([...text.matchAll(/stepBook\b/g)].length).toBe(2);
    expect(text).toMatch(/if \(canStep\(which, history\.value, busy\.value\)\) void stepBook\(which\)/);
  });

  it.each(["undo", "redo"])("disables the %s button from the same predicate the shortcut reads", (which) => {
    expect(text).toContain(`:disabled="!canStep('${which}', history, busy)"`);
    expect(text).toContain(`@click="step('${which}')"`);
    expect(text).toContain(`[shortcutCombo("${which}")]: () => step("${which}")`);
  });
});

describe("stepLabel", () => {
  it("names the edit each control would move over", () => {
    const status = { undo: "resize", redo: "photo replacement" };
    expect(stepLabel("undo", status)).toBe("Undo resize");
    expect(stepLabel("redo", status)).toBe("Redo photo replacement");
  });

  it("reads each end of the timeline from its own side", () => {
    // A book edited and never undone can be undone but not redone, so the
    // two controls must not read the same status field.
    expect(stepLabel("redo", { undo: "crop", redo: null })).toBe("Nothing to redo");
    expect(stepLabel("undo", { undo: "crop", redo: null })).toBe("Undo crop");
    expect(stepLabel("undo", { undo: null, redo: "crop" })).toBe("Nothing to undo");
    expect(stepLabel("redo", { undo: null, redo: "crop" })).toBe("Redo crop");
  });
});

describe("a photo imported from disk", () => {
  const target: PickTarget = { kind: "slot", placement: { page: 2, z: 1 } };
  const extra: PreviewPhoto = {
    ...layout.photos[0]!,
    path: "/elsewhere/IMG_9999.JPG",
    hash: "imported",
  };
  // `import_photo` APPENDS, so the index it hands back is one past the end of
  // the layout the dialog is holding.
  const index = layout.photos.length;
  const shown = withImportedPhotos(layout, new Map([[index, extra]]));
  const candidates: SlotCandidate[] = shown.photos.map(() => ({
    crop: { x: 0, y: 0, w: 0.5, h: 1 },
    refused: null,
  }));

  it("leaves the layout alone when nothing was imported", () => {
    expect(withImportedPhotos(layout, new Map())).toBe(layout);
  });

  it("gets a tile of its own, which is the only reason to import it", () => {
    const rows = replaceCandidates(shown, candidates, target, "all", "best");
    expect(indices(rows)).toContain(index);
    expect(rows.find((row) => row.index === index)?.photo.path).toBe("/elsewhere/IMG_9999.JPG");
  });

  it("counts as left out, since importing it does not place it", () => {
    expect(indices(replaceCandidates(shown, candidates, target, "leftOut", "best"))).toContain(index);
    expect(indices(replaceCandidates(shown, candidates, target, "inBook", "best"))).not.toContain(index);
  });

  it("does not disturb the photos the book already held", () => {
    expect(shown.photos.slice(0, index)).toEqual(layout.photos);
    expect(layout.photos).toHaveLength(index);
  });
});

// Titles are an on-screen guide and never print, so the export sheet must not
// hand them over to be typed into Pixajoy as if they were part of the book.
describe("the export sheet lists no chapter titles", () => {
  it("BookEditor.vue reads no chapter title into the export sheet", () => {
    const text = readFileSync(new URL("../app/components/BookEditor.vue", import.meta.url), "utf8");
    expect(text).not.toMatch(/chapters|chapter\.title|clipboard/);
  });
});

describe("the scrolling book leaves the wheel alone", () => {
  // Zooming a photo with the wheel on the page fought the book's own scroll:
  // two rounds of arbitration (a modifier key, then a gesture latch) and the
  // user still reported it bleeding into scrolling. Zoom moved to the crop
  // dialog, where nothing behind it scrolls. A wheel handler back on a page
  // photo or a cover panel brings the fight back.
  for (const file of ["BookPreviewPage.vue", "BookPreviewCover.vue"]) {
    it(`${file} has no wheel handler`, () => {
      const text = readFileSync(new URL(`../app/components/${file}`, import.meta.url), "utf8");
      expect(text).not.toMatch(/@wheel|addEventListener\("wheel"/);
    });
  }

  it("the crop dialog does, and takes the wheel from the page", () => {
    const text = readFileSync(new URL("../app/components/CropPhotoDialog.vue", import.meta.url), "utf8");
    expect(text).toContain('@wheel="onWheel"');
    expect(text).toContain("event.preventDefault();");
  });
});

describe("chapter titles in the editor", () => {
  const chapter = layout.chapters[0]!;

  it("marks a chapter's title spot on the page its box is on, and only there", () => {
    expect(chapter.titleBox?.page).toBe(3);
    const drawn = pageTitle(layout, 3);
    expect(drawn?.text).toBe("Reykjavík");
    expect(drawn?.align).toBe("center");
    expect(drawn?.box).toEqual(rectStyle(chapter.titleBox!.rect));
    expect(pageTitle(layout, 2)).toBeNull();
  });

  it("draws nothing for a chapter the user left untitled", () => {
    const untitled = structuredClone(layout);
    untitled.chapters[0]!.title = "";
    expect(pageTitle(untitled, 3)).toBeNull();
  });

  it("heads the spread a chapter opens on, and no other", () => {
    const spreads = toSpreads(layout.pages);
    expect(spreads.map((spread) => chapterOpening(layout, spread)?.number ?? null)).toEqual([null, 1, null]);
  });

  describe("titleEdit", () => {
    const auto = { ...chapter, custom: false, title: chapter.autoTitle };

    it("sends nothing when the title did not change", () => {
      expect(titleEdit(chapter, "Reykjavík")).toBeNull();
      expect(titleEdit(chapter, "  Reykjavík ")).toBeNull();
      expect(titleEdit(auto, auto.autoTitle)).toBeNull();
    });

    it("sends the trimmed new title", () => {
      expect(titleEdit(auto, " Golden Circle ")).toEqual({
        kind: "setChapterTitle",
        event: 4,
        title: "Golden Circle",
      });
    });

    // Typing the automatic title back is going back to automatic, so a later
    // place name can still replace it.
    it("goes back to automatic when the automatic title is typed back", () => {
      expect(titleEdit(chapter, chapter.autoTitle)).toEqual({ kind: "setChapterTitle", event: 4, title: null });
    });

    it("sends an empty title, which prints none", () => {
      expect(titleEdit(chapter, "   ")).toEqual({ kind: "setChapterTitle", event: 4, title: "" });
    });
  });
});

// The page viewer steps one opening at a time and stops at either end rather
// than wrapping: a book has a first and a last page, and wrapping from the
// back cover to page 1 reads as a jump, not a turn.
describe("viewerStep", () => {
  it("moves one opening either way", () => {
    expect(viewerStep(3, 1, 10)).toBe(4);
    expect(viewerStep(3, -1, 10)).toBe(2);
  });

  it("stops at the first and the last opening", () => {
    expect(viewerStep(0, -1, 10)).toBe(0);
    expect(viewerStep(9, 1, 10)).toBe(9);
  });

  it("keeps an index the book has since shrunk past inside it", () => {
    expect(viewerStep(12, 0, 10)).toBe(9);
    expect(viewerStep(4, 0, 0)).toBe(0);
  });
});

describe("crop dialog geometry", () => {
  // A landscape slot's window on a landscape photo: as wide as the photo can
  // give is w = 1, so this window is zoomed 2x.
  const crop = { x: 0.2, y: 0.1, w: 0.5, h: 0.25 };

  it("reads the zoom as how much narrower the window is than the widest it can be", () => {
    expect(cropZoom(crop)).toBeCloseTo(2, 12);
    expect(cropZoom({ x: 0, y: 0, w: 1, h: 0.5 })).toBeCloseTo(1, 12);
    // A tall window on the photo is limited by its height, not its width.
    expect(cropZoom({ x: 0.3, y: 0, w: 0.25, h: 1 })).toBeCloseTo(1, 12);
  });

  it("sets a zoom exactly, about the window's centre, keeping its shape", () => {
    const four = cropAtZoom(crop, 4);
    expect(cropZoom(four)).toBeCloseTo(4, 12);
    expect(four.h / four.w).toBeCloseTo(crop.h / crop.w, 12);
    expect(four.x + four.w / 2).toBeCloseTo(crop.x + crop.w / 2, 12);
    expect(four.y + four.h / 2).toBeCloseTo(crop.y + crop.h / 2, 12);
  });

  it("clamps a zoom to between the whole photo and the most it allows", () => {
    expect(cropZoom(cropAtZoom(crop, 0.1))).toBeCloseTo(1, 12);
    expect(cropZoom(cropAtZoom(crop, 1000))).toBeCloseTo(MAX_CROP_ZOOM, 12);
  });

  it("moves the window the way it is dragged, in fractions of the photo", () => {
    // The user drags the window over the whole photo, so it follows the pointer 1:1.
    const moved = windowMoved(crop, { dx: 0.1, dy: 0.05 });
    expect(moved.x).toBeCloseTo(0.3, 12);
    expect(moved.y).toBeCloseTo(0.15, 12);
    expect([moved.w, moved.h]).toEqual([crop.w, crop.h]);
  });

  it("stops the window at the photo's edges", () => {
    expect(windowMoved(crop, { dx: 9, dy: 9 })).toEqual({ x: 0.5, y: 0.75, w: 0.5, h: 0.25 });
    expect(windowMoved(crop, { dx: -9, dy: -9 })).toEqual({ x: 0, y: 0, w: 0.5, h: 0.25 });
  });
});

describe("cropSubject", () => {
  it("finds the placement's own photo and saved crop, not the page's first", () => {
    const page = layout.pages.find((p) => p.placements.length > 1);
    if (!page) throw new Error("fixture has no page with two photos");
    const second = page.placements[1];
    const subject = cropSubject(layout, { kind: "slot", placement: { page: page.number, z: second.z } });
    expect(subject?.crop).toEqual(second.crop);
    expect(subject?.photo).toBe(layout.photos[second.photoIndex]);
    expect(subject?.label).toBe(`Photo on page ${page.number}`);
  });

  it("finds a cover side's photo, and nothing for a side without one or a missing slot", () => {
    const side = layout.cover.front.photo ? "front" : "back";
    const cover = layout.cover[side].photo;
    const subject = cropSubject(layout, { kind: "cover", side });
    if (cover) {
      expect(subject?.crop).toEqual(cover.crop);
      expect(subject?.photo).toBe(layout.photos[cover.photoIndex]);
    } else {
      expect(subject).toBeNull();
    }
    expect(cropSubject(layout, { kind: "slot", placement: { page: 9999, z: 0 } })).toBeNull();
  });
});

describe("escapeDeselects", () => {
  const key = { key: "Escape", inField: false, inOverlay: false };

  it("clears the selection on Esc wherever focus is, even on the page itself", () => {
    // WebKit never focuses a clicked <button>, so in the app the key lands on
    // <body>, outside the book. Esc must still clear the selection there.
    expect(escapeDeselects(key)).toBe(true);
  });

  it("leaves other keys alone", () => {
    expect(escapeDeselects({ ...key, key: "Enter" })).toBe(false);
    expect(escapeDeselects({ ...key, key: "Esc" })).toBe(true);
  });

  it("lets an open dialog or menu take the Esc instead", () => {
    expect(escapeDeselects({ ...key, inOverlay: true })).toBe(false);
  });

  it("lets a text field take the Esc, so renaming a chapter can be cancelled", () => {
    expect(escapeDeselects({ ...key, inField: true })).toBe(false);
  });
});
