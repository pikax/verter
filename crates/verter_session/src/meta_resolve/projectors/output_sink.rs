//! The per-member-publication projectors' TERMINAL output-sink module.
//!
//! This is the ONLY module in the `meta_resolve::projectors` subtree that can
//! MINT [`MetaResolveProjectorsOutputCap`] (the projectors' reverse-
//! materialization capability) and unwrap a sealed carrier to a bare
//! [`TypeExpr`]. The capability's `new` constructor is scoped
//! `pub(in crate::meta_resolve::projectors::output_sink)`, so the projection
//! code in the parent `projectors` module (and its NON-sink helper siblings
//! `macro_payload_substrate`, `published_reducer`, `define_shapes`, `emits`,
//! `exposed`, `options`, `props`, `slots`, `model`) can NAME the cap type (it
//! is re-exported `pub(crate)` from the parent) but CANNOT call `new` — a
//! planted mint there is `E0624`.
//!
//! Every site that touches the reverse boundary lives INSIDE this sink:
//! the raw boundary primitive ([`raise_node_to_sealed_carrier`]) is
//! MODULE-PRIVATE here, and the boundary-consuming reduction / gate / cache work
//! (`member_shape_peek_or_compute`, `reduce_field_value_node`) is sink-private
//! alongside it. The reduce/gate orchestrators decide on node FACTS and pass
//! CARRIERS; only the registered terminal seals materialise, and the published
//! DTO positions carry content-free SOURCES (a resolved leaf projects to its
//! complete closed leaf fact via [`published_source_for_node`]; every richer
//! shape stays the field's shallow source). The sink
//! exposes ONLY policy-complete publication operations that hand back an
//! already-published DTO — [`surface_member_to_expanded_field`] /
//! [`project_model`] return an [`ExpandedField`], and
//! [`reduce_published_field_types`] mutates the published surface in place —
//! NEVER a bare [`TypeExpr`] or a thin boundary helper. A non-sink projector
//! child may call those high-level APIs and receive a published DTO, but cannot
//! pick an arbitrary node / carrier and unwrap it.
//!
//! Each primitive mints the cap from an ALREADY-CONSTRUCTED
//! `&ProjectSemanticDispatch` (so the dispatch construction count — and its
//! non-request-bound bare-engine-construction accounting — stays identical to
//! before the mint relocation) and performs exactly one sealed materialize /
//! seal / unwrap, returning the already-unwrapped value. The capability mint and
//! the carrier unwrap therefore live ONLY in this terminal sink module.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{ExpandedField, ExpansionExecutionStatus};
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};
use verter_type_expr::TypeExpr;

use crate::meta_resolve::exactness::classify_node;
use crate::project_semantic_dispatch::output_materialization::{
    wrap_output_type_expr, MaterializedOutputTypeExpr, OutputProjector,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    DeclIdentity, ProjectionMode, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticOutcome, SemanticQueryKey, SurfaceMember,
};
use crate::types::FileAnalysisSnapshot;

use crate::meta_resolve::dep_signature::emit_dispatch_dep_signature_facts;

pub(crate) use super::published_source::MemberValuePosition;
use super::published_source::{
    published_member_source_upgrade_for_node, published_source_upgrade_for_node,
    structural_member_value_source,
};

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The per-member publication PROJECTORS' output-sink capability. The
    /// projector publication functions in `meta_resolve::projectors` reach this
    /// capability ONLY through the high-level publication APIs in this
    /// `output_sink` module; the capability itself can be MINTED only here. Its
    /// constructor is visible ONLY within
    /// `crate::meta_resolve::projectors::output_sink` (this terminal sink
    /// module) — NOT the parent `projectors` module and NOT its NON-sink helper
    /// children — so a planted `MetaResolveProjectorsOutputCap::new` in a
    /// non-sink helper (e.g. `macro_payload_substrate`) is `E0624`. This
    /// terminal-sink scoping is what makes the output-materialization fence
    /// compiler-enforced rather than convention-based: only a true output sink
    /// can mint.
    pub(crate) struct MetaResolveProjectorsOutputCap;
    mint: pub(in crate::meta_resolve::projectors::output_sink)
}

/// Raw raise of a graph `node` into a SEALED [`MaterializedOutputTypeExpr`]
/// carrier — the node-domain seal the publication gates use after their
/// node-fact decisions have been made. Mints the cap, materialises the node into
/// the sealed `OutputTypeExpr` payload (NO `into_type_expr` — never produces a
/// bare [`TypeExpr`]), and assembles the carrier with the supplied
/// `dep_signature`. A raise miss seals an `Unknown` shell so the per-member slot
/// still publishes a carrier.
///
/// TERMINAL one-shot sink: materialises ONCE and builds the carrier, making NO
/// decision on the materialised value. The node-fact gate decisions happen on the
/// `node` BEFORE this is called, so the gates never touch a materialised
/// `TypeExpr`. The carrier `node_id` is the producing `node`, so a downstream
/// consumer reads node facts off it (e.g. the root-sentinel gate) instead of
/// re-materialising.
fn raise_node_to_sealed_carrier(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    dep_signature: crate::semantic_query::DepSignature,
) -> MaterializedOutputTypeExpr {
    let cap = MetaResolveProjectorsOutputCap::new(dispatch);
    let sealed = match cap.materialize_output_type_expr(node) {
        Some(sealed) => sealed,
        None => wrap_output_type_expr(&cap, TypeExpr::Unknown { raw: String::new() }),
    };
    MaterializedOutputTypeExpr::from_parts(Some(node), sealed, dep_signature, false)
}

/// The publication SOURCE for a reduced field value: a node that resolved to
/// a complete LEAF (primitive / string / number / boolean literal) projects to
/// its closed leaf fact — the one node class whose fact is complete by itself;
/// any richer resolved shape publishes the field's existing content-free
/// source unchanged (shallow-by-default: the consumer re-raises it through the
/// one engine on demand — the reduction above already warmed the shared
/// dispatch memos).
/// The owner's authored import-binding alias for a PACKAGE-backed
/// declaration-reference carrier: the local binding name whose forward
/// export-target resolution lands on the carrier's declaration identity.
/// `None` when the carrier is not package-backed or no owner binding
/// resolves to it (the caller keeps the authored annotation slot).
///
/// Typed-domain only: the owner's analyzed import bindings + the shared
/// shallow export-target resolver — no raw-text recovery.
fn authored_package_alias_for_carrier(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    node: SemanticNodeId,
) -> Option<String> {
    let data = crate::project_semantic_dispatch::node_data_for(ctx, node)?;
    let crate::semantic_query::SemanticNodeData::DeclRef { identity } = data.as_ref() else {
        return None;
    };
    if !ctx.workspace_is_package_backed(identity.canonical_id.as_ref()) {
        return None;
    }
    let snapshot = ctx.get_raw_analysis_snapshot(owner_canonical)?;
    snapshot.imports.iter().find_map(|import| {
        let target = import.resolved_canonical_id.as_deref()?;
        if !ctx.workspace_is_package_backed(target) {
            return None;
        }
        import.bindings.iter().find_map(|binding| {
            let imported = binding.imported_name.as_deref().unwrap_or(&binding.name);
            let (resolved_id, resolved_name) =
                ctx.resolve_named_type_export_target_shallow(target, imported)?;
            (resolved_id == identity.canonical_id.as_ref()
                && resolved_name == identity.decl_name.as_ref())
            .then(|| binding.name.clone())
        })
    })
}

