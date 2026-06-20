#![deny(missing_docs)]
//! Footprint miner — converts a drained `AccumulatorState` plus a
//! reference to the live [`SemanticGraphStore`] into a deterministic
//! [`RequestFootprintAudit`].
//!
//! Determinism rules:
//!
//! 1. Walk `state.derivation_edges_raw` and collect the unique
//!    [`SemanticNodeId`]s touched (as result or source).
//! 2. For each unique id, compute a [`NodeRecord`] whose
//!    `structural_hash` is content-derived (xxh3-128 of a stable
//!    debug fingerprint) — never intern-order.
//! 3. Sort the resulting node table by
//!    `(kind as u32, structural_hash, named_identity_key)`. The sorted
//!    index becomes the in-audit [`NodeId`].
//! 4. Translate every raw edge through the
//!    `SemanticNodeId → NodeId` map; sort edges by
//!    `(result, kind, sources)`; truncate at
//!    `HostConfig::max_derivation_edges` and set
//!    `graph_completeness.has_orphan_edges` when the cap fires.
//! 5. Derive the typed record vectors (`instantiations`, `projections`,
//!    `conditional_decisions`, `substitutions`, `alias_resolutions`)
//!    FROM the sorted edges so they inherit determinism.
//! 6. Translate `IndexedReadyBuilt` structured events into the typed
//!    `indexed_ready_builds` vector.
//! 7. Read the per-context cache-event counters from `ctx` —
//!    `CacheOutcomeTally` is exact even under concurrent audits because
//!    each request's context isolates its own events (this kills the
//!    `is_approximate` field).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::xxh3_128;

