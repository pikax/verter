//! Semantic Query Graph
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
//! This module introduces the type surface; the implementation that binds
//! it to [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)
//! lives in the `project_type_store` consumers.

use std::sync::Arc;

use verter_semantic::analysis::Hash16;

// Re-export the solver's primitive enum so semantic nodes and the type
// solver agree on the same set of primitive kinds.
pub use verter_semantic::analysis::type_solver::arena::PrimitiveKind;

// Literal-value carrier for [`SemanticNodeData::Literal`]. Re-exported
// so callers working with the semantic graph can match on exact literal
// shapes (`"idle"` vs `"busy"`) without collapsing them to the broader
// `Primitive(String)`.
pub use verter_type_expr::LiteralValue;

// Reuse the existing structured failure shape from the resolver — there is no
// second failure-domain type in this rewrite.
pub use crate::resolver_core::shallow_file_state::BudgetExceededFailure;

/// The `ProjectionDemand × EvalPolicy` lattice algebra (Deliverable #3 of
/// `docs/arch/u2-query-value-domain-design.md`). The five [`ProjectionMode`]
/// presets are points in this lattice; `meet`/`join`, satisfaction, and the
/// §3.4 materialised-record carrier types live here.
pub mod demand;

/// Canonical display policy: [`display`](display::display) projects an
/// already-computed [`SemanticQueryValue`] under a display-only
/// [`DisplayNeeds`](demand::DisplayNeeds) bitset. Display is NEVER a stored or
/// re-parsed string and never re-resolves
/// (`docs/arch/u2-query-value-domain-design.md` §14).
pub mod display;

/// The `SemanticQueryKeySpec` table: one authoritative, hand-encoded row per
/// live [`SemanticQueryKey`] variant, plus the deterministic text renderer the
/// generator binary writes and the diff-test re-checks. See the module for the
/// generator-is-sole-writer contract.
pub mod query_key_spec;
pub use demand::{
    apply_mask, backfill_points, cached_satisfies, demand_at_hop, relevant_demand_axes,
    AliasPreservation, AxisMask, CarrierStopPolicy, Demand, DemandAxis, DisplayFacet, DisplayNeeds,
    EvalPolicy, GenericOpenPolicy, MaterializedPoint, MaterializedSet, MemberBodyDemand, MergeRole,
    NormalizationDepth, OperatorReduction, ProjectionDemand, ProjectionPath, ProvenanceNeed,
    Regime, SurfaceFacet, SurfaceFacetSet, SurfaceRole,
};

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
    ///
    /// Ambient `declare module "X"` / `declare global` augmentation inventories
    /// are addressed separately by the live
    /// [`AugmentationScopeKind`](verter_semantic::analysis::type_eval::AugmentationScopeKind)
    /// path (the binder's `EvalEnv.augmentation_scopes`), not through this key —
    /// a `ResolveDecl` always targets the file top-level surface.
    pub local_scope: Option<u32>,
}

impl ScopeId {
    /// A file top-level scope rooted in `canonical_id`.
    pub fn file(canonical_id: Arc<str>) -> Self {
        Self {
            canonical_id,
            local_scope: None,
        }
    }
}

/// Source-scope sidecar tag for a [`SemanticNodeId`].
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

impl NodeScopeId {
    /// The canonical origin file this scope binds to, when it is a
    /// declaration-bound [`NodeScopeId::File`] scope. `None` for the
    /// structural / scope-less [`NodeScopeId::Global`] origin.
    ///
    /// This is the declaration-site file id a surface member / index
    /// signature stamps as its `declaration_origin` at lowering — the file
    /// the member's `name` / `: T` annotation actually lives in, independent
    /// of where the member's VALUE type resolves (a member whose value is an
    /// unresolved / scope-less node still has a real declaration file).
    #[must_use]
    pub fn canonical_file(&self) -> Option<Arc<str>> {
        match self {
            Self::Global => None,
            Self::File { canonical_id, .. } => Some(Arc::clone(canonical_id)),
        }
    }
}

/// Resolved-declaration lookup key. Two callers from the same scope for the
/// same name produce the same key and dedup automatically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolveDeclKey {
    pub scope: ScopeId,
    pub name: Arc<str>,
}

/// Declaration identity for [`SemanticNodeData::TypeParam`].
///
/// Distinguishes two unrelated `type A<T> = ...` and `type B<T> = ...`
/// declarations in the same source file. Identical parameter names
/// across unrelated declarations must not collide structurally under
/// the semantic-graph interner, so the `TypeParam` variant carries
/// declaration identity (owning file + content generation +
/// declaring entity name) in the payload itself — the compound
/// `(payload, scope)` interner key has a primary discriminator at
/// the payload level rather than relying solely on display name.
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

    /// Project this identity onto a content-free [`DeclKey`] suitable
    /// for use as a query-identity cache key component. The version
    /// (`whole_hash`) is dropped — query-identity keys hold no
    /// content/version hashes (R6); per-value version rooting lives
    /// on the cached `MemoEntry`'s `ReadSetSignature.facts` and
    /// `self_root_canonicals`, re-sourced at value-build time from the
    /// live indexed view.
    #[must_use]
    pub fn to_decl_key(&self) -> DeclKey {
        DeclKey {
            canonical_id: Arc::clone(&self.canonical_id),
            decl_name: Arc::clone(&self.decl_name),
        }
    }
}

/// Content-free declaration key used as a query-identity component
/// inside derived-`Hash` `SemanticQueryKey` / `FamilyKey` variants.
///
/// Two file-content versions of "same decl" produce equal `DeclKey`s;
/// version-rooting lives entirely on the cached value via the
/// multi-candidate `FamilySlots` substrate (each candidate carries its
/// own `ReadSetSignature.facts` + `self_root_canonicals`). The build
/// path re-sources the owning file's content hash from
/// [`ResolverContext::ensure_indexed_ready`] at value-compute time
/// (R6 + R20).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclKey {
    /// Canonical id of the declaring file. Empty for the global /
    /// structural sentinel; `"__builtin__"` for built-in utility
    /// carriers (`Pick`, `Omit`, …); a concrete canonical otherwise.
    pub canonical_id: Arc<str>,
    /// Stable declaring-entity name (interface, type alias, class,
    /// script-setup sentinel, `"Pick"` / `"Omit"` for builtins, etc.).
    pub decl_name: Arc<str>,
}

impl DeclKey {
    /// Project a [`DeclIdentity`] onto its content-free [`DeclKey`].
    #[must_use]
    pub fn from_identity(identity: &DeclIdentity) -> Self {
        DeclKey {
            canonical_id: Arc::clone(&identity.canonical_id),
            decl_name: Arc::clone(&identity.decl_name),
        }
    }

    /// Build a built-in utility decl key (`Pick` / `Omit` / `Extract`
    /// / `Exclude` / `NonNullable` / `Required` / `Partial` / `Readonly`
    /// / `ReturnType` / …). The `canonical_id` is the `"__builtin__"`
    /// sentinel; builtins root self-version through their `args` nodes
    /// (no file fact).
    #[must_use]
    pub fn builtin(name: &str) -> Self {
        DeclKey {
            canonical_id: Arc::from("__builtin__"),
            decl_name: Arc::from(name),
        }
    }
}

/// Per-domain symbol space tag. Distinguishes declarations sharing
/// the same `(defining_canonical, merged_symbol_name)` but living in
/// disjoint symbol spaces (TypeScript's type-space vs value-space).
///
/// Used as a key dimension on [`ResolvedDeclSlotIdentity`] per R7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SemanticSymbolSpace {
    /// Type-space declaration (interface, type alias, enum's type
    /// half, class's type half).
    Type,
    /// Value-space declaration (function, const, let, var, enum's
    /// value half, class's value half).
    Value,
    /// Namespace-space declaration (`namespace N {}` / `module N {}`
    /// with no value/type half). A namespace-only declaration keys on
    /// a slot with `symbol_space = Namespace`.
    ///
    /// There is deliberately NO `BothTypeValue` arm: a class or enum
    /// that occupies BOTH type and value space is TWO slots (one
    /// `Type`, one `Value`), resolved by the class dual-space
    /// algorithm ([`ProjectSemanticDispatch::build_class_surface`]),
    /// never one fused arm.
    Namespace,
}

/// Per-declaration-part identifier. Inside a merged declaration
/// group (e.g. multiple `interface Foo` declarations sharing the
/// same `merged_symbol_name`), each contributing part is tagged
/// with a stable `DeclPartId` so the per-part fingerprint can be
/// stored on [`VersionedDeclIdentity::merged_parts`].
///
/// **Validation contract (R7):** `merged_parts` is **payload, not
/// validation**. Slot-level fact validation is the oracle.
/// Consumers that observe a specific part's facts invalidate on
/// that part's change; adding an overload does NOT invalidate
/// consumers that observed only another overload's facts.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct DeclPartId(pub u32);

/// Per-part fingerprint hash. Carried as payload on
/// [`VersionedDeclIdentity::merged_parts`]; not used as a
/// validation oracle (slot-level facts are).
pub type DeclPartFingerprint = HashValue;

/// Cache-identity key for the resolved declaration slot. Six
/// fields (R7):
///
/// - `defining_canonical`: canonical id of the declaring file.
/// - `merged_symbol_name`: stable merged-symbol identity that
///   survives declaration reordering and TS declaration merging.
/// - `symbol_space`: type vs value disambiguator
///   ([`SemanticSymbolSpace`]).
/// - `project_identity`: workspace + tsconfig + provider-root
///   discriminator.
/// - `type_env_hash`: TS compiler-options dimension.
/// - `lib_env_hash`: TS lib selection + typeRoots + ambient corpus
///   fingerprint.
///
/// **Cache invariant (R7 + multi-candidate substrate):** the slot
/// identity is **content-free**. Two file versions of "same decl"
/// produce equal slot keys; the multi-candidate `ValidatedFactCache`
/// separates them via per-candidate `fact_dep_signature`.
/// File-content versioning lives in [`VersionedDeclIdentity`]
/// inside the cached payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedDeclSlotIdentity {
    /// Canonical id of the declaring file. NOT the consumer scope —
    /// see audit doc `docs/arch/materialize-owner-local-audit.md`
    /// (a) for the `local_fence_seed` derivation rationale.
    pub defining_canonical: Arc<str>,
    /// Stable merged-symbol name. Invariant under declaration
    /// reordering AND under TS declaration merging.
    pub merged_symbol_name: Arc<str>,
    /// Type-space vs value-space discriminator.
    pub symbol_space: SemanticSymbolSpace,
    /// Project identity dimension (workspace + tsconfig + provider).
    pub project_identity: u32,
    /// Type-env dimension (strict, noImplicitAny, target, …).
    pub type_env_hash: HashValue,
    /// Lib-env dimension (lib selection + typeRoots + ambient corpus).
    pub lib_env_hash: HashValue,
}

impl ResolvedDeclSlotIdentity {
    /// Build a slot identity for a type-space declaration.
    #[must_use]
    pub fn type_slot(
        defining_canonical: Arc<str>,
        merged_symbol_name: Arc<str>,
        project_identity: u32,
        type_env_hash: HashValue,
        lib_env_hash: HashValue,
    ) -> Self {
        Self {
            defining_canonical,
            merged_symbol_name,
            symbol_space: SemanticSymbolSpace::Type,
            project_identity,
            type_env_hash,
            lib_env_hash,
        }
    }

    /// Build a slot identity for a value-space declaration.
    #[must_use]
    pub fn value_slot(
        defining_canonical: Arc<str>,
        merged_symbol_name: Arc<str>,
        project_identity: u32,
        type_env_hash: HashValue,
        lib_env_hash: HashValue,
    ) -> Self {
        Self {
            defining_canonical,
            merged_symbol_name,
            symbol_space: SemanticSymbolSpace::Value,
            project_identity,
            type_env_hash,
            lib_env_hash,
        }
    }

    /// Compatibility constructor: derive a slot identity from a
    /// legacy [`DeclIdentity`] plus the env dimensions.
    /// `whole_hash` is intentionally NOT consumed — the slot is
    /// content-free; per-file content versioning belongs on
    /// [`VersionedDeclIdentity`].
    #[must_use]
    pub fn from_decl_identity(
        identity: &DeclIdentity,
        symbol_space: SemanticSymbolSpace,
        project_identity: u32,
        type_env_hash: HashValue,
        lib_env_hash: HashValue,
    ) -> Self {
        Self {
            defining_canonical: Arc::clone(&identity.canonical_id),
            merged_symbol_name: Arc::clone(&identity.decl_name),
            symbol_space,
            project_identity,
            type_env_hash,
            lib_env_hash,
        }
    }

    /// Return a clone of this slot with its [`symbol_space`] replaced.
    ///
    /// Used to CANONICALIZE the symbol space of a slot to a single value
    /// when the consumer's identity must not fork on it. The
    /// `ResolveClassSurface` family key canonicalizes its `decl_slot` to
    /// `Type` (`semantic_query_memo::family::family_and_slot`): the class
    /// dual-space algorithm selects the half via `side` and derives the
    /// composed sub-query from `(defining_canonical, merged_symbol_name)`
    /// alone, so the incoming slot's `symbol_space` must not become part of
    /// the cache identity. Every other field of the slot identity is
    /// preserved verbatim.
    ///
    /// [`symbol_space`]: Self::symbol_space
    #[must_use]
    pub fn with_symbol_space(&self, symbol_space: SemanticSymbolSpace) -> Self {
        Self {
            defining_canonical: Arc::clone(&self.defining_canonical),
            merged_symbol_name: Arc::clone(&self.merged_symbol_name),
            symbol_space,
            project_identity: self.project_identity,
            type_env_hash: self.type_env_hash,
            lib_env_hash: self.lib_env_hash,
        }
    }
}

/// Project a value-space [`ResolvedDeclSlotIdentity`] onto the
/// [`ValueRootKey`] that names its constructor value at the file
/// top-level scope.
///
/// Used by the class dual-space algorithm's static side: the static
/// surface of a class is the type of its constructor VALUE, reached via
/// `TypeOf(value_root_of(value_slot))`. The value root is the slot's
/// `merged_symbol_name` resolved in the declaring file's top-level
/// scope.
#[must_use]
pub fn value_root_of(value_slot: &ResolvedDeclSlotIdentity) -> ValueRootKey {
    ValueRootKey {
        scope: ScopeId::file(Arc::clone(&value_slot.defining_canonical)),
        name: Arc::clone(&value_slot.merged_symbol_name),
    }
}

/// Per-content-version payload tag for a
/// [`ResolvedDeclSlotIdentity`]. Three fields per R7:
///
/// - `slot`: the content-free [`ResolvedDeclSlotIdentity`] this
///   value is associated with.
/// - `content_hash`: per-file content version (sourced from
///   `IndexedReady.whole_hash` at admission time). Two file versions
///   of "same decl" produce equal slot keys + distinct `content_hash`
///   payloads — the multi-candidate `ValidatedFactCache` separates
///   them via per-candidate fact validation.
/// - `parse_env_hash`: parser flags + SFC compiler flags
///   dimension. Cosmetic edits within the same parser flags
///   produce the same `parse_env_hash`; flag changes invalidate
///   the cached entry.
/// - `merged_parts`: per-part fingerprints inside a merged
///   declaration group. **Payload, NOT a validation oracle (R7).**
///   Consumers observe specific parts via their `fact_dep_signature`;
///   adding an overload does NOT, by itself, invalidate consumers
///   that observed only one overload's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedDeclIdentity {
    pub slot: ResolvedDeclSlotIdentity,
    pub content_hash: HashValue,
    pub parse_env_hash: HashValue,
    pub merged_parts: smallvec::SmallVec<[(DeclPartId, DeclPartFingerprint); 2]>,
}

impl VersionedDeclIdentity {
    /// Build a versioned identity with a single declaration part.
    #[must_use]
    pub fn single_part(
        slot: ResolvedDeclSlotIdentity,
        content_hash: HashValue,
        parse_env_hash: HashValue,
        part: (DeclPartId, DeclPartFingerprint),
    ) -> Self {
        let mut merged_parts = smallvec::SmallVec::new();
        merged_parts.push(part);
        Self {
            slot,
            content_hash,
            parse_env_hash,
            merged_parts,
        }
    }
}

/// A reference that is either a declaration identity (not interned
/// in the arena) or a concrete semantic node. Declaration identity
/// is carried as a `DeclIdentity` value, not as an interned node
/// variant, so a generic instantiation's `base` does not require a
/// distinct `DeclAnchor` node in the arena.
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

