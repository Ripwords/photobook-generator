/**
 * The agent's tools: one table, each entry a thin mapping onto a Tauri
 * command.
 *
 * Every result a model reads is built from `agent_view`, the Rust projection
 * that carries no path, hash, GPS, timestamp or face box. A write tool runs
 * `edit_book` and then re-reads `agent_view`; `edit_book`'s own reply is a
 * `BookLayout` with file names in it and is never returned. A refused edit
 * comes back as `{ refused, view }` so the model reads the reason and the
 * unchanged book.
 */
import { tool, type Tool } from "ai";
import { z } from "zod";
import type { BookEdit, PlacementRef } from "~/types/preview";
import type { AgentOpening, AgentPhoto, AgentSlot, AgentView } from "./view";

/** Tauri's `invoke`, narrowed to what the tools call. Injected so tests need no Tauri. */
export type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

/**
 * Orders the photos that match `query`, best first, by `AgentPhoto.id`.
 * Photos that do not match are left out. Phase 6 puts Jev behind this.
 */
export type PhotoRanker = (query: string, photos: readonly AgentPhoto[]) => Promise<number[]>;

/** An edit Rust refused, or an opening the book does not have. */
export interface Refusal {
  refused: string;
  view: AgentView;
}

export interface OpeningResult {
  opening: AgentOpening;
  /** The facts of each photo in the opening, in slot order. */
  photos: AgentPhoto[];
}

export interface SearchResult {
  /** Best match first. */
  photos: AgentPhoto[];
  /** Where each returned photo is placed, in the same order. Unplaced photos have none. */
  slots: AgentSlot[];
}

interface ReadContext {
  view: AgentView;
  rankPhotos: PhotoRanker;
}

interface ReadSpec<S extends z.ZodType, R> {
  description: string;
  input: S;
  writes: false;
  read(input: z.output<S>, ctx: ReadContext): R | Promise<R>;
}

interface WriteSpec<S extends z.ZodType> {
  description: string;
  input: S;
  writes: true;
  /** Method syntax on purpose: it lets the table be read as `WriteSpec<z.ZodType>`. */
  toEdit(input: z.output<S>): BookEdit;
}

const readTool = <S extends z.ZodType, R>(
  spec: Omit<ReadSpec<S, R>, "writes">,
): ReadSpec<S, R> => ({
  ...spec,
  writes: false,
});

const writeTool = <S extends z.ZodType>(spec: Omit<WriteSpec<S>, "writes">): WriteSpec<S> => ({
  ...spec,
  writes: true,
});

const openingIndex = z
  .int()
  .min(0)
  .describe(
    "An opening's `index` from get_book: 0 is page 1, then each spread, then the last page.",
  );

const slotRef = z
  .object({
    page: z.int().min(1).describe("The slot's printed page number."),
    z: z.int().min(0).describe("The slot's `z` on that page."),
  })
  .describe("A slot, as get_book lists it under an opening's `slots`.");

const placement = (slot: z.output<typeof slotRef>): PlacementRef => ({
  page: slot.page,
  z: slot.z,
});

function openingOf(view: AgentView, index: number): AgentOpening | Refusal {
  return (
    view.openings.find((o) => o.index === index) ?? {
      refused: `this book has no opening ${index}`,
      view,
    }
  );
}