use super::accumulator::AccumulatorState;
use super::{
    AliasResolveRecord, CacheOutcomeTally, ConditionalBranch, ConditionalRecord,
    DerivationEdgeRecord, DerivationSubgraph, GraphCompletenessReport, IndexedReadyBuildRecord,
    InstantiationRecord, NamedIdentity, NodeId, NodeRecord, NormalizeKind, OriginEdgeKind,
    OriginEdgeMetaDto, ProjectPathSegment, ProjectionRecord, RequestFootprintAudit,
    SemanticNodeKind, StructuredAuditEvent, SubstitutionRecord,
};
use crate::request_context::RequestContext;
use crate::semantic_query::{
    BranchSelection, DeclIdentity, IndexKey, IndexSignature, LiteralValue, NodeScopeId,
    OriginEdgeKind as CoreOriginEdgeKind, OriginMeta, PathSegment, ScopeId, SemanticNodeData,
    SemanticNodeId, SurfaceMember, ValueRootKey,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::types::Hash16;
use verter_audit::AuditCaps;

/// Mine a deterministic [`RequestFootprintAudit`] from the drained
/// accumulator state, using `graph` for node-data lookups and `ctx` for
/// per-context cache counters. `max_edges` caps the derivation subgraph
/// — when truncation happens, the report's
/// `has_orphan_edges` flag is set. `caps` carries the
/// post-canonicalisation node cap (the raw-push caps were already
/// enforced at the accumulator surface — their counters arrived on
/// `state.truncation_counters`).
pub fn mine_footprint(
    graph: &SemanticGraphStore,
    state: AccumulatorState,
    ctx: &RequestContext,
    max_edges: usize,
    caps: &AuditCaps,
) -> RequestFootprintAudit {
    // ── 1. Collect unique SemanticNodeIds touched by raw edges ──────
    let mut touched: Vec<SemanticNodeId> = Vec::new();
    {
        let mut seen = std::collections::HashSet::with_capacity_and_hasher(
            state.derivation_edges_raw.len() * 2,
            rustc_hash::FxBuildHasher,
        );
        for raw in &state.derivation_edges_raw {
            if seen.insert(raw.result) {
                touched.push(raw.result);
            }
            for src in raw.edge.sources.iter() {
                if seen.insert(*src) {
                    touched.push(*src);
                }
            }
        }
    }

    // ── 2. Build NodeRecord for each touched id ────────────────────
    let mut nodes: Vec<(SemanticNodeId, NodeRecord)> = touched
        .into_iter()
        .map(|id| {
            let data = graph.node_data(id);
            let record = build_node_record(graph, data.as_deref());
            (id, record)
        })
        .collect();

    // ── 3. Sort and assign NodeId = index ──────────────────────────
    nodes.sort_by_key(|node| node_sort_key(&node.1));
    // Post-canonicalisation node cap. The raw-edge push surface
    // already capped the upstream growth (via `caps.derivation_edges`
    // at the accumulator); this is the corresponding cap on the
    // post-canonicalisation node table. Distinct count is recorded
    // into `truncation_counters.derivation_nodes_truncated`.
    let nodes_cap = caps.derivation_nodes();
    let nodes_truncated_count: u64 = if nodes.len() > nodes_cap {
        let dropped = nodes.len() - nodes_cap;
        nodes.truncate(nodes_cap);
        dropped as u64
    } else {
        0
    };
    let mut id_map: FxHashMap<SemanticNodeId, NodeId> =
        FxHashMap::with_capacity_and_hasher(nodes.len(), Default::default());
    let mut node_table: Vec<NodeRecord> = Vec::with_capacity(nodes.len());
    for (idx, (sid, rec)) in nodes.into_iter().enumerate() {
        id_map.insert(sid, NodeId(idx as u32));
        node_table.push(rec);
    }

    // ── 4. Translate raw edges, sort, and apply max_edges cap ──────
    let mut edges: Vec<DerivationEdgeRecord> = state
        .derivation_edges_raw
        .iter()
        .map(|raw| translate_edge(raw, &id_map))
        .collect();
    edges.sort_by_key(edge_sort_key);

    let mut completeness = GraphCompletenessReport::default();
    if edges.len() > max_edges {
        completeness.has_orphan_edges = true;
        completeness.edges_truncated = (edges.len() - max_edges) as u32;
        edges.truncate(max_edges);
    }

    // ── 5. Derive typed record vectors from the sorted edges ───────
    let mut instantiations: Vec<InstantiationRecord> = Vec::new();
    let mut projections: Vec<ProjectionRecord> = Vec::new();
    let mut conditional_decisions: Vec<ConditionalRecord> = Vec::new();
    let mut substitutions: Vec<SubstitutionRecord> = Vec::new();
    let mut alias_resolutions: Vec<AliasResolveRecord> = Vec::new();
    for edge in &edges {
        match edge.kind {
            OriginEdgeKind::Instantiate => {
                instantiations.push(InstantiationRecord {
                    result: edge.result,
                    decl_canonical_id: instantiate_decl_canonical(edge, &node_table),
                    decl_symbol_name: instantiate_decl_symbol(edge, &node_table),
                    args_fingerprint: instantiate_args_fingerprint(edge),
                    args: edge.sources.clone(),
                });
            }
            OriginEdgeKind::ProjectMember
            | OriginEdgeKind::ProjectIndex
            | OriginEdgeKind::ProjectPath => {
                // `MemberEdgeProvenance::PublishedField` edges are
                // publication-boundary markers (the producer
                // declaring "this member is admitted to the
                // user-visible surface"), NOT structural projection
                // steps. The `projections` lane summarises structural
                // walk steps (path / keyof-enumeration /
                // mapped-enumeration). Lifting `PublishedField` into
                // `projections` would conflate the declaration with a
                // walk hop and break leak audits that count
                // intermediate projection-path Member segments.
                if let OriginEdgeMetaDto::ProjectMember { provenance, .. } = &edge.meta {
                    if matches!(
                        provenance,
                        verter_audit::MemberEdgeProvenance::PublishedField
                    ) {
                        continue;
                    }
                }
                let path = match &edge.meta {
                    OriginEdgeMetaDto::ProjectMember { member_name, .. } => {
                        vec![ProjectPathSegment::Member {
                            name: Arc::clone(member_name),
                        }]
                    }
                    OriginEdgeMetaDto::ProjectIndex { index_key } => {
                        vec![ProjectPathSegment::Index {
                            key: Arc::clone(index_key),
                        }]
                    }
                    OriginEdgeMetaDto::ProjectPath { path } => path.clone(),
                    _ => Vec::new(),
                };
                let base = edge.sources.first().copied().unwrap_or(NodeId(0));
                projections.push(ProjectionRecord {
                    result: edge.result,
                    base,
                    path,
                });
            }
            OriginEdgeKind::ConditionalSelect => {
                if let OriginEdgeMetaDto::ConditionalSelect { branch } = &edge.meta {
                    conditional_decisions.push(ConditionalRecord {
                        result: edge.result,
                        branch: *branch,
                    });
                }
            }
            OriginEdgeKind::SubstituteTypeParam => {
                if let OriginEdgeMetaDto::SubstituteTypeParam {
                    param_name,
                    substituted_with,
                } = &edge.meta
                {
                    substitutions.push(SubstitutionRecord {
                        result: edge.result,
                        param_name: Arc::clone(param_name),
                        substituted_with: *substituted_with,
                    });
                }
            }
            OriginEdgeKind::AliasResolve => {
                if let OriginEdgeMetaDto::AliasResolve { alias_name } = &edge.meta {
                    alias_resolutions.push(AliasResolveRecord {
                        result: edge.result,
                        alias_name: Arc::clone(alias_name),
                    });
                }
            }
            // InferBind / Normalize / SharedLoadReuse — covered by the
            // edge subgraph itself; no flat-record vector lifts them in v1.
            _ => {}
        }
    }

    // ── 6. Lift typed records that come from accumulator paths ────
    let indexed_ready_builds = if state.indexed_ready_builds.is_empty() {
        extract_indexed_ready_builds(&state.structured_events)
    } else {
        state.indexed_ready_builds.clone()
    };
    // Surface the verbatim structured-event log on the published
    // footprint so the audit dump exposes materializer envelopes and
    // dispatch markers without recomputation.
    let structured_events = state.structured_events.clone();

    // ── 7. Cache outcomes from per-context counters (exact) ────────
    let cache_outcomes = CacheOutcomeTally {
        cold_builds: ctx.cold_builds.load(Ordering::Relaxed) as u32,
        warm_hits: ctx.warm_hits.load(Ordering::Relaxed) as u32,
        joined_waits: ctx.joined_waits.load(Ordering::Relaxed) as u32,
        sentinels: ctx.sentinels.load(Ordering::Relaxed) as u32,
        inflight_aborted_retries: ctx.inflight_aborted_retries.load(Ordering::Relaxed) as u32,
        cold_aborts_swept: ctx.cold_aborts_swept.load(Ordering::Relaxed) as u32,
    };

    // ── 7b. Resolver / import-route hot-path counters (exact) ─────
    let resolver_hot_path = verter_audit::ResolverHotPathCounters {
        frontier_closure_invocations_total: ctx
            .frontier_closure_invocations_total
            .load(Ordering::Relaxed) as u32,
        frontier_closure_invocations_target_none: ctx
            .frontier_closure_invocations_target_none
            .load(Ordering::Relaxed) as u32,
        frontier_closure_redundant_target_none_pairs: ctx
            .frontier_closure_redundant_target_none_pairs
            .load(Ordering::Relaxed) as u32,
        resolved_external_type_cache_negative_hits: ctx
            .resolved_external_type_cache_negative_hits
            .load(Ordering::Relaxed) as u32,
        resolved_external_type_cache_negative_misses: ctx
            .resolved_external_type_cache_negative_misses
            .load(Ordering::Relaxed) as u32,
        resolve_import_cold_positive: ctx.resolve_import_cold_positive.load(Ordering::Relaxed)
            as u32,
        resolve_import_cold_negative: ctx.resolve_import_cold_negative.load(Ordering::Relaxed)
            as u32,
        resolve_import_warm_positive: ctx.resolve_import_warm_positive.load(Ordering::Relaxed)
            as u32,
        resolve_import_warm_negative: ctx.resolve_import_warm_negative.load(Ordering::Relaxed)
            as u32,
        known_miss_route_served: ctx.known_miss_route_served.load(Ordering::Relaxed) as u32,
        known_miss_route_revalidated: ctx.known_miss_route_revalidated.load(Ordering::Relaxed)
            as u32,
        known_miss_route_recomputed: ctx.known_miss_route_recomputed.load(Ordering::Relaxed) as u32,
        imported_registry_cold: ctx.imported_registry_cold.load(Ordering::Relaxed) as u32,
        imported_registry_warm: ctx.imported_registry_warm.load(Ordering::Relaxed) as u32,
        imported_registry_negative: ctx.imported_registry_negative.load(Ordering::Relaxed) as u32,
        imported_root_cold: ctx.imported_root_cold.load(Ordering::Relaxed) as u32,
        imported_root_warm: ctx.imported_root_warm.load(Ordering::Relaxed) as u32,
        route_db_barrel_steps: ctx.route_db_barrel_steps.load(Ordering::Relaxed) as u32,
        route_db_wildcard_fanout: ctx.route_db_wildcard_fanout.load(Ordering::Relaxed) as u32,
        prepared_decl_bundle_cold: ctx.prepared_decl_bundle_cold.load(Ordering::Relaxed) as u32,
        prepared_decl_bundle_warm: ctx.prepared_decl_bundle_warm.load(Ordering::Relaxed) as u32,
        prepared_decl_bundle_reject_entry_missing: ctx
            .prepared_decl_bundle_reject_entry_missing
            .load(Ordering::Relaxed) as u32,
        prepared_decl_bundle_reject_self_root_untracked: ctx
            .prepared_decl_bundle_reject_self_root_untracked
            .load(Ordering::Relaxed)
            as u32,
        prepared_decl_bundle_reject_self_root_hash_mismatch: ctx
            .prepared_decl_bundle_reject_self_root_hash_mismatch
            .load(Ordering::Relaxed)
            as u32,
        prepared_decl_bundle_reject_import_route_absent: ctx
            .prepared_decl_bundle_reject_import_route_absent
            .load(Ordering::Relaxed)
            as u32,
        prepared_decl_bundle_reject_import_route_mismatch: ctx
            .prepared_decl_bundle_reject_import_route_mismatch
            .load(Ordering::Relaxed)
            as u32,
        prepared_decl_bundle_reject_other: ctx
            .prepared_decl_bundle_reject_other
            .load(Ordering::Relaxed) as u32,
        semantic_query_typeof_cold: ctx.semantic_query_typeof_cold.load(Ordering::Relaxed) as u32,
        semantic_query_typeof_warm: ctx.semantic_query_typeof_warm.load(Ordering::Relaxed) as u32,
        semantic_query_instantiate_cold: ctx.semantic_query_instantiate_cold.load(Ordering::Relaxed)
            as u32,
        semantic_query_instantiate_warm: ctx.semantic_query_instantiate_warm.load(Ordering::Relaxed)
            as u32,
        semantic_query_conditional_cold: ctx.semantic_query_conditional_cold.load(Ordering::Relaxed)
            as u32,
        semantic_query_conditional_warm: ctx.semantic_query_conditional_warm.load(Ordering::Relaxed)
            as u32,
        semantic_query_mapped_type_cold: ctx.semantic_query_mapped_type_cold.load(Ordering::Relaxed)
            as u32,
        semantic_query_mapped_type_warm: ctx.semantic_query_mapped_type_warm.load(Ordering::Relaxed)
            as u32,
        semantic_query_indexed_access_cold: ctx
            .semantic_query_indexed_access_cold
            .load(Ordering::Relaxed) as u32,
        semantic_query_indexed_access_warm: ctx
            .semantic_query_indexed_access_warm
            .load(Ordering::Relaxed) as u32,
        semantic_query_keyof_cold: ctx.semantic_query_keyof_cold.load(Ordering::Relaxed) as u32,
        semantic_query_keyof_warm: ctx.semantic_query_keyof_warm.load(Ordering::Relaxed) as u32,
        semantic_query_project_path_cold: ctx
            .semantic_query_project_path_cold
            .load(Ordering::Relaxed) as u32,
        semantic_query_project_path_warm: ctx
            .semantic_query_project_path_warm
            .load(Ordering::Relaxed) as u32,
        substitute_top_level_calls: ctx.substitute_top_level_calls.load(Ordering::Relaxed) as u32,
        substitute_memo_hits: ctx.substitute_memo_hits.load(Ordering::Relaxed) as u32,
        substitute_typeof_opaque: ctx.substitute_typeof_opaque.load(Ordering::Relaxed) as u32,
        substitute_conditional_descend: ctx.substitute_conditional_descend.load(Ordering::Relaxed)
            as u32,
        substitute_mapped_type_descend: ctx.substitute_mapped_type_descend.load(Ordering::Relaxed)
            as u32,
        build_typeof_calls: ctx.build_typeof_calls.load(Ordering::Relaxed) as u32,
        build_typeof_prepared_value_misses: ctx
            .build_typeof_prepared_value_misses
            .load(Ordering::Relaxed) as u32,
        mapped_member_plain_unique: ctx.mapped_member_plain_unique.load(Ordering::Relaxed) as u32,
        mapped_member_plain_repeated: ctx.mapped_member_plain_repeated.load(Ordering::Relaxed)
            as u32,
        mapped_member_selected_key_unique: ctx
            .mapped_member_selected_key_unique
            .load(Ordering::Relaxed) as u32,
        mapped_member_selected_key_repeated: ctx
            .mapped_member_selected_key_repeated
            .load(Ordering::Relaxed) as u32,
        prepared_decl_bundle_callsite_scope_payload: ctx
            .prepared_decl_bundle_callsite_scope_payload
            .load(Ordering::Relaxed) as u32,
        prepared_decl_bundle_callsite_build_instantiate: ctx
            .prepared_decl_bundle_callsite_build_instantiate
            .load(Ordering::Relaxed)
            as u32,
        prepared_decl_bundle_callsite_other: ctx
            .prepared_decl_bundle_callsite_other
            .load(Ordering::Relaxed) as u32,
        mapped_binder_ordinal_collision: ctx.mapped_binder_ordinal_collision.load(Ordering::Relaxed)
            as u32,
        recursive_substitute_unique: ctx.recursive_substitute_unique.load(Ordering::Relaxed) as u32,
        recursive_substitute_repeated: ctx.recursive_substitute_repeated.load(Ordering::Relaxed)
            as u32,
        substitute_mapped_rebuild: ctx.substitute_mapped_rebuild.load(Ordering::Relaxed) as u32,
        substitute_conditional_rebuild: ctx.substitute_conditional_rebuild.load(Ordering::Relaxed)
            as u32,
        recursive_substitute_memo_hits: ctx.recursive_substitute_memo_hits.load(Ordering::Relaxed)
            as u32,
        imported_macro_surface_projection: ctx
            .imported_macro_surface_projection
            .load(Ordering::Relaxed) as u32,
    };

    // Truncation counters: combine the accumulator-side counters
    // (raw-push caps applied at `push_*` time) with the
    // post-canonicalisation node cap applied above. The
    // accumulator-side counters already record drops for every
    // raw lane.
    let mut truncation_counters = state.truncation_counters.clone();
    truncation_counters.derivation_nodes_truncated += nodes_truncated_count;

    RequestFootprintAudit {
        indexed_ready_builds,
        vfs_reads: state.vfs_reads,
        shared_load_reuses: state.shared_load_reuses,
        instantiations,
        projections,
        conditional_decisions,
        substitutions,
        alias_resolutions: if alias_resolutions.is_empty() {
            state.alias_resolutions
        } else {
            alias_resolutions
        },
        materializations: state.materializations,
        cache_outcomes,
        graph_completeness: completeness,
        derivation_subgraph: DerivationSubgraph {
            nodes: node_table,
            edges,
        },
        structured_events,
        resolver_hot_path,
        truncation_counters,
    }
}

// ──────────────────────────────────────────────────────────────────────
// NodeRecord construction
// ──────────────────────────────────────────────────────────────────────

fn build_node_record(graph: &SemanticGraphStore, data: Option<&SemanticNodeData>) -> NodeRecord {
    let kind = data.map(map_node_kind).unwrap_or(SemanticNodeKind::Opaque);
    let display_label = data
        .map(display_label_for)
        .unwrap_or_else(|| Arc::from("<unknown>"));
    let structural_hash = data
        .map(|d| structural_hash_of(graph, d))
        .unwrap_or([0u8; 16]);
    let named_identity = data.and_then(named_identity_of);
    NodeRecord {
        kind,
        named_identity,
        structural_hash,
        display_label,
    }
}

fn map_node_kind(data: &SemanticNodeData) -> SemanticNodeKind {
    match data {
        SemanticNodeData::Alias(_) => SemanticNodeKind::Alias,
        SemanticNodeData::Object(_) => SemanticNodeKind::Object,
        SemanticNodeData::Union(_) => SemanticNodeKind::Union,
        SemanticNodeData::Intersection(_) => SemanticNodeKind::Intersection,
        SemanticNodeData::Primitive(_) => SemanticNodeKind::Primitive,
        // Literals collapse to `Primitive` in v1 — `display_label`
        // distinguishes the exact literal value.
        SemanticNodeData::Literal(_) => SemanticNodeKind::Primitive,
        SemanticNodeData::Opaque(_) => SemanticNodeKind::Opaque,
        SemanticNodeData::Array { .. } => SemanticNodeKind::Array,
        SemanticNodeData::Tuple { .. } => SemanticNodeKind::Tuple,
        SemanticNodeData::TemplateLiteral { .. } => SemanticNodeKind::TemplateLiteral,
        SemanticNodeData::KeyOf { .. } => SemanticNodeKind::KeyOf,
        SemanticNodeData::IndexedAccess { .. } => SemanticNodeKind::IndexedAccess,
        SemanticNodeData::Mapped { .. } => SemanticNodeKind::Mapped,
        SemanticNodeData::TypeOf(_) => SemanticNodeKind::TypeOf,
        SemanticNodeData::TypeParam { .. } => SemanticNodeKind::TypeParam,
        SemanticNodeData::Infer { .. } => SemanticNodeKind::Other {
            name: Arc::from("Infer"),
        },
        SemanticNodeData::Conditional { .. } => SemanticNodeKind::Conditional,
        SemanticNodeData::VueMacroElements(_) => SemanticNodeKind::Other {
            name: Arc::from("VueMacroElements"),
        },
        SemanticNodeData::Function { .. } => SemanticNodeKind::Other {
            name: Arc::from("Function"),
        },
        SemanticNodeData::DeclRef { .. } => SemanticNodeKind::Other {
            name: Arc::from("DeclRef"),
        },
        SemanticNodeData::InstantiationRef { .. } => SemanticNodeKind::Other {
            name: Arc::from("InstantiationRef"),
        },
        SemanticNodeData::MergedDecl { .. } => SemanticNodeKind::Other {
            name: Arc::from("MergedDecl"),
        },
        SemanticNodeData::BareRef(_) => SemanticNodeKind::Other {
            name: Arc::from("BareRef"),
        },
        SemanticNodeData::ImportType(_) => SemanticNodeKind::Other {
            name: Arc::from("ImportType"),
        },
        SemanticNodeData::RawFallback { .. } => SemanticNodeKind::Other {
            name: Arc::from("RawFallback"),
        },
        SemanticNodeData::ConstructorType { .. } => SemanticNodeKind::Other {
            name: Arc::from("ConstructorType"),
        },
        SemanticNodeData::SyntheticBinding { .. } => SemanticNodeKind::Other {
            name: Arc::from("SyntheticBinding"),
        },
    }
}

fn display_label_for(data: &SemanticNodeData) -> Arc<str> {
    match data {
        SemanticNodeData::Alias(_) => Arc::from("Alias"),
        SemanticNodeData::Object(_) => Arc::from("Object"),
        SemanticNodeData::Union(arms) => Arc::from(format!("Union[{}]", arms.len())),
        SemanticNodeData::Intersection(arms) => Arc::from(format!("Intersection[{}]", arms.len())),
        SemanticNodeData::Primitive(p) => Arc::from(format!("{p:?}")),
        SemanticNodeData::Literal(lit) => Arc::from(format!("{lit:?}")),
        SemanticNodeData::Opaque(_) => Arc::from("Opaque"),
        SemanticNodeData::Array { readonly, .. } => {
            Arc::from(if *readonly { "ReadonlyArray" } else { "Array" })
        }
        SemanticNodeData::Tuple {
            elements, readonly, ..
        } => Arc::from(format!(
            "{}Tuple[{}]",
            if *readonly { "Readonly" } else { "" },
            elements.len()
        )),
        SemanticNodeData::TemplateLiteral { quasis, .. } => {
            Arc::from(format!("Template[{}]", quasis.len()))
        }
        SemanticNodeData::KeyOf { .. } => Arc::from("keyof"),
        SemanticNodeData::IndexedAccess { index, .. } => {
            Arc::from(format!("IndexedAccess({})", index_key_label(index)))
        }
        SemanticNodeData::Mapped { .. } => Arc::from("Mapped"),
        SemanticNodeData::TypeOf(_) => {
            let (value_root, _path) = data.typeof_head().expect("TypeOf carrier head");
            Arc::from(format!("typeof {}", value_root.name))
        }
        SemanticNodeData::TypeParam { display_name, .. } => Arc::clone(display_name),
        SemanticNodeData::Infer { name } => Arc::from(format!("infer {name}")),
        SemanticNodeData::Conditional { distributive, .. } => Arc::from(if *distributive {
            "Conditional<distributive>"
        } else {
            "Conditional"
        }),
        SemanticNodeData::VueMacroElements(_) => Arc::from("VueMacroElements"),
        SemanticNodeData::Function { params, .. } => {
            Arc::from(format!("Function[{} params]", params.len()))
        }
        SemanticNodeData::DeclRef { identity } => {
            Arc::from(format!("DeclRef({})", identity.decl_name))
        }
        SemanticNodeData::InstantiationRef { base, args } => Arc::from(format!(
            "InstantiationRef({}<{} args>)",
            base.decl_name,
            args.len()
        )),
        SemanticNodeData::MergedDecl { contributors } => {
            Arc::from(format!("MergedDecl[{}]", contributors.len()))
        }
        SemanticNodeData::BareRef(_) => {
            let (name, _scope) = data.bare_ref_head().expect("BareRef carrier head");
            Arc::from(format!("BareRef({name})"))
        }
        SemanticNodeData::ImportType(_) => {
            let (specifier, _qualifier, _typeof_query) =
                data.import_type_head().expect("ImportType carrier head");
            Arc::from(format!("import(\"{specifier}\")"))
        }
        SemanticNodeData::RawFallback { .. } => Arc::from("RawFallback"),
        SemanticNodeData::ConstructorType { .. } => Arc::from("ConstructorType"),
        SemanticNodeData::SyntheticBinding { id, .. } => {
            Arc::from(format!("SyntheticBinding({})", id.binding_name))
        }
    }
}

/// Depth backstop for the structural walk, secondary to the visited-set cycle
/// guard. The visited set already terminates every cycle reachable through the
/// interned DAG; this ceiling is a defensive bound against a pathologically deep
/// (but acyclic) chain. At the ceiling the walk encodes a fixed
/// [`TAG_DEPTH_CEILING`] sentinel — never an arena ordinal.
const STRUCTURAL_HASH_MAX_DEPTH: u32 = 64;

/// Content-only, recursive, variant-tagged structural fingerprint of a semantic
/// node.
///
/// The fingerprint is derived EXCLUSIVELY from semantic CONTENT — it never folds
/// a raw [`SemanticNodeId`] arena ordinal. A `SemanticNodeId` is allocated
/// sequentially on intern-miss and is only meaningful inside one store for one
/// project generation (see the contract on [`SemanticNodeId`]); folding it would
/// make two equivalent `Foo<String>` carriers hash differently whenever `String`
/// received a different ordinal, breaking the file-level content-determinism
/// contract. Instead, every child reference is replaced by the RECURSIVE
/// structural fingerprint of the node it points at, resolved through
/// `graph.node_data`.
///
/// Encoding shape:
///
/// - Each variant emits a one-byte [`VariantTag`] discriminant, so two different
///   variants can never collide on equal payload bytes.
/// - Scalars are emitted by their little-endian bytes; `Arc<str>` and string
///   collections are LENGTH-PREFIXED; child-reference fields are replaced by the
///   recursive child fingerprint, also length/order-prefixed where a collection.
/// - The three carriers (`TypeOf` / `BareRef` / `ImportType`) are fingerprinted
///   from their public HEAD (which exposes no args) PLUS the ordered recursive
///   fingerprints of their `type_args` children, reached through the sole
///   descent accessor [`SemanticNodeData::carrier_type_args`]. Their private
///   `type_args` layout is NEVER `Debug`-rendered.
///
/// Cycle safety: an in-progress visited set of `SemanticNodeId`s on the current
/// descent path terminates any cycle in the interned graph by emitting a fixed
/// [`TAG_CYCLE`] back-reference sentinel instead of recursing forever. A
/// secondary depth ceiling ([`STRUCTURAL_HASH_MAX_DEPTH`]) backstops a
/// pathologically deep acyclic chain with a fixed [`TAG_DEPTH_CEILING`]
/// sentinel. A child id that does not resolve in `graph` emits a fixed
/// [`TAG_UNRESOLVED_CHILD`] sentinel — never the ordinal value (two distinct but
/// equally-unresolved children collapsing is acceptable; two equal-content
/// children diverging by ordinal is not).
fn structural_hash_of(graph: &SemanticGraphStore, data: &SemanticNodeData) -> Hash16 {
    // TODO(follow-up): a per-mine `FxHashMap<SemanticNodeId, Hash16>` content-hash
    // memo would let `encode_child` reuse a child's already-computed fingerprint
    // instead of re-walking shared subtrees per reference. It is NOT a drop-in:
    // the encoder emits a node's FULL recursive bytes inline (not its 16-byte
    // hash), and the same node legitimately encodes as `TAG_CYCLE` vs full
    // content depending on the descent path — a naive `SemanticNodeId → bytes`
    // (or `→ Hash16`) memo is path-INSENSITIVE and would both change every
    // fingerprint's bytes and mis-handle the cycle sentinel. The durable shape is
    // an encode-once-then-splice cache that preserves byte-identity and respects
    // the visited-path sentinels; deferred so this fix does not risk any
    // fingerprint value. The depth-64 ceiling + visited set already bound the
    // current re-walk, so it terminates.
    let mut enc = StructuralEncoder {
        graph,
        buf: Vec::with_capacity(128),
        visited: Vec::new(),
    };
    enc.encode_node_data(data, 0);
    xxh3_128(&enc.buf).to_le_bytes()
}

/// Stateful structural encoder. Owns the growing byte `buf`, the in-progress
/// `visited` path set (for cycle detection), and a borrow of the graph for child
/// resolution.
struct StructuralEncoder<'g> {
    graph: &'g SemanticGraphStore,
    buf: Vec<u8>,
    /// `SemanticNodeId`s currently on the descent path. Used ONLY to detect a
    /// back-edge — never folded into the hash bytes.
    visited: Vec<SemanticNodeId>,
}

