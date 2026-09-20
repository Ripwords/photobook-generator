/**
 * The print geometry a book is laid out under, as the webview holds it.
 *
 * **Nothing here decides what a valid spec is.** `src-tauri/src/print_spec.rs`
 * owns that: `PrintSpec` there has private fields and no `Deserialize` that
 * skips its validator, so every spec that reaches the engine -- over IPC, out
 * of a saved `book_json`, out of a hand-edited database row -- has passed
 * `TryFrom<RawPrintSpec>`. This file mirrors the shape so the panel can hold
 * one, show it, and hand it back. Anything that looks like a rule belongs on
 * the Rust side.
 *
 * The one exception is deliberate and documented where it lives: `shapeNote`
 * warns that the built-in templates were drawn for a landscape page. That is
 * advice about the template library, not a fact about the geometry, and
 * `SpecError` is reserved for things that make a book unbuildable.
 *
 * **The defaults are not retyped here.** A new book's geometry comes from the
 * `default_print_spec` command, for the reason `preview.rs`'s header gives
 * about guides: a number hand-copied across this boundary is a number that
 * can drift out of step with the one the engine enforces.
 */

import type { PreflightFinding } from "./book";
import type { PreviewGeometry, PreviewRect } from "./preview";

/**
 * Mirrors `print_spec::PrintSpec`, which serialises camelCase.
 *
 * Inches at full `f64` precision, always, whatever unit is on screen. The
 * display unit is a separate preference and must never be stored alongside
 * these numbers: `PrintSpec` derives `PartialEq` in Rust, and `SetPrintSpec`
 * asks whether the spec actually changed before it re-crops every placement,
 * so a unit field would make flipping a display toggle re-crop the book.
 */
export interface PrintSpec {
  /** Finished page width after trimming, plus the bleed on the outer edge. */
  pageWIn: number;
  pageHIn: number;
  /** Bleed on the three outer edges. There is none at the fold. */
  bleedIn: number;
  /** Width of the dead strip measured inward from the fold. */
  gutterIn: number;
  /** Inset from the trim that nothing important should cross. */
  safeMarginIn: number;
  /** Below this, a photo blocks the export. */
  minDpi: number;
  /** At or above this, resolution stops counting against a placement. */
  warnDpi: number;
  /**
   * How far a cover photo runs past the trim to fold around the board, on
   * the top, bottom and outer edge.
   */
  coverWrapIn: number;
}

/** The panel's one display unit. A preference, never part of the spec. */
export type LengthUnit = "in" | "mm";

export const MM_PER_IN = 25.4;

/** Inches rendered in `unit`: 3 dp for inches, 2 dp for millimetres. */
export function formatLength(inches: number, unit: LengthUnit): string {
  return unit === "in" ? inches.toFixed(3) : (inches * MM_PER_IN).toFixed(2);
}

/** A typed value in `unit`, as exact inches. No rounding on the way in. */
export function toInches(value: number, unit: LengthUnit): number {
  return unit === "in" ? value : value / MM_PER_IN;
}

/**
 * Whether a typed value is an edit rather than the displayed value read back.
 *
 * Half a display step either way is "unchanged". Without this, tabbing
 * through a field would write `0.197 in` back as `5.00 mm` = 0.19685 in, and
 * every unit flip would quietly change the book.
 */
export function commitsAsChange(typed: number, canonicalIn: number, unit: LengthUnit): boolean {
  const halfStep = unit === "in" ? 0.0005 : 0.005;
  return Math.abs(typed - (unit === "in" ? canonicalIn : canonicalIn * MM_PER_IN)) > halfStep;
}

/**
 * What the panel's inputs hold. The book size is the TRIM size -- what a
 * printer quotes -- not `pageWIn`, which includes the bleed.
 */
export interface PrintSizeFields {
  trimW: string;
  trimH: string;
  bleed: string;
  safeMargin: string;
  fold: string;
  minDpi: string;
  warnDpi: string;
  coverWrap: string;
}

export type PrintSizeField = keyof PrintSizeFields;

const LABELS: Record<PrintSizeField, string> = {
  trimW: "Book width",
  trimH: "Book height",
  bleed: "Bleed",
  safeMargin: "Safe margin",
  fold: "Fold",
  minDpi: "Lowest print resolution",
  warnDpi: "Target resolution",
  coverWrap: "Cover wrap",
};

