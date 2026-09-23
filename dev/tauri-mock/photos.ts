/**
 * Synthetic analysed photos for the browser harness. See `core.ts`.
 *
 * Thumbnails are picsum.photos URLs rather than file paths: `convertFileSrc` in
 * the harness returns what it is given, so a URL renders in an ordinary browser
 * and the contact sheet, the book preview and the README screenshots show real
 * photographs instead of 36 broken-image icons. A preview judged against empty
 * boxes is not judged. They are stock images, never the user's, so the harness
 * needs a network connection but nothing about the privacy rule changes; the
 * real app's CSP would block them, which is fine because it never sees them.
 */
import type { AnalyzedPhoto } from "../../app/types/features";

/**
 * The engine-only keys `AnalyzedPhoto` deliberately does not declare, which
 * Rust reads off the forwarded records. Nothing in the harness parses them;
 * they are here so the mock's records are the same SHAPE as the real ones.
 */
interface EnginePhoto extends AnalyzedPhoto {
  faces: { captureQuality: number }[];
  faceAreaFraction: number;
  saliencyBox: { x: number; y: number; w: number; h: number };
  palette: number[][];
}

/**
 * Picsum ids picked by eye to look like one trip: coast, mountains, towns and
 * food, no laptops or flat lays. Pinned by id so every run, and every
 * screenshot, shows the same pictures.
 */
const PICSUM_IDS = [
  10, 11, 12, 13, 14, 15, 16, 17, 27, 28, 29, 100, 108, 110, 162, 163, 164, 166, 170, 270, 338, 342, 349,
  1015, 1016, 1018, 1022, 1026, 1035, 1036, 1039, 1040, 1043, 1044, 1045, 1047, 1049, 1050, 1051, 1052,
  1057, 1060, 1061, 1065, 1067,
];

/** A real photograph, cropped to the orientation of the photo it stands in for. */
export function thumbnail(index: number, landscape: boolean): string {
  const id = PICSUM_IDS[index % PICSUM_IDS.length] ?? PICSUM_IDS[0];
  return `https://picsum.photos/id/${id}/${landscape ? "400/300" : "300/400"}`;
}

/**
 * A folder's worth of analysed photos: four event clusters, two bursts of
 * near-duplicates, and three utility images, so the contact sheet has every
 * badge it can draw and the keeper count is not simply the total.
 */
export function mockPhotos(count = 36): AnalyzedPhoto[] {
  const photos: EnginePhoto[] = [];

  for (let i = 0; i < count; i += 1) {
    const label = `IMG_${String(1000 + i)}`;
    const isUtility = i % 13 === 5;
    // Photos 8-10 and 22-24 share a near-duplicate cluster, so those render as
    // a burst with one winner; every other photo is its own cluster.
    const burst = (i >= 8 && i <= 10) || (i >= 22 && i <= 24);
    const nearDupCluster = burst ? (i <= 10 ? 900 : 901) : i;
    const winner = i === 9 || i === 23;
    const landscape = i % 3 !== 1;

    photos.push({
      status: "ok",
      path: `/mock/Holiday 2026/${label}.jpg`,
      hash: `hash-${i.toString(16).padStart(4, "0")}`,
      width: landscape ? 4032 : 3024,
      height: landscape ? 3024 : 4032,
      isUtility,
      faceCount: i % 4,
      smileFraction: i % 4 === 0 ? undefined : ((i * 17) % 100) / 100,
      sceneTags: ["outdoor", "beach", "portrait", "food"].slice(0, (i % 3) + 1),
      // A burst is the same moment shot again, so it shows the same picture.
      thumbnailPath: thumbnail(burst ? (i <= 10 ? 8 : 22) : i, landscape),
      aestheticPct: (i * 37) % 100,
      sharpnessPct: (i * 61) % 100,
      nearDupCluster,
      eventCluster: Math.floor(i / 9),
      kept: !isUtility && (!burst || winner),
      faces: Array.from({ length: i % 4 }, () => ({ captureQuality: 0.6 })),
      faceAreaFraction: (i % 4) * 0.04,
      saliencyBox: { x: 0.2, y: 0.2, w: 0.6, h: 0.6 },
      palette: [[0.6, 0.1, -0.05]],
    });
  }

  return photos;
}

/**
 * What `place_chapters` returns for `mockPhotos`: each event moves to a
 * second town after its fifth photo. `located` is 0 when `noLocation` is set,
 * for the draft screen's disabled switch.
 */
export function mockPlaceChapters(noLocation: boolean, count = 36) {
  const chapters: Record<string, number> = {};
  for (const [i, photo] of mockPhotos(count).entries()) {
    chapters[photo.path] = noLocation ? photo.eventCluster : photo.eventCluster * 2 + (i % 9 >= 5 ? 1 : 0);
  }
  return { located: noLocation ? 0 : count, chapters };
}

/**
 * Names for `mockPlaceChapters`' chapters, in chapter order. One chapter is
 * left unnamed, as a lookup Apple could not answer is, and two names are
 * long enough to exercise the sheet header's give-way order (fix3):
 * ~30 chars and ~80 chars, both past the point a short town name would
 * ever reach, so a header stress-tested only against them would still
 * have missed the fix3 overflow.
 */
const MOCK_TOWNS = [
  "Kyoto",
  "Arashiyama",
  null,
  "Osaka",
  "Llanfairpwllgwyngyll, Anglesey",
  "Llanfairpwllgwyngyllgogerychwyrndrobwllllantysiliogogogoch, Isle of Anglesey, Wales",
  "Nara",
];

export function mockPlaceNames(noLocation: boolean, count = 36): Record<number, string> {
  if (noLocation) return {};
  const ids = [...new Set(Object.values(mockPlaceChapters(false, count).chapters))].toSorted((a, b) => a - b);
  return Object.fromEntries(
    ids.flatMap((id, i) => {
      const town = MOCK_TOWNS[i % MOCK_TOWNS.length];
      return town ? [[id, town]] : [];
    }),
  );
}