/// Lowering-time classification of a [`MapperKey::value_expr`].
///
/// `Identity` means the lowered `value_expr` is structurally the
/// `T[K]` projection of the mapped source — `IndexedAccess { object:
/// source_param_ref, index: TypeNode(mapper_param_ref) }` — so
/// `build_mapped_type` may read each source member's value directly
/// (the canonical `{ [K in keyof T]: T[K] }` pattern behind
/// `Partial<T>` / `Required<T>` / `Readonly<T>`). `Computed` means
/// the value is any other shape (computed projection, conditional
/// body, intersected helper) and must go through
/// `substitute_semantic_type_param` +
/// `evaluate_deferred_semantic_node`.
///
/// Classification happens at lowering time so the build path matches
/// on a stable tag rather than re-inspecting the AST shape at every
/// `build_mapped_type` call.
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
    /// `mapper_param_node` is the mapper's binder node id (the
    /// `TypeParam` introduced by the enclosing `[K in ...]`
    /// binding). Classification checks that the indexed-access
    /// `index`'s type node equals this binder by `SemanticNodeId`,
    /// not by `display_name` string — two binders that share a
    /// display name but are semantically distinct must not conflate.
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
/// `parameter_node` carries the mapper's binder identity as the
/// interned `TypeParam`'s [`SemanticNodeId`] rather than as a
/// display-name `Arc<str>`. Binder matching across
/// substitute / classify paths is node-id equality, so two distinct
/// mapped binders sharing a display name do not conflate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapperKey {
    /// The binder's interned `TypeParam` node id — binder identity
    /// is by `SemanticNodeId` rather than by display name.
    pub parameter_node: SemanticNodeId,
    pub key_space: SemanticNodeId,
    pub value_expr: SemanticNodeId,
    pub optionality: OptionalityMod,
    pub readonly: ReadonlyMod,
    /// Optional `as` clause remapping the key.
    pub name_remap: Option<SemanticNodeId>,
    /// Lowering-time classification of `value_expr`. See
    /// [`MapperKind`].
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
/// - `Skeleton`: open-generic body access for cycle detection.
///   / R10-2. `build_instantiate` synthesizes `TypeParam` shells for
///   unbound type parameters when invoked in this mode, preserving
///   project-rule "Navigate/Shallow over open generics preserves type
///   parameters". Used by `ref_root_reaches_transitive_cycle_node`'s BFS
///   step. Existing Navigate/Expanded callers see no change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionMode {
    Identity,
    Navigate,
    Shallow,
    Expanded,
    Skeleton,
}

impl From<ProjectionMode> for verter_audit::ProjectionModeTag {
    fn from(mode: ProjectionMode) -> Self {
        match mode {
            ProjectionMode::Identity => verter_audit::ProjectionModeTag::Identity,
            ProjectionMode::Navigate => verter_audit::ProjectionModeTag::Navigate,
            ProjectionMode::Shallow => verter_audit::ProjectionModeTag::Shallow,
            ProjectionMode::Expanded => verter_audit::ProjectionModeTag::Expanded,
            ProjectionMode::Skeleton => verter_audit::ProjectionModeTag::Skeleton,
        }
    }
}

/// Reduction-demand axis (codex-hybrid spec).
///
/// Distinguishes whether the query result is going to be *published*
/// (a consumer of the projector pipeline will read it on the final
/// component-meta surface) or is needed *internally* as a structural
/// transit value (e.g. by the relation engine to bind an `infer`
/// parameter, by the deferred-shell evaluator to walk a `KeyOf` /
/// `Mapped` carrier, by the BFS cycle guard).
///
/// The carrier-stop predicate [`may_reduce_operator`] gates whether
/// `keyof T` enumerates its keyspace into per-key literal anchors and
/// whether `{ [K in S]: V }` materialises its produced surface.
/// Under `StructuralTransit` both operators return a carrier
/// ([`SemanticNodeData::KeyOf`] / [`SemanticNodeData::Mapped`]) so the
/// caller can substitute / inspect the operator structurally without
/// paying for member materialisation that no published demand selects
/// through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReductionDemand {
    /// The caller will publish the result on a consumer-visible
    /// surface. Operators reduce when `mode` admits.
    Published,
    /// The caller needs the result as a structural transit value
    /// (relation engine binding, deferred-shell evaluation, generic
    /// substitution, cycle BFS). Operators carrier-stop regardless of
    /// `mode`.
    StructuralTransit,
    /// Vue macro object-surface publication (`defineProps` / `defineSlots`).
    ///
    /// Distinct from [`Self::Published`] in EXACTLY ONE behavioural axis:
    /// the empty-path Shallow terminal-surface synthesis enumerates the
    /// **UNION of object-arm members** for a `Union` root (a prop / slot
    /// present in ANY arm is part of the component macro surface — the Vue
    /// macro convention), instead of the TS property-access **intersection**
    /// of common members that ordinary `ProjectPath` / `keyof` produces.
    /// Operators reduce exactly like `Published` ([`may_reduce_operator`]),
    /// so generic carriers, `Pick`/`Omit`, and mapped slots materialise
    /// identically — only the union-arm merge rule differs.
    ///
    /// Cache-keyed DISTINCTLY from `Published` (a dedicated `ModeSlot`) so a
    /// macro object-surface read and an ordinary `Published` path read of the
    /// SAME `(base, path)` never collide on one memo cell. Ordinary
    /// `ProjectPath` / `keyof` keep `Published` (intersection is correct
    /// there).
    MacroObjectSurface,
}

/// Surface-provenance axis — codex BINDING design.
///
/// Records whether the surface members produced by a lowering /
/// instantiation reached the surface as the **macro type-argument's own
/// body** (the SFC author literally wrote the member name inside
/// `defineProps<T>()`'s `T`) versus a plain structural lowering
/// (heritage descent, member-value lowering, generic substitution, any
/// non-macro-root query).
///
/// This is the typed-IR equivalent of the parser's `from_root_body`
/// flag and the prepared-surface walker's body-vs-heritage entry
/// context. It is the single input that lets the canonical dispatch
/// stamp [`SurfaceMember::declared_in_macro_type_arg`] correctly without
/// a post-resolution name-set classification (which would misclassify
/// `Omit`-then-reintroduce, intersection collisions, and external-ref
/// arms).
///
/// Folded into the [`crate::semantic_query_memo::FamilyKey`] identity
/// for the context-bearing `Instantiate` / `ProjectPath` families so a
/// macro-root surface and a plain structural surface of the SAME node
/// never collide on one memo slot. It is NOT an env-hash dimension (R21)
/// — it is a query-identity dimension, like the projection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SurfaceProvenanceContext {
    /// Plain structural lowering. Object members reached here carry
    /// `declared_in_macro_type_arg = false`. This is the default for
    /// every query that is not a macro-type-argument own-body entry —
    /// heritage descent, member-value lowering, generic substitution,
    /// relation-engine transit, and all non-macro queries.
    #[default]
    Structural,
    /// The lowering / instantiation is entering the
    /// `defineProps<T>()` / `withDefaults(defineProps<T>(), …)` type
    /// argument's OWN body. Object members lowered directly at this
    /// entry (an inline `TSTypeLiteral`, the directly-referenced
    /// declaration's own body, or an explicit Object arm of an
    /// intersection literal) carry `declared_in_macro_type_arg = true`.
    /// Heritage-backfilled members, utility-target sources
    /// (`Omit`/`Pick`), mapped-produced members, and member VALUE
    /// bodies downgrade to [`Self::Structural`].
    MacroTypeArgOwnBody,
}

/// Member-merge role — the surface-merge provenance axis (codex BINDING
/// design for the type-resolution unification). Orthogonal to
/// [`SurfaceProvenanceContext`] (which records macro-type-argument own-body
/// participation): a plain non-macro `resolve_named_symbol` carries
/// `SurfaceProvenanceContext::Structural` yet still needs the merge role to
/// implement TS derived-member precedence.
///
/// The role drives the intersection surface merge's own-body-shadows-heritage
/// decision (`merge_intersection_surfaces_with_graph`): an interface/class
/// `extends`/`implements` overlay shadows (the derived `OwnBody` member wins
/// over the inherited `Heritage` member), but an authored intersection
/// (`type Props = Base & { dup }`) does NOT shadow — its arms intersect.
///
/// Stamped at the object-member lowering leaf from the threaded
/// [`ProjectionReductionContext::merge_role`], exactly like
/// `declared_in_macro_type_arg` is stamped from the provenance axis. The role
/// is RELATIVE to the consuming declaration: a member inherited through an
/// `interface Props extends Base` arm is `Heritage` even though it is
/// `Base`'s own-body member, because the consuming declaration's heritage-arm
/// context flows into the carrier resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MemberMergeRole {
    /// Reached via an authored reference / synthesized construction — an
    /// authored intersection's reference arm (`type Props = Base & { … }`'s
    /// `Base`), a union common-member, a mapped-type produced member, or any
    /// plain structural object lowering. Authored members do NOT participate
    /// in own-body-shadows-heritage; duplicate authored members intersect.
    #[default]
    Authored,
    /// Declared in the consuming declaration's OWN body (an inline object
    /// literal at the declaration / macro-T own body). For an interface/class
    /// the own-body member SHADOWS an inherited `Heritage` member of the same
    /// name (TS derived-member precedence).
    OwnBody,
    /// Reached via a REAL interface/class `extends`/`implements` heritage
    /// overlay of the consuming declaration. SHADOWED by an `OwnBody` member
    /// of the same name.
    Heritage,
}

/// Projection reduction context — the `(mode, demand, provenance, merge_role)`
/// tuple threaded through every operator dispatch (`Instantiate` /
/// `KeyOf` / `MappedType` and the builtin-utility dispatch that
/// composes them).
///
/// The cache slot is per-context so a `StructuralTransit/Shallow`
/// result does not collide with a `Published/Shallow` result on the
/// same node — they are distinct evaluations. The `provenance` axis is
/// folded into `FamilyKey` for `Instantiate` / `ProjectPath` (codex
/// BINDING design) so a macro-root surface and a structural surface of
/// the same node cache in distinct slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectionReductionContext {
    pub mode: ProjectionMode,
    pub demand: ReductionDemand,
    /// Surface-provenance axis (codex BINDING design). Defaults to
    /// [`SurfaceProvenanceContext::Structural`]; only the macro
    /// type-argument own-body entry points opt into
    /// [`SurfaceProvenanceContext::MacroTypeArgOwnBody`]. Part of the
    /// query identity (folded into `FamilyKey` for `Instantiate` /
    /// `ProjectPath`), so a macro-root surface and a structural surface
    /// of the same node cache in distinct slots.
    pub provenance: SurfaceProvenanceContext,
    /// Member-merge role axis (codex BINDING design for the type-resolution
    /// unification). Orthogonal to [`Self::provenance`]; drives the
    /// own-body-shadows-heritage decision in the intersection surface merge.
    /// Defaults to [`MemberMergeRole::Authored`]; the consuming declaration's
    /// per-arm lowering (`lower_decl_body_with_provenance`) and the walker's
    /// heritage-arm propagation set `OwnBody` / `Heritage`. Folded into
    /// `FamilyKey` for `Instantiate` / `ProjectPath` so a heritage-arm
    /// surface and a structural surface of the same node cache distinctly.
    pub merge_role: MemberMergeRole,
}

impl ProjectionReductionContext {
    /// Construct a `Published` context with the supplied mode. The
    /// canonical entry point for the publication pipeline (projector,
    /// component-meta materialisation, typeinfo, explicit path walks).
    ///
    /// Provenance defaults to [`SurfaceProvenanceContext::Structural`];
    /// merge role defaults to [`MemberMergeRole::Authored`].
    pub const fn published(mode: ProjectionMode) -> Self {
        Self {
            mode,
            demand: ReductionDemand::Published,
            provenance: SurfaceProvenanceContext::Structural,
            merge_role: MemberMergeRole::Authored,
        }
    }

    /// Construct a `Published` context entering the macro
    /// type-argument's OWN body — codex BINDING design.
    ///
    /// Used by the macro-payload projector entry points
    /// (`defineProps<T>()` / `withDefaults`) so the object members
    /// lowered directly at the macro-T root carry
    /// `declared_in_macro_type_arg = true`. Heritage descent,
    /// member-value lowering, and utility-target sources downgrade to
    /// [`SurfaceProvenanceContext::Structural`] internally (see
    /// `lower.rs` / `build_instantiate`).
    pub const fn published_macro_type_arg_body(mode: ProjectionMode) -> Self {
        Self {
            mode,
            demand: ReductionDemand::Published,
            provenance: SurfaceProvenanceContext::MacroTypeArgOwnBody,
            merge_role: MemberMergeRole::Authored,
        }
    }

    /// Construct a Vue macro object-surface publication context
    /// (`defineProps` / `defineSlots`).
    ///
    /// [`ReductionDemand::MacroObjectSurface`] enumerates the UNION of
    /// object-arm members at the empty-path Shallow terminal surface (the
    /// Vue macro convention) while reducing operators exactly like
    /// `Published`. `provenance` carries the supplied macro own-body /
    /// structural axis (props use `MacroTypeArgOwnBody`; slots / emits use
    /// `Structural`).
    pub const fn macro_object_surface(
        mode: ProjectionMode,
        provenance: SurfaceProvenanceContext,
    ) -> Self {
        Self {
            mode,
            demand: ReductionDemand::MacroObjectSurface,
            provenance,
            merge_role: MemberMergeRole::Authored,
        }
    }

    /// Whether this context publishes a Vue macro object-surface —
    /// the single predicate the union-arm merge consults to choose
    /// union-of-members over the common-member intersection.
    #[must_use]
    pub const fn is_macro_object_surface(self) -> bool {
        matches!(self.demand, ReductionDemand::MacroObjectSurface)
    }

    /// Construct a `StructuralTransit` context — used by the relation
    /// engine, deferred-shell evaluator, and other internal transit
    /// callers that need the structural shape but never publish the
    /// result. `mode` is `Shallow` for transit callers per the
    /// codex-hybrid spec.
    pub const fn structural_transit() -> Self {
        Self {
            mode: ProjectionMode::Shallow,
            demand: ReductionDemand::StructuralTransit,
            provenance: SurfaceProvenanceContext::Structural,
            merge_role: MemberMergeRole::Authored,
        }
    }

    /// Construct a `StructuralTransit` context with an explicit
    /// `mode`. Used by the macro publication boundary's carrier
    /// lowering: the slot/object surface publisher lowers the macro
    /// expression with `(Navigate, StructuralTransit)` so every
    /// recursive lowering frame propagates the transit demand and
    /// `may_reduce_operator` evaluates false at every nested
    /// `Instantiate` / `KeyOf` / `MappedType` dispatch (no keyspace
    /// reification along the lowering carrier).
    ///
    /// The publication terminal then walks the structural carrier
    /// under `Published(Shallow)` so the consumer observes a one-level
    /// Object surface; member values stay as their carrier nodes per
    /// the shallow-by-default rule.
    pub const fn structural_transit_with_mode(mode: ProjectionMode) -> Self {
        Self {
            mode,
            demand: ReductionDemand::StructuralTransit,
            provenance: SurfaceProvenanceContext::Structural,
            merge_role: MemberMergeRole::Authored,
        }
    }

    /// Return a copy with the surface provenance downgraded to
    /// [`SurfaceProvenanceContext::Structural`].
    ///
    /// Used at every lowering edge that leaves the macro type-argument's
    /// own body — member VALUE lowering, heritage descent, utility-type
    /// source/target lowering, mapped-type production — so a nested
    /// object inside a member value is NOT mis-stamped as macro-root
    /// own-body.
    ///
    /// The [`Self::merge_role`] axis is PRESERVED — downgrading the macro
    /// own-body provenance does not change a member's heritage/own/authored
    /// merge role (the two axes are orthogonal).
    #[must_use]
    pub const fn into_structural_provenance(self) -> Self {
        Self {
            mode: self.mode,
            demand: self.demand,
            provenance: SurfaceProvenanceContext::Structural,
            merge_role: self.merge_role,
        }
    }

    /// Return a copy with the projection `mode` replaced (demand +
    /// provenance + merge_role preserved). Used by the indexed-access
    /// deferred-shell evaluator to demote an INTERMEDIATE object hop to
    /// [`ProjectionMode::Navigate`] while the terminal single-hop
    /// projection runs in the caller's mode — the path-precision rule
    /// "intermediate hops run in Navigate, the terminal hop runs in the
    /// caller's mode" applied to the `T[K]` reduction.
    #[must_use]
    pub const fn with_mode(self, mode: ProjectionMode) -> Self {
        Self {
            mode,
            demand: self.demand,
            provenance: self.provenance,
            merge_role: self.merge_role,
        }
    }

    /// Return a copy with the surface provenance replaced by `provenance`
    /// (mode + demand + merge_role preserved). Used by the path walker's
    /// `DeclPlaceholder` expansion to carry the caller's provenance onto
    /// the unwrap `Instantiate`.
    #[must_use]
    pub const fn with_provenance(self, provenance: SurfaceProvenanceContext) -> Self {
        Self {
            mode: self.mode,
            demand: self.demand,
            provenance,
            merge_role: self.merge_role,
        }
    }

    /// Return a copy with the member-merge role replaced by `merge_role`
    /// (mode + demand + provenance preserved). Used by
    /// `lower_decl_body_with_provenance` to stamp an interface/class own
    /// `Object` arm `OwnBody` and a heritage reference arm `Heritage`, and by
    /// the empty-path Shallow walker to propagate the heritage role into a
    /// deferred-carrier resolution.
    #[must_use]
    pub const fn with_merge_role(self, merge_role: MemberMergeRole) -> Self {
        Self {
            mode: self.mode,
            demand: self.demand,
            provenance: self.provenance,
            merge_role,
        }
    }

    /// Whether this context is entering the macro type-argument's own
    /// body (the single predicate the object-member lowering consults to
    /// decide `declared_in_macro_type_arg`).
    #[must_use]
    pub const fn is_macro_type_arg_own_body(self) -> bool {
        matches!(
            self.provenance,
            SurfaceProvenanceContext::MacroTypeArgOwnBody
        )
    }

    /// The member-merge role this context stamps onto object members it
    /// lowers (the single accessor the object-member lowering consults to
    /// set [`SurfaceMember::merge_role`]).
    #[must_use]
    pub const fn merge_role(self) -> MemberMergeRole {
        self.merge_role
    }
}

