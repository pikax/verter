/**
 * @ai-generated - Tests for projectStorage localStorage CRUD operations.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import type { SerializedState } from "./urlState";

const mockState: SerializedState = {
  files: { "App.vue": "<template><div /></template>" },
  activeFile: "App.vue",
  outputMode: "preview",
  compilerOptions: { isProduction: false, ssr: false },
};

// Create a minimal localStorage mock for Node.js
function installLocalStorage(): Record<string, string> {
  const store: Record<string, string> = {};
  const mock = {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      for (const k of Object.keys(store)) delete store[k];
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (i: number) => Object.keys(store)[i] ?? null,
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: mock,
    writable: true,
    configurable: true,
  });
  return store;
}

describe("projectStorage", () => {
  let backingStore: Record<string, string>;

  beforeEach(() => {
    backingStore = installLocalStorage();
    vi.resetModules();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  async function loadModule() {
    return await import("./projectStorage");
  }

  it("listProjects returns empty array when nothing stored", async () => {
    const { listProjects } = await loadModule();
    expect(listProjects()).toEqual([]);
  });

  it("saveProject creates and retrieves a project", async () => {
    const { saveProject, getProject } = await loadModule();
    saveProject("test", mockState);
    const project = getProject("test");
    expect(project).not.toBeNull();
    expect(project!.name).toBe("test");
    expect(project!.state.files["App.vue"]).toBe("<template><div /></template>");
    expect(project!.updatedAt).toBeGreaterThan(0);
    // Negative: non-existent project returns null
    expect(getProject("nonexistent")).toBeNull();
  });

  it("saveProject overwrites existing project with same name", async () => {
    const { saveProject, listProjects } = await loadModule();
    saveProject("test", mockState);
    const updated: SerializedState = { ...mockState, activeFile: "Other.vue" };
    saveProject("test", updated);
    const projects = listProjects();
    expect(projects).toHaveLength(1);
    expect(projects[0].state.activeFile).toBe("Other.vue");
    // Negative: should not create a second entry
    expect(projects.length).not.toBe(2);
  });

  it("listProjects returns sorted by updatedAt descending", async () => {
    const { saveProject, listProjects } = await loadModule();
    vi.spyOn(Date, "now")
      .mockReturnValueOnce(100)
      .mockReturnValueOnce(300)
      .mockReturnValueOnce(200);
    saveProject("a", mockState);
    saveProject("b", mockState);
    saveProject("c", mockState);
    const names = listProjects().map((p) => p.name);
    expect(names).toEqual(["b", "c", "a"]);
    // Negative: not sorted ascending
    expect(names).not.toEqual(["a", "c", "b"]);
  });

  it("deleteProject removes a project", async () => {
    const { saveProject, deleteProject, getProject, listProjects } = await loadModule();
    saveProject("test", mockState);
    deleteProject("test");
    expect(getProject("test")).toBeNull();
    expect(listProjects()).toHaveLength(0);
  });

  it("deleteProject is a no-op for non-existent project", async () => {
    const { saveProject, deleteProject, listProjects } = await loadModule();
    saveProject("keep", mockState);
    deleteProject("nonexistent");
    expect(listProjects()).toHaveLength(1);
    expect(listProjects()[0].name).toBe("keep");
  });

  it("renameProject renames and updates timestamp", async () => {
    const { saveProject, renameProject, getProject } = await loadModule();
    saveProject("old", mockState);
    const result = renameProject("old", "new");
    expect(result).toBe(true);
    expect(getProject("old")).toBeNull();
    expect(getProject("new")).not.toBeNull();
  });

  it("renameProject returns false if source does not exist", async () => {
    const { renameProject } = await loadModule();
    expect(renameProject("nope", "new")).toBe(false);
  });

  it("renameProject returns false if target name already exists", async () => {
    const { saveProject, renameProject, getProject } = await loadModule();
    saveProject("a", mockState);
    saveProject("b", mockState);
    expect(renameProject("a", "b")).toBe(false);
    // Both still exist unchanged
    expect(getProject("a")).not.toBeNull();
    expect(getProject("b")).not.toBeNull();
  });

  it("handles corrupted localStorage gracefully", async () => {
    const { listProjects, getProject } = await loadModule();
    backingStore["verter-playground-projects"] = "not-valid-json";
    expect(listProjects()).toEqual([]);
    expect(getProject("test")).toBeNull();
  });
});
