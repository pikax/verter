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
  FrameworkSurfaceKindEntrySchema,
  FrameworkSurfaceKindStatusSchema,
  ComponentSelectorSchema,
  FrameworkProfileRefSchema,
  FrameworkProfileAdvertisementSchema,
  TypeInfoRequestErrorUnknownFrameworkProfileSchema,
  TypeInfoRequestErrorStaleBasisSchema,
  TypeInfoRequestErrorUnknownNodeRefSchema,
  TypeInfoRequestErrorUnsupportedOperationSchema,
  TypeInfoRequestErrorUnknownSourceSchema,
  NodeRefSchema,
  GraphProjectionRequestSchema,
  // Graph node messages (kept as the prefixed names — the schema is
  // protobuf-authoritative, and the proto-side prefix avoids collision
  // with the component-meta TypeNode under the same package).
  StructuredTypeExpressionSchema,
  GraphTypeNodeSchema,
  GraphObjectSpreadProgramSchema,
  GraphIntrinsicApplicationSchema,
  GraphObjectConstructionEffectSchema,
  GraphObjectNamedEffectSchema,
  GraphObjectIndexEffectSchema,
  GraphObjectSignatureEffectSchema,
  GraphObjectSpreadEffectSchema,
  GraphObjectMemberSpansSchema,
  GraphObjectIndexSpansSchema,
  GraphFactIdentitySchema,
  GraphSignatureSchema,
  GraphSymbolNodeSchema,
  GraphOriginEdgeSchema,
  GraphNodeStatusSchema,
  GraphDiagnosticSchema,
  GraphNodeIdMapEntrySchema,
  GraphSymbolIdMapEntrySchema,
  GraphRelationProofEntrySchema,
  GraphClosurePolicySchema,
  GraphTypePathSegmentSchema,
  GraphProjectionReductionContextSchema,
  GraphDisplayPolicySchema,
  GraphResolvedDeclSlotIdentitySchema,
  GraphSpanRefSchema,
  GraphStringTableSchema,
  // Closed enums.
  FrameworkSurfaceKind,
  FrameworkSurfaceKindSupport,
  FrameworkSurfaceOriginHopKind,
  FrameworkSurfaceDeclarationKind,
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
  GraphObjectExcessOrigin,
  GraphObjectMergeRole,
  GraphObjectMemberKind,
  GraphOperation,
  GraphOriginEdgeKind,
  GraphPrimitiveKind,
  GraphProjectionKind,
  GraphProjectionMode,
  GraphRelateEndpoint,
  GraphReductionDemand,
  GraphRelationFailureCode,
  GraphBudgetExceededKind,
  GraphSubRelationPositionKind,
  GraphRelationUnknownReason,
  GraphSignatureKind,
  GraphSignatureOrigin,
  GraphSubstitutionTag,
  GraphSymbolNamespace,
  GraphUnsupportedConstruct,
  GraphVariance,
} from "./gen/verter/v1/typeinfo_pb.js";

// The SHARED compiler-intrinsic op vocabulary lives in its own generated
// module (compiler_intrinsics.proto) and is referenced by BOTH envelopes, so
// it is re-exported from there rather than through either envelope.
export { CompilerIntrinsicTypeOp } from "./gen/verter/v1/compiler_intrinsics_pb.js";

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
  FrameworkSurfaceKindEntry,
  FrameworkSurfaceKindStatus,
  FrameworkSurfaceMember,
  FrameworkSurfaceMemberOrigin,
  FrameworkSurfaceMemberDeclaration,
  FrameworkSurfaceOriginHop,
  ComponentSelector,
  FrameworkProfileRef,
  FrameworkProfileAdvertisement,
  TypeInfoRequestErrorUnknownFrameworkProfile,
  StructuredTypeExpression,
  GraphTypeNode,
  GraphObjectSpreadProgram,
  GraphObjectConstructionEffect,
  GraphObjectNamedEffect,
  GraphObjectIndexEffect,
  GraphObjectSignatureEffect,
  GraphObjectSpreadEffect,
  GraphObjectMemberSpans,
  GraphObjectIndexSpans,
  GraphFactIdentity,
  GraphSignature,
  GraphSymbolNode,
  GraphOriginEdge,
  GraphNodeStatus,
  GraphDiagnostic,
  GraphNodeIdMapEntry,
  GraphSymbolIdMapEntry,
  GraphRelationProofEntry,
  GraphClosurePolicy,
  GraphTypePathSegment,
  GraphProjectionReductionContext,
  GraphDisplayPolicy,
  GraphResolvedDeclSlotIdentity,
  NodeRef,
  TypeInfoRequestErrorStaleBasis,
  TypeInfoRequestErrorUnknownNodeRef,
  TypeInfoRequestErrorUnsupportedOperation,
  TypeInfoRequestErrorUnknownSource,
  GraphSpanRef,
  GraphStringTable,
  GraphProjectionRequest,
} from "./gen/verter/v1/typeinfo_pb.js";

