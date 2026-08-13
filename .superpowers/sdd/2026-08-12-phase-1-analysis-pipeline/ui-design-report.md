# Contact sheet UI - design report

Branch: `feat/phase-1-analysis-pipeline`
Owned paths only: `app/`, `tests/*.test.ts`. `src-tauri/` and `sidecar/` were not touched (another agent is fixing the sidecar IPC timeout there concurrently).

## What changed

- `app/pages/index.vue` - replaced the flat `UTable` with a state-machine page: `entry | running | error | no-images | no-analyzed | results`.
- `app/components/PhotoTile.vue` - new. Renders one contact-sheet tile: thumbnail, burst-size badge, and a scrim-backed caption row (aesthetic percentile, sharpness percentile, face count, smile share).
- `app/types/features.ts` - added three pure, tested helpers: `groupByEvent`, `burstSizes`, `basename`.
- `app/composables/useAnalysis.ts` - added `folder` (last-picked path, for display) and `retry()` (re-run on the same folder without reopening the picker, for the error state's "Try again").
- `app/app.config.ts` - new. Sets `primary: 'blue'`, `neutral: 'slate'` - the one accent used everywhere.
- `app/assets/css/main.css` - added a `prefers-reduced-motion: reduce` block that collapses animation/transition durations globally (Nuxt UI's own components, e.g. `UProgress`'s indeterminate carousel, already degrade via `motion-reduce:` utilities baked into the theme; `USkeleton`'s `animate-pulse` did not, so this covers it and everything else).
- `tests/features.test.ts` - 9 new tests for the three new pure helpers.

## Design decisions and reasoning

**Information architecture.** The old page was one flat table of paths and numbers with a thumbnail bolted on. The brief's real ask was to make the two clustering signals - event chapters and near-duplicate bursts - visible, since "those groupings are the app's actual insight." I settled on: group the *kept* photos (one per near-dup cluster, utility images dropped - the existing `keepers()` logic, unchanged) into `<section>`s by `eventCluster`, in first-appearance order, each with a plain "Event N, k photos" heading. Any keeper that beat out siblings in its near-dup cluster gets a small dark badge with an image-stack icon and the cluster size, so the burst is visible without cluttering the grid with the frames that got culled. This directly shows both clusterings without inventing a review/expand workflow the brief didn't ask for.

**Why show only keepers, not every frame.** The brief's framing is "the user reviews which photos survive culling" - the grid is the review surface for what would go in the book, not a raw dump of every burst frame. The burst badge already tells the user "this one beat N others," which is the insight; showing all N would defeat the point of culling.

**Color.** `primary: 'blue'` (Nuxt UI's default is green; the brief flagged the AI-default purple/violet specifically to avoid, and blue reads as native macOS chrome rather than a marketing accent). `neutral: 'slate'`. Blue is used consistently for the one primary action per state (folder picker, progress bar, "Try again") and nowhere else - no per-tile color coding of percentile "goodness," since a rainbow of thresholds would compete with the single-accent rule and isn't asked for. Text-on-photo captions use a fixed black gradient scrim regardless of theme, so legibility doesn't depend on light/dark mode.

**Radius.** No custom `rounded-*` overrides anywhere; every surface (tiles, `UCard`-family components, badges) uses Nuxt UI's default `--ui-radius` scale, so cards, tiles, and buttons share one corner language.

**Typography for numbers.** Every percentile, count, and percentage is wrapped in `class="font-mono tabular-nums"` - labels stay in the default UI sans font, only the digits go monospace, so summary-bar columns and tile numbers align without turning the whole page into a terminal.

**Iconography instead of letter codes.** Early draft used "A 82 / S 74" abbreviations on tiles; replaced with `i-lucide-sparkles` (aesthetic) and `i-lucide-focus` (sharpness), each with a `title` tooltip, plus `i-lucide-user-round` (faces) and `i-lucide-smile` (smile share). All icons come from `@iconify-json/lucide` via Nuxt UI's `UIcon`/icon props - no hand-rolled SVG.

**`smileFraction: null` vs `0`.** `PhotoTile.vue` computes `smilePct = photo.smileFraction === null ? null : Math.round(photo.smileFraction * 100)` and the smile icon+percentage only renders `v-if="smilePct !== null"`. A real `0` (nobody smiling) renders "0%"; `null` (couldn't determine) renders nothing. Verified both cases side by side in the results screenshot (see below): one tile shows "0%" and a different tile with `smileFraction: null` shows no smile indicator at all.

**Running state.** Per the brief, no spinner-in-a-modal: a `UProgress` with no `model-value` (indeterminate, Nuxt UI's built-in "carousel" animation) plus plain status text ("Analyzing photos in `<folder>`. This can take a few minutes for large folders.") sits above a skeleton grid using the exact same grid class (`grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-3`) as the real results grid, with 24 `USkeleton` tiles at `aspect-square`, so the loading state previews the real layout instead of a generic placeholder. No fake percentage is shown - the backend has no progress-event channel (out of scope, sidecar-owned), so a fabricated "43%" would violate the no-fake-precision rule.

**Error and empty states.** Both use `UEmpty` (icon, title, description, actions) rather than a bare alert, matching the brief's "real treatment" ask:
- `error` - "Analysis failed" + the raw error text + "Try again" (calls the new `retry()`, which re-invokes `analyze_folder` on the last picked path instead of forcing the user back through the OS folder dialog) + "Choose a different folder."
- `no-images` - `summary.total === 0`: "No photos found" - the folder had nothing to scan.
- `no-analyzed` - `summary.total > 0 && summary.photos.length === 0`: every file that was found failed to analyze (distinct from "no images" - files existed but none were readable). Description reports `X of Y files failed`.
- Inside `results`, if `eventGroups.length === 0` (every analyzed photo was `isUtility`), a smaller inline `UEmpty` explains that rather than showing a bare empty grid under a summary bar that already says "0 keepers."

**Copy.** Re-read every visible string for em-dashes (none used - periods and commas instead) and for LLM-thoughtful phrasing. Entry-state description states what the app does and how ("sharpness, faces, color palette, and Apple's aesthetic model... groups burst shots and events, then ranks each photo against the rest of the folder") without hero/marketing language.

**Header.** Present in every state so light/dark toggling (`UColorModeButton`) is always reachable, including from the very first screen - initially I'd hidden the whole header-right cluster on the entry state (to avoid a redundant second "choose folder" button next to the `UEmpty` action), which also hid the color-mode toggle; split it so the folder button is conditional but the toggle is always visible.

## Verification

```
$ bun run test
 Test Files  3 passed (3)
      Tests  421 passed (421)   # 412 existing + 9 new (groupByEvent x3, burstSizes x3, basename x3)

$ bun run lint
$ oxlint --deny-warnings
(no output, exit 0)
```

TypeScript: no root `tsconfig.json` exists in this repo (pre-existing; `nuxi typecheck` fails immediately on "Cannot find matching tsconfig.json" before even reaching app code) and `bunx vue-tsc` fails in this sandbox with `ERR_PACKAGE_PATH_NOT_EXPORTED` on `typescript`'s `./lib/tsc` export regardless of vue-tsc version (2.x or latest) - an environment/registry issue unrelated to this change. Did not chase further since it's outside the established verification bar for this repo (the prior thumbnails-report.md also verified via `bun run test` + `bun run lint` only, no typecheck script exists in `package.json`). Reviewed all new/changed files by hand for `any` usage (none) and correct prop/slot types against the installed `@nuxt/ui@4.10.0` source (`node_modules/@nuxt/ui/dist/runtime/components/*.d.ts` and `.nuxt/ui/*.ts` theme files) rather than assuming an API, the same lesson called out by the previous UTable v2-API bug.

### Visual verification (both color modes)

`analyze_folder` was not exercised in this task: the sidecar IPC timeout documented in `thumbnails-report.md` (blocking `recv_timeout` inside an async Tauri command starving the stdout-drain task) is `src-tauri`-owned and explicitly out of scope here, and re-confirming it wasn't necessary - the brief pre-authorized falling back to representative fixture data.

Rather than fighting the same native-window/Accessibility-permission limitations the thumbnails agent hit (no Accessibility access in this sandbox to drive the folder dialog or devtools), I ran the page directly with `bun run ui:dev` (plain `nuxt dev`, real Vue/Nuxt/Tailwind/Nuxt-UI rendering, no Tauri shell) and drove it with the Playwright MCP tools - viewport-sized screenshots only, never full-screen. All three real on-disk thumbnails produced by the previous agent's earlier sidecar run (`$APPDATA/thumbnails/*.jpg`) were copied into a temporary `public/debug-thumbs/` and `window.__TAURI_INTERNALS__.convertFileSrc` was shimmed to identity, so tiles rendered real decoded JPEG content, not placeholders. A temporary `?debug=running|error|no-images|results` branch in `index.vue`'s script populated `summary`/`running`/`error`/`folder` directly (bypassing the picker dialog and the slow sidecar round-trip) with a representative fixture: 2 events, a 3-frame burst cluster, a tile with a missing thumbnail (fallback icon), `smileFraction: 0` next to `smileFraction: null` on different tiles, and one `isUtility` photo to confirm it's excluded from the grid and from burst counts.

States captured, in both light and dark (toggled live via the real `UColorModeButton`, not a class hack):
- `entry` - copy readable, single CTA, no duplicate header button, color-mode toggle present.
- `results` - burst badge "3" on the cluster-1 winner; missing-thumbnail fallback icon on a tile with `thumbnailPath: null`; `0%` smile shown distinctly from no-smile-indicator; monospace-aligned summary counts; two event headers; utility photo correctly absent from both the grid and the "6 keepers" count (14 total / 9 analyzed / 4 cached / 2 failed / 6 keepers, matching the fixture).
- `running` - indeterminate progress bar, status text, 24-tile skeleton grid in the same column layout as `results`.
- `error` - "Analysis failed" + raw message + "Try again" / "Choose a different folder" actions.
- `no-images` - "No photos found" empty state.

All temporary changes (the `?debug=` branch in `index.vue`, `public/debug-thumbs/`, the `__TAURI_INTERNALS__` shim) were fully reverted after capture; `git status`/`git diff` confirmed only the intended files remain changed, and `bun run test` / `bun run lint` were re-run clean afterward (see above - both ran *after* the revert).

Contrast: white caption text sits on a `from-black/85 via-black/55 to-transparent` gradient anchored at the tile bottom (not a flat scrim thinning to nothing under the text) - verified visually against the darkest test tile (deep red) and lightest (green) in both color modes; the caption region itself is always near-black regardless of the underlying photo or page theme, which is the point - text-on-photo contrast shouldn't depend on either.

## Uncertain / worth a second look

- **No real end-to-end run.** Per the brief, this was expected - the sidecar IPC bug blocks it and is out of scope for `app/`. The fixture-based verification proves the Vue/Tailwind/Nuxt-UI rendering and the pure grouping logic; it does not prove the real `AnalysisSummary` shape survives the wire round-trip into this UI (that contract was already exercised by `thumbnails-report.md`'s manual sidecar wire check and by `tests/features.test.ts`'s type-level fixtures, which this task extended rather than replaced).
- **`retry()` on a stale folder handle.** If the folder was moved/deleted between the failed attempt and "Try again," `analyze_folder` will presumably error again with a different message - untested since it requires a real backend call. Not expected to be a UI-side bug, just unverified end to end.
- **No root `tsconfig.json` / typecheck script in this repo** predates this task; flagged above rather than fixed, since adding one is a build-tooling decision outside a UI task's remit and touching it risked side effects on the concurrent `src-tauri` work.
- **`no-analyzed` state** (every file found failed to analyze) has copy and layout written and reviewed but was not screenshotted - it reuses the exact same `UEmpty` pattern as `error` and `no-images`, which were both visually verified, so the marginal risk is low, but it's worth a quick look if there's time before shipping.

---

## Retheme: brand palette from the app-icon render

Structure was not touched - same state machine, same grid, same grouping logic. This is a palette swap plus one new grouping concept (the hero photo) that the new accent made room for.

### Why `blue`/`slate` had to go

The first pass used Nuxt UI's stock `blue` as primary and `slate` as neutral - a reasonable generic choice, but not the user's brand. The user supplied a specific palette derived from the app icon's 3D render, with an explicit, narrow usage rule: *"guide lines and the hero tile only, everything else stays neutral."* That rule is the whole design brief in one sentence - the accents are not decoration, they each mark exactly one thing.

### The four ramps

All four are full 11-stop `@theme static` ramps in `app/assets/css/main.css` (not single hex values), so Nuxt UI's hover/active/disabled variants derive correctly instead of being hand-picked per component. Each ramp is a single hue/saturation pair with a hand-tuned lightness curve (not a formula-generated ramp) - the exact `500`/`400` stops were chosen to hit the contrast targets below, so the curve isn't perfectly uniform between every step and that's intentional.

**`steel`** (primary, hue 208°, sat 16%) - the one interactive colour: buttons, focus rings, links, selection, the header icon. Desaturated and cool on purpose, nowhere near stock Tailwind `blue` (`#3b82f6`-family, hue ~217° but far more saturated - reads as a marketing/CTA blue, not steel).

```
50  #f6f7f9   400 #8a9baa   800 #333e47
100 #edf0f2   500 #5d7081   900 #283037
200 #dadfe4   600 #4d5d6a   950 #1b2025
300 #b8c2cc   700 #3f4c57
```

**`charcoal`** (neutral, hue 210°, sat 7%) - cool grey through soft charcoal, everywhere else, both modes. Same hue family as `steel` for cohesion, far less saturated so it reads as grey, not blue. `charcoal-900` (`#2b2e31`) is the dark-mode page background - deliberately not near-black; "soft charcoal" per the spec, not a harsh navy-black (the original `slate`-based dark background leaned more blue-navy than this).

```
50  #fafafa   400 #a2a8ae   800 #393d41
100 #f3f4f4   500 #6b737b   900 #2b2e31
200 #e4e6e7   600 #585e65   950 #1c1f21
300 #ced1d4   700 #474d52
```

**`lavender`** (structure, hue 255°, sat 26%) - not registered as a Nuxt UI semantic colour at all. It marks exactly one thing today: the divider between event chapters (`border-t` on each `<section>` after the first, in `app/pages/index.vue`). This is a deliberate, narrower scope than "chapter and cluster dividers" in the spec - see the note on cluster dividers below.

```
50  #f8f7fa   400 #b4abce   800 #504474
100 #f1eff6   500 #9a8fbd   900 #3c3357
200 #e2dfec   600 #7d6eaa   950 #28223a
300 #cec8df   700 #645591
```

**`sunlight`** (hero, hue 46°, sat 62%) - also not a Nuxt UI semantic colour. Marks exactly one photo per event group: the top-ranked survivor. A small badge (`sunlight-300` fill, `charcoal-900` star icon, fixed regardless of mode - same pattern the existing burst-count badge already uses) plus a 2px ring around the tile (mode-dependent shade, see contrast below).

```
50  #fdfbf5   400 #e4cf8b   800 #8c7321
100 #f9f5e6   500 #dcc26a   900 #675518
200 #f2e9c9   600 #d2b041   950 #423610
300 #ecddac   700 #b6952b
```

### Why lavender and sunlight are NOT registered in `ui.colors`

`app/app.config.ts` only maps `primary: 'steel'` and `neutral: 'charcoal'`. `lavender` and `sunlight` are deliberately absent from the semantic colour list, even though Nuxt UI supports registering additional named roles (`ui.theme.colors` in `nuxt.config.ts`). Registering them would give every component a `color="lavender"` / `color="sunlight"` option - and the whole point of the spec's usage rule is that these two accents mark exactly two things. Giving them a generic slot on every button and badge in the app is exactly the kind of drift the rule is meant to prevent. Instead they're applied as plain Tailwind utility classes at their two call sites only: the section divider in `app/pages/index.vue`, and the hero ring/badge in `app/components/PhotoTile.vue`.

### The hero: a new concept the accent required

The spec's hero role ("the top-ranked photo, the surviving keeper in a burst cluster... one thing per group") needed a concrete rule, since the existing code had no notion of "the best photo in a group." Read literally, "one thing per group" pointed at the event chapter as the group: `pickHero()` (new pure function, `app/types/features.ts`) picks the single highest-`aestheticPct` photo per event group, tie-broken by `sharpnessPct` - the same tie-break direction as `keepers()` but with the two metrics swapped, because `keepers()` is choosing between near-identical burst frames (sharpness is the differentiator there), while `pickHero()` is choosing the best photo across a whole event (aesthetic quality is the app's headline ranking metric). 4 new tests in `tests/features.test.ts` cover the empty/singleton/tie cases (425 total, up from 421).

### Cluster dividers: reserved, not implemented

The spec says lavender marks "chapter **and cluster** dividers, group boundaries," and separately that it becomes "the safe-area and gutter indicator when Phase 2 draws real page geometry." In the current contact-sheet layout, a near-duplicate cluster renders as a single surviving tile (the burst badge already says "this one beat N others") - there's no sub-grid of frames within a cluster to draw a boundary around yet. Drawing a cluster-level lavender line today would mean inventing a visual grouping that doesn't correspond to anything in the layout. I implemented the one concrete application that exists now (the event-chapter divider) and left `lavender` defined and ready as a token for Phase 2's real per-page gutters/safe-area lines, which is exactly the future use the coordinator flagged.

### Contrast: measured, not asserted

All ratios below were computed with the standard WCAG relative-luminance formula against the actual hex values above (script in `/private/tmp/.../scratchpad/palette.mjs` during this session, not retained in the repo), then independently confirmed by reading `getComputedStyle(...).boxShadow` / `borderTopColor` / `backgroundColor` off the real rendered page in Playwright, in both `light` and `dark` (`document.documentElement.className` checked at each measurement to confirm which mode was active).

| Element | Pair | Ratio | Threshold | Result |
|---|---|---|---|---|
| Light solid button (primary) | white text on `steel-500` | 5.12:1 | 4.5:1 (text) | Pass |
| Dark solid button (primary) | `charcoal-900` text on `steel-400` | 4.78:1 | 4.5:1 (text) | Pass |
| Light `text-primary` (icons, outline/ghost/link) | `steel-500` on white | 5.12:1 | 4.5:1 (text) | Pass |
| Dark `text-primary` | `steel-400` on `charcoal-900` | 4.78:1 | 4.5:1 (text) | Pass |
| Light body text | `charcoal-700` on white | 8.57:1 | 4.5:1 (text) | Pass |
| Light heading text | `charcoal-900` on white | 13.66:1 | 4.5:1 (text) | Pass |
| Dark body text | `charcoal-200` on `charcoal-900` | 10.91:1 | 4.5:1 (text) | Pass |
| Dark heading text | white on `charcoal-900` | 13.66:1 | 4.5:1 (text) | Pass |
| Light `text-muted` (summary bar, captions) | `charcoal-500` on white | 4.81:1 | 4.5:1 (text) | Pass |
| Dark `text-muted` | `charcoal-400` on `charcoal-900` | 5.69:1 | 4.5:1 (text) | Pass |
| **Light** chapter divider line | `lavender-600` on white | 4.48:1 | 3:1 (non-text) | Pass |
| **Dark** chapter divider line | `lavender-300` on `charcoal-900` | 8.42:1 | 3:1 (non-text) | Pass |
| **Light** hero ring | `sunlight-800` on white | 4.58:1 | 3:1 (non-text) | Pass |
| **Dark** hero ring | `sunlight-400` on `charcoal-900` | 8.84:1 | 3:1 (non-text) | Pass |
| Hero badge (both modes, fixed) | `charcoal-900` text on `sunlight-300` fill | 10.08:1 | 4.5:1 (text) | Pass |
| Fallback "no thumbnail" icon, light | `charcoal-500` on `bg-elevated` (`charcoal-100`) | 4.37:1 | 3:1 (graphical) | Pass |
| Fallback icon, dark | `charcoal-400` on `bg-elevated` (`charcoal-800`) | 4.56:1 | 3:1 (graphical) | Pass |

Live-measured confirmation (Playwright `getComputedStyle`, `?debug=results` fixture, both modes):
- Dark mode: hero ring `boxShadow` resolved to `rgb(228, 207, 139)` = `#e4cf8b` = `sunlight-400`, exactly as specified. Chapter divider `borderTopColor` = `rgb(206, 200, 223)` = `#cec8df` = `lavender-300`. Page background = `rgb(43, 46, 49)` = `#2b2e31` = `charcoal-900`.
- Light mode (`localStorage.nuxt-color-mode = 'light'`, confirmed via `document.documentElement.className === 'light'`): hero ring = `rgb(140, 115, 33)` = `#8c7321` = `sunlight-800`. Chapter divider = `rgb(125, 110, 170)` = `#7d6eaa` = `lavender-600`. Page background = `rgb(255, 255, 255)`.

**Where the pastels failed as-specified, and the fix applied (per the coordinator's explicit fallback instruction):** both `lavender` and `sunlight` fail non-text contrast as light/mid pastel shades directly against a *white* background - `lavender-400` on white measured 2.17:1, `lavender-500` 2.98:1 (both fail 3:1); `sunlight-500` on white measured 1.75:1, `sunlight-600` 2.09:1, even `sunlight-700` 2.87:1 (all fail 3:1). Rather than force a pastel value that fails, the light-mode divider and ring use a **darker shade of the same hue** as foreground (`lavender-600`, `sunlight-800` - both pass, see table), while the dark-mode versions can stay closer to the pastel end (`lavender-300`, `sunlight-400`) since a near-black background is far more forgiving. The hero **badge** sidesteps the light-on-light trap entirely by using the pastel as a **fill** with dark `charcoal-900` text on top (10.08:1, fixed in both modes) - exactly the "fills, borders, or plates with dark text" pattern from the brief, rather than pastel-as-foreground anywhere.

**One regression caught and fixed before it shipped:** the first `charcoal` ramp (lightness 54 at the `500` stop) produced `text-muted` at 3.51:1 on white - below AA, and worse than stock `slate-500`'s 4.76:1 (measured for comparison). `text-muted` is real content in this UI (summary-bar counts, the running-state status line, the percentile caption), not decorative, so this would have been a real regression introduced by the retheme. Fixed by darkening the `500` stop to lightness 45 (`#6b737b`, 4.81:1, now slightly better than stock). While tuning this I also found the fallback "no thumbnail" icon was using `text-dimmed`, which measured 2.18:1 (light) / 2.28:1 (dark) against its own tile background - well under the 3:1 graphical-object minimum, and this was true even with the original stock `slate` values (not a regression I introduced, just one I noticed while re-verifying contrast rigorously). Switched that one icon, plus the two remaining prose usages of `text-dimmed` (the percentile caption, the per-event photo count), from `text-dimmed` to `text-muted`, which passes in all four spots. `text-dimmed` itself remains under AA in both this palette and stock Nuxt UI's own default (`slate-400` on white measures 2.56:1) - that appears to be an intentional library-level design decision for a placeholder/tertiary tier, not something this task's palette should try to fix, so I stopped using `text-dimmed` for real content instead of trying to force that specific token to pass.

### Naming: PhotobookGen

Mid-task, the coordinator renamed the app from "Photobook Generator" to "PhotobookGen" (one word) across `productName`, the window title, the Rust binary, `package.json`, and the README - all outside `app/`. The two remaining visible strings inside `app/pages/index.vue` (the header `<h1>` and the opening clause of the entry-state description) were updated to match; the rest of the description copy was left as-is per the coordinator's instruction.

### A note on how this got committed

While this retheme was in progress, a concurrent commit (`f0d9757`, "chore: rename app to PhotobookGen", not authored by this task) captured the working tree mid-edit - it includes the full palette/divider/hero implementation, but predates the `PhotobookGen` string updates and the removal of a temporary `?debug=` verification scaffold in `index.vue` (plus the scratch files that scaffold depended on: `public/debug-thumbs/*.jpg`, one `.playwright-mcp/*.yml`, and a leftover screenshot). This task's own commit is a follow-up on top of `f0d9757` that finishes the rename in `index.vue`, removes the debug scaffold, and deletes the four accidentally-committed scratch files - `src-tauri/` changes present in the working tree at commit time (from the concurrent sidecar-fix agent) were left untouched, as before.

### Verification

```
$ bun run test
 Test Files  3 passed (3)
      Tests  425 passed (425)   # 421 prior + 4 new for pickHero

$ bun run lint
$ oxlint --deny-warnings
(no output, exit 0)
```

Same TypeScript caveat as the first pass: no root `tsconfig.json` in this repo, `bunx vue-tsc` fails on an unrelated `ERR_PACKAGE_PATH_NOT_EXPORTED` in this sandbox regardless of version. Reviewed the new/changed files by hand for `any` (none) and for whether the class strings actually match the tokens defined in `@theme` (confirmed by reading the computed styles back from the live page, per the measurements above, rather than trusting the source alone).

### Uncertain / worth a second look

- **Ramp lightness curves are hand-authored, not algorithmic.** The `500`/`400` (and other) stops were chosen to hit specific contrast targets first, smooth visual progression second. They read fine in the screenshots taken, but a designer eyeballing the full 11-stop ramp in isolation (e.g. in a swatch strip) might want to true up a step or two - functionally nothing depends on the ramp being perceptually uniform, only on the specific stops used in this UI.
- **`sunlight-800` as the light-mode hero ring** is a fairly saturated gold/mustard, not a "pastel" in the way `sunlight-300`/`400` are - this is the direct, intended consequence of the coordinator's fallback instruction ("use a darker shade of the same hue for the foreground"), but it does mean the hero ring looks visibly different in light vs. dark mode (gold outline vs. soft pastel outline) rather than being the exact same hex in both. The badge fill stays pastel and identical in both modes, so the "this is the hero" signal is still consistent; only the ring's exact tone shifts per mode, same as the chapter divider.
- **No `success`/`warning`/`info`/`secondary` roles were touched** - they still point at Nuxt UI's stock palettes, since nothing in the current UI uses them (`color="error"` isn't used anywhere either, after the earlier pass replaced the old `UAlert` with `UEmpty`). If a future state introduces a genuine warning/success affordance, those stock colours would sit awkwardly next to this custom palette and probably want their own custom ramps at that point.
- **`no-analyzed` state** (every file found failed to analyze) has copy and layout written and reviewed but was not screenshotted - it reuses the exact same `UEmpty` pattern as `error` and `no-images`, which were both visually verified, so the marginal risk is low, but it's worth a quick look if there's time before shipping.
