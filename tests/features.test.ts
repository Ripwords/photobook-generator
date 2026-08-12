import { describe, expect, it } from "vitest";
import {
  basename,
  burstSizes,
  groupByEvent,
  isFailed,
  keepers,
  pickHero,
  type AnalyzedPhoto,
} from "../app/types/features";

const photo = (over: Partial<AnalyzedPhoto> = {}): AnalyzedPhoto => ({
  status: "ok",
  path: "/p/a.jpg",
  hash: "h",
  width: 4032,
  height: 3024,
  isUtility: false,
  aestheticPct: 50,
  sharpnessPct: 50,
  faceCount: 0,
  smileFraction: null,
  sceneTags: [],
  nearDupCluster: 0,
  eventCluster: 0,
  thumbnailPath: null,
  ...over,
});

describe("feature helpers", () => {
  it("identifies failed records", () => {
    expect(isFailed({ status: "failed", path: "/p/b.jpg", message: "boom" })).toBe(true);
    expect(isFailed(photo())).toBe(false);
  });

  it("drops utility photos from keepers", () => {
    const result = keepers([photo(), photo({ isUtility: true, path: "/p/shot.png" })]);
    expect(result).toHaveLength(1);
  });

  it("keeps one photo per near-duplicate cluster", () => {
    const result = keepers([
      photo({ path: "/p/1.jpg", nearDupCluster: 7, sharpnessPct: 40 }),
      photo({ path: "/p/2.jpg", nearDupCluster: 7, sharpnessPct: 90 }),
      photo({ path: "/p/3.jpg", nearDupCluster: 8, sharpnessPct: 10 }),
    ]);
    expect(result).toHaveLength(2);
    expect(result.find((p) => p.nearDupCluster === 7)?.path).toBe("/p/2.jpg");
  });

  it("returns an empty array for no input", () => {
    expect(keepers([])).toEqual([]);
  });

  // --- Additional tests beyond the brief ---

  /// The brief's "drops utility photos from keepers" test is vacuous: both
  /// its photos share nearDupCluster: 0 and tie on sharpness/aesthetic, so
  /// the near-duplicate dedup logic alone collapses them to length 1 even if
  /// the `isUtility` filter were deleted entirely. This test isolates the
  /// utility filter by putting the utility photo in its own cluster, so a
  /// dropped filter would leave it in the result (length 2) instead of
  /// filtering it out (length 1).
  it("drops a utility photo even when it is the sole member of its own cluster", () => {
    const result = keepers([
      photo({ path: "/p/keep.jpg", nearDupCluster: 1 }),
      photo({ path: "/p/utility.png", nearDupCluster: 2, isUtility: true }),
    ]);
    expect(result).toHaveLength(1);
    expect(result[0]?.path).toBe("/p/keep.jpg");
  });

  it("drops all photos when every photo is utility", () => {
    const result = keepers([
      photo({ path: "/p/u1.png", nearDupCluster: 1, isUtility: true }),
      photo({ path: "/p/u2.png", nearDupCluster: 2, isUtility: true }),
    ]);
    expect(result).toEqual([]);
  });

  it("breaks a sharpness tie using aesthetic percentile", () => {
    const result = keepers([
      photo({ path: "/p/dull.jpg", nearDupCluster: 3, sharpnessPct: 70, aestheticPct: 20 }),
      photo({ path: "/p/pretty.jpg", nearDupCluster: 3, sharpnessPct: 70, aestheticPct: 80 }),
    ]);
    expect(result).toHaveLength(1);
    expect(result[0]?.path).toBe("/p/pretty.jpg");
  });
});

describe("groupByEvent", () => {
  it("returns an empty array for no input", () => {
    expect(groupByEvent([])).toEqual([]);
  });

  it("groups photos sharing an eventCluster together", () => {
    const result = groupByEvent([
      photo({ path: "/p/1.jpg", eventCluster: 1 }),
      photo({ path: "/p/2.jpg", eventCluster: 2 }),
      photo({ path: "/p/3.jpg", eventCluster: 1 }),
    ]);
    expect(result).toHaveLength(2);
    expect(result.find((g) => g.eventCluster === 1)?.photos.map((p) => p.path)).toEqual([
      "/p/1.jpg",
      "/p/3.jpg",
    ]);
    expect(result.find((g) => g.eventCluster === 2)?.photos.map((p) => p.path)).toEqual([
      "/p/2.jpg",
    ]);
  });

  it("orders groups by first appearance, not by cluster id", () => {
    const result = groupByEvent([
      photo({ path: "/p/1.jpg", eventCluster: 5 }),
      photo({ path: "/p/2.jpg", eventCluster: 1 }),
    ]);
    expect(result.map((g) => g.eventCluster)).toEqual([5, 1]);
  });
});

describe("burstSizes", () => {
  it("returns an empty map for no input", () => {
    expect(burstSizes([])).toEqual(new Map());
  });

  it("counts non-utility photos per near-duplicate cluster", () => {
    const result = burstSizes([
      photo({ path: "/p/1.jpg", nearDupCluster: 7 }),
      photo({ path: "/p/2.jpg", nearDupCluster: 7 }),
      photo({ path: "/p/3.jpg", nearDupCluster: 8 }),
    ]);
    expect(result.get(7)).toBe(2);
    expect(result.get(8)).toBe(1);
  });

  it("excludes utility photos from burst counts", () => {
    const result = burstSizes([
      photo({ path: "/p/1.jpg", nearDupCluster: 7 }),
      photo({ path: "/p/2.jpg", nearDupCluster: 7, isUtility: true }),
    ]);
    expect(result.get(7)).toBe(1);
  });
});

describe("basename", () => {
  it("returns the last path segment", () => {
    expect(basename("/Users/jj/Pictures/Family Trip")).toBe("Family Trip");
  });

  it("strips a trailing slash before taking the last segment", () => {
    expect(basename("/Users/jj/Pictures/Family Trip/")).toBe("Family Trip");
  });

  it("returns the input unchanged when there is no separator", () => {
    expect(basename("Pictures")).toBe("Pictures");
  });
});

describe("pickHero", () => {
  it("returns undefined for no input", () => {
    expect(pickHero([])).toBeUndefined();
  });

  it("returns the only photo for a group of one", () => {
    const solo = photo({ path: "/p/solo.jpg" });
    expect(pickHero([solo])).toBe(solo);
  });

  it("picks the highest aesthetic percentile", () => {
    const result = pickHero([
      photo({ path: "/p/low.jpg", aestheticPct: 40 }),
      photo({ path: "/p/high.jpg", aestheticPct: 90 }),
      photo({ path: "/p/mid.jpg", aestheticPct: 60 }),
    ]);
    expect(result?.path).toBe("/p/high.jpg");
  });

  it("breaks an aesthetic tie using sharpness percentile", () => {
    const result = pickHero([
      photo({ path: "/p/soft.jpg", aestheticPct: 70, sharpnessPct: 30 }),
      photo({ path: "/p/sharp.jpg", aestheticPct: 70, sharpnessPct: 85 }),
    ]);
    expect(result?.path).toBe("/p/sharp.jpg");
  });
});
