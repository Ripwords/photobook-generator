<script setup lang="ts">
/** Names a layout: saving one from a book's page, or renaming one. */
const open = defineModel<boolean>("open", { required: true });

const { name, title, action } = defineProps<{
  name: string;
  title: string;
  /** The confirm button's label. */
  action: string;
}>();

const emit = defineEmits<{ save: [name: string] }>();

const draft = ref(name);

watch(open, (isOpen) => {
  if (isOpen) draft.value = name;
});

const trimmed = computed(() => draft.value.trim());

function save() {
  if (!trimmed.value) return;
  open.value = false;
  emit("save", trimmed.value);
}
</script>

<template>
  <UModal v-model:open="open" :title :ui="{ footer: 'justify-end' }">
    <template #body>
      <form id="name-layout" @submit.prevent="save">
        <UFormField label="Name">
          <UInput v-model="draft" autofocus class="w-full" />
        </UFormField>
      </form>
    </template>
    <template #footer>
      <UButton color="neutral" variant="outline" @click="open = false">Cancel</UButton>
      <UButton type="submit" form="name-layout" :disabled="!trimmed">{{ action }}</UButton>
    </template>
  </UModal>
</template>
