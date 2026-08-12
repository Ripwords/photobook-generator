export interface AnalyzedPhoto {
  status: "ok";
  path: string;
  hash: string;
  width: number;
  height: number;
  isUtility: boolean;
  aestheticPct: number;
  sharpnessPct: number;
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
  nearDupCluster: number;
  eventCluster: number;
  /**
   * Small JPEG contact-sheet thumbnail path, or absent/null if writing it
   * failed -- same `encodeIfPresent` omission as `smileFraction` above.
   * Currently safe to read with a plain truthy check (`undefined`, `null`
   * and `""` are all falsy), but typed accurately here regardless.
   */
  thumbnailPath?: string | null;
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

/**
 * Drops utility images, then keeps the best photo from each near-duplicate
 * cluster, ranked by sharpness then aesthetic percentile.
 */
export function keepers(photos: AnalyzedPhoto[]): AnalyzedPhoto[] {
  const best = new Map<number, AnalyzedPhoto>();

  for (const photo of photos) {
    if (photo.isUtility) continue;
    const incumbent = best.get(photo.nearDupCluster);
    if (
      !incumbent ||
      photo.sharpnessPct > incumbent.sharpnessPct ||
      (photo.sharpnessPct === incumbent.sharpnessPct &&
        photo.aestheticPct > incumbent.aestheticPct)
    ) {
      best.set(photo.nearDupCluster, photo);
    }
  }

  return [...best.values()];
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
