/**
 * Visual contact sheet for the spread template library.
 *
 *   bun scripts/template-sheet.ts > /tmp/templates.html && open /tmp/templates.html
 *
 * Emits ONE self-contained HTML page (no external assets, no script tags) that
 * draws every file in `templates/` to scale on the 22.394" x 8.894" spread
 * canvas: the fold, the gutter dead band, the trim/safe rectangle, every slot
 * with its real-world aspect and declared `aspect_pref`, and every bleed edge.
 *
 * Why it exists: the library's failure mode is invisible in JSON. A "2x3 grid"
 * authored on the 2.518:1 spread canvas produces 3.6:1 letterbox bands that no
 * 4:3 photo can fill, and reading rect arrays will not tell you that. Drawing
 * them will.
 */

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";

// --- geometry, mirroring tests/templates.test.ts and src-tauri/src/geometry.rs

const CANVAS_W_IN = 22.394;
const CANVAS_H_IN = 8.894;
const CANVAS_RATIO = CANVAS_W_IN / CANVAS_H_IN;

const SAFE_X0 = 0.008797;
const SAFE_X1 = 0.991203;
const SAFE_Y0 = 0.02215;
const SAFE_Y1 = 0.97785;
const GUTTER_X0 = 0.491203;
const GUTTER_X1 = 0.508797;
const FOLD_X = 0.5;

/**
 * The aspect ratios a consumer camera actually produces. A slot that matches
 * none of them can only be filled by throwing most of the frame away, and
 * because a clipped face is a HARD rejection in the scorer, such a slot is
 * inert for any photo containing a person.
 */
const COMMON_ASPECTS: ReadonlyArray<[label: string, ratio: number]> = [
  ["4:3", 4 / 3],
  ["3:2", 3 / 2],
  ["3:4", 3 / 4],
  ["2:3", 2 / 3],
];

/**
 * Retention threshold below which a slot counts as unusable. 0.80 means the
 * best-matching common aspect still loses more than 20% of the frame. This is
 * the same cut that produces the spec's "12 of 19 fully usable" count, so the
 * sheet's verdicts and the spec's arithmetic agree.
 */
const FIT_THRESHOLD = 0.8;

type Rect = [x: number, y: number, width: number, height: number];
type BleedEdge = "left" | "right" | "top" | "bottom";

interface Slot {
  rect: Rect;
  role: "hero" | "support";
  bleed: BleedEdge[];
  aspect_pref: [number, number];
}

interface TextZone {
  rect: Rect;
  align: "left" | "center" | "right";
}

interface Template {
  id: string;
  slots: Slot[];
  text_zones: TextZone[];
  min_photos: number;
  max_photos: number;
  density: "sparse" | "medium" | "dense";
  energy: "calm" | "neutral" | "lively";
}

const BLEED_EDGES: readonly string[] = ["left", "right", "top", "bottom"];

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
    (v.role === "hero" || v.role === "support") &&
    Array.isArray(v.bleed) &&
    v.bleed.every((e) => typeof e === "string" && BLEED_EDGES.includes(e)) &&
    Array.isArray(v.aspect_pref) &&
    v.aspect_pref.length === 2 &&
    v.aspect_pref.every((n) => typeof n === "number" && Number.isFinite(n))
  );
}

function isTextZone(value: unknown): value is TextZone {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return isRect(v.rect) && (v.align === "left" || v.align === "center" || v.align === "right");
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
    (v.density === "sparse" || v.density === "medium" || v.density === "dense") &&
    (v.energy === "calm" || v.energy === "neutral" || v.energy === "lively")
  );
}

const TEMPLATES_DIR = path.resolve(import.meta.dirname, "../templates");
const NOT_A_TEMPLATE = new Set(["weights.json"]);

function loadTemplates(): Template[] {
  return readdirSync(TEMPLATES_DIR)
    .filter((f) => f.endsWith(".json") && !NOT_A_TEMPLATE.has(f))
    .toSorted()
    .map((filename) => {
      const raw: unknown = JSON.parse(readFileSync(path.join(TEMPLATES_DIR, filename), "utf-8"));
      if (!isTemplate(raw)) throw new Error(`${filename}: does not conform to the Template shape`);
      return raw;
    });
}

