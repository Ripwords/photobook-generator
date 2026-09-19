/**
 * See `core.ts`. A browser tab cannot replace the bundle it is running, so the
 * check answers from a fixed release and the download is a row of timers.
 *
 * Set `window.pbgUpdate` from the console (or from a browser automation tool)
 * to reach the branches the default does not: `"none"` for the up-to-date
 * answer, `"fail"` for a check that rejects with a plain string the way the
 * real bridge does, and `"unsized"` for a download whose Started event carries
 * no content length, which is what drives the indeterminate bar.
 */
type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

export interface Update {
  version: string;
  body?: string;
  downloadAndInstall(onEvent: (event: DownloadEvent) => void): Promise<void>;
}

declare global {
  interface Window {
    pbgUpdate?: "none" | "fail" | "unsized";
  }
}

const NOTES = [
  "Contact sheet",
  "  - Culling a photo no longer rescores the whole spread.",
  "  - Faces found at the edge of a frame keep their crop.",
  "",
  "Export",
  "  - The gutter dead zone is honoured on the last spread.",
].join("\n");

const TOTAL_BYTES = 24 * 1024 * 1024;
const CHUNKS = 12;
const CHUNK_MS = 220;

export async function check(): Promise<Update | null> {
  if (globalThis.window?.pbgUpdate === "none") return null;
  if (globalThis.window?.pbgUpdate === "fail") throw "Could not reach the update server.";

  return {
    version: "0.2.0",
    body: NOTES,
    async downloadAndInstall(onEvent) {
      const sized = globalThis.window?.pbgUpdate !== "unsized";
      onEvent({ event: "Started", data: sized ? { contentLength: TOTAL_BYTES } : {} });
      for (let sent = 0; sent < CHUNKS; sent++) {
        await new Promise((resolve) => setTimeout(resolve, CHUNK_MS));
        onEvent({ event: "Progress", data: { chunkLength: TOTAL_BYTES / CHUNKS } });
      }
      onEvent({ event: "Finished" });
    },
  };
}
