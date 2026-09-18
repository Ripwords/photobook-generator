import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type {
  LanguageModelV4CallOptions,
  LanguageModelV4GenerateResult,
  LanguageModelV4StreamPart,
} from "@ai-sdk/provider";
import type { ModelMessage, UIMessage } from "ai";
import { createDeepSeek } from "@ai-sdk/deepseek";
import { convertArrayToReadableStream, MockLanguageModelV4 } from "ai/test";
import { describe, expect, it, vi } from "vitest";
import {
  CHECK_WARN_BELOW,
  MODEL_ID,
  createBookAgent,
  lastUserText,
  pruneHistory,
  savedEdits,
} from "../app/agent/agent";
import { INTENTS, type Jev } from "../app/agent/jev";
import { AGENT_TOOLS, tagRanker, type Invoke } from "../app/agent/tools";
import { DEEPSEEK_TEXT_REPLY, deepseekStream } from "../dev/tauri-mock/model";
import type { AgentView } from "../app/agent/view";

const VIEW = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/agent-view.json", import.meta.url)), "utf8"),
) as AgentView;

const usage = {
  inputTokens: { total: 10, noCache: 10, cacheRead: 0, cacheWrite: 0 },
  outputTokens: { total: 5, text: 5, reasoning: 0 },
};

type Result = LanguageModelV4GenerateResult;

const toolCall = (toolName: string, input: unknown, toolCallId = "call-1"): Result => ({
  content: [
    {
      type: "tool-call",
      toolCallId,
      toolName,
      input: typeof input === "string" ? input : JSON.stringify(input),
    },
  ],
  finishReason: { unified: "tool-calls", raw: "tool_calls" },
  usage,
  warnings: [],
});

const text = (value: string): Result => ({
  content: value === "" ? [] : [{ type: "text", text: value }],
  finishReason: { unified: "stop", raw: "stop" },
  usage,
  warnings: [],
});

/** A model that plays `results` in order, then repeats the last one. */
function scripted(...results: Result[]) {
  let turn = 0;
  return new MockLanguageModelV4({
    doGenerate: async () => results[Math.min(turn++, results.length - 1)]!,
  });
}

/** The same answers as a stream, the way `DirectChatTransport` calls the model. */
function streamed(...results: Result[]) {
  let turn = 0;
  return new MockLanguageModelV4({
    doStream: async () => {
      const result = results[Math.min(turn++, results.length - 1)]!;
      const parts: LanguageModelV4StreamPart[] = [{ type: "stream-start", warnings: [] }];
      for (const part of result.content) {
        if (part.type === "text") {
          parts.push({ type: "text-start", id: "t" });
          parts.push({ type: "text-delta", id: "t", delta: part.text });
          parts.push({ type: "text-end", id: "t" });
        } else if (part.type === "tool-call") {
          parts.push(part);
        }
      }
      parts.push({ type: "finish", finishReason: result.finishReason, usage });
      return { stream: convertArrayToReadableStream(parts) };
    },
  });
}

function mockInvoke() {
  return vi.fn<Invoke>(async () => structuredClone(VIEW));
}

const quietJev = (overrides: Partial<Jev> = {}): Jev => ({
  routeIntent: async () => null,
  rankPhotos: tagRanker,
  checkProposal: async () => null,
  ...overrides,
});

const user = (content: string): ModelMessage => ({ role: "user", content });

const SWAP = { a: { page: 1, z: 0 }, b: { page: 2, z: 0 } };

const commands = (invoke: ReturnType<typeof mockInvoke>) => invoke.mock.calls.map(([cmd]) => cmd);

const toolNames = (call: LanguageModelV4CallOptions | undefined) =>
  (call?.tools ?? []).map((t) => t.name).toSorted();

