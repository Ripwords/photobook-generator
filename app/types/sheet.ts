/**
 * The contact sheet as a list of rows, so it can be virtualized: only the
 * rows on screen are rendered. A folder of thousands of photos otherwise
 * mounts thousands of tiles every time its draft is opened, which took
 * seconds and blocked the switch between screens.
 */

export interface SheetEventRow {
  kind: "event";
  key: string;
  /** 1-based position among the sheet's events, which is how they are labelled. */
  ordinal: number;
  count: number;
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

/** Each event's header followed by its photos' rows. */
export function sheetRows<T>(
  groups: { eventCluster: number; photos: T[] }[],
  columns: number,
): SheetRow<T>[] {
  return groups.flatMap((group, index) => [
    {
      kind: "event" as const,
      key: `event-${group.eventCluster}`,
      ordinal: index + 1,
      count: group.photos.length,
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
