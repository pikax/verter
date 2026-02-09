/**
 * @ai-generated - Tests for Buffer input support in @verter/native compile functions.
 * Verifies that compile, compileSync, and compileForVite accept both string and Buffer inputs.
 */
import { describe, it, expect } from "vitest";
import { compile, compileSync, compileForVite } from "./index.js";

const SFC_INPUT = "<template><div>{{ msg }}</div></template>";

// NAPI-RS converts snake_case Rust fields to camelCase JS properties at runtime,
// but the TS declarations in index.ts use snake_case. Access via bracket notation
// to test the actual runtime properties.
function getSourceMap(result: Record<string, unknown>): unknown {
  return result["sourceMap"] ?? result["source_map"];
}

describe("Buffer input support", () => {
  describe("compile", () => {
    // @ai-generated - String and Buffer inputs produce identical output
    it("should produce identical results for string and Buffer input", () => {
      const stringResult = compile(SFC_INPUT, { filename: "Test.vue" });
      const bufferResult = compile(Buffer.from(SFC_INPUT), { filename: "Test.vue" });

      expect(bufferResult.code).toBe(stringResult.code);
      expect(getSourceMap(bufferResult as any)).toBe(getSourceMap(stringResult as any));
    });

    // @ai-generated - Buffer input returns valid compilation result
    it("should compile from Buffer and return valid result", () => {
      const result = compile(Buffer.from(SFC_INPUT));

      expect(result.code).toContain("_createElementBlock");
      expect(result.code).toBeTruthy();
      expect(getSourceMap(result as any)).toBeTruthy();
    });

    // @ai-generated - Invalid UTF-8 Buffer throws descriptive error
    it("should throw on invalid UTF-8 Buffer", () => {
      const invalidUtf8 = Buffer.from([0x80, 0x81, 0x82]);

      expect(() => compile(invalidUtf8)).toThrow("UTF-8");
    });
  });

  describe("compileSync", () => {
    // @ai-generated - compileSync accepts Buffer input
    it("should produce identical results for string and Buffer input", () => {
      const stringResult = compileSync(SFC_INPUT, { filename: "Test.vue" });
      const bufferResult = compileSync(Buffer.from(SFC_INPUT), { filename: "Test.vue" });

      expect(bufferResult.code).toBe(stringResult.code);
    });

    // @ai-generated - compileSync throws on invalid UTF-8
    it("should throw on invalid UTF-8 Buffer", () => {
      const invalidUtf8 = Buffer.from([0x80, 0x81, 0x82]);

      expect(() => compileSync(invalidUtf8)).toThrow("UTF-8");
    });
  });

  describe("compileForVite", () => {
    // @ai-generated - compileForVite accepts Buffer input
    it("should produce identical results for string and Buffer input", () => {
      const stringResult = compileForVite(SFC_INPUT, { filename: "Test.vue" });
      const bufferResult = compileForVite(Buffer.from(SFC_INPUT), { filename: "Test.vue" });

      expect(bufferResult.script?.code).toBe(stringResult.script?.code);
      expect(bufferResult.template?.code).toBe(stringResult.template?.code);
      expect(bufferResult.styles.length).toBe(stringResult.styles.length);
    });

    // @ai-generated - compileForVite throws on invalid UTF-8
    it("should throw on invalid UTF-8 Buffer", () => {
      const invalidUtf8 = Buffer.from([0x80, 0x81, 0x82]);

      expect(() => compileForVite(invalidUtf8)).toThrow("UTF-8");
    });
  });
});
