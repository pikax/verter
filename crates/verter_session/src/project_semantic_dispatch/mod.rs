//! Project-global [`SemanticQueryApi`] dispatcher.
//!
//! Binds [`SemanticQueryKey`] variants onto the shared
//! [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore) memo
//! and routes them to the existing resolver/solver entry points. This is the
//! single dispatch site every reusable type-resolution operation flows
//! through, regardless of which higher-level request initiated it.
//!
//! ## Scope of this landing
//!
//! Every [`SemanticQueryKey`] variant dispatches through this module:
//!
//! - `ResolveDecl` — returns a structural node sourced from the shallow state
//!   (an `Object` surface when the declaration carries member signatures,
//!   otherwise a `Primitive(Never)` anchor that memoizes the declaration's
//!   scope/name identity).
//! - `Instantiate` — identity-preserving alias anchored to the base node.
//!   Memoizing the key is the dedup guarantee; the richer instantiation
//!   shape is produced on demand by the solver once the caller walks
//!   into it. Mode-free per the lazy-materialisation rule.
//! - `ProjectPath` — path-precise projection rooted at `base` walking each
//!   [`PathSegment`]. The empty-path form is the canonical shape of
//!   "expand the whole surface" and supersedes the retired `Expand`
//!   variant. `ProjectMember { base, member, mode }` and
//!   `IndexedAccess { base, index, mode }` admission-canonicalise to the
//!   length-1 `ProjectPath` form **before** memo hashing so sugar and
//!   canonical share one warm entry and one in-flight wait graph (plan
//!   B1a).
//! - `NormalizeUnion` / `NormalizeIntersection` — structural dedup over the
//!   supplied members with stable ordering.
//! - `KeyOf` / `MappedType` / `Conditional` — navigation operations that
//!   walk the base node's shared-graph payload. Paths that do not reach a
//!   concrete node fall through to a recorded `Opaque(Miss)` entry — this
//!   is distinct from a dispatch miss because the warm entry observes the
//!   base node identity and therefore dedups repeated asks.
//! - `TypeOf` — mirrors `ResolveDecl` but routes through the shallow
//!   value-symbol space.
//!
//! Every variant observes a dep-signature fragment (at minimum the project
//! generation; additional file hashes flow in from the base nodes' origin
//! scopes where the dispatcher can observe them). Dep-signature propagation
//! is the substrate [`CompletionFence`](crate::completion_fence::CompletionFence)
//! relies on for transitive final-result validation.
//!
//! ## Design rules
//!
//! - Navigators stay non-owning: new semantic nodes must enter through
//!   [`SemanticQueryApi::execute`], not through ad-hoc helpers on the
//!   dispatcher.
//! - Errors, partial results, and recursion sentinels never promote to warm
//!   memo entries — the underlying [`SemanticGraphStore`] enforces this
//!   invariant at publish time.
//! - Dep-signatures returned from warm hits must merge into the caller's
//!   active [`CompletionFence`](crate::completion_fence::CompletionFence)
//!   so final-result validation stays transitive.

use std::sync::Arc;

use verter_semantic::analysis::type_solver::host::{
    BareRefOrigin, ResolvedRootIdentity, UtilitySource,
};
use verter_semantic::analysis::type_solver::PreparedTypeDecl;

use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    BranchSelection, CacheRead, DeclIdentity, DepSignature, DepVersion, IndexKey, LiteralValue,
    NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind, ProjectionMode,
    QueryError, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SurfaceView,
};
use crate::semantic_query_memo::SemanticGraphStore;
use verter_type_expr::{PrimitiveName, TypeExpr};

// Phase D §5.2 WIP-Split — module tree. Extracted sub-modules are `pub(crate)`
// so external callers see only the `ProjectSemanticDispatch` struct / trait
// impl. Stub sub-modules (`build`, `guards`, `enumerate`, `relation`, `lower`,
// `substitute`, `evaluate`, `origin`) are placeholders for the extractions
// performed in §5.3–§5.10; for now the corresponding content still lives in
// this `mod.rs`.
pub(crate) mod build;
pub(crate) mod enumerate;
pub(crate) mod evaluate;
pub(crate) mod lower;
pub(crate) mod raise;
pub(crate) mod relation;
pub(crate) mod substitute;
pub(crate) mod walk;

/// Declaration identity used for in-flight instantiation tracking
/// ( recursive-ref back-edge detection during body
/// lowering). `(canonical_id, name)` tuple keyed on refcount-shared
/// `Arc<str>` so membership checks are refcount compares, not string
/// compares. Lives on [`ProjectSemanticDispatch`] so nested
/// `build_instantiate` invocations share the active set.
pub(super) type InstantiateIdentity = (Arc<str>, Arc<str>);

