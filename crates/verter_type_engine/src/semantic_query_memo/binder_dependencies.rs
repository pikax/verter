//! Argument-independent summaries of immutable semantic-node dependencies.
//! Entries belong to the graph epoch, retain their roots, and share its budget.

use std::sync::Arc;

use dashmap::mapref::entry::Entry;
use rustc_hash::{FxHashMap, FxHashSet};

use super::SemanticGraphStore;
use crate::semantic_query::{InferBinderId, SemanticNodeData, SemanticNodeId};

#[derive(Default)]
pub(crate) struct BinderDependencies {
    nodes: FxHashSet<SemanticNodeId>,
    infer: FxHashSet<InferBinderId>,
    has_declaration: bool,
    has_infer_declaration: bool,
    complete: bool,
}

impl BinderDependencies {
    pub(crate) fn proves_absent(&self, target: &SemanticNodeData, id: SemanticNodeId) -> bool {
        if !self.complete {
            return false;
        }
        match target {
            SemanticNodeData::TypeParam { .. } => !self.nodes.contains(&id),
            SemanticNodeData::Infer { binder, .. } => !self.infer.contains(binder),
            _ => false,
        }
    }

    pub(crate) fn may_declare_infer(&self) -> bool {
        self.has_infer_declaration
    }

    pub(crate) fn may_mention_binder(&self) -> bool {
        !self.complete || self.has_declaration
    }
}

pub(crate) struct BinderDependencyEntry {
    summary: Arc<BinderDependencies>,
    _roots: super::node_roots::NodeRootSet,
    _retention: verter_session_query::retention::RetentionCharge,
}

impl SemanticGraphStore {
    fn binder_dependencies_cached(&self, node: SemanticNodeId) -> Option<Arc<BinderDependencies>> {
        self.binder_dependencies
            .get(&node)
            .map(|entry| Arc::clone(&entry.summary))
    }

    fn publish_binder_dependencies(&self, node: SemanticNodeId, summary: Arc<BinderDependencies>) {
        if !summary.complete {
            return;
        }
        let bytes = std::mem::size_of::<BinderDependencyEntry>()
            + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
            + summary.nodes.capacity() * std::mem::size_of::<SemanticNodeId>()
            + summary.infer.capacity() * std::mem::size_of::<InferBinderId>();
        let charge = match self.retention_account().reserve(
            verter_session_query::retention::ChargeClass::Retained,
            bytes,
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => charge,
            verter_session_query::retention::RetentionAdmission::Refused(_) => return,
        };
        let Ok(roots) = self.root_nodes([node]) else {
            return;
        };
        if let Entry::Vacant(slot) = self.binder_dependencies.entry(node) {
            slot.insert(BinderDependencyEntry {
                summary,
                _roots: roots,
                _retention: charge,
            });
            let mut fifo = self.binder_dependencies_fifo.lock();
            fifo.push_back(node);
            while fifo.len() > super::hash_cons_memos::HASH_CONS_MEMO_RETENTION_CAP {
                if let Some(victim) = fifo.pop_front() {
                    drop(fifo);
                    self.binder_dependencies.remove(&victim);
                    fifo = self.binder_dependencies_fifo.lock();
                }
            }
        }
    }

    pub(crate) fn binder_dependencies_for(&self, root: SemanticNodeId) -> Arc<BinderDependencies> {
        if let Some(summary) = self.binder_dependencies_cached(root) {
            return summary;
        }
        let mut done: FxHashMap<SemanticNodeId, Arc<BinderDependencies>> = FxHashMap::default();
        let mut active = FxHashSet::default();
        let mut work = vec![(root, false)];
        while let Some((node, finish)) = work.pop() {
            if done.contains_key(&node) {
                continue;
            }
            if let Some(summary) = self.binder_dependencies_cached(node) {
                done.insert(node, summary);
                continue;
            }
            let Some(data) = self.node_data(node) else {
                done.insert(node, Arc::new(BinderDependencies::default()));
                continue;
            };
            let mut children = Vec::new();
            let children_complete = matches!(
                data.for_each_child(|child| children.push(child)),
                crate::semantic_query::ChildWalk::Enumerated
            );
            if !finish {
                if !active.insert(node) {
                    done.insert(node, Arc::new(BinderDependencies::default()));
                    continue;
                }
                work.push((node, true));
                work.extend(children.into_iter().rev().map(|child| (child, false)));
                continue;
            }
            active.remove(&node);
            let mut summary = BinderDependencies {
                complete: children_complete,
                ..Default::default()
            };
            match data.as_ref() {
                SemanticNodeData::TypeParam { .. } => {
                    summary.nodes.insert(node);
                    summary.has_declaration = true;
                }
                SemanticNodeData::Infer { binder, .. } => {
                    summary.infer.insert(binder.clone());
                    summary.has_declaration = true;
                    summary.has_infer_declaration = true;
                }
                SemanticNodeData::InferRef { binder, .. } => {
                    summary.infer.insert(binder.clone());
                }
                _ => {}
            }
            for child in children {
                if let Some(child) = done.get(&child) {
                    summary.complete &= child.complete;
                    summary.has_declaration |= child.has_declaration;
                    // A nested conditional or signature owns its infer binders;
                    // they do not declare a binder in an enclosing pattern.
                    if !matches!(
                        data.as_ref(),
                        SemanticNodeData::Conditional { .. }
                            | SemanticNodeData::Signature { .. }
                            | SemanticNodeData::DeferredCallable(_)
                    ) {
                        summary.has_infer_declaration |= child.has_infer_declaration;
                    }
                    summary.nodes.extend(child.nodes.iter().copied());
                    summary.infer.extend(child.infer.iter().cloned());
                } else {
                    summary.complete = false;
                }
            }
            let summary = Arc::new(summary);
            self.publish_binder_dependencies(node, Arc::clone(&summary));
            done.insert(node, summary);
        }
        done.remove(&root).unwrap_or_default()
    }
}