describe("approval", () => {
  it("stops a write for approval and does not call agent_edit", async () => {
    const invoke = mockInvoke();
    const model = scripted(toolCall("swap_photos", SWAP), text("done"));
    const agent = createBookAgent(1, { model, invoke, jev: quietJev() });

    const result = await agent.generate({ messages: [user("swap pages 1 and 2")] });

    expect(result.content.filter((p) => p.type === "tool-approval-request")).toHaveLength(1);
    expect(commands(invoke)).not.toContain("agent_edit");
    expect(model.doGenerateCalls).toHaveLength(1);
  });

  async function answered(approved: boolean, jev = quietJev()) {
    const invoke = mockInvoke();
    const model = scripted(toolCall("swap_photos", SWAP), text("done"));
    const agent = createBookAgent(1, { model, invoke, jev });
    const first = await agent.generate({ messages: [user("swap pages 1 and 2")] });
    const request = first.content.find((p) => p.type === "tool-approval-request");
    if (!request) throw new Error("no approval request");
    await agent.generate({
      messages: [
        user("swap pages 1 and 2"),
        ...first.response.messages,
        {
          role: "tool",
          content: [{ type: "tool-approval-response", approvalId: request.approvalId, approved }],
        },
      ],
    });
    return { invoke, model };
  }

  it("runs the edit once approved", async () => {
    const { invoke } = await answered(true);
    expect(invoke).toHaveBeenCalledWith("agent_edit", {
      projectId: 1,
      edit: { kind: "swapPhotos", ...SWAP },
    });
  });

  it("never runs a denied edit", async () => {
    const { invoke } = await answered(false);
    expect(commands(invoke)).not.toContain("agent_edit");
  });

  // Resuming, the SDK asks toolApproval again about every approved call, and
  // restarts at step 0 with no new words from the user.
  it("asks Jev once per proposal and routes once per user message, across an approval", async () => {
    const routeIntent = vi.fn(async () => INTENTS.swap.tools);
    const checkProposal = vi.fn(async () => 0.9);
    const { invoke, model } = await answered(true, quietJev({ routeIntent, checkProposal }));
    expect(commands(invoke)).toContain("agent_edit");
    expect(checkProposal).toHaveBeenCalledOnce();
    expect(routeIntent).toHaveBeenCalledOnce();
    expect(toolNames(model.doGenerateCalls[1])).toEqual(Object.keys(AGENT_TOOLS).toSorted());
  });

  it("still checks a new proposal after an earlier one was answered", async () => {
    const checkProposal = vi.fn(async () => 0.9);
    const model = scripted(
      toolCall("swap_photos", SWAP, "call-1"),
      text("done"),
      toolCall("swap_photos", SWAP, "call-2"),
    );
    const agent = createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev({ checkProposal }) });
    const first = await agent.generate({ messages: [user("swap pages 1 and 2")] });
    const request = first.content.find((p) => p.type === "tool-approval-request");
    if (!request) throw new Error("no approval request");
    const history: ModelMessage[] = [
      user("swap pages 1 and 2"),
      ...first.response.messages,
      { role: "tool", content: [{ type: "tool-approval-response", approvalId: request.approvalId, approved: true }] },
    ];
    const second = await agent.generate({ messages: history });
    await agent.generate({
      messages: [...history, ...second.response.messages, user("now swap them back")],
    });
    expect(checkProposal.mock.calls.map(([words]) => words)).toEqual([
      "swap pages 1 and 2",
      "now swap them back",
    ]);
  });

  it("runs a read without asking", async () => {
    const invoke = mockInvoke();
    const agent = createBookAgent(1, {
      model: scripted(toolCall("get_book", {}), text("12 pages")),
      invoke,
      jev: quietJev(),
    });
    const result = await agent.generate({ messages: [user("how long is it?")] });
    expect(result.content.some((p) => p.type === "tool-approval-request")).toBe(false);
    expect(commands(invoke)).toEqual(["agent_view"]);
    expect(result.text).toBe("12 pages");
  });

  it.each([
    [0.1, true],
    [CHECK_WARN_BELOW - 0.01, true],
    [CHECK_WARN_BELOW, false],
    [0.95, false],
    [null, false],
  ])("with Jev's check at %s, warns the approver: %s", async (probability, warns) => {
    const checkProposal = vi.fn(async () => probability);
    const agent = createBookAgent(1, {
      model: scripted(toolCall("swap_photos", SWAP)),
      invoke: mockInvoke(),
      jev: quietJev({ checkProposal }),
    });
    const result = await agent.generate({ messages: [user("swap pages 1 and 2")] });
    const request = result.content.find((p) => p.type === "tool-approval-request");
    expect(request?.reason !== undefined).toBe(warns);
    expect(checkProposal).toHaveBeenCalledWith("swap pages 1 and 2", {
      toolName: "swap_photos",
      input: SWAP,
    });
  });
});

describe("the loop", () => {
  it("stops after 20 steps", async () => {
    const model = scripted(toolCall("get_book", {}));
    const agent = createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() });
    await agent.generate({ messages: [user("keep looking")] });
    expect(model.doGenerateCalls).toHaveLength(20);
  });

  it("turns DeepSeek's thinking off", async () => {
    const model = scripted(text("hi"));
    await createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() }).generate({
      messages: [user("hi")],
    });
    expect(model.doGenerateCalls[0]?.providerOptions).toMatchObject({
      deepseek: { thinking: { type: "disabled" } },
    });
  });
});