/// One-byte variant discriminants for the structural encoding. Each
/// `SemanticNodeData` variant — plus the descent sentinels — occupies a distinct
/// tag so disjoint variants live in disjoint hash-input spaces. Values are fixed
/// and independent of source declaration order; a new variant takes the next
/// free tag.
#[repr(u8)]
enum VariantTag {
    Alias = 1,
    Object = 2,
    Union = 3,
    Intersection = 4,
    Primitive = 5,
    Literal = 6,
    Opaque = 7,
    Array = 8,
    Tuple = 9,
    TemplateLiteral = 10,
    KeyOf = 11,
    IndexedAccess = 12,
    Mapped = 13,
    TypeOf = 14,
    TypeParam = 15,
    Infer = 16,
    Conditional = 17,
    VueMacroElements = 18,
    Function = 19,
    DeclRef = 20,
    InstantiationRef = 21,
    MergedDecl = 22,
    BareRef = 23,
    ImportType = 24,
    RawFallback = 25,
    ConstructorType = 26,
    SyntheticBinding = 27,
}

/// Descent sentinel: a child id currently on the descent path (a graph cycle).
const TAG_CYCLE: u8 = 0xF0;
/// Descent sentinel: a child id that did not resolve in `graph`.
const TAG_UNRESOLVED_CHILD: u8 = 0xF1;
/// Descent sentinel: the recursion depth backstop fired.
const TAG_DEPTH_CEILING: u8 = 0xF2;

