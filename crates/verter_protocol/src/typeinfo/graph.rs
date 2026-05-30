//! Typeinfo graph wire contracts.
//!
//! This module is the public Rust surface for the typeinfo graph
//! schemas defined in `crates/verter_protocol/proto/verter/v1/typeinfo.proto`.
//! Every type below is re-exported from the prost-generated module
//! `crate::verter::v1`; consumers depend on `verter_protocol::typeinfo::graph::*`
//! so the indirection layer can rename / wrap generated names without
//! breaking call sites.
//!
//! # Naming convention
//!
//! Proto-side messages carry a `Graph` prefix to avoid collision
//! with `component_meta.proto`'s pre-existing `TypeNode` /
//! `TypeGraph` / `Signature` types that share the `verter.v1`
//! package. The Rust re-exports below drop the prefix where the
//! unprefixed name is unambiguous within this submodule (e.g.,
//! `TypeNode`, `Signature`).
//!
//! **Callers MUST NOT glob-import this module** (`use
//! verter_protocol::typeinfo::graph::*`). The unprefixed names
//! collide with the `component_meta`-side types the moment a
//! consumer imports both modules from the same crate. The
//! disambiguation is provided by referencing the type through the
//! `graph::` path qualifier:
//!
//! ```ignore
//! use verter_protocol::typeinfo::graph;
//!
//! fn produce_typeinfo_node() -> graph::TypeNode { /* ... */ }
//! ```
//!
//! This convention is permanent — it is the wire-side answer to the
//! `verter.v1`-package collision and stays in place even if the
//! `component_meta` side later renames its `TypeNode`. Reviewers who
//! see a `pub use wire::Graph* as *` re-export should read it as
//! "the proto name keeps its `Graph` prefix on the wire; the Rust
//! ergonomic short-name lives only inside the `graph::` namespace".

#![allow(missing_docs)]

use crate::verter::v1 as wire;

// -------------------------------------------------------------------------
// Top-level envelope, request/response wrappers, and identity.
// -------------------------------------------------------------------------

pub use wire::SemanticTypeGraph;
pub use wire::TypeInfoCapabilityHandshakeRequest;
pub use wire::TypeInfoCapabilityHandshakeResponse;
pub use wire::TypeInfoGraphRequest;
pub use wire::TypeInfoGraphResponse;
pub use wire::TypeInfoRequestError;

pub use wire::ComponentSelector;
pub use wire::ContextualTypeRequest;
pub use wire::EvaluateTypeExpressionGraphRequest;
pub use wire::ExpandGraphAroundRequest;
pub use wire::FlowNarrowingRequest;
pub use wire::FrameworkSurfaceKindEntry;
pub use wire::FrameworkSurfaceMember;
pub use wire::FrameworkSurfacePayload;
pub use wire::FrameworkSurfaceRequest;
pub use wire::ProjectPathGraphRequest;
pub use wire::ResolveSymbolGraphRequest;

// -------------------------------------------------------------------------
// Graph nodes — re-exported under un-prefixed names for ergonomic call sites.
// -------------------------------------------------------------------------

pub use wire::graph_type_node::Kind as TypeNodeKind;
pub use wire::GraphTypeNode as TypeNode;

