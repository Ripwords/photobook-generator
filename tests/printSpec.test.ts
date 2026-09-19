import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";
import {
  MM_PER_IN,
  applyState,
  checkSummary,
  fieldsFromSpec,
  formatLength,
  lowResolutionNote,
  proposeSpec,
  refusalText,
  shapeNote,
  sizeLabel,
  specDiagram,
  toInches,
  commitsAsChange,
  type PrintSpec,
  type Proposal,
  type SpecCheck,
} from "../app/types/printSpec";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { usePrintSpecCheck, SPEC_CHECK_SETTLE_MS } = await import("../app/composables/usePrintSpecCheck");

/** Pixajoy's numbers as `book-layout.json` carries them. */
const pixajoy: PrintSpec = {
  pageWIn: 11.197,
  pageHIn: 8.894,
  bleedIn: 0.197,
  gutterIn: 0.197,
  safeMarginIn: 0.125,
  minDpi: 200,
  warnDpi: 300,
};

function accepted(p: Proposal): PrintSpec {
  if (!p.ok) throw new Error(`refused: ${p.message}`);
  return p.spec;
}

describe("units", () => {
  /**
   * T19. 1 in is 25.4 mm. The fixture is not self-symmetric: feeding 0 or
   * 25.4 would pass with the conversion divided instead of multiplied.
   */
  it("renders an inch as twenty-five point four millimetres", () => {
    expect(MM_PER_IN).toBe(25.4);
    expect(formatLength(1, "mm")).toBe("25.40");
    expect(formatLength(1, "in")).toBe("1.000");
    expect(formatLength(0.197, "mm")).toBe("5.00");
  });

  /** T21. No rounding on the way in: 5 mm is stored as its exact inch value. */
  it("stores a millimetre value as its exact inch equivalent", () => {
    expect(toInches(5, "mm")).toBe(0.1968503937007874);
    expect(toInches(0.197, "in")).toBe(0.197);
  });

  it("commits a typed value only when it differs by more than half a display step", () => {
    // 0.197 in is shown as 5.00 mm; tabbing through that field must not
    // turn it into 0.19685 in.
    expect(commitsAsChange(5.0, 0.197, "mm")).toBe(false);
    expect(commitsAsChange(5.01, 0.197, "mm")).toBe(true);
    expect(commitsAsChange(0.1974, 0.197, "in")).toBe(false);
    expect(commitsAsChange(0.198, 0.197, "in")).toBe(true);
  });
});

