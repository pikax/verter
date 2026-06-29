//! `host_manage::component_meta_extract` — component-meta extraction
//! free functions: snapshot → ComponentMetaAnalysis projection,
//! evaluated-type merge, and SFC sidecar population.
//!
//! Domain K. Owns the public-facing
//! `extract_component_meta_from_resolved` /
//! `extract_component_meta_from_resolved_with_facts` entry points
//! plus their internal helpers (`merge_evaluated_prop_types_into_meta`,
//! `populate_sfc_blocks_sidecar`, `populate_public_instance_sidecar`,
//! etc.). The `crate::host_manage::*` import paths used by `meta.rs`,
//! `component_meta_host.rs`, and
//! `component_meta_resolution_policy.rs` are preserved by a `pub(crate)
//! use` re-export block in the parent shell — see §11c.5.

use std::sync::Arc;

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
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let started = component_meta_debug_enabled().then(Instant::now);
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
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
    crate::host_resolve::populate_sfc_blocks_sidecar(host, &canonical, &mut meta);
    meta
}

fn merge_evaluated_prop_types_into_meta(
    host: &VerterHost,
    owner_canonical: &str,
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
) {
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
    use verter_semantic::analysis::AnalyzedMacroKind;

    let Some(evaluated_types) = evaluated_types else {
        return;
    };

    // Macro kinds whose type arguments contribute to the "props-like"
    // surface: `defineProps`, `withDefaults` (which wraps defineProps),
    // and `defineModel` (whose declared type joins the props surface).
    // The same helper is reused for emit (`&[DefineEmits]`), slot
    // (`&[DefineSlots]`), and model (`&[DefineModel]`) callers.
    let prop_macro_kinds: &[AnalyzedMacroKind] = &[
        AnalyzedMacroKind::DefineProps,
        AnalyzedMacroKind::WithDefaults,
        AnalyzedMacroKind::DefineModel,
    ];

    // Build the macro-participation index ONCE per call by reading
    // analyzer-published facts (`AnalyzedMacro.type_references`,
    // `parsed_type_argument`'s pre-recorded refs, and the
    // `resolved_local_types[i].type_expr` closure). The set keys by
    // `ResolvedRootIdentity` so the same name declared in two scopes
    // is not collapsed. Type-role classification is structural — see
    // the Typed-IR-Only Resolver Rule in CLAUDE.md.
    let macro_participating_identities =
        build_macro_participating_identities(host, owner_canonical, snapshot, prop_macro_kinds);

    /// Iterative worklist walk over a `TypeExpr` checking whether any node
    /// resolves to `target` at the requested arity.
    ///
    /// W6.1 contract: the walker MUST be iterative (no recursion — stack
    /// overflow is a real failure mode on deeply nested types like 100-level
    /// `Object` chains, deep `Conditional` ladders, Pinia/strict tuple
    /// builders) AND exhaustive over every `TypeExpr` variant that can
    /// transitively reach a `Ref`. The previous recursive walker covered
    /// only Parenthesized / Ref / Union / Intersection / Array / Tuple /
    /// IndexedAccess — Object / Function / Conditional / Mapped / KeyOf /
    /// RecursiveRef / TemplateLiteral nodes silently terminated the walk
    /// and produced false negatives.
    fn expr_contains_root_identity(
        root: &verter_type_expr::TypeExpr,
        host: &VerterHost,
        owner_canonical: &str,
        target: &ResolvedRootIdentity,
        type_argument_arity: usize,
    ) -> bool {
        // Real logic lives at module scope (`expr_contains_root_identity_impl`)
        // so it is directly unit-testable via `expr_contains_root_identity_for_test`;
        // this binding preserves the merge-path call site that references the
        // predicate by name.
        expr_contains_root_identity_impl(root, host, owner_canonical, target, type_argument_arity)
    }

    let evaluated_by_name = evaluated_types
        .props
        .iter()
        .map(|field| (field.name.as_str(), &field.r#type))
        .collect::<rustc_hash::FxHashMap<_, _>>();

    for prop in &mut meta.props {
        let Some(evaluated) = evaluated_by_name.get(prop.name.as_str()) else {
            continue;
        };
        let imported_macro_participating_refs = collect_imported_macro_participating_refs(
            host,
            owner_canonical,
            &prop.type_expr,
            &macro_participating_identities,
        );
        if !imported_macro_participating_refs.is_empty()
            && !imported_macro_participating_refs
                .iter()
                .all(|(identity, type_argument_arity)| {
                    expr_contains_root_identity(
                        evaluated,
                        host,
                        owner_canonical,
                        identity,
                        *type_argument_arity,
                    )
                })
        {
            // Allow the merge when the evaluated type is a materialized Object
            // and the current type_expr is a bare Ref — the evaluate_types path
            // already decided to materialize this imported type.
            let evaluated_is_materialized_form = matches!(
                (evaluated, &prop.type_expr),
                (
                    verter_type_expr::TypeExpr::Object(_),
                    verter_type_expr::TypeExpr::Ref {
                        type_arguments, ..
                    }
                ) if type_arguments.is_empty()
            );
            if !evaluated_is_materialized_form {
                continue;
            }
        }
        if crate::meta_resolve::compare_type_expr_improvement(evaluated, &prop.type_expr)
            || matches!(
                (&prop.type_expr, *evaluated),
                (
                    verter_type_expr::TypeExpr::Object(_),
                    verter_type_expr::TypeExpr::Union(_) | verter_type_expr::TypeExpr::Primitive(_),
                )
            )
        {
            prop.type_expr = (*evaluated).clone();
        }
    }
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
    host: &VerterHost,
    owner_canonical: &str,
    name: &str,
) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

    if host
        .local_type_declaration_id(owner_canonical, name)
        .is_some()
    {
        return Some(ResolvedRootIdentity::new(owner_canonical, name));
    }
    host.resolve_local_import_symbol_target(owner_canonical, name)
        .map(|(canonical_id, exported_name)| ResolvedRootIdentity::new(canonical_id, exported_name))
}

/// Build the set of macro-participating `ResolvedRootIdentity` values
/// for an owner file's analysis snapshot, restricted to macros whose
/// `kind` matches `macro_kinds`.
///
/// Reads ONLY analyzer-published facts:
/// - `AnalyzedMacro.type_references` — names directly declared in the
///   macro's `<Type>` argument.
/// - `AnalyzedMacro.parsed_type_argument` — the macro's typed argument
///   (`Arc<TypeExpr>`); the walker harvests every `Ref` name in the
///   subtree.
/// - `AnalyzedMacro.resolved_local_types[i].type_expr` — local-scope
///   type expansions the analyzer already linked to the macro chain;
///   every `Ref` name in the subtree contributes.
///
/// Names resolve to identities through `resolve_ref_to_root_identity`
/// (scope-aware). Each identity is added once regardless of how many
/// times its name appears.
///
/// Per the walker contract: this is an INDEX over analyzer-published
/// facts. The walker does NOT recurse into alias bodies, does NOT walk
/// the cross-file declaration graph, and does NOT trigger semantic
/// expansion. Shallow-by-default holds; semantic expansion remains the
/// consumer's lazy concern at the projector layer.
pub(crate) fn build_macro_participating_identities(
    host: &VerterHost,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    macro_kinds: &[verter_semantic::analysis::AnalyzedMacroKind],
) -> rustc_hash::FxHashSet<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
    let mut identities = rustc_hash::FxHashSet::default();

    // Per-walk visited set for raw names so a recursive type alias
    // (`type Foo = { next: Foo | null }`) is harvested exactly once
    // even when both the name and the macro chain reach it from
    // multiple anchors.
    let mut visited_names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();

    let record_name = |name: &str,
                       identities: &mut rustc_hash::FxHashSet<_>,
                       visited_names: &mut rustc_hash::FxHashSet<String>| {
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

        for type_name in mac.type_references.iter() {
            record_name(type_name.as_str(), &mut identities, &mut visited_names);
        }

        if let Some(parsed_arg) = mac.parsed_type_argument.as_ref() {
            harvest_ref_names_iterative(parsed_arg.as_ref(), |name| {
                record_name(name, &mut identities, &mut visited_names);
            });
        }

        for resolved_local in mac.resolved_local_types.iter() {
            // The local-type name itself participates (it is by
            // definition a macro chain participant — the analyzer
            // linked it).
            record_name(
                resolved_local.name.as_str(),
                &mut identities,
                &mut visited_names,
            );
            if let Some(local_expr) = resolved_local.type_expr.as_ref() {
                harvest_ref_names_iterative(local_expr, |name| {
                    record_name(name, &mut identities, &mut visited_names);
                });
            }
        }
    }

    identities
}