impl StructuralEncoder<'_> {
    /// Push a length-prefixed string.
    fn push_str(&mut self, s: &str) {
        self.buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// Push a length-prefixed list of strings (order-preserving).
    fn push_str_slice(&mut self, items: &[Arc<str>]) {
        self.buf
            .extend_from_slice(&(items.len() as u64).to_le_bytes());
        for item in items {
            self.push_str(item);
        }
    }

    /// Push an `Option<scalar-tag>`: `0` for `None`, `1` for `Some`.
    fn push_present(&mut self, present: bool) {
        self.buf.push(u8::from(present));
    }

    /// Resolve `id` and fold its RECURSIVE structural fingerprint into `buf`.
    /// Never folds the ordinal: a cycle, an unresolved id, or the depth backstop
    /// each emit a FIXED sentinel byte instead.
    fn encode_child(&mut self, id: SemanticNodeId, depth: u32) {
        if self.visited.contains(&id) {
            self.buf.push(TAG_CYCLE);
            return;
        }
        if depth >= STRUCTURAL_HASH_MAX_DEPTH {
            self.buf.push(TAG_DEPTH_CEILING);
            return;
        }
        match self.graph.node_data(id) {
            Some(child) => {
                self.visited.push(id);
                self.encode_node_data(&child, depth + 1);
                self.visited.pop();
            }
            None => self.buf.push(TAG_UNRESOLVED_CHILD),
        }
    }

    /// Encode every child id of an ordered id slice, length/order-prefixed.
    fn encode_child_slice(&mut self, ids: &[SemanticNodeId], depth: u32) {
        self.buf
            .extend_from_slice(&(ids.len() as u64).to_le_bytes());
        for id in ids {
            self.encode_child(*id, depth);
        }
    }

    /// Encode an `Option<SemanticNodeId>`: a presence byte followed by the child
    /// fingerprint when present.
    fn encode_child_opt(&mut self, id: Option<SemanticNodeId>, depth: u32) {
        match id {
            Some(id) => {
                self.push_present(true);
                self.encode_child(id, depth);
            }
            None => self.push_present(false),
        }
    }

    /// Encode an [`IndexKey`] structurally: a kind byte, then the key content
    /// (string / canonical integer bytes / recursive child fingerprint).
    fn encode_index_key(&mut self, index: &IndexKey, depth: u32) {
        match index {
            IndexKey::String(s) => {
                self.buf.push(0);
                self.push_str(s);
            }
            IndexKey::Number(n) => {
                self.buf.push(1);
                self.buf.extend_from_slice(&n.get().to_le_bytes());
            }
            IndexKey::TypeNode(id) => {
                self.buf.push(2);
                self.encode_child(*id, depth);
            }
        }
    }

    /// Encode a [`NodeScopeId`] by its SEMANTIC content (canonical id +
    /// whole-hash + local scope), never an arena ordinal.
    fn encode_node_scope(&mut self, scope: &NodeScopeId) {
        match scope {
            NodeScopeId::Global => self.buf.push(0),
            NodeScopeId::File {
                canonical_id,
                whole_hash,
                local_scope,
            } => {
                self.buf.push(1);
                self.push_str(canonical_id);
                self.buf.extend_from_slice(whole_hash);
                match local_scope {
                    Some(s) => {
                        self.push_present(true);
                        self.buf.extend_from_slice(&s.to_le_bytes());
                    }
                    None => self.push_present(false),
                }
            }
        }
    }

    /// Encode a [`ScopeId`] by content (canonical id + optional local scope).
    fn encode_scope_id(&mut self, scope: &ScopeId) {
        self.push_str(&scope.canonical_id);
        match scope.local_scope {
            Some(s) => {
                self.push_present(true);
                self.buf.extend_from_slice(&s.to_le_bytes());
            }
            None => self.push_present(false),
        }
    }

    /// Encode a [`ValueRootKey`] by content (scope + name).
    fn encode_value_root(&mut self, root: &ValueRootKey) {
        self.encode_scope_id(&root.scope);
        self.push_str(&root.name);
    }

    /// Encode a [`DeclIdentity`] by content (canonical id + whole-hash + name).
    fn encode_decl_identity(&mut self, identity: &DeclIdentity) {
        self.push_str(&identity.canonical_id);
        self.buf.extend_from_slice(&identity.whole_hash);
        self.push_str(&identity.decl_name);
    }

    /// Encode one [`SurfaceMember`]: scalar/string fields by content, the value
    /// type by recursive child fingerprint.
    fn encode_surface_member(&mut self, m: &SurfaceMember, depth: u32) {
        self.push_str(&m.name);
        self.encode_child(m.value, depth);
        self.buf.push(u8::from(m.optional));
        self.buf.push(u8::from(m.readonly));
        self.buf.push(u8::from(m.is_method));
        // `visibility` / `merge_role` are id-free C-like enums; a Debug of them
        // cannot transitively print a `SemanticNodeId`, so a stable string of
        // their discriminant is content-deterministic.
        self.push_str(&format!("{:?}", m.visibility));
        self.push_str(&format!("{:?}", m.spans));
        match &m.declaration_origin {
            Some(o) => {
                self.push_present(true);
                self.push_str(o);
            }
            None => self.push_present(false),
        }
        self.buf.push(u8::from(m.declared_in_macro_type_arg));
        self.push_str(&format!("{:?}", m.merge_role));
    }

    /// Encode one [`IndexSignature`]: key / value types by recursive child
    /// fingerprint, the rest by content.
    fn encode_index_signature(&mut self, sig: &IndexSignature, depth: u32) {
        self.encode_child(sig.key_type, depth);
        self.encode_child(sig.value_type, depth);
        self.buf.push(u8::from(sig.readonly));
        self.push_str(&format!("{:?}", sig.spans));
        match &sig.declaration_origin {
            Some(o) => {
                self.push_present(true);
                self.push_str(o);
            }
            None => self.push_present(false),
        }
    }

    /// The structural encoder body. EXHAUSTIVE over every `SemanticNodeData`
    /// variant — NO `_` wildcard, so a new variant fails to compile here and
    /// must be classified (a content-bearing scalar, or a child-bearing variant
    /// whose ids are descended). `depth` is the current descent depth.
    fn encode_node_data(&mut self, data: &SemanticNodeData, depth: u32) {
        match data {
            // ── Single-child variants. ──
            SemanticNodeData::Alias(child) => {
                self.buf.push(VariantTag::Alias as u8);
                self.encode_child(*child, depth);
            }
            SemanticNodeData::Array { element, readonly } => {
                self.buf.push(VariantTag::Array as u8);
                self.buf.push(u8::from(*readonly));
                self.encode_child(*element, depth);
            }
            SemanticNodeData::KeyOf { base } => {
                self.buf.push(VariantTag::KeyOf as u8);
                self.encode_child(*base, depth);
            }
            SemanticNodeData::ConstructorType { signature } => {
                self.buf.push(VariantTag::ConstructorType as u8);
                self.encode_child(*signature, depth);
            }

            // ── Child-list variants. ──
            SemanticNodeData::Union(arms) => {
                self.buf.push(VariantTag::Union as u8);
                self.encode_child_slice(arms, depth);
            }
            SemanticNodeData::Intersection(arms) => {
                self.buf.push(VariantTag::Intersection as u8);
                self.encode_child_slice(arms, depth);
            }
            SemanticNodeData::MergedDecl { contributors } => {
                self.buf.push(VariantTag::MergedDecl as u8);
                self.encode_child_slice(contributors, depth);
            }

            // ── Compound-payload variants carrying child ids. ──
            SemanticNodeData::Object(surface) => {
                self.buf.push(VariantTag::Object as u8);
                self.buf
                    .extend_from_slice(&(surface.members.len() as u64).to_le_bytes());
                for m in surface.members.iter() {
                    self.encode_surface_member(m, depth);
                }
                self.encode_child_slice(&surface.call_signatures, depth);
                self.encode_child_slice(&surface.construct_signatures, depth);
                self.buf
                    .extend_from_slice(&(surface.index_signatures.len() as u64).to_le_bytes());
                for sig in surface.index_signatures.iter() {
                    self.encode_index_signature(sig, depth);
                }
                self.encode_child_opt(surface.keyspace, depth);
                self.buf.push(u8::from(surface.has_index_signature));
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                self.buf.push(VariantTag::Tuple as u8);
                self.buf.push(u8::from(*readonly));
                self.buf
                    .extend_from_slice(&(elements.len() as u64).to_le_bytes());
                for el in elements.iter() {
                    match &el.label {
                        Some(l) => {
                            self.push_present(true);
                            self.push_str(l);
                        }
                        None => self.push_present(false),
                    }
                    self.encode_child(el.value, depth);
                    self.buf.push(u8::from(el.optional));
                    self.buf.push(u8::from(el.rest));
                }
            }
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => {
                self.buf.push(VariantTag::TemplateLiteral as u8);
                self.push_str_slice(quasis);
                self.encode_child_slice(expressions, depth);
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                self.buf.push(VariantTag::IndexedAccess as u8);
                self.encode_child(*object, depth);
                self.encode_index_key(index, depth);
            }
            SemanticNodeData::Mapped { source, mapper } => {
                self.buf.push(VariantTag::Mapped as u8);
                self.encode_child(*source, depth);
                self.encode_child(mapper.parameter_node, depth);
                self.encode_child(mapper.key_space, depth);
                self.encode_child(mapper.value_expr, depth);
                self.push_str(&format!("{:?}", mapper.optionality));
                self.push_str(&format!("{:?}", mapper.readonly));
                self.encode_child_opt(mapper.name_remap, depth);
                self.push_str(&format!("{:?}", mapper.kind));
            }
            SemanticNodeData::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                display_name,
            } => {
                self.buf.push(VariantTag::TypeParam as u8);
                self.encode_decl_identity(decl);
                self.buf.extend_from_slice(&param_index.to_le_bytes());
                self.encode_child_opt(*constraint, depth);
                self.encode_child_opt(*default, depth);
                self.push_str(display_name);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                self.buf.push(VariantTag::Conditional as u8);
                self.buf.push(u8::from(*distributive));
                self.encode_child(*check, depth);
                self.encode_child(*extends, depth);
                self.encode_child(*true_branch_ref, depth);
                self.encode_child(*false_branch_ref, depth);
            }
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                signature_span,
                return_type_span,
            } => {
                self.buf.push(VariantTag::Function as u8);
                self.buf
                    .extend_from_slice(&(params.len() as u64).to_le_bytes());
                for p in params.iter() {
                    match &p.name {
                        Some(n) => {
                            self.push_present(true);
                            self.push_str(n);
                        }
                        None => self.push_present(false),
                    }
                    self.encode_child(p.ty, depth);
                    self.buf.push(u8::from(p.optional));
                    self.buf.push(u8::from(p.rest));
                    self.push_str(&format!("{:?}", p.span));
                }
                self.encode_child(*return_type, depth);
                self.buf
                    .extend_from_slice(&(type_parameters.len() as u64).to_le_bytes());
                for tp in type_parameters.iter() {
                    self.push_str(&tp.name);
                    self.encode_child_opt(tp.constraint, depth);
                    self.encode_child_opt(tp.default, depth);
                }
                self.push_str(&format!("{signature_span:?}"));
                self.push_str(&format!("{return_type_span:?}"));
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                self.buf.push(VariantTag::InstantiationRef as u8);
                self.encode_decl_identity(base);
                self.encode_child_slice(args, depth);
            }

            // ── Carrier arms: HEAD (no args) + ordered recursive child hashes.
            // NEVER `Debug`-render the carrier — its private `type_args` layout
            // is a representation detail; descend through the sole accessor. ──
            SemanticNodeData::TypeOf(_) => {
                self.buf.push(VariantTag::TypeOf as u8);
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                self.encode_value_root(value_root);
                self.push_str_slice(path);
                let args = data.carrier_type_args().to_vec();
                self.encode_child_slice(&args, depth);
            }
            SemanticNodeData::BareRef(_) => {
                self.buf.push(VariantTag::BareRef as u8);
                let (name, scope) = data.bare_ref_head().expect("BareRef carrier head");
                self.push_str(name);
                self.encode_node_scope(scope);
                let args = data.carrier_type_args().to_vec();
                self.encode_child_slice(&args, depth);
            }
            SemanticNodeData::ImportType(_) => {
                self.buf.push(VariantTag::ImportType as u8);
                let (specifier, qualifier, typeof_query) =
                    data.import_type_head().expect("ImportType carrier head");
                self.push_str(specifier);
                self.push_str_slice(qualifier);
                self.buf.push(u8::from(typeof_query));
                let args = data.carrier_type_args().to_vec();
                self.encode_child_slice(&args, depth);
            }

            // ── Pure-scalar / id-free variants. Encoded by content. None of
            // these payloads can transitively hold a `SemanticNodeId`:
            // `PrimitiveKind` / `LiteralValue` / `QueryError` / the `Infer`
            // name / `RawFallback` raw text / the `DeclRef` identity are all
            // scalar/string or live in a lower crate than `SemanticNodeId`, so a
            // `Debug` of them is content-only. (`SyntheticBinding` is NOT in this
            // group: its `id` is scalar, but its `value_node` is a child node id
            // that is descended via `encode_child` — see that arm below.) ──
            SemanticNodeData::Primitive(p) => {
                self.buf.push(VariantTag::Primitive as u8);
                self.push_str(&format!("{p:?}"));
            }
            SemanticNodeData::Literal(lit) => {
                self.buf.push(VariantTag::Literal as u8);
                match lit {
                    LiteralValue::String(s) => {
                        self.buf.push(0);
                        self.push_str(s);
                    }
                    LiteralValue::Number(n) => {
                        self.buf.push(1);
                        // f64 by bit pattern — NaN folds to one stable encoding.
                        self.buf.extend_from_slice(&n.to_bits().to_le_bytes());
                    }
                    LiteralValue::Boolean(b) => {
                        self.buf.push(2);
                        self.buf.push(u8::from(*b));
                    }
                    LiteralValue::BigInt(s) => {
                        self.buf.push(3);
                        self.push_str(s);
                    }
                }
            }
            SemanticNodeData::Opaque(err) => {
                self.buf.push(VariantTag::Opaque as u8);
                // `QueryError` is an entirely scalar/string enum (no
                // `SemanticNodeId` in any arm), so its `Debug` is content-only.
                self.push_str(&format!("{err:?}"));
            }
            SemanticNodeData::Infer { name } => {
                self.buf.push(VariantTag::Infer as u8);
                self.push_str(name);
            }
            SemanticNodeData::RawFallback { raw } => {
                self.buf.push(VariantTag::RawFallback as u8);
                self.push_str(raw);
            }
            SemanticNodeData::DeclRef { identity } => {
                self.buf.push(VariantTag::DeclRef as u8);
                self.encode_decl_identity(identity);
            }
            SemanticNodeData::SyntheticBinding { id, value_node } => {
                self.buf.push(VariantTag::SyntheticBinding as u8);
                // `SyntheticBindingId` is content-free (canonical id + surface
                // kind + slot/binding names); no `SemanticNodeId`.
                self.push_str(&id.scope_canonical_id);
                self.push_str(&format!("{:?}", id.surface_kind));
                match &id.slot_name {
                    Some(n) => {
                        self.push_present(true);
                        self.push_str(n);
                    }
                    None => self.push_present(false),
                }
                self.push_str(&id.binding_name);
                // `value_node` is a [`SemanticNodeId`] arena ordinal stored as a
                // raw `u64` on the payload — store/generation-relative, NOT
                // content. It is NEVER folded as the ordinal: it is descended as
                // a graph child via [`Self::encode_child`], so the fingerprint
                // carries the RECURSIVE CONTENT of the node it points at (or a
                // fixed cycle / unresolved / depth sentinel), exactly like every
                // other child id. `value_node` participates in node interning
                // Eq/Hash, so two bindings pointing at different value nodes stay
                // structurally DISTINCT (descending by content preserves that),
                // while two content-equivalent bindings whose target was interned
                // at a different ordinal hash IDENTICALLY (the cross-run
                // byte-identity contract this encoder establishes).
                self.encode_child(SemanticNodeId(*value_node), depth);
            }
            SemanticNodeData::VueMacroElements(elements) => {
                self.buf.push(VariantTag::VueMacroElements as u8);
                // `ResolvedElements` is parser-built and provably never carries a
                // `TypeExpr::SyntheticSlotBinding` (the sole ordinal-bearing
                // `TypeExpr` variant — its `SyntheticCarrierKey.value_node: u64` is
                // a store/generation-relative `SemanticNodeId` arena ordinal).
                // ENFORCED by the static guard
                // `vue_macro_elements_ordinal_leak_is_producer_unreachable`
                // (parser/compiler carrier-free + single `insert_resolved_named_type`
                // caller + single `VueMacroElements` producer). Therefore the
                // `Debug` of `ResolvedElements` here folds NO `SemanticNodeId`
                // ordinal — its rendering is content-only and is the stable
                // identity available at this arm (the type exposes no
                // structural-hash accessor).
                //
                // The "lower crate" relationship is NOT the guarantee — a lower
                // crate can still carry a raw `u64` ordinal on a payload (as
                // `SyntheticBinding.value_node` does). The guarantee is the pinned
                // producer surface. If that producer invariant ever changes (e.g.
                // `ResolvedElements` survives the second-engine deletion or a
                // session-origin carrier is threaded in), the guard FIRES and this
                // arm MUST move to an explicit ordinal-free child-descending
                // encoder (like `SyntheticBinding`), not a `Debug` of the ordinal.
                self.push_str(&format!("{elements:?}"));
            }
        }
    }
}

