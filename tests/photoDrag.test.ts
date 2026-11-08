import { describe, expect, it } from "vitest";
import {
  autoScrollSpeed,
  doneToast,
  dropBadge,
  photoAt,
  dropOutcome,
  dropTargetAt,
  placementKey,
  swapStepFor,
  AUTO_SCROLL_EDGE_PX,
  AUTO_SCROLL_MAX_PX,
  type DragSource,
  type DropTarget,
} from "~/types/photoDrag";
import type { BookLayout } from "~/types/preview";

/**
 * A stand-in for the DOM: an element with attributes and a parent, answering
 * `closest` the way the browser does for the one selector shape used here.
 */
interface FakeEl {
  attrs: Record<string, string>;
  parent: FakeEl | null;
  closest(selector: string): FakeEl | null;
  getAttribute(name: string): string | null;
}
function el(attrs: Record<string, string>, parent: FakeEl | null = null): FakeEl {
  const node: FakeEl = {
    attrs,
    parent,
    getAttribute: (name) => node.attrs[name] ?? null,
    closest(selector) {
      const name = selector.replace(/^\[|\]$/g, "");
      let at: FakeEl | null = node;
      while (at) {
        if (name in at.attrs) return at;
        at = at.parent;
      }
      return null;
    },
  };
  return node;
}

const target = (page: number, z: number, locked = false): DropTarget => ({
  kind: "slot",
  placement: { page, z },
  locked,
});
const slotSource = (page: number, z: number, photo: number | null): DragSource => ({
  kind: "slot",
  placement: { page, z },
  photo,
});
const photo = (path: string) => ({ path }) as BookLayout["photos"][number];
const placement = (z: number, photoIndex: number) =>
  ({ z, photoIndex }) as BookLayout["pages"][number]["placements"][number];

describe("placementKey", () => {
  it("encodes page and slot", () => {
    expect(placementKey({ page: 12, z: 3 })).toBe("12:3");
  });
});

describe("dropTargetAt", () => {
  it("resolves the photo's slot from an element inside it, such as its <img>", () => {
    const slot = el({ "data-placement": "7:2" });
    const img = el({}, slot);
    expect(dropTargetAt(img)).toEqual({ kind: "slot", placement: { page: 7, z: 2 }, locked: false });
  });

  it("reads a locked slot as locked", () => {
    const slot = el({ "data-placement": "4:1", "data-locked": "true" });
    expect(dropTargetAt(slot)).toEqual({ kind: "slot", placement: { page: 4, z: 1 }, locked: true });
  });

  it("prefers the photo over the page it sits on", () => {
    const page = el({ "data-drop-page": "2:5" });
    const slot = el({ "data-placement": "5:1" }, page);
    expect(dropTargetAt(el({}, slot))?.kind).toBe("slot");
  });

  it("resolves a page's empty area to its opening and page", () => {
    const page = el({ "data-drop-page": "2:5", "data-locked": "true" });
    expect(dropTargetAt(el({}, page))).toEqual({ kind: "page", opening: 2, page: 5, locked: true });
  });

  it("resolves a cover panel to its side", () => {
    const panel = el({ "data-cover-side": "back" });
    expect(dropTargetAt(el({}, panel))).toEqual({ kind: "cover", side: "back" });
    expect(dropTargetAt(el({ "data-cover-side": "spine" }))).toBeNull();
  });

  it("is null off any photo, and for nothing under the pointer", () => {
    const desk = el({ class: "desk" });
    expect(dropTargetAt(desk)).toBeNull();
    expect(dropTargetAt(null)).toBeNull();
  });

  it("is null for a malformed key rather than a NaN placement", () => {
    expect(dropTargetAt(el({ "data-placement": "cover" }))).toBeNull();
    expect(dropTargetAt(el({ "data-placement": "3:" }))).toBeNull();
    expect(dropTargetAt(el({ "data-drop-page": "x:1" }))).toBeNull();
  });
});

