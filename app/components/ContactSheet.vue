<script setup lang="ts" generic="T extends { path: string }">
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/vue-virtual";
import type { PlaceNames } from "~/types/book";
import { activeEventRow, sheetColumns, sheetRows, tileRows } from "~/types/sheet";

/**
 * A grid of photos, optionally in event chapters, that renders only the rows
 * in view. A draft of thousands of photos mounts a screenful of tiles rather
 * than all of them, so opening and leaving it stays quick.
 *
 * Row heights are computed, not measured: a tile is square and as wide as
 * its column, and a header is fixed, so every row's height is known before
 * it renders.
 */
const {
  groups,
  tileSize,
  scrollElement,
  headers = true,
  stickyTop = 0,
  names = {},
} = defineProps<{
  groups: { eventCluster: number; photos: T[] }[];
  /** The smallest a tile may be, in CSS pixels. */
  tileSize: number;
  /** The element that scrolls the sheet. */
  scrollElement: HTMLElement | null;
  /** Whether each event gets a numbered header. */
  headers?: boolean;
  /** How far below the scroller's top a header sticks, clearing anything pinned above it. */
  stickyTop?: number;
  /** Town names by `eventCluster`; a chapter without one is numbered. */
  names?: PlaceNames;
}>();

defineSlots<{ tile(props: { photo: T }): unknown }>();

const GAP = 12;
/** Between one event's last row and the next event's header. */
const EVENT_GAP = 32;
/** A header's own height (`h-9`) plus the gap under it. */
const HEADER_ROW = 36 + GAP;

const root = useTemplateRef("root");
const width = ref(0);
/** Where the sheet starts inside the scroller, which the virtualizer's offsets count from. */
const scrollMargin = ref(0);

function measure() {
  const el = root.value;
  if (!el) return;
  width.value = el.clientWidth;
  if (scrollElement) {
    scrollMargin.value =
      el.getBoundingClientRect().top - scrollElement.getBoundingClientRect().top + scrollElement.scrollTop;
  }
}

let observer: ResizeObserver | undefined;
onMounted(() => {
  measure();
  observer = new ResizeObserver(measure);
  // The parent too: something appearing above the sheet moves where it starts.
  for (const el of [root.value, root.value?.parentElement]) if (el) observer.observe(el);
});
onBeforeUnmount(() => observer?.disconnect());

const columns = computed(() => sheetColumns(width.value, tileSize, GAP));
const tileWidth = computed(() => (width.value - GAP * (columns.value - 1)) / columns.value);
const rows = computed(() =>
  headers
    ? sheetRows(groups, columns.value, names)
    : groups.flatMap((group) => tileRows(group.photos, columns.value, `group-${group.eventCluster}`)),
);

function rowGap(index: number): number {
  const row = rows.value[index];
  if (row?.kind !== "tiles") return 0;
  if (index === rows.value.length - 1) return 0;
  return row.last ? EVENT_GAP : GAP;
}

function rowHeight(index: number): number {
  return rows.value[index]?.kind === "event" ? HEADER_ROW : tileWidth.value + rowGap(index);
}

const virtualizer = useVirtualizer(
  computed(() => ({
    count: rows.value.length,
    getScrollElement: () => scrollElement,
    estimateSize: rowHeight,
    getItemKey: (index: number) => rows.value[index]?.key ?? index,
    overscan: 4,
    scrollMargin: scrollMargin.value,
    // The header of the event at the top of the view stays rendered, so it
    // can stick however far down its event the user has scrolled.
    rangeExtractor: (range: Range) => {
      const indexes = defaultRangeExtractor(range);
      const active = activeEventRow(rows.value, range.startIndex);
      return active === undefined || indexes.includes(active) ? indexes : [active, ...indexes];
    },
  })),
);

const activeHeader = computed(() =>
  activeEventRow(rows.value, virtualizer.value.range?.startIndex ?? 0),
);

const visible = computed(() =>
  virtualizer.value.getVirtualItems().flatMap((item) => {
    const row = rows.value[item.index];
    return row ? [{ item, row, offset: item.start - scrollMargin.value }] : [];
  }),
);

// Sizes come from `rowHeight`, which the virtualizer caches; a new width or
// tile size changes them without changing any row's key.
watch([tileWidth, rows], () => virtualizer.value.measure());
</script>

<template>
  <div ref="root" class="relative" :style="{ height: `${virtualizer.getTotalSize()}px` }">
    <template v-for="{ item, row, offset } in visible" :key="item.key">
      <div
        v-if="row.kind === 'event'"
        :style="
          item.index === activeHeader
            ? { position: 'sticky', top: `${stickyTop}px`, zIndex: 10 }
            : { position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${offset}px)` }
        "
      >
        <h2
          class="-mx-6 flex h-9 items-baseline gap-2 bg-default/95 px-6 py-2 text-sm font-semibold text-highlighted backdrop-blur"
        >
          <span class="min-w-0 truncate">{{ row.title }}</span>
          <span class="shrink-0 font-normal text-muted tabular-nums"
            >{{ row.count }} {{ row.count === 1 ? "photo" : "photos" }}</span
          >
        </h2>
      </div>
      <div
        v-else
        class="absolute top-0 left-0 grid w-full gap-3"
        :style="{
          gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
          transform: `translateY(${offset}px)`,
        }"
      >
        <template v-for="photo in row.photos" :key="photo.path">
          <slot name="tile" :photo />
        </template>
      </div>
    </template>
  </div>
</template>