fn named_identity_of(data: &SemanticNodeData) -> Option<NamedIdentity> {
    match data {
        SemanticNodeData::TypeParam {
            decl, display_name, ..
        } => Some(NamedIdentity {
            canonical_id: Arc::clone(&decl.canonical_id),
            symbol_name: Arc::clone(display_name),
            args_fingerprint: decl.whole_hash,
        }),
        _ => None,
    }
}

fn index_key_label(idx: &IndexKey) -> String {
    match idx {
        IndexKey::String(s) => format!("\"{s}\""),
        IndexKey::Number(n) => n.to_string(),
        IndexKey::TypeNode(_) => "<type>".to_string(),
    }
}

// ──────────────────────────────────────────────────────────────────────
// Sort keys — content-only so two hosts deriving the same footprint
// produce byte-identical bytes after serialisation.
// ──────────────────────────────────────────────────────────────────────

fn node_sort_key(rec: &NodeRecord) -> (u32, Hash16, NamedKey) {
    (
        node_kind_discriminant(&rec.kind),
        rec.structural_hash,
        rec.named_identity
            .as_ref()
            .map(named_key)
            .unwrap_or_default(),
    )
}

type NamedKey = (Arc<str>, Arc<str>, Hash16);

fn named_key(id: &NamedIdentity) -> NamedKey {
    (
        Arc::clone(&id.canonical_id),
        Arc::clone(&id.symbol_name),
        id.args_fingerprint,
    )
}

