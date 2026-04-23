//! Semantic Query Graph (Phase 2.2)
//!
//! Host-owned memo table keyed by [`SemanticQueryKey`] that deduplicates reusable
//! type-resolution work across all higher-level requests. This is the shared
//! substrate component-meta and the resolver are migrating onto as part of the
//! project-global cache overhaul.
//!
//! ## Contract
//!
//! - **Identity is semantic meaning**, not request identity or source text.
//! - Query keys are **version-rooted** via [`ScopeId::canonical_id`] and any
//!   enclosing whole-hash carried in resolved node data.
//! - Semantic nodes are **immutable**; file changes create new node identities
//!   rather than mutating existing nodes in place.
//! - Shared semantic entries **never retain borrowed OXC AST pointers** — they
//!   store owned immutable semantic data or interned node ids only.
//! - **Partial / cancelled / budget-exceeded results never become warm shared
//!   cache entries.**
//! - **Navigators are non-owning**: [`TypeNavigator`] may choose the next hop
//!   and perform limited normalization, but any reusable semantic work must
//!   enter through [`SemanticQueryApi::execute`].
//!
//! This module intentionally introduces the type surface without wiring it
//! through the hot path yet; Phase 2.2 lands the implementation that binds it
//! to [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore).

use std::sync::Arc;

use verter_semantic::analysis::Hash16;

// Re-export the solver's primitive enum so semantic nodes and the type
// solver agree on the same set of primitive kinds.
pub use verter_semantic::analysis::type_solver::arena::PrimitiveKind;

// Literal-value carrier for [`SemanticNodeData::Literal`]. Re-exported
// so callers working with the semantic graph can match on exact literal
// shapes (`"idle"` vs `"busy"`) without collapsing them to the broader
// `Primitive(String)`.
pub use verter_semantic::analysis::type_expr::LiteralValue;

// Reuse the existing structured failure shape from the resolver — there is no
// second failure-domain type in this rewrite.
pub use crate::resolver_core::shallow_file_state::BudgetExceededFailure;

/// Canonical content-hash type used everywhere this subsystem talks about
/// file-version identity. Mirrors the workspace-wide [`Hash16`].
pub type HashValue = Hash16;

/// Stable identity for a semantic node inside the project-global semantic
/// graph. The ID is handed out by the graph interner and is only meaningful
/// inside one [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)
/// for one project generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticNodeId(pub u64);

/// Lexical scope key used to disambiguate bare-name lookups during
/// [`SemanticQueryKey::ResolveDecl`]. Two callers that reach the same name
/// from the same scope converge to the same cache entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeId {
    /// Canonical file id the declaration-lookup is rooted in. Carries the
    /// version indirectly via any [`ResolveDeclKey`] whose scope points to a
    /// specific `(canonical_id, whole_hash)` artifact.
    pub canonical_id: Arc<str>,
    /// Optional local scope index for inner scopes (lambda body, type-param
    /// scope, block scope). `None` means the file top-level scope. Only valid
    /// inside the lowered version of the owning canonical.
    pub local_scope: Option<u32>,
}

/// Source-scope sidecar tag for a [`SemanticNodeId`] (plan §7.10 + C1).
///
/// Every non-exempt interned node records the scope it was first interned in
/// so dispatch builders that need per-base-scope resolution (e.g. selecting
/// the correct [`SessionSolverHost`](crate::resolver_core::solver_host::SessionSolverHost)
/// for an `Instantiate` lookup) can reconstruct the originating scope without
/// threading it through every call.
///
/// - `Global` — fully structural nodes (primitives, shared literal-unions,
///   helper intermediates) that carry no scope-bound origin. The default
///   when a non-exempt node is interned via the unscoped
///   [`SemanticGraphStore::intern_node`](crate::semantic_query_memo::SemanticGraphStore::intern_node)
///   overload.
/// - `File { canonical_id, whole_hash, local_scope }` — declaration-origin
///   nodes (declaration anchors, instantiated shells, surface members when
///   their value carries a declaration identity, etc.).
///
/// **Exempt variants.** [`SemanticNodeData::VueMacroElements`] nodes do NOT
/// populate the sidecar — they live on the parser's refcount-only hot path
/// and are never consumed by dispatch builders that walk
/// [`SemanticGraphStore::node_scope`](crate::semantic_query_memo::SemanticGraphStore::node_scope).
/// For an exempt node id, `node_scope` returns `None` (no sidecar entry).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeScopeId {
    /// Structural / scope-less origin. Primitives, shared literal-unions,
    /// helper intermediates.
    Global,
    /// Declaration-bound origin. `canonical_id` names the owning file;
    /// `whole_hash` pins the file's content generation; `local_scope`
    /// optionally disambiguates inner scopes (block, lambda body,
    /// type-param scope).
    File {
        canonical_id: Arc<str>,
        whole_hash: HashValue,
        local_scope: Option<u32>,
    },
}

/// Resolved-declaration lookup key. Two callers from the same scope for the
/// same name produce the same key and dedup automatically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolveDeclKey {
    pub scope: ScopeId,
    pub name: Arc<str>,
}

/// Declaration identity for [`SemanticNodeData::TypeParam`] (Path C C6).
///
/// Distinguishes two unrelated `type A<T> = ...` and `type B<T> = ...`
/// declarations in the same source file. Pre-C6 the `TypeParam` variant
/// keyed on `name` alone, so identical parameter names across unrelated
/// declarations would collide structurally once C7 introduces interning.
/// C6 folds declaration identity (owning file + content generation +
/// declaring entity name) into the payload itself so the compound
/// `(payload, scope)` key in C7 gets a primary discriminator at the
/// payload level.
///
/// `decl_name` names the declaring entity — typically an interface,
/// type-alias, or class name, plus a script-setup sentinel
/// (`"<script-setup>"`) for `<script setup generic="T">` parameters,
/// and `"<synthetic>"` for test-only fixtures that construct
/// `TypeParam` nodes without a real declaration context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclIdentity {
    pub canonical_id: Arc<str>,
    pub whole_hash: HashValue,
    pub decl_name: Arc<str>,
}

impl DeclIdentity {
    /// Build a `DeclIdentity` from the lowering-site [`NodeScopeId`]
    /// plus an explicit declaring-entity name. For `NodeScopeId::Global`
    /// this produces a sentinel identity with empty canonical_id and
    /// `whole_hash == 0`; non-global scopes carry their file identity
    /// forward.
    #[must_use]
    pub fn from_scope(scope: &NodeScopeId, decl_name: Arc<str>) -> Self {
        match scope {
            NodeScopeId::Global => Self {
                canonical_id: Arc::from(""),
                whole_hash: HashValue::default(),
                decl_name,
            },
            NodeScopeId::File {
                canonical_id,
                whole_hash,
                ..
            } => Self {
                canonical_id: Arc::clone(canonical_id),
                whole_hash: *whole_hash,
                decl_name,
            },
        }
    }

    /// Build a synthetic identity for test-only `TypeParam`
    /// constructions that have no real declaration context. The
    /// `display_name` doubles as the `decl_name` so test fixtures that
    /// use distinct parameter names produce distinct identities.
    #[must_use]
    pub fn synthetic(display_name: &str) -> Self {
        Self {
            canonical_id: Arc::from("<synthetic>"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from(display_name),
        }
    }
}

/// A reference that is either a declaration identity (not interned in the
/// arena) or a concrete semantic node. Path C C16: declaration identity
/// is carried as `DeclIdentity` data, not as an interned node variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SemanticRef {
    Decl(DeclIdentity),
    Node(SemanticNodeId),
}

