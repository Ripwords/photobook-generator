/**
 * The layout editor's geometry, as pure functions: moving, resizing and
 * snapping boxes, the guide lines a print size implies, and turning a book's
 * opening into a shape to save. The editor component owns the pointer and
 * nothing else.
 *
 * None of this decides whether a layout is valid. `check_custom_layout` does,
 * in Rust, and the editor shows what it says. Clamping here only keeps a box
 * a box: on the canvas, the right way round, and no smaller than Rust's
 * `tooSmall` limit, so a drag can never produce something that cannot be
 * drawn.
 */
import type { LayoutBox, LayoutShape } from "~/types/customLayouts";
import { templateLabel, type PageSide, type PreviewPage, type PreviewRect, type PreviewSpread } from "~/types/preview";
import type { PrintSpec } from "~/types/printSpec";

export type ShapeKind = LayoutShape["kind"];
/** A box's corner, by compass point. */
export type Handle = "nw" | "ne" | "sw" | "se";

/** Lines a box edge snaps to, normalised on the canvas. */
export interface Guides {
  x: number[];
  y: number[];
}

/** The fold, on a spread canvas. */
export const FOLD_X = 0.5;
/** How close an edge must come to a guide to snap, in canvas units. */
export const SNAP = 0.01;

/** The canvas's width over its height: two pages for a spread. */
export function canvasAspect(kind: ShapeKind, pageAspect: number): number {
  return kind === "spread" ? 2 * pageAspect : pageAspect;
}

/** Rust's `tooSmall` limit, 5% of a page each way, in canvas units. */
export function minBox(kind: ShapeKind): { w: number; h: number } {
  return { w: kind === "spread" ? 0.025 : 0.05, h: 0.05 };
}

/**
 * Where the print size puts its lines on this canvas: the paper edges, the
 * bleed (trim) lines on the outer edges, and on a spread the fold with the
 * gutter either side of it. A page layout is one page, bled on its outer
 * edge and guttered on the fold's.
 */
export function editorGuides(shape: { kind: "spread" } | { kind: "page"; side: PageSide }, spec: PrintSpec): Guides {
  const bleedY = spec.bleedIn / spec.pageHIn;
  const y = [0, bleedY, 1 - bleedY, 1];
  if (shape.kind === "spread") {
    const bleedX = spec.bleedIn / (2 * spec.pageWIn);
    const gutterX = spec.gutterIn / (2 * spec.pageWIn);
    return { x: [0, bleedX, FOLD_X - gutterX, FOLD_X, FOLD_X + gutterX, 1 - bleedX, 1], y };
  }
  const bleedX = spec.bleedIn / spec.pageWIn;
  const gutterX = spec.gutterIn / spec.pageWIn;
  return { x: shape.side === "left" ? [0, bleedX, 1 - gutterX, 1] : [0, gutterX, 1 - bleedX, 1], y };
}

/**
 * The shift that lands the edge nearest a guide exactly on it, or 0 when no
 * edge is within `threshold` of any guide.
 */
export function snapDelta(edges: number[], guides: number[], threshold: number): number {
  let best = 0;
  let bestDistance = Infinity;
  for (const edge of edges) {
    for (const guide of guides) {
      const distance = Math.abs(guide - edge);
      if (distance <= threshold && distance < bestDistance) {
        best = guide - edge;
        bestDistance = distance;
      }
    }
  }
  return best;
}

function clamp(value: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, value));
}

/** `rect` dragged by `(dx, dy)`, kept whole on the canvas, its edges snapped. */
export function moveBox(rect: PreviewRect, dx: number, dy: number, guides: Guides, threshold: number): PreviewRect {
  const x = rect.x + dx;
  const y = rect.y + dy;
  return {
    x: clamp(x + snapDelta([x, x + rect.w], guides.x, threshold), 0, 1 - rect.w),
    y: clamp(y + snapDelta([y, y + rect.h], guides.y, threshold), 0, 1 - rect.h),
    w: rect.w,
    h: rect.h,
  };
}

/** One axis of a resize: the moving edge snapped, then held on the canvas and at least `min` from the fixed one. */
function resizeAxis(lo: number, hi: number, delta: number, movesLo: boolean, min: number, guides: number[], threshold: number) {
  if (movesLo) {
    let edge = lo + delta;
    edge += snapDelta([edge], guides, threshold);
    edge = clamp(edge, 0, hi - min);
    return { start: edge, size: hi - edge };
  }
  let edge = hi + delta;
  edge += snapDelta([edge], guides, threshold);
  edge = clamp(edge, lo + min, 1);
  return { start: lo, size: edge - lo };
}

/** `rect` with corner `handle` dragged by `(dx, dy)`; the opposite corner stays put. */
export function resizeBox(
  rect: PreviewRect,
  handle: Handle,
  dx: number,
  dy: number,
  kind: ShapeKind,
  guides: Guides,
  threshold: number,
): PreviewRect {
  const min = minBox(kind);
  const h = resizeAxis(rect.x, rect.x + rect.w, dx, handle.endsWith("w"), min.w, guides.x, threshold);
  const v = resizeAxis(rect.y, rect.y + rect.h, dy, handle.startsWith("n"), min.h, guides.y, threshold);
  return { x: h.start, y: v.start, w: h.size, h: v.size };
}

