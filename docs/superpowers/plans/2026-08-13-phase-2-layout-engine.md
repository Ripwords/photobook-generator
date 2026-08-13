# Phase 2 — Layout Engine and Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the analysed photos from Phase 1 into print-resolution page files plus a manifest, so a book can be assembled in Pixajoy's editor.

**Architecture:** A pure-Rust layout engine (cull → pack → crop → score → pace) consumes the *full* Phase 1 feature records, which Rust already holds as `serde_json::Value` and caches in SQLite. It emits a `Book` of pages. The Swift sidecar gains one new `render` request that writes one transparent PNG per photo at page canvas size. Nuxt gains a single "Generate book" step. The spread JSON stays the authoring format; the page is the engine's atom.

**Tech Stack:** Rust (Tauri 2), Swift 6 + Core Graphics (sidecar), Nuxt 4 + Vue 3, vitest, swift-testing, cargo test.

**Spec:** `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md`

## Global Constraints

- **Images never leave the machine.** All pixel work is on-device. Only derived JSON may be sent anywhere.
- **arm64 only, macOS 15+.** Never configure a universal build.
- **`bun run sidecar` before any `cargo` command.** `tauri-build` validates the `externalBin` path at compile time.
- Bun, not npm. Conventional Commits. Never `git commit --no-verify`.
- Never use `any` in TypeScript. `bun run lint` (oxlint) treats warnings as failures.
- **swift-testing, not XCTest.** Top-level `@Test func`s share one module namespace — **prefix every test name by area** or it is a compile error.
- Page canvas: **11.197" × 8.894" = 3359 × 2668 px @ 300 DPI**. Spread canvas: 22.394" × 8.894".
- Bleed: **0.197"** (5 mm) on the three outer edges. Gutter dead strip: **0.197"** inward from the fold.
- `aspect_pref` in `templates/*.json` is a **real-world inch ratio**, never the normalised rect ratio. The spread canvas is 2.518:1, so conflating them mis-scores every slot.
- **Extract pure functions.** The codebase does this five times so core logic is reachable without an `AppHandle`. Follow it.
- **Do not trust that a test tests what it says.** Break the implementation deliberately and confirm the test notices. Mutation-check anything load-bearing and paste the evidence.

### Test anti-patterns that are plan failures

Phase 1 shipped eight tests that passed under a broken implementation. Each rule below is a real bug from that phase:

1. **No square fixtures** for anything aspect-dependent — `height/width == 1` hides the entire class of bug.
2. **Aspect fixtures must have differing normalised and inch ratios**, or the test passes under the exact `aspect_pref` trap it exists to catch.
3. **Boundary tests use values at the boundary**, not near it.
4. **Ordering tests use unsorted input.**
5. **A zero-width box can "pass" a guard test via NaN propagation.** Use a negative width.

---

## File Structure

**Created:**

| File | Responsibility |
|---|---|
| `src-tauri/src/geometry.rs` | `Rect`, `Side`, page/spread constants, the three predicates, spread→page conversion |
| `src-tauri/src/templates.rs` | Loading `templates/*.json`, decomposing to `PageLayout`, the page-half pool, weights |
| `src-tauri/src/book/mod.rs` | `Book`/`Page`/`Placement` types and the `assemble` orchestrator |
| `src-tauri/src/book/cull.rs` | `cull` — the single authority on which photo survives |
| `src-tauri/src/book/pack.rs` | Chapters → per-spread photo groups |
| `src-tauri/src/book/crop.rs` | Deterministic crop-window selection |
| `src-tauri/src/book/score.rs` | Hard constraints + weighted soft terms |
| `src-tauri/src/book/pace.rs` | Density/energy/edge-treatment variation sweep |
| `src-tauri/src/book/manifest.rs` | `manifest.json` shape and serialisation |
| `src-tauri/src/book/preflight.rs` | Block/warn checks before any file is written |
| `src-tauri/src/render.rs` | Rust-side render client: builds render requests, calls the sidecar |
| `sidecar/Sources/PhotobookEngine/Renderer.swift` | Core Graphics composite of one transparent page PNG |
| `sidecar/Tests/PhotobookEngineTests/RendererTests.swift` | Renderer tests |
| `templates/weights.json` | Soft-term weights, hot-reloadable |
| `app/types/book.ts` | TypeScript mirror of the book/manifest wire types |
| `app/composables/useBook.ts` | Generate-book invocation and progress state |
| `app/components/GenerateBook.vue` | The generate step UI |
| `scripts/template-sheet.ts` | Renders the visual contact sheet of all templates |
| `src-tauri/tests/render_roundtrip.rs` | End-to-end against the real sidecar binary |

**Modified:**

| File | Change |
|---|---|
| `tests/templates.test.ts` | New fold-spanning rule, page-decomposition check, corrected count bound |
| `src-tauri/src/protocol.rs` | `RequestKind::Render`, `ResponseResult::Rendered` |
| `sidecar/Sources/PhotobookEngine/Protocol.swift` | Matching `render` case and `RenderRecord` |
| `sidecar/Sources/PhotobookEngine/Handler.swift` | Dispatch `.render` |
| `src-tauri/src/lib.rs` | Register new modules and the `generate_book` command |
| `src-tauri/src/commands.rs` | Extract `count_keepers` to `book::cull`; expose full feature records |
| `src-tauri/tauri.conf.json` | `bundle.resources` for `templates/` |
| `app/types/features.ts` | `keepers()` delegates to the Rust-owned rule via the book command |
| `app/pages/index.vue` | Mount the generate step |
| `docs/PROJECT-STATUS.md` | Phase 2 status, resolved unknowns |

---

## Task 0: Verify Pixajoy's alpha behaviour

**This is a human task and it gates Tasks 9–11 only.** Tasks 1–8 are independent of the outcome and should proceed in parallel.

**Files:** none.

- [ ] **Step 1: Make a test PNG**

```bash
mkdir -p /tmp/alpha-test && cd /tmp/alpha-test
# 3359x2668 fully transparent, with an opaque red square in the middle-left
magick -size 3359x2668 xc:none -fill red -draw "rectangle 200,200 1400,1200" a.png
magick -size 3359x2668 xc:none -fill blue -draw "rectangle 1800,1400 3100,2400" b.png
ls -lh a.png b.png
```

If ImageMagick is not installed: `brew install imagemagick`.

- [ ] **Step 2: Upload both into one Pixajoy page**

In the editor, on any spread page: `Add Picture Box`, drag it to the blue bleed line, drop `a.png` in. Repeat for `b.png` on the same page, also dragged to the bleed line so the two boxes fully overlap.

- [ ] **Step 3: Record three answers**

1. Is the area outside the red square **transparent** (page background shows through) or **white/black**?
2. With both boxes stacked, are **both** the red and blue squares visible?
3. Did either upload fail or get re-encoded? Note each file's size — they will be ~1–3 MB.

- [ ] **Step 4: Record the outcome in the spec**

Edit `docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md` §8, replacing open item 1's "How to settle" cell with the measured result and the date.

```bash
git add docs/superpowers/specs/2026-08-13-phase-2-layout-engine-design.md
git commit -m "docs: record measured Pixajoy alpha behaviour"
```

**Decision gate:** if answers 1 and 2 are both favourable, proceed with Tasks 9–11 as written. If either fails, stop and re-plan Tasks 9–11 against §5.2's fallback (cropped photos plus an app-rendered visual placement guide, with widened engine safety margins). Do not attempt the transparent-PNG path anyway.

---

## Task 1: Cull the template library and add the fold-spanning rule

**Files:**
- Modify: `tests/templates.test.ts`
- Delete: 21 files under `templates/`

**Interfaces:**
- Produces: a `templates/` directory containing exactly 19 files, none with a fold-spanning slot. Every later task depends on this.

- [ ] **Step 1: Write the failing rule test**

Add to `tests/templates.test.ts`, inside the `describe(filename, ...)` block, after the existing `"no text zone overlaps the gutter dead band"` test:

```ts
      it("no slot spans the fold", () => {
        template.slots.forEach((slot, i) => {
          const [x, , w] = slot.rect;
          const label = rectLabel(filename, "slots", i, slot.rect);
          expect(
            x < FOLD_X - EPS && x + w > FOLD_X + EPS,
            `${label}: spans the fold at x=${FOLD_X}. Pixajoy picture boxes are ` +
              `page-local and cannot cross the fold, so this template cannot be built. ` +
              `Running flush TO the fold is legal; crossing it is not.`,
          ).toBe(false);
        });
      });
```

And add the constant beside `GUTTER_X0`/`GUTTER_X1`:

```ts
/** The fold, normalised on the spread canvas: 11.197" of 22.394". */
const FOLD_X = 0.5;
```

- [ ] **Step 2: Run it and confirm it fails on exactly 15 templates**

Run: `bun run test tests/templates.test.ts`
Expected: FAIL. Exactly 15 failures, one per file: `01`, `02`, `09`, `10`, `15`, `16`, `17`, `26`, `27`, `29`, `31`, `32`, `33`, `37`, `40`.

If the count is not 15, stop — the geometry constant is wrong, not the templates.

- [ ] **Step 3: Delete the 21 dead templates**

15 fold-spanning, plus 6 whose slots fit no common camera aspect (see spec §6.1 — these are not revisable in place, because any 2×2 or 2×3 subdivision *of the spread* yields wide cells no matter how the rects are nudged):

```bash
cd /Users/jiajingteoh/Documents/photobook-generator
git rm templates/01-full-bleed-1up.json \
       templates/02-full-bleed-1up-corner-caption.json \
       templates/09-two-up-hero-left-asym.json \
       templates/10-two-up-hero-right-asym.json \
       templates/15-three-up-hero-top-two-bottom.json \
       templates/16-three-up-hero-bottom-two-top.json \
       templates/17-three-up-hero-diagonal-lively.json \
       templates/26-full-bleed-bg-inset-corner.json \
       templates/27-full-bleed-bg-two-insets.json \
       templates/29-text-forward-title-spread.json \
       templates/31-panorama-hero-bottom-supports.json \
       templates/32-panorama-hero-flanked-supports.json \
       templates/33-panorama-matted-solo.json \
       templates/37-two-up-band-hero-corner-support.json \
       templates/40-panorama-with-single-support.json \
       templates/12-two-up-uneven-l-shape.json \
       templates/18-four-up-grid-2x2-generous.json \
       templates/19-four-up-grid-2x2-tight.json \
       templates/23-six-up-grid-2x3-generous.json \
       templates/24-six-up-grid-2x3-tight.json \
       templates/39-four-up-filmstrip-footer.json
ls templates/*.json | wc -l   # expect 19
```

- [ ] **Step 4: Fix the library-size assertion**

The existing test asserts 35–50 templates and now fails. Replace it:

```ts
  it("contains at least 19 templates", () => {
    // 40 authored, minus 15 fold-spanning and 6 whose slots fit no common
    // camera aspect (spec 2026-08-13 sections 1 and 6.1). The upper bound is
    // deliberately gone: Task 13 authors 15-20 more, and a ceiling here would
    // fail on the authoring commit for no design reason.
    expect(loaded.length).toBeGreaterThanOrEqual(19);
  });
```

- [ ] **Step 5: Add the page-decomposition check**

Every slot, once converted to its page's coordinates, must land inside that page. This is plain arithmetic, deliberately *not* shared with the Rust decomposition — duplicating a formula is cheaper than the `count_keepers`-style split-authority bug that sharing across languages has already caused here.

```ts
      it("every slot decomposes into a valid page-local rect", () => {
        template.slots.forEach((slot, i) => {
          const [x, y, w, h] = slot.rect;
          const label = rectLabel(filename, "slots", i, slot.rect);
          const onLeft = x + w <= FOLD_X + EPS;
          const u = onLeft ? x / FOLD_X : (x - FOLD_X) / FOLD_X;
          const uw = w / FOLD_X;
          expect(u, `${label}: page-local x must be >= 0`).toBeGreaterThanOrEqual(-EPS);
          expect(u + uw, `${label}: page-local x+w must be <= 1`).toBeLessThanOrEqual(1 + EPS);
          expect(y, `${label}: page-local y must be >= 0`).toBeGreaterThanOrEqual(-EPS);
          expect(y + h, `${label}: page-local y+h must be <= 1`).toBeLessThanOrEqual(1 + EPS);
        });
      });
```

- [ ] **Step 6: Run the full suite**

Run: `bun run test && bun run lint`
Expected: PASS, 19 templates validated.

- [ ] **Step 7: Mutation-check the new rule**

Temporarily add a template that spans the fold and confirm the rule catches it:

```bash
cat > templates/99-mutation-check.json <<'EOF'
{"id":"99-mutation-check","slots":[{"rect":[0.2,0.1,0.6,0.8],"role":"hero","bleed":[],
"aspect_pref":[3.7,3.9]}],"text_zones":[],"min_photos":1,"max_photos":1,
"density":"sparse","energy":"calm"}
EOF
bun run test tests/templates.test.ts   # expect FAIL on "no slot spans the fold"
rm templates/99-mutation-check.json
bun run test tests/templates.test.ts   # expect PASS
```

Paste both outputs into the commit body.

- [ ] **Step 8: Commit**

```bash
git add tests/templates.test.ts templates/
git commit -m "feat(templates): forbid fold-spanning slots, remove 21 unbuildable templates"
```

---

## Task 2: Rust geometry module

**Files:**
- Create: `src-tauri/src/geometry.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }` with `Rect::new(x,y,w,h)`, `right()`, `bottom()`, `area()`, `intersect(&self, other: &Rect) -> Option<Rect>`, `aspect_in(&self, canvas_w_in: f64, canvas_h_in: f64) -> f64`
  - `pub enum Side { Left, Right }`
  - `pub fn spread_to_page(rect: &Rect) -> Option<(Side, Rect)>`
  - `pub fn in_trim(rect: &Rect, side: Side) -> bool`
  - `pub fn bleeds_correctly(rect: &Rect, edges: &[BleedEdge], side: Side) -> bool`
  - `pub fn clear_of_gutter(rect: &Rect, side: Side) -> bool`
  - `pub enum BleedEdge { Left, Right, Top, Bottom }`
  - constants `PAGE_W_IN`, `PAGE_H_IN`, `PAGE_W_PX`, `PAGE_H_PX`, `SPREAD_W_IN`, `SPREAD_H_IN`, `BLEED_IN`, `TRIM_U`, `TRIM_V`, `GUTTER_U`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/geometry.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // --- geometry: spread -> page conversion

    #[test]
    fn geometry_converts_a_left_page_rect_to_page_coordinates() {
        // Spans a quarter of the spread starting at the left edge -> the left
        // half of the LEFT page. Deliberately not symmetric about anything.
        let r = Rect::new(0.05, 0.1, 0.20, 0.6);
        let (side, page) = spread_to_page(&r).expect("does not span the fold");
        assert_eq!(side, Side::Left);
        assert!((page.x - 0.10).abs() < 1e-9, "x was {}", page.x);
        assert!((page.w - 0.40).abs() < 1e-9, "w was {}", page.w);
        assert!((page.y - 0.1).abs() < 1e-9, "y is unchanged");
        assert!((page.h - 0.6).abs() < 1e-9, "h is unchanged");
    }

    #[test]
    fn geometry_converts_a_right_page_rect_to_page_coordinates() {
        let r = Rect::new(0.55, 0.1, 0.20, 0.6);
        let (side, page) = spread_to_page(&r).expect("does not span the fold");
        assert_eq!(side, Side::Right);
        assert!((page.x - 0.10).abs() < 1e-9, "x was {}", page.x);
        assert!((page.w - 0.40).abs() < 1e-9, "w was {}", page.w);
    }

    #[test]
    fn geometry_rejects_a_rect_that_spans_the_fold() {
        assert!(spread_to_page(&Rect::new(0.4, 0.0, 0.2, 1.0)).is_none());
    }

    /// Flush TO the fold is legal on both sides — this is the whole reason
    /// the rule is "spans the fold" rather than "touches the dead band".
    #[test]
    fn geometry_accepts_a_rect_flush_to_the_fold_from_either_side() {
        let (side, page) = spread_to_page(&Rect::new(0.3, 0.0, 0.2, 1.0)).unwrap();
        assert_eq!(side, Side::Left);
        assert!((page.right() - 1.0).abs() < 1e-9);

        let (side, page) = spread_to_page(&Rect::new(0.5, 0.0, 0.2, 1.0)).unwrap();
        assert_eq!(side, Side::Right);
        assert!((page.x - 0.0).abs() < 1e-9);
    }

    // --- geometry: predicates

    #[test]
    fn geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_left_page() {
        // On a LEFT page the outer edge is x=0, so trim starts at TRIM_U.
        assert!(!in_trim(&Rect::new(0.0, 0.5, 0.1, 0.1), Side::Left));
        assert!(in_trim(&Rect::new(TRIM_U, 0.5, 0.1, 0.1), Side::Left));
    }

    #[test]
    fn geometry_in_trim_rejects_a_rect_over_the_outer_bleed_on_a_right_page() {
        // On a RIGHT page the outer edge is x=1, so trim ends at 1 - TRIM_U.
        assert!(!in_trim(&Rect::new(0.95, 0.5, 0.1, 0.1), Side::Right));
        assert!(in_trim(&Rect::new(0.8, 0.5, 1.0 - TRIM_U - 0.8, 0.1), Side::Right));
    }

    /// Boundary test AT the boundary, per the plan's anti-pattern rules.
    #[test]
    fn geometry_clear_of_gutter_is_exact_at_the_strip_edge() {
        let strip_start = 1.0 - GUTTER_U;
        assert!(clear_of_gutter(&Rect::new(0.5, 0.4, strip_start - 0.5, 0.2), Side::Left));
        assert!(!clear_of_gutter(
            &Rect::new(0.5, 0.4, strip_start - 0.5 + 1e-6, 0.2),
            Side::Left
        ));
    }

    #[test]
    fn geometry_clear_of_gutter_uses_the_opposite_edge_on_a_right_page() {
        assert!(clear_of_gutter(&Rect::new(GUTTER_U, 0.4, 0.2, 0.2), Side::Right));
        assert!(!clear_of_gutter(&Rect::new(GUTTER_U - 1e-6, 0.4, 0.2, 0.2), Side::Right));
    }

    #[test]
    fn geometry_bleeds_correctly_requires_reaching_past_the_declared_edge() {
        let flush = Rect::new(0.0, 0.0, 0.5, 1.0);
        assert!(bleeds_correctly(&flush, &[BleedEdge::Left], Side::Left));
        let short = Rect::new(0.01, 0.0, 0.5, 1.0);
        assert!(!bleeds_correctly(&short, &[BleedEdge::Left], Side::Left));
    }

    /// A left page has no bleed at the fold: declaring `right` bleed on a
    /// left page is meaningless and must not be satisfiable.
    #[test]
    fn geometry_bleeds_correctly_rejects_bleed_at_the_fold_edge() {
        let to_fold = Rect::new(0.5, 0.0, 0.5, 1.0);
        assert!(!bleeds_correctly(&to_fold, &[BleedEdge::Right], Side::Left));
    }

    // --- geometry: aspect

    /// The `aspect_pref` trap, pinned. A slot half the canvas wide and half
    /// tall is 1:1 normalised but 2.518:1 in inches. A fixture where those
    /// two numbers coincide would pass under the bug.
    #[test]
    fn geometry_aspect_in_uses_real_inches_not_the_normalised_ratio() {
        let r = Rect::new(0.0, 0.0, 0.5, 0.5);
        let normalised = r.w / r.h;
        let real = r.aspect_in(SPREAD_W_IN, SPREAD_H_IN);
        assert!((normalised - 1.0).abs() < 1e-9, "sanity: normalised is 1:1");
        assert!((real - 2.518).abs() < 0.001, "real was {real}");
        assert!((real - normalised).abs() > 1.0, "the fixture must distinguish the two");
    }

    #[test]
    fn geometry_page_pixels_sum_to_the_spread_width() {
        assert_eq!(PAGE_W_PX * 2, 6718);
        assert_eq!(PAGE_H_PX, 2668);
    }

    // --- geometry: intersection

    #[test]
    fn geometry_intersect_returns_none_when_disjoint() {
        let a = Rect::new(0.0, 0.0, 0.2, 0.3);
        let b = Rect::new(0.5, 0.6, 0.2, 0.3);
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn geometry_intersect_returns_the_overlap() {
        let a = Rect::new(0.0, 0.0, 0.4, 0.6);
        let b = Rect::new(0.2, 0.3, 0.4, 0.6);
        let i = a.intersect(&b).unwrap();
        assert!((i.x - 0.2).abs() < 1e-9);
        assert!((i.w - 0.2).abs() < 1e-9);
        assert!((i.h - 0.3).abs() < 1e-9);
    }

    /// A zero-width box can "pass" a guard via NaN propagation, so guard
    /// tests use a NEGATIVE width — see the plan's anti-pattern rules.
    #[test]
    fn geometry_intersect_rejects_a_negative_width_rect() {
        let a = Rect::new(0.0, 0.0, -0.4, 0.6);
        let b = Rect::new(0.0, 0.0, 0.4, 0.6);
        assert!(a.intersect(&b).is_none());
    }
}
```

- [ ] **Step 2: Run and confirm it fails to compile**

```bash
bun run sidecar && cargo test --manifest-path src-tauri/Cargo.toml geometry
```
Expected: FAIL — `cannot find type Rect in this scope`.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/geometry.rs`:

