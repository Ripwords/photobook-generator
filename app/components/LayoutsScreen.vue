<script setup lang="ts">
import type { DropdownMenuItem } from "@nuxt/ui";
import type { CustomLayout, LayoutShape } from "~/types/customLayouts";
import { copyName, nextLayoutName, shapePages } from "~/types/layoutEditor";
import type { LayoutDraft } from "~/types/navigation";
import { templateLabel } from "~/types/preview";

/**
 * The layout library: the user's own layouts, which every book can use, and
 * the built-in ones to copy from. Opening one, or "New layout", swaps the
 * list for the editor.
 */
const { draft = null } = defineProps<{
  /** Opens the editor on this at once -- a book's "New layout…". */
  draft?: LayoutDraft | null;
}>();

const emit = defineEmits<{
  /** Back to the book the draft came from, now that its layout is saved or dropped. */
  back: [projectId: number];
}>();

const { layouts, builtins, defaultSpec, refresh, create, update, rename, duplicate, remove, setAuto } =
  useCustomLayouts();
const toast = useToast();

const loaded = ref(false);
void refresh().finally(() => (loaded.value = true));

const pageAspect = computed(() => (defaultSpec.value ? defaultSpec.value.pageWIn / defaultSpec.value.pageHIn : 1.259));

/** What the editor is open on: a saved layout, or a new one not yet saved. */
type Editing = { id: string | null; name: string; shape: LayoutShape; projectId: number | null };
const editing = ref<Editing | null>(draft ? { id: null, ...draft } : null);
const saving = ref(false);

function failed(title: string, message: string) {
  toast.add({ title, description: message, icon: "i-lucide-circle-x", color: "error" });
}

function startNew(shape: LayoutShape) {
  editing.value = { id: null, name: nextLayoutName(layouts.value.map((l) => l.name)), shape, projectId: null };
}

const newItems: DropdownMenuItem[] = [
  { label: "Spread", description: "Both pages of an opening", icon: "i-lucide-book-open", onSelect: () => startNew({ kind: "spread", boxes: [] }) },
  { label: "Left page", description: "The last page, or half a spread", icon: "i-lucide-panel-left", onSelect: () => startNew({ kind: "page", side: "left", boxes: [] }) },
  { label: "Right page", description: "Page 1, or half a spread", icon: "i-lucide-panel-right", onSelect: () => startNew({ kind: "page", side: "right", boxes: [] }) },
];

function leave(projectId: number | null) {
  editing.value = null;
  if (projectId !== null) emit("back", projectId);
}

async function onSave(name: string, shape: LayoutShape) {
  const current = editing.value;
  if (!current) return;
  saving.value = true;
  try {
    if (current.id === null) {
      const made = await create(name, shape);
      if (!made.ok) return failed("The layout was not saved", made.message);
      toast.add({
        title: `Saved “${made.value.name}”`,
        description: current.projectId === null ? undefined : "It's in the layout menu of pages with the same photo count.",
        icon: "i-lucide-check",
        color: "neutral",
      });
    } else {
      const old = layouts.value.find((l) => l.id === current.id);
      const reshaped = await update(current.id, shape);
      if (!reshaped.ok) return failed("The layout was not saved", reshaped.message);
      if (old && old.name !== name) {
        const renamed = await rename(current.id, name);
        if (!renamed.ok) return failed("The layout was saved but not renamed", renamed.message);
      }
      toast.add({ title: `Saved “${name}”`, icon: "i-lucide-check", color: "neutral" });
    }
    leave(current.projectId);
  } finally {
    saving.value = false;
  }
}

function edit(layout: CustomLayout) {
  editing.value = { id: layout.id, name: layout.name, shape: layout.shape, projectId: null };
}

async function copyBuiltin(id: string, shape: LayoutShape) {
  const made = await create(copyName(id), shape);
  if (!made.ok) return failed("The copy was not made", made.message);
  edit(made.value);
}

const renaming = ref<CustomLayout | null>(null);
const renameOpen = ref(false);
const deleting = ref<CustomLayout | null>(null);
const deleteOpen = ref(false);

async function onRename(name: string) {
  const target = renaming.value;
  if (!target || name === target.name) return;
  const result = await rename(target.id, name);
  if (!result.ok) failed("The layout was not renamed", result.message);
}

async function onDelete() {
  const target = deleting.value;
  deleteOpen.value = false;
  if (!target) return;
  const result = await remove(target.id);
  if (!result.ok) return failed("The layout was not deleted", result.message);
  toast.add({ title: `Deleted “${target.name}”`, icon: "i-lucide-trash-2", color: "neutral" });
}

async function onAuto(layout: CustomLayout, auto: boolean) {
  const result = await setAuto(layout.id, auto);
  if (!result.ok) failed("That did not change", result.message);
}

async function onDuplicate(layout: CustomLayout) {
  const result = await duplicate(layout.id);
  if (!result.ok) failed("The copy was not made", result.message);
}

function actions(layout: CustomLayout): DropdownMenuItem[][] {
  return [
    [
      { label: "Edit", icon: "i-lucide-pencil-ruler", onSelect: () => edit(layout) },
      {
        label: "Rename…",
        icon: "i-lucide-pencil",
        onSelect: () => {
          renaming.value = layout;
          renameOpen.value = true;
        },
      },
      { label: "Duplicate", icon: "i-lucide-copy", onSelect: () => void onDuplicate(layout) },
    ],
    [
      {
        label: "Delete…",
        icon: "i-lucide-trash-2",
        color: "error",
        onSelect: () => {
          deleting.value = layout;
          deleteOpen.value = true;
        },
      },
    ],
  ];
}