/// NODE-start per-field reducer for the publication finaliser: run the shallow
/// gates in NODE DOMAIN off the raised input node, reduce through the
/// graph-native member-value reducer when the gates clear, and apply the
/// input-side no-poison gate off the SAME observed input node.
///
/// Gate order (all node-domain, no `TypeExpr` materialised for a decision):
///
/// 1. package-backed object-like root → publish the input carrier verbatim
///    (shallow-by-default);
/// 2. generic-instantiation transitive-cycle root → publish the input carrier;
/// 3. no reducible operator AND not a generic instantiation → publish the
///    input carrier (nothing to reduce);
/// 4. reduce via [`reduce_member_value_graph_native_with_context`] under the
///    node-derived reduction context;
/// 5. no-poison: when the reduction's ROOT came back as the unmaterialised
///    sentinel and the INPUT node confidently carried no semantic miss (the
///    whole-tree miss fact read off the SAME input node the caller raised —
///    one observed lowering, no second live lower), the input carrier IS the
///    published shape.
fn reduce_field_value_node(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    input_node: SemanticNodeId,
    publish_mode: ProjectionMode,
) -> MaterializedOutputTypeExpr {
    let ctx: &dyn ResolverContext = query_engine.ctx;
    let dispatch = ProjectSemanticDispatch::new(ctx);

    let (package_backed, _fence) =
        crate::meta_resolve::node_package_backed_object_like_root_with_fence(
            query_engine,
            scope_canonical_id,
            input_node,
        );
    if package_backed {
        return raise_node_to_sealed_carrier(&dispatch, input_node, Arc::from(Vec::new()));
    }
    let gates = super::classify_node_reduction_gates(ctx, input_node);
    if gates.generic_instantiation_ref {
        let (reaches_cycle, _fence) =
            crate::meta_resolve::node_root_reaches_transitive_cycle_with_fence(
                ctx,
                scope_canonical_id,
                input_node,
            );
        if reaches_cycle {
            return raise_node_to_sealed_carrier(&dispatch, input_node, Arc::from(Vec::new()));
        }
    }
    if !(gates.contains_reducible_operator || gates.generic_instantiation_ref) {
        return raise_node_to_sealed_carrier(&dispatch, input_node, Arc::from(Vec::new()));
    }

    let context = crate::meta_resolve::materialize::node_materialize_reduction_context(
        ctx,
        input_node,
        publish_mode,
    );
    let reduced = crate::meta_resolve::materialize::reduce_member_value_graph_native_with_context(
        ctx,
        scope_canonical_id,
        input_node,
        context,
    );

    let root_is_sentinel = reduced.node_id().is_some_and(|node| {
        crate::project_semantic_dispatch::raise::node_root_is_unmaterialized_sentinel_with_dispatch(
            &dispatch, node,
        )
    });
    if root_is_sentinel
        && crate::project_semantic_dispatch::raise::node_contains_semantic_miss_with_dispatch(
            &dispatch, input_node,
        ) == Some(false)
    {
        return raise_node_to_sealed_carrier(&dispatch, input_node, Arc::from(Vec::new()));
    }
    reduced
}

/// Default model property name when `defineModel()` is called without
/// an explicit name argument.
const DEFAULT_MODEL_NAME: &str = "modelValue";