```rust
//! Page-local geometry for the layout engine.
//!
//! The authoring format is the SPREAD canvas (22.394" x 8.894", normalised
//! [0,1]); the engine's atom is the PAGE (11.197" x 8.894"). Pixajoy's
//! picture boxes are page-local and cannot cross the fold, so a rect that
//! spans the fold has no page representation at all -- `spread_to_page`
//! returns `None` rather than clamping, and the template validator rejects
//! such slots before they ever reach here.

use serde::{Deserialize, Serialize};

pub const SPREAD_W_IN: f64 = 22.394;
pub const SPREAD_H_IN: f64 = 8.894;
pub const PAGE_W_IN: f64 = 11.197;
pub const PAGE_H_IN: f64 = 8.894;

/// 11.197" and 8.894" at 300 DPI. Two pages sum to 6718 px, matching the
/// spread canvas exactly, so no rounding drift accumulates across a book.
pub const PAGE_W_PX: u32 = 3359;
pub const PAGE_H_PX: u32 = 2668;

/// 5 mm, on the three OUTER edges only. There is no bleed at the fold.
pub const BLEED_IN: f64 = 0.197;

/// Page-normalised trim inset on the outer vertical edge, and on top/bottom.
pub const TRIM_U: f64 = BLEED_IN / PAGE_W_IN;
pub const TRIM_V: f64 = BLEED_IN / PAGE_H_IN;

/// Page-normalised width of the gutter dead strip, measured inward from the
/// fold. Numerically equal to `TRIM_U` (both are 0.197" on an 11.197" page)
/// but conceptually unrelated -- one is a guillotine allowance, the other is
/// where the paper curls into the binding. Kept separate so changing one
/// does not silently change the other.
pub const GUTTER_U: f64 = BLEED_IN / PAGE_W_IN;

/// The fold, normalised on the spread canvas.
const FOLD_X: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn area(&self) -> f64 {
        if self.w <= 0.0 || self.h <= 0.0 {
            0.0
        } else {
            self.w * self.h
        }
    }

    /// Real-world width/height ratio on a canvas of the given inch
    /// dimensions. NEVER compare `aspect_pref` against `w / h` -- the
    /// canvases are not square, so the normalised ratio and the inch ratio
    /// are different numbers (2.518x apart on the spread canvas).
    pub fn aspect_in(&self, canvas_w_in: f64, canvas_h_in: f64) -> f64 {
        (self.w * canvas_w_in) / (self.h * canvas_h_in)
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        if self.w <= 0.0 || self.h <= 0.0 || other.w <= 0.0 || other.h <= 0.0 {
            return None;
        }
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r <= x || b <= y {
            return None;
        }
        Some(Rect::new(x, y, r - x, b - y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BleedEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Converts a spread-normalised rect into `(side, page-normalised rect)`.
/// Returns `None` for a rect that spans the fold: such a rect cannot be
/// built in Pixajoy's editor and has no page representation.
pub fn spread_to_page(rect: &Rect) -> Option<(Side, Rect)> {
    const EPS: f64 = 1e-9;
    if rect.x < FOLD_X - EPS && rect.right() > FOLD_X + EPS {
        return None;
    }
    let side = if rect.right() <= FOLD_X + EPS { Side::Left } else { Side::Right };
    let origin = match side {
        Side::Left => 0.0,
        Side::Right => FOLD_X,
    };
    Some((side, Rect::new((rect.x - origin) / FOLD_X, rect.y, rect.w / FOLD_X, rect.h)))
}

/// True when the rect lies wholly inside the trim rectangle of its page.
/// The fold edge has no trim inset -- the paper is continuous there.
pub fn in_trim(rect: &Rect, side: Side) -> bool {
    const EPS: f64 = 1e-9;
    let (x0, x1) = match side {
        Side::Left => (TRIM_U, 1.0),
        Side::Right => (0.0, 1.0 - TRIM_U),
    };
    rect.x >= x0 - EPS
        && rect.right() <= x1 + EPS
        && rect.y >= TRIM_V - EPS
        && rect.bottom() <= 1.0 - TRIM_V + EPS
}

/// True when the rect keeps clear of the gutter dead strip -- the 0.197"
/// nearest the fold, which curls into the binding. This is a CONTENT
/// predicate (faces, salient regions), never a slot rejection: a slot may
/// legitimately run flush to the fold.
pub fn clear_of_gutter(rect: &Rect, side: Side) -> bool {
    const EPS: f64 = 1e-9;
    match side {
        Side::Left => rect.right() <= 1.0 - GUTTER_U + EPS,
        Side::Right => rect.x >= GUTTER_U - EPS,
    }
}

/// True when every declared bleed edge actually reaches past the page
/// boundary. A slot that declares bleed but stops short leaves a white
/// sliver after trimming.
///
/// The fold edge cannot bleed: on a left page that is the RIGHT edge, on a
/// right page the LEFT edge. Declaring it is always an error.
pub fn bleeds_correctly(rect: &Rect, edges: &[BleedEdge], side: Side) -> bool {
    const EPS: f64 = 1e-9;
    edges.iter().all(|edge| match (edge, side) {
        (BleedEdge::Right, Side::Left) | (BleedEdge::Left, Side::Right) => false,
        (BleedEdge::Left, Side::Left) => rect.x <= EPS,
        (BleedEdge::Right, Side::Right) => rect.right() >= 1.0 - EPS,
        (BleedEdge::Top, _) => rect.y <= EPS,
        (BleedEdge::Bottom, _) => rect.bottom() >= 1.0 - EPS,
    })
}
```

Register it in `src-tauri/src/lib.rs`, keeping the module list alphabetical:

```rust
pub mod cluster;
pub mod commands;
pub mod db;
pub mod geometry;
pub mod protocol;
pub mod ranking;
pub mod sidecar;
```

- [ ] **Step 4: Run the tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml geometry
```
Expected: PASS, 13 tests.

- [ ] **Step 5: Mutation-check the fold rule and the aspect trap**

```bash
# 1. Make spread_to_page clamp instead of rejecting: change the `return None`
#    to `return Some((Side::Left, *rect))`.
cargo test --manifest-path src-tauri/Cargo.toml geometry
# Expect FAIL on geometry_rejects_a_rect_that_spans_the_fold. Revert.

# 2. Make aspect_in return the normalised ratio: `self.w / self.h`.
cargo test --manifest-path src-tauri/Cargo.toml geometry
# Expect FAIL on geometry_aspect_in_uses_real_inches_not_the_normalised_ratio. Revert.
```

Paste both failures into the commit body. If either mutation does **not** fail a test, the test is decoration — fix it before continuing.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/geometry.rs src-tauri/src/lib.rs
git commit -m "feat(geometry): page-local rects, predicates and spread decomposition"
```

---

## Task 2b: Property tests on the predicates

**Files:**
- Modify: `src-tauri/src/geometry.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: everything from Task 2.
- Produces: nothing new; hardens the predicates.

- [ ] **Step 1: Add the dev-dependency**

In `src-tauri/Cargo.toml` under `[dev-dependencies]`:

```toml
# Property tests on the geometry predicates. The design doc (section 12)
# calls for these by name: the interesting failures are the ones no
# hand-written fixture thinks to try.
proptest = "1"
```

- [ ] **Step 2: Write the failing property tests**

Append inside `mod tests` in `src-tauri/src/geometry.rs`:

```rust
    use proptest::prelude::*;

    prop_compose! {
        /// Rects with strictly positive extent, sized so they can sit on one
        /// page. Deliberately excludes squares in normalised space being the
        /// ONLY case: w and h vary independently.
        fn any_page_rect()(
            x in 0.0f64..0.9,
            y in 0.0f64..0.9,
            w in 0.01f64..0.5,
            h in 0.01f64..0.5,
        ) -> Rect {
            Rect::new(x, y, w.min(1.0 - x), h.min(1.0 - y))
        }
    }

    proptest! {
        /// The core invariant: a rect inside the trim rectangle of a left
        /// page can never simultaneously intrude on the gutter strip of that
        /// page... except that it can, because trim runs TO the fold. This
        /// property therefore asserts the opposite and documents why: the
        /// two predicates are independent by design, and any implementation
        /// that makes `in_trim` imply `clear_of_gutter` has wrongly folded
        /// the content rule into the geometry rule.
        #[test]
        fn geometry_prop_in_trim_does_not_imply_clear_of_gutter(r in any_page_rect()) {
            let flush = Rect::new(r.x, r.y, (1.0 - r.x).max(0.01), r.h);
            if in_trim(&flush, Side::Left) && flush.right() > 1.0 - GUTTER_U {
                prop_assert!(!clear_of_gutter(&flush, Side::Left));
            }
        }

        /// Round-tripping through the decomposition preserves width in
        /// inches: a page is exactly half the spread's width, so a rect's
        /// real-world width must be identical before and after.
        #[test]
        fn geometry_prop_decomposition_preserves_real_width(r in any_page_rect()) {
            let on_left = Rect::new(r.x * 0.5, r.y, r.w * 0.5, r.h);
            if let Some((side, page)) = spread_to_page(&on_left) {
                prop_assert_eq!(side, Side::Left);
                let before_in = on_left.w * SPREAD_W_IN;
                let after_in = page.w * PAGE_W_IN;
                prop_assert!((before_in - after_in).abs() < 1e-9,
                    "{} vs {}", before_in, after_in);
            }
        }

        /// `intersect` is commutative and never yields more area than either
        /// input -- the two ways a naive min/max implementation goes wrong.
        #[test]
        fn geometry_prop_intersect_is_commutative_and_bounded(
            a in any_page_rect(), b in any_page_rect()
        ) {
            prop_assert_eq!(a.intersect(&b), b.intersect(&a));
            if let Some(i) = a.intersect(&b) {
                prop_assert!(i.area() <= a.area() + 1e-9);
                prop_assert!(i.area() <= b.area() + 1e-9);
            }
        }
    }
```

- [ ] **Step 3: Run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml geometry_prop
```
Expected: PASS, 3 property tests.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/geometry.rs
git commit -m "test(geometry): property tests on the layout predicates"
```

---

## Task 3: Template loading and page decomposition

**Files:**
- Create: `src-tauri/src/templates.rs`
- Create: `templates/weights.json`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: `geometry::{Rect, Side, BleedEdge, spread_to_page}`.
- Produces:
  - `pub struct Slot { pub rect: Rect, pub role: Role, pub bleed: Vec<BleedEdge>, pub aspect_pref: (f64, f64) }`
  - `pub enum Role { Hero, Support }`, `pub enum Density { Sparse, Medium, Dense }`, `pub enum Energy { Calm, Neutral, Lively }`, `pub enum EdgeTreatment { Bleed, Margin }`
  - `pub struct PageLayout { pub side: Side, pub slots: Vec<Slot>, pub edge_treatment: EdgeTreatment }`
  - `pub struct SpreadTemplate { pub id: String, pub left: PageLayout, pub right: PageLayout, pub density: Density, pub energy: Energy }` with `photo_count(&self) -> usize`
  - `pub struct Library { pub spreads: Vec<SpreadTemplate> }` with `Library::load(dir: &Path) -> Result<Library, TemplateError>`, `spreads_with(&self, n: usize) -> Vec<&SpreadTemplate>`, `page_half_pool(&self) -> Vec<&PageLayout>`
  - `pub struct Weights { … }` with `Weights::load(path: &Path) -> Result<Weights, TemplateError>` and `Weights::default()`
  - `pub enum TemplateError { Io(String), Parse(String), SpansFold { template: String, slot: usize } }`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/templates.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_template(dir: &std::path::Path, name: &str, json: &str) {
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        f.write_all(json.as_bytes()).unwrap();
    }

    /// Two slots, one per page, with DIFFERENT extents so a left/right mixup
    /// is detectable. Never symmetric: a mirrored fixture passes under a
    /// side-swap bug.
    const TWO_UP: &str = r#"{
        "id": "t-two-up",
        "slots": [
            {"rect":[0.05,0.10,0.30,0.60],"role":"hero","bleed":[],"aspect_pref":[2.2,2.4]},
            {"rect":[0.60,0.20,0.20,0.40],"role":"support","bleed":[],"aspect_pref":[2.4,2.6]}
        ],
        "text_zones": [],
        "min_photos": 2, "max_photos": 2,
        "density": "medium", "energy": "calm"
    }"#;

    #[test]
    fn templates_loads_and_decomposes_into_two_page_layouts() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();

        assert_eq!(lib.spreads.len(), 1);
        let t = &lib.spreads[0];
        assert_eq!(t.id, "t-two-up");
        assert_eq!(t.left.slots.len(), 1);
        assert_eq!(t.right.slots.len(), 1);
        assert_eq!(t.left.side, Side::Left);
        assert_eq!(t.right.side, Side::Right);
        assert_eq!(t.photo_count(), 2);
    }

    #[test]
    fn templates_converts_slot_rects_to_page_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();
        let t = &lib.spreads[0];

        // 0.05 on the spread -> 0.10 on the left page; width 0.30 -> 0.60.
        assert!((t.left.slots[0].rect.x - 0.10).abs() < 1e-9);
        assert!((t.left.slots[0].rect.w - 0.60).abs() < 1e-9);
        // 0.60 on the spread -> (0.60-0.5)/0.5 = 0.20 on the right page.
        assert!((t.right.slots[0].rect.x - 0.20).abs() < 1e-9);
        assert!((t.right.slots[0].rect.w - 0.40).abs() < 1e-9);
    }

    #[test]
    fn templates_rejects_a_fold_spanning_slot() {
        let dir = tempfile::tempdir().unwrap();
        write_template(
            dir.path(),
            "bad.json",
            r#"{"id":"bad","slots":[
                {"rect":[0.2,0.1,0.6,0.8],"role":"hero","bleed":[],"aspect_pref":[3.7,3.9]}],
                "text_zones":[],"min_photos":1,"max_photos":1,
                "density":"sparse","energy":"calm"}"#,
        );
        match Library::load(dir.path()) {
            Err(TemplateError::SpansFold { template, slot }) => {
                assert_eq!(template, "bad");
                assert_eq!(slot, 0);
            }
            other => panic!("expected SpansFold, got {other:?}"),
        }
    }

    #[test]
    fn templates_selects_spreads_by_exact_photo_count() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();
        assert_eq!(lib.spreads_with(2).len(), 1);
        assert_eq!(lib.spreads_with(3).len(), 0);
        // The gap that matters: no 5-photo template exists yet (spec 6.3).
        assert_eq!(lib.spreads_with(5).len(), 0);
    }

    #[test]
    fn templates_page_half_pool_yields_both_halves() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        let lib = Library::load(dir.path()).unwrap();
        let pool = lib.page_half_pool();
        assert_eq!(pool.len(), 2);
        assert!(pool.iter().any(|p| p.side == Side::Left));
        assert!(pool.iter().any(|p| p.side == Side::Right));
    }

    /// Edge treatment is DERIVED, not authored -- it is the third pacing
    /// axis and must not require touching 19 existing files.
    #[test]
    fn templates_derives_edge_treatment_from_the_bleed_arrays() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "t-two-up.json", TWO_UP);
        write_template(
            dir.path(),
            "t-bleed.json",
            r#"{"id":"t-bleed","slots":[
                {"rect":[0.0,0.0,0.5,1.0],"role":"hero","bleed":["left","top","bottom"],
                 "aspect_pref":[1.2,1.3]},
                {"rect":[0.6,0.2,0.2,0.4],"role":"support","bleed":[],"aspect_pref":[2.4,2.6]}],
                "text_zones":[],"min_photos":2,"max_photos":2,
                "density":"sparse","energy":"lively"}"#,
        );
        let lib = Library::load(dir.path()).unwrap();
        let bleedy = lib.spreads.iter().find(|t| t.id == "t-bleed").unwrap();
        let plain = lib.spreads.iter().find(|t| t.id == "t-two-up").unwrap();
        assert_eq!(bleedy.left.edge_treatment, EdgeTreatment::Bleed);
        assert_eq!(bleedy.right.edge_treatment, EdgeTreatment::Margin);
        assert_eq!(plain.left.edge_treatment, EdgeTreatment::Margin);
    }

    #[test]
    fn templates_weights_fall_back_to_defaults_when_the_file_is_absent() {
        let w = Weights::load(std::path::Path::new("/nonexistent/weights.json"));
        assert!(w.is_err());
        let d = Weights::default();
        assert!(d.aspect_fit > 0.0);
        assert!(d.face_area_retention > d.saliency_retention,
            "faces must outweigh generic saliency");
    }
}
```

- [ ] **Step 2: Add the test dependency and run**

In `src-tauri/Cargo.toml` under `[dev-dependencies]`:

