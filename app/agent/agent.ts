/**
 * The agent loop: DeepSeek driving the tools in `tools.ts`, with every write
 * held for the user's approval and Jev's route and check folded in.
 *
 * The model is passed in so tests can script it; the app builds it over
 * `modelFetch("deepseek")`, which keeps the key in Rust.
 */
import {
  getToolName,
  InvalidToolInputError,
  isStepCount,
  isToolUIPart,
  pruneMessages,
  ToolLoopAgent,
  wrapLanguageModel,
  type LanguageModelMiddleware,
  type ModelMessage,
  type ToolApprovalStatus,
  type UIMessage,
} from "ai";
import type { Jev } from "./jev";
import { AGENT_TOOLS, createAgentTools, type Invoke, type ToolName } from "./tools";

export type AgentModel = Parameters<typeof wrapLanguageModel>[0]["model"];

/**
 * DeepSeek's name for V4.1 Flash, as an explicit string because the
 * provider's typed union still lists retired models (design §9.2). The old
 * `deepseek-v4-flash` is still accepted, but only as an alias.
 */
export const MODEL_ID = "deepseek-flash";

/** The loop stops after this many model calls, whatever the model wants. */
export const MAX_STEPS = 20;

/**
 * Below this, Jev's check puts a warning on the approval request. Chosen, not
 * measured, like `ROUTE_CONFIDENCE`: the decision log is how it gets measured.
 */
export const CHECK_WARN_BELOW = 0.5;

/** How many trailing messages keep their tool calls and results when the history is pruned. */
const KEEP_TOOL_MESSAGES = 6;

const INSTRUCTIONS = [
  "You edit a printed photobook for its owner. The book is a list of openings (two-page",
  "spreads); each has a template and photos in slots.",
  "Read the book with get_book or get_opening before changing it, and refer to photos",
  "and slots only by what those tools return.",
  "Each change you propose is shown to the user, who approves or declines it. If it is",
  "declined, do not propose the same change again; ask what they want instead.",
  "If a tool says an edit was refused, tell the user why in plain words.",
  "Answer briefly. You cannot see the photos, only the facts the tools give.",
].join(" ");

export interface BookAgentDeps {
  model: AgentModel;
  invoke: Invoke;
  jev: Jev;
}

/** The text of the latest user message, or "" when there is none. */
export function lastUserText(messages: readonly ModelMessage[]): string {
  const message = messages.findLast((m) => m.role === "user");
  if (!message) return "";
  if (typeof message.content === "string") return message.content;
  return message.content
    .flatMap((part) => (part.type === "text" ? [part.text] : []))
    .join("\n");
}

/**
 * Drops tool calls and results older than the last few messages. Reasoning is
 * never pruned: DeepSeek answers 400 when `reasoning_content` is missing from
 * an assistant turn that called a tool.
 */
export function pruneHistory(messages: ModelMessage[]): ModelMessage[] {
  return pruneMessages({
    messages,
    reasoning: "none",
    toolCalls: `before-last-${KEEP_TOOL_MESSAGES}-messages`,
    emptyMessages: "remove",
  });
}

/** Pulls a JSON object out of text a model wrapped in a fence or prose. */
function extractJson(text: string): unknown {
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start === -1 || end < start) return undefined;
  try {
    return JSON.parse(text.slice(start, end + 1));
  } catch {
    return undefined;
  }
}

const isToolName = (name: string): name is ToolName => Object.hasOwn(AGENT_TOOLS, name);

type Content = Awaited<ReturnType<NonNullable<LanguageModelMiddleware["wrapGenerate"]>>>["content"];

const hasAnswer = (content: Content) =>
  content.some((p) => (p.type === "text" && p.text !== "") || p.type === "tool-call");

/**
 * Buffers a stream until it shows an answer. Resolves to the whole stream
 * replayed, or to `null` if it finished with nothing to show.
 */
async function answeredStream<P extends { type: string }>(
  stream: ReadableStream<P>,
  answers: (part: P) => boolean,
): Promise<ReadableStream<P> | null> {
  const reader = stream.getReader();
  const seen: P[] = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return null;
    seen.push(value);
    if (answers(value)) break;
  }
  return new ReadableStream<P>({
    start(controller) {
      for (const part of seen) controller.enqueue(part);
    },
    async pull(controller) {
      const { done, value } = await reader.read();
      if (done) controller.close();
      else controller.enqueue(value);
    },
    cancel: (reason) => reader.cancel(reason),
  });
}

/** DeepSeek documents that it occasionally answers with no content; one retry covers it. */
const retryEmpty: LanguageModelMiddleware = {
  specificationVersion: "v4",
  async wrapGenerate({ doGenerate }) {
    const first = await doGenerate();
    return hasAnswer(first.content) ? first : doGenerate();
  },
  async wrapStream({ doStream }) {
    const first = await doStream();
    const stream = await answeredStream(
      first.stream,
      (p) => (p.type === "text-delta" && p.delta !== "") || p.type === "tool-call",
    );
    return stream ? { ...first, stream } : doStream();
  },
};

export function createBookAgent(projectId: number, { model, invoke, jev }: BookAgentDeps) {
  return new ToolLoopAgent({
    model: wrapLanguageModel({ model, middleware: retryEmpty }),
    instructions: INSTRUCTIONS,
    tools: createAgentTools(projectId, { invoke, rankPhotos: jev.rankPhotos }),
    stopWhen: isStepCount(MAX_STEPS),
    providerOptions: { deepseek: { thinking: { type: "disabled" } } },

    async toolApproval({ toolCall, messages }): Promise<ToolApprovalStatus> {
      const { toolName, input } = toolCall;
      if (!isToolName(toolName) || !AGENT_TOOLS[toolName].writes) return "not-applicable";
      const fit = await jev.checkProposal(lastUserText(messages), { toolName, input });
      if (fit === null || fit >= CHECK_WARN_BELOW) return "user-approval";
      return {
        type: "user-approval",
        reason: `This may not be what you asked for (${Math.round(fit * 100)}% match).`,
      };
    },

    async prepareStep({ stepNumber, messages }) {
      const pruned = pruneHistory(messages);
      if (stepNumber > 0) return { messages: pruned };
      const routed = await jev.routeIntent(lastUserText(messages));
      return routed ? { messages: pruned, activeTools: [...routed] } : { messages: pruned };
    },

    // The SDK validates the repaired input against the tool's schema again, once.
    async repairToolCall({ toolCall, error }) {
      if (!InvalidToolInputError.isInstance(error)) return null;
      const json = extractJson(toolCall.input);
      return json === undefined ? null : { ...toolCall, input: JSON.stringify(json) };
    },
  });
}

export type BookAgent = ReturnType<typeof createBookAgent>;

/**
 * The tool call ids of every write in `messages` that Rust saved: its output
 * is the book, not a refusal. The chat calls this after each turn to know
 * when the book on screen is stale.
 */
export function savedEdits(messages: readonly UIMessage[]): string[] {
  return messages.flatMap((m) =>
    m.parts.flatMap((part) => {
      if (!isToolUIPart(part) || part.state !== "output-available") return [];
      const name = getToolName(part);
      if (!isToolName(name) || !AGENT_TOOLS[name].writes) return [];
      const refused = typeof part.output === "object" && part.output !== null && "refused" in part.output;
      return refused ? [] : [part.toolCallId];
    }),
  );
}