/// Host-bound dispatcher for [`SemanticQueryApi`].
///
/// The dispatcher borrows the host for the duration of a query — every
/// `execute()` call threads through the host's
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore) and its
/// owned [`SemanticGraphStore`].
///
/// `instantiate_active` records every declaration identity currently being
/// materialised by a `build_instantiate` call chain rooted at this
/// dispatcher. When `shallow_lower_type_expr` encounters a
/// `TypeExpr::Ref` whose resolved root matches an active identity, it
/// emits an `Opaque(RecursiveRef)` back-edge immediately — no further
/// dispatch, no deep recursion into the body a second time. This is the
/// dispatch-side analogue of the retired solver's `RecursionTracker`
/// and closes the stack-bound recursion hole Session 4 traced down to
/// the `type TreeNode = { children: TreeNode[] }` materialisation path.
pub struct ProjectSemanticDispatch<'a> {
    pub(super) ctx: &'a dyn ResolverContext,
    pub(super) instantiate_active: std::cell::RefCell<smallvec::SmallVec<[InstantiateIdentity; 8]>>,
    /// Path C C6a item 3 — per-dispatcher mapped-binder ordinal
    /// counter. Increments on every `TypeExpr::Mapped` binder
    /// intern so two `[K in ...]` binders lowered in the same
    /// dispatcher get distinct `param_index` ordinals even when
    /// their declaring file, hash, and `"<mapper-param>"`
    /// sentinel would otherwise collide.
    ///
    /// **Documented fallback from per-owning-scope** per plan
    /// §14.2 item 3. The per-owning-scope ordinal would require
    /// threading owner-scope context through every recursive
    /// `shallow_lower_type_expr` call; per-dispatcher is strictly
    /// coarser but still correct (distinct binders get distinct
    /// ordinals). Trade-off: identity is stable within one
    /// dispatcher lifetime but not across dispatcher instances,
    /// so re-lowering the same file in a fresh dispatcher may
    /// assign different ordinals. Per, this is
    /// acceptable for C6a-through-C7 correctness; C17's sharded
    /// interner can revisit if hot-path cache stability becomes
    /// a measurable concern.
    pub(super) mapped_binder_ordinal: std::cell::Cell<u16>,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Create a dispatcher bound to `ctx`.
    ///
    /// Locked-in signature: takes `&dyn ResolverContext`,
    /// not concrete `&VerterHost`. External callers (test fixtures and
    /// `component_meta_materialize.rs`) pass `&host` directly; the
    /// implicit `&host as &dyn ResolverContext` upcast handles
    /// type-erasure at the call site because `impl ResolverContext for
    /// VerterHost` is registered in `resolver_core/resolver_context.rs`.
    #[must_use]
    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
        Self {
            ctx,
            instantiate_active: std::cell::RefCell::new(smallvec::SmallVec::new()),
            mapped_binder_ordinal: std::cell::Cell::new(0),
        }
    }

    /// Path C C6a item 3 — acquire the next mapped-binder ordinal
    /// from the per-dispatcher counter, then increment. Used by
    /// `shallow_lower_type_expr`'s Mapped arm to assign distinct
    /// `param_index` values to each `[K in ...]` binder.
    pub(super) fn next_mapped_binder_ordinal(&self) -> u16 {
        let current = self.mapped_binder_ordinal.get();
        self.mapped_binder_ordinal.set(current.saturating_add(1));
        current
    }

    /// Push `identity` onto the active-instantiation stack. Returns `true`
    /// when the identity was not already present (caller MUST pair with
    /// `pop_instantiate_active` on the same identity). Returns `false`
    /// when the identity is already active — caller should emit
    /// `Opaque(RecursiveRef)` and must NOT pop.
    pub(super) fn push_instantiate_active(&self, identity: InstantiateIdentity) -> bool {
        let mut active = self.instantiate_active.borrow_mut();
        if active.iter().any(|existing| {
            existing.0.as_ref() == identity.0.as_ref() && existing.1.as_ref() == identity.1.as_ref()
        }) {
            return false;
        }
        active.push(identity);
        true
    }

    /// Pop the most-recent active-instantiation entry. Caller MUST only
    /// call this after a successful `push_instantiate_active`.
    pub(super) fn pop_instantiate_active(&self) {
        let mut active = self.instantiate_active.borrow_mut();
        active.pop();
    }

    /// Check whether `identity` is currently being instantiated on this
    /// dispatcher's call chain without taking ownership.
    pub(super) fn is_instantiate_active(&self, canonical_id: &str, name: &str) -> bool {
        let active = self.instantiate_active.borrow();
        active.iter().any(|(existing_canonical, existing_name)| {
            existing_canonical.as_ref() == canonical_id && existing_name.as_ref() == name
        })
    }

    pub(super) fn graph(&self) -> &Arc<SemanticGraphStore> {
        self.ctx.project_type_store().semantic_graph()
    }

    /// Intern an opaque node carrying the supplied query error. Used as the
    /// fallback when a semantic subquery cannot be satisfied but the caller
    /// wants a node id rather than a top-level error.
    pub(super) fn opaque(&self, err: QueryError) -> SemanticNodeId {
        self.graph().intern_node(SemanticNodeData::Opaque(err))
    }

    /// Build the dep-signature fragment for a canonical file at a given
    /// content hash. Carries both the file-version fact and the current
    /// project generation so the completion fence picks up both.
    pub(super) fn dep_signature_for(
        &self,
        canonical_id: &Arc<str>,
        hash: [u8; 16],
    ) -> DepSignature {
        let project_gen = self.ctx.project_type_store().project_generation();
        Arc::from(
            vec![
                (canonical_id.clone(), DepVersion::WholeHash(hash)),
                (
                    canonical_id.clone(),
                    DepVersion::ProjectGeneration(project_gen),
                ),
            ]
            .into_boxed_slice(),
        )
    }

    /// Build a dep-signature fragment that records only the project
    /// generation. Used by derived semantic operations (e.g. `Instantiate`,
    /// `NormalizeUnion`) where no single canonical scope owns the result —
    /// dep signatures flow in through the warm memo hits of the bases the
    /// caller already supplied.
    pub(super) fn project_generation_signature(&self) -> DepSignature {
        let project_gen = self.ctx.project_type_store().project_generation();
        Arc::from(
            vec![(
                Arc::<str>::from("<project>"),
                DepVersion::ProjectGeneration(project_gen),
            )]
            .into_boxed_slice(),
        )
    }

    /// Convenience wrapper for consumer migrations.
    /// Lowers a [`TypeExpr`] rooted at the supplied canonical file scope
    /// via [`Self::shallow_lower_type_expr`] with empty env, empty
    /// name-resolution, and the scope's prepared-decl payload for
    /// bare-name fallback. The dispatch walker consults the scope's
    /// `DeclarationScopePayload` + host-owned `shallow_file_state` to
    /// resolve bare `TypeExpr::Ref` hops directly — no
    /// `SessionSolverHost` is constructed on this path (
    /// step 3).
    ///
    /// Lowers under [`ProjectionMode::Expanded`]. Callers that need
    /// mode-aware lowering should use
    /// [`Self::lower_type_expr_in_scope_with_mode`] (plan Step 1 / D1.6).
    ///
    /// Returns `None` when the scope's file is not known to the host or
    /// the expression lowers to an opaque miss.
    pub fn lower_type_expr_in_scope(
        &self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> Option<SemanticNodeId> {
        self.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            crate::semantic_query::ProjectionMode::Expanded,
        )
    }

    /// Mode-aware variant of [`Self::lower_type_expr_in_scope`].
    ///
    /// Threads `mode` into the underlying
    /// [`Self::shallow_lower_type_expr`] call so callers can request a
    /// `Navigate` lowering (lazy-terminal: keep `Ref`-shells lazy
    /// instead of triggering wholesale body expansion at the lowering
    /// site) or an `Expanded` lowering (the legacy default of the
    /// non-mode-aware sibling).
    ///
    /// Used by Step 1's host-side closure (`compute_evaluated_types_*`)
    /// to thread the macro shell through dispatch with the consumer's
    /// chosen mode (currently `Expanded` — the consumer wants reduced
    /// output for component-meta projection).
    pub fn lower_type_expr_in_scope_with_mode(
        &self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
        mode: crate::semantic_query::ProjectionMode,
    ) -> Option<SemanticNodeId> {
        let shallow = self.ctx.shallow_file_state(scope_canonical_id)?;
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(scope_canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        let env = rustc_hash::FxHashMap::default();
        let name_resolution = rustc_hash::FxHashMap::default();
        let scope_payload = self
            .ctx
            .prepared_decl_bundle(scope_canonical_id)
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        // R15/F11 — construct the resolver-context
        // `ScopeShadowing` once at the dispatch entry point. The
        // shadow set is derived from the same `scope_payload` the
        // foundation (`524f469d`) consumed inline; any future
        // shadow-source addition lands in `ScopeShadowing::from_*`
        // rather than at every call site.
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        let id = self.shallow_lower_type_expr(
            expr,
            &env,
            &scope,
            &name_resolution,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            mode,
        );
        Some(id)
    }
}

