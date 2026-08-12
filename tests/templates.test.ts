import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// ---------------------------------------------------------------------------
// Types (mirrors docs/superpowers/specs/2026-08-12-photobook-generator-design.md
// section 7.2 — the template format). No `any` anywhere: every JSON file is
// parsed as `unknown` and narrowed through a hand-written type guard so a
// malformed template fails with a clear assertion instead of a silent `any`.
// ---------------------------------------------------------------------------

type Rect = [x: number, y: number, width: number, height: number];

type Role = "hero" | "support";

type BleedEdge = "left" | "right" | "top" | "bottom";

type Density = "sparse" | "medium" | "dense";

type Energy = "calm" | "neutral" | "lively";

type Align = "left" | "center" | "right";

interface Slot {
  rect: Rect;
  role: Role;
  bleed: BleedEdge[];
  aspect_pref: [number, number];
}

interface TextZone {
  rect: Rect;
  align: Align;
}

interface Template {
  id: string;
  slots: Slot[];
  text_zones: TextZone[];
  min_photos: number;
  max_photos: number;
  density: Density;
  energy: Energy;
}

const ROLES: readonly Role[] = ["hero", "support"];
const BLEED_EDGES: readonly BleedEdge[] = ["left", "right", "top", "bottom"];
const DENSITIES: readonly Density[] = ["sparse", "medium", "dense"];
const ENERGIES: readonly Energy[] = ["calm", "neutral", "lively"];
const ALIGNS: readonly Align[] = ["left", "center", "right"];

function isRect(value: unknown): value is Rect {
  return (
    Array.isArray(value) &&
    value.length === 4 &&
    value.every((n) => typeof n === "number" && Number.isFinite(n))
  );
}

function isSlot(value: unknown): value is Slot {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    isRect(v.rect) &&
    typeof v.role === "string" &&
    ROLES.includes(v.role as Role) &&
    Array.isArray(v.bleed) &&
    v.bleed.every((e) => typeof e === "string" && BLEED_EDGES.includes(e as BleedEdge)) &&
    Array.isArray(v.aspect_pref) &&
    v.aspect_pref.length === 2 &&
    v.aspect_pref.every((n) => typeof n === "number" && Number.isFinite(n))
  );
}

function isTextZone(value: unknown): value is TextZone {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return isRect(v.rect) && typeof v.align === "string" && ALIGNS.includes(v.align as Align);
}

function isTemplate(value: unknown): value is Template {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.id === "string" &&
    Array.isArray(v.slots) &&
    v.slots.every(isSlot) &&
    Array.isArray(v.text_zones) &&
    v.text_zones.every(isTextZone) &&
    typeof v.min_photos === "number" &&
    typeof v.max_photos === "number" &&
    typeof v.density === "string" &&
    DENSITIES.includes(v.density as Density) &&
    typeof v.energy === "string" &&
    ENERGIES.includes(v.energy as Energy)
  );
}

// ---------------------------------------------------------------------------
// Geometry constants — the spread canvas, normalised [0,1], origin top-left.
// Derived in docs/superpowers/specs/2026-08-12-photobook-generator-design.md
// section 3. Do not round these differently than the design doc; the values
// below are exact to 6 decimal places, matching the constants in the task.
// ---------------------------------------------------------------------------

const SAFE_X0 = 0.008797;
const SAFE_X1 = 0.991203;
const SAFE_Y0 = 0.02215;
const SAFE_Y1 = 0.97785;
const GUTTER_X0 = 0.491203;
const GUTTER_X1 = 0.508797;

// The spread canvas in inches (section 3 of the design doc): 22.394 x 8.894, a
// 2.518:1 ratio. `rect` is normalised to the canvas ([0,1] on each axis), but
// `aspect_pref` is a real-world (inches) width/height ratio — the thing a photo's
// own aspect ratio is measured in. Converting a slot's normalised w/h into a real
// aspect requires multiplying by (CANVAS_WIDTH_IN / CANVAS_HEIGHT_IN), because the
// canvas itself is wide, not square: a slot that is "half as tall as it is wide" in
// normalised units is not 2:1 in real inches, it's 2.518x that. Do not "simplify"
// this away by comparing aspect_pref against the raw normalised rect ratio — that
// silently mis-scores every slot once Phase 2's scoring engine compares aspect_pref
// against a photo's real aspect ratio.
const CANVAS_WIDTH_IN = 22.394;
const CANVAS_HEIGHT_IN = 8.894;

/** Tolerance for float rounding in authored JSON. Not a design allowance. */
const EPS = 0.0005;

interface LoadedTemplate {
  filename: string;
  template: Template;
}

