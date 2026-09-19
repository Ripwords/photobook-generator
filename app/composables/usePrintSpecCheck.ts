import { invoke } from "@tauri-apps/api/core";
// Explicit rather than auto-imported so `tests/printSpec.test.ts` can run this
// under plain vitest, like `usePhotoOverrides`.
import { ref, watch, type Ref } from "vue";
import type { CheckState, Proposal, SpecCheck } from "~/types/printSpec";

/** Typing pauses this long before Rust is asked about the new numbers. */
export const SPEC_CHECK_SETTLE_MS = 300;

/**
 * Rust's verdict on the spec the Print size panel is showing.
 *
 * Every keystroke that parses becomes one debounced `check_print_spec`. The
 * answer is the only thing that enables Apply (see `applyState`), so the
 * validator, the dry-run findings and the proposed guides all come from the
 * engine, never from a TypeScript copy of its rules.
 *
 * `projectId` is `null` on the draft screen, where there is no book to try
 * the spec on and the answer is only "buildable or not".
 */
export function usePrintSpecCheck(projectId: Ref<number | null>, proposal: Ref<Proposal>) {
  const state = ref<CheckState>({ status: "idle" }) as Ref<CheckState>;
  // Only the newest request may land: a slow answer about an older spec
  // would enable Apply for numbers no longer on screen.
  let latest = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function run(generation: number) {
    const p = proposal.value;
    if (!p.ok) return;
    try {
      const result = await invoke<SpecCheck>("check_print_spec", { projectId: projectId.value, spec: p.spec });
      if (generation === latest) state.value = { status: "done", result };
    } catch (e) {
      if (generation === latest) state.value = { status: "failed", error: String(e) };
    }
  }

  watch(
    [proposal, projectId],
    () => {
      const generation = ++latest;
      clearTimeout(timer);
      if (!proposal.value.ok) {
        state.value = { status: "idle" };
        return;
      }
      state.value = { status: "checking" };
      timer = setTimeout(() => void run(generation), SPEC_CHECK_SETTLE_MS);
    },
    { immediate: true },
  );

  return { state };
}