/// `typeof` operand key: a value-root identity that the resolver needs to
/// reach a type through a value binding. Kept separate from
/// [`ResolveDeclKey`] because the value-symbol space is distinct from the
/// type-symbol space.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueRootKey {
    pub scope: ScopeId,
    pub name: Arc<str>,
}

/// Optionality modifier for mapped-type `[K in ...]?` rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionalityMod {
    Add,
    Remove,
    Keep,
}

/// Readonly modifier for mapped-type `readonly [K in ...]` rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadonlyMod {
    Add,
    Remove,
    Keep,
}

/// Lowering-time classification of a [`MapperKey::value_expr`] (Path C C5).
///
/// `Identity` means the lowered `value_expr` is structurally the
/// `T[K]` projection of the mapped source — `IndexedAccess { object:
/// source_param_ref, index: TypeNode(mapper_param_ref) }` — so
/// `build_mapped_type` may read each source member's value directly
/// (the canonical `{ [K in keyof T]: T[K] }` pattern behind
/// `Partial<T>` / `Required<T>` / `Readonly<T>`). `Computed` means
/// the value is any other shape (computed projection, conditional
/// body, intersected helper) and must go through
/// `substitute_semantic_type_param` + `evaluate_deferred_semantic_node`.
///
/// Pre-C5, `build_mapped_type` ran the runtime helper
/// `mapper_value_is_identity_t_of_k` on every call. C5 hoists the
/// classification to lowering time so the build path matches on a
/// stable tag and the helper retires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapperKind {
    /// `value_expr` is structurally `IndexedAccess { object:
    /// source_param_ref, index: TypeNode(mapper_param_ref) }` and the
    /// build path may reuse `source_member.value` per key.
    Identity,
    /// `value_expr` is any other shape; the build path must
    /// substitute and evaluate.
    Computed,
}

impl MapperKind {
    /// Classify `value_expr` against `source` and `mapper_param` by
    /// checking whether it is structurally the identity projection
    /// `source[mapper_param]`. Used by the lowering path to tag each
    /// `MapperKey` at construction time.
    ///
    /// Matches two shapes after unwrapping a single `Alias`:
    /// - `IndexedAccess { object = source, index = TypeNode(id) }` where
    ///   `id` resolves (possibly through one `Alias`) to a
    ///   `TypeParam` node with the same name as `mapper_param`.
    /// - Anything else → [`MapperKind::Computed`].
    ///
    /// Ported verbatim from the retired `mapper_value_is_identity_t_of_k`
    /// helper (plan §2 Pass C5) so existing fast-path coverage is
    /// preserved.
    ///
    /// **Path C C6a item 7.** `mapper_param_node` is the mapper's
    /// binder node id (the `TypeParam` introduced by the enclosing
    /// `[K in ...]` binding). Classification checks that the
    /// indexed-access `index`'s type node equals this binder by
    /// `SemanticNodeId`, not by `display_name` string. This avoids
    /// conflating two binders that share a display name but are
    /// semantically distinct.
    #[must_use]
    pub fn classify_value_expr(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        value_expr: SemanticNodeId,
        source: SemanticNodeId,
        mapper_param_node: SemanticNodeId,
    ) -> Self {
        let mut value_id = value_expr;
        if let Some(SemanticNodeData::Alias(inner)) = graph.node_data(value_id).as_deref() {
            value_id = *inner;
        }
        let (object, index) = match graph.node_data(value_id).as_deref() {
            Some(SemanticNodeData::IndexedAccess { object, index }) => (*object, index.clone()),
            _ => return MapperKind::Computed,
        };
        if object != source {
            return MapperKind::Computed;
        }
        let mut index_id = match index {
            IndexKey::TypeNode(id) => id,
            _ => return MapperKind::Computed,
        };
        if let Some(SemanticNodeData::Alias(inner)) = graph.node_data(index_id).as_deref() {
            index_id = *inner;
        }
        if index_id == mapper_param_node {
            MapperKind::Identity
        } else {
            MapperKind::Computed
        }
    }
}

/// Mapper identity for mapped-type queries. Separates the key space from the
/// value expression so two mappers that share the same key space but differ
/// in the value expression do not alias.
///
/// **Path C C6a item 6.** `parameter_node` carries the mapper's
/// binder identity as the interned `TypeParam`'s [`SemanticNodeId`]
/// (rather than the pre-C6a `parameter: Arc<str>` display name).
/// Binder matching across substitute/classify paths is now node-id
/// equality, so two distinct mapped binders sharing a display name
/// no longer conflate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapperKey {
    /// The binder's interned `TypeParam` node id (Path C C6a item 6).
    pub parameter_node: SemanticNodeId,
    pub key_space: SemanticNodeId,
    pub value_expr: SemanticNodeId,
    pub optionality: OptionalityMod,
    pub readonly: ReadonlyMod,
    /// Optional `as` clause remapping the key.
    pub name_remap: Option<SemanticNodeId>,
    /// Path C C5 classification of `value_expr`. See [`MapperKind`].
    pub kind: MapperKind,
}

/// Indexed-access key operand. Mirrors the three TypeScript forms:
/// `T[K]` where `K` is a literal string, a literal number, or another type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IndexKey {
    String(Arc<str>),
    Number(i64),
    TypeNode(SemanticNodeId),
}

/// Projection mode for member projection and indexed access.
///
/// - `Identity`: return the resolved declaration identity only (no shape).
/// - `Navigate`: walk into the surface to pick the next hop without expanding
///   siblings — used by intermediate hops in a `A['c']['full']['bar']` path.
/// - `Shallow`: expose one level of surface members but do not recurse into
///   them.
/// - `Expanded`: recursively materialize the requested projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionMode {
    Identity,
    Navigate,
    Shallow,
    Expanded,
}

/// One hop in a navigation path. Used by [`TypeNavigator::choose_next_hop`]
/// and as the segment list in [`SemanticQueryKey::ProjectPath`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Member(Arc<str>),
    Index(IndexKey),
}

/// Navigator decision at a hop. Navigators never resolve new semantic nodes
/// privately; `EnterQuery` hands the key back to
/// [`SemanticQueryApi::execute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopDecision {
    /// Continue navigation within the already-resolved node.
    Continue(SemanticNodeId),
    /// A new semantic node is required — ask the shared query API to produce it.
    EnterQuery(SemanticQueryKey),
    /// Navigation is complete.
    Done,
}

/// One member of a [`SurfaceView`]. Members carry the full TypeScript
/// member metadata that consumers downstream of dispatch need (component-meta,
/// LSP hover, etc.) so no parallel "projected member" type has to exist.
///
/// `value` is a reference-style [`SemanticNodeId`] under the lazy-materialisation
/// rule (plan §2): the member's body is **not** eagerly expanded when the
/// owning surface is interned. A walker that needs the body issues
/// [`SemanticQueryApi::execute`] with a [`SemanticQueryKey::ProjectPath`]
/// rooted at `value`; the family memo dedups across distinct entry paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SurfaceMember {
    pub name: Arc<str>,
    pub value: SemanticNodeId,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
}

/// One index signature (`{ [K: K_T]: V_T }` or `{ readonly [K: K_T]: V_T }`)
/// on a surface. Carried as structured metadata rather than an opaque
/// [`SemanticNodeId`] so consumers downstream of dispatch can read the
/// declared `key_type` / `value_type` shape directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexSignature {
    pub key_type: SemanticNodeId,
    pub value_type: SemanticNodeId,
    pub readonly: bool,
}