/// Iterative `TypeExpr` walk collecting every `Ref` name in the
/// subtree. Stack-overflow safe for deeply nested object/intersection
/// types — the dedicated termination test exercises a programmatic
/// 100-level nest.
///
/// Visited pointer-set guards against shared sub-expression revisits
/// when the same `TypeExpr` node appears under multiple parents in a
/// shared `Arc`-rooted tree.
fn harvest_ref_names_iterative<F: FnMut(&str)>(root: &verter_type_expr::TypeExpr, mut sink: F) {
    use verter_type_expr::TypeExpr;

    let mut visited: rustc_hash::FxHashSet<*const TypeExpr> = rustc_hash::FxHashSet::default();
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
            TypeExpr::IndexedAccess { object, index } => {
                worklist.push(object);
                worklist.push(index);
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    match member {
                        verter_type_expr::ObjectMember::Property(prop) => worklist.push(&prop.ty),
                        verter_type_expr::ObjectMember::Method(method) => {
                            for param in method.function.parameters.iter() {
                                worklist.push(&param.ty);
                            }
                            if let Some(ret) = method.function.return_type.as_ref() {
                                worklist.push(ret.as_ref());
                            }
                        }
                        verter_type_expr::ObjectMember::IndexSignature(idx) => {
                            worklist.push(&idx.key_type);
                            worklist.push(&idx.value_type);
                        }
                        verter_type_expr::ObjectMember::CallSignature(func)
                        | verter_type_expr::ObjectMember::ConstructSignature(func) => {
                            for param in func.parameters.iter() {
                                worklist.push(&param.ty);
                            }
                            if let Some(ret) = func.return_type.as_ref() {
                                worklist.push(ret.as_ref());
                            }
                        }
                    }
                }
            }
            // A function type and a bare constructor type (`new (...) => R`)
            // carry the same `FunctionExpr` payload; both expose their
            // parameter and return types to the reachability walk.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                for param in func.parameters.iter() {
                    worklist.push(&param.ty);
                }
                if let Some(ret) = func.return_type.as_ref() {
                    worklist.push(ret.as_ref());
                }
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                worklist.push(check);
                worklist.push(extends);
                worklist.push(true_type);
                worklist.push(false_type);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                worklist.push(source);
                worklist.push(value);
                if let Some(nt) = name_type.as_ref() {
                    worklist.push(nt.as_ref());
                }
            }
            TypeExpr::KeyOf(inner) => worklist.push(inner),
            TypeExpr::Rest(inner) => worklist.push(inner),
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
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for ty in expressions.iter() {
                    worklist.push(ty);
                }
            }
            _ => {}
        }
    }
}

