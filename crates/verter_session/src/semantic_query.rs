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

/// Explicit expansion depth for [`SemanticQueryKey::Expand`]. Kept distinct
/// from [`ProjectionMode`] because expansion is a standalone operation and
/// projection is a per-hop choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpandMode {
    Shallow,
    Expanded,
}

/// One hop in a navigation path. Used by [`TypeNavigator::choose_next_hop`].
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

/// One-level surface view of a semantic node. Members are ordered to keep
/// hashing stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceView {
    pub members: Arc<[(Arc<str>, SemanticNodeId)]>,
    pub index_signatures: Arc<[SemanticNodeId]>,
    pub keyspace: Option<SemanticNodeId>,
}

impl std::hash::Hash for SurfaceView {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (name, id) in self.members.iter() {
            name.hash(state);
            id.hash(state);
        }
        self.index_signatures.hash(state);
        self.keyspace.hash(state);
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
    Expand {
        base: SemanticNodeId,
        mode: ExpandMode,
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
    fn expand(&self, base: SemanticNodeId, mode: ExpandMode) -> QueryResult<SemanticNodeId> {
        self.execute(SemanticQueryKey::Expand { base, mode })
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