/// Peek-before-raise per-member helper.
///
/// Wraps the cold compute path for one `(scope, member, mode)`
/// triple around the host-owned per-member slot of
/// [`crate::component_meta_caches::ShapeCacheDb`] (indexed by
/// [`crate::component_meta_caches::ShapeSubject::MemberValueNode`] via
/// `ShapeCacheKey::surface_member_value_whole_with_context`). The contract:
///
///  1. **Peek first.** Warm hits return the cached
///     [`crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr`]
///     WITHOUT paying any raise or gate cost — the goal of the cache
///     is that the per-member hot path returns in `peek` time.
///  2. **Cold path runs NODE-domain gates, then seals once.** A cold miss
///     runs the same shallow gates `reduce_field_value_node` runs,
///     but in NODE DOMAIN off `member_value` directly — never by materialising
///     a `TypeExpr` first: `node_package_backed_object_like_root_with_fence` +
///     `node_root_reaches_transitive_cycle_with_fence` +
///     `classify_node_reduction_gates`. A gate-stop publishes the shallow
///     carrier via the node→carrier terminal `raise_node_to_sealed_carrier`
///     (one terminal materialize, no decide on the result).
///  3. **Gate-rejected outcomes do NOT admit.** The node is sealed into a
///     `MaterializedOutputTypeExpr` carrier and returned verbatim. Admitting a
///     gate-rejected entry would store the input shape verbatim — the cache
///     would grow for no compute win, since the node-domain gates are cheap to
///     re-run.
///  4. **Cold compute is single-shot.** When a reduction is required,
///     `reduce_member_value_graph_native` runs ONCE
///     (single-compute pattern). The cache's
///     `get_or_compute` closure captures the pre-computed
///     `MaterializedOutputTypeExpr` by move; if the fact signature cannot
///     be built (no tear-free scope observation or a
///     `RouteGeneration`-tagged dep), admission is refused but the
///     pre-computed value is still returned to the caller — no second
///     reducer call.
fn member_shape_peek_or_compute(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    admitted: &super::publication_authority::AdmittedPublishedMember<'_>,
    mode: ProjectionMode,
) -> crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr {
    // ONE cacheability tracer scope around the WHOLE compute — the reduction-context
    // classification that builds the cache KEY, the node-domain gates, the peek, and
    // the cold reduce. Every admission below consults its `CacheabilityProbe`, which
    // the `ShapeCacheDb` funnel REQUIRES, so no read point in this producer can lie
    // outside a traced scope and no arm can drop the verdict. Scoping the tracer to
    // the reduce alone left the gates' and the key-classification's serves
    // unobserved.
    let outer_ctx: &dyn ResolverContext = query_engine.ctx;
    let cache = outer_ctx.project_type_store().shape_cache_db();
    let value = cache.with_owner_scope(outer_ctx, |scope| {
            // The admitted member's value graph node is the raise/reduce subject; the
            // cache key keys on the ADMITTED member (`admitted.member().value`) so an
            // arbitrary / unadmitted `SemanticNodeId` cannot be routed through the
            // sealed shape subject.
            let member_value = admitted.member().value;
            let ctx: &dyn ResolverContext = query_engine.ctx;
            // Publication sink: one dispatch for the cold-path raise and every
            // sealed-carrier assembly below. The capability mint + unwrap are the
            // module-private primitives of this terminal `output_sink` sink — they
            // mint from this already-constructed dispatch.
            let dispatch = ProjectSemanticDispatch::new(ctx);
            // Key the per-member MemberValueNode slot by the EXACT reduction
            // context the cold path reduces under
            // (`node_materialize_reduction_context(ctx, member_value, mode)` —
            // `Published(Navigate)` when the member's raised root is a published
            // operator, since the explicit member demand IS consumer demand for a
            // closed whole-utility terminal; `StructuralTransit` for every other
            // `Navigate`; `Published` otherwise). A bare `published(mode)` key
            // collided a transit-lowered carrier publication with a published
            // consumer over the same `(scope, node)`.
            let member_reduction_context =
                crate::meta_resolve::materialize::node_materialize_reduction_context(
                    ctx,
                    member_value,
                    mode,
                );
            let key = crate::component_meta_caches::ShapeCacheKey::surface_member_value_whole_with_context(
        Arc::<str>::from(scope_canonical_id),
        admitted,
        member_reduction_context,
    );

            // (1) Peek FIRST — warm path pays zero raise/gate cost. The cached
            // entry's dep_signature must be re-emitted into the active fact
            // tracer + dispatch dep-signature accumulator so the request's
            // dep set sees the same facts the cold compute emitted.
            if let Some(cached) = scope.peek(&key) {
                emit_dispatch_dep_signature_facts(ctx, cached.dep_signature());
                return cached;
            }

            // (2) Node-domain package-backed gate on `member_value` — decided from the
            // node's root identity (Pick/Omit source-root trap + indexed-access roots
            // handled), NEVER by materialising the value first. Gates run BEFORE any
            // reduction: `MaterializeMemoDb` is shared with the typed-IR materialiser
            // callers (model / registry candidate materialisation) which do not apply
            // these projector shallow gates, so honouring the gates first publishes a
            // package-backed root (`External['x']`) as the shallow carrier the
            // shallow-by-default rule requires.
            //
            // The gate's cross-file fence is threaded into the admit so an edit to the
            // package-backing declaration file invalidates the gate-shortcut entry.
            // The fence is `Option<DepSignature>`; `None` means "refuse shared
            // admission" (a contributing canonical's `authoritative_current_content_hash`
            // was unavailable, so the verdict cannot be rooted on the file state it was
            // decided against) — the caller returns the carrier verbatim without admitting.
            //
            // NON-CACHEABILITY: the fence is HASH-AVAILABILITY, not publication status — a
            // FENCED (ReturnOnly, `store_published == false`) serve WITH an available
            // content hash passes it. The gate itself CONSUMES such serves: it resolves the
            // member value's carrier head through the shared carrier resolver
            // (`node_root_identity` -> `resolve_carrier_subject_node`), which rides
            // `ensure_indexed_ready_serve`. The enclosing cacheability scope already covers
            // it — as it covers the key classification, the peek, the cycle BFS, the shell
            // raises, and the cold reduce — so every admission arm below reads ONE verdict
            // off `probe` instead of stitching per-step tracer bits together.
            let (route_is_package_backed, package_backed_fence_opt) =
                crate::meta_resolve::node_package_backed_object_like_root_with_fence(
                    query_engine,
                    scope_canonical_id,
                    member_value,
                );
            if route_is_package_backed {
                // Package-backed roots stay shallow carriers. Admit so sibling members of
                // the same package-backed parent reuse the verdict at peek time.
                let Some(package_backed_fence) = package_backed_fence_opt else {
                    return raise_node_to_sealed_carrier(
                        &dispatch,
                        member_value,
                        Arc::from(Vec::new()),
                    );
                };
                let value =
                    raise_node_to_sealed_carrier(&dispatch, member_value, package_backed_fence);
                return admit_member_shape_if_possible(ctx, &key, value, &scope);
            }
            // Non-package-backed: the gate returns `Some(empty)` unless a contributing
            // canonical's authoritative hash was unavailable mid-gate, in which case it
            // refuses (`None`) and the carrier is returned without admission.
            let Some(package_backed_fence) = package_backed_fence_opt else {
                return raise_node_to_sealed_carrier(
                    &dispatch,
                    member_value,
                    Arc::from(Vec::new()),
                );
            };

            // (3) Node reduction gates — the leaf / bare-carrier / generic-instantiation /
            // reducible-operator facts read off `member_value` directly (no materialise).
            let gates = super::classify_node_reduction_gates(ctx, member_value);

            // (4) Cycle gate — only a generic instantiation can reach a transitive cycle,
            // so the BFS fires lazily on that fact. A recursive parameterised helper stays
            // a shallow carrier (the cycle prevents finite reduction); admit so subsequent
            // peeks skip the BFS. The BFS resolves carrier heads through the same shared
            // resolver; the enclosing cacheability scope observes those serves.
            let cycle_fence: crate::semantic_query::DepSignature = if gates
                .generic_instantiation_ref
            {
                let (reaches_cycle, fence) =
                    crate::meta_resolve::node_root_reaches_transitive_cycle_with_fence(
                        ctx,
                        scope_canonical_id,
                        member_value,
                    );
                if reaches_cycle {
                    // Combine both gate fences (package-backed + cycle BFS) so the
                    // admit's `fact_dep_signature` invalidates on edits to any visited
                    // declaration file.
                    let combined_fence =
                        combine_dep_signatures(&package_backed_fence, &fence, scope_canonical_id);
                    let value =
                        raise_node_to_sealed_carrier(&dispatch, member_value, combined_fence);
                    return admit_member_shape_if_possible(ctx, &key, value, &scope);
                }
                fence
            } else {
                Arc::from(Vec::new())
            };

            // The carrier-stop decision lives on the dispatch-layer reduction-demand
            // context, NOT on a projector-side name predicate. A generic instantiation
            // enters the reducer; the dispatch carrier-stops downstream operators when the
            // context does not admit reduction.
            let needs_reduction =
                gates.contains_reducible_operator || gates.generic_instantiation_ref;
            // The combined gate fence is threaded through the remaining admit paths so the
            // cache entries do not self-root on the scope file only.
            let gate_fence =
                combine_dep_signatures(&package_backed_fence, &cycle_fence, scope_canonical_id);
            if !needs_reduction {
                // Universal-caching invariant: a non-reducible shape (primitive / literal /
                // bare alias / closed object / function / union / intersection without
                // operator nodes) is a STABLE shape — admit it as the shallow carrier so
                // sibling members hitting the same `SurfaceMember.value` short-circuit at
                // peek time.
                let value = raise_node_to_sealed_carrier(&dispatch, member_value, gate_fence);
                return admit_member_shape_if_possible(ctx, &key, value, &scope);
            }

            // (5) Cold compute via the graph-native reducer. Single-shot —
            // pre-computed ONCE outside the cache call (single-compute
            // pattern). The cache's `get_or_compute` closure either captures
            // and moves the pre-computed `materialized` into the cache entry,
            // or returns `None` (signature-refusal) — in either case the
            // pre-computed value is the correct answer; no second reducer
            // call.
            //
            // The reducer uses the same demand context as the node-start
            // materializer: `Expanded` remains whole-surface publication; a
            // per-prop `Navigate` whose raised root is a published operator
            // (`Pick`/`Omit`/`IndexedAccess`/...) reduces under
            // `Published(Navigate)` — the explicit member demand IS consumer
            // demand, so a closed whole-utility terminal materialises its named
            // keys path-precisely; every other per-prop `Navigate` stays a
            // structural-transit carrier publication that does not enumerate
            // mapped/keyof interiors. The cache `key` uses this SAME context so
            // carrier publication does not collide with a published consumer
            // slot over the same `(scope, node)`.
            let reduction_context =
                crate::meta_resolve::materialize::node_materialize_reduction_context(
                    ctx,
                    member_value,
                    mode,
                );
            // The cold reduce runs inside the caller's cacheability scope, alongside every
            // other read this producer makes. A FENCED (ReturnOnly, `store_published ==
            // false`) `IndexedReady` serve consumed anywhere in the compute derives this
            // member SHAPE from a served-without-publication basis while its fact signature
            // validates against the LIVE view — a non-cacheable read the
            // `MaterializedOutputTypeExpr` `result_is_partial()`-only admission gate cannot
            // reject (a fenced serve is non-cacheable but NOT partial), so a fenced-but-
            // `Complete` shape would otherwise stale-serve a later same-generation warm hit.
            //
            // The admit below builds its own signature from the carrier's `dep_signature`,
            // NOT from a tracer's finalised set, so the boundary reads the scope's
            // CACHEABILITY verdict — which folds the non-cacheable-read bit together with a
            // fact-signature overflow (a second, INDEPENDENT non-admission condition that
            // must not be dropped here).
            let materialized =
                crate::meta_resolve::materialize::reduce_member_value_graph_native_with_context(
                    ctx,
                    scope_canonical_id,
                    member_value,
                    reduction_context,
                );
            let observed_scope = ctx.observe_materialize_scope(scope_canonical_id);
            // Merge the gate fence into the materialised entry's dep
            // signature so the cold-path admit's `fact_dep_signature` also
            // captures the gates' cross-file observations. Without this, the
            // cold-path admit would self-root only on `scope` + the reducer's
            // observed deps, missing gate-only deps (e.g., package-backed
            // declaration scope) that should invalidate.
            let materialized_with_gate_fence = merge_gate_fence_into_materialized(
                materialized.clone(),
                &gate_fence,
                scope_canonical_id,
            );
            // Local early return: a GENUINE-partial member shape (keyed on the
            // value's OWN `result_is_partial` — a budget-tripped contributing read
            // folds its partiality onto this value via the per-cold-compute
            // completeness scope) must NOT enter the shared shape cache. The gate
            // is PURE over `result_is_partial`; it does NOT OR-in a request-global
            // partial sticky. The central `get_or_compute` gate also refuses, but
            // returning here keeps the per-member producer from depending on the
            // cache layer remembering the rule and skips the futile admission
            // plumbing.
            if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                materialized_with_gate_fence.result_is_partial(),
            ) || scope.non_cacheable()
            {
                // A non-cacheable read was consumed ANYWHERE in this compute — the key
                // classification, a gate whose fence roots this entry, or the reduce: the
                // value flows to the caller, but the shared `ShapeCacheDb` slot is NOT
                // written; the next request recomputes cold and revalidates against the
                // then-live view. (The `get_or_compute` funnel refuses independently; this
                // early return skips the futile admission plumbing.)
                return materialized_with_gate_fence;
            }
            let materialized_for_closure = materialized_with_gate_fence.clone();
            let admitted = scope.get_or_compute(&key, move || {
        let scope_obs = observed_scope?;
        let parse_fact = scope_obs.syntactic_export_set.clone()?;
        match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
            &scope_obs,
            parse_fact,
            materialized_for_closure.dep_signature(),
        ) {
            crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                Some((materialized_for_closure, sig.facts))
            }
            crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
        }
    });
            admitted.unwrap_or(materialized_with_gate_fence)
        });
    value
}

