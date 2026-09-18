/**
 * See `core.ts`. A browser tab has no native window to close, so the close
 * request never fires and destroying does nothing. `pbgRequestClose()` from
 * the console (or a browser automation tool) runs the registered handler the
 * way the red traffic light would, which is how the quit confirmation gets
 * driven.
 */
type CloseHandler = (event: { preventDefault: () => void }) => void | Promise<void>;

declare global {
  interface Window {
    pbgRequestClose?: () => Promise<"closed" | "kept">;
  }
}

const handlers: CloseHandler[] = [];

globalThis.window.pbgRequestClose = async () => {
  let prevented = false;
  for (const handler of handlers) {
    await handler({ preventDefault: () => (prevented = true) });
  }
  return prevented ? "kept" : "closed";
};

export function getCurrentWindow() {
  return {
    async onCloseRequested(handler: CloseHandler): Promise<() => void> {
      handlers.push(handler);
      return () => handlers.splice(handlers.indexOf(handler), 1);
    },
    async destroy() {
      console.info("[tauri-mock] window destroyed");
    },
  };
}
