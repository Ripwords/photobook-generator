# Spread template library — report

40 templates in `templates/`, validated by `tests/templates.test.ts` (run via
`bun run test`). All 404 assertions pass; `bun run lint` is clean.

## Validator result

```
$ bun run test
 Test Files  2 passed (2)
      Tests  404 passed (404)
```

The suite (`tests/templates.test.ts`) loads every `templates/*.json` file, narrows it
through a hand-written type guard (no `any`), and asserts per template: well-formed rects
(width/height > 0), text zones entirely within the safe area, no text zone overlapping the
gutter dead band, every declared `bleed` edge reaching the canvas boundary, every
undeclared edge staying inside the canvas, that every slot's `aspect_pref` is a valid
positive range and matches the slot's own real-world (inches) aspect ratio,
`min_photos <= max_photos` with a consistent slot count, allowed `density`/`energy` enums,
a unique `id` matching the filename slug, and no meaningful overlap between a template's
own slots. Failures name the offending file and
rect.

## Templates by category

**Full-bleed single image (2)**
- `01-full-bleed-1up` — one photo across the entire spread, bleeding all four edges.
- `02-full-bleed-1up-corner-caption` — the same, with a small caption in the safe
  bottom-right corner.

**Single image + whitespace or text, both handednesses (4)**
- `03-hero-left-text-right` / `04-hero-right-text-left` — full-bleed hero on one page,
  generous text zone on the other.
- `05-hero-left-whitespace-right` / `06-hero-right-whitespace-left` — a matted, floating
  hero with wide margins; the opposite page is pure whitespace, no text.

**Two-up (6)**
- `07-two-up-symmetric-margin` — two equal matted photos, generous shared margins.
- `08-two-up-symmetric-bleed-outer` — two equal photos bleeding their outer edges, meeting
  cleanly at the gutter.
