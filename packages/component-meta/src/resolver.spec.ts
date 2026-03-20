import { describe, it, expect } from "vitest";
import { parseType, runtimeTypeToDescriptor } from "./resolver.js";
import type { TypeDescriptor } from "./type-ir.js";

describe("parseType", () => {
  // ── Primitives ──────────────────────────────────────────────

  it("parses primitive types", () => {
    expect(parseType("string")).toEqual({ kind: "primitive", name: "string" });
    expect(parseType("number")).toEqual({ kind: "primitive", name: "number" });
    expect(parseType("boolean")).toEqual({ kind: "primitive", name: "boolean" });
    expect(parseType("void")).toEqual({ kind: "primitive", name: "void" });
    expect(parseType("never")).toEqual({ kind: "primitive", name: "never" });
    expect(parseType("any")).toEqual({ kind: "primitive", name: "any" });
    expect(parseType("unknown")).toEqual({ kind: "primitive", name: "unknown" });
    expect(parseType("null")).toEqual({ kind: "primitive", name: "null" });
    expect(parseType("undefined")).toEqual({ kind: "primitive", name: "undefined" });
    expect(parseType("symbol")).toEqual({ kind: "primitive", name: "symbol" });
    expect(parseType("bigint")).toEqual({ kind: "primitive", name: "bigint" });
    expect(parseType("object")).toEqual({ kind: "primitive", name: "object" });
  });

  it("does not produce unknown for known primitives", () => {
    for (const prim of ["string", "number", "boolean", "null", "undefined"]) {
      const result = parseType(prim);
      expect(result.kind).not.toBe("unknown");
    }
  });

  // ── Literals ────────────────────────────────────────────────

  it("parses string literals", () => {
    expect(parseType("'primary'")).toEqual({ kind: "literal", value: "primary" });
    expect(parseType('"secondary"')).toEqual({ kind: "literal", value: "secondary" });
  });

  it("parses number literals", () => {
    expect(parseType("42")).toEqual({ kind: "literal", value: 42 });
    expect(parseType("3.14")).toEqual({ kind: "literal", value: 3.14 });
    expect(parseType("-1")).toEqual({ kind: "literal", value: -1 });
  });

  it("parses boolean literals", () => {
    expect(parseType("true")).toEqual({ kind: "literal", value: true });
    expect(parseType("false")).toEqual({ kind: "literal", value: false });
  });

  // ── Unions ──────────────────────────────────────────────────

  it("parses simple unions", () => {
    const result = parseType("string | number");
    expect(result).toEqual({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
  });

  it("parses literal unions", () => {
    const result = parseType("'a' | 'b' | 'c'");
    expect(result).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "a" },
        { kind: "literal", value: "b" },
        { kind: "literal", value: "c" },
      ],
    });
    // Negative: should not contain primitives
    expect(result.kind).not.toBe("primitive");
  });

  it("handles leading pipe in unions", () => {
    const result = parseType("| 'a' | 'b'");
    expect(result).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "a" },
        { kind: "literal", value: "b" },
      ],
    });
  });

  // ── Intersections ───────────────────────────────────────────

  it("parses intersections", () => {
    const result = parseType("A & B");
    expect(result).toEqual({
      kind: "intersection",
      types: [
        { kind: "ref", name: "A" },
        { kind: "ref", name: "B" },
      ],
    });
  });

  // ── Arrays ──────────────────────────────────────────────────

  it("parses array shorthand", () => {
    expect(parseType("string[]")).toEqual({
      kind: "array",
      element: { kind: "primitive", name: "string" },
    });
  });

  it("parses multi-dimensional arrays", () => {
    expect(parseType("number[][]")).toEqual({
      kind: "array",
      element: {
        kind: "array",
        element: { kind: "primitive", name: "number" },
      },
    });
  });

  it("parses Array<T> generic form", () => {
    expect(parseType("Array<string>")).toEqual({
      kind: "array",
      element: { kind: "primitive", name: "string" },
    });
  });

  it("does not produce ref for Array<T>", () => {
    const result = parseType("Array<number>");
    expect(result.kind).toBe("array");
    expect(result.kind).not.toBe("ref");
  });

  // ── Tuples ──────────────────────────────────────────────────

  it("parses tuples", () => {
    expect(parseType("[string, number]")).toEqual({
      kind: "tuple",
      elements: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
  });

  it("parses empty tuple", () => {
    expect(parseType("[]")).toEqual({ kind: "tuple", elements: [] });
  });

  it("parses labeled tuples", () => {
    const result = parseType("[id: number, name: string]");
    expect(result.kind).toBe("tuple");
    expect(result.kind === "tuple" && result.elements).toEqual([
      { kind: "primitive", name: "number" },
      { kind: "primitive", name: "string" },
    ]);
    // Should NOT be unknown
    expect(result.kind).not.toBe("unknown");
  });

  it("parses single labeled tuple element", () => {
    const result = parseType("[id: number]");
    expect(result.kind).toBe("tuple");
    expect(result.kind === "tuple" && result.elements).toEqual([
      { kind: "primitive", name: "number" },
    ]);
  });

  it("parses mixed labeled and unlabeled tuple elements", () => {
    // TypeScript actually doesn't allow mixing, but we should handle gracefully
    const result = parseType("[id: number, string]");
    expect(result.kind).toBe("tuple");
  });

  // ── Objects ─────────────────────────────────────────────────

  it("parses object types", () => {
    const result = parseType("{ name: string; age?: number }");
    expect(result).toEqual({
      kind: "object",
      properties: [
        { name: "name", type: { kind: "primitive", name: "string" }, optional: false },
        { name: "age", type: { kind: "primitive", name: "number" }, optional: true },
      ],
    });
  });

  it("parses empty object", () => {
    expect(parseType("{}")).toEqual({ kind: "object", properties: [] });
  });

  it("parses object with comma separators", () => {
    const result = parseType("{ x: number, y: number }");
    expect(result).toEqual({
      kind: "object",
      properties: [
        { name: "x", type: { kind: "primitive", name: "number" }, optional: false },
        { name: "y", type: { kind: "primitive", name: "number" }, optional: false },
      ],
    });
  });

  it("does not confuse object type with array", () => {
    const result = parseType("{ name: string }");
    expect(result.kind).toBe("object");
    expect(result.kind).not.toBe("array");
  });

  // ── Functions ───────────────────────────────────────────────

  it("parses function types", () => {
    const result = parseType("(x: string) => void");
    expect(result).toEqual({
      kind: "function",
      parameters: [{ name: "x", type: { kind: "primitive", name: "string" }, optional: false }],
      returnType: { kind: "primitive", name: "void" },
    });
  });

  it("parses function with no params", () => {
    const result = parseType("() => void");
    expect(result).toEqual({
      kind: "function",
      parameters: [],
      returnType: { kind: "primitive", name: "void" },
    });
  });

  it("parses function with optional params", () => {
    const result = parseType("(x: string, y?: number) => boolean");
    expect(result).toEqual({
      kind: "function",
      parameters: [
        { name: "x", type: { kind: "primitive", name: "string" }, optional: false },
        { name: "y", type: { kind: "primitive", name: "number" }, optional: true },
      ],
      returnType: { kind: "primitive", name: "boolean" },
    });
  });

  // ── Generics ────────────────────────────────────────────────

  it("parses generic type references", () => {
    expect(parseType("Map<string, number>")).toEqual({
      kind: "ref",
      name: "Map",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
  });

  it("parses simple named type reference", () => {
    expect(parseType("Date")).toEqual({ kind: "ref", name: "Date" });
  });

  it("parses nested generics", () => {
    const result = parseType("Promise<Array<string>>");
    expect(result).toEqual({
      kind: "ref",
      name: "Promise",
      typeArguments: [{ kind: "array", element: { kind: "primitive", name: "string" } }],
    });
  });

  // ── Parenthesized ───────────────────────────────────────────

  it("parses parenthesized groups", () => {
    const result = parseType("(string | number)[]");
    expect(result).toEqual({
      kind: "array",
      element: {
        kind: "union",
        types: [
          { kind: "primitive", name: "string" },
          { kind: "primitive", name: "number" },
        ],
      },
    });
  });

  // ── Union flattening ────────────────────────────────────────

  it("returns single type for single-member union", () => {
    // `union([T])` should return `T` directly
    const result = parseType("string");
    expect(result.kind).toBe("primitive");
    expect(result.kind).not.toBe("union");
  });

  // ── Complex / fallback ──────────────────────────────────────

  // The legacy JS parser produces partial parses for conditional and template
  // literal types. The native evaluator (evaluateTypes) handles these correctly.
  // These tests document the JS parser's actual behavior.
  it("produces partial parse for conditional types (native evaluator handles correctly)", () => {
    const result = parseType("T extends string ? A : B");
    // JS parser only sees "T" as identifier, doesn't understand "extends"
    expect(result.kind).toBe("ref");
  });

  it("produces partial parse for template literal types (native evaluator handles correctly)", () => {
    const result = parseType("`${number}px`");
    // JS parser can't handle backtick-delimited template literal types
    expect(result.kind).not.toBe("primitive");
  });

  it("falls back to unknown for empty input", () => {
    expect(parseType("")).toEqual({ kind: "unknown", rawType: "" });
  });

  it("handles whitespace-only input", () => {
    expect(parseType("   ")).toEqual({ kind: "unknown", rawType: "" });
  });

  // ── Readonly modifier ───────────────────────────────────────

  it("handles readonly prefix on types", () => {
    const result = parseType("readonly string[]");
    expect(result).toEqual({
      kind: "array",
      element: { kind: "primitive", name: "string" },
    });
    // Should not contain "readonly" in the output
    expect(JSON.stringify(result)).not.toContain("readonly");
  });

  // ── Real-world Vue prop types ───────────────────────────────

  it("parses real-world Vue prop type: string | number", () => {
    const result = parseType("string | number");
    expect(result.kind).toBe("union");
    if (result.kind === "union") {
      expect(result.types).toHaveLength(2);
      expect(result.types[0]).toEqual({ kind: "primitive", name: "string" });
    }
  });

  it("parses real-world Vue prop type: 'sm' | 'md' | 'lg'", () => {
    const result = parseType("'sm' | 'md' | 'lg'");
    expect(result.kind).toBe("union");
    if (result.kind === "union") {
      expect(result.types).toHaveLength(3);
      expect(result.types[0]).toEqual({ kind: "literal", value: "sm" });
    }
  });

  it("parses real-world Vue prop type: Record<string, unknown>", () => {
    const result = parseType("Record<string, unknown>");
    expect(result).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "unknown" },
      ],
    });
  });
});