/// Combine two `DepSignature` slices, dropping duplicate
/// `(canonical, DepVersion)` entries and the scope's self-entry (the
/// scope is self-rooted by `engine_fact_signature_for_materialize_memo`).
///
/// Order is preserved (first occurrence wins) so the resulting
/// signature is deterministic given deterministic inputs.
fn combine_dep_signatures(
    a: &crate::semantic_query::DepSignature,
    b: &crate::semantic_query::DepSignature,
    scope_canonical_id: &str,
) -> crate::semantic_query::DepSignature {
    let mut out: Vec<(Arc<str>, crate::semantic_query::DepVersion)> =
        Vec::with_capacity(a.len() + b.len());
    let mut seen: rustc_hash::FxHashSet<(Arc<str>, crate::semantic_query::DepVersion)> =
        rustc_hash::FxHashSet::default();
    for entry in a.iter().chain(b.iter()) {
        if entry.0.as_ref() == scope_canonical_id || entry.0.as_ref().is_empty() {
            continue;
        }
        let pair = (Arc::clone(&entry.0), entry.1.clone());
        if seen.insert(pair.clone()) {
            out.push(pair);
        }
    }
    Arc::from(out.into_boxed_slice())
}

/// Append the gate fence's dep entries to the materialised
/// `MaterializedOutputTypeExpr.dep_signature`, deduplicating against the
/// already-observed entries. Used on the cold-compute admit path so
/// the entry's fact signature captures BOTH the reducer's observed
/// deps AND the gate-observed deps.
fn merge_gate_fence_into_materialized(
    mut materialized: crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr,
    gate_fence: &crate::semantic_query::DepSignature,
    scope_canonical_id: &str,
) -> crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr {
    if gate_fence.is_empty() {
        return materialized;
    }
    let combined =
        combine_dep_signatures(materialized.dep_signature(), gate_fence, scope_canonical_id);
    materialized.set_dep_signature(combined);
    materialized
}