```toml
tempfile = "3"
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml templates
```
Expected: FAIL — `cannot find type Library in this scope`.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/templates.rs`:

```rust
//! Loading the authored spread templates and decomposing them into the
//! page layouts the engine actually reasons about.
//!
//! The spread JSON stays the authoring format because cross-fold ALIGNMENT
//! (grid rows, footer bands) is real design intent that survives only if the
//! two pages are authored together. The engine's atom is the page because
//! Pixajoy's boxes are page-local. Decomposition at load reconciles the two.

use crate::geometry::{spread_to_page, BleedEdge, Rect, Side};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Hero,
    Support,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Sparse,
    Medium,
    Dense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Energy {
    Calm,
    Neutral,
    Lively,
}

/// The third pacing axis, derived rather than authored: a page whose slots
/// declare any bleed reads as edge-to-edge, one whose slots do not reads as
/// matted. The user wants both present and alternating, and deriving it
/// avoids editing 19 existing files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeTreatment {
    Bleed,
    Margin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub rect: Rect,
    pub role: Role,
    pub bleed: Vec<BleedEdge>,
    pub aspect_pref: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    pub side: Side,
    pub slots: Vec<Slot>,
    pub edge_treatment: EdgeTreatment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpreadTemplate {
    pub id: String,
    pub left: PageLayout,
    pub right: PageLayout,
    pub density: Density,
    pub energy: Energy,
}

impl SpreadTemplate {
    pub fn photo_count(&self) -> usize {
        self.left.slots.len() + self.right.slots.len()
    }
}

#[derive(Debug)]
pub enum TemplateError {
    Io(String),
    Parse(String),
    SpansFold { template: String, slot: usize },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::Io(m) => write!(f, "template io error: {m}"),
            TemplateError::Parse(m) => write!(f, "template parse error: {m}"),
            TemplateError::SpansFold { template, slot } => write!(
                f,
                "template '{template}' slot {slot} spans the fold; Pixajoy picture \
                 boxes are page-local and cannot cross it"
            ),
        }
    }
}

// --- the on-disk shape, deliberately separate from the in-memory shape so
// the JSON can stay spread-normalised while the engine sees pages.

#[derive(Deserialize)]
struct RawSlot {
    rect: [f64; 4],
    role: Role,
    bleed: Vec<BleedEdge>,
    aspect_pref: [f64; 2],
}

#[derive(Deserialize)]
struct RawTemplate {
    id: String,
    slots: Vec<RawSlot>,
    min_photos: usize,
    max_photos: usize,
    density: Density,
    energy: Energy,
}

pub struct Library {
    pub spreads: Vec<SpreadTemplate>,
}

impl Library {
    pub fn load(dir: &Path) -> Result<Library, TemplateError> {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| TemplateError::Io(e.to_string()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("json")
                    && p.file_name().and_then(|s| s.to_str()) != Some("weights.json")
            })
            .collect();
        // Sorted so the library order -- and therefore every tie-break that
        // falls through to it -- is stable across machines and filesystems.
        files.sort();

        let mut spreads = Vec::with_capacity(files.len());
        for path in files {
            let text =
                std::fs::read_to_string(&path).map_err(|e| TemplateError::Io(e.to_string()))?;
            let raw: RawTemplate =
                serde_json::from_str(&text).map_err(|e| TemplateError::Parse(e.to_string()))?;
            spreads.push(decompose(raw)?);
        }
        Ok(Library { spreads })
    }

    /// Templates are exact-count, so eligibility is equality, not a range.
    pub fn spreads_with(&self, n: usize) -> Vec<&SpreadTemplate> {
        self.spreads.iter().filter(|t| t.photo_count() == n).collect()
    }

    /// Every half of every template. Pages 1 and N draw from this: a half is
    /// by construction a valid page layout, so the single pages get a
    /// library for free and one that matches the book's style.
    pub fn page_half_pool(&self) -> Vec<&PageLayout> {
        self.spreads.iter().flat_map(|t| [&t.left, &t.right]).collect()
    }
}

fn edge_treatment(slots: &[Slot]) -> EdgeTreatment {
    if slots.iter().any(|s| !s.bleed.is_empty()) {
        EdgeTreatment::Bleed
    } else {
        EdgeTreatment::Margin
    }
}

fn decompose(raw: RawTemplate) -> Result<SpreadTemplate, TemplateError> {
    debug_assert_eq!(
        raw.min_photos, raw.max_photos,
        "templates are exact-count by contract"
    );
    let mut left = Vec::new();
    let mut right = Vec::new();

    for (i, rs) in raw.slots.iter().enumerate() {
        let spread_rect = Rect::new(rs.rect[0], rs.rect[1], rs.rect[2], rs.rect[3]);
        let (side, page_rect) = spread_to_page(&spread_rect).ok_or(TemplateError::SpansFold {
            template: raw.id.clone(),
            slot: i,
        })?;
        let slot = Slot {
            rect: page_rect,
            role: rs.role,
            bleed: rs.bleed.clone(),
            aspect_pref: (rs.aspect_pref[0], rs.aspect_pref[1]),
        };
        match side {
            Side::Left => left.push(slot),
            Side::Right => right.push(slot),
        }
    }

    Ok(SpreadTemplate {
        id: raw.id,
        left: PageLayout { side: Side::Left, edge_treatment: edge_treatment(&left), slots: left },
        right: PageLayout {
            side: Side::Right,
            edge_treatment: edge_treatment(&right),
            slots: right,
        },
        density: raw.density,
        energy: raw.energy,
    })
}

/// Soft-term weights, hot-reloadable from `templates/weights.json` so taste
/// is tunable without a rebuild. Hard constraints are NOT weighted and are
/// deliberately absent here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    pub aspect_fit: f64,
    pub saliency_retention: f64,
    pub face_area_retention: f64,
    pub hero_match: f64,
    pub resolution_headroom: f64,
    pub palette_harmony: f64,
    pub variety: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            aspect_fit: 1.0,
            saliency_retention: 0.8,
            face_area_retention: 1.2,
            hero_match: 0.6,
            resolution_headroom: 0.4,
            palette_harmony: 0.2,
            variety: 0.5,
        }
    }
}

impl Weights {
    pub fn load(path: &Path) -> Result<Weights, TemplateError> {
        let text = std::fs::read_to_string(path).map_err(|e| TemplateError::Io(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| TemplateError::Parse(e.to_string()))
    }
}
```

Create `templates/weights.json` with the same values as `Default`:

```json
{
  "aspect_fit": 1.0,
  "saliency_retention": 0.8,
  "face_area_retention": 1.2,
  "hero_match": 0.6,
  "resolution_headroom": 0.4,
  "palette_harmony": 0.2,
  "variety": 0.5
}
```

Add `pub mod templates;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Ship the templates in the bundle**

`templates/` is currently not bundled, so a packaged `.app` has no template library at all — the engine would find an empty directory and produce a zero-page book. In `src-tauri/tauri.conf.json`, add `resources` to the `bundle` object:

```json
  "bundle": {
    "active": true,
    "targets": "app",
    "macOS": { "minimumSystemVersion": "15.0" },
    "externalBin": ["binaries/photobook-engine"],
    "resources": { "../templates": "templates" }
  }
```

- [ ] **Step 5: Run the tests**

```bash
bun run sidecar && cargo test --manifest-path src-tauri/Cargo.toml templates
```
Expected: PASS, 7 tests.

- [ ] **Step 6: Verify against the real library**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored real_library
```

Add this test to `mod tests` first:

```rust
    /// Guards the real authored library, not a fixture: every file in
    /// `templates/` must decompose. Ignored by default so the unit tests
    /// stay hermetic, but run in CI and after any authoring change.
    #[test]
    #[ignore = "reads the real templates/ directory"]
    fn templates_real_library_decomposes_cleanly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        let lib = Library::load(&dir).expect("the real library must decompose");
        assert!(lib.spreads.len() >= 19, "got {}", lib.spreads.len());
        assert!(
            lib.page_half_pool().len() >= 38,
            "every spread contributes two halves"
        );
    }
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/templates.rs src-tauri/src/lib.rs src-tauri/Cargo.toml \
        src-tauri/tauri.conf.json templates/weights.json
git commit -m "feat(templates): load spread JSON and decompose into page layouts"
```

---

## Task 4: Cull — one authority on which photo survives

**Files:**
- Create: `src-tauri/src/book/mod.rs`, `src-tauri/src/book/cull.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct Photo { pub path: String, pub hash: String, pub width: u32, pub height: u32, pub is_utility: bool, pub aesthetic_pct: u8, pub sharpness_pct: u8, pub near_dup_cluster: u32, pub event_cluster: u32, pub faces: Vec<Face>, pub face_area_fraction: f64, pub saliency_box: Option<Rect>, pub palette: Vec<PaletteColor>, pub capture_quality: Option<f64> }`
  - `pub struct Face { pub box_: Rect, pub capture_quality: Option<f64> }`
  - `pub struct PaletteColor { pub r: f64, pub g: f64, pub b: f64, pub weight: f64 }`
  - `pub fn from_features(v: &serde_json::Value) -> Option<Photo>`
  - `pub fn cull(photos: &[Photo]) -> Vec<Photo>`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/book/cull.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn photo(path: &str, cluster: u32, sharp: u8, aesth: u8) -> Photo {
        Photo {
            path: path.into(),
            hash: format!("h-{path}"),
            width: 4032,
            height: 3024,
            is_utility: false,
            aesthetic_pct: aesth,
            sharpness_pct: sharp,
            near_dup_cluster: cluster,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::new(),
            capture_quality: None,
        }
    }

    #[test]
    fn cull_drops_utility_images() {
        let mut u = photo("/a.jpg", 0, 90, 90);
        u.is_utility = true;
        let kept = cull(&[u, photo("/b.jpg", 1, 10, 10)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg");
    }

    #[test]
    fn cull_keeps_one_photo_per_near_duplicate_cluster() {
        let kept = cull(&[
            photo("/a.jpg", 7, 50, 90),
            photo("/b.jpg", 7, 80, 10),
            photo("/c.jpg", 8, 20, 20),
        ]);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|p| p.path == "/b.jpg"), "sharpness wins first");
        assert!(!kept.iter().any(|p| p.path == "/a.jpg"));
    }

    /// The documented ranking is sharpness -> capture quality -> aesthetic.
    /// A fixture where sharpness already decides it cannot test the second
    /// and third keys at all, so these are tied deliberately.
    #[test]
    fn cull_breaks_a_sharpness_tie_on_face_capture_quality() {
        let mut a = photo("/a.jpg", 7, 50, 90);
        a.capture_quality = Some(0.2);
        let mut b = photo("/b.jpg", 7, 50, 10);
        b.capture_quality = Some(0.9);
        let kept = cull(&[a, b]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg", "capture quality outranks aesthetic");
    }

    #[test]
    fn cull_breaks_a_sharpness_and_quality_tie_on_aesthetic() {
        let kept = cull(&[photo("/a.jpg", 7, 50, 30), photo("/b.jpg", 7, 50, 80)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/b.jpg");
    }

    /// smile_fraction is NOT in the ranking (spec 3.1): it is miscalibrated
    /// with a proven 100% false-negative rate. This test exists so that
    /// re-adding it fails loudly rather than silently reordering books.
    #[test]
    fn cull_ignores_smile_fraction_entirely() {
        // Photo has no smile field at all; if the ranking ever reads one,
        // deserialisation or ordering would have to change to accommodate it.
        let kept = cull(&[photo("/a.jpg", 7, 90, 10), photo("/b.jpg", 7, 10, 90)]);
        assert_eq!(kept[0].path, "/a.jpg");
    }

    /// Input is deliberately UNSORTED -- a sorted fixture cannot detect an
    /// ordering bug (a real Phase 1 test failure mode).
    #[test]
    fn cull_output_order_is_stable_regardless_of_input_order() {
        let forward = cull(&[photo("/c.jpg", 3, 10, 10), photo("/a.jpg", 1, 10, 10),
                             photo("/b.jpg", 2, 10, 10)]);
        let reverse = cull(&[photo("/b.jpg", 2, 10, 10), photo("/a.jpg", 1, 10, 10),
                             photo("/c.jpg", 3, 10, 10)]);
        let f: Vec<_> = forward.iter().map(|p| p.path.clone()).collect();
        let r: Vec<_> = reverse.iter().map(|p| p.path.clone()).collect();
        assert_eq!(f, r);
        assert_eq!(f, vec!["/a.jpg", "/b.jpg", "/c.jpg"]);
    }

    // --- from_features: the boundary between Swift's wire JSON and the engine

    #[test]
    fn cull_from_features_reads_boxes_and_palette() {
        let v = serde_json::json!({
            "path": "/p/a.jpg", "hash": "abc", "width": 4032, "height": 3024,
            "isUtility": false, "aestheticPct": 80, "sharpnessPct": 60,
            "nearDupCluster": 2, "eventCluster": 1,
            "faces": [{"box":[0.1,0.2,0.3,0.4],"captureQuality":0.7}],
            "faceAreaFraction": 0.12,
            "saliencyBox": [0.2,0.1,0.5,0.6],
            "palette": [{"r":0.5,"g":0.2,"b":0.1,"weight":0.6}]
        });
        let p = from_features(&v).expect("well-formed record");
        assert_eq!(p.faces.len(), 1);
        assert!((p.faces[0].box_.w - 0.3).abs() < 1e-9);
        assert!((p.saliency_box.unwrap().h - 0.6).abs() < 1e-9);
        assert_eq!(p.palette.len(), 1);
        assert_eq!(p.capture_quality, Some(0.7));
    }

    /// Vision returns no saliency on some images; the engine must degrade,
    /// not panic.
    #[test]
    fn cull_from_features_tolerates_a_missing_saliency_box() {
        let v = serde_json::json!({
            "path": "/p/a.jpg", "hash": "abc", "width": 4032, "height": 3024,
            "isUtility": false, "aestheticPct": 80, "sharpnessPct": 60,
            "nearDupCluster": 2, "eventCluster": 1,
            "faces": [], "faceAreaFraction": 0.0, "palette": []
        });
        let p = from_features(&v).expect("well-formed record");
        assert!(p.saliency_box.is_none());
        assert_eq!(p.capture_quality, None);
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cull
```
Expected: FAIL — `cannot find type Photo in this scope`.

- [ ] **Step 3: Implement**

Create `src-tauri/src/book/mod.rs`:

```rust
//! The layout engine: cull -> pack -> crop -> score -> pace -> assemble.
//!
//! Every stage is a pure function over plain data, testable without an
//! `AppHandle` or a live sidecar -- the pattern this codebase already
//! follows in `finalize_photos`, `analyze_batches`, `lookup_cache`,
//! `percentiles` and `imageNormalizedTopLeft`.

pub mod cull;
```

Prepend to `src-tauri/src/book/cull.rs`:

```rust
use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// Image-normalised, top-left origin -- matching Swift's
    /// `VisionAnalyzer.topLeft`.
    pub box_: Rect,
    pub capture_quality: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaletteColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub weight: f64,
}

/// The engine's view of one analysed photo. Built from the FULL Swift
/// feature record, not from the narrowed `AnalyzedPhoto` the webview sees:
/// the scorer needs face boxes, the saliency box and the palette, none of
/// which `partial_photo` forwards.
///
/// `width`/`height` are post-orientation. `ExifReader` swaps them for EXIF
/// orientations 5-8 before they ever reach this struct, so layout boxes
/// computed here are already correct for portrait photos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub path: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub is_utility: bool,
    pub aesthetic_pct: u8,
    pub sharpness_pct: u8,
    pub near_dup_cluster: u32,
    pub event_cluster: u32,
    pub faces: Vec<Face>,
    pub face_area_fraction: f64,
    pub saliency_box: Option<Rect>,
    pub palette: Vec<PaletteColor>,
    /// Best face capture quality on the photo, or `None` when there are no
    /// faces or Vision returned none. Precomputed because culling reads it
    /// once per comparison.
    pub capture_quality: Option<f64>,
}

impl Photo {
    /// Real-world aspect ratio of the photo itself, width over height.
    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height as f64
    }
}

fn rect_from(v: &serde_json::Value) -> Option<Rect> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    Some(Rect::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?, a[3].as_f64()?))
}

/// Builds a `Photo` from one finalized feature record. Returns `None` when a
/// required field is missing or the wrong type, which the caller reports as
/// a skipped photo rather than failing the whole book.
pub fn from_features(v: &serde_json::Value) -> Option<Photo> {
    let faces: Vec<Face> = v["faces"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    Some(Face {
                        box_: rect_from(&f["box"])?,
                        capture_quality: f["captureQuality"].as_f64(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let capture_quality = faces
        .iter()
        .filter_map(|f| f.capture_quality)
        .max_by(|a, b| a.total_cmp(b));

    let palette = v["palette"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(PaletteColor {
                        r: c["r"].as_f64()?,
                        g: c["g"].as_f64()?,
                        b: c["b"].as_f64()?,
                        weight: c["weight"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(Photo {
        path: v["path"].as_str()?.to_string(),
        hash: v["hash"].as_str()?.to_string(),
        width: v["width"].as_u64()? as u32,
        height: v["height"].as_u64()? as u32,
        is_utility: v["isUtility"].as_bool().unwrap_or(false),
        aesthetic_pct: v["aestheticPct"].as_u64().unwrap_or(0) as u8,
        sharpness_pct: v["sharpnessPct"].as_u64().unwrap_or(0) as u8,
        near_dup_cluster: v["nearDupCluster"].as_u64().unwrap_or(0) as u32,
        event_cluster: v["eventCluster"].as_u64().unwrap_or(0) as u32,
        faces,
        face_area_fraction: v["faceAreaFraction"].as_f64().unwrap_or(0.0),
        saliency_box: rect_from(&v["saliencyBox"]),
        palette,
        capture_quality,
    })
}

/// The single authority on which photo survives.
///
/// Drops `is_utility`, then keeps one winner per near-duplicate cluster,
/// ranked sharpness percentile -> face capture quality -> aesthetic
/// percentile.
///
/// `smile_fraction` is deliberately NOT in the ranking, departing from
/// section 7.4 of the original design: it is miscalibrated with a proven
/// 100% false-negative rate (see `PROJECT-STATUS.md`), so including it adds
/// noise rather than signal.
///
/// This function replaces BOTH the old Rust `count_keepers` and the
/// TypeScript `keepers()`. Two implementations of one rule in two languages
/// disagreed silently; Phase 2 makes that disagreement visible in printed
/// output, so there is now exactly one.
pub fn cull(photos: &[Photo]) -> Vec<Photo> {
    use std::collections::BTreeMap;

    let mut best: BTreeMap<u32, &Photo> = BTreeMap::new();
    for photo in photos.iter().filter(|p| !p.is_utility) {
        best.entry(photo.near_dup_cluster)
            .and_modify(|incumbent| {
                if beats(photo, incumbent) {
                    *incumbent = photo;
                }
            })
            .or_insert(photo);
    }

    // `BTreeMap` iterates by cluster id, so the output order depends only on
    // cluster ids, never on input order -- the property the ordering test
    // pins with deliberately unsorted input.
    let mut kept: Vec<Photo> = best.into_values().cloned().collect();
    kept.sort_by(|a, b| a.path.cmp(&b.path));
    kept
}

fn beats(challenger: &Photo, incumbent: &Photo) -> bool {
    use std::cmp::Ordering;
    match challenger.sharpness_pct.cmp(&incumbent.sharpness_pct) {
        Ordering::Greater => return true,
        Ordering::Less => return false,
        Ordering::Equal => {}
    }
    let cq = |p: &Photo| p.capture_quality.unwrap_or(-1.0);
    match cq(challenger).total_cmp(&cq(incumbent)) {
        Ordering::Greater => return true,
        Ordering::Less => return false,
        Ordering::Equal => {}
    }
    challenger.aesthetic_pct > incumbent.aesthetic_pct
}
```