/// One-level surface view of a semantic node. Members are ordered to keep
/// hashing stable.
///
/// Extended in B1a (plan §2 lazy-materialisation block) to carry the full
/// member + signature metadata previously held by the soon-to-be-retired
/// `ProjectedMember` / `ProjectedSurface` / `ProjectedKeyspace` types in
/// `verter_semantic::analysis::type_solver::query_engine`. Consumers should
/// read these fields directly instead of going through the legacy projected
/// types, which retire in D3 alongside `TypeSurfaceDb`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceView {
    pub members: Arc<[SurfaceMember]>,
    pub call_signatures: Arc<[SemanticNodeId]>,
    pub construct_signatures: Arc<[SemanticNodeId]>,
    pub index_signatures: Arc<[IndexSignature]>,
    pub keyspace: Option<SemanticNodeId>,
    pub has_index_signature: bool,
}

impl std::hash::Hash for SurfaceView {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for member in self.members.iter() {
            member.hash(state);
        }
        self.call_signatures.hash(state);
        self.construct_signatures.hash(state);
        self.index_signatures.hash(state);
        self.keyspace.hash(state);
        self.has_index_signature.hash(state);
    }
}

/// Dependency-version variant recorded alongside each cache read.
///
/// Warm hits return a recorded `DepSignature` that merges into the active
/// [`CompletionFence`](crate::completion_fence::CompletionFence), so
/// final-result validation is transitive, not root-key-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DepVersion {
    WholeHash(HashValue),
    RouteGeneration(u64),
    ProjectGeneration(u64),
}

/// Dependency signature returned alongside every reusable cache read.
pub type DepSignature = Arc<[(Arc<str>, DepVersion)]>;

/// Returned by cache reads so callers can merge transitive dep facts into the
/// active [`CompletionFence`](crate::completion_fence::CompletionFence).
#[derive(Debug, Clone)]
pub struct CacheRead<T> {
    pub value: T,
    pub dep_signature: DepSignature,
}

// ──────────────────────────────────────────────────────────────────────────
// Derivation / origin layer (plan B2 + Derivation/Origin Layer Contract)
// ──────────────────────────────────────────────────────────────────────────

/// Required edge kinds for the derivation/origin layer (plan §2 Derivation
/// + Origin Layer Contract). Names are normative — semantics MUST NOT drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginEdgeKind {
    /// `result = decl<args>`. From the instantiated result back to the
    /// declaration identity and concrete argument nodes.
    Instantiate,
    /// `result_position = T -> V`. From a concrete type in a substituted
    /// position back to the declaration's type parameter and the binding
    /// that produced the substitution.
    SubstituteTypeParam,
    /// `result = select(conditional, True | False | Deferred)`. Records
    /// the branch taken (or `Deferred` if the check stayed open).
    ConditionalSelect,
    /// `result = T bound via infer`. From the inferred type back to the
    /// `infer` binding site and the concrete type captured by the
    /// relation check.
    InferBind,
    /// `result = base.member`. From the projected member back to the
    /// base node and the member name.
    ProjectMember,
    /// `result = base[index]`. From the projected result back to the
    /// base node and the index expression.
    ProjectIndex,
    /// `result = base.path...`. From the projected result back to the
    /// base node and the full path segment list.
    ProjectPath,
    /// `result = normalize(source_members)`. From a normalized union /
    /// intersection / simplified result back to each contributing member
    /// node.
    Normalize,
    /// `result = unwrap(alias)`. From the unwrapped-target result node
    /// back to the alias declaration identity. Emitted once per alias hop
    /// (direct alias, re-export alias, barrel alias). Chains are walkable
    /// end-to-end.
    AliasResolve,
}

/// Branch decision recorded on a [`OriginEdgeKind::ConditionalSelect`] edge.
/// `Deferred` covers the open-conditional case where the path projection
/// distributes into both branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BranchSelection {
    True,
    False,
    Deferred,
}

/// Per-edge metadata payload. Carries the variant-specific scalars an edge
/// needs to be self-describing (the branch selected, the member name
/// projected, the path walked, etc.) without inflating the
/// [`OriginEdgeKind`] discriminant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginMeta {
    /// No payload beyond the edge kind itself.
    None,
    /// Branch decision for [`OriginEdgeKind::ConditionalSelect`].
    Branch(BranchSelection),
    /// Member name for [`OriginEdgeKind::ProjectMember`] /
    /// [`OriginEdgeKind::AliasResolve`].
    MemberName(Arc<str>),
    /// Index expression for [`OriginEdgeKind::ProjectIndex`].
    Index(IndexKey),
    /// Full path segment list for [`OriginEdgeKind::ProjectPath`].
    Path(Arc<[PathSegment]>),
    /// Type-parameter name for [`OriginEdgeKind::SubstituteTypeParam`].
    SubstitutedParam(Arc<str>),
}

/// One origin edge: a derivation hop from a source-set to a result. Carries
/// the per-edge dep-signature snapshot that walkers merge into their fence
/// at hop-time (plan §7.16 — edges are the only dep-sig propagation route
/// for builders).
#[derive(Debug, Clone)]
pub struct OriginEdge {
    /// Source nodes this edge derives from (typically 1; multiple for
    /// `Normalize` over union/intersection arms).
    pub sources: Arc<[SemanticNodeId]>,
    /// Variant-specific scalar metadata.
    pub meta: OriginMeta,
    /// Snapshot of the publishing builder's active fence at the moment the
    /// edge was committed. Interned by [`crate::semantic_query_memo`] so
    /// builders that emit dozens of edges with identical fences share one
    /// `Arc` allocation.
    pub edge_dep_signature: Arc<DepSignature>,
}

// ──────────────────────────────────────────────────────────────────────────
// Telemetry — first-class observability surface (plan B2 + §7.4)
// ──────────────────────────────────────────────────────────────────────────

/// Public snapshot of the semantic graph store's telemetry counters. Read
/// from `SemanticGraphStore::stats_snapshot()`. Per `SemanticGraphKey`
/// variant counters are aggregated; per-builder counters are aggregated;
/// path / projection / origin-edge metrics are reduced to p50 / p95 from
/// host-side reservoir-sampled histograms (cap 8192 samples per metric).
///
/// Field set is normative against plan §6.3 — every field listed there
/// (expected-to-fire + exceptional-path) must appear here, and every
/// field here must appear in one of those lists. F2's
/// `counter_taxonomy_matches_plan` test enforces the bidirectional
/// equality.
///
/// Snapshots are immutable and safe to read mid-request. The trace-check
/// harness, the benchmark pipeline, and the F3 corpus benchmark consume
/// these snapshots directly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticGraphStats {
    // ── Memo / coordination counters ────────────────────────────────
    pub hits: u64,
    pub misses: u64,
    pub same_path_sentinel_returns: u64,
    pub in_flight_peak: u32,
    pub waits_ms: u64,
    pub memo_entry_count: u64,
    // ── Derivation / origin counters ─────────────────────────────────
    pub origin_edge_count: u64,
    pub origin_edges_emitted: u64,
    pub origin_edges_per_node_p50: u32,
    pub origin_edges_per_node_p95: u32,
    // ── Per-builder counters (incremented by builders in C-phase) ───
    pub instantiate_count: u64,
    pub conditional_decided_count: u64,
    pub conditional_deferred_count: u64,
    pub branch_selections_true: u64,
    pub branch_selections_false: u64,
    pub budget_fallback_count: u64,
    // ── Path / projection histogram percentiles (B2 baseline:
    // reservoir-sampled by C-phase builders; F3 corpus consumes p50/p95).
    pub path_length_p50: u32,
    pub path_length_p95: u32,
    pub projection_depth_p50: u32,
    pub projection_depth_p95: u32,
}

