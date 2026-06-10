import { describe, expect, it } from "vitest";

import { create, toBinary, fromBinary } from "@bufbuild/protobuf";

import {
  EvaluateTypeExpressionGraphRequestSchema,
  ExpandGraphAroundRequestSchema,
  ContextualTypeRequestSchema,
  FlowNarrowingRequestSchema,
  FrameworkSurfaceKindEntrySchema,
  FrameworkSurfaceKindSupport,
  FrameworkSurfacePayloadSchema,
  FrameworkSurfaceRequestSchema,
  GraphDiagnosticSeverity,
  GraphPrimitiveKind,
  GraphProjectionMode,
  GraphReductionDemand,
  GraphSignatureKind,
  GraphTypeNodeSchema,
  GraphMemberNameKind,
  GraphIndexKeyKind,
  GraphAccessibility,
  GraphVariance,
  GraphOperation,
  GraphMappedModifier,
  GraphSymbolNamespace,
  GraphDeclarationPartKind,
  GraphRelationOutcome,
  GraphRelationStepKind,
  GraphExactness,
  GraphOriginEdgeKind,
  FrameworkTag,
  FrameworkSurfaceKind,
  ProjectPathGraphRequestSchema,
  ResolveSymbolGraphRequestSchema,
  SemanticTypeGraphSchema,
  StructuredTypeExpressionSchema,
  TYPEINFO_GRAPH_SCHEMA_VERSION,
  TypeInfoCapabilityHandshakeRequestSchema,
  TypeInfoCapabilityHandshakeResponseSchema,
  TypeInfoGraphRequestSchema,
  TypeInfoGraphResponseSchema,
  TypeInfoRequestErrorSchema,
} from "./typeinfo.js";

function primStringExpr() {
  return create(StructuredTypeExpressionSchema, {
    kind: {
      case: "primitive",
      value: { kind: GraphPrimitiveKind.STRING },
    },
  });
}

