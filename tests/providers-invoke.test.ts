import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SendEvent } from "../app/types/providers";

/**
 * The Rust commands are written in parallel and cannot run here, so this pins
 * the exact command names and camelCase arguments the contract fixes.
 */
const invoke = vi.hoisted(() => vi.fn());
const channels = vi.hoisted(() => [] as { onmessage: (message: unknown) => void }[]);
class RecordingChannel {
  onmessage: (message: unknown) => void = () => {};
  constructor() {
    channels.push(this);
  }
}
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: RecordingChannel }));

const { useProviders } = await import("../app/composables/useProviders");

describe("useProviders invoke payloads", () => {
  beforeEach(() => {
    invoke.mockReset();
    channels.length = 0;
  });

  it("asks for status, sign-in and sign-out with the provider id", async () => {
    invoke.mockResolvedValue({ signedIn: true, email: "a@b.c" });
    const pixajoy = useProviders(7);
    await pixajoy.refreshStatus();
    await pixajoy.signIn();
    invoke.mockResolvedValue(undefined);
    await pixajoy.signOut();
    expect(invoke.mock.calls).toEqual([
      ["provider_status", { provider: "pixajoy" }],
      ["provider_sign_in", { provider: "pixajoy" }],
      ["provider_sign_out", { provider: "pixajoy" }],
    ]);
    expect(pixajoy.status.value).toEqual({ signedIn: false, email: null });
  });

  it("shows a rejected plan's message and keeps no plan", async () => {
    invoke.mockRejectedValue("This book has 152 pages. Pixajoy takes at most 150.");
    const pixajoy = useProviders(7);
    await pixajoy.loadPlan();
    expect(invoke).toHaveBeenCalledWith("provider_send_plan", { projectId: 7, provider: "pixajoy" });
    expect(pixajoy.plan.value).toBeNull();
    expect(pixajoy.planError.value).toBe("This book has 152 pages. Pixajoy takes at most 150.");
  });

  it("follows the channel's events and ends on the last one", async () => {
    const pixajoy = useProviders(7);
    invoke.mockImplementation(async (command: string) => {
      expect(command).toBe("send_to_provider");
      const [channel] = channels;
      const events: SendEvent[] = [
        { kind: "uploading", done: 3, total: 4 },
        { kind: "done", projectName: "Trip", openUrl: "https://x" },
      ];
      channel?.onmessage(events[0]);
      expect(pixajoy.view.value).toMatchObject({ kind: "sending", progress: 0.75 });
      expect(pixajoy.sending.value).toBe(true);
      channel?.onmessage(events[1]);
    });
    await pixajoy.send();
    expect(invoke).toHaveBeenCalledWith("send_to_provider", {
      projectId: 7,
      provider: "pixajoy",
      onEvent: channels[0],
    });
    expect(pixajoy.view.value).toEqual({ kind: "done", projectName: "Trip", openUrl: "https://x" });
    expect(pixajoy.sending.value).toBe(false);
  });

  it("turns a rejected send into a failure", async () => {
    invoke.mockRejectedValue(new Error("connection reset"));
    const pixajoy = useProviders(7);
    await pixajoy.send();
    expect(pixajoy.view.value).toEqual({ kind: "failed", message: "connection reset", leftover: null });
    expect(pixajoy.sending.value).toBe(false);
  });

  it("keeps a failed event's partial project when the invoke also rejects", async () => {
    const pixajoy = useProviders(7);
    invoke.mockImplementation(async () => {
      channels[0]?.onmessage({ kind: "failed", message: "Upload failed", projectName: "Trip" });
      throw new Error("Upload failed");
    });
    await pixajoy.send();
    expect(pixajoy.view.value).toMatchObject({ kind: "failed", leftover: expect.stringContaining("Trip") });
  });

  it("opens My Projects through Rust", async () => {
    invoke.mockResolvedValue(undefined);
    await useProviders(7).openProjects();
    expect(invoke).toHaveBeenCalledWith("open_provider_projects", { provider: "pixajoy" });
  });
});
