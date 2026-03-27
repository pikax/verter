/**
 * @ai-generated - Tests for the playground store.
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock the compiler module before importing store
vi.mock("./compiler", () => ({
  initCompilers: vi.fn().mockResolvedValue(undefined),
  compileFile: vi.fn().mockResolvedValue({
    verterNewJs: null,
    parseDurationMs: null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs: null,
    tscMs: null,
    lintMs: null,
  }),
  relintFile: vi.fn().mockReturnValue(0.5),
  switchWasmVersion: vi.fn().mockResolvedValue(undefined),
}));

import { useStore, IMPORT_MAP_FILENAME, type Store } from "./store";
import { relintFile } from "./compiler";
import { File } from "./types";
import { serializeToHash } from "./urlState";

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
      expect(store.compileTiming.verterNewJs).toBeNull();
      expect(store.compileTiming.parseDurationMs).toBeNull();
      expect(store.compileTiming.scriptMs).toBeNull();
      expect(store.compileTiming.templateMs).toBeNull();
      expect(store.compileTiming.styleMs).toBeNull();
      expect(store.compileTiming.tsxMs).toBeNull();
      expect(store.compileTiming.tscMs).toBeNull();
      expect(store.compileTiming.lintMs).toBeNull();
    });

    it("starts with active type checker status", () => {
      expect(store.typeCheckerStatus).toBe("active");
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

    it("sets to files", () => {
      store.setOutputMode("files");
      expect(store.outputMode).toBe("files");
    });

    it("redirects types to files", () => {
      store.setOutputMode("types");
      expect(store.outputMode).toBe("files");
    });

    it("sets to analysis", () => {
      store.setOutputMode("analysis");
      expect(store.outputMode).toBe("analysis");
    });

    it("redirects js to files", () => {
      store.setOutputMode("js");
      expect(store.outputMode).toBe("files");
    });

    it("redirects css to files", () => {
      store.setOutputMode("css");
      expect(store.outputMode).toBe("files");
    });

    it("redirects tsc to files", () => {
      store.setOutputMode("tsc");
      expect(store.outputMode).toBe("files");
    });

    it("redirects removed componentMeta to analysis", () => {
      store.setOutputMode("componentMeta" as any);
      expect(store.outputMode).toBe("analysis");
      expect(store.outputMode).not.toBe("componentMeta");
    });
  });

  describe("init", () => {
    it("restores removed componentMeta output mode as analysis", async () => {
      serializeToHash({
        files: { "App.vue": "<template><div /></template>" },
        activeFile: "App.vue",
        outputMode: "componentMeta" as any,
        compilerOptions: { isProduction: false, ssr: false, strictSlots: false },
      });

      await store.init();

      expect(store.outputMode).toBe("analysis");
      expect(store.outputMode).not.toBe("componentMeta");
    });
  });

  describe("setTypeCheckerStatus", () => {
    it("sets status to active", () => {
      store.setTypeCheckerStatus("active");
      expect(store.typeCheckerStatus).toBe("active");
    });

    it("sets status to unavailable", () => {
      store.setTypeCheckerStatus("unavailable");
      expect(store.typeCheckerStatus).toBe("unavailable");
    });

    it("sets status to initializing", () => {
      store.setTypeCheckerStatus("initializing");
      expect(store.typeCheckerStatus).toBe("initializing");
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

  describe("disabledRules", () => {
    it("starts with empty disabled rules set", () => {
      expect(store.disabledRules).toBeDefined();
      expect(store.disabledRules.size).toBe(0);
    });

    it("toggleLintRule adds a rule to disabled set", () => {
      store.toggleLintRule("no-bare-strings-in-template");
      expect(store.disabledRules.has("no-bare-strings-in-template")).toBe(true);
    });

    it("toggleLintRule removes a previously disabled rule", () => {
      store.toggleLintRule("no-bare-strings-in-template");
      expect(store.disabledRules.has("no-bare-strings-in-template")).toBe(true);
      store.toggleLintRule("no-bare-strings-in-template");
      expect(store.disabledRules.has("no-bare-strings-in-template")).toBe(false);
    });

    it("toggleLintRule supports multiple disabled rules", () => {
      store.toggleLintRule("no-bare-strings-in-template");
      store.toggleLintRule("html-button-has-type");
      expect(store.disabledRules.size).toBe(2);
      expect(store.disabledRules.has("no-bare-strings-in-template")).toBe(true);
      expect(store.disabledRules.has("html-button-has-type")).toBe(true);
    });

    it("relint calls relintFile with disabled rules", () => {
      store.files["App.vue"] = new File("App.vue", "<template><div>hello</div></template>");
      store.activeFilename = "App.vue";
      store.toggleLintRule("no-bare-strings-in-template");
      store.relint();
      expect(relintFile).toHaveBeenCalledWith(store.activeFile, store.disabledRules);
    });

    it("relint does not crash when no active file", () => {
      store.activeFilename = IMPORT_MAP_FILENAME;
      store.relint(); // should not throw
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
