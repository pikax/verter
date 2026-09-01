//! `host_manage::component_meta_extract` — component-meta extraction
//! free functions: snapshot → ComponentMetaAnalysis projection and SFC
//! sidecar population.
//!
//! Domain K. Owns the public-facing
//! `extract_component_meta_from_resolved` /
//! `extract_component_meta_from_resolved_with_facts` entry points
//! plus their internal helpers (`populate_ordered_sfc_structure`,
//! `populate_public_instance_sidecar`,
//! etc.). The `crate::host_manage::*` import paths used by `meta.rs`,
//! `component_meta_host.rs`, and
//! `component_meta_resolution_policy.rs` are preserved by a `pub(crate)
//! use` re-export block in the parent shell — see §11c.5.

use crate::instant::Instant;

use crate::resolver_core::{
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry,
};
use crate::types::*;
use crate::VerterHost;

use super::{component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom};

// Legacy TypeExpr walkers (collect_required_owner_import_names, collect_slot_eval_import_names_*,
// collect_surface_eval_import_names_*, collect_runtime_value_names_*, etc.) were deleted.
// The solver host now resolves cross-file types on demand through prepared-decl caches.

/// Collect the set of runtime value names referenced by the template.
/// This reads pre-analyzed snapshot data (binding_occurrences, prop.referenced_bindings),
/// NOT TypeExpr trees — it is not a walker.
pub(in crate::host_manage) fn collect_required_template_runtime_value_names(
    snapshot: &FileAnalysisSnapshot,
) -> rustc_hash::FxHashSet<String> {
    let mut required = rustc_hash::FxHashSet::default();
    let Some(template) = snapshot.template.as_ref() else {
        return required;
    };

    required.extend(
        template
            .binding_occurrences
            .iter()
            .map(|occurrence| occurrence.name.clone()),
    );

    for component in &template.components {
        for prop in &component.props {
            required.extend(prop.referenced_bindings.iter().cloned());
            if prop.is_shorthand {
                required.insert(prop.name.clone());
            }
        }
    }

    required
}

pub(in crate::host_manage) fn collect_required_root_fallthrough_runtime_value_names(
    snapshot: &FileAnalysisSnapshot,
    root_reachability: &verter_semantic::analysis::component_meta::RootReachability,
) -> rustc_hash::FxHashSet<String> {
    use verter_semantic::analysis::component_meta::{RootReachability, RootTargetRef};
    use verter_semantic::analysis::template::BindingUsageKind;

    let mut required = rustc_hash::FxHashSet::default();
    let Some(template) = snapshot.template.as_ref() else {
        return required;
    };

    let RootReachability::Branches { branches } = root_reachability else {
        return required;
    };

    for branch in branches {
        let element_index = match &branch.target {
            RootTargetRef::NativeElement { element_index, .. }
            | RootTargetRef::DynamicComponentUsage { element_index, .. }
            | RootTargetRef::ComponentUsage { element_index, .. }
            | RootTargetRef::UnresolvedTarget { element_index, .. } => *element_index as usize,
        };

        let Some(element) = template.elements.get(element_index) else {
            continue;
        };

        for occurrence in &template.binding_occurrences {
            if occurrence.span.start < element.span.start
                || occurrence.span.end > element.tag_span_end
            {
                continue;
            }
            if matches!(
                occurrence.usage_kind,
                BindingUsageKind::DirectiveValue | BindingUsageKind::EventHandler,
            ) {
                required.insert(occurrence.name.clone());
            }
        }

        let usage_index = match &branch.target {
            RootTargetRef::DynamicComponentUsage { usage_index, .. }
            | RootTargetRef::ComponentUsage { usage_index, .. } => Some(*usage_index as usize),
            RootTargetRef::NativeElement { .. } | RootTargetRef::UnresolvedTarget { .. } => None,
        };

        let Some(usage) = usage_index.and_then(|usage_index| template.components.get(usage_index))
        else {
            continue;
        };

        for prop in &usage.props {
            required.extend(prop.referenced_bindings.iter().cloned());
            if prop.is_shorthand {
                required.insert(prop.name.clone());
            }
        }
    }

    required
}

