/**
 * What the chat panel shows, derived from the AI SDK's messages. Pure, so
 * every state a proposal card can be in is tested without a component.
 */
import { APICallError, getToolName, isToolUIPart, type UIMessage } from "ai";
import { templateLabel } from "~/types/preview";
import { MissingKeyError } from "./fetch";
import { AGENT_TOOLS, type ToolName, type WriteToolName } from "./tools";

/** The printed name of an opening, as the book preview labels it. */
export type OpeningLabel = (opening: number) => string;

export interface TextBlock {
  kind: "text";
  key: string;
  text: string;
}

/**
 * - `preparing`: the model is still writing the edit.
 * - `pending`: waiting for the user.
 * - `applying`: approved, not yet saved.
 * - `applied`: Rust saved it.
 * - `refused`: a rule refused it; `reason` says which.
 * - `declined`: the user said no.
 * - `failed`: it could not be saved for any other reason.
 */
export type ProposalStatus =
  | "preparing"
  | "pending"
  | "applying"
  | "applied"
  | "refused"
  | "declined"
  | "failed";

export interface ProposalBlock {
  kind: "proposal";
  key: string;
  title: string;
  /** The pages it touches, or `null` while the input is still arriving. */
  where: string | null;
  status: ProposalStatus;
  /** Set only while `pending`: the id `approve` answers. */
  approvalId: string | null;
  /** Jev's warning, when its check was low. */
  warning: string | null;
  /** The engine's reason, when `refused`. */
  reason: string | null;
}

export type ChatBlock = TextBlock | ProposalBlock;

export interface EditDescription {
  title: string;
  where: string | null;
}

const isWrite = (name: string): name is WriteToolName =>
  Object.hasOwn(AGENT_TOOLS, name) && AGENT_TOOLS[name as ToolName].writes;

function pages(a: number, b: number): string {
  return a === b ? `Page ${a}` : `Pages ${Math.min(a, b)} and ${Math.max(a, b)}`;
}

/**
 * A write in the user's words. `input` may be partial while it streams; the
 * title never depends on it, and `where` is `null` until it parses.
 */
export function describeEdit(
  name: WriteToolName,
  input: unknown,
  label: OpeningLabel,
): EditDescription {
  const parsed = AGENT_TOOLS[name].input.safeParse(input);
  const data: Record<string, unknown> = parsed.success ? parsed.data : {};
  const opening = parsed.success && typeof data.opening === "number" ? label(data.opening) : null;
  switch (name) {
    case "regenerate":
      return { title: "Try a different layout", where: opening };
    case "reject_layout":
      return { title: "Drop this layout for good", where: opening };
    case "set_layout": {
      const id = typeof data.templateId === "string" ? data.templateId : null;
      return {
        title: id ? `Use the “${templateLabel(id)}” layout` : "Use another layout",
        where: opening,
      };
    }
    case "set_locked":
      return {
        title: data.locked === false ? "Unlock this spread" : "Lock this spread",
        where: opening,
      };
    case "shuffle":
      return { title: "Try new layouts on every unlocked spread", where: "Whole book" };
    case "swap_photos": {
      if (!parsed.success) return { title: "Swap two photos", where: null };
      const { a, b } = AGENT_TOOLS.swap_photos.input.parse(input);
      return { title: "Swap two photos", where: pages(a.page, b.page) };
    }
    case "set_crop": {
      if (!parsed.success) return { title: "Change the crop", where: null };
      const { slot } = AGENT_TOOLS.set_crop.input.parse(input);
      return { title: "Change the crop", where: `Page ${slot.page}` };
    }
  }
}

type ToolPart = Extract<UIMessage["parts"][number], { toolCallId: string }>;

function statusOf(part: ToolPart): Pick<ProposalBlock, "status" | "approvalId" | "reason"> {
  const none = { approvalId: null, reason: null };
  switch (part.state) {
    case "input-streaming":
    case "input-available":
      return { status: "preparing", ...none };
    case "approval-requested":
      return { status: "pending", approvalId: part.approval.id, reason: null };
    case "approval-responded":
      return { status: part.approval.approved ? "applying" : "declined", ...none };
    case "output-denied":
      return { status: "declined", ...none };
    case "output-error":
      return { status: "failed", ...none };
    case "output-available": {
      const output: unknown = part.output;
      if (
        typeof output === "object" &&
        output !== null &&
        "refused" in output &&
        typeof output.refused === "string"
      ) {
        return { status: "refused", approvalId: null, reason: output.refused };
      }
      return { status: "applied", ...none };
    }
  }
}

/** One message as the panel draws it: its text, and a card for each write. */
export function messageBlocks(message: UIMessage, label: OpeningLabel): ChatBlock[] {
  return message.parts.flatMap((part, index): ChatBlock[] => {
    if (part.type === "text") {
      return part.text.trim() === "" ? [] : [{ kind: "text", key: `${message.id}-${index}`, text: part.text }];
    }
    if (!isToolUIPart(part)) return [];
    const name = getToolName(part);
    if (!isWrite(name)) return [];
    return [
      {
        kind: "proposal",
        key: part.toolCallId,
        ...describeEdit(name, part.input, label),
        ...statusOf(part),
        warning: part.approval?.requestReason ?? null,
      },
    ];
  });
}

/** The text the panel shows when there is no DeepSeek key, and knows to offer Settings for. */
export const NO_DEEPSEEK_KEY = "Add your DeepSeek API key to chat about this book.";

function causes(error: unknown): unknown[] {
  const chain: unknown[] = [];
  for (let e = error; e !== undefined && chain.length < 8; ) {
    chain.push(e);
    e = e instanceof Error ? e.cause : undefined;
  }
  return chain;
}

/**
 * What a failed turn tells the user. It becomes `useChat`'s `error.message`.
 * The model's traffic goes through Rust, so no message here can carry a key.
 */
export function chatErrorText(error: unknown): string {
  const chain = causes(error);
  if (chain.some((e) => e instanceof MissingKeyError)) return NO_DEEPSEEK_KEY;
  const api = chain.find((e) => APICallError.isInstance(e));
  if (api) return `DeepSeek answered ${api.statusCode ?? "with an error"}: ${api.message}`;
  return error instanceof Error ? error.message : String(error);
}
