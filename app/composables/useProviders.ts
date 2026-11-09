import { Channel, invoke } from "@tauri-apps/api/core";
// Imported explicitly rather than left to Nuxt's auto-imports, so
// `tests/providers-invoke.test.ts` can run this under plain vitest.
import { computed, ref } from "vue";
import {
  sendView,
  type ProviderId,
  type ProviderStatus,
  type SendEvent,
  type SendPlan,
} from "~/types/providers";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Sending one book to Pixajoy: the sign-in, what the send will do, and the
 * send itself. Every failure ends as a `failed` event, whether Rust sent one
 * or the invoke rejected.
 */
export function useProviders(projectId: number) {
  const provider: ProviderId = "pixajoy";

  const status = ref<ProviderStatus | null>(null);
  const statusError = ref<string | null>(null);
  const signingIn = ref(false);
  const plan = ref<SendPlan | null>(null);
  const planError = ref<string | null>(null);
  const lastEvent = ref<SendEvent | null>(null);
  const sending = ref(false);
  const view = computed(() => sendView(lastEvent.value));

  async function runStatus(work: () => Promise<ProviderStatus>) {
    statusError.value = null;
    try {
      status.value = await work();
    } catch (e) {
      statusError.value = message(e);
    }
  }

  const refreshStatus = () => runStatus(() => invoke<ProviderStatus>("provider_status", { provider }));

  async function signIn() {
    signingIn.value = true;
    await runStatus(() => invoke<ProviderStatus>("provider_sign_in", { provider }));
    signingIn.value = false;
  }

  const signOut = () =>
    runStatus(async () => {
      await invoke<void>("provider_sign_out", { provider });
      return { signedIn: false, email: null };
    });

  async function loadPlan() {
    planError.value = null;
    try {
      plan.value = await invoke<SendPlan>("provider_send_plan", { projectId, provider });
    } catch (e) {
      plan.value = null;
      planError.value = message(e);
    }
  }

  async function send() {
    if (sending.value) return;
    sending.value = true;
    lastEvent.value = null;
    const onEvent = new Channel<SendEvent>();
    // Tauri's `Channel` has no `addEventListener`, only this settable field.
    // oxlint-disable-next-line unicorn/prefer-add-event-listener
    onEvent.onmessage = (event) => {
      lastEvent.value = event;
    };
    try {
      await invoke<void>("send_to_provider", { projectId, provider, onEvent });
    } catch (e) {
      // Rust's own `failed` event knows whether a project was created; keep it.
      if (view.value.kind !== "failed") {
        lastEvent.value = { kind: "failed", message: message(e), projectName: null };
      }
    } finally {
      sending.value = false;
    }
  }

  const openProjects = () => invoke<void>("open_provider_projects", { provider });

  /** Forgets the last send's outcome, so the dialog opens fresh. */
  function reset() {
    if (!sending.value) lastEvent.value = null;
  }

  return {
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
  };
}
