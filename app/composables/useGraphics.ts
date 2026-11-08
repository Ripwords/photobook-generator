import { Channel, convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  type GraphicsLevel,
  type GraphicsMode,
  type GraphicsStatus,
  type PreviewFiles,
  type PreviewItem,
  photoFile,
} from "~/types/graphics";
import type { PreviewPhoto } from "~/types/preview";

/** Bumped by every pass, so a batch from a superseded one is dropped. */
let latestPass = 0;

/**
 * The Graphics setting and the sharper previews it asks for. Every editor
 * image draws through `src`, which shows the thumbnail at once and the
 * sharper file as soon as its batch lands, in the same box, so nothing
 * shifts. `status` is `null` until the first answer.
 */
export function useGraphics() {
  const status = useState<GraphicsStatus | null>("graphics-status", () => null);
  const previews = useState<PreviewFiles>("graphics-previews", () => ({}));
  /** The level `previews` was written at; a different one empties it. */
  const previewsLevel = useState<GraphicsLevel | null>("graphics-previews-level", () => null);
  const error = ref<string | null>(null);

  async function run(command: string, args?: Record<string, unknown>) {
    error.value = null;
    try {
      status.value = await invoke<GraphicsStatus>(command, args);
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  }

  /** Writes and streams in the previews for `items`, superseding any pass still running. */
  async function sharpen(items: PreviewItem[]) {
    const level = status.value?.effective ?? null;
    const pass = ++latestPass;
    if (level !== previewsLevel.value) {
      previews.value = {};
      previewsLevel.value = level;
    }
    if (level === null || level === "low" || items.length === 0) return;
    const onBatch = new Channel<PreviewFiles>();
    // Tauri's `Channel` has no `addEventListener`, only this settable field.
    // oxlint-disable-next-line unicorn/prefer-add-event-listener
    onBatch.onmessage = (batch) => {
      if (pass === latestPass) previews.value = { ...previews.value, ...batch };
    };
    try {
      await invoke("photo_previews", { items, onBatch });
    } catch (e) {
      // The thumbnails stay; a failed pass costs sharpness, never a photo.
      console.warn("photo previews failed", e);
    }
  }

  return {
    status,
    error,
    refresh: () => run("graphics_status"),
    setMode: (mode: GraphicsMode) => run("set_graphics_mode", { mode }),
    sharpen,
    src: (photo: Pick<PreviewPhoto, "hash" | "thumbnailPath">) => {
      const file = photoFile(photo, previews.value);
      return file ? convertFileSrc(file) : null;
    },
  };
}