/// Structured query-level failure. Distinct from panics — callers decide
/// whether an error maps to a top-level public failure or to an opaque
/// semantic node in the result.
#[derive(Debug, Clone)]
pub enum QueryError {
    /// No cache entry and no cold build succeeded.
    Miss,
    /// A declaration resolved to `= intrinsic` but the active TS SDK advertises
    /// an intrinsic the verter intrinsic registry does not implement.
    UnsupportedIntrinsic { name: Arc<str> },
    /// The resolver hit one of its structured safety rails.
    BudgetExceeded(BudgetExceededFailure),
    /// The completion fence exhausted its retry budget (default: 3).
    UnstableState { attempts: u8 },
    /// The path walker re-entered an alias it had already visited on the
    /// same `build_project_path` invocation (plan §3 C3). Carries the
    /// cycle participants so diagnostics can render the chain end-to-end.
    AliasCycle { chain: Arc<[Arc<str>]> },
    /// Recursive back-edge sentinel emitted when dispatch detects a
    /// declaration expanding into itself (e.g., `type TreeNode = {
    /// children: TreeNode[] }`). `semantic_node_to_type_expr` converts
    /// this to [`TypeExpr::RecursiveRef`] so the materialiser stops at
    /// the back-edge instead of recursing indefinitely. Plan §5.8 /
    /// plan §1.4 recursion handling.
    RecursiveRef { name: Arc<str> },
    /// Catch-all for text-bearing failures surfaced to the caller.
    Other(Arc<str>),
    /// C16: Declaration resolved but not yet materialized. The node
    /// carries the file scope sidecar so callers can construct a
    /// `DeclIdentity` for `Instantiate` keys. Walk/enumerate code
    /// treats this as "expandable via Instantiate" rather than "not found."
    DeclPlaceholder {
        canonical_id: Arc<str>,
        name: Arc<str>,
        whole_hash: HashValue,
    },
}

// Path C C7 — `SemanticNodeData` structurally interns under a compound
// `(payload, scope)` key, so every field transitively reachable from a
// variant must implement `Hash` / `Eq`. `QueryError::BudgetExceeded`
// wraps `BudgetExceededFailure` which has no `Hash`/`Eq`; treat all
// `BudgetExceeded` carriers as one identity for interning purposes (they
// are opaque error tokens, not structural distinguishers). Other variants
// compare by their fields.
impl PartialEq for QueryError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Miss, Self::Miss) => true,
            (Self::UnsupportedIntrinsic { name: a }, Self::UnsupportedIntrinsic { name: b }) => {
                a == b
            }
            (Self::BudgetExceeded(_), Self::BudgetExceeded(_)) => true,
            (Self::UnstableState { attempts: a }, Self::UnstableState { attempts: b }) => a == b,
            (Self::AliasCycle { chain: a }, Self::AliasCycle { chain: b }) => a == b,
            (Self::RecursiveRef { name: a }, Self::RecursiveRef { name: b }) => a == b,
            (Self::Other(a), Self::Other(b)) => a == b,
            (
                Self::DeclPlaceholder {
                    canonical_id: a_c,
                    name: a_n,
                    whole_hash: a_h,
                },
                Self::DeclPlaceholder {
                    canonical_id: b_c,
                    name: b_n,
                    whole_hash: b_h,
                },
            ) => a_c == b_c && a_n == b_n && a_h == b_h,
            _ => false,
        }
    }
}

impl Eq for QueryError {}

impl std::hash::Hash for QueryError {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Discriminant tag, then per-variant field hashing. BudgetExceeded
        // hashes tag-only because its payload is an opaque carrier.
        match self {
            Self::Miss => {
                0u8.hash(state);
            }
            Self::UnsupportedIntrinsic { name } => {
                1u8.hash(state);
                name.hash(state);
            }
            Self::BudgetExceeded(_) => {
                2u8.hash(state);
            }
            Self::UnstableState { attempts } => {
                3u8.hash(state);
                attempts.hash(state);
            }
            Self::AliasCycle { chain } => {
                4u8.hash(state);
                chain.hash(state);
            }
            Self::RecursiveRef { name } => {
                5u8.hash(state);
                name.hash(state);
            }
            Self::Other(msg) => {
                6u8.hash(state);
                msg.hash(state);
            }
            Self::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            } => {
                7u8.hash(state);
                canonical_id.hash(state);
                name.hash(state);
                whole_hash.hash(state);
            }
        }
    }
}

/// Query-level execution result. `Recursive` is a query-local placeholder for
/// a same-path recursion sentinel; it must never be published as a finalized
/// shared cache entry.
#[derive(Debug, Clone)]
pub enum QueryResult<T> {
    Value(T),
    Recursive(SemanticNodeId),
    Error(QueryError),
}

// ──────────────────────────────────────────────────────────────────────────
// Canonical semantic key surface
// ──────────────────────────────────────────────────────────────────────────

/// Canonical semantic query key. Every reusable type-resolution operation
/// dispatches through [`SemanticQueryApi::execute`] with one of these
/// variants; two callers that mean the same thing produce the same key and
/// share one cache entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SemanticQueryKey {
    ResolveDecl(ResolveDeclKey),
    Instantiate {
        base: DeclIdentity,
        args: Arc<[SemanticNodeId]>,
    },
    ProjectMember {
        base: SemanticNodeId,
        member: Arc<str>,
        mode: ProjectionMode,
    },
    IndexedAccess {
        base: SemanticNodeId,
        index: IndexKey,
        mode: ProjectionMode,
    },
    KeyOf {
        base: SemanticNodeId,
    },
    MappedType {
        source: SemanticNodeId,
        mapper: MapperKey,
    },
    Conditional {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    },
    TypeOf {
        value_root: ValueRootKey,
    },
    NormalizeUnion {
        members: Arc<[SemanticNodeId]>,
    },
    NormalizeIntersection {
        members: Arc<[SemanticNodeId]>,
    },
    /// Path-precise projection rooted at `base` and walking each
    /// [`PathSegment`] in `path`. The empty-path form (`path: Arc::from([])`)
    /// is the canonical shape of "expand the whole surface" — the retired
    /// `Expand` variant collapses into this. Single-hop `ProjectMember` /
    /// `IndexedAccess` variants are admission-canonicalised to the equivalent
    /// length-1 `ProjectPath` so sugar and canonical hit the same memo entry.
    ProjectPath {
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        mode: ProjectionMode,
    },
    /// Identity for a Vue macro resolution artifact cached in the shared
    /// semantic graph under a [`HostResolvedNamedTypeKey`].
    ///
    /// This key is read-dominant: hot-path lookups go through
    /// [`SemanticGraphStore::get_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::get_resolved_named_type)
    /// directly so the parser's named-type cache stays refcount-only. The
    /// formal [`SemanticQueryApi::execute`] entry point returns
    /// [`QueryError::Miss`] when the key has not been written (writes come
    /// from the [`NamedTypeCache`](verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache)
    /// adapter side, not from `execute`).
    ///
    /// Wrapping the key in `Arc` keeps equality / hashing cheap because the
    /// inner key already carries `Arc<str>` and `Arc<[…]>` allocations — we
    /// move the key behind one more refcount so clones during key
    /// construction do not deep-copy the contained slices.
    ResolvedNamedType {
        key: Arc<HostResolvedNamedTypeKey>,
    },
    /// Assignability / relation query between `source` and `target` semantic
    /// nodes (plan §2 Relation engine + §3 Change S). Dispatches through
    /// `ProjectSemanticDispatch::relate_nodes` and memoises the result in
    /// [`SemanticGraphStore::relation_memo`](crate::semantic_query_memo::SemanticGraphStore)
    /// with dep-signature fencing. Added in Phase D §5.4 WIP-S.
    Relate {
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
}

