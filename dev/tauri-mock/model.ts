/**
 * The browser harness's `model_request`: scripted responses played back over
 * the channel in the same event shapes Rust sends, so the chat panel can be
 * driven with no key and no network.
 *
 * `SCRIPTS` is the whole behaviour. The first entry whose provider matches
 * and whose `when` accepts the request body answers it; an unmatched request
 * is refused loudly, like an unmocked command.
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

interface Script {
  provider: ModelProvider;
  when: (body: unknown) => boolean;
  response: CannedResponse;
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

/**
 * Jev is always overloaded in the harness, so the chat panel runs on each
 * role's no-answer path: every tool offered, `tagRanker` for search, no
 * check. Those paths are the ones that must always work.
 */
export const JEV_OVERLOADED = { error: "Service temporarily unavailable" };

export const SCRIPTS: Script[] = [
  {
    provider: "jev",
    when: () => true,
    response: {
      status: 529,
      headers: [["content-type", "application/json"]],
      body: JSON.stringify(JEV_OVERLOADED),
    },
  },
  {
    provider: "deepseek",
    when: () => true,
    response: deepseekStream(
      [{ role: "assistant", content: "" }, ...DEEPSEEK_TEXT_REPLY.split(/(?<= )/).map((content) => ({ content }))],
      "stop",
      12,
    ),
  },
];

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
  const parsed: unknown = JSON.parse(body);
  const script = SCRIPTS.find((s) => s.provider === provider && s.when(parsed));
  if (!script) throw refuse(`the browser harness has no scripted ${provider} response for this request`);

  const { status, headers } = script.response;
  const bytes = new TextEncoder().encode(script.response.body);
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
