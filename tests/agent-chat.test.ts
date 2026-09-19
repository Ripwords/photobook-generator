import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createDeepSeek } from "@ai-sdk/deepseek";
import { readUIMessageStream, type UIMessage, type UIMessageChunk } from "ai";
import { describe, expect, it, vi } from "vitest";
import { createBookAgent, MODEL_ID } from "../app/agent/agent";
import {
  NO_DEEPSEEK_KEY,
  chatErrorText,
  describeEdit,
  messageBlocks,
  type ProposalBlock,
} from "../app/agent/chat";
import { MissingKeyError } from "../app/agent/fetch";
import { createJev, type Jev } from "../app/agent/jev";
import { tagRanker, type Invoke, type WriteToolName } from "../app/agent/tools";
import type { AgentView } from "../app/agent/view";
import {
  DEEPSEEK_TEXT_REPLY,
  DEEPSEEK_THINKING,
  deepseekStream,
  scriptedResponse,
} from "../dev/tauri-mock/model";
import type { ModelProvider } from "../app/agent/fetch";

const VIEW = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/agent-view.json", import.meta.url)), "utf8"),
) as AgentView;

const label = (opening: number) => `Pages ${opening * 2}–${opening * 2 + 1}`;

type Part = UIMessage["parts"][number];

const assistant = (...parts: Part[]): UIMessage => ({ id: "m", role: "assistant", parts });

const write = (
  name: WriteToolName,
  { state, ...fields }: { state: string } & Record<string, unknown>,
): Part => ({ type: `tool-${name}`, toolCallId: `call-${name}`, state, input: {}, ...fields }) as Part;

const SWAP = { a: { page: 4, z: 0 }, b: { page: 9, z: 1 } };

function proposals(message: UIMessage): ProposalBlock[] {
  return messageBlocks(message, label).filter((b): b is ProposalBlock => b.kind === "proposal");
}

describe("describeEdit", () => {
  it.each<[WriteToolName, unknown, string, string | null]>([
    ["regenerate", { opening: 2 }, "Try a different layout", "Pages 4–5"],
    ["reject_layout", { opening: 2 }, "Drop this layout for good", "Pages 4–5"],
    [
      "set_layout",
      { opening: 1, templateId: "07-two-up-symmetric-margin" },
      "Use the “two up symmetric margin” layout",
      "Pages 2–3",
    ],
    ["set_locked", { opening: 3, locked: true }, "Lock this spread", "Pages 6–7"],
    ["set_locked", { opening: 3, locked: false }, "Unlock this spread", "Pages 6–7"],
    ["shuffle", {}, "Try new layouts on every unlocked spread", "Whole book"],
    ["swap_photos", SWAP, "Swap two photos", "Pages 4 and 9"],
    ["swap_photos", { a: { page: 4, z: 0 }, b: { page: 4, z: 1 } }, "Swap two photos", "Page 4"],
    ["set_crop", { slot: { page: 5, z: 0 }, x: 0.1, y: 0.1, w: 0.5 }, "Change the crop", "Page 5"],
  ])("%s %j reads %s, on %s", (name, input, title, where) => {
    expect(describeEdit(name, input, label)).toEqual({ title, where });
  });

  it("names the kind of edit while its input is still arriving", () => {
    expect(describeEdit("swap_photos", { a: { page: 4 } }, label)).toEqual({
      title: "Swap two photos",
      where: null,
    });
  });
});

