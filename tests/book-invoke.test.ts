import { beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import type { AnalyzedPhoto, EventTiers, PhotoOverrides } from "../app/types/features";
import type { BookOptions } from "../app/types/book";
import type { PrintSpec } from "../app/types/printSpec";

/**
 * `useBook`'s doc comment says `tiers` is "Sent with BOTH commands: ...
 * like `overrides`", but nothing pinned the actual `invoke` payload -- so a
 * refactor could silently stop sending it and every other test would stay
 * green (Rust would just see an empty `EventTiers` and fall back to its own
 * suggestions, not throw). This file is that pin.
 */
const invoke = vi.hoisted(() => vi.fn());
// A stub, not a mock: `exportBook` (the only caller of `Channel`) is not
// exercised by this file, but the module import still needs the name.
class StubChannel {
  onmessage: (message: unknown) => void = () => {};
}
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: StubChannel }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { useBook } = await import("../app/composables/useBook");

const TIERS: EventTiers = { abc123: "featured" };
const OVERRIDES: PhotoOverrides = { abc123: "include" };
const BOOK_OPTIONS: BookOptions = { places: false, featuredFloor: 6, briefCap: 2 };

function photo(hash: string): AnalyzedPhoto {
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
    kept: true,
  };
}

describe("useBook invoke payloads", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue({});
  });

  it("sends tiers and options to recommend_book", async () => {
    const { refreshRecommendation } = useBook(ref([photo("abc123")]), ref(["/f"]), ref(OVERRIDES), ref(TIERS), ref(7));

    await refreshRecommendation(BOOK_OPTIONS);

    expect(invoke).toHaveBeenCalledWith(
      "recommend_book",
      expect.objectContaining({ tiers: TIERS, options: BOOK_OPTIONS }),
    );
  });

  it("sends tiers to generate_book", async () => {
    const { generate } = useBook(ref([photo("abc123")]), ref(["/f"]), ref(OVERRIDES), ref(TIERS), ref(7));

    await generate("My book", 20, null as PrintSpec | null, BOOK_OPTIONS);

    expect(invoke).toHaveBeenCalledWith("generate_book", expect.objectContaining({ tiers: TIERS }));
  });
});
