/**
 * A stand-in for `@tauri-apps/api/core` so the webview can be opened in an
 * ordinary browser and LOOKED AT, driven by a browser automation tool, with
 * no Tauri process behind it.
 *
 * `bun run ui:mock` aliases the real module to this one (see
 * `nuxt.config.ts`). Only the commands the book preview needs are answered,
 * from the same wire fixture the Rust and TypeScript suites pin; everything
 * else rejects loudly, so a screen that depends on analysis cannot be
 * mistaken for working. Edits are applied to the in-memory copy just far
 * enough to make each control's effect visible: lock toggles, regenerate and
 * choose-layout rename the template, swap exchanges the two photo indices.
 *
 * Never imported by the app itself. It exists so a change to `BookPreview`
 * can be rasterised and inspected before it ships.
 */
import fixture from "../../tests/fixtures/wire/book-layout.json";
import type { BookEdit, BookLayout, PlacementRef } from "../../app/types/preview";
import type { ProjectDetail, ProjectListItem } from "../../app/types/book";

const layout: BookLayout = structuredClone(fixture) as BookLayout;
// The fixture is pinned against an EMPTY library, so it offers no alternative
// layouts. Give the spread two so the buttons have something to do.
layout.openings[1] = {
  index: 1,
  locked: false,
  rejected: [],
  alternatives: ["07-two-up-symmetric-margin", "08-two-up-symmetric-bleed-outer"],
};

const project: ProjectDetail = {
  id: 7,
  name: "Mock book",
  sourceFolder: "/mock/Holiday 2026",
  sourceFolders: ["/mock/Holiday 2026", "/mock/Phone camera roll"],
  createdAt: 1_757_000_000,
  updatedAt: 1_757_000_000,
  pageCount: layout.pageCount,
  photoCount: layout.placedPhotos,
  droppedPhotos: layout.droppedPhotos,
  seed: layout.seed,
  overrides: {},
  exports: [],
};

const listed: ProjectListItem = {
  id: project.id,
  name: project.name,
  sourceFolder: project.sourceFolder,
  sourceFolders: project.sourceFolders,
  pageCount: project.pageCount,
  photoCount: project.photoCount,
  createdAt: project.createdAt,
  updatedAt: project.updatedAt,
  lastExport: null,
};

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

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  switch (command) {
    case "list_projects":
      return [listed] as T;
    case "open_project":
      return project as T;
    case "book_layout":
      return structuredClone(layout) as T;
    case "edit_book":
      return applyEdit(args?.edit as BookEdit) as T;
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
