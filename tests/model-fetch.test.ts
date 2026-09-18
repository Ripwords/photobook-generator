import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createDeepSeek } from "@ai-sdk/deepseek";
import { APICallError, streamText } from "ai";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelEvent, ModelRequestError } from "../app/agent/fetch";
import type { ModelRequestArgs } from "../dev/tauri-mock/model";

/**
 * `modelFetch` is the only way a model request leaves the webview, so these
 * tests drive it the way Rust does: a mocked `invoke` that hands its channel
 * events one at a time, in the shapes `tests/fixtures/wire/model-events.json`
 * pins against `agent::request::ModelEvent` and `ModelRequestError`.
 */
const { invoke, FakeChannel } = vi.hoisted(() => ({
  invoke: vi.fn(),
  FakeChannel: class<T> {
    onmessage: (message: T) => void = () => {};
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: FakeChannel }));

const { MissingKeyError, modelFetch } = await import("../app/agent/fetch");
const harness = await import("../dev/tauri-mock/model");

interface WireFixture {
  stream: ModelEvent[];
  failed: ModelEvent;
  cancelled: ModelEvent;
  errors: ModelRequestError[];
}
const wire: WireFixture = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/wire/model-events.json", import.meta.url)), "utf8"),
) as WireFixture;

interface Sent {
  id: string;
  provider: string;
  path: string;
  body: string;
  onEvent: { onmessage: (event: ModelEvent) => void };
}

/**
 * Stands in for Rust. `model_request` stays pending until the test settles
 * it, and the test pushes events through the channel by hand, so each test
 * decides exactly where an abort or a failure lands.
 */
function rust() {
  const requests: Sent[] = [];
  const cancels: string[] = [];
  let settle: { resolve: () => void; reject: (error: unknown) => void } | undefined;
  invoke.mockImplementation((command: string, args: Sent) => {
    if (command === "cancel_model_request") {
      cancels.push(args.id);
      return Promise.resolve();
    }
    if (command !== "model_request") return Promise.reject(new Error(`unexpected ${command}`));
    requests.push(args);
    return new Promise<void>((resolve, reject) => {
      settle = { resolve, reject };
    });
  });
  const request = () => {
    const sent = requests[0];
    if (!sent) throw new Error("model_request was never invoked");
    return sent;
  };
  return {
    requests,
    cancels,
    request,
    send: (...events: ModelEvent[]) => {
      for (const event of events) request().onEvent.onmessage(event);
    },
    finish: () => settle?.resolve(),
    reject: (error: unknown) => settle?.reject(error),
  };
}

const post = (body = "{}", signal?: AbortSignal) =>
  modelFetch("deepseek")("https://api.deepseek.com/chat/completions", {
    method: "POST",
    body,
    signal,
  });

const head: ModelEvent = { kind: "head", status: 200, headers: [] };

async function readAll(body: ReadableStream<Uint8Array> | null): Promise<number[][]> {
  if (!body) throw new Error("the response has no body");
  const reader = body.getReader();
  const reads: number[][] = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return reads;
    reads.push([...value]);
  }
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  invoke.mockReset();
});

describe("modelFetch's wire fixture", () => {
  it("builds the Response from the fixture's head and streams its bytes, split character and all", async () => {
    const r = rust();
    const pending = post();
    r.send(...wire.stream);
    r.finish();
    const response = await pending;

    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("text/event-stream; charset=utf-8");
    expect(response.headers.get("vary")).toBe("origin, accept-encoding");
    expect(await response.text()).toBe("data: €\n\n");
  });

  it("errors the stream with a failed event's message", async () => {
    const r = rust();
    const pending = post();
    r.send(head, { kind: "chunk", bytes: [1, 2] });
    const reader = (await pending).body!.getReader();

    expect([...((await reader.read()).value ?? [])]).toEqual([1, 2]);
    r.send(wire.failed);
    await expect(reader.read()).rejects.toThrow(
      "error decoding response body: connection reset",
    );
  });

  it("rejects missingKey with a MissingKeyError naming the provider", async () => {
    const missing = wire.errors.filter((e) => e.kind === "missingKey");
    expect(missing.map((e) => e.provider)).toEqual(["deepseek", "jev"]);
    for (const error of missing) {
      const r = rust();
      const pending = post();
      r.reject(error);
      const thrown = await pending.catch((e: unknown) => e);

      expect(thrown).toBeInstanceOf(MissingKeyError);
      expect(thrown).toMatchObject({ provider: error.provider, message: error.message });
    }
  });

  it("rejects refused and keychain errors with their message, and not as a missing key", async () => {
    const others = wire.errors.filter((e) => e.kind !== "missingKey");
    expect(others.map((e) => e.kind)).toEqual(["refused", "keychain"]);
    for (const error of others) {
      const r = rust();
      const pending = post();
      r.reject(error);
      const thrown = await pending.catch((e: unknown) => e);

      expect(thrown).toBeInstanceOf(Error);
      expect(thrown).not.toBeInstanceOf(MissingKeyError);
      expect((thrown as Error).message).toBe(error.message);
    }
  });
});

