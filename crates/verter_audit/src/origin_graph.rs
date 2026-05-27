#![deny(missing_docs)]
//! Origin-graph DTOs — derivation-edge tags, node identities, and the
//! audit-side mirrors of session enums consumed by
//! [`crate::structured_event::StructuredAuditEvent`].
//!
//! These types are pure data — value semantics, `Clone + Serialize +
//! Deserialize + ts_rs::TS`. Producers in `verter_session::*` lower
//! their domain types into these mirrors at the audit boundary.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::record::{u64_as_decimal_string, Hash16};

/// In-audit opaque NodeId. Assigned by the miner from a sorted
/// canonicalisation of touched semantic node ids so identical
/// requests produce identical serialised footprints regardless of
/// thread interleaving.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NodeId(
    /// Index in the canonicalised node list.
    pub u32,
);

/// In-audit opaque edge id. Assigned by the miner from the sorted
/// canonicalisation of edges so identical requests produce identical
/// serialised footprints.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct EdgeId(
    /// Index in the canonicalised edge list.
    pub u32,
);

/// Derivation subgraph captured by the audit. Nodes and edges are
/// assigned stable opaque ids by the miner. Field docs name the sort
/// keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct DerivationSubgraph {
    /// Sorted by `(kind, structural_hash, named_identity)`; `NodeId`
    /// is the index in this sorted list.
    pub nodes: Vec<NodeRecord>,
    /// Sorted by `(result, kind, sources)`; `EdgeId` is the index in
    /// this sorted list.
    pub edges: Vec<DerivationEdgeRecord>,
}

/// One node entry in the derivation subgraph. Identity fields
/// (`kind`, `named_identity`, `structural_hash`) participate in the
/// deterministic NodeId assignment.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NodeRecord {
    /// Structural kind (union / intersection / alias / instantiated /
    /// etc.). See [`SemanticNodeKind`].
    pub kind: SemanticNodeKind,
    /// Named-type identity projection, when this node corresponds to
    /// an exported type symbol. `None` for anonymous nodes.
    pub named_identity: Option<NamedIdentity>,
    /// Content-deterministic hash distinguishing anonymous nodes.
    /// Computed from the semantic graph's node data.
    pub structural_hash: Hash16,
    /// Short human-readable label for the node — used by walker /
    /// chain renderers.
    pub display_label: Arc<str>,
}

/// Named-type identity projection — `(canonical, symbol, args)` triple
/// used to key instantiation equality.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NamedIdentity {
    /// Canonical id of the file declaring the type symbol.
    pub canonical_id: Arc<str>,
    /// Declared symbol name.
    pub symbol_name: Arc<str>,
    /// Fingerprint over the type arguments applied to the symbol.
    pub args_fingerprint: Hash16,
}

/// `#[non_exhaustive]` + `Other` catchall future-proofs against new
/// semantic-node-data variants without breaking the audit.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
#[non_exhaustive]
pub enum SemanticNodeKind {
    /// Anchor node for a declaration — used as the root of a
    /// derivation starting from an exported type.
    DeclAnchor,
    /// Instantiation of a generic with concrete arguments.
    Instantiated,
    /// Alias target (`type A = B`).
    Alias,
    /// Conditional type (`T extends U ? X : Y`).
    Conditional,
    /// Union type (`A | B`).
    Union,
    /// Intersection type (`A & B`).
    Intersection,
    /// Tuple type.
    Tuple,
    /// Object literal type.
    Object,
    /// Array / readonly-array type.
    Array,
    /// Primitive (`string`, `number`, `boolean`, etc.).
    Primitive,
    /// Unbound type parameter.
    TypeParam,
    /// Opaque placeholder (e.g. a miss / unknown).
    Opaque,
    /// Indexed-access type (`T[K]`).
    IndexedAccess,
    /// `keyof T`.
    KeyOf,
    /// `typeof expr`.
    TypeOf,
    /// Mapped type (`{ [K in ...] : ... }`).
    Mapped,
    /// Template-literal type.
    TemplateLiteral,
    /// Normalized union (post-flatten).
    NormalizeUnion,
    /// Normalized intersection (post-flatten).
    NormalizeIntersection,
    /// Catch-all for variants added to the semantic graph after the
    /// substrate's enum was last refreshed. `#[non_exhaustive]` +
    /// `Other` lets future variants land without breaking the audit
    /// consumer contract.
    Other {
        /// Name of the unrecognized variant — preserved verbatim for
        /// human inspection.
        name: Arc<str>,
    },
}

