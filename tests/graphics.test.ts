import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  type GraphicsStatus,
  graphicsOptions,
  photoFile,
  previewOrder,
} from "../app/types/graphics";
import type { BookLayout, PreviewPhoto } from "../app/types/preview";

function wire(): GraphicsStatus {
  return JSON.parse(
    readFileSync(new URL("./fixtures/wire/graphics-status.json", import.meta.url), "utf8"),
  ) as GraphicsStatus;
}

function photo(n: number, thumbnailPath: string | null = `/t/${n}.jpg`): PreviewPhoto {
  return {
    path: `/p/${n}.ARW`,
    hash: `${n}`.padStart(64, "0"),
    width: 6000,
    height: 4000,
    thumbnailPath,
    aestheticPct: 50,
    capturedAt: null,
  };
}

const rect = { x: 0, y: 0, w: 1, h: 1 };

function placement(photoIndex: number) {
  return { photoIndex, slotRect: rect, crop: rect, z: 0, filename: null };
}

function side(photoIndex: number | null) {
  return {
    board: rect,
    visible: rect,
    photo: photoIndex === null ? null : { photoIndex, crop: rect, filename: null },
  };
}

/** Photos 0..5; pages place 3, 1 then 1 again and 4; the cover is 2 front, 5 back. */
function layout(): Pick<BookLayout, "photos" | "pages" | "cover"> {
  return {
    photos: [0, 1, 2, 3, 4, 5].map((n) => photo(n)),
    pages: [
      { number: 1, side: "right", templateId: "a", blank: false, placements: [placement(3), placement(1)] },
      { number: 2, side: "left", templateId: "a", blank: false, placements: [placement(1), placement(4)] },
    ],
    cover: { aspect: 1, spine: "#000000", front: side(2), back: side(5) },
  };
}

describe("the graphics-status wire", () => {
  it("carries the mode, what Auto resolves to here, and what the editor draws", () => {
    const status = wire();
    expect(status.mode).toBe("auto");
    expect(status.detected).toBe("optimised");
    expect(status.effective).toBe("optimised");
    expect(graphicsOptions(status)[0]).toMatchObject({ value: "auto", detail: "Optimised on this Mac" });
  });
});

describe("graphicsOptions", () => {
  it("names what Auto picks on this Mac", () => {
    const options = graphicsOptions({ mode: "auto", detected: "maximum", effective: "maximum" });
    expect(options.map((o) => o.value)).toEqual(["auto", "low", "optimised", "maximum"]);
    expect(options[0]!.detail).toBe("Maximum on this Mac");
  });

  it("does not offer Auto when the app could not tell what this Mac can handle", () => {
    const options = graphicsOptions({ mode: "optimised", detected: null, effective: "optimised" });
    expect(options.map((o) => o.value)).toEqual(["low", "optimised", "maximum"]);
  });
});

describe("previewOrder", () => {
  it("sends the cover first, then pages in order, each photo once", () => {
    const order = previewOrder(layout());
    expect(order.map((i) => i.path)).toEqual(["/p/2.ARW", "/p/5.ARW", "/p/3.ARW", "/p/1.ARW", "/p/4.ARW"]);
    expect(order[0]!.hash).toBe(photo(2).hash);
  });

  it("skips a cover side with no photo and an index the photo list does not have", () => {
    const book = layout();
    book.cover.front = side(null);
    book.pages[0]!.placements.push(placement(99));
    expect(previewOrder(book).map((i) => i.path)).toEqual(["/p/5.ARW", "/p/3.ARW", "/p/1.ARW", "/p/4.ARW"]);
  });
});

describe("photoFile", () => {
  it("draws the sharper preview once it has landed", () => {
    const p = photo(1);
    expect(photoFile(p, { [p.hash]: "/t/2400/1.jpg" })).toBe("/t/2400/1.jpg");
  });

  it("draws the thumbnail until then, and nothing when there is neither", () => {
    expect(photoFile(photo(1), { [photo(2).hash]: "/t/2400/2.jpg" })).toBe("/t/1.jpg");
    expect(photoFile(photo(1, null), {})).toBeNull();
  });
});
