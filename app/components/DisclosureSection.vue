<script setup lang="ts">
/**
 * A closed-by-default section of a side panel: a full-width trigger naming
 * what is inside, and a chevron that turns when it opens. For what most people
 * never need on screen -- technical counts, a key to the marks, advanced
 * knobs -- so the panel stays short until someone asks.
 */
const { label } = defineProps<{ label: string }>();
</script>

<template>
  <UCollapsible class="group/disclosure">
    <UButton
      color="neutral"
      variant="ghost"
      block
      :label
      trailing-icon="i-lucide-chevron-down"
      class="-mx-2.5 w-[calc(100%+1.25rem)] justify-between font-medium text-highlighted"
      :ui="{
        trailingIcon: 'transition-transform duration-200 group-data-[state=open]/disclosure:rotate-180',
      }"
    />
    <template #content>
      <div class="pt-2">
        <slot />
      </div>
    </template>
  </UCollapsible>
</template>
