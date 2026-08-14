/**
 * Everything intrinsic to a single photo: derivable the instant that ONE
 * photo has been analyzed, with no knowledge of the rest of the folder.
 * Rust's `partial_photo` (`src-tauri/src/commands.rs`) produces exactly this
 * shape and streams it in `AnalysisEvent.Batch` as each batch of photos
 * finishes, so the UI can render a tile long before the whole folder is
 * done.
 *
 * Deliberately does NOT include `aestheticPct`, `sharpnessPct`,
 * `nearDupCluster`, or `eventCluster` -- those are whole-set derivations
 * (percentile rank, near-duplicate/event clustering) that don't exist, even
 * provisionally, until every photo in the folder has been seen. A tile
 * typed against `PartialAnalyzedPhoto` structurally cannot render a
 * percentile that will later change out from under the viewer.
 */
export interface PartialAnalyzedPhoto {
  status: "ok";
  path: string;
  hash: string;
  width: number;
  height: number;
  isUtility: boolean;
  faceCount: number;
  /**
   * Swift's synthesized `Codable` uses `encodeIfPresent` for optional
   * properties, so a `nil` `smileFraction` is OMITTED from the wire JSON
   * entirely rather than encoded as `null`. `JSON.parse` then leaves this
   * `undefined`, not `null`, on the deserialized object -- hence `?:` here,
   * not just `| null`. Always read this through `smilePercent()` below
   * rather than comparing directly against `null`.
   */
  smileFraction?: number | null;
  sceneTags: string[];
  /**
   * Small JPEG contact-sheet thumbnail path, or absent/null if writing it
   * failed -- same `encodeIfPresent` omission as `smileFraction` above.
   * Currently safe to read with a plain truthy check (`undefined`, `null`
   * and `""` are all falsy), but typed accurately here regardless.
   */
  thumbnailPath?: string | null;
}

/**
 * A fully-ranked photo: `PartialAnalyzedPhoto` plus the whole-set
 * derivations that only exist once `analyze_folder` has seen every photo in
 * the folder (Rust's `finalize_photos`, delivered in `AnalysisEvent.Done`).
 */
export interface AnalyzedPhoto extends PartialAnalyzedPhoto {
  aestheticPct: number;
  sharpnessPct: number;
  nearDupCluster: number;
  eventCluster: number;
  /**
   * Whether this photo survives culling -- Rust's VERDICT, not an input to
   * one. Stamped by `commands::stamp_kept` from `book::cull::cull`, the
   * single authority on which photo survives, and the same function the
   * printed book is built from.
   *
   * Do NOT recompute this in the webview. It used to be recomputed here (see
   * `keepers()` below) and the two implementations ranked differently, so the
   * contact sheet could disagree with the book about both the count and the
   * identity of the survivors. The webview cannot get it right by copying the
   * rule more carefully either: the rule's sharpness tie-break reads a face's
   * `captureQuality`, which `AnalyzedPhoto` does not carry.
   *
   * Only present on a fully-ranked photo, never on a streaming
   * `PartialAnalyzedPhoto`: culling is a whole-set derivation (it needs
   * `nearDupCluster`, which does not exist until every photo has been seen),
   * exactly like the percentiles above it.
   */
  kept: boolean;
}

/** Type guard distinguishing a still-streaming tile from a fully-ranked one. */
export function isRanked(photo: PartialAnalyzedPhoto | AnalyzedPhoto): photo is AnalyzedPhoto {
  return "aestheticPct" in photo;
}

export interface FailedPhoto {
  status: "failed";
  path: string;
  message: string;
}

export type PhotoRecord = AnalyzedPhoto | FailedPhoto;

export interface AnalysisSummary {
  total: number;
  failed: number;
  cached: number;
  photos: AnalyzedPhoto[];
}

export function isFailed(record: PhotoRecord): record is FailedPhoto {
  return record.status === "failed";
}

// --- Streaming: mirrors Rust's `AnalysisEvent` (`src-tauri/src/commands.rs`)
// exactly, field for field. Sent over a `tauri::ipc::Channel`, not Tauri's
// event system -- events are JSON-string-only and explicitly not designed
// for high-throughput/low-latency streaming per Tauri's own docs, which a
// folder of hundreds of photos with thumbnails very much is.

