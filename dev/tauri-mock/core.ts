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
import type { BookEdit, BookLayout, PlacementRef } from "../../app/types/preview";
import type {
  BookRecommendation,
  ExportEvent,
  ExportResult,
  GeneratedBook,
  ProjectDetail,
  ProjectListItem,
} from "../../app/types/book";
import type {
  AnalysisEvent,
  AnalysisSummary,
  AnalyzedPhoto,
  PhotoOverrides,
} from "../../app/types/features";
import { cancelModelRequest, modelRequest, type ModelRequestArgs } from "./model";
import { mockPhotos, thumbnail } from "./photos";

const layout: BookLayout = structuredClone(layoutFixture) as BookLayout;
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

const projects: ProjectListItem[] = structuredClone(listFixture) as ProjectListItem[];
/** Decisions each saved project was generated with, so reopening restores them. */
const savedOverrides = new Map<number, PhotoOverrides>();
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

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function placement(ref: PlacementRef) {
  const page = layout.pages.find((p) => p.number === ref.page);
  return page?.placements.find((pl) => pl.z === ref.z) ?? null;
}

function applyEdit(edit: BookEdit): BookLayout {
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
  }
  return structuredClone(layout);
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

/** Streams `analyze_folders`'s events at a speed a person can watch. */
async function analyze(channel: Channel<AnalysisEvent>): Promise<AnalysisSummary> {
  channel.onmessage({ kind: "scanned", total: photos.length + 2 });
  await sleep(250);

  let analysed = 0;
  for (let start = 0; start < photos.length; start += 6) {
    const batch = photos.slice(start, start + 6);
    analysed += batch.length;
    channel.onmessage({ kind: "batch", photos: batch, analysed, cached: 0, failed: 0 });
    await sleep(220);
  }

  // Paths are left exactly as `photos.ts` wrote them. `apply_photo_overrides`
  // answers with kept PATHS, and the contact sheet matches on them, so
  // rewriting one and not the other silently drops every photo from the book.
  const summary: AnalysisSummary = {
    runId: 1,
    total: photos.length + 2,
    failed: 2,
    cached: 0,
    photos: viewPhotos,
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

type Args = Record<string, unknown> | undefined;

export async function invoke<T>(command: string, args?: Args): Promise<T> {
  switch (command) {
    case "list_projects":
      return structuredClone(projects) as T;

    case "open_project":
      return detailFor(args?.id as number) as T;

    case "book_layout":
      return structuredClone(layout) as T;

    case "edit_book":
      return applyEdit(args?.edit as BookEdit) as T;

    case "rename_project": {
      const project = projects.find((p) => p.id === (args?.id as number));
      if (!project) throw new Error("that photobook is no longer saved");
      project.name = args?.name as string;
      project.updatedAt = Math.floor(Date.now() / 1000);
      return undefined as T;
    }

    case "delete_project": {
      const index = projects.findIndex((p) => p.id === (args?.id as number));
      if (index === -1) throw new Error("that photobook is no longer saved");
      projects.splice(index, 1);
      return undefined as T;
    }

    case "analyze_folders":
      return (await analyze(args?.onEvent as Channel<AnalysisEvent>)) as T;

    case "apply_photo_overrides":
      return keptPaths((args?.overrides as PhotoOverrides) ?? {}) as T;

    case "recommend_book": {
      const overrides = (args?.overrides as PhotoOverrides) ?? {};
      const base = structuredClone(recommendationFixture) as BookRecommendation;
      return {
        ...base,
        keeperCount: keptPaths(overrides).length,
        includedCount: Object.values(overrides).filter((state) => state === "include").length,
      } as T;
    }

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
      });
      savedOverrides.set(id, { ...(args?.overrides as PhotoOverrides | undefined) });
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