Add `pub mod book;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cull
```
Expected: PASS, 8 tests.

- [ ] **Step 5: Retire the duplicate culling rule**

Find `count_keepers` in `src-tauri/src/commands.rs` and replace its body with a delegation, so the notification count and the book cannot disagree:

```bash
grep -n "fn count_keepers" -A 30 src-tauri/src/commands.rs
```

Replace the function with:

```rust
/// Counts surviving photos for the completion notification.
///
/// Delegates to `book::cull::cull` rather than reimplementing the rule.
/// Before Phase 2 there were two implementations of "which photo survives"
/// -- this one and TypeScript's `keepers()` -- and nothing kept them in
/// agreement. The notification is now guaranteed to report the same number
/// the book is built from.
pub(crate) fn count_keepers(photos: &[serde_json::Value]) -> usize {
    let parsed: Vec<crate::book::cull::Photo> =
        photos.iter().filter_map(crate::book::cull::from_features).collect();
    crate::book::cull::cull(&parsed).len()
}
```

- [ ] **Step 6: Run the whole Rust suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```
Expected: PASS. If an existing `count_keepers` test now fails, read it before changing it — a genuine behaviour difference between the old and new rules is exactly what this task exists to surface. Record the difference in the commit body.

- [ ] **Step 7: Mutation-check the ranking order**

```bash
# Swap capture quality and aesthetic in `beats` so aesthetic is checked first.
cargo test --manifest-path src-tauri/Cargo.toml cull
# Expect FAIL on cull_breaks_a_sharpness_tie_on_face_capture_quality. Revert.
```

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/book/ src-tauri/src/lib.rs src-tauri/src/commands.rs
git commit -m "feat(book): single culling authority, replacing the Rust/TS duplicate"
```

---

## Task 5: Deterministic crop selection

**Files:**
- Create: `src-tauri/src/book/crop.rs`
- Modify: `src-tauri/src/book/mod.rs`

**Interfaces:**
- Consumes: `book::cull::{Photo, Face}`, `geometry::Rect`.
- Produces: `pub fn choose_crop(photo: &Photo, target_aspect: f64) -> Rect` — returns a normalised rect in the photo's own coordinate space.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/book/crop.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::{Face, PaletteColor};

    /// NEVER square: a square photo makes width/height == 1 and hides the
    /// entire class of aspect bug (a real Phase 1 failure).
    fn landscape_photo() -> Photo {
        Photo {
            path: "/p.jpg".into(),
            hash: "h".into(),
            width: 4000,
            height: 3000, // 4:3
            is_utility: false,
            aesthetic_pct: 50,
            sharpness_pct: 50,
            near_dup_cluster: 0,
            event_cluster: 0,
            faces: Vec::new(),
            face_area_fraction: 0.0,
            saliency_box: None,
            palette: Vec::<PaletteColor>::new(),
            capture_quality: None,
        }
    }

    #[test]
    fn crop_to_a_wider_target_keeps_full_width_and_trims_height() {
        let p = landscape_photo(); // 1.333
        let c = choose_crop(&p, 2.0);
        assert!((c.w - 1.0).abs() < 1e-9, "full width kept, was {}", c.w);
        // 4000 wide at 2:1 needs 2000 tall of 3000 -> 0.6667 normalised.
        assert!((c.h - 2.0 / 3.0).abs() < 1e-6, "h was {}", c.h);
    }

    #[test]
    fn crop_to_a_narrower_target_keeps_full_height_and_trims_width() {
        let p = landscape_photo();
        let c = choose_crop(&p, 1.0);
        assert!((c.h - 1.0).abs() < 1e-9, "full height kept, was {}", c.h);
        // 3000 tall at 1:1 needs 3000 wide of 4000 -> 0.75 normalised.
        assert!((c.w - 0.75).abs() < 1e-6, "w was {}", c.w);
    }

    #[test]
    fn crop_centres_when_there_is_no_saliency_or_face() {
        let p = landscape_photo();
        let c = choose_crop(&p, 1.0);
        assert!((c.x - 0.125).abs() < 1e-6, "x was {}", c.x);
    }

    #[test]
    fn crop_follows_the_saliency_box_off_centre() {
        let mut p = landscape_photo();
        // Salient content hard against the LEFT of the frame.
        p.saliency_box = Some(Rect::new(0.02, 0.3, 0.20, 0.4));
        let c = choose_crop(&p, 1.0);
        assert!(c.x < 0.125, "crop should move left, x was {}", c.x);
        assert!(c.x >= 0.0, "crop must stay inside the photo, x was {}", c.x);
    }

    #[test]
    fn crop_prefers_a_face_over_generic_saliency() {
        let mut p = landscape_photo();
        p.saliency_box = Some(Rect::new(0.02, 0.3, 0.10, 0.4)); // left
        p.faces = vec![Face { box_: Rect::new(0.80, 0.3, 0.15, 0.3), capture_quality: Some(0.9) }];
        let c = choose_crop(&p, 1.0);
        assert!(c.x > 0.125, "the face should pull the crop right, x was {}", c.x);
    }

    #[test]
    fn crop_never_leaves_the_photo_bounds() {
        let mut p = landscape_photo();
        p.faces = vec![Face { box_: Rect::new(0.97, 0.9, 0.03, 0.1), capture_quality: Some(0.5) }];
        let c = choose_crop(&p, 1.0);
        assert!(c.x >= -1e-9 && c.right() <= 1.0 + 1e-9, "x={} right={}", c.x, c.right());
        assert!(c.y >= -1e-9 && c.bottom() <= 1.0 + 1e-9);
    }

    #[test]
    fn crop_is_deterministic_across_repeated_calls() {
        let mut p = landscape_photo();
        p.saliency_box = Some(Rect::new(0.3, 0.2, 0.4, 0.5));
        p.faces = vec![Face { box_: Rect::new(0.5, 0.4, 0.1, 0.2), capture_quality: Some(0.6) }];
        let a = choose_crop(&p, 1.6);
        let b = choose_crop(&p, 1.6);
        assert_eq!(a, b);
    }

    #[test]
    fn crop_matches_the_target_aspect_in_real_pixels() {
        let p = landscape_photo();
        for target in [0.7, 1.0, 1.6, 2.4] {
            let c = choose_crop(&p, target);
            let real = (c.w * p.width as f64) / (c.h * p.height as f64);
            assert!((real - target).abs() < 1e-6, "target {target} produced {real}");
        }
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml crop
```
Expected: FAIL — `cannot find function choose_crop`.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/book/crop.rs`:

```rust
//! Deterministic crop-window selection.
//!
//! No search and no seed: given a photo and a target aspect there is exactly
//! one answer, which is what keeps golden files stable and makes
//! "regenerate" feel like browsing alternatives rather than rolling dice.

use crate::book::cull::Photo;
use crate::geometry::Rect;

/// Weight of a face relative to the generic saliency box when deciding where
/// to centre the crop. Faces are what a viewer looks at first; Vision's
/// attention saliency is a weaker signal and routinely lands on scenery.
const FACE_WEIGHT: f64 = 3.0;
const SALIENCY_WEIGHT: f64 = 1.0;

/// Returns the crop window in the photo's own normalised coordinates.
///
/// The window is the LARGEST rect of `target_aspect` that fits inside the
/// photo, positioned so its centre is as close as possible to the weighted
/// centroid of faces and salient content, clamped to stay in bounds. It
/// never upscales and never distorts.
pub fn choose_crop(photo: &Photo, target_aspect: f64) -> Rect {
    let photo_aspect = photo.aspect();

    // Largest rect of the target aspect that fits. Working in normalised
    // space, a target WIDER than the photo keeps full width; a NARROWER
    // target keeps full height.
    let (w, h) = if target_aspect >= photo_aspect {
        (1.0, photo_aspect / target_aspect)
    } else {
        (target_aspect / photo_aspect, 1.0)
    };

    let (cx, cy) = focus_centroid(photo);

    // Centre the window on the focus, then clamp. `max(0.0)` guards the
    // degenerate case where the window is the full extent on an axis.
    let x = (cx - w / 2.0).clamp(0.0, (1.0 - w).max(0.0));
    let y = (cy - h / 2.0).clamp(0.0, (1.0 - h).max(0.0));

    Rect::new(x, y, w, h)
}

/// Weighted centroid of everything worth keeping. Falls back to the frame
/// centre when the photo has neither faces nor a saliency box, which is the
/// correct neutral answer rather than a bias toward any corner.
fn focus_centroid(photo: &Photo) -> (f64, f64) {
    let mut total = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;

    for face in &photo.faces {
        // Area-weighted so a large foreground face outranks a small one in
        // the background, and quality-weighted so a blurred face pulls less.
        let quality = face.capture_quality.unwrap_or(0.5).clamp(0.0, 1.0);
        let weight = FACE_WEIGHT * face.box_.area().max(1e-6) * (0.5 + quality);
        total += weight;
        sx += weight * (face.box_.x + face.box_.w / 2.0);
        sy += weight * (face.box_.y + face.box_.h / 2.0);
    }

    if let Some(s) = photo.saliency_box {
        let weight = SALIENCY_WEIGHT * s.area().max(1e-6);
        total += weight;
        sx += weight * (s.x + s.w / 2.0);
        sy += weight * (s.y + s.h / 2.0);
    }

    if total <= 0.0 {
        return (0.5, 0.5);
    }
    (sx / total, sy / total)
}
```

Add `pub mod crop;` to `src-tauri/src/book/mod.rs`.

- [ ] **Step 4: Run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml crop
```
Expected: PASS, 8 tests.

- [ ] **Step 5: Mutation-check the face preference and the aspect maths**

```bash
# 1. Set FACE_WEIGHT to 1.0 (equal to saliency).
cargo test --manifest-path src-tauri/Cargo.toml crop
# Expect FAIL on crop_prefers_a_face_over_generic_saliency. Revert.

# 2. Swap the branches: `if target_aspect < photo_aspect`.
cargo test --manifest-path src-tauri/Cargo.toml crop
# Expect FAIL on crop_matches_the_target_aspect_in_real_pixels. Revert.
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/book/crop.rs src-tauri/src/book/mod.rs
git commit -m "feat(book): deterministic saliency- and face-aware crop selection"
```

---

## Task 6: Scoring

**Files:**
- Create: `src-tauri/src/book/score.rs`
- Modify: `src-tauri/src/book/mod.rs`

**Interfaces:**
- Consumes: `templates::{SpreadTemplate, PageLayout, Slot, Role, Weights}`, `book::cull::Photo`, `book::crop::choose_crop`, `geometry::*`.
- Produces:
  - `pub struct Candidate { pub template_id: String, pub assignment: Vec<usize>, pub score: f64 }`
  - `pub const MIN_DPI: f64 = 150.0`
  - `pub fn slot_aspect(slot: &Slot) -> f64`
  - `pub fn effective_dpi(photo: &Photo, crop: &Rect, slot: &Slot) -> f64`
  - `pub fn rejects(photo: &Photo, crop: &Rect, slot: &Slot, side: Side) -> Option<Rejection>`
  - `pub enum Rejection { FaceClipped, FaceInGutter, TooLowResolution }`
  - `pub fn score_spread(t: &SpreadTemplate, photos: &[&Photo], assignment: &[usize], previous: Option<&str>, w: &Weights) -> Option<f64>`
  - `pub fn best_spread<'a>(templates: &[&'a SpreadTemplate], photos: &[&Photo], previous: Option<&str>, w: &Weights) -> Option<(&'a SpreadTemplate, Vec<usize>, f64)>`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/book/score.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::{Face, PaletteColor};
    use crate::geometry::BleedEdge;

    fn photo(w: u32, h: u32) -> Photo {
        Photo {
            path: "/p.jpg".into(), hash: "h".into(), width: w, height: h,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: 0,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
        }
    }

    /// A slot whose normalised ratio and inch ratio are DIFFERENT numbers.
    /// A fixture where they coincide passes under the aspect_pref trap.
    fn wide_slot() -> Slot {
        Slot {
            // On an 11.197 x 8.894 page: 0.5 x 0.5 normalised is
            // 5.5985" x 4.447" = 1.259 real, not 1.0.
            rect: Rect::new(0.2, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        }
    }

    #[test]
    fn score_slot_aspect_uses_page_inches_not_the_normalised_ratio() {
        let s = wide_slot();
        let normalised = s.rect.w / s.rect.h;
        let real = slot_aspect(&s);
        assert!((normalised - 1.0).abs() < 1e-9, "sanity: normalised is 1:1");
        assert!((real - 1.259).abs() < 0.002, "real was {real}");
    }

    #[test]
    fn score_effective_dpi_accounts_for_the_crop() {
        let p = photo(4000, 3000);
        let s = wide_slot(); // 5.5985" wide
        let full = Rect::new(0.0, 0.0, 1.0, 1.0);
        let half = Rect::new(0.25, 0.0, 0.5, 1.0);
        let d_full = effective_dpi(&p, &full, &s);
        let d_half = effective_dpi(&p, &half, &s);
        assert!((d_full - 4000.0 / 5.5985).abs() < 1.0, "was {d_full}");
        assert!((d_half - d_full / 2.0).abs() < 1.0, "cropping halves the DPI");
    }

    #[test]
    fn score_rejects_a_photo_below_the_dpi_floor() {
        let p = photo(400, 300);
        let s = wide_slot();
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &s, Side::Left),
            Some(Rejection::TooLowResolution)
        ));
    }

    /// Boundary test AT the boundary: exactly MIN_DPI must pass.
    #[test]
    fn score_dpi_floor_is_inclusive_at_exactly_min_dpi() {
        let s = wide_slot();
        let slot_w_in = s.rect.w * PAGE_W_IN;
        let px = (MIN_DPI * slot_w_in).round() as u32;
        let p = photo(px, (px as f64 / 1.259) as u32);
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(rejects(&p, &crop, &s, Side::Left).is_none(), "exactly at the floor must pass");
    }

    #[test]
    fn score_rejects_a_crop_that_clips_a_face() {
        let mut p = photo(4000, 3000);
        // Face against the right edge; a crop that stops short of it clips it.
        p.faces = vec![Face { box_: Rect::new(0.85, 0.4, 0.12, 0.2), capture_quality: Some(0.8) }];
        let crop = Rect::new(0.0, 0.0, 0.7, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &wide_slot(), Side::Left),
            Some(Rejection::FaceClipped)
        ));
    }

    #[test]
    fn score_accepts_a_crop_that_fully_contains_the_face() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.30, 0.4, 0.12, 0.2), capture_quality: Some(0.8) }];
        let crop = Rect::new(0.0, 0.0, 0.7, 1.0);
        assert!(rejects(&p, &crop, &wide_slot(), Side::Left).is_none());
    }

    #[test]
    fn score_rejects_a_face_landing_in_the_gutter_strip() {
        let mut p = photo(4000, 3000);
        p.faces = vec![Face { box_: Rect::new(0.90, 0.4, 0.08, 0.2), capture_quality: Some(0.8) }];
        // A slot running flush to the fold on a LEFT page.
        let s = Slot {
            rect: Rect::new(0.5, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(matches!(
            rejects(&p, &crop, &s, Side::Left),
            Some(Rejection::FaceInGutter)
        ));
    }

    /// Generic salient content in the dead strip is only a PENALTY, never a
    /// rejection -- otherwise no slot could ever run to the fold, which is
    /// the layout the user explicitly asked to keep available.
    #[test]
    fn score_does_not_reject_generic_saliency_in_the_gutter_strip() {
        let mut p = photo(4000, 3000);
        p.saliency_box = Some(Rect::new(0.90, 0.4, 0.08, 0.2));
        let s = Slot {
            rect: Rect::new(0.5, 0.2, 0.5, 0.5),
            role: Role::Hero,
            bleed: Vec::<BleedEdge>::new(),
            aspect_pref: (1.2, 1.35),
        };
        let crop = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert!(rejects(&p, &crop, &s, Side::Left).is_none());
    }

    #[test]
    fn score_prefers_the_assignment_matching_slot_aspects() {
        let lib = fixture_library();
        let t = &lib[0]; // hero slot is wide, support slot is narrow
        let wide = photo(4000, 2000);
        let tall = photo(2000, 4000);
        let photos = vec![&wide, &tall];

        let matched = score_spread(t, &photos, &[0, 1], None, &Weights::default());
        let swapped = score_spread(t, &photos, &[1, 0], None, &Weights::default());
        assert!(matched.unwrap() > swapped.unwrap(), "aspect fit must drive the choice");
    }

    #[test]
    fn score_puts_the_highest_aesthetic_photo_in_the_hero_slot() {
        let lib = fixture_library();
        let t = &lib[0];
        let mut a = photo(3000, 2000);
        a.aesthetic_pct = 95;
        let mut b = photo(3000, 2000);
        b.aesthetic_pct = 10;
        let photos = vec![&a, &b];
        let hero_first = score_spread(t, &photos, &[0, 1], None, &Weights::default()).unwrap();
        let hero_last = score_spread(t, &photos, &[1, 0], None, &Weights::default()).unwrap();
        assert!(hero_first > hero_last);
    }

    #[test]
    fn score_penalises_reusing_the_previous_template() {
        let lib = fixture_library();
        let t = &lib[0];
        let p = photo(3000, 2000);
        let photos = vec![&p, &p];
        let fresh = score_spread(t, &photos, &[0, 1], None, &Weights::default()).unwrap();
        let repeat =
            score_spread(t, &photos, &[0, 1], Some(&t.id), &Weights::default()).unwrap();
        assert!(fresh > repeat, "variety must penalise an immediate repeat");
    }

    #[test]
    fn score_best_spread_returns_none_when_every_candidate_is_rejected() {
        let lib = fixture_library();
        let tiny = photo(60, 40);
        let refs: Vec<&SpreadTemplate> = lib.iter().collect();
        let photos = vec![&tiny, &tiny];
        assert!(best_spread(&refs, &photos, None, &Weights::default()).is_none());
    }

    /// Two photos, two slots, ASYMMETRIC on purpose -- a symmetric fixture
    /// cannot detect an assignment that is silently reversed.
    fn fixture_library() -> Vec<SpreadTemplate> {
        vec![SpreadTemplate {
            id: "fx-hero-support".into(),
            left: PageLayout {
                side: Side::Left,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.1, 0.2, 0.8, 0.35), // wide
                    role: Role::Hero,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (2.5, 3.0),
                }],
            },
            right: PageLayout {
                side: Side::Right,
                edge_treatment: EdgeTreatment::Margin,
                slots: vec![Slot {
                    rect: Rect::new(0.3, 0.1, 0.3, 0.75), // tall
                    role: Role::Support,
                    bleed: Vec::<BleedEdge>::new(),
                    aspect_pref: (0.4, 0.6),
                }],
            },
            density: Density::Medium,
            energy: Energy::Calm,
        }]
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml score
```
Expected: FAIL — `cannot find function slot_aspect`.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/book/score.rs`:

