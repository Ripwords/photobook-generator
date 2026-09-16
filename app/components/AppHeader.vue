<script setup lang="ts">
const {
  title,
  subtitle = null,
  subtitleTitle = null,
  back = null,
} = defineProps<{
  title: string;
  /** A second line under the title, usually the folders a screen is working over. */
  subtitle?: string | null;
  /** Hover text for the subtitle, for when the subtitle itself is an elision. */
  subtitleTitle?: string | null;
  /** Label for the back button, or `null` on a screen with nowhere to go back to. */
  back?: string | null;
}>();

const emit = defineEmits<{
  back: [];
}>();
</script>

<template>
  <header
    class="sticky top-0 flex shrink-0 items-center justify-between gap-4 border-b border-default bg-default px-6 py-4"
  >
    <div class="flex min-w-0 items-center gap-2.5">
      <UButton
        v-if="back"
        icon="i-lucide-chevron-left"
        color="neutral"
        variant="ghost"
        size="sm"
        @click="emit('back')"
      >
        {{ back }}
      </UButton>
      <UIcon v-else name="i-lucide-images" class="size-5 shrink-0 text-primary" />
      <div class="min-w-0">
        <h1 class="truncate text-sm font-semibold text-highlighted">{{ title }}</h1>
        <p v-if="subtitle" class="truncate text-xs text-muted" :title="subtitleTitle ?? undefined">
          {{ subtitle }}
        </p>
      </div>
    </div>
    <div class="flex items-center gap-2">
      <slot />
      <UColorModeButton size="sm" />
    </div>
  </header>
</template>
