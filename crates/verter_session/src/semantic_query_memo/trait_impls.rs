//! Trait adapters owned by the semantic graph store.

use std::sync::Arc;

use crate::semantic_query::{QueryError, SemanticGraphRead, SemanticNodeData, SemanticNodeId};

use super::SemanticGraphStore;

impl SemanticGraphRead for SemanticGraphStore {
    fn node_data(&self, node: SemanticNodeId) -> Arc<SemanticNodeData> {
        SemanticGraphStore::node_data(self, node).unwrap_or_else(|| {
            // Missing node id — fabricate an Opaque sentinel rather than
            // panicking. Ids are only handed out by `intern_node`, so this is
            // defensive; in debug builds the arena invariant is expected to
            // be consistent.
            Arc::new(SemanticNodeData::Opaque(QueryError::Miss))
        })
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for SemanticGraphStore {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, TypeGraph, ResolverState, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            // `invalidate_all` clears every `SemanticNodeId`-keyed structure
            // so no stale judgement survives the project-generation bump.
            // The node arena is append-only and is not reset.
            let _ = self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for SemanticGraphStore {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        self.invalidate_canonical(canonical_id)
    }
}
