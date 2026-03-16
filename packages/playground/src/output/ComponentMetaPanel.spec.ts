import { describe, it, expect } from "vitest";
import { formatTypeDescriptor } from "./formatTypeDescriptor";
import type { TypeDescriptor } from "@verter/component-meta/browser";

describe("formatTypeDescriptor", () => {
  it("formats primitive types", () => {
    const td: TypeDescriptor = { kind: "primitive", name: "string" };
    expect(formatTypeDescriptor(td)).toBe("string");
  });

  it("formats literal string", () => {
    const td: TypeDescriptor = { kind: "literal", value: "hello" };
    expect(formatTypeDescriptor(td)).toBe('"hello"');
  });

  it("formats literal number", () => {
    const td: TypeDescriptor = { kind: "literal", value: 42 };
    expect(formatTypeDescriptor(td)).toBe("42");
  });

  it("formats literal boolean", () => {
    const td: TypeDescriptor = { kind: "literal", value: true };
    expect(formatTypeDescriptor(td)).toBe("true");
  });

  it("formats union types", () => {
    const td: TypeDescriptor = {
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("string | number");
  });

  it("formats intersection types", () => {
    const td: TypeDescriptor = {
      kind: "intersection",
      types: [
        { kind: "ref", name: "Foo" },
        { kind: "ref", name: "Bar" },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("Foo & Bar");
  });

  it("formats array of primitive", () => {
    const td: TypeDescriptor = {
      kind: "array",
      element: { kind: "primitive", name: "string" },
    };
    expect(formatTypeDescriptor(td)).toBe("string[]");
  });

  it("parenthesizes union inside array", () => {
    const td: TypeDescriptor = {
      kind: "array",
      element: {
        kind: "union",
        types: [
          { kind: "primitive", name: "string" },
          { kind: "primitive", name: "number" },
        ],
      },
    };
    expect(formatTypeDescriptor(td)).toBe("(string | number)[]");
  });

  it("formats tuple types", () => {
    const td: TypeDescriptor = {
      kind: "tuple",
      elements: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("[string, number]");
  });

  it("formats object types", () => {
    const td: TypeDescriptor = {
      kind: "object",
      properties: [
        { name: "foo", type: { kind: "primitive", name: "string" }, optional: false },
        { name: "bar", type: { kind: "primitive", name: "number" }, optional: true },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("{ foo: string; bar?: number }");
  });

  it("formats function types", () => {
    const td: TypeDescriptor = {
      kind: "function",
      parameters: [{ name: "x", type: { kind: "primitive", name: "number" }, optional: false }],
      returnType: { kind: "primitive", name: "void" },
    };
    expect(formatTypeDescriptor(td)).toBe("(x: number) => void");
  });

  it("formats enum types", () => {
    const td: TypeDescriptor = {
      kind: "enum",
      name: "Color",
      members: [
        { name: "Red", value: 0 },
        { name: "Green", value: 1 },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("Color");
  });

  it("formats ref types without type arguments", () => {
    const td: TypeDescriptor = { kind: "ref", name: "MyType" };
    expect(formatTypeDescriptor(td)).toBe("MyType");
  });

  it("formats ref types with type arguments", () => {
    const td: TypeDescriptor = {
      kind: "ref",
      name: "Map",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("Map<string, number>");
  });

  it("formats unknown types", () => {
    const td: TypeDescriptor = { kind: "unknown", rawType: "SomeComplexType" };
    expect(formatTypeDescriptor(td)).toBe("SomeComplexType");
  });

  it("handles nested complex types", () => {
    const td: TypeDescriptor = {
      kind: "ref",
      name: "Promise",
      typeArguments: [
        {
          kind: "union",
          types: [
            { kind: "primitive", name: "string" },
            { kind: "primitive", name: "null" },
          ],
        },
      ],
    };
    expect(formatTypeDescriptor(td)).toBe("Promise<string | null>");
  });
});
