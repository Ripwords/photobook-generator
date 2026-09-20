import { describe, expect, it } from "vitest";
import { useBusy } from "../app/composables/useBusy";

/** A promise this test resolves by hand, so two guards can be in flight at once. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useBusy", () => {
  it("is busy for the length of one piece of work", async () => {
    const { busy, guard } = useBusy();
    const work = deferred<number>();

    expect(busy.value).toBe(false);
    const done = guard(() => work.promise);
    expect(busy.value).toBe(true);

    work.resolve(7);
    expect(await done).toBe(7);
    expect(busy.value).toBe(false);
  });

  // The bug this exists for: a boolean flag is set by both and cleared by
  // whichever returns first, so every `:disabled="busy"` control on the editor
  // -- Export, Undo, Regenerate -- came back to life while the slower command
  // was still running. Overlap is ordinary here: the undo shortcut fires
  // whether or not a save is in flight, and the agent writes at the same time
  // as the user.
  it("stays busy until the LAST piece of work finishes", async () => {
    const { busy, guard } = useBusy();
    const first = deferred<string>();
    const second = deferred<string>();

    const a = guard(() => first.promise);
    const b = guard(() => second.promise);
    expect(busy.value).toBe(true);

    second.resolve("inner");
    await b;
    expect(busy.value, "the slower command is still running").toBe(true);

    first.resolve("outer");
    await a;
    expect(busy.value).toBe(false);
  });

  it("keeps the failure and stops being busy", async () => {
    const { busy, error, guard } = useBusy();

    expect(await guard(async () => { throw new Error("no disk"); })).toBeNull();
    expect(error.value).toBe("Error: no disk");
    expect(busy.value).toBe(false);
  });

  it("is still busy for the others when one of them fails", async () => {
    const { busy, guard } = useBusy();
    const slow = deferred<string>();
    const doomed = deferred<string>();

    const a = guard(() => slow.promise);
    const b = guard(() => doomed.promise);

    doomed.reject(new Error("refused"));
    expect(await b).toBeNull();
    expect(busy.value).toBe(true);

    slow.resolve("done");
    await a;
    expect(busy.value).toBe(false);
  });

  it("clears the last failure when fresh work starts", async () => {
    const { error, guard } = useBusy();

    await guard(async () => { throw new Error("no disk"); });
    expect(error.value).not.toBeNull();

    await guard(async () => "fine");
    expect(error.value).toBeNull();
  });
});
