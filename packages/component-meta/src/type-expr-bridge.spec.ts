import { describe, it, expect } from "vitest";
import {
  typeExprToDescriptor,
  buildEvaluatedTypeMap,
  escapeTemplateQuasi,
} from "./type-expr-bridge.js";
import type { NativeTypeExpr, NativeEvaluatedField } from "./type-expr-bridge.js";
import {
  DecodedTypeGraph,
  createGraphTypeExprRef,
  LITERAL_STRING,
  LITERAL_NUMBER,
  MEMBER_PROPERTY,
  MEMBER_METHOD,
  NODE_ARRAY,
  NODE_CONDITIONAL,
  NODE_FUNCTION,
  NODE_INDEXED_ACCESS,
  NODE_KEY_OF,
  NODE_LITERAL,
  NODE_MAPPED,
  NODE_OBJECT,
  NODE_PRIMITIVE,
  NODE_REF,
  NODE_TUPLE,
  NODE_TYPE_PARAMETER,
  NODE_UNION,
} from "./type-graph-core.js";

// =============================================================================
// typeExprToDescriptor: primitives
// =============================================================================

/**
 * `infer` declares its binder only inside a conditional's `extends` clause;
 * the true branch references it by name.
 */
function inExtendsClause(infer: NativeTypeExpr): NativeTypeExpr {
  return {
    kind: "conditional",
    check: { kind: "ref", name: "T", typeArguments: [] },
    extends: infer,
    trueType: { kind: "infer", name: "U" },
    falseType: { kind: "primitive", name: "never" },
  };
}

