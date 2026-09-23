/**
 * A stand-in for `@tauri-apps/api/core` so the webview can be opened in an
 * ordinary browser and LOOKED AT, driven by a browser automation tool, with
 * no Tauri process behind it.
 *
 * `bun run ui:mock` aliases the real module to this one (see
 * `nuxt.config.ts`). Every command the UI can reach is answered -- from the
 * same wire fixtures the Rust and TypeScript suites pin where one exists, and
 * from `photos.ts` where the fixture is a whole analysed folder. An unknown
 * command still rejects loudly, so a screen that depends on something
 * unmocked cannot be mistaken for working.
 *
 * The state below is deliberately mutable and module-scoped: generating,
 * renaming and deleting have to be visible in the project list afterwards, or
 * the navigation between the library, the select screen and the editor cannot
 * be driven end to end.
 *
 * Never imported by the app itself. It exists so a change to the UI can be
 * rasterised and inspected before it ships.
 */
import layoutFixture from "../../tests/fixtures/wire/book-layout.json";
import recommendationFixture from "../../tests/fixtures/wire/book-recommendation.json";
import listFixture from "../../tests/fixtures/wire/project-list.json";
import exportFixture from "../../tests/fixtures/wire/export-result.json";
import agentViewFixture from "../../tests/fixtures/wire/agent-view.json";
import type { ModelProvider } from "../../app/agent/fetch";
import type { AgentPhoto, AgentView } from "../../app/agent/view";
import type {
  BookEdit,
  BookLayout,
  CoverSide,
  HistoryStatus,
  HistoryStep,
  PlacementRef,
  PreviewCoverSide,
  PreviewGeometry,
  PreviewRect,
  SlotCandidate,
} from "../../app/types/preview";
import type { PrintSpec, SpecCheck, SpecError } from "../../app/types/printSpec";
import { DEFAULT_BOOK_OPTIONS } from "../../app/types/book";
import type {
  BookOptions,
  BookRecommendation,
  EventRow,
  ExportEvent,
  ExportResult,
  GeneratedBook,
  PageOption,
  ProjectDetail,
  ProjectListItem,
  TierCounts,
} from "../../app/types/book";
import type {
  AnalysisEvent,
  AnalysisSummary,
  AnalyzedPhoto,
  EventTiers,
  PhotoOverrides,
  Tier,
} from "../../app/types/features";
import type { PreflightFinding } from "../../app/types/book";
import { cancelModelRequest, modelRequest, type ModelRequestArgs } from "./model";
import { mockPhotos, mockPlaceChapters, mockPlaceNames, thumbnail } from "./photos";

let layout: BookLayout = structuredClone(layoutFixture) as BookLayout;
// The fixture is pinned against an EMPTY library, so it offers no alternative
// layouts. Give the spread two so the buttons have something to do.
layout.openings[1] = {
  index: 1,
  locked: false,
  rejected: [],
  alternatives: ["07-two-up-symmetric-margin", "08-two-up-symmetric-bleed-outer"],
};
// The fixture's thumbnail paths point at a disk this browser cannot read.
layout.photos = layout.photos.map((photo, index) => ({
  ...photo,
  thumbnailPath: thumbnail(index + 3, photo.hash.slice(-4)),
}));

const photos: AnalyzedPhoto[] = mockPhotos();

// Rust wrote these numbers, so the harness's default cannot drift from
// `PrintSpec::pixajoy()` the way a hand-typed copy would. Taken before any
// `setPrintSpec` can change the book's own.
const PIXAJOY: PrintSpec = structuredClone(layout.spec);

/** `preview::preview_geometry`, restated for the harness only. */
function mockGeometry(spec: PrintSpec): PreviewGeometry {
  const tu = spec.bleedIn / spec.pageWIn;
  const tv = spec.bleedIn / spec.pageHIn;
  const iu = tu + spec.safeMarginIn / spec.pageWIn;
  const iv = tv + spec.safeMarginIn / spec.pageHIn;
  const g = spec.gutterIn / spec.pageWIn;
  return {
    pageWIn: spec.pageWIn,
    pageHIn: spec.pageHIn,
    left: {
      trim: { x: tu, y: tv, w: 1 - tu, h: 1 - 2 * tv },
      safe: { x: iu, y: iv, w: 1 - iu, h: 1 - 2 * iv },
      gutter: { x: 1 - g, y: 0, w: g, h: 1 },
    },
    right: {
      trim: { x: 0, y: tv, w: 1 - tu, h: 1 - 2 * tv },
      safe: { x: 0, y: iv, w: 1 - iu, h: 1 - 2 * iv },
      gutter: { x: 0, y: 0, w: g, h: 1 },
    },
  };
}

/** `PrintSpec::cover_aspect`, restated for the harness only. */
function coverAspect(spec: PrintSpec): number {
  return (
    (spec.pageWIn - spec.bleedIn + spec.coverWrapIn) / (spec.pageHIn - 2 * spec.bleedIn + 2 * spec.coverWrapIn)
  );
}