/// Iterative worklist walk over a `TypeExpr` checking whether any node
/// resolves to `target` at the requested arity.
///
/// The walker MUST be iterative (no recursion — stack overflow is a real
/// failure mode on deeply nested types like 100-level `Object` chains,
/// deep `Conditional` ladders, strict tuple builders) AND exhaustive over
/// every `TypeExpr` variant that can transitively reach a `Ref`.
///
/// A function type and a bare constructor type (`new (...) => R`) carry the
/// same `FunctionExpr` payload; both expose their parameter and return
/// types to the reachability walk.
pub(in crate::host_manage) fn expr_contains_root_identity_impl(
    root: &verter_type_expr::TypeExpr,
    host: &VerterHost,
    owner_canonical: &str,
    target: &verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    type_argument_arity: usize,
) -> bool {
    use verter_type_expr::TypeExpr;

    let mut visited: rustc_hash::FxHashSet<*const TypeExpr> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<&TypeExpr> = vec![root];

    while let Some(node) = worklist.pop() {
        if !visited.insert(node as *const TypeExpr) {
            continue;
        }
        match node {
            TypeExpr::Parenthesized(inner) => worklist.push(inner),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if type_arguments.len() == type_argument_arity {
                    if let Some(identity) =
                        resolve_ref_to_root_identity(host, owner_canonical, name.as_ref())
                    {
                        if identity == *target {
                            return true;
                        }
                    }
                }
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
            TypeExpr::IndexedAccess { object, index } => {
                worklist.push(object);
                worklist.push(index);
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    match member {
                        verter_type_expr::ObjectMember::Property(prop) => worklist.push(&prop.ty),
                        verter_type_expr::ObjectMember::Method(method) => {
                            for param in method.function.parameters.iter() {
                                worklist.push(&param.ty);
                            }
                            if let Some(ret) = method.function.return_type.as_ref() {
                                worklist.push(ret.as_ref());
                            }
                        }
                        verter_type_expr::ObjectMember::IndexSignature(idx) => {
                            worklist.push(&idx.key_type);
                            worklist.push(&idx.value_type);
                        }
                        verter_type_expr::ObjectMember::CallSignature(func)
                        | verter_type_expr::ObjectMember::ConstructSignature(func) => {
                            for param in func.parameters.iter() {
                                worklist.push(&param.ty);
                            }
                            if let Some(ret) = func.return_type.as_ref() {
                                worklist.push(ret.as_ref());
                            }
                        }
                    }
                }
            }
            // A function type and a bare constructor type (`new (...) => R`)
            // carry the same `FunctionExpr` payload; both expose their
            // parameter and return types to the reachability walk.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                for param in func.parameters.iter() {
                    worklist.push(&param.ty);
                }
                if let Some(ret) = func.return_type.as_ref() {
                    worklist.push(ret.as_ref());
                }
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                worklist.push(check);
                worklist.push(extends);
                worklist.push(true_type);
                worklist.push(false_type);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                worklist.push(source);
                worklist.push(value);
                if let Some(nt) = name_type.as_ref() {
                    worklist.push(nt.as_ref());
                }
            }
            TypeExpr::KeyOf(inner) => worklist.push(inner),
            TypeExpr::Rest(inner) => worklist.push(inner),
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                ..
            } => {
                if type_arguments.len() == type_argument_arity {
                    if let Some(identity) =
                        resolve_ref_to_root_identity(host, owner_canonical, name.as_ref())
                    {
                        if identity == *target {
                            return true;
                        }
                    }
                }
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for ty in expressions.iter() {
                    worklist.push(ty);
                }
            }
            _ => {}
        }
    }

    false
}