/// One derivation edge. `result` is the node produced; `sources` are
/// the nodes consumed; `meta` carries kind-specific payload.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct DerivationEdgeRecord {
    /// NodeId of the node produced by this edge.
    pub result: NodeId,
    /// Kind of derivation step (see [`OriginEdgeKind`]).
    pub kind: OriginEdgeKind,
    /// NodeIds of the input nodes consumed to produce `result`.
    pub sources: Vec<NodeId>,
    /// Kind-specific payload (substitution names, projection
    /// segments, etc.). See [`OriginEdgeMetaDto`].
    pub meta: OriginEdgeMetaDto,
}

/// Pre-canonicalisation derivation-edge value, stored on the
/// per-request accumulator before the miner assigns
/// [`NodeId`]/[`EdgeId`] indices. Identity fields are interned via
/// `Arc<str>` and `Hash16` so the accumulator stays cheap.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct DerivationEdgeRaw {
    /// Display label for the result node.
    pub result_label: Arc<str>,
    /// Optional named-identity for the result node.
    pub result_identity: Option<NamedIdentity>,
    /// Structural hash distinguishing anonymous result nodes.
    pub result_structural_hash: Hash16,
    /// Edge kind.
    pub kind: OriginEdgeKind,
    /// Display labels for the source nodes.
    pub source_labels: Vec<Arc<str>>,
    /// Optional named-identities for the source nodes.
    pub source_identities: Vec<Option<NamedIdentity>>,
    /// Structural hashes for the source nodes.
    pub source_structural_hashes: Vec<Hash16>,
    /// Edge metadata.
    pub meta: OriginEdgeMetaDto,
}

/// Audit-side origin edge kind. Mirrors the semantic graph's
/// `verter_session::semantic_query::OriginEdgeKind` (nine kinds) and
/// adds `SharedLoadReuse` — an audit-only edge emitted when a joiner
/// attaches to a winner's in-flight artifact.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum OriginEdgeKind {
    /// Instantiation of a generic with concrete arguments.
    Instantiate,
    /// Substitution of a type parameter with an argument.
    SubstituteTypeParam,
    /// Selected branch of a conditional type.
    ConditionalSelect,
    /// Binding of an `infer` clause.
    InferBind,
    /// Member projection (`T["name"]` or `.name`).
    ProjectMember,
    /// Indexed projection (`T[K]`).
    ProjectIndex,
    /// Multi-segment path projection.
    ProjectPath,
    /// Normalization step (union / intersection flatten, simplify).
    Normalize,
    /// Alias-resolve hop.
    AliasResolve,
    /// Audit-only edge — this request joined a winner's in-flight
    /// artifact via scheduler dedup. Terminates chain walks into
    /// `shared_load_terminals`.
    SharedLoadReuse,
}

/// Kind-specific payload attached to a [`DerivationEdgeRecord`].
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum OriginEdgeMetaDto {
    /// Instantiation — carries the names of the generic's type
    /// parameters, in declaration order, to aid display rendering.
    Instantiate {
        /// Names of the declared type parameters.
        type_params: Vec<Arc<str>>,
    },
    /// Type-parameter substitution.
    SubstituteTypeParam {
        /// Name of the parameter being substituted.
        param_name: Arc<str>,
        /// NodeId of the substituted-in type.
        substituted_with: NodeId,
    },
    /// Conditional-type branch selection.
    ConditionalSelect {
        /// Which branch the solver chose.
        branch: ConditionalBranch,
    },
    /// `infer` binding.
    InferBind {
        /// Name of the inferred parameter.
        param_name: Arc<str>,
        /// NodeId the parameter was bound to.
        bound_to: NodeId,
    },
    /// Single-segment member projection.
    ProjectMember {
        /// Member name that was projected out.
        member_name: Arc<str>,
        /// Typed discriminator naming WHY this edge was emitted. See
        /// [`MemberEdgeProvenance`]. Always populated at the producer
        /// (no default); the audit-validator's Rule-5 check inspects
        /// this field to distinguish published-surface fields from
        /// legitimate structural intermediates.
        provenance: MemberEdgeProvenance,
    },
    /// Single-segment indexed projection.
    ProjectIndex {
        /// Index key that was projected out.
        index_key: Arc<str>,
    },
    /// Multi-segment path projection.
    ProjectPath {
        /// Path segments, in traversal order.
        path: Vec<ProjectPathSegment>,
    },
    /// Normalization pass (union/intersection flatten, simplify).
    Normalize {
        /// Specific normalization kind performed.
        kind: NormalizeKind,
    },
    /// Alias-resolve hop.
    AliasResolve {
        /// Name of the alias that was followed.
        alias_name: Arc<str>,
    },
    /// Audit-only edge — this request joined a winner's slot.
    /// Terminates chain walks.
    SharedLoadReuse {
        /// Winning request's id.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        winner_request_id: u64,
        /// `true` when the winner's own request was audited so its
        /// record can be consulted.
        winner_audited: bool,
    },
}

