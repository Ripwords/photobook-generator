/**
 * Jev, TypeSafe's classifier, in its three roles: routing a request to the
 * tools it needs, ranking photos for `search_photos`, and checking a proposed
 * edit before the user is asked to approve it.
 *
 * Each role degrades to what DeepSeek alone would do. No key, a 429, a
 * malformed body or an answer outside the question all read as "no answer":
 * routing offers every tool, ranking falls back to `tagRanker`, and a check
 * has no opinion. Jev can make the agent better but never worse.
 *
 * What goes to Jev is the user's message, a tool call's input, and
 * `AgentPhoto` facts. Nothing else is in reach here.
 */
import { z } from "zod";
import { tagRanker, type PhotoRanker, type ToolName } from "./tools";
import type { AgentPhoto } from "./view";

/**
 * What a user can ask the agent for, and the tools each needs. A table so
 * routing is a lookup; `tests/agent-jev.test.ts` checks every tool is named
 * and every name is a tool.
 */
export const INTENTS = {
  ask: {
    description: "A question about the book or its photos; nothing should change.",
    tools: ["get_book", "get_opening", "search_photos"],
  },
  swap: {
    description: "Move particular photos: put one on a page, or exchange two.",
    tools: ["get_book", "get_opening", "search_photos", "swap_photos"],
  },
  relayout: {
    description: "Change how a spread or the whole book is laid out: a different template.",
    tools: ["get_book", "get_opening", "regenerate", "reject_layout", "set_layout", "shuffle"],
  },
  crop: {
    description: "Change how a photo is framed in its slot: zoom, move or recentre it.",
    tools: ["get_book", "get_opening", "set_crop"],
  },
  lock: {
    description: "Keep a spread as it is, or allow it to change again.",
    tools: ["get_book", "get_opening", "set_locked"],
  },
} as const satisfies Record<string, { description: string; tools: readonly ToolName[] }>;

export type Intent = keyof typeof INTENTS;

/**
 * The routing probability at or above which the agent sees only the intent's
 * tools. Chosen, not measured: the decision log is how it gets measured.
 */
export const ROUTE_CONFIDENCE = 0.8;

/** One Jev decision, for the local log. Holds no message text. */
export type JevDecision =
  | { role: "route"; intent: Intent | null; probability: number | null; narrowed: boolean }
  | { role: "rank"; photos: number; kept: number | null; answered: boolean }
  | { role: "check"; tool: ToolName; probability: number | null };

export interface JevDeps {
  /** `modelFetch("jev")` in the app: Rust picks the host and attaches the key. */
  fetch: typeof globalThis.fetch;
  log: (decision: JevDecision) => void;
}

export interface ProposedCall {
  toolName: ToolName;
  input: unknown;
}

export interface Jev {
  /** The tools this message needs, or `null` for all of them. */
  routeIntent(message: string): Promise<readonly ToolName[] | null>;
  rankPhotos: PhotoRanker;
  /** The probability the call does what the message asked, or `null` for no opinion. */
  checkProposal(message: string, call: ProposedCall): Promise<number | null>;
}

const ENDPOINT = "https://api.typesafe.ai/v1/systemone";

const choiceAnswer = z.object({
  type: z.literal("choice"),
  probabilities: z.record(z.string(), z.number().min(0).max(1)),
});
const noulAnswer = z.object({ type: z.literal("noul"), noul: z.number().min(0).max(1) });
const response = z.object({ answers: z.record(z.string(), z.unknown()) });

type Question =
  | { type: "choice"; instructions: string; criteria: Record<string, string> }
  | { type: "noul"; instructions: string; criteria: { true: string; false: string } };

/** The option with the highest probability among `options`, if Jev chose one of them. */
function best(probabilities: Record<string, number>): [string, number] | null {
  const [top] = Object.entries(probabilities).toSorted(([, a], [, b]) => b - a);
  return top ?? null;
}

const isIntent = (key: string): key is Intent => Object.hasOwn(INTENTS, key);

function describePhoto(photo: AgentPhoto): string {
  const tags = photo.tags.length > 0 ? photo.tags.join(", ") : "no tags";
  const faces = photo.faces === 1 ? "1 face" : `${photo.faces} faces`;
  return `${tags}; ${faces}`;
}

export function createJev({ fetch, log }: JevDeps): Jev {
  /** Jev's answer to one question, or `null` whenever there is none to trust. */
  async function ask<A>(
    state: unknown,
    id: string,
    question: Question,
    answer: z.ZodType<A>,
  ): Promise<A | null> {
    try {
      const res = await fetch(ENDPOINT, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ state, model: "jev-latest", questions: { [id]: question } }),
      });
      const body = response.safeParse(await res.json());
      if (!body.success) return null;
      const parsed = answer.safeParse(body.data.answers[id]);
      return parsed.success ? parsed.data : null;
    } catch {
      return null;
    }
  }

  return {
    async routeIntent(message) {
      const answer = await ask(
        message,
        "intent",
        {
          type: "choice",
          instructions: "What is the user asking a photobook editor to do?",
          criteria: Object.fromEntries(
            Object.entries(INTENTS).map(([key, { description }]) => [key, description]),
          ),
        },
        choiceAnswer,
      );
      const top = answer && best(answer.probabilities);
      const intent = top && isIntent(top[0]) ? top[0] : null;
      const probability = intent && top ? top[1] : null;
      const narrowed = intent !== null && probability !== null && probability >= ROUTE_CONFIDENCE;
      log({ role: "route", intent, probability, narrowed });
      return narrowed ? INTENTS[intent].tools : null;
    },

    async rankPhotos(query, photos) {
      if (photos.length === 0) return [];
      const answer = await ask(
        query,
        "photo",
        {
          type: "choice",
          instructions: "Which photo best matches what the user is looking for?",
          criteria: Object.fromEntries(photos.map((p) => [String(p.id), describePhoto(p)])),
        },
        choiceAnswer,
      );
      if (!answer) {
        log({ role: "rank", photos: photos.length, kept: null, answered: false });
        return tagRanker(query, photos);
      }
      const offered = new Set(photos.map((p) => p.id));
      const evenShare = 1 / photos.length;
      const kept = Object.entries(answer.probabilities)
        .map(([key, p]) => ({ id: Number(key), p }))
        .filter(({ id, p }) => offered.has(id) && p > evenShare)
        .toSorted((a, b) => b.p - a.p || a.id - b.id)
        .map(({ id }) => id);
      log({ role: "rank", photos: photos.length, kept: kept.length, answered: true });
      return kept;
    },

    async checkProposal(message, call) {
      const answer = await ask(
        { request: message, proposal: { tool: call.toolName, input: call.input } },
        "fits",
        {
          type: "noul",
          instructions: "Does the proposed photobook edit do what the user asked for?",
          criteria: {
            true: "The edit does what was asked, on the pages or photos the user meant.",
            false: "The edit changes something else, or more than was asked.",
          },
        },
        noulAnswer,
      );
      const probability = answer?.noul ?? null;
      log({ role: "check", tool: call.toolName, probability });
      return probability;
    },
  };
}
