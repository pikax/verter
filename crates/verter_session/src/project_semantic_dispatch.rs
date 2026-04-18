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
//! This module introduces the host-facing `SemanticQueryApi` binding in a
//! compile-green slice. Currently:
//!
//! - `ResolveDecl` is wired end-to-end: lookup shares the host-owned memo,
//!   cold builds consult `IndexedReady`, and dep-signatures flow back to
//!   the caller's active `CompletionFence`.
//! - `Instantiate`, `ProjectMember`, `IndexedAccess`, `Expand`, and the
//!   remaining variants return `QueryError::Miss` until Phase 4 cuts the
//!   legacy `_in_view` surface and lets us route them through the shared
//!   memo without carrying request-view identity.
//!
//! The memo itself is fully operational — warm hits, cross-thread joiners,
//! recursion sentinels, and completion-fence dep-signature merging all live
//! behind [`SemanticGraphStore::execute_cooperative`]. Wiring the remaining
//! variants to real solver work is a line-by-line migration of existing
//! `_in_view` helpers, tracked as Phase 4 follow-on work.
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
    CacheRead, DepSignature, DepVersion, QueryError, QueryResult, ResolveDeclKey, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
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

    /// Resolve a top-level declaration lookup via the host's shallow state.
    ///
    /// The current binding returns an `Alias` node pointing at an
    /// `Opaque(Miss)` placeholder for bindings that exist but have no
    /// semantic body yet — the body population lands in Phase 4 together
    /// with the `_in_view` signature cut. Absent bindings miss and never
    /// warm the shared memo.
    fn build_resolve_decl(
        &self,
        key: &ResolveDeclKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state_in_view(key.scope.canonical_id.as_ref(), None)
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

        // Phase 2.2 publishes a canonical placeholder node so the memo shape
        // and dep-signature propagation land now. Phase 4 swaps the body
        // carrier for the real resolved node once the legacy `_in_view`
        // surface goes away.
        let node_id = self
            .graph()
            .intern_node(SemanticNodeData::Alias(self.opaque(QueryError::Miss)));
        let signature = self.dep_signature_for(&key.scope.canonical_id, shallow.whole_hash);
        (QueryResult::Value(node_id), signature)
    }
}

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

impl<'a> SemanticQueryApi for ProjectSemanticDispatch<'a> {
    fn execute(&self, key: SemanticQueryKey) -> QueryResult<SemanticNodeId> {
        let graph = Arc::clone(self.graph());
        let sentinel = || self.recursion_sentinel();
        let key_for_build = key.clone();
        let build = move || match &key_for_build {
            SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key),
            // Phase 4 wires these through the legacy solver paths as they
            // lose their `_in_view` parameter. Until then return a
            // conservative Miss so the memo does not accidentally warm an
            // unimplemented dispatch path.
            SemanticQueryKey::Instantiate { .. }
            | SemanticQueryKey::ProjectMember { .. }
            | SemanticQueryKey::IndexedAccess { .. }
            | SemanticQueryKey::KeyOf { .. }
            | SemanticQueryKey::MappedType { .. }
            | SemanticQueryKey::Conditional { .. }
            | SemanticQueryKey::TypeOf { .. }
            | SemanticQueryKey::NormalizeUnion { .. }
            | SemanticQueryKey::NormalizeIntersection { .. }
            | SemanticQueryKey::Expand { .. } => {
                (QueryResult::Error(QueryError::Miss), empty_signature())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::PrimitiveKind;
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

    /// Phase 2.2 returns `Miss` for unwired variants; the memo must not
    /// warm them so Phase 4 can replace the dispatch arms without having
    /// to evict stale placeholder entries.
    #[test]
    fn unwired_variants_miss_without_warming_memo() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let base = host
            .project_type_store()
            .semantic_graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let before = host
            .project_type_store()
            .semantic_graph()
            .memo_entry_count();
        let result = dispatch.execute(SemanticQueryKey::Instantiate {
            base,
            args: Arc::from(Vec::new().into_boxed_slice()),
        });
        assert!(matches!(result, QueryResult::Error(QueryError::Miss)));
        let after = host
            .project_type_store()
            .semantic_graph()
            .memo_entry_count();
        assert_eq!(
            before, after,
            "unwired dispatch must not warm the shared memo"
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
}
