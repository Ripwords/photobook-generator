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
  /** Present only on a file pick; the folder pickers pass none. */
  filters?: { name: string; extensions: readonly string[] }[];
}

declare global {
  interface Window {
    pbgCancelPicker?: boolean;
  }
}

export async function open(options?: PickerOptions): Promise<string | string[] | null> {
  if (globalThis.window?.pbgCancelPicker) return null;
  // Only "Add from disk" passes filters, and it wants one file, not a folder.
  if (options?.filters?.length) return "/mock/Desktop/from-disk.jpg";
  return options?.multiple
    ? ["/mock/Pictures/Holiday 2026", "/mock/Pictures/Phone camera roll"]
    : "/mock/Desktop/photobook-export";
}

/** The native yes/no sheet, as the browser's own confirm. */
export async function ask(message: string, options?: { title?: string }): Promise<boolean> {
  return globalThis.window.confirm(options?.title ? `${options.title}\n\n${message}` : message);
}
