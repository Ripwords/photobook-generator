import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  cropStyle,
  gutterRect,
  leftOutPhotos,
  pageSlots,
  photoFor,
  rectStyle,
  safeRect,
  slotStyle,
  toSpreads,
  trimRect,
  type BookLayout,
  type PreviewGeometry,
  type PreviewPage,
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
  it("carries every key the preview reads, including a page that holds nothing", () => {
    expect(layout.projectId).toBe(7);
    expect(layout.pageCount).toBe(4);
    expect(layout.placedPhotos).toBe(3);
    expect(layout.droppedPhotos).toBe(2);
    expect(layout.pages).toHaveLength(4);
    expect(layout.photos).toHaveLength(5);
    // A real template id on a page that still prints white -- the text-zone
    // case, which is the common one.
    expect(layout.pages[2]?.templateId).toBe("03-hero-left-text-right");
    expect(layout.pages[2]?.blank).toBe(true);
    expect(layout.pages[2]?.placements).toHaveLength(0);
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
    expect(seen.filter((p) => p.blank).map((p) => p.number)).toEqual([3, 4]);
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

describe("slotStyle", () => {
  it("places the slot as a percentage of the page box", () => {
    expect(slotStyle({ x: 0.06, y: 0.08, w: 0.5, h: 0.62 })).toEqual({
      left: "6%",
      top: "8%",
      width: "50%",
      height: "62%",
    });
  });
});

describe("the guides", () => {
  /**
   * The named regression: **a guide drawn at the wrong place.** These mirror
   * `geometry::in_trim`, `in_safe_margin` and `clear_of_gutter` exactly, using
   * the constants the engine itself shipped on the wire.
   */
  it("insets the trim on the outer edge but never at the fold", () => {
    const left = trimRect(geometry, "left");
    // A LEFT page's outer edge is x=0; its fold edge is x=1.
    expect(left.x).toBeCloseTo(geometry.trimU, 12);
    expect(left.x + left.w).toBeCloseTo(1, 12);

    const right = trimRect(geometry, "right");
    // A RIGHT page is the mirror: fold at x=0, outer edge at x=1.
    expect(right.x).toBeCloseTo(0, 12);
    expect(right.x + right.w).toBeCloseTo(1 - geometry.trimU, 12);

    for (const rect of [left, right]) {
      expect(rect.y).toBeCloseTo(geometry.trimV, 12);
      expect(rect.y + rect.h).toBeCloseTo(1 - geometry.trimV, 12);
    }
  });

  /**
   * `in_safe_margin` is a STRICT subset of `in_trim` -- the same property
   * `geometry_in_safe_margin_is_a_strict_subset_of_in_trim` pins in Rust. A
   * safe guide wired to the trim inset would draw two lines on top of each
   * other and look entirely plausible doing it.
   */
  it("insets the safe margin strictly inside the trim, except at the fold", () => {
    const trim = trimRect(geometry, "left");
    const safe = safeRect(geometry, "left");

    expect(safe.x).toBeGreaterThan(trim.x);
    expect(safe.y).toBeGreaterThan(trim.y);
    expect(safe.y + safe.h).toBeLessThan(trim.y + trim.h);
    // The fold edge is NOT inset a second time -- the gutter band governs it.
    expect(safe.x + safe.w).toBeCloseTo(trim.x + trim.w, 12);
    expect(safe.x).toBeCloseTo(geometry.trimU + geometry.safeU, 12);

    const rightSafe = safeRect(geometry, "right");
    expect(rightSafe.x).toBeCloseTo(0, 12);
    expect(rightSafe.x + rightSafe.w).toBeCloseTo(1 - geometry.trimU - geometry.safeU, 12);
  });

  /**
   * The gutter dead band is measured inward FROM THE FOLD, so it sits on the
   * INNER edge of each page: x=1 on a left page, x=0 on a right one. Putting
   * it on the outer edge is the mistake this pins.
   */
  it("puts the gutter band against the fold on both pages, never the outer edge", () => {
    const left = gutterRect(geometry, "left");
    expect(left.x).toBeCloseTo(1 - geometry.gutterU, 12);
    expect(left.x + left.w).toBeCloseTo(1, 12);

    const right = gutterRect(geometry, "right");
    expect(right.x).toBeCloseTo(0, 12);
    expect(right.x + right.w).toBeCloseTo(geometry.gutterU, 12);

    // Full page height on both: the band is a strip, not a box.
    for (const rect of [left, right]) {
      expect(rect.y).toBe(0);
      expect(rect.h).toBe(1);
    }
  });

  it("uses the engine's real Pixajoy figures, not round numbers", () => {
    // 5mm bleed on an 11.197" page, and Pixajoy's published 1/8" safe margin.
    expect(geometry.trimU).toBeCloseTo(0.197 / 11.197, 12);
    expect(geometry.trimV).toBeCloseTo(0.197 / 8.894, 12);
    expect(geometry.safeU).toBeCloseTo(0.125 / 11.197, 12);
    expect(geometry.safeV).toBeCloseTo(0.125 / 8.894, 12);
    expect(geometry.gutterU).toBeCloseTo(0.197 / 11.197, 12);
    expect(geometry.pageWIn).toBe(11.197);
    expect(geometry.pageHIn).toBe(8.894);
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
});
