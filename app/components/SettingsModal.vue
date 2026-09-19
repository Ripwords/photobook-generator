<script setup lang="ts">
import type { SettingsTab } from "~/composables/useShell";
import { SHORTCUTS, type ShortcutId, shortcutKbds } from "~/types/shortcuts";
import { DEFAULT_CACHE_LIMIT, formatBytes, limitItems, storageSummary } from "~/types/storage";
import type { UpdaterState } from "~/types/updater";

const { settingsOpen, settingsTab } = useShell();
const colorMode = useColorMode();
const version = useRuntimeConfig().public.appVersion;

const TABS: { id: SettingsTab; label: string; icon: string }[] = [
  { id: "general", label: "General", icon: "i-lucide-sliders-horizontal" },
  { id: "keys", label: "API keys", icon: "i-lucide-key-round" },
  { id: "about", label: "About", icon: "i-lucide-info" },
];

const APPEARANCES = [
  { value: "system", label: "System", icon: "i-lucide-monitor" },
  { value: "light", label: "Light", icon: "i-lucide-sun" },
  { value: "dark", label: "Dark", icon: "i-lucide-moon" },
] as const;

const shortcutIds = Object.keys(SHORTCUTS) as ShortcutId[];

const { status: cache, error: cacheError, busy: cacheBusy, refresh: refreshCache, setLimit, clearUnused } =
  useCacheStorage();
const summary = computed(() => (cache.value ? storageSummary(cache.value) : null));
const limits = computed(() => limitItems(cache.value?.limitBytes ?? DEFAULT_CACHE_LIMIT));
const freed = ref<string | null>(null);

watch(
  settingsOpen,
  (open) => {
    freed.value = null;
    if (open) void refreshCache();
  },
  { immediate: true },
);

async function clearCache() {
  const before = cache.value?.usedBytes ?? 0;
  await clearUnused();
  if (!cacheError.value && cache.value) {
    freed.value = `Freed ${formatBytes(Math.max(0, before - cache.value.usedBytes))}.`;
  }
}

function changeLimit(limitBytes: number) {
  freed.value = null;
  void setLimit(limitBytes);
}

const { state: updater, check, install, dismiss } = useUpdater();
const availableUpdate = computed(() => (updater.value.phase === "available" ? updater.value.update : null));
const downloadPercent = computed(() => (updater.value.phase === "downloading" ? updater.value.percent : null));
const updateError = computed(() => (updater.value.phase === "error" ? updater.value.message : null));

const CHECK_LABELS: Partial<Record<UpdaterState["phase"], string>> = {
  checking: "Checking…",
  error: "Try again",
};
const checkLabel = computed(() => CHECK_LABELS[updater.value.phase] ?? "Check for updates");
</script>

