import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import {
  AGENT_TOOLS,
  READ_TOOLS,
  WRITE_TOOLS,
  createAgentTools,
  tagRanker,
  toBookEdit,
  type Invoke,
  type PhotoRanker,
  type ToolName,
  type WriteToolName,
} from "../app/agent/tools";
import type { AgentPhoto, AgentView } from "../app/agent/view";
import type { BookEdit } from "../app/types/preview";
import { POISON, WITHHELD, leaksIn } from "./helpers/leak";

function fixture<T>(name: string): T {
  const url = new URL(`./fixtures/wire/${name}`, import.meta.url);
  return JSON.parse(readFileSync(fileURLToPath(url), "utf8")) as T;
}

const VIEW = fixture<AgentView>("agent-view.json");
const EDITS = fixture<BookEdit[]>("book-edits.json");

const PROJECT_ID = 42;

/**
 * A mocked Tauri `invoke`. `agent_view` answers with `view()`; `edit_book`
 * either refuses with `refusal` or answers with a poisoned `BookLayout`
 * stand-in, because the real reply carries file names and must never reach
 * the model.
 */
function mockInvoke(options: { view?: () => AgentView; refusal?: string } = {}) {
  const view = options.view ?? (() => structuredClone(VIEW));
  return vi.fn<Invoke>(async (cmd, args) => {
    if (cmd === "agent_view") return view();
    if (cmd === "edit_book") {
      if (options.refusal !== undefined) throw options.refusal;
      return { pages: [{ placements: [{ filename: POISON.hash, photo: POISON }] }], args };
    }
    throw new Error(`unexpected command ${cmd}`);
  });
}

const OPTIONS = { toolCallId: "call-1", messages: [], context: {} };

async function run(
  tools: ReturnType<typeof createAgentTools>,
  name: ToolName,
  input: unknown,
): Promise<unknown> {
  const parsed: unknown = AGENT_TOOLS[name].input.parse(input);
  const execute = tools[name].execute as (input: unknown, options: typeof OPTIONS) => unknown;
  return await execute(parsed, OPTIONS);
}

/**
 * One valid input per write tool, each chosen to produce one entry of
 * `book-edits.json`. Every number is distinct from its neighbours, so a
 * mapping that swaps two fields or reads the wrong one produces a different
 * edit.
 */
const WRITE_INPUTS: Record<WriteToolName, { input: unknown; kind: BookEdit["kind"] }> = {
  regenerate: { input: { opening: 3 }, kind: "regenerate" },
  reject_layout: { input: { opening: 0 }, kind: "rejectTemplate" },
  set_layout: {
    input: { opening: 2, templateId: "07-two-up-symmetric-margin" },
    kind: "setTemplate",
  },
  set_locked: { input: { opening: 1, locked: true }, kind: "setLocked" },
  shuffle: { input: {}, kind: "shuffle" },
  swap_photos: {
    input: { a: { page: 2, z: 1, photo: 9 }, b: { page: 5, z: 2, photo: 8 } },
    kind: "swapPhotos",
  },
  set_crop: {
    input: { slot: { page: 3, z: 2 }, x: 0.125, y: 0.0, w: 0.75 },
    kind: "setCrop",
  },
};

const ALL_INPUTS: Record<ToolName, unknown> = {
  get_book: {},
  get_opening: { opening: 1 },
  search_photos: { query: "beach" },
  ...Object.fromEntries(Object.entries(WRITE_INPUTS).map(([name, { input }]) => [name, input])),
} as Record<ToolName, unknown>;

describe("the tool table", () => {
  it("splits into read and write tools derived from the table", () => {
    expect(READ_TOOLS.toSorted()).toEqual(["get_book", "get_opening", "search_photos"]);
    expect(WRITE_TOOLS.toSorted()).toEqual(
      [
        "regenerate",
        "reject_layout",
        "set_crop",
        "set_layout",
        "set_locked",
        "shuffle",
        "swap_photos",
      ].toSorted(),
    );
    expect(new Set([...READ_TOOLS, ...WRITE_TOOLS]).size).toBe(Object.keys(AGENT_TOOLS).length);
  });

  it("drives every BookEdit variant except setSlot", () => {
    const driven = Object.values(WRITE_INPUTS).map((w) => w.kind);
    const wire = EDITS.map((e) => e.kind).filter((k) => k !== "setSlot");
    expect(driven.toSorted()).toEqual(wire.toSorted());
  });

  it("describes every tool for the model", () => {
    for (const spec of Object.values(AGENT_TOOLS)) {
      expect(spec.description.length).toBeGreaterThan(40);
    }
  });
});

