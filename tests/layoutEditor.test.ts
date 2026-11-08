import { describe, expect, it } from "vitest";
import { reactive } from "vue";
import {
  addBox,
  canvasAspect,
  copyName,
  editorGuides,
  minBox,
  nextLayoutName,
  openingBlankShape,
  openingPages,
  plainShape,
  moveBox,
  resizeBox,
  shapeFromPages,
  shapePages,
  snapDelta,
} from "~/types/layoutEditor";
import type { LayoutShape } from "~/types/customLayouts";
import type { PreviewPage, PreviewRect } from "~/types/preview";
import type { PrintSpec } from "~/types/printSpec";

/** Pixajoy's numbers, as `default_print_spec` sends them. */
const SPEC: PrintSpec = {
  pageWIn: 11.197,
  pageHIn: 8.894,
  bleedIn: 0.197,
  gutterIn: 0.394,
  safeMarginIn: 0.25,
  minDpi: 150,
  warnDpi: 240,
  coverWrapIn: 0.75,
};

const NO_GUIDES = { x: [], y: [] };

function rect(x: number, y: number, w: number, h: number): PreviewRect {
  return { x, y, w, h };
}

function close(actual: PreviewRect, expected: PreviewRect) {
  for (const key of ["x", "y", "w", "h"] as const) {
    expect(actual[key], key).toBeCloseTo(expected[key], 6);
  }
}

describe("canvasAspect", () => {
  it("is two pages wide for a spread and one for a page", () => {
    expect(canvasAspect("spread", 1.25)).toBe(2.5);
    expect(canvasAspect("page", 1.25)).toBe(1.25);
  });
});

describe("minBox", () => {
  // Rust refuses a box under 5% of the PAGE each way; a spread canvas is two
  // pages wide, so the same box is 2.5% of it across.
  it("is 5% of a page each way on either canvas", () => {
    expect(minBox("spread")).toEqual({ w: 0.025, h: 0.05 });
    expect(minBox("page")).toEqual({ w: 0.05, h: 0.05 });
  });
});

describe("editorGuides", () => {
  it("puts a spread's fold, gutter lines and bleed lines where the spec does", () => {
    const g = editorGuides({ kind: "spread" }, SPEC);
    const bleedX = SPEC.bleedIn / (2 * SPEC.pageWIn);
    const gutterX = SPEC.gutterIn / (2 * SPEC.pageWIn);
    const bleedY = SPEC.bleedIn / SPEC.pageHIn;
    expect(g.x).toEqual([0, bleedX, 0.5 - gutterX, 0.5, 0.5 + gutterX, 1 - bleedX, 1]);
    expect(g.y).toEqual([0, bleedY, 1 - bleedY, 1]);
  });

  it("bleeds a left page on its left and gutters it on its right, and mirrors a right page", () => {
    const bleedX = SPEC.bleedIn / SPEC.pageWIn;
    const gutterX = SPEC.gutterIn / SPEC.pageWIn;
    expect(editorGuides({ kind: "page", side: "left" }, SPEC).x).toEqual([0, bleedX, 1 - gutterX, 1]);
    expect(editorGuides({ kind: "page", side: "right" }, SPEC).x).toEqual([0, gutterX, 1 - bleedX, 1]);
  });
});

describe("snapDelta", () => {
  it("pulls the nearest edge onto a guide inside the threshold", () => {
    expect(snapDelta([0.304, 0.6], [0, 0.3, 1], 0.01)).toBeCloseTo(-0.004, 9);
  });

  it("prefers the closer of two candidate snaps", () => {
    expect(snapDelta([0.295, 0.597], [0.3, 0.6], 0.01)).toBeCloseTo(0.003, 9);
  });

  it("snaps an edge exactly at the threshold", () => {
    // Binary-exact numbers, so the distance is the threshold to the bit.
    expect(snapDelta([0.375], [0.5], 0.125)).toBe(0.125);
  });

  it("leaves an edge just outside the threshold where it is", () => {
    expect(snapDelta([0.3101], [0.3], 0.01)).toBe(0);
  });
});

describe("moveBox", () => {
  it("moves by the drag", () => {
    close(moveBox(rect(0.1, 0.1, 0.2, 0.3), 0.05, 0.1, NO_GUIDES, 0.01), rect(0.15, 0.2, 0.2, 0.3));
  });

  it("stops at the canvas edge without shrinking", () => {
    close(moveBox(rect(0.7, 0.6, 0.2, 0.3), 0.5, 0.5, NO_GUIDES, 0.01), rect(0.8, 0.7, 0.2, 0.3));
    close(moveBox(rect(0.1, 0.1, 0.2, 0.3), -0.5, -0.5, NO_GUIDES, 0.01), rect(0, 0, 0.2, 0.3));
  });

  it("snaps the right edge onto the fold", () => {
    // Right edge would land at 0.495, five thousandths short of the fold.
    close(
      moveBox(rect(0.1, 0.1, 0.2, 0.3), 0.195, 0, { x: [0.5], y: [] }, 0.01),
      rect(0.3, 0.1, 0.2, 0.3),
    );
  });
});

