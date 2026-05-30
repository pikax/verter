// Public re-exports for the typeinfo graph wire contracts. The
// generated module at `./gen/verter/v1/typeinfo_pb.ts` is the
// authoritative DTO carrier; this file selects the symbols the
// package exposes so consumers do not need to depend on the
// `gen/verter/v1/typeinfo_pb.js` path directly.

// Top-level envelope, request/response wrappers, identity.
export {
  // Schemas — descriptors used for runtime encode/decode and for
  // building MessageInit literals via `create()`.
  SemanticTypeGraphSchema,
  TypeInfoGraphRequestSchema,
  TypeInfoGraphResponseSchema,
  TypeInfoRequestErrorSchema,
  TypeInfoCapabilityHandshakeRequestSchema,
  TypeInfoCapabilityHandshakeResponseSchema,
  ResolveSymbolGraphRequestSchema,
  EvaluateTypeExpressionGraphRequestSchema,
  ProjectPathGraphRequestSchema,
  FlowNarrowingRequestSchema,
  ContextualTypeRequestSchema,
  ExpandGraphAroundRequestSchema,
  FrameworkSurfaceRequestSchema,
  FrameworkSurfacePayloadSchema,
  ComponentSelectorSchema,
  GraphProjectionRequestSchema,
  // Graph node messages (kept as the prefixed names — the schema is
  // protobuf-authoritative, and the proto-side prefix avoids collision
  // with the component-meta TypeNode under the same package).
  StructuredTypeExpressionSchema,
  GraphTypeNodeSchema,
  GraphQueryIdentitySchema,
  GraphSignatureSchema,
  GraphSymbolNodeSchema,
  GraphOriginEdgeSchema,
  GraphNodeStatusSchema,
  GraphDiagnosticSchema,
  GraphClosurePolicySchema,
  GraphTypePathSegmentSchema,
  GraphProjectionReductionContextSchema,
  GraphDisplayPolicySchema,
  GraphResolvedDeclSlotIdentitySchema,
  GraphDeclSlotRefSchema,
  GraphHandleSchema,
  GraphTypeNodeRefSchema,
  GraphSpanRefSchema,
  GraphStringTableSchema,
  // Closed enums.
  FrameworkSurfaceKind,
  FrameworkTag,
  GraphAccessibility,
  GraphBudgetDomain,
  GraphDeclarationPartKind,
  GraphDiagnosticSeverity,
  GraphDisplayBranding,
  GraphDisplayQualification,
  GraphExactness,
  GraphHeritageKind,
  GraphIndexKeyKind,
  GraphInferencePolicy,
  GraphMappedModifier,
  GraphMemberNameKind,
  GraphOperation,
  GraphOriginEdgeKind,
  GraphPrimitiveKind,
  GraphProjectionKind,
  GraphProjectionMode,
  GraphRelateEndpoint,
  GraphReductionDemand,
  GraphRelationOutcome,
  GraphRelationStepKind,
  GraphRelationUnknownReason,
  GraphSignatureKind,
  GraphSignatureOrigin,
  GraphSubstitutionTag,
  GraphSymbolNamespace,
  GraphUnsupportedConstruct,
  GraphVariance,
} from "./gen/verter/v1/typeinfo_pb.js";

export type {
  SemanticTypeGraph,
  TypeInfoGraphRequest,
  TypeInfoGraphResponse,
  TypeInfoRequestError,
  TypeInfoCapabilityHandshakeRequest,
  TypeInfoCapabilityHandshakeResponse,
  ResolveSymbolGraphRequest,
  EvaluateTypeExpressionGraphRequest,
  ProjectPathGraphRequest,
  FlowNarrowingRequest,
  ContextualTypeRequest,
  ExpandGraphAroundRequest,
  FrameworkSurfaceRequest,
  FrameworkSurfacePayload,
  ComponentSelector,
  StructuredTypeExpression,
  GraphTypeNode,
  GraphQueryIdentity,
  GraphSignature,
  GraphSymbolNode,
  GraphOriginEdge,
  GraphNodeStatus,
  GraphDiagnostic,
  GraphClosurePolicy,
  GraphTypePathSegment,
  GraphProjectionReductionContext,
  GraphDisplayPolicy,
  GraphResolvedDeclSlotIdentity,
  GraphDeclSlotRef,
  GraphHandle,
  GraphTypeNodeRef,
  GraphSpanRef,
  GraphStringTable,
  GraphProjectionRequest,
} from "./gen/verter/v1/typeinfo_pb.js";

/**
 * The current typeinfo graph wire schema version. Matches the Rust
 * constant `verter_protocol::typeinfo::graph::TYPEINFO_GRAPH_SCHEMA_VERSION`.
 * The taxonomy guard in `crates/verter_session/tests/architecture_guards.rs`
 * keeps the two values in lock-step.
 */
export const TYPEINFO_GRAPH_SCHEMA_VERSION = 1;
