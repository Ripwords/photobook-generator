export interface Rewrite {
  file: string;
  pattern: RegExp;
  replacement: string;
}

/** Every file that carries the app version, and the one field in each that holds it. */
export const versionRewrites = (version: string): Rewrite[] => [
  { file: "package.json", pattern: /("version":\s*)"[^"]+"/g, replacement: `$1"${version}"` },
  { file: "src-tauri/tauri.conf.json", pattern: /("version":\s*)"[^"]+"/g, replacement: `$1"${version}"` },
  { file: "src-tauri/Cargo.toml", pattern: /^version = "[^"]+"$/gm, replacement: `version = "${version}"` },
  {
    file: "src-tauri/Cargo.lock",
    pattern: /(name = "photobook-generator"\nversion = )"[^"]+"/g,
    replacement: `$1"${version}"`,
  },
];

const countMatches = (text: string, pattern: RegExp) => text.match(pattern)?.length ?? 0;

/** Applies `rewrite`, throwing unless its pattern matched exactly once. */
export const applyRewrite = (text: string, rewrite: Rewrite): string => {
  const found = countMatches(text, rewrite.pattern);
  if (found !== 1) {
    throw new Error(`${rewrite.file}: expected 1 version field, found ${found} matching ${rewrite.pattern}`);
  }
  return text.replace(rewrite.pattern, rewrite.replacement);
};

/** How many versioned download buttons the README has: one, for Apple Silicon. */
export const README_DOWNLOAD_LINKS = 1;

/**
 * Points the README's download button at `version`'s dmg. The tag and the
 * filename both carry the version, so one pattern rewrites them together. A
 * miss would ship a dead button on the repo homepage, so a README that does
 * not hold exactly the expected links is refused.
 */
export const stampReadme = (readme: string, version: string): string => {
  const pattern = /(\/releases\/download\/)v[^/"\s]+\/(PhotobookGen_)[^_/"\s]+(_aarch64\.dmg)/g;
  const found = countMatches(readme, pattern);
  if (found !== README_DOWNLOAD_LINKS) {
    throw new Error(
      `README.md: expected ${README_DOWNLOAD_LINKS} download link(s), rewrote ${found}. ` +
        `Check the dmg filename the release workflow produces against the link in README.md.`,
    );
  }
  return readme.replace(pattern, `$1v${version}/$2${version}$3`);
};