export interface ScannedEvent {
  kind: "scanned";
  total: number;
}

export interface BatchEvent {
  kind: "batch";
  /** Only the photos that finished in THIS batch -- append these, do not replace. */
  photos: PartialAnalyzedPhoto[];
  /**
   * Cumulative running totals as of this event, NOT per-batch deltas --
   * read the latest event's value directly (e.g. for "analysed X / Y"),
   * never sum `analysed`/`cached`/`failed` across events. Summing them
   * double-counts every photo already reflected in an earlier `Batch`.
   */
  analysed: number;
  cached: number;
  failed: number;
}

export interface DoneEvent {
  kind: "done";
  summary: AnalysisSummary;
}

export type AnalysisEvent = ScannedEvent | BatchEvent | DoneEvent;

/**
 * Accumulated state built up from a stream of `AnalysisEvent`s over the
 * course of one `analyze_folder` run.
 */
export interface StreamState {
  scannedTotal: number;
  /** Partial photos in arrival order -- the order batches resolved in, which is input (path) order; see `applyAnalysisEvent`. */
  partialPhotos: PartialAnalyzedPhoto[];
  analysed: number;
  cached: number;
  failed: number;
  summary: AnalysisSummary | null;
}

// `useAnalysis.ts` assigns this shared, module-level object straight into
// its ref on every `analyze()` call, so it must never be mutated in place --
// doing so would leak state between analysis runs permanently. `Object.freeze`
// makes that guarantee structural: a future reducer branch that mutates
// `state` instead of spreading it throws immediately in dev/test rather than
// silently corrupting state that looks fine until the second run.
export const initialStreamState: StreamState = Object.freeze({
  scannedTotal: 0,
  partialPhotos: [],
  analysed: 0,
  cached: 0,
  failed: 0,
  summary: null,
});

/**
 * Pure reducer over `AnalysisEvent`s: the entire client-side "batching
 * accumulation" logic, extracted so it is unit-testable without a live
 * Tauri `Channel`/`invoke` -- the same reasoning `finalize_photos` and
 * `lookup_cache` document on the Rust side.
 *
 * `Batch` events are guaranteed by the Rust side (`analyze_batches_with_progress`,
 * `SidecarPool::analyze_all`) to arrive in the same order their photos were
 * submitted in, one event per resolved batch (including a failed batch,
 * which streams zero photos but still advances `failed`) -- so appending
 * `event.photos` here, in event-arrival order, reconstructs exactly the same
 * order `finalize_photos` sees before it re-sorts by path. A failed batch
 * contributes no photos and therefore cannot shift a later batch's photos to
 * the wrong position in this array.
 */
export function applyAnalysisEvent(state: StreamState, event: AnalysisEvent): StreamState {
  switch (event.kind) {
    case "scanned":
      return { ...state, scannedTotal: event.total };
    case "batch":
      return {
        ...state,
        partialPhotos: [...state.partialPhotos, ...event.photos],
        analysed: event.analysed,
        cached: event.cached,
        failed: event.failed,
      };
    case "done":
      return { ...state, summary: event.summary };
  }
}

/**
 * The photos that survive culling: exactly the ones the book is built from.
 *
 * This is a FILTER, not a rule. `book::cull::cull` in Rust decides which
 * photo survives -- dropping utility images, then keeping one winner per
 * near-duplicate cluster ranked sharpness -> face capture quality ->
 * aesthetic -- and `commands::stamp_kept` writes that verdict onto each
 * record as `kept`. Reading the flag is the whole implementation.
 *
 * It used to re-derive the rule here, and that was a real user-visible
 * defect rather than mere duplication: this copy went straight from
 * sharpness to aesthetic with no capture-quality tie-break, so on any burst
 * where two frames tied on sharpness the screen could show one survivor and
 * the exported book contain another. Reproducing the Rust rule faithfully is
 * not an option from here -- `AnalyzedPhoto` does not carry the
 * `captureQuality` the tie-break reads.
 *
 * Input order is preserved, which is `finalize_photos`'s path order -- the
 * same order `cull` itself returns.
 */