/// Carrier-stop predicate (codex-hybrid spec).
///
/// Returns `true` exactly when an operator (`keyof T`, `{ [K in S]: V }`)
/// should reduce — i.e. the caller is on the publication path.
/// Otherwise the operator returns a deferred carrier
/// ([`SemanticNodeData::KeyOf`] / [`SemanticNodeData::Mapped`])
/// without enumerating keys or materialising produced members.
///
/// **Implementation note (codex-hybrid amendment).** The codex spec
/// originally added `&& matches!(ctx.mode, ProjectionMode::Expanded)`
/// to the predicate. Empirically that mode restriction is too tight:
/// it carrier-stops the macro projector's `Published + Navigate`
/// publication path, which breaks userland-`MyPick` structural
/// equivalence (a userland `{ [P in K]: T[P] }` must materialise
/// identically to the builtin `Pick<T,K>` — both enter the publication
/// pipeline with `Published` demand). The demand axis alone is the
/// load-bearing rail: `StructuralTransit` carrier-stops (relation
/// engine, deferred-shell evaluation, generic substitution),
/// `Published` reduces (every publication-reachable operator
/// dispatch).
///
/// The ChatMessages leak fix is preserved: the relation engine's
/// identity-carrier unwrap and Object-vs-Record arm both explicitly
/// dispatch under `ProjectionReductionContext::structural_transit()`,
/// so the inference-time chain that produces the
/// `outputSchema|execute` derivation edges carrier-stops at the
/// relation-engine boundary regardless of the surrounding mode.
///
/// Structural — does NOT inspect the operand's declaration name.
/// A userland `type MyPick<T,K extends keyof T> = { [P in K]: T[P] }`
/// follows the same code path as the builtin `Pick<T,K>` and obeys
/// the SAME predicate.
pub const fn may_reduce_operator(ctx: ProjectionReductionContext) -> bool {
    // `MacroObjectSurface` is a publication demand: operators reduce
    // exactly like `Published`. The two demands differ ONLY in the
    // union-arm merge rule at the empty-path Shallow terminal surface.
    matches!(
        ctx.demand,
        ReductionDemand::Published | ReductionDemand::MacroObjectSurface
    )
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
/// rule: the member's body is **not** eagerly expanded when the
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
    /// Declared accessibility of the member, carried verbatim from the IR
    /// ([`verter_type_expr::MemberVisibility`]). `Public` for every non-class
    /// origin (interface / type-literal / object-literal / mapped); a class
    /// member carries its `TSAccessibility`. A member produced by a surface
    /// merge (intersection / union / heritage) carries the MOST-RESTRICTIVE
    /// accessibility across its contributing arms (`Public` only when Public in
    /// EVERY contributor). Participates in node interning / graph identity
    /// (eq + hash) — a `private foo` and a `public foo` intern to DISTINCT
    /// nodes, mirroring how `spans` already extends member identity. EVERY
    /// published-member surface (props / emits / slots / slot-bindings /
    /// options / exposed) re-applies a `Public`-only filter at the publication
    /// boundary, so non-public class members stay recorded here (the keep-all
    /// `native_props` carrier reads the full recorded member set) without leaking
    /// onto any published Vue surface.
    pub visibility: verter_type_expr::MemberVisibility,
    /// OXC declaration-site spans for this member, stamped during shallow
    /// lowering and carried verbatim from the `verter_type_expr` IR
    /// ([`verter_type_expr::MemberSpans`]). Coordinates are in the member's
    /// declaring file — the file recorded by [`Self::declaration_origin`], set
    /// from the LOWERING scope of the object the member is declared in (NOT the
    /// member's value-type node). The span-rich
    /// [`crate::typeinfo::TypeInfoSurface`] projection indexes these offsets
    /// against `declaration_origin`; a member whose value lowers to a
    /// scope-less node still reports its real declaration spans against that
    /// file. Spans are content-version facts: BOTH `spans` and
    /// `declaration_origin` participate in node interning / graph identity
    /// (eq + hash) — an identical same-file shape at a different source location
    /// interns to a distinct node — but never enter `parse_stable_hash`. `None`
    /// components for genuinely synthetic members (union common-members,
    /// mapped-produced members) with no single source site.
    pub spans: verter_type_expr::MemberSpans,
    /// Canonical file the member's DECLARATION (its `name` / `: T` annotation)
    /// lives in — set from the LOWERING scope of the object the member is
    /// declared in, NOT from the member's value-type node. A member declared in
    /// file F has its name/decl/type spans in F regardless of where its value
    /// type resolves; in particular an unresolved / scope-less value
    /// (`{ present: MissingType }`) does NOT erase the member's real
    /// declaration file. The span-rich [`crate::typeinfo::TypeInfoSurface`]
    /// projection pairs the [`Self::spans`] offsets with THIS file. `None` only
    /// for genuinely synthetic / multi-origin members (union common-members,
    /// mapped-produced members) — the same members whose `spans` are absent.
    pub declaration_origin: Option<Arc<str>>,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source). See
    /// [`verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp::declared_in_macro_type_arg`]
    /// for the structural definition. Propagated through the prepared-surface
    /// walker and `surface_member_to_expanded_field`.
    pub declared_in_macro_type_arg: bool,
    /// Surface-merge role (codex BINDING design for the type-resolution
    /// unification). Distinguishes a member declared in the consuming
    /// declaration's OWN body ([`MemberMergeRole::OwnBody`]) from one reached
    /// via REAL interface/class `extends`/`implements` heritage
    /// ([`MemberMergeRole::Heritage`]) and from an authored-reference /
    /// synthesized member ([`MemberMergeRole::Authored`]). Drives the
    /// own-body-shadows-heritage decision in the intersection surface merge
    /// (`merge_intersection_surfaces_with_graph`). Stamped at the
    /// object-member lowering leaf from
    /// [`ProjectionReductionContext::merge_role`], orthogonal to
    /// `declared_in_macro_type_arg` (which is macro-only).
    pub merge_role: MemberMergeRole,
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
    /// OXC declaration-site spans for this index signature, carried verbatim
    /// from the `verter_type_expr` IR ([`verter_type_expr::IndexSignatureSpans`]).
    /// Coordinates are in the owning declaration's source file. Participates in
    /// node interning but never enters `parse_stable_hash`. `None` components
    /// for a synthetic index signature with no source site.
    pub spans: verter_type_expr::IndexSignatureSpans,
    /// Canonical file the index-signature DECLARATION lives in — set from the
    /// LOWERING scope of the object it is declared in, NOT from the value-type
    /// node. Mirrors [`SurfaceMember::declaration_origin`]: a `[k: string]:
    /// MissingType` index signature whose value type is unresolved /
    /// scope-less still has a real declaration file. `None` only for a
    /// genuinely synthetic index signature.
    pub declaration_origin: Option<Arc<str>>,
}

/// One-level surface view of a semantic node. Members are ordered to keep
/// hashing stable.
///
/// Extended in B1a to carry the full
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
/// Warm hits return a recorded `DepSignature` that folds into the
/// caller's dependency-fact set for the publish-side completion-fence
/// revalidation, so final-result validation is transitive, not
/// root-key-only.
///
/// `Ord` is derived for `DepSignatureInterner`
/// canonicalises bucket contents by sorting `(canonical, version)` pairs
/// before equality comparison so dep_signatures with the same logical
/// content but different declaration order share a single Arc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum DepVersion {
    WholeHash(HashValue),
    RouteGeneration(u64),
    ProjectGeneration(u64),
}

/// Dependency signature returned alongside every reusable cache read.
pub type DepSignature = Arc<[(Arc<str>, DepVersion)]>;

/// Returned by cache reads so callers can fold transitive dep facts into
/// their dependency-fact set for the publish-side completion-fence
/// revalidation.
///
/// Carries `walker_diagnostics` produced during the query's computation
/// (empty for queries that do not run the shallow-mode terminal-surface
/// walker) and a `cache_suppress` flag the memo consults to refuse
/// insertion when the build hit a pathological-input cap or a fatal
/// `QueryError`. Warm reads replay both fields transparently.
#[derive(Debug, Clone)]
pub struct CacheRead<T> {
    pub value: T,
    pub dep_signature: DepSignature,
    /// Walker diagnostics produced during this query's computation.
    /// Stored on the memo entry; warm reads replay transparently. Empty
    /// for queries that don't run the walker.
    pub walker_diagnostics: Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
    /// True if this query's computation (or any nested query) hit a
    /// fatal `QueryError` (BudgetExceeded / UnstableState) or the
    /// pathological-input cap. Aggregates via OR through nested queries'
    /// `cache_suppress`. The memo refuses insertion when this is true
    /// so the caller observes the suppressed result and subsequent
    /// requests cold-recompute.
    pub cache_suppress: bool,
}

impl<T> CacheRead<T> {
    /// Convert a `(value, dep_signature)` pair into a `CacheRead` with
    /// no walker diagnostics and `cache_suppress = false`. Used by
    /// build closures for queries that do not run the shallow-mode
    /// terminal-surface walker.
    #[inline]
    #[must_use]
    pub fn from_value_and_signature(value: T, dep_signature: DepSignature) -> Self {
        Self {
            value,
            dep_signature,
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Derivation / origin layer (plan B2 + Derivation/Origin Layer Contract)
// ──────────────────────────────────────────────────────────────────────────

/// Required edge kinds for the derivation/origin layer ( Derivation
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
    /// Single-hop projected-member payload for
    /// [`OriginEdgeKind::ProjectMember`]. Carries the projected member
    /// name plus a typed [`verter_audit::MemberEdgeProvenance`]
    /// discriminator that names WHY the edge was emitted (KeyOf
    /// enumeration, Mapped key enumeration, path-projection step,
    /// direct published-field emission). The provenance is set at
    /// every production emit site and consumed by the audit-validator's
    /// Rule-5 compliance check.
    ProjectedMember {
        /// Projected member name.
        name: Arc<str>,
        /// Structural provenance for this single-hop ProjectMember edge.
        provenance: verter_audit::MemberEdgeProvenance,
    },
    /// Alias-name payload for [`OriginEdgeKind::AliasResolve`].
    /// Distinct from [`OriginMeta::ProjectedMember`] so the audit
    /// bridge translates exhaustively without a `_ =>` fallback.
    AliasName(Arc<str>),
    /// Index expression for [`OriginEdgeKind::ProjectIndex`].
    Index(IndexKey),
    /// Full path segment list for [`OriginEdgeKind::ProjectPath`].
    Path(Arc<[PathSegment]>),
    /// Type-parameter name for [`OriginEdgeKind::SubstituteTypeParam`].
    SubstitutedParam(Arc<str>),
}

/// One origin edge: a derivation hop from a source-set to a result. Carries
/// the per-edge dep-signature snapshot that walkers merge into their fence
/// at hop-time ( — edges are the only dep-sig propagation route
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
/// Field set is normative against — every field listed there
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
    /// Number of times a joiner thread cooperatively blocked on an
    /// in-flight entry's condvar (one increment per `wait_while` return,
    /// including subsequent waits after an abort-retry).
    pub joined_waits: u64,
    /// Number of times a joiner re-entered dispatch because its in-flight
    /// entry was aborted by a canonical-invalidation sweep. Bounded per
    /// joined call by [`MAX_INFLIGHT_RETRIES`].
    pub inflight_aborted_retries: u64,
    /// Number of times a cold winner observed its in-flight entry
    /// aborted by a concurrent sweep and skipped the warm publish.
    pub cold_aborts_swept: u64,
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
    // ── F2 navigation-once + relation invariants ────────────────────
    pub decl_subexpression_lowering_count: u64,
    pub relation_check_count: u64,
    /// Per-K mapped-type materialiser invocations observed by the
    /// store — discriminating signal for the key-space-independent
    /// value hoist (`build_mapped_type`'s and
    /// `synthesise_mapped_surface`'s per-K loop short-circuit).
    /// `0` for hoist-eligible mapped types; `N = key_count` for
    /// K-dependent mapped types whose per-K materialiser must run.
    pub mapped_per_k_materializations: u64,
    /// Hash-cons memo hits on
    /// `substitute_semantic_type_param`. Discriminates the
    /// substitute cache: a second call with the same
    /// `(value_expr, parameter_node, arg)` triple bumps this
    /// counter and skips the recursive walk.
    pub substitute_memo_hits: u64,
    /// Hash-cons memo misses on
    /// `substitute_semantic_type_param`. Bumped when the cache has
    /// no entry for the queried triple and a fresh recursive walk
    /// runs.
    pub substitute_memo_misses: u64,
    /// Hash-cons memo hits on
    /// `evaluate_deferred_semantic_node_with_context`. Bumped when
    /// a `(node, context)` pair is served from the memo, skipping
    /// the recursive fix-point walk.
    pub evaluate_deferred_memo_hits: u64,
    /// Hash-cons memo misses on
    /// `evaluate_deferred_semantic_node_with_context`. Bumped when
    /// the cache has no entry for the queried pair and a fresh
    /// fix-point walk runs.
    pub evaluate_deferred_memo_misses: u64,
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
    /// same `build_project_path` invocation. Carries the
    /// cycle participants so diagnostics can render the chain end-to-end.
    AliasCycle { chain: Arc<[Arc<str>]> },
    /// Recursive back-edge sentinel emitted when dispatch detects a
    /// declaration expanding into itself (e.g., `type TreeNode = {
    /// children: TreeNode[] }`). `raise_node_to_type_expr` converts
    /// this to [`TypeExpr::RecursiveRef`] so the materialiser stops at
    /// the back-edge instead of recursing indefinitely. /
    /// recursion handling.
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
    /// A typed caller asked a query to narrow to a specific value domain
    /// (e.g. `execute_type_node` expects [`SemanticQueryValue::TypeNode`])
    /// but the query produced a different domain. Carries both
    /// discriminant tags so diagnostics can name the mismatch precisely.
    ValueDomainMismatch {
        expected: SemanticQueryValueTag,
        actual: SemanticQueryValueTag,
    },
}

// `SemanticNodeData` structurally interns under a compound
// `(payload, scope)` key, so every field transitively reachable from
// a variant must implement `Hash` / `Eq`. `QueryError::BudgetExceeded`
// wraps `BudgetExceededFailure` which has no `Hash`/`Eq`; treat all
// `BudgetExceeded` carriers as one identity for interning purposes
// (they are opaque error tokens, not structural distinguishers).
// Other variants compare by their fields.
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
            (
                Self::ValueDomainMismatch {
                    expected: a_e,
                    actual: a_a,
                },
                Self::ValueDomainMismatch {
                    expected: b_e,
                    actual: b_a,
                },
            ) => a_e == b_e && a_a == b_a,
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
            Self::ValueDomainMismatch { expected, actual } => {
                8u8.hash(state);
                expected.hash(state);
                actual.hash(state);
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
// Value domains
// ──────────────────────────────────────────────────────────────────────────

/// Canonical typed value a semantic query resolves to.
///
/// Every live [`SemanticQueryKey`] that PRODUCES a value produces
/// [`TypeNode`](Self::TypeNode): the interned graph node id for the resolved
/// type. The non-producing keys (`Relate`, `ResolveOverloadSet`) return `Miss`
/// and forward-declare the `Relation` / `OverloadSet` domains they will produce
/// once their reducers land. The remaining arms are shells with no live
/// producer — flow / contextual program analysis, declaration / augmentation
/// analysis, and diagnostic analysis. They carry honest data shapes so the
/// value domain is closed and exhaustive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticQueryValue {
    /// The interned graph node id for the resolved type — the only domain a
    /// live query produces today.
    TypeNode(SemanticNodeId),
    /// Narrowed / contextual type produced by flow or contextual program
    /// analysis. No live producer.
    ProgramAnalysis(ProgramAnalysisValue),
    /// Non-live SHELL variant. **NOT the merge/augmentation carrier.** The LIVE
    /// merged-declaration / module-augmentation carrier is the graph node
    /// [`SemanticNodeData::MergedDecl`] (interned by the peer-merge reducer and
    /// the same-file / cross-file augmentation stitch), surfaced on the wire as
    /// a `GraphTypeNode` (`GraphMergedDeclaration`, kinds 21–25, with anchors as
    /// nested `GraphDeclarationPart`). This value variant has no live producer
    /// and never crosses the wire — do not read it as the merge wire source.
    DeclarationAnalysis(DeclarationAnalysisValue),
    /// An ordered set of call/construct signatures (an overload set). The
    /// forward-declared value domain of
    /// [`SemanticQueryKey::ResolveOverloadSet`]: that key's spec row records
    /// this domain, but its `execute` arm is non-producing (returns `Miss`)
    /// until the overload-producing reducer that fills this carrier lands.
    /// No live producer.
    OverloadSet(Arc<[SignatureRef]>),
    /// The tri-state outcome of a relation query. No live producer; the
    /// live relation path is the separate relation memo.
    Relation(RelationPayload),
    /// Reserved native-checker seam for diagnostic analysis. NON-LIVE: no
    /// producer, no key spec row. Uses a local shell so this crate keeps no
    /// back-edge to `verter_tsc::CheckResult`.
    DiagnosticAnalysis(DiagnosticAnalysisShell),
}

/// Content-free discriminant for [`SemanticQueryValue`]. Used by
/// [`QueryError::ValueDomainMismatch`] to name a value's domain without a
/// payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticQueryValueTag {
    TypeNode,
    ProgramAnalysis,
    DeclarationAnalysis,
    OverloadSet,
    Relation,
    DiagnosticAnalysis,
}