/// One inference binding produced by a successful `Relate` judgement (plan
/// §2 Relation engine). When a conditional type's check matches against
/// `infer`-bearing extends, the solver binds each `infer T` to a concrete
/// substituted type; those bindings flow into the true branch.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferBinding {
    pub name: Arc<str>,
    pub bound: SemanticNodeId,
}

/// Tri-state result of a `Relate` judgement (plan §2 Relation engine).
///
/// All three variants memoise with dep-signature fencing — `Unknown` is
/// included so repeated cyclic re-entry short-circuits without recomputing
/// the undecidable judgement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RelationResult {
    Assignable { bindings: Arc<[InferBinding]> },
    NotAssignable,
    Unknown,
}

/// One element of a [`SemanticNodeData::Tuple`] shell (plan §3 B4 + §7.14).
///
/// Preserves the declaration-site metadata the dispatcher needs to render
/// tuples correctly: the optional label, whether the slot is optional (`?`),
/// and whether the slot is a rest element (`...T`). `value` points at a
/// regular [`SemanticNodeId`] under the lazy-materialisation rule — the
/// element's body is not eagerly recursed into when the owning tuple shell
/// is interned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TupleElement {
    pub label: Option<Arc<str>>,
    pub value: SemanticNodeId,
    pub optional: bool,
    pub rest: bool,
}

/// Immutable semantic-node payload. Storage for this enum is owned by the
/// shared semantic graph; node ids hand out views without copying.
///
/// **Publication boundary (plan §7.14).** Solver `Infer`, `Rest`, and
/// `RecursiveRef` nodes are scratch-only and never enter this enum. Solver
/// `Error` values publish at the boundary as [`SemanticNodeData::Opaque`]
/// carrying the concrete [`QueryError`]. Functions publish through
/// [`SemanticNodeData::Object`] with empty `members` and populated
/// `call_signatures` / `construct_signatures` — there is no dedicated
/// `Function` variant.
#[derive(Debug, Clone)]
pub enum SemanticNodeData {
    Alias(SemanticNodeId),
    Object(SurfaceView),
    Union(Arc<[SemanticNodeId]>),
    Intersection(Arc<[SemanticNodeId]>),
    Primitive(PrimitiveKind),
    /// Literal-value carrier. Preserves exact literal identity
    /// (`"idle"`, `42`, `true`) so unions of literals don't collapse
    /// into their broader primitive kind. This is the semantic-graph
    /// equivalent of the solver-arena [`Node::Literal`]
    /// (see [`verter_semantic::analysis::type_solver::arena::SolverLiteral`]).
    ///
    /// Matching discipline: consumers that previously matched
    /// `Primitive(String)` for any string-like node should now match
    /// BOTH `Primitive(String)` AND `Literal(String(_))` when they want
    /// to admit any string-shaped node. The two are distinct semantic
    /// classes: `Primitive(String)` is the broad string kind (all
    /// strings), `Literal(String("idle"))` is the specific literal.
    Literal(LiteralValue),
    Opaque(QueryError),
    /// Array shell (plan §3 B4 + §7.14). Publishes `T[]` / `Array<T>` /
    /// `ReadonlyArray<T>` directly rather than routing through generic
    /// `Array<T>` declaration instantiation — array indexed-access is
    /// hot and must not pay generic-instantiation + prototype-surface
    /// cost on every access. `element` is a regular [`SemanticNodeId`]
    /// and may be lazily materialised (the element's body expands only
    /// when a caller's path projects into it).
    Array {
        element: SemanticNodeId,
        readonly: bool,
    },
    /// Tuple shell (plan §3 B4 + §7.14). Preserves per-element label /
    /// optionality / rest metadata so consumers can render tuples without
    /// going through a side channel. Each element's `value` is a regular
    /// [`SemanticNodeId`] under the lazy-materialisation rule.
    Tuple {
        elements: Arc<[TupleElement]>,
        readonly: bool,
    },
    /// Template-literal shell (plan §3 B4 + §7.14). Carries the
    /// alternating quasi text spans and expression references verbatim
    /// from the parser's [`TypeExpr::TemplateLiteral`] shape. Relation-
    /// engine support for infer-heavy template matching remains a
    /// separate follow-up (plan §1.4) — the shell carrier itself is not
    /// deferred.
    TemplateLiteral {
        quasis: Arc<[Arc<str>]>,
        expressions: Arc<[SemanticNodeId]>,
    },
    /// Deferred `keyof` shell used when the operand still carries open
    /// type-parameter structure and cannot be collapsed to a concrete key
    /// union yet.
    KeyOf {
        base: SemanticNodeId,
    },
    /// Deferred indexed-access shell used when the object / index pair still
    /// depends on open generic structure and must survive until a later
    /// substitution or path-projection step.
    IndexedAccess {
        object: SemanticNodeId,
        index: IndexKey,
    },
    /// Deferred mapped-type shell used when the solver/bridge needs to
    /// preserve `{ [K in Source]: Value }` structure across the dispatch
    /// boundary without eagerly materialising the produced surface.
    ///
    /// `source` is the underlying mapped source passed to
    /// [`SemanticQueryKey::MappedType`]; the original key-space
    /// expression lives on `mapper.key_space` so callers can reconstruct
    /// the surface syntax for materialisation/serialization.
    Mapped {
        source: SemanticNodeId,
        mapper: MapperKey,
    },
    /// Deferred `typeof` shell used when the bridge needs to carry a
    /// value-rooted lookup plus any remaining member path segments as a
    /// first-class semantic node.
    ///
    /// `value_root` is the mode-free `typeof` query identity; `path`
    /// stores the remaining dotted member segments that must be
    /// projected from that root.
    TypeOf {
        value_root: ValueRootKey,
        path: Arc<[Arc<str>]>,
    },
    TypeParam {
        /// Declaration identity (Path C C6). Distinguishes cross-
        /// declaration same-name parameters (`type A<T>` vs
        /// `type B<T>` in the same file) so structural interning in
        /// C7 does not collide unrelated decls.
        decl: DeclIdentity,
        /// Position in the declaration's generic clause (0-based).
        /// Path C C6 sets this to `0` by default; downstream
        /// plumbing through the lowering pipeline can refine it to
        /// the true clause position.
        param_index: u16,
        /// Declaration-site constraint (`T extends Constraint`). Lowered
        /// once at declaration time; callers substituting the TypeParam
        /// do NOT re-substitute constraint or default (those carry
        /// declaration-local meaning, not call-site meaning — plan §3
        /// Cluster A + anti-pattern #4 structural basis).
        constraint: Option<SemanticNodeId>,
        /// Declaration-site default (`T = Default`). Same contract as
        /// `constraint`.
        default: Option<SemanticNodeId>,
        /// Human-readable parameter name for `Debug` / error output.
        /// Path C C6 excludes this from `Hash`/`Eq` identity so two
        /// calls that construct the same declaration-identity
        /// TypeParam with the same constraint/default/index but
        /// different display names intern to the same slot. In
        /// practice `display_name` is consistent with `decl +
        /// param_index`; the exclusion is defensive.
        display_name: Arc<str>,
    },
    /// `infer X` placeholder inside a conditional's `extends` clause
    /// (plan §3 Cluster A). Modelled as an explicit variant rather than
    /// encoded via scope overloading (rejects anti-pattern #3 — scope-
    /// as-discriminator). The `name` is the infer binding name the
    /// `true` branch will substitute the bound type into.
    Infer {
        name: Arc<str>,
    },
    // DeclAnchor variant retired in Path C C16. Declaration identity is now
    // carried as `DeclIdentity` in `SemanticQueryKey::Instantiate.base`
    // instead of being interned as a node in the arena.
    /// Conditional shell node (plan §3 C2 + §2 lazy block).
    ///
    /// Carries the `check extends extends ? true_branch_ref : false_branch_ref`
    /// structure without recursively materialising either branch. Produced
    /// by [`SemanticQueryKey::Conditional`] when the relation engine's
    /// judgement is undecidable (open check) or when the caller asked for
    /// the structural shell without branch selection.
    ///
    /// - `check` / `extends` — the two sides of the conditional test.
    /// - `true_branch_ref` / `false_branch_ref` — shell-level references
    ///   to each branch. Not pre-expanded; a walker that projects into
    ///   the branch materialises its body via
    ///   [`SemanticQueryKey::ProjectPath`] sub-queries (C3).
    /// - `distributive` — `true` when `check` is a naked type parameter
    ///   (TS distributive-conditional semantics).
    ///
    /// When the relation engine decides the check (closed case),
    /// [`build_conditional`](crate::project_semantic_dispatch::ProjectSemanticDispatch)
    /// returns the selected branch ref directly and emits a
    /// [`OriginEdgeKind::ConditionalSelect`] edge with
    /// [`BranchSelection::True`] or [`BranchSelection::False`] — no
    /// `Conditional` node is interned. The interned `Conditional` node
    /// variant only appears for undecidable (deferred) checks.
    Conditional {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch_ref: SemanticNodeId,
        false_branch_ref: SemanticNodeId,
        distributive: bool,
    },
    /// Pragmatic carrier for Vue macro resolution artifacts (spans, text,
    /// prop/emit metadata) produced by the parser's cross-file type resolver
    /// via the [`NamedTypeCache`](verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache)
    /// trait.
    ///
    /// This variant is an **interim** shape: Vue codegen consumers (props /
    /// emits / defineModel) still drive from the concrete `ResolvedElements`
    /// struct rather than from a pure `SemanticNodeData` surface. Folding
    /// `ResolvedElements` behind a `SemanticNodeId` keeps the shared semantic
    /// graph as the single storage / identity backbone while we migrate
    /// those consumers off the direct struct. The cache entries are
    /// whole-hash-scoped (see
    /// [`HostResolvedNamedTypeKey`]), so reads are self-validating within
    /// one project generation.
    VueMacroElements(Arc<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>),
    /// Phase D §5.6 WIP-L — function shape (plan §2 architectural decision).
    ///
    /// Classes / interfaces lower to `SemanticNodeData::Object` with heritage
    /// merged; only function signatures have distinct semantics (call
    /// signatures, parameter variance, return covariance) that Object cannot
    /// represent.
    Function {
        params: Arc<[FunctionParam]>,
        return_type: SemanticNodeId,
        type_parameters: Arc<[TypeParamDecl]>,
    },
}

