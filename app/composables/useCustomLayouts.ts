import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import {
  customLayoutErrorText,
  type BuiltinLayout,
  type CustomLayout,
  type CustomLayoutError,
  type LayoutShape,
  type Problem,
} from "~/types/customLayouts";
import { plainShape } from "~/types/layoutEditor";
import type { PrintSpec } from "~/types/printSpec";

/** A store command's answer: the value, or the one line its rejection reads as. */
export type LayoutResult<T> = { ok: true; value: T } | { ok: false; message: string };

function isLayoutError(e: unknown): e is CustomLayoutError {
  return typeof e === "object" && e !== null && "kind" in e;
}

async function attempt<T>(work: () => Promise<T>): Promise<LayoutResult<T>> {
  try {
    return { ok: true, value: await work() };
  } catch (e) {
    return { ok: false, message: isLayoutError(e) ? customLayoutErrorText(e) : String(e) };
  }
}

// One list for the whole app: the library is global, and the Layouts screen
// and a book's "Save as layout…" must agree on it.
const layouts = ref<CustomLayout[]>([]);
const builtins = ref<BuiltinLayout[]>([]);
/** The size new books get, which the Layouts screen checks against. */
const defaultSpec = ref<PrintSpec | null>(null);

/**
 * The user's layouts and the shipped ones, and the store commands. Every
 * rule lives in Rust (`custom_layouts.rs`); this only calls and relays.
 */
export function useCustomLayouts() {
  async function refresh() {
    const [mine, shipped, spec] = await Promise.all([
      invoke<CustomLayout[]>("list_custom_layouts"),
      invoke<BuiltinLayout[]>("list_builtin_layouts"),
      defaultSpec.value ? Promise.resolve(defaultSpec.value) : invoke<PrintSpec>("default_print_spec"),
    ]);
    layouts.value = mine;
    builtins.value = shipped;
    defaultSpec.value = spec;
  }

  /** Runs a store command and reloads the list after one that changed it. */
  async function change<T>(work: () => Promise<T>): Promise<LayoutResult<T>> {
    const result = await attempt(work);
    if (result.ok) layouts.value = await invoke<CustomLayout[]>("list_custom_layouts");
    return result;
  }

  return {
    layouts,
    builtins,
    defaultSpec,
    refresh,
    create: (name: string, shape: LayoutShape) =>
      change(() => invoke<CustomLayout>("create_custom_layout", { name, shape: plainShape(shape) })),
    update: (id: string, shape: LayoutShape) =>
      change(() => invoke<CustomLayout>("update_custom_layout", { id, shape: plainShape(shape) })),
    rename: (id: string, name: string) => change(() => invoke<CustomLayout>("rename_custom_layout", { id, name })),
    duplicate: (id: string) => change(() => invoke<CustomLayout>("duplicate_custom_layout", { id })),
    remove: (id: string) => change(() => invoke<void>("delete_custom_layout", { id })),
    setAuto: (id: string, auto: boolean) =>
      change(() => invoke<CustomLayout>("set_custom_layout_auto", { id, auto })),
    /** Every problem with a shape, at `spec` when given. Never rejects. */
    check: (shape: LayoutShape, spec: PrintSpec | null) =>
      invoke<Problem[]>("check_custom_layout", spec ? { shape: plainShape(shape), spec: { ...spec } } : { shape: plainShape(shape) }),
  };
}