<template>
  <UModal
    v-model:open="settingsOpen"
    title="Settings"
    description="Appearance, storage, keyboard shortcuts and the keys the chat uses."
    :ui="{ content: 'sm:max-w-3xl', body: 'p-0 sm:p-0' }"
  >
    <template #body>
      <div class="flex min-h-[26rem]">
        <div class="w-44 shrink-0 space-y-0.5 border-r border-default bg-muted p-2">
          <button
            v-for="tab in TABS"
            :key="tab.id"
            type="button"
            class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors focus-visible:outline-2 focus-visible:outline-primary"
            :class="
              settingsTab === tab.id
                ? 'bg-accented/70 font-medium text-highlighted'
                : 'text-toned hover:bg-elevated hover:text-highlighted'
            "
            :aria-current="settingsTab === tab.id ? 'page' : undefined"
            @click="settingsTab = tab.id"
          >
            <UIcon :name="tab.icon" class="size-4 shrink-0" />
            {{ tab.label }}
          </button>
        </div>

        <div class="min-w-0 flex-1 p-6">
          <div v-if="settingsTab === 'general'" class="space-y-8">
            <section class="space-y-3" aria-labelledby="settings-appearance">
              <h3 id="settings-appearance" class="text-sm font-medium text-highlighted">Appearance</h3>
              <div class="grid grid-cols-3 gap-2" role="radiogroup" aria-labelledby="settings-appearance">
                <button
                  v-for="option in APPEARANCES"
                  :key="option.value"
                  type="button"
                  role="radio"
                  :aria-checked="colorMode.preference === option.value"
                  class="flex flex-col items-center gap-2 rounded-md border px-3 py-3 text-sm transition-colors focus-visible:outline-2 focus-visible:outline-primary"
                  :class="
                    colorMode.preference === option.value
                      ? 'border-inverted text-highlighted ring-1 ring-inverted'
                      : 'border-default text-toned hover:bg-elevated'
                  "
                  @click="colorMode.preference = option.value"
                >
                  <UIcon :name="option.icon" class="size-5" />
                  {{ option.label }}
                </button>
              </div>
            </section>

            <section class="space-y-3" aria-labelledby="settings-storage">
              <h3 id="settings-storage" class="text-sm font-medium text-highlighted">Storage</h3>
              <div class="divide-y divide-default rounded-md border border-default text-sm">
                <div class="space-y-2 px-3 py-3">
                  <div class="flex items-baseline justify-between gap-4">
                    <span>Analysis cache</span>
                    <span v-if="summary" class="tabular-nums text-highlighted">
                      {{ summary.used }} <span class="text-muted">of {{ summary.limit }}</span>
                    </span>
                    <span v-else class="text-muted">…</span>
                  </div>
                  <UProgress
                    :model-value="Math.round((summary?.fraction ?? 0) * 100)"
                    color="neutral"
                    size="xs"
                    aria-label="Share of the limit in use"
                  />
                  <p v-if="summary?.warning" class="text-xs text-muted">{{ summary.warning }}</p>
                </div>
                <div class="flex items-center justify-between gap-4 px-3 py-3">
                  <div class="min-w-0">
                    <label for="settings-cache-limit">Limit</label>
                    <p class="text-xs text-muted">
                      Past it, the photos no book or draft uses are removed, oldest first. They are
                      analysed again if you need them.
                    </p>
                  </div>
                  <USelect
                    id="settings-cache-limit"
                    :model-value="cache?.limitBytes"
                    :items="limits"
                    :disabled="!cache || cacheBusy"
                    class="w-28 shrink-0"
                    @update:model-value="changeLimit"
                  />
                </div>
                <div class="flex items-center justify-between gap-4 px-3 py-3">
                  <div class="min-w-0">
                    <p>Clear unused</p>
                    <p class="text-xs text-muted">
                      {{ freed ?? "Removes every analysed photo that no saved book or open draft uses." }}
                    </p>
                  </div>
                  <UButton
                    color="neutral"
                    variant="outline"
                    size="sm"
                    class="shrink-0"
                    :loading="cacheBusy"
                    :disabled="!cache"
                    @click="clearCache"
                  >
                    Clear unused
                  </UButton>
                </div>
              </div>
              <p v-if="cacheError" class="text-xs text-error">{{ cacheError }}</p>
            </section>

            <section class="space-y-3" aria-labelledby="settings-shortcuts">
              <h3 id="settings-shortcuts" class="text-sm font-medium text-highlighted">Keyboard shortcuts</h3>
              <ul class="divide-y divide-default rounded-md border border-default">
                <li v-for="id in shortcutIds" :key="id" class="flex items-center justify-between gap-4 px-3 py-2 text-sm">
                  <span class="flex-1">{{ SHORTCUTS[id].label }}</span>
                  <span class="text-xs text-muted">{{ SHORTCUTS[id].where }}</span>
                  <span class="flex w-14 justify-end gap-0.5">
                    <UKbd v-for="key in shortcutKbds(id)" :key :value="key" />
                  </span>
                </li>
              </ul>
            </section>
          </div>

          <ApiKeysPanel v-else-if="settingsTab === 'keys'" />

          <div v-else class="space-y-4 text-sm">
            <div class="flex items-center gap-3">
              <span class="flex size-10 items-center justify-center rounded-lg bg-inverted text-inverted">
                <UIcon name="i-lucide-book-open" class="size-5" />
              </span>
              <div>
                <p class="font-semibold text-highlighted">PhotobookGen</p>
                <p class="text-muted">Version {{ version }}</p>
              </div>
            </div>

            <section class="space-y-2" aria-label="Updates">
              <div v-if="availableUpdate" class="space-y-3 rounded-md border border-default p-3">
                <p class="text-highlighted">Version {{ availableUpdate.version }} is available.</p>
                <p
                  v-if="availableUpdate.notes"
                  class="max-h-32 overflow-y-auto whitespace-pre-line text-xs text-toned"
                >{{ availableUpdate.notes }}</p>
                <div class="flex items-center gap-2">
                  <UButton
                    color="neutral"
                    variant="outline"
                    size="sm"
                    icon="i-lucide-download"
                    @click="install"
                  >
                    Install and restart
                  </UButton>
                  <UButton color="neutral" variant="ghost" size="sm" @click="dismiss">Not now</UButton>
                </div>
              </div>

              <div v-else-if="updater.phase === 'downloading'" class="space-y-2">
                <div class="flex items-baseline justify-between gap-4">
                  <span class="text-toned">Downloading the update</span>
                  <span v-if="downloadPercent !== null" class="tabular-nums text-highlighted">
                    {{ downloadPercent }}%
                  </span>
                </div>
                <UProgress
                  :model-value="downloadPercent"
                  color="neutral"
                  size="xs"
                  aria-label="Update download progress"
                />
              </div>

              <p v-else-if="updater.phase === 'installing'" class="text-toned">
                Installing. PhotobookGen will restart.
              </p>

              <template v-else>
                <div class="flex items-center gap-3">
                  <UButton
                    color="neutral"
                    variant="outline"
                    size="sm"
                    icon="i-lucide-rotate-cw"
                    :loading="updater.phase === 'checking'"
                    :disabled="updater.phase === 'checking'"
                    @click="check(false)"
                  >
                    {{ checkLabel }}
                  </UButton>
                  <span v-if="updater.phase === 'uptodate'" class="text-xs text-muted">
                    You're up to date.
                  </span>
                </div>
                <p v-if="updateError" class="flex items-start gap-1.5 text-xs text-error">
                  <UIcon name="i-lucide-triangle-alert" class="mt-0.5 size-3.5 shrink-0" />
                  {{ updateError }}
                </p>
              </template>
            </section>

            <p class="max-w-prose text-toned">
              Turns folders of photos into a print-ready photobook. Every photo is analysed on this
              Mac, and no image is ever uploaded. The chat sends only a description of the book:
              its layouts, page numbers and photo tags.
            </p>
          </div>
        </div>
      </div>
    </template>
  </UModal>
</template>
