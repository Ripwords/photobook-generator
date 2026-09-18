/**
 * The TypeScript half of the privacy chokepoint's leak test.
 *
 * `agent::view::tests::agent_view_contains_no_path_hash_gps_timestamp_face_box_or_raw_score`
 * searches the serialised `AgentView` for every value design §9.1 withholds.
 * This is the same search, for anything the webview hands a model: a tool
 * result, a Jev request body. It searches values, not keys, because a leaked
 * value under a renamed key still leaves the machine.
 *
 * `WITHHELD` is the Rust test's list, from the same secret record.
 */
export const WITHHELD: readonly string[] = [
  "/Users/alice",
  "IMG_4821",
  "Lisbon",
  "HEIC",
  "9f2c41d7e8ab3605",
  "51.501364",
  "-0.141889",
  "0.141889",
  "1726012345",
  "1726141945",
  "0.1234",
  "0.2345",
  "0.0913",
  "0.1177",
  "0.6621",
  "0.6123",
  "0.0789",
  "0.2718",
  "0.3141",
  "0.873142",
  "1234.567",
  "Canon",
  "EOS R5",
  "0.8123",
  "0.5512",
  "12345678901234",
  "c0ffee",
  "4284",
  "5712",
];

/**
 * A value carrying every withheld value, shaped like the photo records and
 * layouts Rust holds. Hand it to a mock wherever a real command returns
 * something the model must not see (`edit_book` returns a `BookLayout` with
 * file names; a refusal is a string Rust wrote), so a code path that forwards
 * it fails `leaksIn`.
 */
export const POISON = {
  path: "/Users/alice/Pictures/Lisbon Trip/IMG_4821.HEIC",
  hash: "9f2c41d7e8ab3605",
  width: 4284,
  height: 5712,
  exif: {
    latitude: 51.501364,
    longitude: -0.141889,
    captureDate: 1726012345,
    laterCapture: 1726141945,
    make: "Canon",
    model: "EOS R5",
  },
  aestheticScore: 0.873142,
  sharpness: 1234.567,
  faces: [{ box: [0.1234, 0.2345, 0.0913, 0.1177], captureQuality: 0.6621 }],
  saliencyBox: [0.6123, 0.0789, 0.2718, 0.3141],
  palette: [{ r: 0.8123, weight: 0.5512 }],
  phash: "12345678901234",
  thumbnailPath: "/Users/alice/Library/Caches/pbg/thumbs/c0ffee.jpg",
};

/** Every withheld value found anywhere in `value`'s JSON. */
export function leaksIn(value: unknown): string[] {
  const text = JSON.stringify(value) ?? "";
  return WITHHELD.filter((secret) => text.includes(secret));
}