/// Walk `expr` (typed) collecting every `Ref` name with arity that
/// resolves to a macro-participating identity in `participating`.
///
/// The walker is iterative (worklist-based) to avoid stack overflow on
/// deeply nested types — see W6.1 deep-nesting termination test.
/// Visited pointer-set prevents re-resolving the same `TypeExpr` node,
/// and visited identity-set deduplicates the result set per call.
///
/// Cross-file resolution lookups (`resolve_ref_to_root_identity`) hit
/// the shared host cache; no fresh resolver instance is constructed.
fn collect_imported_macro_participating_refs(
    host: &VerterHost,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
    participating: &rustc_hash::FxHashSet<
        verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    >,
) -> rustc_hash::FxHashSet<(
    verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    usize,
)> {
    use verter_type_expr::TypeExpr;

    let mut out = rustc_hash::FxHashSet::default();
    if participating.is_empty() {
        return out;
    }

    let mut visited: rustc_hash::FxHashSet<*const TypeExpr> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<&TypeExpr> = vec![expr];

    while let Some(node) = worklist.pop() {
        if !visited.insert(node as *const TypeExpr) {
            continue;
        }
        match node {
            TypeExpr::Parenthesized(inner) => worklist.push(inner),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(identity) =
                    resolve_ref_to_root_identity(host, owner_canonical, name.as_ref())
                {
                    if participating.contains(&identity) && identity.canonical_id != owner_canonical
                    {
                        out.insert((identity, type_arguments.len()));
                    }
                }
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
            TypeExpr::IndexedAccess { object, index } => {
                worklist.push(object);
                worklist.push(index);
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    match member {
                        verter_type_expr::ObjectMember::Property(prop) => worklist.push(&prop.ty),
                        verter_type_expr::ObjectMember::Method(method) => {
                            for param in method.function.parameters.iter() {
                                worklist.push(&param.ty);
                            }
                            if let Some(ret) = method.function.return_type.as_ref() {
                                worklist.push(ret.as_ref());
                            }
                        }
                        verter_type_expr::ObjectMember::IndexSignature(idx) => {
                            worklist.push(&idx.key_type);
                            worklist.push(&idx.value_type);
                        }
                        verter_type_expr::ObjectMember::CallSignature(func)
                        | verter_type_expr::ObjectMember::ConstructSignature(func) => {
                            for param in func.parameters.iter() {
                                worklist.push(&param.ty);
                            }
                            if let Some(ret) = func.return_type.as_ref() {
                                worklist.push(ret.as_ref());
                            }
                        }
                    }
                }
            }
            // A function type and a bare constructor type (`new (...) => R`)
            // carry the same `FunctionExpr` payload; both expose their
            // parameter and return types to the reachability walk.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                for param in func.parameters.iter() {
                    worklist.push(&param.ty);
                }
                if let Some(ret) = func.return_type.as_ref() {
                    worklist.push(ret.as_ref());
                }
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                worklist.push(check);
                worklist.push(extends);
                worklist.push(true_type);
                worklist.push(false_type);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                worklist.push(source);
                worklist.push(value);
                if let Some(nt) = name_type.as_ref() {
                    worklist.push(nt.as_ref());
                }
            }
            TypeExpr::KeyOf(inner) => worklist.push(inner),
            TypeExpr::Rest(inner) => worklist.push(inner),
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                ..
            } => {
                if let Some(identity) =
                    resolve_ref_to_root_identity(host, owner_canonical, name.as_ref())
                {
                    if participating.contains(&identity) && identity.canonical_id != owner_canonical
                    {
                        out.insert((identity, type_arguments.len()));
                    }
                }
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for ty in expressions.iter() {
                    worklist.push(ty);
                }
            }
            _ => {}
        }
    }

    out
}

