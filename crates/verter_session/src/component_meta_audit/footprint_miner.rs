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
    BranchSelection, IndexKey, OriginEdgeKind as CoreOriginEdgeKind, OriginMeta, PathSegment,
    SemanticNodeData, SemanticNodeId,
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
            let record = build_node_record(data.as_deref());
            (id, record)
        })
        .collect();

    // ── 3. Sort and assign NodeId = index ──────────────────────────
    nodes.sort_by(|a, b| node_sort_key(&a.1).cmp(&node_sort_key(&b.1)));
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

fn build_node_record(data: Option<&SemanticNodeData>) -> NodeRecord {
    let kind = data.map(map_node_kind).unwrap_or(SemanticNodeKind::Opaque);
    let display_label = data
        .map(display_label_for)
        .unwrap_or_else(|| Arc::from("<unknown>"));
    let structural_hash = data.map(structural_hash_of).unwrap_or([0u8; 16]);
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
        SemanticNodeData::TypeOf { .. } => SemanticNodeKind::TypeOf,
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
        SemanticNodeData::TypeOf { value_root, .. } => {
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
    }
}

fn structural_hash_of(data: &SemanticNodeData) -> Hash16 {
    // Xxh3-128 of the Debug rendering. Debug is content-deterministic
    // for these payloads — same content prints the same bytes — so two
    // arenas that intern equivalent content produce equal hashes.
    // Intern-order ids appear inside the Debug rendering only as
    // arena-stable references; for the determinism contract we care
    // about (same-host repeat requests, test
    // `mine_footprint_identical_requests_produce_byte_identical_footprints`)
    // these are themselves stable.
    let dbg = format!("{data:?}");
    xxh3_128(dbg.as_bytes()).to_le_bytes()
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
                 `crates/verter_audit/tests/member_edge_provenance_arch_guard.rs`.",
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
mod tests {
    use super::*;
    use crate::component_meta_audit::accumulator::{
        DerivationEdgeRaw, RequestFootprintAccumulator,
    };
    use crate::semantic_query::OriginEdge;

    fn make_ctx(id: u64) -> Arc<RequestContext> {
        let acc = Arc::new(RequestFootprintAccumulator::new());
        RequestContext::new(id, Arc::from("/x.vue"), true, Some(acc))
    }

    fn synth_edge(
        result_raw: u64,
        sources: &[u64],
        kind: CoreOriginEdgeKind,
        meta: OriginMeta,
    ) -> DerivationEdgeRaw {
        DerivationEdgeRaw {
            result: SemanticNodeId(result_raw),
            kind,
            edge: OriginEdge {
                sources: sources.iter().copied().map(SemanticNodeId).collect(),
                meta,
                edge_dep_signature: Arc::new(
                    Arc::<[(Arc<str>, crate::semantic_query::DepVersion)]>::from(Vec::<(
                        Arc<str>,
                        crate::semantic_query::DepVersion,
                    )>::new(
                    )),
                ),
            },
        }
    }

    fn empty_graph() -> SemanticGraphStore {
        SemanticGraphStore::new()
    }

    #[test]
    fn mine_footprint_empty_state_yields_empty_subgraph_and_zero_counters() {
        let graph = empty_graph();
        let ctx = make_ctx(1);
        let state = AccumulatorState::default();
        let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
        assert_eq!(fp.derivation_subgraph.nodes.len(), 0);
        assert_eq!(fp.derivation_subgraph.edges.len(), 0);
        assert_eq!(fp.cache_outcomes.cold_builds, 0);
        assert!(!fp.graph_completeness.has_orphan_edges);
    }

    #[test]
    fn mine_footprint_cache_outcomes_read_from_per_context_atomic_counters() {
        let graph = empty_graph();
        let ctx = make_ctx(2);
        ctx.cold_builds.store(3, Ordering::Relaxed);
        ctx.warm_hits.store(5, Ordering::Relaxed);
        ctx.joined_waits.store(2, Ordering::Relaxed);
        ctx.sentinels.store(1, Ordering::Relaxed);
        ctx.inflight_aborted_retries.store(7, Ordering::Relaxed);
        ctx.cold_aborts_swept.store(11, Ordering::Relaxed);
        let fp = mine_footprint(
            &graph,
            AccumulatorState::default(),
            &ctx,
            10_000,
            &AuditCaps::default(),
        );
        assert_eq!(fp.cache_outcomes.cold_builds, 3);
        assert_eq!(fp.cache_outcomes.warm_hits, 5);
        assert_eq!(fp.cache_outcomes.joined_waits, 2);
        assert_eq!(fp.cache_outcomes.sentinels, 1);
        assert_eq!(fp.cache_outcomes.inflight_aborted_retries, 7);
        assert_eq!(fp.cache_outcomes.cold_aborts_swept, 11);
    }

    #[test]
    fn mine_footprint_truncates_at_max_derivation_edges_sets_orphan_flag() {
        let graph = empty_graph();
        let ctx = make_ctx(3);
        let mut state = AccumulatorState::default();
        for i in 0..10u64 {
            state.derivation_edges_raw.push(synth_edge(
                i,
                &[i + 100],
                CoreOriginEdgeKind::AliasResolve,
                OriginMeta::None,
            ));
        }
        let fp = mine_footprint(&graph, state, &ctx, 5, &AuditCaps::default());
        assert_eq!(fp.derivation_subgraph.edges.len(), 5);
        assert!(fp.graph_completeness.has_orphan_edges);
        assert_eq!(fp.graph_completeness.edges_truncated, 5);
    }

    #[test]
    fn mine_footprint_identical_inputs_produce_byte_identical_outputs() {
        let graph = empty_graph();
        let ctx_a = make_ctx(1);
        let ctx_b = make_ctx(1);
        let mut state_a = AccumulatorState::default();
        let mut state_b = AccumulatorState::default();
        for i in 0..6u64 {
            state_a.derivation_edges_raw.push(synth_edge(
                i,
                &[i + 100],
                CoreOriginEdgeKind::ProjectMember,
                OriginMeta::ProjectedMember {
                    name: Arc::from(format!("m{i}")),
                    provenance: verter_audit::MemberEdgeProvenance::PathProjection,
                },
            ));
            state_b.derivation_edges_raw.push(synth_edge(
                i,
                &[i + 100],
                CoreOriginEdgeKind::ProjectMember,
                OriginMeta::ProjectedMember {
                    name: Arc::from(format!("m{i}")),
                    provenance: verter_audit::MemberEdgeProvenance::PathProjection,
                },
            ));
        }
        let fp_a = mine_footprint(&graph, state_a, &ctx_a, 10_000, &AuditCaps::default());
        let fp_b = mine_footprint(&graph, state_b, &ctx_b, 10_000, &AuditCaps::default());
        let bytes_a = serde_json::to_vec(&fp_a).expect("serialise a");
        let bytes_b = serde_json::to_vec(&fp_b).expect("serialise b");
        assert_eq!(
            bytes_a, bytes_b,
            "identical inputs must produce byte-identical mined footprints"
        );
    }

    #[test]
    fn mine_footprint_conditional_decisions_distinguish_true_false_deferred() {
        let graph = empty_graph();
        let ctx = make_ctx(4);
        let mut state = AccumulatorState::default();
        state.derivation_edges_raw.push(synth_edge(
            1,
            &[10],
            CoreOriginEdgeKind::ConditionalSelect,
            OriginMeta::Branch(BranchSelection::True),
        ));
        state.derivation_edges_raw.push(synth_edge(
            2,
            &[10],
            CoreOriginEdgeKind::ConditionalSelect,
            OriginMeta::Branch(BranchSelection::False),
        ));
        state.derivation_edges_raw.push(synth_edge(
            3,
            &[10],
            CoreOriginEdgeKind::ConditionalSelect,
            OriginMeta::None,
        ));
        let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
        let branches: Vec<ConditionalBranch> =
            fp.conditional_decisions.iter().map(|c| c.branch).collect();
        assert!(branches.contains(&ConditionalBranch::True));
        assert!(branches.contains(&ConditionalBranch::False));
        assert!(branches.contains(&ConditionalBranch::Deferred));
    }

    #[test]
    fn mine_footprint_alias_resolve_emits_one_record_per_hop() {
        let graph = empty_graph();
        let ctx = make_ctx(5);
        let mut state = AccumulatorState::default();
        for hop in 0..3u64 {
            state.derivation_edges_raw.push(synth_edge(
                hop,
                &[hop + 1],
                CoreOriginEdgeKind::AliasResolve,
                OriginMeta::AliasName(Arc::from(format!("alias_{hop}"))),
            ));
        }
        let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
        assert_eq!(fp.alias_resolutions.len(), 3);
        for rec in &fp.alias_resolutions {
            assert!(rec.alias_name.starts_with("alias_"));
        }
    }

    #[test]
    fn mine_footprint_path_segments_preserve_member_index_distinction() {
        let graph = empty_graph();
        let ctx = make_ctx(6);
        let mut state = AccumulatorState::default();
        let path = [
            PathSegment::Member(Arc::from("a")),
            PathSegment::Index(IndexKey::String(Arc::from("b"))),
            PathSegment::Index(IndexKey::Number(7)),
        ];
        state.derivation_edges_raw.push(synth_edge(
            1,
            &[10],
            CoreOriginEdgeKind::ProjectPath,
            OriginMeta::Path(Arc::from(path)),
        ));
        let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
        assert_eq!(fp.projections.len(), 1);
        let segs = &fp.projections[0].path;
        assert!(matches!(&segs[0], ProjectPathSegment::Member { name } if name.as_ref() == "a"));
        assert!(matches!(&segs[1], ProjectPathSegment::Index { key } if key.as_ref() == "b"));
        assert!(matches!(&segs[2], ProjectPathSegment::Index { key } if key.as_ref() == "7"));
    }

    #[test]
    fn mine_footprint_indexed_ready_builds_extracted_from_structured_events() {
        let graph = empty_graph();
        let ctx = make_ctx(7);
        let mut state = AccumulatorState::default();
        state
            .structured_events
            .push(StructuredAuditEvent::IndexedReadyBuilt {
                canonical_id: Arc::from("/a.ts"),
                whole_hash: [9u8; 16],
            });
        state
            .structured_events
            .push(StructuredAuditEvent::IndexedReadyBuilt {
                canonical_id: Arc::from("/b.ts"),
                whole_hash: [10u8; 16],
            });
        let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
        assert_eq!(fp.indexed_ready_builds.len(), 2);
        assert_eq!(fp.indexed_ready_builds[0].canonical_id.as_ref(), "/a.ts");
        assert_eq!(fp.indexed_ready_builds[1].canonical_id.as_ref(), "/b.ts");
    }

    #[test]
    fn mine_footprint_multiple_derivations_for_same_result_produce_multiple_edges() {
        let graph = empty_graph();
        let ctx = make_ctx(8);
        let mut state = AccumulatorState::default();
        // Two derivations of the same result via different alias hops.
        state.derivation_edges_raw.push(synth_edge(
            42,
            &[10],
            CoreOriginEdgeKind::AliasResolve,
            OriginMeta::AliasName(Arc::from("path_a")),
        ));
        state.derivation_edges_raw.push(synth_edge(
            42,
            &[20],
            CoreOriginEdgeKind::AliasResolve,
            OriginMeta::AliasName(Arc::from("path_b")),
        ));
        let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
        assert_eq!(fp.derivation_subgraph.edges.len(), 2);
        assert_eq!(
            fp.derivation_subgraph.edges[0].result, fp.derivation_subgraph.edges[1].result,
            "both derivations of the same SemanticNodeId must produce the same NodeId result"
        );
    }
}
