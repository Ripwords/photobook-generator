/**
 * The user's own layouts, as the webview holds them.
 *
 * **Nothing here decides whether a layout is valid.** `src-tauri/src/custom_layouts.rs`
 * owns every rule and words every message: a save is refused with
 * `CustomLayoutError` `invalid`, and a book that cannot print a layout lists it
 * on the opening's `refused` with the same `Problem`s. The UI shows `message`
 * as it arrives. `check_custom_layout` answers the same question for a shape
 * still being drawn, so an editor can show problems before saving.
 *
 * The library is global: one list in the app database, shared by every book.
 * Changes reach the next command; nothing is cached across commands.
 */

import type { PageSide, PreviewRect } from "./preview";

/** Mirrors `templates::Role`. */
export type LayoutRole = "hero" | "support";

/**
 * Mirrors `custom_layouts::LayoutBox`. `rect` is normalised on the layout's
 * own canvas: the whole spread (fold at x = 0.5) for a spread, the page for a
 * page layout.
 */
export interface LayoutBox {
  rect: PreviewRect;
  role: LayoutRole;
}

/**
 * Mirrors `custom_layouts::LayoutShape`. A page layout prints on one side only:
 * page 1 is a right page, the last page a left one.
 */
export type LayoutShape = { kind: "spread"; boxes: LayoutBox[] } | { kind: "page"; side: PageSide; boxes: LayoutBox[] };

/** Mirrors `custom_layouts::CustomLayout`. */
export interface CustomLayout {
  /** `user-` and 32 hex digits. Never collides with a built-in id. */
  id: string;
  name: string;
  shape: LayoutShape;
  /**
   * "Use in auto layout". Off (the default): only the layout menu and
   * `setTemplate` use it. On: generate, Regenerate, Shuffle and Add photo may pick it.
   */
  auto: boolean;
  /** Unix seconds. */
  createdAt: number;
  updatedAt: number;
}

/**
 * Mirrors `custom_layouts::BuiltinLayout`: a shipped template as the Layouts
 * screen draws it, and the shape a copy of it starts from. Named by
 * `templateLabel(id)`.
 */
export interface BuiltinLayout {
  id: string;
  shape: LayoutShape;
}

/** Mirrors `geometry::BleedEdge`. */
export type BleedEdge = "left" | "right" | "top" | "bottom";

/**
 * Mirrors `custom_layouts::LayoutProblem`. `slot` is a 0-based box index.
 * The first seven are structural and refuse a save; the last three depend on
 * the book's print size and only refuse the layout for that book.
 */
export type LayoutProblem =
  | { kind: "noBoxes" }
  | { kind: "tooManyBoxes"; count: number; max: number }
  | { kind: "notFinite"; slot: number }
  | { kind: "offCanvas"; slot: number }
  | { kind: "tooSmall"; slot: number }
  | { kind: "crossesFold"; slot: number }
  | { kind: "overlap"; slot: number; other: number }
  | { kind: "edgeInBleed"; slot: number; edge: BleedEdge; bleedMm: number }
  | { kind: "edgeInGutter"; slot: number; gutterMm: number }
  | { kind: "mostlyHidden"; slot: number };

/** Mirrors `custom_layouts::Problem`: the reason and Rust's wording of it. */
export type Problem = LayoutProblem & { message: string };

/**
 * Mirrors `templates::RefusedLayout`: a user layout this book cannot print,
 * listed on the opening it would otherwise be offered to.
 */
export interface RefusedLayout {
  templateId: string;
  problems: Problem[];
}

/** Mirrors `custom_layouts::CustomLayoutError`, the rejection of every store command. */
export type CustomLayoutError =
  | { kind: "notFound"; id: string }
  | { kind: "emptyName" }
  | { kind: "nameTooLong"; max: number }
  | { kind: "invalid"; problems: Problem[] }
  | { kind: "storage"; message: string };

/** The one line a store command's rejection reads as. */
export function customLayoutErrorText(error: CustomLayoutError): string {
  switch (error.kind) {
    case "notFound":
      return "That layout no longer exists.";
    case "emptyName":
      return "A layout needs a name.";
    case "nameTooLong":
      return `A layout name can be at most ${error.max} characters.`;
    case "invalid":
      return error.problems.map((p) => p.message).join(" ");
    case "storage":
      return `Could not save layouts: ${error.message}`;
  }
}

/**
 * Whether a problem kind is a print rule: one that refuses the layout only
 * for books whose size cannot print it, and never refuses a save.
 */
export function isPrintRule(kind: LayoutProblem["kind"]): boolean {
  return kind === "edgeInBleed" || kind === "edgeInGutter" || kind === "mostlyHidden";
}

/** The boxes a problem points at, 0-based, so an editor can mark them. */
export function problemSlots(problem: LayoutProblem): number[] {
  switch (problem.kind) {
    case "noBoxes":
    case "tooManyBoxes":
      return [];
    case "overlap":
      return [problem.slot, problem.other];
    default:
      return [problem.slot];
  }
}