pub(super) fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Tri-state result of [`ProjectSemanticDispatch::shallow_relation_check`].
/// The relation authority is [`ProjectSemanticDispatch::relate_nodes`];
/// this enum only carries the hot-path fast-decision cases handled
/// inline by `build_conditional` before falling through to the full
/// engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShallowRelation {
    Assignable,
    NotAssignable,
    Unknown,
}

/// Map a [`PrimitiveName`] from the parser's IR onto the semantic-graph
/// [`PrimitiveKind`]. Kept colocated with `build_instantiate` because no
/// other dispatch builder produces primitive nodes from `TypeExpr`.
pub(super) fn map_primitive_name(name: PrimitiveName) -> PrimitiveKind {
    match name {
        PrimitiveName::String => PrimitiveKind::String,
        PrimitiveName::Number => PrimitiveKind::Number,
        PrimitiveName::Boolean => PrimitiveKind::Boolean,
        PrimitiveName::Symbol => PrimitiveKind::Symbol,
        PrimitiveName::BigInt => PrimitiveKind::BigInt,
        PrimitiveName::Any => PrimitiveKind::Any,
        PrimitiveName::Unknown => PrimitiveKind::Unknown,
        PrimitiveName::Void => PrimitiveKind::Void,
        PrimitiveName::Never => PrimitiveKind::Never,
        PrimitiveName::Null => PrimitiveKind::Null,
        PrimitiveName::Undefined => PrimitiveKind::Undefined,
        PrimitiveName::Object => PrimitiveKind::Object,
    }
}

/// Type-parameter names for built-in TS utilities, ordered positionally.
///
/// Origin edges on utility results carry these names so they match the
/// names a userland-equivalent alias would emit. `Partial<T>` gets
/// `["T"]` — not a synthesised `["T0"]` — so origin walks on
/// `Partial<T>` match `type MyPartial<T> = ...` byte-for-byte.
///
/// See utility-equivalence rule and §3 C7.
pub(super) fn utility_param_names(name: &str) -> &'static [&'static str] {
    match name {
        "Partial"
        | "Required"
        | "Readonly"
        | "NonNullable"
        | "NoInfer"
        | "ReturnType"
        | "Parameters"
        | "ConstructorParameters"
        | "InstanceType"
        | "Uppercase"
        | "Lowercase"
        | "Capitalize"
        | "Uncapitalize" => &["T"],
        "Awaited" => &["P"],
        "Pick" | "Omit" => &["T", "K"],
        "Record" => &["K", "V"],
        "Extract" | "Exclude" => &["T", "U"],
        _ => &[],
    }
}

// `primitive_for_literal` was the pre-C6 approximation that collapsed
// literal types to their underlying primitive — retired when
// `SemanticNodeData::Literal(LiteralValue)` landed in WIP 3-quater
// Literal identity is now
// preserved through the semantic graph; callers that need the
// primitive kind of a literal should match on `SemanticNodeData::Literal`
// and derive the kind inline.

/// Stable-sort + dedup a list of semantic node ids. Used to canonicalize
/// the key surface for union / intersection dispatches so
/// `NormalizeUnion({A,B})` and `NormalizeUnion({B,A})` converge on one
/// warm memo entry.
pub(super) fn canonicalize_node_list(members: &[SemanticNodeId]) -> Arc<[SemanticNodeId]> {
    let mut sorted: Vec<SemanticNodeId> = members.to_vec();
    sorted.sort_by_key(|id| id.0);
    sorted.dedup();
    Arc::from(sorted.into_boxed_slice())
}

