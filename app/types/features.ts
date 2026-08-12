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
