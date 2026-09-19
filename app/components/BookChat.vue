<script setup lang="ts">
import type { UIMessage } from "ai";
import { NO_DEEPSEEK_KEY, messageBlocks, type ProposalBlock } from "~/agent/chat";
import { toSpreads, type BookLayout } from "~/types/preview";

const { projectId, layout } = defineProps<{
  projectId: number;
  layout: BookLayout | null;
}>();

const emit = defineEmits<{
  /** Rust saved an edit: reload the book on screen from Rust. */
  bookChanged: [];
  openKeys: [];
}>();

const { messages, status, error, sendMessage, stop, regenerate, clearError, approve } = useBookAgent(
  projectId,
  async () => emit("bookChanged"),
);
const { status: keys, refresh } = useApiKeys();

const spreads = computed(() => (layout ? toSpreads(layout.pages) : []));
const label = (opening: number) => spreads.value[opening]?.label ?? `Spread ${opening + 1}`;

const noKey = computed(
  () => keys.value?.deepseek === false || error.value?.message === NO_DEEPSEEK_KEY,
);

const SUGGESTIONS = [
  "Swap the two photos on page 2",
  "Try a different layout on pages 2–3",
  "Try a different layout on the last page",
];

const input = ref("");

function send(text: string) {
  const trimmed = text.trim();
  if (!trimmed || noKey.value) return;
  input.value = "";
  void sendMessage({ text: trimmed });
}

interface StatusLine {
  icon: string;
  text: string;
  tone: string;
  spin?: boolean;
}

function statusLine(block: ProposalBlock): StatusLine | null {
  switch (block.status) {
    case "preparing":
      return { icon: "i-lucide-loader-circle", text: "Preparing…", tone: "text-muted", spin: true };
    case "pending":
      return null;
    case "applying":
      return { icon: "i-lucide-loader-circle", text: "Applying…", tone: "text-muted", spin: true };
    case "applied":
      return { icon: "i-lucide-check", text: "Applied", tone: "text-success" };
    case "declined":
      return { icon: "i-lucide-x", text: "Not applied", tone: "text-muted" };
    case "refused":
      return { icon: "i-lucide-ban", text: "Couldn't apply", tone: "text-error" };
    case "failed":
      return { icon: "i-lucide-triangle-alert", text: "Couldn't be saved", tone: "text-error" };
  }
}

/** A message's blocks, each proposal with the status line it shows under its buttons. */
const blocks = (message: UIMessage) =>
  messageBlocks(message, label).map((block) =>
    block.kind === "proposal" ? { ...block, line: statusLine(block) } : block,
  );

watch(
  () => keys.value?.deepseek,
  (set) => {
    if (set && error.value?.message === NO_DEEPSEEK_KEY) clearError();
  },
);

// UChatMessages decides whether to show its scroll-down button only on a
// scroll event or mid-stream, so a stream that ends at the bottom leaves the
// button up. One synthetic scroll makes it measure again.
const body = useTemplateRef("body");
watch(status, (now, before) => {
  if (before === "streaming" && now !== "streaming") {
    void nextTick(() => body.value?.dispatchEvent(new Event("scroll")));
  }
});

onMounted(() => {
  if (!keys.value) void refresh();
});
</script>

<template>
  <div class="relative flex h-full min-h-0 flex-col">
    <div class="flex items-center gap-2 border-b border-default px-4 py-3">
      <UIcon name="i-lucide-sparkles" class="size-4 shrink-0 text-primary" />
      <h2 class="text-sm font-medium text-highlighted">Ask about this book</h2>
    </div>

    <div ref="body" class="min-h-0 flex-1 overflow-y-auto px-4 py-4">
      <div v-if="noKey" class="flex h-full flex-col items-center justify-center gap-3 px-4 text-center">
        <UIcon name="i-lucide-key-round" class="size-6 text-muted" />
        <p class="text-sm text-toned">{{ NO_DEEPSEEK_KEY }}</p>
        <UButton color="primary" size="sm" icon="i-lucide-key-round" @click="emit('openKeys')">
          Add API key
        </UButton>
      </div>

      <div
        v-else-if="messages.length === 0"
        class="flex h-full flex-col justify-end gap-3"
      >
        <p class="text-sm text-toned">
          Ask for a change in your own words. Each edit is shown to you first, and nothing changes
          until you apply it.
        </p>
        <div class="flex flex-col items-start gap-2">
          <UButton
            v-for="suggestion in SUGGESTIONS"
            :key="suggestion"
            color="neutral"
            variant="outline"
            size="sm"
            class="text-left"
            @click="send(suggestion)"
          >
            {{ suggestion }}
          </UButton>
        </div>
      </div>

      <UChatMessages
        v-else
        :messages="[...messages]"
        :status
        :should-auto-scroll="true"
        :user="{ side: 'right', variant: 'soft' }"
        :assistant="{ side: 'left', variant: 'naked' }"
        compact
      >
        <template #content="message">
          <div class="space-y-2">
            <template v-for="block in blocks(message)" :key="block.key">
              <p v-if="block.kind === 'text'" class="whitespace-pre-wrap text-sm">{{ block.text }}</p>

              <UChatReasoning
                v-else-if="block.kind === 'reasoning'"
                :text="block.text"
                :streaming="block.streaming"
              >
                <p class="whitespace-pre-wrap text-xs text-muted">{{ block.text }}</p>
              </UChatReasoning>

              <div
                v-else
                class="space-y-2 rounded-lg bg-elevated/60 p-3 ring ring-default"
                :data-proposal="block.status"
              >
                <div class="space-y-0.5">
                  <p class="text-sm font-medium text-highlighted">{{ block.title }}</p>
                  <p v-if="block.where" class="text-xs text-muted tabular-nums">{{ block.where }}</p>
                </div>

                <p v-if="block.warning" class="flex items-start gap-1.5 text-xs text-amber-700 dark:text-amber-300">
                  <UIcon name="i-lucide-triangle-alert" class="mt-px size-3.5 shrink-0" />
                  <span>{{ block.warning }}</span>
                </p>

                <div v-if="block.approvalId" class="flex gap-2">
                  <UButton
                    color="primary"
                    size="sm"
                    icon="i-lucide-check"
                    @click="approve(block.approvalId, true)"
                  >
                    Apply
                  </UButton>
                  <UButton
                    color="neutral"
                    variant="outline"
                    size="sm"
                    @click="approve(block.approvalId, false)"
                  >
                    Don't apply
                  </UButton>
                </div>

                <p v-if="block.line" class="flex items-center gap-1.5 text-xs" :class="block.line.tone">
                  <UIcon
                    :key="block.line.icon"
                    :name="block.line.icon"
                    class="size-3.5 shrink-0"
                    :class="block.line.spin && 'animate-spin'"
                  />
                  {{ block.line.text }}
                </p>
                <p v-if="block.reason" class="text-xs text-toned">{{ block.reason }}</p>
              </div>
            </template>
          </div>
        </template>
      </UChatMessages>

      <UAlert
        v-if="error && !noKey"
        class="mt-3"
        icon="i-lucide-triangle-alert"
        color="error"
        variant="subtle"
        :description="error.message"
        :ui="{ description: 'break-words' }"
        :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => regenerate() }]"
      />
    </div>

    <div class="border-t border-default p-3">
      <UChatPrompt
        v-model="input"
        placeholder="Ask for a change…"
        :disabled="noKey"
        variant="subtle"
        @submit="send(input)"
      >
        <UChatPromptSubmit :status color="primary" @stop="stop" @reload="regenerate" />
      </UChatPrompt>
    </div>
  </div>
</template>
