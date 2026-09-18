# Phase 8: the chat panel

[Overview](overview.md)

## Goal

The user can talk to the book, see each proposed edit with Jev's check on it, and approve
or deny it.

## Changes

- `BookEditor.vue`: the scrolling `main` and a new `aside` sit side by side. The panel
  collapses to a button in the header.
- New `BookChat.vue`. It shows the message list, and each proposed write as a card: the
  plain-language edit, the opening it touches, Jev's warning when the check is low, and
  **Apply** and **Don't apply** buttons. Refusals show the engine's reason.
- New `ApiKeys.vue` in a modal reached from the header. It stores and clears keys and shows
  which are set.
- `dev/tauri-mock/`: answers `agent_view`, the key commands and `model_request` with
  scripted streams, so every state can be driven in the browser.

## Verification

- `bun run check:build`, then drive the harness with the scratchpad Playwright script:
  - a proposal card;
  - Apply updating the spread;
  - Don't apply leaving it unchanged;
  - a refusal message;
  - the no-key prompt.
- Rasterise light and dark, and check the panel's right edge and bottom.
- **Real run, not fakeable by the mock:** with both keys in the keychain, run `bun run dev`
  on a real project. Ask for a swap and a regenerate, approve one and deny one. Record the
  latency and each Jev decision from the local log.