impl SemanticNodeData {
    /// Stable discriminant index used by Path C C1 instrumentation
    /// (per `/tmp/d-cutover-path-c-full-architectural-cleanup.md` §2 Stage 1)
    /// to bucket per-variant push counts on
    /// [`crate::types::MetaProvenance::node_arena_pushes_per_discriminant`].
    ///
    /// Values are independent of the variant declaration order so that
    /// variant additions / removals (e.g. C16 retiring `DeclAnchor`) do not
    /// renumber unrelated buckets. The returned index must stay below
    /// [`crate::types::SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT`].
    #[must_use]
    pub fn discriminant_index(&self) -> usize {
        match self {
            Self::Alias(_) => 0,
            Self::Object(_) => 1,
            Self::Union(_) => 2,
            Self::Intersection(_) => 3,
            Self::Primitive(_) => 4,
            Self::Literal(_) => 5,
            Self::Opaque(_) => 6,
            Self::Array { .. } => 7,
            Self::Tuple { .. } => 8,
            Self::TemplateLiteral { .. } => 9,
            Self::KeyOf { .. } => 10,
            Self::IndexedAccess { .. } => 11,
            Self::Mapped { .. } => 12,
            Self::TypeOf { .. } => 13,
            Self::TypeParam { .. } => 14,
            Self::Infer { .. } => 15,
            Self::Conditional { .. } => 17,
            Self::VueMacroElements(_) => 18,
            Self::Function { .. } => 19,
        }
    }
}

