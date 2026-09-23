import { describe, expect, it } from "vitest";
import {
  applyAnalysisEvent,
  basename,
  burstSizes,
  eventHashes,
  eventTitle,
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
  // Rust's culling verdict, stamped by `commands::stamp_kept`. Defaulted true
  // so fixtures for the OTHER helpers below (which have nothing to do with
  // culling) read as ordinary photos.
  kept: true,
  ...over,
});

const partialPhoto =(over: Partial<PartialAnalyzedPhoto> = {}): PartialAnalyzedPhoto => ({
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

  // --- keepers: a filter on Rust's verdict, with no rule of its own --------
  //
  // These tests used to assert the culling RULE (utility filtering,
  // one-winner-per-cluster, the sharpness/aesthetic ranking). That rule no
  // longer lives here: `book::cull::cull` in Rust is the single authority and
  // `commands::stamp_kept` sends its verdict over as `kept`. Asserting the
  // rule from this side again would be asserting a copy of it, which is the
  // exact defect that was removed -- the old TypeScript copy went straight
  // from sharpness to aesthetic while Rust breaks the tie on face capture
  // quality first, so the screen and the exported book could disagree.
  //
  // So what is worth testing here is precisely that NO rule was left behind.
  // Every fixture below sets `kept` in DELIBERATE CONFLICT with what a local
  // rule would have concluded, so any re-added condition -- an `isUtility`
  // check, a cluster dedup, a percentile comparison -- changes the result.

  it("returns exactly the photos Rust flagged as kept", () => {
    const result = keepers([
      photo({ path: "/p/1.jpg", kept: true }),
      photo({ path: "/p/2.jpg", kept: false }),
      photo({ path: "/p/3.jpg", kept: true }),
    ]);
    expect(result.map((p) => p.path)).toEqual(["/p/1.jpg", "/p/3.jpg"]);
  });

  it("returns an empty array for no input", () => {
    expect(keepers([])).toEqual([]);
  });

  /// A re-added `if (photo.isUtility) continue;` would drop this photo.
  /// Rust flagged it kept, so it must survive: `isUtility` is an input to the
  /// rule Rust already applied, not a second filter to apply again here.
  it("keeps a utility photo when Rust flagged it kept, applying no filter of its own", () => {
    const result = keepers([photo({ path: "/p/shot.png", isUtility: true, kept: true })]);
    expect(result.map((p) => p.path)).toEqual(["/p/shot.png"]);
  });

  /// Three photos in ONE near-duplicate cluster, two of them flagged kept.
  /// A re-added one-winner-per-cluster dedup would collapse these to a single
  /// result no matter which one it picked.
  it("returns two photos from the same near-duplicate cluster when Rust kept both", () => {
    const result = keepers([
      photo({ path: "/p/1.jpg", nearDupCluster: 7, kept: true }),
      photo({ path: "/p/2.jpg", nearDupCluster: 7, kept: false }),
      photo({ path: "/p/3.jpg", nearDupCluster: 7, kept: true }),
    ]);
    expect(result.map((p) => p.path)).toEqual(["/p/1.jpg", "/p/3.jpg"]);
  });

  /// The dropped photo is the SHARPEST and the most AESTHETIC of the three,
  /// and the two survivors are the worst on both axes. Any percentile
  /// comparison re-added here -- in either direction -- picks a different
  /// set. This is the assertion that would have caught the old divergence.
  it("ignores sharpness and aesthetic percentiles entirely", () => {
    const result = keepers([
      photo({ path: "/p/dull.jpg", nearDupCluster: 3, sharpnessPct: 10, aestheticPct: 10, kept: true }),
      photo({ path: "/p/best.jpg", nearDupCluster: 3, sharpnessPct: 99, aestheticPct: 99, kept: false }),
      photo({ path: "/p/dim.jpg", nearDupCluster: 3, sharpnessPct: 20, aestheticPct: 20, kept: true }),
    ]);
    expect(result.map((p) => p.path)).toEqual(["/p/dull.jpg", "/p/dim.jpg"]);
  });

  /// Input order is `finalize_photos`'s path order, which is also the order
  /// `cull` returns, so the sheet and the book list survivors identically.
  /// The fixture is deliberately NOT already in the asserted order under any
  /// other plausible sort (it is reverse-alphabetical), so a filter that
  /// reordered would be visible.
  it("preserves input order rather than imposing its own", () => {
    const result = keepers([
      photo({ path: "/p/c.jpg", kept: true }),
      photo({ path: "/p/b.jpg", kept: true }),
      photo({ path: "/p/a.jpg", kept: true }),
    ]);
    expect(result.map((p) => p.path)).toEqual(["/p/c.jpg", "/p/b.jpg", "/p/a.jpg"]);
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

describe("groupByEvent with place chapters", () => {
  it("regroups by the chapter each path was given, not the time-only cluster", () => {
    const result = groupByEvent(
      [
        photo({ path: "/p/osaka.jpg", eventCluster: 0 }),
        photo({ path: "/p/kyoto.jpg", eventCluster: 0 }),
        photo({ path: "/p/osaka-2.jpg", eventCluster: 0 }),
      ],
      { "/p/kyoto.jpg": 0, "/p/osaka.jpg": 1, "/p/osaka-2.jpg": 1 },
    );
    expect(result.map((g) => [g.eventCluster, g.photos.map((p) => p.path)])).toEqual([
      [0, ["/p/kyoto.jpg"]],
      [1, ["/p/osaka.jpg", "/p/osaka-2.jpg"]],
    ]);
  });

  it("keeps a photo the chapters do not name in its own cluster", () => {
    const result = groupByEvent([photo({ path: "/p/new.jpg", eventCluster: 4 })], {});
    expect(result.map((g) => g.eventCluster)).toEqual([4]);
  });
});

describe("eventHashes", () => {
  // The whole reason this function exists (C1): a filtered list must never
  // reach it, so every test feeds ALL of an event's photos, including ones a
  // "Keepers only"-style filter would have dropped before this ever runs.
  //
  // This asserts eventHashes' own contract -- it applies no filter of its
  // own beyond the event -- rather than just that two different inputs give
  // two different outputs (true of nearly any implementation, including a
  // buggy one, and so not evidence of anything). A version that re-applies
  // `kept` internally (the actual C1 bug, reintroduced) drops h2 here and
  // turns this red; see the mutation-check in task-11-report.md.
  it("includes every one of an event's photos, not just ones a keepers filter would keep", () => {
    const all = [
      photo({ path: "/p/1.jpg", hash: "h1", eventCluster: 1, kept: true }),
      photo({ path: "/p/2.jpg", hash: "h2", eventCluster: 1, kept: false }),
      photo({ path: "/p/3.jpg", hash: "h3", eventCluster: 2, kept: true }),
    ];
    expect(eventHashes(all, null, 1)).toEqual(["h1", "h2"]);
  });

  it("returns an empty array for an event with no photos", () => {
    expect(eventHashes([photo({ eventCluster: 1 })], null, 9)).toEqual([]);
  });

  it("resolves through a chapter override, like groupByEvent", () => {
    const all = [
      photo({ path: "/p/osaka.jpg", hash: "ho", eventCluster: 0 }),
      photo({ path: "/p/kyoto.jpg", hash: "hk", eventCluster: 0 }),
    ];
    const chapters = { "/p/osaka.jpg": 1, "/p/kyoto.jpg": 0 };
    expect(eventHashes(all, chapters, 1)).toEqual(["ho"]);
    expect(eventHashes(all, chapters, 0)).toEqual(["hk"]);
  });
});

describe("eventTitle", () => {
  // The whole reason this function exists (I3): numbering must come from
  // ALL of the job's events, not whatever a filter currently shows, so the
  // same event keeps the same number under any filter.
  it("numbers the same event the same way whether or not a filter would hide another event", () => {
    const all = [
      photo({ path: "/p/1.jpg", eventCluster: 1, kept: true }),
      photo({ path: "/p/2.jpg", eventCluster: 2, kept: false }),
      photo({ path: "/p/3.jpg", eventCluster: 3, kept: true }),
    ];
    // Event 3 is "Event 3" over the full set...
    expect(eventTitle(groupByEvent(all, null), {}, 3)).toBe("Event 3");
    // ...and still "Event 3" even fed groups a filter has already thinned,
    // as long as those thinned groups are what's passed. This shows the
    // caller must group ALL photos, not that the function itself filters --
    // so this asserts the unfiltered call is what SelectPhotos must make.
    const withoutEvent2 = all.filter((p) => p.kept);
    expect(eventTitle(groupByEvent(withoutEvent2, null), {}, 3)).not.toBe("Event 3");
    expect(eventTitle(groupByEvent(withoutEvent2, null), {}, 3)).toBe("Event 2");
  });

  it("prefers a place name over the positional number", () => {
    const all = [photo({ path: "/p/1.jpg", eventCluster: 1 }), photo({ path: "/p/2.jpg", eventCluster: 2 })];
    const groups = groupByEvent(all, null);
    expect(eventTitle(groups, { 2: "Kyoto" }, 2)).toBe("Kyoto");
    expect(eventTitle(groups, { 2: "Kyoto" }, 1)).toBe("Event 1");
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

  // Since useAnalysisJobs.ts applies `done` from BOTH the `invoke` return
  // value (authoritative) and the `Done` channel event (optimisation, may
  // arrive before or after, or not at all on a swallowed send failure), a
  // `done` event routinely gets applied twice for the same run. Applying it
  // a second time must be a no-op, not accumulate or corrupt state --
  // whichever of the two arrives second must leave the state exactly as the
  // first left it.
  it("applying done twice is idempotent", () => {
    const summary = { total: 1, failed: 0, cached: 0, photos: [photo()] };
    const once = applyAnalysisEvent(initialStreamState, { kind: "done", summary });
    const twice = applyAnalysisEvent(once, { kind: "done", summary });
    expect(twice).toEqual(once);
    expect(twice.summary).toBe(summary);
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

  // `before === initialStreamState` (same reference) would make
  // `expect(before).toEqual(initialStreamState)` compare the object to
  // itself, passing even if `applyAnalysisEvent` mutated its argument in
  // place. Snapshot with `structuredClone` first so this actually pins the
  // guarantee: `useAnalysisJobs.ts` assigns the shared module-level
  // `initialStreamState` straight into its ref on every run, so
  // an in-place mutation here would leak state between analysis runs.
  it("does not mutate the state object passed in (each call returns a new state)", () => {
    const before = structuredClone(initialStreamState);
    applyAnalysisEvent(initialStreamState, { kind: "scanned", total: 5 });
    expect(initialStreamState).toEqual(before);
  });
});
