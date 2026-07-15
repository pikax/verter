//! Origin-graph builder for component-meta audit footprint capture.
//!
//! Domain 13 — owns the standalone `build_origin_graph`
//! function that reduces `SemanticGraphStore` + a set of surface
//! identities to a `verter_protocol::types::OriginGraphDto` (the
//! audit-trace shape). Called once per component-meta request when
//! `audit_enabled && config.footprint_capture` is true (the gate
//! enforced by `gate_text_includes_audit_enabled` in
//! `meta_resolve/host_methods.rs`).
//!
//! Visibility: the formerly-private `fn build_origin_graph` is exposed
//! at `pub(crate) fn` so the impl block in `host_methods.rs` keeps
//! calling it via the shell's
//! `pub(crate) use origin_graph::build_origin_graph;` re-export.

use std::sync::Arc;

use super::resolved_state::SurfaceNodeIdentities;

// / §4.19 — registry-route inline composition
// predicate deleted (verified callerless in production; the only
// consumer was a composition test that has also been deleted
// commit).

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn build_origin_graph(
    graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>,
    surface_identities: Option<&SurfaceNodeIdentities>,
) -> verter_protocol::types::OriginGraphDto {
    use crate::semantic_query::OriginEdgeKind;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::VecDeque;
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    // Step 9.2 / F6 scoped origin export: when surface_identities are
    // populated, reverse-walk via walk_origin_chain starting from each
    // surface node and collect only the reachable subgraph. Falls back
    // to export_all_origin_edges when surface_identities is None
    // (audit-off path or pre-populated state).
    let all_edges = if let Some(ids) = surface_identities {
        let mut roots: Vec<crate::semantic_query::SemanticNodeId> = Vec::new();
        let push_some =
            |roots: &mut Vec<_>, opt: &Option<crate::semantic_query::SemanticNodeId>| {
                if let Some(id) = opt {
                    roots.push(*id);
                }
            };
        for id in &ids.prop_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.emit_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.slot_binding_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.binding_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.registry_node_ids {
            push_some(&mut roots, id);
        }
        if roots.is_empty() {
            return OriginGraphDto::default();
        }
        let mut reached: FxHashSet<crate::semantic_query::SemanticNodeId> = FxHashSet::default();
        let mut worklist: VecDeque<crate::semantic_query::SemanticNodeId> =
            roots.into_iter().collect();
        let mut collected: Vec<(
            crate::semantic_query::SemanticNodeId,
            OriginEdgeKind,
            crate::semantic_query::OriginEdge,
        )> = Vec::new();
        while let Some(node) = worklist.pop_front() {
            if !reached.insert(node) {
                continue;
            }
            graph.walk_origin_chain(node, |kind, edge| {
                collected.push((node, kind, edge.clone()));
                for source in edge.sources.iter() {
                    if !reached.contains(source) {
                        worklist.push_back(*source);
                    }
                }
            });
        }
        collected
    } else {
        graph.export_all_origin_edges()
    };

    if all_edges.is_empty() {
        return OriginGraphDto::default();
    }

    let mut node_index: FxHashMap<crate::semantic_query::SemanticNodeId, u32> =
        FxHashMap::default();
    let mut nodes: Vec<OriginNodeDto> = Vec::new();
    let mut meta_strings: Vec<String> = Vec::new();
    let mut meta_index_map: FxHashMap<String, u32> = FxHashMap::default();

    let mut intern_node = |id: crate::semantic_query::SemanticNodeId,
                           graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>|
     -> u32 {
        if let Some(&idx) = node_index.get(&id) {
            return idx;
        }
        let idx = nodes.len() as u32;
        let (kind, label) = graph
            .node_data(id)
            .map(|d| {
                use crate::semantic_query::SemanticNodeData;
                let k = format!("{:?}", &*d).split_once('{').map_or_else(
                    || {
                        format!("{:?}", &*d)
                            .split_once('(')
                            .map_or_else(|| format!("{:?}", &*d), |(name, _)| name.to_string())
                    },
                    |(name, _)| name.to_string(),
                );
                let l = match &*d {
                    SemanticNodeData::Primitive(p) => Some(format!("{p:?}").to_lowercase()),
                    SemanticNodeData::Object(_) => Some("{...}".to_string()),
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(display_name.to_string())
                    }
                    SemanticNodeData::Literal(lit) => Some(format!("{lit:?}")),
                    SemanticNodeData::Array { readonly, .. } => {
                        Some(if *readonly { "readonly T[]" } else { "T[]" }.to_string())
                    }
                    SemanticNodeData::Tuple { .. } => Some("[...]".to_string()),
                    SemanticNodeData::Union(_) => Some("A | B".to_string()),
                    SemanticNodeData::Intersection(_) => Some("A & B".to_string()),
                    SemanticNodeData::Function { .. } => Some("(...) => R".to_string()),
                    _ => None,
                };
                (k, l)
            })
            .unwrap_or_else(|| ("Unknown".to_string(), None));
        nodes.push(OriginNodeDto {
            id: idx,
            kind,
            label,
        });
        node_index.insert(id, idx);
        idx
    };

    let mut edges_dto: Vec<OriginEdgeDto> = Vec::new();
    for (target_node, kind, edge) in &all_edges {
        let target_idx = intern_node(*target_node, graph);
        let edge_kind = match kind {
            OriginEdgeKind::Instantiate => "instantiate",
            OriginEdgeKind::SubstituteTypeParam => "substituteTypeParam",
            OriginEdgeKind::ConditionalSelect => "conditionalSelect",
            OriginEdgeKind::InferBind => "inferBind",
            OriginEdgeKind::ProjectMember => "projectMember",
            OriginEdgeKind::ProjectIndex => "projectIndex",
            OriginEdgeKind::ProjectPath => "projectPath",
            OriginEdgeKind::Normalize => "normalize",
            OriginEdgeKind::AliasResolve => "aliasResolve",
        };
        let meta_str = format!("{:?}", edge.meta);
        let meta_idx = if meta_str == "None" {
            None
        } else {
            let idx = if let Some(&existing) = meta_index_map.get(&meta_str) {
                existing
            } else {
                let idx = meta_strings.len() as u32;
                meta_strings.push(meta_str.clone());
                meta_index_map.insert(meta_str, idx);
                idx
            };
            Some(idx)
        };
        for source in edge.sources.iter() {
            let source_idx = intern_node(*source, graph);
            edges_dto.push(OriginEdgeDto {
                source: source_idx,
                target: target_idx,
                kind: edge_kind.to_string(),
                meta_index: meta_idx,
            });
        }
    }

    OriginGraphDto {
        nodes,
        edges: edges_dto,
        meta_strings,
    }
}
