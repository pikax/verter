//! Component-meta publication policy.
//!
//! The externally-observable component-meta resolution policy: it decides,
//! per published type, whether the public surface stays symbolic (a shallow
//! reference the consumer re-resolves on demand) or materializes to a
//! concrete shape, per the Shallow-By-Default publication rule.
//!
//! The pass operates on a dispatch-resolved [`ComponentMetaAnalysis`] and
//! rewrites the public type SOURCES in place per the rules derived in
//! `docs/arch/debt-closure/06-step4b-consumer-surface.md`: every decision
//! raises the published source to a semantic-graph node through the ONE
//! shared dispatch bridge and classifies node-domain; a fired rule
//! publishes a replacement content-free source (materialized `TypeExpr`
//! exists only at the sealed output sink).
//!
//! ## Inputs
//!
//! `(resolution.resolved_type_registry, resolution.resolved_type_registry_meta,
//! &VerterHost)`. The host is consulted for cross-file declaration lookup of
//! transitively-referenced types not seeded into the registry's BFS root set
//! (e.g. `defineProps<ExternalProps>()` registers `ExternalProps` but the
//! `Status` referenced from `ExternalProps.status: Status` is not in the
//! registry — its declaration is reachable only via `ComponentMetaQueryEngine`
//! cross-file lookup).
//!
//! ## Contract
//!
//! * Adapters (`packages/component-meta/src/adapters/{zod,json-schema,
//!   histoire,storybook}.ts`) want concrete `Object` shapes for project-local
//!   non-Props imports — Rule 3.
//! * The compat layer (`packages/component-meta/src/compat/checker.ts`) wants
//!   *Props imports kept symbolic — Rules 2, 4.
//! * Package-backed types (`/node_modules/...`) stay symbolic — Rule 1.
//! * The alias-spine recursion (Rule 5) descends one reference hop at a
//!   time under the `(DeclIdentity, NormalizedTypeArgs)` cycle guard.

use rustc_hash::FxHashSet;
use verter_semantic::analysis::component_meta::{ComponentMetaAnalysis, ResolvedTypeAnalysis};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_semantic::analysis::AnalyzedMacroKind;

use crate::host_manage::component_meta_extract::resolve_ref_to_root_identity;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::ComponentMetaQueryEngine;
use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};
use crate::types::FileAnalysisSnapshot;
use crate::VerterHost;

mod core;
mod cycle_guard;
pub(crate) mod policy_helpers;
mod raw_restoration;
mod slot_preservation;

use self::core::{rewrite_source_in_place, PolicyCtx, PolicyRegistry};
use self::raw_restoration::restore_props_suffix_from_raw;
use self::slot_preservation::slot_binding_should_preserve_symbolic_raw_type;

/// Type-role-bearing Vue SFC macro kinds whose type arguments classify
/// the referenced type as "macro-participating" (kept symbolic per
/// Rules 2 / 4 + raw-restoration).
///
/// `DefineExpose` and `DefineOptions` are deliberately excluded — they
/// do not confer a role-classification under the §3.4 contract.
const TYPE_ROLE_MACRO_KINDS: &[AnalyzedMacroKind] = &[
    AnalyzedMacroKind::DefineProps,
    AnalyzedMacroKind::DefineEmits,
    AnalyzedMacroKind::DefineModel,
    AnalyzedMacroKind::DefineSlots,
    AnalyzedMacroKind::WithDefaults,
];

