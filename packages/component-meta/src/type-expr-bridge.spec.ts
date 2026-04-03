import { describe, it, expect } from "vitest";
import { typeExprToDescriptor, buildEvaluatedTypeMap } from "./type-expr-bridge.js";
import type { NativeTypeExpr, NativeEvaluatedField } from "./type-expr-bridge.js";

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

    expect(result.kind).toBe("unknown");
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