// --- derived facts

/** Real-world inch ratio of a spread-normalised rect. NOT `w / h`. */
function realAspect(rect: Rect): number {
  const [, , w, h] = rect;
  return (w * CANVAS_W_IN) / (h * CANVAS_H_IN);
}

interface Fit {
  label: string;
  retention: number;
  ok: boolean;
}

/** The common camera aspect this slot suits best, and how much frame survives. */
function bestFit(rect: Rect): Fit {
  const target = realAspect(rect);
  let best: [string, number] = ["-", 0];
  for (const [label, ratio] of COMMON_ASPECTS) {
    const retention = target > ratio ? ratio / target : target / ratio;
    if (retention > best[1]) best = [label, retention];
  }
  return { label: best[0], retention: best[1], ok: best[1] >= FIT_THRESHOLD };
}

type Side = "left" | "right" | "fold";

function sideOf(rect: Rect): Side {
  const [x, , w] = rect;
  if (x + w <= FOLD_X + 1e-6) return "left";
  if (x >= FOLD_X - 1e-6) return "right";
  return "fold";
}

/** Mirrors `edge_treatment` in src-tauri/src/templates.rs: derived, not authored. */
function edgeTreatment(slots: Slot[]): "bleed" | "margin" {
  return slots.some((s) => s.bleed.length > 0) ? "bleed" : "margin";
}

function touchesFold(rect: Rect): boolean {
  const [x, , w] = rect;
  return Math.abs(x - FOLD_X) < 1e-6 || Math.abs(x + w - FOLD_X) < 1e-6;
}

// --- rendering

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function pct(n: number): string {
  return `${(n * 100).toFixed(4)}%`;
}

function slotBox(slot: Slot, index: number): string {
  const [x, y, w, h] = slot.rect;
  const aspect = realAspect(slot.rect);
  const fit = bestFit(slot.rect);
  const [lo, hi] = slot.aspect_pref;
  const classes = [
    "slot",
    slot.role === "hero" ? "hero" : "support",
    fit.ok ? "fits" : "misfits",
    ...slot.bleed.map((e) => `bleed-${e}`),
  ];
  const flags = [
    slot.bleed.length > 0 ? `bleed ${slot.bleed.join("+")}` : "",
    touchesFold(slot.rect) ? "fold-flush" : "",
  ].filter(Boolean);
  return `<div class="${classes.join(" ")}" style="left:${pct(x)};top:${pct(y)};width:${pct(w)};height:${pct(h)}">
      <div class="tag">
        <b>${index}${slot.role === "hero" ? " HERO" : ""}</b>
        <span class="asp">${aspect.toFixed(3)}</span>
        <span class="pref">pref ${lo}–${hi}</span>
        <span class="fit ${fit.ok ? "good" : "bad"}">${fit.label} ${(fit.retention * 100).toFixed(0)}%</span>
        ${flags.length > 0 ? `<span class="flag">${esc(flags.join(" · "))}</span>` : ""}
      </div>
    </div>`;
}

function textBox(zone: TextZone, index: number): string {
  const [x, y, w, h] = zone.rect;
  return `<div class="tz" style="left:${pct(x)};top:${pct(y)};width:${pct(w)};height:${pct(h)}">
      <span>text ${index} · ${zone.align}</span>
    </div>`;
}

function card(t: Template): string {
  const left = t.slots.filter((s) => sideOf(s.rect) === "left");
  const right = t.slots.filter((s) => sideOf(s.rect) === "right");
  const straddling = t.slots.filter((s) => sideOf(s.rect) === "fold");
  const misfits = t.slots.filter((s) => !bestFit(s.rect).ok).length;
  const verdict =
    straddling.length > 0
      ? `<span class="badge bad">SPANS FOLD ×${straddling.length}</span>`
      : misfits === 0
        ? `<span class="badge good">all slots fit</span>`
        : `<span class="badge bad">${misfits}/${t.slots.length} slots fit no common aspect</span>`;

  return `<section class="card">
    <header>
      <h2>${esc(t.id)}</h2>
      <div class="meta">
        <span class="badge">${t.slots.length} photo${t.slots.length === 1 ? "" : "s"}</span>
        <span class="badge">${t.density}</span>
        <span class="badge">${t.energy}</span>
        <span class="badge">L: ${edgeTreatment(left)} (${left.length})</span>
        <span class="badge">R: ${edgeTreatment(right)} (${right.length})</span>
        ${t.slots.some((s) => touchesFold(s.rect)) ? '<span class="badge">fold-flush</span>' : ""}
        ${verdict}
      </div>
    </header>
    <div class="canvas">
      <div class="trim"></div>
      <div class="gutter"></div>
      <div class="fold"></div>
      ${t.slots.map(slotBox).join("\n")}
      ${t.text_zones.map(textBox).join("\n")}
    </div>
  </section>`;
}

