import type { PlaceNames } from "~/types/book";

/**
 * The contact sheet as a list of rows, so it can be virtualized: only the
 * rows on screen are rendered. A folder of thousands of photos otherwise
 * mounts thousands of tiles every time its draft is opened, which took
 * seconds and blocked the switch between screens.
 */

export interface SheetEventRow {
  kind: "event";
  key: string;
  /** The chapter's town, or "Event N" by its 1-based position among the sheet's events. */
  title: string;
  count: number;
  /** The cluster id this header is for -- see `EventRow.event` in `~/types/book`. */
  event: number;
}

export interface SheetTileRow<T> {
  kind: "tiles";
  key: string;
  photos: T[];
  /** The last row of its event, or of the sheet, which is spaced off from what follows. */
  last: boolean;
}

export type SheetRow<T> = SheetEventRow | SheetTileRow<T>;

/**
 * How many columns the sheet has, matching CSS
 * `repeat(auto-fill, minmax(minTile, 1fr))` with a `gap` between columns.
 */
export function sheetColumns(width: number, minTile: number, gap: number): number {
  return Math.max(1, Math.floor((width + gap) / (minTile + gap)));
}

/** Photos cut into rows of `columns`, in order. */
export function tileRows<T>(photos: T[], columns: number, keyPrefix = "row"): SheetTileRow<T>[] {
  const rows: SheetTileRow<T>[] = [];
  for (let start = 0; start < photos.length; start += columns) {
    rows.push({
      kind: "tiles",
      key: `${keyPrefix}-${start / columns}`,
      photos: photos.slice(start, start + columns),
      last: start + columns >= photos.length,
    });
  }
  return rows;
}

/** Each event's header followed by its photos' rows. `names` titles chapters by `eventCluster`. */
export function sheetRows<T>(
  groups: { eventCluster: number; photos: T[] }[],
  columns: number,
  names: PlaceNames = {},
): SheetRow<T>[] {
  return groups.flatMap((group, index) => [
    {
      kind: "event" as const,
      key: `event-${group.eventCluster}`,
      title: names[group.eventCluster] ?? `Event ${index + 1}`,
      count: group.photos.length,
      event: group.eventCluster,
    },
    ...tileRows(group.photos, columns, `event-${group.eventCluster}`),
  ]);
}

/** The index of the header over the row at `top`: the nearest event row at or above it. */
export function activeEventRow<T>(rows: SheetRow<T>[], top: number): number | undefined {
  for (let index = Math.min(top, rows.length - 1); index >= 0; index -= 1) {
    if (rows[index]?.kind === "event") return index;
  }
  return undefined;
}

/**
 * The row whose span covers `offset`: the last row that starts at or before
 * it. `starts` are the rows' top edges in ascending order; an offset above the
 * first row answers the first row, and one past the end the last.
 */
export function rowIndexAtOffset(starts: readonly number[], offset: number): number | undefined {
  if (starts.length === 0) return undefined;
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    // Rounded up, so `low = middle` always makes progress.
    const middle = Math.ceil((low + high) / 2);
    if ((starts[middle] ?? 0) <= offset) low = middle;
    else high = middle - 1;
  }
  return low;
}

/**
 * The header that should be pinned when `offset` is the first line visible
 * below whatever sits above the sheet: the header of the event the row there
 * belongs to. A header counts from its own top edge, so it takes over the
 * moment it reaches the offset, not a pixel later.
 */
export function pinnedEventRow<T>(
  rows: SheetRow<T>[],
  starts: readonly number[],
  offset: number,
): number | undefined {
  const index = rowIndexAtOffset(starts, offset);
  return index === undefined ? undefined : activeEventRow(rows, index);
}
