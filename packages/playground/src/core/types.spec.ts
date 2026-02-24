/**
 * @ai-generated - Tests for File class, language detection, and isTS detection.
 */
import { describe, it, expect } from "vitest";
import { File } from "./types";

describe("File", () => {
  describe("constructor", () => {
    it("sets filename and code", () => {
      const file = new File("App.vue", "<template></template>");
      expect(file.filename).toBe("App.vue");
      expect(file.code).toBe("<template></template>");
    });

    it("defaults code to empty string", () => {
      const file = new File("App.vue");
      expect(file.code).toBe("");
    });

    it("initializes compiled with empty defaults", () => {
      const file = new File("App.vue");
      expect(file.compiled).toEqual({
        js: "",
        css: "",
        types: "",
        typesSourceMap: "",
        verterSourceMap: "",
        errors: [],
        compilerDiagnostics: [],
        analysis: null,
        lintDiagnostics: [],
      });
    });
  });

  describe("language getter", () => {
    it('returns "vue" for .vue files', () => {
      expect(new File("App.vue").language).toBe("vue");
    });

    it('returns "typescript" for .ts files', () => {
      expect(new File("utils.ts").language).toBe("typescript");
    });

    it('returns "javascript" for .js files', () => {
      expect(new File("utils.js").language).toBe("javascript");
    });

    it('returns "css" for .css files', () => {
      expect(new File("style.css").language).toBe("css");
    });

    it('returns "json" for .json files', () => {
      expect(new File("import-map.json").language).toBe("json");
    });

    it('falls back to "typescript" for unknown extensions', () => {
      expect(new File("something.txt").language).toBe("typescript");
    });
  });

  describe("isTS getter", () => {
    it("returns true for .ts files", () => {
      expect(new File("utils.ts").isTS).toBe(true);
    });

    it("returns true for .tsx files", () => {
      expect(new File("Component.tsx").isTS).toBe(true);
    });

    it("returns true for .vue with lang=\"ts\" (double quotes)", () => {
      const file = new File("App.vue", '<script setup lang="ts">\n</script>');
      expect(file.isTS).toBe(true);
    });

    it("returns true for .vue with lang='ts' (single quotes)", () => {
      const file = new File("App.vue", "<script setup lang='ts'>\n</script>");
      expect(file.isTS).toBe(true);
    });

    it("returns true for .vue with lang=\"tsx\"", () => {
      const file = new File("App.vue", '<script setup lang="tsx">\n</script>');
      expect(file.isTS).toBe(true);
    });

    it("returns false for .vue without lang attribute", () => {
      const file = new File("App.vue", "<script setup>\n</script>");
      expect(file.isTS).toBe(false);
    });

    it("returns false for .vue with lang=\"js\"", () => {
      const file = new File("App.vue", '<script setup lang="js">\n</script>');
      expect(file.isTS).toBe(false);
    });

    it("returns false for .vue without script tag", () => {
      const file = new File("App.vue", "<template><div></div></template>");
      expect(file.isTS).toBe(false);
    });

    it("returns true for .vue with script (no setup) and lang=\"ts\"", () => {
      const file = new File("App.vue", '<script lang="ts">\nexport default {}\n</script>');
      expect(file.isTS).toBe(true);
    });

    it("returns false for .js files", () => {
      expect(new File("utils.js").isTS).toBe(false);
    });

    it("returns false for .css files", () => {
      expect(new File("style.css").isTS).toBe(false);
    });
  });

  describe("compiled property", () => {
    it("allows mutation", () => {
      const file = new File("App.vue");
      file.compiled.js = "compiled js";
      file.compiled.errors = ["error1"];
      expect(file.compiled.js).toBe("compiled js");
      expect(file.compiled.errors).toEqual(["error1"]);
    });
  });
});