impl<'a> SemanticQueryApi for ProjectSemanticDispatch<'a> {
    fn execute(&self, key: SemanticQueryKey) -> QueryResult<SemanticNodeId> {
        // Type-resolution audit: bump the per-request hop / mode /
        // projection / conditional counters BEFORE admission-time
        // rewriting so we attribute the dispatch to the caller's
        // intent (path-projection sugar still counts as one hop). The
        // counters are no-ops when no `RequestContext` is installed
        // on the calling thread.
        if let Some(ctx) = crate::request_context::current_request_context() {
            // Walker depth high-water — observe per-dispatch nesting
            // depth via the dispatcher's `instantiate_active` stack.
            let depth = self.instantiate_active.borrow().len();
            ctx.observe_type_resolution_depth(u16::try_from(depth).unwrap_or(u16::MAX));
            match &key {
                SemanticQueryKey::ProjectPath { mode, .. }
                | SemanticQueryKey::ProjectMember { mode, .. }
                | SemanticQueryKey::IndexedAccess { mode, .. } => {
                    ctx.bump_type_resolution_hop(*mode);
                    ctx.bump_type_resolution_projection_op();
                }
                SemanticQueryKey::Instantiate { body_mode, .. } => {
                    ctx.bump_type_resolution_hop(*body_mode);
                }
                SemanticQueryKey::Conditional { .. } => {
                    ctx.bump_type_resolution_conditional_decision();
                    // Conditional decisions still count as a single
                    // dispatched hop in `Identity` mode — the
                    // dispatcher allocates one node id for the
                    // resolution.
                    ctx.bump_type_resolution_hop(crate::semantic_query::ProjectionMode::Identity);
                }
                // ResolveDecl, KeyOf, MappedType, TypeOf,
                // NormalizeUnion, NormalizeIntersection,
                // ResolvedNamedType, Relate, ResolveMacroPayload —
                // each is a single dispatched hop with no consumer-
                // visible projection mode. Count them as Identity hops
                // so the audit's `hops` field is the total dispatch
                // count, not just the projection-bearing subset.
                _ => {
                    ctx.bump_type_resolution_hop(crate::semantic_query::ProjectionMode::Identity);
                }
            }
        }
        // Admission-time canonicalisation per plan B1a:
        //   - `ProjectMember { base, member, mode }` rewrites to
        //     `ProjectPath { base, path: [Member(member)], mode }` BEFORE the
        //     key is hashed into the memo, so sugar and canonical share one
        //     entry and one in-flight wait graph.
        //   - `IndexedAccess { base, index, mode }` rewrites the same way to
        //     `ProjectPath { base, path: [Index(index)], mode }`.
        //   - `NormalizeUnion` / `NormalizeIntersection` get structural
        //     member-list canonicalisation so `{A, B}` and `{B, A}` converge.
        // Other variants key off [`SemanticNodeId`]s that are already hashed
        // verbatim.
        let key = match key {
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Member(member)].into_boxed_slice()),
                    mode,
                }
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Index(index)].into_boxed_slice()),
                    mode,
                }
            }
            SemanticQueryKey::NormalizeUnion { members } => SemanticQueryKey::NormalizeUnion {
                members: canonicalize_node_list(&members),
            },
            SemanticQueryKey::NormalizeIntersection { members } => {
                SemanticQueryKey::NormalizeIntersection {
                    members: canonicalize_node_list(&members),
                }
            }
            other => other,
        };

        let graph = Arc::clone(self.graph());
        // Per-key recursion sentinel: when the memo detects same-path
        // re-entry on an `Instantiate` key, extract the decl name from
        // to its name and emit `Opaque(RecursiveRef { name })`. The
        // materialiser (`meta_resolve.rs:7447+`) recognises
        // `TypeExpr::RecursiveRef` as a leaf and stops expansion, which
        // is how recursive aliases like `type Tree = { children: Tree[] }`
        // terminate without stack overflow. Non-Instantiate re-entry
        // falls back to `Opaque(Miss)`.
        let sentinel_key = key.clone();
        let sentinel = {
            let graph = Arc::clone(&graph);
            move || {
                // C16: DeclIdentity carries the name directly — no
                // DeclAnchor node lookup needed.
                if let SemanticQueryKey::Instantiate { base, .. } = &sentinel_key {
                    return graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
                        name: Arc::clone(&base.decl_name),
                    }));
                }
                graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss))
            }
        };
        let key_for_build = key.clone();
        let build = move || -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
            match &key_for_build {
                SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key).into(),
                SemanticQueryKey::TypeOf { value_root } => self.build_typeof(value_root).into(),
                SemanticQueryKey::Instantiate {
                    base,
                    args,
                    body_mode,
                } => self.build_instantiate(base, args, *body_mode).into(),
                // C4: `ProjectMember` / `IndexedAccess` are API sugar
                // that admission-time canonicalisation rewrites to
                // `ProjectPath` above. The build closure never observes
                // these variants on the rewritten key; the arms below are
                // pure exhaustiveness: they forward to `build_project_path`
                // with a length-1 path so any future refactor that skips
                // admission canonicalisation still gets the correct
                // path-precise semantics.
                SemanticQueryKey::ProjectMember { base, member, mode } => {
                    let path: Arc<[PathSegment]> =
                        Arc::from(vec![PathSegment::Member(Arc::clone(member))].into_boxed_slice());
                    self.build_project_path(*base, &path, *mode)
                }
                SemanticQueryKey::IndexedAccess { base, index, mode } => {
                    let path: Arc<[PathSegment]> =
                        Arc::from(vec![PathSegment::Index(index.clone())].into_boxed_slice());
                    self.build_project_path(*base, &path, *mode)
                }
                SemanticQueryKey::ProjectPath { base, path, mode } => {
                    self.build_project_path(*base, path, *mode)
                }
                SemanticQueryKey::KeyOf { base } => self.build_key_of(*base).into(),
                SemanticQueryKey::MappedType { source, mapper } => {
                    self.build_mapped_type(*source, mapper).into()
                }
                SemanticQueryKey::Conditional {
                    check,
                    extends,
                    true_branch,
                    false_branch,
                    distributive,
                } => self
                    .build_conditional(*check, *extends, *true_branch, *false_branch, *distributive)
                    .into(),
                SemanticQueryKey::NormalizeUnion { members } => {
                    self.build_normalize_union(members).into()
                }
                SemanticQueryKey::NormalizeIntersection { members } => {
                    self.build_normalize_intersection(members).into()
                }
                SemanticQueryKey::ResolvedNamedType { key } => {
                    self.build_resolved_named_type(key).into()
                }
                // Phase D §5.4 WIP-S: the relation engine routes through its own
                // `SemanticGraphStore::relation_memo` DashMap rather than the
                // family memo. Executing `Relate` through the family path is a
                // degenerate build that always produces an `Opaque(Miss)` node
                // — callers use `ProjectSemanticDispatch::relate_nodes` directly
                // (see `relation.rs`) which consults the pairwise memo.
                SemanticQueryKey::Relate { .. } => {
                    let fence = self.project_generation_signature();
                    (QueryResult::Error(QueryError::Miss), fence).into()
                }
                // Binding amendment — `ResolveMacroPayload`.
                SemanticQueryKey::ResolveMacroPayload {
                    owner,
                    macro_index,
                    macro_kind,
                    type_args,
                    mode,
                } => self
                    .build_resolve_macro_payload(owner, *macro_index, *macro_kind, type_args, *mode)
                    .into(),
            }
        };
        let cache_read = graph.execute_cooperative(key.clone(), sentinel, build);
        // Dispatch-layer event — emitted once per `execute()` call.
        // The memo's per-call hit/miss/suppress events fire from
        // inside `execute_cooperative` and are independent. This
        // event captures the caller's view: the key, whether the
        // result is suppressed, and the kind of result observed.
        tracing::debug!(
            target: "verter::dispatch::execute_read",
            ?key,
            suppress = cache_read.cache_suppress,
            "execute_read"
        );
        cache_read.value
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Utilities exposed for higher-level callers
// ──────────────────────────────────────────────────────────────────────────