/// Apply the publication policy to `analysis`, rewriting public type
/// SOURCES in place per the rules in
/// `docs/arch/debt-closure/06-step4b-consumer-surface.md`.
///
/// `host` is consulted for cross-file declaration lookup of transitively-
/// referenced types not seeded into the BFS root set of the resolved type
/// registry (e.g. `Status` referenced from `ExternalProps.status`
/// when `ExternalProps` is the only macro-arg root). Lookups go through
/// [`ComponentMetaQueryEngine`] which delegates to the host-owned typed DBs,
/// so warm hits are O(1).
///
/// `snapshot` is the owner SFC's analyzer snapshot. The policy reads
/// `snapshot.macros` to build the structural macro-participation
/// classifier (§3.4 Typed-IR-Only Resolver Rule) — types are
/// "role-bearing" because a Vue SFC macro (`defineProps` /
/// `defineEmits` / `defineModel` / `defineSlots` / `withDefaults`)
/// consumes them, NOT because their identifier name ends in `"Props"`
/// or similar. Tests with no snapshot may pass `None` — the classifier
/// set is then empty (no references are classified as macro-participating,
/// so Rules 2 / 4 + the raw-restoration helpers never fire); production
/// callsites always supply the snapshot via the resolved-state.
///
/// The pass is host-bounded: source raises route through the shared
/// dispatch bridge, and declaration lookups walk the already-resolved
/// registry plus on-demand declaration metadata.
pub(crate) fn apply_component_meta_resolution_policy(
    analysis: &mut ComponentMetaAnalysis,
    type_registry: &[ResolvedTypeAnalysis],
    type_registry_meta: &[ResolvedTypeRegistryMeta],
    host: &VerterHost,
    owner_canonical: &str,
    snapshot: Option<&FileAnalysisSnapshot>,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) {
    let macro_participating_idents: FxHashSet<ResolvedRootIdentity> = match snapshot {
        Some(snap) => build_policy_macro_role_identities(
            host,
            ctx,
            owner_canonical,
            snap,
            TYPE_ROLE_MACRO_KINDS,
        ),
        None => FxHashSet::default(),
    };
    apply_component_meta_resolution_policy_with_participation(
        analysis,
        type_registry,
        type_registry_meta,
        host,
        owner_canonical,
        &macro_participating_idents,
        ctx,
    );
}

