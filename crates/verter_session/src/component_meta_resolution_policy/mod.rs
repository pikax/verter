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
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_semantic::analysis::AnalyzedMacroKind;
use verter_type_expr::TypeExpr;

use crate::host_manage::component_meta_extract::resolve_ref_to_root_identity;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::ComponentMetaQueryEngine;
use crate::types::FileAnalysisSnapshot;
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
/// `snapshot` is the owner SFC's analyzer snapshot. The policy reads
/// `snapshot.macros` to build the structural macro-participation
/// classifier (§3.4 Typed-IR-Only Resolver Rule) — types are
/// "role-bearing" because a Vue SFC macro (`defineProps` /
/// `defineEmits` / `defineModel` / `defineSlots` / `withDefaults`)
/// consumes them, NOT because their identifier name ends in `"Props"`
/// or similar. Tests with no snapshot may pass `None` — the classifier
/// set is then empty (no Refs are classified as macro-participating, so
/// Rules 2 / 4 + the raw-restoration helpers never fire); production
/// callsites always supply the snapshot via the resolved-state.
///
/// The pass is host-bounded but never invokes dispatch — it walks the
/// already-resolved registry plus on-demand declaration metadata.
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
        Some(snap) => {
            build_policy_macro_role_identities(host, owner_canonical, snap, TYPE_ROLE_MACRO_KINDS)
        }
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

/// Build the policy's role-bearing identity set: §3.4 structural
/// macro-participation classifier scoped to **the macro's named
/// composition closure**.
///
/// Two sources contribute:
///
/// 1. `parsed_type_argument` — the macro's type argument. Walks
///    through `Parenthesized` / `Union` / `Intersection` / inline
///    `Object` property types / `Array` element types / `Tuple`
///    element types / `Ref`-with-type-args. Surfaces every named
///    composition ref the user references through the macro shape.
///    **Stops at `IndexedAccess` indices** — only the chain ROOT
///    contributes. This preserves path-precise materialisation for
///    `Foo['member']` extracts: `Foo` is role-bearing, but the
///    sub-types reached via the indexed-access chain are not.
/// 2. `resolved_local_types[i].name` AND every `Ref` in
///    `resolved_local_types[i].type_expr` (deep walk). The analyzer
///    records named local aliases reached through the macro chain;
///    their full Ref closures contribute because the user named the
///    composition.
///
/// Examples (with `defineProps<X>()`):
/// - `X = Foo`                          → `{ Foo }`
/// - `X = Foo<Bar>`                     → `{ Foo, Bar }`
/// - `X = Pick<MyType, K>`              → `{ Pick, MyType, K }`
/// - `X = Foo & Bar`                    → `{ Foo, Bar }`
/// - `X = { avatar?: AvatarProps }`     → `{ AvatarProps }` (Object
///   properties surface composition refs)
/// - `X = { ui?: Button['slots'] }`     → `{}` (IndexedAccess is a
///   value-extraction operation, not role-bearing reference; the
///   materialiser resolves `Button.slots` path-precisely)
/// - `X = Foo['a']['b']`                → `{}` (same — value
///   extraction)
/// - `X = ButtonProps` with `type ButtonProps = { a?: AvatarProps }`
///   → `{ ButtonProps, AvatarProps }` (named alias body contributes
///   its full Ref closure)
fn build_policy_macro_role_identities(
    host: &VerterHost,
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

    for mac in snapshot.macros.iter() {
        if !macro_kinds.contains(&mac.kind) {
            continue;
        }
        // Source 1: parsed_type_argument — walk only the structural
        // skeleton (no descent into Object/Array/Tuple/Function inner
        // types or IndexedAccess indices).
        if let Some(parsed_arg) = mac.parsed_type_argument.as_ref() {
            harvest_macro_arg_skeleton_refs(parsed_arg.as_ref(), |name| {
                record_name(name, &mut identities, &mut visited_names);
            });
        }
        // Source 2: resolved_local_types — the analyzer's record of
        // named aliases reached through the macro chain. Both the
        // chain link's name AND a full deep walk of its expanded
        // body contribute (the body's Ref closure is the user's
        // named composition reaching to other Refs).
        for resolved_local in mac.resolved_local_types.iter() {
            record_name(
                resolved_local.name.as_str(),
                &mut identities,
                &mut visited_names,
            );
            if let Some(local_expr) = resolved_local.type_expr.as_ref() {
                harvest_named_alias_body_refs(local_expr, |name| {
                    record_name(name, &mut identities, &mut visited_names);
                });
            }
        }
    }

    identities
}

