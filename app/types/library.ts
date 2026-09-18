import type { ProjectListItem } from "./book";

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