/// Admit a freshly-computed SemanticNode-subject shape into the
/// universal [`crate::component_meta_caches::ShapeCacheDb`] when the
/// scope has a tear-free `observe_materialize_scope` observation.
///
/// Universal-caching invariant: every successful `(node, scope, mode)`
/// shape compute admits so sibling members and future peeks return
/// the cached value rather than re-paying the raise + gate cost.
///
/// Falls through to returning the value verbatim when the
/// observation is unavailable (session tombstone / evicted scope /
/// no recoverable `IndexedReady`) — without a view-correct scope
/// identity to self-root, admitting would mis-root the entry. This
/// is the documented degradation path; the caller still receives
/// the same value the cold compute produced.
/// Universal-caching admission for the projector pipeline. Computes
/// the `fact_dep_signature` from the value's `dep_signature` + scope
/// observation, then delegates to
/// [`crate::component_meta_caches::ShapeCacheDb::admit_computed`] —
/// the single centralised admission point that handles the
/// `get_or_compute` invocation and the verbatim fallback when
/// admission is refused.
///
/// Returns the input `value` verbatim when:
/// - the enclosing cacheability scope reports NON-CACHEABLE (`probe`): a FENCED
///   (ReturnOnly, `store_published == false`) `IndexedReady` serve, a broken
///   decl-body lease, an unrootable route, an unobservable source env, or a
///   fact-signature overflow was consumed ANYWHERE in the producing compute —
///   the key classification, a gate verdict, or the carrier raise. Such a
///   value's fact stamps read the LIVE view while its payload came from a
///   served-without-publication / unrootable basis, so no read-side rail can
///   reject the entry once it is warm;
/// - the scope observation cannot be obtained (no scope view);
/// - the scope has no `syntactic_export_set` parse fact;
/// - the engine fact-signature builder refuses (overflow / missing
///   provenance).
///
/// In all refusal cases the caller receives the same
/// `MaterializedOutputTypeExpr` it computed — admission is best-effort. Refusal
/// is CACHE-ONLY: the shape stays `Complete` and is never marked `Partial`.
fn admit_member_shape_if_possible(
    ctx: &dyn ResolverContext,
    key: &crate::component_meta_caches::ShapeCacheKey,
    value: crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr,
    owner_scope: &crate::component_meta_caches::ShapeCacheOwnerScope<'_, '_>,
) -> crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr {
    // The producing compute consumed a non-cacheable read (or overflowed its
    // signature): serve the value, publish nothing. `package_backed_fence_opt` —
    // the arms' pre-existing gate — is only hash AVAILABILITY, so a fenced serve
    // with an available hash passes it; this is the rail that catches it. (The
    // `get_or_compute` funnel refuses independently; the early return skips the
    // futile signature plumbing.)
    if owner_scope.non_cacheable() {
        return value;
    }
    // Local early return: refuse a GENUINE-partial shape before any
    // admission plumbing. The central `admit_computed` → `get_or_compute`
    // gate also refuses, but the early return keeps this single
    // centralised admission point from depending on the cache layer
    // remembering the rule. The value is returned verbatim.
    if crate::cache_runtime::refuse_result_cache_admission_if_partial(value.result_is_partial()) {
        return value;
    }
    let scope = key.scope_canonical().clone();
    let Some(observed_scope) = ctx.observe_materialize_scope(scope.as_ref()) else {
        return value;
    };
    let Some(parse_fact) = observed_scope.syntactic_export_set.clone() else {
        return value;
    };
    let fact_sig = match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
        &observed_scope,
        parse_fact,
        value.dep_signature(),
    ) {
        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => sig.facts,
        crate::cache_runtime::SignatureAdmission::NonCacheable(_) => return value,
    };
    owner_scope.admit_computed(key, value, fact_sig)
}

