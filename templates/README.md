# Spread template library

This directory holds the curated layout templates the (future, Phase 2) scoring engine
picks from to lay out each two-page spread of a photobook. Each `NN-slug.json` file
describes where photos and text go on one spread. Background: see
`docs/superpowers/specs/2026-08-12-photobook-generator-design.md`, sections 3 and 7.

## Coordinate system

Everything is normalised to the spread canvas as `[x, y, width, height]` in `[0, 1]`,
origin top-left. The canvas is 22.394 × 8.894 inches (a 2-page spread at 300 DPI =
6718 × 2668 px), and **already includes** 5 mm of bleed on the outer, top, and bottom
edges — the canvas boundary (`0` and `1`) is the true edge of the printed sheet, not the
trim line.

| Region | Normalised x | Normalised y |
|---|---|---|
| Safe area | 0.008797 → 0.991203 | 0.022150 → 0.977850 |
| Fold centre | 0.5 | — |
| Gutter dead band | 0.491203 → 0.508797 | (full height) |

These constants come from the print geometry in section 3 of the design doc and must be
reproduced exactly — do not re-derive or round them differently. `tests/templates.test.ts`
hardcodes the same values and is the source of truth if this file and the test ever
disagree.

## The three geometric rules

1. **Text zones must lie entirely inside the safe area.** Anything outside gets trimmed
   off the printed page.
2. **Nothing important may overlap the gutter dead band**, the strip that disappears into
   the binding. A photo *may* span it — a full-bleed image crossing the fold is a
   legitimate, striking layout, and several templates here do it deliberately (see the
   panorama and asymmetric-hero designs). But a text zone must never overlap the band, and
   a small photo should never be centred on it.
3. **A full-bleed slot must extend past the canvas edge, not stop short of it.** A slot
   meant to bleed off the left edge starts at `x = 0` (or negative), never at `0.005` —
   stopping short leaves a white sliver after trimming. Every edge named in a slot's
   `bleed` array must actually reach the canvas boundary on that side; every edge *not*
   named must stay inside `[0, 1]` on that side.

## Subdivide the PAGE, never the spread

This is the rule that decides whether a template is usable or inert, and it is invisible in
the JSON. The spread canvas is **2.518:1** — very wide and very short. A "2 × 3 grid"
authored as two columns by three rows of the *spread* yields cells of 8.96" × 2.49", i.e.
**3.6:1 letterbox bands**. A 4:3 photo loses about 65% of its frame in one, and because a
clipped face is a *hard rejection* in the scorer, any such candidate containing a person is
thrown out entirely. Six of the original 40 templates were deleted for exactly this, and
seven more had to be recut.

A **page**, though, is 11.197" × 8.894" = **1.259:1** — close to 4:3. Cells that match real
photographs come from subdividing a page. Subdividing a page into `m` columns and `n` rows
gives cells of real aspect `1.259 × n / m`:

| Page subdivision | Cell aspect | Suits |
|---|---|---|
| 1 × 1 (whole page) | 1.259 | 4:3 (94%), 3:2 (84%) |
| 2 × 2 | 1.259 | 4:3 — the workhorse dense cell |
| 3 × 3 | 1.259 | 4:3, very dense |
| 2 cols × 1 row (side by side) | 0.630 | 2:3 (94%), 3:4 (84%) |
| 3 cols × 2 rows | 0.839 | 3:4 (89%) |
| half-page portrait + 2 stacked | 0.630 / 1.259 | the 3-up mosaic; both halves fit |
| 1 col × 2 rows (stacked) | 2.518 | genuinely panoramic photos only |
| 2 cols × 3 rows | 1.889 | **nothing** — 3:2 at 79%. Do not author this. |

Rule of thumb: a slot is usable when its real aspect retains **at least 80%** of the frame
of its nearest common camera aspect (4:3, 3:2, 3:4, 2:3). Below that the slot cannot hold a
picture of a person at all.

Two consequences worth internalising:

- **Two full-width photos stacked on one page can never both fit.** Each would need ~79% of
  the page height to be 4:3. If you want two landscapes on one page they must be a narrower
  centred column, which leaves wide side margins — that is the geometry, not sloppiness.
