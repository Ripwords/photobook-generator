import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";
import type { AnalyzedPhoto, PhotoOverrides } from "../app/types/features";

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
 * override must SEND the decision to Rust and take the surviving set from
 * Rust's reply. The mocked command below returns a verdict that DISAGREES
 * with the override in the obvious naive direction, so a composable that
 * applied the decision itself produces different output from one that
 * forwarded it -- which a mock returning the "expected" answer could never
 * distinguish.
 */
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { usePhotoOverrides, OVERRIDE_SETTLE_MS } = await import(
  "../app/composables/usePhotoOverrides"
);

/** The run every fixture set below came from; Rust refuses a mismatch. */
const RUN_ID = 3;
const runId = ref(RUN_ID);

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

/** Verdicts are keyed by PATH, matching Rust's `kept_paths`. */
const paths = (...hashes: string[]) => hashes.map((h) => `/p/${h}.jpg`);

describe("usePhotoOverrides", () => {
  beforeEach(() => {
    invoke.mockReset();
    vi.useRealTimers();
  });

  it("starts with the analysed set and no decisions", () => {
    const source = ref([photo("a", true), photo("b", false)]);
    const { overrides, photos } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    expect(overrides.value).toEqual({});
    expect(photos.value.map((p) => p.hash)).toEqual(["a", "b"]);
    expect(invoke).not.toHaveBeenCalled();
  });

  /**
   * The whole point. `"b"` is excluded by the user; Rust is mocked to reply
   * that `"b"` survives and `"a"` does not. That reply is deliberately the
   * OPPOSITE of what applying the override locally would produce, so the
   * assertion can only pass if Rust's answer is what reaches the screen.
   *
   * The request carries ONLY the override map: sending the records instead
   * was ~2.5 KB per photo each way, i.e. ~20 MB of round trip per click on a
   * 1000-photo folder.
   */
  it("sends the decision to Rust and takes the verdict back from it", async () => {
    const source = ref([photo("a", true), photo("b", false)]);
    invoke.mockResolvedValue(paths("b"));
    const { overrides, photos, setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    await setOverride("b", "exclude");

    expect(invoke).toHaveBeenCalledWith("apply_photo_overrides", {
      runId: RUN_ID,
      overrides: { b: "exclude" },
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(overrides.value).toEqual({ b: "exclude" });
    expect(photos.value.map((p) => [p.hash, p.kept])).toEqual([
      ["a", false],
      ["b", true],
    ]);
  });

  /**
   * The request must not carry the analysed records. This is asserted on the
   * payload's own shape rather than on a size, so it fails the moment anyone
   * reintroduces `photos:` -- which is the change that would silently take a
   * 1000-photo folder back to a ~10 MB upload per click.
   */
  it("uploads only the decisions, never the analysed records", async () => {
    const source = ref([photo("a", true), photo("b", false)]);
    invoke.mockResolvedValue(paths("a"));
    const { setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    await setOverride("b", "include");

    const [, payload] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(Object.keys(payload).toSorted()).toEqual(["overrides", "runId"]);
  });

  it("returning a photo to auto sends a map with the decision removed", async () => {
    const source = ref([photo("a", true)]);
    invoke.mockResolvedValue(paths("a"));
    const { overrides, setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    await setOverride("a", "include");
    await setOverride("a", "auto");

    expect(overrides.value).toEqual({});
    expect(invoke).toHaveBeenLastCalledWith("apply_photo_overrides", { runId: RUN_ID, overrides: {} });
  });

  /**
   * A failed command must not silently leave a stale verdict on screen with
   * no explanation. The decision itself is kept -- it is the user's stated
   * intent, and `generate_book` is what finally acts on it.
   */
  it("reports a failed re-check rather than swallowing it", async () => {
    const source = ref([photo("a", true)]);
    invoke.mockRejectedValue(new Error("sidecar unavailable"));
    const { overrides, error, busy, setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    await setOverride("a", "exclude");

    expect(error.value).toContain("sidecar unavailable");
    expect(overrides.value).toEqual({ a: "exclude" });
    expect(busy.value).toBe(false);
  });

  /**
   * **The race.** Two clicks in flight, and the FIRST reply lands last.
   * Without sequencing the stale verdict wins and the contact sheet shows a
   * selection that is not the one Rust computed -- and the override map stays
   * correct, so the printed book is right and only the screen lies, which is
   * worse than a visible bug because nothing about it looks wrong.
   *
   * The debounce is bypassed here (each `setOverride` is awaited past its
   * timer via fake timers) so the test exercises the generation counter and
   * not merely the coalescing that usually hides this.
   */
  it("ignores a slow reply that a newer one has already superseded", async () => {
    const source = ref([photo("a", true), photo("b", true)]);
    const resolvers: Array<(value: string[]) => void> = [];
    invoke.mockImplementation(
      () => new Promise<string[]>((resolve) => resolvers.push(resolve)),
    );
    const { photos, setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    vi.useFakeTimers();
    const first = setOverride("a", "exclude");
    await vi.advanceTimersByTimeAsync(OVERRIDE_SETTLE_MS);
    const second = setOverride("b", "exclude");
    await vi.advanceTimersByTimeAsync(OVERRIDE_SETTLE_MS);
    expect(resolvers).toHaveLength(2);

    // The newer call answers first, then the stale one arrives.
    resolvers[1]!(paths());
    resolvers[0]!(paths("a", "b"));
    await second;
    await first;
    vi.useRealTimers();

    expect(photos.value.map((p) => p.kept)).toEqual(
      [false, false],
      "the stale reply must not overwrite the newer verdict",
    );
  });

  /**
   * Clicking through a row of photos must not fire one whole cull per click.
   * Three toggles inside the settle window are one command carrying all
   * three decisions.
   */
  it("coalesces a burst of clicks into a single command", async () => {
    const source = ref([photo("a", true), photo("b", true), photo("c", true)]);
    invoke.mockResolvedValue(paths("a"));
    const { setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    vi.useFakeTimers();
    void setOverride("a", "exclude");
    void setOverride("b", "exclude");
    const last = setOverride("c", "include");
    await vi.advanceTimersByTimeAsync(OVERRIDE_SETTLE_MS);
    vi.useRealTimers();
    await last;

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("apply_photo_overrides", {
      runId: RUN_ID,
      overrides: { a: "exclude", b: "exclude", c: "include" },
    });
  });

  /**
   * The decisions belong to the job, not to this composable, and the job
   * clears them on a new run (see `tests/jobs.test.ts`). What this side owes
   * is that decisions it is handed are judged by Rust, never assumed: `"a"`
   * is included here and Rust answers that only `"b"` survives.
   */
  it("has Rust judge decisions it was handed along with a set", async () => {
    const source = ref([photo("a", true), photo("b", true)]);
    invoke.mockResolvedValue(paths("b"));
    const { photos } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({ a: "exclude" }));
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));

    expect(invoke).toHaveBeenCalledWith("apply_photo_overrides", {
      runId: RUN_ID,
      overrides: { a: "exclude" },
    });
    await vi.waitFor(() =>
      expect(photos.value.map((p) => [p.hash, p.kept])).toEqual([
        ["a", false],
        ["b", true],
      ]),
    );
  });

  it("asks Rust nothing about a set with no decisions", async () => {
    const source = ref([photo("a", true)]);
    usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));
    source.value = [photo("z", true)];
    await nextTick();
    expect(invoke).not.toHaveBeenCalled();
  });

  /**
   * **`photoSetId` is the identity `GenerateBook` watches, and it must not
   * move on a toggle.**
   *
   * It used to watch `photos`, whose array identity changes on every
   * override, and its reset branch then discarded the generated book, the
   * opened project, the output directory, a typed name and a chosen page
   * length -- on every single click. A counter that ticked per toggle would
   * reintroduce exactly that.
   */
  it("bumps the photo-set identity once per analysis and never on a toggle", async () => {
    const source = ref([photo("a", true), photo("b", true)]);
    invoke.mockResolvedValue(paths("a"));
    const { photoSetId, setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));
    const afterFirstSet = photoSetId.value;

    await setOverride("a", "exclude");
    await setOverride("b", "include");
    await setOverride("a", "auto");

    expect(photoSetId.value).toBe(afterFirstSet, "a toggle is not a new photo set");

    source.value = [photo("z", true)];
    await nextTick();

    expect(photoSetId.value).toBe(afterFirstSet + 1, "a new analysis is");
  });

  it("counts the decisions for display without recomputing the verdict", async () => {
    const source = ref([photo("a", true), photo("b", true), photo("c", true)]);
    invoke.mockResolvedValue(paths("a"));
    const { includedCount, excludedCount, setOverride } = usePhotoOverrides(source, runId, ref<PhotoOverrides>({}));

    await setOverride("a", "include");
    await setOverride("b", "include");
    await setOverride("c", "exclude");

    expect(includedCount.value).toBe(2);
    expect(excludedCount.value).toBe(1);
  });
});

/**
 * **A source pin, and it is weaker than a behavioural test — labelled as such.**
 *
 * This project has no component test harness, so `GenerateBook.vue`'s
 * watchers cannot be driven. The regression they carry is severe enough to
 * be worth an imperfect net: the reset branch discards the generated book,
 * the opened project, the output directory, a name the user typed and a page
 * length they chose, so keying it on `photos` -- whose array identity changes
 * on every override -- destroyed all five on every single click.
 *
 * `usePhotoOverrides > bumps the photo-set identity once per analysis and
 * never on a toggle` is the behavioural half: it guarantees `photoSetId` is
 * stable across toggles. This half guarantees the component watches that and
 * not the array. Neither alone is sufficient.
 */
describe("GenerateBook's reset is keyed on the photo set, not the array", () => {
  const source = readFileSync(
    fileURLToPath(new URL("../app/components/GenerateBook.vue", import.meta.url)),
    "utf-8",
  );

  it("watches the photo-set identity", () => {
    expect(source).toMatch(/watch\(\s*\(\)\s*=>\s*photoSetId\s*,/);
  });

  it("never watches the photos array, which changes on every toggle", () => {
    expect(source).not.toMatch(/watch\(\s*\(\)\s*=>\s*photos\s*,/);
  });

});

/**
 * The other half of "restored overrides are inert": `selectionLabel` is
 * unit-tested above, but a project's decisions are only actually visible if
 * the screen RENDERS it, and only editable if the control that re-analyses
 * the folders exists. Same source-pin caveat as the watchers above.
 *
 * Both live on `BookEditor.vue`, which is the screen a saved book opens into.
 * They were on `GenerateBook.vue` until that component was split into the
 * "pick a length and generate" panel and the editor screen.
 */
describe("the editor renders a project's restored selection and can edit it", () => {
  const source = readFileSync(
    fileURLToPath(new URL("../app/components/BookEditor.vue", import.meta.url)),
    "utf-8",
  );

  it("renders the restored selection and offers a way to edit it", () => {
    expect(source).toContain("restoredSelection");
    expect(source).toMatch(/emit\(\s*["']editPhotos["']/);
  });
});
