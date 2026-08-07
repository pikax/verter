/**
 * @ai-generated - Tests for File class, language detection, and isTS detection.
 */
import { describe, it, expect } from "vitest";
import { File } from "./types";
import type { OrderedSfcStructure } from "./types";

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

    // The literal is EXHAUSTIVE on purpose: `toEqual` fails on an unexpected key
    // as well as a missing one, so a field added to `CompiledFile` without a
    // default lands here rather than reaching the UI as `undefined`. Adding the
    // field to this literal is the intended way to satisfy it — never widening
    // the assertion to a subset match.
    it("initializes compiled with empty defaults", () => {
      const file = new File("App.vue");
      expect(file.compiled).toEqual({
        js: "",
        css: "",
        types: "",
        typesSourceMap: "",
        destructuredBlock: null,
        templateCode: "",
        verterSourceMap: "",
        tscCode: "",
        publicApiOutcome: { kind: "absent" },
        declCode: "",
        declarationOutcome: { kind: "absent" },
        declSourceMap: "",
        ssrCode: "",
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

    it('returns "svelte" for .svelte files (manifest-driven, not "vue")', () => {
      const lang = new File("App.svelte").language;
      expect(lang).toBe("svelte");
      expect(lang).not.toBe("vue");
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

    const withScriptDialect = (filename: string, code: string, dialect: string | null): File => {
      const file = new File(filename, code);
      file.structure = {
        schemaVersion: 1,
        artifactToken: "test",
        blocks:
          dialect === null
            ? []
            : [
                {
                  kind: "section",
                  markupRootTokens: [],
                  section: {
                    blockToken: "b0",
                    role: { kind: "script", role: "setup", dialect },
                    openingRange: { sourceSpaceToken: "s", start: 0, end: 14 },
                    contentRange: { sourceSpaceToken: "s", start: 14, end: 20 },
                    fullRange: { sourceSpaceToken: "s", start: 0, end: 29 },
                    attributeInsertionAnchor: { sourceSpaceToken: "s", start: 13, end: 13 },
                  },
                },
              ],
        markupNodes: [],
      };
      return file;
    };

    it("returns true for .vue whose stamped script dialect is TypeScript", () => {
      const file = withScriptDialect(
        "App.vue",
        '<script setup lang="ts">\n</script>',
        "TypeScript",
      );
      expect(file.isTS).toBe(true);
    });

    it("returns true for .vue whose stamped script dialect is Tsx", () => {
      const file = withScriptDialect("App.vue", '<script setup lang="tsx">\n</script>', "Tsx");
      expect(file.isTS).toBe(true);
    });

    it("returns false for .vue whose stamped script dialect is JavaScript", () => {
      const file = withScriptDialect("App.vue", "<script setup>\n</script>", "JavaScript");
      expect(file.isTS).toBe(false);
    });

    it("returns false for .vue without a stamped script block", () => {
      const file = withScriptDialect("App.vue", "<template><div></div></template>", null);
      expect(file.isTS).toBe(false);
    });

    it("returns false for a carrier before any structure is stamped (fail closed)", () => {
      const file = new File("App.vue", '<script setup lang="ts">\n</script>');
      expect(file.isTS).toBe(false);
    });

    it("returns true for .svelte whose stamped script dialect is TypeScript", () => {
      const file = withScriptDialect(
        "App.svelte",
        '<script lang="ts">\nlet count = $state(0)\n</script>',
        "TypeScript",
      );
      expect(file.isTS).toBe(true);
    });

    it("returns false for .svelte whose stamped script dialect is JavaScript", () => {
      const file = withScriptDialect(
        "App.svelte",
        "<script>\nlet count = 0\n</script>",
        "JavaScript",
      );
      expect(file.isTS).toBe(false);
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

describe("isTS structure projection (scanner replacement)", () => {
  const scriptStructure = (dialect: string): OrderedSfcStructure => ({
    schemaVersion: 1,
    artifactToken: "test",
    blocks: [
      {
        kind: "section",
        markupRootTokens: [],
        section: {
          blockToken: "b0",
          role: { kind: "script", role: "setup", dialect },
          openingRange: { sourceSpaceToken: "s", start: 0, end: 14 },
          contentRange: { sourceSpaceToken: "s", start: 14, end: 20 },
          fullRange: { sourceSpaceToken: "s", start: 0, end: 29 },
          attributeInsertionAnchor: { sourceSpaceToken: "s", start: 13, end: 13 },
        },
      },
    ],
    markupNodes: [],
  });

  it("a decoy '<script lang=\"ts\">' literal inside a JS carrier is not TypeScript", () => {
    // The stamped structure records a JavaScript script dialect; the string
    // literal inside the code must not re-derive the dialect from raw source.
    const file = new File("App.vue", "<script setup>\nconst s = '<script lang=\"ts\">'\n</script>");
    file.structure = scriptStructure("JavaScript");
    expect(file.isTS).toBe(false);
  });

  it("the stamped TypeScript dialect makes a carrier TS regardless of authoring quirks", () => {
    const file = new File("App.vue", "<script setup lang=ts>\n</script>");
    file.structure = scriptStructure("TypeScript");
    expect(file.isTS).toBe(true);
  });
});
