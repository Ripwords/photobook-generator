/**
 * The browser harness's `model_request`: scripted responses played back over
 * the channel in the same event shapes Rust sends, so the chat panel can be
 * driven with no key and no network.
 *
 * `SCRIPTS` is the whole behaviour. The first entry whose provider matches
 * and whose `respond` answers the request body is played back; an unmatched
 * request is refused loudly, like an unmocked command.
 */
import type { ModelEvent, ModelProvider, ModelRequestError } from "../../app/agent/fetch";

export interface ModelRequestArgs {
  id: string;
  provider: ModelProvider;
  path: string;
  body: string;
  onEvent: { onmessage: (event: ModelEvent) => void };
}

interface CannedResponse {
  status: number;
  headers: [string, string][];
  body: string;
}

/** The part of a request body the scripts read. Anything else in it is ignored. */
interface RequestBody {
  messages?: { role: string; content?: string | null }[];
  questions?: Record<string, unknown>;
}

interface Script {
  provider: ModelProvider;
  /** The response, or `null` to let the next script answer. */
  respond: (body: RequestBody) => CannedResponse | null;
}

/** Mirrors `agent::request::allowed_path`. */
const ALLOWED_PATH: Record<ModelProvider, string> = {
  deepseek: "/chat/completions",
  jev: "/v1/systemone",
};

/** One `choices[0].delta` of a DeepSeek chat-completions stream. */
interface DeepSeekDelta {
  role?: "assistant";
  content?: string;
  reasoning_content?: string;
  tool_calls?: {
    index: number;
    id?: string;
    type?: "function";
    function: { name?: string; arguments?: string };
  }[];
}

const chunk = (delta: DeepSeekDelta, finish: string | null, usage?: object) => ({
  id: "mock-chatcmpl-1",
  object: "chat.completion.chunk",
  created: 1_758_000_000,
  model: "deepseek-flash",
  system_fingerprint: "fp_mock",
  choices: [{ index: 0, delta, logprobs: null, finish_reason: finish }],
  ...(usage ? { usage } : {}),
});

/**
 * A chat-completions stream shaped like DeepSeek's: a keep-alive comment,
 * one `chat.completion.chunk` per delta, a final chunk with the finish
 * reason and usage, then `[DONE]`.
 */
export function deepseekStream(
  deltas: DeepSeekDelta[],
  finishReason: "stop" | "tool_calls",
  completionTokens: number,
): CannedResponse {
  const events = [
    ...deltas.map((delta) => chunk(delta, null)),
    chunk({ content: "" }, finishReason, {
      prompt_tokens: 40,
      completion_tokens: completionTokens,
      total_tokens: 40 + completionTokens,
      prompt_cache_hit_tokens: 0,
      prompt_cache_miss_tokens: 40,
    }),
  ];
  return {
    status: 200,
    headers: [
      ["content-type", "text/event-stream; charset=utf-8"],
      ["cache-control", "no-cache"],
    ],
    body: `: keep-alive\n\n${events.map((e) => `data: ${JSON.stringify(e)}\n\n`).join("")}data: [DONE]\n\n`,
  };
}

export const DEEPSEEK_TEXT_REPLY =
  "I can make chapter 2 calmer — it would use quieter layouts on spreads 3 and 4.";

/** What the harness's DeepSeek thinks before its plain reply, when asked to think. */
export const DEEPSEEK_THINKING =
  "Chapter 2 is spreads 3 and 4. Both use busy four-photo layouts, so quieter ones would calm it.";

/**
 * Jev answers only the proposal check in the harness, so a card can show its
 * warning. Routing and search get this 529 and run on their no-answer paths:
 * every tool offered, `tagRanker` for search. Those paths must always work.
 */
export const JEV_OVERLOADED = { error: "Service temporarily unavailable" };

/** Jev's answer to every proposal check: low enough to put the warning on the card. */
export const JEV_LOW_FIT = 0.3;

/** The swap the harness proposes: the two photos on page 2. */
export const HARNESS_SWAP = { a: { page: 2, z: 1 }, b: { page: 2, z: 2 } };

const json = (status: number, body: unknown): CannedResponse => ({
  status,
  headers: [["content-type", "application/json"]],
  body: JSON.stringify(body),
});

const words = (text: string) => text.split(/(?<= )/);