```rust
//! Scoring a template plus a photo-to-slot assignment.
//!
//! Hard constraints REJECT a candidate; soft terms weight it. The split is
//! deliberate: a face cut in half is not a slightly worse layout, it is a
//! ruined photo, and no weighting scheme should ever be able to outvote it.

use crate::book::crop::choose_crop;
use crate::book::cull::Photo;
use crate::geometry::{clear_of_gutter, Rect, Side, PAGE_H_IN, PAGE_W_IN};
use crate::templates::{
    Density, EdgeTreatment, Energy, PageLayout, Role, Slot, SpreadTemplate, Weights,
};

/// Below this, print is visibly soft and no downstream step can fix it.
/// The design's 300 DPI target is a WARNING; this is the hard floor.
pub const MIN_DPI: f64 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    FaceClipped,
    FaceInGutter,
    TooLowResolution,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub template_id: String,
    /// `assignment[i]` is the index into the photo slice for slot `i`, in
    /// left-page-then-right-page slot order.
    pub assignment: Vec<usize>,
    pub score: f64,
}

/// A slot's real-world aspect ratio on the PAGE canvas.
///
/// The trap: `rect` is normalised to the page (11.197" x 8.894" = 1.259:1),
/// so `w / h` is NOT the ratio `aspect_pref` is expressed in. Comparing
/// against the normalised ratio mis-scores every slot.
pub fn slot_aspect(slot: &Slot) -> f64 {
    slot.rect.aspect_in(PAGE_W_IN, PAGE_H_IN)
}

/// Pixels per inch the photo actually resolves at, once cropped, when placed
/// in this slot.
pub fn effective_dpi(photo: &Photo, crop: &Rect, slot: &Slot) -> f64 {
    let cropped_px = photo.width as f64 * crop.w;
    let slot_in = slot.rect.w * PAGE_W_IN;
    if slot_in <= 0.0 {
        return 0.0;
    }
    cropped_px / slot_in
}

/// Maps a face box from photo coordinates into the slot's page coordinates,
/// given the crop window. Returns `None` when the face falls outside the
/// crop entirely, which the caller treats as "clipped".
fn face_in_page(face: &Rect, crop: &Rect, slot: &Slot) -> Option<Rect> {
    if crop.w <= 0.0 || crop.h <= 0.0 {
        return None;
    }
    let u = (face.x - crop.x) / crop.w;
    let v = (face.y - crop.y) / crop.h;
    let uw = face.w / crop.w;
    let vh = face.h / crop.h;
    Some(Rect::new(
        slot.rect.x + u * slot.rect.w,
        slot.rect.y + v * slot.rect.h,
        uw * slot.rect.w,
        vh * slot.rect.h,
    ))
}

/// The three hard constraints. Returns the first violation, or `None` when
/// the placement is acceptable.
pub fn rejects(photo: &Photo, crop: &Rect, slot: &Slot, side: Side) -> Option<Rejection> {
    if effective_dpi(photo, crop, slot) < MIN_DPI {
        return Some(Rejection::TooLowResolution);
    }

    for face in &photo.faces {
        // Clipped: the crop window does not fully contain the face box.
        let contained = face.box_.x >= crop.x - 1e-9
            && face.box_.y >= crop.y - 1e-9
            && face.box_.right() <= crop.right() + 1e-9
            && face.box_.bottom() <= crop.bottom() + 1e-9;
        if !contained {
            // A face wholly outside the crop is not "clipped" -- it is
            // simply not in the picture, which is fine. Only a PARTIAL
            // overlap is a half-face.
            if face.box_.intersect(crop).is_some() {
                return Some(Rejection::FaceClipped);
            }
            continue;
        }

        if let Some(page_rect) = face_in_page(&face.box_, crop, slot) {
            if !clear_of_gutter(&page_rect, side) {
                return Some(Rejection::FaceInGutter);
            }
        }
    }

    None
}

/// How well a photo's aspect matches a slot, in [0,1]. 1.0 when the photo
/// needs no crop at all; falls off with the fraction of the frame discarded.
fn aspect_fit(photo: &Photo, slot: &Slot) -> f64 {
    let target = slot_aspect(slot);
    let actual = photo.aspect();
    let ratio = if target > actual { actual / target } else { target / actual };
    ratio.clamp(0.0, 1.0)
}

/// Fraction of the saliency box surviving the crop, in [0,1]. A photo with
/// no saliency box scores neutrally rather than zero -- absence of a signal
/// is not evidence of a bad crop.
fn saliency_retention(photo: &Photo, crop: &Rect) -> f64 {
    match photo.saliency_box {
        None => 0.5,
        Some(s) => {
            let area = s.area();
            if area <= 0.0 {
                return 0.5;
            }
            s.intersect(crop).map_or(0.0, |i| i.area()) / area
        }
    }
}

/// Fraction of total face area surviving the crop. Neutral when faceless.
fn face_area_retention(photo: &Photo, crop: &Rect) -> f64 {
    let total: f64 = photo.faces.iter().map(|f| f.box_.area()).sum();
    if total <= 0.0 {
        return 0.5;
    }
    let kept: f64 = photo
        .faces
        .iter()
        .map(|f| f.box_.intersect(crop).map_or(0.0, |i| i.area()))
        .sum();
    kept / total
}

/// Rewards the highest-aesthetic photo landing in a `hero` slot.
fn hero_match(photo: &Photo, slot: &Slot, best_aesthetic: u8) -> f64 {
    match slot.role {
        Role::Hero => {
            if best_aesthetic == 0 {
                0.5
            } else {
                photo.aesthetic_pct as f64 / best_aesthetic as f64
            }
        }
        Role::Support => 0.5,
    }
}

/// Headroom above the hard floor, saturating at the 300 DPI target.
fn resolution_headroom(photo: &Photo, crop: &Rect, slot: &Slot) -> f64 {
    let dpi = effective_dpi(photo, crop, slot);
    ((dpi - MIN_DPI) / (300.0 - MIN_DPI)).clamp(0.0, 1.0)
}

/// Rewards a spread whose photos share a coherent dominant hue. Uses the
/// Oklab-ish palette Phase 1 already computes; a spread whose photos scatter
/// across the hue circle reads as noisy.
///
/// Deliberately low-weighted by default: it is the fuzziest term in the
/// scorer and zeroing its weight must leave a usable book.
fn palette_harmony(photos: &[&Photo]) -> f64 {
    let hues: Vec<f64> = photos
        .iter()
        .filter_map(|p| p.palette.first())
        .map(|c| c.b.atan2(c.r))
        .collect();
    if hues.len() < 2 {
        return 0.5;
    }
    // Circular variance: 1.0 when all hues agree, 0.0 when uniformly spread.
    let (sx, sy) = hues.iter().fold((0.0, 0.0), |(x, y), h| (x + h.cos(), y + h.sin()));
    let n = hues.len() as f64;
    ((sx / n).hypot(sy / n)).clamp(0.0, 1.0)
}

/// Penalises repeating the immediately preceding template.
fn variety(template_id: &str, previous: Option<&str>) -> f64 {
    match previous {
        Some(prev) if prev == template_id => 0.0,
        _ => 1.0,
    }
}

fn ordered_slots(t: &SpreadTemplate) -> Vec<(&Slot, Side)> {
    t.left
        .slots
        .iter()
        .map(|s| (s, Side::Left))
        .chain(t.right.slots.iter().map(|s| (s, Side::Right)))
        .collect()
}

/// Scores one candidate. Returns `None` when any hard constraint rejects it.
pub fn score_spread(
    t: &SpreadTemplate,
    photos: &[&Photo],
    assignment: &[usize],
    previous: Option<&str>,
    w: &Weights,
) -> Option<f64> {
    let slots = ordered_slots(t);
    if slots.len() != assignment.len() || assignment.len() != photos.len() {
        return None;
    }

    let best_aesthetic = photos.iter().map(|p| p.aesthetic_pct).max().unwrap_or(0);
    let mut total = 0.0;

    for (i, (slot, side)) in slots.iter().enumerate() {
        let photo = photos[assignment[i]];
        let crop = choose_crop(photo, slot_aspect(slot));
        if rejects(photo, &crop, slot, *side).is_some() {
            return None;
        }
        total += w.aspect_fit * aspect_fit(photo, slot)
            + w.saliency_retention * saliency_retention(photo, &crop)
            + w.face_area_retention * face_area_retention(photo, &crop)
            + w.hero_match * hero_match(photo, slot, best_aesthetic)
            + w.resolution_headroom * resolution_headroom(photo, &crop, slot);
    }

    // Spread-global terms, added once rather than per slot.
    total += w.palette_harmony * palette_harmony(photos);
    total += w.variety * variety(&t.id, previous);

    Some(total)
}

/// Enumerates every eligible template and every assignment, returning the
/// best. Brute force: at most 6! = 720 assignments over box arithmetic with
/// no pixels touched.
pub fn best_spread<'a>(
    templates: &[&'a SpreadTemplate],
    photos: &[&Photo],
    previous: Option<&str>,
    w: &Weights,
) -> Option<(&'a SpreadTemplate, Vec<usize>, f64)> {
    let mut best: Option<(&SpreadTemplate, Vec<usize>, f64)> = None;

    for t in templates {
        if t.photo_count() != photos.len() {
            continue;
        }
        for assignment in permutations(photos.len()) {
            let Some(score) = score_spread(t, photos, &assignment, previous, w) else {
                continue;
            };
            let better = match &best {
                None => true,
                // Ties break on template id so the result is stable across
                // runs and machines, never on iteration order.
                Some((bt, _, bs)) => score > *bs || (score == *bs && t.id < bt.id),
            };
            if better {
                best = Some((t, assignment, score));
            }
        }
    }
    best
}

/// All permutations of `0..n`, in lexicographic order so enumeration is
/// deterministic.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let mut current: Vec<usize> = (0..n).collect();
    let mut used = vec![false; n];
    let mut buf = Vec::with_capacity(n);
    fn go(n: usize, used: &mut Vec<bool>, buf: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if buf.len() == n {
            out.push(buf.clone());
            return;
        }
        for i in 0..n {
            if used[i] {
                continue;
            }
            used[i] = true;
            buf.push(i);
            go(n, used, buf, out);
            buf.pop();
            used[i] = false;
        }
    }
    current.clear();
    go(n, &mut used, &mut buf, &mut out);
    out
}
```

Add `pub mod score;` to `src-tauri/src/book/mod.rs`.

- [ ] **Step 4: Run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml score
```
Expected: PASS, 12 tests.

- [ ] **Step 5: Mutation-check every soft term**

This is the highest-risk surface in the plan. For each term, zero its weight in `Weights::default()` and confirm a test fails:

```bash
# aspect_fit = 0.0    -> expect score_prefers_the_assignment_matching_slot_aspects to FAIL
# hero_match = 0.0    -> expect score_puts_the_highest_aesthetic_photo_in_the_hero_slot to FAIL
# variety = 0.0       -> expect score_penalises_reusing_the_previous_template to FAIL
```

Run after each edit:
```bash
cargo test --manifest-path src-tauri/Cargo.toml score
```

`saliency_retention`, `face_area_retention`, `resolution_headroom` and `palette_harmony` have no dedicated behavioural test yet. **Write one for each before continuing** — a term with no test that fails when it is zeroed is decoration. Model them on `score_prefers_the_assignment_matching_slot_aspects`: construct two photos where only that term differs, and assert the ordering.

Paste all seven mutation results into the commit body.

- [ ] **Step 6: Also mutate the aspect trap**

```bash
# Change slot_aspect to `slot.rect.w / slot.rect.h`.
cargo test --manifest-path src-tauri/Cargo.toml score
# Expect FAIL on score_slot_aspect_uses_page_inches_not_the_normalised_ratio. Revert.
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/book/score.rs src-tauri/src/book/mod.rs
git commit -m "feat(book): template scoring with hard face and gutter constraints"
```

---

## Task 7: Packing photos into spreads

**Files:**
- Create: `src-tauri/src/book/pack.rs`
- Modify: `src-tauri/src/book/mod.rs`

**Interfaces:**
- Consumes: `book::cull::Photo`, `templates::Library`.
- Produces:
  - `pub struct Capacity { pub pages: u32, pub singles: u32, pub spreads: u32, pub min_photos: usize, pub max_photos: usize }`
  - `pub fn Capacity::from_sizes(pages: u32, buildable: &[usize]) -> Capacity`
  - `pub fn Capacity::from_library(pages: u32, lib: &Library) -> Capacity`
  - `pub fn buildable_sizes(lib: &Library) -> Vec<usize>`
  - `pub fn recommend_pages_with(keeper_count: usize, buildable: &[usize]) -> u32`
  - `pub fn recommend_pages(keeper_count: usize, lib: &Library) -> u32`
  - `pub struct Group { pub photos: Vec<usize>, pub event_cluster: u32 }`
  - `pub fn pack(photos: &[Photo], capacity: &Capacity, buildable: &[usize]) -> Vec<Group>`

  The `_with`/`from_sizes` pair takes buildable sizes directly rather than a
  `Library` so every test in this task runs without loading template files.
  The `Library` wrappers exist for callers in Task 8.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/book/pack.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::PaletteColor;

    fn photo(path: &str, event: u32, aesthetic: u8) -> Photo {
        Photo {
            path: path.into(), hash: format!("h{path}"), width: 4000, height: 3000,
            is_utility: false, aesthetic_pct: aesthetic, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: event,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
        }
    }

    /// Buildable group sizes {1,2,3}: matches the real library's usable
    /// counts today (spec 6.1). Deliberately EXCLUDES 5 so the missing-5-up
    /// behaviour is exercised.
    fn sizes() -> Vec<usize> {
        vec![1, 2, 3]
    }

    #[test]
    fn pack_capacity_follows_two_singles_plus_spreads() {
        let c = Capacity::from_sizes(20, &sizes());
        assert_eq!(c.singles, 2);
        assert_eq!(c.spreads, 9, "20 pages = 2 singles + 9 spreads");
        let c40 = Capacity::from_sizes(40, &sizes());
        assert_eq!(c40.spreads, 19);
    }

    /// Boundary tests AT the boundaries, not near them.
    #[test]
    fn pack_recommends_twenty_pages_at_exactly_the_capacity_limit() {
        let twenty = Capacity::from_sizes(20, &sizes());
        assert_eq!(recommend_pages_with(twenty.max_photos, &sizes()), 20);
        assert_eq!(recommend_pages_with(twenty.max_photos + 1, &sizes()), 40);
    }

    #[test]
    fn pack_recommends_twenty_pages_for_a_tiny_set() {
        assert_eq!(recommend_pages_with(1, &sizes()), 20);
    }

    #[test]
    fn pack_never_emits_a_group_the_library_cannot_build() {
        let photos: Vec<Photo> =
            (0..11).map(|i| photo(&format!("/p{i:02}.jpg"), 0, 50)).collect();
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        for g in &groups {
            assert!(sizes().contains(&g.photos.len()),
                "group of {} is unbuildable", g.photos.len());
        }
    }

    /// The 5-photo gap, pinned. With only {1,2,3} buildable, five photos in
    /// one chapter must split, never emit a single group of five.
    #[test]
    fn pack_splits_a_chapter_of_five_because_no_five_up_template_exists() {
        let photos: Vec<Photo> =
            (0..5).map(|i| photo(&format!("/p{i}.jpg"), 7, 50)).collect();
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        assert!(groups.iter().all(|g| g.photos.len() != 5));
        assert_eq!(groups.iter().map(|g| g.photos.len()).sum::<usize>(), 5);
    }

    #[test]
    fn pack_does_not_mix_two_chapters_in_one_group() {
        let photos = vec![
            photo("/a.jpg", 1, 50), photo("/b.jpg", 1, 50),
            photo("/c.jpg", 2, 50), photo("/d.jpg", 2, 50),
        ];
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        for g in &groups {
            let clusters: std::collections::BTreeSet<u32> =
                g.photos.iter().map(|&i| photos[i].event_cluster).collect();
            assert_eq!(clusters.len(), 1, "a group must not span chapters");
        }
    }

    /// Input in NON-chronological order -- a pre-sorted fixture cannot
    /// detect a missing sort (a real Phase 1 failure mode).
    #[test]
    fn pack_orders_groups_by_chapter_regardless_of_input_order() {
        let photos = vec![
            photo("/z.jpg", 3, 50),
            photo("/a.jpg", 1, 50),
            photo("/m.jpg", 2, 50),
        ];
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        let clusters: Vec<u32> = groups.iter().map(|g| g.event_cluster).collect();
        assert_eq!(clusters, vec![1, 2, 3]);
    }

    #[test]
    fn pack_drops_the_lowest_ranked_photos_when_over_capacity() {
        let photos: Vec<Photo> = (0..100)
            .map(|i| photo(&format!("/p{i:03}.jpg"), 0, i as u8))
            .collect();
        let c = Capacity::from_sizes(20, &sizes());
        let groups = pack(&photos, &c, &sizes());
        let used: usize = groups.iter().map(|g| g.photos.len()).sum();
        assert!(used <= c.max_photos, "used {used}, capacity {}", c.max_photos);
        // The best photo must survive; the worst must not.
        let kept: std::collections::BTreeSet<usize> =
            groups.iter().flat_map(|g| g.photos.iter().copied()).collect();
        assert!(kept.contains(&99), "highest aesthetic must be kept");
        assert!(!kept.contains(&0), "lowest aesthetic must be dropped");
    }
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml pack
```
Expected: FAIL — `cannot find type Capacity`.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/book/pack.rs`:

```rust
//! Cutting chapters into per-spread photo groups.
//!
//! Group sizes are constrained by what the library can actually BUILD:
//! templates are exact-count, so a group of five is unbuildable until
//! 5-photo templates are authored. The packer takes the buildable sizes as
//! input rather than assuming 1..=6, so the missing-5-up gap degrades into a
//! different split rather than an unfillable spread.

use crate::book::cull::Photo;
use crate::templates::Library;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capacity {
    pub pages: u32,
    pub singles: u32,
    pub spreads: u32,
    pub min_photos: usize,
    pub max_photos: usize,
}

impl Capacity {
    /// A book is 2 single pages facing the inside covers plus (N-2)/2
    /// spreads -- confirmed against Pixajoy's page navigator, which reads
    /// `Cover . 1 . 2-3 . 4-5 . ...`. It is NOT N/2 spreads.
    pub fn from_sizes(pages: u32, buildable: &[usize]) -> Capacity {
        let singles = 2;
        let spreads = (pages.saturating_sub(2)) / 2;
        let smallest = buildable.iter().copied().min().unwrap_or(1);
        let largest = buildable.iter().copied().max().unwrap_or(1);
        // A single page holds at most what one page-half holds. The half
        // pool's largest half is bounded by the largest spread, so `largest`
        // is a safe upper bound and `smallest` a safe lower one.
        Capacity {
            pages,
            singles,
            spreads,
            min_photos: (spreads as usize + singles as usize) * smallest,
            max_photos: spreads as usize * largest + singles as usize * largest,
        }
    }

