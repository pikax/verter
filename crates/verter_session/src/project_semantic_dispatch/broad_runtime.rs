use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::intrinsic_registry::RuntimeNominal;
use crate::semantic_query::{
    BroadRuntimeClassification, BroadRuntimeKind, LiteralValue, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RuntimeWork {
    node: SemanticNodeId,
    filter_unknown: bool,
}

impl ProjectSemanticDispatch<'_> {
    pub(super) fn build_classify_broad_runtime(
        &self,
        subject: SemanticNodeId,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let mut work = vec![RuntimeWork {
            node: subject,
            filter_unknown: false,
        }];
        let mut visited = FxHashSet::default();
        let mut observed_nodes = Vec::new();
        let mut kinds = Vec::new();
        let transit =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

        while let Some(item) = work.pop() {
            if !visited.insert(item) {
                continue;
            }
            observed_nodes.push(item.node);
            let Some(data) = self.graph().node_data(item.node) else {
                if !item.filter_unknown {
                    kinds.push(BroadRuntimeKind::Unknown);
                }
                continue;
            };

            let push_unknown = |kinds: &mut Vec<BroadRuntimeKind>| {
                if !item.filter_unknown {
                    kinds.push(BroadRuntimeKind::Unknown);
                }
            };

            match data.as_ref() {
                SemanticNodeData::Alias(inner) => work.push(RuntimeWork {
                    node: *inner,
                    filter_unknown: item.filter_unknown,
                }),
                SemanticNodeData::Union(arms) => {
                    for arm in arms.iter().rev() {
                        work.push(RuntimeWork {
                            node: *arm,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::Intersection(arms) => {
                    for arm in arms.iter().rev() {
                        work.push(RuntimeWork {
                            node: *arm,
                            filter_unknown: true,
                        });
                    }
                }
                SemanticNodeData::Primitive(kind) => match kind {
                    PrimitiveKind::String => kinds.push(BroadRuntimeKind::String),
                    PrimitiveKind::Number => kinds.push(BroadRuntimeKind::Number),
                    PrimitiveKind::Boolean => kinds.push(BroadRuntimeKind::Boolean),
                    PrimitiveKind::Symbol => kinds.push(BroadRuntimeKind::Symbol),
                    PrimitiveKind::Null => kinds.push(BroadRuntimeKind::Null),
                    PrimitiveKind::Undefined => push_unknown(&mut kinds),
                    PrimitiveKind::Object => kinds.push(BroadRuntimeKind::Object),
                    PrimitiveKind::BigInt
                    | PrimitiveKind::Any
                    | PrimitiveKind::Unknown
                    | PrimitiveKind::Void
                    | PrimitiveKind::Never => push_unknown(&mut kinds),
                },
                SemanticNodeData::Literal(literal) => match literal {
                    LiteralValue::String(_) => kinds.push(BroadRuntimeKind::String),
                    LiteralValue::Number(_) => kinds.push(BroadRuntimeKind::Number),
                    LiteralValue::Boolean(_) => kinds.push(BroadRuntimeKind::Boolean),
                    LiteralValue::BigInt(_) => kinds.push(BroadRuntimeKind::Number),
                },
                SemanticNodeData::TemplateLiteral { .. } => kinds.push(BroadRuntimeKind::String),
                SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. } => {
                    kinds.push(BroadRuntimeKind::Array)
                }
                SemanticNodeData::Function { .. } | SemanticNodeData::ConstructorType { .. } => {
                    kinds.push(BroadRuntimeKind::Function)
                }
                SemanticNodeData::Object(surface) => {
                    let callable = !surface.call_signatures.is_empty()
                        || !surface.construct_signatures.is_empty();
                    if callable {
                        kinds.push(BroadRuntimeKind::Function);
                    }
                    if !callable
                        || !surface.members.is_empty()
                        || !surface.index_signatures.is_empty()
                        || surface.keyspace.is_some()
                        || surface.has_index_signature
                    {
                        kinds.push(BroadRuntimeKind::Object);
                    }
                }
                SemanticNodeData::MergedDecl { .. } => kinds.push(BroadRuntimeKind::Object),
                SemanticNodeData::DeclRef { identity } => {
                    if let Some(kind) = self.runtime_nominal_identity(identity) {
                        kinds.push(broad_kind_for_nominal(kind));
                        continue;
                    }
                    self.push_instantiated_runtime_subject(
                        identity,
                        Arc::from([]),
                        item,
                        transit,
                        &mut work,
                        &mut kinds,
                    );
                }
                SemanticNodeData::InstantiationRef { base, args } => {
                    if let Some(kind) = self.runtime_nominal_identity(base) {
                        kinds.push(broad_kind_for_nominal(kind));
                        continue;
                    }
                    self.push_instantiated_runtime_subject(
                        base,
                        Arc::clone(args),
                        item,
                        transit,
                        &mut work,
                        &mut kinds,
                    );
                }
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    ..
                }) => {
                    let identity = crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        whole_hash: Default::default(),
                        decl_name: Arc::clone(name),
                    };
                    self.push_instantiated_runtime_subject(
                        &identity,
                        Arc::from([]),
                        item,
                        transit,
                        &mut work,
                        &mut kinds,
                    );
                }
                SemanticNodeData::BareRef(_)
                | SemanticNodeData::ImportType(_)
                | SemanticNodeData::TypeOf(_) => {
                    let resolved = self.resolve_carrier_subject_node(item.node, transit);
                    if resolved == item.node {
                        push_unknown(&mut kinds);
                    } else {
                        work.push(RuntimeWork {
                            node: resolved,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                        base: *object,
                        index: index.clone(),
                        mode: ProjectionMode::Navigate,
                    });
                    match read.value {
                        QueryResult::Value(node) if node != item.node => work.push(RuntimeWork {
                            node,
                            filter_unknown: item.filter_unknown,
                        }),
                        _ => push_unknown(&mut kinds),
                    }
                }
                SemanticNodeData::Conditional { .. } | SemanticNodeData::KeyOf { .. } => {
                    let reduced = self
                        .evaluate_deferred_semantic_node_with_context(item.node, transit)
                        .into_active_query_build_node(self);
                    if reduced == item.node {
                        push_unknown(&mut kinds);
                    } else {
                        work.push(RuntimeWork {
                            node: reduced,
                            filter_unknown: item.filter_unknown,
                        });
                    }
                }
                SemanticNodeData::Mapped { .. }
                | SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::RawFallback { .. }
                | SemanticNodeData::SyntheticBinding { .. }
                | SemanticNodeData::Opaque(_) => push_unknown(&mut kinds),
            }
        }

        let observed_self_roots = self.observed_self_roots_from_nodes(observed_nodes);
        let classification = BroadRuntimeClassification::new(kinds);
        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Value(SemanticQueryValue::BroadRuntime(classification)),
            self.project_generation_signature(),
        )
            .into();
        output.observed_self_roots = observed_self_roots;
        if output.observed_self_roots.is_empty() {
            output.cache_suppress = true;
        }
        output
    }

    fn push_instantiated_runtime_subject(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
        args: Arc<[SemanticNodeId]>,
        item: RuntimeWork,
        transit: ProjectionReductionContext,
        work: &mut Vec<RuntimeWork>,
        kinds: &mut Vec<BroadRuntimeKind>,
    ) {
        let slot = self.type_slot_for(
            Arc::clone(&identity.canonical_id),
            identity.owner,
            Arc::clone(&identity.decl_name),
        );
        let read = self.execute_read(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                args,
                self.instantiate_context_for(&identity.canonical_id, transit),
            ),
        ));
        match read.value {
            QueryResult::Value(node) if node != item.node => {
                work.push(RuntimeWork {
                    node,
                    filter_unknown: item.filter_unknown,
                });
            }
            _ if !item.filter_unknown => kinds.push(BroadRuntimeKind::Unknown),
            _ => {}
        }
    }
}

fn broad_kind_for_nominal(kind: RuntimeNominal) -> BroadRuntimeKind {
    match kind {
        RuntimeNominal::Date => BroadRuntimeKind::Date,
        RuntimeNominal::Map => BroadRuntimeKind::Map,
        RuntimeNominal::Set => BroadRuntimeKind::Set,
        RuntimeNominal::WeakMap => BroadRuntimeKind::WeakMap,
        RuntimeNominal::WeakSet => BroadRuntimeKind::WeakSet,
        RuntimeNominal::Promise => BroadRuntimeKind::Promise,
        RuntimeNominal::Error => BroadRuntimeKind::Error,
    }
}
