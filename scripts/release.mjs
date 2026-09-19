/**
 * Cut a release: sync the version across package.json, tauri.conf.json,
 * Cargo.toml and Cargo.lock, write CHANGELOG.md, then commit, tag, and push.
 * Pushing the tag starts the Release workflow, which waits for the commit's CI,
 * builds the Apple Silicon dmg, publishes the GitHub release, and then points
 * the README's download button at it.
 *
 *   bun run release 0.2.0      # explicit version (recommended, deterministic)
 *   bun run release            # changelogen picks the next version from commits
 *   bun run release --minor    # force a patch/minor/major bump via changelogen
 */
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { applyRewrite, versionRewrites } from "./release-stamp.ts";

const run = (cmd) => execSync(cmd, { stdio: "inherit" });
const args = process.argv.slice(2);
const explicit = args.find((a) => /^\d+\.\d+\.\d+([-+.].+)?$/.test(a));

if (explicit) {
  const [pkg] = versionRewrites(explicit);
  writeFileSync(pkg.file, applyRewrite(readFileSync(pkg.file, "utf8"), pkg));
  run("bun x changelogen --output CHANGELOG.md");
} else {
  run(`bun x changelogen --bump --output CHANGELOG.md ${args.join(" ")}`.trim());
}

const version = JSON.parse(readFileSync("package.json", "utf8")).version;

// Compute every rewrite before writing any, so a miss leaves these files untouched.
const rewrites = versionRewrites(version).map((rewrite) => [
  rewrite.file,
  applyRewrite(readFileSync(rewrite.file, "utf8"), rewrite),
]);
for (const [file, text] of rewrites) writeFileSync(file, text);

run(`git add CHANGELOG.md ${rewrites.map(([file]) => file).join(" ")}`);
run(`git commit -m "chore(release): v${version}"`);
run(`git tag v${version}`);
run("git push");
run(`git push origin v${version}`);

console.log(`\nReleased v${version}. GitHub Actions is now building and publishing it.`);
