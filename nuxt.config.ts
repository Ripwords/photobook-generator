import { defineNuxtConfig } from "nuxt/config";

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
  },
  ignore: ["**/src-tauri/**"],
});
