<script setup lang="ts" generic="T extends { path: string }">
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/vue-virtual";
import type { PlaceNames } from "~/types/book";
import { pinnedEventRow, sheetColumns, sheetRows, tileRows } from "~/types/sheet";

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

defineSlots<{
  tile(props: { photo: T }): unknown;
  /**
   * Extra header content, after the count -- see `EventTierControl` in `SelectPhotos.vue`.
   * Place its root nodes with `col-start-2` (a note that gives way first) and
   * `col-start-3` (a control that never shrinks); see the header row's grid.
   */
  header(props: { event: number }): unknown;
}>();

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

/** Every row's top edge, from the sheet's own top -- see `pinnedHeader`. */
const rowStarts = computed(() => {
  let next = 0;
  return rows.value.map((_, index) => {
    const start = next;
    next += rowHeight(index);
    return start;
  });
});

/**
 * The scroller's `scrollTop`, kept here because the virtualizer only reports
 * a scroll when its rendered range changes -- not when a header crosses the
 * toolbar, which is what `pinnedHeader` follows.
 */
const scrollTop = ref(0);
watch(
  () => scrollElement,
  (el, _, onCleanup) => {
    if (!el) return;
    const read = () => (scrollTop.value = el.scrollTop);
    read();
    el.addEventListener("scroll", read, { passive: true });
    onCleanup(() => el.removeEventListener("scroll", read));
  },
  { immediate: true },
);

/**
 * The header of the event whose row is the first one visible BELOW the
 * sticky toolbar. `range.startIndex` is the wrong row for this: it is the row
 * at the scroller's very top, which is hidden behind the toolbar, so the
 * previous event's header stayed pinned for `stickyTop` more pixels -- and
 * after `revealEvent`, which parks a header exactly at the toolbar's bottom
 * edge, it sat on top of the one just revealed.
 *
 * The arithmetic, all in the scroller's content coordinates: the line just
 * under the toolbar is `scrollTop + stickyTop`, and the sheet starts
 * `scrollMargin` into the scroller, so that line is
 * `scrollTop + stickyTop - scrollMargin` into the sheet. `rowStarts` are from
 * the sheet's own top, the same as the virtualizer's `item.start` minus
 * `scrollMargin`: both are the running sum of `rowHeight`, which the
 * virtualizer takes as each row's exact size (nothing is measured). A header
 * whose start equals that line is the pinned one -- the position
 * `revealEvent` scrolls it to.
 */
const pinnedHeader = computed(() =>
  pinnedEventRow(rows.value, rowStarts.value, scrollTop.value + stickyTop - scrollMargin.value),
);

const virtualizer = useVirtualizer(
  computed(() => {
    // Read here, not inside `rangeExtractor`: the virtualizer re-runs its
    // extractor only when its range or its options change, so a new pinned
    // header has to arrive as new options or it may never be rendered.
    const pinned = pinnedHeader.value;
    return {
      count: rows.value.length,
      getScrollElement: () => scrollElement,
      estimateSize: rowHeight,
      getItemKey: (index: number) => rows.value[index]?.key ?? index,
      overscan: 4,
      scrollMargin: scrollMargin.value,
      // The pinned header stays rendered, so it can stick however far down
      // its event the user has scrolled.
      rangeExtractor: (range: Range) => {
        const indexes = defaultRangeExtractor(range);
        return pinned === undefined || indexes.includes(pinned) ? indexes : [pinned, ...indexes];
      },
    };
  }),
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

/**
 * Scrolls so `event`'s header lands just under the sticky toolbar -- the
 * events panel's row click. `scrollToIndex({ align: "start" })` alone put
 * the header UNDER that toolbar (`stickyTop`, 44px in `SelectPhotos`): the
 * virtualizer knows nothing about it. So the target is the header's own
 * start in the scroller (`rowStarts` + `scrollMargin`, the same numbers
 * `pinnedHeader` compares against) minus `stickyTop`.
 *
 * `scrollToOffset` clamps that to [0, max scroll] once. The subtraction
 * must come before the clamp: `getOffsetForIndex` clamps to the max scroll
 * itself, so subtracting `stickyTop` after it stopped a late event 44px
 * short of the end of the sheet (round 5). An event near the end of the
 * sheet whose header cannot reach the toolbar still scrolls as far as the
 * sheet goes, with its header visible lower down.
 */
function revealEvent(event: number) {
  const index = rows.value.findIndex((row) => row.kind === "event" && row.event === event);
  const start = rowStarts.value[index];
  if (start === undefined) return;
  virtualizer.value.scrollToOffset(start + scrollMargin.value - stickyTop, { align: "start" });
}
defineExpose({ revealEvent });
</script>

<template>
  <div ref="root" class="relative" :style="{ height: `${virtualizer.getTotalSize()}px` }">
    <!--
      `row.key` is the value `getItemKey` handed the virtualizer for this index.
      It is read from the row because `item.key`'s type also admits a `bigint`,
      which a `:key` cannot take.
    -->
    <template v-for="{ item, row, offset } in visible" :key="row.key">
      <div
        v-if="row.kind === 'event'"
        :style="
          item.index === pinnedHeader
            ? { position: 'sticky', top: `${stickyTop}px`, zIndex: 10 }
            : { position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${offset}px)` }
        "
      >
        <div
          class="-mx-6 grid h-9 grid-cols-[minmax(0,max-content)_minmax(0,1fr)_max-content] items-baseline bg-default/95 px-6 py-2 text-sm backdrop-blur"
        >
          <!--
            The slot's content (the tier control, a11y-wise M5) must NOT be a
            descendant of the h2: an h2's accessible name is built from every
            text descendant, including a slotted button's own label, which
            made the heading announce "Kyoto 5 photos Featured Normal Brief
            Skip" instead of just its title. It sits beside the h2 as a
            direct child of this row instead.

            A grid, not a flex row, so the title is truncated only after the
            note is gone (round 5). Flex shrink is proportional: however high
            the note's shrink factor, the title still gave up a fraction of
            a pixel, enough to turn "Osaka" into "Osa…" beside a readable
            note. Grid track sizing is ordered instead. Column 3
            (`max-content`) holds the tier control and is never squeezed.
            Column 1 (`minmax(0, max-content)`) holds the title and grows to
            its full width before any `fr` track gets space. Column 2
            (`minmax(0, 1fr)`) holds the note and takes only what is left,
            down to 0. With a short title, column 2 also absorbs the slack,
            which keeps the control flush right. A slotted note goes in
            `col-start-2` and a control in `col-start-3`. There is no column
            gap, because a gap stays even when the note column is empty and
            took 5px from a note-less title; spacing is on the items (`ml-2`)
            instead.
          -->
          <h2 class="flex min-w-0 items-baseline gap-2 font-semibold text-highlighted">
            <span class="min-w-0 truncate">{{ row.title }}</span>
            <span class="shrink-0 font-normal text-muted tabular-nums"
              >{{ row.count }} {{ row.count === 1 ? "photo" : "photos" }}</span
            >
          </h2>
          <slot name="header" :event="row.event" />
        </div>
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
