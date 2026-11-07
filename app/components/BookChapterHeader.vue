<script setup lang="ts">
import { dateRange } from "~/types/dates";
import { titleEdit, type BookEdit, type PreviewChapter } from "~/types/preview";

const { chapter, busy = false } = defineProps<{
  chapter: PreviewChapter;
  busy?: boolean;
}>();

const emit = defineEmits<{
  /** A `setChapterTitle` edit, sent like every other edit on the desk. */
  edit: [edit: BookEdit];
}>();

/** Mirrors `book::titles::MAX_TITLE_CHARS`; Rust refuses anything longer. */
const MAX_TITLE_CHARS = 60;

const editing = ref(false);
const draft = ref("");

function startEditing() {
  draft.value = chapter.title;
  editing.value = true;
}

/** Enter and blur both save. Esc has already left editing, so its blur does not. */
function commit() {
  if (!editing.value) return;
  editing.value = false;
  const edit = titleEdit(chapter, draft.value);
  if (edit) emit("edit", edit);
}

function cancel() {
  editing.value = false;
}

function useAutomatic() {
  emit("edit", { kind: "setChapterTitle", event: chapter.event, title: null });
}

const dates = computed(() =>
  chapter.start !== null && chapter.end !== null ? dateRange(chapter.start, chapter.end) : null,
);
const pages = computed(() =>
  chapter.firstPage === chapter.lastPage
    ? `Page ${chapter.firstPage}`
    : `Pages ${chapter.firstPage}–${chapter.lastPage}`,
);
</script>

<template>
  <header class="flex flex-col items-start gap-1.5 border-t border-default pt-5">
    <p class="flex flex-wrap items-center gap-x-2 text-sm text-muted tabular-nums">
      <span class="font-medium text-toned">Chapter {{ chapter.number }}</span>
      <span aria-hidden="true">·</span>
      <span>{{ pages }}</span>
      <template v-if="dates && dates !== chapter.title">
        <span aria-hidden="true">·</span>
        <span>{{ dates }}</span>
      </template>
      <template v-if="chapter.events.length > 1">
        <span aria-hidden="true">·</span>
        <span>{{ chapter.events.length }} events</span>
      </template>
    </p>

    <div class="flex min-h-9 flex-wrap items-center gap-x-2 gap-y-1">
      <UInput
        v-if="editing"
        v-model="draft"
        autofocus
        size="lg"
        :maxlength="MAX_TITLE_CHARS"
        :placeholder="chapter.autoTitle"
        aria-label="Chapter title"
        class="w-96"
        @keydown.enter.prevent="commit"
        @keydown.esc.stop.prevent="cancel"
        @blur="commit"
      />
      <template v-else>
        <h3
          class="text-lg font-semibold text-highlighted"
          :class="chapter.title === '' && 'font-normal text-muted'"
        >
          {{ chapter.title === "" ? "No title" : chapter.title }}
        </h3>
        <UTooltip text="Edit the title this chapter prints">
          <UButton
            icon="i-lucide-pencil"
            color="neutral"
            variant="ghost"
            :disabled="busy"
            :aria-label="`Edit the title of chapter ${chapter.number}`"
            @click="startEditing"
          />
        </UTooltip>
        <UButton
          v-if="chapter.custom"
          color="neutral"
          variant="link"
          :disabled="busy"
          @click="useAutomatic"
        >
          Use automatic title
        </UButton>
      </template>
    </div>

    <p v-if="editing" class="text-sm text-muted">
      Enter saves, Esc cancels. Leave it empty to print no title.
    </p>
    <p
      v-else-if="chapter.titleBox?.overlapsPhoto"
      class="flex items-start gap-1.5 text-sm text-warning"
    >
      <UIcon name="i-lucide-triangle-alert" class="mt-0.5 size-4 shrink-0" />
      Prints over a photo on page {{ chapter.titleBox.page }}: this layout has no clear band for
      it. Choose another layout, or move a box.
    </p>
    <p v-else-if="chapter.titleBox" class="text-sm text-muted">
      Prints on page {{ chapter.titleBox.page }}
    </p>
  </header>
</template>