- **Running flush TO the fold is legal and wanted**; crossing it is not. A slot ending at
  exactly `x = 0.5`, or starting at exactly `0.5`, is how fold-flush and full-bleed spreads
  are built. Keep the *subject* out of the innermost 0.197", which curls into the binding —
  the scorer enforces that for faces, but composition is the author's job.

## Look at it before you trust it

```sh
bun scripts/template-sheet.ts > /tmp/templates.html && open /tmp/templates.html
```

Draws every file in this directory to scale with the fold, the gutter dead band, the trim /
safe area, every bleed edge, and each slot's real aspect next to the nearest camera aspect
and the fraction of the frame that survives. Slots that fit nothing are drawn in red. The
letterbox failure above is obvious on the sheet and invisible in the rect arrays.

## Template format

```jsonc
{
  "id": "hero-left-2up",           // must equal the filename, minus ".json"
  "slots": [
    { "rect": [0.0, 0.0, 0.52, 1.0], "role": "hero",
      "bleed": ["left", "top", "bottom"], "aspect_pref": [1.2, 1.6] },
    { "rect": [0.58, 0.18, 0.36, 0.64], "role": "support",
      "bleed": [], "aspect_pref": [1.0, 1.5] }
  ],
  "text_zones": [ { "rect": [0.58, 0.86, 0.36, 0.08], "align": "left" } ],
  "min_photos": 2, "max_photos": 2,
  "density": "sparse", "energy": "calm"
}
```

- `role`: `"hero"` (the dominant photo) or `"support"`. Not every template needs a hero —
  symmetric and grid layouts often mark every slot `"support"`.
- `bleed`: any subset of `"left"`, `"right"`, `"top"`, `"bottom"`.
- `aspect_pref`: `[min, max]`, the real-world (inches) width/height ratio this slot suits.
  Because the canvas itself is 2.518:1 (wide), a slot's *normalised* w/h ratio is not its
  real aspect — multiply by 2.518 (canvas width ÷ height) to convert. A tall portrait slot
  reads as roughly `[0.6, 0.95]`; a wide panorama slot as `[2.5, 5.0]`. This is enforced by
  `tests/templates.test.ts`: every slot's own real-world aspect ratio must fall inside its
  declared `aspect_pref`, so an accidentally-normalised range fails the build instead of
  silently mis-scoring Phase 2's template selection.
- `density`: `"sparse" | "medium" | "dense"`.
- `energy`: `"calm" | "neutral" | "lively"`.
- `text_zones` may be empty.
- **`min_photos == max_photos == slots.length` for every template in this library.** Each
  template accepts an exact photo count, not a range — none of the 40 have optional slots.
  This is a design choice, not a schema requirement: the format supports `min_photos <
  max_photos` for a template with optional slots, but no template here uses it. If you add
  one, the Phase 2 packer will need to handle a template whose slot count varies by photo
  count, which no existing template exercises.

## Adding a template

1. Copy the closest existing template as a starting point rather than writing rects from
   scratch — most of the geometry mistakes to avoid (gutter-straddling text, bleed edges
   that don't quite reach 0/1, accidental slot overlap) are easiest to sidestep by editing
   a known-good layout.
2. Name the file `NN-slug.json` where `NN` is the next unused two-digit number and `slug`
   is a short, descriptive, kebab-case name. Set `"id"` to exactly that filename stem.
3. Respect the three rules above. In particular: if a photo slot is meant to bleed, set
   the relevant edge to exactly `0` or `1` (never a near-boundary value like `0.002`); if
   it isn't meant to bleed, keep it inside `[0, 1]` on that edge.
4. Run `bun run test` (which runs `tests/templates.test.ts` against every file in this
   directory). The validator names the offending file and rect on any failure — fix and
   re-run rather than trying to reason about the geometry by hand.
5. Look at the resulting rect numbers with a mental picture of the spread: does it read as
   a layout a person would want printed? Vary the whitespace ratio and hierarchy between
   hero and support — avoid defaulting to evenly-sized grids unless a grid is genuinely
   the intended composition.
6. Mirror-image variants (e.g. hero-left ↔ hero-right) are useful for handedness balance
   but count as one *design* — don't add both just to pad the library. Only add a mirror
   when the brief specifically calls for both handednesses, or when varying whitespace
   would otherwise be a near-duplicate of an existing template.