/// Extract slot bindings from a type_text that encodes a slot's function signature.
///
/// Handles property signature types like `(props: { row: Item; index: number }) => any`.
/// Extract slot bindings and return type from a type_text encoding a slot function signature.
///
/// Handles both arrow-style (`(props: { row: Item }) => VNode[]`) and
/// method-style (`(props: { row: Item }): VNode[]`) signatures.
/// Returns `(bindings, return_type)`.
/// Build a `ComponentMetaAnalysis` from a resolved-meta state.
/// Shared by `get_component_meta` and `get_component_meta_with_resolution`.
fn extract_component_meta_from_inputs(
    host: &VerterHost,
    canonical_or_alias: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[verter_semantic::analysis::component_meta::ResolvedMacroInput],
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let started = component_meta_debug_enabled().then(Instant::now);
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    let resolved_binding_reactivity = resolved_binding_reactivity(ctx, snapshot);
    component_meta_trace_custom!(
        "extract_component_meta",
        format!(
            "owner={} macros={} resolved_macros={} resolved_type_registry={} has_evaluated_types={}",
            canonical,
            snapshot.macros.len(),
            resolved_macros.len(),
            resolved_type_registry.len(),
            evaluated_types.is_some(),
        ),
    );
    let input = verter_semantic::analysis::component_meta::ComponentMetaInput {
        macros: &snapshot.macros,
        bindings: &snapshot.bindings,
        imports: &snapshot.imports,
        template: snapshot.template.as_deref(),
        options_api: snapshot.options_api.as_ref(),
        analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
            snapshot.script_flags,
        ),
        styles: &snapshot.styles,
        vue_api_calls: &snapshot.vue_api_calls,
        store_usages: &snapshot.store_usages,
        resolved_macros,
        resolved_binding_reactivity: &resolved_binding_reactivity,
        resolved_type_registry,
        evaluated_types,
        file_path: &canonical,
    };
    let mut meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
    component_meta_trace_custom!(
        "extract_component_meta_declared_surface",
        format!(
            "owner={} props={} events={} slots={}",
            canonical,
            meta.props.len(),
            meta.events.len(),
            meta.slots.len(),
        ),
    );

    if let Some(started) = started {
        component_meta_debug(format!(
            "extract_component_meta owner={} took {:?}",
            canonical,
            started.elapsed(),
        ));
    }

    populate_public_instance_sidecar(&mut meta);
    if let Some(view) = ctx.active_session_view() {
        if let Some(structure) = host.registered_structure_for_view(&canonical, view) {
            meta.ordered_sfc_structure = Some(crate::host_resolve::ordered_sfc_structure_analysis(
                &structure,
            ));
        }
    } else {
        crate::host_resolve::populate_ordered_sfc_structure(host, &canonical, &mut meta);
    }
    meta
}

