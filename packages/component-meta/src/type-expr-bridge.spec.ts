import { describe, it, expect } from "vitest";
import { typeExprToDescriptor, buildEvaluatedTypeMap } from "./type-expr-bridge.js";
import type { NativeTypeExpr, NativeEvaluatedField } from "./type-expr-bridge.js";
import {
  DecodedTypeGraph,
  createGraphTypeExprRef,
  NODE_MAPPED,
  NODE_PRIMITIVE,
} from "./type-graph-core.js";

// =============================================================================
// typeExprToDescriptor: primitives
// =============================================================================

describe("typeExprToDescriptor", () => {
  it("converts primitive types", () => {
    const result = typeExprToDescriptor({ kind: "primitive", name: "string" });
    expect(result).toEqual({ kind: "primitive", name: "string" });
  });

  it("converts all primitive names", () => {
    for (const name of [
      "string",
      "number",
      "boolean",
      "symbol",
      "bigint",
      "any",
      "unknown",
      "void",
      "never",
      "null",
      "undefined",
      "object",
    ]) {
      const result = typeExprToDescriptor({ kind: "primitive", name });
      expect(result.kind).toBe("primitive");
      expect((result as { name: string }).name).toBe(name);
    }
  });

  // =============================================================================
  // Literals
  // =============================================================================

  it("converts string literal", () => {
    const result = typeExprToDescriptor({
      kind: "literal",
      literalKind: "string",
      value: "hello",
    });
    expect(result).toEqual({ kind: "literal", value: "hello" });
  });

  it("converts number literal", () => {
    const result = typeExprToDescriptor({
      kind: "literal",
      literalKind: "number",
      value: 42,
    });
    expect(result).toEqual({ kind: "literal", value: 42 });
  });

  it("converts boolean literal", () => {
    const result = typeExprToDescriptor({
      kind: "literal",
      literalKind: "boolean",
      value: true,
    });
    expect(result).toEqual({ kind: "literal", value: true });
  });

  // =============================================================================
  // Unions and intersections
  // =============================================================================

  it("converts union types", () => {
    const result = typeExprToDescriptor({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
    expect(result).toEqual({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
  });

  it("converts intersection types", () => {
    const result = typeExprToDescriptor({
      kind: "intersection",
      types: [
        { kind: "ref", name: "A", typeArguments: [] },
        { kind: "ref", name: "B", typeArguments: [] },
      ],
    });
    expect(result.kind).toBe("intersection");
  });

  // =============================================================================
  // Array and tuple
  // =============================================================================

  it("converts array type", () => {
    const result = typeExprToDescriptor({
      kind: "array",
      element: { kind: "primitive", name: "string" },
      readonly: false,
    });
    expect(result).toEqual({
      kind: "array",
      element: { kind: "primitive", name: "string" },
    });
  });

  it("converts tuple type", () => {
    const result = typeExprToDescriptor({
      kind: "tuple",
      elements: [
        { ty: { kind: "primitive", name: "string" }, optional: false, rest: false },
        { ty: { kind: "primitive", name: "number" }, optional: false, rest: false },
      ],
      readonly: false,
    });
    expect(result.kind).toBe("tuple");
    expect((result as { elements: unknown[] }).elements).toHaveLength(2);
  });

  it("converts native recursiveRef types without degrading to unknown", () => {
    const result = typeExprToDescriptor({
      kind: "recursiveRef",
      name: "Tree",
      typeArguments: [{ kind: "primitive", name: "string" }],
      conditionalContext: [
        {
          branch: "true",
          decided: true,
          check: { kind: "primitive", name: "string" },
          extends: { kind: "primitive", name: "string" },
        },
      ],
    } as NativeTypeExpr);

    expect(result).toEqual({
      kind: "recursiveRef",
      name: "Tree",
      typeArguments: [{ kind: "primitive", name: "string" }],
      conditionalContext: [
        {
          branch: "true",
          decided: true,
          check: { kind: "primitive", name: "string" },
          extends: { kind: "primitive", name: "string" },
        },
      ],
    });
  });

  // =============================================================================
  // Object
  // =============================================================================

  it("converts object with properties", () => {
    const result = typeExprToDescriptor({
      kind: "object",
      properties: [
        {
          memberKind: "property",
          name: "id",
          ty: { kind: "primitive", name: "number" },
          optional: false,
          readonly: false,
        },
        {
          memberKind: "property",
          name: "name",
          ty: { kind: "primitive", name: "string" },
          optional: true,
          readonly: false,
        },
      ],
    });
    expect(result.kind).toBe("object");
    const obj = result as { kind: "object"; properties: { name: string; optional: boolean }[] };
    expect(obj.properties).toHaveLength(2);
    expect(obj.properties[0].name).toBe("id");
    expect(obj.properties[0].optional).toBe(false);
    expect(obj.properties[1].name).toBe("name");
    expect(obj.properties[1].optional).toBe(true);
  });

  it("preserves index signatures on object types", () => {
    const result = typeExprToDescriptor({
      kind: "object",
      properties: [
        {
          memberKind: "property",
          name: "x",
          ty: { kind: "primitive", name: "number" },
          optional: false,
        },
        {
          memberKind: "indexSignature",
          keyName: "key",
          keyType: { kind: "primitive", name: "string" },
          valueType: { kind: "primitive", name: "any" },
        },
      ],
    });
    const obj = result as {
      kind: "object";
      properties: unknown[];
      indexSignatures?: Array<{
        keyName: string;
        keyType: { kind: string; name: string };
        valueType: { kind: string; name: string };
      }>;
    };
    expect(obj.properties).toHaveLength(1);
    expect(obj.indexSignatures).toEqual([
      {
        keyName: "key",
        keyType: { kind: "primitive", name: "string" },
        valueType: { kind: "primitive", name: "any" },
      },
    ]);
  });

  it("preserves construct signatures on object types", () => {
    const result = typeExprToDescriptor({
      kind: "object",
      properties: [
        {
          memberKind: "constructSignature",
          function: {
            parameters: [
              {
                name: "id",
                ty: { kind: "primitive", name: "number" },
                optional: false,
                rest: false,
              },
            ],
            returnType: {
              kind: "object",
              properties: [
                {
                  memberKind: "property",
                  name: "id",
                  ty: { kind: "primitive", name: "number" },
                  optional: false,
                },
              ],
            },
          },
        },
      ],
    });
    const obj = result as {
      kind: "object";
      constructSignatures?: Array<{
        kind: string;
        parameters: Array<{ name: string; type: { kind: string; name: string } }>;
      }>;
    };
    expect(obj.constructSignatures).toHaveLength(1);
    expect(obj.constructSignatures?.[0]?.parameters[0]).toEqual({
      name: "id",
      type: { kind: "primitive", name: "number" },
      optional: false,
    });
  });

  // =============================================================================
  // Function
  // =============================================================================

  it("converts function type", () => {
    const result = typeExprToDescriptor({
      kind: "function",
      parameters: [
        { name: "x", ty: { kind: "primitive", name: "string" }, optional: false, rest: false },
      ],
      returnType: { kind: "primitive", name: "void" },
    });
    expect(result.kind).toBe("function");
    const fn = result as {
      kind: "function";
      parameters: { name: string }[];
      returnType: { kind: string };
    };
    expect(fn.parameters).toHaveLength(1);
    expect(fn.parameters[0].name).toBe("x");
    expect(fn.returnType.kind).toBe("primitive");
  });

  it("preserves generic parameter nodes and function type parameters", () => {
    const result = typeExprToDescriptor({
      kind: "function",
      parameters: [
        {
          name: "value",
          ty: {
            kind: "typeParameter",
            name: "T",
          },
          optional: false,
          rest: false,
        },
      ],
      returnType: {
        kind: "typeParameter",
        name: "T",
        constraint: { kind: "primitive", name: "number" },
        default: { kind: "primitive", name: "string" },
      },
      typeParameters: [
        {
          name: "T",
          constraint: { kind: "primitive", name: "number" },
          default: { kind: "primitive", name: "string" },
        },
      ],
    } as NativeTypeExpr);
    expect(result).toEqual({
      kind: "function",
      parameters: [
        {
          name: "value",
          type: {
            kind: "typeParameter",
            name: "T",
          },
          optional: false,
        },
      ],
      returnType: {
        kind: "typeParameter",
        name: "T",
        constraint: { kind: "primitive", name: "number" },
        default: { kind: "primitive", name: "string" },
      },
      typeParameters: [
        {
          kind: "typeParameter",
          name: "T",
          constraint: { kind: "primitive", name: "number" },
          default: { kind: "primitive", name: "string" },
        },
      ],
    });
  });

  // =============================================================================
  // Ref
  // =============================================================================

  it("converts ref without type arguments", () => {
    const result = typeExprToDescriptor({
      kind: "ref",
      name: "MyType",
      typeArguments: [],
    });
    expect(result).toEqual({ kind: "ref", name: "MyType" });
  });

  it("converts ref with type arguments", () => {
    const result = typeExprToDescriptor({
      kind: "ref",
      name: "Promise",
      typeArguments: [{ kind: "primitive", name: "string" }],
    });
    expect(result).toEqual({
      kind: "ref",
      name: "Promise",
      typeArguments: [{ kind: "primitive", name: "string" }],
    });
  });

  // =============================================================================
  // Operator forms → unknown fallback
  // =============================================================================

  it("falls back to unknown for unreduced keyof", () => {
    const result = typeExprToDescriptor({
      kind: "keyOf",
      operand: { kind: "ref", name: "T", typeArguments: [] },
    });
    expect(result.kind).toBe("unknown");
  });

  it("falls back to unknown for unreduced typeof", () => {
    const result = typeExprToDescriptor({
      kind: "typeOf",
      path: ["myVar"],
    });
    expect(result.kind).toBe("unknown");
  });

  it("resolves indexed access through object unions when every branch has the key", () => {
    const result = typeExprToDescriptor(
      {
        kind: "indexedAccess",
        object: {
          kind: "ref",
          name: "Variants",
          typeArguments: [],
        },
        index: {
          kind: "literal",
          literalKind: "string",
          value: "color",
        },
      },
      new Map([
        [
          "Variants",
          {
            kind: "union",
            types: [
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "color",
                    ty: { kind: "literal", literalKind: "string", value: "red" },
                    optional: false,
                    readonly: false,
                  },
                ],
              },
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "color",
                    ty: { kind: "literal", literalKind: "string", value: "blue" },
                    optional: true,
                    readonly: false,
                  },
                ],
              },
            ],
          } satisfies NativeTypeExpr,
        ],
      ]),
    );

    expect(result).toEqual({
      kind: "union",
      types: [
        {
          kind: "union",
          types: [
            { kind: "literal", value: "red" },
            { kind: "literal", value: "blue" },
          ],
        },
        { kind: "primitive", name: "undefined" },
      ],
    });
  });

  it("resolves indexed access through object intersections", () => {
    const result = typeExprToDescriptor(
      {
        kind: "indexedAccess",
        object: {
          kind: "ref",
          name: "Props",
          typeArguments: [],
        },
        index: {
          kind: "literal",
          literalKind: "string",
          value: "tone",
        },
      },
      new Map([
        [
          "Props",
          {
            kind: "intersection",
            types: [
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "tone",
                    ty: { kind: "primitive", name: "string" },
                    optional: false,
                    readonly: false,
                  },
                ],
              },
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "size",
                    ty: { kind: "primitive", name: "number" },
                    optional: false,
                    readonly: false,
                  },
                ],
              },
            ],
          } satisfies NativeTypeExpr,
        ],
      ]),
    );

    expect(result).toEqual({ kind: "primitive", name: "string" });
  });

  it("preserves symbolic indexed access when a union branch does not have the key", () => {
    const result = typeExprToDescriptor(
      {
        kind: "indexedAccess",
        object: {
          kind: "ref",
          name: "Variants",
          typeArguments: [],
        },
        index: {
          kind: "literal",
          literalKind: "string",
          value: "color",
        },
      },
      new Map([
        [
          "Variants",
          {
            kind: "union",
            types: [
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "color",
                    ty: { kind: "literal", literalKind: "string", value: "red" },
                    optional: false,
                    readonly: false,
                  },
                ],
              },
              {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "size",
                    ty: { kind: "primitive", name: "number" },
                    optional: false,
                    readonly: false,
                  },
                ],
              },
            ],
          } satisfies NativeTypeExpr,
        ],
      ]),
    );

    // Unresolvable indexed access surfaces as the dedicated
    // `IndexedAccessType` variant so structural consumers (the compat
    // checker) can match `t.kind === "indexedAccess"` instead
    // of regex-scanning a raw-type string.
    expect(result.kind).toBe("indexedAccess");
    if (result.kind === "indexedAccess") {
      expect(result.objectType).toEqual({ kind: "ref", name: "Variants" });
      expect(result.indexType).toEqual({ kind: "literal", value: "color" });
    }
  });

  it("emits IndexedAccessType when no native registry is supplied", () => {
    // Discriminating fixture: the legacy path would collapse
    // to `kind: "unknown"`; the structural form survives here.
    const result = typeExprToDescriptor({
      kind: "indexedAccess",
      object: { kind: "ref", name: "NuxtLinkProps", typeArguments: [] },
      index: { kind: "literal", literalKind: "string", value: "to" },
    });

    expect(result).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "NuxtLinkProps" },
      indexType: { kind: "literal", value: "to" },
    });
  });

  it("emits IndexedAccessType for non-literal index types (T[K])", () => {
    // The legacy path returned `unknown("T[K]")` — the regex-based
    // `looksLikeIndexedAccessType` heuristic in compat/checker.ts
    // existed precisely because the structural shape was lost here.
    // The descriptor now preserves both sub-shapes.
    const result = typeExprToDescriptor({
      kind: "indexedAccess",
      object: { kind: "ref", name: "T", typeArguments: [] },
      index: { kind: "ref", name: "K", typeArguments: [] },
    });

    expect(result).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "T" },
      indexType: { kind: "ref", name: "K" },
    });
  });

  it("resolves chained indexed access through registry-materialized objects", () => {
    const result = typeExprToDescriptor(
      {
        kind: "indexedAccess",
        object: {
          kind: "indexedAccess",
          object: {
            kind: "ref",
            name: "Button",
            typeArguments: [],
          },
          index: {
            kind: "literal",
            literalKind: "string",
            value: "variants",
          },
        },
        index: {
          kind: "literal",
          literalKind: "string",
          value: "color",
        },
      },
      new Map([
        [
          "Button",
          {
            kind: "object",
            properties: [
              {
                memberKind: "property",
                name: "variants",
                ty: {
                  kind: "object",
                  properties: [
                    {
                      memberKind: "property",
                      name: "color",
                      ty: {
                        kind: "union",
                        types: [
                          { kind: "literal", literalKind: "string", value: "primary" },
                          { kind: "literal", literalKind: "string", value: "secondary" },
                        ],
                      },
                      optional: false,
                      readonly: false,
                    },
                  ],
                },
                optional: false,
                readonly: false,
              },
            ],
          } satisfies NativeTypeExpr,
        ],
      ]),
    );

    expect(result).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "primary" },
        { kind: "literal", value: "secondary" },
      ],
    });
  });

  it("materializes finite mapped variant helpers into object descriptors", () => {
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "K",
      source: {
        kind: "keyOf",
        operand: {
          kind: "object",
          properties: [
            {
              memberKind: "property",
              name: "color",
              ty: {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "primary",
                    ty: { kind: "primitive", name: "string" },
                    optional: false,
                  },
                  {
                    memberKind: "property",
                    name: "secondary",
                    ty: { kind: "primitive", name: "string" },
                    optional: false,
                  },
                ],
              },
              optional: false,
            },
            {
              memberKind: "property",
              name: "variant",
              ty: {
                kind: "object",
                properties: [
                  {
                    memberKind: "property",
                    name: "solid",
                    ty: { kind: "primitive", name: "string" },
                    optional: false,
                  },
                ],
              },
              optional: false,
            },
          ],
        },
      },
      value: {
        kind: "keyOf",
        operand: {
          kind: "indexedAccess",
          object: {
            kind: "object",
            properties: [
              {
                memberKind: "property",
                name: "color",
                ty: {
                  kind: "object",
                  properties: [
                    {
                      memberKind: "property",
                      name: "primary",
                      ty: { kind: "primitive", name: "string" },
                      optional: false,
                    },
                    {
                      memberKind: "property",
                      name: "secondary",
                      ty: { kind: "primitive", name: "string" },
                      optional: false,
                    },
                  ],
                },
                optional: false,
              },
              {
                memberKind: "property",
                name: "variant",
                ty: {
                  kind: "object",
                  properties: [
                    {
                      memberKind: "property",
                      name: "solid",
                      ty: { kind: "primitive", name: "string" },
                      optional: false,
                    },
                  ],
                },
                optional: false,
              },
            ],
          },
          index: {
            kind: "typeParameter",
            name: "K",
          },
        },
      },
    });

    expect(result).toEqual({
      kind: "object",
      properties: [
        {
          name: "color",
          type: {
            kind: "union",
            types: [
              { kind: "literal", value: "primary" },
              { kind: "literal", value: "secondary" },
            ],
          },
          optional: false,
        },
        {
          name: "variant",
          type: { kind: "literal", value: "solid" },
          optional: false,
        },
      ],
    });
  });

  it("resolves Id-style mapped values through type-parameter defaults", () => {
    const mappedSource = {
      kind: "object",
      properties: [
        {
          memberKind: "property",
          name: "base",
          ty: { kind: "primitive", name: "string" },
          optional: true,
        },
        {
          memberKind: "property",
          name: "label",
          ty: { kind: "primitive", name: "string" },
          optional: true,
        },
      ],
    } satisfies NativeTypeExpr;

    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: {
        kind: "keyOf",
        operand: {
          kind: "typeParameter",
          name: "T",
          default: mappedSource,
        },
      },
      value: {
        kind: "indexedAccess",
        object: {
          kind: "typeParameter",
          name: "T",
          default: mappedSource,
        },
        index: {
          kind: "typeParameter",
          name: "P",
        },
      },
    });

    expect(result).toEqual({
      kind: "object",
      properties: [
        {
          name: "base",
          type: {
            kind: "union",
            types: [
              { kind: "primitive", name: "string" },
              { kind: "primitive", name: "undefined" },
            ],
          },
          optional: true,
        },
        {
          name: "label",
          type: {
            kind: "union",
            types: [
              { kind: "primitive", name: "string" },
              { kind: "primitive", name: "undefined" },
            ],
          },
          optional: true,
        },
      ],
    });
  });

  it("collapses Id-style empty-object intersections after mapped helper expansion", () => {
    const result = typeExprToDescriptor({
      kind: "intersection",
      types: [
        {
          kind: "object",
          properties: [],
        },
        {
          kind: "mapped",
          parameter: "K",
          source: {
            kind: "keyOf",
            operand: {
              kind: "object",
              properties: [
                {
                  memberKind: "property",
                  name: "base",
                  ty: { kind: "primitive", name: "string" },
                  optional: false,
                },
                {
                  memberKind: "property",
                  name: "label",
                  ty: { kind: "primitive", name: "string" },
                  optional: false,
                },
              ],
            },
          },
          value: {
            kind: "function",
            parameters: [
              {
                name: "props",
                ty: {
                  kind: "ref",
                  name: "Record",
                  typeArguments: [
                    { kind: "primitive", name: "string" },
                    { kind: "primitive", name: "any" },
                  ],
                },
                optional: true,
                rest: false,
              },
            ],
            returnType: { kind: "primitive", name: "string" },
          },
        },
      ],
    });

    expect(result).toMatchObject({
      kind: "object",
      properties: [
        {
          name: "base",
          optional: false,
        },
        {
          name: "label",
          optional: false,
        },
      ],
    });
  });

  it("resolves an open-string-domain mapped type to a Record alias instead of unknown('mapped')", () => {
    // `Record<string, any>` is `{ [P in string]: any }`. The native producer
    // can surface it as a mapped type over the OPEN `string` key domain (no
    // finite key enumeration). The bridge must reconstruct the `Record<K, V>`
    // alias so the compat display renders `Record<string, any>` rather than
    // degrading to the lossy `unknown("mapped")` placeholder.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
    });

    expect(result).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "any" },
      ],
    });
    // Negative: the lossy mapped placeholder is gone.
    expect(result.kind).not.toBe("unknown");
  });

  it("reconstructs Record over number / symbol / union open key domains", () => {
    expect(
      typeExprToDescriptor({
        kind: "mapped",
        parameter: "P",
        source: { kind: "primitive", name: "number" },
        value: { kind: "primitive", name: "boolean" },
      }),
    ).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "number" },
        { kind: "primitive", name: "boolean" },
      ],
    });

    expect(
      typeExprToDescriptor({
        kind: "mapped",
        parameter: "P",
        source: {
          kind: "union",
          types: [
            { kind: "primitive", name: "string" },
            { kind: "primitive", name: "number" },
          ],
        },
        value: { kind: "primitive", name: "string" },
      }),
    ).toMatchObject({
      kind: "ref",
      name: "Record",
    });
  });

  it("keeps a generic open-domain mapped value (referencing the key) as unknown('mapped')", () => {
    // `{ [P in string]: P }` is NOT the `Record<K, V>` alias — its value
    // references the mapped key parameter. The conservative bridge keeps the
    // existing `unknown` fallback rather than leaking the bare parameter name.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "typeParameter", name: "P" },
    });
    expect(result.kind).toBe("unknown");
  });

  it("keeps a `+readonly` open-domain mapped type as unknown('mapped') (Record cannot express readonly)", () => {
    // `+readonly [P in string]: any` is NOT `Record<string, any>` — `Record`
    // has no readonly modifier. The readonly add/remove modifier only survives
    // on the graph mapped node, so this is exercised through the decoded graph.
    // The conservative bridge bails to `unknown` rather than silently dropping
    // the readonly modifier and emitting a MUTABLE `Record`.
    const readonlyAdd = new DecodedTypeGraph(
      ["P"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 }, // node 1: `string` source
        { kind: NODE_PRIMITIVE, primitive: 6 }, // node 2: `any` value
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 1,
          valueNodeId: 2,
          optionalModifier: 1, // MappedModifier::None
          readonlyModifier: 2, // MappedModifier::Add (`+readonly`)
          nameTypeNodeId: 0,
        },
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(readonlyAdd, 3)).kind).toBe("unknown");

    // A `-readonly` (remove) modifier is likewise inexpressible as `Record`.
    const readonlyRemove = new DecodedTypeGraph(
      ["P"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_PRIMITIVE, primitive: 6 },
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 1,
          valueNodeId: 2,
          optionalModifier: 1,
          readonlyModifier: 3, // MappedModifier::Remove (`-readonly`)
          nameTypeNodeId: 0,
        },
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(readonlyRemove, 3)).kind).toBe("unknown");

    // Discriminator: the SAME graph shape with NO readonly modifier still
    // recovers `Record<string, any>` — proving the bail is readonly-specific
    // and the graph-backed Record recovery path is intact.
    const noReadonly = new DecodedTypeGraph(
      ["P"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_PRIMITIVE, primitive: 6 },
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 1,
          valueNodeId: 2,
          optionalModifier: 1,
          readonlyModifier: 1, // MappedModifier::None
          nameTypeNodeId: 0,
        },
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(noReadonly, 3))).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "any" },
      ],
    });
  });

  it("keeps a key-remapped open-domain mapped type as unknown('mapped') (Record cannot express `as`)", () => {
    // `{ [P in string as N]: any }` — a key REMAPPING clause (`as`) FILTERS
    // (`as never` drops keys) or TRANSFORMS (`as `prefix_${P}`` renames) the
    // produced key domain, so the result is NOT expressible as a plain
    // `Record<string, any>`. The remap clause only survives on the graph mapped
    // node (`nameTypeNodeId`), so this is exercised through the decoded graph.
    // The conservative bridge bails to `unknown` rather than silently dropping
    // the remap and emitting an unsound `Record`.
    const keyRemapped = new DecodedTypeGraph(
      ["P"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 }, // node 1: `string` source
        { kind: NODE_PRIMITIVE, primitive: 6 }, // node 2: `any` value
        { kind: NODE_PRIMITIVE, primitive: 2 }, // node 3: a name-type (`as <…>` remap target)
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 1,
          valueNodeId: 2,
          optionalModifier: 1, // MappedModifier::None
          readonlyModifier: 1, // MappedModifier::None
          nameTypeNodeId: 3, // `as <name-type>` key remap PRESENT (non-zero)
        },
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(keyRemapped, 4)).kind).toBe("unknown");

    // Discriminator (no over-block): the SAME graph shape with NO remap
    // (`nameTypeNodeId: 0`) still recovers `Record<string, any>` — proving the
    // bail is remap-specific and the plain graph-backed Record recovery path
    // (the XP.5 slot target) is intact.
    const noRemap = new DecodedTypeGraph(
      ["P"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_PRIMITIVE, primitive: 6 },
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 1,
          valueNodeId: 2,
          optionalModifier: 1,
          readonlyModifier: 1,
          nameTypeNodeId: 0, // plain `{ [P in string]: any }` — no remap
        },
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(noRemap, 3))).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "any" },
      ],
    });
  });

  // ===========================================================================
  // Native (raw-JSON) open-domain mapped — modifier / remap soundness.
  //
  // The native `NativeTypeExpr` mapped form DOES carry the optional / readonly
  // modifier and the `as N` key-remap clause: the Rust producer emits
  // `optional` / `readonly` / `nameType` on `TypeExpr::Mapped`. A modifier- or
  // remap-bearing native mapped must therefore bail to `unknown` exactly like
  // its graph counterpart, NOT recover an unsound `Record` by treating the
  // modifier as absent. A genuinely-plain native mapped still recovers `Record`.
  // ===========================================================================

  it("keeps a `+optional` native open-domain mapped type as unknown('mapped') (native raw-JSON carries the optional modifier)", () => {
    // `+? [P in string]: any` — the native producer surfaces the ADDED optional
    // modifier in the raw-JSON `optional` field. `Record` cannot express an
    // added optional, so the recovery must bail rather than silently drop it
    // and emit an over-permissive `Record<string, any>`.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
      optional: "add",
    });
    expect(result.kind).toBe("unknown");
    expect(result).toEqual({ kind: "unknown", rawType: "mapped" });
    // Negative: the unsound `Record` recovery is gone for the modified form.
    expect(result.kind).not.toBe("ref");
  });

  it("keeps `-optional` / `+readonly` / `-readonly` native open-domain mapped types as unknown('mapped')", () => {
    const removeOptional = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
      optional: "remove",
    });
    expect(removeOptional.kind).toBe("unknown");

    const addReadonly = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
      readonly: "add",
    });
    expect(addReadonly.kind).toBe("unknown");

    const removeReadonly = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
      readonly: "remove",
    });
    expect(removeReadonly.kind).toBe("unknown");
  });

  it("keeps a key-remapped native open-domain mapped type as unknown('mapped') (native raw-JSON carries the `as N` nameType)", () => {
    // `{ [P in string as `prefix_${P}`]: any }` — the native producer surfaces
    // the `as N` key-remap clause in the raw-JSON `nameType` field. A remap
    // FILTERS / TRANSFORMS the produced key domain, so the surface is not a
    // plain `Record`; the recovery must bail on a non-null `nameType`.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
      nameType: {
        kind: "templateLiteral",
        quasis: ["prefix_", ""],
        expressions: [{ kind: "typeParameter", name: "P" }],
      },
    });
    expect(result.kind).toBe("unknown");
  });

  it("still recovers Record for a genuinely-plain native open-domain mapped with explicit no-op modifier/remap fields", () => {
    // The native form may carry the fields EXPLICITLY as the no-op encoding
    // (`optional: "none"`, `readonly: "none"`, `nameType: null`). That is a
    // genuinely-plain `{ [P in string]: any }` and STILL recovers `Record`.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "primitive", name: "any" },
      optional: "none",
      readonly: "none",
      nameType: null,
    });
    expect(result).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "any" },
      ],
    });
    expect(result.kind).not.toBe("unknown");
  });

  it("still recovers Record for a graph plain `{ [P in string]: any }` (the slot-binding production target is unaffected)", () => {
    // The GRAPH path observes all six mapped fields and gates them; a plain
    // graph mapped (no modifiers, no remap) — the XP.5 slot-binding production
    // target — STILL recovers `Record<string, any>`. The native-path soundness
    // fix does not touch the graph recovery.
    const plainGraph = new DecodedTypeGraph(
      ["P"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 }, // node 1: `string` source
        { kind: NODE_PRIMITIVE, primitive: 6 }, // node 2: `any` value
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 1,
          valueNodeId: 2,
          optionalModifier: 1, // MappedModifier::None
          readonlyModifier: 1, // MappedModifier::None
          nameTypeNodeId: 0, // no `as N` remap
        },
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(plainGraph, 3))).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "any" },
      ],
    });
  });

  it("keeps an open-domain mapped value of a nested indexed access (`T[P]`) as unknown('mapped')", () => {
    // `{ [P in string]: T[P] }` — the value references the mapped key `P`
    // NESTED in the index position of an indexed access. The conservative
    // bridge must detect the nested `P` (not just a top-level one) and keep
    // the `unknown` fallback instead of emitting `Record<string, T[P]>` (which
    // would falsely fix a per-key-varying value).
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: {
        kind: "indexedAccess",
        object: { kind: "ref", name: "T", typeArguments: [] },
        index: { kind: "typeParameter", name: "P" },
      },
    });
    expect(result.kind).toBe("unknown");
  });

  it("keeps an open-domain mapped value lowered from a conditional operator as unknown('mapped')", () => {
    // `{ [P in string]: P extends X ? A : B }` — the value lowers to
    // `unknown(...)` (conditional is an unsupported operator in the bridge) and
    // it references `P`. The type-parameter collector cannot see `P` through
    // the `unknown` residue, so the bridge must bail on ANY `unknown`-operator
    // residue rather than emitting `Record<string, unknown(...)>`.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: {
        kind: "conditional",
        check: { kind: "typeParameter", name: "P" },
        extends: { kind: "ref", name: "X", typeArguments: [] },
        trueType: { kind: "ref", name: "A", typeArguments: [] },
        falseType: { kind: "ref", name: "B", typeArguments: [] },
      },
    });
    expect(result.kind).toBe("unknown");
  });

  it("still recovers Record when the open-domain mapped value is a concrete ref independent of the key", () => {
    // `{ [P in string]: Foo }` — the value is a fully-understood ref that does
    // NOT reference `P` and carries no `unknown` residue. The tightened guards
    // must still produce `Record<string, Foo>` (the original XP.5 recovery
    // target is preserved, not over-blocked).
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: { kind: "ref", name: "Foo", typeArguments: [] },
    });
    expect(result).toEqual({
      kind: "ref",
      name: "Record",
      typeArguments: [
        { kind: "primitive", name: "string" },
        { kind: "ref", name: "Foo" },
      ],
    });
    expect(result.kind).not.toBe("unknown");
  });

  it("keeps an open-domain mapped value whose generic-function type-param DEFAULT lowers to unknown as unknown('mapped')", () => {
    // `{ [P in string]: <Q = P extends X ? A : B>() => Q }` — the mapped key
    // `P` is hidden inside the function type-param `Q`'s DEFAULT, which lowers
    // to `unknown(...)` (a conditional is an unsupported operator). The unknown
    // residue is nested under a `typeParameter` descriptor, so the value-side
    // unknown guard must recurse a type parameter's `constraint`/`default` (a
    // type parameter is NOT a leaf) instead of emitting `Record<string, fn>`.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: {
        kind: "function",
        parameters: [],
        returnType: { kind: "typeParameter", name: "Q" },
        typeParameters: [
          {
            name: "Q",
            default: {
              kind: "conditional",
              check: { kind: "typeParameter", name: "P" },
              extends: { kind: "ref", name: "X", typeArguments: [] },
              trueType: { kind: "ref", name: "A", typeArguments: [] },
              falseType: { kind: "ref", name: "B", typeArguments: [] },
            },
          },
        ],
      },
    });
    expect(result.kind).toBe("unknown");
  });

  it("keeps an open-domain mapped value whose generic-function type-param CONSTRAINT lowers to unknown as unknown('mapped')", () => {
    // `{ [P in string]: <Q extends (P extends X ? A : B)>() => Q }` — the same
    // hazard, with the `unknown(...)` residue hiding in the type-param
    // CONSTRAINT sub-position instead of the default.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: {
        kind: "function",
        parameters: [],
        returnType: { kind: "typeParameter", name: "Q" },
        typeParameters: [
          {
            name: "Q",
            constraint: {
              kind: "conditional",
              check: { kind: "typeParameter", name: "P" },
              extends: { kind: "ref", name: "X", typeArguments: [] },
              trueType: { kind: "ref", name: "A", typeArguments: [] },
              falseType: { kind: "ref", name: "B", typeArguments: [] },
            },
          },
        ],
      },
    });
    expect(result.kind).toBe("unknown");
  });

  it("keeps an open-domain mapped value whose generic-function type-param CONSTRAINT references the key `P` directly as unknown('mapped')", () => {
    // `{ [P in string]: <Q extends P>() => Q }` — the mapped key `P` appears
    // directly (as a `typeParameter`) inside `Q`'s constraint. The key-reference
    // collector must recurse a type parameter's `constraint`/`default` so the
    // nested `P` is collected and the recovery bails rather than emitting
    // `Record<string, fn>` over a per-key-varying value.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: {
        kind: "function",
        parameters: [],
        returnType: { kind: "typeParameter", name: "Q" },
        typeParameters: [{ name: "Q", constraint: { kind: "typeParameter", name: "P" } }],
      },
    });
    expect(result.kind).toBe("unknown");
  });

  it("still recovers Record when a generic-function mapped value's type-param constraint is key-independent and fully understood", () => {
    // `{ [P in string]: <Q extends string>() => Q }` — the type-param constraint
    // is key-INDEPENDENT (`string`, no `P`) and fully understood (no `unknown`
    // residue). The recursing guards must NOT over-block: the value is still a
    // valid `Record<string, V>` value, so the recovery is preserved.
    const result = typeExprToDescriptor({
      kind: "mapped",
      parameter: "P",
      source: { kind: "primitive", name: "string" },
      value: {
        kind: "function",
        parameters: [],
        returnType: { kind: "typeParameter", name: "Q" },
        typeParameters: [{ name: "Q", constraint: { kind: "primitive", name: "string" } }],
      },
    });
    expect(result.kind).toBe("ref");
    if (result.kind === "ref") {
      expect(result.name).toBe("Record");
      expect(result.typeArguments?.[0]).toEqual({ kind: "primitive", name: "string" });
      expect(result.typeArguments?.[1]?.kind).toBe("function");
    }
    expect(result.kind).not.toBe("unknown");
  });

  // =============================================================================
  // Parenthesized (unwrap)
  // =============================================================================

  it("unwraps parenthesized type", () => {
    const result = typeExprToDescriptor({
      kind: "parenthesized",
      inner: { kind: "primitive", name: "string" },
    } as NativeTypeExpr);
    expect(result).toEqual({ kind: "primitive", name: "string" });
  });

  // =============================================================================
  // Unknown passthrough
  // =============================================================================

  it("passes through unknown type", () => {
    const result = typeExprToDescriptor({ kind: "unknown", raw: "complex stuff" });
    expect(result).toEqual({ kind: "unknown", rawType: "complex stuff" });
  });
});