function summary(templates: Template[]): string {
  const bySize = new Map<number, Template[]>();
  for (const t of templates) {
    const list = bySize.get(t.slots.length) ?? [];
    list.push(t);
    bySize.set(t.slots.length, list);
  }
  const sizes = [...bySize.keys()].toSorted((a, b) => a - b);

  const rows = sizes.map((n) => {
    const group = bySize.get(n) ?? [];
    const usable = group.filter((t) => t.slots.every((s) => bestFit(s.rect).ok));
    const bleeding = group.filter((t) => t.slots.some((s) => s.bleed.length > 0));
    return `<tr>
      <td>${n}</td>
      <td>${group.length}</td>
      <td class="${usable.length === 0 ? "bad" : "good"}">${usable.length}</td>
      <td>${bleeding.length}</td>
      <td class="ids">${group.map((t) => esc(t.id)).join(", ")}</td>
    </tr>`;
  });

  const halves = templates.flatMap((t) => [
    edgeTreatment(t.slots.filter((s) => sideOf(s.rect) === "left")),
    edgeTreatment(t.slots.filter((s) => sideOf(s.rect) === "right")),
  ]);
  const bleedHalves = halves.filter((h) => h === "bleed").length;
  const slots = templates.flatMap((t) => t.slots);
  const foldFlush = slots.filter((s) => touchesFold(s.rect)).length;

  return `<section class="summary">
    <h2>Library coverage</h2>
    <p>
      ${templates.length} templates ·
      ${slots.length} slots ·
      ${slots.filter((s) => s.bleed.length > 0).length} slots declare bleed ·
      ${foldFlush} slots run flush to the fold ·
      ${bleedHalves}/${halves.length} page halves derive <code>EdgeTreatment::Bleed</code>
    </p>
    <table>
      <thead><tr><th>photos</th><th>templates</th><th>fully usable</th><th>with bleed</th><th>ids</th></tr></thead>
      <tbody>${rows.join("\n")}</tbody>
    </table>
    <p class="note">
      "Fully usable" = every slot's real-world aspect retains at least
      ${(FIT_THRESHOLD * 100).toFixed(0)}% of the frame of its best-matching common camera
      aspect (4:3, 3:2, 3:4, 2:3). A slot below that can only be filled by discarding most
      of the photo, and since a clipped face is a hard rejection in the scorer, such a slot
      never places a picture of a person.
    </p>
  </section>`;
}