/// Build an [`ExpandedField`] for a single surface member.
///
/// Raises the member's value node back to a [`TypeExpr`] (falling back
/// to `TypeExpr::Unknown` if raise fails), classifies its exactness
/// through the shared [`classify_node`] predicate, then runs the
/// bounded fixed-point reducer on the raised expression so nested
/// `IndexedAccess` chains collapse to concrete leaves.
///
/// `raw_type` is taken from the parser's `analyzed_prop.type_annotation`
/// when available. The caller passes `None` when no analyzed prop
/// matches the surface member's name.
///
/// The member's value is also resolved through one additional
/// `ProjectPath { mode: Shallow }` so that `DeclRef` carriers
/// (the terminal Navigate-mode form for unparameterised type
/// aliases) collapse to their underlying primitive / object /
/// function shape. Without this hop, `defineProps<{ msg: MyStr }>`
/// where `type MyStr = string` would publish `msg` as
/// `ExactSymbolic`.
///
/// The bounded fixed-point reducer
/// ([`materialize_component_meta_type_expr_until_stable`]) makes
/// the projector self-sufficient for nested `IndexedAccess` shapes
/// (e.g. `Pick<Foo, 'a'>['a']['nested']`). Generic substitutions
/// travel through the dispatch `lower → raise_and_reduce` pipeline
/// inside the reducer; cache keys include the relevant scope / expr
/// / mode tuple, dep_signature is accumulated into the per-request
/// thread-local accumulator, and any dispatch fence
/// `MacroExpansionDiagnostics` flow through the same accumulator
/// the projector's other dispatches use.
pub(crate) fn surface_member_to_expanded_field(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    admitted: &super::publication_authority::AdmittedPublishedMember<'_>,
    raw_type: Option<String>,
    shallow_payload: Option<verter_type_expr::locators::MacroPayloadLocator>,
    type_arg_base: Option<&verter_type_expr::locators::MacroPayloadLocator>,
    value_position: MemberValuePosition,
) -> ExpandedField {
    // The publication subject is the policy-admitted token: the sink reads the
    // member + its descended cursor FROM the token, never from a forgeable
    // `(&SurfaceMember, ProjectionCursor)` pair. Admission already enforced
    // public visibility, the derived-kind/cursor match, `descend_published_member`
    // success, and the recorded published-field edge.
    let member: &SurfaceMember = admitted.member();
    let ctx: &dyn ResolverContext = query_engine.ctx;
    // The publication mode comes from the admitted token's descended member
    // cursor. `Navigate` (carrier) mode means the member's type body is
    // published as a carrier `Ref`, not breadth-expanded.
    let publish_mode = admitted.cursor().terminal_publication_mode();
    let carrier_mode = matches!(publish_mode, ProjectionMode::Navigate);
    // Exactness classification is independent of the member's reduced
    // TypeExpr; it walks the member's resolved-value graph. Keep it
    // isolated in its own dispatch scope so the peek-before-raise
    // contract for the type reduction is not coupled to a TypeExpr
    // raise that exactness does not need. In carrier mode the
    // classification does NOT expand a generic instantiation to its
    // object surface — that would re-open the breadth leak.
    let exactness = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let resolved_value =
            resolve_member_value_for_classification(&dispatch, member.value, carrier_mode);
        classify_node(&dispatch, resolved_value)
    };
    // Peek the per-member graph-native materialiser cache BEFORE any
    // `raise_node_to_type_expr(member.value)` call. Warm hits return
    // the cached `MaterializedOutputTypeExpr` without paying the raise cost
    // or the shallow-gate cost; cold misses raise once, run the
    // gates, then dispatch the graph-native reducer + admit.
    //
    // `publish_mode` (from the admitted token's descended cursor, computed
    // above) drives the per-member materialise. `Navigate` keeps a generic
    // instantiation `Tool<INPUT, OUTPUT>` as a `Ref` carrier instead
    // of breadth-enumerating `Tool`'s own members into the published
    // surface — Rule-5 shallow-by-default depth gate.
    let materialized =
        member_shape_peek_or_compute(query_engine, scope_canonical_id, admitted, publish_mode);
    // Publication: the member's content-free SOURCE POSITION — its authored
    // shallow payload position when the analyzer stamped one, upgraded to
    // the complete closed leaf fact / declaration-identity carrier when the
    // reduced member value resolved to one. The reduced carrier's node is
    // consumed for the leaf projection only; no `TypeExpr` is unwrapped.
    // With NO authored position and NO upgrade, the classification is the
    // member's VALUE-POSITION requirement: a REQUIRED payload position (an
    // emit member) is the typed source-construction FAILURE — never a
    // fabricated `unknown` success.
    let shallow_source =
        shallow_payload.map(verter_type_expr::locators::AuthoredBodyLocator::MacroPayload);
    let authored_source = shallow_source
        .clone()
        .map(verter_type_expr::facts::SemanticTypeSource::Authored);
    let r#type = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        // A PAYLOAD-LESS member whose reduced value is an ARGUMENT-BEARING
        // named reference (an imported / heritage-reached
        // `message: MessageBase<string>`) publishes the arg-preserving
        // authored USE-SITE carrier: the declaring decl's member-value slot,
        // whose deref replays the instantiation WITH its type arguments
        // through the one shared dispatch. Content-free and NON-EXECUTED.
        // Recovery fails closed to the source upgrades below.
        let use_site_source = if shallow_source.is_none() {
            materialized
                .node_id()
                .and_then(|node| {
                    crate::meta_resolve::arg_preserving_member_use_site_slot(
                        &dispatch,
                        member.name.as_ref(),
                        member.declaration_origin.as_deref(),
                        node,
                    )
                })
                .map(|slot| {
                    verter_type_expr::facts::SemanticTypeSource::Authored(
                        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot),
                    )
                })
        } else {
            None
        };
        match use_site_source {
            Some(source) => verter_type_expr::facts::SourcePosition::Present(source),
            // The lossy `InstantiationRef` upgrade applies only to a
            // PAYLOAD-LESS resolved-surface member whose use-site slot was
            // NOT recoverable; an authored position keeps its authored
            // source (the raise reproduces the full instantiation, arguments
            // included). The lossless `DeclRef` / leaf upgrades always apply
            // — an authored operator position (`Theme['header']`) whose
            // publication reduce navigated to the terminal declaration
            // carrier publishes THAT carrier ("the declaration reference
            // survives to the published surface"); a reduce that kept a
            // symbolic / package-backed / carrier-stop shape keeps the
            // authored source unchanged.
            None => match published_member_source_upgrade_for_node(
                &dispatch,
                materialized.node_id(),
                shallow_source.is_none(),
            )
            .or(authored_source)
            {
                Some(source) => verter_type_expr::facts::SourcePosition::Present(source),
                // No authored position, no use-site slot, no upgrade on the
                // published node: the STRUCTURAL member-source projection. A
                // member value with a valid structural replay address (an
                // imported function / inline object / rich tuple / array /
                // composite / instantiation) publishes its faithful shallow
                // carrier — the closed/ref upgrade on the admitted node, or
                // the projected MEMBER-PATH replay route off the macro's
                // stamped type-argument base (the consumer re-resolves it on
                // demand through the one shared dispatch; nothing flattens
                // eagerly). ONLY a genuine miss stays the typed
                // source-construction FAILURE: an operationally partial
                // materialization (a torn read is never a replay-address
                // proof), an unknown-materializing resolver failure carrier,
                // or a structural value with no stamped base to replay off.
                // A stable unresolved reference/import is itself a faithful
                // Complete carrier. There
                // is no proven-open producer position on this arm: every
                // member enumerated from a type-based macro surface carries a
                // REQUIRED value-type position (runtime/unannotated positions
                // are separately typed `Absent` at their producers). Emit
                // PAYLOAD sources never land here — the normalized
                // `ResolvedEmitField.payload_source` rows (closed tuple /
                // member-path / callable-params replay) own them.
                None => match value_position {
                    MemberValuePosition::ShallowMember => {
                        // Source construction is anchored in the admitted
                        // member's original carrier node, not the reduced
                        // result node. A terminal reduction may legitimately
                        // stop on an `Opaque(RecursiveRef)` or projection miss
                        // while the authored member path remains a faithful,
                        // replayable address. Partiality is retained on the
                        // materialized outcome and still refuses cache
                        // admission; it does not erase the best safe carrier.
                        let structural = structural_member_value_source(
                            &dispatch,
                            member.value,
                            member.name.as_ref(),
                            type_arg_base,
                        );
                        match structural {
                            Some(source) => {
                                verter_type_expr::facts::SourcePosition::Present(source)
                            }
                            None => verter_type_expr::facts::SourcePosition::Failed(
                                verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredMemberValue,
                            ),
                        }
                    }
                },
            },
        }
    };
    // Provenance downgrade through transparent carriers: a member reached
    // ONLY via REAL heritage (`extends PlainProps` / `extends Vendor`) is NOT an
    // own-body member of the macro type argument, so it MUST carry
    // `declared_in_macro_type_arg = false` even though the macro-T own-body
    // synthesis can over-stamp the raw bit `true` on a heritage-reached member.
    // The `merge_role` is INDEPENDENTLY baked per arm (`Heritage` for
    // `extends`-reached, `OwnBody` for the declaration's own body), so it is the
    // authoritative discriminator. This is the SAME downgrade
    // `props_from_typeinfo_surface` applies on the DTO path — applying it here
    // keeps the flat `evaluated_types.props` field (which `define_props_shape`
    // reads first) in agreement, so an own-body member keeps `true` and a
    // heritage-reached member downgrades to `false`. NOT
    // `source_field.unwrap_or(false)`: that would also strip own-body members
    // (the cross-file-simple discriminating positive test rejects that accident).
    let declared_in_macro_type_arg = member.declared_in_macro_type_arg.get()
        && member.merge_role != crate::semantic_query::MemberMergeRole::Heritage;
    ExpandedField {
        name: member.name.as_ref().to_string(),
        r#type,
        raw_type,
        optional: member.optional,
        exactness,
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
        shallow_source,
        declared_in_macro_type_arg,
    }
}

