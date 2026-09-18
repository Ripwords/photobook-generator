<script setup lang="ts">
import type { SettingsTab } from "~/composables/useShell";
import { SHORTCUTS, type ShortcutId, shortcutKbds } from "~/types/shortcuts";

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
</script>

<template>
  <UModal
    v-model:open="settingsOpen"
    title="Settings"
    description="Appearance, keyboard shortcuts and the keys the chat uses."
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
