import { invoke } from "@tauri-apps/api/core";
import type { ModelProvider } from "~/agent/fetch";

/** Mirrors `agent::keys::KeyStatus`: whether each provider has a usable key. */
export type KeyStatus = Record<ModelProvider, boolean>;

/**
 * Which API keys are set, and saving or clearing them. The keys themselves
 * live in the macOS keychain: the webview hands one to Rust once, when it is
 * saved, and can only ask afterwards whether it is there.
 *
 * The status is shared state, so the chat panel sees a key the moment the
 * Settings dialog saves it. `null` until the first answer.
 */
export function useApiKeys() {
  const status = useState<KeyStatus | null>("api-key-status", () => null);
  const error = ref<string | null>(null);

  async function run(work: () => Promise<unknown>) {
    error.value = null;
    try {
      await work();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    }
    status.value = await invoke<KeyStatus>("api_key_status").catch(() => status.value);
  }

  return {
    status,
    error,
    refresh: () => run(async () => {}),
    save: (provider: ModelProvider, key: string) =>
      run(() => invoke("set_api_key", { provider, key: key.trim() })),
    clear: (provider: ModelProvider) => run(() => invoke("clear_api_key", { provider })),
  };
}