fn build_public_instance_slot_type(
    slot: &verter_semantic::analysis::component_meta::SlotAnalysis,
) -> verter_type_expr::TypeExpr {
    let parameter_type =
        verter_type_expr::TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: slot
                .bindings
                .iter()
                .map(|binding| {
                    verter_type_expr::ObjectMember::Property(
                        verter_type_expr::ObjectProperty::synthetic_public(
                            binding.name.clone(),
                            binding.type_expr.clone(),
                            false,
                            false,
                        ),
                    )
                })
                .collect(),
        }));
    // Typed-IR-only: read the analyzer-populated `return_expr` directly.
    // `return_type` is display-only; we never reparse it. See the
    // Typed-IR-Only Resolver Rule in CLAUDE.md.
    let return_type = slot
        .return_expr
        .clone()
        .unwrap_or(verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Unknown,
        ));
    let function =
        verter_type_expr::TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::synthetic(
            if slot.bindings.is_empty() {
                Vec::new()
            } else {
                vec![verter_type_expr::FunctionParam::synthetic(
                    Some("props".to_string()),
                    parameter_type,
                    false,
                    false,
                )]
            },
            Some(Arc::new(return_type)),
            Vec::new(),
        )));
    if slot.is_required {
        function
    } else {
        verter_type_expr::TypeExpr::union(vec![
            function,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Undefined),
        ])
    }
}

