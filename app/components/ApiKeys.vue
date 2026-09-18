<script setup lang="ts">
import type { ModelProvider } from "~/agent/fetch";

const open = defineModel<boolean>("open", { required: true });

const { status, error, refresh, save, clear } = useApiKeys();

interface Provider {
  id: ModelProvider;
  name: string;
  required: boolean;
  purpose: string;
  placeholder: string;
}

const PROVIDERS: Provider[] = [
  {
    id: "deepseek",
    name: "DeepSeek",
    required: true,
    purpose: "Runs the chat. It reads a description of the book: layouts, page numbers and photo tags.",
    placeholder: "sk-…",
  },
  {
    id: "jev",
    name: "Jev by TypeSafe",
    required: false,
    purpose:
      "Checks each proposed edit against what you asked for, and finds photos by description. Chat works without it.",
    placeholder: "Your TypeSafe key",
  },
];

const drafts = reactive<Record<ModelProvider, string>>({ deepseek: "", jev: "" });
const saving = ref<ModelProvider | null>(null);

async function saveKey(provider: ModelProvider) {
  saving.value = provider;
  await save(provider, drafts[provider]);
  saving.value = null;
  if (!error.value) drafts[provider] = "";
}

watch(open, (isOpen) => {
  if (isOpen) void refresh();
});
</script>

<template>
  <UModal
    v-model:open="open"
    title="API keys"
    description="Your photos never leave this Mac. Only the book's description and your messages are sent."
  >
    <template #body>
      <div class="space-y-6">
        <section
          v-for="provider in PROVIDERS"
          :key="provider.id"
          class="space-y-2"
          :aria-labelledby="`key-${provider.id}`"
        >
          <div class="flex items-center justify-between gap-3">
            <h3 :id="`key-${provider.id}`" class="flex items-center gap-2 text-sm font-medium text-highlighted">
              {{ provider.name }}
              <UBadge
                :label="provider.required ? 'Required' : 'Optional'"
                :color="provider.required ? 'primary' : 'neutral'"
                variant="subtle"
                size="sm"
              />
            </h3>
            <span
              v-if="status"
              class="flex items-center gap-1.5 text-xs"
              :class="status[provider.id] ? 'text-success' : 'text-muted'"
            >
              <UIcon
                :key="String(status[provider.id])"
                :name="status[provider.id] ? 'i-lucide-check' : 'i-lucide-circle-dashed'"
                class="size-3.5 shrink-0"
              />
              {{ status[provider.id] ? "Saved" : "Not set" }}
            </span>
          </div>
          <p class="text-sm text-muted">{{ provider.purpose }}</p>
          <form class="flex items-center gap-2" @submit.prevent="saveKey(provider.id)">
            <UInput
              v-model="drafts[provider.id]"
              type="password"
              autocomplete="off"
              :placeholder="status?.[provider.id] ? 'Replace the saved key' : provider.placeholder"
              :aria-label="`${provider.name} API key`"
              class="flex-1"
            />
            <UButton
              type="submit"
              color="primary"
              :loading="saving === provider.id"
              :disabled="!drafts[provider.id].trim()"
            >
              Save
            </UButton>
            <UButton
              v-if="status?.[provider.id]"
              color="neutral"
              variant="ghost"
              :aria-label="`Clear the ${provider.name} key`"
              @click="clear(provider.id)"
            >
              Clear
            </UButton>
          </form>
        </section>

        <UAlert
          v-if="error"
          icon="i-lucide-triangle-alert"
          color="error"
          variant="subtle"
          :description="error"
        />

        <p class="flex items-start gap-2 text-xs text-muted">
          <UIcon name="i-lucide-key-round" class="mt-0.5 size-3.5 shrink-0" />
          Keys are kept in your macOS keychain. This app can check that a key is saved, but never
          shows it again.
        </p>
      </div>
    </template>
  </UModal>
</template>