impl SemanticQueryValue {
    /// The content-free discriminant tag for this value.
    #[must_use]
    pub fn tag(&self) -> SemanticQueryValueTag {
        match self {
            Self::TypeNode(_) => SemanticQueryValueTag::TypeNode,
            Self::ProgramAnalysis(_) => SemanticQueryValueTag::ProgramAnalysis,
            Self::DeclarationAnalysis(_) => SemanticQueryValueTag::DeclarationAnalysis,
            Self::OverloadSet(_) => SemanticQueryValueTag::OverloadSet,
            Self::Relation(_) => SemanticQueryValueTag::Relation,
            Self::DiagnosticAnalysis(_) => SemanticQueryValueTag::DiagnosticAnalysis,
        }
    }
}

/// A single call/construct signature, identified by its graph node.
/// No live producer; carried by the overload-set domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureRef {
    pub node: SemanticNodeId,
}

/// A type produced by flow / contextual program analysis (the narrowed or
/// contextual node). No live producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramAnalysisValue {
    pub node: SemanticNodeId,
}

/// Non-live SHELL payload for [`SemanticQueryValue::DeclarationAnalysis`]. It is
/// NOT the live merge carrier: the live merged-declaration / augmentation carrier
/// is the graph node [`SemanticNodeData::MergedDecl { contributors }`], reduced
/// to an `Object` / overload surface and emitted on the wire as a `GraphTypeNode`
/// (`GraphMergedDeclaration`). This shell has no live producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationAnalysisValue {
    pub contributors: Arc<[SemanticNodeId]>,
}

/// The forward-declared value-domain payload of a [`SemanticQueryKey::Relate`]
/// judgement — the carrier of [`SemanticQueryValue::Relation`].
///
/// Final-state shape (design "Decision 4 — Relation-proof acceptance"). The
/// payload carries the public relation [`RelationOutcome`], the inference
/// [`InferBinding`]s a binding-producing relation deposited, and an opaque
/// [`RelationProofId`] indexing the payload-side [`RelationProofTable`]: the
/// derivation / failure / cycle witness rides that table BY ID — it is never
/// embedded on the value and never crosses the typeinfo wire. The budget regime
/// is FOLDED into the outcome as [`RelationOutcome::BudgetExceeded`] rather than
/// carried as a separate field, so there is a single source of truth and an
/// `Assignable`-with-exceeded-budget contradiction is unrepresentable. There is
/// NO public `Unknown`: a deferred / opaque / undischarged relation has no
/// `SemanticQueryValue::Relation` form and routes through `ReturnOnly` in the
/// cold compute.
///
/// SHAPE only: no live `execute` producer fills it yet —
/// `ProjectSemanticDispatch::execute` returns `Miss` for `Relate`, and the
/// production authority `relate_nodes` emits the engine's transient
/// [`RelationResult`] into the dedicated relation memo. The relation reducer
/// (U2.RELATION_INFER) populates this payload and its proof table once it lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationPayload {
    /// The public relation outcome — `Assignable`, `NotAssignable`, or
    /// `BudgetExceeded`. NEVER `Unknown`.
    pub outcome: RelationOutcome,
    /// The inference bindings a binding-producing relation produced (the
    /// `infer T` captures a successful assignability judgement surfaces). Empty
    /// for a pure non-binding assignability check and for a `BudgetExceeded`
    /// outcome.
    pub bindings: Arc<[InferBinding]>,
    /// Opaque id into the payload-side [`RelationProofTable`]. The proof witness
    /// lives OFF this value (and off the typeinfo wire); a consumer that wants
    /// the derivation / failure / cycle detail dereferences the table by id.
    pub relation_proof: RelationProofId,
}

/// Public value-domain outcome of a relation query (assignability / subtype /
/// identity).
///
/// THREE-valued — there is no `Unknown`: a deferred / undischarged / opaque
/// relation has no public `SemanticQueryValue::Relation` form and routes through
/// `ReturnOnly` in the cold compute (design "Decision 4"). `BudgetExceeded` IS
/// public — so [`display_relation`](crate::semantic_query::display) can render
/// it — yet is `ReturnOnly` at the admission gate: expressible, never
/// warm-admitted.
///
/// The outcome is the MINIMAL public discriminant. `NotAssignable` is
/// FIELD-LESS by design (Decision 4): the failure reason and the failing
/// structural sub-relation do NOT ride the outcome — they live OFF the value on
/// the payload-side [`RelationProof::NotAssignable`] table entry
/// ([`RelationFailureCode`] + content-free [`SubRelationRef`]). Keeping the
/// reason off the outcome is load-bearing for the negative carve-out: a
/// published `NotAssignable` proof is content-free and never leaks a transient
/// `SessionId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationOutcome {
    /// The source relates to the target.
    Assignable,
    /// The source provably does NOT relate to the target. FIELD-LESS: the
    /// failure reason ([`RelationFailureCode`]) and failing structural
    /// sub-relation ride the payload-side [`RelationProof::NotAssignable`] table
    /// entry, never the outcome itself (design "Decision 4").
    NotAssignable,
    /// A budget / recursion cap stopped the relate before it decided.
    /// PUBLIC-but-never-warm: expressible and rendered, gated `ReturnOnly`.
    BudgetExceeded(BudgetExceededKind),
}

/// Forward-declared, reducer-extensible set of codes a relation provably fails
/// with, in DETERMINISTIC PRIORITY ORDER: declaration order IS priority,
/// fastest-reject / most-specific first.
///
/// When a relate fails for several reasons the relation engine selects the
/// highest-priority (earliest-declared) code as the
/// [`RelationProof::NotAssignable`] entry's `reason`. SHAPE only: the relation
/// engine (U2.RELATION_INFER) populates these and OWNS the taxonomy; the
/// declared variants are the currently-known failure reasons and their order is
/// the architectural priority contract consumers branch on. The enum is
/// `#[non_exhaustive]` so the relation reducer can add further reasons without a
/// breaking change for downstream crates — match arms outside this crate must
/// carry a wildcard. This code rides ONLY the payload-side proof table — never
/// the public [`RelationOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RelationFailureCode {
    /// Disagreeing primitive kinds (e.g. a string-literal `"a"` vs `number`).
    PrimitiveKindMismatch,
    /// Same primitive kind, different literal values.
    LiteralValueMismatch,
    /// The target requires a member the source lacks.
    MissingProperty,
    /// A member present on both sides has non-relating types.
    PropertyTypeMismatch,
    /// Call / construct signature parameter counts disagree.
    SignatureArityMismatch,
    /// A signature parameter (contravariant position) does not relate.
    SignatureParameterMismatch,
    /// A signature return (covariant position) does not relate.
    SignatureReturnMismatch,
    /// A generic type-argument violates its declared variance.
    VarianceMismatch,
    /// An excess property on a fresh object literal (excess-property check).
    ExcessProperty,
    /// A structural mismatch not captured by a more specific reason above.
    Structural,
}

/// Which budget tripped a [`RelationOutcome::BudgetExceeded`] (design Decision 4
/// admission row 4). SHAPE only: the budget accounting substrate is
/// U2.RELATION_INFER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetExceededKind {
    /// The relation worklist budget (deep structural recursion / conditional
    /// distribution).
    RelationBudget,
    /// The call-resolution budget (overload probing during inference).
    CallResolutionBudget,
    /// The flow-slice budget.
    FlowSliceBudget,
}

/// Opaque index into a [`RelationProofTable`]. Content-free (R6): it identifies
/// a proof slot, not any versioned content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelationProofId(pub u32);

/// Opaque id of a COMPLETED full `Relate` key that co-discharged a coinductive
/// cycle. Content-free (R6): the durable cycle proof references co-discharged
/// keys by this id rather than embedding a session-bearing [`RelateMemoKey`]
/// directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelateKeyId(pub u32);

/// One entry of a payload-side [`RelationProofTable`], addressed by an opaque
/// [`RelationProofId`]. FOUR shapes (design "Decision 4"). SHAPE only: the
/// proof-search substrate is U2.RELATION_INFER.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationProof {
    /// Positive structural derivation: which sub-relations held, member by
    /// member and across variance arms.
    Assignable { witness: DerivationTree },
    /// Negative: the failure reason plus the failing structural sub-relation.
    /// CONTENT-FREE — [`SubRelationRef`] carries no session-bearing full
    /// `Relate` key (no inference context, no `SessionId`, no `RelationContext`),
    /// which is exactly what lets a NEGATIVE member publish on the identity-leak
    /// axis (design §2.3).
    NotAssignable {
        reason: RelationFailureCode,
        failing_sub: SubRelationRef,
    },
    /// The budget / recursion cap that stopped the relate (rides a
    /// `ReturnOnly`-but-public [`RelationOutcome::BudgetExceeded`]).
    BudgetExceeded { cap: RecursionOrBudgetCap },
    /// The set of COMPLETED full `Relate` keys that co-discharged a coinductive
    /// cycle, referenced by opaque [`RelateKeyId`] — never by [`RelateMemoKey`]
    /// directly (design §4.1).
    CoinductiveCycle { keys: Arc<[RelateKeyId]> },
}

/// The payload-side table of relation proofs. A [`RelationPayload`] references
/// exactly one entry by [`RelationProofId`]; the table is the durable typed
/// carrier the relation reducer (U2.RELATION_INFER) populates. SHAPE only —
/// population is the reducer's job; this declares the carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationProofTable {
    pub proofs: Arc<[RelationProof]>,
}

/// Positive-derivation witness: the structural sub-relations that held to
/// discharge a relation, in structural order. SHAPE only: U2.RELATION_INFER
/// fills it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationTree {
    pub sub_derivations: Arc<[SubRelationRef]>,
}

/// Content-free reference to a structural sub-relation: the source / target
/// interned nodes plus the structural position they relate at. EXCLUDES any
/// session-bearing full `Relate` key (no inference context, no `SessionId`, no
/// `RelationContext`) — design §2.3 / Decision 4. SHAPE only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubRelationRef {
    pub source: SemanticNodeId,
    pub target: SemanticNodeId,
    pub position: SubRelationPosition,
}

/// The structural axis a [`SubRelationRef`] relates at. Forward-declared,
/// reducer-extensible: the relation reducer (U2.RELATION_INFER) owns this
/// taxonomy and the declared variants are the currently-known structural
/// positions. The enum is `#[non_exhaustive]` so the reducer can add further
/// positions without a breaking change for downstream crates — match arms
/// outside this crate must carry a wildcard. SHAPE only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SubRelationPosition {
    /// The root obligation between two whole types.
    Root,
    /// A named member / property.
    Member(Arc<str>),
    /// A call / construct signature parameter at the given index.
    Parameter(u32),
    /// A call / construct signature return.
    Return,
    /// A generic type-argument at the given index.
    TypeArgument(u32),
}

/// The budget / recursion cap that stopped a relate. Rides
/// [`RelationProof::BudgetExceeded`]. SHAPE only: U2.RELATION_INFER owns the
/// accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecursionOrBudgetCap {
    /// Which budget tripped.
    pub kind: BudgetExceededKind,
    /// The cap value the relate hit.
    pub limit: u32,
}

/// Reserved native-checker seam for diagnostic analysis. NON-LIVE: no
/// producer, no key spec row, never `verter_tsc::CheckResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticAnalysisShell;

/// A query value paired with the provenance of the work that produced it.
///
/// Provenance lives ONLY on the [`Value`](QueryResult::Value) arm of a
/// [`QueryResult`]; the `Recursive` / `Error` arms are control / failure arms
/// that never carry provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticQueryOutput<T> {
    pub value: T,
    pub provenance: ResultProvenance,
}

/// Provenance of a resolved value: how trustworthy the inputs that produced it
/// were. Provenance is shape-only at this layer: every result is
/// [`clean`](Self::clean). Taint producers and the taint join live with the
/// passes that introduce broken inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultProvenance {
    pub taint: ResultTaint,
}

impl ResultProvenance {
    /// Provenance for a value derived from fully resolved, well-formed inputs.
    #[must_use]
    pub fn clean() -> Self {
        Self {
            taint: ResultTaint::Clean,
        }
    }
}

/// Whether a resolved value was derived from broken or incomplete inputs.
///
/// Shape only at this layer — every value the dispatch produces is
/// [`Clean`](Self::Clean). Taint producers and the taint join live with the
/// passes that introduce broken inputs; this enum just closes the domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultTaint {
    /// Every input was fully resolved and well-formed.
    Clean,
    /// Some, but not all, of the value was derived from a broken input.
    Partial(BrokenInputClass),
    /// The value as a whole stands on a broken input.
    Broken(BrokenInputClass),
}

/// The class of broken input that tainted a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrokenInputClass {
    /// The source for an input did not parse.
    SyntaxError,
    /// A referenced symbol could not be resolved.
    UnresolvedReference,
    /// A declaration was found but its body was incomplete.
    IncompleteDeclaration,
    /// A cached read observed a torn / mid-flight state.
    TornRead,
    /// A required dependency was absent.
    MissingDependency,
}

/// Narrow a domain-agnostic query result to the [`TypeNode`] domain.
///
/// [`SemanticQueryApi::execute_type_node`] is defined in terms of this free
/// function so the narrowing decision lives in exactly one place. A non-
/// [`TypeNode`] value becomes a [`QueryError::ValueDomainMismatch`]; provenance
/// rides through unchanged on the [`Value`](QueryResult::Value) arm.
///
/// [`TypeNode`]: SemanticQueryValue::TypeNode
#[must_use]
pub(crate) fn narrow_output_to_type_node(
    result: QueryResult<SemanticQueryOutput<SemanticQueryValue>>,
) -> QueryResult<SemanticQueryOutput<SemanticNodeId>> {
    match result {
        QueryResult::Value(out) => match out.value {
            SemanticQueryValue::TypeNode(node) => QueryResult::Value(SemanticQueryOutput {
                value: node,
                provenance: out.provenance,
            }),
            other => QueryResult::Error(QueryError::ValueDomainMismatch {
                expected: SemanticQueryValueTag::TypeNode,
                actual: other.tag(),
            }),
        },
        QueryResult::Recursive(n) => QueryResult::Recursive(n),
        QueryResult::Error(e) => QueryResult::Error(e),
    }
}

/// Drop provenance from a typed-node result, recovering the node-or-control
/// [`QueryResult`] the convenience wrappers expose.
#[must_use]
pub(crate) fn strip_output_provenance<T>(
    result: QueryResult<SemanticQueryOutput<T>>,
) -> QueryResult<T> {
    match result {
        QueryResult::Value(out) => QueryResult::Value(out.value),
        QueryResult::Recursive(n) => QueryResult::Recursive(n),
        QueryResult::Error(e) => QueryResult::Error(e),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Class / namespace / enum / overload key context shapes
// ──────────────────────────────────────────────────────────────────────────

/// Which symbol-space half of a class declaration a
/// [`SemanticQueryKey::ResolveClassSurface`] query addresses.
///
/// A class occupies BOTH symbol spaces (the dual-space model). The
/// `side` is a MANDATORY identity discriminator: two `ResolveClassSurface`
/// keys that differ only in `side` are NON-equal and occupy DISTINCT memo
/// slots (`FamilyKey::ResolveClassSurface` carries `side`).
///
/// - [`Instance`](Self::Instance) → the TYPE-space half. The instance
///   surface is `Instantiate(type_slot, Shallow)`.
/// - [`Static`](Self::Static) → the VALUE-space half. The static surface
///   is `TypeOf(value_root)` (the constructor value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClassSurfaceSide {
    /// Instance side — the TYPE-space half of the class.
    Instance,
    /// Static side — the VALUE-space half (constructor value) of the class.
    Static,
}

/// Extra env (beyond the slot's intrinsic `T,L,J`) plus the projection
/// axis a [`SemanticQueryKey::ResolveClassSurface`] value depends on
/// (env dims `R T L J`).
///
/// Per R21 this carries ONLY the dimensions the class-surface value
/// genuinely depends on beyond the [`ResolvedDeclSlotIdentity`]:
///
/// - `resolve_env_hash` (`R`) — the surface resolves imported heritage /
///   member-type references.
/// - `mode` — the projection rung the composed surface is produced under
///   (mirrors how [`ProjectionReductionContext`] carries `mode`).
///
/// The substitution axis is carried by the key's `type_args` field, not
/// duplicated here. There is deliberately NO `parse_env_hash` (`P`): the
/// composed surface identity-routes `Instantiate` + `TypeOf` (both `R`-env
/// keys) and does not read the parsed body skeleton at query time, so
/// keying on `parse_env` would be a dead axis (R21). The `P` dimension
/// enters when the decorator-reading surface reducer is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassSurfaceContext {
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
    /// Projection rung the composed surface is produced under.
    pub mode: ProjectionMode,
}

/// Extra env (beyond the slot's intrinsic `T,L,J`) plus the projection
/// axis a [`SemanticQueryKey::ResolveAmbientNamespace`] value depends on
/// (env dims `R T L J`).
///
/// Per R21 this carries ONLY:
///
/// - `resolve_env_hash` (`R`) — namespace members resolve imported
///   references.
/// - `mode` — the projection rung the namespace surface is produced under.
///
/// The substitution axis is carried by the key's `type_args` field. There
/// is deliberately NO `parse_env_hash` (`P`): the value does not read the
/// parsed body skeleton at query time, so keying on `parse_env` would be a
/// dead axis (R21). The `P` dimension enters when the body-reading
/// namespace-member reducer is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmbientNamespaceContext {
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
    /// Projection rung the namespace surface is produced under.
    pub mode: ProjectionMode,
}