export const AGENT_TOOLS = {
  get_book: readTool({
    description:
      "Read the whole book: every opening (its pages, template, lock, alternative templates " +
      "and which photo is in which slot) and every photo's facts (day, event, faces, tags, " +
      "aesthetic and sharpness percentiles within this book, and whether the book placed it).",
    input: z.object({}),
    read: (_input, { view }): AgentView => view,
  }),

  get_opening: readTool({
    description:
      "Read one opening and the facts of the photos in it, in slot order. Cheaper than " +
      "get_book when you already know which opening you mean.",
    input: z.object({ opening: openingIndex }),
    read: ({ opening }, { view }): OpeningResult | Refusal => {
      const found = openingOf(view, opening);
      if ("refused" in found) return found;
      const photos = found.slots.flatMap((s) => view.photos.filter((p) => p.id === s.photo));
      return { opening: found, photos };
    },
  }),

  search_photos: readTool({
    description:
      "Find photos by what is in them, e.g. 'beach', 'dog', 'food'. Returns the matching " +
      "photos best first, and the slot each placed one sits in, ready for swap_photos.",
    input: z.object({
      query: z.string().trim().min(1).max(200).describe("What to look for, in plain words."),
      limit: z.int().min(1).max(50).default(10).describe("At most this many photos."),
    }),
    read: async ({ query, limit }, { view, rankPhotos }): Promise<SearchResult> => {
      const byId = new Map(view.photos.map((p) => [p.id, p]));
      const ids = [...new Set(await rankPhotos(query, view.photos))].filter((id) => byId.has(id));
      const photos = ids.slice(0, limit).flatMap((id) => byId.get(id) ?? []);
      const slots = view.openings.flatMap((o) => o.slots);
      return {
        photos,
        slots: photos.flatMap((p) => slots.filter((s) => s.photo === p.id)),
      };
    },
  }),

  regenerate: writeTool({
    description:
      "Lay an opening's photos out on a different template: the best one it has not shown. " +
      "Refused when the opening is locked or has no alternatives left.",
    input: z.object({ opening: openingIndex }),
    toEdit: ({ opening }) => ({ kind: "regenerate", opening }),
  }),

  reject_layout: writeTool({
    description:
      "Never use this opening's current template again, and lay it out on the best remaining " +
      "one. Use when the user dislikes a layout, not just wants to see another.",
    input: z.object({ opening: openingIndex }),
    toEdit: ({ opening }) => ({ kind: "rejectTemplate", opening }),
  }),

  set_layout: writeTool({
    description:
      "Lay an opening's photos out on exactly this template. Pick one of the opening's " +
      "`alternatives` from get_book; any other id is refused.",
    input: z.object({
      opening: openingIndex,
      templateId: z.string().min(1).describe("A template id from the opening's `alternatives`."),
    }),
    toEdit: ({ opening, templateId }) => ({ kind: "setTemplate", opening, templateId }),
  }),

  set_locked: writeTool({
    description:
      "Lock or unlock an opening. A locked opening is skipped by shuffle and refuses every " +
      "other edit until it is unlocked.",
    input: z.object({
      opening: openingIndex,
      locked: z.boolean().describe("true to lock, false to unlock."),
    }),
    toEdit: ({ opening, locked }) => ({ kind: "setLocked", opening, locked }),
  }),

  shuffle: writeTool({
    description:
      "Lay out every unlocked opening that has an alternative on a different template. " +
      "Locked openings stay as they are.",
    input: z.object({}),
    toEdit: () => ({ kind: "shuffle" }),
  }),

  swap_photos: writeTool({
    description:
      "Exchange the photos in two slots, anywhere in the book. Refused when either photo " +
      "would have a face cut, sit in the gutter or margin, or print below 200 DPI.",
    input: z.object({ a: slotRef, b: slotRef }),
    toEdit: ({ a, b }) => ({ kind: "swapPhotos", a: placement(a), b: placement(b) }),
  }),

  set_crop: writeTool({
    description:
      "Move or resize the crop window of the photo in one slot. x, y and w are fractions of " +
      "the photo's width and height, from its top-left; the height follows from the slot's " +
      "shape. Refused when the crop would leave the photo or cut a face.",
    input: z
      .object({
        slot: slotRef,
        x: z.number().min(0).max(1).describe("Left edge, as a fraction of the photo's width."),
        y: z.number().min(0).max(1).describe("Top edge, as a fraction of the photo's height."),
        w: z.number().gt(0).max(1).describe("Width, as a fraction of the photo's width."),
      })
      .refine(({ x, w }) => x + w <= 1, { message: "x + w must not pass the photo's right edge" }),
    toEdit: ({ slot, x, y, w }) => ({ kind: "setCrop", placement: placement(slot), x, y, w }),
  }),
};

