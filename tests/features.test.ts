import { describe, expect, it } from "vitest";
import { isFailed, keepers, type AnalyzedPhoto } from "../app/types/features";

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