describe("modelFetch", () => {
  it("reads three chunks back as the same bytes, in order", async () => {
    const r = rust();
    const pending = post();
    r.send(
      head,
      { kind: "chunk", bytes: [104, 105] },
      { kind: "chunk", bytes: [0, 255, 7] },
      { kind: "chunk", bytes: [33] },
      { kind: "end" },
    );
    r.finish();

    expect(await readAll((await pending).body)).toEqual([[104, 105], [0, 255, 7], [33]]);
  });

  it("puts the head's status and headers on the Response", async () => {
    const r = rust();
    const pending = post();
    r.send({
      kind: "head",
      status: 429,
      headers: [
        ["retry-after", "3"],
        ["x-ratelimit-remaining-requests", "0"],
      ],
    });
    const response = await pending;

    expect(response.status).toBe(429);
    expect(response.ok).toBe(false);
    expect(response.headers.get("retry-after")).toBe("3");
    expect(response.headers.get("x-ratelimit-remaining-requests")).toBe("0");
  });

  it("stays pending until the head arrives, even after invoke resolves", async () => {
    const r = rust();
    let settled = false;
    const pending = post().finally(() => {
      settled = true;
    });
    r.finish();
    await flush();
    expect(settled).toBe(false);

    r.send(head, { kind: "end" });
    expect((await pending).status).toBe(200);
  });

  it("forwards only the URL's path and the body, never the host or the SDK's headers", async () => {
    const r = rust();
    const pending = modelFetch("jev")("https://attacker.example/v1/systemone?x=1#y", {
      method: "POST",
      headers: { Authorization: "Bearer placeholder", "content-type": "application/json" },
      body: '{"q":"beach"}',
    });
    r.send(head, { kind: "end" });
    await pending;

    const sent = r.request();
    expect(Object.keys(sent).toSorted()).toEqual(["body", "id", "onEvent", "path", "provider"]);
    expect(sent.provider).toBe("jev");
    expect(sent.path).toBe("/v1/systemone");
    expect(sent.body).toBe('{"q":"beach"}');
    expect(sent.id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(JSON.stringify(sent)).not.toContain("attacker");
    expect(JSON.stringify(sent)).not.toContain("placeholder");
  });

  it("gives every request its own id", async () => {
    const r = rust();
    const first = post();
    const second = post();
    for (const sent of r.requests) sent.onEvent.onmessage({ kind: "end" });
    await Promise.allSettled([first, second]);

    expect(r.requests).toHaveLength(2);
    expect(r.requests[0]?.id).not.toBe(r.requests[1]?.id);
  });

  it("refuses a body that is not a string without invoking anything", async () => {
    rust();
    await expect(
      modelFetch("deepseek")("https://api.deepseek.com/chat/completions", {
        method: "POST",
        body: new Uint8Array([123, 125]),
      }),
    ).rejects.toThrow(TypeError);
    await expect(
      modelFetch("deepseek")("https://api.deepseek.com/chat/completions", { method: "POST" }),
    ).rejects.toThrow(/string body/);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects like fetch when the request fails before any head", async () => {
    const r = rust();
    const pending = post();
    r.send(wire.failed);
    const thrown = await pending.catch((e: unknown) => e);

    expect(thrown).toBeInstanceOf(TypeError);
    expect((thrown as TypeError).message).toBe("fetch failed");
    expect(((thrown as TypeError).cause as Error).message).toBe(
      "error decoding response body: connection reset",
    );
  });

  it("errors the stream with an AbortError when Rust reports the request cancelled", async () => {
    const r = rust();
    const pending = post();
    r.send(head, wire.cancelled);
    const reader = (await pending).body!.getReader();

    await expect(reader.read()).rejects.toMatchObject({ name: "AbortError" });
  });
});

describe("aborting a model request", () => {
  it("before the head rejects with an AbortError and cancels the same request in Rust", async () => {
    const r = rust();
    const controller = new AbortController();
    const pending = post("{}", controller.signal);
    controller.abort();
    const thrown = await pending.catch((e: unknown) => e);

    expect(thrown).toBeInstanceOf(DOMException);
    expect((thrown as DOMException).name).toBe("AbortError");
    expect(r.cancels).toEqual([r.request().id]);
  });

  it("mid-stream errors the body with an AbortError and cancels the request in Rust", async () => {
    const r = rust();
    const controller = new AbortController();
    const pending = post("{}", controller.signal);
    r.send(head, { kind: "chunk", bytes: [1] });
    const reader = (await pending).body!.getReader();
    await reader.read();

    controller.abort();
    const thrown = await reader.read().catch((e: unknown) => e);

    expect(thrown).toBeInstanceOf(DOMException);
    expect((thrown as DOMException).name).toBe("AbortError");
    expect(r.cancels).toEqual([r.request().id]);

    r.send({ kind: "chunk", bytes: [2] }, { kind: "cancelled" });
    expect(r.cancels).toHaveLength(1);
  });

  it("with an already-aborted signal never invokes", async () => {
    rust();
    const thrown = await post("{}", AbortSignal.abort()).catch((e: unknown) => e);

    expect((thrown as DOMException).name).toBe("AbortError");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("after the end does not cancel anything", async () => {
    const r = rust();
    const controller = new AbortController();
    const pending = post("{}", controller.signal);
    r.send(head, { kind: "chunk", bytes: [9] }, { kind: "end" });
    const response = await pending;
    controller.abort();

    expect(await readAll(response.body)).toEqual([[9]]);
    expect(r.cancels).toEqual([]);
  });

  it("by cancelling the body cancels the request in Rust", async () => {
    const r = rust();
    const pending = post();
    r.send(head);
    await (await pending).body!.cancel();

    expect(r.cancels).toEqual([r.request().id]);
  });
});

describe("the AI SDK over modelFetch", () => {
  const deepseek = createDeepSeek({ fetch: modelFetch("deepseek"), apiKey: "placeholder" });

  it("streams the browser harness's scripted DeepSeek reply as text", async () => {
    const sent: ModelRequestArgs[] = [];
    invoke.mockImplementation((_command: string, args: ModelRequestArgs) => {
      sent.push(args);
      return harness.modelRequest(args, { pace: 0 });
    });

    const result = streamText({
      model: deepseek("deepseek-flash"),
      prompt: "Make chapter 2 calmer.",
      maxRetries: 0,
    });

    expect(await result.text).toBe(harness.DEEPSEEK_TEXT_REPLY);
    expect(await result.finishReason).toBe("stop");
    expect((await result.usage).outputTokens).toBe(12);
    expect(sent).toHaveLength(1);
    expect(sent[0]?.path).toBe("/chat/completions");
    expect(JSON.parse(sent[0]?.body ?? "")).toMatchObject({
      model: "deepseek-flash",
      stream: true,
      messages: [{ role: "user", content: "Make chapter 2 calmer." }],
    });
    expect(JSON.stringify(sent[0])).not.toContain("placeholder");
  });

  it("surfaces a failure before any head as the SDK's connection error", async () => {
    invoke.mockImplementation((_command: string, args: Sent) => {
      args.onEvent.onmessage({ kind: "failed", message: "connection refused" });
      return Promise.resolve();
    });

    let error: unknown;
    await streamText({
      model: deepseek("deepseek-flash"),
      prompt: "hi",
      maxRetries: 0,
      onError: (event) => {
        error = event.error;
      },
    }).consumeStream();

    expect(APICallError.isInstance(error)).toBe(true);
    expect((error as APICallError).message).toBe("Cannot connect to API: connection refused");
    expect((error as APICallError).isRetryable).toBe(true);
  });
});

function sink() {
  const events: ModelEvent[] = [];
  return { events, onEvent: { onmessage: (event: ModelEvent) => events.push(event) } };
}

describe("the browser harness", () => {
  it("answers Jev as overloaded", async () => {
    const { events, onEvent } = sink();
    await harness.modelRequest(
      { id: "j", provider: "jev", path: "/v1/systemone", body: "{}", onEvent },
      { pace: 0 },
    );
    const bytes = events.flatMap((e) => (e.kind === "chunk" ? e.bytes : []));

    expect(events[0]).toMatchObject({ kind: "head", status: 529 });
    expect(JSON.parse(new TextDecoder().decode(new Uint8Array(bytes)))).toEqual(
      harness.JEV_OVERLOADED,
    );
    expect(events.at(-1)).toEqual({ kind: "end" });
  });

  it("refuses a path Rust would refuse, before any event", async () => {
    const { events, onEvent } = sink();
    await expect(
      harness.modelRequest(
        { id: "x", provider: "deepseek", path: "/v1/models", body: "{}", onEvent },
        { pace: 0 },
      ),
    ).rejects.toMatchObject({ kind: "refused" });
    expect(events).toEqual([]);
  });

  it("stops with a cancelled event when the request is cancelled", async () => {
    const { events, onEvent } = sink();
    const running = harness.modelRequest(
      { id: "c", provider: "deepseek", path: "/chat/completions", body: "{}", onEvent },
      { pace: 5 },
    );
    await flush();
    harness.cancelModelRequest("c");
    await running;

    expect(events.at(-1)).toEqual({ kind: "cancelled" });
    expect(events.filter((e) => e.kind === "end")).toEqual([]);
  });
});