type Table = typeof AGENT_TOOLS;
export type ToolName = keyof Table;
export type WriteToolName = {
  [K in ToolName]: Table[K]["writes"] extends true ? K : never;
}[ToolName];
export type ReadToolName = Exclude<ToolName, WriteToolName>;

const NAMES = Object.keys(AGENT_TOOLS) as ToolName[];
const isWrite = (name: ToolName): name is WriteToolName => AGENT_TOOLS[name].writes;

export const WRITE_TOOLS: readonly WriteToolName[] = NAMES.filter(isWrite);
export const READ_TOOLS: readonly ReadToolName[] = NAMES.filter(
  (name): name is ReadToolName => !isWrite(name),
);

/** Parses a write tool's input and maps it onto the `BookEdit` it sends. */
export function toBookEdit(name: WriteToolName, input: unknown): BookEdit {
  const spec: WriteSpec<z.ZodType> = AGENT_TOOLS[name];
  return spec.toEdit(spec.input.parse(input));
}

const WORD = /[^a-z0-9]+/;

function words(text: string): string[] {
  return text.toLowerCase().split(WORD).filter(Boolean);
}

/**
 * The ranker with no model behind it: a photo scores one per distinct query
 * word that one of its tags contains as a word (a plural `s`/`es` on the query
 * word still matches). Ties go to the higher aesthetic percentile, then the
 * lower id.
 */
export const tagRanker: PhotoRanker = async (query, photos) => {
  const terms = [...new Set(words(query))];
  const matches = (photo: AgentPhoto) => {
    const tagWords = new Set(photo.tags.flatMap(words));
    return terms.filter((t) =>
      [t, t.replace(/s$/, ""), t.replace(/es$/, "")].some((form) => tagWords.has(form)),
    ).length;
  };
  return photos
    .map((photo) => ({ photo, score: matches(photo) }))
    .filter(({ score }) => score > 0)
    .toSorted(
      (a, b) =>
        b.score - a.score || b.photo.aestheticPct - a.photo.aestheticPct || a.photo.id - b.photo.id,
    )
    .map(({ photo }) => photo.id);
};

export type WriteResult = AgentView | Refusal;

type AgentToolOf<T> =
  T extends ReadSpec<infer S, infer R>
    ? Tool<z.output<S>, Awaited<R>>
    : T extends WriteSpec<infer S>
      ? Tool<z.output<S>, WriteResult>
      : never;

export type AgentToolSet = { [K in ToolName]: AgentToolOf<Table[K]> };

export interface AgentToolDeps {
  invoke: Invoke;
  /** Defaults to `tagRanker`. */
  rankPhotos?: PhotoRanker;
}

/** A refusal from `edit_book` arrives as the string Rust wrote, or wrapped in an `Error`. */
function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The AI SDK tool set for one project. Approval is not wired here: phase 7
 * sets `toolApproval` on the agent from `WRITE_TOOLS`.
 */
export function createAgentTools(projectId: number, deps: AgentToolDeps): AgentToolSet {
  const { invoke, rankPhotos = tagRanker } = deps;
  const readView = async () => (await invoke("agent_view", { projectId })) as AgentView;

  const build = (spec: ReadSpec<z.ZodType, unknown> | WriteSpec<z.ZodType>) =>
    tool({
      description: spec.description,
      inputSchema: spec.input,
      execute: async (input: unknown) => {
        if (!spec.writes) return spec.read(input, { view: await readView(), rankPhotos });
        try {
          await invoke("edit_book", { projectId, edit: spec.toEdit(input) });
        } catch (error) {
          return { refused: reasonOf(error), view: await readView() } satisfies Refusal;
        }
        return await readView();
      },
    });

  return Object.fromEntries(NAMES.map((name) => [name, build(AGENT_TOOLS[name])])) as AgentToolSet;
}
