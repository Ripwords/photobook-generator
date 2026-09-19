import { describe, expect, it } from "vitest";
import {
  type UpdateInfo,
  type UpdaterState,
  updaterMessage,
  updaterReducer,
} from "../app/types/updater";

const IDLE: UpdaterState = { phase: "idle" };
const DOWNLOADING: UpdaterState = { phase: "downloading", percent: null };
const RELEASE: UpdateInfo = { version: "0.2.0", notes: "Faster contact sheet." };

describe("a check that finds nothing", () => {
  it("stays quiet when it ran at startup and says so when the user asked", () => {
    const startup = updaterReducer({ phase: "checking" }, { type: "checked", update: null, silent: true });
    const asked = updaterReducer({ phase: "checking" }, { type: "checked", update: null, silent: false });

    expect(startup).toEqual({ phase: "idle" });
    expect(asked).toEqual({ phase: "uptodate" });
    expect(startup.phase).not.toBe(asked.phase);
  });
});

describe("a check that fails", () => {
  it("stays quiet when it ran at startup, carrying no message to show", () => {
    const state = updaterReducer(
      { phase: "checking" },
      { type: "failed", message: "Network unreachable", silent: true },
    );

    expect(state).toEqual({ phase: "idle" });
    expect(state.phase).toBe("idle");
    expect("message" in state).toBe(false);
    expect(JSON.stringify(state)).not.toContain("Network unreachable");
  });

  it("surfaces the message when the user asked", () => {
    expect(
      updaterReducer({ phase: "checking" }, { type: "failed", message: "Network unreachable", silent: false }),
    ).toEqual({ phase: "error", message: "Network unreachable" });
  });
});

describe("a check that finds an update", () => {
  it("offers it whether the check was silent or explicit", () => {
    for (const silent of [true, false]) {
      expect(updaterReducer({ phase: "checking" }, { type: "checked", update: RELEASE, silent })).toEqual({
        phase: "available",
        update: RELEASE,
      });
    }
  });

  it("passes absent release notes through as null", () => {
    const state = updaterReducer(
      { phase: "checking" },
      { type: "checked", update: { version: "0.2.0", notes: null }, silent: false },
    );

    expect(state).toEqual({ phase: "available", update: { version: "0.2.0", notes: null } });
  });
});

describe("download progress", () => {
  it("has no percent to show until the first progress event", () => {
    expect(updaterReducer({ phase: "available", update: RELEASE }, { type: "download" })).toEqual({
      phase: "downloading",
      percent: null,
    });
  });

  it("reads `downloaded` as the running total rather than adding it to the percent so far", () => {
    const first = updaterReducer(DOWNLOADING, { type: "progress", downloaded: 30, total: 100 });
    expect(first).toEqual({ phase: "downloading", percent: 30 });

    const second = updaterReducer(first, { type: "progress", downloaded: 50, total: 100 });
    expect(second).toEqual({ phase: "downloading", percent: 50 });
  });

  it("shows an indeterminate bar when the server sent no content length", () => {
    expect(updaterReducer(DOWNLOADING, { type: "progress", downloaded: 1024, total: null })).toEqual({
      phase: "downloading",
      percent: null,
    });
    expect(updaterReducer(DOWNLOADING, { type: "progress", downloaded: 1024, total: 0 })).toEqual({
      phase: "downloading",
      percent: null,
    });
  });

  it("rounds to a whole percent", () => {
    expect(updaterReducer(DOWNLOADING, { type: "progress", downloaded: 1, total: 3 })).toEqual({
      phase: "downloading",
      percent: 33,
    });
    expect(updaterReducer(DOWNLOADING, { type: "progress", downloaded: 2, total: 3 })).toEqual({
      phase: "downloading",
      percent: 67,
    });
  });

  it("clamps both ends, so a mis-sized download never reads past 100 or below 0", () => {
    expect(updaterReducer(DOWNLOADING, { type: "progress", downloaded: 150, total: 100 })).toEqual({
      phase: "downloading",
      percent: 100,
    });
    expect(updaterReducer(DOWNLOADING, { type: "progress", downloaded: -20, total: 100 })).toEqual({
      phase: "downloading",
      percent: 0,
    });
  });
});

describe("the rest of the lifecycle", () => {
  it("moves to checking when a check starts, silent or not", () => {
    expect(updaterReducer(IDLE, { type: "check", silent: true })).toEqual({ phase: "checking" });
    expect(updaterReducer(IDLE, { type: "check", silent: false })).toEqual({ phase: "checking" });
  });

  it("shows the restart notice once the download is installed", () => {
    expect(updaterReducer(DOWNLOADING, { type: "installed" })).toEqual({ phase: "installing" });
  });

  it("goes back to idle when the offer is dismissed", () => {
    expect(updaterReducer({ phase: "available", update: RELEASE }, { type: "dismiss" })).toEqual({
      phase: "idle",
    });
  });
});

describe("updaterMessage", () => {
  const FALLBACK = "Could not check for updates.";

  it("uses the plain string Tauri rejects with", () => {
    expect(updaterMessage("signature mismatch", FALLBACK)).toBe("signature mismatch");
  });

  it("trims the string it was handed", () => {
    expect(updaterMessage("  signature mismatch\n", FALLBACK)).toBe("signature mismatch");
  });

  it("falls back for an empty string", () => {
    expect(updaterMessage("", FALLBACK)).toBe(FALLBACK);
  });

  it("falls back for a string that is only whitespace", () => {
    expect(updaterMessage("   ", FALLBACK)).toBe(FALLBACK);
    expect(updaterMessage("\n\t ", FALLBACK)).toBe(FALLBACK);
  });

  it("uses an Error's message, and falls back when it is empty", () => {
    expect(updaterMessage(new Error("endpoint unreachable"), FALLBACK)).toBe("endpoint unreachable");
    expect(updaterMessage(new Error(""), FALLBACK)).toBe(FALLBACK);
  });

  it("falls back for anything that is neither", () => {
    expect(updaterMessage(null, FALLBACK)).toBe(FALLBACK);
    expect(updaterMessage(undefined, FALLBACK)).toBe(FALLBACK);
    expect(updaterMessage(404, FALLBACK)).toBe(FALLBACK);
    expect(updaterMessage({}, FALLBACK)).toBe(FALLBACK);
    expect(updaterMessage({ message: "not an Error" }, FALLBACK)).toBe(FALLBACK);
  });
});