const TEMPLATES_DIR = path.resolve(import.meta.dirname, "../templates");

function loadTemplates(): LoadedTemplate[] {
  const files = readdirSync(TEMPLATES_DIR).filter((f) => f.endsWith(".json"));
  return files.map((filename) => {
    const raw: unknown = JSON.parse(readFileSync(path.join(TEMPLATES_DIR, filename), "utf-8"));
    if (!isTemplate(raw)) {
      throw new Error(`${filename}: does not conform to the Template shape`);
    }
    return { filename, template: raw };
  });
}

function rectLabel(filename: string, kind: string, index: number, rect: Rect): string {
  return `${filename} ${kind}[${index}] rect=[${rect.join(", ")}]`;
}

function rectsOverlap(a: Rect, b: Rect): boolean {
  const [ax, ay, aw, ah] = a;
  const [bx, by, bw, bh] = b;
  const overlapX = Math.min(ax + aw, bx + bw) - Math.max(ax, bx);
  const overlapY = Math.min(ay + ah, by + bh) - Math.max(ay, by);
  return overlapX > EPS && overlapY > EPS;
}

function rectOverlapsGutterBand(rect: Rect): boolean {
  const [x, , w] = rect;
  const x1 = x + w;
  // A vertical strip spanning the full height of the canvas. Any rect whose
  // x-range intersects [GUTTER_X0, GUTTER_X1] overlaps the band, regardless
  // of its y-range.
  return x1 > GUTTER_X0 + EPS && x < GUTTER_X1 - EPS;
}

/** A slot's real-world (inches) width/height ratio — see the comment on
 * CANVAS_WIDTH_IN/CANVAS_HEIGHT_IN above for why this isn't just `w / h`. */
function realAspectRatio(rect: Rect): number {
  const [, , w, h] = rect;
  return (w * CANVAS_WIDTH_IN) / (h * CANVAS_HEIGHT_IN);
}

const loaded = loadTemplates();

