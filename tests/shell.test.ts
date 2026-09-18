import { describe, expect, it } from "vitest";
import type { ProjectListItem } from "../app/types/book";
import { libraryView } from "../app/types/library";
import { SHORTCUTS, shortcutCombo } from "../app/types/shortcuts";

function project(id: number, name: string, createdAt: number, updatedAt: number): ProjectListItem {
  return {
    id,
    name,
    sourceFolder: `/Volumes/Card ${id}`,
    sourceFolders: [`/Volumes/Card ${id}`, `/Volumes/Trip-${id}`],
    pageCount: 20,
    photoCount: 24,
    createdAt,
    updatedAt,
    lastExport: null,
    coverThumbnails: [],
  };
}

// Three orders that all differ, so each sort is distinguishable from the
// others. The names are cased so that byte order ("Bali", "Kyoto", "arles")
// is wrong, and no folder contains a name, so a name match and a folder match
// cannot stand in for each other.
const books = [
  project(1, "Kyoto", 300, 100),
  project(2, "arles", 100, 300),
  project(3, "Bali", 200, 200),
];

describe("libraryView", () => {
  it("sorts by last edit, newest first", () => {
    expect(libraryView(books, "", "updated").map((p) => p.id)).toEqual([2, 3, 1]);
  });

  it("sorts by creation, newest first", () => {
    expect(libraryView(books, "", "created").map((p) => p.id)).toEqual([1, 3, 2]);
  });

  it("sorts by name ignoring case", () => {
    expect(libraryView(books, "", "name").map((p) => p.name)).toEqual(["arles", "Bali", "Kyoto"]);
  });

  it("matches the name or any folder, ignoring case and surrounding space", () => {
    expect(libraryView(books, "  KYO ", "name").map((p) => p.id)).toEqual([1]);
    expect(libraryView(books, "trip-3", "name").map((p) => p.id)).toEqual([3]);
    expect(libraryView(books, "nowhere", "name")).toEqual([]);
  });

  it("does not reorder the list it was given", () => {
    const input = [...books];
    libraryView(input, "", "name");
    expect(input.map((p) => p.id)).toEqual([1, 2, 3]);
  });
});

describe("shortcuts", () => {
  it("binds each as defineShortcuts spells it", () => {
    expect(shortcutCombo("newBook")).toBe("meta_n");
    expect(shortcutCombo("settings")).toBe("meta_,");
  });

  it("never binds two actions to one key", () => {
    const combos = (Object.keys(SHORTCUTS) as (keyof typeof SHORTCUTS)[]).map(shortcutCombo);
    expect(new Set(combos).size).toBe(combos.length);
  });
});
