import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  // Nuxt resolves `~` to `app/` at build time; plain vitest does not, so the
  // alias is restated here. Without it no file under `app/` that imports
  // `~/types/...` can be unit-tested at all -- which is how
  // `usePhotoOverrides` (the single mechanism keeping the culling rule out of
  // the webview) would have gone untested.
  resolve: { alias: { "~": fileURLToPath(new URL("./app", import.meta.url)) } },
  test: { include: ["tests/**/*.test.ts"] },
});
