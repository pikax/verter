//! Project-global [`SemanticQueryApi`] dispatcher (Phase 2.2).
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
//!   shape is produced on demand by the solver once the caller walks into
//!   it. Mode-free per the lazy-materialisation rule (plan §7.14).
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

use crate::semantic_query::{
    BranchSelection, CacheRead, DepSignature, DepVersion, IndexKey, NodeScopeId, OriginEdgeKind,
    OriginMeta, PathSegment, PrimitiveKind, ProjectionMode, QueryError, QueryResult,
    ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SurfaceView,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::VerterHost;
use verter_semantic::analysis::type_expr::PrimitiveName;

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
/// (plan §3 D-Cutover — recursive-ref back-edge detection during body
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
    pub(super) host: &'a VerterHost,
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
    /// assign different ordinals. Per plan §14.2, this is
    /// acceptable for C6a-through-C7 correctness; C17's sharded
    /// interner can revisit if hot-path cache stability becomes
    /// a measurable concern.
    pub(super) mapped_binder_ordinal: std::cell::Cell<u16>,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Create a dispatcher bound to `host`.
    #[must_use]
    pub fn new(host: &'a VerterHost) -> Self {
        Self {
            host,
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
        self.host.project_type_store().semantic_graph()
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
        let project_gen = self.host.project_type_store().project_generation();
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
        let project_gen = self.host.project_type_store().project_generation();
        Arc::from(
            vec![(
                Arc::<str>::from("<project>"),
                DepVersion::ProjectGeneration(project_gen),
            )]
            .into_boxed_slice(),
        )
    }

    /// Convenience wrapper for consumer migrations (plan §5.7 WIP-C).
    /// Lowers a [`TypeExpr`] rooted at the supplied canonical file scope
    /// via [`Self::shallow_lower_type_expr`] with empty env, empty
    /// name-resolution, and the scope's prepared-decl payload for
    /// bare-name fallback. The dispatch walker consults the scope's
    /// `DeclarationScopePayload` + host-owned `shallow_file_state` to
    /// resolve bare `TypeExpr::Ref` hops directly — no
    /// `SessionSolverHost` is constructed on this path (plan §5.7
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
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
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
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        mode: crate::semantic_query::ProjectionMode,
    ) -> Option<SemanticNodeId> {
        let shallow = self.host.shallow_file_state(scope_canonical_id)?;
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(scope_canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        let env = rustc_hash::FxHashMap::default();
        let name_resolution = rustc_hash::FxHashMap::default();
        let scope_payload = self
            .host
            .prepared_decl_bundle(scope_canonical_id)
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        let id = self.shallow_lower_type_expr(
            expr,
            &env,
            &scope,
            &name_resolution,
            scope_payload.as_ref(),
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
/// See plan §7.2 utility-equivalence rule and §3 C7.
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
// (plan §3 D-Cutover staging branch). Literal identity is now
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
        let build = move || match &key_for_build {
            SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key),
            SemanticQueryKey::TypeOf { value_root } => self.build_typeof(value_root),
            SemanticQueryKey::Instantiate {
                base,
                args,
                body_mode,
            } => self.build_instantiate(base, args, *body_mode),
            // Plan §3 C4: `ProjectMember` / `IndexedAccess` are API sugar
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
            SemanticQueryKey::KeyOf { base } => self.build_key_of(*base),
            SemanticQueryKey::MappedType { source, mapper } => {
                self.build_mapped_type(*source, mapper)
            }
            SemanticQueryKey::Conditional {
                check,
                extends,
                true_branch,
                false_branch,
                distributive,
            } => {
                self.build_conditional(*check, *extends, *true_branch, *false_branch, *distributive)
            }
            SemanticQueryKey::NormalizeUnion { members } => self.build_normalize_union(members),
            SemanticQueryKey::NormalizeIntersection { members } => {
                self.build_normalize_intersection(members)
            }
            SemanticQueryKey::ResolvedNamedType { key } => self.build_resolved_named_type(key),
            // Phase D §5.4 WIP-S: the relation engine routes through its own
            // `SemanticGraphStore::relation_memo` DashMap rather than the
            // family memo. Executing `Relate` through the family path is a
            // degenerate build that always produces an `Opaque(Miss)` node
            // — callers use `ProjectSemanticDispatch::relate_nodes` directly
            // (see `relation.rs`) which consults the pairwise memo.
            SemanticQueryKey::Relate { .. } => {
                let fence = self.project_generation_signature();
                (QueryResult::Error(QueryError::Miss), fence)
            }
        };
        let CacheRead { value, .. } = graph.execute_cooperative(key, sentinel, build);
        value
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
#[must_use]
pub fn node_data_for(host: &VerterHost, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
    host.project_type_store().semantic_graph().node_data(node)
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
// DispatchHost trait + session-owned adapter (plan §7.9 + §7.10 + C1)
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
// stay scope-free (plan §7.9).

/// Scope-free, minimum-surface host seam for dispatch builders (plan §7.9 +
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
    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin;
}

/// Session-owned [`DispatchHost`] implementation (plan §7.10).
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
    host: &'a VerterHost,
}

impl<'a> SessionDispatchHost<'a> {
    /// Construct an adapter bound to `host`. The adapter does not retain a
    /// base node or a scope payload — it re-resolves per-base scope on
    /// every call via [`Self::base_scope`].
    #[must_use]
    pub fn new(host: &'a VerterHost) -> Self {
        Self { host }
    }

    /// Public accessor for `base`'s recorded origin scope. Returns
    /// [`NodeScopeId::Global`] for exempt or missing nodes so every base
    /// has a well-defined routing decision.
    ///
    /// Exposed so tests can observe routing behaviour without retaining
    /// a scope payload directly.
    #[must_use]
    pub fn base_scope(&self, base: SemanticNodeId) -> NodeScopeId {
        self.host
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
                let payload = self.host.prepared_decl_bundle(canonical_id.as_ref()).map(
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
            self.host,
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
            self.host,
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
            .host
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