describe("runtimeTypeToDescriptor", () => {
  it("converts runtime constructors to TypeDescriptors", () => {
    expect(runtimeTypeToDescriptor("String")).toEqual({ kind: "primitive", name: "string" });
    expect(runtimeTypeToDescriptor("Number")).toEqual({ kind: "primitive", name: "number" });
    expect(runtimeTypeToDescriptor("Boolean")).toEqual({ kind: "primitive", name: "boolean" });
    expect(runtimeTypeToDescriptor("Symbol")).toEqual({ kind: "primitive", name: "symbol" });
    expect(runtimeTypeToDescriptor("BigInt")).toEqual({ kind: "primitive", name: "bigint" });
  });

  it("converts Object/Array/Function constructors", () => {
    expect(runtimeTypeToDescriptor("Object")).toEqual({ kind: "object", properties: [] });
    expect(runtimeTypeToDescriptor("Array").kind).toBe("array");
    expect(runtimeTypeToDescriptor("Function").kind).toBe("function");
  });

  it("converts Date/RegExp/Promise to refs", () => {
    expect(runtimeTypeToDescriptor("Date")).toEqual({ kind: "ref", name: "Date" });
    expect(runtimeTypeToDescriptor("RegExp")).toEqual({ kind: "ref", name: "RegExp" });
    expect(runtimeTypeToDescriptor("Promise").kind).toBe("ref");
  });

  it("converts unknown constructors to refs", () => {
    const result = runtimeTypeToDescriptor("MyCustomType");
    expect(result).toEqual({ kind: "ref", name: "MyCustomType" });
    expect(result.kind).not.toBe("unknown");
  });
});
