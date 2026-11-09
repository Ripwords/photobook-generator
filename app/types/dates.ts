const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * "12 Mar 2026", "12–14 Mar 2026", "28 Mar – 2 Apr 2026" or
 * "30 Dec 2025 – 2 Jan 2026", from wall-clock seconds. Mirrors Rust's
 * `book::chapter_pages::date_range`; `tests/fixtures/wire/date-ranges.json` pins both.
 *
 * Capture times are the camera's local wall clock stored as if it were UTC,
 * so this reads the UTC fields: the machine's own zone must not shift a day.
 */
export function dateRange(start: number, end: number): string {
  const a = new Date(Math.min(start, end) * 1000);
  const b = new Date(Math.max(start, end) * 1000);
  const [y0, m0, d0] = [a.getUTCFullYear(), a.getUTCMonth(), a.getUTCDate()];
  const [y1, m1, d1] = [b.getUTCFullYear(), b.getUTCMonth(), b.getUTCDate()];
  const [n0, n1] = [MONTHS[m0], MONTHS[m1]];
  if (y0 === y1 && m0 === m1 && d0 === d1) return `${d0} ${n0} ${y0}`;
  if (y0 === y1 && m0 === m1) return `${d0}–${d1} ${n0} ${y0}`;
  if (y0 === y1) return `${d0} ${n0} – ${d1} ${n1} ${y0}`;
  return `${d0} ${n0} ${y0} – ${d1} ${n1} ${y1}`;
}
