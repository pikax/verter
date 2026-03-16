/**
 * @ai-generated - Tests for the @verter/types virtual stub content.
 * Verifies the stub contains all symbols that Rust TSX codegen may import.
 */
import { describe, it, expect } from "vitest";
import { VERTER_TYPES_STUB } from "./verterTypesStub";

describe("VERTER_TYPES_STUB", () => {
  it("is a non-empty string", () => {
    expect(VERTER_TYPES_STUB).toBeTruthy();
    expect(typeof VERTER_TYPES_STUB).toBe("string");
  });

  it("imports only from vue (no circular @verter/types dependency)", () => {
    expect(VERTER_TYPES_STUB).toContain('from "vue"');
    expect(VERTER_TYPES_STUB).not.toContain('from "@verter/types"');
  });

  describe("always-imported core types (tsx/script.rs:2338-2343)", () => {
    it("exports Prettify type", () => {
      expect(VERTER_TYPES_STUB).toContain("export type Prettify<");
    });

    it("exports PublicInstanceFromMacro type", () => {
      expect(VERTER_TYPES_STUB).toContain("export type PublicInstanceFromMacro<");
    });

    it("exports ExtractComponentProps type", () => {
      expect(VERTER_TYPES_STUB).toContain("export type ExtractComponentProps<");
    });

    it("exports OmitConstructorSignature type", () => {
      expect(VERTER_TYPES_STUB).toContain("export type OmitConstructorSignature<");
    });
  });

  describe("always-imported runtime functions (tsx/script.rs:2356-2358)", () => {
    it("exports shallowUnwrapRef function", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function shallowUnwrapRef<");
    });

    it("exports enhanceElementWithProps function", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function enhanceElementWithProps<");
    });

    it("exports createMacroReturn function", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function createMacroReturn<");
    });
  });

  describe("conditional Box helpers (tsx/script.rs:2346, via box_standard_macro)", () => {
    it("exports defineProps_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function defineProps_Box");
    });

    it("exports defineEmits_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function defineEmits_Box");
    });

    it("exports defineSlots_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function defineSlots_Box");
    });

    it("exports defineExpose_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function defineExpose_Box");
    });

    it("exports defineOptions_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function defineOptions_Box");
    });

    it("exports defineModel_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function defineModel_Box");
    });

    it("exports withDefaults_Box", () => {
      expect(VERTER_TYPES_STUB).toContain("export declare function withDefaults_Box");
    });
  });

  it("does not contain import() expressions (self-contained)", () => {
    // The stub should use direct imports, not dynamic import() type references
    expect(VERTER_TYPES_STUB).not.toMatch(/import\(/);
  });
});