fn node_kind_discriminant(kind: &SemanticNodeKind) -> u32 {
    match kind {
        SemanticNodeKind::DeclAnchor => 0,
        SemanticNodeKind::Instantiated => 1,
        SemanticNodeKind::Alias => 2,
        SemanticNodeKind::Conditional => 3,
        SemanticNodeKind::Union => 4,
        SemanticNodeKind::Intersection => 5,
        SemanticNodeKind::Tuple => 6,
        SemanticNodeKind::Object => 7,
        SemanticNodeKind::Array => 8,
        SemanticNodeKind::Primitive => 9,
        SemanticNodeKind::TypeParam => 10,
        SemanticNodeKind::Opaque => 11,
        SemanticNodeKind::IndexedAccess => 12,
        SemanticNodeKind::KeyOf => 13,
        SemanticNodeKind::TypeOf => 14,
        SemanticNodeKind::Mapped => 15,
        SemanticNodeKind::TemplateLiteral => 16,
        SemanticNodeKind::NormalizeUnion => 17,
        SemanticNodeKind::NormalizeIntersection => 18,
        SemanticNodeKind::Other { .. } => 19,
        // `SemanticNodeKind` is `#[non_exhaustive]`; the catch-all
        // here is a defensive guard for future variants added to the
        // substrate. The miner's sort order is canonicalised so a
        // new variant lands in the highest-discriminant slot until
        // an explicit arm is added.
        _ => 20,
    }
}

