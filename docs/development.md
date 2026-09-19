# Development

Requires an Apple Silicon Mac on macOS 15+, [Bun](https://bun.sh) (the package manager,
not npm), a stable Rust toolchain for `aarch64-apple-darwin`, and Xcode or the Swift
toolchain for the sidecar package.

```bash
bun install
bun run sidecar     # build the Swift sidecar first; every cargo command needs it
bun run dev         # the desktop app
```

`bun run dev` and `bun run build` also build the sidecar themselves, through Tauri's
`beforeDevCommand` and `beforeBuildCommand` hooks in `src-tauri/tauri.conf.json`.

```bash
bun run test        # frontend tests (Vitest)
bun run test:rust   # Rust tests (cargo test)
bun run test:swift  # Swift sidecar tests (swift test)
bun run lint        # oxlint; warnings are failures
bun run check:build # nuxt generate, the only step that compiles .vue files
bun run build       # PhotobookGen.app and the dmg, under src-tauri/target/.../bundle/
```

Run `bun run check:build` before committing any change to a `.vue` file: lint and the
tests never parse a Vue template, so a broken component passes both. CI runs every command
above except `build` on each push and pull request.

Contributor notes, conventions and the traps that already caused real bugs are in
[`CLAUDE.md`](../CLAUDE.md) and [`docs/PROJECT-STATUS.md`](PROJECT-STATUS.md).

<details>
<summary><strong>Building or testing the Rust crate directly</strong></summary>

`src-tauri/tauri.conf.json` declares the sidecar binary under `bundle.externalBin`. This
makes `tauri-build`'s build script validate, **at compile time**, that
`src-tauri/binaries/photobook-engine-<target-triple>` exists on disk, before any Rust
code is compiled.

If you invoke `cargo build` or `cargo test` **directly** inside `src-tauri/` (bypassing
the `bun run dev` / `bun run build` hooks), build the sidecar first:

```bash
bun run sidecar
cargo test --manifest-path src-tauri/Cargo.toml
```

Skipping this produces an error like:

```
resource path `binaries/photobook-engine-aarch64-apple-darwin` doesn't exist
```

which points at the missing binary, not at the missing `bun run sidecar` step.

</details>

<details>
<summary><strong>Driving the UI in a browser (<code>bun run ui:mock</code>)</strong></summary>

`bun run ui:mock` opens the webview in an ordinary browser on port 3123 with the Tauri
bridge replaced by `dev/tauri-mock/`, so a UI change can be rasterised and looked at, or
driven by a browser automation tool, without a Tauri process. Every command the UI can
reach is answered, from the wire fixtures where one exists and from
`dev/tauri-mock/photos.ts` for a whole analysed folder, so all three screens and the flows
between them can be driven end to end. Set `window.pbgCancelPicker = true` to make the next
folder pick resolve to nothing. Nothing in the production build sees this alias.

`bun run ui:dev` runs the plain Nuxt dev server without the mock.

</details>

<details>
<summary><strong>Benchmarking photo analysis performance</strong></summary>

`scripts/benchmark.sh <folder>` drives the sidecar's `benchmark` NDJSON request (alongside
`ping` and `analyze`; see `sidecar/Sources/PhotobookEngine/Protocol.swift`) over every
supported image in a folder and reports per-stage wall-clock timings: file hash, EXIF read,
decode, Vision pass, classical metrics, and (optionally) thumbnail write. It prints a
per-file table and a per-extension aggregate, so RAW, HEIC and JPEG throughput are directly
comparable instead of blended into one number.

This is a permanent diagnostic, not scaffolding. Re-run it after any change to
`ImageLoader`, `Analyzer`, `VisionAnalyzer` or `Metrics` to catch a throughput or
correctness regression before it ships. It is also how the RAW decode performance fix was
measured; see
`.superpowers/sdd/2026-08-12-phase-1-analysis-pipeline/raw-performance-report.md` for the
baseline-vs-fix numbers, the per-format embedded-preview fallback rates, and a quality
comparison between the preview-decode and full-decode paths on real RAW files.

```bash
bun run sidecar                                    # builds the sidecar binary if needed
scripts/benchmark.sh ~/Pictures/SonyImport --recursive
scripts/benchmark.sh sidecar/Fixtures --limit 20    # quick smoke test, no real photos needed
```

