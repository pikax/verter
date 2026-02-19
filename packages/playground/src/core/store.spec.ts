/**
 * @ai-generated - Tests for the playground store.
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock the compiler module before importing store
vi.mock("./compiler", () => ({
  initCompilers: vi.fn().mockResolvedValue(undefined),
  compileFile: vi.fn().mockResolvedValue({
    verterNew: null,
    verterNewJs: null,
  }),
  switchWasmVersion: vi.fn().mockResolvedValue(undefined),
}));

import { useStore, IMPORT_MAP_FILENAME, type Store } from "./store";
import { File } from "./types";

describe("store", () => {
  let store: Store;

  beforeEach(() => {
    window.location.hash = "";
    store = useStore();
  });

  describe("initial state", () => {
    it("starts with loading true", () => {
      expect(store.loading).toBe(true);
    });

    it("starts with preview output mode", () => {
      expect(store.outputMode).toBe("preview");
    });

    it("starts with autoSave true", () => {
      expect(store.autoSave).toBe(true);
    });

    it("starts with default compiler options", () => {
      expect(store.compilerOptions.isProduction).toBe(false);
      expect(store.compilerOptions.ssr).toBe(false);
    });

    it("starts with empty errors", () => {
      expect(store.errors).toEqual([]);
    });

    it("has App.vue as mainFile", () => {
      expect(store.mainFile).toBe("App.vue");
    });

    it("starts with null compile timing", () => {
      expect(store.compileTiming.verterNew).toBeNull();
      expect(store.compileTiming.verterNewJs).toBeNull();
    });
  });

  describe("addFile", () => {
    it("creates a new .vue file with default template", () => {
      store.addFile("Child.vue");
      expect(store.files["Child.vue"]).toBeDefined();
      expect(store.files["Child.vue"].code).toContain("<script setup");
      expect(store.files["Child.vue"].code).toContain("<template>");
    });

    it("sets newly added file as active", () => {
      store.addFile("Child.vue");
      expect(store.activeFilename).toBe("Child.vue");
    });

    it("does not overwrite existing file", () => {
      store.files["App.vue"] = new File("App.vue", "existing code");
      store.addFile("App.vue");
      expect(store.files["App.vue"].code).toBe("existing code");
    });

    it("creates non-vue file with empty code", () => {
      store.addFile("utils.ts");
      expect(store.files["utils.ts"]).toBeDefined();
      expect(store.files["utils.ts"].code).toBe("");
    });

    it("creates .js file with empty code", () => {
      store.addFile("helpers.js");
      expect(store.files["helpers.js"]).toBeDefined();
      expect(store.files["helpers.js"].code).toBe("");
    });
  });

  describe("deleteFile", () => {
    beforeEach(() => {
      store.files["App.vue"] = new File("App.vue", "main");
      store.files["Child.vue"] = new File("Child.vue", "child");
    });

    it("deletes an existing non-main file", () => {
      store.deleteFile("Child.vue");
      expect(store.files["Child.vue"]).toBeUndefined();
    });

    it("resets active file to mainFile when deleting active", () => {
      store.activeFilename = "Child.vue";
      store.deleteFile("Child.vue");
      expect(store.activeFilename).toBe("App.vue");
    });

    it("does not delete mainFile", () => {
      store.deleteFile("App.vue");
      expect(store.files["App.vue"]).toBeDefined();
    });

    it("does not delete import-map.json", () => {
      store.deleteFile(IMPORT_MAP_FILENAME);
      // Should not throw or crash
    });

    it("no-ops on non-existent file", () => {
      store.deleteFile("NonExistent.vue");
      // Should not throw
    });
  });

  describe("setActiveFile", () => {
    beforeEach(() => {
      store.files["App.vue"] = new File("App.vue", "main");
      store.files["Child.vue"] = new File("Child.vue", "child");
    });

    it("switches to existing file", () => {
      store.setActiveFile("Child.vue");
      expect(store.activeFilename).toBe("Child.vue");
    });

    it("allows switching to import-map.json", () => {
      store.setActiveFile(IMPORT_MAP_FILENAME);
      expect(store.activeFilename).toBe(IMPORT_MAP_FILENAME);
    });

    it("ignores non-existent file", () => {
      store.setActiveFile("NonExistent.vue");
      expect(store.activeFilename).toBe("App.vue");
    });
  });

  describe("updateCode", () => {
    beforeEach(() => {
      store.files["App.vue"] = new File("App.vue", "original");
      store.activeFilename = "App.vue";
    });

    it("updates active file code", () => {
      store.updateCode("new code");
      expect(store.files["App.vue"].code).toBe("new code");
    });

    it("no-ops when active file is import-map.json", () => {
      store.activeFilename = IMPORT_MAP_FILENAME;
      store.updateCode("should not apply");
    });
  });

  describe("updateImportMap", () => {
    it("parses valid JSON with imports key", () => {
      store.updateImportMap(JSON.stringify({ imports: { vue: "https://custom.url" } }));
      expect(store.importMap.imports.vue).toBe("https://custom.url");
    });

    it("ignores invalid JSON", () => {
      const originalImports = { ...store.importMap.imports };
      store.updateImportMap("not json {{{");
      expect(store.importMap.imports).toEqual(originalImports);
    });

    it("ignores JSON without imports key", () => {
      const originalImports = { ...store.importMap.imports };
      store.updateImportMap(JSON.stringify({ something: "else" }));
      expect(store.importMap.imports).toEqual(originalImports);
    });
  });

  describe("setOutputMode", () => {
    it("sets to preview", () => {
      store.setOutputMode("preview");
      expect(store.outputMode).toBe("preview");
    });

    it("sets to js", () => {
      store.setOutputMode("js");
      expect(store.outputMode).toBe("js");
    });

    it("sets to css", () => {
      store.setOutputMode("css");
      expect(store.outputMode).toBe("css");
    });
  });

  describe("toggles", () => {
    it("toggleDarkMode flips darkMode", () => {
      const original = store.darkMode;
      store.toggleDarkMode();
      expect(store.darkMode).toBe(!original);
      store.toggleDarkMode();
      expect(store.darkMode).toBe(original);
    });

    it("toggleAutoSave flips autoSave", () => {
      expect(store.autoSave).toBe(true);
      store.toggleAutoSave();
      expect(store.autoSave).toBe(false);
      store.toggleAutoSave();
      expect(store.autoSave).toBe(true);
    });

    it("toggleProduction flips isProduction", () => {
      expect(store.compilerOptions.isProduction).toBe(false);
      store.toggleProduction();
      expect(store.compilerOptions.isProduction).toBe(true);
    });

    it("toggleSSR flips ssr", () => {
      expect(store.compilerOptions.ssr).toBe(false);
      store.toggleSSR();
      expect(store.compilerOptions.ssr).toBe(true);
    });
  });

  describe("activeFile computed", () => {
    it("returns the active file from files map", () => {
      store.files["App.vue"] = new File("App.vue", "test code");
      store.activeFilename = "App.vue";
      expect(store.activeFile?.filename).toBe("App.vue");
      expect(store.activeFile?.code).toBe("test code");
    });

    it("returns import map File when import-map.json is active", () => {
      store.activeFilename = IMPORT_MAP_FILENAME;
      expect(store.activeFile?.filename).toBe(IMPORT_MAP_FILENAME);
      expect(store.activeFile?.code).toContain("vue");
    });

    it("returns undefined for non-existent file", () => {
      store.activeFilename = "NonExistent.vue";
      expect(store.activeFile).toBeUndefined();
    });
  });
});
