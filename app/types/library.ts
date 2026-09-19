import type { ContextMenuItem, DropdownMenuItem } from "@nuxt/ui";
import { defaultProjectName, type ProjectListItem } from "./book";

export type LibrarySort = "updated" | "created" | "name";

export const LIBRARY_SORTS: { value: LibrarySort; label: string }[] = [
  { value: "updated", label: "Last edited" },
  { value: "created", label: "Date created" },
  { value: "name", label: "Name" },
];

const byName = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/**
 * The library as shown: the books matching `query` by name or by any source
 * folder, in the chosen order. Returns a new array; the list Rust sent stays
 * as it came.
 */
export function libraryView(
  projects: ProjectListItem[],
  query: string,
  sort: LibrarySort,
): ProjectListItem[] {
  const needle = query.trim().toLowerCase();
  const matching = needle
    ? projects.filter(
        (p) =>
          p.name.toLowerCase().includes(needle) ||
          p.sourceFolders.some((folder) => folder.toLowerCase().includes(needle)),
      )
    : [...projects];
  switch (sort) {
    case "updated":
      return matching.toSorted((a, b) => b.updatedAt - a.updatedAt);
    case "created":
      return matching.toSorted((a, b) => b.createdAt - a.createdAt);
    case "name":
      return matching.toSorted((a, b) => byName.compare(a.name, b.name));
  }
}

/** The sidebar's two lists: starred books, then the rest, each in the order given. */
export function sidebarSections(projects: ProjectListItem[]): {
  favourites: ProjectListItem[];
  others: ProjectListItem[];
} {
  return {
    favourites: projects.filter((p) => p.favourite),
    others: projects.filter((p) => !p.favourite),
  };
}

export type BookMenuItem = DropdownMenuItem & ContextMenuItem;

/** What each of a book's actions does; the caller owns the dialogs and the invokes. */
export interface BookActionHandlers {
  open: (id: number) => void;
  favourite: (id: number, favourite: boolean) => void;
  rename: (id: number) => void;
  reveal: (path: string) => void;
  delete: (id: number) => void;
}

/**
 * One book's actions, for the library card's menus and the sidebar's right
 * click alike, so the two never offer different things. Revealing in Finder
 * changes nothing, so it stays available while another action is in flight.
 */
export function bookActions(
  project: ProjectListItem,
  on: BookActionHandlers,
  busy = false,
): BookMenuItem[][] {
  const [onlyFolder] = project.sourceFolders;
  const photos: BookMenuItem =
    project.sourceFolders.length > 1
      ? {
          label: "Show photos in Finder",
          icon: "i-lucide-folder",
          children: project.sourceFolders.map((folder) => ({
            label: defaultProjectName(folder),
            onSelect: () => on.reveal(folder),
          })),
        }
      : {
          label: "Show photos in Finder",
          icon: "i-lucide-folder",
          disabled: onlyFolder === undefined,
          onSelect: () => onlyFolder !== undefined && on.reveal(onlyFolder),
        };
  const lastExport = project.lastExport;
  return [
    [
      { label: "Open", icon: "i-lucide-book-open", disabled: busy, onSelect: () => on.open(project.id) },
      {
        label: project.favourite ? "Remove from favourites" : "Add to favourites",
        icon: project.favourite ? "i-lucide-star-off" : "i-lucide-star",
        disabled: busy,
        onSelect: () => on.favourite(project.id, !project.favourite),
      },
      { label: "Rename…", icon: "i-lucide-pencil", disabled: busy, onSelect: () => on.rename(project.id) },
    ],
    [
      photos,
      ...(lastExport
        ? [
            {
              label: "Show export in Finder",
              icon: "i-lucide-folder-output",
              onSelect: () => on.reveal(lastExport.outputDir),
            },
          ]
        : []),
    ],
    [
      {
        label: "Delete…",
        icon: "i-lucide-trash-2",
        color: "error",
        disabled: busy,
        onSelect: () => on.delete(project.id),
      },
    ],
  ];
}