pub use wire::GraphAliasInstantiation as AliasInstantiationNode;
pub use wire::GraphAmbientModule as AmbientModuleNode;
pub use wire::GraphAmbientNamespace as AmbientNamespaceNode;
pub use wire::GraphArray as ArrayNode;
pub use wire::GraphClass as ClassNode;
pub use wire::GraphConditional as ConditionalNode;
pub use wire::GraphConditionalResolution as ConditionalResolution;
pub use wire::GraphContextualType as ContextualTypeNode;
pub use wire::GraphCycle as CycleNode;
pub use wire::GraphDeclarationPart as DeclarationPart;
pub use wire::GraphDistributedConditionalCase as DistributedConditionalCase;
pub use wire::GraphEnum as EnumNode;
pub use wire::GraphEnumMember as EnumMember;
pub use wire::GraphEnumMemberValue as EnumMemberValue;
pub use wire::GraphFlowNarrowing as FlowNarrowingNode;
pub use wire::GraphGlobalAugmentation as GlobalAugmentationNode;
pub use wire::GraphHeritageClause as HeritageClause;
pub use wire::GraphIndexSignature as IndexSignature;
pub use wire::GraphIndexedAccess as IndexedAccessNode;
pub use wire::GraphInfer as InferNode;
pub use wire::GraphIntersection as IntersectionNode;
pub use wire::GraphKeyOf as KeyOfNode;
pub use wire::GraphLiteral as LiteralNode;
pub use wire::GraphLiteralValue as LiteralValue;
pub use wire::GraphMapped as MappedNode;
pub use wire::GraphMergedDeclaration as MergedDeclarationNode;
pub use wire::GraphModuleAugmentation as ModuleAugmentationNode;
pub use wire::GraphObject as ObjectNode;
pub use wire::GraphObjectMember as ObjectMember;
pub use wire::GraphOpaque as OpaqueNode;
pub use wire::GraphPrimitive as PrimitiveNode;
pub use wire::GraphReference as ReferenceNode;
pub use wire::GraphRelationProof as RelationProofNode;
pub use wire::GraphRelationStep as RelationStep;
pub use wire::GraphSatisfies as SatisfiesNode;
pub use wire::GraphTemplateLiteral as TemplateLiteralNode;
pub use wire::GraphThisType as ThisTypeNode;
pub use wire::GraphTuple as TupleNode;
pub use wire::GraphTupleElement as TupleElement;
pub use wire::GraphTypeOf as TypeOfNode;
pub use wire::GraphTypeParameter as TypeParameterNode;
pub use wire::GraphTypeParameterBinding as TypeParameterBinding;
pub use wire::GraphUnion as UnionNode;
pub use wire::GraphUniqueSymbol as UniqueSymbolNode;

pub use wire::GraphAssertionEffect as AssertionEffect;
pub use wire::GraphPredicateSubject as PredicateSubject;
pub use wire::GraphSignature as Signature;
pub use wire::GraphSignatureParameter as SignatureParameter;
pub use wire::GraphThisParameter as ThisParameter;
pub use wire::GraphTypePredicate as TypePredicate;

pub use wire::GraphDeclSlotRef as DeclSlotRef;
pub use wire::GraphNodeIdMapEntry as NodeIdMapEntry;
pub use wire::GraphResolvedDeclSlotIdentity as ResolvedDeclSlotIdentityDto;
pub use wire::GraphStringTable as StringTable;
pub use wire::GraphSymbolIdMapEntry as SymbolIdMapEntry;
pub use wire::GraphSymbolNode as SymbolNode;

pub use wire::GraphDiagnostic as Diagnostic;
pub use wire::GraphNodeStatus as NodeStatus;
pub use wire::GraphOriginEdge as OriginEdge;

pub use wire::GraphClosureExpanded as ClosureExpanded;
pub use wire::GraphClosureOneLevel as ClosureOneLevel;
pub use wire::GraphClosurePath as ClosurePath;
pub use wire::GraphClosurePolicy as ClosurePolicy;
pub use wire::GraphClosureProjectionRequired as ClosureProjectionRequired;
pub use wire::GraphClosureRootOnly as ClosureRootOnly;
pub use wire::GraphDisplayBudgets as DisplayBudgets;
pub use wire::GraphDisplayPolicy as DisplayPolicy;
pub use wire::GraphProjectionReductionContext as ProjectionReductionContext;
pub use wire::GraphProjectionRequest as ProjectionRequest;
pub use wire::GraphQueryIdentity as QueryIdentity;
pub use wire::GraphSubstitutionBinding as SubstitutionBinding;
pub use wire::GraphTypePathSegment as TypePathSegment;

pub use wire::GraphHandle as Handle;
pub use wire::GraphSpanRef as SpanRef;
pub use wire::GraphTypeNodeRef as TypeNodeRef;

// -------------------------------------------------------------------------
// Closed enums — re-exported.
// -------------------------------------------------------------------------

