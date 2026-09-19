<script setup lang="ts">
/**
 * Asks before a book is deleted. The delete itself is the caller's, which
 * then offers Undo -- see `deleteBook` in `pages/index.vue`.
 */
const open = defineModel<boolean>("open", { required: true });

const { name } = defineProps<{ name: string }>();

const emit = defineEmits<{ confirm: [] }>();

function confirm() {
  open.value = false;
  emit("confirm");
}
</script>

<template>
  <UModal v-model:open="open" :title="`Delete “${name}”?`" :ui="{ footer: 'justify-end' }">
    <template #body>
      <div class="space-y-3 text-sm">
        <p class="text-default">
          This removes <span class="font-medium">{{ name }}</span>'s page layout, your
          include/exclude decisions and its export history from PhotobookGen. The notice that
          follows can undo it.
        </p>
        <p class="text-muted">
          Files you already exported to disk are not touched, and the photo analysis cache is
          kept, so reopening this folder later will not re-scan your photos.
        </p>
      </div>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="open = false">Cancel</UButton>
      <UButton color="error" @click="confirm">Delete photobook</UButton>
    </template>
  </UModal>
</template>
