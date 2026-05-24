//! `evaluate_deferred_semantic_node` — deferred-shell evaluation
//! fix-point loop ( Change Split + §2 guard contract row for
//! `evaluate_deferred_semantic_node`).
//!
//! Walks `SemanticNodeData` unwrapping `Alias(target)` hops,
//! substituting `Instantiate` shells, and projecting single-segment
//! `IndexedAccess` shells through dispatch re-entry. Returns the
//! caller's current node on cyclic re-entry (fix-point) per
//! Also hosts `normalized_index_key_node` which belongs to the
//! evaluation surface.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, IndexKey, LiteralValue, PathSegment, ProjectionMode, ProjectionReductionContext,
    QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
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

    pub(super) fn evaluate_deferred_semantic_node(&self, node: SemanticNodeId) -> SemanticNodeId {
        // Default to a `Published + Expanded` context. Publication
        // callers (the bounded reducer, mapper value substitution,
        // conditional check evaluation, builtin-utility argument
        // resolution) all need operator dispatches to terminate at
        // their fully-reduced surface. The codex-hybrid retires the
        // *implicit* Expanded unwrap by exposing
        // [`Self::evaluate_deferred_semantic_node_with_context`] so
        // structural-transit callers (relation engine identity-
        // carrier unwrap and object-vs-record arms) can opt out of
        // publication reduction explicitly.
        self.evaluate_deferred_semantic_node_with_context(
            node,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
    }

    /// Context-explicit variant of
    /// [`Self::evaluate_deferred_semantic_node`] (codex-hybrid,
    /// codex-hybrid). The caller supplies the
    /// [`ProjectionReductionContext`] that flows into every operator
    /// re-dispatch (`KeyOf`, `MappedType`, decl-placeholder
    /// `Instantiate`) so a `StructuralTransit` walk does not reify
    /// per-member edges along its evaluation path.
    pub(super) fn evaluate_deferred_semantic_node_with_context(
        &self,
        mut node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let mut visited = rustc_hash::FxHashSet::default();
        visited.insert(node);
        loop {
            let Some(data) = self.graph().node_data(node) else {
                return self.opaque(QueryError::Miss);
            };
            let next = match data.as_ref() {
                SemanticNodeData::Alias(target) => *target,
                SemanticNodeData::KeyOf { base } => {
                    let base =
                        self.evaluate_deferred_semantic_node_with_context(*base, reduction_context);
                    match self.execute(SemanticQueryKey::KeyOf {
                        base,
                        context: reduction_context,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    let object = self
                        .evaluate_deferred_semantic_node_with_context(*object, reduction_context);
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
                        context: reduction_context,
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
                            context: crate::semantic_query::ProjectionReductionContext::published(
                                ProjectionMode::Navigate,
                            ),
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
                SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                } => {
                    // When every expression resolves to a
                    // single string literal, fold the template into a
                    // `Literal::String` by concatenating
                    // `quasis[0] expr[0] quasis[1] expr[1] … quasis[n]`.
                    // This closes the `template_literal_as_key` mapped-
                    // type case where a mapper's `name_remap` carries
                    // a template-literal expression — the post-
                    // substitution `${K}` resolves to a string literal,
                    // and the surrounding template can be folded into
                    // a single name. When any expression resolves to a
                    // non-string-literal shape (Primitive, Union, an
                    // unresolved deferred shell), the template stays
                    // deferred — caller falls back to the iteration key.
                    let quasis = Arc::clone(quasis);
                    let expressions = Arc::clone(expressions);
                    drop(data);
                    let mut literals: Vec<Arc<str>> = Vec::with_capacity(expressions.len());
                    let mut all_literal = true;
                    for expr in expressions.iter() {
                        let resolved = self
                            .evaluate_deferred_semantic_node_with_context(*expr, reduction_context);
                        match self.graph().node_data(resolved).as_deref() {
                            Some(SemanticNodeData::Literal(LiteralValue::String(s))) => {
                                literals.push(Arc::from(s.as_str()));
                            }
                            _ => {
                                all_literal = false;
                                break;
                            }
                        }
                    }
                    if !all_literal {
                        return node;
                    }
                    let mut buf = String::new();
                    for (idx, quasi) in quasis.iter().enumerate() {
                        buf.push_str(quasi);
                        if let Some(lit) = literals.get(idx) {
                            buf.push_str(lit);
                        }
                    }
                    self.graph()
                        .intern_node(SemanticNodeData::Literal(LiteralValue::String(buf)))
                }
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
                    // Codex-hybrid spec: the
                    // declaration-placeholder unwrap inherits the
                    // caller's reduction context. The implicit
                    // `Published + Expanded` unwrap was the path that
                    // re-opened nested `keyof` / `Mapped` reification
                    // during relation-engine binding; the caller's
                    // `StructuralTransit` context now carries through.
                    match self.execute(SemanticQueryKey::Instantiate {
                        base: identity,
                        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        context: reduction_context,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    }
                }
                // `DeclRef`/`InstantiationRef`
                // arms are deliberately NOT added to the deferred-
                // shell evaluator. The path-walker (walk.rs) and the
                // keyspace enumerator (enumerate.rs) handle these
                // carriers under explicit demand contexts, but the
                // deferred-shell evaluator is called from intermediate
                // IndexedAccess hops where eagerly resolving a
                // `DeclRef` would over-evaluate symbolic forms that
                // the slot-binding indexed-access preservation policy
                // expects to stay carrier-shaped (e.g.
                // `AppProps['avatar']` must stay
                // `IndexedAccess { object: DeclRef(AppProps), index }`).
                // The brief's conditional "if they are on the
                // macro-shape enumeration path" scopes this symmetry
                // to the enumeration path only.
                _ => return node,
            };
            if next == node {
                return node;
            }
            // Cyclic re-entry detected (self-referential evaluation) — return
            // current node as fix-point per guard contract row.
            if !visited.insert(next) {
                return node;
            }
            node = next;
        }
    }
}