Flags:

- `--recursive` descends into subfolders, as the app's own folder scan does (real photo
  exports are often nested).
- `--limit N` benchmarks only the first N matching files.
- `--thumbnails DIR` also times the contact-sheet thumbnail write stage, writing into
  `DIR`. Omitting it skips that stage, which is the default: most throughput questions are
  about decode and Vision, not thumbnail writing.
- `--json OUT.json` dumps the raw sidecar response alongside the printed tables.

Requires `jq`. The script builds the sidecar binary automatically (via `bun run sidecar`)
if `src-tauri/binaries/photobook-engine-<target-triple>` doesn't exist yet.

Benchmark runs are deliberately **sequential**, not the concurrent `concurrentPerform`
fan-out a real `analyze` batch uses (see `Benchmarker.swift`'s doc comment). That makes
per-stage numbers clean and comparable across formats, but a benchmark run's total wall
time under-represents real multi-core throughput. Judge real-world speed by the aggregate
ratios and per-format comparisons, not the raw total.

#### Timing what the user waits for

`scripts/benchmark.sh` times the sidecar's stages one photo at a time. To time the app's own
analysis path (Rust hashing and cache lookup, the batched sidecar calls, finalize) over a
folder, cold and then with every photo cached:

```bash
bun run sidecar
cd src-tauri && cargo run --release --example analyze_bench -- ~/Pictures/SomeFolder 3
```

It points `HOME` at a throwaway directory, so the app's real cache is never touched, and
discards a first run that spawns the sidecar. Each run logs its split, for example
`gather 419ms = hash+lookup 44ms + sidecar 370ms`, and the app writes the same line to its
log for every analysis.

To time a folder the app has already analysed without running Vision on it again, point
`PBG_BENCH_SEED_DB` at a copy of the app's database (`sqlite3 <database> ".backup copy.sqlite"`).
Every run then starts from that cache. The first run is the one after a restart. The first
run after upgrading from a version without file stamps reads every file once.

</details>


## Releases

```bash
bun run release 0.2.0   # explicit version (recommended)
bun run release         # changelogen picks the next version from the commits
bun run release --minor # force a patch, minor or major bump
```

`scripts/release.mjs` writes the version into `package.json`, `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`, and updates `CHANGELOG.md` with
[changelogen](https://github.com/unjs/changelogen). It refuses to continue if any of those
fields is missing. It then commits `chore(release): v<version>`, tags `v<version>`, and
pushes both.

The tag starts [`.github/workflows/release.yml`](../.github/workflows/release.yml). It opens a
draft release named `PhotobookGen v<version>` with changelogen notes, builds
`PhotobookGen_<version>_aarch64.dmg` on a macOS 15 Apple Silicon runner, and uploads it with
the updater archive and its `latest.json`. It publishes the release only once CI has passed
on the tagged commit. It does not run CI again. It waits for the run that pushing the release
commit to master already started. A tag on a commit that was never pushed to master has no
such run, so the release stops there. The dmg builds while CI runs. If CI fails, the release
stays a draft that users and the updater cannot see, and you delete it along with the tag.

After publishing, the workflow points the download button at the top of the README at the
new dmg and pushes that as a `docs(readme)` commit from `github-actions[bot]`. Pull before
your next commit. The button changes only once the dmg exists, so it never links to a
release that failed. The stamp (`scripts/stamp-readme.ts`) refuses a README that does not
hold exactly one download link, rather than shipping a dead button.

Running the workflow by hand from the Actions tab builds the dmg and attaches it to the run
as an artifact, without creating a release.

### Updater signing

The updater trusts one minisign keypair. The public half sits in
`src-tauri/tauri.conf.json` under `plugins.updater.pubkey`, and the app refuses any update
whose signature it cannot verify. The private half lives only in the repository secrets,
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and the release
workflow reads them when it builds. A new keypair comes from `bunx tauri signer generate -w
<path outside the repo>`; replacing the public key in the config means every already
installed copy stops accepting updates and has to be reinstalled by hand, so rotate it only
when the private key is compromised.

This is Tauri's own signing, not Apple's. The bundle still has no Developer ID and is not
notarized, which is why the first launch needs the Gatekeeper step under
[Install](../README.md#install).
