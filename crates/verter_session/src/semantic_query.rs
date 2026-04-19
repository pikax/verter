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

/// Mapper identity for mapped-type queries. Separates the key space from the
/// value expression so two mappers that share the same key space but differ
/// in the value expression do not alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapperKey {
    pub key_space: SemanticNodeId,
    pub value_expr: SemanticNodeId,
    pub optionality: OptionalityMod,
    pub readonly: ReadonlyMod,
    /// Optional `as` clause remapping the key.
    pub name_remap: Option<SemanticNodeId>,
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
    /// Catch-all for text-bearing failures surfaced to the caller.
    Other(Arc<str>),
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
        base: SemanticNodeId,
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
}

/// Immutable semantic-node payload. Storage for this enum is owned by the
/// shared semantic graph; node ids hand out views without copying.
///
/// This is the minimum variant set the rewrite requires — arrays, tuples,
/// call signatures, generic carriers, and other TS-flavored variants will
/// grow inside this enum as the rewrite binds more of the existing resolver
/// to the query API.
#[derive(Debug, Clone)]
pub enum SemanticNodeData {
    Alias(SemanticNodeId),
    Object(SurfaceView),
    Union(Arc<[SemanticNodeId]>),
    Intersection(Arc<[SemanticNodeId]>),
    Primitive(PrimitiveKind),
    Opaque(QueryError),
    /// Declaration-identity anchor (plan §2 Lazy block + C1).
    ///
    /// Produced by [`SemanticQueryKey::ResolveDecl`]. Carries enough
    /// identity (`canonical_id`, `name`, `whole_hash`) for
    /// [`SemanticQueryKey::Instantiate`] to resolve the base to a
    /// [`PreparedTypeDecl`](verter_semantic::analysis::type_solver::PreparedTypeDecl)
    /// through `DispatchHost::resolve_prepared_type_decl` without a
    /// side-table or reverse-lookup.
    ///
    /// `DeclAnchor` is not a "lazy subexpression reference" (§7.14 bans
    /// those) — it is an explicit identity carrier for the decl dispatch
    /// surface. Two calls to `ResolveDecl(same-key)` always produce the
    /// same identity tuple, so structural interning converges on one
    /// [`SemanticNodeId`] per `(canonical_id, name, whole_hash)`.
    DeclAnchor {
        canonical_id: Arc<str>,
        name: Arc<str>,
        whole_hash: HashValue,
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
        base: SemanticNodeId,
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
        let base = SemanticNodeId(7);
        let string_id = SemanticNodeId(1);
        let number_id = SemanticNodeId(2);
        let a = SemanticQueryKey::Instantiate {
            base,
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
}