fn build_public_instance_slots_member(
    slots: &[verter_semantic::analysis::component_meta::SlotAnalysis],
) -> verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
    let slot_properties = slots
        .iter()
        .map(|slot| {
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    slot.name.clone(),
                    build_public_instance_slot_type(slot),
                    !slot.is_required,
                    false,
                ),
            )
        })
        .collect();

    verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
        name: "$slots".to_string(),
        kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::SlotContainer,
        type_expr: verter_type_expr::TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: slot_properties,
        })),
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
            type_expr: prop.type_expr.clone(),
            type_expansion: prop.type_expansion.clone(),
            raw_type: prop.raw_type.clone(),
            description: prop.description.clone(),
            tags: prop.tags.clone(),
        }
    }));

    for exposed in &meta.exposed {
        let next = verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
            name: exposed.name.clone(),
            kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed,
            type_expr: exposed.type_expr.clone(),
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

pub(crate) fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    include_fallthrough: bool,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) -> (
    verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    crate::semantic_query::ResultCompleteness,
) {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    // The macro-DTO surface read (`vue_macro_dtos_with_ctx` ->
    // `ctx.store_view()`) MUST run under the request-bound `ctx`, not the
    // bare `&VerterHost` rail (whose `store_view()` panics in a
    // `debug_assertions`-off build). See
    // `tests/cases/g_session/session_meta_store_view_regression.rs`.
    let resolved_macros = resolver_component_meta_resolved_macros(
        ctx,
        canonical.as_str(),
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
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
    );
    merge_evaluated_prop_types_into_meta(
        host,
        canonical.as_str(),
        &mut meta,
        &resolved.snapshot,
        resolved.evaluated_types.as_ref(),
    );
    // The fallthrough cold compute runs AFTER `resolved` was produced, so a
    // fallthrough-only partial (a budget/fuse trip, a fatal semantic read, a
    // same-path truncation) is too late for `resolved.synthesis_should_suppress`.
    // Capture its COMPUTE completeness here through the shared per-cold-compute
    // scope (the walker fold + the dispatch-level read suppressors fold into it)
    // and carry it up so the outer publish gate refuses a partial. This is the
    // COMPUTE completeness, NOT the surface-shape `accepted_surface_completeness`
    // (a representable `LowerBound` surface stays cacheable).
    let mut fallthrough_completeness = crate::semantic_query::ResultCompleteness::Complete;
    if include_fallthrough {
        let mut visiting = rustc_hash::FxHashSet::default();
        // The completeness travels WITH the resolution via the outcome carrier
        // (centralised per-call scope), so a stale partial from a discarded
        // completion-fence retry cannot taint this attempt.
        let outcome = host.compute_fallthrough_outcome_from_resolved_state(
            &canonical,
            resolved,
            None,
            &mut visiting,
            ctx,
        );
        if let Some(resolution) = outcome.resolution {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        }
        fallthrough_completeness = outcome.completeness;
    }
    // apply the publication policy over (resolved_type_registry,
    // resolved_type_registry_meta) + snapshot.macros (§3.4 structural
    // classification); see docs/arch/debt-closure/06-step4b-consumer-surface.md.
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
    (meta, fallthrough_completeness)
}

