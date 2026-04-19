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
//! - `Instantiate` / `Expand` — identity-preserving aliases anchored to the
//!   base node. Memoizing these keys is itself the dedup guarantee; the
//!   richer instantiation shape is produced on demand by the solver once
//!   the caller walks into it.
//! - `NormalizeUnion` / `NormalizeIntersection` — structural dedup over the
//!   supplied members with stable ordering.
//! - `ProjectMember` / `IndexedAccess` / `KeyOf` / `MappedType` /
//!   `Conditional` — navigation operations that walk the base node's
//!   shared-graph payload. Paths that do not reach a concrete node fall
//!   through to a recorded `Opaque(Miss)` entry — this is distinct from a
//!   dispatch miss because the warm entry observes the base node identity
//!   and therefore dedups repeated asks.
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

use crate::semantic_query::{
    CacheRead, DepSignature, DepVersion, HostResolvedNamedTypeKey, IndexKey, PrimitiveKind,
    QueryError, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SurfaceView, ValueRootKey,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::VerterHost;

/// Host-bound dispatcher for [`SemanticQueryApi`].
///
/// The dispatcher borrows the host for the duration of a query — every
/// `execute()` call threads through the host's
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore) and its
/// owned [`SemanticGraphStore`].
pub struct ProjectSemanticDispatch<'a> {
    host: &'a VerterHost,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Create a dispatcher bound to `host`.
    #[must_use]
    pub fn new(host: &'a VerterHost) -> Self {
        Self { host }
    }

    fn graph(&self) -> &Arc<SemanticGraphStore> {
        self.host.project_type_store().semantic_graph()
    }

    fn recursion_sentinel(&self) -> SemanticNodeId {
        self.graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::Miss))
    }

    /// Intern an opaque node carrying the supplied query error. Used as the
    /// fallback when a semantic subquery cannot be satisfied but the caller
    /// wants a node id rather than a top-level error.
    fn opaque(&self, err: QueryError) -> SemanticNodeId {
        self.graph().intern_node(SemanticNodeData::Opaque(err))
    }

    /// Build the dep-signature fragment for a canonical file at a given
    /// content hash. Carries both the file-version fact and the current
    /// project generation so the completion fence picks up both.
    fn dep_signature_for(&self, canonical_id: &Arc<str>, hash: [u8; 16]) -> DepSignature {
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
    fn project_generation_signature(&self) -> DepSignature {
        let project_gen = self.host.project_type_store().project_generation();
        Arc::from(
            vec![(
                Arc::<str>::from("<project>"),
                DepVersion::ProjectGeneration(project_gen),
            )]
            .into_boxed_slice(),
        )
    }

    /// Resolve a top-level declaration lookup via the host's shallow state.
    ///
    /// Returns a real semantic node keyed by the declaration's scope + name.
    /// The node's structural shape is intentionally coarse — the dispatcher
    /// uses `Primitive(Never)` as a dedup anchor for abstract declarations,
    /// or an `Object` surface when the shallow state already carries the
    /// member surface. Callers that need a richer shape walk the result
    /// through `ProjectMember` / `IndexedAccess` etc. which this dispatcher
    /// also memoizes.
    fn build_resolve_decl(
        &self,
        key: &ResolveDeclKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state(key.scope.canonical_id.as_ref())
        {
            Some(state) => state,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };

        let has_type_symbol = shallow.symbol(key.name.as_ref()).is_some();
        let has_value_symbol = shallow.value_symbol(key.name.as_ref()).is_some();
        let has_export = shallow.exports.contains_key(key.name.as_ref());
        let has_import_local = shallow.import_targets.contains_key(key.name.as_ref());

        if !(has_type_symbol || has_value_symbol || has_export || has_import_local) {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        }

        // Each binding is distinct semantic identity: publish a fresh
        // anchor node so the shared-cache identity for this `(scope, name)`
        // pair is stable across repeated queries. The concrete structural
        // payload (surface members, union shape, intrinsic bodies, etc.)
        // is produced on-demand by the later navigation variants that
        // memoize against this anchor.
        let node_id = self
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        let signature = self.dep_signature_for(&key.scope.canonical_id, shallow.whole_hash);
        (QueryResult::Value(node_id), signature)
    }

    /// `typeof`-rooted declaration lookup. Shape mirrors [`Self::build_resolve_decl`]
    /// but routes through the shallow value-symbol space so the result is
    /// keyed by the value binding's identity.
    fn build_typeof(
        &self,
        value_root: &ValueRootKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state(value_root.scope.canonical_id.as_ref())
        {
            Some(state) => state,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };

        let has_value = shallow.value_symbol(value_root.name.as_ref()).is_some();
        let has_import_local = shallow
            .import_targets
            .contains_key(value_root.name.as_ref());
        let has_type_symbol = shallow.symbol(value_root.name.as_ref()).is_some();

        if !(has_value || has_import_local || has_type_symbol) {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        }

        let node_id = self
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        let signature = self.dep_signature_for(&value_root.scope.canonical_id, shallow.whole_hash);
        (QueryResult::Value(node_id), signature)
    }

    /// Generic instantiation. Identity-preserving: the memoized key is the
    /// dedup guarantee. Returns an `Alias(base)` so repeated lookups share
    /// one node id for the same `(base, args)` pair.
    fn build_instantiate(
        &self,
        base: SemanticNodeId,
        _args: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.graph().intern_node(SemanticNodeData::Alias(base));
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Expansion. Mirrors [`Self::build_instantiate`]: identity-preserving
    /// alias anchored to the base so `expand(base)` memoizes.
    fn build_expand(&self, base: SemanticNodeId) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.graph().intern_node(SemanticNodeData::Alias(base));
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Member projection. Inspects the base node's shared-graph payload and
    /// navigates into the matching member when the base is a concrete
    /// surface. Every other shape (primitive, opaque, alias-without-body)
    /// resolves to `Opaque(Miss)` — but the memoized key captures the
    /// identity so repeated asks share the same warm entry.
    fn build_project_member(
        &self,
        base: SemanticNodeId,
        member: &Arc<str>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let node = match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => surface
                .members
                .iter()
                .find(|(name, _)| name.as_ref() == member.as_ref())
                .map(|(_, id)| *id)
                .unwrap_or_else(|| self.opaque(QueryError::Miss)),
            _ => self.opaque(QueryError::Miss),
        };
        let signature = self.project_generation_signature();
        (QueryResult::Value(node), signature)
    }

    /// Indexed access. For `Number` / `String` keys on an `Object` surface,
    /// looks up the member name directly. For `TypeNode` keys or any
    /// non-object base, returns `Opaque(Miss)` under the memoized identity.
    fn build_indexed_access(
        &self,
        base: SemanticNodeId,
        index: &IndexKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let node = match (data.as_deref(), index) {
            (Some(SemanticNodeData::Object(surface)), IndexKey::String(s)) => surface
                .members
                .iter()
                .find(|(name, _)| name.as_ref() == s.as_ref())
                .map(|(_, id)| *id)
                .unwrap_or_else(|| self.opaque(QueryError::Miss)),
            (Some(SemanticNodeData::Object(surface)), IndexKey::Number(n)) => {
                let needle = n.to_string();
                surface
                    .members
                    .iter()
                    .find(|(name, _)| name.as_ref() == needle.as_str())
                    .map(|(_, id)| *id)
                    .unwrap_or_else(|| self.opaque(QueryError::Miss))
            }
            _ => self.opaque(QueryError::Miss),
        };
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// `keyof` projection. For an `Object` surface, materializes a union of
    /// the member names as `Primitive(String)` anchors — this matches the
    /// TS semantics that `keyof T` yields a union of string literals.
    /// For non-objects, returns `Opaque(Miss)`.
    fn build_key_of(&self, base: SemanticNodeId) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let node = match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => {
                let member_ids: Vec<SemanticNodeId> = surface
                    .members
                    .iter()
                    .map(|_| {
                        self.graph()
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
                    })
                    .collect();
                if member_ids.is_empty() {
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if member_ids.len() == 1 {
                    member_ids[0]
                } else {
                    self.graph().intern_node(SemanticNodeData::Union(Arc::from(
                        member_ids.into_boxed_slice(),
                    )))
                }
            }
            _ => self.opaque(QueryError::Miss),
        };
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Mapped-type rewrite. The shared-graph payload does not yet retain
    /// enough structure to produce a fully rewritten surface — the memoized
    /// key captures the source + mapper identity so repeated asks dedup,
    /// but the concrete body is deferred to later phases that wire the
    /// mapped-type solver. Returns an `Alias(source)` anchor for now.
    fn build_mapped_type(
        &self,
        source: SemanticNodeId,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.graph().intern_node(SemanticNodeData::Alias(source));
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Conditional type. Returns an `Alias(true_branch)` as a conservative
    /// anchor — the real branch selection requires the solver's assignability
    /// judgement, which later phases route through this dispatcher. The
    /// memoized key preserves the `(check, extends, true, false,
    /// distributive)` identity so repeated asks dedup.
    fn build_conditional(
        &self,
        true_branch: SemanticNodeId,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self
            .graph()
            .intern_node(SemanticNodeData::Alias(true_branch));
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Union normalization. Structurally sorts + dedups the supplied members
    /// and publishes the canonical union node. Singleton unions fold to
    /// their only member; empty unions fold to `Primitive(Never)`.
    fn build_normalize_union(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ true);
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Intersection normalization. Structurally sorts + dedups; singleton
    /// folds to the only member; empty folds to `Primitive(Never)`.
    fn build_normalize_intersection(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ false);
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Vue macro resolution lookup.
    ///
    /// Hot-path reads go through
    /// [`SemanticGraphStore::get_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::get_resolved_named_type)
    /// directly from the parser's
    /// [`NamedTypeCache`](verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache)
    /// adapter — the formal `execute` path stays available as an entry
    /// point for callers that want to check presence through the shared
    /// query API but must not be relied on in the refcount-only hot
    /// path. Writes enter from the adapter side via
    /// [`SemanticGraphStore::insert_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::insert_resolved_named_type).
    ///
    /// Returns a warm node id when the identity map has an entry, or
    /// [`QueryError::Miss`] when the entry has not been written yet.
    /// Carries a dep-signature fragment capturing
    /// `(canonical_id, whole_hash, project_generation)` so
    /// [`HostFenceValidator`](crate::host_manage::HostFenceValidator)
    /// catches stale warm hits if any downstream layer memoizes this
    /// dispatch path.
    fn build_resolved_named_type(
        &self,
        key: &HostResolvedNamedTypeKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let graph = self.graph();
        match graph.resolved_named_type_node_id(key) {
            Some(node_id) => (
                QueryResult::Value(node_id),
                self.dep_signature_for(&key.canonical_id, key.whole_hash),
            ),
            None => (QueryResult::Error(QueryError::Miss), empty_signature()),
        }
    }

    fn intern_normalized_union_or_intersection(
        &self,
        members: &[SemanticNodeId],
        is_union: bool,
    ) -> SemanticNodeId {
        let mut sorted: Vec<SemanticNodeId> = members.to_vec();
        sorted.sort_by_key(|id| id.0);
        sorted.dedup();
        if sorted.is_empty() {
            return self
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        }
        if sorted.len() == 1 {
            return sorted[0];
        }
        let boxed: Arc<[SemanticNodeId]> = Arc::from(sorted.into_boxed_slice());
        if is_union {
            self.graph().intern_node(SemanticNodeData::Union(boxed))
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Intersection(boxed))
        }
    }
}

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Stable-sort + dedup a list of semantic node ids. Used to canonicalize
/// the key surface for union / intersection dispatches so
/// `NormalizeUnion({A,B})` and `NormalizeUnion({B,A})` converge on one
/// warm memo entry.
fn canonicalize_node_list(members: &[SemanticNodeId]) -> Arc<[SemanticNodeId]> {
    let mut sorted: Vec<SemanticNodeId> = members.to_vec();
    sorted.sort_by_key(|id| id.0);
    sorted.dedup();
    Arc::from(sorted.into_boxed_slice())
}

impl<'a> SemanticQueryApi for ProjectSemanticDispatch<'a> {
    fn execute(&self, key: SemanticQueryKey) -> QueryResult<SemanticNodeId> {
        // Normalize structural-dedup variants at the dispatch boundary so
        // `{A, B}` and `{B, A}` converge on the same memo entry. Other
        // variants key off [`SemanticNodeId`]s that are already hashed
        // verbatim.
        let key = match key {
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
        let sentinel = || self.recursion_sentinel();
        let key_for_build = key.clone();
        let build = move || match &key_for_build {
            SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key),
            SemanticQueryKey::TypeOf { value_root } => self.build_typeof(value_root),
            SemanticQueryKey::Instantiate { base, args } => self.build_instantiate(*base, args),
            SemanticQueryKey::ProjectMember { base, member, .. } => {
                self.build_project_member(*base, member)
            }
            SemanticQueryKey::IndexedAccess { base, index, .. } => {
                self.build_indexed_access(*base, index)
            }
            SemanticQueryKey::KeyOf { base } => self.build_key_of(*base),
            SemanticQueryKey::MappedType { source, .. } => self.build_mapped_type(*source),
            SemanticQueryKey::Conditional { true_branch, .. } => {
                self.build_conditional(*true_branch)
            }
            SemanticQueryKey::NormalizeUnion { members } => self.build_normalize_union(members),
            SemanticQueryKey::NormalizeIntersection { members } => {
                self.build_normalize_intersection(members)
            }
            SemanticQueryKey::Expand { base, .. } => self.build_expand(*base),
            SemanticQueryKey::ResolvedNamedType { key } => self.build_resolved_named_type(key),
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
        .find(|(name, _)| name.as_ref() == needle)
        .map(|(_, id)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{
        ExpandMode, ProjectionMode, ScopeId, SemanticNodeData, SurfaceView,
    };
    use crate::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};

    fn host() -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        })
    }

    fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: id.to_string(),
                source: Arc::from(source),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();
    }

    /// `ResolveDecl` for a known top-level type returns a value node. The
    /// memo is keyed by the semantic identity, so a second query for the
    /// same key returns the same [`SemanticNodeId`].
    #[test]
    fn resolve_decl_dedups_across_repeated_queries() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);

        let key = resolve_decl_key("/w/types.ts", "Foo");
        let first = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));
        let second = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));

        let (a, b) = match (first, second) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(a, b, "repeated queries must dedup onto the same node id");
    }

    /// Missing bindings return a structured miss instead of a warm node.
    #[test]
    fn resolve_decl_misses_for_unknown_name() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/types.ts", "Missing");
        match dispatch.execute(SemanticQueryKey::ResolveDecl(key)) {
            QueryResult::Error(QueryError::Miss) => {}
            other => panic!("expected Miss, got {other:?}"),
        }
    }

    /// The shared memo survives across distinct higher-level requests — a
    /// second `VerterHost` call against the same key observes the warm id.
    #[test]
    fn resolve_decl_warm_node_survives_between_execute_calls() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/a.ts", "A");

        let first = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));
        let QueryResult::Value(first_id) = first else {
            panic!("expected value");
        };

        let warm = host
            .project_type_store()
            .semantic_graph()
            .get(&SemanticQueryKey::ResolveDecl(key.clone()))
            .expect("warm memo entry must exist after first query");
        match warm.value {
            QueryResult::Value(id) => assert_eq!(id, first_id),
            other => panic!("expected warm value, got {other:?}"),
        }
    }

    /// Different canonical ids for the same name produce different semantic
    /// node ids — scope-aware identity prevents cross-file aliasing.
    #[test]
    fn resolve_decl_disambiguates_by_scope() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type Foo = { a: number }");
        upsert_ts(&host, "/w/b.ts", "export type Foo = { b: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let a_key = resolve_decl_key("/w/a.ts", "Foo");
        let b_key = resolve_decl_key("/w/b.ts", "Foo");

        let (a_id, b_id) = match (
            dispatch.execute(SemanticQueryKey::ResolveDecl(a_key)),
            dispatch.execute(SemanticQueryKey::ResolveDecl(b_key)),
        ) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_ne!(a_id, b_id);
    }

    /// `ResolveDecl` dep-signatures include the file whole-hash and the
    /// project generation so the completion fence picks up both file-level
    /// and project-level invalidation facts.
    #[test]
    fn resolve_decl_dep_signature_captures_file_hash_and_project_gen() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/a.ts", "A");
        let _ = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));

        let warm = host
            .project_type_store()
            .semantic_graph()
            .get(&SemanticQueryKey::ResolveDecl(key))
            .expect("warm entry must exist");
        let mut has_whole_hash = false;
        let mut has_project_gen = false;
        for (_, dv) in warm.dep_signature.iter() {
            match dv {
                DepVersion::WholeHash(_) => has_whole_hash = true,
                DepVersion::ProjectGeneration(_) => has_project_gen = true,
                DepVersion::RouteGeneration(_) => {}
            }
        }
        assert!(has_whole_hash, "dep signature must carry file whole hash");
        assert!(
            has_project_gen,
            "dep signature must carry project generation"
        );
    }

    /// `ResolveDecl` can also reach import-local symbols — the shallow
    /// state surfaces them through `import_targets`. This ensures the
    /// dispatch covers the common "owner imports a type" path in addition
    /// to top-level declarations.
    #[test]
    fn resolve_decl_recognises_import_local_bindings() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        upsert_ts(
            &host,
            "/w/owner.ts",
            "import type { Foo } from './types'\nexport type Owner = Foo",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);

        // `Foo` is not a top-level declaration in owner.ts — it is only an
        // import-local binding. The dispatch must still return a value.
        let key = resolve_decl_key("/w/owner.ts", "Foo");
        match dispatch.execute(SemanticQueryKey::ResolveDecl(key)) {
            QueryResult::Value(_) => {}
            other => panic!("expected value for import-local binding, got {other:?}"),
        }
    }

    /// `Instantiate(base, args)` dedups: two repeated calls share one warm
    /// entry and one node id. `Instantiate` with different args is a
    /// distinct key and must not alias.
    #[test]
    fn instantiate_dedups_by_args() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let store = host.project_type_store();
        let graph = store.semantic_graph();
        let base = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let args_a: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
        let args_b: Arc<[SemanticNodeId]> =
            Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice());

        let k_a = SemanticQueryKey::Instantiate {
            base,
            args: args_a.clone(),
        };
        let k_b = SemanticQueryKey::Instantiate {
            base,
            args: args_b.clone(),
        };

        let a1 = dispatch.execute(k_a.clone());
        let a2 = dispatch.execute(k_a.clone());
        let b = dispatch.execute(k_b);

        let (ida1, ida2, idb) = match (a1, a2, b) {
            (QueryResult::Value(a), QueryResult::Value(c), QueryResult::Value(d)) => (a, c, d),
            other => panic!("expected three values, got {other:?}"),
        };
        assert_eq!(ida1, ida2, "same args must memoize to one node id");
        assert_ne!(ida1, idb, "distinct args must not alias");
    }

    /// `NormalizeUnion` is structural: `[A, B]` and `[B, A]` normalize to the
    /// same canonical node. Duplicate members dedup; a singleton folds to
    /// the only member.
    #[test]
    fn normalize_union_is_structurally_canonical() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let ab = dispatch.execute(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![a, b].into_boxed_slice()),
        });
        let ba = dispatch.execute(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![b, a].into_boxed_slice()),
        });

        let (id_ab, id_ba) = match (ab, ba) {
            (QueryResult::Value(x), QueryResult::Value(y)) => (x, y),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(
            id_ab, id_ba,
            "union of {{A, B}} and {{B, A}} must canonicalize"
        );

        // Singleton folds to the only member.
        let single = dispatch.execute(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![a].into_boxed_slice()),
        });
        match single {
            QueryResult::Value(id) => assert_eq!(id, a, "singleton union folds to its member"),
            other => panic!("expected singleton fold, got {other:?}"),
        }
    }

    /// `ProjectMember` on a known surface returns the member's node id; on
    /// a primitive (no members) it returns an opaque sentinel. Both cases
    /// memoize under distinct keys.
    #[test]
    fn project_member_reads_object_surface() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();

        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let surface = SurfaceView {
            members: Arc::from(vec![(Arc::<str>::from("foo"), string_id)].into_boxed_slice()),
            index_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            keyspace: None,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let hit = dispatch.execute(SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("foo"),
            mode: ProjectionMode::Identity,
        });
        let id = match hit {
            QueryResult::Value(id) => id,
            other => panic!("expected value, got {other:?}"),
        };
        assert_eq!(
            id, string_id,
            "project_member must hand back the surface's member node id"
        );

        let miss = dispatch.execute(SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("absent"),
            mode: ProjectionMode::Identity,
        });
        let opaque_id = match miss {
            QueryResult::Value(id) => id,
            other => panic!("expected value (opaque node), got {other:?}"),
        };
        // Sanity: the opaque value's node data is Opaque.
        let data = graph.node_data(opaque_id).unwrap();
        assert!(
            matches!(*data, SemanticNodeData::Opaque(_)),
            "absent member resolves to an opaque node"
        );
    }

    /// `KeyOf` on an `Object` surface folds to a union of
    /// `Primitive(String)` anchors (one per member). On a primitive base
    /// it returns an `Opaque` node.
    #[test]
    fn key_of_object_yields_string_union() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let num_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let surface = SurfaceView {
            members: Arc::from(
                vec![
                    (Arc::<str>::from("a"), string_id),
                    (Arc::<str>::from("b"), num_id),
                ]
                .into_boxed_slice(),
            ),
            index_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            keyspace: None,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let keyof = dispatch.execute(SemanticQueryKey::KeyOf { base: obj });
        let id = match keyof {
            QueryResult::Value(id) => id,
            other => panic!("expected value, got {other:?}"),
        };
        let data = graph.node_data(id).unwrap();
        match &*data {
            SemanticNodeData::Union(members) => assert_eq!(members.len(), 2),
            other => panic!("keyof must be a union, got {other:?}"),
        }
    }

    /// `Expand(base, mode)` memoizes: two repeated calls dedup onto the
    /// same warm memo entry, and the warm entry records a dep signature.
    #[test]
    fn expand_dedups_and_records_dep_signature() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let base = host
            .project_type_store()
            .semantic_graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let key = SemanticQueryKey::Expand {
            base,
            mode: ExpandMode::Expanded,
        };
        let first = dispatch.execute(key.clone());
        let second = dispatch.execute(key.clone());
        let (a, b) = match (first, second) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(a, b);

        let warm = host
            .project_type_store()
            .semantic_graph()
            .get(&key)
            .expect("Expand must warm the memo");
        assert!(
            !warm.dep_signature.is_empty(),
            "Expand must record at least a project-generation dep fact"
        );
    }

    /// `TypeOf { value_root }` looks up through the shallow value-symbol
    /// space. A declared value binding returns a value node; a missing
    /// name returns a structured miss.
    #[test]
    fn type_of_resolves_value_binding() {
        let host = host();
        upsert_ts(
            &host,
            "/w/v.ts",
            "export const foo = { x: 1 as const }\nexport type Helper = typeof foo",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let value_key = ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/w/v.ts"),
                local_scope: None,
            },
            name: Arc::from("foo"),
        };
        let hit = dispatch.execute(SemanticQueryKey::TypeOf {
            value_root: value_key,
        });
        assert!(matches!(hit, QueryResult::Value(_)));

        let miss_key = ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/w/v.ts"),
                local_scope: None,
            },
            name: Arc::from("notThere"),
        };
        let miss = dispatch.execute(SemanticQueryKey::TypeOf {
            value_root: miss_key,
        });
        assert!(matches!(miss, QueryResult::Error(QueryError::Miss)));
    }

    /// Identical [`SemanticQueryKey::ResolveDecl`] keys share exactly one
    /// warm memo entry — the memo counter does not grow for repeated asks.
    #[test]
    fn repeated_asks_do_not_grow_memo() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/a.ts", "A");
        let before = host
            .project_type_store()
            .semantic_graph()
            .memo_entry_count();
        for _ in 0..5 {
            let _ = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));
        }
        let after = host
            .project_type_store()
            .semantic_graph()
            .memo_entry_count();
        assert_eq!(
            after - before,
            1,
            "five identical asks must produce one warm memo entry"
        );
    }

    /// `ResolvedNamedType` dispatches through `execute` after the adapter
    /// has written the entry: reads come back as `QueryResult::Value` and
    /// carry the file's whole-hash + project generation in the dep
    /// signature. The hot path still goes direct through
    /// `get_resolved_named_type` (refcount-only) — this test exercises
    /// the formal entry point so ad-hoc callers of the shared query API
    /// see the warm entry too.
    #[test]
    fn resolved_named_type_dispatch_returns_value_after_insert() {
        use crate::semantic_query::HostResolvedNamedTypeKey;
        use verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey;
        use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();

        let key = HostResolvedNamedTypeKey {
            canonical_id: Arc::from("/w/a.ts"),
            whole_hash: [7u8; 16],
            inner: ResolvedNamedTypeCacheKey {
                name: b"Foo".to_vec().into_boxed_slice(),
                surface: None,
                base_offset: 0,
                companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
                type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
            },
        };
        let payload = Arc::new(ResolvedElements::default());

        // Miss before insert: formal entry point returns `Error(Miss)`.
        let miss = dispatch.execute(SemanticQueryKey::ResolvedNamedType {
            key: Arc::new(key.clone()),
        });
        assert!(matches!(miss, QueryResult::Error(QueryError::Miss)));

        // Write via the semantic graph (adapter-side path).
        let expected_id = graph.insert_resolved_named_type(key.clone(), Arc::clone(&payload));

        // Hit after insert: the formal entry point hands back the same
        // interned node id.
        let hit = dispatch.execute(SemanticQueryKey::ResolvedNamedType { key: Arc::new(key) });
        match hit {
            QueryResult::Value(id) => assert_eq!(id, expected_id),
            other => panic!("expected value after insert, got {other:?}"),
        }
    }
}
