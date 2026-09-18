# Phase 4: a fetch that goes through Rust

[Overview](overview.md)

## Goal

A standard `fetch`-shaped function the AI SDK and the Jev client can use, which actually
calls `model_request`.

## Changes

- New `app/agent/fetch.ts`. `modelFetch(provider)` returns a `FetchFunction`. It maps the
  request's URL path and body onto `model_request` and builds a `Response` whose body is a
  `ReadableStream` fed by the channel. The SDK is given a placeholder base URL and API key,
  because the real ones never exist in the webview.
- The browser harness answers `model_request` with scripted responses
  (`dev/tauri-mock/model.ts`), so the chat panel can be driven without a network.

## Data structures

- `ModelEvent`, mirrored from phase 3's Rust enum, pinned by a wire fixture
  `tests/fixtures/wire/model-events.json`.

## Verification

- With a mocked `invoke`, a three-chunk event sequence reads back as the same bytes in order
  through `response.body`.
- A `Failed` event rejects the stream with its message.
- `Head` status and headers appear on the `Response`.