const text = (reply: string, thinking = "") =>
  deepseekStream(
    [
      { role: "assistant", content: "" },
      ...(thinking ? words(thinking).map((reasoning_content) => ({ reasoning_content })) : []),
      ...words(reply).map((content) => ({ content })),
    ],
    "stop",
    12,
  );

/** Tool call ids are unique within a conversation, as DeepSeek's are. */
let toolCalls = 0;

const toolCall = (name: string, input: unknown) =>
  deepseekStream(
    [
      {
        role: "assistant",
        tool_calls: [
          { index: 0, id: `mock-${name}-${++toolCalls}`, type: "function", function: { name, arguments: JSON.stringify(input) } },
        ],
      },
    ],
    "tool_calls",
    9,
  );

/** What a tool result told the model, as the harness reads it back. */
function toolResult(content: string): { refused: string } | "denied" | "saved" {
  try {
    const parsed: unknown = JSON.parse(content);
    if (typeof parsed === "object" && parsed !== null && "refused" in parsed) {
      return { refused: String(parsed.refused) };
    }
    return "saved";
  } catch {
    // A declined write reaches the model as the SDK's plain-text denial.
    return "denied";
  }
}

/**
 * The harness's DeepSeek. It reads the last message only: a user's words pick
 * a proposal, a tool's result picks the reply to it. Words it has no script
 * for get `DEEPSEEK_TEXT_REPLY`.
 */
function deepseekReply(body: RequestBody): CannedResponse {
  const last = body.messages?.at(-1);
  const content = last?.content ?? "";
  if (last?.role === "tool") {
    const result = toolResult(content);
    if (result === "denied") return text("Okay, I left it as it was. What would you like instead?");
    if (result === "saved") return text("Done. The book on screen is updated.");
    return text(`I couldn't do that: ${result.refused}.`);
  }
  const asked = content.toLowerCase();
  if (asked.includes("swap")) return toolCall("swap_photos", HARNESS_SWAP);
  if (asked.includes("last")) return toolCall("regenerate", { opening: 2 });
  if (asked.includes("layout")) return toolCall("regenerate", { opening: 1 });
  if (asked.includes("think")) return text(DEEPSEEK_TEXT_REPLY, DEEPSEEK_THINKING);
  return text(DEEPSEEK_TEXT_REPLY);
}

export const SCRIPTS: Script[] = [
  {
    provider: "jev",
    respond: (body) =>
      body.questions && "fits" in body.questions
        ? json(200, { answers: { fits: { type: "noul", noul: JEV_LOW_FIT } } })
        : null,
  },
  { provider: "jev", respond: () => json(529, JEV_OVERLOADED) },
  { provider: "deepseek", respond: deepseekReply },
];

/** The scripted answer to one request body, or `null` when no script has one. */
export function scriptedResponse(provider: ModelProvider, body: RequestBody): CannedResponse | null {
  for (const script of SCRIPTS) {
    if (script.provider !== provider) continue;
    const response = script.respond(body);
    if (response) return response;
  }
  return null;
}

/** Small enough that some chunks end inside a multi-byte character, as real ones can. */
const CHUNK_BYTES = 37;

const cancelled = new Set<string>();

export function cancelModelRequest(id: string): void {
  cancelled.add(id);
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const refuse = (message: string): ModelRequestError => ({ kind: "refused", message });

/** Answers `model_request`; `pace` is the pause between chunks, in ms. */
export async function modelRequest(
  { id, provider, path, body, onEvent }: ModelRequestArgs,
  { pace = 40 }: { pace?: number } = {},
): Promise<void> {
  if (path !== ALLOWED_PATH[provider]) throw refuse(`${path} is not a ${provider} endpoint.`);
  const response = scriptedResponse(provider, JSON.parse(body) as RequestBody);
  if (!response) throw refuse(`the browser harness has no scripted ${provider} response for this request`);

  const { status, headers } = response;
  const bytes = new TextEncoder().encode(response.body);
  onEvent.onmessage({ kind: "head", status, headers });
  for (let start = 0; start < bytes.length; start += CHUNK_BYTES) {
    await sleep(pace);
    if (cancelled.delete(id)) {
      onEvent.onmessage({ kind: "cancelled" });
      return;
    }
    onEvent.onmessage({ kind: "chunk", bytes: [...bytes.subarray(start, start + CHUNK_BYTES)] });
  }
  onEvent.onmessage({ kind: "end" });
}
