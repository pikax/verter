//! The node arena facade of the semantic graph store: interning and
//! reading semantic nodes by id. Storage and release live in `arena.rs`;
//! the retention counters over it in `observability.rs`.

use super::*;

impl SemanticGraphStore {
    /// Intern a new immutable [`SemanticNodeData`] and return its stable id.
    ///
    /// The interned node records [`NodeScopeId::Global`] in the origin
    /// sidecar (see [`Self::node_scope`]) — use
    /// [`Self::intern_node_with_scope`] when the node's origin scope is
    /// known (declaration anchors, instantiated shells, surface members
    /// whose value carries a declaration identity, etc.).
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.arena.push(data)
    }

    /// Intern `data` and record `scope` in the origin sidecar. Dispatch
    /// builders that know the node's declaration origin (e.g.
    /// `build_resolve_decl` / `build_typeof` / `build_instantiate`) use
    /// this entry point so per-base-scope routing via [`Self::node_scope`]
    /// returns the originating scope later.
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node_with_scope(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.arena.push_with_scope(data, scope)
    }

    /// Intern a rebuilt shell `data` while preserving the scope of
    /// an `origin` shell.
    ///
    /// **Invariant.** When a rebuilt SHELL `X'` is derived from `X`
    /// with substituted sub-expressions,
    /// `node_scope(X') == node_scope(X)`. Used by
    /// [`crate::project_semantic_dispatch::ProjectSemanticDispatch::substitute_semantic_type_param`]
    /// and any other shell-rebuild site that would otherwise call
    /// the scope-less `intern_node` and drop the origin scope under
    /// the compound `(payload, scope)` interning.
    ///
    /// **Deliberate exception — derived composites.** A substituted
    /// union (and a provably order-safe substituted intersection) is a
    /// DERIVED composite: it routes through the canonical authority,
    /// NOT through this helper, and a multi-arm canonical result
    /// interns under [`NodeScopeId::Global`] — a derived composite has
    /// no lexical scope, contributors retain their own scopes, and the
    /// file dependence rides the canonical evidence / observed
    /// self-roots rather than `NodeScopeId`. Overload-ordered
    /// (possibly-callable) substituted intersections still preserve
    /// scope through this helper. Copying the origin `File` scope onto
    /// a canonical composite would re-split canonical identity by
    /// scope, re-creating the cross-scope duplicate class the algebra
    /// exists to collapse.
    ///
    /// Falls back to [`NodeScopeId::Global`] when `origin`'s sidecar
    /// is empty (`origin` is out of bounds) — these cases are
    /// already scope-less.
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_preserving_scope(
        &self,
        origin: SemanticNodeId,
        data: SemanticNodeData,
    ) -> SemanticNodeId {
        self.stats
            .intern_preserving_scope_calls
            .fetch_add(1, Ordering::Relaxed);
        let scope = self.node_scope(origin).unwrap_or(NodeScopeId::Global);
        self.arena.push_with_scope(data, scope)
    }

    /// Test/diagnostic — read the cumulative count of
    /// `intern_preserving_scope` calls. Acts as the discriminating
    /// signal for the substitute change-tracking optimisation: a
    /// no-op substitution must not increment this counter at all,
    /// because identical sub-results short-circuit the rebuild +
    /// re-intern path entirely.
    #[must_use]
    pub fn intern_preserving_scope_call_count(&self) -> u64 {
        self.stats
            .intern_preserving_scope_calls
            .load(Ordering::Relaxed)
    }

    /// Return the recorded origin scope for `id`.
    ///
    /// Returns:
    /// - `None` — the id is out of bounds for the arena.
    /// - `Some(NodeScopeId::Global)` — scope-less structural node
    ///   (primitive, shared literal-union, helper intermediate).
    /// - `Some(NodeScopeId::File { .. })` — declaration-bound node whose
    ///   origin scope is the recorded `(canonical_id, whole_hash,
    ///   local_scope)` triple.
    ///
    /// The sidecar records the scope at the moment of **first intern**; a
    /// reader that calls `node_scope(id)` from a different scope observes
    /// the origin scope, not their own.
    #[must_use]
    pub fn node_scope(&self, id: SemanticNodeId) -> Option<NodeScopeId> {
        self.arena.scope(id)
    }

    /// Read the resolved payload for a semantic node id. Returns `None` if
    /// the id has not been interned. An id whose payload a document close
    /// released ([`Self::release_canonical`]) is still a known id: it reads
    /// as the shared `Opaque(Miss)` placeholder (see
    /// [`Self::node_is_live`]), so a stale holder sees an unresolved value
    /// rather than an invalid-id fault.
    #[must_use]
    pub fn node_data(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.arena.get(id)
    }
}