describe("typeExprToDescriptor", () => {
  it.each(["native", "graph"])("keeps generic references symbolic in the %s arm", (arm) => {
    const graph = new DecodedTypeGraph(
      ["Box"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_REF, nameId: 1, typeArgumentNodeIds: [1] },
      ],
    );
    const expr =
      arm === "native"
        ? ({
            kind: "ref",
            name: "Box",
            typeArguments: [{ kind: "primitive", name: "string" }],
          } as NativeTypeExpr)
        : createGraphTypeExprRef(graph, 2);
    const registry = new Map([
      [
        "Box",
        {
          kind: "object",
          properties: [
            { memberKind: "property", name: "value", ty: { kind: "typeParameter", name: "T" } },
          ],
        },
      ],
    ]);
    expect(Reflect.apply(typeExprToDescriptor, undefined, [expr, registry])).toEqual({
      kind: "ref",
      name: "Box",
      typeArguments: [{ kind: "primitive", name: "string" }],
    });
  });

  it.each(["native", "graph"])("keeps constrained indexed access symbolic in the %s arm", (arm) => {
    const constraint: NativeTypeExpr = {
      kind: "object",
      properties: [
        { memberKind: "property", name: "a", ty: { kind: "primitive", name: "string" } },
      ],
    };
    const graph = new DecodedTypeGraph(
      ["T", "a"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_TYPE_PARAMETER, nameId: 1, constraintNodeId: 5, defaultNodeId: 0 },
        { kind: NODE_LITERAL, literalKind: LITERAL_STRING, stringId: 2 },
        { kind: NODE_INDEXED_ACCESS, objectNodeId: 2, indexNodeId: 3 },
        {
          kind: NODE_OBJECT,
          members: [
            {
              kind: MEMBER_PROPERTY,
              key: { kind: "string", nameId: 2 },
              typeNodeId: 1,
              optional: false,
              readonly: false,
              keyNameId: 0,
              keyTypeNodeId: 0,
              valueTypeNodeId: 0,
              functionNodeId: 0,
              methodKind: 0,
              hasImplementationBody: false,
            },
          ],
        },
      ],
    );
    const expr =
      arm === "native"
        ? ({
            kind: "indexedAccess",
            object: { kind: "typeParameter", name: "T", constraint },
            index: { kind: "literal", literalKind: "string", value: "a" },
          } as NativeTypeExpr)
        : createGraphTypeExprRef(graph, 4);
    expect(Reflect.apply(typeExprToDescriptor, undefined, [expr, new Map()])).toEqual({
      kind: "indexedAccess",
      objectType: {
        kind: "typeParameter",
        name: "T",
        constraint: {
          kind: "object",
          properties: [{ name: "a", type: { kind: "primitive", name: "string" }, optional: false }],
        },
      },
      indexType: { kind: "literal", value: "a" },
    });
  });

  it.each(["native", "graph"])("prints unreduced operators in the %s arm", (arm) => {
    const keyOf: NativeTypeExpr = {
      kind: "keyOf",
      operand: {
        kind: "object",
        properties: [
          { memberKind: "property", name: "a", ty: { kind: "primitive", name: "number" } },
        ],
      },
    };
    const mapped: NativeTypeExpr = {
      kind: "mapped",
      parameter: "K",
      source: {
        kind: "union",
        types: ["a", "b"].map((value) => ({ kind: "literal", literalKind: "string", value })),
      },
      value: { kind: "primitive", name: "number" },
    };
    const graph = new DecodedTypeGraph(
      ["K", "a", "b"],
      [
        { kind: NODE_PRIMITIVE, primitive: 2 },
        { kind: NODE_LITERAL, literalKind: LITERAL_STRING, stringId: 2 },
        { kind: NODE_LITERAL, literalKind: LITERAL_STRING, stringId: 3 },
        { kind: NODE_UNION, typeNodeIds: [2, 3] },
        {
          kind: NODE_MAPPED,
          parameterId: 1,
          sourceNodeId: 4,
          valueNodeId: 1,
          optionalModifier: 0,
          readonlyModifier: 0,
          nameTypeNodeId: 0,
        },
        { kind: NODE_KEY_OF, operandNodeId: 7 },
        {
          kind: NODE_OBJECT,
          members: [
            {
              kind: MEMBER_PROPERTY,
              key: { kind: "string", nameId: 2 },
              typeNodeId: 1,
              optional: false,
              readonly: false,
              keyNameId: 0,
              keyTypeNodeId: 0,
              valueTypeNodeId: 0,
              functionNodeId: 0,
              methodKind: 0,
              hasImplementationBody: false,
            },
          ],
        },
      ],
    );
    const expressions =
      arm === "native"
        ? [keyOf, mapped]
        : [createGraphTypeExprRef(graph, 6), createGraphTypeExprRef(graph, 5)];
    expect(typeExprToDescriptor(expressions[0])).toEqual({
      kind: "unknown",
      rawType: "keyof { a: number }",
    });
    expect(typeExprToDescriptor(expressions[1])).toEqual({
      kind: "unknown",
      rawType: '{ [K in "a" | "b"]: number }',
    });
  });
  it("keeps a nested infer declaration that shadows an enclosing binder", () => {
    const never: NativeTypeExpr = { kind: "primitive", name: "never" };
    const result = typeExprToDescriptor({
      kind: "conditional",
      check: { kind: "ref", name: "T", typeArguments: [] },
      extends: { kind: "infer", name: "U" },
      trueType: {
        kind: "conditional",
        check: { kind: "primitive", name: "number" },
        extends: { kind: "infer", name: "U" },
        trueType: { kind: "ref", name: "U", typeArguments: [] },
        falseType: never,
      },
      falseType: never,
    });
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends infer U ? number extends infer U ? U : never : never",
    });
  });

  it("renders an array constraint on an unreduced infer", () => {
    const result = typeExprToDescriptor(
      inExtendsClause({
        kind: "infer",
        name: "U",
        constraint: {
          kind: "array",
          readonly: false,
          element: { kind: "primitive", name: "string" },
        },
      }),
    );
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends (infer U extends string[]) ? U : never",
    });
  });

  it("renders a graph constraint on an unreduced infer", () => {
    const graph = new DecodedTypeGraph([], [{ kind: NODE_PRIMITIVE, primitive: 1 }]);
    const result = typeExprToDescriptor(
      inExtendsClause({
        kind: "infer",
        name: "U",
        constraint: createGraphTypeExprRef(graph, 1),
      }),
    );
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends (infer U extends string) ? U : never",
    });
  });

  it("renders a function constraint on an unreduced infer", () => {
    const result = typeExprToDescriptor(
      inExtendsClause({
        kind: "infer",
        name: "U",
        constraint: {
          kind: "function",
          parameters: [],
          returnType: { kind: "primitive", name: "string" },
        },
      }),
    );
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends (infer U extends (() => string)) ? U : never",
    });
  });

  it("parenthesizes a union array constraint on an unreduced infer", () => {
    const result = typeExprToDescriptor(
      inExtendsClause({
        kind: "infer",
        name: "U",
        constraint: {
          kind: "array",
          readonly: false,
          element: {
            kind: "union",
            types: [
              { kind: "primitive", name: "string" },
              { kind: "primitive", name: "number" },
            ],
          },
        },
      }),
    );
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends (infer U extends (string | number)[]) ? U : never",
    });
  });

  it("renders a graph function constraint on an unreduced infer", () => {
    const graph = new DecodedTypeGraph(
      [],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        {
          kind: NODE_FUNCTION,
          parameters: [],
          returnTypeNodeId: 1,
          typeParameterNodeIds: [],
        },
      ],
    );
    const result = typeExprToDescriptor(
      inExtendsClause({
        kind: "infer",
        name: "U",
        constraint: createGraphTypeExprRef(graph, 2),
      }),
    );
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends (infer U extends (() => string)) ? U : never",
    });
  });

  it("parenthesizes a graph union array constraint on an unreduced infer", () => {
    const graph = new DecodedTypeGraph(
      [],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_PRIMITIVE, primitive: 2 },
        { kind: NODE_UNION, typeNodeIds: [1, 2] },
        { kind: NODE_ARRAY, elementNodeId: 3, readonly: false },
      ],
    );
    const result = typeExprToDescriptor(
      inExtendsClause({
        kind: "infer",
        name: "U",
        constraint: createGraphTypeExprRef(graph, 4),
      }),
    );
    expect(result).toEqual({
      kind: "unknown",
      rawType: "T extends (infer U extends (string | number)[]) ? U : never",
    });
  });

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

  it("preserves empty-object arms in mixed intersections for string brands", () => {
    expect(
      typeExprToDescriptor({
        kind: "intersection",
        types: [
          { kind: "primitive", name: "string" },
          { kind: "object", properties: [] },
        ],
      }),
    ).toEqual({
      kind: "intersection",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "object", properties: [] },
      ],
    });
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

  it.each(["native JSON", "graph"])("keeps tuple modifiers from %s", (arm) => {
    const graph = new DecodedTypeGraph(
      ["id", "name", "rest"],
      [
        { kind: NODE_PRIMITIVE, primitive: 2 },
        { kind: NODE_PRIMITIVE, primitive: 1 },
        { kind: NODE_PRIMITIVE, primitive: 3 },
        { kind: NODE_ARRAY, elementNodeId: 3, readonly: false },
        {
          kind: NODE_TUPLE,
          readonly: true,
          elements: [
            { labelId: 1, typeNodeId: 1, optional: false, rest: false },
            { labelId: 2, typeNodeId: 2, optional: true, rest: false },
            { labelId: 3, typeNodeId: 4, optional: false, rest: true },
          ],
        },
      ],
    );
    const native: NativeTypeExpr = {
      kind: "tuple",
      readonly: true,
      elements: [
        { label: "id", ty: { kind: "primitive", name: "number" }, optional: false, rest: false },
        { label: "name", ty: { kind: "primitive", name: "string" }, optional: true, rest: false },
        {
          label: "rest",
          ty: { kind: "array", element: { kind: "primitive", name: "boolean" }, readonly: false },
          optional: false,
          rest: true,
        },
      ],
    };
    expect(
      typeExprToDescriptor(arm === "graph" ? createGraphTypeExprRef(graph, 5) : native),
    ).toEqual({
      kind: "tuple",
      elements: [
        { kind: "primitive", name: "number" },
        { kind: "primitive", name: "string" },
        { kind: "array", element: { kind: "primitive", name: "boolean" } },
      ],
      modifiers: [
        { label: "id" },
        { label: "name", optional: true },
        { label: "rest", rest: true },
      ],
      readonly: true,
    });
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

  it("names, indexes and enumerates members by their typed key", () => {
    const keyed: NativeTypeExpr = {
      kind: "object",
      properties: [
        {
          memberKind: "property",
          key: { kind: "string", value: "color" },
          ty: { kind: "literal", literalKind: "string", value: "red" },
          optional: false,
        },
        {
          memberKind: "property",
          key: { kind: "number", value: 7 },
          ty: { kind: "primitive", name: "number" },
          optional: true,
        },
        {
          memberKind: "method",
          key: { kind: "string", value: "go" },
          function: { parameters: [], returnType: { kind: "primitive", name: "void" } },
        },
      ],
    };

    const obj = typeExprToDescriptor(keyed) as {
      kind: "object";
      properties: { name: string; optional: boolean }[];
    };
    expect(obj.properties.map((p) => [p.name, p.optional])).toEqual([
      ["color", false],
      ["7", true],
      ["go", false],
    ]);
  });

  it("transcribes number-keyed graph members without evaluating indexed access", () => {
    const graph = new DecodedTypeGraph(
      ["K", "go"],
      [
        { kind: NODE_PRIMITIVE, primitive: 1 }, // 1: string
        {
          kind: NODE_OBJECT,
          members: [
            {
              kind: MEMBER_PROPERTY,
              key: { kind: "number", value: 7 },
              typeNodeId: 1,
              optional: false,
              readonly: false,
              keyNameId: 0,
              keyTypeNodeId: 0,
              valueTypeNodeId: 0,
              functionNodeId: 0,
              methodKind: 0,
              hasImplementationBody: false,
            },
            {
              kind: MEMBER_METHOD,
              key: { kind: "string", nameId: 2 },
              typeNodeId: 0,
              optional: false,
              readonly: false,
              keyNameId: 0,
              keyTypeNodeId: 0,
              valueTypeNodeId: 0,
              functionNodeId: 6,
              methodKind: 0,
              hasImplementationBody: false,
            },
          ],
        }, // 2: { 7: string; go(): string }
        { kind: NODE_LITERAL, literalKind: LITERAL_NUMBER, numberValue: 7 }, // 3: 7
        { kind: NODE_INDEXED_ACCESS, objectNodeId: 2, indexNodeId: 3 }, // 4: { 7: string; go(): string }[7]
        { kind: NODE_KEY_OF, operandNodeId: 2 }, // 5: keyof { 7: string; go(): string }
        {
          kind: NODE_FUNCTION,
          parameters: [],
          returnTypeNodeId: 1,
          typeParameterNodeIds: [],
        }, // 6: () => string
        { kind: NODE_LITERAL, literalKind: LITERAL_STRING, stringId: 2 }, // 7: "go"
        { kind: NODE_INDEXED_ACCESS, objectNodeId: 2, indexNodeId: 7 }, // 8: { 7: string; go(): string }["go"]
      ],
    );
    expect(typeExprToDescriptor(createGraphTypeExprRef(graph, 4))).toMatchObject({
      kind: "indexedAccess",
      indexType: { kind: "literal", value: 7 },
      objectType: {
        kind: "object",
        properties: [
          { name: "7", type: { kind: "primitive", name: "string" }, optional: false },
          {
            name: "go",
            type: { kind: "function", returnType: { kind: "primitive", name: "string" } },
          },
        ],
      },
    });
    expect(typeExprToDescriptor(createGraphTypeExprRef(graph, 5))).toEqual({
      kind: "unknown",
      rawType: "keyof { 7: string; go(): string }",
    });
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

  it("preserves indexed-access operands without a registry", () => {
    expect(
      typeExprToDescriptor({
        kind: "indexedAccess",
        object: { kind: "ref", name: "Shape", typeArguments: [] },
        index: { kind: "literal", literalKind: "string", value: "a" },
      }),
    ).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Shape" },
      indexType: { kind: "literal", value: "a" },
    });
  });

  it.each(["none", "add", "remove"] as const)("keeps mapped modifiers symbolic: %s", (modifier) => {
    expect(
      typeExprToDescriptor({
        kind: "mapped",
        parameter: "K",
        source: { kind: "primitive", name: "string" },
        value: { kind: "primitive", name: "number" },
        optional: modifier,
        readonly: modifier,
      }).kind,
    ).toBe("unknown");
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

// =============================================================================
// Graph operator display text
// =============================================================================

describe("operator-shaped graph nodes render structurally in display text", () => {
  /**
   * `Theme["color"] extends Dark ? A : B` as a graph: node 3 is the indexed
   * access, node 7 the conditional that consumes it.
   */
  function operatorGraph(): DecodedTypeGraph {
    return new DecodedTypeGraph(
      ["Theme", "color", "Dark", "A", "B"],
      [
        { kind: NODE_REF, nameId: 1, typeArgumentNodeIds: [] }, // 1: Theme
        { kind: NODE_LITERAL, literalKind: LITERAL_STRING, stringId: 2 }, // 2: "color"
        { kind: NODE_INDEXED_ACCESS, objectNodeId: 1, indexNodeId: 2 }, // 3: Theme["color"]
        { kind: NODE_REF, nameId: 3, typeArgumentNodeIds: [] }, // 4: Dark
        { kind: NODE_REF, nameId: 4, typeArgumentNodeIds: [] }, // 5: A
        { kind: NODE_REF, nameId: 5, typeArgumentNodeIds: [] }, // 6: B
        {
          kind: NODE_CONDITIONAL,
          checkNodeId: 3,
          extendsNodeId: 4,
          trueTypeNodeId: 5,
          falseTypeNodeId: 6,
        }, // 7
        { kind: NODE_KEY_OF, operandNodeId: 3 }, // 8: keyof Theme["color"]
      ],
    );
  }

  // A conditional has no structural descriptor variant, so it lands on the
  // `unknown` residue. `rawType` is what compat renders as the display string,
  // so an opaque node-discriminator placeholder there (`graphNode(14)`) reaches
  // user-visible output; the operands must render as authored type syntax.
  it("renders a conditional and its nested indexed access as type syntax", () => {
    const result = typeExprToDescriptor(createGraphTypeExprRef(operatorGraph(), 7));
    expect(result).toEqual({
      kind: "unknown",
      rawType: 'Theme["color"] extends Dark ? A : B',
    });
  });

  it("renders an unresolvable keyof over an indexed access as type syntax", () => {
    const result = typeExprToDescriptor(createGraphTypeExprRef(operatorGraph(), 8));
    expect(result).toEqual({ kind: "unknown", rawType: 'keyof Theme["color"]' });
  });
});

describe("escapeTemplateQuasi", () => {
  // A quasi is cooked text; tsc 7.1.0-beta prints `x${string}\\`, `a\t${string}`
  // and a line feed as it is.
  it("escapes cooked template text as the checker prints it", () => {
    expect(escapeTemplateQuasi("a`b${c\\d\te\nf\r\ng\b\u0001")).toBe(
      "a\\`b\\${c\\\\d\\te\nf\\r\\ng\\b\\u0001",
    );
    expect(escapeTemplateQuasi("\0")).toBe("\\0");
    expect(escapeTemplateQuasi("\u00001 $")).toBe("\\x001\\u2028$");
  });
});