describe("template library", () => {
  it("contains between 35 and 50 templates", () => {
    expect(loaded.length).toBeGreaterThanOrEqual(35);
    expect(loaded.length).toBeLessThanOrEqual(50);
  });

  it("has unique ids across the library", () => {
    const ids = loaded.map((l) => l.template.id);
    const duplicates = ids.filter((id, i) => ids.indexOf(id) !== i);
    expect(duplicates, `duplicate template ids: ${duplicates.join(", ")}`).toEqual([]);
  });

  for (const { filename, template } of loaded) {
    describe(filename, () => {
      it("id matches the filename slug", () => {
        const slug = filename.replace(/\.json$/, "");
        expect(template.id).toBe(slug);
      });

      it("every rect is well-formed with width and height > 0", () => {
        const allRects: Array<{ kind: string; index: number; rect: Rect }> = [
          ...template.slots.map((s, i) => ({ kind: "slots", index: i, rect: s.rect })),
          ...template.text_zones.map((tz, i) => ({ kind: "text_zones", index: i, rect: tz.rect })),
        ];
        for (const { kind, index, rect } of allRects) {
          const [x, y, w, h] = rect;
          const label = rectLabel(filename, kind, index, rect);
          expect(w, `${label}: width must be > 0`).toBeGreaterThan(0);
          expect(h, `${label}: height must be > 0`).toBeGreaterThan(0);
          expect(Number.isFinite(x), `${label}: x must be finite`).toBe(true);
          expect(Number.isFinite(y), `${label}: y must be finite`).toBe(true);
        }
      });

      it("every text zone lies entirely within the safe area", () => {
        template.text_zones.forEach((tz, i) => {
          const [x, y, w, h] = tz.rect;
          const label = rectLabel(filename, "text_zones", i, tz.rect);
          expect(x, `${label}: left edge must be >= safe area x0`).toBeGreaterThanOrEqual(
            SAFE_X0 - EPS,
          );
          expect(x + w, `${label}: right edge must be <= safe area x1`).toBeLessThanOrEqual(
            SAFE_X1 + EPS,
          );
          expect(y, `${label}: top edge must be >= safe area y0`).toBeGreaterThanOrEqual(
            SAFE_Y0 - EPS,
          );
          expect(y + h, `${label}: bottom edge must be <= safe area y1`).toBeLessThanOrEqual(
            SAFE_Y1 + EPS,
          );
        });
      });

      it("no text zone overlaps the gutter dead band", () => {
        template.text_zones.forEach((tz, i) => {
          const label = rectLabel(filename, "text_zones", i, tz.rect);
          expect(
            rectOverlapsGutterBand(tz.rect),
            `${label}: overlaps the gutter dead band [${GUTTER_X0}, ${GUTTER_X1}]`,
          ).toBe(false);
        });
      });

      it("every edge named in a slot's bleed array reaches the canvas boundary", () => {
        template.slots.forEach((slot, i) => {
          const [x, y, w, h] = slot.rect;
          const label = rectLabel(filename, "slots", i, slot.rect);
          for (const edge of slot.bleed) {
            if (edge === "left") {
              expect(x, `${label}: bleed 'left' must reach x <= 0`).toBeLessThanOrEqual(EPS);
            } else if (edge === "right") {
              expect(x + w, `${label}: bleed 'right' must reach x+w >= 1`).toBeGreaterThanOrEqual(
                1 - EPS,
              );
            } else if (edge === "top") {
              expect(y, `${label}: bleed 'top' must reach y <= 0`).toBeLessThanOrEqual(EPS);
            } else if (edge === "bottom") {
              expect(
                y + h,
                `${label}: bleed 'bottom' must reach y+h >= 1`,
              ).toBeGreaterThanOrEqual(1 - EPS);
            }
          }
        });
      });

      it("a slot not declaring a bleed edge stays inside the canvas on that edge", () => {
        template.slots.forEach((slot, i) => {
          const [x, y, w, h] = slot.rect;
          const label = rectLabel(filename, "slots", i, slot.rect);
          if (!slot.bleed.includes("left")) {
            expect(x, `${label}: left edge (no bleed) must be >= 0`).toBeGreaterThanOrEqual(-EPS);
          }
          if (!slot.bleed.includes("right")) {
            expect(x + w, `${label}: right edge (no bleed) must be <= 1`).toBeLessThanOrEqual(
              1 + EPS,
            );
          }
          if (!slot.bleed.includes("top")) {
            expect(y, `${label}: top edge (no bleed) must be >= 0`).toBeGreaterThanOrEqual(-EPS);
          }
          if (!slot.bleed.includes("bottom")) {
            expect(y + h, `${label}: bottom edge (no bleed) must be <= 1`).toBeLessThanOrEqual(
              1 + EPS,
            );
          }
        });
      });

      it("aspect_pref is a valid, positive range describing the slot's own real-world shape", () => {
        template.slots.forEach((slot, i) => {
          const label = rectLabel(filename, "slots", i, slot.rect);
          const [lo, hi] = slot.aspect_pref;
          expect(lo, `${label}: aspect_pref[0] must be > 0`).toBeGreaterThan(0);
          expect(hi, `${label}: aspect_pref[1] must be > 0`).toBeGreaterThan(0);
          expect(lo, `${label}: aspect_pref[0] must be < aspect_pref[1]`).toBeLessThan(hi);

          const real = realAspectRatio(slot.rect);
          expect(
            real,
            `${label}: real-world aspect ratio ${real.toFixed(3)} falls outside its own ` +
              `declared aspect_pref [${lo}, ${hi}] (aspect_pref is real-world w/h, not the ` +
              `rect's normalised ratio — see CANVAS_WIDTH_IN comment above)`,
          ).toBeGreaterThanOrEqual(lo - EPS);
          expect(real, `${label}: real-world aspect ratio above aspect_pref[1]`).toBeLessThanOrEqual(
            hi + EPS,
          );
        });
      });

      it("min_photos <= max_photos, and slot count is consistent", () => {
        expect(
          template.min_photos,
          `${filename}: min_photos must be <= max_photos`,
        ).toBeLessThanOrEqual(template.max_photos);
        expect(
          template.slots.length,
          `${filename}: slot count must be >= min_photos`,
        ).toBeGreaterThanOrEqual(template.min_photos);
        expect(
          template.slots.length,
          `${filename}: slot count must be <= max_photos`,
        ).toBeLessThanOrEqual(template.max_photos);
      });

      it("density and energy are among the allowed values", () => {
        expect(DENSITIES, `${filename}: density='${template.density}'`).toContain(
          template.density,
        );
        expect(ENERGIES, `${filename}: energy='${template.energy}'`).toContain(template.energy);
      });

      it("slots do not overlap each other by more than a negligible epsilon", () => {
        for (let i = 0; i < template.slots.length; i++) {
          for (let j = i + 1; j < template.slots.length; j++) {
            const a = template.slots[i]!;
            const b = template.slots[j]!;
            expect(
              rectsOverlap(a.rect, b.rect),
              `${filename}: slots[${i}] rect=[${a.rect.join(", ")}] overlaps ` +
                `slots[${j}] rect=[${b.rect.join(", ")}]`,
            ).toBe(false);
          }
        }
      });
    });
  }
});