/** The trim is the page minus the bleed: once across, top and bottom down. */
function trimOf(spec: PrintSpec): { w: number; h: number } {
  return { w: spec.pageWIn - spec.bleedIn, h: spec.pageHIn - 2 * spec.bleedIn };
}

export function fieldsFromSpec(spec: PrintSpec, unit: LengthUnit): PrintSizeFields {
  const trim = trimOf(spec);
  return {
    trimW: formatLength(trim.w, unit),
    trimH: formatLength(trim.h, unit),
    bleed: formatLength(spec.bleedIn, unit),
    safeMargin: formatLength(spec.safeMarginIn, unit),
    fold: formatLength(spec.gutterIn, unit),
    minDpi: String(spec.minDpi),
    warnDpi: String(spec.warnDpi),
    coverWrap: formatLength(spec.coverWrapIn, unit),
  };
}

export type Proposal = { ok: true; spec: PrintSpec } | { ok: false; field: PrintSizeField; message: string };

/**
 * The spec the fields describe, built on `base` so an untouched field keeps
 * its exact inches.
 *
 * Parsing only. A blank field is not zero and a typo is not a number, and
 * neither can cross the wire as JSON. Every RULE -- negative, too big, no
 * room left for photos -- is `PrintSpec::try_from`'s, and reaches the panel
 * as `check_print_spec`'s refusal.
 */
export function proposeSpec(fields: PrintSizeFields, base: PrintSpec, unit: LengthUnit): Proposal {
  const typed = {} as Record<PrintSizeField, number>;
  for (const key of Object.keys(LABELS) as PrintSizeField[]) {
    const text = fields[key].trim();
    if (text === "") return { ok: false, field: key, message: `Enter a ${LABELS[key].toLowerCase()}.` };
    const value = Number(text);
    if (!Number.isFinite(value)) return { ok: false, field: key, message: `${LABELS[key]} must be a number.` };
    typed[key] = value;
  }

  const trim = trimOf(base);
  const length = (key: PrintSizeField, canonical: number) =>
    commitsAsChange(typed[key], canonical, unit) ? toInches(typed[key], unit) : canonical;
  const trimW = length("trimW", trim.w);
  const trimH = length("trimH", trim.h);
  const bleedIn = length("bleed", base.bleedIn);
  const widthMoved = trimW !== trim.w || bleedIn !== base.bleedIn;
  const heightMoved = trimH !== trim.h || bleedIn !== base.bleedIn;

  return {
    ok: true,
    spec: {
      // Recomputing an unmoved page from its own trim would drift it by a
      // ULP, and `SetPrintSpec` would then call it a new page shape.
      pageWIn: widthMoved ? trimW + bleedIn : base.pageWIn,
      pageHIn: heightMoved ? trimH + 2 * bleedIn : base.pageHIn,
      bleedIn,
      gutterIn: length("fold", base.gutterIn),
      safeMarginIn: length("safeMargin", base.safeMarginIn),
      minDpi: typed.minDpi,
      warnDpi: typed.warnDpi,
      coverWrapIn: length("coverWrap", base.coverWrapIn),
    },
  };
}

export function sameSpec(a: PrintSpec, b: PrintSpec): boolean {
  return (Object.keys(a) as (keyof PrintSpec)[]).every((k) => a[k] === b[k]);
}

/** `11 x 8.5 in`: the trim, as a printer's catalogue names the book. */
export function sizeLabel(spec: PrintSpec, unit: LengthUnit): string {
  const trim = trimOf(spec);
  const n = (inches: number) =>
    unit === "in" ? String(Number(inches.toFixed(3))) : String(Number((inches * MM_PER_IN).toFixed(1)));
  return `${n(trim.w)} x ${n(trim.h)} ${unit}`;
}

/** One page of `specDiagram`, in inches across the spread. */
export interface DiagramPage {
  page: PreviewRect;
  trim: PreviewRect;
  safe: PreviewRect;
  fold: PreviewRect;
}

export interface SpecDiagram {
  width: number;
  height: number;
  left: DiagramPage;
  right: DiagramPage;
  /** Some margin was drawn wider than scale. */
  exaggerated: boolean;
}

/** A non-zero margin is drawn at least this share of the spread's width. */
const DIAGRAM_MIN_INSET = 0.025;

