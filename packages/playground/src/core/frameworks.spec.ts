import { describe, it, expect } from "vitest";
import { CLIENT_FRAMEWORKS } from "@verter/language-shared";
import {
  FRAMEWORKS,
  allFrameworkExtensions,
  detectFrameworkId,
  fileKindForFilename,
  frameworkById,
  frameworkCarrierExtension,
  frameworkCarrierFilename,
  frameworkClientLanguageId,
  frameworkLanguageIdForFilename,
  isCarrierFilename,
  isExperimentalFramework,
  languageOptions,
  NON_FRAMEWORK_FILE_KIND,
} from "./frameworks";

describe("frameworks (manifest-derived)", () => {
  it("FRAMEWORKS is exactly CLIENT_FRAMEWORKS (manifest authority, manifest order)", () => {
    expect(FRAMEWORKS).toBe(CLIENT_FRAMEWORKS);
    expect(FRAMEWORKS.map((f) => f.frameworkId)).toEqual(
      CLIENT_FRAMEWORKS.map((f) => f.frameworkId),
    );
  });

  it("the manifest registers both vue and svelte (sanity for the corpus)", () => {
    const ids = FRAMEWORKS.map((f) => f.frameworkId);
    expect(ids).toContain("vue");
    expect(ids).toContain("svelte");
  });

  describe("longest-suffix auto-detect", () => {
    it("detects .vue carrier", () => {
      expect(detectFrameworkId("App.vue")).toBe("vue");
    });

    it("detects .svelte carrier", () => {
      expect(detectFrameworkId("App.svelte")).toBe("svelte");
    });

    it("classifies store.svelte.ts as svelte BEFORE plain .ts (longest suffix)", () => {
      expect(detectFrameworkId("store.svelte.ts")).toBe("svelte");
    });

    it("classifies counter.svelte.js as svelte BEFORE plain .js (longest suffix)", () => {
      expect(detectFrameworkId("counter.svelte.js")).toBe("svelte");
    });

    it("plain .ts is NOT a framework", () => {
      expect(detectFrameworkId("util.ts")).toBeNull();
    });

    it("plain .js is NOT a framework", () => {
      expect(detectFrameworkId("util.js")).toBeNull();
    });
  });

  describe("fileKindForFilename", () => {
    it("maps .vue to its framework id", () => {
      expect(fileKindForFilename("App.vue")).toBe("vue");
    });

    it("maps .svelte to svelte", () => {
      expect(fileKindForFilename("App.svelte")).toBe("svelte");
    });

    it("maps .svelte.ts adapter module to svelte (not non_sfc)", () => {
      expect(fileKindForFilename("store.svelte.ts")).toBe("svelte");
    });

    it("maps a plain .ts file to the non-framework fallback", () => {
      expect(fileKindForFilename("util.ts")).toBe(NON_FRAMEWORK_FILE_KIND);
    });
  });

  describe("framework lookups", () => {
    it("frameworkById returns the manifest entry", () => {
      expect(frameworkById("svelte")?.frameworkId).toBe("svelte");
      expect(frameworkById("nope")).toBeUndefined();
      expect(frameworkById(null)).toBeUndefined();
    });

    it("frameworkCarrierExtension returns the first carrier extension", () => {
      expect(frameworkCarrierExtension(frameworkById("vue")!)).toBe(".vue");
      expect(frameworkCarrierExtension(frameworkById("svelte")!)).toBe(".svelte");
    });

    it("frameworkCarrierFilename uses the first carrier extension", () => {
      const vue = frameworkById("vue")!;
      const svelte = frameworkById("svelte")!;
      expect(frameworkCarrierFilename(vue)).toBe("App.vue");
      expect(frameworkCarrierFilename(svelte)).toBe("App.svelte");
    });

    it("frameworkClientLanguageId returns the first client language id", () => {
      expect(frameworkClientLanguageId(frameworkById("vue")!)).toBe("vue");
      expect(frameworkClientLanguageId(frameworkById("svelte")!)).toBe("svelte");
    });
  });

  describe("frameworkLanguageIdForFilename", () => {
    it("maps carrier files to the framework client language id", () => {
      expect(frameworkLanguageIdForFilename("App.vue")).toBe("vue");
      expect(frameworkLanguageIdForFilename("App.svelte")).toBe("svelte");
    });

    it("maps adapter modules to the framework client language id", () => {
      expect(frameworkLanguageIdForFilename("store.svelte.ts")).toBe("svelte");
    });

    it("returns null for non-framework files", () => {
      expect(frameworkLanguageIdForFilename("util.ts")).toBeNull();
      expect(frameworkLanguageIdForFilename("styles.css")).toBeNull();
    });
  });

  describe("isCarrierFilename", () => {
    it("is true for carrier files only (not adapter modules)", () => {
      expect(isCarrierFilename("App.vue")).toBe(true);
      expect(isCarrierFilename("App.svelte")).toBe(true);
      expect(isCarrierFilename("store.svelte.ts")).toBe(false);
      expect(isCarrierFilename("util.ts")).toBe(false);
    });
  });

  describe("allFrameworkExtensions", () => {
    it("includes every carrier + adapter-module extension (manifest-derived, de-duped)", () => {
      const exts = allFrameworkExtensions();
      // The dep-graph resolver relies on this set to resolve .svelte imports.
      expect(exts).toContain(".vue");
      expect(exts).toContain(".svelte");
      expect(exts).toContain(".svelte.ts");
      expect(exts).toContain(".svelte.js");
      // De-duplicated (no extension appears twice).
      expect(new Set(exts).size).toBe(exts.length);
    });
  });

  describe("isExperimentalFramework", () => {
    it("vue is not experimental; svelte is", () => {
      expect(isExperimentalFramework("vue")).toBe(false);
      expect(isExperimentalFramework("svelte")).toBe(true);
      expect(isExperimentalFramework(null)).toBe(false);
    });
  });

  describe("languageOptions (dropdown source)", () => {
    it("is exactly [Auto, ...CLIENT_FRAMEWORKS] in manifest order", () => {
      const options = languageOptions();
      // First is the Auto state.
      expect(options[0]).toEqual({ id: null, label: "Auto", experimental: false });
      // The rest are exactly the manifest frameworks, in order.
      expect(options.slice(1).map((o) => o.id)).toEqual(
        CLIENT_FRAMEWORKS.map((f) => f.frameworkId),
      );
      // Exactly one Auto + one entry per manifest framework.
      expect(options.length).toBe(CLIENT_FRAMEWORKS.length + 1);
    });

    it("marks non-vue frameworks experimental", () => {
      const byId = new Map(languageOptions().map((o) => [o.id, o]));
      expect(byId.get("vue")?.experimental).toBe(false);
      expect(byId.get("svelte")?.experimental).toBe(true);
    });
  });
});