/// Resolve a surface member's value to its underlying body for
/// exactness classification. For `DeclRef` carriers (e.g. an
/// unparameterised type alias `MyStr` referenced from a property
/// signature), dispatches `ProjectPath { base: value, path: [],
/// mode: Shallow }` which expands the `DeclRef` to its body. For
/// other variants the value is returned unchanged — `classify_node`
/// already alias-unwraps a single `Alias` hop.
///
/// When `carrier_mode` is set (the member is published as a
/// `Navigate` carrier), an `InstantiationRef` (a generic
/// instantiation such as `Tool<INPUT, OUTPUT>`) is NOT expanded to
/// its `Shallow` object surface. `Shallow` synthesises the one-level
/// object surface — for an interface-bodied generic that breadth-
/// enumerates the instantiated type's members into the audit
/// footprint, which is a Rule-5 (shallow-by-default) violation. A
/// carrier member's exactness IS `ExactSymbolic` (the un-expanded
/// `InstantiationRef` node classifies as symbolic), so skipping the
/// expansion produces the correct exactness without the leak.
/// `DeclRef` (an unparameterised alias such as
/// `type MyStr = string`) is still expanded — that is a single-hop
/// alias unwrap, not an object breadth-enumeration.
///
/// Dep-signature is fanned into every active fact tracer
/// unconditionally so the final-result cache observes the same
/// revalidation surface as the projector's other dispatches.
fn resolve_member_value_for_classification(
    dispatch: &ProjectSemanticDispatch<'_>,
    value: SemanticNodeId,
    carrier_mode: bool,
) -> SemanticNodeId {
    // Under the query-free macro hot mirror an inline-object member value
    // (`defineProps<{ msg: MyStr }>()`) is interned as an unresolved-reference
    // carrier (`BareRef("MyStr")`), not a pre-resolved `DeclRef`. Resolve the
    // carrier head ONE hop through the shared carrier-subject normalization so
    // an alias-to-primitive (`type MyStr = string`) reaches its concrete body
    // and classifies `ExactConcrete` — the same hop the path-walker / query
    // entry run. Done in `Navigate` (carrier-preserving for deeper structure);
    // the alias unwrap below then proceeds on the resolved `DeclRef`.
    let value =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, value).as_deref() {
            Some(data) if data.bare_ref_head().is_some() || data.import_type_head().is_some() => {
                dispatch.resolve_carrier_subject_node(
                    value,
                    crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Navigate,
                    ),
                )
            }
            _ => value,
        };
    let should_expand =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, value).as_deref() {
            Some(SemanticNodeData::DeclRef { .. }) => true,
            // Carrier-mode: do NOT expand a generic instantiation to
            // its shallow object surface — that would breadth-
            // enumerate the instantiated type's members (a Rule-5
            // shallow-by-default violation). The un-expanded node
            // classifies as `ExactSymbolic`, the correct exactness
            // for a carrier.
            Some(SemanticNodeData::InstantiationRef { .. }) => !carrier_mode,
            _ => false,
        };
    if !should_expand {
        return value;
    }
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: value,
        path: super::empty_path(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    });
    crate::request_context::observe_component_meta_read_suppress(&read);
    emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
    match read.value {
        QueryResult::Value(id) => id,
        _ => value,
    }
}

pub(crate) fn project_model(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Option<ExpandedField> {
    if !mac.is_type_based {
        return None;
    }

    // Enforce the Model surface kind from the cursor's CARRIED surface (DERIVED,
    // not caller-asserted): `descend_published_member` does NOT validate the
    // cursor's surface kind, so the API would otherwise be weaker than the
    // per-member admission invariant (which gates on a derived surface kind).
    // `project_model` publishes a MODEL payload, so a non-`Model` cursor is a
    // caller-misuse — fail closed rather than publish the model under the wrong
    // surface. The sole production caller passes `Model`.
    if !matches!(
        cursor.surface,
        crate::meta_resolve::projection_demand::PublishedSurfaceKind::Model
    ) {
        return None;
    }

    let model_name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL_NAME.to_string());

    // Descend into the published model member.
    // `descend_published_member` returns `None` (the model is dropped
    // from the published surface) when a narrowed projection does not
    // admit the model name; for the whole-surface default it yields a
    // terminal carrier cursor. `project_model` raises a single payload
    // (no surface walk) so the carrier mode does not gate a per-member
    // breadth loop here — the descend gate IS the load-bearing use.
    let _member_cursor = cursor.descend_published_member(&model_name)?;

    let ctx: &dyn ResolverContext = query_engine.ctx;
    let (payload_node, presence_outcome, exactness, contains_reducible) = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        // `project_model` is a ROOT publication path that keeps a root cursor
        // (the `descend_published_member` gate above). It resolves + admits the
        // payload INTERNALLY here through the publication-authority token API —
        // it mints the payload token and reads its node, never handing a raw
        // `SemanticNodeId` to a non-sink helper.
        let payload = super::publication_authority::resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineModel,
            MacroExpansionKind::DefineProps,
            diag_sink,
        )?;
        let payload_node = payload.node();

        // Emit `PublishedField` for the model member. `defineModel<T>()`
        // publishes the raised payload under `model_name` (defaulting
        // to `modelValue`). payload_node serves as both parent surface
        // and member value because model is a single-field projection
        // (no wrapping object surface separate from the payload).
        let model_name_arc: std::sync::Arc<str> = std::sync::Arc::from(model_name.as_str());
        dispatch.record_published_field_edge(owner, payload_node, payload_node, &model_name_arc);

        // The payload's PRESENCE gate carried as the TYPED degradation state
        // (`SemanticOutcome::Degraded(QueryError::RaiseMiss)` for a node the
        // live graph store cannot serve) — the semantic decision below
        // branches on that typed state, never on a fabricated sentinel
        // string. No `TypeExpr` is materialised for the decision.
        let presence_outcome: SemanticOutcome<()> =
            match crate::project_semantic_dispatch::node_data_for(ctx, payload_node) {
                Some(_) => SemanticOutcome::Value(()),
                None => SemanticOutcome::Degraded(QueryError::RaiseMiss),
            };
        let exactness = classify_node(&dispatch, payload_node);
        // Reducibility is decided on the payload NODE (node-domain) —
        // `project_model` makes no decision on any materialised value.
        let contains_reducible =
            super::classify_node_reduction_gates(ctx, payload_node).contains_reducible_operator;
        (
            payload_node,
            presence_outcome,
            exactness,
            contains_reducible,
        )
    };

    if let SemanticOutcome::Degraded(reason) = presence_outcome {
        // Record the degradation so the consumer observes the missing
        // payload through the diagnostic stream rather than as a silent
        // shell — the published source below stays the model's authored
        // position (the demand side re-raises it).
        diag_sink.push(super::macro_expansion_for_query_error(
            macro_index,
            MacroExpansionKind::DefineProps,
            format!("model-payload-raise-failed:{reason:?}"),
        ));
    }

    // An operator-shape model type (`defineModel<Foo['a']>`) carries EXPLICIT
    // path demand inside the type expression — reduce it path-precisely (the
    // reduction warms the shared dispatch memos). A bare carrier
    // (`defineModel<Tool<I, O>>`) has no operator node, so no reduction runs —
    // published as a carrier.
    let reduced_node = if contains_reducible {
        reduce_field_value_node(query_engine, file, payload_node, ProjectionMode::Navigate)
            .node_id()
    } else {
        Some(payload_node)
    };

    // The model's published SOURCE POSITION: its own T — the macro
    // type-argument payload position — unless the reduction resolved a
    // complete leaf. An UNTYPED `defineModel()` has no type-argument
    // annotation: a PROVEN unannotated schema absence, never a fabricated
    // `unknown` value.
    let authored_model_source = mac.parsed_type_argument.as_ref().map(|locator| {
        verter_type_expr::facts::SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(locator.clone()),
        )
    });
    let r#type = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        published_source_upgrade_for_node(&dispatch, reduced_node)
            .or(authored_model_source)
            .map(verter_type_expr::facts::SourcePosition::Present)
            .unwrap_or_else(verter_type_expr::facts::SourcePosition::unannotated)
    };

    let name = model_name;

    Some(ExpandedField {
        name,
        r#type,
        raw_type: None,
        optional: false,
        exactness,
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
        shallow_source: None,
        // `defineModel<T>()` synthesizes the model member at the
        // macro's T position. The member is structurally
        // author-declared in the macro's type argument by virtue of
        // the `defineModel` syntax itself — set `true`.
        declared_in_macro_type_arg: true,
    })
}

