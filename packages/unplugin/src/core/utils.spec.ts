/**
 * @ai-generated - Tests for parseVueRequest and descriptor cache utilities.
 */
import { describe, it, expect, beforeEach } from "vitest";
import {
  parseVueRequest,
  getDescriptor,
  setDescriptor,
  deleteDescriptor,
  clearCache,
} from "./utils";
import type { ViteCodegenResult } from "@verter/native";

describe("parseVueRequest", () => {
  it("parses a plain .vue filename with no query", () => {
    const result = parseVueRequest("/path/to/App.vue");
    expect(result.filename).toBe("/path/to/App.vue");
    expect(result.query.vue).toBe(false);
    expect(result.query.type).toBeUndefined();
    expect(result.query.index).toBeUndefined();
  });

  it("parses a .vue file with style query parameters", () => {
    const result = parseVueRequest(
      "/path/to/App.vue?vue&type=style&index=0&lang=css&scoped=true",
    );
    expect(result.filename).toBe("/path/to/App.vue");
    expect(result.query.vue).toBe(true);
    expect(result.query.type).toBe("style");
    expect(result.query.index).toBe(0);
    expect(result.query.lang).toBe("css");
    expect(result.query.scoped).toBe(true);
  });

  it("parses a .vue file with template query", () => {
    const result = parseVueRequest("/path/to/App.vue?vue&type=template");
    expect(result.query.vue).toBe(true);
    expect(result.query.type).toBe("template");
    expect(result.query.scoped).toBe(false);
  });

  it("parses a .vue file with script query", () => {
    const result = parseVueRequest("/path/to/App.vue?vue&type=script");
    expect(result.query.vue).toBe(true);
    expect(result.query.type).toBe("script");
  });

  it("returns vue: false for non-vue query strings", () => {
    const result = parseVueRequest("/path/to/file.ts?some=param");
    expect(result.query.vue).toBe(false);
  });

  it("parses scss lang correctly", () => {
    const result = parseVueRequest(
      "/path/to/App.vue?vue&type=style&index=1&lang=scss",
    );
    expect(result.query.lang).toBe("scss");
    expect(result.query.index).toBe(1);
  });
});

describe("descriptor cache", () => {
  const mockResult: ViteCodegenResult = {
    script: {
      code: "export default {}",
      imports: [],
      body_start_utf16: 0,
    },
    styles: [],
    duration_ms: 1,
  };

  beforeEach(() => {
    clearCache();
  });

  it("returns undefined for uncached files", () => {
    expect(getDescriptor("/path/to/App.vue")).toBeUndefined();
  });

  it("stores and retrieves a descriptor", () => {
    setDescriptor("/path/to/App.vue", mockResult);
    expect(getDescriptor("/path/to/App.vue")).toBe(mockResult);
  });

  it("deletes a descriptor", () => {
    setDescriptor("/path/to/App.vue", mockResult);
    deleteDescriptor("/path/to/App.vue");
    expect(getDescriptor("/path/to/App.vue")).toBeUndefined();
  });

  it("clears all cached descriptors", () => {
    setDescriptor("/path/to/App.vue", mockResult);
    setDescriptor("/path/to/Other.vue", mockResult);
    clearCache();
    expect(getDescriptor("/path/to/App.vue")).toBeUndefined();
    expect(getDescriptor("/path/to/Other.vue")).toBeUndefined();
  });
});
