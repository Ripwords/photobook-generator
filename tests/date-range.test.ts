import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { dateRange } from "../app/types/dates";

/**
 * The same cases `titles_date_range_matches_the_shared_wire_fixture` pins in
 * Rust, so the contact sheet's chapter headers and the printed titles cannot
 * drift apart.
 */
const cases: [number, number, string][] = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/date-ranges.json", import.meta.url)), "utf8"),
);

describe("dateRange", () => {
  it("formats every case the Rust formatter is pinned to", () => {
    expect(cases.length).toBeGreaterThanOrEqual(6);
    for (const [start, end, want] of cases) expect(dateRange(start, end), `${start}..${end}`).toBe(want);
  });
});