/// Convenience: construct a [`ResolveDeclKey`] for a top-level symbol in
/// `canonical_id`. Wrapping the arc-conversion here keeps call sites tidy
/// and avoids having each caller re-invent the scope construction.
#[must_use]
pub fn resolve_decl_key(canonical_id: &str, name: &str) -> ResolveDeclKey {
    ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical_id),
            local_scope: None,
        },
        name: Arc::from(name),
    }
}

/// Convenience: fetch the resolved semantic-node payload for a previously
/// executed key. Returns `None` if the memo has not warmed the key yet.
///
/// Accepts `&dyn ResolverContext`; the trait method
/// `dispatch_node_data` provides the same access from any context.
/// Existing callers passing `&VerterHost` upcast implicitly.
#[must_use]
pub(crate) fn node_data_for(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<Arc<SemanticNodeData>> {
    ctx.project_type_store().semantic_graph().node_data(node)
}

// Small helper to let the dispatcher express "this node has no member of
// name N on a concrete surface" without the call-site pattern-matching
// boilerplate leaking into the build_* functions. Not part of the public
// API; `SurfaceView` itself owns the traversal structure.
#[allow(dead_code)]
fn find_member(surface: &SurfaceView, needle: &str) -> Option<SemanticNodeId> {
    surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == needle)
        .map(|m| m.value)
}

// ──────────────────────────────────────────────────────────────────────────
// Dispatch helpers (NON-variant; compose existing variants
// + read sidecars). These are the 3 helpers that replace the 3 originally
// proposed `SemanticQueryKey` variants per the §0 binding amendment
// (`MaterializeSurface`, `ResolvePublicInstance`, `ResolveFallthroughSurface`)
// + 1 extra utility helper (`execute_to_type_expr`).
//
// Per §0 amendment: these are NON-variant — `ProjectSemanticDispatch` does
// not gain new variants; instead, it exposes plain methods that compose
// existing dispatchers and read the
// `ComponentMetaResultDb<ComponentMetaAnalysis>` sidecar at
// `verter_semantic/src/analysis/component_meta.rs:83-110`.
//
// **No callsite changes in this commit** — callsite migrations (4a/4b/4c
// classes A/B/C, 5/6/7/8/9 classes D + R) land in 5d-5f.
// ──────────────────────────────────────────────────────────────────────────

/// `__builtin__` decl identity for the `Pick` utility.
///
/// Used by [`ProjectSemanticDispatch::execute_pick`] to dispatch through
/// the existing `Instantiate` variant's built-in utility branch
/// (`build_builtin_utility` Pick arm at `build.rs:870`). The
/// `canonical_id = "__builtin__"` sentinel matches the convention at
/// `meta_resolve.rs:9959/9977/9998` and the route through `UtilitySource::Builtin`
/// per the existing `adapter.utility_source(base, "Pick")` code path.
#[must_use]
pub fn pick_builtin_decl_identity() -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        whole_hash: [0u8; 16],
        decl_name: Arc::from("Pick"),
    }
}