/// Variant of `apply_component_meta_resolution_policy` that takes the
/// macro-participating identity set directly, bypassing snapshot
/// construction.
///
/// Used by tests that exercise specific structural-classification
/// branches without standing up a full project + analysis pipeline.
/// Production code paths build the set from the resolved snapshot and
/// call `apply_component_meta_resolution_policy`.
pub(crate) fn apply_component_meta_resolution_policy_with_participation(
    analysis: &mut ComponentMetaAnalysis,
    type_registry: &[ResolvedTypeAnalysis],
    type_registry_meta: &[ResolvedTypeRegistryMeta],
    host: &VerterHost,
    owner_canonical: &str,
    macro_participating_idents: &FxHashSet<ResolvedRootIdentity>,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) {
    let registry = PolicyRegistry::build(type_registry, type_registry_meta);
    // Bind the engine to the supplied request-bound `ctx` so every
    // nested dispatch / validator inherits the overlay-aware view.
    let mut engine = ComponentMetaQueryEngine::new(ctx);
    let mut ctx = PolicyCtx {
        registry: &registry,
        engine: &mut engine,
        owner_canonical,
        host,
        macro_participating_idents,
        active_refs: FxHashSet::default(),
        active_refs_max_depth: 0,
    };

    let mut changed = false;

    for prop in analysis.props.iter_mut() {
        // Pre-step: restore macro-participating references the evaluator may
        // have eagerly resolved away — the authored annotation source is the
        // canonical form for macro-participating imports. Runs before the
        // rule walk.
        if restore_props_suffix_from_raw(
            &mut prop.type_source,
            prop.raw_type_source.as_ref(),
            &mut ctx,
        ) {
            changed = true;
        }
        if rewrite_source_in_place(&mut prop.type_source, &mut ctx) {
            changed = true;
        }
    }
    for event in analysis.events.iter_mut() {
        if rewrite_source_in_place(&mut event.payload, &mut ctx) {
            changed = true;
        }
    }
    for slot in analysis.slots.iter_mut() {
        for binding in slot.bindings.iter_mut() {
            // When the binding's authored annotation is an `IndexedAccess`
            // whose deref chain transits through an imported declaration,
            // force the symbolic authored source back onto the published
            // source and skip the rule walk. The eager evaluator may have
            // widened the indexed access through an open `[k: string]: any`
            // index signature; the consumer is better served by the
            // navigable `AppProps['avatar']` member-path contract.
            if slot_binding_should_preserve_symbolic_raw_type(
                binding.raw_type_source.as_ref(),
                &mut ctx,
            ) {
                // The guard already confirmed the authored annotation is an
                // `IndexedAccess` whose root resolves through an imported
                // declaration; restore that exact source onto the public
                // surface (it's necessarily `Some` here).
                if let Some(restored) = binding.raw_type_source.as_ref() {
                    if binding.type_source.present() != Some(restored) {
                        binding.type_source =
                            verter_type_expr::facts::SourcePosition::Present(restored.clone());
                        changed = true;
                    }
                }
                continue;
            }
            if restore_props_suffix_from_raw(
                &mut binding.type_source,
                binding.raw_type_source.as_ref(),
                &mut ctx,
            ) {
                changed = true;
            }
            // The graph publisher's shallow named-reference carrier
            // (`Closed(Leaf(Ref))`) is the RATIFIED published shape for a
            // named binding value (`message: MessageBase<T>` stays the
            // re-resolvable `Ref` — shallow-by-default); the rule walk must
            // not re-expand it into the located declaration body. The
            // arg-preserving authored USE-SITE carrier (`Authored(DeclBody)`
            // — the declaring decl's member-value slot for an
            // argument-bearing binding value) is equally final: the rule
            // walk could only keep the instantiation carrier (Rule 3
            // declines argument-bearing heads), so raising it per row is
            // pure fan-out.
            let publisher_shallow_ref = matches!(
                binding.type_source.present(),
                Some(verter_type_expr::facts::SemanticTypeSource::Closed(
                    verter_type_expr::facts::ClosedTypeFact::Leaf(
                        verter_type_expr::facts::LeafTypeFact::Ref(_)
                    )
                )) | Some(verter_type_expr::facts::SemanticTypeSource::Authored(
                    verter_type_expr::locators::AuthoredBodyLocator::DeclBody(_)
                ))
            );
            if !publisher_shallow_ref && rewrite_source_in_place(&mut binding.type_source, &mut ctx)
            {
                changed = true;
            }
        }
    }
    for model in analysis.models.iter_mut() {
        if rewrite_source_in_place(&mut model.type_source, &mut ctx) {
            changed = true;
        }
    }
    for exposed in analysis.exposed.iter_mut() {
        if rewrite_source_in_place(&mut exposed.type_source, &mut ctx) {
            changed = true;
        }
    }
    for accepted in analysis.accepted_props.iter_mut() {
        if restore_props_suffix_from_raw(
            &mut accepted.type_source,
            accepted.raw_type_source.as_ref(),
            &mut ctx,
        ) {
            changed = true;
        }
        if rewrite_source_in_place(&mut accepted.type_source, &mut ctx) {
            changed = true;
        }
    }
    for accepted in analysis.accepted_events.iter_mut() {
        if rewrite_source_in_place(&mut accepted.payload, &mut ctx) {
            changed = true;
        }
    }

    if changed {
        crate::host_manage::populate_public_instance_sidecar(analysis);
    }
}