    pub fn from_library(pages: u32, lib: &Library) -> Capacity {
        Capacity::from_sizes(pages, &buildable_sizes(lib))
    }
}

/// The photo counts the library can actually build a spread for.
pub fn buildable_sizes(lib: &Library) -> Vec<usize> {
    let mut sizes: Vec<usize> =
        lib.spreads.iter().map(|t| t.photo_count()).collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
    sizes.sort_unstable();
    sizes
}

/// Smallest SKU that fits the keepers, defaulting to 20 pages.
pub fn recommend_pages_with(keeper_count: usize, buildable: &[usize]) -> u32 {
    let twenty = Capacity::from_sizes(20, buildable);
    if keeper_count <= twenty.max_photos {
        20
    } else {
        40
    }
}

pub fn recommend_pages(keeper_count: usize, lib: &Library) -> u32 {
    recommend_pages_with(keeper_count, &buildable_sizes(lib))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Indices into the photo slice handed to `pack`.
    pub photos: Vec<usize>,
    pub event_cluster: u32,
}

/// Walks chapters in chronological order, cutting each into buildable
/// groups. A group never spans two chapters: a new chapter opening halfway
/// through a spread reads as an accident rather than a decision.
///
/// When the keepers exceed capacity, the LOWEST aesthetic percentiles are
/// dropped first, chapter proportions preserved.
pub fn pack(photos: &[Photo], capacity: &Capacity, buildable: &[usize]) -> Vec<Group> {
    use std::collections::BTreeMap;

    if photos.is_empty() || buildable.is_empty() {
        return Vec::new();
    }

    // Chapters, keyed by cluster id so iteration is chronological regardless
    // of the input slice's order.
    let mut chapters: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, p) in photos.iter().enumerate() {
        chapters.entry(p.event_cluster).or_default().push(i);
    }

    // Trim to capacity by dropping the weakest photos overall.
    let total: usize = chapters.values().map(Vec::len).sum();
    if total > capacity.max_photos {
        let mut ranked: Vec<usize> = (0..photos.len()).collect();
        // Sort worst-first: aesthetic, then sharpness, then path for
        // determinism.
        ranked.sort_by(|&a, &b| {
            photos[a]
                .aesthetic_pct
                .cmp(&photos[b].aesthetic_pct)
                .then(photos[a].sharpness_pct.cmp(&photos[b].sharpness_pct))
                .then(photos[a].path.cmp(&photos[b].path))
        });
        let drop_count = total - capacity.max_photos;
        let dropped: std::collections::BTreeSet<usize> =
            ranked.into_iter().take(drop_count).collect();
        for bucket in chapters.values_mut() {
            bucket.retain(|i| !dropped.contains(i));
        }
        chapters.retain(|_, v| !v.is_empty());
    }

    let mut groups = Vec::new();
    let mut slots_left = capacity.spreads as usize + capacity.singles as usize;

    for (cluster, mut members) in chapters {
        // Chronological within the chapter is not knowable without capture
        // times here, so path order is used -- stable, and the same order
        // `finalize_photos` already established.
        members.sort_by(|&a, &b| photos[a].path.cmp(&photos[b].path));

        let mut i = 0;
        while i < members.len() && slots_left > 0 {
            let remaining = members.len() - i;
            let take = choose_group_size(remaining, buildable, slots_left);
            groups.push(Group {
                photos: members[i..i + take].to_vec(),
                event_cluster: cluster,
            });
            i += take;
            slots_left -= 1;
        }
    }

    groups
}

/// Largest buildable size that does not strand an unbuildable remainder.
///
/// With buildable sizes {1,2,3} and 5 remaining, taking 3 leaves 2 (fine);
/// taking 2 leaves 3 (also fine). With {2,3} and 5 remaining, taking 3
/// leaves 2 -- but taking 2 leaves 3, so both work. The lookahead matters
/// when a size would strand a remainder no size can cover.
fn choose_group_size(remaining: usize, buildable: &[usize], slots_left: usize) -> usize {
    let largest = buildable.iter().copied().max().unwrap_or(1);

    // On the last available slot, take as much as one spread can hold.
    if slots_left == 1 {
        return remaining.min(largest);
    }

    let mut best = *buildable
        .iter()
        .filter(|&&s| s <= remaining)
        .max()
        .unwrap_or(&1);

    // Prefer a size whose remainder is itself coverable.
    for &size in buildable.iter().rev() {
        if size > remaining {
            continue;
        }
        let rest = remaining - size;
        if rest == 0 || buildable.iter().any(|&s| s <= rest) {
            best = size;
            break;
        }
    }
    best.min(remaining).max(1)
}
```

Add `pub mod pack;` to `src-tauri/src/book/mod.rs`.

- [ ] **Step 4: Run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml pack
```
Expected: PASS, 8 tests.

- [ ] **Step 5: Mutation-check the page-structure maths**

```bash
# Change `(pages - 2) / 2` to `pages / 2` -- the pre-2026-08-13 assumption.
cargo test --manifest-path src-tauri/Cargo.toml pack
# Expect FAIL on pack_capacity_follows_two_singles_plus_spreads. Revert.
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/book/pack.rs src-tauri/src/book/mod.rs
git commit -m "feat(book): chapter-aware packing with buildable group sizes"
```

---

## Task 8: Pacing and book assembly

**Files:**
- Create: `src-tauri/src/book/pace.rs`
- Modify: `src-tauri/src/book/mod.rs`

**Interfaces:**
- Consumes: everything from Tasks 2–7.
- Produces:
  - `pub struct Placement { pub photo_index: usize, pub slot_rect: Rect, pub crop: Rect, pub z: u32 }`
  - `pub struct Page { pub number: u32, pub side: Side, pub template_id: String, pub placements: Vec<Placement> }`
  - `pub struct Book { pub pages: Vec<Page>, pub seed: u64, pub dropped: usize }`
  - `pub fn assemble(photos: &[Photo], pages: u32, lib: &Library, w: &Weights, seed: u64) -> Book`
  - `pub fn repace(book: &mut Book, lib: &Library)`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/book/pace.rs` with a test module covering:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_assembles_the_correct_page_count_for_twenty_pages() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let book = assemble(&photos, 20, &lib, &Weights::default(), 42);
        assert_eq!(book.pages.len(), 20, "2 singles + 9 spreads = 20 pages");
        assert_eq!(book.pages[0].side, Side::Right, "page 1 faces the inside front cover");
        assert_eq!(book.pages.last().unwrap().side, Side::Left,
            "the final page faces the inside back cover");
    }

    #[test]
    fn pace_numbers_pages_consecutively_from_one() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 42);
        let numbers: Vec<u32> = book.pages.iter().map(|p| p.number).collect();
        assert_eq!(numbers, (1..=20).collect::<Vec<u32>>());
    }

    #[test]
    fn pace_is_deterministic_for_the_same_seed() {
        let lib = fixture_library();
        let photos = fixture_photos(24);
        let a = assemble(&photos, 20, &lib, &Weights::default(), 7);
        let b = assemble(&photos, 20, &lib, &Weights::default(), 7);
        assert_eq!(a.pages, b.pages);
    }

    #[test]
    fn pace_avoids_three_consecutive_spreads_of_the_same_density() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(40), 40, &lib, &Weights::default(), 3);
        let ids: Vec<&str> = book.pages.iter().map(|p| p.template_id.as_str()).collect();
        for window in ids.windows(6) {
            let distinct: std::collections::BTreeSet<&&str> = window.iter().collect();
            assert!(distinct.len() > 1, "six consecutive pages share one template: {window:?}");
        }
    }

    #[test]
    fn pace_records_how_many_photos_were_dropped() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(500), 20, &lib, &Weights::default(), 1);
        assert!(book.dropped > 0, "a 500-photo set cannot fit 20 pages");
    }

    #[test]
    fn pace_places_every_photo_within_its_page() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
        for page in &book.pages {
            for pl in &page.placements {
                assert!(pl.slot_rect.x >= -1e-9 && pl.slot_rect.right() <= 1.0 + 1e-9,
                    "page {} slot escapes the canvas", page.number);
            }
        }
    }

    #[test]
    fn pace_assigns_distinct_z_order_within_a_page() {
        let lib = fixture_library();
        let book = assemble(&fixture_photos(24), 20, &lib, &Weights::default(), 5);
        for page in &book.pages {
            let zs: std::collections::BTreeSet<u32> =
                page.placements.iter().map(|p| p.z).collect();
            assert_eq!(zs.len(), page.placements.len(), "z values must be distinct");
        }
    }
}
```

Add the two fixture helpers (`fixture_library`, `fixture_photos`) to the same module. `fixture_library` must contain at least a 1-up, a 2-up and a 3-up spread with **asymmetric** slot rects; `fixture_photos(n)` must produce photos with varying aspect ratios (4:3, 3:2, 3:4) and varying `event_cluster` values — never all-square, never all one chapter.

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml pace
```
Expected: FAIL — `cannot find function assemble`.

- [ ] **Step 3: Implement `assemble`**

Prepend to `src-tauri/src/book/pace.rs`:

```rust
//! Assembling a whole book, then sweeping it once for rhythm.
//!
//! Ordering is deliberate and load-bearing: cull before sizing (a book
//! cannot be sized before it is culled), pack before scoring (exact-count
//! templates mean group size decides eligibility), pace after scoring (a
//! rhythm cannot be varied before it exists).

use crate::book::crop::choose_crop;
use crate::book::cull::{cull, Photo};
use crate::book::pack::{buildable_sizes, pack, Capacity, Group};
use crate::book::score::{best_spread, slot_aspect};
use crate::geometry::{Rect, Side};
use crate::templates::{Library, PageLayout, Weights};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Index into the ORIGINAL photo slice handed to `assemble`, so the
    /// manifest can name the source file without a second lookup table.
    pub photo_index: usize,
    /// Page-normalised destination rect.
    pub slot_rect: Rect,
    /// Crop window in the photo's own normalised coordinates.
    pub crop: Rect,
    /// Stacking order within the page, 1-based. Only matters where slots
    /// overlap, but it is also the order the user places the boxes in.
    pub z: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub number: u32,
    pub side: Side,
    pub template_id: String,
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub pages: Vec<Page>,
    pub seed: u64,
    /// How many analysed photos did not make it into the book, for the
    /// confirmation the user sees before anything is rendered.
    pub dropped: usize,
}

/// Deterministic tie-breaker. The ONLY stochastic choice in the engine is
/// breaking an exact score tie; everything else is a total order over the
/// inputs. "Regenerate this spread" ADVANCES the seed rather than calling a
/// random source, which is what makes regeneration feel like browsing
/// alternatives instead of rolling dice -- and what makes golden files
/// possible at all.
fn tie_break(seed: u64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    // SplitMix64: tiny, dependency-free, and stable across platforms and
    // Rust versions -- `DefaultHasher` guarantees none of those.
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) % n as u64) as usize
}

/// Builds the placements for one page layout given the photos assigned to it.
fn place(layout: &PageLayout, photo_indices: &[usize], photos: &[Photo]) -> Vec<Placement> {
    layout
        .slots
        .iter()
        .zip(photo_indices)
        .enumerate()
        .map(|(i, (slot, &photo_index))| Placement {
            photo_index,
            slot_rect: slot.rect,
            crop: choose_crop(&photos[photo_index], slot_aspect(slot)),
            z: i as u32 + 1,
        })
        .collect()
}

/// Picks the best single-page layout from the half-pool for a group.
/// Pages 1 and N have no partner page, so they draw from every half of every
/// template -- a half is by construction a valid page layout, so the single
/// pages get a library for free and one that matches the book's style.
fn best_single<'a>(
    pool: &[&'a PageLayout],
    photos: &[Photo],
    group: &Group,
    seed: u64,
) -> Option<(&'a PageLayout, Vec<usize>)> {
    let n = group.photos.len();
    let eligible: Vec<&&PageLayout> = pool.iter().filter(|p| p.slots.len() == n).collect();
    if eligible.is_empty() {
        return None;
    }
    // Score by aspect fit alone: a single page has no spread-level terms.
    let mut best: Option<(&PageLayout, f64)> = None;
    for layout in &eligible {
        let fit: f64 = layout
            .slots
            .iter()
            .zip(&group.photos)
            .map(|(slot, &pi)| {
                let target = slot_aspect(slot);
                let actual = photos[pi].aspect();
                if target > actual { actual / target } else { target / actual }
            })
            .sum();
        if best.is_none_or(|(_, b)| fit > b) {
            best = Some((layout, fit));
        }
    }
    let (chosen, _) = best?;
    let _ = seed; // tie-breaking is unnecessary here; kept for symmetry
    Some((chosen, group.photos.clone()))
}

/// Full pipeline: cull -> size -> pack -> score -> place -> pace.
pub fn assemble(photos: &[Photo], pages: u32, lib: &Library, w: &Weights, seed: u64) -> Book {
    let kept = cull(photos);
    let sizes = buildable_sizes(lib);
    let cap = Capacity::from_sizes(pages, &sizes);
    let groups = pack(&kept, &cap, &sizes);

    // `pack` indexes into `kept`; the manifest wants indices into `photos`.
    let index_of = |p: &Photo| photos.iter().position(|q| q.hash == p.hash).unwrap_or(0);
    let remap = |g: &Group| Group {
        photos: g.photos.iter().map(|&i| index_of(&kept[i])).collect(),
        event_cluster: g.event_cluster,
    };
    let groups: Vec<Group> = groups.iter().map(remap).collect();

    let mut out: Vec<Page> = Vec::with_capacity(pages as usize);
    let pool = lib.page_half_pool();
    let mut placed = 0usize;
    let mut number = 1u32;

    // Page 1: a single, facing the inside front cover.
    if let Some(first) = groups.first() {
        if let Some((layout, indices)) = best_single(&pool, photos, first, seed) {
            placed += indices.len();
            out.push(Page {
                number,
                side: Side::Right, // page 1 is a RIGHT-hand page
                template_id: format!("half:{:?}:{}", layout.side, layout.slots.len()),
                placements: place(layout, &indices, photos),
            });
            number += 1;
        }
    }

    // The middle: true spreads, two pages each.
    let middle = groups.iter().skip(1).take(groups.len().saturating_sub(2));
    let mut previous: Option<String> = None;
    for group in middle {
        let refs: Vec<&Photo> = group.photos.iter().map(|&i| &photos[i]).collect();
        let eligible = lib.spreads_with(group.photos.len());
        let Some((template, assignment, _)) =
            best_spread(&eligible, &refs, previous.as_deref(), w)
        else {
            continue;
        };

        let left_n = template.left.slots.len();
        let left_idx: Vec<usize> =
            assignment[..left_n].iter().map(|&a| group.photos[a]).collect();
        let right_idx: Vec<usize> =
            assignment[left_n..].iter().map(|&a| group.photos[a]).collect();

        out.push(Page {
            number,
            side: Side::Left,
            template_id: template.id.clone(),
            placements: place(&template.left, &left_idx, photos),
        });
        out.push(Page {
            number: number + 1,
            side: Side::Right,
            template_id: template.id.clone(),
            placements: place(&template.right, &right_idx, photos),
        });
        number += 2;
        placed += group.photos.len();
        previous = Some(template.id.clone());
    }

    // The final page: a single, facing the inside back cover.
    if groups.len() > 1 {
        if let Some(last) = groups.last() {
            if let Some((layout, indices)) = best_single(&pool, photos, last, seed) {
                placed += indices.len();
                out.push(Page {
                    number,
                    side: Side::Left, // the last page is a LEFT-hand page
                    template_id: format!("half:{:?}:{}", layout.side, layout.slots.len()),
                    placements: place(layout, &indices, photos),
                });
            }
        }
    }

    // Renumber consecutively: a skipped group must not leave a gap.
    for (i, page) in out.iter_mut().enumerate() {
        page.number = i as u32 + 1;
    }

    let mut book = Book { pages: out, seed, dropped: photos.len().saturating_sub(placed) };
    repace(&mut book, lib);
    book
}

/// Sweeps the assembled book once and breaks flat stretches.
///
/// Runs AFTER scoring because a rhythm cannot be varied before it exists.
/// The three axes are `density`, `energy` (both authored) and edge treatment
/// (derived from the bleed arrays) -- the user wants bleed and margin both
/// present and alternating, not a book that commits to either.
pub fn repace(book: &mut Book, lib: &Library) {
    let template_of = |id: &str| lib.spreads.iter().find(|t| t.id == id);

    // Find runs of three or more consecutive SPREADS sharing a density, and
    // swap the middle one for a same-photo-count template of another density.
    let mut i = 2;
    while i < book.pages.len() {
        let ids: Vec<&str> = book.pages[i - 2..=i].iter().map(|p| p.template_id.as_str()).collect();
        let densities: Vec<_> = ids.iter().filter_map(|id| template_of(id)).map(|t| t.density).collect();
        if densities.len() == 3 && densities[0] == densities[1] && densities[1] == densities[2] {
            let middle = &book.pages[i - 1];
            let count = middle.placements.len();
            if let Some(alt) = lib
                .spreads
                .iter()
                .filter(|t| t.left.slots.len() == count || t.right.slots.len() == count)
                .find(|t| t.density != densities[1])
            {
                book.pages[i - 1].template_id = alt.id.clone();
            }
        }
        i += 1;
    }
}
```

Note `is_none_or` requires Rust 1.82+; if the toolchain is older, use `best.map_or(true, |(_, b)| fit > b)`.

- [ ] **Step 4: Run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml pace
```
Expected: PASS, 7 tests.

- [ ] **Step 5: Add a golden-file test against a frozen fixture library**

Create `src-tauri/tests/fixtures/templates/` holding **copies** of five real templates. The goldens must never read `templates/`: Task 13 authors 15–20 more, and every golden would churn on that commit, degrading into noise people regenerate without reading.

```rust
#[test]
fn pace_golden_twenty_page_book() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates");
    let lib = Library::load(&dir).unwrap();
    let book = assemble(&fixture_photos(30), 20, &lib, &Weights::default(), 1234);
    let actual = serde_json::to_string_pretty(&book).unwrap();
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/book-20.json");
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(&golden_path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&golden_path).expect("run with UPDATE_GOLDEN=1 first");
    assert_eq!(actual, expected);
}
```