/// Demand the whole-return reactive-wrapper role for every composable-call
/// binding on the owner whose reactivity the analyzer's value-space body walk
/// could not decide.
///
/// This is the component-meta consumer of the shared whole-return demand entry
/// [`crate::project_semantic_dispatch::reactive_wrapper::wrapper_role_for_sole_value_signature_return`].
/// It runs at the ONE demand point where the fixed mechanism already holds: the
/// caller's `ctx` is request-bound (`is_request_bound() == true`, so the demand
/// reads the request's own view — a session overlay included — rather than the
/// shared base, and `ProjectSemanticDispatch::new` counts no bare construction),
/// the request's installed `RequestContext` has the projection fuse armed, the
/// enclosing cold compute runs under `with_fact_tracer`, and the enclosing
/// `ColdComputeCompletenessScope` is already open. It is NOT reachable from any
/// context-free snapshot accessor.
///
/// Demand is per requested subject and strictly gated — there is no owner-wide
/// or workspace-wide scan. A binding is demanded only when ALL of the following
/// hold:
///
/// 1. its reactivity is still `MaybeRef` — the analyzer's "composable call,
///    shape undecided" state. Anything the value-space walk already decided
///    keeps its answer and costs nothing here;
/// 2. its initializer is a `FunctionCall` that binds the call's WHOLE result
///    (`const c = useCounter()`, not `const { count } = useCounter()`) — a
///    whole-return type answers nothing about a destructured member;
/// 3. the callee is imported, and the owner's import row for that local name
///    carries a resolved canonical id plus an imported name.
///
/// One shared `ProjectSemanticDispatch` serves the whole loop: each top-level
/// route walk is its own connected-demand root, so sharing the dispatch neither
/// pools nor leaks work budget between subjects.
///
/// A typed degradation folds into the enclosing cold-compute completeness, so
/// the component-meta result is refused warm admission (the no-poison
/// invariant) — but it is folded as a PARTIAL propagation of the whole result,
/// never as a fatal that drops the row: the degraded role is still published on
/// its binding, per-row and fail-closed.
pub(crate) fn resolved_binding_reactivity(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    snapshot: &FileAnalysisSnapshot,
) -> Vec<verter_semantic::analysis::component_meta::ResolvedBindingReactivityInput> {
    use verter_semantic::analysis::types::{BindingInitializer, ReactivityKind};

    let mut subjects: Vec<(usize, String, String)> = Vec::new();
    for (binding_index, binding) in snapshot.bindings.iter().enumerate() {
        if binding.reactivity_kind != ReactivityKind::MaybeRef {
            continue;
        }
        let Some(BindingInitializer::FunctionCall {
            callee,
            callee_import_source: Some(import_source),
            binds_whole_call_result: true,
            ..
        }) = binding.initializer.as_ref()
        else {
            continue;
        };
        // The owner's own import inventory is the routing authority: match the
        // import declaration by specifier AND the specifier row by LOCAL name,
        // so a renamed value import (`import { useCounter as useC }`) demands
        // the imported name, never the local spelling.
        let Some((dep_canonical, imported_name)) = snapshot
            .imports
            .iter()
            .filter(|import| import.source == *import_source)
            .find_map(|import| {
                let dep_canonical = import.resolved_canonical_id.as_deref()?;
                let specifier = import
                    .bindings
                    .iter()
                    .find(|specifier| specifier.name == *callee)?;
                // An imported NAME is mandatory. That requirement is also what
                // fails a namespace import closed: `import * as ns` binds a
                // module object and carries no imported name, so there is no
                // exported symbol for the callee spelling to address.
                let imported_name = specifier.imported_name.as_deref()?;
                Some((dep_canonical.to_string(), imported_name.to_string()))
            })
        else {
            continue;
        };
        subjects.push((binding_index, dep_canonical, imported_name));
    }
    if subjects.is_empty() {
        return Vec::new();
    }

    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let mut rows = Vec::with_capacity(subjects.len());
    for (binding_index, dep_canonical, imported_name) in subjects {
        // Resolve the imported symbol's declaring ROOT through the shared
        // name-keyed export walk (barrels and re-exports included), and record
        // the route-chain facts onto the active tracer so a barrel retarget
        // misses the enclosing warm component-meta entry.
        let (root, facts) =
            ctx.resolve_imported_type_root_with_facts(&dep_canonical, &imported_name);
        ctx.observe_borrowed_signature(&facts);
        let Some(root) = root else {
            continue;
        };
        // The returned route provenance is DROPPED here on purpose: it embeds
        // authored `TypeArgLocator`s and stays session-internal. Only the closed
        // role vocabulary is published, so nothing locator-bearing can cross FFI.
        let (role, _provenance) =
            crate::project_semantic_dispatch::reactive_wrapper::wrapper_role_for_sole_value_signature_return(
                &dispatch,
                root.canonical_id.as_ref(),
                root.owner,
                root.symbol_name.as_ref(),
            );
        if let verter_type_expr::ReactiveWrapperRole::Unresolved { .. } = role {
            crate::request_context::fold_result_completeness(
                crate::semantic_query::ResultCompleteness::partial(
                    crate::semantic_query::PartialReasonSet::PROPAGATED,
                ),
            );
        }
        rows.push(
            verter_semantic::analysis::component_meta::ResolvedBindingReactivityInput {
                binding_index,
                return_wrapper_role: role,
            },
        );
    }
    rows
}