/// Extra env (beyond the slot's intrinsic `T,L,J`) a
/// [`SemanticQueryKey::ResolveEnum`] value depends on (env dims `R T L J`).
///
/// Per R21 this carries ONLY `resolve_env_hash` (`R`): an enum
/// declaration is not generic (NO substitution axis) and enum-member
/// analysis does not read the parsed body skeleton at query time (NO
/// `parse_env`), so the only extra dimension beyond the slot is the
/// import / name-resolution env.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumContext {
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
}

/// Extra env (beyond the slot's intrinsic `T,L,J`) a
/// [`SemanticQueryKey::ResolveOverloadSet`] value depends on (env dims
/// `R T L J`).
///
/// Per R21 this carries ONLY `resolve_env_hash` (`R`) — signature
/// lowering resolves imported parameter / return references. The
/// substitution axis is carried by the key's `type_args` field. NO
/// `parse_env`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverloadSetContext {
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
}

/// Env a [`SemanticQueryKey::ApparentType`] value depends on (env dims
/// `T L J`).
///
/// `ApparentType` has NO slot (its identity core is the `base`
/// [`SemanticNodeId`]), so the R21 env dimensions ride here IN the context
/// rather than inside a [`ResolvedDeclSlotIdentity`]. Per R21 this carries
/// ONLY the dimensions the apparent surface genuinely depends on:
///
/// - `type_env_hash` (`T`) — the active type-env (e.g. `strictNullChecks`).
/// - `lib_env_hash` (`L`) — the apparent surface of a primitive widens to
///   its lib wrapper (`string` → `String`'s members), so the value is a
///   function of the lib-member index.
/// - `project_identity` (`J`) — project isolation.
///
/// There is deliberately NO `resolve_env_hash` (`R`) and NO `parse_env_hash`
/// (`P`): the apparent surface is a function of the base node + the
/// lib/type/project env, not of import resolution or the parsed body
/// skeleton, so keying on those would be a dead axis (R21). The
/// substitution axis is NOT carried here — the `base` node is already
/// substituted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApparentTypeContext {
    /// Type-env dimension (`T`).
    pub type_env_hash: HashValue,
    /// Lib-env dimension (`L`).
    pub lib_env_hash: HashValue,
    /// Project-isolation dimension (`J`).
    pub project_identity: u32,
}

/// Env a [`SemanticQueryKey::TemplateLiteralReduce`] value depends on (env
/// dims `R T L J`).
///
/// `TemplateLiteralReduce` has NO slot (its identity core is the `pattern`
/// quasis + the `args` expression nodes), so the R21 env dimensions ride
/// here IN the context. Per R21 this carries `R T L J`:
///
/// - `resolve_env_hash` (`R`) — an arg expression may resolve imported
///   references on its own step.
/// - `type_env_hash` (`T`), `lib_env_hash` (`L`) — the standard structural
///   reduction env.
/// - `project_identity` (`J`) — project isolation.
///
/// There is deliberately NO `parse_env_hash` (`P`): the reduction operates
/// over already-lowered interned arg nodes (content-version rooted via
/// `ReadSetSignature`), not a fresh parsed body skeleton. The substitution
/// axis rides on the key's `args` field, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TemplateLiteralReduceContext {
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
    /// Type-env dimension (`T`).
    pub type_env_hash: HashValue,
    /// Lib-env dimension (`L`).
    pub lib_env_hash: HashValue,
    /// Project-isolation dimension (`J`).
    pub project_identity: u32,
}

/// A point in a program — the identity core of
/// [`SemanticQueryKey::FlowNarrowingAt`] and
/// [`SemanticQueryKey::ContextualTypeAt`].
///
/// A program point names a SOURCE LOCATION inside a canonical file: the
/// position at which a flow-narrowed or contextually-expected type is being
/// asked for (e.g. the use-site of an identifier under a sequence of
/// type-guard branches, or an argument position whose contextual type is
/// the parameter type of the called signature).
///
/// **Content-free (R6).** The identity is `(canonical_id, offset)` only — a
/// position, never a content/version hash. The file's live content version
/// is re-sourced at value-compute time (when the U6 flow / contextual
/// reducer lands) and rooted on the cached value via `ReadSetSignature`, NOT
/// folded into this key. Carries no projection mode: flow narrowing and
/// contextual typing are not projection-rung operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProgramPointId {
    /// Canonical id of the file the program point lives in.
    pub canonical_id: Arc<str>,
    /// Byte offset of the program point within the file.
    pub offset: u32,
}

/// Env a [`SemanticQueryKey::FlowNarrowingAt`] / [`ContextualTypeAt`] value
/// depends on (env dims `P R T L J` — the full set).
///
/// [`ContextualTypeAt`]: SemanticQueryKey::ContextualTypeAt
///
/// Both keys have NO slot (their identity core is the
/// [`ProgramPointId`]), so the R21 env dimensions ride here IN the context.
/// Program analysis is the WIDEST-env operation in the key surface — unlike
/// the structural reducers it genuinely depends on every dimension:
///
/// - `parse_env_hash` (`P`) — flow narrowing and contextual typing walk the
///   program point's parsed body / control-flow skeleton, so the value is a
///   function of the parse env (the lowered statement/expression structure
///   the analysis traverses), not just already-interned nodes.
/// - `resolve_env_hash` (`R`) — the narrowed/contextual surface resolves
///   imported references on its own step (a guard against an imported type,
///   a contextual parameter type from an imported signature).
/// - `type_env_hash` (`T`) — narrowing is governed by the type env
///   (`strictNullChecks` decides whether `x` narrows out `null`).
/// - `lib_env_hash` (`L`) — guards and contextual surfaces reach lib types
///   (`Array`, `Promise`, the global `this`).
/// - `project_identity` (`J`) — project isolation.
///
/// The substitution axis is NOT carried here — a program point is already
/// instantiated in its enclosing context. This is the only context that
/// carries the full `P R T L J` set: it is `EnvDimMask::all()`, not a
/// structural / resolve subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProgramAnalysisContext {
    /// Parse-env dimension (`P`).
    pub parse_env_hash: HashValue,
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
    /// Type-env dimension (`T`).
    pub type_env_hash: HashValue,
    /// Lib-env dimension (`L`).
    pub lib_env_hash: HashValue,
    /// Project-isolation dimension (`J`).
    pub project_identity: u32,
}

/// Which relation a [`SemanticQueryKey::Relate`] judgement asks for.
///
/// The relation kind is part of relation IDENTITY: `S` assignable-to `T` is a
/// genuinely different question from `S` identical-to `T`, and the two must
/// occupy distinct memo slots. SHAPE only — the per-kind relation algorithm is
/// U2.RELATION_INFER. The current `relate_nodes` engine computes
/// [`RelationKind::Assignable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationKind {
    /// `S` is assignable to `T` — the default relation TS checks at
    /// assignments / argument passing.
    Assignable,
    /// `S` is a subtype of `T`.
    Subtype,
    /// `S` is a strict subtype of `T` (no `any`/`unknown` widening).
    StrictSubtype,
    /// `S` and `T` are mutually identical.
    Identity,
    /// `S` and `T` are comparable (the bidirectional relation behind
    /// `switch` / `===`).
    Comparable,
}

impl Default for RelationKind {
    /// [`Assignable`](Self::Assignable) — the relation the current engine
    /// computes.
    fn default() -> Self {
        Self::Assignable
    }
}

/// The comparison policy that governs a relation and is part of relation
/// IDENTITY — the three §2.7 / §4.0 policy axes: overload selection,
/// excess-property checking, and variance (including method-parameter
/// bivariance). Two judgements over the same nodes that differ in any policy
/// axis can reach a different OUTCOME / bindings, so they are DISTINCT and must
/// not share a memo slot. SHAPE only: the policy-driven comparison substrate is
/// U2.RELATION_INFER.
///
/// `report_errors` is deliberately ABSENT: error elaboration is a diagnostic
/// side-effect, not a relation-identity axis — it does not change the
/// outcome/bindings, so folding it into the key would over-split the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RelationPolicy {
    /// How an overloaded callee's signatures are selected when relating
    /// against it (§2.7:705).
    pub overload_selection: OverloadSelectionPolicy,
    /// Excess-property checking applies to a fresh object-literal source.
    pub excess_property_check: bool,
    /// Variance regime — including the method-parameter bivariance quirk
    /// (§4.0:1176-1182).
    pub variance: VariancePolicy,
}

/// How an overloaded callee's signatures are selected during a relation
/// (§2.7:705 overload-selection axis). Closed enum, content-free (R6). SHAPE
/// only: the overload-resolution substrate is U2.RELATION_INFER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OverloadSelectionPolicy {
    /// Relate against the WHOLE overload set — every target call/construct
    /// signature must be satisfied. This is the structural object-relation
    /// default (matches the current engine's `relate_objects` signature loop).
    #[default]
    All,
    /// Select the first applicable overload (call-resolution order) and relate
    /// against it alone.
    FirstApplicable,
}

/// The variance regime a relation comparison runs under (§4.0:1176-1182).
/// Distinguishes TS's method-parameter bivariance from strict contravariance —
/// the policy flag the relation reads to decide method-parameter variance,
/// rather than a scattered per-call-site special-case. Closed enum,
/// content-free (R6). SHAPE only: the variance-driven comparison substrate is
/// U2.RELATION_INFER. NOTE: this names the policy REGIME, not the MEASURED
/// variance of a generic parameter (which §4.1's relation measures separately
/// via the marker-probe fixed point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VariancePolicy {
    /// Method-shaped members' parameters relate BIVARIANTLY — the
    /// non-`strictFunctionTypes` method rule (TS's always-on default for
    /// method-shaped members).
    #[default]
    MethodParameterBivariance,
    /// Method/function parameters relate strictly CONTRAVARIANTLY — the
    /// `strictFunctionTypes` regime with no method-bivariance exemption.
    StrictContravariance,
}

/// Whether the relation SOURCE carries freshness — part of relation IDENTITY.
///
/// A FRESH object-literal type is subject to excess-property checking against
/// its target; a widened (REGULAR) source is not. The same `(source, target)`
/// nodes therefore relate differently depending on freshness, so it is a key
/// discriminator. SHAPE only: the freshness-tracking substrate is
/// U2.RELATION_INFER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreshnessKey {
    /// The source is a fresh literal / object-literal type — excess-property
    /// checking applies.
    Fresh,
    /// The source has been widened to its regular type.
    Regular,
}

impl Default for FreshnessKey {
    /// [`Regular`](Self::Regular) — the widened default.
    fn default() -> Self {
        Self::Regular
    }
}

/// Content-free projection of an inference session (design §4.2) — the
/// cache-identity fingerprint of the active inference setup a relation runs
/// WITHIN, NOT a standalone bag of axes assembled independently of the session.
///
/// A binding-producing relation runs inside the enclosing transaction's active
/// `InferenceSession`; a judgement discharged under one session's setup must NOT
/// warm-hit a judgement discharged under a different setup, so this projection
/// is part of relation identity. The six axes are exactly the fields the active
/// session carries (§4.2), projected content-free onto the cache key.
///
/// **Content-free (R6).** Every axis is a node-set interning identity, a closed
/// enum, or a small occurrence-local mask — never an env / content / parse /
/// version hash and never a `fact_dep_signature`. SHAPE only: the
/// inference-session substrate is U2.RELATION_INFER.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct InferenceContextKey {
    /// Which type parameters are open / inferable in this session.
    pub inferable_params: InferableParamSetId,
    /// The covariant / contravariant / invariant variance MEASUREMENT pass this
    /// relation runs under (§4.0). Names the pass; does not settle measured
    /// variance.
    pub variance_phase: VariancePhase,
    /// Candidate priority — return-type vs argument vs naked-type-parameter
    /// inference position (§4.2).
    pub candidate_priority: InferenceCandidatePriority,
    /// The occurrence-local `NoInfer` suppression mask in effect (§1.2 / §4.2).
    pub no_infer_mask: NoInferMask,
    /// `<const T>` const-ness propagation policy (§4.2).
    pub const_param_policy: ConstParamPolicy,
    /// Whether / how a contextual target drives inference in this session
    /// (§4.2).
    pub contextual_inference_mode: ContextualInferenceMode,
}

/// Content-free identity of the set of inferable (open) type parameters in an
/// inference session (§4.2). Realised as the interned set of graph node ids —
/// the same content-free realisation the graph uses for node sets elsewhere
/// (e.g. [`SemanticQueryKey::NormalizeUnion`]) — never a content/version hash
/// (R6). `Default` / [`empty`](Self::empty) is the empty set (no open
/// parameters). SHAPE only: the interning substrate is U2.RELATION_INFER.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferableParamSetId(pub Arc<[SemanticNodeId]>);

impl InferableParamSetId {
    /// The empty inferable-parameter set (no open type parameters).
    #[must_use]
    pub fn empty() -> Self {
        Self(Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()))
    }

    /// Construct from an ordered set of interned type-parameter node ids.
    #[must_use]
    pub fn new(params: Arc<[SemanticNodeId]>) -> Self {
        Self(params)
    }
}

impl Default for InferableParamSetId {
    fn default() -> Self {
        Self::empty()
    }
}

/// The variance MEASUREMENT pass a binding-producing relation runs under
/// (§4.0). Names the pass — it does NOT settle the MEASURED variance of a
/// generic parameter (which §4.1's relation measures separately via the
/// marker-probe fixed point). Closed enum, content-free (R6). SHAPE only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VariancePhase {
    /// Covariant measurement pass — the default outbound direction.
    #[default]
    Covariant,
    /// Contravariant measurement pass.
    Contravariant,
    /// Invariant measurement pass (both directions must hold).
    Invariant,
}

/// Inference candidate priority — which inference position is in scope when a
/// relation deposits a candidate (§4.2). Closed enum, content-free (R6). SHAPE
/// only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InferenceCandidatePriority {
    /// Ordinary argument-position inference — the baseline priority.
    #[default]
    Argument,
    /// Return-type-position inference (lower priority than naked-type-parameter
    /// inference, higher precedence semantics handled by the engine).
    ReturnType,
    /// Naked-type-parameter inference (a bare `T` in the source position).
    NakedTypeParameter,
}

/// The occurrence-local `NoInfer<T>` suppression mask in effect for a relation
/// (§1.2 / §4.2). A content-free bitset of the occurrence-local positions where
/// inference is suppressed; `Default` / [`empty`](Self::empty) is no
/// suppression. Never a content/version hash (R6). SHAPE only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoInferMask(pub u64);

impl NoInferMask {
    /// The empty mask — no `NoInfer` suppression in effect.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }
}

/// `<const T>` const-ness propagation policy in effect for a relation (§4.2).
/// Closed enum, content-free (R6). SHAPE only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConstParamPolicy {
    /// No const-ness propagation — the ordinary (non-`const`) type parameter.
    #[default]
    NonConst,
    /// `<const T>` const-ness propagation is active for this inference.
    Const,
}

/// Whether / how a contextual target drives inference in a session (§4.2).
/// Closed enum, content-free (R6). SHAPE only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContextualInferenceMode {
    /// No contextual target drives this inference.
    #[default]
    None,
    /// A contextual target (e.g. the expected return type) drives inference.
    Contextual,
}

/// The env + substitution + projection-reduction context a
/// [`SemanticQueryKey::Relate`] value depends on (design §2.7:711-718).
///
/// `Relate` has NO slot (its identity core is the `source` / `target` nodes
/// plus the relation-kind / policy / freshness / inference discriminators), so
/// the env dimensions ride here IN the context. Per design §2.7 this carries
/// the `R T L J` env set PLUS the canonical substitution hash and the
/// projection-reduction context:
///
/// - `resolve_env_hash` (`R`) — relating imported surfaces resolves their
///   references on the relation's own step.
/// - `type_env_hash` (`T`) — `strictNullChecks` / `strictFunctionTypes` change
///   the relation outcome.
/// - `lib_env_hash` (`L`) — relations reach lib types (`Array`, `Promise`).
/// - `project_identity` (`J`) — project isolation.
/// - `substitution` — the canonical hash of the active substitution
///   environment (the same canonical-substitution hash the flow / call keys
///   use). Two relations over the same `(source, target)` under a different
///   ambient substitution can relate differently, so this is part of identity.
/// - `projection_reduction` — the reduction context relation descent runs
///   under (structural-transit vs published reduction). Load-bearing: a
///   relation descended via structural transit reads a different surface than
///   one descended via published reduction.
///
/// There is deliberately NO `parse_env_hash` (`P`): a relation operates over
/// already-lowered interned `source` / `target` nodes (content-version rooted
/// via `ReadSetSignature`), not a fresh parsed body skeleton. There is NO
/// `project_config_hash` (R21) and NO content / `parse_stable_hash` /
/// `fact_dep_signature` (R6) — every field is a content-free key dimension.
///
/// **Env-sourcing note.** `relate_nodes` populates the env fields from the
/// WORKSPACE-GLOBAL host view (the established one-engine dispatch convention),
/// not per-canonical: per-judgement multi-project env threading is a
/// cross-cutting concern owned by U2.RELATION_INFER, not introduced here. The
/// fields ARE real env hashes (e.g. `project_identity` is the live
/// [`crate::file_artifact_store::ProjectIdentity`] hash), but isolation is at
/// workspace-env granularity, not per-judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelationContext {
    /// Import / name-resolution dimension (`R`).
    pub resolve_env_hash: HashValue,
    /// Type-env dimension (`T`).
    pub type_env_hash: HashValue,
    /// Lib-env dimension (`L`).
    pub lib_env_hash: HashValue,
    /// Project-isolation dimension (`J`) — the live `ProjectIdentity` hash.
    pub project_identity: HashValue,
    /// Canonical hash of the active substitution environment (§2.7:716).
    pub substitution: SubstitutionCanonicalHash,
    /// Reduction context relation descent runs under (§2.7:717).
    pub projection_reduction: ProjectionReductionContext,
}