/**
 * The Print size panel's picture of a spread: bleed, safe margin and fold, in
 * inches, for an SVG `viewBox`.
 *
 * A schematic, not the guides. At panel size Pixajoy's 0.197" bleed is under
 * 3px, so each non-zero margin is widened to `DIAGRAM_MIN_INSET` and the
 * numbers beside the drawing carry the truth. The editor's guides stay
 * Rust's (`PreviewGeometry`). The fold edge carries no trim or safe inset,
 * as in `PrintSpec::trim_rect` and `safe_rect`.
 */
export function specDiagram(spec: PrintSpec): SpecDiagram {
  const w = spec.pageWIn;
  const h = spec.pageHIn;
  const floor = DIAGRAM_MIN_INSET * 2 * w;
  const widen = (inches: number) => (inches > 0 ? Math.max(inches, floor) : 0);
  let [bleed, safe, fold] = [spec.bleedIn, spec.safeMarginIn, spec.gutterIn].map(widen) as [number, number, number];
  const fits = bleed + safe + fold < w && 2 * (bleed + safe) < h;
  if (!fits) [bleed, safe, fold] = [spec.bleedIn, spec.safeMarginIn, spec.gutterIn];
  const exaggerated = bleed !== spec.bleedIn || safe !== spec.safeMarginIn || fold !== spec.gutterIn;

  const inset = bleed + safe;
  const left: DiagramPage = {
    page: { x: 0, y: 0, w, h },
    trim: { x: bleed, y: bleed, w: w - bleed, h: h - 2 * bleed },
    safe: { x: inset, y: inset, w: w - inset, h: Math.max(0, h - 2 * inset) },
    fold: { x: w - fold, y: 0, w: fold, h },
  };
  const mirror = (r: PreviewRect): PreviewRect => ({ ...r, x: 2 * w - r.x - r.w });
  const right: DiagramPage = {
    page: mirror(left.page),
    trim: mirror(left.trim),
    safe: mirror(left.safe),
    fold: mirror(left.fold),
  };
  return { width: 2 * w, height: h, left, right, exaggerated };
}

/**
 * The template library's limit, said rather than enforced. All 36 templates
 * were drawn on a landscape spread; normalised rects keep them valid at any
 * shape, so this is advice and never disables anything. Silent between
 * roughly 1.15:1 and 1.6:1 of trim, so nudging 11 x 8.5 to 12 x 9 does not nag.
 */
export function shapeNote(spec: PrintSpec): string | null {
  const trim = trimOf(spec);
  const aspect = trim.w / trim.h;
  if (aspect >= 1.15 && aspect <= 1.6) return null;
  return "The built-in layouts are designed for a landscape page. On a portrait or square book they will still fit, but the compositions were not drawn for this shape.";
}

export function lowResolutionNote(spec: PrintSpec): string | null {
  return spec.minDpi < 150 ? "Below 150 DPI, photos print visibly soft." : null;
}

/** Mirrors `print_spec::SpecField`: the spec key a refusal is about. */
export type SpecField = keyof PrintSpec;

/** Mirrors `print_spec::SpecError`. Lengths are inches; DPI is DPI. */
export type SpecError =
  | { kind: "notFinite"; field: SpecField }
  | { kind: "notPositive"; field: SpecField; value: number }
  | { kind: "negative"; field: SpecField; value: number }
  | { kind: "tooLarge"; field: SpecField; value: number; limit: number }
  | { kind: "noSafeArea"; axis: "horizontal" | "vertical"; insetsIn: number; pageIn: number }
  | { kind: "warnBelowFloor"; minDpi: number; warnDpi: number };

/** Mirrors `reprint::SpecCheck`. */
export type SpecCheck =
  | { kind: "refused"; error: SpecError }
  | {
      kind: "checked";
      geometry: PreviewGeometry;
      findings: PreflightFinding[];
      recrops: boolean;
      recropsCover: boolean;
    };

const FIELD_OF: Record<SpecField, PrintSizeField> = {
  pageWIn: "trimW",
  pageHIn: "trimH",
  bleedIn: "bleed",
  gutterIn: "fold",
  safeMarginIn: "safeMargin",
  minDpi: "minDpi",
  warnDpi: "warnDpi",
  coverWrapIn: "coverWrap",
};