/**
 * The current typeinfo graph wire schema version. Matches the Rust
 * constant `verter_protocol::typeinfo::graph::TYPEINFO_GRAPH_SCHEMA_VERSION`.
 * The wire-surface guards under `crates/verter_session/tests/g_block/`
 * keep the two values in lock-step.
 *
 * Schema 3 added the `framework_surface` response arm plus the
 * per-kind `FrameworkSurfaceKindStatus`. Schema 4 added the add-only
 * `FrameworkSurfaceMember.default_value_id` + `origin` fields (a
 * member's runtime default source text and its resolver-known
 * declaration provenance). Schema 5 retires the
 * `GraphTypeNode.relation_proof = 28` oneof arm (tag + name reserved)
 * and relocates the proof witness to the payload-side
 * `SemanticTypeGraph.relation_proofs` table (field 13, add-only).
 * Schema 6 adds typed property-key oneofs and explicit member kinds.
 * Schema 7 adds the tag-33 canonical ordered object-spread program with
 * lossless direct effects and raw spread operands.
 * Schema 9 states the coordinate contract: every span is a 0-based half-open
 * range of UTF-8 byte offsets into the file its `canonicalId` names; no field
 * is added or removed.
 * Schema 10 adds the facet demand `FrameworkSurfaceRequest.requestedKinds`:
 * a v10 framework-surface request names the kinds it asks for (non-empty, no
 * duplicate, no UNSPECIFIED) and its response carries exactly one entry per
 * requested kind in enum-tag order.
 * Schema 11 types the `Component` selector's framework: the open
 * `ComponentSelector.frameworkAdapterId` string (field 4) is reserved and
 * `ComponentSelector.frameworkProfile` (a `FrameworkProfileRef`, the 32-byte
 * digest of one host-admitted framework profile) replaces it. Profiles are
 * discovered through `TypeInfoCapabilityHandshakeResponse.frameworkProfiles`
 * (`VerterHost.typeInfoCapabilities`); a well-shaped reference the host does
 * not admit is the `unknownFrameworkProfile` error arm, and the response's
 * selector echo names the admitted profile.
 * Schema 12 addresses exported positions by host-issued `NodeRef`s:
 * `GraphNodeIdMapEntry.nodeRef` (field 3, add-only) carries the address of
 * each exactly replayable exported node, whether or not provenance was
 * requested. `ExpandGraphAroundRequest` reserves its parent graph handle and
 * graph-local target (fields 1 and 2) and names its position by `nodeRef`
 * (field 8); `GraphHandle` and `GraphTypeNodeRef` are deleted. A reference
 * the host does not hold is the `unknownNodeRef` error arm, one whose basis
 * no longer describes the current inputs is `staleBasis`. An operation the
 * host does not serve is the `unsupportedOperation` error arm at every
 * accepted version; it echoes the requested operation.
 * Schema 2 stays accepted for the legacy operations; the framework-surface
 * operation requires 11 and the expand-around operation 12.
 */
export const TYPEINFO_GRAPH_SCHEMA_VERSION = 12;
