import type { BookLayout, PreviewPhoto } from "./preview";

/** Mirrors `graphics::GraphicsMode`: what the user chose. */
export type GraphicsMode = "auto" | "low" | "optimised" | "maximum";

/** Mirrors `graphics::Level`: what the editor draws. */
export type GraphicsLevel = "low" | "optimised" | "maximum";

/** Mirrors `graphics::GraphicsStatus`, pinned by `tests/fixtures/wire/graphics-status.json`. */
export interface GraphicsStatus {
  mode: GraphicsMode;
  /** What Auto picks on this Mac; `null` when it could not tell, which hides Auto. */
  detected: GraphicsLevel | null;
  effective: GraphicsLevel;
}

/** Mirrors `protocol::PreviewItem`. */
export interface PreviewItem {
  path: string;
  hash: string;
}

/** Hash to the sharper preview file written for it at the current level. */
export type PreviewFiles = Record<string, string>;

const LEVEL_NAMES: Record<GraphicsLevel, string> = {
  low: "Low",
  optimised: "Optimised",
  maximum: "Maximum",
};

export interface GraphicsOption {
  value: GraphicsMode;
  label: string;
  detail: string;
}

/** The choices Settings shows, Auto first and only when it means something here. */
export function graphicsOptions(status: GraphicsStatus): GraphicsOption[] {
  const levels: GraphicsOption[] = [
    { value: "low", label: "Low", detail: "Fastest. The editor shows small previews, so photos can look soft." },
    {
      value: "optimised",
      label: "Optimised",
      detail: "Sharp on most screens. Uses the preview your camera saved, so it adds little time.",
    },
    {
      value: "maximum",
      label: "Maximum",
      detail: "Sharpest. Reads each photo in full the first time, which takes longer and uses more disk.",
    },
  ];
  if (status.detected === null) return levels;
  return [{ value: "auto", label: "Auto", detail: `${LEVEL_NAMES[status.detected]} on this Mac` }, ...levels];
}

/**
 * The book's photos in the order they are worth sharpening: the cover, then
 * every page in order. Each photo once.
 */
export function previewOrder(layout: Pick<BookLayout, "photos" | "pages" | "cover">): PreviewItem[] {
  const indices = [
    layout.cover.front.photo?.photoIndex,
    layout.cover.back.photo?.photoIndex,
    ...layout.pages.flatMap((page) => page.placements.map((p) => p.photoIndex)),
  ];
  const seen = new Set<string>();
  const items: PreviewItem[] = [];
  for (const index of indices) {
    const photo = index === undefined ? undefined : layout.photos[index];
    if (!photo || seen.has(photo.hash)) continue;
    seen.add(photo.hash);
    items.push({ path: photo.path, hash: photo.hash });
  }
  return items;
}

/** The file to draw a photo from: its sharper preview once written, else its thumbnail. */
export function photoFile(photo: Pick<PreviewPhoto, "hash" | "thumbnailPath">, previews: PreviewFiles): string | null {
  return previews[photo.hash] ?? photo.thumbnailPath;
}