/// `__builtin__` decl identity for the `Omit` utility.
///
/// Used by [`ProjectSemanticDispatch::execute_omit`]. Mirrors the Pick
/// helper above, dispatching through the existing
/// `build_builtin_utility` Omit arm at `build.rs:911` (which preserves
/// call/construct/index signatures per `:937-939`).
#[must_use]
#[allow(dead_code)]
pub fn omit_builtin_decl_identity() -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        whole_hash: [0u8; 16],
        decl_name: Arc::from("Omit"),
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Direct mirror of
    /// [`materialize_component_meta_structure`](crate::component_meta_materialize::materialize_component_meta_structure).
    ///
    /// Caller pattern-matches `MaterializeOutcome` directly — no
    /// conversion to `QueryResult`. Use this when callers need
    /// `PackageRefTopLevel` / `FunctionPropertyAtNested` /
    /// `RecursiveHelperCycleGuard` / `MAX_DEPTH` gates (which
    /// `ProjectPath` does NOT inherit).
    ///
    /// **Why not a `SemanticQueryKey` variant?** Per §0 binding
    /// amendment, `MaterializeSurface` is redundant with the existing
    /// `ComponentMetaResultDb<ComponentMetaAnalysis>` sidecar +
    /// `MaterializeStructureDb` cache. This helper exposes the
    /// existing function as a method on the dispatcher so consumers
    /// can route policy-gated materialisations through dispatch
    /// without inflating the variant set.
    #[allow(dead_code)]
    pub fn materialize_surface(
        &self,
        key: crate::component_meta_materialize::MaterializeStructureCacheKey,
    ) -> CacheRead<crate::component_meta_materialize::MaterializeOutcome> {
        crate::component_meta_materialize::materialize_component_meta_structure(self.ctx, key)
    }

    /// `Pick<base, members>` via the existing builtin
    /// Pick dispatch (`build_builtin_utility` Pick arm at `build.rs:870`).
    ///
    /// Inherits TS Pick semantics including modifier preservation
    /// (`optional`/`readonly` flow through). The members slice is
    /// interned as a `Union` of string literals via
    /// [`Self::intern_string_literal_union`] before dispatch — empty
    /// members produce a `Primitive(Never)` keyspace which the Pick
    /// arm reduces to an empty Object (TS spec §4.4 Pick semantics).
    ///
    /// **Why not a variant?** Per §0 binding amendment, `Pick` /
    /// `Omit` are already routed through the
    /// `build_builtin_utility` path under
    /// `SemanticQueryKey::Instantiate`. This helper is a typed
    /// convenience wrapper — the underlying memo entry is the same
    /// `Instantiate` family/slot.
    pub fn execute_pick(
        &self,
        base: SemanticNodeId,
        members: &[Arc<str>],
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        let key_set = self.intern_string_literal_union(members);
        self.execute(SemanticQueryKey::Instantiate {
            base: pick_builtin_decl_identity(),
            args: Arc::from(vec![base, key_set].into_boxed_slice()),
            body_mode: mode,
        })
    }

    /// `Omit<base, members>` via the existing builtin
    /// Omit dispatch (`build_builtin_utility` Omit arm at `build.rs:911`).
    ///
    /// Inherits TS Omit semantics including the "Omit preserves
    /// source signatures" rule (per `build.rs:937-939` doc) — call /
    /// construct / index signatures of the base are kept when the
    /// keys-to-omit don't shadow them.
    #[allow(dead_code)]
    pub fn execute_omit(
        &self,
        base: SemanticNodeId,
        members: &[Arc<str>],
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        let key_set = self.intern_string_literal_union(members);
        self.execute(SemanticQueryKey::Instantiate {
            base: omit_builtin_decl_identity(),
            args: Arc::from(vec![base, key_set].into_boxed_slice()),
            body_mode: mode,
        })
    }

    /// `execute(key)` + `raise_node_to_type_expr` in
    /// one call, returning the full `CacheRead` so dep_signature is
    /// preserved for the caller's fence merge (Codex round 7 P1
    /// rejected lossy `Option<TypeExpr>` returns).
    ///
    /// Used by trampoline conversions and consumer migrations that
    /// need a `TypeExpr` for downstream `ComponentMetaAnalysis` field
    /// shape consumption.
    pub fn execute_to_type_expr(&self, key: &SemanticQueryKey) -> CacheRead<QueryResult<TypeExpr>> {
        let read = self.execute_read(key.clone());
        let typed_value = match read.value {
            QueryResult::Value(node) => match self.raise_node_to_type_expr(node) {
                Some(expr) => QueryResult::Value(expr),
                None => QueryResult::Error(QueryError::Miss),
            },
            QueryResult::Recursive(id) => QueryResult::Recursive(id),
            QueryResult::Error(e) => QueryResult::Error(e),
        };
        CacheRead {
            value: typed_value,
            dep_signature: read.dep_signature,
            walker_diagnostics: read.walker_diagnostics,
            cache_suppress: read.cache_suppress,
        }
    }

    /// Slot-binding-parameter type lowering.
    ///
    /// Given the `defineSlots<T>()` macro payload's lowered base node
    /// and the target `slot_name` + `binding_name` pair, projects the
    /// slot's first-parameter Object's `binding_name` member through
    /// dispatch via existing variants — i.e., composes:
    ///
    /// 1. `ProjectPath { base, [Member(slot_name)], Navigate }` →
    ///    yields the slot value's `SemanticNodeId`. Navigate mode is
    ///    correct here because this is an intermediate hop per
    ///    CLAUDE.md "Macro Type Traversal Rule"
    ///    (path-precise rule — only the terminal hop runs in the
    ///    caller's mode).
    /// 2. Reads the slot value's [`SemanticNodeData`]: a slot's value
    ///    is a `Function` (call signature), and the binding lives on
    ///    its first parameter's `Object` surface. Pull `params[0].ty`.
    /// 3. `ProjectPath { base: param0_ty, [Member(binding_name)], mode }` →
    ///    yields the binding's lowered type in the caller's mode
    ///    (typically `Expanded` for component-meta).
    ///
    /// **Why a helper, not a new variant?** The slot-binding lowering
    /// composes existing variants and lives as a non-variant dispatch
    /// helper. Mirrors `execute_pick` / `execute_omit` /
    /// `materialize_surface` / `execute_to_type_expr`, which are also
    /// non-variant dispatch helpers.
    ///
    /// **Migration source:** the engine analysis path's
    /// `expand_field_expr` closure used to dispatch a single
    /// `ProjectPath { base, [Member(slot), Member(binding)], Expanded }`
    /// directly. The walker emits `Opaque(Miss)` when it reaches the
    /// slot's `Function` value with `Member(binding)` remaining (per
    /// `walk.rs:606-625` — `SemanticNodeData::Function { .. }` falls
    /// through to `opaque_miss`), so the engine output for typed slot
    /// bindings was `Unknown { raw: "semanticMiss" }`. This helper
    /// closes that gap by descending through the `Function`'s
    /// first-parameter into the binding's Object member.
    ///
    /// Returns:
    /// - `CacheRead<QueryResult<TypeExpr>>` so dep_signature flows
    ///   back to the caller's local fence (mirrors
    ///   `execute_to_type_expr`).
    /// - `QueryResult::Error(Miss)` when the intermediate hop misses,
    ///   the slot value is not a `Function`, or the function has no
    ///   parameters. Caller falls back to symbolic preservation per
    ///   the engine's existing pattern.
    pub fn project_slot_binding_member(
        &self,
        base: SemanticNodeId,
        slot_name: &str,
        binding_name: &str,
        mode: ProjectionMode,
    ) -> CacheRead<QueryResult<TypeExpr>> {
        // Hop 1: navigate the slot member from the macro payload base.
        // Path-precise rule: intermediate hops use Navigate mode so the
        // shared memo stores the intermediate at Navigate-mode key
        // regardless of the caller's terminal mode (CLAUDE.md "Macro
        // Type Traversal Rule").
        let slot_path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from(slot_name))].into_boxed_slice());
        let slot_read = self.execute_read(SemanticQueryKey::ProjectPath {
            base,
            path: slot_path,
            mode: ProjectionMode::Navigate,
        });
        let slot_node = match slot_read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(id) => {
                return CacheRead {
                    value: QueryResult::Recursive(id),
                    dep_signature: slot_read.dep_signature,
                    walker_diagnostics: slot_read.walker_diagnostics,
                    cache_suppress: slot_read.cache_suppress,
                };
            }
            QueryResult::Error(e) => {
                return CacheRead {
                    value: QueryResult::Error(e),
                    dep_signature: slot_read.dep_signature,
                    walker_diagnostics: slot_read.walker_diagnostics,
                    cache_suppress: slot_read.cache_suppress,
                };
            }
        };

        // Hop 2: read the slot value's first-parameter type.
        // Per the slot-binding semantics (Verter macros §slots), every
        // slot key surfaces as a slot whose bindings live on the slot
        // function's first parameter Object literal.
        let param0_ty = match node_data_for(self.ctx, slot_node).as_deref() {
            Some(SemanticNodeData::Function { params, .. }) => match params.first() {
                Some(param) => param.ty,
                None => {
                    return CacheRead {
                        value: QueryResult::Error(QueryError::Miss),
                        dep_signature: slot_read.dep_signature,
                        walker_diagnostics: slot_read.walker_diagnostics,
                        cache_suppress: slot_read.cache_suppress,
                    };
                }
            },
            _ => {
                return CacheRead {
                    value: QueryResult::Error(QueryError::Miss),
                    dep_signature: slot_read.dep_signature,
                    walker_diagnostics: slot_read.walker_diagnostics,
                    cache_suppress: slot_read.cache_suppress,
                };
            }
        };

        // Hop 3: project the binding member off the param Object in
        // the caller's mode (terminal hop runs in the requested mode
        // per the path-precise rule).
        let binding_path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from(binding_name))].into_boxed_slice());
        let binding_read = self.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
            base: param0_ty,
            path: binding_path,
            mode,
        });
        // Merge dep signatures across the three hops so any change in
        // the intermediate (slot Function shape) or terminal (binding
        // Object) is observed by the caller's local fence.
        let merged: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = slot_read
            .dep_signature
            .iter()
            .cloned()
            .chain(binding_read.dep_signature.iter().cloned())
            .collect();
        CacheRead {
            value: binding_read.value,
            dep_signature: Arc::from(merged.into_boxed_slice()),
            walker_diagnostics: binding_read.walker_diagnostics,
            cache_suppress: binding_read.cache_suppress,
        }
    }

    /// Trivial helper: lower a `[String]` member-name
    /// list to an `Arc<[PathSegment]>` for `ProjectPath` queries.
    ///
    /// Each member name becomes a `PathSegment::Member(Arc<str>)`. The
    /// result has the same length and order as the input.
    #[must_use]
    #[allow(dead_code)]
    pub fn lower_path_segments(p: &[String]) -> Arc<[PathSegment]> {
        let segs: Vec<PathSegment> = p
            .iter()
            .map(|s| PathSegment::Member(Arc::from(s.as_str())))
            .collect();
        Arc::from(segs.into_boxed_slice())
    }

    /// Trivial helper: intern a `[Arc<str>]` member-name
    /// list as a `Union` of string-literal nodes.
    ///
    /// Empty input produces `Primitive(Never)` — caller-side pickup of
    /// the TS spec §4.4 rule that `Pick<T, never>` reduces to `{}`.
    /// Single-element input produces a `Union<[lit]>` (always
    /// uniformly `Union` even at arity 1, for caller uniformity).
    /// `Pick` / `Omit` callers can pass the result directly as the
    /// 2nd argument of `Instantiate`.
    #[must_use]
    pub fn intern_string_literal_union(&self, members: &[Arc<str>]) -> SemanticNodeId {
        let graph = self.graph();
        if members.is_empty() {
            return graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        }
        let lit_ids: Vec<SemanticNodeId> = members
            .iter()
            .map(|m| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    m.to_string(),
                )))
            })
            .collect();
        graph.intern_node(SemanticNodeData::Union(Arc::from(
            lit_ids.into_boxed_slice(),
        )))
    }
}

