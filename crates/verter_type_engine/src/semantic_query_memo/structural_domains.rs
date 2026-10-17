//! Pure key-domain results for immutable syntax. Live declaration resolution
//! remains with the ordinary fact-backed query owners.
use std::sync::Arc;

use dashmap::mapref::entry::Entry;

use super::SemanticGraphStore;
use crate::project_semantic_dispatch::enumerate::KeySpaceDomain;
use crate::semantic_query::SemanticNodeId;

pub(crate) struct StructuralDomainEntry {
    domain: Option<Arc<KeySpaceDomain>>,
    _roots: super::node_roots::NodeRootSet,
    _retention: verter_session_query::retention::RetentionCharge,
}

impl SemanticGraphStore {
    pub(crate) fn structural_domain_get(
        &self,
        node: SemanticNodeId,
    ) -> Option<Option<Arc<KeySpaceDomain>>> {
        self.structural_domains
            .get(&node)
            .map(|entry| entry.domain.clone())
    }

    pub(crate) fn structural_domain_publish(
        &self,
        node: SemanticNodeId,
        domain: Option<Arc<KeySpaceDomain>>,
    ) {
        let bytes = std::mem::size_of::<StructuralDomainEntry>()
            + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
            + domain.as_ref().map_or(0, |domain| domain.retained_bytes());
        let charge = match self.retention_account().reserve(
            verter_session_query::retention::ChargeClass::Retained,
            bytes,
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => charge,
            verter_session_query::retention::RetentionAdmission::Refused(_) => return,
        };
        let Ok(roots) = self.root_nodes(vec![node]) else {
            return;
        };
        if let Entry::Vacant(slot) = self.structural_domains.entry(node) {
            slot.insert(StructuralDomainEntry {
                domain,
                _roots: roots,
                _retention: charge,
            });
            let mut fifo = self.structural_domains_fifo.lock();
            fifo.push_back(node);
            while fifo.len() > super::hash_cons_memos::HASH_CONS_MEMO_RETENTION_CAP {
                if let Some(victim) = fifo.pop_front() {
                    drop(fifo);
                    self.structural_domains.remove(&victim);
                    fifo = self.structural_domains_fifo.lock();
                }
            }
        }
    }
}