/** `PrintSpec::cover_board_rect` and `cover_visible_rect`, restated for the harness only. */
function coverRects(spec: PrintSpec, side: CoverSide): Pick<PreviewCoverSide, "board" | "visible"> {
  const trimW = spec.pageWIn - spec.bleedIn;
  const trimH = spec.pageHIn - 2 * spec.bleedIn;
  const pw = trimW + spec.coverWrapIn;
  const ph = trimH + 2 * spec.coverWrapIn;
  const wrap = spec.coverWrapIn;
  const safe = spec.safeMarginIn;
  const x = side === "front" ? 0 : wrap;
  return {
    board: { x: x / pw, y: wrap / ph, w: trimW / pw, h: trimH / ph },
    visible: { x: (x + safe) / pw, y: (wrap + safe) / ph, w: (trimW - 2 * safe) / pw, h: (trimH - 2 * safe) / ph },
  };
}

/** A few of `PrintSpec::try_from`'s refusals, enough to drive the panel's error states. */
function mockRefusal(spec: PrintSpec): SpecError | null {
  for (const field of ["pageWIn", "pageHIn", "minDpi"] as const) {
    if (!(spec[field] > 0)) return { kind: "notPositive", field, value: spec[field] };
  }
  for (const field of ["bleedIn", "gutterIn", "safeMarginIn"] as const) {
    if (spec[field] < 0) return { kind: "negative", field, value: spec[field] };
  }
  for (const field of ["pageWIn", "pageHIn"] as const) {
    if (spec[field] > 100) return { kind: "tooLarge", field, value: spec[field], limit: 100 };
  }
  const across = spec.bleedIn + spec.safeMarginIn + spec.gutterIn;
  if (across >= spec.pageWIn) return { kind: "noSafeArea", axis: "horizontal", insetsIn: across, pageIn: spec.pageWIn };
  const down = 2 * (spec.bleedIn + spec.safeMarginIn);
  if (down >= spec.pageHIn) return { kind: "noSafeArea", axis: "vertical", insetsIn: down, pageIn: spec.pageHIn };
  if (spec.warnDpi <= spec.minDpi) return { kind: "warnBelowFloor", minDpi: spec.minDpi, warnDpi: spec.warnDpi };
  return null;
}

/**
 * A stand-in for pre-flight at a new size: a bigger page spreads the same
 * pixels thinner. Every photo is taken as 2400 px across its slot, so a
 * full-page photo clears 200 DPI on Pixajoy's page and fails on a 13" one.
 */
function mockFindings(spec: PrintSpec): PreflightFinding[] {
  return layout.pages.flatMap((page) =>
    page.placements.flatMap((pl) => {
      const dpi = Math.round(2400 / (pl.slotRect.w * spec.pageWIn));
      const photoPath = layout.photos[pl.photoIndex]?.path ?? "";
      if (dpi < spec.minDpi) {
        return [{ severity: "block" as const, page: page.number, photoPath, message: `would print at ${dpi} DPI, below the ${spec.minDpi} DPI minimum` }];
      }
      if (dpi < spec.warnDpi) {
        return [{ severity: "warn" as const, page: page.number, photoPath, message: `prints at ${dpi} DPI, under the ${spec.warnDpi} DPI target` }];
      }
      return [];
    }),
  );
}

function mockCheck(spec: PrintSpec, withBook: boolean): SpecCheck {
  const error = mockRefusal(spec);
  if (error) return { kind: "refused", error };
  return {
    kind: "checked",
    geometry: mockGeometry(spec),
    findings: withBook ? mockFindings(spec) : [],
    recrops: withBook && spec.pageWIn / spec.pageHIn !== layout.spec.pageWIn / layout.spec.pageHIn,
    recropsCover:
      withBook &&
      (layout.cover.front.photo !== null || layout.cover.back.photo !== null) &&
      coverAspect(spec) !== coverAspect(layout.spec),
  };
}

// The fixture's cover paths point at a disk this browser cannot read, and
// one of its two books has none; give each a cover from the mock folder.
const projects: ProjectListItem[] = (structuredClone(listFixture) as ProjectListItem[]).map(
  (project, index) => ({ ...project, coverThumbnails: coverFrom(index * 9) }),
);
/** Deleted books and where they were listed, for `restore_project`. */
const trash: { project: ProjectListItem; index: number }[] = [];
/** Decisions each saved project was generated with, so reopening restores them. */
const savedOverrides = new Map<number, PhotoOverrides>();
/** Tier choices each saved project was generated with, so reopening restores them. */
const savedTiers = new Map<number, EventTiers>();
let nextProjectId = 100;

function detailFor(id: number): ProjectDetail {
  const listed = projects.find((project) => project.id === id);
  if (!listed) throw new Error(`no saved photobook with id ${id}`);
  return {
    id: listed.id,
    name: listed.name,
    sourceFolder: listed.sourceFolder,
    sourceFolders: listed.sourceFolders,
    createdAt: listed.createdAt,
    updatedAt: listed.updatedAt,
    pageCount: listed.pageCount,
    photoCount: listed.photoCount,
    droppedPhotos: layout.droppedPhotos,
    seed: layout.seed,
    overrides: savedOverrides.get(id) ?? {},
    tiers: savedTiers.get(id) ?? {},
    exports: listed.lastExport ? [listed.lastExport] : [],
  };
}