/// Build the policy's role-bearing identity set: §3.4 structural
/// macro-participation classifier scoped to **the macro's named
/// composition closure**, gathered NODE-DOMAIN off the macro's authored
/// payload locator and the analyzer's synthesized local-type shapes.
///
/// Two sources contribute:
///
/// 1. `parsed_type_argument` — the macro's type-argument payload locator,
///    raised through the shared dispatch. The walk descends structural
///    composition (unions / intersections / inline object property values /
///    array elements / tuple elements / reference type-args) so references
///    inside an inline shape (e.g. `{ avatar?: AvatarProps }`) reach the
///    participation set. **Stops at `IndexedAccess` entirely** — a
///    value-extraction operation is not role-bearing composition: when the
///    user writes `defineProps<Foo['member']>`, `Foo.member` is the
///    role-bearing prop value type and the materialiser resolves it
///    path-precisely; neither `Foo` nor any sub-type reached through the
///    chain contributes.
/// 2. `resolved_local_types[i].name` AND every reference in the resolved
///    local type's synthesized SHAPE (raised and deep-walked). The analyzer
///    records named local aliases reached through the macro chain; their
///    full reference closures contribute because the user named the
///    composition.
///
/// Examples (with `defineProps<X>()`):
/// - `X = Foo`                          → `{ Foo }`
/// - `X = Foo<Bar>`                     → `{ Foo, Bar }`
/// - `X = Pick<MyType, K>`              → `{ Pick, MyType, K }`
/// - `X = Foo & Bar`                    → `{ Foo, Bar }`
/// - `X = { avatar?: AvatarProps }`     → `{ AvatarProps }` (object
///   properties surface composition refs)
/// - `X = { ui?: Button['slots'] }`     → `{}` (IndexedAccess is a
///   value-extraction operation, not role-bearing reference)
/// - `X = Foo['a']['b']`                → `{}` (same — value extraction)
/// - `X = ButtonProps` with `type ButtonProps = { a?: AvatarProps }`
///   → `{ ButtonProps, AvatarProps }` (named alias body contributes
///   its full reference closure)
fn build_policy_macro_role_identities(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    macro_kinds: &[AnalyzedMacroKind],
) -> FxHashSet<ResolvedRootIdentity> {
    let mut identities = FxHashSet::default();
    let mut visited_names: FxHashSet<String> = FxHashSet::default();

    let record_name = |name: &str,
                       identities: &mut FxHashSet<ResolvedRootIdentity>,
                       visited_names: &mut FxHashSet<String>| {
        if !visited_names.insert(name.to_string()) {
            return;
        }
        if let Some(identity) = resolve_ref_to_root_identity(host, owner_canonical, name) {
            identities.insert(identity);
        }
    };

    // One dispatch for every payload / shape raise below.
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let transit_ctx =
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );

    for mac in snapshot.macros.iter() {
        if !macro_kinds.contains(&mac.kind) {
            continue;
        }
        // Source 1: the parsed type-argument payload locator — raised
        // through the shared dispatch, walked over the structural skeleton
        // (no descent into function inner types or IndexedAccess).
        if let Some(locator) = mac.parsed_type_argument.as_ref() {
            let payload = dispatch.raise_semantic_type_source_to_hot(
                &verter_type_expr::facts::SemanticTypeSource::Authored(
                    verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(locator.clone()),
                ),
                crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                    scope_canonical_id: owner_canonical,
                    context: transit_ctx,
                    interior_failures: None,
                },
            );
            if let Some(hot) = payload {
                harvest_role_bearing_refs_node(ctx, hot.node(), |name| {
                    record_name(name, &mut identities, &mut visited_names);
                });
            }
        }
        // Source 2: resolved_local_types — the analyzer's record of named
        // aliases reached through the macro chain. Both the chain link's
        // name AND a full walk of its raised synthesized shape contribute
        // (the shape's reference closure is the user's named composition
        // reaching to other references).
        for resolved_local in mac.resolved_local_types.iter() {
            record_name(
                resolved_local.name.as_str(),
                &mut identities,
                &mut visited_names,
            );
            let shape_hot = dispatch.raise_semantic_type_source_to_hot(
                &verter_type_expr::facts::SemanticTypeSource::Synthesized(
                    resolved_local.shape.clone(),
                ),
                crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                    scope_canonical_id: owner_canonical,
                    context: transit_ctx,
                    interior_failures: None,
                },
            );
            if let Some(hot) = shape_hot {
                harvest_role_bearing_refs_node(ctx, hot.node(), |name| {
                    record_name(name, &mut identities, &mut visited_names);
                });
            }
        }
    }

    // Source 3: OWNER-local declaration-body closure. A named alias /
    // interface the macro composes through contributes its full reference
    // closure (doc example above: `defineProps<ButtonProps>` with
    // `interface ButtonProps { a?: AvatarProps }` participates
    // `AvatarProps`). The analyzer records this chain in
    // `resolved_local_types` when it resolves the link; an owner-declared
    // interface referenced from the payload head may not be recorded there,
    // so close over the already-harvested OWNER-LOCAL identities' decl
    // bodies (raised through the one shared dispatch, carrier mode) to a
    // fixpoint. Imported identities are recorded but never descended — the
    // closure crosses no file boundary.
    let mut frontier: Vec<ResolvedRootIdentity> = identities.iter().cloned().collect();
    let mut descended: FxHashSet<ResolvedRootIdentity> = FxHashSet::default();
    while let Some(identity) = frontier.pop() {
        if identity.canonical_id != owner_canonical || !descended.insert(identity.clone()) {
            continue;
        }
        let body_source = verter_type_expr::facts::SemanticTypeSource::Authored(
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                verter_type_expr::locators::TypeBodySlot {
                    anchor: verter_type_expr::locators::AuthoredAnchor {
                        canonical_id: std::sync::Arc::from(identity.canonical_id.as_str()),
                        symbol: std::sync::Arc::from(identity.symbol_name.as_str()),
                        space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                    },
                    path: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                },
            ),
        );
        let Some(hot) = dispatch.raise_semantic_type_source_to_hot(
            &body_source,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: owner_canonical,
                context: transit_ctx,
                interior_failures: None,
            },
        ) else {
            continue;
        };
        let mut newly_recorded: Vec<String> = Vec::new();
        harvest_role_bearing_refs_node(ctx, hot.node(), |name| {
            if !visited_names.contains(name) {
                newly_recorded.push(name.to_string());
            }
            record_name(name, &mut identities, &mut visited_names);
        });
        for name in newly_recorded {
            if let Some(new_identity) = resolve_ref_to_root_identity(host, owner_canonical, &name) {
                frontier.push(new_identity);
            }
        }
    }

    identities
}