/// Conditional-select branch discriminator.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ConditionalBranch {
    /// The `extends U` clause was proved true — `X` was selected.
    True,
    /// The `extends U` clause was proved false — `Y` was selected.
    False,
    /// The conditional stayed unresolved (open over a type parameter)
    /// — both arms survive into the result.
    Deferred,
}

/// One step in a projection path (member / index / keyof).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ProjectPathSegment {
    /// `.<name>` member access.
    Member {
        /// Member name.
        name: Arc<str>,
    },
    /// `[<key>]` indexed access.
    Index {
        /// Literal key used as the index.
        key: Arc<str>,
    },
    /// `keyof T` — yields the union of keys.
    KeyOf,
}

/// Typed discriminator naming WHY a single-hop `ProjectMember` edge
/// was emitted. The variant identifies the structural operation that
/// produced the member edge; it is set at every production emit site
/// in `verter_session` (exhaustively — there is no default).
///
/// Audit-side consumers (the validator, the inspect CLI) use this
/// provenance to distinguish edges whose member names are part of the
/// user-visible published surface from edges whose member names are
/// legitimate intermediates of a structural walk
/// (KeyOf-enumeration, Mapped-instantiation, multi-hop path projection).
///
/// Adding a new emit site **must** add a new variant here and update
/// the validator's allowlist explicitly. The translator in
/// `verter_session::component_meta_audit::footprint_miner::translate_meta`
/// matches exhaustively on `OriginMeta::ProjectedMember`'s provenance
/// field, so no `_ =>` wildcard is permitted.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MemberEdgeProvenance {
    /// Final published surface field — direct emission onto the
    /// user-visible surface (a published prop / emit / slot / exposed).
    /// Edges with this provenance participate in the published-set
    /// check; they are NOT subtracted as legitimate intermediates.
    PublishedField,
    /// Member name produced by walking a declared multi-segment path
    /// (e.g. `Foo['a']['b']` — each segment's per-hop ProjectMember
    /// edge carries this provenance). Legitimate intermediate; the
    /// name is on the user's declared path.
    PathProjection,
    /// Key literal enumerated from a `keyof T` / keyspace expansion.
    /// One ProjectMember edge is emitted per discovered key; legitimate
    /// intermediate of a structural keyspace operation.
    KeyOfEnumerated,
    /// Member produced by a Mapped-type instantiation
    /// (`{ [K in keyof T]: ... }` and its derivatives Pick / Omit /
    /// Partial / Required / Readonly, which lower to Mapped). One
    /// ProjectMember edge is emitted per produced key; legitimate
    /// intermediate.
    MappedKeyEnumerated,
}

/// Kind of normalization performed (union-flatten, intersection-
/// flatten, or a simplification pass).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum NormalizeKind {
    /// Union flattening / dedup pass.
    Union,
    /// Intersection flattening / dedup pass.
    Intersection,
    /// Miscellaneous simplify pass.
    Simplify,
}