describe("messageBlocks", () => {
  it("keeps text and reasoning, drops reads, and turns each write into a proposal", () => {
    const blocks = messageBlocks(
      assistant(
        { type: "reasoning", text: "hmm", state: "done" },
        { type: "text", text: "Let me look.", state: "done" },
        {
          type: "tool-get_book",
          toolCallId: "r1",
          state: "output-available",
          input: {},
          output: VIEW,
        } as Part,
        write("swap_photos", {
          state: "approval-requested",
          input: SWAP,
          approval: { id: "ap-1" },
        }),
        { type: "text", text: "", state: "done" },
      ),
      label,
    );
    expect(blocks).toEqual([
      { kind: "reasoning", key: "m-0", text: "hmm", streaming: false },
      { kind: "text", key: "m-1", text: "Let me look." },
      {
        kind: "proposal",
        key: "call-swap_photos",
        title: "Swap two photos",
        where: "Pages 4 and 9",
        status: "pending",
        approvalId: "ap-1",
        warning: null,
        reason: null,
      },
    ]);
  });

  it.each<[string, { state: string } & Record<string, unknown>, ProposalBlock["status"], string | null]>([
    ["input still streaming", { state: "input-streaming" }, "preparing", null],
    ["input complete", { state: "input-available", input: SWAP }, "preparing", null],
    [
      "approved, not yet run",
      { state: "approval-responded", input: SWAP, approval: { id: "ap", approved: true } },
      "applying",
      null,
    ],
    [
      "declined, not yet sent",
      { state: "approval-responded", input: SWAP, approval: { id: "ap", approved: false } },
      "declined",
      null,
    ],
    [
      "declined",
      { state: "output-denied", input: SWAP, approval: { id: "ap", approved: false } },
      "declined",
      null,
    ],
    ["saved", { state: "output-available", input: SWAP, output: VIEW }, "applied", null],
    [
      "refused",
      {
        state: "output-available",
        input: SWAP,
        output: { refused: "a face would be cut on page 9", view: VIEW },
      },
      "refused",
      "a face would be cut on page 9",
    ],
    ["failed", { state: "output-error", input: SWAP, errorText: "x" }, "failed", null],
  ])("a write %s is %s", (_, fields, status, reason) => {
    const [block] = proposals(assistant(write("swap_photos", fields)));
    expect(block?.status).toBe(status);
    expect(block?.reason).toBe(reason);
  });

  it("marks reasoning as streaming only while it is still arriving, and drops it when empty", () => {
    expect(
      messageBlocks(
        assistant(
          { type: "reasoning", text: "", state: "done" },
          { type: "reasoning", text: "Page 2 has two photos", state: "streaming" },
        ),
        label,
      ),
    ).toEqual([{ kind: "reasoning", key: "m-1", text: "Page 2 has two photos", streaming: true }]);
  });

  it("offers an approval id only while the write waits for an answer", () => {
    const answered = proposals(
      assistant(
        write("swap_photos", {
          state: "approval-responded",
          input: SWAP,
          approval: { id: "ap", approved: true },
        }),
      ),
    );
    expect(answered[0]?.approvalId).toBeNull();
  });

  it("carries the approval's reason as the warning", () => {
    const [block] = proposals(
      assistant(
        write("swap_photos", {
          state: "approval-requested",
          input: SWAP,
          approval: { id: "ap", requestReason: "This may not be what you asked for (20% match)." },
        }),
      ),
    );
    expect(block?.warning).toBe("This may not be what you asked for (20% match).");
  });

  it("shows a user's own message as text", () => {
    expect(
      messageBlocks(
        { id: "u", role: "user", parts: [{ type: "text", text: "swap them" }] },
        label,
      ),
    ).toEqual([{ kind: "text", key: "u-0", text: "swap them" }]);
  });
});

function deepseekFetch(...responses: ReturnType<typeof deepseekStream>[]) {
  let turn = 0;
  return vi.fn<typeof globalThis.fetch>(async () => {
    const { status, headers, body } = responses[Math.min(turn++, responses.length - 1)]!;
    return new Response(body, { status, headers });
  });
}

