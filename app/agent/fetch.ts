/**
 * A `fetch` for the AI SDK and the Jev client that goes through Rust's
 * `model_request` instead of the network. Rust picks the host and attaches
 * the key, so this sends only the URL's path and the body; the SDK's
 * placeholder host and `Authorization` header never leave the webview.
 *
 * The wire types below mirror `agent::request` and are pinned against
 * `tests/fixtures/wire/model-events.json` from both languages.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

/** What `@ai-sdk/provider-utils` calls `FetchFunction`. */
type FetchFunction = typeof globalThis.fetch;

/** Mirrors `agent::keys::Provider`. */
export type ModelProvider = "deepseek" | "jev";

/**
 * Mirrors `agent::request::ModelEvent`: at most one `head`, then `chunk`s,
 * then exactly one of `end`, `failed` or `cancelled`. `failed` can come
 * without a `head`.
 */
export type ModelEvent =
  | { kind: "head"; status: number; headers: [string, string][] }
  /** Raw body bytes; a chunk can end inside a UTF-8 character. */
  | { kind: "chunk"; bytes: number[] }
  | { kind: "end" }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" };

/** Mirrors `agent::request::ModelRequestError`: why Rust refused before connecting. */
export type ModelRequestError =
  | { kind: "missingKey"; provider: ModelProvider; message: string }
  | { kind: "refused"; message: string }
  | { kind: "keychain"; message: string };

/** No key is saved for `provider`; the UI answers this by asking for one. */
export class MissingKeyError extends Error {
  override name = "MissingKeyError";
  constructor(
    readonly provider: ModelProvider,
    message: string,
  ) {
    super(message);
  }
}

function isModelRequestError(value: unknown): value is ModelRequestError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    "message" in value &&
    typeof value.message === "string"
  );
}

function rejection(value: unknown): Error {
  if (!isModelRequestError(value)) return value instanceof Error ? value : new Error(String(value));
  if (value.kind === "missingKey") return new MissingKeyError(value.provider, value.message);
  return new Error(value.message);
}

const abortError = () => new DOMException("The model request was aborted.", "AbortError");

/** Statuses the `Response` constructor refuses to give a body. */
const NULL_BODY_STATUS = new Set([101, 103, 204, 205, 304]);

type Phase =
  | { at: "awaitingHead"; resolve: (response: Response) => void; reject: (error: unknown) => void }
  | { at: "streaming"; body: ReadableStreamDefaultController<Uint8Array> }
  | { at: "done" };

export function modelFetch(provider: ModelProvider): FetchFunction {
  return (input, init) => {
    const signal = init?.signal ?? undefined;
    if (signal?.aborted) return Promise.reject(abortError());
    const body = init?.body;
    if (typeof body !== "string") {
      return Promise.reject(new TypeError("modelFetch needs a string body in init.body."));
    }
    const url = input instanceof Request ? input.url : input.toString();
    const path = new URL(url, "http://model.invalid").pathname;

    const id = crypto.randomUUID();
    const onEvent = new Channel<ModelEvent>();

    return new Promise<Response>((resolve, reject) => {
      let phase: Phase = { at: "awaitingHead", resolve, reject };

      const finish = () => {
        phase = { at: "done" };
        signal?.removeEventListener("abort", abort);
      };
      /** Reports `error`, or a clean end when there is none, wherever the request had got to. */
      const stop = (error?: unknown) => {
        const was = phase;
        finish();
        if (was.at === "awaitingHead") was.reject(error);
        else if (was.at === "streaming") {
          if (error === undefined) was.body.close();
          else was.body.error(error);
        }
      };
      /**
       * Stops a request Rust is still running. Only reachable before a terminal
       * event: `finish` detaches the abort listener and `onmessage` ignores
       * everything once `done`.
       */
      const cancel = (error: unknown) => {
        stop(error);
        void invoke("cancel_model_request", { id });
      };
      const abort = () => cancel(abortError());
      signal?.addEventListener("abort", abort);

      const open = (status: number, headers: [string, string][]) => {
        let feed: ReadableStreamDefaultController<Uint8Array> | undefined;
        const stream = new ReadableStream<Uint8Array>({
          start: (controller) => {
            feed = controller;
          },
          // The reader already closed the stream, so there is nothing to report.
          cancel: () => {
            if (phase.at === "done") return;
            finish();
            void invoke("cancel_model_request", { id });
          },
        });
        const response = new Response(NULL_BODY_STATUS.has(status) ? null : stream, {
          status,
          headers,
        });
        return { response, body: feed! };
      };

      // Tauri's `Channel` has only this settable field, not `addEventListener`.
      // oxlint-disable-next-line unicorn/prefer-add-event-listener
      onEvent.onmessage = (event) => {
        const now = phase;
        if (now.at === "done") return;
        switch (event.kind) {
          case "head": {
            if (now.at !== "awaitingHead") return cancel(new TypeError("model_request sent a second head."));
            let opened: ReturnType<typeof open>;
            try {
              opened = open(event.status, event.headers);
            } catch (error) {
              return cancel(error);
            }
            phase = { at: "streaming", body: opened.body };
            return now.resolve(opened.response);
          }
          case "chunk":
            if (now.at !== "streaming") return cancel(new TypeError("model_request sent a chunk before its head."));
            return now.body.enqueue(new Uint8Array(event.bytes));
          case "end":
            return stop(
              now.at === "streaming" ? undefined : new TypeError("model_request ended without a head."),
            );
          case "failed":
            return stop(
              now.at === "streaming"
                ? new Error(event.message)
                : new TypeError("fetch failed", { cause: new Error(event.message) }),
            );
          case "cancelled":
            return stop(abortError());
        }
      };

      invoke("model_request", { id, provider, path, body, onEvent }).catch((error: unknown) => {
        if (phase.at !== "done") stop(rejection(error));
      });
    });
  };
}