describe("resizeBox", () => {
  it("moves only the dragged corner's two edges", () => {
    close(resizeBox(rect(0.2, 0.2, 0.2, 0.2), "se", 0.1, 0.05, "page", NO_GUIDES, 0.01), rect(0.2, 0.2, 0.3, 0.25));
    close(resizeBox(rect(0.2, 0.2, 0.2, 0.2), "nw", -0.1, -0.05, "page", NO_GUIDES, 0.01), rect(0.1, 0.15, 0.3, 0.25));
    close(resizeBox(rect(0.2, 0.2, 0.2, 0.2), "ne", 0.1, -0.05, "page", NO_GUIDES, 0.01), rect(0.2, 0.15, 0.3, 0.25));
    close(resizeBox(rect(0.2, 0.2, 0.2, 0.2), "sw", -0.1, 0.05, "page", NO_GUIDES, 0.01), rect(0.1, 0.2, 0.3, 0.25));
  });

  it("never shrinks below the minimum, even dragged past the opposite edge", () => {
    close(resizeBox(rect(0.2, 0.2, 0.2, 0.2), "se", -0.5, -0.5, "page", NO_GUIDES, 0.01), rect(0.2, 0.2, 0.05, 0.05));
    close(resizeBox(rect(0.2, 0.2, 0.2, 0.2), "nw", 0.5, 0.5, "spread", NO_GUIDES, 0.01), rect(0.375, 0.35, 0.025, 0.05));
  });

  it("stops at the canvas edge", () => {
    close(resizeBox(rect(0.7, 0.7, 0.2, 0.2), "se", 0.5, 0.5, "page", NO_GUIDES, 0.01), rect(0.7, 0.7, 0.3, 0.3));
  });

  it("snaps the moving edge, never the fixed one", () => {
    // The left edge (fixed) sits 0.004 from a guide and must stay put; the
    // right edge lands 0.004 short of the guide at 0.6 and snaps to it.
    close(
      resizeBox(rect(0.304, 0.2, 0.2, 0.2), "se", 0.092, 0, "page", { x: [0.3, 0.6], y: [] }, 0.01),
      rect(0.304, 0.2, 0.296, 0.2),
    );
  });
});

describe("addBox", () => {
  it("puts the first box of a spread on the left page, clear of the fold", () => {
    const shape = addBox({ kind: "spread", boxes: [] });
    expect(shape.boxes).toHaveLength(1);
    const [box] = shape.boxes;
    expect(box!.rect.x + box!.rect.w).toBeLessThanOrEqual(0.5);
    expect(box!.role).toBe("hero");
  });

  it("puts a second box where it overlaps nothing, as support", () => {
    const one = addBox({ kind: "spread", boxes: [] });
    const two = addBox(one);
    const [a, b] = two.boxes.map((x) => x.rect);
    const overlaps =
      a!.x < b!.x + b!.w && b!.x < a!.x + a!.w && a!.y < b!.y + b!.h && b!.y < a!.y + a!.h;
    expect(overlaps).toBe(false);
    expect(two.boxes[1]!.role).toBe("support");
    // and it does not straddle the fold
    expect(b!.x < 0.5 && b!.x + b!.w > 0.5).toBe(false);
  });

  it("does not change the shape it was given", () => {
    const empty: LayoutShape = { kind: "page", side: "right", boxes: [] };
    addBox(empty);
    expect(empty.boxes).toHaveLength(0);
  });
});

function page(side: "left" | "right", rects: PreviewRect[]): PreviewPage {
  return {
    number: side === "left" ? 2 : 3,
    side,
    templateId: "x",
    blank: false,
    placements: rects.map((slotRect, z) => ({
      photoIndex: z,
      slotRect,
      crop: rect(0, 0, 1, 1),
      z,
      filename: null,
    })),
  };
}