Generate it once: `UPDATE_GOLDEN=1 cargo test --manifest-path src-tauri/Cargo.toml pace_golden`, then **read the produced JSON** and confirm the layout is sane before committing it. A golden nobody read is a golden that pins a bug.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/book/pace.rs src-tauri/src/book/mod.rs src-tauri/tests/fixtures/
git commit -m "feat(book): assemble and pace a full book from scored spreads"
```

---

## Task 9: Swift renderer

**Blocked on Task 0.** Do not start until the alpha test has passed.

**Files:**
- Create: `sidecar/Sources/PhotobookEngine/Renderer.swift`, `sidecar/Tests/PhotobookEngineTests/RendererTests.swift`
- Modify: `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/Handler.swift`

**Interfaces:**
- Produces (Swift):
  - `struct RenderPlacement: Codable { let sourcePath: String; let cropX/cropY/cropW/cropH: Double; let destX/destY/destW/destH: Double; let filename: String }`
  - `struct RenderRequest: Codable { let outputDir: String; let pageWidthPx: Int; let pageHeightPx: Int; let placements: [RenderPlacement] }`
  - `enum RenderRecord: Codable { case ok(path: String); case failed(filename: String, message: String) }`
  - `enum Renderer { static func render(_ p: RenderPlacement, request: RenderRequest) -> RenderRecord }`
  - `RequestKind.render`, `ResponseResult.rendered([RenderRecord])`

- [ ] **Step 1: Write the failing tests**

Create `sidecar/Tests/PhotobookEngineTests/RendererTests.swift`. **Prefix every test name with `renderer`** — top-level `@Test func`s share one module namespace and a generic name is a compile error, not a test failure.

```swift
import Testing
import Foundation
import CoreGraphics
import ImageIO
@testable import PhotobookEngine

@Test func rendererProducesExactPageDimensions() throws {
    let out = try tempDir()
    let src = try writeFixtureJPEG(width: 1200, height: 800, into: out)  // 3:2, never square
    let req = RenderRequest(outputDir: out.path, pageWidthPx: 3359, pageHeightPx: 2668,
                            placements: [])
    let p = RenderPlacement(sourcePath: src.path,
                            cropX: 0, cropY: 0, cropW: 1, cropH: 1,
                            destX: 0.1, destY: 0.1, destW: 0.5, destH: 0.4,
                            filename: "p01-z1-test.png")
    guard case .ok(let path) = Renderer.render(p, request: req) else {
        Issue.record("render failed"); return
    }
    let img = try loadCGImage(atPath: path)
    #expect(img.width == 3359)
    #expect(img.height == 2668)
}

@Test func rendererLeavesAreaOutsideTheSlotFullyTransparent() throws {
    let out = try tempDir()
    let src = try writeFixtureJPEG(width: 1200, height: 800, into: out)
    let req = RenderRequest(outputDir: out.path, pageWidthPx: 3359, pageHeightPx: 2668,
                            placements: [])
    let p = RenderPlacement(sourcePath: src.path,
                            cropX: 0, cropY: 0, cropW: 1, cropH: 1,
                            destX: 0.25, destY: 0.25, destW: 0.5, destH: 0.5,
                            filename: "p01-z1-alpha.png")
    guard case .ok(let path) = Renderer.render(p, request: req) else {
        Issue.record("render failed"); return
    }
    // A pixel well outside the slot must be alpha 0. This is the assertion a
    // composite path cannot fake: it is the whole delivery mechanic.
    #expect(try alpha(atX: 100, y: 100, ofImageAtPath: path) == 0)
    // ...and a pixel inside the slot must be opaque.
    #expect(try alpha(atX: 1680, y: 1334, ofImageAtPath: path) == 255)
}

@Test func rendererEmbedsAnSRGBProfile() throws {
    let out = try tempDir()
    let src = try writeFixtureJPEG(width: 1200, height: 800, into: out)
    let req = RenderRequest(outputDir: out.path, pageWidthPx: 3359, pageHeightPx: 2668,
                            placements: [])
    let p = RenderPlacement(sourcePath: src.path,
                            cropX: 0, cropY: 0, cropW: 1, cropH: 1,
                            destX: 0, destY: 0, destW: 1, destH: 1,
                            filename: "p01-z1-icc.png")
    guard case .ok(let path) = Renderer.render(p, request: req) else {
        Issue.record("render failed"); return
    }
    let img = try loadCGImage(atPath: path)
    let name = img.colorSpace?.name as String?
    #expect(name == (CGColorSpace.sRGB as String),
            "expected sRGB, got \(name ?? "nil")")
}

@Test func rendererAppliesTheCropWindow() throws {
    let out = try tempDir()
    // Red LEFT half, blue RIGHT half. 1200x800 -- never square.
    let src = try writeHalvedFixtureJPEG(width: 1200, height: 800,
                                         left: .red, right: .blue, into: out)
    let req = RenderRequest(outputDir: out.path, pageWidthPx: 3359, pageHeightPx: 2668,
                            placements: [])
    // Crop to the RIGHT half only.
    let p = RenderPlacement(sourcePath: src.path,
                            cropX: 0.5, cropY: 0, cropW: 0.5, cropH: 1,
                            destX: 0.2, destY: 0.2, destW: 0.6, destH: 0.6,
                            filename: "p01-z1-crop.png")
    guard case .ok(let path) = Renderer.render(p, request: req) else {
        Issue.record("render failed"); return
    }
    // Sample near the slot's LEFT edge: under a correct crop this is the
    // blue half's left edge, not red. A centre-only sample would pass even
    // if the crop were ignored entirely.
    let x = Int(0.22 * 3359.0)
    let y = Int(0.5 * 2668.0)
    let (r, _, b) = try rgb(atX: x, y: y, ofImageAtPath: path)
    #expect(b > r, "expected the blue half, got r=\(r) b=\(b)")
}

@Test func rendererReportsAFailureForAMissingSourceFile() throws {
    let out = try tempDir()
    let req = RenderRequest(outputDir: out.path, pageWidthPx: 3359, pageHeightPx: 2668,
                            placements: [])
    let p = RenderPlacement(sourcePath: "/nonexistent/nope.jpg",
                            cropX: 0, cropY: 0, cropW: 1, cropH: 1,
                            destX: 0, destY: 0, destW: 1, destH: 1,
                            filename: "p01-z1-missing.png")
    guard case .failed(let filename, _) = Renderer.render(p, request: req) else {
        Issue.record("expected a failure record, not a crash"); return
    }
    #expect(filename == "p01-z1-missing.png")
}

@Test func rendererHandlesATinySourceWithoutCrashing() throws {
    // The DPI guard lives in Rust pre-flight, not here -- the renderer's
    // contract is that a low-resolution source still produces a valid
    // full-page PNG rather than failing or allocating absurdly.
    let out = try tempDir()
    let src = try writeFixtureJPEG(width: 200, height: 150, into: out)
    let req = RenderRequest(outputDir: out.path, pageWidthPx: 3359, pageHeightPx: 2668,
                            placements: [])
    let p = RenderPlacement(sourcePath: src.path,
                            cropX: 0, cropY: 0, cropW: 1, cropH: 1,
                            destX: 0, destY: 0, destW: 1, destH: 1,
                            filename: "p01-z1-tiny.png")
    guard case .ok(let path) = Renderer.render(p, request: req) else {
        Issue.record("a low-resolution source must still render"); return
    }
    let img = try loadCGImage(atPath: path)
    #expect(img.width == 3359)
}
```

Write the helpers in the same file: `tempDir()`, `writeFixtureJPEG(width:height:into:)`, `writeHalvedFixtureJPEG(width:height:left:right:into:)`, `loadCGImage(atPath:)`, `alpha(atX:y:ofImageAtPath:)` and `rgb(atX:y:ofImageAtPath:)`.

**`writeFixtureJPEG` must never produce a square image** — a square makes `height/width == 1` and hides the entire class of aspect bug. Take width and height as separate parameters and assert they differ.

- [ ] **Step 2: Run and confirm failure**

```bash
bun run test:swift 2>&1 | head -30
```
Expected: FAIL — `cannot find 'Renderer' in scope`.

- [ ] **Step 3: Implement `Renderer.swift`**

```swift
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

struct RenderPlacement: Codable {
    let sourcePath: String
    /// Crop window in the SOURCE image's normalised coordinates, top-left origin.
    let cropX: Double, cropY: Double, cropW: Double, cropH: Double
    /// Destination rect in PAGE-normalised coordinates, top-left origin.
    let destX: Double, destY: Double, destW: Double, destH: Double
    let filename: String
}

struct RenderRequest: Codable {
    let outputDir: String
    let pageWidthPx: Int
    let pageHeightPx: Int
    let placements: [RenderPlacement]
}

enum RenderRecord: Codable {
    case ok(path: String)
    case failed(filename: String, message: String)

    private enum CodingKeys: String, CodingKey { case status, path, filename, message }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let path):
            try c.encode("ok", forKey: .status)
            try c.encode(path, forKey: .path)
        case .failed(let filename, let message):
            try c.encode("failed", forKey: .status)
            try c.encode(filename, forKey: .filename)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .status) == "ok" {
            self = .ok(path: try c.decode(String.self, forKey: .path))
        } else {
            self = .failed(filename: try c.decode(String.self, forKey: .filename),
                           message: try c.decode(String.self, forKey: .message))
        }
    }
}

enum Renderer {
    /// Renders ONE photo into a full-page transparent PNG.
    ///
    /// One file per photo rather than one composite per page: Pixajoy's
    /// boxes have no numeric position entry, so the transparency is what
    /// carries the engine's geometry through a drag-only editor. The user
    /// stacks one full-bleed box per photo and the layout reproduces exactly.
    static func render(_ p: RenderPlacement, request: RenderRequest) -> RenderRecord {
        // The autoreleasepool must enclose the DECODE, not just the draw:
        // full decompression happens inside `loadFullImage`, and a worker
        // thread has no pool of its own.
        autoreleasepool {
            guard let source = loadFullImage(path: p.sourcePath) else {
                return .failed(filename: p.filename, message: "could not decode \(p.sourcePath)")
            }

            let cropRect = CGRect(
                x: p.cropX * Double(source.width),
                y: p.cropY * Double(source.height),
                width: p.cropW * Double(source.width),
                height: p.cropH * Double(source.height)
            ).integral

            guard let cropped = source.cropping(to: cropRect) else {
                return .failed(filename: p.filename, message: "crop window outside the image")
            }

            let space = CGColorSpace(name: CGColorSpace.sRGB)!
            guard let ctx = CGContext(
                data: nil,
                width: request.pageWidthPx,
                height: request.pageHeightPx,
                bitsPerComponent: 8,
                bytesPerRow: 0,
                space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else {
                return .failed(filename: p.filename, message: "could not create page context")
            }

            // Fully transparent ground. Everything outside the slot stays
            // alpha 0 -- asserted by `rendererLeavesAreaOutsideTheSlotFullyTransparent`.
            ctx.clear(CGRect(x: 0, y: 0, width: request.pageWidthPx, height: request.pageHeightPx))

            // Core Graphics is bottom-left origin; the wire format is
            // top-left. Flip y here, exactly once.
            let w = p.destW * Double(request.pageWidthPx)
            let h = p.destH * Double(request.pageHeightPx)
            let x = p.destX * Double(request.pageWidthPx)
            let y = Double(request.pageHeightPx) - (p.destY * Double(request.pageHeightPx)) - h
            ctx.interpolationQuality = .high
            ctx.draw(cropped, in: CGRect(x: x, y: y, width: w, height: h))

            guard let out = ctx.makeImage() else {
                return .failed(filename: p.filename, message: "could not rasterise page")
            }

            let url = URL(fileURLWithPath: request.outputDir).appendingPathComponent(p.filename)
            guard let dest = CGImageDestinationCreateWithURL(
                url as CFURL, UTType.png.identifier as CFString, 1, nil
            ) else {
                return .failed(filename: p.filename, message: "could not open \(url.path)")
            }
            CGImageDestinationAddImage(dest, out, nil)
            guard CGImageDestinationFinalize(dest) else {
                return .failed(filename: p.filename, message: "could not write \(url.path)")
            }
            return .ok(path: url.path)
        }
    }

    /// Full-resolution decode with EXIF orientation applied.
    /// `kCGImageSourceCreateThumbnailWithTransform` reconciles EXIF
    /// orientation against HEIC's container `irot`/`imir`, which can
    /// disagree -- the same reasoning as `ImageLoader`.
    private static func loadFullImage(path: String) -> CGImage? {
        guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil)
        else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: 6000,
        ]
        return CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary)
    }
}
```

- [ ] **Step 4: Wire the protocol**

In `Protocol.swift`, add `case render` to `RequestKind`, `case rendered([RenderRecord])` to `ResponseResult` (with matching encode/decode arms following the existing pattern exactly), and add `var render: RenderRequest?` to `Request`.

In `Handler.swift`, add the dispatch arm alongside `.analyze`:

```swift
        case .render:
            guard let r = request.render else {
                return Response(id: request.id,
                                result: .error(ErrorResult(message: "render request missing payload")))
            }
            let records = r.placements.map { Renderer.render($0, request: r) }
            return Response(id: request.id, result: .rendered(records))
```

- [ ] **Step 5: Run**

```bash
bun run test:swift
```
Expected: PASS. All 71 existing Swift tests plus the new ones.

- [ ] **Step 6: Mutation-check the transparency and the y-flip**

```bash
# 1. Replace ctx.clear(...) with ctx.setFillColor(white) + ctx.fill(...).
bun run test:swift
# Expect FAIL on rendererLeavesAreaOutsideTheSlotFullyTransparent. Revert.

# 2. Remove the y-flip: use `y = p.destY * Double(request.pageHeightPx)`.
bun run test:swift
# Expect FAIL on the alpha-position assertion. Revert.
```

- [ ] **Step 7: Commit**

```bash
git add sidecar/
git commit -m "feat(sidecar): render one transparent full-page PNG per photo"
```

---

## Task 10: Rust render client and manifest

**Blocked on Task 0.**

**Files:**
- Create: `src-tauri/src/render.rs`, `src-tauri/src/book/manifest.rs`
- Modify: `src-tauri/src/protocol.rs`, `src-tauri/src/book/mod.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `RequestKind::Render`, `ResponseResult::Rendered(Vec<serde_json::Value>)`, `Request.render: Option<RenderRequest>`
  - `pub struct RenderRequest`, `pub struct RenderPlacement` — field names matching Swift **exactly**
  - `pub fn build_placements(book: &Book, photos: &[Photo]) -> Vec<RenderPlacement>`
  - `pub struct Manifest { … }`, `pub fn manifest(book: &Book, photos: &[Photo], pages: u32, seed: u64) -> Manifest`

- [ ] **Step 1: Write the failing wire-format test**

The Swift↔Rust wire format is pinned from both sides, and this is where a silent key-name disagreement would degrade into "every photo failed" with no cause. Add to `src-tauri/src/protocol.rs`'s test module:

```rust
    /// Pins every field name in the render request against Swift's
    /// `RenderPlacement`/`RenderRequest` in `Renderer.swift`. There is no
    /// blanket `rename_all` here, so each must be spelled out or the two
    /// sides silently disagree -- the exact failure mode `thumbnailDir`
    /// already documents.
    #[test]
    fn serializes_render_request_with_swift_field_names() {
        let req = Request {
            id: "r1".into(),
            kind: RequestKind::Render,
            paths: None,
            thumbnail_dir: None,
            render: Some(crate::render::RenderRequest {
                output_dir: "/out".into(),
                page_width_px: 3359,
                page_height_px: 2668,
                placements: vec![crate::render::RenderPlacement {
                    source_path: "/p/a.jpg".into(),
                    crop_x: 0.1, crop_y: 0.2, crop_w: 0.3, crop_h: 0.4,
                    dest_x: 0.5, dest_y: 0.6, dest_w: 0.2, dest_h: 0.1,
                    filename: "p04-z1-abc.png".into(),
                }],
            }),
        };
        let line = serde_json::to_string(&req).unwrap();
        for key in ["outputDir", "pageWidthPx", "pageHeightPx", "placements",
                    "sourcePath", "cropX", "cropY", "cropW", "cropH",
                    "destX", "destY", "destW", "destH", "filename"] {
            assert!(line.contains(&format!("\"{key}\"")), "missing wire key {key} in {line}");
        }
        assert!(!line.contains("output_dir"), "snake_case leaked onto the wire");
    }

    #[test]
    fn deserializes_rendered_response() {
        let line = r#"{"id":"r1","result":{"type":"rendered","data":[
            {"status":"ok","path":"/out/p04-z1-abc.png"},
            {"status":"failed","filename":"p04-z2-def.png","message":"could not decode"}
        ]}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        match res.result {
            ResponseResult::Rendered(records) => {
                assert_eq!(records.len(), 2);
                assert_eq!(records[0]["path"], "/out/p04-z1-abc.png");
                assert_eq!(records[1]["filename"], "p04-z2-def.png");
            }
            other => panic!("expected rendered, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml protocol
```
Expected: FAIL — no `RequestKind::Render` variant.

- [ ] **Step 3: Implement**

In `src-tauri/src/protocol.rs` add `Render` to `RequestKind`, `Rendered(Vec<serde_json::Value>)` to `ResponseResult`, and to `Request`:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render: Option<crate::render::RenderRequest>,
```

Every existing `Request { … }` literal in the codebase now needs `render: None`. Find them:

```bash
grep -rn "thumbnail_dir:" src-tauri/src src-tauri/tests
```

Create `src-tauri/src/render.rs` with `RenderRequest`/`RenderPlacement` using **explicit per-field `#[serde(rename = …)]`** to camelCase (not a blanket `rename_all`, matching the existing convention and its documented reasoning), plus `build_placements`.

`build_placements` walks `book.pages`, and for each `Placement` emits a `RenderPlacement` with `filename` formatted `p{page:02}-z{z}-{hash_prefix}.png` so files sort into placement order.

Create `src-tauri/src/book/manifest.rs` recording, per photo: source path, source content hash, crop rect, destination rect **in inches**, page number, z-order, output filename — plus book length, seed, and the template id per spread. Include a test asserting a page with zero placements still appears in the manifest with an empty photo list, so page counts reconcile.

- [ ] **Step 4: Run**

```bash
bun run sidecar && cargo test --manifest-path src-tauri/Cargo.toml
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/render.rs src-tauri/src/book/manifest.rs src-tauri/src/protocol.rs \
        src-tauri/src/book/mod.rs src-tauri/src/lib.rs
git commit -m "feat(render): render request wire format and book manifest"
```

---

## Task 11: Pre-flight

**Files:**
- Create: `src-tauri/src/book/preflight.rs`
- Modify: `src-tauri/src/book/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum Severity { Block, Warn }` (deriving `PartialEq`, `Debug`)
  - `pub struct Finding { pub severity: Severity, pub page: u32, pub photo_path: String, pub message: String }` (deriving `PartialEq`, `Debug`)
  - `pub fn preflight(book: &Book, photos: &[Photo], output_dir: &Path) -> Vec<Finding>`
  - `pub fn preflight_with_bleed(book: &Book, photos: &[Photo], output_dir: &Path, bleed: &[Vec<BleedEdge>]) -> Vec<Finding>`
  - `pub fn preflight_with_space(book: &Book, photos: &[Photo], output_dir: &Path, available_bytes: u64) -> Vec<Finding>`
  - `pub const EST_BYTES_PER_PLACEMENT: u64 = 3_000_000`

- [ ] **Step 1: Write the failing tests**

