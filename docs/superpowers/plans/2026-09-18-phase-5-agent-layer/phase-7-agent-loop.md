# Phase 7: the agent loop

[Overview](overview.md)

## Goal

A `ToolLoopAgent` wired to DeepSeek through Rust, with approvals, step limits, pruning and
repair, and the router in `prepareStep`.

## Changes

- New `app/agent/agent.ts`, `createBookAgent(projectId)`:
  - `createDeepSeek({ fetch: modelFetch("deepseek") })`, model `deepseek-flash`, thinking
    disabled for routine calls (design §9.2);
  - `stopWhen: isStepCount(20)`;
  - `toolApproval` requires approval for every write tool and none for reads;
  - `prepareStep` applies the Jev route to `activeTools` on the first step only;
  - `repairToolCall` re-parses against the tool's schema once, then gives up;
  - `pruneMessages` drops old tool results but never reasoning, because DeepSeek returns 400
    when `reasoning_content` is missing on a tool turn.
- An explicit retry for DeepSeek's documented empty-content response.
- `app/composables/useBookAgent.ts` wraps `useChat` from `@ai-sdk/vue` over a
  `DirectChatTransport`. After every approved write it refreshes `useBook`'s layout from the
  command's result, so the book on screen is always what Rust saved.

## Data structures

- No new wire types. The chat's messages are `UIMessage`s and stay in memory. They are not
  persisted in this phase.

## Verification

- With a scripted model (`MockLanguageModel` from `ai/test`):
  - a write tool call stops for approval and does not call `agent_edit` until approved;
  - a denial leaves the book unchanged;
  - after 20 steps the loop stops.
- A pruned history still carries reasoning on every assistant tool turn.

## As built

- `createBookAgent(projectId, { model, invoke, jev })` takes the model instead of building it,
  so tests script it. `useBookAgent` builds it: `createDeepSeek` over `modelFetch("deepseek")`,
  model `deepseek-v4-flash` as an explicit string (design §9.2). The SDK's `apiKey` is a
  placeholder that never leaves the webview.
- `toolApproval` asks the user about every write and nothing else. Before asking, it runs Jev's
  proposal check. Below `CHECK_WARN_BELOW` (0.5) the approval request carries a reason
  ("This may not be what you asked for (31% match)."). With no answer from Jev, the request
  carries no reason. The threshold is chosen, not measured, like `ROUTE_CONFIDENCE`.
- `prepareStep` prunes every step with `pruneHistory`. It keeps tool calls and results for the
  last 6 messages and never prunes reasoning. On step 0 only, it applies Jev's route to
  `activeTools`.
- `repairToolCall` handles only `InvalidToolInputError`. It pulls the JSON object out of
  fenced or wrapped text and hands it back. The SDK validates the repaired input against the
  tool's schema once more, so the repair does not check the schema itself. An unknown tool
  name is not repaired.
- The empty-response retry is a `wrapLanguageModel` middleware. `generate` retries once when
  the result has no text and no tool call. `stream` buffers until the first text or tool call.
  If the stream finishes first, it retries once. Otherwise it replays the buffered parts.
- `stopWhen: isStepCount(20)` matches the SDK's default. It is set explicitly so the limit is
  part of this file, and the test pins it at 20.
- `savedEdits(messages)` names the write calls Rust saved (output available, not a refusal).
  `useBookAgent` calls `onBookChanged` after any turn that adds one. `useBook.refreshLayout()`
  reloads `book_layout`, because `agent_edit` returns an `AgentView`, not a `BookLayout`.
- Jev decisions go to the app log through `@tauri-apps/plugin-log` (`log:default` capability).
  The Rust plugin was already registered.
- There is no UI in this phase, so the manual is unchanged. Phase 8 documents the chat panel.

### Mutation evidence

`tests/agent-loop.test.ts` (33 tests), each mutation applied alone:

| Mutation | Failed |
|---|---|
| Writes need no approval | 9 |
| Reads need approval | 4 |
| Warn threshold `>=` becomes `>` | 1 |
| Never warn | 2 |
| DeepSeek thinking enabled | 2 |
| Route applied on every step | 1 |
| Route ignored | 1 |
| No repair | 1 |
| Step limit 30 / `MAX_STEPS` 25 | 1 / 1 |
| No generate retry / no stream retry / stream always retries | 2 / 2 / 2 |
| Prune reasoning / prune no tool calls / prune all tool calls | 1 / 1 / 1 |
| `savedEdits` keeps refusals / reads / any state | 1 / 1 / 3 |
| `lastUserText` reads the first user message | 1 |

Removing `stopWhen` survives. That mutation is equivalent, because the SDK default is the same
20 steps.
