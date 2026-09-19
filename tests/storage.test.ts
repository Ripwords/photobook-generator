import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  CACHE_LIMITS,
  type CacheStatus,
  DEFAULT_CACHE_LIMIT,
  formatBytes,
  limitItems,
  storageSummary,
} from "../app/types/storage";

const MiB = 1024 * 1024;
const GiB = 1024 * MiB;

function wire(): CacheStatus {
  return JSON.parse(
    readFileSync(new URL("./fixtures/wire/cache-status.json", import.meta.url), "utf8"),
  ) as CacheStatus;
}

describe("formatBytes", () => {
  it("uses whole megabytes below a gigabyte", () => {
    expect(formatBytes(98 * MiB + 400 * 1024)).toBe("98 MB");
    expect(formatBytes(500 * MiB)).toBe("500 MB");
  });

  it("uses one decimal of gigabytes from a gigabyte up, dropping a trailing .0", () => {
    expect(formatBytes(GiB)).toBe("1 GB");
    expect(formatBytes(2.5 * GiB)).toBe("2.5 GB");
    expect(formatBytes(10 * GiB)).toBe("10 GB");
  });

  it("uses kilobytes below a megabyte, and says zero plainly", () => {
    expect(formatBytes(28 * 1024)).toBe("28 KB");
    expect(formatBytes(0)).toBe("0 KB");
  });
});

describe("limitItems", () => {
  it("offers exactly the presets when the stored limit is one of them", () => {
    expect(limitItems(DEFAULT_CACHE_LIMIT)).toEqual(
      CACHE_LIMITS.map((bytes) => ({ value: bytes, label: formatBytes(bytes) })),
    );
    expect(DEFAULT_CACHE_LIMIT).toBe(2 * GiB);
  });

  it("keeps a stored limit that is not a preset selectable, in size order", () => {
    const items = limitItems(3 * GiB);
    expect(items.map((item) => item.value)).toEqual([500 * MiB, GiB, 2 * GiB, 3 * GiB, 5 * GiB, 10 * GiB]);
    expect(items[3]?.label).toBe("3 GB");
  });
});

describe("storageSummary, fed the wire fixture", () => {
  it("reads every field the Storage section shows", () => {
    const summary = storageSummary(wire());
    expect(summary.used).toBe("2.5 GB");
    expect(summary.limit).toBe("2 GB");
    expect(summary.fraction).toBe(1);
    expect(summary.warning).toBe(
      "Your saved books and drafts use 2.2 GB, more than the limit. Nothing they use is removed.",
    );
  });

  it("shows the share of the limit in use and no warning while under it", () => {
    const summary = storageSummary({
      usedBytes: 512 * MiB,
      pinnedBytes: 100 * MiB,
      limitBytes: 2 * GiB,
      overBudget: false,
    });
    expect(summary.fraction).toBe(0.25);
    expect(summary.warning).toBeNull();
  });
});