fn edge_sort_key(edge: &DerivationEdgeRecord) -> (u32, u32, Vec<u32>) {
    (
        edge.result.0,
        edge_kind_discriminant(edge.kind),
        edge.sources.iter().map(|n| n.0).collect(),
    )
}

fn edge_kind_discriminant(kind: OriginEdgeKind) -> u32 {
    match kind {
        OriginEdgeKind::Instantiate => 0,
        OriginEdgeKind::SubstituteTypeParam => 1,
        OriginEdgeKind::ConditionalSelect => 2,
        OriginEdgeKind::InferBind => 3,
        OriginEdgeKind::ProjectMember => 4,
        OriginEdgeKind::ProjectIndex => 5,
        OriginEdgeKind::ProjectPath => 6,
        OriginEdgeKind::Normalize => 7,
        OriginEdgeKind::AliasResolve => 8,
        OriginEdgeKind::SharedLoadReuse => 9,
    }
}

// ──────────────────────────────────────────────────────────────────────
// Edge translation
// ──────────────────────────────────────────────────────────────────────

fn translate_edge(
    raw: &super::accumulator::DerivationEdgeRaw,
    id_map: &FxHashMap<SemanticNodeId, NodeId>,
) -> DerivationEdgeRecord {
    let result = id_map.get(&raw.result).copied().unwrap_or(NodeId(u32::MAX));
    let sources: Vec<NodeId> = raw
        .edge
        .sources
        .iter()
        .map(|sid| id_map.get(sid).copied().unwrap_or(NodeId(u32::MAX)))
        .collect();
    let kind = translate_edge_kind(raw.kind);
    let meta = translate_meta(raw.kind, &raw.edge.meta);
    DerivationEdgeRecord {
        result,
        kind,
        sources,
        meta,
    }
}

