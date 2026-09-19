import { invoke } from "@tauri-apps/api/core";
import type { CacheStatus } from "~/types/storage";

/**
 * The analysis cache's size and limit, and clearing what no book or draft
 * uses. Every command answers with the status after it ran, so the section
 * never shows a size from before its own action. `null` until the first answer.
 */
export function useCacheStorage() {
  const status = useState<CacheStatus | null>("cache-status", () => null);
  const error = ref<string | null>(null);
  const busy = ref(false);

  async function run(command: string, args?: Record<string, unknown>) {
    busy.value = true;
    error.value = null;
    try {
      status.value = await invoke<CacheStatus>(command, args);
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      busy.value = false;
    }
  }

  return {
    status,
    error,
    busy,
    refresh: () => run("cache_status"),
    setLimit: (limitBytes: number) => run("set_cache_limit", { limitBytes }),
    clearUnused: () => run("clear_unused_cache"),
  };
}