// ──────────────────────────────────────────────────────────────────────────
// DispatchHost trait + session-owned adapter
// ──────────────────────────────────────────────────────────────────────────
//
// `DispatchHost` is the scope-free minimum-surface host seam dispatch
// builders use to reach host-owned prepared declarations, root identities,
// utility classification, and bare-reference origin classification.
//
// The session-owned adapter [`SessionDispatchHost`] implements the trait
// by consulting [`SemanticGraphStore::node_scope`] for each base node to
// reconstruct the originating scope and fetch the scope's declaration-
// scope payload via [`crate::resolver_core::bare_name_resolve`]. Dispatch
// builders take `&dyn DispatchHost` rather than `&VerterHost`, so they
// stay scope-free.

/// Scope-free, minimum-surface host seam for dispatch builders ( +
/// §7.10 + C1).
///
/// Builders that need to reach host-owned prepared declarations, classify
/// utility names, resolve root identities, or classify bare references take
/// `&dyn DispatchHost`. The adapter internally queries
/// [`SemanticGraphStore::node_scope`](crate::semantic_query_memo::SemanticGraphStore::node_scope)
/// on the `base` id to route each lookup through the correct per-base
/// scope — builders stay scope-free.
///
/// **Contract:** every method takes the base [`SemanticNodeId`] whose scope
/// informs the lookup. Implementations resolve identities through
/// [`crate::resolver_core::bare_name_resolve`] over the scope's
/// declaration-scope payload.
///
/// Implementations: [`SessionDispatchHost`] routes per-base via the
/// node-scope sidecar.
pub trait DispatchHost {
    /// Look up a prepared type declaration by its canonical root identity,
    /// using the scope recorded in `base`'s sidecar to select the correct
    /// declaration-scope payload.
    fn resolve_prepared_type_decl(
        &self,
        base: SemanticNodeId,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>>;

    /// Resolve a `(canonical_id, symbol_name)` pair into a stable root
    /// declaration identity, following re-exports and barrel hops through
    /// `base`'s scope.
    #[allow(dead_code)]
    fn root_identity(
        &self,
        base: SemanticNodeId,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity>;

    /// Classify whether `name` is a built-in TS utility, user-shadowed, or
    /// unknown in `base`'s scope. Scope matters because a local binding in
    /// scope A can shadow a built-in that is not shadowed in scope B.
    fn utility_source(&self, base: SemanticNodeId, name: &str) -> UtilitySource;

    /// Classify whether `name` resolves locally or through an import in
    /// `base`'s scope. Used by lazy field expansion to keep imported
    /// object-like refs symbolic until a deeper route is requested.
    #[allow(dead_code)]
    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin;
}

/// Session-owned [`DispatchHost`] implementation.
///
/// Given a base [`SemanticNodeId`], consults
/// [`SemanticGraphStore::node_scope`] to reconstruct the originating
/// [`NodeScopeId`]. Fetches the scope's declaration-scope payload via
/// `prepared_decl_bundle` and routes each trait method through
/// [`crate::resolver_core::bare_name_resolve`] helpers:
///
/// - [`NodeScopeId::File { canonical_id, .. }`] → scope canonical + scope payload
/// - [`NodeScopeId::Global`] / exempt / missing → no scope context
///
/// The adapter holds only a host reference; scope is resolved fresh per
/// call so the adapter stays `Send + Sync` and cheap to construct.
pub struct SessionDispatchHost<'a> {
    ctx: &'a dyn ResolverContext,
}

impl<'a> SessionDispatchHost<'a> {
    /// Construct an adapter bound to `ctx`. The adapter does not retain a
    /// base node or a scope payload — it re-resolves per-base scope on
    /// every call via [`Self::base_scope`].
    ///
    /// Locked-in signature: takes `&dyn ResolverContext`.
    /// Existing call sites pass `&host` (concrete `&VerterHost`) and
    /// upcast implicitly because `impl ResolverContext for VerterHost`.
    #[must_use]
    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
        Self { ctx }
    }