impl Default for RelationContext {
    /// All-zero env, empty canonical substitution, and the structural-transit
    /// reduction context the relation engine descends under (it consumes
    /// surfaces for assignability decisions, never publication).
    fn default() -> Self {
        Self {
            resolve_env_hash: HashValue::default(),
            type_env_hash: HashValue::default(),
            lib_env_hash: HashValue::default(),
            project_identity: HashValue::default(),
            substitution: SubstitutionCanonicalHash::empty(),
            projection_reduction: ProjectionReductionContext::structural_transit(),
        }
    }
}

/// Canonical hash of the active substitution environment (design §2.7:716) —
/// the same canonical-substitution hash the flow / call keys use.
///
/// **Content-free (R6).** A hash over the canonical substitution MAPPING, never
/// a file content/version hash. [`empty`](Self::empty) / `Default` is the
/// canonical hash of the EMPTY substitution (no ambient type-parameter
/// bindings) — the absence of any substitution is itself a canonical value, not
/// a missing axis. The relation key carries THIS hash, never raw `type_args`.
/// SHAPE only: the substitution-canonicalisation substrate is U2.RELATION_INFER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SubstitutionCanonicalHash(pub HashValue);

impl SubstitutionCanonicalHash {
    /// The canonical hash of the empty substitution environment.
    #[must_use]
    pub const fn empty() -> Self {
        Self([0u8; 16])
    }
}

/// The relation memo's query-identity key — the full `Relate` identity.
///
/// Mirrors the [`SemanticQueryKey::Relate`] identity fields exactly: the
/// relation memo
/// ([`BudgetedRelationMemo`](crate::semantic_query_memo::SemanticGraphStore))
/// is keyed by THIS, never by the bare `(source, target)` pair. Two relation
/// judgements over the same nodes but a different relation kind / policy /
/// source freshness / inference context / env are DISTINCT and occupy distinct
/// memo slots.
///
/// Version rooting stays on the cached VALUE (the `ReadSetSignature` carrier +
/// `validated_at_generation`), never in this key — env hashes are content-free
/// key dimensions (R6), the same convention as the family memo's
/// `resolve_env_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelateMemoKey {
    /// The source node of the relation.
    pub source: SemanticNodeId,
    /// The target node of the relation.
    pub target: SemanticNodeId,
    /// Which relation is asked (assignable / subtype / identity / …).
    pub relation: RelationKind,
    /// Policy flags that govern the comparison.
    pub policy: RelationPolicy,
    /// Whether the source carries freshness.
    pub source_freshness: FreshnessKey,
    /// The inference session this relation runs within, if any.
    pub inference_context: Option<InferenceContextKey>,
    /// The `R T L J` env the relation outcome depends on.
    pub context: RelationContext,
}

impl RelateMemoKey {
    /// The current engine's relation identity for `(source, target)`:
    /// [`RelationKind::Assignable`], default [`RelationPolicy`], regular
    /// (widened) [`FreshnessKey`], NO inference context, under `context`.
    ///
    /// This is the identity `relate_nodes` keys the memo under today — the
    /// relation engine computes assignability; the kind / policy / freshness /
    /// inference axes become live discriminators with U2.RELATION_INFER.
    #[must_use]
    pub fn assignable(
        source: SemanticNodeId,
        target: SemanticNodeId,
        context: RelationContext,
    ) -> Self {
        Self {
            source,
            target,
            relation: RelationKind::Assignable,
            policy: RelationPolicy::default(),
            source_freshness: FreshnessKey::default(),
            inference_context: None,
            context,
        }
    }
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
    /// Generic instantiation of `base` with the supplied `args`.
    ///
    /// `context` carries the reduction context: `mode` controls how the
    /// decl body is lowered after substitution (Navigate keeps shells
    /// lazy, Expanded fully reduces, Shallow walks one structural level);
    /// `demand` distinguishes publication callers (`Published`) from
    /// internal transit callers (`StructuralTransit`). Under
    /// `StructuralTransit` nested `keyof` / mapped reductions carrier-
    /// stop so spurious member materialisation does not run.
    ///
    /// The memo splits per context so `(Shallow, StructuralTransit)`
    /// and `(Shallow, Published)` evaluations do not collide on a
    /// single shared entry.
    ///
    /// `base` is a content-free [`DeclKey`] (R6) — version-rooting
    /// lives on the cached value's `ReadSetSignature.facts` and
    /// `self_root_canonicals`, re-sourced at value-build time from
    /// the live indexed view.
    Instantiate {
        base: DeclKey,
        args: Arc<[SemanticNodeId]>,
        context: ProjectionReductionContext,
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
    /// `keyof base` deferred / reduced lookup.
    ///
    /// `context` gates whether the build reifies the keyspace as a
    /// union of literal anchors (with one `ProjectMember` edge per
    /// literal) or returns a [`SemanticNodeData::KeyOf`] carrier. Only
    /// `Published + Expanded` admits reification — see
    /// [`may_reduce_operator`].
    KeyOf {
        base: SemanticNodeId,
        context: ProjectionReductionContext,
    },
    /// Mapped-type rewrite for `{ [K in source]: mapper.value_expr }`.
    ///
    /// `context` gates whether the build enumerates the source's keys
    /// and materialises a produced surface (with per-member
    /// `ProjectMember` edges), or returns a [`SemanticNodeData::Mapped`]
    /// carrier without member materialisation. Only `Published +
    /// Expanded` admits materialisation — see [`may_reduce_operator`].
    MappedType {
        source: SemanticNodeId,
        mapper: MapperKey,
        context: ProjectionReductionContext,
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
    ///
    /// `context` carries the reduction context: `mode` controls the
    /// terminal projection mode (Navigate/Shallow/Expanded/Identity/
    /// Skeleton); `demand` distinguishes publication callers
    /// (`Published`) from internal transit callers
    /// (`StructuralTransit`). The memo splits per context so a
    /// `StructuralTransit/Shallow` projection does not collide with a
    /// `Published/Shallow` projection on the same `(base, path)` — they
    /// are distinct evaluations. Parity with `Instantiate` / `KeyOf` /
    /// `MappedType` so a future transit caller dispatching `ProjectPath`
    /// does not poison the publication slot.
    ProjectPath {
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        context: ProjectionReductionContext,
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
    /// Full-identity relation query between `source` and `target` semantic
    /// nodes.
    ///
    /// The identity is NOT the bare `(source, target)` pair: it carries the
    /// relation `relation` (assignable / subtype / identity / …), the comparison
    /// `policy`, the `source_freshness` discriminator, an optional
    /// `inference_context` (the inference session the relation runs within), and
    /// the `R T L J` env `context`. Two judgements over the same nodes that
    /// differ in any of these are DISTINCT and occupy distinct memo slots —
    /// `relate_nodes` keys the dedicated
    /// [`relation_memo`](crate::semantic_query_memo::SemanticGraphStore) by the
    /// full [`RelateMemoKey`], dep-signature fenced.
    ///
    /// **Forward-declared value domain.** The spec row records value domain
    /// [`SemanticQueryValueTag::Relation`]
    /// (`SemanticQueryValue::Relation(RelationPayload)`). The `execute` arm is
    /// non-producing (returns `Miss`): the current production authority is
    /// `relate_nodes`, which emits the engine's [`RelationResult`] into the
    /// relation memo. The per-kind / per-policy / inference-aware relation
    /// algorithm — and the reducer that fills the `RelationPayload` outcome /
    /// proof / budget carrier — is U2.RELATION_INFER. Until then the engine
    /// computes [`RelationKind::Assignable`] and the kind / policy / freshness /
    /// inference axes are identity discriminators with a fixed default value.
    ///
    /// All identity fields are content-free (R6): node ids, content-free kind /
    /// policy / freshness enums, content-free inference targets, and env hashes
    /// (env hashes are key dimensions, never file content/version hashes).
    Relate {
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
        policy: RelationPolicy,
        source_freshness: FreshnessKey,
        inference_context: Option<InferenceContextKey>,
        context: RelationContext,
    },
    /// Resolve a Vue macro (`defineProps`, `defineEmits`, `defineSlots`,
    /// `defineModel`, `defineExpose`, `defineOptions`, `withDefaults`)
    /// payload to a single `SemanticNodeId` representing the macro's
    /// effective TypeExpr.
    ///
    /// Binding amendment. This is the SOLE new variant
    /// introduced in the other 3 originally proposed
    /// (`MaterializeSurface`, `ResolvePublicInstance`,
    /// `ResolveFallthroughSurface`) are non-variant dispatch helpers
    /// that compose existing variants and read the
    /// `ComponentMetaResultDb<ComponentMetaAnalysis>` sidecar.
    ///
    /// `owner` is the synthetic SFC declaration identity (`canonical_id`
    /// = the SFC file path, `decl_name` per repo convention).
    /// `macro_index` is the stable index into `ScriptAnalysisSnapshot.macros`
    /// per `macro_kind` is the semantic-level
    /// [`AnalyzedMacroKind`], NOT [`verter_semantic::analysis::template::MacroKind`].
    /// `type_args` carries the macro's type arguments (already lowered to
    /// `SemanticNodeId`s by the caller). `mode` selects the projection
    /// mode for downstream type lowering inside the macro body.
    ///
    /// The body reuses the sidecar's `AnalyzedMacro` (no AST re-walk per
    /// §A14) for emit/model construction. Per `ResolveMacroPayload`'s
    /// closure rules, dispatch resolves to:
    /// - `DefineProps` / `WithDefaults`: 0 args → `Opaque(Miss)`; 1 arg
    ///   → arg unchanged; ≥2 args → `NormalizeIntersection`.
    /// - `DefineEmits`: build `Object` whose members are
    ///   `name → tuple-of-params` from the parsed type-argument's
    ///   properties + call signatures.
    /// - `DefineSlots`: build `Object` from slot members (function-shape
    ///   values).
    /// - `DefineModel`: build `Object` with `model_name → T` and
    ///   `update:<model_name> → (val: T) -> void` from `analyzed.model_name`
    ///   and `type_args[0]`.
    /// - `DefineExpose` / `DefineOptions`: 0 args → `Opaque(Miss)`;
    ///   else `type_args[0]` unchanged.
    ///
    /// `owner` is a content-free [`DeclKey`] (R6) — version-rooting
    /// lives on the cached value's `ReadSetSignature.facts` and
    /// `self_root_canonicals`, re-sourced at value-build time from
    /// the live indexed view.
    ResolveMacroPayload {
        owner: DeclKey,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: Arc<[SemanticNodeId]>,
        mode: ProjectionMode,
    },
    /// Resolve the instance OR static surface of a class declaration
    /// (the dual-space model).
    ///
    /// `decl_slot` is the content-free [`ResolvedDeclSlotIdentity`] of the
    /// class declaration; `type_args` are its already-lowered generic
    /// arguments (part of semantic identity); `side` selects the instance
    /// (TYPE-space) or static (VALUE-space) half and is a MANDATORY
    /// identity discriminator — two keys differing only in `side` are
    /// non-equal and occupy distinct memo slots; `context` carries the
    /// extra env (`R`) plus the projection rung the value depends on
    /// (R21).
    ///
    /// **LIVE producer** ([`AdmissionSpec::Singleflight`]). The build
    /// realises the dual-space algorithm through the ONE shared engine —
    /// NO query-time OXC:
    /// - `Instance` → `execute(Instantiate { base: type_slot, args,
    ///   context: published(Shallow) })`.
    /// - `Static` → `execute(TypeOf { value_root: value_root_of(value_slot) })`.
    ///
    /// The post-query projection is THIN / non-owning (identity
    /// projection): the build returns the composed sub-query node directly
    /// and does NOT walk heritage or eagerly materialise members. A deep
    /// instance/static surface projection (heritage descent, member-demand
    /// materialisation) is not part of this producer. Value domain:
    /// [`SemanticQueryValueTag::TypeNode`].
    ///
    /// [`AdmissionSpec::Singleflight`]: crate::semantic_query::query_key_spec::AdmissionSpec::Singleflight
    ResolveClassSurface {
        decl_slot: ResolvedDeclSlotIdentity,
        type_args: Arc<[SemanticNodeId]>,
        side: ClassSurfaceSide,
        context: ClassSurfaceContext,
    },
    /// Resolve an ambient namespace (`namespace N {}` / `module N {}`)
    /// surface.
    ///
    /// `namespace_slot` is the content-free [`ResolvedDeclSlotIdentity`]
    /// with `symbol_space = `[`SemanticSymbolSpace::Namespace`]; `type_args`
    /// are its already-lowered generic arguments; `context` carries the
    /// extra env (`R`) plus the projection rung (R21).
    ///
    /// **Non-producing.** This variant has no `execute`-side producer: its
    /// namespace-analysis reducer is unimplemented. The `execute` build arm
    /// returns [`QueryError::Miss`] (`Opaque(Miss)`) and never admits or
    /// caches a value (admission
    /// [`AdmissionSpec::NonProducingPendingReducer`]). Value domain:
    /// [`SemanticQueryValueTag::TypeNode`].
    ///
    /// [`AdmissionSpec::NonProducingPendingReducer`]: crate::semantic_query::query_key_spec::AdmissionSpec::NonProducingPendingReducer
    ResolveAmbientNamespace {
        namespace_slot: ResolvedDeclSlotIdentity,
        type_args: Arc<[SemanticNodeId]>,
        context: AmbientNamespaceContext,
    },
    /// Resolve an enum declaration's surface.
    ///
    /// `enum_slot` is the content-free [`ResolvedDeclSlotIdentity`] of the
    /// enum (an enum is not generic — NO `type_args`); `context` carries
    /// only `resolve_env_hash` (`R`).
    ///
    /// **Non-producing.** This variant has no `execute`-side producer: its
    /// enum value/type-duality reducer is unimplemented. The `execute`
    /// build arm returns [`QueryError::Miss`] and never admits or caches
    /// (admission [`AdmissionSpec::NonProducingPendingReducer`]). Value
    /// domain: [`SemanticQueryValueTag::TypeNode`].
    ///
    /// [`AdmissionSpec::NonProducingPendingReducer`]: crate::semantic_query::query_key_spec::AdmissionSpec::NonProducingPendingReducer
    ResolveEnum {
        enum_slot: ResolvedDeclSlotIdentity,
        context: EnumContext,
    },
    /// Resolve a callee's overload set.
    ///
    /// `callee` is the already-resolved [`SemanticNodeId`] of the callee;
    /// `type_args` are its call-site type arguments (part of semantic
    /// identity); `context` carries only `resolve_env_hash` (`R`).
    ///
    /// **Non-producing.** This variant has no `execute`-side producer: its
    /// signature-lowering reducer is unimplemented. The `execute` build arm
    /// returns [`QueryError::Miss`] and never admits or caches (admission
    /// [`AdmissionSpec::NonProducingPendingReducer`]).
    ///
    /// **Forward-declared value domain.** The row records value domain
    /// [`SemanticQueryValueTag::OverloadSet`]
    /// (`SemanticQueryValue::OverloadSet(Arc<[SignatureRef]>)`) — the
    /// ordered call/construct signature set this key will ultimately carry.
    /// This is a forward-declared contract, mirroring how `Relate`
    /// forward-declares [`SemanticQueryValueTag::Relation`]: the value
    /// carrier capable of holding an `OverloadSet` is introduced together
    /// with the overload-producing reducer. Until then the runtime arm is
    /// non-producing — it returns `Miss`, NEVER a fabricated empty
    /// `OverloadSet(Arc::from([]))`, which would be a stub.
    ///
    /// [`AdmissionSpec::NonProducingPendingReducer`]: crate::semantic_query::query_key_spec::AdmissionSpec::NonProducingPendingReducer
    ResolveOverloadSet {
        callee: SemanticNodeId,
        type_args: Arc<[SemanticNodeId]>,
        context: OverloadSetContext,
    },
    /// Resolve the APPARENT type of `base` — the member surface a value of
    /// that type exposes (a primitive widens to its lib wrapper:
    /// `string` → `String`'s members; an object type contributes its own
    /// members; etc.).
    ///
    /// `base` is the already-substituted [`SemanticNodeId`] (the
    /// substitution axis rides on `base`, so there is NO separate `args`
    /// field and NO `mode` field); `context` carries the `{T, L, J}` env
    /// the apparent surface depends on (NO `R`, NO `P` — see
    /// [`ApparentTypeContext`]).
    ///
    /// **Non-producing.** This variant has no `execute`-side producer: the
    /// lib-member / primitive→wrapper member index it would read does not
    /// exist yet. The `execute` build arm returns
    /// [`QueryError::Miss`] (`Opaque(Miss)`) verbatim and never admits or
    /// caches a value (admission
    /// [`AdmissionSpec::NonProducingPendingReducer`]). Value domain:
    /// [`SemanticQueryValueTag::TypeNode`]. The producer lands with the
    /// lib-member-index block.
    ///
    /// [`AdmissionSpec::NonProducingPendingReducer`]: crate::semantic_query::query_key_spec::AdmissionSpec::NonProducingPendingReducer
    ApparentType {
        base: SemanticNodeId,
        context: ApparentTypeContext,
    },
    /// Reduce a template-literal type `` `${...}${...}` `` to its folded
    /// surface.
    ///
    /// `pattern` mirrors [`SemanticNodeData::TemplateLiteral`]'s `quasis`
    /// (the alternating literal text spans) and `args` mirrors its
    /// `expressions` (the interpolated type nodes). `args` is ORDER-
    /// SIGNIFICANT — it is part of semantic identity and is NEVER reordered
    /// (concatenation order matters, unlike `NormalizeUnion`'s
    /// order-insensitive members). `context` carries the `{R, T, L, J}` env
    /// (NO `P`; substitution rides on `args` — see
    /// [`TemplateLiteralReduceContext`]).
    ///
    /// **LIVE producer** ([`AdmissionSpec::Singleflight`]). The build does
    /// NOT re-implement concatenation: it interns the
    /// [`SemanticNodeData::TemplateLiteral`] carrier and asks the ONE shared
    /// deferred evaluator to fold it. All-literal expressions fold to a
    /// single [`SemanticNodeData::Literal`] string; any non-literal
    /// expression carrier-stops to the `TemplateLiteral` shell. Value
    /// domain: [`SemanticQueryValueTag::TypeNode`].
    ///
    /// Literal-precise intrinsic transforms (`Uppercase<"a">` → `"A"`) are
    /// NOT part of this key — those ride the `Instantiate` path and widen
    /// to `Primitive(String)`.
    ///
    /// [`AdmissionSpec::Singleflight`]: crate::semantic_query::query_key_spec::AdmissionSpec::Singleflight
    TemplateLiteralReduce {
        pattern: Arc<[Arc<str>]>,
        args: Arc<[SemanticNodeId]>,
        context: TemplateLiteralReduceContext,
    },
    /// Resolve the FLOW-NARROWED type of the value referenced at `point` —
    /// the type after control-flow analysis has applied the type guards
    /// dominating that program point (e.g. inside `if (typeof x ===
    /// "string")` the narrowed type of `x` is `string`).
    ///
    /// `point` is the [`ProgramPointId`] identity core (the substitution
    /// axis is already fixed by the enclosing context, so there is NO
    /// separate `args` field and NO `mode` field); `context` carries the
    /// full `{P, R, T, L, J}` env the narrowing depends on (see
    /// [`ProgramAnalysisContext`]).
    ///
    /// **Non-producing.** This variant has no `execute`-side producer: the
    /// flow engine that walks the control-flow graph and applies guard
    /// narrowing does not exist yet (it lands in U6). The `execute` build
    /// arm returns [`QueryError::Miss`] (`Opaque(Miss)`) verbatim and never
    /// admits or caches a value (admission
    /// [`AdmissionSpec::NonProducingPendingReducer`]). Value domain:
    /// [`SemanticQueryValueTag::ProgramAnalysis`]. Fabricating a narrowed
    /// node here would be a stub — `Miss` is the honest non-result.
    ///
    /// [`AdmissionSpec::NonProducingPendingReducer`]: crate::semantic_query::query_key_spec::AdmissionSpec::NonProducingPendingReducer
    FlowNarrowingAt {
        point: ProgramPointId,
        context: ProgramAnalysisContext,
    },
    /// Resolve the CONTEXTUAL (expected) type at `point` — the type the
    /// surrounding context expects an expression at that position to have
    /// (e.g. the parameter type a callee expects for an argument position,
    /// or the declared type an initializer is checked against).
    ///
    /// `point` is the [`ProgramPointId`] identity core (no `args`, no `mode`
    /// — the contextual type is fixed by the program point); `context`
    /// carries the full `{P, R, T, L, J}` env (see
    /// [`ProgramAnalysisContext`]).
    ///
    /// **Non-producing.** This variant has no `execute`-side producer: the
    /// contextual-typing engine that derives the expected type from the
    /// surrounding syntax lands in U6. The `execute` build arm returns
    /// [`QueryError::Miss`] (`Opaque(Miss)`) verbatim and never admits or
    /// caches a value (admission
    /// [`AdmissionSpec::NonProducingPendingReducer`]). Value domain:
    /// [`SemanticQueryValueTag::ProgramAnalysis`].
    ///
    /// [`AdmissionSpec::NonProducingPendingReducer`]: crate::semantic_query::query_key_spec::AdmissionSpec::NonProducingPendingReducer
    ContextualTypeAt {
        point: ProgramPointId,
        context: ProgramAnalysisContext,
    },
}

/// Content-free discriminant for [`SemanticQueryKey`] — the variant identity
/// with no payload.
///
/// This is the row identity of the `SemanticQueryKeySpec` table (see
/// [`crate::semantic_query::query_key_spec`]). What the compiler forces:
/// [`SemanticQueryKey::tag`] is an exhaustive `match`, so adding a
/// `SemanticQueryKey` variant cannot compile without a new `tag()` arm. That
/// is the only compile-time guarantee. Adding the matching
/// `SemanticQueryKeyTag` variant, its membership in [`ALL`](Self::ALL), and its
/// `SemanticQueryKeySpec` row are NOT compiler-forced — the `syn`-based
/// `semantic_query_key_spec_table_equals_enum` diff-test is what forces
/// spec/tag/table alignment and catches a forgotten tag, `ALL` entry, or spec
/// row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SemanticQueryKeyTag {
    ResolveDecl,
    Instantiate,
    ProjectMember,
    IndexedAccess,
    KeyOf,
    MappedType,
    Conditional,
    TypeOf,
    NormalizeUnion,
    NormalizeIntersection,
    ProjectPath,
    ResolvedNamedType,
    Relate,
    ResolveMacroPayload,
    ResolveClassSurface,
    ResolveAmbientNamespace,
    ResolveEnum,
    ResolveOverloadSet,
    ApparentType,
    TemplateLiteralReduce,
    FlowNarrowingAt,
    ContextualTypeAt,
}

impl SemanticQueryKeyTag {
    /// Every live tag, in canonical (enum-declaration) order. The
    /// `SemanticQueryKeySpec` table is ordered by this slice so the rendered
    /// artifact is deterministic.
    pub const ALL: &'static [SemanticQueryKeyTag] = &[
        SemanticQueryKeyTag::ResolveDecl,
        SemanticQueryKeyTag::Instantiate,
        SemanticQueryKeyTag::ProjectMember,
        SemanticQueryKeyTag::IndexedAccess,
        SemanticQueryKeyTag::KeyOf,
        SemanticQueryKeyTag::MappedType,
        SemanticQueryKeyTag::Conditional,
        SemanticQueryKeyTag::TypeOf,
        SemanticQueryKeyTag::NormalizeUnion,
        SemanticQueryKeyTag::NormalizeIntersection,
        SemanticQueryKeyTag::ProjectPath,
        SemanticQueryKeyTag::ResolvedNamedType,
        SemanticQueryKeyTag::Relate,
        SemanticQueryKeyTag::ResolveMacroPayload,
        SemanticQueryKeyTag::ResolveClassSurface,
        SemanticQueryKeyTag::ResolveAmbientNamespace,
        SemanticQueryKeyTag::ResolveEnum,
        SemanticQueryKeyTag::ResolveOverloadSet,
        SemanticQueryKeyTag::ApparentType,
        SemanticQueryKeyTag::TemplateLiteralReduce,
        SemanticQueryKeyTag::FlowNarrowingAt,
        SemanticQueryKeyTag::ContextualTypeAt,
    ];

    /// The EXACT `SemanticQueryKey` variant identifier this tag names. The
    /// diff-test compares these strings against the variant identifiers
    /// scanned from the live `pub enum SemanticQueryKey { … }` source, so the
    /// string must equal the variant name character-for-character.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SemanticQueryKeyTag::ResolveDecl => "ResolveDecl",
            SemanticQueryKeyTag::Instantiate => "Instantiate",
            SemanticQueryKeyTag::ProjectMember => "ProjectMember",
            SemanticQueryKeyTag::IndexedAccess => "IndexedAccess",
            SemanticQueryKeyTag::KeyOf => "KeyOf",
            SemanticQueryKeyTag::MappedType => "MappedType",
            SemanticQueryKeyTag::Conditional => "Conditional",
            SemanticQueryKeyTag::TypeOf => "TypeOf",
            SemanticQueryKeyTag::NormalizeUnion => "NormalizeUnion",
            SemanticQueryKeyTag::NormalizeIntersection => "NormalizeIntersection",
            SemanticQueryKeyTag::ProjectPath => "ProjectPath",
            SemanticQueryKeyTag::ResolvedNamedType => "ResolvedNamedType",
            SemanticQueryKeyTag::Relate => "Relate",
            SemanticQueryKeyTag::ResolveMacroPayload => "ResolveMacroPayload",
            SemanticQueryKeyTag::ResolveClassSurface => "ResolveClassSurface",
            SemanticQueryKeyTag::ResolveAmbientNamespace => "ResolveAmbientNamespace",
            SemanticQueryKeyTag::ResolveEnum => "ResolveEnum",
            SemanticQueryKeyTag::ResolveOverloadSet => "ResolveOverloadSet",
            SemanticQueryKeyTag::ApparentType => "ApparentType",
            SemanticQueryKeyTag::TemplateLiteralReduce => "TemplateLiteralReduce",
            SemanticQueryKeyTag::FlowNarrowingAt => "FlowNarrowingAt",
            SemanticQueryKeyTag::ContextualTypeAt => "ContextualTypeAt",
        }
    }
}