function overlaps(a: PreviewRect, b: PreviewRect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/**
 * The shape with one more box, placed where it overlaps nothing and stays on
 * one page: the biggest of three sizes that fits, scanning each page top-left
 * first. The first box is the hero, the rest support. When nothing fits, it
 * goes in the middle of the first page and the checker says it overlaps.
 */
export function addBox(shape: LayoutShape): LayoutShape {
  const pages: [number, number][] = shape.kind === "spread" ? [[0, FOLD_X], [FOLD_X, 1]] : [[0, 1]];
  const taken = shape.boxes.map((b) => b.rect);
  const role: LayoutBox["role"] = shape.boxes.length === 0 ? "hero" : "support";
  const step = 0.05;
  for (const size of [0.8, 0.4, 0.25]) {
    for (const [start, end] of pages) {
      const w = (end - start) * size;
      const h = size;
      const inset = (end - start - w) / 2;
      const yInset = (1 - h) / 2;
      // Centred first, so a lone box sits where a person would put it.
      const xs = [start + inset];
      for (let x = start; x + w <= end + 1e-9; x += (end - start) * step) xs.push(x);
      const ys = [yInset];
      for (let y = 0; y + h <= 1 + 1e-9; y += step) ys.push(y);
      for (const y of ys) {
        for (const x of xs) {
          const candidate = { x, y, w, h };
          if (!taken.some((t) => overlaps(t, candidate))) {
            return { ...shape, boxes: [...shape.boxes, { rect: candidate, role }] };
          }
        }
      }
    }
  }
  const [start, end] = pages[0]!;
  const w = (end - start) * 0.25;
  const rect = { x: start + (end - start - w) / 2, y: 0.375, w, h: 0.25 };
  return { ...shape, boxes: [...shape.boxes, { rect, role }] };
}

/** A box is the hero when it is at least this many times the next biggest. */
const HERO_RATIO = 1.5;

/**
 * An opening's pages as a shape to save: one page keeps its side, two become
 * a spread with each page's rects halved onto it. Pages carry no roles, so
 * the clearly biggest box is the hero and the rest support; a single box is
 * the hero.
 */
export function shapeFromPages(pages: PreviewPage[]): LayoutShape {
  const spread = pages.length === 2;
  const rects = pages.flatMap((page) =>
    page.placements.map((p): PreviewRect => {
      if (!spread) return { ...p.slotRect };
      const origin = page.side === "left" ? 0 : FOLD_X;
      return { x: origin + p.slotRect.x * FOLD_X, y: p.slotRect.y, w: p.slotRect.w * FOLD_X, h: p.slotRect.h };
    }),
  );
  const areas = rects.map((r) => r.w * r.h);
  const sorted = areas.toSorted((a, b) => b - a);
  const hero = sorted.length === 1 || sorted[0]! >= HERO_RATIO * sorted[1]! ? areas.indexOf(sorted[0]!) : -1;
  const boxes = rects.map((rect, i): LayoutBox => ({ rect, role: i === hero ? "hero" : "support" }));
  return spread ? { kind: "spread", boxes } : { kind: "page", side: pages[0]!.side, boxes };
}

/**
 * A shape as `TemplateSketch` draws it: each page's rects, page-normalised.
 * A spread box belongs to the page its left edge is on; a box that crosses
 * the fold is drawn on that page too, overhanging, which the checker reports.
 */
export function shapePages(shape: LayoutShape): PreviewRect[][] {
  if (shape.kind === "page") return [shape.boxes.map((b) => ({ ...b.rect }))];
  const left: PreviewRect[] = [];
  const right: PreviewRect[] = [];
  for (const { rect } of shape.boxes) {
    const onRight = rect.x >= FOLD_X;
    const origin = onRight ? FOLD_X : 0;
    (onRight ? right : left).push({ x: (rect.x - origin) / FOLD_X, y: rect.y, w: rect.w / FOLD_X, h: rect.h });
  }
  return [left, right];
}

/** The name a copy of a built-in starts with. */
export function copyName(templateId: string): string {
  const words = templateLabel(templateId);
  return `${words.charAt(0).toUpperCase()}${words.slice(1)} copy`;
}

/** A new layout's starting name: the first "Layout N" no layout has. */
export function nextLayoutName(taken: string[]): string {
  const names = new Set(taken);
  for (let n = 1; ; n++) {
    const name = `Layout ${n}`;
    if (!names.has(name)) return name;
  }
}

/**
 * A shape as plain data. The editor's shape is reactive, and a Vue proxy
 * cannot cross IPC or `structuredClone` (`DataCloneError`); every store
 * command gets this instead.
 */
export function plainShape(shape: LayoutShape): LayoutShape {
  const boxes = shape.boxes.map(({ rect: { x, y, w, h }, role }) => ({ rect: { x, y, w, h }, role }));
  return shape.kind === "spread" ? { kind: "spread", boxes } : { kind: "page", side: shape.side, boxes };
}

/** An opening's pages, left first, without the half page 1 and the last page lack. */
export function openingPages(spread: PreviewSpread): PreviewPage[] {
  return [spread.left, spread.right].filter((page): page is PreviewPage => page !== null);
}

/** An empty layout the shape of this opening: a spread, or a page on its side. */
export function openingBlankShape(spread: PreviewSpread): LayoutShape {
  if (spread.left && spread.right) return { kind: "spread", boxes: [] };
  return { kind: "page", side: spread.left ? "left" : "right", boxes: [] };
}
