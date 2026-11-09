<script setup lang="ts">
import { planNote } from "~/types/providers";

/**
 * Sends the open book to the user's Pixajoy account as a new project. Nothing
 * is uploaded until Send is pressed, and nothing is ever ordered.
 */
const open = defineModel<boolean>("open", { required: true });

const { projectId } = defineProps<{ projectId: number }>();

const {
  status,
  statusError,
  signingIn,
  plan,
  planError,
  sending,
  view,
  refreshStatus,
  signIn,
  signOut,
  loadPlan,
  send,
  openProjects,
  reset,
} = useProviders(projectId);

watch(open, (isOpen) => {
  if (!isOpen) return;
  reset();
  void refreshStatus();
  void loadPlan();
});

const note = computed(() => (plan.value ? planNote(plan.value) : null));
const canSend = computed(
  () => !!status.value?.signedIn && plan.value !== null && !sending.value && !signingIn.value,
);
const percent = computed(() => {
  const current = view.value;
  if (current.kind !== "sending" || current.progress === null) return null;
  return Math.round(current.progress * 100);
});
</script>

<template>
  <!--
    Not dismissible mid-send. The send would carry on in Rust, but the only
    word of how it ended is in this dialog.
  -->
  <UModal
    v-model:open="open"
    title="Send to Pixajoy"
    description="Creates a new project in your Pixajoy account from this book."
    :dismissible="!sending"
    :close="!sending"
    :ui="{ footer: 'justify-end' }"
  >
    <template #body>
      <div class="space-y-6 text-sm">
        <section class="space-y-2" aria-labelledby="send-account">
          <h3 id="send-account" class="font-medium text-highlighted">Pixajoy account</h3>
          <div class="flex items-center gap-2 rounded-md border border-default px-3 py-2">
            <UIcon name="i-lucide-user-round" class="size-4 shrink-0 text-muted" />
            <p v-if="signingIn" class="min-w-0 flex-1 text-muted">
              Finish signing in in the Pixajoy window.
            </p>
            <p v-else-if="status?.signedIn" class="min-w-0 flex-1 truncate" :title="status.email ?? undefined">
              Signed in{{ status.email ? ` as ${status.email}` : "" }}
            </p>
            <p v-else-if="status" class="min-w-0 flex-1 text-muted">Not signed in</p>
            <p v-else class="min-w-0 flex-1 text-muted">Checking…</p>
            <UButton
              v-if="status?.signedIn"
              color="neutral"
              variant="outline"
              :disabled="sending"
              @click="signOut"
            >
              Sign out
            </UButton>
            <UButton
              v-else
              color="neutral"
              variant="outline"
              :loading="signingIn"
              :disabled="sending || signingIn"
              @click="signIn"
            >
              Sign in
            </UButton>
          </div>
          <p v-if="statusError" class="text-muted">{{ statusError }}</p>
        </section>

        <section class="space-y-2" aria-labelledby="send-what">
          <h3 id="send-what" class="font-medium text-highlighted">What happens</h3>
          <ul class="list-disc space-y-1 pl-5 text-default">
            <li>A new project is created in your Pixajoy account.</li>
            <li>Each of the book's photos is uploaded whole to Pixajoy's servers, once. A later send reuses it.</li>
            <li>Nothing is ordered. You review and order the book on Pixajoy.</li>
          </ul>
        </section>

        <section class="space-y-1" aria-labelledby="send-pages">
          <h3 id="send-pages" class="font-medium text-highlighted">Pages</h3>
          <p v-if="planError" class="text-default">{{ planError }}</p>
          <template v-else-if="note">
            <p class="text-default tabular-nums">{{ note.pages }}</p>
            <p v-if="note.padding" class="text-muted">{{ note.padding }}</p>
          </template>
          <p v-else class="text-muted">Checking…</p>
        </section>

        <div v-if="view.kind === 'sending'" class="space-y-2" role="status">
          <UProgress color="primary" size="sm" :model-value="percent" />
          <p class="text-muted tabular-nums">{{ view.label }}</p>
        </div>

        <UAlert
          v-else-if="view.kind === 'done'"
          icon="i-lucide-check"
          color="neutral"
          variant="outline"
          :title="`Sent as “${view.projectName}”`"
          description="It is in My Projects on Pixajoy, ready to review and order."
          :ui="{ title: 'break-words' }"
        />

        <UAlert
          v-else-if="view.kind === 'failed'"
          icon="i-lucide-circle-x"
          color="neutral"
          variant="outline"
          title="The book was not sent"
          :description="view.leftover ? `${view.message} ${view.leftover}` : view.message"
          :ui="{ description: 'break-words' }"
        />
      </div>
    </template>

    <template #footer>
      <UButton color="neutral" variant="outline" :disabled="sending" @click="open = false">
        Close
      </UButton>
      <UButton
        v-if="view.kind === 'done'"
        icon="i-lucide-external-link"
        @click="openProjects"
      >
        Open My Projects
      </UButton>
      <UButton
        v-else
        icon="i-lucide-send"
        :loading="sending"
        :disabled="!canSend"
        @click="send"
      >
        Send
      </UButton>
    </template>
  </UModal>
</template>