pub use wire::FrameworkSurfaceKind;
pub use wire::FrameworkTag;
pub use wire::GraphAccessibility as Accessibility;
pub use wire::GraphBudgetDomain as BudgetDomain;
pub use wire::GraphDeclarationPartKind as DeclarationPartKind;
pub use wire::GraphDiagnosticSeverity as DiagnosticSeverity;
pub use wire::GraphDisplayBranding as DisplayBranding;
pub use wire::GraphDisplayQualification as DisplayQualification;
pub use wire::GraphExactness as Exactness;
pub use wire::GraphHeritageKind as HeritageKind;
pub use wire::GraphIndexKeyKind as IndexKeyKind;
pub use wire::GraphInferencePolicy as InferencePolicy;
pub use wire::GraphMappedModifier as MappedModifier;
pub use wire::GraphMemberNameKind as MemberNameKind;
pub use wire::GraphOperation as Operation;
pub use wire::GraphOriginEdgeKind as OriginEdgeKind;
pub use wire::GraphPrimitiveKind as PrimitiveKind;
pub use wire::GraphProjectionKind as ProjectionKind;
pub use wire::GraphProjectionMode as ProjectionMode;
pub use wire::GraphReductionDemand as ReductionDemand;
pub use wire::GraphRelateEndpoint as RelateEndpoint;
pub use wire::GraphRelationOutcome as RelationOutcome;
pub use wire::GraphRelationStepKind as RelationStepKind;
pub use wire::GraphRelationUnknownReason as RelationUnknownReason;
pub use wire::GraphSignatureKind as SignatureKind;
pub use wire::GraphSignatureOrigin as SignatureOrigin;
pub use wire::GraphSubstitutionTag as SubstitutionTag;
pub use wire::GraphSymbolNamespace as SymbolNamespace;
pub use wire::GraphUnsupportedConstruct as UnsupportedConstruct;
pub use wire::GraphVariance as Variance;

// -------------------------------------------------------------------------
// Query error union.
// -------------------------------------------------------------------------

pub use wire::GraphQueryError as QueryError;

// -------------------------------------------------------------------------
// Structured type expression — closed DTO union.
// -------------------------------------------------------------------------

pub use wire::structured_type_expression::Kind as StructuredTypeExpressionKind;
pub use wire::StructuredTypeExpression;

pub use wire::AssertionEffectCondition;
pub use wire::AssertionEffectExpr;
pub use wire::AssertionEffectIdentifier;
pub use wire::AssertionEffectThis;
pub use wire::ExprArray;
pub use wire::ExprClass;
pub use wire::ExprConditional;
pub use wire::ExprFunction;
pub use wire::ExprIndexedAccess;
pub use wire::ExprInfer;
pub use wire::ExprIntersection;
pub use wire::ExprKeyOf;
pub use wire::ExprLiteral;
pub use wire::ExprLocalTypeRef;
pub use wire::ExprMapped;
pub use wire::ExprNoInfer;
pub use wire::ExprObject;
pub use wire::ExprPrimitive;
pub use wire::ExprReference;
pub use wire::ExprSatisfies;
pub use wire::ExprTemplateLiteral;
pub use wire::ExprThisType;
pub use wire::ExprTuple;
pub use wire::ExprTypeOf;
pub use wire::ExprUnion;
pub use wire::ExprUniqueSymbol;
pub use wire::FunctionParameterExpr;
pub use wire::FunctionReturnExpr;
pub use wire::IndexSignatureExpr;
pub use wire::MappedTypeParamExpr;
pub use wire::ObjectMemberExpr;
pub use wire::PredicateSubjectName;
pub use wire::PredicateSubjectThis;
pub use wire::TupleElementExpr;
pub use wire::TypeParameterExpr;
pub use wire::TypePredicateExpr;

// -------------------------------------------------------------------------
// Schema-version constants.
//
// The proto carries `schema_version` as a wire field. Centralising the
// current value here keeps producers from drifting away from the
// freshness check `typeinfo_proto_ts_freshness`.
// -------------------------------------------------------------------------

/// Current typeinfo graph wire schema version. Increment when adding a
/// variant to any closed `oneof` (TypeNode, StructuredTypeExpression,
/// ClosurePolicy, …) or when introducing an additive arm.
pub const TYPEINFO_GRAPH_SCHEMA_VERSION: u32 = 1;