describe("toEdit", () => {
  for (const name of WRITE_TOOLS) {
    it(`${name} produces its book-edits.json entry exactly`, () => {
      const { input, kind } = WRITE_INPUTS[name];
      expect(toBookEdit(name, input)).toStrictEqual(EDITS.find((e) => e.kind === kind));
    });
  }

  it("sends edit_book exactly the edit, under the project id", async () => {
    const invoke = mockInvoke();
    const tools = createAgentTools(PROJECT_ID, { invoke });
    await run(tools, "swap_photos", WRITE_INPUTS.swap_photos.input);
    expect(invoke).toHaveBeenCalledWith("edit_book", {
      projectId: PROJECT_ID,
      edit: EDITS.find((e) => e.kind === "swapPhotos"),
    });
  });
});

describe("write tool results", () => {
  it("return the fresh view read after the edit, never an acknowledgement", async () => {
    let reads = 0;
    const after: AgentView = { ...structuredClone(VIEW), droppedPhotos: 7 };
    const invoke = mockInvoke({ view: () => (reads++ === 0 ? after : VIEW) });
    const tools = createAgentTools(PROJECT_ID, { invoke });

    const result = await run(tools, "set_locked", { opening: 1, locked: true });

    expect(result).toStrictEqual(after);
    const order = invoke.mock.calls.map(([cmd]) => cmd);
    expect(order).toEqual(["edit_book", "agent_view"]);
  });

  it("return a refused edit's reason with the unchanged view", async () => {
    const reason = "this spread is locked; unlock it to change it";
    const invoke = mockInvoke({ refusal: reason });
    const tools = createAgentTools(PROJECT_ID, { invoke });

    const result = await run(tools, "regenerate", { opening: 0 });

    expect(result).toStrictEqual({ refused: reason, view: VIEW });
    expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual(["edit_book", "agent_view"]);
  });

  it("read a refusal's reason from an Error as well as a string", async () => {
    const invoke = vi.fn<Invoke>(async (cmd) => {
      if (cmd === "edit_book") throw new Error('no template is called "x"');
      return structuredClone(VIEW);
    });
    const tools = createAgentTools(PROJECT_ID, { invoke });

    const result = await run(tools, "set_layout", { opening: 1, templateId: "x" });

    expect(result).toStrictEqual({ refused: 'no template is called "x"', view: VIEW });
  });
});

describe("read tools", () => {
  it("get_book returns the whole view", async () => {
    const tools = createAgentTools(PROJECT_ID, { invoke: mockInvoke() });
    expect(await run(tools, "get_book", {})).toStrictEqual(VIEW);
  });

  it("get_opening returns one opening and the facts of the photos in it", async () => {
    const tools = createAgentTools(PROJECT_ID, { invoke: mockInvoke() });

    const result = await run(tools, "get_opening", { opening: 1 });

    expect(result).toStrictEqual({
      opening: VIEW.openings[1],
      photos: [VIEW.photos[1], VIEW.photos[0]],
    });
  });

  it("get_opening refuses an opening the book does not have, with the view", async () => {
    const tools = createAgentTools(PROJECT_ID, { invoke: mockInvoke() });

    const result = await run(tools, "get_opening", { opening: 3 });

    expect(result).toStrictEqual({ refused: "this book has no opening 3", view: VIEW });
  });
});

function photo(id: number, tags: string[], aestheticPct: number): AgentPhoto {
  return {
    id,
    day: 0,
    event: 0,
    faces: 0,
    faceArea: 0,
    tags,
    aestheticPct,
    sharpnessPct: 50,
    placed: true,
  };
}

