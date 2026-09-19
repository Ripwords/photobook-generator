import { describe, expect, it, vi } from "vitest";
import type { ProjectListItem } from "../app/types/book";
import { bookActions, sidebarSections, type BookActionHandlers } from "../app/types/library";

function book(overrides: Partial<ProjectListItem> = {}): ProjectListItem {
  return {
    id: 7,
    name: "Japan 2026",
    sourceFolder: "/Users/jj/Pictures/Japan",
    sourceFolders: ["/Users/jj/Pictures/Japan"],
    pageCount: 20,
    photoCount: 24,
    createdAt: 1,
    updatedAt: 2,
    favourite: false,
    lastExport: null,
    coverThumbnails: [],
    ...overrides,
  };
}

function handlers(): BookActionHandlers {
  return { open: vi.fn(), favourite: vi.fn(), rename: vi.fn(), reveal: vi.fn(), delete: vi.fn() };
}

function labels(groups: ReturnType<typeof bookActions>): (string | undefined)[][] {
  return groups.map((group) => group.map((entry) => entry.label));
}

function item(groups: ReturnType<typeof bookActions>, label: string) {
  const found = groups.flat().find((entry) => entry.label === label);
  if (!found) throw new Error(`no "${label}" in ${JSON.stringify(labels(groups))}`);
  return found;
}

function select(entry: { onSelect?: (event: Event) => void }) {
  entry.onSelect?.(new Event("select"));
}

describe("a book's actions", () => {
  it("offers to star a book that is not starred, and to unstar one that is", () => {
    const on = handlers();
    select(item(bookActions(book(), on), "Add to favourites"));
    expect(on.favourite).toHaveBeenCalledWith(7, true);

    select(item(bookActions(book({ favourite: true }), on), "Remove from favourites"));
    expect(on.favourite).toHaveBeenLastCalledWith(7, false);
  });

  it("opens, renames and deletes the book it was built for", () => {
    const on = handlers();
    const menu = bookActions(book({ id: 12 }), on);
    select(item(menu, "Open"));
    select(item(menu, "Rename…"));
    select(item(menu, "Delete…"));
    expect(on.open).toHaveBeenCalledWith(12);
    expect(on.rename).toHaveBeenCalledWith(12);
    expect(on.delete).toHaveBeenCalledWith(12);
  });

  it("keeps Delete apart, last, and marked destructive", () => {
    const menu = bookActions(book(), handlers());
    expect(menu.at(-1)?.map((entry) => entry.label)).toEqual(["Delete…"]);
    expect(item(menu, "Delete…").color).toBe("error");
  });

  it("shows a one-folder book's photos directly", () => {
    const on = handlers();
    select(item(bookActions(book(), on), "Show photos in Finder"));
    expect(on.reveal).toHaveBeenCalledWith("/Users/jj/Pictures/Japan");
  });

  it("lets a book drawn from several folders show any one of them", () => {
    const on = handlers();
    const entry = item(
      bookActions(book({ sourceFolders: ["/p/Tokyo", "/p/Kyoto"] }), on),
      "Show photos in Finder",
    );
    expect(entry.onSelect).toBeUndefined();
    const children = entry.children ?? [];
    expect(children.map((child) => child.label)).toEqual(["Tokyo", "Kyoto"]);
    select(children[1]!);
    expect(on.reveal).toHaveBeenCalledWith("/p/Kyoto");
  });

  it("shows the last export only for a book that has one", () => {
    expect(bookActions(book(), handlers()).flat().map((entry) => entry.label)).not.toContain(
      "Show export in Finder",
    );

    const on = handlers();
    const exported = book({
      lastExport: { at: 3, outputDir: "/Users/jj/Desktop/out", format: "jpg", fileCount: 24 },
    });
    select(item(bookActions(exported, on), "Show export in Finder"));
    expect(on.reveal).toHaveBeenCalledWith("/Users/jj/Desktop/out");
  });

  it("disables every action that changes the book while another is in flight", () => {
    const menu = bookActions(book({ lastExport: { at: 3, outputDir: "/o", format: "jpg", fileCount: 1 } }), handlers(), true);
    for (const label of ["Open", "Add to favourites", "Rename…", "Delete…"]) {
      expect(item(menu, label).disabled, label).toBe(true);
    }
  });
});

describe("the sidebar's sections", () => {
  it("lists starred books on their own, each book once, in the order given", () => {
    const books = [
      book({ id: 1, favourite: false }),
      book({ id: 2, favourite: true }),
      book({ id: 3, favourite: false }),
      book({ id: 4, favourite: true }),
    ];
    const { favourites, others } = sidebarSections(books);
    expect(favourites.map((b) => b.id)).toEqual([2, 4]);
    expect(others.map((b) => b.id)).toEqual([1, 3]);
  });
});
