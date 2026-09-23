import { describe, expect, it } from "vitest";
import {
  activeEventRow,
  pinnedEventRow,
  rowIndexAtOffset,
  sheetColumns,
  sheetRows,
  tileRows,
} from "~/types/sheet";

const photo = (n: number) => ({ path: `/p/${n}.jpg` });
const photos = (from: number, count: number) =>
  Array.from({ length: count }, (_, i) => photo(from + i));

describe("sheetColumns", () => {
  it("fits as many columns as CSS auto-fill would", () => {
    // auto-fill minmax(160px, 1fr) with a 12px gap: n columns need
    // n * 160 + (n - 1) * 12 pixels.
    expect(sheetColumns(160 * 4 + 12 * 3, 160, 12)).toBe(4);
    expect(sheetColumns(160 * 4 + 12 * 3 - 1, 160, 12)).toBe(3);
    expect(sheetColumns(160 * 5 + 12 * 4, 160, 12)).toBe(5);
  });

  it("never answers fewer than one column", () => {
    expect(sheetColumns(0, 160, 12)).toBe(1);
    expect(sheetColumns(90, 160, 12)).toBe(1);
  });
});

describe("tileRows", () => {
  it("cuts photos into rows of the column count, keeping their order", () => {
    const rows = tileRows(photos(0, 7), 3);
    expect(rows.map((row) => row.photos.map((p) => p.path))).toEqual([
      ["/p/0.jpg", "/p/1.jpg", "/p/2.jpg"],
      ["/p/3.jpg", "/p/4.jpg", "/p/5.jpg"],
      ["/p/6.jpg"],
    ]);
  });

  it("marks only the last row as last", () => {
    expect(tileRows(photos(0, 7), 3).map((row) => row.last)).toEqual([false, false, true]);
  });

  it("gives every row a distinct key", () => {
    const keys = tileRows(photos(0, 10), 3).map((row) => row.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("has no rows for no photos", () => {
    expect(tileRows([], 4)).toEqual([]);
  });
});

describe("sheetRows", () => {
  const groups = [
    { eventCluster: 4, photos: photos(0, 5) },
    { eventCluster: 9, photos: photos(5, 2) },
  ];

  it("puts each event's header before its own rows", () => {
    const rows = sheetRows(groups, 2);
    expect(
      rows.map((row) => (row.kind === "event" ? row.title : row.photos.length)),
    ).toEqual(["Event 1", 2, 2, 1, "Event 2", 2]);
  });

  it("numbers events in order and counts their photos", () => {
    const headers = sheetRows(groups, 2).filter((row) => row.kind === "event");
    expect(headers.map((row) => [row.title, row.count])).toEqual([
      ["Event 1", 5],
      ["Event 2", 2],
    ]);
  });

  it("titles a chapter with its town by chapter id, and numbers the rest by position", () => {
    const three = [...groups, { eventCluster: 12, photos: photos(7, 1) }];
    const headers = sheetRows(three, 2, { 9: "Kyoto", 1: "Osaka", 2: "Nara" }).filter(
      (row) => row.kind === "event",
    );
    expect(headers.map((row) => row.title)).toEqual(["Event 1", "Kyoto", "Event 3"]);
  });

  it("ends each event with its own last row, not only the sheet's", () => {
    const lasts = sheetRows(groups, 2)
      .filter((row) => row.kind === "tiles")
      .map((row) => row.last);
    expect(lasts).toEqual([false, false, true, true]);
  });

  it("keys rows by event, so the same row index in two events differs", () => {
    const keys = sheetRows(groups, 2).map((row) => row.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("carries the cluster id on each header, for the header slot", () => {
    const headers = sheetRows(groups, 2).filter((row) => row.kind === "event");
    // The cluster ids (4, 9), not the 1-based display position (1, 2) --
    // a consumer needs the id to look up tier state and place a callback.
    expect(headers.map((row) => row.event)).toEqual([4, 9]);
  });
});

describe("activeEventRow", () => {
  const rows = sheetRows(
    [
      { eventCluster: 1, photos: photos(0, 4) },
      { eventCluster: 2, photos: photos(4, 4) },
    ],
    2,
  );
  // [event 1, tiles, tiles, event 2, tiles, tiles]

  it("is the header of the event the top row belongs to", () => {
    expect(activeEventRow(rows, 0)).toBe(0);
    expect(activeEventRow(rows, 2)).toBe(0);
    expect(activeEventRow(rows, 3)).toBe(3);
    expect(activeEventRow(rows, 5)).toBe(3);
  });

  it("is undefined when no header precedes the top row", () => {
    expect(activeEventRow(tileRows(photos(0, 4), 2), 1)).toBeUndefined();
  });
});

describe("rowIndexAtOffset", () => {
  // Rows 0..3 spanning [0, 48), [48, 220), [220, 268), [268, 440).
  const starts = [0, 48, 220, 268];

  it("is the row whose span covers the offset, a row's own start included", () => {
    expect(rowIndexAtOffset(starts, 0)).toBe(0);
    expect(rowIndexAtOffset(starts, 47)).toBe(0);
    expect(rowIndexAtOffset(starts, 48)).toBe(1);
    expect(rowIndexAtOffset(starts, 219)).toBe(1);
    expect(rowIndexAtOffset(starts, 220)).toBe(2);
    expect(rowIndexAtOffset(starts, 267.5)).toBe(2);
    expect(rowIndexAtOffset(starts, 268)).toBe(3);
  });

  it("is the first row above the sheet and the last row past its end", () => {
    expect(rowIndexAtOffset(starts, -30)).toBe(0);
    expect(rowIndexAtOffset(starts, 10_000)).toBe(3);
  });

  it("is undefined for a sheet with no rows", () => {
    expect(rowIndexAtOffset([], 0)).toBeUndefined();
  });
});

describe("pinnedEventRow", () => {
  const rows = sheetRows(
    [
      { eventCluster: 1, photos: photos(0, 4) },
      { eventCluster: 2, photos: photos(4, 4) },
    ],
    2,
  );
  // [event 1, tiles, tiles, event 2, tiles, tiles]; a header is 48px and a
  // tile row 172px, the last of an event 192px (its event gap).
  const starts = [0, 48, 220, 412, 460, 632];

  it("switches to an event exactly when its header reaches the offset, not a pixel before", () => {
    expect(pinnedEventRow(rows, starts, 411)).toBe(0);
    expect(pinnedEventRow(rows, starts, 411.5)).toBe(0);
    expect(pinnedEventRow(rows, starts, 412)).toBe(3);
  });

  it("stays on an event through all of its tile rows", () => {
    expect(pinnedEventRow(rows, starts, 0)).toBe(0);
    expect(pinnedEventRow(rows, starts, 300)).toBe(0);
    expect(pinnedEventRow(rows, starts, 700)).toBe(3);
  });

  it("is undefined when no header precedes the offset's row", () => {
    expect(pinnedEventRow(tileRows(photos(0, 4), 2), [0, 172], 200)).toBeUndefined();
    expect(pinnedEventRow([], [], 0)).toBeUndefined();
  });
});
