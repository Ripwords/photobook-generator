import { describe, expect, it } from "vitest";
import config from "../nuxt.config";

describe("nuxt config", () => {
  it("disables SSR because Tauri has no server", () => {
    expect(config.ssr).toBe(false);
  });

  it("does not ignore the app directory", () => {
    expect(config.ignore).toContain("**/src-tauri/**");
  });
});
