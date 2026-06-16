/**
 * @ai-generated - Tests for URL state serialization/deserialization (fflate encoding, flat format).
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { zlibSync, strToU8, strFromU8, unzlibSync } from "fflate";
import { serializeToHash, deserializeFromHash, type SerializedState } from "./urlState";

function makeState(overrides: Partial<SerializedState> = {}): SerializedState {
  return {
    files: { "App.vue": "<template></template>" },
    activeFile: "App.vue",
    outputMode: "preview",
    compilerOptions: { isProduction: false, ssr: false },
    ...overrides,
  };
}

/** Encode a flat object using the Vue playground encoding (fflate zlib + base64). */
function encodeFlat(obj: Record<string, string>): string {
  const json = JSON.stringify(obj);
  const compressed = zlibSync(strToU8(json), { level: 9 });
  return btoa(strFromU8(compressed, true));
}

/** Decode a hash back to a flat object. */
function decodeHash(hash: string): Record<string, string> {
  const binary = atob(hash);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  const json = strFromU8(unzlibSync(bytes));
  return JSON.parse(json);
}

describe("urlState", () => {
  beforeEach(() => {
    window.location.hash = "";
  });

  describe("roundtrip (serialize → deserialize)", () => {
    it("preserves basic state with default metadata omitted", () => {
      const state = makeState();
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result).toEqual(state);
    });

    it("preserves multiple files", () => {
      const state = makeState({
        files: {
          "App.vue": "<template>app</template>",
          "Child.vue": "<template>child</template>",
          "utils.ts": "export const x = 1;",
        },
      });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.files).toEqual(state.files);
    });

    it("preserves all output modes", () => {
      for (const mode of ["preview", "js", "css", "analysis"] as const) {
        const state = makeState({ outputMode: mode });
        serializeToHash(state);
        const result = deserializeFromHash();
        expect(result?.outputMode).toBe(mode);
      }
    });

    it("preserves compiler options", () => {
      const state = makeState({
        compilerOptions: { isProduction: true, ssr: true },
      });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.compilerOptions).toEqual({ isProduction: true, ssr: true });
    });

    it("preserves vueVersion", () => {
      const state = makeState({ vueVersion: "3.4.0" });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.vueVersion).toBe("3.4.0");
    });

    it("preserves tsVersion", () => {
      const state = makeState({ tsVersion: "5.7.3" });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.tsVersion).toBe("5.7.3");
    });

    it("preserves verterVersion", () => {
      const state = makeState({ verterVersion: "release:0.0.1" });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.verterVersion).toBe("release:0.0.1");
    });

    it("preserves the framework language pin", () => {
      const state = makeState({
        files: { "App.svelte": "<h1>hi</h1>" },
        activeFile: "App.svelte",
        language: "svelte",
      });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.language).toBe("svelte");
    });

    it("omits the language pin when in Auto (undefined)", () => {
      const state = makeState();
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.language).toBeUndefined();
    });

    it("preserves typeChecker", () => {
      const state = makeState({ typeChecker: "tsgo" });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.typeChecker).toBe("tsgo");
    });

    it("omits typeChecker when tsc (default)", () => {
      const state = makeState({ typeChecker: "tsc" });
      serializeToHash(state);
      const result = deserializeFromHash();
      // When tsc, it's not serialized, so deserialization returns undefined
      expect(result?.typeChecker).toBeUndefined();
    });

    it("roundtrips multi-file state with import map and versions", () => {
      const state = makeState({
        files: {
          "App.vue": "<template>app</template>",
          "Child.vue": "<script setup>import { ref } from 'vue'</script>",
        },
        importMap: {
          imports: { lodash: "https://cdn.jsdelivr.net/npm/lodash-es" },
        },
        vueVersion: "3.5.26",
        tsVersion: "5.7.3",
        activeFile: "Child.vue",
        outputMode: "js",
      });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.files).toEqual(state.files);
      expect(result?.importMap).toEqual(state.importMap);
      expect(result?.vueVersion).toBe("3.5.26");
      expect(result?.tsVersion).toBe("5.7.3");
      expect(result?.activeFile).toBe("Child.vue");
      expect(result?.outputMode).toBe("js");
    });

    it("preserves tsconfig.json as a regular file", () => {
      const tsconfig = '{"compilerOptions":{"strict":true}}';
      const state = makeState({
        files: {
          "App.vue": "<template></template>",
          "tsconfig.json": tsconfig,
        },
      });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result?.files["tsconfig.json"]).toBe(tsconfig);
    });
  });

  describe("serializeToHash (flat format details)", () => {
    it("uses history.replaceState with #hash", () => {
      const spy = vi.spyOn(history, "replaceState");
      serializeToHash(makeState());
      expect(spy).toHaveBeenCalledOnce();
      expect(spy.mock.calls[0][2]).toMatch(/^#/);
      spy.mockRestore();
    });

    it("serializes _version from vueVersion", () => {
      const state = makeState({ vueVersion: "3.5.26" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_version"]).toBe("3.5.26");
    });

    it("serializes _tsVersion from tsVersion", () => {
      const state = makeState({ tsVersion: "5.7.3" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_tsVersion"]).toBe("5.7.3");
    });

    it("serializes _verterVersion from verterVersion", () => {
      const state = makeState({ verterVersion: "release:0.0.1" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_verterVersion"]).toBe("release:0.0.1");
    });

    it("omits _verterVersion when 'local'", () => {
      const state = makeState({ verterVersion: "local" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_verterVersion"]).toBeUndefined();
    });

    it("omits _tsVersion when 'latest'", () => {
      const state = makeState({ tsVersion: "latest" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_tsVersion"]).toBeUndefined();
    });

    it("omits _activeFile when 'App.vue' (default)", () => {
      const state = makeState({ activeFile: "App.vue" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_activeFile"]).toBeUndefined();
    });

    it("includes _activeFile when not default", () => {
      const state = makeState({
        files: { "App.vue": "", "Child.vue": "" },
        activeFile: "Child.vue",
      });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_activeFile"]).toBe("Child.vue");
    });

    it("omits _outputMode when 'preview' (default)", () => {
      const state = makeState({ outputMode: "preview" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_outputMode"]).toBeUndefined();
    });

    it("includes _outputMode when not default", () => {
      const state = makeState({ outputMode: "js" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_outputMode"]).toBe("js");
    });

    it("omits _isProduction when false (default)", () => {
      const state = makeState({ compilerOptions: { isProduction: false, ssr: false } });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_isProduction"]).toBeUndefined();
    });

    it("includes _isProduction when true", () => {
      const state = makeState({ compilerOptions: { isProduction: true, ssr: false } });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_isProduction"]).toBe("true");
    });

    it("serializes _language when pinned", () => {
      const state = makeState({ language: "svelte" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_language"]).toBe("svelte");
    });

    it("omits _language when in Auto (undefined)", () => {
      const state = makeState();
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_language"]).toBeUndefined();
    });

    it("serializes _typeChecker when not tsc", () => {
      const state = makeState({ typeChecker: "tsgo" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_typeChecker"]).toBe("tsgo");
    });

    it("omits _typeChecker when tsc (default)", () => {
      const state = makeState({ typeChecker: "tsc" });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_typeChecker"]).toBeUndefined();
    });

    it("omits _typeChecker when undefined", () => {
      const state = makeState();
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_typeChecker"]).toBeUndefined();
    });

    it("includes _ssr when true", () => {
      const state = makeState({ compilerOptions: { isProduction: false, ssr: true } });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["_ssr"]).toBe("true");
    });

    it("strips builtin imports from serialized import map", () => {
      const state = makeState({
        vueVersion: "3.5.26",
        importMap: {
          imports: {
            vue: "https://cdn.jsdelivr.net/npm/vue@3.5.26/dist/vue.esm-browser.js",
            "vue/server-renderer":
              "https://cdn.jsdelivr.net/npm/@vue/server-renderer@3.5.26/dist/server-renderer.esm-browser.js",
            lodash: "https://cdn.jsdelivr.net/npm/lodash-es",
          },
        },
      });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      const importMapJson = JSON.parse(flat["import-map.json"]);
      expect(importMapJson.imports.vue).toBeUndefined();
      expect(importMapJson.imports["vue/server-renderer"]).toBeUndefined();
      expect(importMapJson.imports.lodash).toBe("https://cdn.jsdelivr.net/npm/lodash-es");
    });

    it("omits import-map.json when only builtin imports", () => {
      const state = makeState({
        vueVersion: "3.5.26",
        importMap: {
          imports: {
            vue: "https://cdn.jsdelivr.net/npm/vue@3.5.26/dist/vue.esm-browser.js",
            "vue/server-renderer":
              "https://cdn.jsdelivr.net/npm/@vue/server-renderer@3.5.26/dist/server-renderer.esm-browser.js",
          },
        },
      });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["import-map.json"]).toBeUndefined();
    });

    it("includes import-map.json when custom imports exist", () => {
      const state = makeState({
        importMap: {
          imports: { lodash: "https://cdn.jsdelivr.net/npm/lodash-es" },
        },
      });
      serializeToHash(state);
      const hash = location.hash.slice(1);
      const flat = decodeHash(hash);
      expect(flat["import-map.json"]).toBeDefined();
      const parsed = JSON.parse(flat["import-map.json"]);
      expect(parsed.imports.lodash).toBe("https://cdn.jsdelivr.net/npm/lodash-es");
    });
  });

  describe("deserializeFromHash", () => {
    it("returns null for empty hash", () => {
      window.location.hash = "";
      expect(deserializeFromHash()).toBeNull();
    });

    it("returns null for corrupt data", () => {
      window.location.hash = "#not-valid-compressed-data!!!";
      expect(deserializeFromHash()).toBeNull();
    });

    it("decodes Vue-encoded hash (fflate, flat object, _version, _tsVersion)", () => {
      const flat: Record<string, string> = {
        "App.vue": "<template>{{ msg }}</template>",
        "Child.vue": "<script setup>const x = 1</script>",
        _version: "3.5.26",
        _tsVersion: "5.7.3",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result).not.toBeNull();
      expect(result!.files["App.vue"]).toBe("<template>{{ msg }}</template>");
      expect(result!.files["Child.vue"]).toBe("<script setup>const x = 1</script>");
      expect(result!.vueVersion).toBe("3.5.26");
      expect(result!.tsVersion).toBe("5.7.3");
      // _version and _tsVersion should not appear as files
      expect(result!.files["_version"]).toBeUndefined();
      expect(result!.files["_tsVersion"]).toBeUndefined();
    });

    it("extracts _verterVersion metadata", () => {
      const flat: Record<string, string> = {
        "App.vue": "<template></template>",
        _verterVersion: "commit:abc123",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.verterVersion).toBe("commit:abc123");
      expect(result!.files["_verterVersion"]).toBeUndefined();
    });

    it("extracts _activeFile metadata", () => {
      const flat: Record<string, string> = {
        "App.vue": "",
        "Child.vue": "",
        _activeFile: "Child.vue",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.activeFile).toBe("Child.vue");
    });

    it("extracts _outputMode metadata", () => {
      const flat: Record<string, string> = {
        "App.vue": "",
        _outputMode: "js",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.outputMode).toBe("js");
    });

    it("extracts _language metadata", () => {
      const flat: Record<string, string> = {
        "App.svelte": "<h1>hi</h1>",
        _language: "svelte",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.language).toBe("svelte");
      expect(result!.files["_language"]).toBeUndefined();
    });

    it("returns undefined language when _language is absent", () => {
      const flat: Record<string, string> = { "App.vue": "" };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.language).toBeUndefined();
    });

    it("extracts _typeChecker metadata", () => {
      const flat: Record<string, string> = {
        "App.vue": "",
        _typeChecker: "tsgo",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.typeChecker).toBe("tsgo");
      expect(result!.files["_typeChecker"]).toBeUndefined();
    });

    it("returns undefined typeChecker when _typeChecker is absent", () => {
      const flat: Record<string, string> = { "App.vue": "" };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.typeChecker).toBeUndefined();
    });

    it("extracts _isProduction and _ssr from flat object", () => {
      const flat: Record<string, string> = {
        "App.vue": "",
        _isProduction: "true",
        _ssr: "true",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.compilerOptions.isProduction).toBe(true);
      expect(result!.compilerOptions.ssr).toBe(true);
    });

    it("defaults activeFile to App.vue when _activeFile is absent", () => {
      const flat: Record<string, string> = { "App.vue": "" };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.activeFile).toBe("App.vue");
    });

    it("defaults outputMode to preview when _outputMode is absent", () => {
      const flat: Record<string, string> = { "App.vue": "" };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.outputMode).toBe("preview");
    });

    it("defaults compilerOptions when _isProduction/_ssr are absent", () => {
      const flat: Record<string, string> = { "App.vue": "" };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.compilerOptions).toEqual({ isProduction: false, ssr: false });
    });

    it("parses import-map.json into importMap", () => {
      const flat: Record<string, string> = {
        "App.vue": "",
        "import-map.json": JSON.stringify({
          imports: { lodash: "https://cdn.jsdelivr.net/npm/lodash-es" },
        }),
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.importMap).toEqual({
        imports: { lodash: "https://cdn.jsdelivr.net/npm/lodash-es" },
      });
      // import-map.json should not be in files
      expect(result!.files["import-map.json"]).toBeUndefined();
    });

    it("handles legacy Vue format (no zlib, just base64+UTF-8)", () => {
      // Vue playground legacy: base64-encoded UTF-8 JSON (no zlib compression)
      const flat: Record<string, string> = {
        "App.vue": "<template>hello</template>",
        _version: "3.5.0",
      };
      const json = JSON.stringify(flat);
      // Encode as base64 without zlib — plain UTF-8 → binary string → btoa
      const base64 = btoa(unescape(encodeURIComponent(json)));
      window.location.hash = `#${base64}`;
      const result = deserializeFromHash();
      expect(result).not.toBeNull();
      expect(result!.files["App.vue"]).toBe("<template>hello</template>");
      expect(result!.vueVersion).toBe("3.5.0");
    });

    it("ignores unknown _-prefixed keys", () => {
      const flat: Record<string, string> = {
        "App.vue": "",
        _unknownKey: "some-value",
      };
      window.location.hash = `#${encodeFlat(flat)}`;
      const result = deserializeFromHash();
      expect(result!.files["_unknownKey"]).toBeUndefined();
    });
  });
});
