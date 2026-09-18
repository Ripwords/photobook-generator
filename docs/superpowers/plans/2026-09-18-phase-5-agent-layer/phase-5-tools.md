# Phase 5: the agent's tools

[Overview](overview.md)

## Goal

The agent's tools as plain `tool()` definitions, each one a thin mapping onto a command.
Every tool returns the resulting `AgentView`, never `{ ok: true }` (design §9, rule 1).

## Changes

- New `app/agent/tools.ts`. One table maps each tool to its zod input schema, whether it
  writes, and the `BookEdit` it produces.
  - Read tools: `get_book`, `get_opening`, `search_photos`.
  - Write tools: `regenerate`, `reject_layout`, `set_layout`, `set_locked`, `shuffle`,
    `swap_photos`, `set_crop`.
  - `set_slot` is left out. Moving a box by number is precision work the direct
    manipulation UI already does better.
- A refused edit comes back as the tool result `{ refused: <reason>, view }`, so the model
  reads the reason and the unchanged book.
- `search_photos` uses tag matching here. Phase 6 adds the Jev ranker behind the same
  signature.

## Data structures

- `AGENT_TOOLS: Record<ToolName, { input: ZodSchema, writes: boolean, toEdit?: (input) => BookEdit }>`.
- `ToolName` is the key union. The router's intent table (phase 6) is keyed by it.

## Verification

- Every write tool's `toEdit` produces a value that type-checks as `BookEdit` and appears in
  `book-edits.json` form.
- Every tool's result passes the phase 1 leak check (run the same value search in TS on the
  results from a mocked `invoke`).
- A refused edit returns the reason and the unchanged view.
