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
  /** Extra header content, after the count -- see `EventTierControl` in `SelectPhotos.vue`. */
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
 * Scrolls so `event`'s header lands at the top -- the events panel's row
 * click. `scrollToIndex({ align: "start" })` alone put the header UNDER the
 * sticky toolbar above this sheet (`stickyTop`'s own height, 44px in `SelectPhotos`): the
 * virtualizer knows nothing about that toolbar, only about this sheet's own
 * content. `getOffsetForIndex` gives the same target offset `scrollToIndex`
 * would use, and subtracting `stickyTop` from it (then scrolling there
 * directly) leaves that much room above the header for the toolbar to sit
 * in without covering it. `scrollToOffset` clamps a negative result to 0 on
 * its own, so an event near the very top is not a special case here.
 */
function revealEvent(event: number) {
  const index = rows.value.findIndex((row) => row.kind === "event" && row.event === event);
  if (index === -1) return;
  const target = virtualizer.value.getOffsetForIndex(index, "start");
  if (!target) return;
  virtualizer.value.scrollToOffset(target[0] - stickyTop, { align: "start" });
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
          class="-mx-6 flex h-9 items-baseline gap-2 bg-default/95 px-6 py-2 text-sm backdrop-blur"
        >
          <!--
            The slot's content (the tier control, a11y-wise M5) must NOT be a
            descendant of the h2: an h2's accessible name is built from every
            text descendant, including a slotted button's own label, which
            made the heading announce "Kyoto 5 photos Featured Normal Brief
            Skip" instead of just its title. Keeping it a sibling in the same
            flex row leaves the layout unchanged while the heading's name
            stays just the title and count.

            The slot's own root nodes sit directly in this row rather than
            inside a wrapping span: a wrapping `<span class="min-w-0">` here
            (fix2) let the OUTER row's shrink pass squeeze the whole
            note+control cluster below what the control alone needs, before
            the control's own `shrink-0` ever got a say -- overflow at
            1100px with a long title (fix3). With title, note and control as
            flat siblings of one row, `h2`'s own (default) shrink and the
            note's much higher one below resolve who gives way first, and
            `shrink-0` on the control is honoured at the level that actually
            distributes the row's space, not two flex contexts removed from
            it. `h2` grows to absorb any slack, keeping the note+control
            cluster flush right when the title is short, as it was before.
          -->
          <h2 class="flex min-w-0 grow items-baseline gap-2 font-semibold text-highlighted">
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