describe("shapeFromPages", () => {
  it("halves page rects onto the spread canvas, right page past the fold", () => {
    const shape = shapeFromPages([page("left", [rect(0, 0, 1, 1)]), page("right", [rect(0.2, 0.1, 0.6, 0.8)])]);
    expect(shape.kind).toBe("spread");
    close(shape.boxes[0]!.rect, rect(0, 0, 0.5, 1));
    close(shape.boxes[1]!.rect, rect(0.6, 0.1, 0.3, 0.8));
  });

  it("keeps one page as a page layout on its side", () => {
    const shape = shapeFromPages([page("right", [rect(0.1, 0.1, 0.8, 0.8)])]);
    expect(shape).toEqual({ kind: "page", side: "right", boxes: [{ rect: rect(0.1, 0.1, 0.8, 0.8), role: "hero" }] });
  });

  it("makes the clearly biggest box the hero and the rest support", () => {
    const shape = shapeFromPages([
      page("left", [rect(0, 0, 1, 1)]),
      page("right", [rect(0.1, 0.1, 0.35, 0.35), rect(0.55, 0.55, 0.35, 0.35)]),
    ]);
    expect(shape.boxes.map((b) => b.role)).toEqual(["hero", "support", "support"]);
  });

  it("calls boxes of about equal size all support", () => {
    const shape = shapeFromPages([page("left", [rect(0.1, 0.1, 0.8, 0.8)]), page("right", [rect(0.1, 0.1, 0.8, 0.75)])]);
    expect(shape.boxes.map((b) => b.role)).toEqual(["support", "support"]);
  });
});

describe("copyName", () => {
  it("names a built-in's copy in words", () => {
    expect(copyName("07-two-up-symmetric-margin")).toBe("Two up symmetric margin copy");
  });
});

describe("shapePages", () => {
  it("splits a spread's boxes onto the page each sits on, page-normalised", () => {
    const pages = shapePages({
      kind: "spread",
      boxes: [
        { rect: rect(0.1, 0.1, 0.3, 0.8), role: "hero" },
        { rect: rect(0.6, 0.2, 0.2, 0.5), role: "support" },
      ],
    });
    expect(pages).toHaveLength(2);
    close(pages[0]![0]!, rect(0.2, 0.1, 0.6, 0.8));
    expect(pages[0]).toHaveLength(1);
    close(pages[1]![0]!, rect(0.2, 0.2, 0.4, 0.5));
  });

  it("is the inverse of shapeFromPages on a spread", () => {
    const shape = shapeFromPages([page("left", [rect(0.1, 0.2, 0.5, 0.6)]), page("right", [rect(0, 0, 1, 1)])]);
    const [left, right] = shapePages(shape);
    close(left![0]!, rect(0.1, 0.2, 0.5, 0.6));
    close(right![0]!, rect(0, 0, 1, 1));
  });

  it("draws a page layout as one page", () => {
    expect(shapePages({ kind: "page", side: "left", boxes: [{ rect: rect(0, 0, 1, 1), role: "hero" }] })).toEqual([
      [rect(0, 0, 1, 1)],
    ]);
  });
});

describe("nextLayoutName", () => {
  it("is the first Layout N not already taken", () => {
    expect(nextLayoutName([])).toBe("Layout 1");
    expect(nextLayoutName(["Layout 1", "Layout 3"])).toBe("Layout 2");
    expect(nextLayoutName(["Layout 2"])).toBe("Layout 1");
  });
});

describe("plainShape", () => {
  // The editor edits a reactive shape; IPC (and the mock's structuredClone)
  // must get plain data, or the save fails with a DataCloneError.
  it("turns a reactive shape into plain, cloneable data with the same values", () => {
    const live = reactive<LayoutShape>({
      kind: "page",
      side: "left",
      boxes: [{ rect: rect(0.1, 0.1, 0.5, 0.5), role: "hero" }],
    });
    expect(() => structuredClone(live)).toThrow();
    const plain = plainShape(live);
    expect(structuredClone(plain)).toEqual({
      kind: "page",
      side: "left",
      boxes: [{ rect: rect(0.1, 0.1, 0.5, 0.5), role: "hero" }],
    });
  });

  it("keeps a spread a spread", () => {
    const live = reactive<LayoutShape>({ kind: "spread", boxes: [{ rect: rect(0, 0, 0.5, 1), role: "support" }] });
    expect(structuredClone(plainShape(live))).toEqual({ kind: "spread", boxes: [{ rect: rect(0, 0, 0.5, 1), role: "support" }] });
  });
});

describe("openingPages and openingBlankShape", () => {
  const left = page("left", [rect(0, 0, 1, 1)]);
  const right = page("right", [rect(0, 0, 1, 1)]);
  // `toSpreads` marks the missing half of page 1 and the last page with null.
  const first = { key: "1", left: null, right, label: "Page 1" };
  const middle = { key: "2", left, right, label: "Pages 2–3" };
  const last = { key: "4", left, right: null, label: "Page 4" };

  it("lists the pages an opening has, left first, skipping the missing half", () => {
    expect(openingPages(first)).toEqual([right]);
    expect(openingPages(middle)).toEqual([left, right]);
    expect(openingPages(last)).toEqual([left]);
  });

  it("starts a spread for two pages and a page on its own side for one", () => {
    expect(openingBlankShape(middle)).toEqual({ kind: "spread", boxes: [] });
    expect(openingBlankShape(first)).toEqual({ kind: "page", side: "right", boxes: [] });
    expect(openingBlankShape(last)).toEqual({ kind: "page", side: "left", boxes: [] });
  });
});
