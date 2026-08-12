# PhotobookGen

A macOS photobook layout app built with Tauri 2 (Rust) + Nuxt 4, backed by a Swift
sidecar (`sidecar/`, SwiftPM package `PhotobookEngine`) that performs photo analysis.
Target platform is macOS 15.0+, arm64 only.

## Prerequisites

- [Bun](https://bun.sh) (package manager)
- Rust (stable toolchain, arm64 macOS target)
- Xcode / Swift toolchain (for the sidecar package)

## Getting started

```bash
bun install
bun run dev
```

`bun run dev` and `bun run build` both build the Swift sidecar automatically via Tauri's
`beforeDevCommand` / `beforeBuildCommand` hooks (see `src-tauri/tauri.conf.json`), so no
manual step is normally required.

### Important: building/testing the Rust crate directly

`src-tauri/tauri.conf.json` declares the sidecar binary under `bundle.externalBin`. This
makes `tauri-build`'s build script validate, **at compile time**, that
`src-tauri/binaries/photobook-engine-<target-triple>` exists on disk — before any Rust
code is compiled.

If you invoke `cargo build` or `cargo test` **directly** inside `src-tauri/` (bypassing
the `bun run dev` / `bun run build` hooks above), you must build the sidecar first:

```bash
bun run sidecar
cargo test --manifest-path src-tauri/Cargo.toml
```

Skipping this produces an error like:

```
resource path `binaries/photobook-engine-aarch64-apple-darwin` doesn't exist
```

which points at the missing binary, not at the missing `bun run sidecar` step — this note
exists so that error is easy to diagnose.

## Scripts

- `bun run dev` — start the Tauri app in development mode (builds sidecar first)
- `bun run build` — build the production app bundle (builds sidecar first)
- `bun run sidecar` — build the Swift sidecar binary and copy it into
  `src-tauri/binaries/`
- `bun run test` — run frontend tests (Vitest)
- `bun run test:rust` — run Rust tests (`cargo test`)
- `bun run test:swift` — run Swift tests (`swift test`)
