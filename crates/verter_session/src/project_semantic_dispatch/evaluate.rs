//! `evaluate_deferred_semantic_node` — deferred-shell evaluation
//! fix-point loop (plan §3 Change Split + §2 guard contract row for
//! `evaluate_deferred_semantic_node`).
//!
//! Walks `SemanticNodeData` unwrapping `Alias(target)` hops,
//! substituting `Instantiate` shells, and projecting single-segment
//! `IndexedAccess` shells through dispatch re-entry. Returns the
//! caller's current node on cyclic re-entry (fix-point) per plan §2.
//! Also hosts `normalized_index_key_node` which belongs to the
//! evaluation surface.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, IndexKey, LiteralValue, PathSegment, ProjectionMode, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
};

impl<'a> ProjectSemanticDispatch<'a> {
    pub(super) fn normalized_index_key_node(&self, node: SemanticNodeId) -> IndexKey {
        let resolved = self.evaluate_deferred_semantic_node(node);
        match self.graph().node_data(resolved).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                IndexKey::String(Arc::from(text.as_str()))
            }
            Some(SemanticNodeData::Literal(LiteralValue::Number(number)))
                if number.fract() == 0.0
                    && *number >= i64::MIN as f64
                    && *number <= i64::MAX as f64 =>
            {
                IndexKey::Number(*number as i64)
            }
            Some(SemanticNodeData::Alias(target)) => self.normalized_index_key_node(*target),
            _ => IndexKey::TypeNode(resolved),
        }
    }

    pub(super) fn evaluate_deferred_semantic_node(
        &self,
        mut node: SemanticNodeId,
    ) -> SemanticNodeId {
        // Phase D §5.3 WIP-R: the former `for _ in 0..32` hard cap is retired.
        // Cycle detection uses a stack-local visited set (per plan §2 guard
        // contract); the loop converges on graph fix-points in at most
        // graph-size steps.
        let mut visited = rustc_hash::FxHashSet::default();
        visited.insert(node);
        loop {
            let Some(data) = self.graph().node_data(node) else {
                return self.opaque(QueryError::Miss);
            };
            let next = match data.as_ref() {
                SemanticNodeData::Alias(target) => *target,
                SemanticNodeData::KeyOf { base } => {
                    let base = self.evaluate_deferred_semantic_node(*base);
                    match self.execute(SemanticQueryKey::KeyOf { base }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    let object = self.evaluate_deferred_semantic_node(*object);
                    let index = match index {
                        IndexKey::String(text) => IndexKey::String(Arc::clone(text)),
                        IndexKey::Number(number) => IndexKey::Number(*number),
                        IndexKey::TypeNode(node) => self.normalized_index_key_node(*node),
                    };
                    match self.execute(SemanticQueryKey::IndexedAccess {
                        base: object,
                        index,
                        mode: ProjectionMode::Navigate,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    match self.execute(SemanticQueryKey::MappedType {
                        source: *source,
                        mapper: mapper.clone(),
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::TypeOf { value_root, path } => {
                    let root = match self.execute(SemanticQueryKey::TypeOf {
                        value_root: value_root.clone(),
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    };
                    if path.is_empty() {
                        root
                    } else {
                        let projection_path: Arc<[PathSegment]> = Arc::from(
                            path.iter()
                                .map(|segment| PathSegment::Member(Arc::clone(segment)))
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        );
                        match self.execute(SemanticQueryKey::ProjectPath {
                            base: root,
                            path: projection_path,
                            mode: ProjectionMode::Navigate,
                        }) {
                            QueryResult::Value(id) => id,
                            _ => return self.opaque(QueryError::Miss),
                        }
                    }
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    distributive,
                } => match self.execute(SemanticQueryKey::Conditional {
                    check: *check,
                    extends: *extends,
                    true_branch: *true_branch_ref,
                    false_branch: *false_branch_ref,
                    distributive: *distributive,
                }) {
                    QueryResult::Value(id) => id,
                    _ => return self.opaque(QueryError::Miss),
                },
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    name,
                    whole_hash,
                }) => {
                    let identity = DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        whole_hash: *whole_hash,
                        decl_name: Arc::clone(name),
                    };
                    drop(data);
                    match self.execute(SemanticQueryKey::Instantiate {
                        base: identity,
                        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        // Typeof unwraps a deferred declaration body so
                        // the fix-point loop can keep walking into it
                        // (mapper applications, conditionals, object
                        // surfaces). Expanded is required: with Navigate,
                        // the next iteration would observe the lazy Ref
                        // shell and stop short of the value's real type.
                        body_mode: crate::semantic_query::ProjectionMode::Expanded,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    }
                }
                _ => return node,
            };
            if next == node {
                return node;
            }
            // Cyclic re-entry detected (self-referential evaluation) — return
            // current node as fix-point per plan §2 guard contract row.
            if !visited.insert(next) {
                return node;
            }
            node = next;
        }
    }
}