/// Resolve a bare type-name reference in the owner file's scope to its
/// canonical `ResolvedRootIdentity` (defining file + symbol name).
///
/// Scope-aware: handles local declarations (returning
/// `ResolvedRootIdentity { canonical_id: owner_canonical, .. }`) and
/// imported names (returning the import target's canonical_id +
/// imported name). Local declarations take precedence over imports per
/// JavaScript module scoping (a local `Helper` shadows
/// `import type { Helper } from "./b"`).
///
/// Cross-file resolution goes through `host.resolve_local_import_symbol_target`
/// (cache-backed). No fresh resolver; no duplicate route discovery.
pub(crate) fn resolve_ref_to_root_identity(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
    crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
        ctx,
        owner_canonical,
        owner,
        None,
        name,
    )
}

fn build_public_instance_slots_member(
    _slots: &[verter_semantic::analysis::component_meta::SlotAnalysis],
) -> verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
    verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
        name: "$slots".to_string(),
        kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::SlotContainer,
        type_source: verter_type_expr::facts::SourcePosition::unannotated(),
        type_expansion: None,
        raw_type: None,
        description: None,
        tags: Vec::new(),
    }
}

pub(crate) fn populate_public_instance_sidecar(
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) {
    let mut members = Vec::new();

    if !meta.slots.is_empty() {
        members.push(build_public_instance_slots_member(&meta.slots));
    }

    members.extend(meta.props.iter().map(|prop| {
        verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
            name: prop.name.clone(),
            kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Prop,
            type_source: prop.publication.source_position(),
            type_expansion: prop.type_expansion.clone(),
            raw_type: prop
                .publication
                .evidence()
                .map(|evidence| evidence.text().to_string()),
            description: prop.description.clone(),
            tags: prop.tags.clone(),
        }
    }));

    for exposed in &meta.exposed {
        let next = verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
            name: exposed.name.clone(),
            kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed,
            type_source: exposed.type_source.clone(),
            type_expansion: exposed.type_expansion.clone(),
            raw_type: None,
            description: exposed.description.clone(),
            tags: exposed.tags.clone(),
        };
        if let Some(existing) = members.iter_mut().find(|member| member.name == next.name) {
            *existing = next;
        } else {
            members.push(next);
        }
    }

    meta.public_instance = if members.is_empty() {
        None
    } else {
        Some(
            verter_semantic::analysis::component_meta::PublicInstanceAnalysis {
                members,
                completeness:
                    verter_semantic::analysis::component_meta::PublicInstanceCompleteness::Partial,
            },
        )
    };
}

/// Internal carrier for one component-meta extraction. Bundles the projected
/// analysis, the optional fallthrough fact versions, and the extraction's
/// observed COMPUTE completeness.
///
/// `completeness` is the completeness accumulated by ONE full-extract
/// [`ColdComputeCompletenessScope`](crate::request_context::ColdComputeCompletenessScope)
/// that spans the WHOLE extract body — the pre-choke macro-DTO read INCLUDED
/// and the fallthrough compute folded in — so every partiality source inside
/// extraction reaches one signal. The publishing surfaces merge it with the
/// resolve-phase `resolved.completeness`
/// (`final_completeness = resolved.completeness.merge(outcome.completeness)`)
/// and gate admission on that single merged signal, replacing the
/// source-enumerated per-phase gate.
///
/// INTERNAL to `verter_session`: `completeness` is admission metadata only —
/// it never enters a cache key, the query value `V`, any `Hash`/equality, or a
/// wire DTO.
pub(crate) struct ComponentMetaExtractOutcome {
    /// The projected component-meta analysis.
    pub(crate) analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    /// The fallthrough resolution's fact versions. Both the analysis and
    /// payload extraction entries publish this call-owned dependency lane so
    /// their caller can merge it into the resolved-state signature.
    pub(crate) fallthrough_fact_versions: Option<Vec<crate::resolver_core::FactVersionRef>>,
    /// The COMPUTE completeness observed across the WHOLE extract body — the
    /// macro-DTO read, projection, policy, and the folded fallthrough compute.
    /// `Partial` whenever any of those tripped a budget / fuse / fatal read.
    /// This is COMPUTE completeness, NOT the surface-shape
    /// `accepted_surface_completeness` (a representable `LowerBound` surface
    /// with a complete compute stays cacheable).
    pub(crate) completeness: crate::semantic_query::ResultCompleteness,
}