impl SemanticQueryKey {
    /// The content-free discriminant tag for this key. EXHAUSTIVE by
    /// construction — a new enum variant cannot compile until it gains a tag
    /// arm here, which is the mechanism that keeps the spec table honest.
    #[must_use]
    pub fn tag(&self) -> SemanticQueryKeyTag {
        match self {
            SemanticQueryKey::ResolveDecl(_) => SemanticQueryKeyTag::ResolveDecl,
            SemanticQueryKey::Instantiate { .. } => SemanticQueryKeyTag::Instantiate,
            SemanticQueryKey::ProjectMember { .. } => SemanticQueryKeyTag::ProjectMember,
            SemanticQueryKey::IndexedAccess { .. } => SemanticQueryKeyTag::IndexedAccess,
            SemanticQueryKey::KeyOf { .. } => SemanticQueryKeyTag::KeyOf,
            SemanticQueryKey::MappedType { .. } => SemanticQueryKeyTag::MappedType,
            SemanticQueryKey::Conditional { .. } => SemanticQueryKeyTag::Conditional,
            SemanticQueryKey::TypeOf { .. } => SemanticQueryKeyTag::TypeOf,
            SemanticQueryKey::NormalizeUnion { .. } => SemanticQueryKeyTag::NormalizeUnion,
            SemanticQueryKey::NormalizeIntersection { .. } => {
                SemanticQueryKeyTag::NormalizeIntersection
            }
            SemanticQueryKey::ProjectPath { .. } => SemanticQueryKeyTag::ProjectPath,
            SemanticQueryKey::ResolvedNamedType { .. } => SemanticQueryKeyTag::ResolvedNamedType,
            SemanticQueryKey::Relate { .. } => SemanticQueryKeyTag::Relate,
            SemanticQueryKey::ResolveMacroPayload { .. } => {
                SemanticQueryKeyTag::ResolveMacroPayload
            }
            SemanticQueryKey::ResolveClassSurface { .. } => {
                SemanticQueryKeyTag::ResolveClassSurface
            }
            SemanticQueryKey::ResolveAmbientNamespace { .. } => {
                SemanticQueryKeyTag::ResolveAmbientNamespace
            }
            SemanticQueryKey::ResolveEnum { .. } => SemanticQueryKeyTag::ResolveEnum,
            SemanticQueryKey::ResolveOverloadSet { .. } => SemanticQueryKeyTag::ResolveOverloadSet,
            SemanticQueryKey::ApparentType { .. } => SemanticQueryKeyTag::ApparentType,
            SemanticQueryKey::TemplateLiteralReduce { .. } => {
                SemanticQueryKeyTag::TemplateLiteralReduce
            }
            SemanticQueryKey::FlowNarrowingAt { .. } => SemanticQueryKeyTag::FlowNarrowingAt,
            SemanticQueryKey::ContextualTypeAt { .. } => SemanticQueryKeyTag::ContextualTypeAt,
        }
    }
}

#[cfg(test)]
mod query_key_tag_tests {
    use super::SemanticQueryKeyTag;
    use std::collections::BTreeSet;

    /// Every `ALL` entry has a non-empty, distinct `name()` — a cheap guard
    /// that `ALL` and `name()` stay in lockstep (a copy-paste duplicate or an
    /// empty name fails here).
    #[test]
    fn semantic_query_key_tag_roundtrips_every_variant() {
        let names: Vec<&'static str> = SemanticQueryKeyTag::ALL.iter().map(|t| t.name()).collect();
        for name in &names {
            assert!(
                !name.is_empty(),
                "SemanticQueryKeyTag::name() returned an empty string for a tag in ALL"
            );
        }
        let distinct: BTreeSet<&'static str> = names.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            names.len(),
            "SemanticQueryKeyTag::name() produced a duplicate name; ALL and \
             name() have drifted out of sync"
        );
    }
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

/// Tri-state result of a `Relate` judgement.
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

/// One element of a [`SemanticNodeData::Tuple`] shell.
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
/// **Publication boundary.** Solver `Infer`, `Rest`, and
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
    /// Array shell. Publishes `T[]` / `Array<T>` /
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
    /// Tuple shell. Preserves per-element label /
    /// optionality / rest metadata so consumers can render tuples without
    /// going through a side channel. Each element's `value` is a regular
    /// [`SemanticNodeId`] under the lazy-materialisation rule.
    Tuple {
        elements: Arc<[TupleElement]>,
        readonly: bool,
    },
    /// Template-literal shell. Carries the
    /// alternating quasi text spans and expression references verbatim
    /// from the parser's [`TypeExpr::TemplateLiteral`] shape. Relation-
    /// engine support for infer-heavy template matching remains a
    /// separate follow-up — the shell carrier itself is not
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
        /// Declaration identity. Distinguishes cross-declaration
        /// same-name parameters (`type A<T>` vs `type B<T>` in the
        /// same file) so structural interning does not collide
        /// unrelated decls.
        decl: DeclIdentity,
        /// Position in the declaration's generic clause (0-based).
        /// Defaults to `0` for paths that have not yet plumbed the
        /// true clause position through the lowering pipeline; the
        /// script-setup lowering path supplies the true ordinal.
        param_index: u16,
        /// Declaration-site constraint (`T extends Constraint`).
        /// Lowered once at declaration time; callers substituting
        /// the TypeParam do NOT re-substitute constraint or default
        /// — those carry declaration-local meaning, not call-site
        /// meaning, which keeps the substitute helper's identity
        /// preservation invariant.
        constraint: Option<SemanticNodeId>,
        /// Declaration-site default (`T = Default`). Same contract
        /// as `constraint`.
        default: Option<SemanticNodeId>,
        /// Human-readable parameter name for `Debug` / error output.
        /// Excluded from `Hash` / `Eq` identity so two calls that
        /// construct the same declaration-identity TypeParam with
        /// the same constraint/default/index but different display
        /// names intern to the same slot. In practice `display_name`
        /// is consistent with `decl + param_index`; the exclusion
        /// is defensive.
        display_name: Arc<str>,
    },
    /// `infer X` placeholder inside a conditional's `extends` clause
    /// Modelled as an explicit variant rather than
    /// encoded via scope overloading (rejects anti-pattern #3 — scope-
    /// as-discriminator). The `name` is the infer binding name the
    /// `true` branch will substitute the bound type into.
    Infer {
        name: Arc<str>,
    },
    // Declaration identity is carried as `DeclIdentity` in
    // `SemanticQueryKey::Instantiate.base` instead of being interned as
    // a node in the arena.
    /// Conditional shell node.
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
    // §5.6 WIP-L — function shape.
    ///
    /// Classes / interfaces lower to `SemanticNodeData::Object` with heritage
    /// merged; only function signatures have distinct semantics (call
    /// signatures, parameter variance, return covariance) that Object cannot
    /// represent.
    Function {
        params: Arc<[FunctionParam]>,
        return_type: SemanticNodeId,
        type_parameters: Arc<[TypeParamDecl]>,
        /// OXC span of the WHOLE signature, stamped from the IR
        /// [`verter_type_expr::FunctionExpr`]'s signature span (NOT recovered
        /// from child node ids). Coordinates are in the signature's origin
        /// file. Participates in node interning (see the manual `Hash`/`Eq`)
        /// but never enters `parse_stable_hash`. `None` for a synthetic /
        /// composed signature with no single source site.
        signature_span: Option<verter_span::Span>,
        /// OXC span of the return-type annotation, stamped from the IR
        /// `FunctionExpr`'s return span. `None` when absent.
        return_type_span: Option<verter_span::Span>,
    },
    /// Lazy declaration-reference carrier.
    ///
    /// Produced by [`shallow_lower_type_expr`] in `Navigate` mode when it
    /// encounters a `TypeExpr::Ref { name, type_arguments: [] }`. Identity
    /// is the [`DeclIdentity`] (`canonical_id + whole_hash + decl_name`)
    /// — two equivalent navigate-mode references intern to the same node.
    ///
    /// The walker treats this variant as terminal in `Navigate` mode
    /// ([`PathWalker`] does not dispatch `ResolveDecl`); in `Expanded` mode
    /// the reducer ([`raise_and_reduce`]) issues a `ResolveDecl` dispatch
    /// and substitutes the result.
    ///
    /// Raises to `TypeExpr::Ref { name: identity.decl_name, type_arguments: [] }`.
    DeclRef {
        identity: DeclIdentity,
    },
    /// Lazy generic-application carrier.
    ///
    /// Produced by [`shallow_lower_type_expr`] in `Navigate` mode when it
    /// encounters a `TypeExpr::Ref { name, type_arguments: [...non-empty] }`.
    /// Identity is `(DeclIdentity, args)` — two structurally-equivalent
    /// navigate-mode generic applications intern to the same node.
    ///
    /// Terminal in `Navigate` mode (no `Instantiate` dispatch); the
    /// reducer issues `Instantiate` in `Expanded` mode and substitutes
    /// the result.
    ///
    /// Raises to `TypeExpr::Ref { name: base.decl_name, type_arguments: [...raised args] }`.
    InstantiationRef {
        base: DeclIdentity,
        args: Arc<[SemanticNodeId]>,
    },

    /// A same-name merged declaration carrier (TS same-file declaration
    /// merging). `contributors` holds the lowered body of each same-name
    /// `interface` part, in source order. The peer-merge reducer unions their
    /// members and accumulates same-name methods into ordered overload
    /// groups — distinct from [`Intersection`](Self::Intersection), whose
    /// reducer applies heritage-shadow member precedence. Never produced for a
    /// single declaration.
    MergedDecl {
        /// Lowered contributor bodies, in source order.
        contributors: Arc<[SemanticNodeId]>,
    },
}

