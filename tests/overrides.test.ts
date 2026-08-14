import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";
import type { AnalyzedPhoto } from "../app/types/features";

/**
 * **There is exactly one culling authority, and this file is what proves the
 * webview did not become a second one.**
 *
 * Phase 2 spent a whole task collapsing a Rust rule and a TypeScript rule
 * into one, because the contact sheet and the printed book disagreed about
 * which photos survived. `usePhotoOverrides` is where that could happen
 * again: an include/exclude toggle is trivially applicable on the client, and
 * doing so would put the rule back.
 *
 * So the contract asserted here is behavioural, not stylistic: toggling an
 * override must SEND the decision to Rust and take the resulting `kept`
 * stamps from Rust's reply. The mocked command below returns a verdict that
 * DISAGREES with the override in the obvious naive direction, so a composable
 * that applied the decision itself produces different output from one that
 * forwarded it -- which a mock returning the "expected" answer could never
 * distinguish.
 */
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { usePhotoOverrides } = await import("../app/composables/usePhotoOverrides");

function photo(hash: string, kept: boolean): AnalyzedPhoto {
  return {
    status: "ok",
    path: `/p/${hash}.jpg`,
    hash,
    width: 4032,
    height: 3024,
    isUtility: false,
    faceCount: 0,
    sceneTags: [],
    aestheticPct: 50,
    sharpnessPct: 50,
    nearDupCluster: 1,
    eventCluster: 1,
    kept,
  };
}

describe("usePhotoOverrides", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("starts with the analysed set and no decisions", () => {
    const source = ref([photo("a", true), photo("b", false)]);
    const { overrides, photos } = usePhotoOverrides(source);

    expect(overrides.value).toEqual({});
    expect(photos.value.map((p) => p.hash)).toEqual(["a", "b"]);
    expect(invoke).not.toHaveBeenCalled();
  });

  /**
   * The whole point. `"b"` is excluded by the user; Rust is mocked to reply
   * that `"b"` is nonetheless `kept` and `"a"` is not. That reply is
   * deliberately the OPPOSITE of what applying the override locally would
   * produce, so the assertion can only pass if Rust's answer is what reaches
   * the screen.
   */
  it("sends the decision to Rust and takes the verdict back from it", async () => {
    const source = ref([photo("a", true), photo("b", false)]);
    invoke.mockResolvedValue([photo("a", false), photo("b", true)]);
    const { overrides, photos, setOverride } = usePhotoOverrides(source);

    await setOverride("b", "exclude");

    expect(invoke).toHaveBeenCalledWith("apply_photo_overrides", {
      photos: [photo("a", true), photo("b", false)],
      overrides: { b: "exclude" },
    });
    expect(overrides.value).toEqual({ b: "exclude" });
    expect(photos.value.map((p) => [p.hash, p.kept])).toEqual([
      ["a", false],
      ["b", true],
    ]);
  });

  it("returning a photo to auto sends a map with the decision removed", async () => {
    const source = ref([photo("a", true)]);
    invoke.mockImplementation((_cmd: string, args: { photos: AnalyzedPhoto[] }) => args.photos);
    const { overrides, setOverride } = usePhotoOverrides(source);

    await setOverride("a", "include");
    await setOverride("a", "auto");

    expect(overrides.value).toEqual({});
    expect(invoke).toHaveBeenLastCalledWith("apply_photo_overrides", {
      photos: [photo("a", true)],
      overrides: {},
    });
  });

  /**
   * A failed command must not silently leave a stale verdict on screen with
   * no explanation. The decision itself is kept -- it is the user's stated
   * intent, and `generate_book` is what finally acts on it.
   */
  it("reports a failed re-check rather than swallowing it", async () => {
    const source = ref([photo("a", true)]);
    invoke.mockRejectedValue(new Error("sidecar unavailable"));
    const { overrides, error, busy, setOverride } = usePhotoOverrides(source);

    await setOverride("a", "exclude");

    expect(error.value).toContain("sidecar unavailable");
    expect(overrides.value).toEqual({ a: "exclude" }, "the user's decision is not discarded");
    expect(busy.value).toBe(false);
  });

  /**
   * Decisions are keyed by content hash, so carrying them across to a
   * different analysed folder would apply one folder's decision to an
   * identical file in another -- silently, and with no way for the user to
   * see it had happened.
   */
  it("forgets every decision when a different folder is analysed", async () => {
    const source = ref([photo("a", true)]);
    invoke.mockImplementation((_cmd: string, args: { photos: AnalyzedPhoto[] }) => args.photos);
    const { overrides, photos, setOverride } = usePhotoOverrides(source);
    await setOverride("a", "exclude");
    expect(overrides.value).toEqual({ a: "exclude" });

    source.value = [photo("z", true)];
    await nextTick();

    expect(overrides.value).toEqual({});
    expect(photos.value.map((p) => p.hash)).toEqual(["z"]);
  });
});