describe("dropOutcome", () => {
  const from = { page: 3, z: 1 };
  const fromSlot = slotSource(3, 1, 7);
  const fromTray = { kind: "tray", photo: 12 } as const;

  it("swaps a page photo with a photo on another page", () => {
    expect(dropOutcome(fromSlot, target(18, 2))).toEqual({
      kind: "edit",
      edit: { kind: "swapPhotos", a: from, b: { page: 18, z: 2 } },
    });
  });

  it("swaps with another slot on the same page", () => {
    expect(dropOutcome(fromSlot, target(3, 2))).toMatchObject({ kind: "edit", edit: { kind: "swapPhotos" } });
  });

  it("does nothing when dropped back on itself", () => {
    expect(dropOutcome(fromSlot, target(3, 1))).toEqual({ kind: "none" });
  });

  it("does nothing off any photo", () => {
    expect(dropOutcome(fromSlot, null)).toEqual({ kind: "none" });
    expect(dropOutcome(fromTray, null)).toEqual({ kind: "none" });
  });

  it("refuses a locked target without sending an edit, and says which", () => {
    const locked = target(9, 1, true);
    expect(dropOutcome(fromSlot, locked)).toEqual({ kind: "locked", target: locked });
    expect(dropOutcome(fromTray, locked)).toEqual({ kind: "locked", target: locked });
    const page = { kind: "page", opening: 4, page: 8, locked: true } as const;
    expect(dropOutcome(fromTray, page)).toEqual({ kind: "locked", target: page });
  });

  it("replaces a page photo with a left-out photo", () => {
    expect(dropOutcome(fromTray, target(5, 2))).toEqual({
      kind: "edit",
      edit: { kind: "replacePhoto", placement: { page: 5, z: 2 }, photo: 12 },
    });
  });

  it("adds a left-out photo to a page dropped on its empty area", () => {
    expect(dropOutcome(fromTray, { kind: "page", opening: 4, page: 8, locked: false })).toEqual({
      kind: "edit",
      edit: { kind: "addPhoto", opening: 4, photo: 12 },
    });
  });

  it("does nothing for a page photo dropped on a page's empty area", () => {
    expect(dropOutcome(fromSlot, { kind: "page", opening: 4, page: 8, locked: false })).toEqual({ kind: "none" });
  });

  it("puts either kind of photo on the cover, by its photo index", () => {
    const front = { kind: "cover", side: "front" } as const;
    expect(dropOutcome(fromSlot, front)).toEqual({
      kind: "edit",
      edit: { kind: "setCoverPhoto", side: "front", photo: 7 },
    });
    expect(dropOutcome(fromTray, front)).toMatchObject({ edit: { kind: "setCoverPhoto", photo: 12 } });
  });

  it("does nothing for a slot whose photo is unknown dropped on the cover", () => {
    expect(dropOutcome(slotSource(3, 1, null), { kind: "cover", side: "front" })).toEqual({ kind: "none" });
  });
});

const badge = (source: DragSource, to: DropTarget | null) => dropBadge(dropOutcome(source, to), to)?.label;

describe("dropBadge", () => {
  const tray = { kind: "tray", photo: 1 } as const;

  it("names what letting go would do", () => {
    expect(badge(slotSource(3, 1, 7), target(4, 1))).toBe("Swap");
    expect(badge(tray, target(4, 1))).toBe("Replace");
    expect(badge(tray, { kind: "page", opening: 1, page: 2, locked: false })).toBe("Add to page 2");
    expect(badge(tray, { kind: "cover", side: "back" })).toBe("Use on back cover");
    expect(badge(tray, target(4, 1, true))).toBe("Locked");
  });

  it("shows nothing when letting go would do nothing", () => {
    expect(badge(slotSource(3, 1, 7), null)).toBeUndefined();
    expect(badge(slotSource(3, 1, 7), target(3, 1))).toBeUndefined();
  });
});