// Path C C7 — structural interning in `NodeArena` keys on
// `(SemanticNodeData, NodeScopeId)`. Manual `Hash`/`Eq`/`PartialEq`
// rather than a derive because:
//
// - **TypeParam** identity excludes `display_name` per plan §14.2 F11.
//   `decl + param_index` (with `constraint` / `default`) is the
//   semantic identity; `display_name` is a presentational field used
//   for Debug output and error messages. Two `TypeParam` nodes with
//   matching identity but differing `display_name` must alias under
//   C7 dedup.
// - **VueMacroElements** is an identity-carrier with
//   latest-insert-wins semantics (see `SemanticGraphStore::insert_resolved_named_type`
//   at [`semantic_query_memo.rs:287-301`]). Equality and hashing are
//   `Arc::as_ptr`-based so two calls that wrap the *same* `Arc` alias,
//   but any structurally-identical-but-Arc-distinct pair stays
//   distinct — preserving the invariant that separate inserts under
//   the same `HostResolvedNamedTypeKey` still allocate fresh arena
//   slots and never collide with prior payloads even when the inner
//   `ResolvedElements` value happens to be structurally equal.
//
// Other variants compare / hash by their field values. The
// discriminant tag is mixed into the hash so two variants with
// structurally-similar payload (`KeyOf { base }` vs `Alias(base)`)
// do not collide.
impl PartialEq for SemanticNodeData {
    fn eq(&self, other: &Self) -> bool {
        if self.discriminant_index() != other.discriminant_index() {
            return false;
        }
        match (self, other) {
            (Self::Alias(a), Self::Alias(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Union(a), Self::Union(b)) => a == b,
            (Self::Intersection(a), Self::Intersection(b)) => a == b,
            (Self::Primitive(a), Self::Primitive(b)) => a == b,
            (Self::Literal(a), Self::Literal(b)) => a == b,
            (Self::Opaque(a), Self::Opaque(b)) => a == b,
            (
                Self::Array {
                    element: a,
                    readonly: ar,
                },
                Self::Array {
                    element: b,
                    readonly: br,
                },
            ) => a == b && ar == br,
            (
                Self::Tuple {
                    elements: a,
                    readonly: ar,
                },
                Self::Tuple {
                    elements: b,
                    readonly: br,
                },
            ) => a == b && ar == br,
            (
                Self::TemplateLiteral {
                    quasis: aq,
                    expressions: ae,
                },
                Self::TemplateLiteral {
                    quasis: bq,
                    expressions: be,
                },
            ) => aq == bq && ae == be,
            (Self::KeyOf { base: a }, Self::KeyOf { base: b }) => a == b,
            (
                Self::IndexedAccess {
                    object: ao,
                    index: ai,
                },
                Self::IndexedAccess {
                    object: bo,
                    index: bi,
                },
            ) => ao == bo && ai == bi,
            (
                Self::Mapped {
                    source: asrc,
                    mapper: am,
                },
                Self::Mapped {
                    source: bsrc,
                    mapper: bm,
                },
            ) => asrc == bsrc && am == bm,
            (
                Self::TypeOf {
                    value_root: ar,
                    path: ap,
                },
                Self::TypeOf {
                    value_root: br,
                    path: bp,
                },
            ) => ar == br && ap == bp,
            (
                Self::TypeParam {
                    decl: ad,
                    param_index: ai,
                    constraint: ac,
                    default: adf,
                    // `display_name` intentionally excluded per plan §14.2 F11.
                    display_name: _,
                },
                Self::TypeParam {
                    decl: bd,
                    param_index: bi,
                    constraint: bc,
                    default: bdf,
                    display_name: _,
                },
            ) => ad == bd && ai == bi && ac == bc && adf == bdf,
            (Self::Infer { name: a }, Self::Infer { name: b }) => a == b,
            (
                Self::Conditional {
                    check: ack,
                    extends: aex,
                    true_branch_ref: atr,
                    false_branch_ref: afr,
                    distributive: ad,
                },
                Self::Conditional {
                    check: bck,
                    extends: bex,
                    true_branch_ref: btr,
                    false_branch_ref: bfr,
                    distributive: bd,
                },
            ) => ack == bck && aex == bex && atr == btr && afr == bfr && ad == bd,
            (Self::VueMacroElements(a), Self::VueMacroElements(b)) => Arc::ptr_eq(a, b),
            (
                Self::Function {
                    params: ap,
                    return_type: ar,
                    type_parameters: atp,
                },
                Self::Function {
                    params: bp,
                    return_type: br,
                    type_parameters: btp,
                },
            ) => ap == bp && ar == br && atp == btp,
            _ => false,
        }
    }
}

impl Eq for SemanticNodeData {}

impl std::hash::Hash for SemanticNodeData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Discriminant tag first so two variants with structurally-similar
        // payloads (e.g. `Alias(id)` vs `KeyOf { base: id }`) cannot collide.
        (self.discriminant_index() as u8).hash(state);
        match self {
            Self::Alias(inner) => {
                inner.hash(state);
            }
            Self::Object(surface) => {
                surface.hash(state);
            }
            Self::Union(members) => {
                members.hash(state);
            }
            Self::Intersection(members) => {
                members.hash(state);
            }
            Self::Primitive(kind) => {
                kind.hash(state);
            }
            Self::Literal(value) => {
                value.hash(state);
            }
            Self::Opaque(err) => {
                err.hash(state);
            }
            Self::Array { element, readonly } => {
                element.hash(state);
                readonly.hash(state);
            }
            Self::Tuple { elements, readonly } => {
                elements.hash(state);
                readonly.hash(state);
            }
            Self::TemplateLiteral {
                quasis,
                expressions,
            } => {
                quasis.hash(state);
                expressions.hash(state);
            }
            Self::KeyOf { base } => {
                base.hash(state);
            }
            Self::IndexedAccess { object, index } => {
                object.hash(state);
                index.hash(state);
            }
            Self::Mapped { source, mapper } => {
                source.hash(state);
                mapper.hash(state);
            }
            Self::TypeOf { value_root, path } => {
                value_root.hash(state);
                path.hash(state);
            }
            Self::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                // `display_name` intentionally excluded per plan §14.2 F11.
                display_name: _,
            } => {
                decl.hash(state);
                param_index.hash(state);
                constraint.hash(state);
                default.hash(state);
            }
            Self::Infer { name } => {
                name.hash(state);
            }
            Self::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                check.hash(state);
                extends.hash(state);
                true_branch_ref.hash(state);
                false_branch_ref.hash(state);
                distributive.hash(state);
            }
            Self::VueMacroElements(elements) => {
                // Identity-carrier: hash on `Arc::as_ptr` so two calls with
                // distinct `Arc` allocations (fresh inserts under the same
                // `HostResolvedNamedTypeKey`) produce distinct hashes.
                (Arc::as_ptr(elements) as usize).hash(state);
            }
            Self::Function {
                params,
                return_type,
                type_parameters,
            } => {
                params.hash(state);
                return_type.hash(state);
                type_parameters.hash(state);
            }
        }
    }
}

/// Parameter of a [`SemanticNodeData::Function`] (plan §2 + §3 Change L).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParam {
    pub name: Option<Arc<str>>,
    pub ty: SemanticNodeId,
    pub optional: bool,
    pub rest: bool,
}

/// Type-parameter declaration on a [`SemanticNodeData::Function`] (plan §2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeParamDecl {
    pub name: Arc<str>,
    pub constraint: Option<SemanticNodeId>,
    pub default: Option<SemanticNodeId>,
}

// ──────────────────────────────────────────────────────────────────────────
// Vue macro resolution — host-owned cache identity
// ──────────────────────────────────────────────────────────────────────────

/// Host-owned cache key for fully-resolved named local symbols.
///
/// Promotes the per-context `ResolvedNamedTypeCacheKey` used by the parser's
/// `TypeResolutionContext` into a cross-request identity: the original shape
/// `(name, surface, base_offset, companion_cache_key, type_param_bindings)`
/// plus `(canonical_id, whole_hash)` scoping so stored entries stay
/// consistent with the owning file's content generation.
///
/// `canonical_id` is `Arc<str>` so every adapter clone / `get`-time key
/// construction is a refcount bump instead of a `String` heap allocation.
///
/// Entries keyed by this struct live inside
/// [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore) via
/// [`SemanticNodeData::VueMacroElements`]; the graph owns the identity map
/// and backs reads with refcount-only lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostResolvedNamedTypeKey {
    pub canonical_id: Arc<str>,
    pub whole_hash: Hash16,
    pub inner:
        verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey,
}

// ──────────────────────────────────────────────────────────────────────────
// API traits
// ──────────────────────────────────────────────────────────────────────────

/// Navigator API — hop selection and non-owning normalization only.
///
/// **Invariant**: navigators never reach past this API to cross imports,
/// instantiate generics, or produce reusable shared-cache entries. New
/// semantic nodes must enter through [`SemanticQueryApi::execute`].
pub trait TypeNavigator {
    fn inspect_surface(&self, node: SemanticNodeId) -> QueryResult<SurfaceView>;
    fn inspect_member(
        &self,
        node: SemanticNodeId,
        member: Arc<str>,
    ) -> QueryResult<Option<SemanticNodeId>>;
    fn inspect_index(
        &self,
        node: SemanticNodeId,
        index: &IndexKey,
    ) -> QueryResult<Option<SemanticNodeId>>;
    fn choose_next_hop(&self, path: &[PathSegment], at: SemanticNodeId) -> HopDecision;
}

/// Read access to the shared semantic graph. Callers that hold a
/// [`SemanticNodeId`] use this trait to inspect the resolved node data
/// without spawning private side channels.
pub trait SemanticGraphRead {
    fn node_data(&self, node: SemanticNodeId) -> Arc<SemanticNodeData>;
}

/// Authoritative dispatch for every reusable semantic operation.
///
/// [`execute`](Self::execute) is the single entry point; the convenience
/// methods are thin wrappers, not a second API surface.
pub trait SemanticQueryApi {
    fn execute(&self, key: SemanticQueryKey) -> QueryResult<SemanticNodeId>;