One test per row of spec §5.6, each triggering exactly one condition. Create `src-tauri/src/book/preflight.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::cull::{Face, PaletteColor};
    use crate::book::pace::{Book, Page, Placement};
    use crate::geometry::{Side, PAGE_W_IN, TRIM_U, TRIM_V};

    /// 3:2, never square -- a square hides every aspect-dependent bug.
    fn photo(w: u32, h: u32) -> Photo {
        Photo {
            path: "/p/a.jpg".into(), hash: "h".into(), width: w, height: h,
            is_utility: false, aesthetic_pct: 50, sharpness_pct: 50,
            near_dup_cluster: 0, event_cluster: 0,
            faces: Vec::new(), face_area_fraction: 0.0, saliency_box: None,
            palette: Vec::<PaletteColor>::new(), capture_quality: None,
        }
    }

    /// A slot comfortably inside the trim on a LEFT page, with a crop that
    /// keeps the whole frame. Callers mutate one thing at a time.
    fn book_with(slot: Rect, crop: Rect, side: Side) -> Book {
        Book {
            seed: 1,
            dropped: 0,
            pages: vec![Page {
                number: 4,
                side,
                template_id: "fx".into(),
                placements: vec![Placement { photo_index: 0, slot_rect: slot, crop, z: 1 }],
            }],
        }
    }

    fn clean_slot() -> Rect {
        Rect::new(0.10, 0.10, 0.50, 0.40)
    }

    fn full_crop() -> Rect {
        Rect::new(0.0, 0.0, 1.0, 1.0)
    }

    /// Pixels needed for a given DPI in `clean_slot`, so DPI tests hit the
    /// boundary exactly rather than somewhere near it.
    fn px_for_dpi(dpi: f64) -> u32 {
        (dpi * clean_slot().w * PAGE_W_IN).round() as u32
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn preflight_returns_no_findings_for_a_clean_book() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into(); // exists, so the missing-source check passes
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        assert_eq!(preflight(&book, &[p], dir.path()), Vec::new());
    }

    #[test]
    fn preflight_blocks_a_face_outside_the_trim_rectangle() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        // Face at the very top of the frame, in a slot flush to the top
        // bleed -- so it lands above the trim line.
        p.faces = vec![Face { box_: Rect::new(0.4, 0.0, 0.1, 0.02), capture_quality: Some(0.8) }];
        let book = book_with(Rect::new(0.10, 0.0, 0.50, 0.40), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block
                && f.message.contains("trim")),
            "got {findings:?}"
        );
    }

    #[test]
    fn preflight_blocks_a_face_inside_the_gutter_dead_strip() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        p.faces = vec![Face { box_: Rect::new(0.95, 0.4, 0.05, 0.1), capture_quality: Some(0.8) }];
        // Slot running flush to the fold on a LEFT page.
        let book = book_with(Rect::new(0.5, 0.2, 0.5, 0.4), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(
            findings.iter().any(|f| f.severity == Severity::Block
                && f.message.contains("gutter")),
            "got {findings:?}"
        );
    }

    /// The counterpart to the scorer's rule: generic saliency in the dead
    /// strip WARNS, a face BLOCKS. Conflating them makes fold-flush layouts
    /// impossible, which is the look the user explicitly wants available.
    #[test]
    fn preflight_warns_on_salient_content_in_the_dead_strip_without_blocking() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        p.saliency_box = Some(Rect::new(0.95, 0.4, 0.05, 0.1));
        let book = book_with(Rect::new(0.5, 0.2, 0.5, 0.4), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(findings.iter().all(|f| f.severity != Severity::Block), "got {findings:?}");
        assert!(findings.iter().any(|f| f.severity == Severity::Warn));
    }

    #[test]
    fn preflight_blocks_a_photo_below_the_dpi_floor() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(100.0), px_for_dpi(100.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("150")), "got {findings:?}");
    }

    #[test]
    fn preflight_warns_between_150_and_300_dpi_and_reports_the_effective_value() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(200.0), px_for_dpi(200.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        let warn = findings.iter().find(|f| f.severity == Severity::Warn)
            .expect("expected a DPI warning");
        // A warning that does not say HOW soft the photo is cannot be acted on.
        assert!(warn.message.contains("200"), "message was {:?}", warn.message);
    }

    /// Boundary AT the boundary: exactly 300 DPI must be silent.
    #[test]
    fn preflight_is_silent_at_exactly_three_hundred_dpi() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(300.0), px_for_dpi(300.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        assert_eq!(preflight(&book, &[p], dir.path()), Vec::new());
    }

    #[test]
    fn preflight_blocks_a_source_file_that_no_longer_exists() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/definitely/not/here.jpg".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        let findings = preflight(&book, &[p], dir.path());
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("no longer exists")), "got {findings:?}");
    }

    #[test]
    fn preflight_blocks_a_bleed_slot_that_stops_short_of_the_canvas_edge() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        // Declared as reaching the left edge but starting at 0.01.
        let mut book = book_with(Rect::new(0.01, 0.0, 0.5, 1.0), full_crop(), Side::Left);
        book.pages[0].placements[0].slot_rect = Rect::new(0.01, 0.0, 0.5, 1.0);
        let findings = preflight_with_bleed(
            &book, &[p], dir.path(), &[vec![crate::geometry::BleedEdge::Left]],
        );
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("bleed")), "got {findings:?}");
    }

    /// This is why export fails on page one rather than after nineteen.
    #[test]
    fn preflight_blocks_when_the_estimate_exceeds_free_disk_space() {
        let dir = tempdir();
        let mut p = photo(px_for_dpi(400.0), px_for_dpi(400.0) * 2 / 3);
        p.path = "/dev/null".into();
        let book = book_with(clean_slot(), full_crop(), Side::Left);
        // One placement against zero available bytes.
        let findings = preflight_with_space(&book, &[p], dir.path(), 0);
        assert!(findings.iter().any(|f| f.severity == Severity::Block
            && f.message.contains("disk")), "got {findings:?}");
    }
}
```

Note the two seam functions the tests reach for: `preflight_with_bleed` (bleed edges passed in, since `Placement` carries a rect but not the slot's bleed array) and `preflight_with_space` (available bytes injected, so the disk test does not depend on the machine it runs on). `preflight` itself is the thin wrapper that reads both from the real world — the same "extract the pure function" pattern the codebase uses five times already.

- [ ] **Step 2: Run, confirm failure, implement, run**

```bash
cargo test --manifest-path src-tauri/Cargo.toml preflight
```

Disk-space estimation: PNG output is roughly the source photo's decoded size compressed; estimate 3 MB per placement and compare against available space via `std::fs` metadata on the output volume.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/book/preflight.rs src-tauri/src/book/mod.rs
git commit -m "feat(book): pre-flight blocks and warnings before any file is written"
```

---

## Task 12: The `generate_book` command and UI

**Files:**
- Create: `app/types/book.ts`, `app/composables/useBook.ts`, `app/components/GenerateBook.vue`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `app/pages/index.vue`

**Interfaces:**
- Produces:
  - Rust: `#[tauri::command] pub async fn recommend_book(app: AppHandle) -> Result<BookRecommendation, String>` and `pub async fn generate_book(app: AppHandle, pages: u32, output_dir: String, on_event: Channel<BookEvent>) -> Result<BookResult, String>`
  - TypeScript: `interface BookRecommendation { pages: number; keeperCount: number; dropped: number }`, `interface PreflightFinding { severity: "block" | "warn"; page: number; photoPath: string; message: string }`, `interface BookResult { manifestPath: string; written: number; findings: PreflightFinding[] }`

- [ ] **Step 1: Write the failing TypeScript tests**

Create `tests/book.test.ts` covering the pure helpers in `app/types/book.ts`: separating findings into blocks and warnings, and formatting the recommendation sentence. Follow `tests/features.test.ts`'s style. **No `any`.**

- [ ] **Step 2: Run, confirm failure, implement, run**

```bash
bun run test tests/book.test.ts && bun run lint
```

- [ ] **Step 3b: Pin the Rust↔TypeScript key names**

Spec §5.8: `AnalysisSummary.photos` is `Vec<serde_json::Value>` with no compile-time link to the TypeScript types, so a field rename on either side is a silent `undefined`. Phase 2 widens that boundary considerably. Full codegen is out of scope; pinning the names is not. Add to `src-tauri/src/commands.rs`'s test module, mirroring how `protocol.rs` already pins the Swift wire format:

```rust
    /// Every key `app/types/book.ts` reads, asserted against what Rust
    /// actually serialises. There is no shared schema, so a rename on
    /// either side would otherwise surface as an `undefined` in the UI with
    /// no error anywhere -- the failure mode `PROJECT-STATUS.md` names.
    #[test]
    fn book_result_wire_keys_match_the_typescript_interface() {
        let result = BookResult {
            manifest_path: "/out/manifest.json".into(),
            written: 12,
            findings: vec![crate::book::preflight::Finding {
                severity: crate::book::preflight::Severity::Warn,
                page: 4,
                photo_path: "/p/a.jpg".into(),
                message: "effective 212 DPI".into(),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        for key in ["manifestPath", "written", "findings",
                    "severity", "page", "photoPath", "message"] {
            assert!(json.contains(&format!("\"{key}\"")), "missing {key} in {json}");
        }
        assert!(json.contains("\"warn\""), "severity must serialise lowercase");
        assert!(!json.contains("manifest_path"), "snake_case leaked to the webview");
    }

    #[test]
    fn book_recommendation_wire_keys_match_the_typescript_interface() {
        let rec = BookRecommendation { pages: 20, keeper_count: 48, dropped: 3 };
        let json = serde_json::to_string(&rec).unwrap();
        for key in ["pages", "keeperCount", "dropped"] {
            assert!(json.contains(&format!("\"{key}\"")), "missing {key} in {json}");
        }
    }
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml wire_keys`
Expected: FAIL first (types do not exist), then PASS once Step 3 defines them with `#[serde(rename_all = "camelCase")]`.

- [ ] **Step 3: Implement the Rust commands**

`recommend_book` culls the cached analysis and returns `recommend_pages`. `generate_book` assembles, pre-flights, and **returns early with findings if any are `Block`** — writing nothing. Otherwise it creates the output directory, sends the render request through the existing `SidecarPool` inside `spawn_blocking` (`Sidecar::request` blocks and will otherwise starve the tokio worker its own stdout-drain task needs), writes `manifest.json`, and streams progress over a `Channel`.

Register both in `lib.rs`:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::analyze_folder,
            commands::recommend_book,
            commands::generate_book
        ])
```

- [ ] **Step 4: Build the UI**

`GenerateBook.vue` shows the recommended length with the drop count, a length override, an output-folder picker (`open({ directory: true })`), the pre-flight report with blocks listed separately from warnings, a progress bar, and **Reveal in Finder** on completion. Use `useTemplateRef()` over manually typed refs, prop shorthand where the name matches, and destructuring defaults on `defineProps` — not `withDefaults`.

- [ ] **Step 5: Run everything**

```bash
bun run test && bun run test:rust && bun run lint
```

- [ ] **Step 6: Commit**

```bash
git add app/ src-tauri/src/commands.rs src-tauri/src/lib.rs tests/book.test.ts
git commit -m "feat(ui): generate-book step with pre-flight report"
```

---

## Task 13: Author the missing templates

**Files:**
- Create: `scripts/template-sheet.ts`, ~15–20 files under `templates/`

**Interfaces:**
- Consumes: the validator from Task 1.
- Produces: a library where `buildable_sizes()` covers 1–6 with no gap, and both edge treatments are well represented.

- [ ] **Step 1: Build the visual contact sheet**

`scripts/template-sheet.ts` reads every file in `templates/`, and emits a single self-contained HTML page drawing each template to scale: the spread canvas, the fold, the gutter dead strip, the trim rectangle, every slot with its `aspect_pref` label, and bleed edges marked. Run with `bun scripts/template-sheet.ts > /tmp/templates.html`.

- [ ] **Step 2: Review the existing 19 on the sheet**

Open it. Confirm the 12 fully-usable templates look right and identify which of the 7 partial ones can be salvaged by adjusting slot rects (spec §6.3).

- [ ] **Step 3: Author to the page-subdivision rule**

The rule from spec §6.2 — a page is 1.259:1, so cells matching real photos come from subdividing a **page**, not the spread:

| Subdivision | Cell aspect | Suits |
|---|---|---|
| 1 photo per page | 1.26 | 4:3 and 3:2 landscape |
| 2×2 on one page | ~1.29 | 4:3 landscape |
| 2 side by side on a page | 0.63 | 3:4 and 2:3 portrait |
| 3×3 on one page | ~1.26 | 4:3, dense |
| 2 stacked on a page | 2.52 | genuinely wide photos only |

Cover, at minimum: 4-photo spreads, **5-photo spreads** (currently zero exist, so the packer cannot emit a group of five), 6-photo spreads, dense templates (currently zero usable), fold-flush and full-bleed layouts (currently **zero** slots touch the fold and only 10 of 71 declare bleed — the user wants both looks alternating), and some lively energy.

Remember `aspect_pref` is the **real-world inch ratio** of the slot, computed on the SPREAD canvas since that is the authoring format: `(w * 22.394) / (h * 8.894)`.

- [ ] **Step 4: Validate and review**

```bash
bun run test tests/templates.test.ts
bun scripts/template-sheet.ts > /tmp/templates.html && open /tmp/templates.html
```

Present the sheet for approval. Redraw anything rejected. Do not proceed until approved.

- [ ] **Step 5: Confirm the gaps are closed**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored real_library
```

Extend `templates_real_library_decomposes_cleanly` to assert `buildable_sizes` contains 1 through 6 with no gap, and that at least a quarter of page layouts are `EdgeTreatment::Bleed`.

- [ ] **Step 6: Commit**

```bash
git add templates/ scripts/template-sheet.ts src-tauri/src/templates.rs
git commit -m "feat(templates): author page-subdivision layouts closing the 4/5/6-up gaps"
```

---

## Task 14: End-to-end verification

**Files:**
- Create: `src-tauri/tests/render_roundtrip.rs`
- Modify: `docs/PROJECT-STATUS.md`

- [ ] **Step 1: Write the end-to-end test**

Drives the **real sidecar binary** over stdin with a genuine render request — the sidecar is a standalone CLI, so this costs almost nothing and is the only test that proves the wire format works against the actual process rather than a fixture of it.

```rust
use photobook_gen_lib::protocol::{Request, RequestKind, Response, ResponseResult};
use photobook_gen_lib::render::{RenderPlacement, RenderRequest};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Builds a real render request, pipes it to the real sidecar binary over
/// stdin, and asserts a transparent PNG of exactly the page dimensions
/// lands on disk.
///
/// Every other test in this plan exercises ONE side of the wire against a
/// fixture of the other. This is the only one where a key-name disagreement
/// between `protocol.rs` and `Renderer.swift` actually fails, which is the
/// failure mode that otherwise degrades into "every photo failed" with no
/// cause reported anywhere.
#[test]
fn render_roundtrip_writes_a_transparent_page_png() {
    let binary = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join("photobook-engine-aarch64-apple-darwin");
    assert!(binary.exists(), "run `bun run sidecar` first: {binary:?}");

    let dir = tempfile::tempdir().unwrap();
    // 1200x800 -- 3:2, deliberately NOT square.
    let src = write_fixture_jpeg(dir.path(), 1200, 800);

    let request = Request {
        id: "rt1".into(),
        kind: RequestKind::Render,
        paths: None,
        thumbnail_dir: None,
        render: Some(RenderRequest {
            output_dir: dir.path().to_string_lossy().into_owned(),
            page_width_px: 3359,
            page_height_px: 2668,
            placements: vec![RenderPlacement {
                source_path: src.to_string_lossy().into_owned(),
                crop_x: 0.0, crop_y: 0.0, crop_w: 1.0, crop_h: 1.0,
                dest_x: 0.25, dest_y: 0.25, dest_w: 0.5, dest_h: 0.5,
                filename: "p01-z1-roundtrip.png".into(),
            }],
        }),
    };

    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sidecar");

    {
        let stdin = child.stdin.as_mut().unwrap();
        let line = serde_json::to_string(&request).unwrap();
        assert!(!line.contains('\n'), "NDJSON requires one line per request");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).expect("sidecar produced no response");
    let response: Response = serde_json::from_str(&line)
        .unwrap_or_else(|e| panic!("malformed response {line:?}: {e}"));

    let ResponseResult::Rendered(records) = response.result else {
        panic!("expected rendered, got {:?}", response.result);
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["status"], "ok", "record was {:?}", records[0]);

    let png = dir.path().join("p01-z1-roundtrip.png");
    assert!(png.exists(), "no PNG written");
    let (w, h, corner_alpha) = read_png_dims_and_corner_alpha(&png);
    assert_eq!((w, h), (3359, 2668));
    assert_eq!(corner_alpha, 0, "the area outside the slot must be transparent");

    let _ = child.kill();
}
```

Write the two helpers in the same file: `write_fixture_jpeg(dir, w, h) -> PathBuf` (never square — a square hides every aspect bug) and `read_png_dims_and_corner_alpha(path) -> (u32, u32, u8)`. Add `image = "0.25"` to `[dev-dependencies]` for the PNG read, or shell out to `sips -g pixelWidth -g pixelHeight` plus a tiny raw-pixel read if pulling in a decoder is unwelcome.

Confirm the crate name in the `use` lines against `src-tauri/Cargo.toml`'s `[lib] name` before running — the existing integration tests under `src-tauri/tests/` already import it, so copy the spelling from there.

- [ ] **Step 2: Run**

```bash
bun run sidecar && cargo test --manifest-path src-tauri/Cargo.toml render_roundtrip
```

- [ ] **Step 3: Run everything**

```bash
bun run sidecar
bun run test
bun run test:rust
bun run test:swift
bun run lint
bun tauri build --bundles app
```

All must pass. Paste the counts into the commit body. If any suite regressed against Phase 1's baseline (425 TypeScript, 74 Rust, 71 Swift), find out why before proceeding.

- [ ] **Step 4: Generate a real book**

Run the app against a real photo folder, generate a book, and upload one page's PNGs to Pixajoy. **This is the only step that proves Phase 2 works**; every test above proves only that the parts behave as specified.

- [ ] **Step 5: Update `PROJECT-STATUS.md`**

Move Phase 2 from "not built" to complete. Record: the resolved page-structure question, that `ExifReader` already handles orientations 5–8 (spec §8 item 2, resolved during planning), the measured Pixajoy alpha behaviour from Task 0, the final template count, and any new trap found during implementation. Add the Phase 2 test counts.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/tests/render_roundtrip.rs docs/PROJECT-STATUS.md
git commit -m "test: end-to-end render round-trip against the real sidecar"
```

---

## Deferred, deliberately

Out of scope for this plan, each needing its own spec:

- **Multiple source folders** — a design change to ranking population semantics, not a flag.
- **The cover** — different geometry (wrap band plus a page-count-dependent spine).
- **Text rendering** — text zones are whitespace in Phase 2.
- **Preview UI** (Phase 3), **canvas editor** (Phase 4), **agent layer** (Phase 5).
- **Smile calibration** — `smile_fraction` stays excluded from culling.
- **Full codegen across the Rust/TypeScript boundary** — `AnalysisSummary.photos` is still `Vec<serde_json::Value>`. Spec §5.8 says fix this in Phase 2; Task 12 Step 3b does the proportionate version (a Rust test pinning every key name the TypeScript reads, matching how `protocol.rs` already pins the Swift wire format), not a codegen pipeline. A real generator is still worth doing and is deferred deliberately, not forgotten.
