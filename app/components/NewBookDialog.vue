<script setup lang="ts">
import { defaultProjectName } from "~/types/book";
import { pickFolders } from "~/composables/useAnalysisJobs";

/**
 * Starts a book: its name first, then the folders its photos come from.
 * Starting hands both to the caller, which begins the analysis and opens the
 * draft; nothing is analysed while this is open.
 */
const open = defineModel<boolean>("open", { required: true });

const emit = defineEmits<{
  start: [book: { name: string; folders: string[] }];
}>();

const name = ref("");
const folders = ref<string[]>([]);
const picking = ref(false);
/** Whether this opening ended in Start rather than a cancel. */
const started = ref(false);

watch(open, (isOpen) => {
  if (!isOpen) return;
  name.value = "";
  folders.value = [];
  started.value = false;
});

/**
 * After Start the screen changes to the new draft, so focus is not handed
 * back to whatever opened the dialog. Handing it back left that button's
 * tooltip stuck open over the draft. A cancel still returns focus.
 */
function onCloseAutoFocus(event: Event) {
  if (started.value) event.preventDefault();
}

/** What a blank name becomes, shown so leaving it blank is a choice rather than a gap. */
const placeholder = computed(() =>
  folders.value[0] ? defaultProjectName(folders.value[0]) : "Untitled photobook",
);

async function choose() {
  picking.value = true;
  try {
    const picked = await pickFolders();
    folders.value = [...new Set([...folders.value, ...picked])];
  } finally {
    picking.value = false;
  }
}

function removeFolder(folder: string) {
  folders.value = folders.value.filter((other) => other !== folder);
}

function start() {
  if (folders.value.length === 0) return;
  started.value = true;
  emit("start", { name: name.value, folders: [...folders.value] });
  open.value = false;
}
</script>

<template>
  <UModal
    v-model:open="open"
    title="New photobook"
    description="Name it, then choose the folders its photos come from."
    :ui="{ footer: 'justify-end' }"
    :content="{ onCloseAutoFocus }"
  >
    <template #body>
      <form id="new-book" class="space-y-5" @submit.prevent="start">
        <UFormField label="Name" name="name">
          <UInput
            v-model="name"
            autofocus
            :placeholder
            aria-label="Photobook name"
            class="w-full"
          />
        </UFormField>

        <UFormField
          label="Photo folders"
          help="Every photo in them and their subfolders is analysed on this Mac."
        >
          <ul
            v-if="folders.length > 0"
            class="mb-2 divide-y divide-default rounded-md border border-default"
          >
            <li v-for="folder in folders" :key="folder" class="flex items-center gap-2.5 py-1.5 pr-1.5 pl-3">
              <UIcon name="i-lucide-folder" class="size-4 shrink-0 text-muted" />
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm text-highlighted">{{ defaultProjectName(folder) }}</p>
                <p class="truncate text-xs text-muted" :title="folder">{{ folder }}</p>
              </div>
              <UTooltip text="Remove">
                <UButton
                  icon="i-lucide-x"
                  color="neutral"
                  variant="ghost"
                  size="xs"
                  :aria-label="`Remove ${folder}`"
                  @click="removeFolder(folder)"
                />
              </UTooltip>
            </li>
          </ul>
          <UButton
            icon="i-lucide-folder-plus"
            color="neutral"
            variant="outline"
            :loading="picking"
            @click="choose"
          >
            {{ folders.length > 0 ? "Add folders…" : "Choose folders…" }}
          </UButton>
        </UFormField>
      </form>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="open = false">Cancel</UButton>
      <UButton type="submit" form="new-book" color="primary" :disabled="folders.length === 0">
        Start
      </UButton>
    </template>
  </UModal>
</template>
