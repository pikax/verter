import { describe, it, expect } from "vitest";
import { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";
import {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  ref,
  unknown,
  recursiveRef,
  indexedAccess,
} from "@verter/type-ir";
import type { PropertyMetaSchema } from "./types.js";
import { nativeTypeRegistryToMap } from "../native-component-meta.js";
import type { NativeComponentMetaResult } from "../native-component-meta.js";

describe("typeDescriptorToString", () => {
  it("withholds duplicate names when exact declaration positions are absent", () => {
    const rows = ["small", "large"].map((value) => ({
      name: "Size",
      type: { kind: "literal", literalKind: "string", value },
      typeFact: { kind: "complete", value: { kind: "literal", literalKind: "string", value } },
      declaration: {
        canonicalSource: "/types/sizes.ts",
        requestedName: "Size",
        resolvedName: "Size",
        kind: "typeAlias",
      },
    }));
    const registry = nativeTypeRegistryToMap({ typeRegistry: rows } as NativeComponentMetaResult);
    expect(typeDescriptorToSchema(ref("Size"), undefined, registry)).toEqual({
      kind: "object",
      type: "Size",
      schema: {},
    });
  });
  it("joins a producer row once and leaves its referenced row symbolic", () => {
    const registry = new Map([
      ["Size", union([literal("small"), ref("NestedSize")])],
      ["NestedSize", literal("large")],
    ]);
    expect(typeDescriptorToSchema(ref("Size"), undefined, registry)).toEqual({
      kind: "enum",
      type: '"small" | NestedSize',
      schema: ['"small"', { kind: "object", type: "NestedSize", schema: {} }],
    });
  });

  it.each([false, true])(
    "withholds colliding declaration names regardless of row order: %s",
    (reverse) => {
      const rows = ["/types/first.ts", "/types/second.ts"].map((canonicalSource, index) => ({
        name: "Size",
        type: { kind: "literal", literalKind: "string", value: index ? "large" : "small" },
        typeFact: {
          kind: "complete",
          value: { kind: "literal", literalKind: "string", value: index ? "large" : "small" },
        },
        declaration: {
          canonicalSource,
          requestedName: "Size",
          resolvedName: "Size",
          kind: "typeAlias",
        },
      }));
      const registry = nativeTypeRegistryToMap({
        typeRegistry: reverse ? rows.reverse() : rows,
      } as NativeComponentMetaResult);
      expect(typeDescriptorToSchema(ref("Size"), undefined, registry)).toEqual({
        kind: "object",
        type: "Size",
        schema: {},
      });
    },
  );
  it("converts primitives", () => {
    expect(typeDescriptorToString(primitive("string"))).toBe("string");
    expect(typeDescriptorToString(primitive("number"))).toBe("number");
    expect(typeDescriptorToString(primitive("boolean"))).toBe("boolean");
  });

  it("converts literals", () => {
    expect(typeDescriptorToString(literal("hello"))).toBe('"hello"');
    expect(typeDescriptorToString(literal(42))).toBe("42");
    expect(typeDescriptorToString(literal(true))).toBe("true");
  });

  it("converts unions", () => {
    expect(typeDescriptorToString(union([literal("a"), literal("b")]))).toBe('"a" | "b"');
  });

  it("converts intersections", () => {
    expect(typeDescriptorToString(intersection([primitive("string"), primitive("number")]))).toBe(
      "string & number",
    );
  });

  it("converts arrays", () => {
    expect(typeDescriptorToString(array(primitive("string")))).toBe("string[]");
  });

  it("converts tuples", () => {
    expect(typeDescriptorToString(tuple([primitive("string"), primitive("number")]))).toBe(
      "[string, number]",
    );
  });

  it("prints readonly tuples with optional and rest elements", () => {
    const descriptor = tuple(
      [primitive("number"), primitive("string"), array(primitive("boolean"))],
      [{ label: "id" }, { label: "name", optional: true }, { label: "rest", rest: true }],
      true,
    );
    const text = "readonly [id: number, name?: string, ...rest: boolean[]]";
    expect(typeDescriptorToString(descriptor)).toBe(text);
    expect(typeDescriptorToSchema(descriptor)).toEqual({
      kind: "array",
      type: text,
      schema: ["number", "string", { kind: "array", type: "boolean[]", schema: ["boolean"] }],
    });
    expect(
      typeDescriptorToString(
        tuple(
          [union([primitive("number"), primitive("string")]), array(primitive("boolean"))],
          [{ optional: true }, { rest: true }],
        ),
      ),
    ).toBe("[(number | string)?, ...boolean[]]");
  });

  it("converts objects to 'object'", () => {
    expect(
      typeDescriptorToString(object([{ name: "x", type: primitive("string"), optional: false }])),
    ).toBe("object");
  });

  it("renders index-signature objects with literal TS syntax", () => {
    expect(
      typeDescriptorToString(
        object([], {
          indexSignatures: [
            {
              keyName: "key",
              keyType: primitive("string"),
              valueType: primitive("any"),
            },
          ],
        }),
      ),
    ).toBe("{ [key: string]: any; }");
  });

  it("converts functions to 'function'", () => {
    expect(
      typeDescriptorToString(
        func([{ name: "x", type: primitive("string"), optional: false }], primitive("void")),
      ),
    ).toBe("function");
  });

  it("converts refs", () => {
    expect(typeDescriptorToString(ref("MyType"))).toBe("MyType");
    expect(typeDescriptorToString(ref("Map", [primitive("string"), primitive("number")]))).toBe(
      "Map<string, number>",
    );
  });

  it("projects unknown mechanically without rendering diagnostic text", () => {
    expect(typeDescriptorToString(unknown("void"))).toBe("unknown");
    expect(typeDescriptorToString(unknown(""))).toBe("unknown");
  });
});

describe("typeDescriptorToSchema", () => {
  it("converts primitives to strings", () => {
    expect(typeDescriptorToSchema(primitive("string"))).toBe("string");
    expect(typeDescriptorToSchema(primitive("number"))).toBe("number");
  });

  it("converts literals to quoted strings", () => {
    expect(typeDescriptorToSchema(literal("hello"))).toBe('"hello"');
    expect(typeDescriptorToSchema(literal(42))).toBe("42");
  });

  it("converts unions to enum schema", () => {
    const schema = typeDescriptorToSchema(union([literal("a"), literal("b")]));
    expect(schema).toEqual({
      kind: "enum",
      type: '"a" | "b"',
      schema: ['"a"', '"b"'],
    });
  });

  it("converts intersections to object schema", () => {
    const schema = typeDescriptorToSchema(intersection([primitive("string"), primitive("number")]));
    expect(schema).toEqual({
      kind: "object",
      type: "string & number",
      schema: ["string", "number"],
    });
  });

  it("converts arrays to array schema", () => {
    const schema = typeDescriptorToSchema(array(primitive("string")));
    expect(schema).toEqual({
      kind: "array",
      type: "string[]",
      schema: ["string"],
    });
  });

  it("converts tuples to array schema", () => {
    const schema = typeDescriptorToSchema(tuple([primitive("string"), primitive("number")]));
    expect(schema).toEqual({
      kind: "array",
      type: "[string, number]",
      schema: ["string", "number"],
    });
  });

  it("converts objects to object schema with PropertyMeta-like entries", () => {
    const schema = typeDescriptorToSchema(
      object([{ name: "x", type: primitive("string"), optional: false }]),
    );
    expect(schema).toEqual({
      kind: "object",
      type: "{ x: string }",
      schema: {
        x: {
          name: "x",
          global: false,
          description: "",
          tags: [],
          required: true,
          type: "string",
          schema: "string",
        },
      },
    });
  });

  it("preserves index signatures in object schema type strings", () => {
    const schema = typeDescriptorToSchema(
      object([], {
        indexSignatures: [
          {
            keyName: "key",
            keyType: primitive("string"),
            valueType: primitive("any"),
          },
        ],
      }),
    );
    expect(schema).toEqual({
      kind: "object",
      type: "{ [key: string]: any; }",
      schema: {},
    });
  });

  it("converts functions to string (no schema)", () => {
    const schema = typeDescriptorToSchema(
      func([{ name: "x", type: primitive("string"), optional: false }], primitive("void")),
    );
    expect(typeof schema).toBe("string");
    expect(schema).toBe("(x: string) => void");
  });

  it("converts refs to structured object schema", () => {
    expect(typeDescriptorToSchema(ref("MyType"))).toEqual({
      kind: "object",
      type: "MyType",
      schema: {},
    });
    expect(typeDescriptorToSchema(ref("Map", [primitive("string"), primitive("number")]))).toEqual({
      kind: "object",
      type: "Map<string, number>",
      schema: {},
    });
  });

  it("projects unknown schema as unsupported without manufacturing meaning", () => {
    expect(typeDescriptorToSchema(unknown("Booleanish"))).toBe("unknown");
  });

  it("returns 'unknown' when schema: false", () => {
    expect(typeDescriptorToSchema(primitive("string"), { schema: false })).toBe("unknown");
  });

  it("respects schema.ignore filter", () => {
    const schema = typeDescriptorToSchema(union([literal("a"), literal("b")]), {
      schema: { ignore: (type) => type.includes("|") },
    });
    // When ignored, returns the type as a string instead of schema object
    expect(typeof schema).toBe("string");
    expect(schema).toBe('"a" | "b"');
  });

  it("does not produce schema objects for ignored nested types", () => {
    const schema = typeDescriptorToSchema(array(primitive("string")), {
      schema: { ignore: () => true },
    });
    expect(typeof schema).toBe("string");
  });

  // @ai-generated - Verifies benchmark-only literal boolean schema expansion stays opt-in.
  it("can expand booleans into literal enum schema when parity mode is enabled", () => {
    expect(
      typeDescriptorToSchema(primitive("boolean"), {
        schema: { literalBooleanSchema: true },
      }),
    ).toEqual({
      kind: "enum",
      type: "boolean",
      schema: ["false", "true"],
    });
  });

  it("recursiveRef with type arguments renders generic name in schema", () => {
    const schema = typeDescriptorToSchema(recursiveRef("TreeNode", [primitive("string")], []));
    expect(schema).toEqual({
      kind: "object",
      type: "TreeNode<string>",
      schema: {},
    });
  });

  it("recursiveRef with multiple type arguments renders full generic name", () => {
    const schema = typeDescriptorToSchema(
      recursiveRef("Map", [primitive("string"), primitive("number")], []),
    );
    expect(schema).toEqual({
      kind: "object",
      type: "Map<string, number>",
      schema: {},
    });
  });

  it("recursiveRef without type arguments keeps bare name", () => {
    const schema = typeDescriptorToSchema(recursiveRef("Tree", [], []));
    expect(schema).toEqual({
      kind: "object",
      type: "Tree",
      schema: {},
    });
  });

  it("union schema uses recursiveRef generic text in its type field", () => {
    const schema = typeDescriptorToSchema(
      union([recursiveRef("TreeNode", [primitive("string")], []), primitive("undefined")]),
    );
    expect(schema).toEqual({
      kind: "enum",
      type: "TreeNode<string> | undefined",
      schema: [
        {
          kind: "object",
          type: "TreeNode<string>",
          schema: {},
        },
        "undefined",
      ],
    });
  });

  it("keeps nested registry refs shallow when resolving a top-level alias", () => {
    const schema = typeDescriptorToSchema(
      ref("StringOrVNode"),
      undefined,
      new Map([
        ["StringOrVNode", union([primitive("string"), ref("VNode")])],
        [
          "VNode",
          object([
            { name: "type", type: primitive("string"), optional: false },
            { name: "component", type: ref("ComponentInternalInstance"), optional: true },
          ]),
        ],
        [
          "ComponentInternalInstance",
          object([{ name: "vnode", type: ref("VNode"), optional: false }]),
        ],
      ]),
    );

    expect(schema).toEqual({
      kind: "enum",
      type: "string | VNode",
      schema: [
        "string",
        {
          kind: "object",
          type: "VNode",
          schema: {},
        },
      ],
    });
  });

  // @ai-generated - Distinguishes producer-expanded indexed access from an unsubstituted generic body.
  it("expands non-generic indexed access from the producer registry", () => {
    const schema = typeDescriptorToSchema(
      indexedAccess(ref("RecordShape"), literal("value")),
      undefined,
      new Map([
        ["RecordShape", object([{ name: "value", type: primitive("string"), optional: false }])],
      ]),
    );

    expect(schema).toBe("string");
  });

  // @ai-generated - A generic registry declaration is not an instantiated descriptor.
  it("keeps generic indexed access shallow when only an unsubstituted declaration is registered", () => {
    const schema = typeDescriptorToSchema(
      indexedAccess(ref("Box", [primitive("string")]), literal("value")),
      undefined,
      new Map([
        [
          "Box",
          object([{ name: "value", type: { kind: "typeParameter", name: "T" }, optional: false }]),
        ],
      ]),
    );

    expect(schema).toEqual({
      kind: "object",
      type: 'Box<string>["value"]',
      schema: {},
    });
  });

  // Rendered application text cannot identify a producer row.
  it("keeps generic indexed access shallow even when a rendered-name row exists", () => {
    const schema = typeDescriptorToSchema(
      indexedAccess(ref("Box", [primitive("string")]), literal("value")),
      undefined,
      new Map([
        ["Box<string>", object([{ name: "value", type: primitive("string"), optional: false }])],
      ]),
    );

    expect(schema).toEqual({ kind: "object", type: 'Box<string>["value"]', schema: {} });
  });
});