const STYLE = `
:root { color-scheme: light; --ink: #16181d; --line: #c9ced8; --bg: #f6f7f9; }
* { box-sizing: border-box; }
body { margin: 0; padding: 28px 32px 64px; background: var(--bg); color: var(--ink);
  font: 13px/1.45 ui-sans-serif, -apple-system, "Helvetica Neue", sans-serif; }
h1 { font-size: 20px; margin: 0 0 4px; }
h2 { font-size: 15px; margin: 0; }
.lede { color: #5b6270; max-width: 78ch; margin: 0 0 24px; }
code { font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; background: #e8ebf0;
  padding: 1px 4px; border-radius: 3px; }
.summary { background: #fff; border: 1px solid var(--line); border-radius: 8px;
  padding: 16px 18px; margin-bottom: 28px; }
.summary table { border-collapse: collapse; margin: 12px 0 8px; width: 100%; }
.summary th, .summary td { border-bottom: 1px solid var(--line); padding: 4px 8px;
  text-align: left; vertical-align: top; }
.summary td.good { color: #14733f; font-weight: 700; }
.summary td.bad { color: #b3261e; font-weight: 700; }
.summary .ids { font: 11px ui-monospace, Menlo, monospace; color: #5b6270; }
.note { color: #5b6270; margin: 8px 0 0; max-width: 90ch; }
.card { background: #fff; border: 1px solid var(--line); border-radius: 8px;
  padding: 14px 16px 18px; margin-bottom: 22px; }
.card header { display: flex; flex-wrap: wrap; gap: 8px 14px; align-items: baseline;
  margin-bottom: 10px; }
.meta { display: flex; flex-wrap: wrap; gap: 6px; }
.badge { font: 11px ui-monospace, Menlo, monospace; background: #eceff4; color: #3c4250;
  border-radius: 999px; padding: 2px 8px; }
.badge.good { background: #d8f0e2; color: #14733f; }
.badge.bad { background: #fbdcd9; color: #b3261e; }

/* The spread canvas, drawn to scale. */
.canvas { position: relative; width: 100%; aspect-ratio: ${CANVAS_W_IN} / ${CANVAS_H_IN};
  background: repeating-linear-gradient(45deg, #f0f2f5 0 6px, #eaedf2 6px 12px);
  border: 1px solid #9aa3b2; }
.trim { position: absolute; left: ${pct(SAFE_X0)}; top: ${pct(SAFE_Y0)};
  width: ${pct(SAFE_X1 - SAFE_X0)}; height: ${pct(SAFE_Y1 - SAFE_Y0)};
  border: 1px dashed #7b8698; pointer-events: none; }
.gutter { position: absolute; left: ${pct(GUTTER_X0)}; top: 0;
  width: ${pct(GUTTER_X1 - GUTTER_X0)}; height: 100%;
  background: repeating-linear-gradient(45deg, rgba(179,38,30,.30) 0 4px, rgba(179,38,30,.10) 4px 8px);
  pointer-events: none; z-index: 3; }
.fold { position: absolute; left: 50%; top: 0; width: 0; height: 100%;
  border-left: 1px solid #b3261e; pointer-events: none; z-index: 4; }

.slot { position: absolute; border: 1px solid #4a5568; background: rgba(74,85,104,.10);
  overflow: hidden; }
.slot.hero { background: rgba(37,99,235,.16); border-color: #2563eb; }
.slot.misfits { background: rgba(179,38,30,.16); border-color: #b3261e; }
.slot.bleed-left { border-left: 5px solid #e2543a; }
.slot.bleed-right { border-right: 5px solid #e2543a; }
.slot.bleed-top { border-top: 5px solid #e2543a; }
.slot.bleed-bottom { border-bottom: 5px solid #e2543a; }
.tag { display: flex; flex-direction: column; gap: 1px; padding: 3px 4px;
  font: 10px/1.25 ui-monospace, Menlo, monospace; }
.tag b { font-size: 11px; }
.asp { color: #16181d; }
.pref, .flag { color: #4b5361; }
.fit.good { color: #14733f; }
.fit.bad { color: #b3261e; font-weight: 700; }
.tz { position: absolute; border: 1px dashed #7a5af5; background: rgba(122,90,245,.10);
  font: 10px ui-monospace, Menlo, monospace; color: #4c33b8; padding: 2px 3px; z-index: 2; }
`;

function render(templates: Template[]): string {
  const ordered = templates.toSorted(
    (a, b) => a.slots.length - b.slots.length || a.id.localeCompare(b.id),
  );
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Spread template contact sheet</title>
<style>${STYLE}</style>
</head>
<body>
<h1>Spread template contact sheet</h1>
<p class="lede">
  Every file in <code>templates/</code> drawn to scale on the ${CANVAS_W_IN}" × ${CANVAS_H_IN}"
  spread canvas (${CANVAS_RATIO.toFixed(3)}:1). The red hatched strip is the gutter dead band,
  the thin red line is the fold, the dashed rectangle is the trim / safe area. Slots are
  labelled with their <b>real-world inch aspect</b>, their declared <code>aspect_pref</code>,
  and the closest common camera aspect with the fraction of the frame that survives. Slots
  shown in red match no common camera aspect. Thick orange edges are declared bleed.
  Ordered by photo count.
</p>
${summary(ordered)}
${ordered.map(card).join("\n")}
</body>
</html>
`;
}

process.stdout.write(render(loadTemplates()));