function keptPaths(overrides: PhotoOverrides): string[] {
  return photos
    .filter((photo) => {
      const decision = overrides[photo.hash];
      if (decision === "include") return true;
      if (decision === "exclude") return false;
      return photo.kept;
    })
    .map((photo) => photo.path);
}

/**
 * One length's plan, built from the mock's own 36 synthetic photos rather
 * than the static fixture -- so switching page counts in the harness visibly
 * changes what the contact sheet dims. This is NOT a stand-in for the
 * engine's tier/merit/budget logic (that lives only in Rust, in
 * `book::events`); it exists so `isPlaced` and the events panel have
 * something real to react to. It honours three things a screenshot check can
 * actually see:
 *  - an event the user set a tier on (`tiers` holds any of its photos' hashes)
 *    keeps that tier, with `chosen: true`;
 *  - a Skipped event's photos are left out of `selectedPaths`, except a photo
 *    marked **+** (`overrides[hash] === "include"`), which always places;
 *  - a Featured event is topped up to `options.featuredFloor` photos, from
 *    its own kept order, past the length's ordinary capacity if it must.
 * Every OTHER event still places a prefix of the kept order, so a longer
 * book's selection still grows outward from a shorter one's rather than
 * jumping to a differently-chosen set of the same size.
 */
function mockOptionFor(
  option: PageOption,
  keptOrdered: string[],
  overrides: PhotoOverrides,
  tiers: EventTiers,
  bookOptions: BookOptions,
): PageOption {
  const clusterIds = [...new Set(photos.map((photo) => photo.eventCluster))].toSorted((a, b) => a - b);
  const suggestedByRank: readonly Tier[] = ["featured", "normal", "brief", "skipped"];
  const REASON_BY_TIER: Record<Tier, EventRow["reason"]> = {
    featured: { kind: "standout" },
    normal: { kind: "ranked", rank: 2, of: clusterIds.length },
    brief: { kind: "outOfRoom", rank: 3, of: clusterIds.length },
    skipped: { kind: "nothingKept" },
  };

  /** The tier the user chose for this event, by looking up any one of its photos' hashes. */
  function chosenTierOf(inCluster: { hash: string }[]): Tier | null {
    for (const photo of inCluster) {
      const t = tiers[photo.hash];
      if (t) return t;
    }
    return null;
  }

  const eventsByTier: TierCounts = { featured: 0, normal: 0, brief: 0, skipped: 0 };
  const plans = clusterIds.map((event, index) => {
    const inCluster = photos.filter((photo) => photo.eventCluster === event);
    const keptInCluster = inCluster.filter((photo) => {
      const decision = overrides[photo.hash];
      if (decision === "include") return true;
      if (decision === "exclude") return false;
      return photo.kept;
    });
    const suggested = suggestedByRank[index % suggestedByRank.length] ?? "normal";
    const chosenTier = chosenTierOf(inCluster);
    const tier = chosenTier ?? suggested;
    eventsByTier[tier] += 1;
    return { event, tier, suggested, chosen: chosenTier !== null, inCluster, keptInCluster };
  });
  const tierOf = new Map(plans.map((p) => [p.event, p.tier]));

  // Build the selection: walk the kept order, skip a Skipped event's photos
  // unless the user marked them **+**, and stop once `capacityPhotos` is hit.
  const selectedPaths: string[] = [];
  for (const path of keptOrdered) {
    if (selectedPaths.length >= option.capacityPhotos) break;
    const photo = photos.find((p) => p.path === path);
    if (!photo) continue;
    const included = overrides[photo.hash] === "include";
    if (tierOf.get(photo.eventCluster) === "skipped" && !included) continue;
    selectedPaths.push(path);
  }
  // Top up every Featured event to its floor, past capacity if there is no
  // room left, so raising the stepper is visible even on a nearly-full book.
  const selected = new Set(selectedPaths);
  for (const plan of plans) {
    if (plan.tier !== "featured") continue;
    for (const photo of plan.keptInCluster) {
      const inThisEvent = plan.keptInCluster.filter((p) => selected.has(p.path)).length;
      if (inThisEvent >= bookOptions.featuredFloor) break;
      if (selected.has(photo.path)) continue;
      selected.add(photo.path);
      selectedPaths.push(photo.path);
    }
  }

  const events: EventRow[] = plans.map((plan) => ({
    event: plan.event,
    tier: plan.tier,
    suggested: plan.suggested,
    chosen: plan.chosen,
    reason: REASON_BY_TIER[plan.tier],
    merit: 0.5,
    moments: plan.inCluster.length,
    kept: plan.keptInCluster.length,
    photos: plan.inCluster.length,
    selected: plan.inCluster.filter((photo) => selected.has(photo.path)).length,
  }));

  return {
    ...option,
    droppedPhotos: Math.max(0, keptOrdered.length - selectedPaths.length),
    events,
    eventsByTier,
    selectedPaths,
    tierOverflow: null,
  };
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function placement(ref: PlacementRef) {
  const page = layout.pages.find((p) => p.number === ref.page);
  return page?.placements.find((pl) => pl.z === ref.z) ?? null;
}

/**
 * The one photo the harness refuses for every slot, so the dialog's dimmed
 * state can be looked at. A left-out photo in the fixture.
 */
const REFUSED_PHOTO = 4;

/** A centred crop at `want`, as `choose_crop` makes with no faces. */
function centred(photo: number, want: number): PreviewRect {
  const shot = layout.photos[photo];
  if (!shot) throw new Error(`this book has no photo ${photo}`);
  const have = shot.width / shot.height;
  const w = want < have ? want / have : 1;
  const h = want < have ? 1 : have / want;
  return { x: (1 - w) / 2, y: (1 - h) / 2, w, h };
}

/** A centred crop of the slot's printed shape. */
function centredCrop(ref: PlacementRef, photo: number): PreviewRect {
  const slot = placement(ref)?.slotRect;
  if (!slot) throw new Error("there is no photo at that slot");
  return centred(photo, (slot.w * layout.geometry.pageWIn) / (slot.h * layout.geometry.pageHIn));
}

/**
 * The one photo the harness refuses for the cover, so the picker's dimmed
 * state and the editor's refusal can be looked at. Not the front cover's own
 * photo, which is `REFUSED_PHOTO`.
 */
const REFUSED_COVER_PHOTO = 1;

function coverCandidates(): SlotCandidate[] {
  return layout.photos.map((_, index) => ({
    crop: centred(index, layout.cover.aspect),
    refused: index === REFUSED_COVER_PHOTO ? "faceInSafeMargin" : null,
  }));
}

function coverPhoto(side: CoverSide, photo: number) {
  const hash = layout.photos[photo]?.hash ?? "";
  return { photoIndex: photo, crop: centred(photo, layout.cover.aspect), filename: `cover-${side}-${hash.slice(0, 8)}` };
}

function slotCandidates(ref: PlacementRef): SlotCandidate[] {
  return layout.photos.map((_, index) => ({
    crop: centredCrop(ref, index),
    refused: index === REFUSED_PHOTO ? "faceInGutter" : null,
  }));
}

/**
 * The harness's copy of the timeline Rust keeps in SQLite beside the book --
 * see `book::history`. Whole snapshots, for the same reason: the edits below
 * rewrite the layout and none of them can be run backwards. The labels
 * mirror `book::history::edit_label`; `tests/tauri-mock.test.ts` pins them
 * to the Rust source so this cannot quietly drift out of step with the app.
 */
let past: { label: string; layout: BookLayout }[] = [];
let future: { label: string; layout: BookLayout }[] = [];

/**
 * Mirrors Rust's `book::history::edit_label`, including that `before` is the
 * book as it stands: a `setSlot` is a move or a resize depending on the size
 * the slot HAD, which the edit does not carry.
 */
export function editLabel(edit: BookEdit, before: BookLayout): string {
  switch (edit.kind) {
    case "regenerate":
      return "regenerate";
    case "rejectTemplate":
      return "reject layout";
    case "setTemplate":
      return "layout change";
    case "setLocked":
      return edit.locked ? "lock" : "unlock";
    case "shuffle":
      return "shuffle";
    case "swapPhotos":
      return "swap";
    case "setCrop":
      return "crop";
    case "setSlot": {
      const was = before.pages
        .find((page) => page.number === edit.placement.page)
        ?.placements.find((pl) => pl.z === edit.placement.z)?.slotRect;
      const sameSize =
        was !== undefined &&
        Math.abs(was.w - edit.rect.w) < 1e-9 &&
        Math.abs(was.h - edit.rect.h) < 1e-9;
      return sameSize ? "move" : "resize";
    }
    case "replacePhoto":
      return "photo replacement";
    case "setPrintSpec":
      return "print size change";
    case "setCoverPhoto":
      return edit.photo === null ? "cover photo removal" : "cover photo";
    case "setCoverCrop":
      return "cover crop";
    case "setSpineColour":
      return "spine colour";
  }
}

function stepHistory(step: HistoryStep): BookLayout {
  const from = step === "undo" ? past : future;
  const onto = step === "undo" ? future : past;
  const entry = from.pop();
  if (entry) {
    onto.push({ label: entry.label, layout });
    layout = entry.layout;
  }
  return structuredClone(layout);
}

function historyStatus(): HistoryStatus {
  return { undo: past.at(-1)?.label ?? null, redo: future.at(-1)?.label ?? null };
}

function applyEdit(edit: BookEdit): BookLayout {
  const before = structuredClone(layout);
  try {
    applyEditTo(edit);
  } catch (refused) {
    // A refused edit changes nothing, here as in Rust: the screen keeps
    // showing what is saved and the timeline never hears about it. Every
    // case below happens to validate before it touches `layout`, so no test
    // can tell this line from its absence; it is here so the first case that
    // does not cannot leave the harness showing a half-applied edit.
    layout = before;
    throw refused;
  }
  past.push({ label: editLabel(edit, before), layout: before });
  future = [];
  return structuredClone(layout);
}

function applyEditTo(edit: BookEdit): void {
  switch (edit.kind) {
    case "setLocked": {
      const opening = layout.openings[edit.opening];
      if (opening) opening.locked = edit.locked;
      break;
    }
    case "regenerate":
    case "rejectTemplate":
    case "setTemplate": {
      const opening = layout.openings[edit.opening];
      if (!opening) throw new Error(`this book has no opening ${edit.opening}`);
      if (opening.locked) throw new Error("this spread is locked; unlock it to change it");
      const next =
        edit.kind === "setTemplate" ? edit.templateId : opening.alternatives[0];
      if (!next) throw new Error("every layout for 2 photos has been shown or rejected here");
      const pages = edit.opening === 1 ? [layout.pages[1], layout.pages[2]] : [];
      const current = pages[0]?.templateId ?? "";
      for (const page of pages) if (page) page.templateId = next;
      opening.alternatives = opening.alternatives.filter((id) => id !== next).concat(current);
      if (edit.kind === "rejectTemplate") {
        opening.rejected.push(current);
        opening.alternatives = opening.alternatives.filter((id) => id !== current);
      }
      break;
    }
    case "shuffle":
      break;
    case "setCrop": {
      const target = placement(edit.placement);
      if (!target) throw new Error("there is no photo at that slot");
      const shape = target.crop.h / target.crop.w;
      if (edit.x < 0 || edit.y < 0 || edit.x + edit.w > 1 || edit.y + edit.w * shape > 1) {
        throw new Error("the crop window has to stay inside the photo");
      }
      target.crop = { x: edit.x, y: edit.y, w: edit.w, h: edit.w * shape };
      break;
    }
    case "setSlot": {
      const target = placement(edit.placement);
      if (!target) throw new Error("there is no photo at that slot");
      const r = edit.rect;
      if (r.x < 0 || r.y < 0 || r.x + r.w > 1.000001 || r.y + r.h > 1.000001 || r.w < 0.05 || r.h < 0.05) {
        throw new Error("a slot has to stay on the page and be at least 5% of it each way");
      }
      const page = layout.pages.find((p) => p.number === edit.placement.page);
      for (const other of page?.placements ?? []) {
        if (other === target) continue;
        const o = other.slotRect;
        const overlapW = Math.min(r.x + r.w, o.x + o.w) - Math.max(r.x, o.x);
        const overlapH = Math.min(r.y + r.h, o.y + o.h) - Math.max(r.y, o.y);
        if (overlapW > 0.0005 && overlapH > 0.0005) {
          throw new Error(`that would overlap the photo at page ${page?.number} slot ${other.z}`);
        }
      }
      target.slotRect = { ...r };
      break;
    }
    case "swapPhotos": {
      const a = placement(edit.a);
      const b = placement(edit.b);
      if (!a || !b) throw new Error("there is no photo at that slot");
      [a.photoIndex, b.photoIndex] = [b.photoIndex, a.photoIndex];
      break;
    }
    case "setPrintSpec": {
      const recrop = edit.spec.pageWIn / edit.spec.pageHIn !== layout.spec.pageWIn / layout.spec.pageHIn;
      const recutCover = coverAspect(edit.spec) !== coverAspect(layout.spec);
      layout.spec = { ...edit.spec };
      layout.geometry = mockGeometry(layout.spec);
      layout.cover.aspect = coverAspect(layout.spec);
      for (const side of ["front", "back"] as const) {
        const current = layout.cover[side].photo;
        layout.cover[side] = {
          ...coverRects(layout.spec, side),
          photo: current && recutCover ? coverPhoto(side, current.photoIndex) : current,
        };
      }
      if (recrop) {
        for (const page of layout.pages) {
          for (const pl of page.placements) pl.crop = centredCrop({ page: page.number, z: pl.z }, pl.photoIndex);
        }
      }
      break;
    }
    case "replacePhoto": {
      const target = placement(edit.placement);
      if (!target) throw new Error("there is no photo at that slot");
      if (edit.photo === REFUSED_PHOTO) {
        throw new Error("that photo won't fit there: a face would fall in the fold");
      }
      const other = layout.pages
        .flatMap((page) => page.placements.map((pl) => ({ page: page.number, pl })))
        .find(({ pl }) => pl.photoIndex === edit.photo);
      if (other) {
        other.pl.photoIndex = target.photoIndex;
        other.pl.crop = centredCrop({ page: other.page, z: other.pl.z }, other.pl.photoIndex);
      }
      target.photoIndex = edit.photo;
      target.crop = centredCrop(edit.placement, edit.photo);
      break;
    }
    case "setCoverPhoto": {
      if (edit.photo === REFUSED_COVER_PHOTO) {
        throw new Error(
          `on the ${edit.side} cover that would put a face where the cover folds under the board or too near its edge`,
        );
      }
      layout.cover[edit.side].photo = edit.photo === null ? null : coverPhoto(edit.side, edit.photo);
      break;
    }
    case "setCoverCrop": {
      const current = layout.cover[edit.side].photo;
      if (!current) throw new Error(`the ${edit.side} cover has no photo to crop`);
      const shape = current.crop.h / current.crop.w;
      if (edit.x < 0 || edit.y < 0 || edit.x + edit.w > 1 || edit.y + edit.w * shape > 1) {
        throw new Error("the crop window has to stay inside the photo");
      }
      current.crop = { x: edit.x, y: edit.y, w: edit.w, h: edit.w * shape };
      break;
    }
    case "setSpineColour": {
      if (!/^#[0-9a-f]{6}$/i.test(edit.rgb)) throw new Error(`${edit.rgb} is not a #rrggbb colour`);
      layout.cover.spine = edit.rgb.toLowerCase();
      break;
    }
  }
}

/**
 * Which keys the keychain holds. Both start unset, so the chat panel opens on
 * its no-key prompt and the path through Settings is driven like a new user's.
 */
const keys: Record<ModelProvider, boolean> = { deepseek: false, jev: false };

const fixturePhotos = (agentViewFixture as AgentView).photos;

/** `agent_view` over the harness's book, shaped as `agent::view::agent_view` shapes it. */
function agentView(): AgentView {
  const placed = new Set(layout.pages.flatMap((page) => page.placements.map((p) => p.photoIndex)));
  const openings = layout.openings.map((opening) => {
    const numbers = opening.index === 0 ? [1] : [opening.index * 2, opening.index * 2 + 1];
    const pages = layout.pages.filter((page) => numbers.includes(page.number));
    return {
      index: opening.index,
      pages: pages.map((page) => page.number),
      templateId: pages[0]?.templateId ?? "blank",
      locked: opening.locked,
      alternatives: [...opening.alternatives],
      slots: pages.flatMap((page) =>
        page.placements.map((p) => ({ page: page.number, z: p.z, photo: p.photoIndex })),
      ),
    };
  });
  const viewPhotos: AgentPhoto[] = layout.photos.map((_, id) => ({
    ...(fixturePhotos[id] ?? fixturePhotos[0]!),
    id,
    placed: placed.has(id),
  }));
  return {
    pageCount: layout.pageCount,
    placedPhotos: placed.size,
    droppedPhotos: layout.droppedPhotos,
    openings,
    photos: viewPhotos,
  };
}

/** `agent_edit`: the new view, or the refusal `agent::edit` rejects with. */
function agentEdit(edit: BookEdit): AgentView {
  try {
    applyEdit(edit);
  } catch (error) {
    throw { kind: "refused", reason: error instanceof Error ? error.message : String(error) };
  }
  return agentView();
}

declare global {
  interface Window {
    /** Milliseconds between streamed batches. Raise it to leave a draft mid-run. */
    pbgAnalysisBatchMs?: number;
    /** Photos in the analysed folder. Raise it to see how the screens cope with a large one. */
    pbgAnalysisPhotoCount?: number;
  }
}

let nextRunId = 1;

/** Streams `analyze_folders`'s events at a speed a person can watch. */
async function analyze(channel: Channel<AnalysisEvent>): Promise<AnalysisSummary> {
  const runId = nextRunId++;
  const batchMs = globalThis.window?.pbgAnalysisBatchMs ?? 220;
  const count = globalThis.window?.pbgAnalysisPhotoCount;
  const runPhotos = count ? mockPhotos(count) : photos;
  channel.onmessage({ kind: "scanned", total: runPhotos.length + 2 });
  await sleep(250);

  let analysed = 0;
  const batchSize = count ? 16 : 6;
  for (let start = 0; start < runPhotos.length; start += batchSize) {
    const batch = runPhotos.slice(start, start + batchSize);
    analysed += batch.length;
    channel.onmessage({ kind: "batch", photos: batch, analysed, cached: 0, failed: 0 });
    await sleep(batchMs);
  }

  // Paths are left exactly as `photos.ts` wrote them. `apply_photo_overrides`
  // answers with kept PATHS, and the contact sheet matches on them, so
  // rewriting one and not the other silently drops every photo from the book.
  const summary: AnalysisSummary = {
    runId,
    total: runPhotos.length + 2,
    failed: 2,
    cached: 0,
    photos: runPhotos,
  };
  channel.onmessage({ kind: "done", summary });
  return summary;
}

async function runExport(channel: Channel<ExportEvent>): Promise<ExportResult> {
  const total = layout.placedPhotos;
  channel.onmessage({ kind: "started", total });
  for (let completed = 1; completed <= total; completed += 1) {
    await sleep(60);
    channel.onmessage({ kind: "progress", completed, total });
  }
  return structuredClone(exportFixture) as ExportResult;
}

function coverFrom(start: number): string[] {
  return photos
    .slice(start, start + 4)
    .map((photo) => photo.thumbnailPath)
    .filter((path): path is string => typeof path === "string");
}

type Args = Record<string, unknown> | undefined;

const DRAFTS_KEY = "pbg-mock-drafts";
/** Set to any value to analyse a folder whose photos carry no location. */
const NO_GPS_KEY = "pbg-mock-no-gps";

const MiB = 1024 * 1024;
const cache = { usedBytes: 1450 * MiB, pinnedBytes: 610 * MiB, limitBytes: 2048 * MiB };

function cacheStatus() {
  return { ...cache, overBudget: cache.usedBytes > cache.limitBytes };
}

function evictTo(limit: number) {
  cache.usedBytes = Math.max(cache.pinnedBytes, Math.min(cache.usedBytes, limit));
}

function savedDrafts(): Record<number, string> {
  try {
    return JSON.parse(localStorage.getItem(DRAFTS_KEY) ?? "{}") as Record<number, string>;
  } catch {
    return {};
  }
}

function writeDrafts(drafts: Record<number, string>) {
  localStorage.setItem(DRAFTS_KEY, JSON.stringify(drafts));
}

export async function invoke<T>(command: string, args?: Args): Promise<T> {
  switch (command) {
    case "list_projects":
      return structuredClone(projects) as T;

    case "open_project":
      return detailFor(args?.id as number) as T;

    case "book_layout":
      return structuredClone(layout) as T;

    // The real command hashes the file, analyses it through the sidecar and
    // APPENDS it to the book's photo list, so `slot_candidates` comes back one
    // longer than the layout the dialog is holding. The harness appended
    // nothing and handed back the last existing photo, which made a broken
    // import look like a working one here -- the whole point of the feature is
    // a photo the book did not have. Set `localStorage.PBG_MOCK_IMPORT_FAILS`
    // to drive the failure path instead.
    case "import_photo": {
      await sleep(400);
      // Optional-chained, unlike the flags above: this case is the one
      // `tests/tauri-mock.test.ts` calls, and vitest runs it with no DOM.
      if (globalThis.localStorage?.getItem("PBG_MOCK_IMPORT_FAILS") === "1") {
        throw new Error("from-disk.jpg could not be analysed.");
      }
      const path = args?.path as string;
      const known = layout.photos.findIndex((photo) => photo.path === path);
      if (known !== -1) {
        return { photoIndex: known, alreadyKnown: true, photo: structuredClone(layout.photos[known]!) } as T;
      }
      const photo = { ...structuredClone(layout.photos[0]!), path, hash: `imported-${layout.photos.length}` };
      // Onto the snapshots as well as the live book: in Rust the photo list is
      // `project_photos`, which is not part of the `Book` the timeline saves,
      // so undoing past the import cannot take the photo back out.
      for (const snapshot of [...past, ...future]) snapshot.layout.photos.push(structuredClone(photo));
      const photoIndex = layout.photos.push(photo) - 1;
      return { photoIndex, alreadyKnown: false, photo: structuredClone(photo) } as T;
    }

    // The real command stats every file in the book's folders. There are no
    // folders here, so the harness just reports a plausible answer; set
    // `localStorage.PBG_MOCK_NEW_PHOTOS` or `PBG_MOCK_UNANALYSABLE` to drive the notice.
    case "folder_check":
      return {
        newPhotos: Number(localStorage.getItem("PBG_MOCK_NEW_PHOTOS") ?? 0),
        unanalysable: Number(localStorage.getItem("PBG_MOCK_UNANALYSABLE") ?? 0),
        unreadable: localStorage.getItem("PBG_MOCK_FOLDER_UNREADABLE") === "1",
      } as T;

    case "edit_book":
      return applyEdit(args?.edit as BookEdit) as T;

    case "step_book":
      return stepHistory(args?.step as HistoryStep) as T;

    case "book_history":
      return historyStatus() as T;

    case "slot_candidates":
      await sleep(250);
      return slotCandidates(args?.placement as PlacementRef) as T;

    case "cover_candidates":
      await sleep(250);
      return coverCandidates() as T;

    case "rename_project": {
      const project = projects.find((p) => p.id === (args?.id as number));
      if (!project) throw new Error("that photobook is no longer saved");
      project.name = args?.name as string;
      project.updatedAt = Math.floor(Date.now() / 1000);
      return undefined as T;
    }

    case "set_favourite": {
      const project = projects.find((p) => p.id === (args?.id as number));
      if (!project) throw new Error(`project ${args?.id as number} no longer exists`);
      project.favourite = args?.favourite as boolean;
      return undefined as T;
    }

    case "delete_project": {
      const index = projects.findIndex((p) => p.id === (args?.id as number));
      if (index === -1) throw new Error("that photobook is no longer saved");
      trash.push(...projects.splice(index, 1).map((project) => ({ project, index })));
      return undefined as T;
    }

    case "restore_project": {
      const at = trash.findIndex((t) => t.project.id === (args?.id as number));
      if (at === -1) throw new Error(`project ${args?.id as number} no longer exists`);
      for (const { project, index } of trash.splice(at, 1)) projects.splice(index, 0, project);
      return undefined as T;
    }

    case "analyze_folders":
      return (await analyze(args?.onEvent as Channel<AnalysisEvent>)) as T;

    case "forget_run":
      return undefined as T;

    // Kept in localStorage, so reloading the page is the mock's restart.
    case "list_drafts":
      return Object.values(savedDrafts()) as T;

    case "save_draft":
      writeDrafts({ ...savedDrafts(), [args?.id as number]: args?.json as string });
      return undefined as T;

    case "delete_draft": {
      const drafts = savedDrafts();
      delete drafts[args?.id as number];
      writeDrafts(drafts);
      return undefined as T;
    }

    case "apply_photo_overrides":
      return keptPaths((args?.overrides as PhotoOverrides) ?? {}) as T;

    case "recommend_book": {
      const overrides = (args?.overrides as PhotoOverrides) ?? {};
      const tiers = (args?.tiers as EventTiers) ?? {};
      const bookOptions = (args?.options as BookOptions) ?? DEFAULT_BOOK_OPTIONS;
      const base = structuredClone(recommendationFixture) as BookRecommendation;
      const keptOrdered = keptPaths(overrides);
      return {
        ...base,
        keeperCount: keptOrdered.length,
        includedCount: Object.values(overrides).filter((state) => state === "include").length,
        options: base.options.map((option) => mockOptionFor(option, keptOrdered, overrides, tiers, bookOptions)),
      } as T;
    }

    case "place_chapters":
      return mockPlaceChapters(localStorage.getItem(NO_GPS_KEY) !== null) as T;

    case "place_names":
      await sleep(300);
      return mockPlaceNames(localStorage.getItem(NO_GPS_KEY) !== null) as T;

    case "default_print_spec":
      return structuredClone(PIXAJOY) as T;

    case "check_print_spec":
      await sleep(150);
      return mockCheck(args?.spec as PrintSpec, args?.projectId != null) as T;

    case "generate_book": {
      await sleep(400);
      // Cloned on the way in: the webview hands over a Vue reactive proxy, and
      // `structuredClone` on the next `list_projects` throws `DataCloneError`
      // if the proxy is what got stored.
      const folders = [...((args?.sourceFolders as string[]) ?? [])];
      const id = nextProjectId++;
      projects.unshift({
        id,
        name: (args?.name as string) || "Untitled photobook",
        sourceFolder: folders[0] ?? "/mock",
        sourceFolders: folders,
        pageCount: (args?.pages as number) ?? layout.pageCount,
        photoCount: layout.placedPhotos,
        createdAt: Math.floor(Date.now() / 1000),
        updatedAt: Math.floor(Date.now() / 1000),
        lastExport: null,
        coverThumbnails: coverFrom(0),
      });
      savedOverrides.set(id, { ...(args?.overrides as PhotoOverrides | undefined) });
      savedTiers.set(id, { ...(args?.tiers as EventTiers | undefined) });
      const book: GeneratedBook = {
        projectId: id,
        pageCount: (args?.pages as number) ?? layout.pageCount,
        placedPhotos: layout.placedPhotos,
        droppedPhotos: layout.droppedPhotos,
        seed: layout.seed,
      };
      return book as T;
    }

    case "export_book": {
      const result = await runExport(args?.onEvent as Channel<ExportEvent>);
      const project = projects.find((p) => p.id === (args?.projectId as number));
      if (project) {
        project.lastExport = {
          at: Math.floor(Date.now() / 1000),
          outputDir: args?.outputDir as string,
          format: result.format,
          fileCount: result.written.length,
        };
      }
      return { ...result, outputDir: (args?.outputDir as string) ?? result.outputDir } as T;
    }

    case "reveal_in_finder":
      return undefined as T;

    case "agent_view":
      return agentView() as T;

    case "agent_edit":
      await sleep(300);
      return agentEdit(args?.edit as BookEdit) as T;

    case "api_key_status":
      return { ...keys } as T;

    case "set_api_key":
      if (!String(args?.key ?? "").trim()) throw new Error("the key is empty");
      keys[args?.provider as ModelProvider] = true;
      return undefined as T;

    case "clear_api_key":
      keys[args?.provider as ModelProvider] = false;
      return undefined as T;

    case "model_request": {
      const provider = args?.provider as ModelProvider;
      if (!keys[provider]) {
        throw { kind: "missingKey", provider, message: `No ${provider} API key is set.` };
      }
      return (await modelRequest({
        id: args?.id as string,
        provider: args?.provider as ModelRequestArgs["provider"],
        path: args?.path as string,
        body: args?.body as string,
        onEvent: args?.onEvent as ModelRequestArgs["onEvent"],
      })) as T;
    }

    case "cache_status":
      return cacheStatus() as T;

    case "set_cache_limit":
      cache.limitBytes = args?.limitBytes as number;
      evictTo(cache.limitBytes);
      return cacheStatus() as T;

    case "clear_unused_cache":
      await sleep(400);
      evictTo(0);
      return cacheStatus() as T;

    case "cancel_model_request":
      return cancelModelRequest(args?.id as string) as T;

    default:
      throw new Error(`${command} is not available in the browser harness`);
  }
}

export function convertFileSrc(path: string): string {
  return path;
}

export class Channel<T> {
  onmessage: (message: T) => void = () => {};
}
