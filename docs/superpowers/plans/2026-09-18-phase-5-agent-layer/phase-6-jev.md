# Phase 6: the Jev client and its three roles

[Overview](overview.md)

## Goal

Jev routes requests, ranks photo search results and checks proposed edits. Each role has a
defined behaviour when no Jev key is set or the call fails, so Jev can never make the agent
worse than DeepSeek alone.

## Changes

- New `app/agent/jev.ts`. A minimal client for `POST /v1/systemone` over `modelFetch("jev")`.
  It is about 40 lines and doesn't need `@typesafe-ai/sdk`, which targets Node 20.
- `routeIntent(message, view)` asks one `choice` over the intents in `INTENTS`. Each intent
  lists the tool names it allows. At or above `ROUTE_CONFIDENCE` it returns those tools;
  otherwise it returns `null` (every tool available).
- `rankPhotos(query, photos)` asks one `choice` over photo ids and returns them ordered by
  probability. This is TypeSafe's documented batch pattern.
- `checkProposal(message, toolCall)` asks one `noul` and returns the probability.
- Every decision is appended to a local log (the design's "every tool call logged locally"),
  so `ROUTE_CONFIDENCE` can be set from real use rather than guessed. It starts at `0.8`.
  That value is chosen, not measured, and is recorded as such in PROJECT-STATUS.

## Data structures

- `INTENTS: Record<Intent, { description: string, tools: ToolName[] }>`, a table rather than
  branching.
- `JevAnswer`, parsed at the boundary with zod. A malformed response is treated as "no
  answer", the same as no key.

## Verification

- With a mocked fetch:
  - a confident answer narrows the tools;
  - a low-confidence answer, a 429, a malformed body and a missing key all give `null`.
- Mutation: invert the threshold comparison and paste the failure.
- Every intent's tools exist in `AGENT_TOOLS` (a table-consistency test).
- The request body contains only the message and the `AgentView` fields. Reuse the leak
  check.
