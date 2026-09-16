/**
 * See `core.ts`. There is no native picker in a browser, so it answers with
 * fixed paths -- the folder pick returns two folders, so the "and N more
 * folders" label is exercised rather than only the single-folder case.
 *
 * Set `window.pbgCancelPicker = true` from the console (or from a browser
 * automation tool) to make the next pick resolve to nothing, which is how the
 * cancelled-picker path gets driven.
 */
interface PickerOptions {
  multiple?: boolean;
}

declare global {
  interface Window {
    pbgCancelPicker?: boolean;
  }
}

export async function open(options?: PickerOptions): Promise<string | string[] | null> {
  if (globalThis.window?.pbgCancelPicker) return null;
  return options?.multiple
    ? ["/mock/Pictures/Holiday 2026", "/mock/Pictures/Phone camera roll"]
    : "/mock/Desktop/photobook-export";
}