describe("routing", () => {
  it("offers only the routed tools on the first step, then every tool", async () => {
    const routeIntent = vi.fn(async () => INTENTS.crop.tools);
    const model = scripted(toolCall("get_book", {}), text("done"));
    const agent = createBookAgent(1, {
      model,
      invoke: mockInvoke(),
      jev: quietJev({ routeIntent }),
    });
    await agent.generate({ messages: [user("zoom in on the dog")] });
    expect(routeIntent).toHaveBeenCalledWith("zoom in on the dog");
    expect(toolNames(model.doGenerateCalls[0])).toEqual([...INTENTS.crop.tools].toSorted());
    expect(toolNames(model.doGenerateCalls[1])).toEqual(Object.keys(AGENT_TOOLS).toSorted());
  });

  it("offers every tool when Jev gives no route", async () => {
    const model = scripted(text("done"));
    await createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() }).generate({
      messages: [user("hello")],
    });
    expect(toolNames(model.doGenerateCalls[0])).toEqual(Object.keys(AGENT_TOOLS).toSorted());
  });
});

describe("repair", () => {
  it("recovers a call whose input came wrapped in a code fence", async () => {
    const invoke = mockInvoke();
    const agent = createBookAgent(1, {
      model: scripted(toolCall("get_opening", '```json\n{"opening": 1}\n```'), text("ok")),
      invoke,
      jev: quietJev(),
    });
    await agent.generate({ messages: [user("what is on opening 1?")] });
    expect(commands(invoke)).toEqual(["agent_view"]);
  });

  it("gives up on input that still does not fit the tool", async () => {
    const invoke = mockInvoke();
    const agent = createBookAgent(1, {
      model: scripted(toolCall("get_opening", '{"opening": "first"}'), text("sorry")),
      invoke,
      jev: quietJev(),
    });
    await agent.generate({ messages: [user("what is on the first opening?")] });
    expect(commands(invoke)).toEqual([]);
  });
});

describe("an empty response", () => {
  it("is retried once when generating", async () => {
    const model = scripted(text(""), text("hello"));
    const result = await createBookAgent(1, {
      model,
      invoke: mockInvoke(),
      jev: quietJev(),
    }).generate({ messages: [user("hi")] });
    expect(result.text).toBe("hello");
    expect(model.doGenerateCalls).toHaveLength(2);
  });

  it("is retried once when streaming", async () => {
    const model = streamed(text(""), text("hello"));
    const result = await createBookAgent(1, {
      model,
      invoke: mockInvoke(),
      jev: quietJev(),
    }).stream({ messages: [user("hi")] });
    expect(await result.text).toBe("hello");
    expect(model.doStreamCalls).toHaveLength(2);
  });

  it("is not retried twice", async () => {
    const model = scripted(text(""));
    await createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() }).generate({
      messages: [user("hi")],
    });
    expect(model.doGenerateCalls).toHaveLength(2);
  });

  it("does not retry a response that has content", async () => {
    const model = streamed(text("hello"));
    const result = await createBookAgent(1, {
      model,
      invoke: mockInvoke(),
      jev: quietJev(),
    }).stream({ messages: [user("hi")] });
    expect(await result.text).toBe("hello");
    expect(model.doStreamCalls).toHaveLength(1);
  });
});

  const results = (messages: ModelMessage[]) =>
    messages.flatMap((m) =>
      m.role === "tool" ? m.content.filter((p) => p.type === "tool-result") : [],
    );
  const reasoning = (messages: ModelMessage[]) =>
    messages.flatMap((m) =>
      m.role === "assistant" && typeof m.content !== "string"
        ? m.content.filter((p) => p.type === "reasoning").map((p) => p.text)
        : [],
    );
  const users = (messages: ModelMessage[]) => messages.filter((m) => m.role === "user");

describe("pruneHistory", () => {
  const round = (n: number): ModelMessage[] => [
    user(`request ${n}`),
    {
      role: "assistant",
      content: [
        { type: "reasoning", text: `thinking ${n}` },
        { type: "tool-call", toolCallId: `c${n}`, toolName: "get_book", input: {} },
      ],
    },
    {
      role: "tool",
      content: [
        {
          type: "tool-result",
          toolCallId: `c${n}`,
          toolName: "get_book",
          output: { type: "json", value: { round: n } },
        },
      ],
    },
    { role: "assistant", content: [{ type: "text", text: `answer ${n}` }] },
  ];
  const history = [1, 2, 3, 4, 5].flatMap(round);

  it("drops old tool results", () => {
    expect(results(pruneHistory(history)).length).toBeLessThan(results(history).length);
    expect(results(pruneHistory(history)).length).toBeGreaterThan(0);
  });

  it("keeps the reasoning on every assistant turn", () => {
    expect(reasoning(pruneHistory(history))).toEqual(reasoning(history));
  });

  it("keeps every user message", () => {
    expect(users(pruneHistory(history))).toEqual(users(history));
  });
});