/// The ONE centralized missing-source output policy: a lane position whose
/// analysis source is `None` (no typed payload available) materializes to the
/// canonical typed `TypeExpr::Unknown`. Every output lane routes an absent
/// source through THIS function — absence semantics are session-owned and
/// decided in exactly one place, never re-derived per lane or at the wire.
fn missing_source_output_type_expr() -> TypeExpr {
    TypeExpr::Unknown { raw: String::new() }
}

/// Materialize ONE present output source to its wire `TypeExpr` at this
/// terminal sink.
///
/// - `Closed(Leaf)` / `Closed(LeafUnion)` render their published shallow
///   value DIRECTLY — they are NOT raised. Raising a closed source re-runs
///   name resolution in the owner scope, which can resolve a package
///   re-export alias to its internal declaration name: a semantic demand
///   that breaks shallow-by-default (see the shallow-probe special case in
///   `project_semantic_dispatch::semantic_source`).
/// - Every other source raises through the ONE shared engine — the STRICT
///   raise entry (`raise_semantic_type_source_to_hot_strict`) under
///   `Navigate` structural transit — then shell-materializes through the
///   sealed output capability (PLAIN SHELL — refs stay refs; never the
///   reduced/`Expanded` materializer) and strictly unwraps the sealed
///   carrier here, inside the sink.
/// - FAIL-CLOSED: a present source that cannot be raised or
///   shell-materialized returns a typed failure, AND a failed REQUIRED
///   interior dereference inside a successfully-composed root (an interned
///   `Opaque(Miss)` the shell algebra would render as `Unknown`) is
///   REJECTED BEFORE shell materialization with its nested position path
///   ([`crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss`]).
///   Genuinely ABSENT schema positions at the LANE level render the
///   centralized typed `Unknown` through the `Absent` arm and never reach
///   this raise. There is NO `Unknown`-synthesizing fallback on this path —
///   a materialization failure must never silently become `Unknown`.
/// - CONSERVATIVE interior fail-close: a successfully-raised
///   schema-PRESENT position (a direct source-root deref or a composed
///   shell's present slot) whose materialized shape carries an
///   unknown-materializing resolver-failure carrier ANYWHERE (root or
///   interior) — a graph-interned `Opaque` control failure the shell fold
///   would render as a completed `unknown` — is REJECTED by the strict
///   raise
///   ([`crate::meta_resolve::ComponentMetaOutputFailure::UnknownMaterializingSourceInterior`]),
///   via the shared node-domain whole-tree miss fact (the legitimately
///   publishable carriers — a recursive reference, a declaration
///   placeholder — are not misses and pass). Proven schema absence keeps
///   its typed `Unknown`: an ABSENT slot of a composed fact shell interns
///   the typed miss WITHOUT a deref (the schema `Option` is the proof), so
///   it is never checked. The graph does not carry per-position
///   absent-vs-failed `Opaque` provenance yet, so a DEREF'D body's
///   interior failure conservatively fails the whole position. A nested
///   position can render `unknown` only when the schema itself proves that
///   position absent; an opaque failure inside a dereferenced body carries
///   no equivalent proof.
fn materialize_output_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    cap: &MetaResolveProjectorsOutputCap<'_, '_>,
    scope_canonical_id: &str,
    source: &verter_type_expr::facts::SemanticTypeSource,
) -> Result<TypeExpr, crate::meta_resolve::ComponentMetaOutputFailure> {
    use verter_type_expr::facts::{ClosedTypeFact, SemanticTypeSource};
    match source {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)) => {
            Ok(crate::project_semantic_dispatch::lower::leaf_type_fact_expr(leaf))
        }
        SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(leaves)) => Ok(TypeExpr::Union(
            leaves
                .iter()
                .map(crate::project_semantic_dispatch::lower::leaf_type_fact_expr)
                .collect(),
        )),
        other => {
            let transit_ctx =
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                );
            let hot = dispatch
                .raise_semantic_type_source_to_hot_strict(other, scope_canonical_id, transit_ctx)
                .map_err(|failure| match failure {
                    crate::project_semantic_dispatch::semantic_source::StrictSourceRaiseFailure::InteriorMiss(path) => {
                        crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss { path }
                    }
                    crate::project_semantic_dispatch::semantic_source::StrictSourceRaiseFailure::UnknownMaterializing(path) => {
                        crate::meta_resolve::ComponentMetaOutputFailure::UnknownMaterializingSourceInterior { path }
                    }
                })?
                .ok_or(crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource)?;
            let sealed = cap
                .materialize_output_type_expr(hot.node())
                .ok_or(crate::meta_resolve::ComponentMetaOutputFailure::ShellMaterializationMiss)?;
            Ok(sealed.into_type_expr(cap))
        }
    }
}

// The output-ENVELOPE assembly (the request-local source memo, the 11-lane
// materializer, the session-owned registry overlay finalize, and the envelope
// builder) lives in the `envelope` CHILD module of this sink — inside the
// capability mint scope (`pub(in ...::output_sink)` covers descendants), so
// the terminal-sink fence is unchanged.
// The published-field FINALIZE pass (the whole-surface field-type reducer
// over `ExpandedComponentTypes`) lives in the `published_finalize` CHILD
// module of this sink — inside the capability mint scope
// (`pub(in ...::output_sink)` covers descendants), so the terminal-sink
// fence is unchanged.
mod published_finalize;
pub(crate) use published_finalize::reduce_published_field_types;

mod envelope;
pub(crate) use envelope::build_component_meta_output;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use envelope::OUTPUT_MATERIALIZE_FORCE_FAIL_FOR;
#[cfg(test)]
pub(crate) use envelope::{
    LAST_OUTPUT_MATERIALIZE_CALLS, LAST_OUTPUT_MEMO_HASH_OPS, OUTPUT_MATERIALIZE_FORCE_FAIL,
};

#[cfg(test)]
#[path = "output_sink_tests.rs"]
mod semantic_outcome_tests;