describe("search_photos", () => {
  const photos = [
    photo(0, ["food"], 90),
    photo(1, ["beach", "sky"], 20),
    photo(2, ["sky"], 99),
    photo(3, ["beach", "people"], 10),
    photo(4, ["sky", "beach"], 60),
  ];

  it("ranks by how many query words a photo's tags match, then by aesthetic", async () => {
    expect(await tagRanker("beach sky", photos)).toEqual([4, 1, 2, 3]);
  });

  it("matches a plural query word and ignores case and punctuation", async () => {
    expect(await tagRanker("Beaches, PEOPLE!", photos)).toEqual([3, 4, 1]);
  });

  it("finds nothing when no tag matches", async () => {
    expect(await tagRanker("mountain", photos)).toEqual([]);
  });

  it("returns the ranked photos with where each is placed", async () => {
    const tools = createAgentTools(PROJECT_ID, { invoke: mockInvoke() });

    const result = await run(tools, "search_photos", { query: "sky food" });

    expect(result).toStrictEqual({
      photos: [VIEW.photos[0], VIEW.photos[1]],
      slots: [
        { page: 3, z: 1, photo: 0 },
        { page: 2, z: 1, photo: 1 },
      ],
    });
  });

  it("uses an injected ranker, keeps its order, drops unknown ids and applies the limit", async () => {
    const ranker = vi.fn<PhotoRanker>(async () => [3, 99, 1, 3, 0]);
    const tools = createAgentTools(PROJECT_ID, { invoke: mockInvoke(), rankPhotos: ranker });

    const result = await run(tools, "search_photos", { query: "the dog", limit: 2 });

    expect(ranker).toHaveBeenCalledWith("the dog", VIEW.photos);
    expect(result).toStrictEqual({
      photos: [VIEW.photos[3], VIEW.photos[1]],
      slots: [
        { page: 1, z: 1, photo: 3 },
        { page: 2, z: 1, photo: 1 },
      ],
    });
  });
});

describe("the privacy chokepoint", () => {
  it("the leak search finds every withheld value in the poison, so it is live", () => {
    expect(leaksIn(POISON)).toEqual(WITHHELD);
  });

  it("the mocked agent_view response carries no withheld value", () => {
    expect(leaksIn(VIEW)).toEqual([]);
  });

  for (const name of Object.keys(AGENT_TOOLS) as ToolName[]) {
    it(`${name}'s result carries no withheld value, though edit_book's reply does`, async () => {
      const invoke = mockInvoke();
      const tools = createAgentTools(PROJECT_ID, { invoke });

      const result = await run(tools, name, ALL_INPUTS[name]);

      expect(leaksIn(result)).toEqual([]);
      expect(JSON.stringify(result)).toContain('"aestheticPct"');
    });
  }
});

describe("input schemas", () => {
  const bad: [ToolName, unknown][] = [
    ["get_opening", { opening: -1 }],
    ["get_opening", { opening: 1.5 }],
    ["get_opening", { opening: "1" }],
    ["search_photos", { query: "" }],
    ["search_photos", { query: "beach", limit: 0 }],
    ["search_photos", { query: "beach", limit: 51 }],
    ["regenerate", {}],
    ["reject_layout", { opening: -2 }],
    ["set_layout", { opening: 1, templateId: "" }],
    ["set_layout", { opening: 1 }],
    ["set_locked", { opening: 1, locked: "yes" }],
    ["swap_photos", { a: { page: 0, z: 1 }, b: { page: 2, z: 1 } }],
    ["swap_photos", { a: { page: 2, z: -1 }, b: { page: 2, z: 1 } }],
    ["swap_photos", { a: { page: 2, z: 1 } }],
    ["set_crop", { slot: { page: 3, z: 2 }, x: -0.1, y: 0, w: 0.5 }],
    ["set_crop", { slot: { page: 3, z: 2 }, x: 0, y: 1.1, w: 0.5 }],
    ["set_crop", { slot: { page: 3, z: 2 }, x: 0, y: 0, w: 0 }],
    ["set_crop", { slot: { page: 3, z: 2 }, x: 0, y: 0, w: 1.2 }],
    ["set_crop", { slot: { page: 3, z: 2 }, x: 0.5, y: 0, w: 0.75 }],
    ["set_crop", { x: 0, y: 0, w: 0.5 }],
  ];

  for (const [name, input] of bad) {
    it(`${name} rejects ${JSON.stringify(input)}`, () => {
      expect(AGENT_TOOLS[name].input.safeParse(input).success).toBe(false);
    });
  }

  it("accepts every input the other tests use", () => {
    for (const name of Object.keys(AGENT_TOOLS) as ToolName[]) {
      expect(AGENT_TOOLS[name].input.safeParse(ALL_INPUTS[name]).success, name).toBe(true);
    }
  });
});
