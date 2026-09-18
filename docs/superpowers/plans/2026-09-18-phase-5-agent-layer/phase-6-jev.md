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

## As built

- `routeIntent(message)` takes no `AgentView`. Classifying intent needs only the user's words,
  so the book stays out of that request.
- `rankPhotos` keeps a photo only when Jev gives it more than an even share of the probability
  (`p > 1 / n`), best first. Without a cut, a query that matches nothing would still return
  `limit` photos. The rule is chosen, not measured, like `ROUTE_CONFIDENCE`.
- A photo is described to Jev by its tags and face count only, keyed by `AgentPhoto.id`. An
  id Jev was not offered is dropped.
- `createJev({ fetch, log })` takes the log as a callback. Phase 7 decides where it is written.
  A `JevDecision` holds the role, the intent or tool, the probability and the outcome, never
  the message.
- The browser harness answers every Jev request with a 529, so the panel runs on the
  no-answer paths there.

## Jev is optional

Added after Phase 7. A missing key is not a failure. `ask` tells a `MissingKeyError` apart from
every other rejection. Each role then gives its no-answer result: every tool, `tagRanker`, no
opinion. It writes no decision to the log, because Jev decided nothing. Any other failure still
logs as before. Tests are in `tests/agent-jev.test.ts` under "with no Jev key". Each of these
five mutations fails a test:

- log a missing key as a failure;
- treat every failure as a missing key;
- log the route when Jev is off;
- rank nothing when Jev is off;
- warn when Jev is off.
