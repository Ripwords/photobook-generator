import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  customLayoutErrorText,
  problemSlots,
  type CustomLayout,
  type CustomLayoutError,
  type Problem,
  type RefusedLayout,
} from "../app/types/customLayouts";

/**
 * The TypeScript half of `tests/fixtures/wire/custom-layouts.json`; the Rust
 * half is `custom_layouts_serialise_exactly_the_keys_the_webview_reads`. A key
 * renamed on either side reads as `undefined` here, or fails `to_value` there.
 */
interface Fixture {
  layouts: CustomLayout[];
  problems: Problem[];
  refused: RefusedLayout;
  errors: CustomLayoutError[];
}

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/custom-layouts.json", import.meta.url)), "utf8"),
) as Fixture;

describe("custom layouts on the wire", () => {
  it("reads a spread and a page layout with their boxes and flags", () => {
    const [spread, page] = fixture.layouts;
    expect(spread?.id).toMatch(/^user-[0-9a-f]{32}$/);
    expect(spread?.auto).toBe(false);
    expect(spread?.shape.kind).toBe("spread");
    expect(spread?.shape.boxes.map((b) => b.rect.x)).toEqual([0, 0.5]);
    expect(page?.auto).toBe(true);
    expect(page?.shape.kind === "page" ? page.shape.side : undefined).toBe("right");
    expect(page?.shape.boxes.map((b) => b.role)).toEqual(["hero", "support"]);
    expect(page && page.updatedAt - page.createdAt).toBe(3600);
  });

  it("points every problem at the boxes it names", () => {
    expect(fixture.problems.map((p) => [p.kind, problemSlots(p)])).toEqual([
      ["noBoxes", []],
      ["tooManyBoxes", []],
      ["notFinite", [0]],
      ["offCanvas", [1]],
      ["tooSmall", [2]],
      ["crossesFold", [0]],
      ["overlap", [0, 1]],
      ["edgeInBleed", [1]],
      ["edgeInGutter", [0]],
      ["mostlyHidden", [3]],
    ]);
  });

  it("carries the print size each print rule was measured against", () => {
    const bleed = fixture.problems.find((p) => p.kind === "edgeInBleed");
    const gutter = fixture.problems.find((p) => p.kind === "edgeInGutter");
    expect(bleed?.kind === "edgeInBleed" ? [bleed.edge, bleed.bleedMm] : undefined).toEqual(["top", 5]);
    expect(gutter?.kind === "edgeInGutter" ? gutter.gutterMm : undefined).toBe(5);
    expect(bleed?.message).toContain("5 mm bleed");
  });

  it("lists a refused layout by id with Rust's reasons", () => {
    expect(fixture.refused.templateId).toBe(fixture.layouts[0]?.id);
    expect(fixture.refused.problems.map((p) => problemSlots(p))).toEqual([[0]]);
    expect(fixture.refused.problems[0]?.message).toMatch(/^Box 1's left edge/);
  });

  it("words every store rejection", () => {
    expect(fixture.errors.map(customLayoutErrorText)).toEqual([
      "That layout no longer exists.",
      "A layout needs a name.",
      "A layout name can be at most 60 characters.",
      "A layout needs at least one photo box.",
      "Could not save layouts: disk I/O error",
    ]);
  });

  it("joins several problems of an invalid save into one line", () => {
    const [first, second] = fixture.problems;
    if (!first || !second) throw new Error("fixture lost its problems");
    expect(customLayoutErrorText({ kind: "invalid", problems: [first, second] })).toBe(
      `${first.message} ${second.message}`,
    );
  });
});
