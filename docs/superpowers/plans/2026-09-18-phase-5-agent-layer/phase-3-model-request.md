# Phase 3: the model request command

[Overview](overview.md)

## Goal

Every outbound model request goes through one Rust command. It only reaches the two known
hosts, and it attaches the key itself.

## Changes

- New `src-tauri/src/agent/request.rs` and a command `model_request(provider, path, body,
  onChunk: Channel)`. The provider picks the base URL (`https://api.deepseek.com`,
  `https://api.typesafe.ai`), so the webview can never name a host. `path` is checked
  against that provider's allowed paths (`/chat/completions`, `/v1/systemone`).
- The response status and headers come back first. The body then streams through the
  channel as byte chunks, so DeepSeek's server-sent events reach the SDK as they arrive.
- `reqwest` with `rustls` and a long read timeout: the design notes DeepSeek can hold a
  connection for up to 10 minutes before inference starts.
- A missing key is a typed error the UI can turn into "Add your DeepSeek key in Settings".

## Data structures

- `ModelRequest { provider, path, body: String }`.
- `ModelEvent = Head { status, headers } | Chunk { bytes } | End | Failed { message }`.

## Verification

- Unit tests against a local test server (bound to 127.0.0.1) with the base URL injected:
  - the auth header is set from the store;
  - a path outside the allowlist is refused before any connection is made;
  - a chunked body arrives as several `Chunk` events in order;
  - a 429 is passed through with its `retry-after` header.
- Mutation: drop the path check and paste the failing test.