describe("lastUserText", () => {
  it("reads the latest user message, in parts or as a string", () => {
    expect(
      lastUserText([
        user("first"),
        { role: "assistant", content: "ok" },
        { role: "user", content: [{ type: "text", text: "second" }] },
        { role: "tool", content: [] },
      ]),
    ).toBe("second");
    expect(lastUserText([])).toBe("");
  });
});

type Part = UIMessage["parts"][number];
const part = (name: string, state: string, output?: unknown): Part =>
  ({ type: `tool-${name}`, toolCallId: `${name}-${state}`, state, input: {}, output }) as Part;
const message = (...parts: Part[]): UIMessage[] => [{ id: "m", role: "assistant", parts }];

describe("savedEdits", () => {

  it("names each write whose output is the saved book", () => {
    expect(
      savedEdits(
        message(
          part("swap_photos", "output-available", VIEW),
          part("set_crop", "output-available", VIEW),
        ),
      ),
    ).toEqual(["swap_photos-output-available", "set_crop-output-available"]);
  });

  it.each([
    ["a refusal", part("swap_photos", "output-available", { refused: "no", view: VIEW })],
    ["a read", part("get_book", "output-available", VIEW)],
    ["a write awaiting approval", part("swap_photos", "approval-requested")],
    ["a denied write", part("swap_photos", "output-denied")],
    ["a failed write", part("swap_photos", "output-error")],
  ])("leaves out %s", (_, p) => {
    expect(savedEdits(message(p))).toEqual([]);
  });
});

function deepseek(...responses: ReturnType<typeof deepseekStream>[]) {
  const bodies: unknown[] = [];
  let turn = 0;
  const fetch = vi.fn<typeof globalThis.fetch>(async (_url, init) => {
    bodies.push(JSON.parse(String(init?.body)));
    const { status, headers, body } = responses[Math.min(turn++, responses.length - 1)]!;
    return new Response(body, { status, headers });
  });
  const model = createDeepSeek({ apiKey: "test", fetch })(MODEL_ID);
  return { model, fetch, bodies };
}

describe("over the DeepSeek provider", () => {
  const reply = deepseekStream([{ role: "assistant", content: DEEPSEEK_TEXT_REPLY }], "stop", 12);
  const empty = deepseekStream([{ role: "assistant", content: "" }], "stop", 0);
  const swap = deepseekStream(
    [
      {
        role: "assistant",
        tool_calls: [
          {
            index: 0,
            id: "call-1",
            type: "function",
            function: { name: "swap_photos", arguments: JSON.stringify(SWAP) },
          },
        ],
      },
    ],
    "tool_calls",
    9,
  );


  it("streams a reply", async () => {
    const { model } = deepseek(reply);
    const agent = createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() });
    const result = await agent.stream({ messages: [user("make chapter 2 calmer")] });
    expect(await result.text).toBe(DEEPSEEK_TEXT_REPLY);
  });

  it("retries an empty reply once", async () => {
    const { model, fetch } = deepseek(empty, reply);
    const agent = createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() });
    const result = await agent.stream({ messages: [user("hi")] });
    expect(await result.text).toBe(DEEPSEEK_TEXT_REPLY);
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("holds a streamed write for approval", async () => {
    const invoke = mockInvoke();
    const { model } = deepseek(swap, reply);
    const agent = createBookAgent(1, { model, invoke, jev: quietJev() });
    const result = await agent.stream({ messages: [user("swap pages 1 and 2")] });
    const content = await result.content;
    expect(content.filter((p) => p.type === "tool-approval-request")).toHaveLength(1);
    expect(commands(invoke)).not.toContain("agent_edit");
  });

  it("asks for deepseek-flash, the name DeepSeek serves V4.1 Flash under", async () => {
    const { model, bodies } = deepseek(reply);
    await (
      await createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() }).stream({
        messages: [user("hi")],
      })
    ).text;
    expect(bodies[0]).toMatchObject({ model: "deepseek-flash" });
  });

  it("sends DeepSeek thinking disabled", async () => {
    const { model, bodies } = deepseek(reply);
    await (
      await createBookAgent(1, { model, invoke: mockInvoke(), jev: quietJev() }).stream({
        messages: [user("hi")],
      })
    ).text;
    expect(bodies[0]).toMatchObject({ thinking: { type: "disabled" } });
  });
});
