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
  which are set. DeepSeek is required; Jev is marked optional. With no Jev key the agent
  still works: `createJev` reads the missing-key rejection as "off", so routing offers every
  tool, search uses `tagRanker`, no check runs, and nothing is written to the decision log.
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

## As built

- `app/agent/chat.ts` holds everything the panel derives from the AI SDK's messages, as pure
  functions. `messageBlocks` turns a message into text blocks and one proposal block per
  write. A proposal is `preparing`, `pending`, `applying`, `applied`, `refused`, `declined`
  or `failed`. `describeEdit` names the edit in the user's words, and the opening comes from
  the same `toSpreads` labels the book preview prints. `chatErrorText` is the transport's
  `onError`. A missing DeepSeek key becomes `NO_DEEPSEEK_KEY`, which the panel recognises and
  answers with **Add API key**.
- `useApiKeys` keeps the key status in `useState`, so the header, the modal and the panel
  share one copy. It re-reads `api_key_status` after every save or clear. A key is in the
  webview only while it is typed into the modal.
- The panel is open by default. **Chat** in the header folds it with `v-show`, so the
  conversation survives folding.
- The Jev loop now asks Jev once per proposal and routes once per user message. Resuming
  after an approval, the SDK calls `toolApproval` again for each approved call
  (`validateApprovedToolApprovals`), and it starts again at step 0. The re-ask can only deny,
  and ours never denies a write, so the second check was wasted. `wasShown` skips the check
  for a call that already has an approval request in the history. `prepareStep` routes only
  when the last message is the user's. In the harness this cut Jev calls from 11 to 6 over
  three proposals. `wasShown` assumes tool-call ids are unique within a conversation, as
  DeepSeek's are, so the harness now numbers its ids.
- Two Nuxt UI traps, both handled in the components:
  - With icons in CSS mode, changing a `UIcon`'s `name` on the same element generated the new
    rule with the old icon's SVG, so **Applied** showed a spinner. Every `UIcon` whose name
    changes is keyed by it (`BookEditor` save status, `BookChat` status line, `ApiKeys`).
  - `UChatMessages` decides whether to show its scroll-down button only on a scroll event or
    mid-stream. A stream that ended at the bottom left the button up. `BookChat` fires one
    `scroll` on its body when a stream ends. The button is placed against the nearest
    positioned ancestor, so the panel root is `relative`.
- Jev's warning is amber-700 / amber-300. `text-warning` on the card background was too faint
  to read.
- The manual does not exist yet; Phase 9 writes it, with the panel and the keys modal in it.

### Browser evidence

`bun run ui:mock`, driven by the scratchpad Playwright script at 1440×900, light and dark.
14 checks, all passing in both schemes, no console errors:

- the prompt is disabled and **Add API key** shows with no key;
- both keys read **Not set**, then **Saved**;
- the swap proposal names Page 2 and carries Jev's 30% warning;
- the book is unchanged while the proposal is pending;
- **Apply** swaps the two photos on screen, the same photos in new places;
- **Don't apply** leaves the book unchanged;
- a refusal shows the engine's reason and leaves the book unchanged;
- no scroll-down button once the thread ends at the bottom (fails with the `scroll` fix
  removed);
- **Chat** folds the panel away, and reopening keeps the conversation;
- clearing the key brings the no-key prompt back.

Checked by eye in both schemes: the panel's right edge (cards end 26 px inside the window),
the prompt at the bottom (not clipped), the keys modal, and the status icons.

### Mutation evidence

`tests/agent-chat.test.ts`, the panel's derivations: 11 of 11 mutants caught, among them an
approved read shown as declined, a refusal shown as applied, and a missing key shown as a
generic error.

The harness's scripts, same file, each mutation alone:

| Mutation | Failed |
|---|---|
| A refusal read back as saved | 1 |
| A denial read back as saved | 1 |
| The saved reply replaced by the denied one | 1 |
| "swap" no longer scripted | 2 |
| "last page" regenerates opening 1 | 1 |
| "layout" regenerates opening 2 | 1 |
| Jev's fit 0.9 instead of 0.3 | 1 |
| The fit question ignored | 1 |
| The swap's second photo on page 3 | 1 |

`tests/agent-loop.test.ts`, the Jev call guards:

| Mutation | Failed |
|---|---|
| No `wasShown` guard | 2 |
| `wasShown` matches any earlier request | 1 |
| `wasShown` matches only other calls | 2 |
| Route on every step 0 | 1 |
| Never route | 2 |

### Found, not fixed here

The built app has no icon data. No `@iconify-json/*` collection is installed, so Nuxt Icon
falls back to `api.iconify.design` at runtime. Every icon in the app loads over the network,
and offline none show. No image data is sent. It predates this phase and affects every
screen.

### Still to do

The real run with both keys in the keychain, described under Verification, needs the user.