impl SemanticNodeData {
    /// Stable discriminant index used by Path C instrumentation
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
            Self::MergedDecl { .. } => 16,
            Self::Conditional { .. } => 17,
            Self::VueMacroElements(_) => 18,
            Self::Function { .. } => 19,
            Self::DeclRef { .. } => 20,
            Self::InstantiationRef { .. } => 21,
        }
    }
}

// Structural interning in `NodeArena` keys on
// `(SemanticNodeData, NodeScopeId)`. Manual `Hash`/`Eq`/`PartialEq`
// rather than a derive because:
//
// - **TypeParam** identity excludes `display_name`.
//   `decl + param_index` (with `constraint` / `default`) is the
//   semantic identity; `display_name` is a presentational field
//   used for Debug output and error messages. Two `TypeParam` nodes
//   with matching identity but differing `display_name` must alias
//   under dedup.
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
                    // `display_name` intentionally excluded per F11.
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
                    signature_span: asig,
                    return_type_span: aret,
                },
                Self::Function {
                    params: bp,
                    return_type: br,
                    type_parameters: btp,
                    signature_span: bsig,
                    return_type_span: bret,
                },
                // Spans participate in identity: provenance-aware interning so
                // an identical same-file signature shape at a different source
                // location does not alias another node's spans (codex BINDING).
            ) => ap == bp && ar == br && atp == btp && asig == bsig && aret == bret,
            (Self::DeclRef { identity: a }, Self::DeclRef { identity: b }) => a == b,
            (
                Self::InstantiationRef { base: ab, args: aa },
                Self::InstantiationRef { base: bb, args: ba },
            ) => ab == bb && aa == ba,
            (Self::MergedDecl { contributors: a }, Self::MergedDecl { contributors: b }) => a == b,
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
                // `display_name` intentionally excluded per F11.
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
                signature_span,
                return_type_span,
            } => {
                params.hash(state);
                return_type.hash(state);
                type_parameters.hash(state);
                // Spans participate in identity (provenance-aware interning).
                signature_span.hash(state);
                return_type_span.hash(state);
            }
            Self::DeclRef { identity } => {
                identity.hash(state);
            }
            Self::InstantiationRef { base, args } => {
                base.hash(state);
                args.hash(state);
            }
            Self::MergedDecl { contributors } => {
                contributors.hash(state);
            }
        }
    }
}

/// Parameter of a [`SemanticNodeData::Function`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParam {
    pub name: Option<Arc<str>>,
    pub ty: SemanticNodeId,
    pub optional: bool,
    pub rest: bool,
    /// OXC span of the whole parameter, carried verbatim from the
    /// `verter_type_expr` IR parameter. Coordinates are in the signature's
    /// origin file. Participates in node interning (the derived `Hash`/`Eq`
    /// include it) but never enters `parse_stable_hash`. `None` for a synthetic
    /// parameter with no source site.
    pub span: Option<verter_span::Span>,
}

impl FunctionParam {
    /// Construct a graph parameter with NO source span (a synthesized /
    /// test-fixture parameter with no single declaration site).
    #[must_use]
    pub fn synthetic(
        name: Option<Arc<str>>,
        ty: SemanticNodeId,
        optional: bool,
        rest: bool,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            rest,
            span: None,
        }
    }
}

/// Type-parameter declaration on a [`SemanticNodeData::Function`].
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
    /// Canonical entry point. Returns the domain-agnostic
    /// [`SemanticQueryValue`] wrapped with the provenance of the producing
    /// work. Every live key that produces a value resolves to
    /// [`SemanticQueryValue::TypeNode`]; the non-producing keys (`Relate`,
    /// `ResolveOverloadSet`) return `Miss`. Callers that want the node narrow
    /// with [`execute_type_node`].
    ///
    /// [`execute_type_node`]: Self::execute_type_node
    fn execute(
        &self,
        key: SemanticQueryKey,
    ) -> QueryResult<SemanticQueryOutput<SemanticQueryValue>>;

    /// Narrow [`execute`](Self::execute) to the type-node domain, preserving
    /// provenance. A non-[`TypeNode`](SemanticQueryValue::TypeNode) value
    /// becomes a [`QueryError::ValueDomainMismatch`]. This is the single
    /// narrowing definition — there is no second admission path.
    fn execute_type_node(
        &self,
        key: SemanticQueryKey,
    ) -> QueryResult<SemanticQueryOutput<SemanticNodeId>> {
        narrow_output_to_type_node(self.execute(key))
    }

    fn resolve_decl(&self, key: ResolveDeclKey) -> QueryResult<SemanticNodeId> {
        strip_output_provenance(self.execute_type_node(SemanticQueryKey::ResolveDecl(key)))
    }
    fn instantiate(
        &self,
        base: DeclKey,
        args: Arc<[SemanticNodeId]>,
        body_mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        strip_output_provenance(self.execute_type_node(SemanticQueryKey::Instantiate {
            base,
            args,
            context: ProjectionReductionContext::published(body_mode),
        }))
    }
    fn project_member(
        &self,
        base: SemanticNodeId,
        member: Arc<str>,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        strip_output_provenance(self.execute_type_node(SemanticQueryKey::ProjectMember {
            base,
            member,
            mode,
        }))
    }
    fn indexed_access(
        &self,
        base: SemanticNodeId,
        index: IndexKey,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        strip_output_provenance(self.execute_type_node(SemanticQueryKey::IndexedAccess {
            base,
            index,
            mode,
        }))
    }
    fn project_path(
        &self,
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        strip_output_provenance(self.execute_type_node(SemanticQueryKey::ProjectPath {
            base,
            path,
            context: ProjectionReductionContext::published(mode),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Semantic subqueries with the same resolved meaning produce the same
    /// key even when reached through different higher-level expressions —
    /// this is the core dedup guarantee the dispatch layer builds on.
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
        let base = DeclKey::from_identity(&DeclIdentity::synthetic("Foo"));
        let string_id = SemanticNodeId(1);
        let number_id = SemanticNodeId(2);
        let a = SemanticQueryKey::Instantiate {
            base: base.clone(),
            args: Arc::from(vec![string_id].into_boxed_slice()),
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
        };
        let b = SemanticQueryKey::Instantiate {
            base,
            args: Arc::from(vec![number_id].into_boxed_slice()),
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
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

    /// F2 — counter taxonomy matches
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
    /// §6.3 source-of-truth field list (see F2 + §6.3):
    /// - Expected-to-fire: hits, misses, waits_ms, in_flight_peak,
    ///   memo_entry_count, origin_edge_count, instantiate_count,
    ///   conditional_decided_count, conditional_deferred_count,
    ///   branch_selections_true, branch_selections_false,
    ///   origin_edges_emitted, path_length_p50, path_length_p95,
    ///   projection_depth_p50, projection_depth_p95,
    ///   origin_edges_per_node_p50, origin_edges_per_node_p95,
    ///   decl_subexpression_lowering_count, relation_check_count,
    ///   mapped_per_k_materializations, substitute_memo_hits,
    ///   substitute_memo_misses, evaluate_deferred_memo_hits,
    ///   evaluate_deferred_memo_misses.
    /// - Exceptional-path: budget_fallback_count, same_path_sentinel_returns,
    ///   joined_waits, inflight_aborted_retries, cold_aborts_swept.
    #[test]
    fn counter_taxonomy_matches_plan() {
        let stats = SemanticGraphStats::default();
        let debug = format!("{stats:?}");

        // Expected-to-fire counters.
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
            "decl_subexpression_lowering_count",
            "relation_check_count",
            "mapped_per_k_materializations",
            "substitute_memo_hits",
            "substitute_memo_misses",
            "evaluate_deferred_memo_hits",
            "evaluate_deferred_memo_misses",
        ];
        for field in expected_to_fire {
            assert!(
                debug.contains(&format!("{field}: ")),
                "SemanticGraphStats is missing expected-to-fire counter `{field}`"
            );
        }

        // Exceptional-path counters (legitimately zero on the corpus;
        // forcing tests prove they exist and increment under
        // dedicated fixtures).
        let exceptional_path = [
            "budget_fallback_count",
            "same_path_sentinel_returns",
            "joined_waits",
            "inflight_aborted_retries",
            "cold_aborts_swept",
        ];
        for field in exceptional_path {
            assert!(
                debug.contains(&format!("{field}: ")),
                "SemanticGraphStats is missing exceptional-path counter `{field}`"
            );
        }

        // Cardinality check: total field count on `SemanticGraphStats`
        // equals expected-to-fire + exceptional-path. Catches a field
        // added to the struct without a corresponding entry — which
        // would otherwise slip past the one-way `contains` checks
        // above. Uses `": "` as the field delimiter in Debug output
        // (every primitive field emits exactly one `: ` separator
        // between name and value).
        let expected_total = expected_to_fire.len() + exceptional_path.len();
        let field_count = debug.matches(": ").count();
        assert_eq!(
            field_count,
            expected_total,
            "SemanticGraphStats has {field_count} fields in Debug output but \
             {expected_total} counters were expected (expected_to_fire = {}, \
             exceptional_path = {}). A field was added/removed without updating \
             this test; see debug output:\n{debug}",
            expected_to_fire.len(),
            exceptional_path.len(),
        );
    }

    /// F2 — navigation-once invariant contract.
    ///
    /// The contract says: for N distinct concrete instantiations of the
    /// same parameterised declaration, subexpression lowering count
    /// equals the number of structurally distinct visited subexpressions
    /// — not N × body_size. This test locks the contract; the counter
    /// `decl_subexpression_lowering_count` that drives the strict form
    /// is a post-track refinement per follow-up item 4.
    ///
    /// Today the invariant is enforced by the family memo (B1b) which
    /// dedups every `ProjectPath(member, path, mode)` sub-query across
    /// distinct `Instantiate` calls that visit the same path.
    #[test]
    fn navigation_once_invariant_contract() {
        // Structural: `SemanticQueryKey::Instantiate { base, args, body_mode }`
        // splits the family memo per `body_mode`, so two distinct
        // projections under the SAME body_mode into the same declaration
        // body share family memo entries for every structurally-equal
        // path segment.
        //
        // When the F2 counter `decl_subexpression_lowering_count` lands
        // as a post-track refinement, this test's strict assertion
        // becomes: after N `Instantiate(Foo, [V_i], body_mode)` +
        // matching `ProjectPath(result, [p], Identity)`, the counter
        // equals the size of the visited path intersection, not
        // N × body_size — within one body_mode slot.
        //
        // The current assertion is the structural invariant: same
        // `(base, args, body_mode)` triple constructs an equal key.
        let base = DeclKey::from_identity(&DeclIdentity::synthetic("Foo"));
        let args = Arc::from(vec![SemanticNodeId(2)].into_boxed_slice());
        let key = SemanticQueryKey::Instantiate {
            base: base.clone(),
            args: Arc::clone(&args),
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
        };
        let mut map = std::collections::HashMap::new();
        map.insert(key.clone(), 1);
        let key2 = SemanticQueryKey::Instantiate {
            base,
            args,
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
        };
        assert_eq!(map.get(&key2), Some(&1), "same args dedup to one entry");
    }

    /// `narrow_output_to_type_node` — the single narrowing definition that
    /// backs `execute_type_node` — maps a `TypeNode` value through with its
    /// provenance preserved, and converts any other domain into a
    /// `ValueDomainMismatch` carrying both tags.
    #[test]
    fn narrow_output_maps_type_node_and_rejects_other_domains() {
        let node = SemanticNodeId(7);
        let clean = QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::TypeNode(node),
            provenance: ResultProvenance::clean(),
        });
        match narrow_output_to_type_node(clean) {
            QueryResult::Value(out) => {
                assert_eq!(out.value, node, "TypeNode must narrow to the same node");
                assert_eq!(
                    out.provenance,
                    ResultProvenance::clean(),
                    "provenance must ride through narrowing unchanged"
                );
            }
            other => panic!("expected Value(TypeNode), got {other:?}"),
        }

        // A non-TypeNode domain (an overload set) must NOT narrow to a node.
        let overloads = QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::OverloadSet(Arc::from(Vec::<SignatureRef>::new())),
            provenance: ResultProvenance::clean(),
        });
        match narrow_output_to_type_node(overloads) {
            QueryResult::Error(QueryError::ValueDomainMismatch { expected, actual }) => {
                assert_eq!(expected, SemanticQueryValueTag::TypeNode);
                assert_eq!(actual, SemanticQueryValueTag::OverloadSet);
            }
            other => panic!("expected ValueDomainMismatch, got {other:?}"),
        }

        // Control arms pass through untouched.
        match narrow_output_to_type_node(
            QueryResult::<SemanticQueryOutput<SemanticQueryValue>>::Recursive(node),
        ) {
            QueryResult::Recursive(n) => assert_eq!(n, node),
            other => panic!("expected Recursive passthrough, got {other:?}"),
        }
        match narrow_output_to_type_node(
            QueryResult::<SemanticQueryOutput<SemanticQueryValue>>::Error(QueryError::Miss),
        ) {
            QueryResult::Error(QueryError::Miss) => {}
            other => panic!("expected Error passthrough, got {other:?}"),
        }
    }

    /// Every value-domain tag round-trips through
    /// [`SemanticQueryValue::tag`], and the discriminants are distinct so
    /// `ValueDomainMismatch` can name an exact mismatch.
    #[test]
    fn value_domain_tags_are_exhaustive_and_distinct() {
        let node = SemanticNodeId(1);
        let cases = [
            (
                SemanticQueryValue::TypeNode(node),
                SemanticQueryValueTag::TypeNode,
            ),
            (
                SemanticQueryValue::ProgramAnalysis(ProgramAnalysisValue { node }),
                SemanticQueryValueTag::ProgramAnalysis,
            ),
            (
                SemanticQueryValue::DeclarationAnalysis(DeclarationAnalysisValue {
                    contributors: Arc::from(vec![node].into_boxed_slice()),
                }),
                SemanticQueryValueTag::DeclarationAnalysis,
            ),
            (
                SemanticQueryValue::OverloadSet(Arc::from(
                    vec![SignatureRef { node }].into_boxed_slice(),
                )),
                SemanticQueryValueTag::OverloadSet,
            ),
            (
                SemanticQueryValue::Relation(RelationPayload {
                    outcome: RelationOutcome::Assignable,
                    bindings: Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                    relation_proof: RelationProofId(0),
                }),
                SemanticQueryValueTag::Relation,
            ),
            (
                SemanticQueryValue::DiagnosticAnalysis(DiagnosticAnalysisShell),
                SemanticQueryValueTag::DiagnosticAnalysis,
            ),
        ];
        for (value, tag) in &cases {
            assert_eq!(value.tag(), *tag, "tag must match the value domain");
        }
        // Distinctness: six cases, six unique tags. Sort before dedup so
        // non-adjacent duplicates are caught (`Vec::dedup` only collapses
        // consecutive runs).
        let mut tags: Vec<SemanticQueryValueTag> = cases.iter().map(|(_, t)| *t).collect();
        tags.sort_by_key(|t| *t as u8);
        tags.dedup();
        assert_eq!(tags.len(), 6, "every value domain must have a distinct tag");
    }

    /// Taint shape: every `ResultTaint` / `BrokenInputClass` variant is
    /// representable, and the boundary provenance constructor is `Clean`.
    /// (No join is asserted — this layer carries provenance shape only.)
    #[test]
    fn result_taint_shape_is_representable_and_clean_by_default() {
        assert_eq!(ResultProvenance::clean().taint, ResultTaint::Clean);
        let classes = [
            BrokenInputClass::SyntaxError,
            BrokenInputClass::UnresolvedReference,
            BrokenInputClass::IncompleteDeclaration,
            BrokenInputClass::TornRead,
            BrokenInputClass::MissingDependency,
        ];
        // All taint variants are constructible over every broken-input class.
        for class in classes {
            assert_ne!(ResultTaint::Partial(class), ResultTaint::Clean);
            assert_ne!(ResultTaint::Broken(class), ResultTaint::Clean);
            assert_ne!(ResultTaint::Partial(class), ResultTaint::Broken(class));
        }
    }
}
