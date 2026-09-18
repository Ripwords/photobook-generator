import { fileURLToPath } from "node:url";
import { defineNuxtConfig } from "nuxt/config";

/**
 * `bun run ui:mock` opens the webview in an ordinary browser with the Tauri
 * bridge replaced by `dev/tauri-mock/`, so a screen can be rasterised and
 * looked at without a Tauri process. Off unless asked for; the production
 * build never sees these aliases.
 */
const mockTauri = process.env.PBG_MOCK_TAURI === "1";

export default defineNuxtConfig({
  modules: ["@nuxt/ui"],
  ssr: false,
  devtools: { enabled: true },
  compatibilityDate: "2026-08-12",
  css: ["~/assets/css/main.css"],
  vite: {
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_"],
    server: { strictPort: true },
    resolve: mockTauri
      ? {
          alias: {
            "@tauri-apps/api/core": fileURLToPath(new URL("./dev/tauri-mock/core.ts", import.meta.url)),
            "@tauri-apps/plugin-dialog": fileURLToPath(
              new URL("./dev/tauri-mock/dialog.ts", import.meta.url),
            ),
            "@tauri-apps/plugin-log": fileURLToPath(new URL("./dev/tauri-mock/log.ts", import.meta.url)),
          },
        }
      : undefined,
  },
  ignore: ["**/src-tauri/**"],
});
