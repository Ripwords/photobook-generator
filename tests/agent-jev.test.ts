import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import {
  INTENTS,
  ROUTE_CONFIDENCE,
  createJev,
  type Intent,
  type JevDecision,
} from "../app/agent/jev";
import { AGENT_TOOLS, tagRanker } from "../app/agent/tools";
import type { AgentPhoto, AgentView } from "../app/agent/view";
import { POISON, leaksIn } from "./helpers/leak";

const VIEW = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/agent-view.json", import.meta.url)), "utf8"),
) as AgentView;

type Fetch = typeof globalThis.fetch;

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });

/** A Jev that answers every request with `reply`, recording each request body. */
function mockJev(reply: () => Response | Promise<Response>) {
  const bodies: unknown[] = [];
  const fetch = vi.fn<Fetch>(async (_url, init) => {
    bodies.push(JSON.parse(String(init?.body)));
    return reply();
  });
  const decisions: JevDecision[] = [];
  const jev = createJev({ fetch, log: (d) => decisions.push(d) });
  return { jev, fetch, bodies, decisions };
}

function choice(id: string, probabilities: Record<string, number>) {
  const [top] = Object.entries(probabilities).toSorted(([, a], [, b]) => b - a);
  return {
    model: "jev-latest",
    answers: {
      [id]: { type: "choice", choice: top?.[0], probabilities, confidence: 0.5 },
    },
    usage: { input_tokens: 1, output_tokens: 1 },
  };
}

function intentAnswer(intent: Intent, p: number) {
  const others = Object.keys(INTENTS).filter((k) => k !== intent);
  const rest = (1 - p) / others.length;
  return choice("intent", {
    [intent]: p,
    ...Object.fromEntries(others.map((k) => [k, rest])),
  });
}

const photo = (id: number, tags: string[]): AgentPhoto => ({
  id,
  day: 0,
  event: 0,
  faces: 0,
  faceArea: 0,
  tags,
  aestheticPct: 50,
  sharpnessPct: 50,
  placed: true,
});

describe("INTENTS", () => {
  it("names only tools the agent has", () => {
    for (const [intent, { tools }] of Object.entries(INTENTS)) {
      expect(tools.length, intent).toBeGreaterThan(0);
      for (const name of tools) expect(Object.keys(AGENT_TOOLS), `${intent}: ${name}`).toContain(name);
    }
  });

  it("gives every tool to some intent, so routing never hides one for good", () => {
    const routed = new Set(Object.values(INTENTS).flatMap(({ tools }) => tools));
    expect([...routed].toSorted()).toEqual(Object.keys(AGENT_TOOLS).toSorted());
  });
});

describe("routeIntent", () => {
  it("narrows the tools to the intent's when Jev is confident", async () => {
    const { jev, bodies } = mockJev(() => json(200, intentAnswer("crop", 0.93)));
    await expect(jev.routeIntent("zoom in on the dog on page 4")).resolves.toEqual(
      INTENTS.crop.tools,
    );
    expect(bodies[0]).toMatchObject({
      model: "jev-latest",
      questions: { intent: { type: "choice" } },
    });
  });

  it("narrows exactly at the threshold and not just below it", async () => {
    const at = mockJev(() => json(200, intentAnswer("lock", ROUTE_CONFIDENCE)));
    await expect(at.jev.routeIntent("keep page 2 as it is")).resolves.toEqual(
      INTENTS.lock.tools,
    );
    const below = mockJev(() => json(200, intentAnswer("lock", ROUTE_CONFIDENCE - 0.01)));
    await expect(below.jev.routeIntent("keep page 2 as it is")).resolves.toBeNull();
  });

  it("asks over every intent, described", async () => {
    const { jev, bodies } = mockJev(() => json(200, intentAnswer("ask", 0.9)));
    await jev.routeIntent("how many pages?");
    const { criteria } = (bodies[0] as { questions: { intent: { criteria: object } } }).questions
      .intent;
    expect(criteria).toEqual(
      Object.fromEntries(Object.entries(INTENTS).map(([k, v]) => [k, v.description])),
    );
  });

  it.each<[string, () => Response | Promise<Response>]>([
    ["a 429", () => json(429, { error: "Rate limit exceeded" })],
    ["a 529", () => json(529, { error: "Overloaded" })],
    ["a malformed body", () => json(200, { answers: { intent: { type: "choice" } } })],
    ["an answer to another question", () => json(200, choice("other", { crop: 1 }))],
    ["an intent Jev made up", () => json(200, choice("intent", { paint: 0.99, crop: 0.01 }))],
    ["a body that is not JSON", () => new Response("<html>", { status: 200 })],
  ])("gives every tool (null) on %s", async (_, reply) => {
    const { jev, decisions } = mockJev(reply);
    await expect(jev.routeIntent("crop page 4")).resolves.toBeNull();
    expect(decisions).toHaveLength(1);
    expect(decisions[0]).toMatchObject({ role: "route", narrowed: false });
  });

  it("gives every tool (null) when there is no key or the request throws", async () => {
    const missing = createJev({
      fetch: () => Promise.reject(new Error("No Jev key is saved.")),
      log: () => {},
    });
    await expect(missing.routeIntent("crop page 4")).resolves.toBeNull();
  });

  it("logs the decision with its probability", async () => {
    const { jev, decisions } = mockJev(() => json(200, intentAnswer("swap", 0.85)));
    await jev.routeIntent("put the beach photo on page 1");
    expect(decisions).toEqual([
      { role: "route", intent: "swap", probability: 0.85, narrowed: true },
    ]);
  });
});