    fn resolve_decl(&self, key: ResolveDeclKey) -> QueryResult<SemanticNodeId> {
        self.execute(SemanticQueryKey::ResolveDecl(key))
    }
    fn instantiate(
        &self,
        base: DeclIdentity,
        args: Arc<[SemanticNodeId]>,
    ) -> QueryResult<SemanticNodeId> {
        self.execute(SemanticQueryKey::Instantiate { base, args })
    }
    fn project_member(
        &self,
        base: SemanticNodeId,
        member: Arc<str>,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        self.execute(SemanticQueryKey::ProjectMember { base, member, mode })
    }
    fn indexed_access(
        &self,
        base: SemanticNodeId,
        index: IndexKey,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        self.execute(SemanticQueryKey::IndexedAccess { base, index, mode })
    }
    fn project_path(
        &self,
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        self.execute(SemanticQueryKey::ProjectPath { base, path, mode })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Semantic subqueries with the same resolved meaning produce the same
    /// key even when reached through different higher-level expressions —
    /// this is the core dedup guarantee Phase 2.2 builds on.
    #[test]
    fn resolve_decl_keys_dedup_by_scope_and_name() {
        let scope = ScopeId {
            canonical_id: Arc::from("/w/src/types.ts"),
            local_scope: None,
        };
        let a = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope.clone(),
            name: Arc::from("C"),
        });
        let b = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope,
            name: Arc::from("C"),
        });
        assert_eq!(a, b);
    }

    /// Scope-aware identity prevents cross-scope poisoning when two files
    /// both declare `Foo` in their top level.
    #[test]
    fn resolve_decl_keys_disambiguate_by_scope() {
        let a = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::from("/w/a.ts"),
                local_scope: None,
            },
            name: Arc::from("Foo"),
        });
        let b = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::from("/w/b.ts"),
                local_scope: None,
            },
            name: Arc::from("Foo"),
        });
        assert_ne!(a, b);
    }

    /// Generic substitutions are part of semantic meaning — two different
    /// instantiations of the same base must not alias to one cache entry.
    #[test]
    fn instantiate_keys_disambiguate_by_args() {
        let base = DeclIdentity::synthetic("Foo");
        let string_id = SemanticNodeId(1);
        let number_id = SemanticNodeId(2);
        let a = SemanticQueryKey::Instantiate {
            base: base.clone(),
            args: Arc::from(vec![string_id].into_boxed_slice()),
        };
        let b = SemanticQueryKey::Instantiate {
            base,
            args: Arc::from(vec![number_id].into_boxed_slice()),
        };
        assert_ne!(a, b);
    }

    /// Projection mode participates in the cache key — a `Navigate` hop and
    /// an `Expanded` request differ even against the same base and member.
    #[test]
    fn project_member_keys_disambiguate_by_mode() {
        let base = SemanticNodeId(42);
        let a = SemanticQueryKey::ProjectMember {
            base,
            member: Arc::from("foo"),
            mode: ProjectionMode::Navigate,
        };
        let b = SemanticQueryKey::ProjectMember {
            base,
            member: Arc::from("foo"),
            mode: ProjectionMode::Expanded,
        };
        assert_ne!(a, b);
    }

    /// F2 — counter taxonomy matches plan §6.3 (plan §3 F2).
    ///
    /// Asserts bidirectional set-equality between `SemanticGraphStats`
    /// fields and §6.3's expected-to-fire + exceptional-path counter
    /// lists. A new counter on the struct without a corresponding
    /// §6.3 entry (or vice versa) fails the test.
    ///
    /// The assertion is structural: we construct a `SemanticGraphStats`
    /// with `Default::default()`, dump it via `{:?}`, and confirm every
    /// expected field name appears in the debug output AND the total
    /// field count matches the plan's expected cardinality. This
    /// catches additions, renames, and removals without requiring
    /// reflection.
    ///
    /// §6.3 source-of-truth field list (see plan §3 F2 + §6.3):
    /// - Expected-to-fire: hits, misses, waits_ms, in_flight_peak,
    ///   memo_entry_count, origin_edge_count, instantiate_count,
    ///   conditional_decided_count, conditional_deferred_count,
    ///   branch_selections_true, branch_selections_false,
    ///   origin_edges_emitted, path_length_p50, path_length_p95,
    ///   projection_depth_p50, projection_depth_p95,
    ///   origin_edges_per_node_p50, origin_edges_per_node_p95.
    /// - Exceptional-path: budget_fallback_count, same_path_sentinel_returns.
    #[test]
    fn counter_taxonomy_matches_plan() {
        let stats = SemanticGraphStats::default();
        let debug = format!("{stats:?}");

        // Expected-to-fire counters (plan §6.3).
        let expected_to_fire = [
            "hits",
            "misses",
            "waits_ms",
            "in_flight_peak",
            "memo_entry_count",
            "origin_edge_count",
            "instantiate_count",
            "conditional_decided_count",
            "conditional_deferred_count",
            "branch_selections_true",
            "branch_selections_false",
            "origin_edges_emitted",
            "path_length_p50",
            "path_length_p95",
            "projection_depth_p50",
            "projection_depth_p95",
            "origin_edges_per_node_p50",
            "origin_edges_per_node_p95",
        ];
        for field in expected_to_fire {
            assert!(
                debug.contains(&format!("{field}: ")),
                "SemanticGraphStats is missing expected-to-fire counter `{field}` \
                 (plan §6.3)"
            );
        }

        // Exceptional-path counters (legitimately zero on the corpus;
        // §6.2 forcing tests prove they exist and increment under
        // dedicated fixtures).
        let exceptional_path = ["budget_fallback_count", "same_path_sentinel_returns"];
        for field in exceptional_path {
            assert!(
                debug.contains(&format!("{field}: ")),
                "SemanticGraphStats is missing exceptional-path counter `{field}` \
                 (plan §6.3)"
            );
        }

        // Cardinality check: total field count on `SemanticGraphStats`
        // equals expected-to-fire + exceptional-path. Catches a field
        // added to the struct without a corresponding §6.3 entry —
        // which would otherwise slip past the one-way `contains`
        // checks above. Uses `": "` as the field delimiter in Debug
        // output (every primitive field emits exactly one `: `
        // separator between name and value).
        let expected_total = expected_to_fire.len() + exceptional_path.len();
        let field_count = debug.matches(": ").count();
        assert_eq!(
            field_count,
            expected_total,
            "SemanticGraphStats has {field_count} fields in Debug output but \
             plan §6.3 specifies {expected_total} counters (expected_to_fire = {}, \
             exceptional_path = {}). A field was added/removed without updating \
             this test and plan §6.3; see debug output:\n{debug}",
            expected_to_fire.len(),
            exceptional_path.len(),
        );
    }

    /// F2 — navigation-once invariant contract (plan §3 F2).
    ///
    /// The contract says: for N distinct concrete instantiations of the
    /// same parameterised declaration, subexpression lowering count
    /// equals the number of structurally distinct visited subexpressions
    /// — not N × body_size. This test locks the contract; the counter
    /// `decl_subexpression_lowering_count` that drives the strict form
    /// is a post-track refinement per plan §1.4 follow-up item 4.
    ///
    /// Today the invariant is enforced by the family memo (B1b) which
    /// dedups every `ProjectPath(member, path, mode)` sub-query across
    /// distinct `Instantiate` calls that visit the same path.
    #[test]
    fn navigation_once_invariant_contract() {
        // Structural: `SemanticQueryKey::Instantiate { base, args }` is
        // mode-free per §7.14, so two distinct projections into the
        // same declaration body share family memo entries for every
        // structurally-equal path segment.
        //
        // When the F2 counter `decl_subexpression_lowering_count` lands
        // as a post-track refinement, this test's strict assertion
        // becomes: after N `Instantiate(Foo, [V_i])` + matching
        // `ProjectPath(result, [p], Identity)`, the counter equals the
        // size of the visited path intersection, not N × body_size.
        //
        // The current assertion is the structural invariant: the key
        // shape admits this form (base + args, no mode field).
        let base = DeclIdentity::synthetic("Foo");
        let args = Arc::from(vec![SemanticNodeId(2)].into_boxed_slice());
        let key = SemanticQueryKey::Instantiate {
            base: base.clone(),
            args: Arc::clone(&args),
        };
        // Verify the key can be constructed and hashed (mode-free).
        let mut map = std::collections::HashMap::new();
        map.insert(key.clone(), 1);
        let key2 = SemanticQueryKey::Instantiate { base, args };
        assert_eq!(map.get(&key2), Some(&1), "same args dedup to one entry");
    }
}
