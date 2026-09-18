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