    /// Public accessor for `base`'s recorded origin scope. Returns
    /// [`NodeScopeId::Global`] for exempt or missing nodes so every base
    /// has a well-defined routing decision.
    ///
    /// Exposed so tests can observe routing behaviour without retaining
    /// a scope payload directly.
    #[must_use]
    pub fn base_scope(&self, base: SemanticNodeId) -> NodeScopeId {
        self.ctx
            .project_type_store()
            .semantic_graph()
            .node_scope(base)
            .unwrap_or(NodeScopeId::Global)
    }

    /// Fetch the declaration-scope payload for `base`'s origin scope.
    /// Returns `None` when the scope is global/exempt, or when the scope
    /// has no prepared-decl bundle (e.g. a declaration file the host has
    /// not yet shallow-indexed). The payload is consulted by the
    /// `DispatchHost` trait methods for scope-local name resolution +
    /// shadowing.
    fn scope_payload_for_base(
        &self,
        base: SemanticNodeId,
    ) -> (
        Option<String>,
        Option<crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
    ) {
        match self.base_scope(base) {
            NodeScopeId::File { canonical_id, .. } => {
                let payload = self.ctx.prepared_decl_bundle(canonical_id.as_ref()).map(
                    |bundle| {
                        crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                            &bundle,
                        )
                    },
                );
                (Some(canonical_id.as_ref().to_string()), payload)
            }
            NodeScopeId::Global => (None, None),
        }
    }
}

impl<'a> DispatchHost for SessionDispatchHost<'a> {
    fn resolve_prepared_type_decl(
        &self,
        base: SemanticNodeId,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        let (scope_canonical, payload) = self.scope_payload_for_base(base);
        crate::resolver_core::bare_name_resolve::resolve_prepared_type_decl_via_host(
            self.ctx,
            scope_canonical.as_deref(),
            payload.as_ref(),
            root_identity,
        )
    }

    fn root_identity(
        &self,
        base: SemanticNodeId,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity> {
        let (scope_canonical, payload) = self.scope_payload_for_base(base);
        // Match `SessionSolverHost::root_identity`: empty caller
        // canonical defers to the base's origin scope.
        let resolution_scope = if canonical_id.is_empty() {
            scope_canonical.as_deref().unwrap_or("")
        } else {
            canonical_id
        };
        crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.ctx,
            resolution_scope,
            payload.as_ref(),
            symbol_name,
        )
    }

    fn utility_source(&self, base: SemanticNodeId, name: &str) -> UtilitySource {
        use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
        let (_scope_canonical, payload) = self.scope_payload_for_base(base);
        // Scope-local shadowing takes priority: a userland `type Partial`
        // in scope wins over the built-in utility.
        if let Some(payload) = payload.as_ref() {
            if payload.scope_type_names.contains(name)
                || payload.scope_type_bindings.contains_key(name)
            {
                return UtilitySource::Shadowed;
            }
        }
        // SDK-declared intrinsics always classify as `Builtin` regardless
        // of shadowing — matches `SessionSolverHost::utility_source`.
        if let crate::intrinsic_registry::IntrinsicLookup::Found(_) = self
            .ctx
            .project_type_store()
            .intrinsic_registry()
            .lookup(name)
        {
            return UtilitySource::Builtin;
        }
        if BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin {
        let (_scope_canonical, payload) = self.scope_payload_for_base(base);
        if let Some(payload) = payload.as_ref() {
            if payload.import_bindings.contains_key(name) {
                return BareRefOrigin::Imported;
            }
            if payload.scope_type_bindings.contains_key(name)
                || payload.scope_type_names.contains(name)
                || payload.scope_value_names.contains(name)
            {
                return BareRefOrigin::Local;
            }
        }
        BareRefOrigin::Unknown
    }
}

#[cfg(test)]
mod tests;
