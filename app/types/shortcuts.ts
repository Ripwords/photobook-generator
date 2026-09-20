/**
 * Every keyboard shortcut, in one table. The bindings, the tooltips' key
 * hints and the list in Settings all read it, so none of them can drift
 * from what the keys actually do.
 *
 * `keys` is in the form `UKbd` renders and `defineShortcuts` parses.
 */
export const SHORTCUTS = {
  newBook: { keys: ["meta", "N"], label: "New photobook", where: "Anywhere" },
  sidebar: { keys: ["meta", "B"], label: "Show or hide the sidebar", where: "Anywhere" },
  settings: { keys: ["meta", ","], label: "Settings", where: "Anywhere" },
  undo: { keys: ["meta", "Z"], label: "Undo the last change", where: "Book editor" },
  redo: { keys: ["meta", "shift", "Z"], label: "Redo the last undone change", where: "Book editor" },
  chat: { keys: ["meta", "J"], label: "Show or hide the chat", where: "Book editor" },
  export: { keys: ["meta", "E"], label: "Export", where: "Book editor" },
} as const satisfies Record<string, { keys: readonly string[]; label: string; where: string }>;

export type ShortcutId = keyof typeof SHORTCUTS;

/** The shortcut as a `defineShortcuts` key, e.g. `meta_n`. */
export function shortcutCombo(id: ShortcutId): string {
  return SHORTCUTS[id].keys.join("_").toLowerCase();
}

/** The shortcut's keys, as a `UTooltip`'s `kbds`. */
export function shortcutKbds(id: ShortcutId): string[] {
  return [...SHORTCUTS[id].keys];
}