/** The input a refusal points at, or `null` when it is about several. */
export function refusalField(error: SpecError): PrintSizeField | null {
  if (error.kind === "noSafeArea") return null;
  if (error.kind === "warnBelowFloor") return "warnDpi";
  return FIELD_OF[error.field];
}

/** Rust's refusal in words, with every length in the unit on screen. */
export function refusalText(error: SpecError, unit: LengthUnit): string {
  const len = (inches: number) => `${formatLength(inches, unit)} ${unit}`;
  switch (error.kind) {
    case "notFinite":
      return `${LABELS[FIELD_OF[error.field]]} must be a number.`;
    case "notPositive":
      return `${LABELS[FIELD_OF[error.field]]} must be greater than zero.`;
    case "negative":
      return `${LABELS[FIELD_OF[error.field]]} cannot be negative.`;
    case "tooLarge":
      return error.field === "coverWrapIn"
        ? `${LABELS.coverWrap} can be at most ${len(error.limit)}.`
        : `${LABELS[FIELD_OF[error.field]]} with its bleed can be at most ${len(error.limit)}.`;
    case "noSafeArea":
      return error.axis === "horizontal"
        ? `Bleed, safe margin and fold add up to ${len(error.insetsIn)} across a ${len(error.pageIn)} page, which leaves no room for photos.`
        : `Bleed and safe margin, top and bottom, add up to ${len(error.insetsIn)} on a ${len(error.pageIn)} page, which leaves no room for photos.`;
    case "warnBelowFloor":
      return `Target resolution (${error.warnDpi} DPI) must be above the lowest print resolution (${error.minDpi} DPI).`;
  }
}

export interface CheckSummary {
  text: string;
  /** Which crops applying re-cuts, hand-adjusted ones included, or `null` when none move. */
  recropNote: string | null;
  blocking: PreflightFinding[];
  warnings: PreflightFinding[];
}

function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

function recropNote(pages: boolean, cover: boolean): string | null {
  if (pages && cover) {
    return "Every crop, on the pages and the cover, is recomputed for the new shape, including ones you adjusted by hand.";
  }
  if (pages) return "Crops you adjusted by hand are recomputed for the new page shape.";
  if (cover) return "The cover photos are re-cropped for the new cover shape, including a crop you adjusted by hand.";
  return null;
}

/** What the dry run found, split the way the export sheet splits it. */
export function checkSummary(check: Extract<SpecCheck, { kind: "checked" }>): CheckSummary {
  const blocking = check.findings.filter((f) => f.severity === "block");
  const warnings = check.findings.filter((f) => f.severity === "warn");
  const parts = [
    blocking.length > 0 ? `${count(blocking.length, "problem", "problems")} would block the export` : null,
    warnings.length > 0 ? count(warnings.length, "warning", "warnings") : null,
  ].filter((p): p is string => p !== null);
  return {
    text: parts.length === 0 ? "Nothing in the book fails at this size." : `At this size, ${parts.join(" and ")}.`,
    recropNote: recropNote(check.recrops, check.recropsCover),
    blocking,
    warnings,
  };
}

export type CheckState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "done"; result: SpecCheck }
  | { status: "failed"; error: string };

export interface ApplyState {
  enabled: boolean;
  /** Named by consequence: "anyway" when the dry run found blocks. */
  label: string;
  /** The first refusal, shown at the foot of the panel. */
  alert: string | null;
}

/**
 * The Apply button. Enabled only on Rust's say-so: a proposal that parsed
 * locally but has not come back `checked` cannot be applied.
 */
export function applyState(current: PrintSpec, proposal: Proposal, check: CheckState, unit: LengthUnit): ApplyState {
  const label = "Change print size";
  if (!proposal.ok) return { enabled: false, label, alert: proposal.message };
  if (check.status === "failed") return { enabled: false, label, alert: check.error };
  if (check.status !== "done") return { enabled: false, label, alert: null };
  if (check.result.kind === "refused") return { enabled: false, label, alert: refusalText(check.result.error, unit) };
  if (sameSpec(proposal.spec, current)) return { enabled: false, label, alert: null };
  const blocks = check.result.findings.some((f) => f.severity === "block");
  return { enabled: true, label: blocks ? `${label} anyway` : label, alert: null };
}
