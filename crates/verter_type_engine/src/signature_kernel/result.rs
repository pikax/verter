//! Demand-driven signature results: the closed projection vocabulary and the
//! full memo identity of one result read.

use crate::semantic_query::{ResultEvaluationContextId, SemanticContextId};

use std::sync::Arc;

use crate::semantic_query::SemanticNodeId;

use super::records::{AppliedResultId, CallSubstitutionId, SignatureDescriptorId, SignatureSetRef};

/// Which half of a signature's result a read demands. A closed vocabulary,
/// not a license for a private body analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultDemand {
    Return,
    Effects,
    Both,
}

impl ResultDemand {
    #[must_use]
    pub const fn reads_return(self) -> bool {
        matches!(self, Self::Return | Self::Both)
    }

    #[must_use]
    pub const fn reads_effects(self) -> bool {
        matches!(self, Self::Effects | Self::Both)
    }
}

/// The complete demand identity of one `ReadSignatureResult`: the
/// descriptor, the frozen call substitution over its residual binders, the
/// projection, the body/return evaluation context, and the semantic
/// context. No content version or edit counter is part of it — freshness is
/// dependency evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadSignatureResultKey {
    pub descriptor: SignatureDescriptorId,
    pub call_substitution: CallSubstitutionId,
    pub projection: ResultDemand,
    pub evaluation: ResultEvaluationContextId,
    pub semantic_context: SemanticContextId,
}

/// Value of a `SignaturesOfType` query.
///
/// Every copy owns the graph nodes its candidates were published from, so a
/// cloned value stays readable through its final holder. The candidate
/// table is borrowed from the value, never handed out as a detached array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureSetValue {
    /// The immutable tables that produced this value; transport ownership
    /// does not participate in semantic equality.
    pub(crate) version: SignatureVersion,
    pub(crate) set: SignatureSetRef,
    /// Per candidate, in set order: the graph nodes THIS read's subject walk
    /// published it from, with the roots that own them. A descriptor is
    /// content-interned, so one descriptor outlives the file version that
    /// first published it; the node a consumer may read is the one this
    /// subject carries, which only the walk knows.
    pub(crate) nodes: verter_session_query::retention::owned::Owned<CandidateTable>,
}

/// A value's candidate nodes and the roots every copy of the value shares.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CandidateTable {
    candidates: Box<[SignatureCandidateNodes]>,
    /// The candidate nodes' roots; transport, never semantic identity.
    roots: crate::semantic_query_memo::node_roots::IdentityNodeRoots,
}

impl std::ops::Deref for CandidateTable {
    type Target = [SignatureCandidateNodes];
    fn deref(&self) -> &Self::Target {
        &self.candidates
    }
}

impl crate::semantic_query::allocation_footprint::OwnedHeap for SignatureSetValue {
    /// The shared candidate table, an owned allocation charged by its own
    /// cell. The signature records live in the epoch's tables.
    fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        self.nodes.offer(aliases)
    }
}

impl verter_session_query::retention::owned::Measured for CandidateTable {
    /// The candidate slice, each composite's constituent list and the
    /// table's root owner.
    fn heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use crate::semantic_query::allocation_footprint::OwnedHeap as _;
        use verter_session_query::retention::allocation::heap_block_bytes;
        heap_block_bytes(std::mem::size_of_val::<[SignatureCandidateNodes]>(
            &self.candidates,
        )) + self
            .candidates
            .iter()
            .map(|candidate| candidate.constituents.owned_heap_bytes(aliases))
            .sum::<usize>()
            + self.roots.0.owned_heap_bytes(aliases)
    }
}

impl SignatureSetValue {
    /// Mint a value owning every candidate node in `graph`, the graph whose
    /// walk published them — never whichever graph's scope is innermost on
    /// the thread, which may belong to another engine.
    pub(crate) fn new(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        version: SignatureVersion,
        set: SignatureSetRef,
        nodes: Vec<SignatureCandidateNodes>,
    ) -> Self {
        let roots = crate::semantic_query_memo::node_roots::IdentityNodeRoots::in_graph(
            graph,
            nodes.iter().flat_map(SignatureCandidateNodes::node_ids),
        );
        Self {
            version,
            set,
            nodes: verter_session_query::retention::owned::Owned::new(CandidateTable {
                candidates: nodes.into_boxed_slice(),
                roots,
            }),
        }
    }

    /// Bytes the candidate table's own charge holds, and its class; `None`
    /// while no owner has charged it.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn candidate_table_charge_for_tests(
        &self,
    ) -> Option<(usize, verter_session_query::retention::ChargeClass)> {
        self.nodes.charge_bytes().zip(self.nodes.charge_class())
    }

    /// The interned candidate set.
    #[must_use]
    pub fn set(&self) -> SignatureSetRef {
        self.set
    }

    /// The signature tables this value was produced from.
    #[must_use]
    pub fn version(&self) -> &SignatureVersion {
        &self.version
    }

    /// The candidates' publishing nodes, borrowed from this value.
    #[must_use]
    pub fn candidate_nodes(&self) -> &[SignatureCandidateNodes] {
        &self.nodes
    }

    /// Borrow one candidate node this value owns.
    pub fn node(
        &self,
        id: SemanticNodeId,
    ) -> Result<
        crate::semantic_query_memo::node_roots::NodeRef<'_>,
        crate::semantic_query::QueryError,
    > {
        self.nodes.roots.0.node_ref(id)
    }
}

/// The graph nodes one candidate of a [`SignatureSetValue`] was published
/// from. Read only through the value that owns them.
#[derive(Debug, PartialEq, Eq)]
pub struct SignatureCandidateNodes {
    /// The authored signature node of a leaf; `None` for a composite (it
    /// has constituents, not a node of its own).
    pub(crate) authored: Option<SemanticNodeId>,
    /// For a composite, the authored node of each constituent in
    /// constituent-sequence order; empty for a leaf, and empty when the walk
    /// never published one of them.
    pub(crate) constituents: verter_type_expr::OwnedSlice<SemanticNodeId>,
}

impl SignatureCandidateNodes {
    /// The authored signature node of a leaf candidate.
    #[must_use]
    pub fn authored(&self) -> Option<SemanticNodeId> {
        self.authored
    }

    /// A composite's constituent nodes, borrowed.
    #[must_use]
    pub fn constituents(&self) -> &[SemanticNodeId] {
        &self.constituents
    }

    fn node_ids(&self) -> impl Iterator<Item = SemanticNodeId> + '_ {
        self.authored
            .into_iter()
            .chain(self.constituents.iter().copied())
    }
}

/// Value of a `ReadSignatureResult` query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureResultValue {
    pub version: SignatureVersion,
    pub result: AppliedResultId,
}

#[derive(Debug, Clone, Default)]
pub struct SignatureVersion(pub(crate) Option<super::lifetime::SignatureEpochLease>);
impl PartialEq for SignatureVersion {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for SignatureVersion {}