describe("typeinfo proto TS bindings", () => {
  it("StructuredTypeExpression roundtrips every oneof variant", () => {
    type Case = Parameters<typeof create<typeof StructuredTypeExpressionSchema>>[1] & {
      kind: { case: NonNullable<unknown> };
    };

    const cases: Case[] = [
      { kind: { case: "reference", value: { scopeCanonical: "/a.ts", name: "Foo" } } },
      { kind: { case: "union", value: { members: [] } } },
      { kind: { case: "intersection", value: { members: [] } } },
      {
        kind: {
          case: "indexedAccess",
          value: { object: primStringExpr(), index: primStringExpr() },
        },
      },
      { kind: { case: "keyof", value: { operand: primStringExpr() } } },
      { kind: { case: "typeofExpr", value: { valueRootCanonical: "/a.ts", path: ["x"] } } },
      {
        kind: {
          case: "tuple",
          value: {
            elements: [{ value: primStringExpr(), optionalElement: false, rest: false }],
            readonly: true,
          },
        },
      },
      { kind: { case: "array", value: { element: primStringExpr(), readonly: false } } },
      {
        kind: {
          case: "objectLiteral",
          value: {
            members: [
              {
                name: "x",
                nameKind: GraphMemberNameKind.IDENTIFIER,
                value: primStringExpr(),
                optionalMember: false,
                readonly: false,
              },
            ],
          },
        },
      },
      {
        kind: {
          case: "mapped",
          value: {
            typeParam: { binderId: "T", name: "T", constraint: primStringExpr() },
            valueType: primStringExpr(),
            readonlyModifier: GraphMappedModifier.NONE,
            optionalModifier: GraphMappedModifier.NONE,
          },
        },
      },
      {
        kind: {
          case: "conditional",
          value: {
            check: primStringExpr(),
            extendsType: primStringExpr(),
            trueBranch: primStringExpr(),
            falseBranch: primStringExpr(),
          },
        },
      },
      {
        kind: {
          case: "literal",
          value: { value: { kind: { case: "booleanValue", value: true } } },
        },
      },
      { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.NUMBER } } },
      {
        kind: {
          case: "templateLiteral",
          value: { quasis: ["a"], expressions: [primStringExpr()] },
        },
      },
      { kind: { case: "inferExpr", value: { name: "U" } } },
      {
        kind: {
          case: "functionExpr",
          value: {
            parameters: [],
            typeParameters: [],
            returnExpr: { kind: { case: "type", value: primStringExpr() } },
            signatureKind: GraphSignatureKind.CALL,
          },
        },
      },
      {
        kind: {
          case: "classExpr",
          value: { typeParameters: [], instanceMembers: [], staticMembers: [] },
        },
      },
      { kind: { case: "thisType", value: {} } },
      {
        kind: {
          case: "satisfiesExpr",
          value: { value: primStringExpr(), constraint: primStringExpr() },
        },
      },
      {
        kind: {
          case: "uniqueSymbol",
          value: { declCanonical: "/a.ts", name: "id" },
        },
      },
      { kind: { case: "noInfer", value: { inner: primStringExpr() } } },
      { kind: { case: "localTypeRef", value: { binderId: "T" } } },
    ];

    expect(cases.length).toBe(22);

    const seen = new Set<string>();
    for (const init of cases) {
      const expr = create(StructuredTypeExpressionSchema, init);
      const bytes = toBinary(StructuredTypeExpressionSchema, expr);
      const decoded = fromBinary(StructuredTypeExpressionSchema, bytes);
      // Deep equality: every nested field must survive the
      // encode/decode roundtrip identically. A shallow `case`
      // check is non-discriminating; the wire surface must
      // preserve full payload identity.
      expect(decoded).toEqual(expr);
      seen.add(decoded.kind.case as string);
    }
    expect(seen.size).toBe(22);
  });

  it("GraphTypeNode roundtrips every oneof variant", () => {
    type Case = Parameters<typeof create<typeof GraphTypeNodeSchema>>[1] & {
      kind: { case: NonNullable<unknown> };
    };

    const cases: Case[] = [
      { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
      {
        kind: {
          case: "literal",
          value: { value: { kind: { case: "booleanValue", value: true } } },
        },
      },
      { kind: { case: "uniqueSymbol", value: { declSymbolId: 1 } } },
      { kind: { case: "union", value: { memberNodeIds: [0, 1] } } },
      { kind: { case: "intersection", value: { memberNodeIds: [0, 2] } } },
      {
        kind: {
          case: "object",
          value: {
            members: [
              {
                nameId: 1,
                nameKind: GraphMemberNameKind.IDENTIFIER,
                valueNodeId: 0,
                optional: false,
                readonly: true,
                accessibility: GraphAccessibility.PUBLIC,
                staticSide: false,
                declarationSymbolId: 2,
              },
            ],
            indexSignatures: [
              {
                keyKind: GraphIndexKeyKind.STRING,
                valueNodeId: 0,
                readonly: false,
              },
            ],
            callSignatureRefs: [0],
            constructSignatureRefs: [],
            flags: 0,
          },
        },
      },
      { kind: { case: "array", value: { elementNodeId: 0, readonly: false } } },
      {
        kind: {
          case: "tuple",
          value: {
            elements: [{ labelNameId: 0, valueNodeId: 0, optional: false, rest: false }],
            readonly: true,
          },
        },
      },
      { kind: { case: "reference", value: { symbolId: 1 } } },
      {
        kind: {
          case: "aliasInstantiation",
          value: {
            aliasSymbolId: 3,
            typeArgumentNodeIds: [0],
            targetNodeId: 0,
            displayRefNodeId: 0,
          },
        },
      },
      {
        kind: {
          case: "typeParameter",
          value: {
            symbolId: 4,
            declSlotRef: 5,
            paramIndex: 0,
            nameId: 6,
            constraintNodeId: 0,
            defaultNodeId: 0,
            variance: GraphVariance.INDEPENDENT,
            isConst: false,
            noInfer: false,
            binding: { kind: { case: "unbound", value: {} } },
          },
        },
      },
      { kind: { case: "keyOf", value: { baseNodeId: 0 } } },
      { kind: { case: "indexedAccess", value: { objectNodeId: 0, indexNodeId: 0 } } },
      {
        kind: {
          case: "conditional",
          value: {
            checkNodeId: 0,
            extendsNodeId: 0,
            trueBranchNodeId: 0,
            falseBranchNodeId: 0,
            distributive: false,
            resolution: { kind: { case: "selectedTrue", value: { proofRef: 0 } } },
          },
        },
      },
      {
        kind: {
          case: "mapped",
          value: {
            keyTypeNodeId: 0,
            sourceNodeId: 0,
            nameRemapNodeId: 0,
            valueTypeNodeId: 0,
            readonlyModifier: GraphMappedModifier.NONE,
            optionalModifier: GraphMappedModifier.ADD,
          },
        },
      },
      {
        kind: {
          case: "templateLiteral",
          value: { quasiNameIds: [1, 2], expressionNodeIds: [0] },
        },
      },
      { kind: { case: "typeofNode", value: { valueRootRef: 7, pathNameIds: [8, 9] } } },
      { kind: { case: "satisfiesNode", value: { valueNodeId: 0, constraintNodeId: 0 } } },
      {
        kind: {
          case: "classNode",
          value: {
            symbolId: 10,
            typeParameterNodeIds: [],
            heritage: [],
            members: [],
            staticMembers: [],
            constructSignatureRefs: [],
            flags: 0,
          },
        },
      },
      { kind: { case: "thisType", value: { declSymbolId: 11 } } },
      {
        kind: {
          case: "mergedDeclaration",
          value: {
            mergedSymbolId: 12,
            parts: [
              {
                sourceCanonicalNameId: 13,
                declarationNodeId: 0,
                kind: GraphDeclarationPartKind.INTERFACE,
              },
            ],
          },
        },
      },
      {
        kind: {
          case: "ambientModule",
          value: { specifierNameId: 14, moduleNamespaceNodeId: 0 },
        },
      },
      {
        kind: {
          case: "moduleAugmentation",
          value: { specifierNameId: 15, parts: [] },
        },
      },
      {
        kind: {
          case: "ambientNamespace",
          value: { namespaceNameId: 16, namespaceNodeId: 0 },
        },
      },
      { kind: { case: "globalAugmentation", value: { parts: [] } } },
      {
        kind: {
          case: "flowNarrowing",
          value: { siteSpanRef: 17, narrowedNodeId: 0, baseNodeId: 0 },
        },
      },
      {
        kind: {
          case: "contextualType",
          value: { siteSpanRef: 18, contextualNodeId: 0 },
        },
      },
      {
        kind: {
          case: "relationProof",
          value: {
            outcome: GraphRelationOutcome.TRUE,
            steps: [
              {
                kind: GraphRelationStepKind.STRUCTURAL,
                sourceNodeId: 0,
                targetNodeId: 0,
              },
            ],
          },
        },
      },
      { kind: { case: "inferNode", value: { nameId: 19, constraintNodeId: 0 } } },
      {
        kind: {
          case: "enumNode",
          value: {
            symbolId: 20,
            members: [
              {
                nameId: 21,
                value: { kind: { case: "numeric", value: 42n } },
              },
            ],
            isConst: false,
          },
        },
      },
      {
        kind: {
          case: "opaque",
          value: { error: { kind: { case: "miss", value: {} } } },
        },
      },
      { kind: { case: "cycle", value: { cycleRootNodeId: 0, participants: [22] } } },
    ];

    expect(cases.length).toBe(32);

    const seen = new Set<string>();
    for (const init of cases) {
      const node = create(GraphTypeNodeSchema, init);
      const bytes = toBinary(GraphTypeNodeSchema, node);
      const decoded = fromBinary(GraphTypeNodeSchema, bytes);
      // Deep equality across every field of every variant —
      // pre-fix the bare `case` check passed even when nested
      // payload corruption survived the round-trip.
      expect(decoded).toEqual(node);
      seen.add(decoded.kind.case as string);
    }
    expect(seen.size).toBe(32);
  });

  it("TypeInfoGraphRequest roundtrips every payload arm", () => {
    type Case = Parameters<typeof create<typeof TypeInfoGraphRequestSchema>>[1] & {
      payload: { case: NonNullable<unknown> };
    };

    const arms: Case[] = [
      {
        operation: GraphOperation.RESOLVE_SYMBOL,
        payload: {
          case: "resolveSymbol",
          value: {
            canonicalId: "/a.ts",
            name: "Foo",
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            closure: { kind: { case: "oneLevel", value: {} } },
            displayPolicy: {},
          },
        },
      },
      {
        operation: GraphOperation.EVALUATE_EXPRESSION,
        payload: {
          case: "evaluateTypeExpression",
          value: {
            scopeCanonical: "/a.ts",
            expression: primStringExpr(),
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            closure: { kind: { case: "oneLevel", value: {} } },
            displayPolicy: {},
          },
        },
      },
      {
        operation: GraphOperation.PROJECT_PATH,
        payload: {
          case: "projectPath",
          value: {
            canonicalId: "/a.ts",
            name: "Foo",
            path: [{ kind: { case: "property", value: { nameId: 7 } } }],
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            closure: { kind: { case: "oneLevel", value: {} } },
            displayPolicy: {},
          },
        },
      },
      {
        operation: GraphOperation.FLOW_NARROWING_AT,
        payload: {
          case: "flowNarrowing",
          value: {
            canonicalId: "/a.ts",
            span: { canonicalId: "/a.ts", start: 1, end: 4 },
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            displayPolicy: {},
          },
        },
      },
      {
        operation: GraphOperation.CONTEXTUAL_TYPE_AT,
        payload: {
          case: "contextualType",
          value: {
            canonicalId: "/a.ts",
            span: { canonicalId: "/a.ts", start: 1, end: 4 },
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            displayPolicy: {},
          },
        },
      },
      {
        operation: GraphOperation.EXPAND_AROUND,
        payload: {
          case: "expandAround",
          value: {
            parentGraph: { opaque: new Uint8Array([1, 2, 3]) },
            target: { nodeId: 3, isCanonical: false },
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            closure: { kind: { case: "oneLevel", value: {} } },
            displayPolicy: {},
          },
        },
      },
      {
        operation: GraphOperation.FRAMEWORK_SURFACES,
        payload: {
          case: "frameworkSurface",
          value: {
            selector: {
              canonicalId: "/Foo.vue",
              hasExportName: false,
              frameworkAdapterId: "vue",
            },
            context: {
              mode: GraphProjectionMode.EXPANDED,
              demand: GraphReductionDemand.PUBLISHED,
            },
            closure: { kind: { case: "oneLevel", value: {} } },
            displayPolicy: {},
            schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
          },
        },
      },
    ];

    expect(arms.length).toBe(7);

    for (const init of arms) {
      const req = create(TypeInfoGraphRequestSchema, {
        schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
        ...init,
      });
      const bytes = toBinary(TypeInfoGraphRequestSchema, req);
      const decoded = fromBinary(TypeInfoGraphRequestSchema, bytes);
      // Deep equality across the full request envelope, including
      // the nested oneof payload. Pre-fix the `case` match alone
      // could not detect inner-field corruption (e.g. a wrong
      // canonical id surviving as the decoded value).
      expect(decoded).toEqual(req);
    }
  });

  it("TypeInfoRequestError roundtrips every variant", () => {
    type Case = Parameters<typeof create<typeof TypeInfoRequestErrorSchema>>[1] & {
      kind: { case: NonNullable<unknown> };
    };

    const variants: Case[] = [
      { kind: { case: "missingProjectionContext", value: {} } },
      { kind: { case: "missingDisplayPolicy", value: {} } },
      { kind: { case: "invalidMode", value: { received: "bogus" } } },
      { kind: { case: "missingClosurePolicy", value: {} } },
      {
        kind: {
          case: "unknownSchemaVersion",
          value: { wireVersion: 7, serverVersion: 1, serverSupportedVersions: [1] },
        },
      },
      { kind: { case: "malformedPayload", value: { detail: "boom" } } },
      { kind: { case: "omittedRoots", value: {} } },
      { kind: { case: "unstableState", value: { attempts: 3 } } },
      { kind: { case: "malformedStructuredExpression", value: { detail: "cycle" } } },
      { kind: { case: "missingProjectPath", value: {} } },
      {
        kind: {
          case: "expansionBudgetOutOfRange",
          value: { nodeBudget: 5000, depthBudget: 256, nodeBudgetMax: 4096, depthBudgetMax: 64 },
        },
      },
    ];

    expect(variants.length).toBe(11);

    for (const init of variants) {
      const err = create(TypeInfoRequestErrorSchema, init);
      const bytes = toBinary(TypeInfoRequestErrorSchema, err);
      const decoded = fromBinary(TypeInfoRequestErrorSchema, bytes);
      // Deep equality so payload fields (received, wireVersion,
      // detail, attempts, endpoint, nodeBudget…) all survive.
      expect(decoded).toEqual(err);
    }
  });

  it("Capability handshake roundtrips", () => {
    const request = create(TypeInfoCapabilityHandshakeRequestSchema, { clientVersion: 4 });
    const requestBytes = toBinary(TypeInfoCapabilityHandshakeRequestSchema, request);
    const requestDecoded = fromBinary(TypeInfoCapabilityHandshakeRequestSchema, requestBytes);
    // Deep equality across the full handshake request — every
    // field must survive byte-equivalent.
    expect(requestDecoded).toEqual(request);

    const response = create(TypeInfoCapabilityHandshakeResponseSchema, {
      serverVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      serverSupportedVersions: [1, 2, 3],
    });
    const responseBytes = toBinary(TypeInfoCapabilityHandshakeResponseSchema, response);
    const responseDecoded = fromBinary(TypeInfoCapabilityHandshakeResponseSchema, responseBytes);
    expect(responseDecoded).toEqual(response);
  });

  it("FrameworkSurfacePayload roundtrips with nested SemanticTypeGraph", () => {
    const payload = create(FrameworkSurfacePayloadSchema, {
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      selector: { canonicalId: "/Foo.vue", frameworkAdapterId: "vue" },
      framework: FrameworkTag.VUE,
      graph: {
        schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
        nodes: [
          {
            kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } },
          },
        ],
      },
      surfaces: [
        {
          kind: FrameworkSurfaceKind.PROPS,
          members: [{ nameId: 1, typeNodeId: 0, required: true, readonly: false }],
        },
      ],
    });

    const bytes = toBinary(FrameworkSurfacePayloadSchema, payload);
    const decoded = fromBinary(FrameworkSurfacePayloadSchema, bytes);
    // Deep equality through the nested SemanticTypeGraph — every
    // member field, every nested node payload, and the selector
    // must survive identically.
    expect(decoded).toEqual(payload);
  });

  it("TypeInfoGraphResponse roundtrips the frameworkSurface arm with per-kind status", () => {
    // Exactly one entry per known FrameworkSurfaceKind, each carrying
    // the per-kind status (UNSPECIFIED is invalid in server-produced
    // v3 payloads).
    const everyKind = Object.values(FrameworkSurfaceKind).filter(
      (v): v is FrameworkSurfaceKind => typeof v === "number",
    );
    expect(everyKind.length).toBe(6);

    const response = create(TypeInfoGraphResponseSchema, {
      kind: {
        case: "frameworkSurface",
        value: {
          schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
          selector: { canonicalId: "/Foo.vue", frameworkAdapterId: "vue" },
          framework: FrameworkTag.VUE,
          graph: { schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION },
          surfaces: everyKind.map((kind) => ({
            kind,
            members: [],
            status: {
              support: FrameworkSurfaceKindSupport.SUPPORTED,
              exactness: GraphExactness.EXACT_RESOLVED,
              diagnostics: [],
            },
          })),
        },
      },
    });

    const bytes = toBinary(TypeInfoGraphResponseSchema, response);
    const decoded = fromBinary(TypeInfoGraphResponseSchema, bytes);
    expect(decoded).toEqual(response);
    expect(decoded.kind.case).toBe("frameworkSurface");
    if (decoded.kind.case !== "frameworkSurface") return;
    expect(decoded.kind.value.surfaces.length).toBe(everyKind.length);
    for (const entry of decoded.kind.value.surfaces) {
      expect(entry.status?.support).toBe(FrameworkSurfaceKindSupport.SUPPORTED);
    }
  });

  it("supported-empty and unsupported-empty decode to distinct typed states", () => {
    // The wire proof that an empty member list alone never means
    // "unsupported": SUPPORTED + empty members is supported-empty;
    // UNSUPPORTED carries exactness UNSUPPORTED plus a diagnostic.
    const supportedEmpty = create(FrameworkSurfaceKindEntrySchema, {
      kind: FrameworkSurfaceKind.SLOTS,
      members: [],
      status: {
        support: FrameworkSurfaceKindSupport.SUPPORTED,
        exactness: GraphExactness.EXACT_RESOLVED,
        diagnostics: [],
      },
    });
    const unsupportedEmpty = create(FrameworkSurfaceKindEntrySchema, {
      kind: FrameworkSurfaceKind.SLOTS,
      members: [],
      status: {
        support: FrameworkSurfaceKindSupport.UNSUPPORTED,
        exactness: GraphExactness.UNSUPPORTED,
        diagnostics: [{ severity: GraphDiagnosticSeverity.WARN, messageNameId: 9 }],
      },
    });

    const decodedSupported = fromBinary(
      FrameworkSurfaceKindEntrySchema,
      toBinary(FrameworkSurfaceKindEntrySchema, supportedEmpty),
    );
    const decodedUnsupported = fromBinary(
      FrameworkSurfaceKindEntrySchema,
      toBinary(FrameworkSurfaceKindEntrySchema, unsupportedEmpty),
    );

    // Both decode with empty member lists…
    expect(decodedSupported.members).toEqual([]);
    expect(decodedUnsupported.members).toEqual([]);
    // …yet remain distinct typed states.
    expect(decodedSupported).not.toEqual(decodedUnsupported);
    expect(decodedSupported.status?.support).toBe(FrameworkSurfaceKindSupport.SUPPORTED);
    expect(decodedUnsupported.status?.support).toBe(FrameworkSurfaceKindSupport.UNSUPPORTED);
    expect(decodedUnsupported.status?.exactness).toBe(GraphExactness.UNSUPPORTED);
    expect(decodedUnsupported.status?.diagnostics.length).toBeGreaterThanOrEqual(1);
  });

  it("wire schema version is 3 with the framework-surface response arm", () => {
    // Schema 2→3: the framework_surface response arm + per-kind
    // status landed under a schema bump per the closed-enum rule.
    expect(TYPEINFO_GRAPH_SCHEMA_VERSION).toBe(3);
  });

  it("deep-equality roundtrip detects nested field corruption that case-match misses", () => {
    // Discriminator for the deep-equality contract: build two
    // distinct structured expressions that share the same outer
    // `kind.case` discriminator but differ on a nested field. A
    // shallow `expect(decoded.kind.case).toBe(init.payload.case)`
    // would pass both inputs equivalently; deep equality rejects
    // the swapped pair.
    const stringExpr = create(StructuredTypeExpressionSchema, {
      kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } },
    });
    const numberExpr = create(StructuredTypeExpressionSchema, {
      kind: { case: "primitive", value: { kind: GraphPrimitiveKind.NUMBER } },
    });

    // Sanity: both share the same `case` discriminator.
    expect(stringExpr.kind.case).toBe(numberExpr.kind.case);

    // Deep equality must distinguish the nested primitive kind.
    expect(stringExpr).not.toEqual(numberExpr);

    // Roundtrip preserves identity end-to-end.
    const bytes = toBinary(StructuredTypeExpressionSchema, stringExpr);
    const decoded = fromBinary(StructuredTypeExpressionSchema, bytes);
    expect(decoded).toEqual(stringExpr);
    expect(decoded).not.toEqual(numberExpr);
  });

  it("closed taxonomies report the documented cardinalities", () => {
    expect(Object.values(GraphPrimitiveKind).filter((v) => typeof v === "number").length).toBe(12);
    expect(Object.values(GraphExactness).filter((v) => typeof v === "number").length).toBe(9);
    expect(Object.values(GraphOriginEdgeKind).filter((v) => typeof v === "number").length).toBe(10);
  });
});