/// Walk a raised macro-payload / alias-shape node harvesting the reference
/// names the user composes the macro through. Descends structural
/// composition (aliases / unions / intersections / object PROPERTY values /
/// array elements / tuple elements / reference type-args) so references
/// inside an inline shape reach the participation set.
///
/// Does NOT walk into `IndexedAccess` at all — a value-extraction operation
/// surfaces no role-bearing references (the chain root included). Method /
/// index-signature / call-signature member values are function-shaped, not
/// role-bearing composition, and are not harvested.
///
/// Iterative (worklist + visited node-set) for stack safety on deeply
/// nested or shared shapes.
fn harvest_role_bearing_refs_node<F: FnMut(&str)>(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    root: SemanticNodeId,
    mut sink: F,
) {
    let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![root];

    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) =
            crate::resolver_core::component_meta_registry::component_meta_registry_node_ref_head(
                ctx, node,
            )
        {
            sink(name.as_str());
            // Type arguments of a reference (e.g. `Pick<MyType, K>`) ARE
            // role-bearing roots — the macro's intended type composes
            // through them.
            worklist.extend(args);
            continue;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(target) => worklist.push(*target),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                worklist.extend(arms.iter().copied());
            }
            SemanticNodeData::Array { element, .. } => worklist.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::Object(surface) => {
                // Property values surface composition references; method /
                // index-signature / call-signature members are
                // function-shaped, not role-bearing composition.
                worklist.extend(
                    surface
                        .members
                        .iter()
                        .filter(|member| !member.is_method)
                        .map(|member| member.value),
                );
            }
            // IndexedAccess is a value-extraction operation, not a
            // role-bearing reference — even the chain ROOT is not
            // harvested. `IndexKey` deliberately unused.
            SemanticNodeData::IndexedAccess {
                object: _,
                index: IndexKey::String(_) | IndexKey::Number(_) | IndexKey::TypeNode(_),
            } => {}
            // STOP — no other construct surfaces role-bearing composition
            // references (function parameter/return types, mapped /
            // conditional / keyof / typeof / template-literal / primitive /
            // literal / type-parameter / infer / opaque shapes).
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../component_meta_resolution_policy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../component_meta_resolution_policy_cycle_tests.rs"]
mod cycle_tests;