describe("proposeSpec", () => {
  /**
   * T20. Flipping the unit re-renders from the canonical inches and never
   * writes back. `toBe`/`toStrictEqual`, not `toBeCloseTo`: the drift this
   * guards is one rounding step, so a closeness check passes under the bug.
   * `0.5` would be a bad fixture -- 12.7 mm round-trips exactly at 2 dp.
   */
  it("never changes the stored spec when the unit is switched", () => {
    let spec = pixajoy;
    for (const unit of ["in", "mm", "in", "mm", "in"] as const) {
      spec = accepted(proposeSpec(fieldsFromSpec(spec, unit), spec, unit));
      expect(spec).toStrictEqual(pixajoy);
    }
    // And the drift is real, so no later refactor may convert on toggle.
    expect(toInches(Number(formatLength(0.197, "mm")), "mm")).not.toBe(0.197);
  });

  /**
   * An untouched page is kept, not rebuilt from its own trim. At Pixajoy's
   * numbers the rebuild happens to be exact, so this uses a 3 mm bleed on a
   * page whose height does NOT survive `(h - 2b) + 2b` -- a one-ULP change
   * `SetPrintSpec` would read as a new page shape and re-cut every crop for.
   */
  it("keeps an untouched page exactly as stored", () => {
    const base: PrintSpec = { ...pixajoy, bleedIn: 3 / 25.4, pageHIn: 7.623 };
    expect(base.pageHIn - 2 * base.bleedIn + 2 * base.bleedIn).not.toBe(base.pageHIn);
    for (const unit of ["in", "mm"] as const) {
      expect(accepted(proposeSpec(fieldsFromSpec(base, unit), base, unit))).toStrictEqual(base);
    }
  });

  it("shows the book size a printer quotes, not the page with bleed", () => {
    const fields = fieldsFromSpec(pixajoy, "in");
    expect(fields.trimW).toBe("11.000");
    expect(fields.trimH).toBe("8.500");
    expect(fields.bleed).toBe("0.197");
    expect(fields.fold).toBe("0.197");
    expect(fields.safeMargin).toBe("0.125");
    expect(fields.minDpi).toBe("200");
    expect(fields.warnDpi).toBe("300");
  });

  it("puts the bleed back on: once on the width, twice on the height", () => {
    const fields = { ...fieldsFromSpec(pixajoy, "in"), trimW: "8", trimH: "10", bleed: "0.25" };
    const spec = accepted(proposeSpec(fields, pixajoy, "in"));
    expect(spec.pageWIn).toBe(8.25);
    expect(spec.pageHIn).toBe(10.5);
    expect(spec.bleedIn).toBe(0.25);
    // Untouched fields keep their exact canonical value.
    expect(spec.gutterIn).toBe(0.197);
    expect(spec.safeMarginIn).toBe(0.125);
  });

  it("keeps the book size when only the bleed changes", () => {
    const spec = accepted(proposeSpec({ ...fieldsFromSpec(pixajoy, "mm"), bleed: "3" }, pixajoy, "mm"));
    expect(spec.bleedIn).toBe(3 / 25.4);
    expect(spec.pageWIn - spec.bleedIn).toBeCloseTo(11, 12);
    expect(spec.pageHIn - 2 * spec.bleedIn).toBeCloseTo(8.5, 12);
  });

  it("refuses a blank or non-numeric field instead of reading it as zero", () => {
    const blank = proposeSpec({ ...fieldsFromSpec(pixajoy, "in"), bleed: " " }, pixajoy, "in");
    expect(blank).toEqual({ ok: false, field: "bleed", message: "Enter a bleed." });
    const junk = proposeSpec({ ...fieldsFromSpec(pixajoy, "in"), warnDpi: "3oo" }, pixajoy, "in");
    expect(junk).toEqual({ ok: false, field: "warnDpi", message: "Target resolution must be a number." });
  });

  /**
   * Parsing is the client's whole job. Every RULE is Rust's: a negative
   * bleed parses fine here and comes back refused from `check_print_spec`.
   */
  it("leaves every rule to Rust", () => {
    const spec = accepted(proposeSpec({ ...fieldsFromSpec(pixajoy, "in"), bleed: "-1" }, pixajoy, "in"));
    expect(spec.bleedIn).toBe(-1);
  });
});

describe("the wire", () => {
  const cases = JSON.parse(
    readFileSync(fileURLToPath(new URL("fixtures/wire/spec-check.json", import.meta.url)), "utf8"),
  ) as SpecCheck[];

  /** The TypeScript half of `spec_check_serialises_the_shape_the_panel_reads`. */
  it("reads the shapes Rust sends", () => {
    const [checked, vertical, floor, negative] = cases;
    expect(checked?.kind).toBe("checked");
    expect(checkSummary(checked!)).toEqual({
      text: "At this size, 1 problem would block the export.",
      recrops: true,
      blocking: [checked!.kind === "checked" ? checked!.findings[0] : null],
      warnings: [],
    });
    if (checked?.kind === "checked") expect(checked.geometry.left.trim.x).toBe(0.015625);

    expect(refusalText(vertical!.kind === "refused" ? vertical!.error : never(), "in")).toBe(
      "Bleed and safe margin, top and bottom, add up to 8.500 in on a 8.000 in page, which leaves no room for photos.",
    );
    expect(refusalText(floor!.kind === "refused" ? floor!.error : never(), "mm")).toBe(
      "Target resolution (200 DPI) must be above the lowest print resolution (300 DPI).",
    );
    expect(refusalText(negative!.kind === "refused" ? negative!.error : never(), "mm")).toBe(
      "Bleed cannot be negative.",
    );
  });

  it("renders a refusal's lengths in the unit on screen", () => {
    const error = { kind: "noSafeArea", axis: "horizontal", insetsIn: 11.197, pageIn: 11.197 } as const;
    expect(refusalText(error, "mm")).toBe(
      "Bleed, safe margin and fold add up to 284.40 mm across a 284.40 mm page, which leaves no room for photos.",
    );
  });
});

function never(): never {
  throw new Error("fixture order changed");
}

