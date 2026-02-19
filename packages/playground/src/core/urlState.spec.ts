/**
 * @ai-generated - Tests for URL state serialization/deserialization.
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { compressToEncodedURIComponent } from "lz-string";
import { serializeToHash, deserializeFromHash, type SerializedState } from "./urlState";

describe("urlState", () => {
  beforeEach(() => {
    window.location.hash = "";
  });

  function makeState(overrides: Partial<SerializedState> = {}): SerializedState {
    return {
      files: { "App.vue": "<template></template>" },
      activeFile: "App.vue",
      outputMode: "preview",
      compilerOptions: { isProduction: false, ssr: false },
      ...overrides,
    };
  }

  describe("roundtrip", () => {
    it("serializes and deserializes basic state", () => {
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
      expect(result).toEqual(state);
    });

    it("preserves importMap", () => {
      const state = makeState({
        importMap: {
          imports: { vue: "https://cdn.jsdelivr.net/npm/vue@3.5.26/dist/vue.esm-browser.js" },
        },
      });
      serializeToHash(state);
      const result = deserializeFromHash();
      expect(result).toEqual(state);
    });

    it("preserves all output modes", () => {
      for (const mode of ["preview", "js", "css"] as const) {
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

    it("returns null for invalid JSON in valid compressed data", () => {
      const compressed = compressToEncodedURIComponent("not json {{{");
      window.location.hash = `#${compressed}`;
      expect(deserializeFromHash()).toBeNull();
    });
  });

  describe("serializeToHash", () => {
    it("uses history.replaceState", () => {
      const spy = vi.spyOn(history, "replaceState");
      const state = makeState();
      serializeToHash(state);
      expect(spy).toHaveBeenCalledOnce();
      expect(spy.mock.calls[0][2]).toMatch(/^#/);
      spy.mockRestore();
    });
  });
});
