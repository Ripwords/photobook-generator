import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineNuxtConfig } from "nuxt/config";

/**
 * `bun run ui:mock` opens the webview in an ordinary browser with the Tauri
 * bridge replaced by `dev/tauri-mock/`, so a screen can be rasterised and
 * looked at without a Tauri process. Off unless asked for; the production
 * build never sees these aliases.
 */
const mockTauri = process.env.PBG_MOCK_TAURI === "1";

/** The bundle's own version, for Settings > About, read where the bundle reads it. */
const appVersion: string = JSON.parse(
  readFileSync(fileURLToPath(new URL("./src-tauri/tauri.conf.json", import.meta.url)), "utf8"),
).version;

export default defineNuxtConfig({
  modules: ["@nuxt/ui"],
  ssr: false,
  devtools: { enabled: true },
  compatibilityDate: "2026-08-12",
  css: ["~/assets/css/main.css"],
  runtimeConfig: { public: { appVersion } },
  /*
   * Every icon ships in the bundle. The app's CSP (`connect-src`) blocks the
   * Iconify API, so an icon that is not bundled renders as nothing. `scan`
   * finds the ones named in app/; `icons` lists the ones Nuxt UI's own
   * components use, which the scan cannot see.
   */
  icon: {
    provider: "none",
    fallbackToApi: false,
    clientBundle: {
      scan: true,
      icons: [
        "lucide:check",
        "lucide:chevron-down",
        "lucide:chevron-up",
        "lucide:chevron-left",
        "lucide:chevron-right",
        "lucide:circle-alert",
        "lucide:circle-check",
        "lucide:circle-x",
        "lucide:copy",
        "lucide:copy-check",
        "lucide:ellipsis",
        "lucide:eye",
        "lucide:eye-off",
        "lucide:info",
        "lucide:loader-circle",
        "lucide:minus",
        "lucide:monitor",
        "lucide:moon",
        "lucide:plus",
        "lucide:search",
        "lucide:sun",
        "lucide:triangle-alert",
        "lucide:x",
        "lucide:arrow-down",
        "lucide:arrow-up",
      ],
    },
  },
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
            "@tauri-apps/api/window": fileURLToPath(
              new URL("./dev/tauri-mock/window.ts", import.meta.url),
            ),
          },
        }
      : undefined,
  },
  ignore: ["**/src-tauri/**"],
});
