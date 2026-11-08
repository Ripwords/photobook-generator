/**
 * The user-layout commands for the browser harness: an in-memory list seeded
 * from `custom-layouts.json`, behaving like `custom_layouts.rs` in what the UI
 * can see. Ids are `user-` plus 32 hex digits, auto is off on create and on
 * duplicate, names are trimmed and capped, and rejections are the same
 * structured `CustomLayoutError`s.
 *
 * Only two structural rules are mocked, `noBoxes` and `tooManyBoxes`. Rust
 * owns the rest and the print rules. A screen that needs to show a refused
 * box can take one from the fixture's `problems`.
 */
import fixture from "../../tests/fixtures/wire/custom-layouts.json";
import type { CustomLayout, CustomLayoutError, LayoutShape, Problem } from "../../app/types/customLayouts";

const MAX_BOXES = 8;
const MAX_NAME_CHARS = 60;

const layouts: CustomLayout[] = structuredClone(fixture.layouts) as CustomLayout[];

type Args = Record<string, unknown> | undefined;

function newId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return `user-${Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("")}`;
}

function now(): number {
  return Math.floor(Date.now() / 1000);
}

function structural(shape: LayoutShape): Problem[] {
  const count = shape.boxes.length;
  if (count === 0) return [{ kind: "noBoxes", message: "A layout needs at least one photo box." }];
  if (count > MAX_BOXES) {
    return [
      {
        kind: "tooManyBoxes",
        count,
        max: MAX_BOXES,
        message: `A layout can hold at most ${MAX_BOXES} photo boxes; this one has ${count}.`,
      },
    ];
  }
  return [];
}

function fail(error: CustomLayoutError): never {
  throw error;
}

function validName(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) fail({ kind: "emptyName" });
  if ([...trimmed].length > MAX_NAME_CHARS) fail({ kind: "nameTooLong", max: MAX_NAME_CHARS });
  return trimmed;
}

function validShape(shape: LayoutShape): LayoutShape {
  const problems = structural(shape);
  if (problems.length > 0) fail({ kind: "invalid", problems });
  return structuredClone(shape);
}

function find(id: string): CustomLayout {
  return layouts.find((l) => l.id === id) ?? fail({ kind: "notFound", id });
}

function changed(id: string, change: (layout: CustomLayout) => void): CustomLayout {
  const layout = find(id);
  change(layout);
  layout.updatedAt = now();
  return structuredClone(layout);
}

/** Answers one user-layout command, or `undefined` for any other command. */
export function layoutCommand(command: string, args: Args): { value: unknown } | undefined {
  const id = String(args?.id ?? "");
  switch (command) {
    case "list_custom_layouts":
      return { value: structuredClone(layouts) };
    case "create_custom_layout": {
      const name = validName(String(args?.name ?? ""));
      const shape = validShape(args?.shape as LayoutShape);
      const at = now();
      const layout: CustomLayout = { id: newId(), name, shape, auto: false, createdAt: at, updatedAt: at };
      layouts.push(layout);
      return { value: structuredClone(layout) };
    }
    case "update_custom_layout": {
      const shape = validShape(args?.shape as LayoutShape);
      return { value: changed(id, (l) => (l.shape = shape)) };
    }
    case "rename_custom_layout": {
      const name = validName(String(args?.name ?? ""));
      return { value: changed(id, (l) => (l.name = name)) };
    }
    case "set_custom_layout_auto":
      return { value: changed(id, (l) => (l.auto = Boolean(args?.auto))) };
    case "duplicate_custom_layout": {
      const source = find(id);
      const at = now();
      const name = `${[...source.name].slice(0, MAX_NAME_CHARS - " copy".length).join("")} copy`;
      const copy: CustomLayout = { ...structuredClone(source), id: newId(), name, auto: false, createdAt: at, updatedAt: at };
      layouts.push(copy);
      return { value: structuredClone(copy) };
    }
    case "delete_custom_layout": {
      find(id);
      layouts.splice(
        layouts.findIndex((l) => l.id === id),
        1,
      );
      return { value: undefined };
    }
    case "check_custom_layout":
      return { value: structural(args?.shape as LayoutShape) };
    default:
      return undefined;
  }
}
