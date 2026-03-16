/**
 * @ai-generated - Tests for import map utilities.
 */
import { describe, it, expect } from "vitest";
import {
  getDefaultImportMap,
  mergeImportMap,
  extractVueVersion,
  isDefaultImport,
  type ImportMap,
} from "./importMap";

describe("getDefaultImportMap", () => {
  it("returns default import map with default vue version", () => {
    const map = getDefaultImportMap();
    expect(map.imports.vue).toBe(
      "https://cdn.jsdelivr.net/npm/vue@3.5.26/dist/vue.esm-browser.js",
    );
    expect(map.imports["vue/server-renderer"]).toBe(
      "https://cdn.jsdelivr.net/npm/@vue/server-renderer@3.5.26/dist/server-renderer.esm-browser.js",
    );
  });

  it("accepts a custom vue version", () => {
    const map = getDefaultImportMap("3.4.0");
    expect(map.imports.vue).toContain("vue@3.4.0");
    expect(map.imports["vue/server-renderer"]).toContain("@3.4.0");
  });

  it("has correct CDN URLs", () => {
    const map = getDefaultImportMap("3.5.26");
    expect(map.imports.vue).toMatch(/^https:\/\/cdn\.jsdelivr\.net\/npm\/vue@/);
    expect(map.imports["vue/server-renderer"]).toMatch(
      /^https:\/\/cdn\.jsdelivr\.net\/npm\/@vue\/server-renderer@/,
    );
  });

  it("does not include scopes by default", () => {
    const map = getDefaultImportMap();
    expect(map.scopes).toBeUndefined();
  });
});

describe("mergeImportMap", () => {
  it("merges disjoint imports", () => {
    const a: ImportMap = { imports: { vue: "a" } };
    const b: ImportMap = { imports: { react: "b" } };
    const merged = mergeImportMap(a, b);
    expect(merged.imports).toEqual({ vue: "a", react: "b" });
  });

  it("b overrides a on conflict", () => {
    const a: ImportMap = { imports: { vue: "old" } };
    const b: ImportMap = { imports: { vue: "new" } };
    const merged = mergeImportMap(a, b);
    expect(merged.imports.vue).toBe("new");
  });

  it("merges scopes", () => {
    const a: ImportMap = { imports: {}, scopes: { "/foo/": { x: "1" } } };
    const b: ImportMap = { imports: {}, scopes: { "/bar/": { y: "2" } } };
    const merged = mergeImportMap(a, b);
    expect(merged.scopes).toEqual({
      "/foo/": { x: "1" },
      "/bar/": { y: "2" },
    });
  });

  it("handles undefined scopes", () => {
    const a: ImportMap = { imports: { vue: "a" } };
    const b: ImportMap = { imports: { react: "b" } };
    const merged = mergeImportMap(a, b);
    expect(merged.scopes).toBeDefined();
  });

  it("does not mutate original import maps", () => {
    const a: ImportMap = { imports: { vue: "a" } };
    const b: ImportMap = { imports: { react: "b" } };
    mergeImportMap(a, b);
    expect(a.imports).toEqual({ vue: "a" });
    expect(b.imports).toEqual({ react: "b" });
  });
});

describe("extractVueVersion", () => {
  it("extracts version from default CDN URL", () => {
    const map = getDefaultImportMap("3.5.26");
    expect(extractVueVersion(map)).toBe("3.5.26");
  });

  it("extracts version from custom version", () => {
    const map = getDefaultImportMap("3.4.0");
    expect(extractVueVersion(map)).toBe("3.4.0");
  });

  it("returns undefined when vue import is missing", () => {
    const map: ImportMap = { imports: { lodash: "https://example.com/lodash" } };
    expect(extractVueVersion(map)).toBeUndefined();
  });

  it("returns undefined when vue URL is not a CDN URL", () => {
    const map: ImportMap = { imports: { vue: "/local/vue.js" } };
    expect(extractVueVersion(map)).toBeUndefined();
  });

  it("extracts pre-release versions", () => {
    const map: ImportMap = {
      imports: {
        vue: "https://cdn.jsdelivr.net/npm/vue@3.5.0-beta.1/dist/vue.esm-browser.js",
        "vue/server-renderer":
          "https://cdn.jsdelivr.net/npm/@vue/server-renderer@3.5.0-beta.1/dist/server-renderer.esm-browser.js",
      },
    };
    expect(extractVueVersion(map)).toBe("3.5.0-beta.1");
  });
});

describe("isDefaultImport", () => {
  it("recognizes default vue import", () => {
    expect(
      isDefaultImport(
        "vue",
        "https://cdn.jsdelivr.net/npm/vue@3.5.26/dist/vue.esm-browser.js",
        "3.5.26",
      ),
    ).toBe(true);
  });

  it("recognizes default vue/server-renderer import", () => {
    expect(
      isDefaultImport(
        "vue/server-renderer",
        "https://cdn.jsdelivr.net/npm/@vue/server-renderer@3.5.26/dist/server-renderer.esm-browser.js",
        "3.5.26",
      ),
    ).toBe(true);
  });

  it("returns false for custom imports", () => {
    expect(isDefaultImport("lodash", "https://cdn.jsdelivr.net/npm/lodash", "3.5.26")).toBe(false);
  });

  it("returns false when vue version differs", () => {
    expect(
      isDefaultImport(
        "vue",
        "https://cdn.jsdelivr.net/npm/vue@3.4.0/dist/vue.esm-browser.js",
        "3.5.26",
      ),
    ).toBe(false);
  });

  it("returns false without vue version", () => {
    expect(
      isDefaultImport(
        "vue",
        "https://cdn.jsdelivr.net/npm/vue@3.5.26/dist/vue.esm-browser.js",
      ),
    ).toBe(false);
  });
});