- `09-two-up-hero-left-asym` / `10-two-up-hero-right-asym` — large hero spanning past the
  fold with a smaller support on the opposite page (mirrors the design doc's own example).
- `11-two-up-portrait-pair` — two tall portrait photos side by side.
- `12-two-up-uneven-l-shape` — a diagonal, uneven pairing (top-left large, bottom-right
  small) leaving two corners as intentional whitespace.

**Three-up: hero + two supports (5)**
- `13`/`14-three-up-hero-left/right-stack` — full-page hero with two stacked supports on
  the other page.
- `15`/`16-three-up-hero-top/bottom-two-…` — a wide hero band across the fold with two
  supports on the opposite half.
- `17-three-up-hero-diagonal-lively` — centred hero with two small photos bleeding off
  opposite corners.

**Four-up: grid and non-grid (5)**
- `18-four-up-grid-2x2-generous` — an even 2×2 grid, page-aligned columns, wide gaps.
- `19-four-up-grid-2x2-tight` — the same grid, tighter gaps, higher energy.
- `20-four-up-windowpane` — non-grid mosaic alternating large/small cells diagonally.
- `21`/`22-four-up-hero-plus-three-left/right` — full-page hero with three stacked
  supports on the opposite page.

**Six-up grids (3)**
- `23-six-up-grid-2x3-generous` / `24-six-up-grid-2x3-tight` — page-aligned 2-column,
  3-row grids at two whitespace densities.
- `25-six-up-mosaic-hero-five` — one dominant hero plus five smaller photos in an
  asymmetric mosaic.

**Full-bleed background + inset photo(s) (2)**
- `26-full-bleed-bg-inset-corner` — a dominant background photo bleeding three edges, with
  a small matted inset in the reserved bottom-right corner.
- `27-full-bleed-bg-two-insets` — a wide background band with two insets side by side
  along the bottom, split cleanly at the fold.

**Text-forward layouts (3)**
- `28-text-forward-hero-side-quote` — a modest matted photo with a large adjacent text
  zone.
- `29-text-forward-title-spread` — a bottom-half photo bleeding three edges under a title
  text zone, single-page-confined to keep it clear of the gutter.
- `30-text-forward-centered-vignette` — a small photo and caption composed entirely on one
  page, the opposite page left as calm whitespace.

**Panorama spanning the fold (4)**
- `31-panorama-hero-bottom-supports` — a wide matted panorama over two small supports.
- `32-panorama-hero-flanked-supports` — a centred panorama flanked by two bleeding
  portrait strips at the outer edges.
- `33-panorama-matted-solo` — a single wide panorama, generously matted, no supports.
- `40-panorama-with-single-support` — a panorama paired with one small corner support.

**Portrait-oriented arrangements (4, plus overlaps with categories above)**
- `34-portrait-diptych` — two tall portraits bleeding top and bottom.
- `35-portrait-triptych-hero-left` — a tall portrait hero with two landscape supports.
- `36-portrait-single-text-right` — a single portrait with an adjacent text zone.
- `38-three-up-portrait-trio` — one tall portrait hero plus two narrower portrait supports.

**Extra composition variety (2)**
- `37-two-up-band-hero-corner-support` — a wide top band hero with one bleeding
  corner support, the remaining space left as deliberate whitespace.
- `39-four-up-filmstrip-footer` — four small footer thumbnails (paired at each page,
  split at the fold) under a title text zone and a large whitespace top half.

## Design decisions and trade-offs

- **`aspect_pref` is real-world (inches), not normalised.** Because the canvas itself is
  2.518:1, a slot's raw normalised width/height ratio reads as far more "landscape" than
  the photo it should actually hold. I converted every `aspect_pref` by the canvas ratio
  (matching the design doc's own worked example: `rect` w/h of 0.52 → real aspect ≈1.31,
  inside the doc's stated `[1.2, 1.6]`). This is now enforced, not just documented:
  `tests/templates.test.ts` computes each slot's real-world aspect ratio as
  `(w * 22.394) / (h * 8.894)` and asserts it falls inside that slot's own `aspect_pref`,
  with a comment explaining why the canvas dimensions appear in that formula — so a future
  reader doesn't "simplify" it back to the normalised ratio and silently break Phase 2's
  scoring.
  - **Outlier found and fixed:** `12-two-up-uneven-l-shape`'s second slot (`rect
    [0.52, 0.55, 0.45, 0.4]`, real aspect ≈2.83) had been given `aspect_pref: [1.3, 2.0]`
    — a plausible-looking landscape range that didn't actually describe this slot's own
    (fairly wide) shape. This was a plain authoring mistake, not a deliberate offset: the
    slot's geometry is intentional (the L-shaped composition), but the preference should
    describe the slot as drawn. Corrected to `[2.3, 3.3]`, matching the same
    natural-ratio × [0.82, 1.17] style used for every other wide support slot in the
    library (e.g. `13`/`14`'s stacked supports at `[2.0, 2.8]` for real aspect ≈2.4).
- **"Full-bleed background + inset" is built by adjacency, not z-order overlap.** The
  validator requires slots not to overlap, but a literal inset "on top of" a background
  photo is, geometrically, an overlapping rect. I resolved this by having the background
  slot bleed on 3 edges but stop short of the corner/strip the inset occupies (e.g.
  `26`'s background is `[0,0,0.78,1.0]`, leaving `x > 0.78` for the inset) — visually it
  still reads as a dominant background with an inset photo, without depending on
  slot z-ordering the current schema doesn't expose.
- **Odd-numbered column/row grids were avoided.** Any N-across arrangement with odd N puts
  a column straddling the fold with its centre in (or very near) the gutter band — bad
  practice per the design brief even though the validator doesn't enforce it for photo
  slots (only for text zones). All grids here use even column counts aligned to the two
  pages, or explicit page-aligned pairs (e.g. `39`'s footer thumbnails).
- **Title/quote text is confined to a single page half.** An early draft of
  `29-text-forward-title-spread` centred its title text zone across both pages; that rect's
  x-range crossed the gutter band even though its centre didn't, which the validator
  correctly rejects (gutter overlap is checked by x-range intersection, not centre point).
  Fixed by keeping all text zones within one page's safe half — a real constraint of this
  print geometry, not just a validator quirk.
- **Mirror pairs kept deliberately small.** Only layouts where "both handednesses" is part
  of the ask (single-image + text/whitespace, hero-left/right asymmetric 2-up, hero+3-up)
  have a mirrored sibling; grid, panorama, and mosaic designs each appear once.
- **`min_photos == max_photos == slot count`** for every template — none of the 40 have
  optional slots, so this collapses to a straightforward equality that the "slot count
  consistent" validator check confirms.

No templates under `sidecar/`, `src-tauri/src/`, or `docs/superpowers/` were touched.