// -------------------------------------------------------------------------
// Typed constructor helpers for the request-error variants. The
// underlying proto messages are unit-shaped (`{}`) or carry a small
// set of fields; the helpers exist so producers do not need to import
// every leaf variant name from the prost-generated module.
// -------------------------------------------------------------------------

/// Build a `MissingProjectionContext` error variant payload.
pub fn wire_error_missing_projection_context() -> wire::TypeInfoRequestErrorMissingProjectionContext
{
    wire::TypeInfoRequestErrorMissingProjectionContext {}
}

/// Build a `MissingDisplayPolicy` error variant payload.
pub fn wire_error_missing_display_policy() -> wire::TypeInfoRequestErrorMissingDisplayPolicy {
    wire::TypeInfoRequestErrorMissingDisplayPolicy {}
}

/// Build an `InvalidMode` error variant payload with the received tag.
pub fn wire_error_invalid_mode(received: &str) -> wire::TypeInfoRequestErrorInvalidMode {
    wire::TypeInfoRequestErrorInvalidMode {
        received: received.to_string(),
    }
}

/// Build a `MissingClosurePolicy` error variant payload.
pub fn wire_error_missing_closure_policy() -> wire::TypeInfoRequestErrorMissingClosurePolicy {
    wire::TypeInfoRequestErrorMissingClosurePolicy {}
}

/// Build an `UnknownSchemaVersion` error variant payload carrying the
/// wire version the client sent plus the server-supported set.
pub fn wire_error_unknown_schema_version(
    wire_version: u32,
    server_version: u32,
    server_supported_versions: &[u32],
) -> wire::TypeInfoRequestErrorUnknownSchemaVersion {
    wire::TypeInfoRequestErrorUnknownSchemaVersion {
        wire_version,
        server_version,
        server_supported_versions: server_supported_versions.to_vec(),
    }
}

/// Build a `MalformedPayload` error variant payload with detail text.
pub fn wire_error_malformed_payload(detail: &str) -> wire::TypeInfoRequestErrorMalformedPayload {
    wire::TypeInfoRequestErrorMalformedPayload {
        detail: detail.to_string(),
    }
}

/// Build an `OmittedRoots` error variant payload.
pub fn wire_error_omitted_roots() -> wire::TypeInfoRequestErrorOmittedRoots {
    wire::TypeInfoRequestErrorOmittedRoots {}
}

/// Build an `UnstableState` error variant payload (retry-budget exhausted).
pub fn wire_error_unstable_state(attempts: u32) -> wire::TypeInfoRequestErrorUnstableState {
    wire::TypeInfoRequestErrorUnstableState { attempts }
}

/// Build a `MalformedStructuredExpression` error variant payload.
pub fn wire_error_malformed_structured_expression(
    detail: &str,
) -> wire::TypeInfoRequestErrorMalformedStructuredExpression {
    wire::TypeInfoRequestErrorMalformedStructuredExpression {
        detail: detail.to_string(),
    }
}

/// Build a `MissingProjectPath` error variant payload.
pub fn wire_error_missing_project_path() -> wire::TypeInfoRequestErrorMissingProjectPath {
    wire::TypeInfoRequestErrorMissingProjectPath {}
}

/// Build an `ExpansionBudgetOutOfRange` error variant payload carrying
/// the requested budgets and the maxima the validator enforces.
pub fn wire_error_expansion_budget_out_of_range(
    node_budget: u32,
    depth_budget: u32,
    node_budget_max: u32,
    depth_budget_max: u32,
) -> wire::TypeInfoRequestErrorExpansionBudgetOutOfRange {
    wire::TypeInfoRequestErrorExpansionBudgetOutOfRange {
        node_budget,
        depth_budget,
        node_budget_max,
        depth_budget_max,
    }
}

/// Build a `GraphPathSegmentProperty` payload for the structured path
/// segments carried in `ProjectPathGraphRequest`.
pub fn wire_path_segment_property(name_id: u32) -> wire::GraphPathSegmentProperty {
    wire::GraphPathSegmentProperty { name_id }
}
