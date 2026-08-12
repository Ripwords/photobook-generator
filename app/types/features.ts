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
  smileFraction: number | null;
  sceneTags: string[];
  nearDupCluster: number;
  eventCluster: number;
  /** Small JPEG contact-sheet thumbnail path, or null if writing it failed. */
  thumbnailPath: string | null;
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
 * Groups photos into event chapters by `eventCluster`, preserving the order
 * in which each cluster first appears in the input.
 */
export function groupByEvent(photos: AnalyzedPhoto[]): EventGroup[] {
  const order: number[] = [];
  const byCluster = new Map<number, AnalyzedPhoto[]>();

  for (const photo of photos) {
    let bucket = byCluster.get(photo.eventCluster);
    if (!bucket) {
      bucket = [];
      byCluster.set(photo.eventCluster, bucket);
      order.push(photo.eventCluster);
    }
    bucket.push(photo);
  }

  return order.map((eventCluster) => ({
    eventCluster,
    photos: byCluster.get(eventCluster) ?? [],
  }));
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