describe("autoScrollSpeed", () => {
  const viewport = { top: 100, bottom: 900 };

  it("is still in the middle of the desk", () => {
    expect(autoScrollSpeed(500, viewport)).toBe(0);
    expect(autoScrollSpeed(100 + AUTO_SCROLL_EDGE_PX + 1, viewport)).toBe(0);
    expect(autoScrollSpeed(900 - AUTO_SCROLL_EDGE_PX - 1, viewport)).toBe(0);
  });

  it("scrolls up near the top and down near the bottom", () => {
    expect(autoScrollSpeed(110, viewport)).toBeLessThan(0);
    expect(autoScrollSpeed(890, viewport)).toBeGreaterThan(0);
  });

  it("gets faster towards the edge and caps at the edge and beyond", () => {
    const near = autoScrollSpeed(900 - AUTO_SCROLL_EDGE_PX / 2, viewport);
    const nearer = autoScrollSpeed(895, viewport);
    expect(nearer).toBeGreaterThan(near);
    expect(autoScrollSpeed(900, viewport)).toBe(AUTO_SCROLL_MAX_PX);
    expect(autoScrollSpeed(1200, viewport)).toBe(AUTO_SCROLL_MAX_PX);
    expect(autoScrollSpeed(-50, viewport)).toBe(-AUTO_SCROLL_MAX_PX);
  });
});

describe("swapStepFor", () => {
  const a = { page: 2, z: 1 };
  const b = { page: 9, z: 3 };

  it("a plain click selects, and never swaps", () => {
    expect(swapStepFor({ selected: a, armed: false }, b)).toEqual({
      next: { selected: b, armed: false },
      edit: null,
    });
  });

  it("a plain click on the selected photo deselects it", () => {
    expect(swapStepFor({ selected: a, armed: false }, a)).toEqual({
      next: { selected: null, armed: false },
      edit: null,
    });
  });

  it("after Swap with…, a click on another photo swaps and clears", () => {
    expect(swapStepFor({ selected: a, armed: true }, b)).toEqual({
      next: { selected: null, armed: false },
      edit: { kind: "swapPhotos", a, b },
    });
  });

  it("after Swap with…, a click on the same photo cancels the swap but keeps it selected", () => {
    expect(swapStepFor({ selected: a, armed: true }, a)).toEqual({
      next: { selected: a, armed: false },
      edit: null,
    });
  });
});

describe("doneToast", () => {
  const a = { page: 2, z: 1 };
  const b = { page: 9, z: 3 };

  it("names the photo-moving edits, with the pages they touched", () => {
    expect(doneToast({ kind: "swapPhotos", a, b })).toBe("Swapped photos on pages 2 and 9");
    expect(doneToast({ kind: "swapPhotos", a, b: { page: 2, z: 2 } })).toBe("Swapped photos on page 2");
    expect(doneToast({ kind: "replacePhoto", placement: b, photo: 4 })).toBe("Replaced the photo on page 9");
    expect(doneToast({ kind: "addPhoto", opening: 3, photo: 4 }, () => "pages 6–7")).toBe("Added a photo to pages 6–7");
    expect(doneToast({ kind: "addPhoto", opening: 3, photo: 4 })).toBe("Added a photo");
    expect(doneToast({ kind: "setCoverPhoto", side: "back", photo: 4 })).toBe("Put a photo on the back cover");
  });

  it("stays quiet for edits whose result is already plain on screen", () => {
    expect(doneToast({ kind: "setCrop", placement: a, x: 0, y: 0, w: 1 })).toBeNull();
    expect(doneToast({ kind: "shuffle" })).toBeNull();
    expect(doneToast({ kind: "setLocked", opening: 1, locked: true })).toBeNull();
    expect(doneToast({ kind: "setCoverPhoto", side: "back", photo: null })).toBeNull();
  });
});

describe("photoAt", () => {
  const layout = {
    photos: [photo("a.jpg"), photo("b.jpg"), photo("c.jpg")],
    pages: [
      { number: 1, placements: [placement(1, 2)] },
      { number: 2, placements: [placement(2, 1), placement(1, 0)] },
    ],
  } as unknown as BookLayout;

  it("finds the photo in a slot on any page, by page number and z rather than position", () => {
    expect(photoAt(layout, { page: 2, z: 2 })?.path).toBe("b.jpg");
    expect(photoAt(layout, { page: 1, z: 1 })?.path).toBe("c.jpg");
  });

  it("is null for a slot the book does not have", () => {
    expect(photoAt(layout, { page: 2, z: 3 })).toBeNull();
    expect(photoAt(layout, { page: 7, z: 1 })).toBeNull();
  });
});