const swapCall = deepseekStream(
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

const jev = (fit: number | null): Jev => ({
  routeIntent: async () => null,
  rankPhotos: tagRanker,
  checkProposal: async () => fit,
});

async function lastMessage(stream: ReadableStream<UIMessageChunk>): Promise<UIMessage | undefined> {
  let last: UIMessage | undefined;
  for await (const message of readUIMessageStream({ stream })) last = message;
  return last;
}

describe("over the real agent stream", () => {
  const invoke = vi.fn<Invoke>(async () => structuredClone(VIEW));

  it("shows Jev's low check on the proposal the user is asked about", async () => {
    const model = createDeepSeek({ apiKey: "t", fetch: deepseekFetch(swapCall) })(MODEL_ID);
    const agent = createBookAgent(1, { model, invoke, jev: jev(0.2) });
    const result = await agent.stream({ messages: [{ role: "user", content: "swap 4 and 9" }] });
    const message = await lastMessage(result.toUIMessageStream());

    const [block] = message ? proposals(message) : [];
    expect(block).toMatchObject({ status: "pending", title: "Swap two photos" });
    expect(block?.approvalId).toEqual(expect.any(String));
    expect(block?.warning).toBe("This may not be what you asked for (20% match).");
  });

  it("puts no warning on a proposal Jev had no opinion on", async () => {
    const model = createDeepSeek({ apiKey: "t", fetch: deepseekFetch(swapCall) })(MODEL_ID);
    const agent = createBookAgent(1, { model, invoke, jev: jev(null) });
    const result = await agent.stream({ messages: [{ role: "user", content: "swap 4 and 9" }] });
    const message = await lastMessage(result.toUIMessageStream());
    expect(message && proposals(message)[0]?.warning).toBeNull();
  });

  it("turns a missing DeepSeek key into the message the panel knows", async () => {
    const fetch = vi.fn<typeof globalThis.fetch>(() =>
      Promise.reject(new MissingKeyError("deepseek", "No DeepSeek API key is set.")),
    );
    const model = createDeepSeek({ apiKey: "t", fetch })(MODEL_ID);
    const agent = createBookAgent(1, { model, invoke, jev: jev(null) });
    const result = await agent.stream({ messages: [{ role: "user", content: "hi" }] });
    const errors: string[] = [];
    for await (const chunk of result.toUIMessageStream({ onError: chatErrorText })) {
      if (chunk.type === "error") errors.push(chunk.errorText);
    }
    expect(errors).toEqual([NO_DEEPSEEK_KEY]);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("passes any other failure's own message through", async () => {
    const fetch = deepseekFetch({
      status: 402,
      headers: [["content-type", "application/json"]],
      body: JSON.stringify({ error: { message: "Insufficient Balance", type: "unknown_error" } }),
    });
    const model = createDeepSeek({ apiKey: "t", fetch })(MODEL_ID);
    const agent = createBookAgent(1, { model, invoke, jev: jev(null) });
    const result = await agent.stream({ messages: [{ role: "user", content: "hi" }] });
    const errors: string[] = [];
    for await (const chunk of result.toUIMessageStream({ onError: chatErrorText })) {
      if (chunk.type === "error") errors.push(chunk.errorText);
    }
    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain("Insufficient Balance");
  });

  it("a plain reply is one text block", async () => {
    const reply = deepseekStream([{ role: "assistant", content: DEEPSEEK_TEXT_REPLY }], "stop", 12);
    const model = createDeepSeek({ apiKey: "t", fetch: deepseekFetch(reply) })(MODEL_ID);
    const agent = createBookAgent(1, { model, invoke, jev: jev(null) });
    const result = await agent.stream({ messages: [{ role: "user", content: "hi" }] });
    const message = await lastMessage(result.toUIMessageStream());
    expect(message && messageBlocks(message, label)).toEqual([
      { kind: "text", key: `${message?.id}-1`, text: DEEPSEEK_TEXT_REPLY },
    ]);
  });

  it("shows DeepSeek's reasoning_content as thinking before the reply", async () => {
    const reply = deepseekStream(
      [
        { role: "assistant", reasoning_content: "Spread 2 has " },
        { reasoning_content: "the busiest layout." },
        { content: DEEPSEEK_TEXT_REPLY },
      ],
      "stop",
      12,
    );
    const model = createDeepSeek({ apiKey: "t", fetch: deepseekFetch(reply) })(MODEL_ID);
    const agent = createBookAgent(1, { model, invoke, jev: jev(null) });
    const result = await agent.stream({ messages: [{ role: "user", content: "hi" }] });
    const message = await lastMessage(result.toUIMessageStream());
    expect(message && messageBlocks(message, label).map(({ kind, text }) => ({ kind, text }))).toEqual([
      { kind: "reasoning", text: "Spread 2 has the busiest layout." },
      { kind: "text", text: DEEPSEEK_TEXT_REPLY },
    ]);
  });
});

/** A fetch answered by the browser harness's scripts, as `model_request` would. */
function harnessFetch(provider: ModelProvider): typeof globalThis.fetch {
  return async (_url, init) => {
    const scripted = scriptedResponse(provider, JSON.parse(String(init?.body)));
    if (!scripted) throw new Error(`no ${provider} script`);
    return new Response(scripted.body, { status: scripted.status, headers: scripted.headers });
  };
}

/** The text a scripted DeepSeek stream spells out. */
function streamedText(provider: ModelProvider, body: unknown): string {
  const scripted = scriptedResponse(provider, body as Parameters<typeof scriptedResponse>[1]);
  return (scripted?.body ?? "")
    .split("\n")
    .filter((line) => line.startsWith("data: {"))
    .map((line) => JSON.parse(line.slice(6)).choices[0].delta.content ?? "")
    .join("");
}

describe("the browser harness's scripts", () => {
  const invoke = vi.fn<Invoke>(async () => structuredClone(VIEW));

  async function ask(words: string, withJev: boolean) {
    const model = createDeepSeek({ apiKey: "t", fetch: harnessFetch("deepseek") })(MODEL_ID);
    const harnessJev = withJev ? createJev({ fetch: harnessFetch("jev"), log: () => {} }) : jev(null);
    const agent = createBookAgent(1, { model, invoke, jev: harnessJev });
    const result = await agent.stream({ messages: [{ role: "user", content: words }] });
    const message = await lastMessage(result.toUIMessageStream());
    return message ? proposals(message) : [];
  }

  it("proposes the page 2 swap, with Jev's check low enough to warn", async () => {
    const [block] = await ask("swap the two photos on page 2", true);
    expect(block).toMatchObject({ status: "pending", title: "Swap two photos", where: "Page 2" });
    expect(block?.warning).toBe("This may not be what you asked for (30% match).");
  });

  it("proposes with no warning when Jev is off", async () => {
    const [block] = await ask("swap them", false);
    expect(block).toMatchObject({ status: "pending", warning: null });
  });

  it.each([
    ["a different layout for the last page", "Try a different layout", "Pages 4–5"],
    ["a different layout here", "Try a different layout", "Pages 2–3"],
  ])("%s proposes %s on %s", async (words, title, where) => {
    const [block] = await ask(words, false);
    expect(block).toMatchObject({ status: "pending", title, where });
  });

  it.each([
    [JSON.stringify({ refused: "every layout for 2 photos has been shown or rejected here" }), "I couldn't do that: every layout for 2 photos has been shown or rejected here."],
    ["Tool call execution denied.", "Okay, I left it as it was. What would you like instead?"],
    [JSON.stringify(VIEW), "Done. The book on screen is updated."],
  ])("answers the tool result %s with %s", (content, reply) => {
    expect(streamedText("deepseek", { messages: [{ role: "tool", content }] })).toBe(reply);
  });

  // The agent tells a new proposal from one it already showed by this id.
  it("gives every tool call its own id", () => {
    const ids = [1, 2].map(() => {
      const scripted = scriptedResponse("deepseek", { messages: [{ role: "user", content: "swap" }] });
      return /"id":"([^"]+)","type":"function"/.exec(scripted?.body ?? "")?.[1];
    });
    expect(ids[0]).toBeTruthy();
    expect(ids[0]).not.toBe(ids[1]);
  });

  it("thinks before answering when asked to think", () => {
    const scripted = scriptedResponse("deepseek", { messages: [{ role: "user", content: "think it through" }] });
    const thought = (scripted?.body ?? "")
      .split("\n")
      .filter((line) => line.startsWith("data: {"))
      .map((line) => JSON.parse(line.slice(6)).choices[0].delta.reasoning_content ?? "")
      .join("");
    expect(thought).toBe(DEEPSEEK_THINKING);
    expect(streamedText("deepseek", { messages: [{ role: "user", content: "think it through" }] })).toBe(
      DEEPSEEK_TEXT_REPLY,
    );
  });

  it("answers words it has no script for with the plain reply", () => {
    expect(streamedText("deepseek", { messages: [{ role: "user", content: "hello" }] })).toBe(
      DEEPSEEK_TEXT_REPLY,
    );
  });
});
