import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { applyRewrite, stampReadme, versionRewrites } from "../scripts/release-stamp";

const link = (ver: string) =>
  `https://github.com/Ripwords/photobook-generator/releases/download/v${ver}/PhotobookGen_${ver}_aarch64.dmg`;

const repoFile = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

describe("stampReadme", () => {
  it("moves the tag and the filename together", () => {
    expect(stampReadme(`<a href="${link("0.1.0")}">`, "0.2.0")).toBe(`<a href="${link("0.2.0")}">`);
  });

  it("leaves the unversioned latest-release badge alone", () => {
    const latest = "https://github.com/Ripwords/photobook-generator/releases/latest";
    expect(stampReadme(`${link("0.1.0")}\n${latest}`, "1.0.0")).toBe(`${link("1.0.0")}\n${latest}`);
  });

  it("refuses a README with no download link", () => {
    expect(() => stampReadme("no links here", "0.2.0")).toThrow(/expected 1 download link.*rewrote 0/);
  });

  it("refuses a link naming a different bundle", () => {
    const renamed = link("0.1.0").replace("PhotobookGen_", "Photobook.Gen_");
    expect(() => stampReadme(renamed, "0.2.0")).toThrow(/rewrote 0/);
  });

  it("refuses two download links rather than stamping both", () => {
    expect(() => stampReadme(`${link("0.1.0")} ${link("0.1.0")}`, "0.2.0")).toThrow(/rewrote 2/);
  });

  it("leaves a README already on that version byte-identical, so a rerun commits nothing", () => {
    const stamped = `<a href="${link("0.2.0")}">`;
    expect(stampReadme(stamped, "0.2.0")).toBe(stamped);
  });

  it("stamps the real README", () => {
    const readme = repoFile("README.md");
    const out = stampReadme(readme, "9.9.9");
    expect(out).toContain(link("9.9.9"));
    expect(out.replace(link("9.9.9"), "")).not.toContain("/releases/download/");
  });
});

describe("versionRewrites", () => {
  it("rewrites exactly one version field in every real file", () => {
    for (const rewrite of versionRewrites("9.9.9")) {
      const before = repoFile(rewrite.file);
      const after = applyRewrite(before, rewrite);
      expect(after, rewrite.file).not.toBe(before);
      expect(after, rewrite.file).toContain("9.9.9");
    }
  });

  it("covers the four files that carry the app version", () => {
    expect(versionRewrites("1.0.0").map((r) => r.file)).toEqual([
      "package.json",
      "src-tauri/tauri.conf.json",
      "src-tauri/Cargo.toml",
      "src-tauri/Cargo.lock",
    ]);
  });

  it("names the file when its version field is missing", () => {
    const [cargoLock] = versionRewrites("1.0.0").filter((r) => r.file === "src-tauri/Cargo.lock");
    expect(() => applyRewrite('name = "other-crate"\nversion = "0.1.0"', cargoLock!)).toThrow(/Cargo\.lock/);
  });
});
