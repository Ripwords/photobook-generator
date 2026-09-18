<script setup lang="ts">
import { shortcutKbds } from "~/types/shortcuts";

const {
  title,
  subtitle = null,
  subtitleTitle = null,
} = defineProps<{
  title: string;
  /** A second line under the title, usually the folders a screen is working over. */
  subtitle?: string | null;
  /** Hover text for the subtitle, for when the subtitle itself is an elision. */
  subtitleTitle?: string | null;
}>();

const { sidebarOpen, toggleSidebar } = useShell();
</script>

<template>
  <!--
    The window's toolbar. The title bar is an overlay (see tauri.conf.json), so
    this strip is what the window is dragged by; with the sidebar folded away
    it also sits under the traffic lights, and keeps clear of them.
  -->
  <header
    data-tauri-drag-region
    class="flex h-12 shrink-0 items-center justify-between gap-4 border-b border-default bg-default pr-3"
    :class="sidebarOpen ? 'pl-4' : 'pl-20'"
  >
    <div data-tauri-drag-region class="flex min-w-0 items-center gap-2">
      <UTooltip v-if="!sidebarOpen" text="Show sidebar" :kbds="shortcutKbds('sidebar')">
        <UButton
          icon="i-lucide-panel-left"
          color="neutral"
          variant="ghost"
          size="sm"
          aria-label="Show sidebar"
          @click="toggleSidebar"
        />
      </UTooltip>
      <div data-tauri-drag-region class="flex min-w-0 items-baseline gap-2">
        <!-- A screen whose title is editable (the book editor's rename) replaces it here. -->
        <slot name="title">
          <h1 class="truncate text-sm font-semibold text-highlighted">{{ title }}</h1>
        </slot>
        <p
          v-if="subtitle"
          class="truncate text-xs text-muted"
          :title="subtitleTitle ?? undefined"
        >
          {{ subtitle }}
        </p>
      </div>
    </div>
    <div class="flex shrink-0 items-center gap-1.5">
      <slot />
    </div>
  </header>
</template>