// =============================================================================
// buildEvaluatedTypeMap
// =============================================================================

describe("buildEvaluatedTypeMap", () => {
  it("returns empty map for undefined input", () => {
    const map = buildEvaluatedTypeMap(undefined);
    expect(map.size).toBe(0);
  });

  it("builds map from evaluated fields", () => {
    const fields: NativeEvaluatedField[] = [
      {
        name: "count",
        type: { kind: "primitive", name: "number" },
        exactness: "exactConcrete",
        executionStatus: "completed",
        diagnostics: [],
      },
      {
        name: "label",
        type: { kind: "primitive", name: "string" },
        exactness: "exactConcrete",
        executionStatus: "completed",
        diagnostics: [],
      },
    ];
    const map = buildEvaluatedTypeMap(fields);
    expect(map.size).toBe(2);
    expect(map.get("count")).toEqual({ kind: "primitive", name: "number" });
    expect(map.get("label")).toEqual({ kind: "primitive", name: "string" });
  });

  it("converts complex types in the map", () => {
    const fields: NativeEvaluatedField[] = [
      {
        name: "items",
        type: {
          kind: "array",
          element: { kind: "primitive", name: "string" },
          readonly: false,
        },
        exactness: "exactConcrete",
        executionStatus: "completed",
        diagnostics: [],
      },
    ];
    const map = buildEvaluatedTypeMap(fields);
    expect(map.get("items")).toEqual({
      kind: "array",
      element: { kind: "primitive", name: "string" },
    });
  });
});
