//! The node arena facade of the semantic graph store: interning and
//! reading semantic nodes by id. Storage and release live in `arena.rs`;
//! the retention counters over it in `observability.rs`.

use super::*;

impl SemanticGraphStore {
    /// Retain a multiset of live operands from this arena. Production close
    /// may still revoke their slots under the legacy activity gate.
    ///
    /// A bare ordinal is construction identity: outside the engine a producer
    /// roots it through the construction scope it holds
    /// ([`node_roots::NodeRootScope::root_nodes`]). Test builds expose this
    /// as a fixture seam.
    #[cfg(any(test, feature = "test-support"))]
    pub fn root_nodes(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Result<node_roots::NodeRootSet, QueryError> {
        node_roots::NodeRootSet::acquire(Arc::clone(&self.arena.core), ids.into_iter().collect())
    }

    /// Construction-lane operand rooting; see the test-build variant.
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn root_nodes(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Result<node_roots::NodeRootSet, QueryError> {
        node_roots::NodeRootSet::acquire(Arc::clone(&self.arena.core), ids.into_iter().collect())
    }

    pub(crate) fn root_existing_nodes(
        &self,
        mut ids: Vec<SemanticNodeId>,
    ) -> Result<node_roots::NodeRootSet, QueryError> {
        // A legacy raw carrier may already name a tombstone (or a synthetic
        // test ordinal). Such an id owns no storage. Counted delivery, where
        // all carriers participate in ownership, rejects it instead.
        if !self.arena.counted() {
            ids.retain(|node| self.node_is_live(*node));
        }
        self.root_nodes(ids)
    }

    /// Open a construction scope even when no observation capture is active.
    ///
    /// Holding the graph grants no construction lane: a producer outside the
    /// engine opens a scope through its [`node_roots::NodeConstructionGrant`].
    /// Test builds expose this as a fixture seam.
    #[cfg(any(test, feature = "test-support"))]
    pub fn enter_node_root_scope(&self) -> node_roots::NodeRootScope {
        node_roots::NodeRootScope::enter(&self.arena.core)
    }

    /// Engine-internal construction scope; see the test-build variant.
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn enter_node_root_scope(&self) -> node_roots::NodeRootScope {
        node_roots::NodeRootScope::enter(&self.arena.core)
    }

    /// Open a nested construction scope inside a construction lane this
    /// thread already holds of this graph, so a reader bounds its own
    /// intermediates. It grants nothing the held lane does not: without one
    /// there is no scope.
    #[must_use]
    pub fn enter_nested_scope(&self) -> Option<node_roots::NodeRootScope> {
        node_roots::NodeRootScope::holds(&self.arena.core)
            .then(|| node_roots::NodeRootScope::enter(&self.arena.core))
    }

    /// A construction grant for a fixture graph built without an engine.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn construction_grant_for_tests(&self) -> node_roots::NodeConstructionGrant {
        node_roots::NodeConstructionGrant::mint(self)
    }

    /// Intern a node whose retained operands are covered by arena-qualified
    /// roots. A foreign scope or an uncovered operand is a typed refusal.
    pub fn intern_rooted_node(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
        operands: &node_roots::NodeRootSet,
    ) -> Result<node_roots::NodeRoot, QueryError> {
        if !operands.belongs_to(&self.arena.core) {
            return Err(QueryError::ForeignSemanticOperand);
        }
        let mut covered = true;
        data.for_each_retained_child(|child| {
            covered &= child.0 >= UNALLOCATABLE_ID_FLOOR || operands.operands().contains(&child);
        });
        if !covered {
            return Err(QueryError::ForeignSemanticOperand);
        }
        self.arena.try_intern_owned(data, scope)
    }

    /// Read a node through an arena-qualified capability. A capability
    /// minted by another graph is foreign; a slot a legacy close already
    /// released is stale. Neither reads as a live payload.
    pub fn read(
        &self,
        node: node_roots::NodeRef<'_>,
    ) -> Result<node_data_lease::NodeDataLease, QueryError> {
        if !Arc::ptr_eq(node.arena(), &self.arena.core) {
            return Err(QueryError::ForeignSemanticOperand);
        }
        node.read()
    }

    /// Read a construction ordinal while this thread holds a construction
    /// scope of this graph — the producer lane a semantic builder runs in.
    /// The scope is the capability: outside one the ordinal authorizes
    /// nothing and the read is `None`. A live node is returned under its own
    /// root, so the lease outlives the scope. A slot a legacy close already
    /// released reads as the unresolved placeholder with no root, exactly as
    /// the production close lifecycle reports it, and none of its children
    /// can be borrowed through it.
    pub fn read_in_scope(&self, id: SemanticNodeId) -> Option<node_data_lease::NodeDataLease> {
        if !node_roots::NodeRootScope::holds(&self.arena.core) {
            return None;
        }
        match node_roots::NodeRootSet::acquire(Arc::clone(&self.arena.core), vec![id]) {
            Ok(roots) => Some(node_data_lease::NodeDataLease {
                data: self.arena.get(id)?,
                root: node_roots::NodeRoot::from_roots(id, roots),
            }),
            Err(QueryError::StaleSemanticOperand) => Some(node_data_lease::NodeDataLease {
                data: self.arena.get(id)?,
                root: node_roots::NodeRoot::from_roots(id, Default::default()),
            }),
            Err(_) => None,
        }
    }

    /// Root a live construction ordinal for delivery beyond the construction
    /// scope this thread holds of this graph. Without such a scope the
    /// ordinal is foreign; a slot a legacy close already released is stale.
    /// Neither is ever delivered as an absent node.
    pub fn root_in_scope(&self, id: SemanticNodeId) -> Result<node_roots::NodeRoot, QueryError> {
        if !node_roots::NodeRootScope::holds(&self.arena.core) {
            return Err(QueryError::ForeignSemanticOperand);
        }
        node_roots::NodeRef::new(id, &self.arena.core).to_root()
    }

    /// Lease an engine-internal ordinal for delivery to a consumer outside
    /// the engine. A released slot has nothing to deliver.
    pub(crate) fn lease_for_delivery(
        &self,
        id: SemanticNodeId,
    ) -> Option<node_data_lease::NodeDataLease> {
        node_roots::NodeRef::new(id, &self.arena.core).read().ok()
    }

    /// Intern a new immutable [`SemanticNodeData`] and return its stable id.
    ///
    /// The interned node records [`NodeScopeId::Global`] in the origin
    /// sidecar (see [`Self::node_scope`]) — use
    /// [`Self::intern_node_with_scope`] when the node's origin scope is
    /// known (declaration anchors, instantiated shells, surface members
    /// whose value carries a declaration identity, etc.).
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.arena
            .push(crate::semantic_query::DeferredCallable::seal_signature_header(data))
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
        self.arena.push_with_scope(
            crate::semantic_query::DeferredCallable::seal_signature_header(data),
            scope,
        )
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
        self.arena.push_with_scope(
            crate::semantic_query::DeferredCallable::seal_signature_header(data),
            scope,
        )
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

    /// Read the resolved payload for a construction ordinal. Returns `None`
    /// if the id has not been interned. An id whose payload a document close
    /// released ([`Self::release_canonical`]) is still a known id: it reads
    /// as the shared `Opaque(Miss)` placeholder (see
    /// [`Self::node_is_live`]), so a stale holder sees an unresolved value
    /// rather than an invalid-id fault.
    ///
    /// A bare ordinal is internal construction identity: outside this crate
    /// a production read goes through [`Self::read`] with a capability its
    /// owner minted. Test builds expose the ordinal read as a fixture seam.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn node_data(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.arena.get(id)
    }

