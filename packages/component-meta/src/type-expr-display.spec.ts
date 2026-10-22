import ts from "typescript";
import { describe, expect, it } from "vitest";

import { typeExprDisplayText, typeExprToDescriptor } from "./type-expr-bridge.js";
import type { NativeTypeExpr } from "./type-expr-bridge.js";
import {
  DecodedTypeGraph,
  type GraphPropertyKeyRecord,
  LITERAL_BIG_INT,
  LITERAL_STRING,
  MEMBER_PROPERTY,
  NODE_ARRAY,
  NODE_CONDITIONAL,
  NODE_FUNCTION,
  NODE_INFER,
  NODE_INTERSECTION,
  NODE_KEY_OF,
  NODE_LITERAL,
  NODE_OBJECT,
  NODE_PRIMITIVE,
  NODE_REF,
  NODE_TUPLE,
  NODE_UNION,
  createGraphTypeExprRef,
} from "./type-graph-core.js";

/**
 * The structure TypeScript parses `annotation` to: node kinds with their
 * names and literal values, parentheses dropped, so two spellings of one
 * tree read the same and a regrouped operator reads differently.
 */
function parsedStructure(annotation: string): string {
  const source = `type __T = ${annotation};`;
  const { diagnostics } = ts.transpileModule(source, { reportDiagnostics: true });
  expect(
    diagnostics?.map((diagnostic) => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")),
    `\`${annotation}\` parses`,
  ).toEqual([]);
  const file = ts.createSourceFile("display.ts", source, ts.ScriptTarget.Latest);
  const alias = file.statements[0] as ts.TypeAliasDeclaration;
  const out: string[] = [];
  const visit = (node: ts.Node): void => {
    if (ts.isParenthesizedTypeNode(node)) {
      visit(node.type);
      return;
    }
    out.push(ts.SyntaxKind[node.kind]);
    if (ts.isIdentifier(node) || ts.isStringLiteral(node) || ts.isNumericLiteral(node)) {
      out.push(JSON.stringify(node.text));
    } else if (ts.isBigIntLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      out.push(JSON.stringify(node.text));
    } else if (ts.isPrefixUnaryExpression(node) || ts.isTypeOperatorNode(node)) {
      out.push(ts.SyntaxKind[node.operator]);
    } else if (
      (ts.isTypeParameterDeclaration(node) || ts.isPropertySignature(node)) &&
      node.modifiers
    ) {
      out.push(...node.modifiers.map((modifier) => ts.SyntaxKind[modifier.kind]));
    }
    out.push("(");
    ts.forEachChild(node, visit);
    out.push(")");
  };
  visit(alias.type);
  return out.join(" ");
}

/** `printed` reparses to the tree `intended` spells. */
function expectReparsesAs(printed: string, intended: string): void {
  expect(parsedStructure(printed), `\`${printed}\` reparses as \`${intended}\``).toBe(
    parsedStructure(intended),
  );
}

const ref = (name: string, typeArguments: NativeTypeExpr[] = []): NativeTypeExpr => ({
  kind: "ref",
  name,
  typeArguments,
});
const union = (...types: NativeTypeExpr[]): NativeTypeExpr => ({ kind: "union", types });
const fn = (returnType: NativeTypeExpr): NativeTypeExpr => ({
  kind: "function",
  parameters: [],
  returnType,
});
const lit = (value: string): NativeTypeExpr => ({ kind: "literal", literalKind: "string", value });
const infer = (name: string): NativeTypeExpr => ({ kind: "infer", name });
const conditional = (
  check: NativeTypeExpr,
  extendsType: NativeTypeExpr,
  trueType: NativeTypeExpr,
  falseType: NativeTypeExpr,
): NativeTypeExpr => ({ kind: "conditional", check, extends: extendsType, trueType, falseType });
const property = (key: NativeObjectKey, ty: NativeTypeExpr): NativeTypeExpr => ({
  kind: "object",
  properties: [{ memberKind: "property", key, ty, optional: false, readonly: false }],
});
type NativeObjectKey = { kind: "string"; value: string } | { kind: "number"; value: number };

describe("native display text reparses to the printed tree", () => {
  const cases: Array<[string, NativeTypeExpr, string]> = [
    [
      "a union inside an intersection",
      { kind: "intersection", types: [union(ref("A"), ref("B")), ref("C")] },
      "(A | B) & C",
    ],
    [
      "a function arm of a union",
      union(ref("A"), fn({ kind: "primitive", name: "void" })),
      "A | (() => void)",
    ],
    ["keyof over a union", { kind: "keyOf", operand: union(ref("A"), ref("B")) }, "keyof (A | B)"],
    [
      "an access through keyof",
      { kind: "indexedAccess", object: { kind: "keyOf", operand: ref("T") }, index: ref("K") },
      "(keyof T)[K]",
    ],
    [
      "an access by a string literal",
      { kind: "indexedAccess", object: ref("Theme"), index: lit('co"lor') },
      'Theme["co\\"lor"]',
    ],
    [
      "a function checked by a conditional",
      {
        kind: "conditional",
        check: fn({ kind: "primitive", name: "void" }),
        extends: ref("T"),
        trueType: { kind: "literal", literalKind: "number", value: 1 },
        falseType: { kind: "literal", literalKind: "number", value: 2 },
      },
      "(() => void) extends T ? 1 : 2",
    ],
    [
      "a conditional as the extends clause",
      {
        kind: "conditional",
        check: ref("T"),
        extends: {
          kind: "conditional",
          check: ref("U"),
          extends: ref("V"),
          trueType: ref("X"),
          falseType: ref("Y"),
        },
        trueType: ref("A"),
        falseType: ref("B"),
      },
      "T extends (U extends V ? X : Y) ? A : B",
    ],
    [
      "optional and rest tuple elements",
      {
        kind: "tuple",
        readonly: false,
        elements: [
          { label: null, ty: union(ref("A"), ref("B")), optional: true, rest: false },
          { label: "named", ty: union(ref("C"), ref("D")), optional: true, rest: false },
          {
            label: null,
            ty: { kind: "array", element: ref("E"), readonly: false },
            optional: false,
            rest: true,
          },
        ],
      },
      "[(A | B)?, named?: C | D, ...E[]]",
    ],
    [
      "arrays of operator forms",
      union(
        { kind: "array", element: { kind: "keyOf", operand: ref("T") }, readonly: false },
        { kind: "array", element: union(ref("A"), ref("B")), readonly: true },
      ),
      "(keyof T)[] | readonly (A | B)[]",
    ],
    [
      "bigint literals",
      union(
        { kind: "literal", literalKind: "bigInt", value: "10" },
        { kind: "literal", literalKind: "bigInt", value: "-5" },
      ),
      "10n | -5n",
    ],
    [
      "alias, keyof and typeof identity",
      ref("Alias", [
        { kind: "keyOf", operand: ref("Model") },
        { kind: "typeOf", path: ["value"] },
      ]),
      "Alias<keyof Model, typeof value>",
    ],
    [
      "a const type parameter, an unnamed parameter and a predicate",
      {
        kind: "function",
        typeParameters: [
          {
            name: "T",
            isConst: true,
            constraint: {
              kind: "array",
              element: { kind: "primitive", name: "unknown" },
              readonly: true,
            },
          },
        ],
        parameters: [
          { name: "x", ty: { kind: "primitive", name: "unknown" }, optional: false, rest: false },
          { name: null, ty: ref("T"), optional: false, rest: false },
        ],
        returnType: { kind: "primitive", name: "boolean" },
        predicate: { asserts: false, parameter: "x", ty: ref("T") },
      },
      "<const T extends readonly unknown[]>(x: unknown, _arg1: T) => x is T",
    ],
    [
      "object members keyed as authored",
      {
        kind: "object",
        properties: [
          {
            memberKind: "property",
            key: { kind: "string", value: "0123" },
            ty: ref("A"),
            optional: false,
            readonly: false,
          },
          {
            memberKind: "property",
            key: { kind: "number", value: 7 },
            ty: ref("B"),
            optional: false,
            readonly: false,
          },
          {
            memberKind: "property",
            key: { kind: "string", value: "c" },
            ty: union(ref("C"), fn(ref("D"))),
            optional: true,
            readonly: true,
          },
          {
            memberKind: "method",
            key: { kind: "string", value: "m" },
            optional: true,
            function: {
              parameters: [{ name: "x", ty: ref("E"), optional: false, rest: false }],
              returnType: ref("F"),
            },
          },
          {
            memberKind: "indexSignature",
            keyName: "key",
            keyType: { kind: "primitive", name: "string" },
            valueType: ref("G"),
          },
        ],
      },
      '{ "0123": A; 7: B; readonly c?: C | (() => D); m?(x: E): F; [key: string]: G }',
    ],
    [
      // `keyof` is `"7"` for the string key and `7` for the number key.
      "a digit-spelled string key apart from its number key",
      union(
        property({ kind: "string", value: "7" }, ref("A")),
        property({ kind: "number", value: 7 }, ref("B")),
      ),
      '{ "7": A } | { 7: B }',
    ],
    [
      "an infer binder referenced in the true branch",
      conditional(
        ref("T"),
        { kind: "array", element: infer("U"), readonly: false },
        { kind: "array", element: infer("U"), readonly: false },
        { kind: "primitive", name: "never" },
      ),
      "T extends (infer U)[] ? U[] : never",
    ],
    [
      "an infer binder referenced by a nested clause and redeclared outside its scope",
      conditional(
        ref("T"),
        property({ kind: "string", value: "a" }, infer("U")),
        conditional(
          ref("T"),
          property({ kind: "string", value: "b" }, ref("U")),
          ref("U"),
          ref("X"),
        ),
        conditional(
          ref("T"),
          property({ kind: "string", value: "c" }, infer("U")),
          ref("U"),
          ref("Y"),
        ),
      ),
      "T extends { a: infer U } ? T extends { b: U } ? U : X : T extends { c: infer U } ? U : Y",
    ],
    [
      // Printed as a reference, `T = string` would select `never`, not `number`.
      "a nested clause redeclaring an enclosing infer binder",
      conditional(
        ref("T"),
        infer("U"),
        conditional({ kind: "primitive", name: "number" }, infer("U"), ref("U"), {
          kind: "primitive",
          name: "never",
        }),
        { kind: "primitive", name: "never" },
      ),
      "T extends infer U ? number extends infer U ? U : never : never",
    ],
    [
      "a remapped mapped type with modifiers",
      {
        kind: "mapped",
        parameter: "K",
        source: { kind: "keyOf", operand: ref("T") },
        value: { kind: "indexedAccess", object: ref("T"), index: ref("K") },
        optional: "remove",
        readonly: "add",
        nameType: {
          kind: "templateLiteral",
          quasis: ["get", ""],
          expressions: [
            { kind: "intersection", types: [ref("K"), { kind: "primitive", name: "string" }] },
          ],
        },
      },
      "{ readonly [K in keyof T as `get${K & string}`]-?: T[K] }",
    ],
  ];

  for (const [name, expr, intended] of cases) {
    it(name, () => {
      const printed = typeExprDisplayText(expr);
      expectReparsesAs(printed, intended);
      expect(printed).toBe(intended);
    });
  }
});

describe("graph display text reparses to the printed tree", () => {
  // 1: A, 2: B, 3: C, 4: A | B, 5: (A | B) & C, 6: keyof (A | B),
  // 7: [(A | B)?], 8: () => void, 9: C | (() => void), 10: void,
  // 11: { "0123": "x" }, 12: "x", 13: 7n, 14: (keyof (A | B))[],
  // 15: { "7": A }, 16: { 7: B }, 17: T, 18: infer U, 19: U[] (a bound U),
  // 20: T extends (infer U)[] ? U[] : A, 21: { a: infer U },
  // 22: T extends { a: infer U } ? U : A,
  // 23: T extends { a: infer U } ? T extends { a: infer U } ? U : A : A,
  // 24: { a: 25 }, 25: 24 | void (a cycle), 26: U (a reference), 27: { a: U },
  // 28: T extends { a: U } ? U : A,
  // 29: T extends { a: infer U } ? T extends { a: U } ? U : A : A
  const property = (key: GraphPropertyKeyRecord, typeNodeId: number) => ({
    kind: MEMBER_PROPERTY,
    key,
    typeNodeId,
    optional: false,
    readonly: false,
    keyNameId: 0,
    keyTypeNodeId: 0,
    valueTypeNodeId: 0,
    functionNodeId: 0,
    methodKind: 0,
    hasImplementationBody: false,
  });
  const graph = new DecodedTypeGraph(
    ["A", "B", "C", "0123", "x", "7", "T", "U", "a"],
    [
      { kind: NODE_REF, nameId: 1, typeArgumentNodeIds: [] },
      { kind: NODE_REF, nameId: 2, typeArgumentNodeIds: [] },
      { kind: NODE_REF, nameId: 3, typeArgumentNodeIds: [] },
      { kind: NODE_UNION, typeNodeIds: [1, 2] },
      { kind: NODE_INTERSECTION, typeNodeIds: [4, 3] },
      { kind: NODE_KEY_OF, operandNodeId: 4 },
      {
        kind: NODE_TUPLE,
        readonly: false,
        elements: [{ labelId: 0, typeNodeId: 4, optional: true, rest: false }],
      },
      { kind: NODE_FUNCTION, parameters: [], returnTypeNodeId: 10, typeParameterNodeIds: [] },
      { kind: NODE_UNION, typeNodeIds: [3, 8] },
      { kind: NODE_PRIMITIVE, primitive: 8 },
      {
        kind: NODE_OBJECT,
        members: [
          {
            kind: MEMBER_PROPERTY,
            key: { kind: "string", nameId: 4 },
            typeNodeId: 12,
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
      { kind: NODE_LITERAL, literalKind: LITERAL_STRING, stringId: 5 },
      { kind: NODE_LITERAL, literalKind: LITERAL_BIG_INT, stringId: 6 },
      { kind: NODE_ARRAY, elementNodeId: 6, readonly: false },
      { kind: NODE_OBJECT, members: [property({ kind: "string", nameId: 6 }, 1)] },
      { kind: NODE_OBJECT, members: [property({ kind: "number", value: 7 }, 2)] },
      { kind: NODE_REF, nameId: 7, typeArgumentNodeIds: [] },
      { kind: NODE_INFER, nameId: 8 },
      { kind: NODE_ARRAY, elementNodeId: 18, readonly: false },
      {
        kind: NODE_CONDITIONAL,
        checkNodeId: 17,
        extendsNodeId: 19,
        trueTypeNodeId: 19,
        falseTypeNodeId: 1,
      },
      { kind: NODE_OBJECT, members: [property({ kind: "string", nameId: 9 }, 18)] },
      {
        kind: NODE_CONDITIONAL,
        checkNodeId: 17,
        extendsNodeId: 21,
        trueTypeNodeId: 18,
        falseTypeNodeId: 1,
      },
      {
        kind: NODE_CONDITIONAL,
        checkNodeId: 17,
        extendsNodeId: 21,
        trueTypeNodeId: 22,
        falseTypeNodeId: 1,
      },
      { kind: NODE_OBJECT, members: [property({ kind: "string", nameId: 9 }, 25)] },
      { kind: NODE_UNION, typeNodeIds: [24, 10] },
      { kind: NODE_REF, nameId: 8, typeArgumentNodeIds: [] },
      { kind: NODE_OBJECT, members: [property({ kind: "string", nameId: 9 }, 26)] },
      {
        kind: NODE_CONDITIONAL,
        checkNodeId: 17,
        extendsNodeId: 27,
        trueTypeNodeId: 26,
        falseTypeNodeId: 1,
      },
      {
        kind: NODE_CONDITIONAL,
        checkNodeId: 17,
        extendsNodeId: 21,
        trueTypeNodeId: 28,
        falseTypeNodeId: 1,
      },
    ],
  );

  for (const [nodeId, intended] of [
    [5, "(A | B) & C"],
    [6, "keyof (A | B)"],
    [7, "[(A | B)?]"],
    [9, "C | (() => void)"],
    [11, '{ "0123": "x" }'],
    [13, "7n"],
    [14, "(keyof (A | B))[]"],
    [15, '{ "7": A }'],
    [16, "{ 7: B }"],
    [20, "T extends (infer U)[] ? U[] : A"],
    // A nested clause redeclaring the enclosing binder keeps its declaration.
    [23, "T extends { a: infer U } ? T extends { a: infer U } ? U : A : A"],
    [29, "T extends { a: infer U } ? T extends { a: U } ? U : A : A"],
    // A cyclic table has no finite spelling; its back-edge prints `unknown`.
    [24, "{ a: unknown | void }"],
  ] as const) {
    it(intended, () => {
      const printed = typeExprDisplayText(createGraphTypeExprRef(graph, nodeId));
      expectReparsesAs(printed, intended);
      expect(printed).toBe(intended);
    });
  }

  it("converts a cyclic node table to a descriptor without overflowing", () => {
    expect(() => typeExprToDescriptor(createGraphTypeExprRef(graph, 24))).not.toThrow();
  });
});
