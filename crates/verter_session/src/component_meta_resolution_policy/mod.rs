//! Component-meta publication policy.
//!
//! Rehomes the externally-observable resolution policy that the deleted
//! `choose_less_symbolic_component_meta_type_expr` /
//! `rematerialize_public_component_meta_types` enforced in
//! `host_manage.rs` (commit `624b14d2` removed both).
//!
//! The pass operates on a dispatch-resolved [`ComponentMetaAnalysis`] and
//! mutates the public type surfaces in place per the rules derived in
//! `docs/arch/debt-closure/06-step4b-consumer-surface.md`.
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
//! * Recursion across compound types (Array/Tuple/Union/Intersection/Object/
//!   Function/IndexedAccess/Conditional/Mapped/KeyOf) — Rule 5.

use rustc_hash::FxHashSet;
use verter_semantic::analysis::component_meta::{ComponentMetaAnalysis, ResolvedTypeAnalysis};

use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::ComponentMetaQueryEngine;
use crate::VerterHost;

mod core;
mod cycle_guard;
mod pick_omit;
pub(crate) mod policy_helpers;
mod raw_restoration;
mod slot_preservation;

use self::core::{rewrite_in_place, PolicyCtx, PolicyRegistry};
use self::raw_restoration::restore_props_suffix_from_raw;
use self::slot_preservation::slot_binding_should_preserve_symbolic_raw_type;

/// Apply the publication policy to `analysis`, rewriting public type surfaces
/// in place per the rules in
/// `docs/arch/debt-closure/06-step4b-consumer-surface.md`.
///
/// `host` is consulted for cross-file declaration lookup of transitively-
/// referenced types not seeded into the BFS root set of the resolved type
/// registry (e.g. `Status` referenced from `ExternalProps.status: Status`
/// when `ExternalProps` is the only macro-arg root). Lookups go through
/// [`ComponentMetaQueryEngine`] which delegates to the host-owned typed DBs
/// populated by Step 3 of the debt-closure plan, so warm hits are O(1).
///
/// The pass is host-bounded but never invokes dispatch — it walks the
/// already-resolved registry plus on-demand declaration metadata.
pub fn apply_component_meta_resolution_policy(
    analysis: &mut ComponentMetaAnalysis,
    type_registry: &[ResolvedTypeAnalysis],
    type_registry_meta: &[ResolvedTypeRegistryMeta],
    host: &VerterHost,
    owner_canonical: &str,
) {
    let registry = PolicyRegistry::build(type_registry, type_registry_meta);
    let mut engine = ComponentMetaQueryEngine::new(host);
    let mut ctx = PolicyCtx {
        registry: &registry,
        engine: &mut engine,
        owner_canonical,
        host,
        active_refs: FxHashSet::default(),
        active_refs_max_depth: 0,
    };

    let mut changed = false;

    for prop in analysis.props.iter_mut() {
        // Pre-step: restore *Props refs the evaluator may have eagerly
        // resolved away. The deleted `imported_props_like_public_raw_type`
        // helper used the raw type annotation as the canonical form for
        // *Props imports — re-instate that contract before the rule walk.
        if restore_props_suffix_from_raw(&mut prop.type_expr, prop.raw_type_expr.as_ref(), &mut ctx)
        {
            changed = true;
        }
        if rewrite_in_place(&mut prop.type_expr, &mut ctx) {
            changed = true;
        }
    }
    for event in analysis.events.iter_mut() {
        if rewrite_in_place(&mut event.payload, &mut ctx) {
            changed = true;
        }
    }
    for slot in analysis.slots.iter_mut() {
        for binding in slot.bindings.iter_mut() {
            // Issue #1 (partial): when the binding's raw type is an
            // `IndexedAccess` whose deref chain transits through an
            // imported declaration, force the symbolic raw form back
            // onto the published `type_expr` and skip the expansion
            // walk. The eager evaluator may have widened the indexed
            // access through an open `[k: string]: any` index
            // signature; the consumer is better served by the
            // navigable `AppProps['avatar']` member-path contract.
            if slot_binding_should_preserve_symbolic_raw_type(
                binding.raw_type_expr.as_ref(),
                &mut ctx,
            ) {
                // The guard already confirmed the typed annotation is an
                // `IndexedAccess` whose root resolves through an imported
                // declaration; restore that exact shape onto the public
                // surface (it's necessarily `Some` here).
                if let Some(restored) = binding.raw_type_expr.as_ref() {
                    if &binding.type_expr != restored {
                        binding.type_expr = restored.clone();
                        changed = true;
                    }
                }
                continue;
            }
            if restore_props_suffix_from_raw(
                &mut binding.type_expr,
                binding.raw_type_expr.as_ref(),
                &mut ctx,
            ) {
                changed = true;
            }
            if rewrite_in_place(&mut binding.type_expr, &mut ctx) {
                changed = true;
            }
        }
    }
    for model in analysis.models.iter_mut() {
        if rewrite_in_place(&mut model.type_expr, &mut ctx) {
            changed = true;
        }
    }
    for exposed in analysis.exposed.iter_mut() {
        if rewrite_in_place(&mut exposed.type_expr, &mut ctx) {
            changed = true;
        }
    }
    for accepted in analysis.accepted_props.iter_mut() {
        if restore_props_suffix_from_raw(
            &mut accepted.type_expr,
            accepted.raw_type_expr.as_ref(),
            &mut ctx,
        ) {
            changed = true;
        }
        if rewrite_in_place(&mut accepted.type_expr, &mut ctx) {
            changed = true;
        }
    }
    for accepted in analysis.accepted_events.iter_mut() {
        if rewrite_in_place(&mut accepted.payload, &mut ctx) {
            changed = true;
        }
    }

    if changed {
        crate::host_manage::populate_public_instance_sidecar(analysis);
    }
}

#[cfg(test)]
#[path = "../component_meta_resolution_policy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../component_meta_resolution_policy_cycle_tests.rs"]
mod cycle_tests;