describe("rankPhotos", () => {
  const photos = [photo(3, ["beach"]), photo(7, ["dog", "grass"]), photo(9, ["food"])];

  it("orders by probability and leaves out photos below an even share", async () => {
    const { jev, bodies } = mockJev(() => json(200, choice("photo", { 3: 0.2, 7: 0.7, 9: 0.1 })));
    await expect(jev.rankPhotos("dog", photos)).resolves.toEqual([7]);
    expect(bodies[0]).toMatchObject({ state: "dog", questions: { photo: { type: "choice" } } });
  });

  it("keeps every photo above an even share, best first", async () => {
    const { jev } = mockJev(() => json(200, choice("photo", { 3: 0.45, 7: 0.1, 9: 0.45001 })));
    await expect(jev.rankPhotos("sea and food", photos)).resolves.toEqual([9, 3]);
  });

  it("drops an id Jev was not offered", async () => {
    const { jev } = mockJev(() => json(200, choice("photo", { 42: 0.6, 7: 0.4, 3: 0, 9: 0 })));
    await expect(jev.rankPhotos("dog", photos)).resolves.toEqual([7]);
  });

  it("offers each photo by id, described by its facts", async () => {
    const { jev, bodies } = mockJev(() => json(200, choice("photo", { 3: 1, 7: 0, 9: 0 })));
    await jev.rankPhotos("beach", photos);
    const { criteria } = (bodies[0] as { questions: { photo: { criteria: object } } }).questions
      .photo;
    expect(Object.keys(criteria).toSorted()).toEqual(["3", "7", "9"]);
    expect(JSON.stringify(criteria)).toContain("dog");
  });

  it("falls back to the tag ranker when Jev does not answer", async () => {
    const { jev, decisions } = mockJev(() => json(429, {}));
    await expect(jev.rankPhotos("dog", photos)).resolves.toEqual(await tagRanker("dog", photos));
    expect(decisions[0]).toMatchObject({ role: "rank", answered: false });
  });

  it("does not call Jev for an empty set", async () => {
    const { jev, fetch } = mockJev(() => json(200, {}));
    await expect(jev.rankPhotos("dog", [])).resolves.toEqual([]);
    expect(fetch).not.toHaveBeenCalled();
  });
});

describe("checkProposal", () => {
  const call = { toolName: "set_crop" as const, input: { page: 4, z: 0, x: 0.1, y: 0.1, w: 0.5 } };

  it("returns Jev's probability that the edit does what was asked", async () => {
    const { jev, bodies, decisions } = mockJev(() =>
      json(200, {
        model: "jev-latest",
        answers: { fits: { type: "noul", noul: 0.31 } },
        usage: { input_tokens: 1, output_tokens: 1 },
      }),
    );
    await expect(jev.checkProposal("zoom in on the dog", call)).resolves.toBe(0.31);
    expect(bodies[0]).toMatchObject({
      state: { request: "zoom in on the dog", proposal: { tool: "set_crop", input: call.input } },
      questions: { fits: { type: "noul" } },
    });
    expect(decisions).toEqual([{ role: "check", tool: "set_crop", probability: 0.31 }]);
  });

  it("returns null when Jev does not answer", async () => {
    const { jev } = mockJev(() => json(500, {}));
    await expect(jev.checkProposal("zoom in", call)).resolves.toBeNull();
  });
});

describe("what reaches Jev", () => {
  it("carries only the message and AgentView facts, never a withheld value", async () => {
    const { jev, bodies } = mockJev(() => json(200, {}));
    await jev.routeIntent("make page 4 nicer");
    await jev.rankPhotos("beach", VIEW.photos);
    await jev.checkProposal("make page 4 nicer", { toolName: "shuffle", input: {} });
    expect(bodies).toHaveLength(3);
    expect(leaksIn(bodies)).toEqual([]);
  });

  it("the leak check sees a withheld value in a body", () => {
    expect(leaksIn([{ state: POISON.path }])).not.toEqual([]);
  });
});
