# PhotobookGen

macOS-only desktop app (Tauri 2 + Nuxt 4 + Swift sidecar, arm64, macOS 15+) that turns a
folder of photos into a print-ready photobook. Phase 1 (on-device analysis) is complete;
Phases 2-5 are not built.

## Read first

**`docs/PROJECT-STATUS.md`** — current state, what is not built, what is verified versus
merely assumed, and the traps that already caused real bugs. Read it before writing code.

- `docs/superpowers/specs/2026-08-12-photobook-generator-design.md` — design. Accurate.
- `docs/superpowers/plans/2026-08-12-phase-1-analysis-pipeline.md` — Phase 1 plan. **Parts
  are wrong and carry `⚠️ SUPERSEDED` warnings.** Following the original text reintroduces
  two real bugs.

## Hard constraints

- **Images never leave the machine.** All pixel analysis is on-device. Only derived JSON
  may be sent anywhere. Never propose a hosted vision model.
- **arm64 only, macOS 15+.** Never configure a universal build.
- **`bun run sidecar` before any `cargo` command.** `tauri-build` validates the
  `externalBin` path at compile time.
- Bun, not npm. Conventional Commits. Never `git commit --no-verify`.
- Never use `any` in TypeScript. oxlint warnings are failures.

## Commands

```sh
bun run sidecar   # build the Swift sidecar (do this first)
bun run dev       # dev app
bun run test      # vitest
bun run test:rust # cargo test
bun run test:swift
bun run lint      # oxlint --deny-warnings
```

## Conventions

**Print geometry is normalised to the spread canvas** (22.394" × 8.894"). One exception:
`aspect_pref` in `templates/*.json` is a **real-world inch ratio**, not the normalised rect
ratio. The canvas is 2.518:1, so conflating them mis-scores every slot. The validator
enforces this.

**Vue:** prop shorthand when the name matches, `useTemplateRef()` over manually typed refs,
destructuring defaults on `defineProps` rather than `withDefaults`.

**Swift tests:** swift-testing (not XCTest). Top-level `@Test func`s share one module
namespace, so **prefix test names by area** or you get a compile error.

**Extract pure functions for testability.** This codebase does it five times
(`finalize_photos`, `analyze_batches`, `lookup_cache`, `percentiles`, `imageNormalizedTopLeft`)
specifically so the interesting logic is reachable without an `AppHandle` or a live Vision
call. Follow the pattern rather than leaving core logic untestable.

## On tests in this project

Phase 1 shipped **eight tests that passed under a broken implementation**, every one of
them written into a detailed plan by an agent that believed they were sound. Examples: a
sharpness baseline whose variance was zero under any implementation; an event-ordering test
whose input was already sorted; a boundary test using a value nowhere near the boundary; a
smile proxy fed four idealised points when the real input is 65.

So: **do not trust that a test tests what it says.** Break the thing deliberately and
confirm the test notices. Mutation-check anything load-bearing and paste the evidence. If a
test passes with the feature deleted, it is worse than no test, because it reads as
protection.
