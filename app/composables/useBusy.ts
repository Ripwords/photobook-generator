import { computed, ref } from "vue";

/**
 * "Is a Tauri command in flight, and what did the last one fail with?" --
 * the pair every screen reads to disable its controls and show its error.
 *
 * Counted, not a boolean. Two commands overlap often enough to matter: the
 * undo shortcut fires whether or not a save is still running, the agent
 * writes while the user edits, and a folder check lands mid-export. A flag
 * set by both and cleared by whichever returned first brought every
 * `:disabled="busy"` control back to life while the slower command was still
 * running, so the user could press Export again mid-export.
 */
export function useBusy() {
  const running = ref(0);
  const busy = computed(() => running.value > 0);
  const error = ref<string | null>(null);

  /**
   * Runs `work` with `busy` held, turning a failure into `error` and `null`
   * rather than a rejection, so callers never need their own try/catch.
   */
  async function guard<T>(work: () => Promise<T>): Promise<T | null> {
    running.value += 1;
    error.value = null;
    try {
      return await work();
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      running.value -= 1;
    }
  }

  return { busy, error, guard };
}
