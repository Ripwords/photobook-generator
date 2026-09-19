import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * A deleted book goes to the trash, so the notice after a delete can put it
 * back. `invoke` is mocked as the Rust side keeps it: a delete hides a book
 * from `list_projects`, a restore shows it again.
 */
interface Row {
  id: number;
  name: string;
  deleted: boolean;
  favourite?: boolean;
}

const rows: Row[] = [];
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { useProjects } = await import("../app/composables/useProjects");

beforeEach(() => {
  rows.splice(0, rows.length, { id: 1, name: "Kyoto", deleted: false }, { id: 2, name: "Bali", deleted: false });
  invoke.mockReset();
  invoke.mockImplementation(async (command: string, args?: { id: number; favourite?: boolean }) => {
    const row = rows.find((r) => r.id === args?.id);
    switch (command) {
      case "list_projects":
        return rows
          .filter((r) => !r.deleted)
          .map(({ id, name, favourite = false }) => ({ id, name, favourite }));
      case "set_favourite":
        if (!row || row.deleted) throw new Error(`project ${args?.id} no longer exists`);
        row.favourite = args?.favourite;
        return undefined;
      case "delete_project":
        if (row) row.deleted = true;
        return undefined;
      case "restore_project":
        if (!row) throw new Error(`project ${args?.id} no longer exists`);
        row.deleted = false;
        return undefined;
      default:
        throw new Error(`unexpected ${command}`);
    }
  });
});

describe("deleting a book", () => {
  it("can be undone, and the book is listed again", async () => {
    const { projects, deleteProject, restoreProject, reload } = useProjects();
    await reload();

    expect(await deleteProject(1)).toBe(true);
    expect(projects.value.map((p) => p.name)).toEqual(["Bali"]);

    await restoreProject(1);
    expect(projects.value.map((p) => p.name)).toEqual(["Kyoto", "Bali"]);
  });

  it("says when it failed, so no Undo is offered for a book still there", async () => {
    invoke.mockImplementationOnce(async () => {
      throw new Error("disk full");
    });
    const { deleteProject, error } = useProjects();

    expect(await deleteProject(1)).toBe(false);
    expect(error.value).toContain("disk full");
  });

  it("reports a book that can no longer be restored", async () => {
    const { restoreProject, error } = useProjects();

    await restoreProject(99);

    expect(error.value).toContain("no longer exists");
  });
});

describe("starring a book", () => {
  it("is saved and shows in the list straight away", async () => {
    const { projects, setFavourite, reload } = useProjects();
    await reload();

    expect(await setFavourite(2, true)).toBe(true);
    expect(projects.value.find((p) => p.id === 2)?.favourite).toBe(true);

    await setFavourite(2, false);
    expect(projects.value.find((p) => p.id === 2)?.favourite).toBe(false);
  });

  it("reports a book that is gone", async () => {
    const { setFavourite, error } = useProjects();

    expect(await setFavourite(99, true)).toBe(false);
    expect(error.value).toContain("no longer exists");
  });
});