function describe(shape: LayoutShape): string {
  const n = shape.boxes.length;
  const photos = `${n} ${n === 1 ? "photo" : "photos"}`;
  if (shape.kind === "spread") return `Spread · ${photos}`;
  return `${shape.side === "left" ? "Left" : "Right"} page · ${photos}`;
}

function sentence(words: string): string {
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The built-ins grouped by photo count, fewest first. */
const builtinGroups = computed(() => {
  const groups = new Map<number, typeof builtins.value>();
  for (const b of builtins.value) {
    const n = b.shape.boxes.length;
    groups.set(n, [...(groups.get(n) ?? []), b]);
  }
  return [...groups.entries()].toSorted(([a], [b]) => a - b).map(([count, items]) => ({ count, items }));
});

const subtitle = computed(() => {
  const n = layouts.value.length;
  return loaded.value ? (n === 1 ? "1 of your own" : `${n} of your own`) : null;
});
</script>

<template>
  <AppHeader :title="editing ? (editing.id === null ? 'New layout' : 'Edit layout') : 'Layouts'" :subtitle="editing ? null : subtitle">
    <UDropdownMenu v-if="!editing" :items="newItems" :content="{ align: 'end' }">
      <UButton icon="i-lucide-plus" color="neutral" variant="outline" trailing-icon="i-lucide-chevron-down">New layout</UButton>
    </UDropdownMenu>
  </AppHeader>

  <LayoutEditor
    v-if="editing"
    :key="editing.id ?? 'new'"
    :shape="editing.shape"
    :name="editing.name"
    :spec="defaultSpec"
    :saving
    :is-new="editing.id === null"
    @save="onSave"
    @cancel="leave(editing.projectId)"
  />

  <div v-else class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto max-w-6xl space-y-10 p-6">
      <section aria-labelledby="layouts-mine" class="space-y-3">
        <div>
          <h2 id="layouts-mine" class="text-base font-semibold text-highlighted">Your layouts</h2>
          <p class="text-sm text-muted">
            Every book can use these. They always show in a page's layout menu when the photo count
            matches; turn on "Use in auto layout" to let books pick one on their own.
          </p>
        </div>
        <p
          v-if="loaded && layouts.length === 0"
          class="rounded-lg border border-dashed border-default p-6 text-sm text-muted"
        >
          No layouts of your own yet. Start one with New layout, copy a built-in below, or choose
          "Save as layout…" in a page's layout menu.
        </p>
        <ul class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-4">
          <li
            v-for="layout in layouts"
            :key="layout.id"
            class="flex flex-col gap-3 rounded-lg border border-default bg-default p-3"
          >
            <button
              type="button"
              class="flex justify-center rounded-md bg-muted p-3 hover:bg-elevated focus-visible:outline-2 focus-visible:outline-primary"
              :aria-label="`Edit ${layout.name}`"
              @click="edit(layout)"
            >
              <TemplateSketch :pages="shapePages(layout.shape)" :aspect="pageAspect" large />
            </button>
            <div class="flex items-start gap-2">
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm font-medium text-highlighted" :title="layout.name">{{ layout.name }}</p>
                <p class="text-sm text-muted">{{ describe(layout.shape) }}</p>
              </div>
              <UDropdownMenu :items="actions(layout)" :content="{ align: 'end' }">
                <UButton icon="i-lucide-ellipsis" color="neutral" variant="ghost" :aria-label="`More for ${layout.name}`" />
              </UDropdownMenu>
            </div>
            <USwitch
              :model-value="layout.auto"
              label="Use in auto layout"
              @update:model-value="(on: boolean) => onAuto(layout, on)"
            />
          </li>
        </ul>
      </section>

      <section aria-labelledby="layouts-builtin" class="space-y-4">
        <div>
          <h2 id="layouts-builtin" class="text-base font-semibold text-highlighted">Built-in layouts</h2>
          <p class="text-sm text-muted">Books use these on their own. Copy one to make a version of your own.</p>
        </div>
        <div v-for="group in builtinGroups" :key="group.count" class="space-y-2">
          <h3 class="text-sm font-medium text-toned">{{ group.count }} {{ group.count === 1 ? "photo" : "photos" }}</h3>
          <ul class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-4">
            <li
              v-for="b in group.items"
              :key="b.id"
              class="flex flex-col gap-3 rounded-lg border border-default bg-default p-3"
            >
              <div class="flex justify-center rounded-md bg-muted p-3">
                <TemplateSketch :pages="shapePages(b.shape)" :aspect="pageAspect" large />
              </div>
              <div class="flex items-center gap-2">
                <p class="min-w-0 flex-1 truncate text-sm text-default" :title="sentence(templateLabel(b.id))">
                  {{ sentence(templateLabel(b.id)) }}
                </p>
                <UTooltip text="Make an editable copy">
                  <UButton
                    icon="i-lucide-copy-plus"
                    color="neutral"
                    variant="ghost"
                    :aria-label="`Copy ${templateLabel(b.id)}`"
                    @click="copyBuiltin(b.id, b.shape)"
                  />
                </UTooltip>
              </div>
            </li>
          </ul>
        </div>
      </section>
    </div>
  </div>

  <LayoutNameDialog
    v-if="renaming"
    v-model:open="renameOpen"
    :name="renaming.name"
    title="Rename layout"
    action="Rename"
    @save="onRename"
  />
  <UModal v-model:open="deleteOpen" :title="`Delete “${deleting?.name ?? ''}”?`" :ui="{ footer: 'justify-end' }">
    <template #body>
      <p class="text-sm text-default">
        Books stop offering it. Pages already laid out with it keep their photos where they are.
      </p>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="deleteOpen = false">Cancel</UButton>
      <UButton color="error" @click="onDelete">Delete layout</UButton>
    </template>
  </UModal>
</template>