/// Like [`extract_component_meta_from_resolved`] with `include_fallthrough=true`,
/// but also returns the fallthrough resolution's fact versions (if available).
/// Used by the payload cache to store Full payloads with the correct fact set.
pub(crate) fn extract_component_meta_from_resolved_with_facts(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
) -> (
    verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    Option<Vec<crate::resolver_core::FactVersionRef>>,
    crate::semantic_query::ResultCompleteness,
) {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    // Macro-DTO surface read runs under the request-bound `ctx` (not the
    // bare host) — see the sibling `extract_component_meta_from_resolved`
    // and `tests/cases/g_session/session_meta_store_view_regression.rs`.
    let resolved_macros = resolver_component_meta_resolved_macros(
        ctx,
        canonical.as_str(),
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
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
    );
    merge_evaluated_prop_types_into_meta(
        host,
        canonical.as_str(),
        &mut meta,
        &resolved.snapshot,
        resolved.evaluated_types.as_ref(),
    );
    let mut visiting = rustc_hash::FxHashSet::default();
    // The outcome carrier centralises the per-call completeness scope: a
    // fallthrough partial folds in so the fallthrough's OWN caches
    // (`store_node`) self-gate on the typed completeness signal, and the
    // captured completeness is RETURNED so the payload-write gate refuses to
    // warm a fallthrough partial (matching the analysis surface's result-cache
    // gate). The completeness travels with the resolution, so a discarded
    // completion-fence retry cannot taint this attempt.
    let (fallthrough_facts, fallthrough_completeness) = {
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
        (facts, outcome.completeness)
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
    (meta, fallthrough_facts, fallthrough_completeness)
}

/// Test-only entry point that exercises `harvest_ref_names_iterative`
/// without requiring host state. Used by the deep-nesting termination
/// characterisation test.
#[cfg(test)]
pub(in crate::host_manage) fn harvest_ref_names_for_test<F: FnMut(&str)>(
    root: &verter_type_expr::TypeExpr,
    sink: F,
) {
    harvest_ref_names_iterative(root, sink)
}

/// Test-only entry point that exercises `expr_contains_root_identity` —
/// the merge-gate reachability walker. Pins that the walker descends into
/// a bare constructor type's parameter and return types (function-like),
/// not just a plain function type.
#[cfg(test)]
pub(in crate::host_manage) fn expr_contains_root_identity_for_test(
    root: &verter_type_expr::TypeExpr,
    host: &VerterHost,
    owner_canonical: &str,
    target: &verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    type_argument_arity: usize,
) -> bool {
    expr_contains_root_identity_impl(root, host, owner_canonical, target, type_argument_arity)
}

/// Test-only entry point that exercises `resolve_ref_to_root_identity`
/// for the scope-correctness characterisation test.
#[cfg(test)]
pub(in crate::host_manage) fn resolve_ref_to_root_identity_for_test(
    host: &VerterHost,
    owner_canonical: &str,
    name: &str,
) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
    resolve_ref_to_root_identity(host, owner_canonical, name)
}

/// Test-only entry point that exercises
/// `build_macro_participating_identities`. Pins the structural macro-
/// participation contract: a type is a participant because a Vue SFC
/// macro (`defineProps`, `defineEmits`, ...) consumes its declaration
/// — NOT because its identifier suffix is `Props` / `Emits` / etc.
/// See the Typed-IR-Only Resolver Rule in CLAUDE.md and the
/// `/component-meta` skill.
#[cfg(test)]
pub(in crate::host_manage) fn build_macro_participating_identities_for_test(
    host: &VerterHost,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    macro_kinds: &[verter_semantic::analysis::AnalyzedMacroKind],
) -> rustc_hash::FxHashSet<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
    build_macro_participating_identities(host, owner_canonical, snapshot, macro_kinds)
}

/// Test-only entry point that exercises
/// `collect_imported_macro_participating_refs` for the positive
/// macro-participation characterisation test.
#[cfg(test)]
pub(in crate::host_manage) fn collect_imported_macro_participating_refs_for_test(
    host: &VerterHost,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
    participating: &rustc_hash::FxHashSet<
        verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    >,
) -> rustc_hash::FxHashSet<(
    verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    usize,
)> {
    collect_imported_macro_participating_refs(host, owner_canonical, expr, participating)
}

/// Test-only entry point that exercises `build_public_instance_slot_type`.
/// Pins the typed-IR contract: `SlotAnalysis.return_expr` (typed) is the
/// authority — `SlotAnalysis.return_type` (display string) MUST NOT feed
/// semantic decisions. See the Typed-IR-Only Resolver Rule in CLAUDE.md.
#[cfg(test)]
pub(in crate::host_manage) fn build_public_instance_slot_type_for_test(
    slot: &verter_semantic::analysis::component_meta::SlotAnalysis,
) -> verter_type_expr::TypeExpr {
    build_public_instance_slot_type(slot)
}

#[cfg(test)]
#[path = "component_meta_extract_tests.rs"]
mod component_meta_extract_tests;
