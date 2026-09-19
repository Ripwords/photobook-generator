<script setup lang="ts">
/** Renames a book from somewhere with no room to edit its name in place. */
const open = defineModel<boolean>("open", { required: true });

const { name } = defineProps<{ name: string }>();

const emit = defineEmits<{ rename: [name: string] }>();

const draft = ref(name);

watch(open, (isOpen) => {
  if (isOpen) draft.value = name;
});

const trimmed = computed(() => draft.value.trim());

function save() {
  if (!trimmed.value) return;
  open.value = false;
  // The same no-op rule as renaming in place: an unchanged name is not sent,
  // so it does not bump the book to the top of "last edited".
  if (trimmed.value !== name) emit("rename", trimmed.value);
}
</script>

<template>
  <UModal v-model:open="open" title="Rename photobook" :ui="{ footer: 'justify-end' }">
    <template #body>
      <form id="rename-book" @submit.prevent="save">
        <UFormField label="Name">
          <UInput v-model="draft" autofocus class="w-full" />
        </UFormField>
      </form>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="open = false">Cancel</UButton>
      <UButton type="submit" form="rename-book" :disabled="!trimmed">Rename</UButton>
    </template>
  </UModal>
</template>