    /// Construction-ordinal read; see the test-build variant.
    #[cfg(not(any(test, feature = "test-support")))]
    #[must_use]
    pub(crate) fn node_data(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.arena.get(id)
    }
}

/// Where a chain of transparent `Alias` redirects from a node ends
/// ([`SemanticGraphStore::alias_chain_end`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasChainEnd {
    /// The first node on the chain that is not an `Alias` — the start
    /// itself when it is not one.
    Node(SemanticNodeId),
    /// A redirect to an id with no data.
    Dangling(SemanticNodeId),
    /// The chain returns to a node it passed.
    Cycle,
}

impl SemanticGraphStore {
    /// Follow `start`'s transparent `Alias` redirects to where they end,
    /// however long the chain is; `visit` sees each node on it, the end
    /// included. A node that is not an alias costs one read and no
    /// allocation.
    pub fn alias_chain_end(
        &self,
        start: SemanticNodeId,
        mut visit: impl FnMut(SemanticNodeId),
    ) -> AliasChainEnd {
        let mut current = start;
        let mut passed: Option<rustc_hash::FxHashSet<SemanticNodeId>> = None;
        loop {
            visit(current);
            let Some(data) = self.node_data(current) else {
                return AliasChainEnd::Dangling(current);
            };
            let SemanticNodeData::Alias(next) = &*data else {
                return AliasChainEnd::Node(current);
            };
            let next = *next;
            if !passed.get_or_insert_with(Default::default).insert(current) {
                return AliasChainEnd::Cycle;
            }
            current = next;
        }
    }
}