pub(crate) fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    include_fallthrough: bool,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) -> ComponentMetaExtractOutcome {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    // ONE full-extract completeness scope spans the WHOLE extract body so every
    // partiality source inside extraction folds into ONE signal — the pre-choke
    // macro-DTO read below INCLUDED. That read can `observe_partial` a
    // budget-tripped DTO (`resolver_component_meta_resolved_macros` →
    // `vue_macro_dtos_with_ctx`, which refuses its own `vue_surface_store`); a
    // gate keyed only on the later fallthrough completeness let such a partial
    // warm the overall result anyway. The scope captures it; the publishing
    // surfaces merge `current_cold_compute_completeness()` with
    // `resolved.completeness`. The scope is DISCARDED (not bubbled) once its
    // completeness is read — the signal travels with the outcome carrier, never
    // via a scope bubble that could over-suppress an enclosing compute.
    let extract_scope = crate::request_context::ColdComputeCompletenessScope::enter();
    // The macro-DTO surface read (`vue_macro_dtos_with_ctx` ->
    // `ctx.store_view()`) MUST run under the request-bound `ctx`, not the
    // bare `&VerterHost` rail (whose `store_view()` panics in a
    // `debug_assertions`-off build). See
    // `tests/cases/g_session/session_meta_store_view_regression.rs`.
    let resolved_macros = resolver_component_meta_resolved_macros(
        ctx,
        canonical.as_str(),
        resolved.snapshot.macros.as_ref(),
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let mut meta = extract_component_meta_from_inputs(
        host,
        canonical_or_alias,
        &resolved.snapshot,
        &resolved_macros,
        &resolved_type_registry,
        resolved.evaluated_types.as_ref(),
        ctx,
    );
    let mut fallthrough_facts = None;
    if include_fallthrough {
        let mut visiting = rustc_hash::FxHashSet::default();
        // The completeness travels WITH the resolution via the outcome carrier
        // (centralised per-call scope), so a stale partial from a discarded
        // completion-fence retry cannot taint this attempt. Fold the captured
        // fallthrough completeness into the full-extract scope so it reaches the
        // one merged signal alongside the macro-DTO partiality.
        let outcome = host.compute_fallthrough_outcome_from_resolved_state(
            &canonical,
            resolved,
            None,
            &mut visiting,
            ctx,
        );
        if let Some(resolution) = outcome.resolution {
            fallthrough_facts = Some(resolution.fact_versions.clone());
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        }
        crate::request_context::fold_result_completeness(outcome.completeness);
    }
    // apply the publication policy over (resolved_type_registry,
    // resolved_type_registry_meta) + snapshot.macros (§3.4 structural
    // classification); see .claude/skills/component-meta/SKILL.md.
    crate::component_meta_resolution_policy::apply_component_meta_resolution_policy(
        &mut meta,
        &resolved.resolved_type_registry,
        &resolved.resolved_type_registry_meta,
        host,
        canonical.as_str(),
        Some(&resolved.snapshot),
        ctx,
    );
    // Merge graph-native slot-binding synthesis diagnostics into the
    // analysis-wide envelope so consumers see one canonical
    // diagnostic stream regardless of which subsystem produced it.
    if !resolved.synthesis_diagnostics.is_empty() {
        meta.macro_expansion_diagnostics
            .extend(resolved.synthesis_diagnostics.iter().cloned());
    }
    let completeness = crate::request_context::current_cold_compute_completeness();
    extract_scope.discard();
    // `accepted_surface_completeness` is an EXHAUSTIVENESS claim over the
    // accepted surface. The fallthrough resolver computes it from root
    // reachability alone, but the accepted set SUBTRACTS the declared
    // props/events — so when the surrounding compute is PARTIAL (an
    // unresolvable props import, a budget-tripped macro read) the declared
    // surface may be missing members and `Exact` is unsupportable: demote to
    // the lower-bound claim. A COMPLETE compute keeps the resolver.s claim —
    // a genuinely props-less component stays `Exact`.
    if resolved.completeness.is_partial() || completeness.is_partial() {
        meta.accepted_surface_completeness =
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound;
    }
    ComponentMetaExtractOutcome {
        analysis: meta,
        fallthrough_fact_versions: fallthrough_facts,
        completeness,
    }
}

/// Like [`extract_component_meta_from_resolved`] with `include_fallthrough=true`,
/// but also returns the fallthrough resolution's fact versions (if available).
/// Used by the payload cache to store Full payloads with the correct fact set.
pub(crate) fn extract_component_meta_from_resolved_with_facts(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) -> ComponentMetaExtractOutcome {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    // ONE full-extract completeness scope spans the WHOLE extract body, the
    // pre-choke macro-DTO read INCLUDED — see the sibling
    // `extract_component_meta_from_resolved` for the rationale and
    // `tests/cases/g_session/session_meta_store_view_regression.rs` for the
    // request-bound `ctx` requirement.
    let extract_scope = crate::request_context::ColdComputeCompletenessScope::enter();
    let resolved_macros = resolver_component_meta_resolved_macros(
        ctx,
        canonical.as_str(),
        resolved.snapshot.macros.as_ref(),
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let mut meta = extract_component_meta_from_inputs(
        host,
        canonical_or_alias,
        &resolved.snapshot,
        &resolved_macros,
        &resolved_type_registry,
        resolved.evaluated_types.as_ref(),
        ctx,
    );
    let mut visiting = rustc_hash::FxHashSet::default();
    // The outcome carrier centralises the per-call completeness scope: a
    // fallthrough partial folds in so the fallthrough's OWN caches
    // (`store_node`) self-gate on the typed completeness signal, and the
    // captured completeness folds into the full-extract scope so the
    // payload-write gate refuses to warm a fallthrough partial (matching the
    // analysis surface's result-cache gate). The completeness travels with the
    // resolution, so a discarded completion-fence retry cannot taint this
    // attempt.
    let fallthrough_facts = {
        let outcome = host.compute_fallthrough_outcome_from_resolved_state(
            &canonical,
            resolved,
            None,
            &mut visiting,
            ctx,
        );
        let facts = if let Some(resolution) = outcome.resolution {
            let facts = resolution.fact_versions.clone();
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
            Some(facts)
        } else {
            None
        };
        crate::request_context::fold_result_completeness(outcome.completeness);
        facts
    };
    // apply the publication policy AFTER fallthrough merge so the
    // pass operates on the final accepted_props/events. Walks
    // (resolved_type_registry, resolved_type_registry_meta) plus the
    // snapshot's macros (for §3.4 structural macro-participation
    // classification).
    crate::component_meta_resolution_policy::apply_component_meta_resolution_policy(
        &mut meta,
        &resolved.resolved_type_registry,
        &resolved.resolved_type_registry_meta,
        host,
        canonical.as_str(),
        Some(&resolved.snapshot),
        ctx,
    );
    let completeness = crate::request_context::current_cold_compute_completeness();
    extract_scope.discard();
    // Same exhaustiveness demotion as `extract_component_meta_from_resolved`:
    // a PARTIAL compute cannot support the `Exact` accepted-surface claim.
    if resolved.completeness.is_partial() || completeness.is_partial() {
        meta.accepted_surface_completeness =
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound;
    }
    ComponentMetaExtractOutcome {
        analysis: meta,
        fallthrough_fact_versions: fallthrough_facts,
        completeness,
    }
}

/// Test-only entry point that exercises `resolve_ref_to_root_identity`
/// for the scope-correctness characterisation test.
#[cfg(test)]
pub(in crate::host_manage) fn resolve_ref_to_root_identity_for_test(
    host: &VerterHost,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
    resolve_ref_to_root_identity(host, owner_canonical, owner, name)
}

#[cfg(test)]
#[path = "component_meta_extract_tests.rs"]
mod component_meta_extract_tests;
