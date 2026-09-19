/**
 * Points README.md's download button at a published release. The release
 * workflow runs this after publishing, so the button never names a dmg that
 * does not exist yet.
 *
 *   bun scripts/stamp-readme.ts 0.2.0
 */
import { readFileSync, writeFileSync } from "node:fs";
import { stampReadme } from "./release-stamp.ts";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+([-+.].+)?$/.test(version)) {
  throw new Error(`usage: bun scripts/stamp-readme.ts <version>, got ${JSON.stringify(version)}`);
}
writeFileSync("README.md", stampReadme(readFileSync("README.md", "utf8"), version));
