# Phase 5: the agent layer

**Spec:** `docs/superpowers/specs/2026-08-12-photobook-generator-design.md` §9 (agent layer)
and §9.1 (privacy chokepoint). This plan changes three things §4.1 and §9 say, listed under
"Deviations from the design".

## Context

Phases 1 to 4 left a book that can be edited by hand through one command, `edit_book`. Every
write returns the whole `BookLayout` and a refusal comes back as plain English. Phase 5 puts
a chat panel beside the book so the user can say "swap the two beach photos" or "make chapter
2 calmer" and approve each edit the agent proposes.

DeepSeek (`deepseek-flash` via `@ai-sdk/deepseek`) does the reasoning. TypeSafe Jev
(`jev-latest`) is added in three narrow roles where a bounded, fast, typed answer is better
than a generated one:

1. **Router.** Before DeepSeek sees a request, Jev picks the intent from a fixed list. When
   it is confident, DeepSeek's callable tools are narrowed to that intent's tools
   (`activeTools`). When it is not confident, or no Jev key is set, every tool stays
   available. Jev never executes anything itself.
2. **Photo search ranker.** `search_photos` hands Jev the query and the derived photo facts
   in one request and asks one `choice` over photo ids. With no key it falls back to tag
   matching.
3. **Proposal check.** When DeepSeek proposes a write, Jev answers a `noul` question: does
   this edit do what the user asked? A low probability puts a warning on the approval card.
   It never approves or blocks on its own; the user still decides.

## Scope

Included:

- A Rust projection of a book into the only shape the agent may see (`AgentView`).
- API keys in the macOS keychain.
- One Rust command that makes every outbound model request.
- The agent's tools, the agent loop, the Jev client and its three roles.
- The chat panel with approval cards, the key settings, and the browser harness support.

Excluded:

- Any image, thumbnail, embedding or face box reaching a network request.
- New edit kinds. The agent drives exactly the `BookEdit` variants that exist.
- Adding or removing slots, and text boxes (still Phase 4 remainders).
- Coarse location labels. GPS clustering is not built, so the payload has no location.
- Photos the book left out cannot be added back. No `BookEdit` exists for it.

## Constraints

- **Images never leave the machine.** Only `AgentView` JSON and the user's own typed
  messages leave. The leak test in phase 1 is the enforcement, not a convention.
- **Keys never enter the webview.** Rust reads the keychain and attaches the header.
- **Determinism of the engine is untouched.** The agent only calls `agent_edit`, which runs
  the same `book::edit::apply` as `edit_book` and is already deterministic. Nothing
  model-driven runs inside `assemble`, `score` or `cull`.
- Bun, Conventional Commits, no `any`, oxlint warnings fail, `bun run check:build` before
  any commit touching `.vue`, `bun run sidecar` before `cargo`.
- Mutation-check every load-bearing test and paste the evidence in the commit message.

## Deviations from the design

| Design says | This plan does | Why |
|---|---|---|
| `buildAgentPayload()` in TypeScript | `agent::view` in Rust, one command `agent_view` | The editor holds no per-photo features. Rust holds them (`resolve_photos`). Tool results also go to the model, so the chokepoint has to cover them too. It is easiest to enforce where the data lives. |
| `@tauri-apps/plugin-http` scoped to `api.deepseek.com` | A custom `model_request` command with a host allowlist | The plugin's `fetch` is called from JS, so the key would pass through the webview. |
| `needsApproval` on each tool | `toolApproval` on the agent | `needsApproval` is deprecated in `ai` 7. |
| Tools call `edit_book` (plan text before step 5 landed) | Tools call `agent_edit`, which answers with the `AgentView` and fails with `AgentError` | `edit_book` answers with a `BookLayout` (file names) and fails with free text, which can include the app data path. `AgentError` is `refused { reason }` (built from `EditError`, which holds only indices and template ids) or `failed` with no text, so the type rules the leak out. |
| `deepseek-v4-flash` | `deepseek-flash` | DeepSeek's primary id now. The old name still works, as an alias of the same model. |

## Alternatives considered

| Option | Verdict |
|---|---|
| Run the agent loop in Rust and keep the webview a thin chat view | Rejected. It reimplements tool loops, approvals and streaming that `ai` 7 already provides, and the design chose the SDK. |
| Webview fetches DeepSeek directly with a CSP exception | Rejected. The key would sit in JS and the CSP would open to the internet. |
| Jev as the only model, no DeepSeek | Rejected. Jev cannot produce tool arguments such as a template id or a crop rectangle. |
| DeepSeek only, no Jev | Kept as the fallback path. Every Jev role has a no-key behaviour, so Jev is additive. |

## Phases

1. [Agent view in Rust](phase-1-agent-view.md)
2. [Keys in the keychain](phase-2-keys.md)
3. [The model request command](phase-3-model-request.md)
4. [A fetch that goes through Rust](phase-4-fetch-adapter.md)
5. [The agent's tools](phase-5-tools.md)
6. [The Jev client and its three roles](phase-6-jev.md)
7. [The agent loop](phase-7-agent-loop.md)
8. [The chat panel](phase-8-chat-panel.md)
9. [Manual and status](phase-9-docs.md)

Phases 1 to 3 are Rust and independent of each other. Phases 4 to 7 are TypeScript and
depend on the Rust commands only through their wire shapes, so they can use a mocked
`invoke`. Phase 8 needs all of them.

## Verification

```sh
bun run sidecar && bun run test:rust
bun run test && bun run lint && bun run check:build
bun run ui:mock   # drive the chat panel against scripted model responses
```

Runtime verification of the real models needs a DeepSeek key and a Jev key in the keychain
and `bun run dev`. It is listed as its own step in phase 8 and is not fakeable by the mock.

## Implementation guidance

- **how** over `book::edit` and `preview.rs` before changing either.
- **interrogate** over phase 1's payload shape and phase 3's allowlist before merging them.
  These are the two privacy boundaries.
- `/deslop` over each diff before commit. **unslop** over the manual and status text.
- The project's own rule: break the thing, confirm the test notices, paste the evidence.
