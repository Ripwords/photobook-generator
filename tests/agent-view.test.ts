import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { AgentView } from "../app/agent/view";

/**
 * The TypeScript half of the agent view's wire pin. `agent-view.json` is
 * asserted exactly by
 * `agent::view::tests::agent_view_serialises_exactly_the_keys_the_agent_reads`;
 * this reads every key `AgentView` declares from the same file, so a rename
 * on either side fails one of the two suites.
 */
const view: AgentView = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/agent-view.json", import.meta.url)), "utf8"),
) as AgentView;

describe("the agent view wire fixture", () => {
  it("carries the book's counts", () => {
    expect(view.pageCount).toBe(4);
    expect(view.placedPhotos).toBe(3);
    expect(view.droppedPhotos).toBe(1);
  });

  it("numbers openings by printed page, with slots naming photos by id", () => {
    expect(view.openings.map((o) => o.index)).toEqual([0, 1, 2]);
    expect(view.openings.map((o) => o.pages)).toEqual([[1], [2, 3], [4]]);

    const spread = view.openings[1];
    expect(spread?.templateId).toBe("07-two-up-symmetric-margin");
    expect(spread?.locked).toBe(false);
    expect(spread?.alternatives).toEqual(["08-two-up-symmetric-bleed-outer"]);
    expect(spread?.slots).toEqual([
      { page: 2, z: 1, photo: 1 },
      { page: 3, z: 1, photo: 0 },
    ]);
    expect(view.openings[0]?.locked).toBe(true);
  });

  it("carries each photo's derived facts and nothing else", () => {
    expect(view.photos[0]).toEqual({
      id: 0,
      day: 1,
      event: 1,
      faces: 1,
      faceArea: 0.0427,
      tags: ["beach", "sky", "people"],
      aestheticPct: 75,
      sharpnessPct: 50,
      placed: true,
    });
  });

  it("sends an undated photo's day as null and a left-out photo as unplaced", () => {
    const undated = view.photos.find((p) => p.id === 3);
    expect(undated?.day).toBeNull();

    const leftOut = view.photos.filter((p) => !p.placed).map((p) => p.id);
    expect(leftOut).toEqual([2]);
    expect(view.photos.find((p) => p.id === 2)?.faces).toBe(0);
  });
});