/// Walk a macro's type argument harvesting `Ref` names that the user
/// composes the macro through. Walks through structural shapes
/// (`Parenthesized` / `Union` / `Intersection` / `Object`-property /
/// `Array`-element / `Tuple`-element) so refs inside an inline shape
/// (e.g. `{ avatar?: AvatarProps }`) reach the participation set.
///
/// Does NOT walk into `IndexedAccess` at all — when the user writes
/// `defineProps<Foo['member']>` or `{ k?: Foo['member'] }`, `Foo` is
/// being consumed for value extraction; the role-bearing prop value
/// type is `Foo.member`, which the materialiser resolves
/// path-precisely. Neither `Foo` nor any sub-type reached through the
/// chain contributes to the role-bearing set.
///
/// Iterative (worklist + visited pointer-set) for stack safety on
/// deeply nested shapes.
fn harvest_macro_arg_skeleton_refs<F: FnMut(&str)>(root: &TypeExpr, mut sink: F) {
    let mut visited: FxHashSet<*const TypeExpr> = FxHashSet::default();
    let mut worklist: Vec<&TypeExpr> = vec![root];

    while let Some(expr) = worklist.pop() {
        if !visited.insert(expr as *const TypeExpr) {
            continue;
        }
        match expr {
            TypeExpr::Parenthesized(inner) => worklist.push(inner),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                sink(name.as_ref());
                // Type arguments of a Ref (e.g. `Pick<MyType, K>`)
                // ARE role-bearing roots — the macro's intended type
                // composes through them.
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter() {
                    worklist.push(ty);
                }
            }
            TypeExpr::Array { element, .. } => worklist.push(element),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter() {
                    worklist.push(&element.ty);
                }
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    if let verter_type_expr::ObjectMember::Property(prop) = member {
                        worklist.push(&prop.ty);
                    }
                    // Method / IndexSignature / CallSignature /
                    // ConstructSignature member types are NOT harvested
                    // — those are function-shaped, not role-bearing
                    // composition.
                }
            }
            TypeExpr::IndexedAccess { .. } => {
                // IndexedAccess is a value-extraction operation, not a
                // role-bearing reference. Even the chain ROOT is NOT
                // harvested — when the user writes
                // `defineProps<Foo['mem']>` or
                // `defineProps<{ k?: Foo['mem'] }>()`, `Foo` is being
                // consumed for value extraction; `Foo.mem` is the
                // role-bearing prop value type, and the materialiser
                // resolves it path-precisely.
            }
            // STOP — these constructs do not surface role-bearing
            // composition refs:
            // - Function parameter/return types
            // - Mapped / Conditional / KeyOf / TypeOf / Rest /
            //   TemplateLiteral / Primitive / Literal / TypeParameter
            //   / Infer / Unknown / RecursiveRef
            _ => {}
        }
    }
}

/// Walk a named alias body harvesting `Ref` names in the user's
/// named composition closure. Used for `resolved_local_types[i].type_expr`
/// walks — the user named the alias chain, so refs in the body
/// participate in the role-bearing composition.
///
/// Walks through `Parenthesized` / `Union` / `Intersection` / `Object`
/// property types / `Array` element / `Tuple` element / `Ref`
/// type-args. Does NOT walk into `IndexedAccess` — value-extraction
/// operations don't surface role-bearing refs (consistent with
/// `harvest_macro_arg_skeleton_refs`).
///
/// Iterative (worklist + visited pointer-set) for stack safety.
fn harvest_named_alias_body_refs<F: FnMut(&str)>(root: &TypeExpr, mut sink: F) {
    let mut visited: FxHashSet<*const TypeExpr> = FxHashSet::default();
    let mut worklist: Vec<&TypeExpr> = vec![root];

    while let Some(expr) = worklist.pop() {
        if !visited.insert(expr as *const TypeExpr) {
            continue;
        }
        match expr {
            TypeExpr::Parenthesized(inner) => worklist.push(inner),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                sink(name.as_ref());
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter() {
                    worklist.push(ty);
                }
            }
            TypeExpr::Array { element, .. } => worklist.push(element),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter() {
                    worklist.push(&element.ty);
                }
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    if let verter_type_expr::ObjectMember::Property(prop) = member {
                        worklist.push(&prop.ty);
                    }
                    // Method / IndexSignature / CallSignature /
                    // ConstructSignature member types are function-
                    // shaped, not role-bearing composition.
                }
            }
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                ..
            } => {
                sink(name.as_ref());
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
            }
            // STOP — these constructs do not surface role-bearing
            // composition refs:
            // - IndexedAccess (value-extraction operation)
            // - Function parameter/return types
            // - Mapped / Conditional / KeyOf / TypeOf / Rest /
            //   TemplateLiteral / Primitive / Literal / TypeParameter
            //   / Infer / Unknown
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
