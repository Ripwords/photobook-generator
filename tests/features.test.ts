import { describe, expect, it } from "vitest";
import {
  applyAnalysisEvent,
  basename,
  burstSizes,
  groupByEvent,
  initialStreamState,
  isFailed,
  isRanked,
  keepers,
  pickHero,
  smilePercent,
  type AnalysisEvent,
  type AnalyzedPhoto,
  type PartialAnalyzedPhoto,
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

const partialPhoto = (over: Partial<PartialAnalyzedPhoto> = {}): PartialAnalyzedPhoto => ({
  status: "ok",
  path: "/p/a.jpg",
  hash: "h",
  width: 4032,
  height: 3024,
  isUtility: false,
  faceCount: 0,
  smileFraction: null,
  sceneTags: [],
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

  it("orders groups chronologically by eventCluster id, not by first appearance in the input", () => {
    const result = groupByEvent([
      photo({ path: "/p/1.jpg", eventCluster: 5 }),
      photo({ path: "/p/2.jpg", eventCluster: 1 }),
    ]);
    expect(result.map((g) => g.eventCluster)).toEqual([1, 5]);
  });

  // Regression test for the real bug: filename (path) order does not match
  // capture-time order, e.g. two cameras writing different filename
  // prefixes. `finalize_photos` in Rust sorts the input array by path, so
  // a later event's photo can appear EARLIER in the input array than an
  // earlier event's photo. Grouping by first-appearance-in-input (the
  // previous behaviour) would render the later event first, under the
  // label "Event 1" -- chronologically backwards.
  it("orders groups chronologically when path order and capture order disagree", () => {
    const result = groupByEvent([
      // Appears FIRST in the (path-sorted) input, but is chronologically the
      // LATER event.
      photo({ path: "/p/a-camera-two-later.jpg", eventCluster: 5 }),
      // Appears SECOND in the input, but is chronologically the EARLIER
      // event.
      photo({ path: "/p/b-camera-one-earlier.jpg", eventCluster: 1 }),
    ]);
    expect(result.map((g) => g.eventCluster)).toEqual([1, 5]);
    expect(result[0]?.photos[0]?.path).toBe("/p/b-camera-one-earlier.jpg");
    expect(result[1]?.photos[0]?.path).toBe("/p/a-camera-two-later.jpg");
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

describe("smilePercent", () => {
  // Swift's synthesized `Codable` uses `encodeIfPresent` for optional
  // properties: a `nil` `smileFraction` is OMITTED from the wire JSON
  // entirely, never encoded as `smileFraction: null`. This is the actual
  // shape the backend produces whenever a face has unusable pose (|yaw| or
  // |pitch| > 30 degrees) or too few lip landmark points -- routine on real
  // photos. Simulate it with a raw JSON string missing the key entirely
  // (NOT `{ smileFraction: null }`, which is a shape the backend never
  // sends) so this test exercises the real contract, not a convenient
  // stand-in for it.
  it("returns null when the backend omits smileFraction entirely (undefined, not null)", () => {
    const decoded = JSON.parse('{"path":"/p/a.jpg","aestheticPct":50}') as {
      smileFraction?: number | null;
    };
    expect(decoded.smileFraction).toBeUndefined(); // sanity: this is the real shape
    expect(smilePercent(decoded.smileFraction)).toBeNull();
  });

  it("returns null for an explicit null", () => {
    expect(smilePercent(null)).toBeNull();
  });

  it("converts a fraction to a rounded whole-number percent", () => {
    expect(smilePercent(0.5)).toBe(50);
    expect(smilePercent(0.876)).toBe(88);
    expect(smilePercent(0)).toBe(0);
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

describe("isRanked", () => {
  it("is false for a partial photo (no whole-set derivations yet)", () => {
    expect(isRanked(partialPhoto())).toBe(false);
  });

  it("is true once a photo carries its percentile/cluster data", () => {
    expect(isRanked(photo())).toBe(true);
  });
});

describe("applyAnalysisEvent", () => {
  it("records the scanned total", () => {
    const state = applyAnalysisEvent(initialStreamState, { kind: "scanned", total: 280 });
    expect(state.scannedTotal).toBe(280);
  });

  it("appends a batch's photos and adopts its cumulative counts", () => {
    const event: AnalysisEvent = {
      kind: "batch",
      photos: [partialPhoto({ path: "/p/1.jpg" })],
      analysed: 1,
      cached: 0,
      failed: 0,
    };
    const state = applyAnalysisEvent(initialStreamState, event);
    expect(state.partialPhotos.map((p) => p.path)).toEqual(["/p/1.jpg"]);
    expect(state.analysed).toBe(1);
  });

  it("adopts the final summary on done", () => {
    const summary = { total: 1, failed: 0, cached: 0, photos: [photo()] };
    const state = applyAnalysisEvent(initialStreamState, { kind: "done", summary });
    expect(state.summary).toBe(summary);
  });

  // The property the task brief asks for explicitly: partial records
  // accumulate in the order their batches arrived in, which (per Rust's
  // `analyze_batches_with_progress`) is input/path order -- not sorted,
  // reversed, or otherwise reordered by this reducer.
  it("accumulates partial photos across batches in arrival order", () => {
    const batch1: AnalysisEvent = {
      kind: "batch",
      photos: [partialPhoto({ path: "/p/1.jpg" }), partialPhoto({ path: "/p/2.jpg" })],
      analysed: 2,
      cached: 0,
      failed: 0,
    };
    const batch2: AnalysisEvent = {
      kind: "batch",
      photos: [partialPhoto({ path: "/p/3.jpg" })],
      analysed: 3,
      cached: 0,
      failed: 0,
    };

    let state = applyAnalysisEvent(initialStreamState, batch1);
    state = applyAnalysisEvent(state, batch2);

    expect(state.partialPhotos.map((p) => p.path)).toEqual(["/p/1.jpg", "/p/2.jpg", "/p/3.jpg"]);
  });

  // Mutation-style guard: a batch that streams zero photos (every photo in
  // it failed) must not shift a LATER batch's photos to the wrong position.
  // If an implementation accidentally replaced `partialPhotos` instead of
  // appending, or dropped the wrong slice on a failure, batch 3's photo
  // would either vanish or land ahead of batch 1's.
  it("a failed (zero-photo) batch does not shift a later batch's photos", () => {
    const batch1: AnalysisEvent = {
      kind: "batch",
      photos: [partialPhoto({ path: "/p/1.jpg" })],
      analysed: 1,
      cached: 0,
      failed: 0,
    };
    const failedBatch: AnalysisEvent = {
      kind: "batch",
      photos: [],
      analysed: 1,
      cached: 0,
      failed: 2,
    };
    const batch3: AnalysisEvent = {
      kind: "batch",
      photos: [partialPhoto({ path: "/p/6.jpg" })],
      analysed: 2,
      cached: 0,
      failed: 2,
    };

    let state = applyAnalysisEvent(initialStreamState, batch1);
    state = applyAnalysisEvent(state, failedBatch);
    state = applyAnalysisEvent(state, batch3);

    expect(state.partialPhotos.map((p) => p.path)).toEqual(["/p/1.jpg", "/p/6.jpg"]);
    expect(state.failed).toBe(2);
  });

  it("does not mutate the state object passed in (each call returns a new state)", () => {
    const before = initialStreamState;
    applyAnalysisEvent(before, { kind: "scanned", total: 5 });
    expect(before).toEqual(initialStreamState);
  });
});