/// Subject of a materialization envelope — which owner+member
/// (or other identity) the envelope covers.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializationSubject {
    /// Member-route materialization (owner's public member lookup).
    MemberRoute {
        /// Owner file's canonical id.
        owner: Arc<str>,
        /// Member name being materialized.
        member: Arc<str>,
    },
    /// Public prop type materialization.
    PublicPropType {
        /// Owner file's canonical id.
        owner: Arc<str>,
        /// Prop name being materialized.
        prop: Arc<str>,
    },
    /// `defineProps<…>()` member materialization.
    DefinePropsMember {
        /// Owner file's canonical id.
        owner: Arc<str>,
        /// Member name being materialized.
        member: Arc<str>,
    },
    /// Fallthrough-inheritance resolver envelope.
    FallthroughInheritance {
        /// Owner file's canonical id.
        owner: Arc<str>,
    },
    /// Generic structural materialisation envelope. Subject of every
    /// `materialize_component_meta_structure` invocation.
    Structure {
        /// Owner scope's canonical id (the scope the materialiser was
        /// dispatched in).
        owner: Arc<str>,
        /// Stable display key for the input semantic node.
        node_key: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
    },
    /// Prepared-decl bundle materialization. Subject of every
    /// cold-path build inside
    /// `materialize_prepared_decl_bundle_from_route_owned_shallow`
    /// and `materialize_prepared_decl_bundle` (the two cold
    /// producers of `prepared_decl_bundles`).
    ///
    /// `cold` distinguishes a true cold rebuild (`true`) from a
    /// warm-cache hit short-circuit the producer detected before
    /// committing the bundle (`false` — currently unused on
    /// production paths, reserved for the `get_if_valid_self_rooted`
    /// re-check branch inside the singleflight leader closure).
    PreparedDeclBundle {
        /// Canonical id of the bundle's keyed file (its self-root).
        canonical: Arc<str>,
        /// `true` when the bundle was built from cold; `false`
        /// reserved for a future re-check warm short-circuit.
        cold: bool,
    },
}

/// PUB mirror of the materialiser's `MaterializationScope` axis.
/// Audit consumers (TS bindings, harness) do not depend on the
/// materialiser type so the substrate carries this mirror.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializationScopeAudit {
    /// Top-level entry — input came from a caller's first
    /// `materialize_component_meta_structure` invocation.
    TopLevel,
    /// Nested entry — input came from a parent materialise frame
    /// recursing into a child shape.
    Nested,
}

/// PUB mirror of `verter_session::semantic_query::ProjectionMode`.
/// Same rationale as [`MaterializationScopeAudit`] — keeps audit
/// consumers independent of the dispatch types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ProjectionModeAudit {
    /// Identity — pass-through, no projection.
    Identity,
    /// Navigate — preserve carriers, no expansion.
    Navigate,
    /// Shallow — expose one level of surface members.
    Shallow,
    /// Expanded — recursively materialize.
    Expanded,
    /// Skeleton — open-generic body access for cycle detection.
    Skeleton,
}

/// Reason a `MaterializeStructurePolicySkip` event fired — captures
/// the policy-table arm that bailed before dispatch.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializeSkipReason {
    /// Object-property lookup hit a function-typed property at
    /// `Nested` depth — function bodies are not materialised through
    /// member position.
    FunctionPropertyAtNested,
    /// Top-level generic ref carried explicit type arguments —
    /// reserved for the dedicated InstantiationRef arm.
    GenericRefWithArgsTopLevel,
    /// Top-level ref resolved to a node under `node_modules/` —
    /// package types are kept opaque.
    PackageRefTopLevel,
    /// Registry-route check rejected the input as not inline-
    /// materializable (e.g., `Pick`/`Omit` over a non-bare root).
    RegistryRouteNotInlineMaterialisable,
    /// Top-level input shape is non-structural (primitive, literal,
    /// type-param, etc.) — nothing to materialise.
    NonStructuralTopLevel,
    /// The registry-route guard's cycle check fired. The wrapping
    /// `Pick` / `Omit` / `IndexedAccess` is kept symbolic because
    /// expanding a recursive helper would publish a circular shape
    /// into the component-meta surface.
    RegistryRouteCycleGuard,
    /// The recursive-helper cycle guard fired on a plain `DeclRef`
    /// or userland `InstantiationRef`. The declaration body reaches
    /// itself via a complex helper; kept symbolic.
    RecursiveHelperCycleGuard,
}

/// Dispatch key kind — semantic-query cache key discriminator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum DispatchKeyKind {
    /// Resolve a declaration (`typeof …`, `type A = …`).
    ResolveDecl,
    /// Instantiate a generic.
    Instantiate,
    /// Member projection.
    ProjectMember,
    /// Indexed projection.
    ProjectIndex,
    /// Multi-segment path projection.
    ProjectPath,
    /// Normalization pass.
    Normalize,
    /// Resolved named-type key (see Vue macro resolution).
    ResolvedNamedType,
}

/// Which VFS layer served the read — mirrored from the workspace's
/// own `VfsAuditLayer`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum VfsLayer {
    /// Overlay (active editor buffer).
    Overlay,
    /// Snapshot cache hit.
    Snapshot,
    /// Disk read.
    Disk,
    /// Directory index returned a negative (file known not to exist).
    DirIndexNegative,
    /// Read missed every layer — the file was not found.
    Missing,
}