describe("notes", () => {
  /** T26. Two-sided, so `shapeNote` hardwired to `null` fails. */
  it("warns about portrait and square pages and not about landscape ones", () => {
    const sized = (w: number, h: number): PrintSpec => ({
      ...pixajoy,
      pageWIn: w + pixajoy.bleedIn,
      pageHIn: h + 2 * pixajoy.bleedIn,
    });
    expect(shapeNote(sized(11, 8.5))).toBeNull();
    expect(shapeNote(sized(12, 9))).toBeNull();
    expect(shapeNote(sized(8.5, 11))).toMatch(/landscape/);
    expect(shapeNote(sized(10, 10))).toMatch(/landscape/);
  });

  it("says soft prints are coming below 150 DPI, and only then", () => {
    expect(lowResolutionNote(pixajoy)).toBeNull();
    expect(lowResolutionNote({ ...pixajoy, minDpi: 149 })).toMatch(/soft/);
  });

  it("labels a book by the size a printer quotes", () => {
    expect(sizeLabel(pixajoy, "in")).toBe("11 x 8.5 in");
    expect(sizeLabel(pixajoy, "mm")).toBe("279.4 x 215.9 mm");
  });
});

describe("applyState", () => {
  const wider = bigger();
  const checked = (blocks: number): SpecCheck => ({
    kind: "checked",
    geometry: {
      pageWIn: 1,
      pageHIn: 1,
      left: { trim: rect, safe: rect, gutter: rect },
      right: { trim: rect, safe: rect, gutter: rect },
    },
    findings: Array.from({ length: blocks }, () => ({
      severity: "block" as const,
      page: 3,
      photoPath: "/p/a.jpg",
      message: "m",
    })),
    recrops: true,
  });

  it("labels the button by its consequence", () => {
    const ok = { ok: true, spec: wider } as const;
    expect(applyState(pixajoy, ok, { status: "done", result: checked(0) }, "in")).toEqual({
      enabled: true,
      label: "Change print size",
      alert: null,
    });
    expect(applyState(pixajoy, ok, { status: "done", result: checked(2) }, "in").label).toBe(
      "Change print size anyway",
    );
  });

  it("has nothing to apply when the spec is unchanged or still being checked", () => {
    expect(applyState(pixajoy, { ok: true, spec: pixajoy }, { status: "done", result: checked(0) }, "in").enabled).toBe(
      false,
    );
    expect(applyState(pixajoy, { ok: true, spec: wider }, { status: "checking" }, "in").enabled).toBe(false);
  });
});

const rect = { x: 0, y: 0, w: 1, h: 1 };

describe("usePrintSpecCheck", () => {
  /**
   * T22. The panel shows Rust's refusal, not its own. The mocked command
   * refuses a spec the client parses without complaint, so a panel that
   * validated locally and never asked -- or asked and ignored the answer --
   * would enable the button.
   */
  it("surfaces Rust's refusal for a spec the client thinks is fine", async () => {
    vi.useFakeTimers();
    const error = { kind: "warnBelowFloor", minDpi: 200, warnDpi: 250 } as const;
    invoke.mockResolvedValue({ kind: "refused", error });
    const proposal = ref<Proposal>({ ok: true, spec: { ...pixajoy, warnDpi: 250 } });
    const { state } = usePrintSpecCheck(ref(7), proposal);

    await vi.advanceTimersByTimeAsync(SPEC_CHECK_SETTLE_MS - 1);
    // Debounced: typing "250" is three keystrokes, not three dry runs.
    expect(invoke).not.toHaveBeenCalled();
    expect(state.value).toEqual({ status: "checking" });
    await vi.advanceTimersByTimeAsync(1);
    await nextTick();

    expect(invoke).toHaveBeenCalledWith("check_print_spec", { projectId: 7, spec: proposal.value.ok && proposal.value.spec });
    expect(state.value).toEqual({ status: "done", result: { kind: "refused", error } });
    const verdict = applyState(pixajoy, proposal.value, state.value, "in");
    expect(verdict.enabled).toBe(false);
    expect(verdict.alert).toBe(refusalText(error, "in"));
    vi.useRealTimers();
  });

  it("lets only the latest answer land", async () => {
    vi.useFakeTimers();
    invoke.mockReset();
    const slow = Promise.withResolvers<SpecCheck>();
    invoke.mockImplementationOnce(() => slow.promise);
    invoke.mockResolvedValueOnce({ kind: "refused", error: { kind: "notFinite", field: "bleedIn" } });
    const proposal = ref<Proposal>({ ok: true, spec: bigger() });
    const { state } = usePrintSpecCheck(ref(null), proposal);

    await vi.advanceTimersByTimeAsync(SPEC_CHECK_SETTLE_MS);
    proposal.value = { ok: true, spec: { ...bigger(), bleedIn: 0.1 } };
    await nextTick();
    await vi.advanceTimersByTimeAsync(SPEC_CHECK_SETTLE_MS);
    slow.resolve({ kind: "checked", geometry: { pageWIn: 1, pageHIn: 1, left: g(), right: g() }, findings: [], recrops: false });
    await vi.advanceTimersByTimeAsync(0);

    expect(state.value).toEqual({
      status: "done",
      result: { kind: "refused", error: { kind: "notFinite", field: "bleedIn" } },
    });
    expect(invoke).toHaveBeenNthCalledWith(1, "check_print_spec", { projectId: null, spec: bigger() });
    vi.useRealTimers();
  });
});