export function keepers(photos: AnalyzedPhoto[]): AnalyzedPhoto[] {
  return photos.filter((photo) => photo.kept);
}

export interface EventGroup {
  eventCluster: number;
  photos: AnalyzedPhoto[];
}

/**
 * Groups photos into event chapters by `eventCluster`, ordered
 * chronologically by cluster id.
 *
 * The input array's own order is NOT chronological: it comes from
 * `finalize_photos` in Rust, which sorts by `path` for UI stability, while
 * `eventCluster` ids are assigned chronologically by capture time
 * (`cluster::event_clusters`). Filename order only matches capture order by
 * coincidence -- multiple cameras, re-exported files, or mixed prefixes all
 * break it. Grouping by first-appearance-in-input (the previous behaviour
 * here) rendered chapters under sequential "Event N" labels in whatever
 * order their earliest photo happened to sort alphabetically, not the order
 * they actually happened. Sorting by `eventCluster` numerically fixes this;
 * the undated bucket (always the highest id -- see `event_clusters`) sorts
 * last as a result, which is the desired place for it regardless of where
 * its members' filenames happen to fall.
 */
export function groupByEvent(photos: AnalyzedPhoto[]): EventGroup[] {
  const byCluster = new Map<number, AnalyzedPhoto[]>();

  for (const photo of photos) {
    let bucket = byCluster.get(photo.eventCluster);
    if (!bucket) {
      bucket = [];
      byCluster.set(photo.eventCluster, bucket);
    }
    bucket.push(photo);
  }

  return [...byCluster.entries()]
    .toSorted(([a], [b]) => a - b)
    .map(([eventCluster, groupPhotos]) => ({ eventCluster, photos: groupPhotos }));
}

/**
 * Counts how many non-utility photos share each near-duplicate cluster, so a
 * surviving photo can be labelled with the size of the burst it came from.
 */
export function burstSizes(photos: AnalyzedPhoto[]): Map<number, number> {
  const sizes = new Map<number, number>();

  for (const photo of photos) {
    if (photo.isUtility) continue;
    sizes.set(photo.nearDupCluster, (sizes.get(photo.nearDupCluster) ?? 0) + 1);
  }

  return sizes;
}

/**
 * Converts a smile-likelihood fraction (0...1) into a whole-number percent
 * for display, or `null` when no usable signal exists.
 *
 * Must use `== null` (loose equality), which is true for both `null` and
 * `undefined`. Swift's synthesized `Codable` uses `encodeIfPresent` for
 * optional properties, so a `nil` `smileFraction` never reaches the wire as
 * `smileFraction: null` -- the key is omitted entirely, and `JSON.parse`
 * leaves it `undefined`. A strict `=== null` check here lets `undefined`
 * through to `Math.round(undefined * 100)`, which is `NaN` -- and `NaN`
 * happily fails a `!== null` render guard too, so it reaches the DOM as the
 * literal string "NaN%". This fires whenever a face has unusable pose
 * (|yaw| or |pitch| > 30 degrees) or too few lip landmark points, which is
 * routine on real photos.
 */
export function smilePercent(fraction: number | null | undefined): number | null {
  return fraction == null ? null : Math.round(fraction * 100);
}

/** Returns the last path segment, for showing a folder name instead of a full path. */
export function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const segments = trimmed.split("/");
  return segments[segments.length - 1] || path;
}

/**
 * Picks the single top-ranked photo from a group (an event chapter's kept
 * photos), by aesthetic percentile, tie-broken by sharpness percentile.
 * This is the one "hero" per group: the pastel-yellow accent marks exactly
 * this photo and nothing else, so the marking stays meaningful.
 */
export function pickHero(photos: AnalyzedPhoto[]): AnalyzedPhoto | undefined {
  let best: AnalyzedPhoto | undefined;

  for (const photo of photos) {
    if (
      !best ||
      photo.aestheticPct > best.aestheticPct ||
      (photo.aestheticPct === best.aestheticPct && photo.sharpnessPct > best.sharpnessPct)
    ) {
      best = photo;
    }
  }

  return best;
}