fn translate_edge_kind(k: CoreOriginEdgeKind) -> OriginEdgeKind {
    match k {
        CoreOriginEdgeKind::Instantiate => OriginEdgeKind::Instantiate,
        CoreOriginEdgeKind::SubstituteTypeParam => OriginEdgeKind::SubstituteTypeParam,
        CoreOriginEdgeKind::ConditionalSelect => OriginEdgeKind::ConditionalSelect,
        CoreOriginEdgeKind::InferBind => OriginEdgeKind::InferBind,
        CoreOriginEdgeKind::ProjectMember => OriginEdgeKind::ProjectMember,
        CoreOriginEdgeKind::ProjectIndex => OriginEdgeKind::ProjectIndex,
        CoreOriginEdgeKind::ProjectPath => OriginEdgeKind::ProjectPath,
        CoreOriginEdgeKind::Normalize => OriginEdgeKind::Normalize,
        CoreOriginEdgeKind::AliasResolve => OriginEdgeKind::AliasResolve,
    }
}

fn translate_meta(kind: CoreOriginEdgeKind, meta: &OriginMeta) -> OriginEdgeMetaDto {
    match kind {
        CoreOriginEdgeKind::Instantiate => {
            let type_params = match meta {
                OriginMeta::SubstitutedParam(name) => vec![Arc::clone(name)],
                _ => Vec::new(),
            };
            OriginEdgeMetaDto::Instantiate { type_params }
        }
        CoreOriginEdgeKind::SubstituteTypeParam => OriginEdgeMetaDto::SubstituteTypeParam {
            param_name: match meta {
                OriginMeta::SubstitutedParam(name) => Arc::clone(name),
                _ => Arc::from(""),
            },
            // `substituted_with` is not carried on `OriginMeta` — the
            // miner stores `NodeId(u32::MAX)` as a sentinel meaning
            // "lookup via the edge's source[0]". Walker consumers that
            // need the substituted node should read `edge.sources[0]`.
            substituted_with: NodeId(u32::MAX),
        },
        CoreOriginEdgeKind::ConditionalSelect => {
            let branch = match meta {
                OriginMeta::Branch(BranchSelection::True) => ConditionalBranch::True,
                OriginMeta::Branch(BranchSelection::False) => ConditionalBranch::False,
                _ => ConditionalBranch::Deferred,
            };
            OriginEdgeMetaDto::ConditionalSelect { branch }
        }
        CoreOriginEdgeKind::InferBind => OriginEdgeMetaDto::InferBind {
            param_name: match meta {
                OriginMeta::SubstitutedParam(name) => Arc::clone(name),
                _ => Arc::from(""),
            },
            bound_to: NodeId(u32::MAX),
        },
        // Exhaustive bridge for ProjectMember: producers MUST emit
        // `OriginMeta::ProjectedMember { name, provenance }` (see the
        // four production emit sites in
        // `crates/verter_session/src/project_semantic_dispatch/build.rs`
        // and `…/walk.rs`). A future producer that emits ProjectMember
        // through any other OriginMeta variant is a structural bug and
        // panics here — the Rule-5 validator depends on the provenance
        // being preserved through the bridge.
        CoreOriginEdgeKind::ProjectMember => match meta {
            OriginMeta::ProjectedMember { name, provenance } => OriginEdgeMetaDto::ProjectMember {
                member_name: Arc::clone(name),
                provenance: *provenance,
            },
            other => panic!(
                "ProjectMember edge emitted with non-ProjectedMember OriginMeta variant: {other:?}. \
                 Every ProjectMember producer MUST construct OriginMeta::ProjectedMember with a \
                 typed MemberEdgeProvenance — see the architecture guard \
                 `crates/verter_audit/tests/cases/member_edge_provenance_arch_guard.rs`.",
            ),
        },
        CoreOriginEdgeKind::ProjectIndex => OriginEdgeMetaDto::ProjectIndex {
            index_key: match meta {
                OriginMeta::Index(idx) => Arc::from(index_key_label(idx)),
                _ => Arc::from(""),
            },
        },
        CoreOriginEdgeKind::ProjectPath => OriginEdgeMetaDto::ProjectPath {
            path: match meta {
                OriginMeta::Path(segs) => segs.iter().map(translate_path_segment).collect(),
                _ => Vec::new(),
            },
        },
        // `OriginMeta` carries no normalize-kind tag today. Default to
        // `Simplify`; v2 may split Union/Intersection-driven normalize
        // origins via a richer `OriginMeta` variant.
        CoreOriginEdgeKind::Normalize => OriginEdgeMetaDto::Normalize {
            kind: NormalizeKind::Simplify,
        },
        // Exhaustive bridge for AliasResolve: producers MUST emit
        // `OriginMeta::AliasName(arc)`. `OriginMeta::None` is tolerated
        // here for legacy test-scaffold emissions that pass no payload.
        CoreOriginEdgeKind::AliasResolve => OriginEdgeMetaDto::AliasResolve {
            alias_name: match meta {
                OriginMeta::AliasName(n) => Arc::clone(n),
                OriginMeta::None => Arc::from(""),
                other => panic!(
                    "AliasResolve edge emitted with non-AliasName OriginMeta variant: {other:?}. \
                     Producers must emit OriginMeta::AliasName(arc).",
                ),
            },
        },
    }
}

fn translate_path_segment(seg: &PathSegment) -> ProjectPathSegment {
    match seg {
        PathSegment::Member(name) => ProjectPathSegment::Member {
            name: Arc::clone(name),
        },
        PathSegment::Index(idx) => match idx {
            IndexKey::String(s) => ProjectPathSegment::Index { key: Arc::clone(s) },
            IndexKey::Number(n) => ProjectPathSegment::Index {
                key: Arc::from(n.to_string()),
            },
            IndexKey::TypeNode(_) => ProjectPathSegment::Index {
                key: Arc::from("<type>"),
            },
        },
    }
}

// ──────────────────────────────────────────────────────────────────────
// Helpers for typed-record extraction
// ──────────────────────────────────────────────────────────────────────

fn extract_indexed_ready_builds(events: &[StructuredAuditEvent]) -> Vec<IndexedReadyBuildRecord> {
    let mut out = Vec::new();
    for e in events {
        if let StructuredAuditEvent::IndexedReadyBuilt {
            canonical_id,
            whole_hash,
        } = e
        {
            out.push(IndexedReadyBuildRecord {
                canonical_id: Arc::clone(canonical_id),
                whole_hash: *whole_hash,
            });
        }
    }
    out
}

fn instantiate_decl_canonical(edge: &DerivationEdgeRecord, nodes: &[NodeRecord]) -> Arc<str> {
    // Prefer the named identity carried by the edge's primary source
    // (where the declaration node lives). Falls back to "" when the
    // sources have not been mapped yet (truncation case).
    edge.sources
        .first()
        .and_then(|n| nodes.get(n.0 as usize))
        .and_then(|rec| {
            rec.named_identity
                .as_ref()
                .map(|id| Arc::clone(&id.canonical_id))
        })
        .unwrap_or_else(|| Arc::from(""))
}

fn instantiate_decl_symbol(edge: &DerivationEdgeRecord, nodes: &[NodeRecord]) -> Arc<str> {
    edge.sources
        .first()
        .and_then(|n| nodes.get(n.0 as usize))
        .and_then(|rec| {
            rec.named_identity
                .as_ref()
                .map(|id| Arc::clone(&id.symbol_name))
        })
        .unwrap_or_else(|| Arc::from(""))
}

fn instantiate_args_fingerprint(edge: &DerivationEdgeRecord) -> Hash16 {
    if edge.sources.len() <= 1 {
        return [0u8; 16];
    }
    let mut buf = String::with_capacity(edge.sources.len() * 8);
    for n in &edge.sources[1..] {
        buf.push_str(&n.0.to_string());
        buf.push(',');
    }
    xxh3_128(buf.as_bytes()).to_le_bytes()
}

#[cfg(test)]
#[path = "footprint_miner_tests.rs"]
mod footprint_miner_tests;