function bigger(): PrintSpec {
  return { ...pixajoy, pageWIn: 12.197 };
}

function g() {
  return { trim: rect, safe: rect, gutter: rect };
}

const right = (r: { x: number; w: number }) => r.x + r.w;

describe("specDiagram", () => {
  it("widens thin margins past scale so they can be seen, and says so", () => {
    const d = specDiagram(pixajoy);
    expect(d.width).toBe(2 * pixajoy.pageWIn);
    expect(d.height).toBe(pixajoy.pageHIn);
    expect(d.exaggerated).toBe(true);
    // 0.197" of a 22.394" spread is under 3px in a 320px panel.
    expect(d.left.trim.x).toBeGreaterThan(pixajoy.bleedIn);
    expect(d.left.safe.x - d.left.trim.x).toBeGreaterThan(pixajoy.safeMarginIn);
    expect(d.left.fold.w).toBeGreaterThan(pixajoy.gutterIn);
  });

  it("draws margins wider than the floor at their true size", () => {
    const wide: PrintSpec = { ...pixajoy, pageWIn: 12, pageHIn: 11, bleedIn: 1, safeMarginIn: 1.25, gutterIn: 1.5 };
    const d = specDiagram(wide);
    expect(d.exaggerated).toBe(false);
    expect(d.left.trim.x).toBe(1);
    expect(d.left.trim.y).toBe(1);
    expect(d.left.safe.x).toBe(2.25);
    expect(d.left.safe.y).toBe(2.25);
    expect(d.left.fold).toEqual({ x: 10.5, y: 0, w: 1.5, h: 11 });
  });

  it("draws no bleed band for a zero bleed rather than a floor-sized one", () => {
    const d = specDiagram({ ...pixajoy, bleedIn: 0 });
    expect(d.left.trim).toEqual(d.left.page);
    expect(d.right.trim).toEqual(d.right.page);
  });

  it("puts no trim or safe inset at the fold, and mirrors the right page", () => {
    const d = specDiagram(pixajoy);
    const fold = pixajoy.pageWIn;
    expect(right(d.left.trim)).toBeCloseTo(fold, 9);
    expect(right(d.left.safe)).toBeCloseTo(fold, 9);
    expect(d.right.trim.x).toBeCloseTo(fold, 9);
    expect(d.right.safe.x).toBeCloseTo(fold, 9);
    expect(d.right.fold.x).toBeCloseTo(fold, 9);
    expect(right(d.left.fold)).toBeCloseTo(fold, 9);
    expect(d.width - right(d.right.trim)).toBeCloseTo(d.left.trim.x, 12);
    expect(d.width - right(d.right.safe)).toBeCloseTo(d.left.safe.x, 12);
  });

  it("falls back to true scale when widened margins would swallow a very wide page", () => {
    const strip: PrintSpec = { ...pixajoy, pageWIn: 30, pageHIn: 4 };
    const d = specDiagram(strip);
    expect(d.exaggerated).toBe(false);
    expect(d.left.trim.x).toBe(strip.bleedIn);
    expect(d.left.safe.h).toBeGreaterThan(0);
  });
});
