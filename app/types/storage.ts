/**
 * Mirrors `commands::CacheStatus`, pinned by `tests/fixtures/wire/cache-status.json`.
 * `pinnedBytes` is what saved books, books in the trash and open drafts use:
 * eviction never touches it, so it can exceed the limit.
 */
export interface CacheStatus {
  usedBytes: number;
  pinnedBytes: number;
  limitBytes: number;
  overBudget: boolean;
}

const KiB = 1024;
const MiB = 1024 * KiB;
const GiB = 1024 * MiB;

/** The limits Settings offers. The same binary units `formatBytes` shows. */
export const CACHE_LIMITS: readonly number[] = [500 * MiB, GiB, 2 * GiB, 5 * GiB, 10 * GiB];

/** `cache::DEFAULT_LIMIT` on the Rust side. */
export const DEFAULT_CACHE_LIMIT = 2 * GiB;

export function formatBytes(bytes: number): string {
  if (bytes >= GiB) return `${Number((bytes / GiB).toFixed(1))} GB`;
  if (bytes >= MiB) return `${Math.round(bytes / MiB)} MB`;
  return `${Math.round(bytes / KiB)} KB`;
}

export interface LimitItem {
  value: number;
  label: string;
}

/** The presets, plus the stored limit when it is not one of them, so the select can show it. */
export function limitItems(current: number): LimitItem[] {
  const values = CACHE_LIMITS.includes(current) ? [...CACHE_LIMITS] : [...CACHE_LIMITS, current];
  return values.toSorted((a, b) => a - b).map((value) => ({ value, label: formatBytes(value) }));
}

export interface StorageSummary {
  used: string;
  limit: string;
  /** Share of the limit in use, capped at 1 for the meter. */
  fraction: number;
  warning: string | null;
}

export function storageSummary(status: CacheStatus): StorageSummary {
  return {
    used: formatBytes(status.usedBytes),
    limit: formatBytes(status.limitBytes),
    fraction: status.limitBytes > 0 ? Math.min(1, status.usedBytes / status.limitBytes) : 1,
    warning: status.overBudget
      ? `Your saved books and drafts use ${formatBytes(status.pinnedBytes)}, more than the limit. Nothing they use is removed.`
      : null,
  };
}
