//! Named-type / interface / class / heritage resolution.
//!
//! This is the substrate the public `resolve_type_elements*` API delegates
//! to. The functions here own the recursive walks that the audit's SCC
//! analysis flagged: `interface extends`, `class extends`, the
//! `Pick`/`Omit`/`Partial`/... heritage utilities, and the body-projection
//! loop that lifts a `TSType` into `ResolvedElements`.
//!
//! Everything stays consistent with the kernel's five query modes (Identity,
//! Navigate, Shallow, Expanded, Skeleton — see `/type-resolution`); each
//! function is the named-type or class-heritage variant of one of those
//! modes. Generic substitutions enter through `instantiate_type_params_ctx`
//! (still in `super::mod.rs`) so cache identities stay in sync.
//!
//! Public entry points remain on `super::mod.rs`; the helpers here are
//! `pub(super)` so the rest of the resolver can drive them through ordinary
//! `use` statements.

use std::sync::Arc;

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use crate::common::Span;

use super::{
    callable_signature_text, component_meta_core_trace_enabled, component_meta_core_trace_event,
    extract_string_literal_keys_with_ctx, get_property_key_name, get_property_key_span,
    get_type_reference_name, has_immediate_vue_ignore_comment, infer_runtime_type,
    instantiate_type_params_ctx, resolve_mapped_type_with_ctx, resolve_type_elements_with_ctx_ref,
    resolve_type_literal_members, resolve_type_literal_members_with_ctx, span_text,
    ClassResolutionPlan, DiagnosticLocation, InterfaceResolutionPlan, NamedTypeHeritageEdge,
    NamedTypeResolutionPlan, ResolutionDepthGuard, ResolutionDiagnostic, ResolutionDiagnosticKind,
    ResolvedElements, ResolvedMemberVisibility, ResolvedProp, RuntimeType, TypeResolutionContext,
};

pub(super) fn inferred_root_runtime_type_for_companion(
    companion: &ResolvedElements,
) -> Vec<RuntimeType> {
    if !companion.root_runtime_types.is_empty() {
        return companion.root_runtime_types.clone();
    }
    if !companion.props.is_empty() || !companion.call_signatures.is_empty() {
        return vec![RuntimeType::Object];
    }
    if companion.has_call_signature {
        return vec![RuntimeType::Function];
    }
    vec![RuntimeType::Unknown]
}

pub(super) fn resolve_root_runtime_type_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<Vec<RuntimeType>> {
    match node {
        TSType::TSTypeReference(type_ref) => {
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            if let Some((aliased_type, _)) = ctx.find_type_alias(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx(aliased_type, ctx)
                        .unwrap_or_else(|| infer_runtime_type(aliased_type)),
                );
            }

            if ctx.find_interface(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if ctx.find_class(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx(constraint, ctx)
                        .unwrap_or_else(|| infer_runtime_type(constraint)),
                );
            }

            ctx.companion_types
                .get(type_name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        TSType::TSTypeQuery(query) => {
            let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
                return None;
            };
            ctx.companion_types
                .get(ident.name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        _ => None,
    }
}

pub(super) fn resolve_root_runtime_type_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<Vec<RuntimeType>> {
    match node {
        TSType::TSTypeReference(type_ref) => {
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            if let Some((aliased_type, _)) = ctx.find_type_alias(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx_ref(aliased_type, ctx)
                        .unwrap_or_else(|| infer_runtime_type(aliased_type)),
                );
            }

            if ctx.find_interface(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if ctx.find_class(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx_ref(constraint, ctx)
                        .unwrap_or_else(|| infer_runtime_type(constraint)),
                );
            }

            ctx.companion_types
                .get(type_name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        TSType::TSTypeQuery(query) => {
            let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
                return None;
            };
            ctx.companion_types
                .get(ident.name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        _ => None,
    }
}

pub(super) fn resolve_named_local_type_with_ctx_ref<'ctx, 'a: 'ctx>(
    type_name: &str,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
    recursion_guard: &mut Vec<String>,
) -> Option<Arc<ResolvedElements>> {
    let type_name_bytes = type_name.as_bytes();

    if let Some((aliased_type, type_params)) = ctx.find_type_alias(type_name_bytes) {
        let child = instantiate_type_params_ctx(ctx, type_params, type_args);
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=alias name={} from_root_body={} bindings={} companions={}",
                    child.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    from_root_body,
                    child.type_param_bindings.len(),
                    child.companion_types.len()
                ),
            );
        }
        // Type alias body is structurally part of the caller's
        // expression — propagate the caller's `from_root_body`.
        let mut resolved =
            resolve_type_elements_with_ctx_ref(aliased_type, base_offset, &child, from_root_body);
        // `type A = ImportedEmits` where ImportedEmits lives only in
        // companion_types (host external map): body resolution can still
        // land empty if the name lookup missed. Copy the companion surface
        // for simple non-generic aliases (AlertDialogEmits = DialogRootEmits).
        if resolved.props.is_empty()
            && resolved.call_signatures.is_empty()
            && !resolved.has_call_signature
        {
            if let TSType::TSTypeReference(type_ref) = aliased_type {
                if type_ref.type_arguments.is_none() {
                    let ref_name = get_type_reference_name(&type_ref.type_name);
                    if let Some(companion) = child.companion_types.get(ref_name.as_str()) {
                        if !companion.props.is_empty()
                            || !companion.call_signatures.is_empty()
                            || companion.has_call_signature
                        {
                            resolved = companion.clone();
                        }
                    }
                }
            }
        }
        return Some(Arc::new(resolved));
    }

    if let Some((members, _extends, heritage, type_params)) = ctx.find_interface(type_name_bytes) {
        let child = instantiate_type_params_ctx(ctx, type_params, type_args);
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=interface name={} from_root_body={} bindings={} companions={} members={} extends={}",
                    child.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    from_root_body,
                    child.type_param_bindings.len(),
                    child.companion_types.len(),
                    members.len(),
                    _extends.len()
                ),
            );
        }
        let plan = NamedTypeResolutionPlan::Interface(build_interface_resolution_plan(
            members,
            _extends,
            heritage,
            base_offset,
            &child,
            from_root_body,
        ));
        let mut resolved = ResolvedElements::default();
        flatten_named_type_plan_with_ctx_ref(
            &plan,
            base_offset,
            &mut resolved,
            &child,
            recursion_guard,
        );
        resolved.root_runtime_types = vec![RuntimeType::Object];
        let resolved = Arc::new(resolved);
        return Some(resolved);
    }

    if let Some(class_decl) = ctx.find_class(type_name_bytes) {
        let type_params = class_decl.type_parameters.as_deref();
        let child = instantiate_type_params_ctx(ctx, type_params, type_args);
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=class name={} from_root_body={} bindings={} companions={}",
                    child.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    from_root_body,
                    child.type_param_bindings.len(),
                    child.companion_types.len()
                ),
            );
        }
        let plan = NamedTypeResolutionPlan::Class(build_class_resolution_plan(
            class_decl,
            base_offset,
            &child,
            from_root_body,
        ));
        let mut resolved = ResolvedElements::default();
        flatten_named_type_plan_with_ctx_ref(
            &plan,
            base_offset,
            &mut resolved,
            &child,
            recursion_guard,
        );
        resolved.root_runtime_types = vec![RuntimeType::Object];
        resolved.dedup_props();
        let resolved = Arc::new(resolved);
        return Some(resolved);
    }

    if let Some(companion) = ctx.companion_types.get(type_name).cloned() {
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=companion cache=hit name={} from_root_body={} bindings={} companions={}",
                    ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    from_root_body,
                    ctx.type_param_bindings.len(),
                    ctx.companion_types.len()
                ),
            );
        }
        // Companion definitions carry accurate per-prop provenance from
        // `extract_companion_types` (resolved with `from_root_body = true`):
        // own-body literal members are marked `true`, heritage-injected
        // members are marked `false`. When the caller is itself at heritage
        // descent (`from_root_body = false`), every member of the companion
        // reaches the carrier via a heritage boundary, so flip the entire
        // surface to `false`. At the macro-T root, use the cached per-prop
        // facts as-is — that preserves the heritage `false` for inherited
        // members instead of blanket-stamping them `true`.
        if from_root_body {
            return Some(Arc::new(companion));
        }
        let mut stamped = companion;
        for prop in &mut stamped.props {
            prop.declared_in_macro_type_arg = false;
        }
        return Some(Arc::new(stamped));
    }

    if component_meta_core_trace_enabled() {
        component_meta_core_trace_event(
            "core_named_resolution",
            format!(
                "file={} kind=missing cache=miss name={} from_root_body={} bindings={} companions={}",
                ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                type_name,
                from_root_body,
                ctx.type_param_bindings.len(),
                ctx.companion_types.len()
            ),
        );
    }
    None
}

pub(super) fn is_supported_heritage_utility(name: &str) -> bool {
    matches!(
        name,
        "Pick" | "Omit" | "Partial" | "Required" | "Readonly" | "Record"
    )
}

/// Stamp a `ResolvedProp` with `declared_in_macro_type_arg = true`.
///
/// Used when consuming a companion-resolved (and therefore
/// `from_root_body=false`-cached) named type at a macro-T root: the
/// user wrote the companion's name in the macro T slot, so the
/// companion's structural members are the macro T's body.
#[inline]
pub(super) fn stamp_prop_in_root_body(mut prop: ResolvedProp) -> ResolvedProp {
    prop.declared_in_macro_type_arg = true;
    prop
}

pub(super) fn build_interface_resolution_plan<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) -> InterfaceResolutionPlan<'ctx, 'a> {
    let mut own = ResolvedElements::default();
    // Own members propagate the caller's macro-T own-body flag.
    // Heritage edges always descend with `from_root_body = false`
    // inside `apply_named_type_heritage_edge_with_ctx_ref` and
    // `try_resolve_heritage_utility_type`.
    resolve_type_literal_members_with_ctx(
        members,
        base_offset,
        &mut own,
        ctx.source,
        from_root_body,
        Some(ctx),
    );

    let heritage = heritage
        .iter()
        .enumerate()
        .filter(|(_, clause)| !has_immediate_vue_ignore_comment(ctx.source, clause.span().start))
        .map(|(index, clause)| {
            let name = extends
                .get(index)
                .cloned()
                .unwrap_or_else(|| "<anonymous>".to_string());
            match clause.type_arguments.as_deref() {
                Some(type_args)
                    if !type_args.params.is_empty()
                        && is_supported_heritage_utility(name.as_str()) =>
                {
                    NamedTypeHeritageEdge::Utility { name, type_args }
                }
                other => NamedTypeHeritageEdge::Named {
                    name,
                    type_args: other,
                },
            }
        })
        .collect();

    InterfaceResolutionPlan {
        own: own.into(),
        heritage,
    }
}

pub(super) fn build_class_resolution_plan<'ctx, 'a: 'ctx>(
    class: &'ctx Class<'a>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) -> ClassResolutionPlan<'ctx, 'a> {
    let mut own = ResolvedElements::default();
    // Own members propagate the caller's macro-T own-body flag.
    // Heritage edges (super-class, implements) descend with
    // `from_root_body = false` via
    // `apply_named_type_heritage_edge_with_ctx_ref`.
    resolve_class_members(
        &class.body.body,
        base_offset,
        &mut own,
        ctx.source,
        from_root_body,
    );

    let mut heritage = Vec::new();
    if let Some(super_class) = &class.super_class {
        if let Some(name) = get_expression_reference_name(super_class) {
            if let Some(type_args) = class.super_type_arguments.as_deref() {
                if !type_args.params.is_empty() && is_supported_heritage_utility(name.as_str()) {
                    heritage.push(NamedTypeHeritageEdge::Utility { name, type_args });
                } else {
                    heritage.push(NamedTypeHeritageEdge::Named {
                        name,
                        type_args: Some(type_args),
                    });
                }
            } else {
                heritage.push(NamedTypeHeritageEdge::Named {
                    name,
                    type_args: None,
                });
            }
        }
    }

    for clause in &class.implements {
        let name = get_type_reference_name(&clause.expression);
        if let Some(type_args) = clause.type_arguments.as_deref() {
            if !type_args.params.is_empty() && is_supported_heritage_utility(name.as_str()) {
                heritage.push(NamedTypeHeritageEdge::Utility { name, type_args });
            } else {
                heritage.push(NamedTypeHeritageEdge::Named {
                    name,
                    type_args: Some(type_args),
                });
            }
        } else {
            heritage.push(NamedTypeHeritageEdge::Named {
                name,
                type_args: None,
            });
        }
    }

    ClassResolutionPlan {
        own: own.into(),
        heritage,
    }
}

pub(super) fn flatten_named_type_plan_with_ctx_ref<'ctx, 'a: 'ctx>(
    plan: &NamedTypeResolutionPlan<'ctx, 'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    match plan {
        NamedTypeResolutionPlan::Interface(plan) => {
            plan.own.apply_to(result);
            for edge in &plan.heritage {
                apply_named_type_heritage_edge_with_ctx_ref(
                    edge,
                    base_offset,
                    result,
                    ctx,
                    recursion_guard,
                );
            }
        }
        NamedTypeResolutionPlan::Class(plan) => {
            plan.own.apply_to(result);
            for edge in &plan.heritage {
                apply_named_type_heritage_edge_with_ctx_ref(
                    edge,
                    base_offset,
                    result,
                    ctx,
                    recursion_guard,
                );
            }
        }
    }
}

/// Apply a heritage edge (from `flatten_named_type_plan_with_ctx_ref`)
/// to the carrier's `result`. Every edge here is a heritage descent —
/// internally dispatch with `from_root_body = false`.
pub(super) fn apply_named_type_heritage_edge_with_ctx_ref<'ctx, 'a: 'ctx>(
    edge: &NamedTypeHeritageEdge<'ctx, 'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    match edge {
        NamedTypeHeritageEdge::Utility { name, type_args } => {
            let _ = try_resolve_heritage_utility_type(
                name.as_str(),
                type_args,
                base_offset,
                result,
                ctx,
            );
        }
        NamedTypeHeritageEdge::Named { name, type_args } => {
            if recursion_guard.contains(name) {
                return;
            }
            recursion_guard.push(name.clone());
            if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
                name.as_str(),
                *type_args,
                base_offset,
                ctx,
                false,
                recursion_guard,
            ) {
                result.props.extend(resolved.props.iter().cloned());
                result
                    .call_signatures
                    .extend(resolved.call_signatures.iter().cloned());
                if resolved.has_call_signature {
                    result.has_call_signature = true;
                }
            }
            recursion_guard.pop();
        }
    }
}

pub(super) fn resolve_type_elements_inner(
    node: &TSType,
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
    from_root_body: bool,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, source, from_root_body);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner(
                &paren.type_annotation,
                base_offset,
                result,
                source,
                from_root_body,
            );
        }

        // Union: Type1 | Type2 — each arm preserves the caller's
        // `from_root_body` (unions enumerate options, not heritage).
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner(ty, base_offset, result, source, from_root_body);
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2 — TSTypeLiteral arms are
        // own-body literals (preserve `from_root_body`); any
        // non-literal arm (TSTypeReference, etc.) is a
        // surface-merging heritage-like construct from the caller's
        // perspective and descends with `from_root_body = false`.
        // (This inner is the source-only path with no resolver
        // context — only TSTypeLiteral arms actually emit members
        // here, so the distinction is structural rather than
        // observable, but we honour it for consistency with the
        // context-aware variants.)
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                if has_immediate_vue_ignore_comment(source, ty.span().start) {
                    continue;
                }
                let arm_from_root_body = matches!(ty, TSType::TSTypeLiteral(_)) && from_root_body;
                resolve_type_elements_inner(ty, base_offset, result, source, arm_from_root_body);
            }
            result.dedup_props();
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(_type_ref) => {
            // For now, we can't resolve type references without a scope
            // This would require tracking type declarations
            // Mark as unknown for now
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Resolve an interface including its extends clauses using mutable context.
/// Recursion guard prevents infinite loops from circular extends.
///
/// `from_root_body` is the caller's macro-T own-body flag.
/// - Own members (the interface's literal body) propagate the
///   caller's flag.
/// - `extends` / heritage descent ALWAYS forces `from_root_body =
///   false` because the user did not type heritage-target member
///   names in the carrier's own literal body.
#[allow(clippy::too_many_arguments)] // explicit `from_root_body` threading
pub(super) fn resolve_interface_with_extends_ctx<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
    recursion_guard: &mut Vec<String>,
) {
    // Resolve own members
    resolve_type_literal_members_with_ctx(
        members,
        base_offset,
        result,
        ctx.source,
        from_root_body,
        Some(&*ctx),
    );

    // Resolve extends — try heritage AST first for utility type support,
    // then fall back to string-based lookup (matches _ctx_ref variant).
    //
    // Every descent below crosses a heritage boundary: members reached
    // through `extends` were not literally typed in this interface's
    // own body, so their `declared_in_macro_type_arg` fact must be
    // `false` regardless of the carrier's `from_root_body`.
    for (i, base_name) in extends.iter().enumerate() {
        if recursion_guard.contains(base_name) {
            continue; // Avoid infinite recursion
        }
        recursion_guard.push(base_name.clone());

        let base_bytes = base_name.as_bytes();

        // When a heritage clause has type arguments (e.g., `extends Pick<T, 'k'>`),
        // resolve through the utility type dispatch inline.
        if let Some(h) = heritage.get(i) {
            if has_immediate_vue_ignore_comment(ctx.source, h.span().start) {
                recursion_guard.pop();
                continue;
            }
            if let Some(type_args) = &h.type_arguments {
                if !type_args.params.is_empty()
                    && try_resolve_heritage_utility_type(
                        base_name.as_str(),
                        type_args,
                        base_offset,
                        result,
                        ctx,
                    )
                {
                    recursion_guard.pop();
                    continue;
                }
            }
        }

        // Check local type aliases — descent is heritage → false.
        if let Some((aliased_type, _)) = ctx.find_type_alias(base_bytes) {
            resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, ctx, false);
        }
        // Check local interfaces (need to clone extends to avoid borrow conflict)
        else if let Some((iface_members, iface_extends, iface_heritage, _)) =
            ctx.find_interface(base_bytes)
        {
            let iface_extends_owned: Vec<String> = iface_extends.to_vec();
            resolve_interface_with_extends_ctx(
                iface_members,
                &iface_extends_owned,
                iface_heritage,
                base_offset,
                result,
                ctx,
                false,
                recursion_guard,
            );
        } else if let Some(class_decl) = ctx.find_class(base_bytes) {
            resolve_class_with_heritage_ctx_ref(
                class_decl,
                base_offset,
                result,
                ctx,
                false,
                recursion_guard,
            );
        }
        // Check companion types — heritage descent, companion's
        // cached `false` already matches.
        else if let Some(companion) = ctx.companion_types.get(base_name.as_str()) {
            result.props.extend(companion.props.iter().cloned());
            result
                .call_signatures
                .extend(companion.call_signatures.iter().cloned());
            if companion.has_call_signature {
                result.has_call_signature = true;
            }
        }

        recursion_guard.pop();
    }
}

/// Resolve an interface including its extends clauses using immutable context.
///
/// `from_root_body` is the caller's macro-T own-body flag.
/// - Own members (the interface's literal body) propagate the
///   caller's flag.
/// - `extends` / heritage descent ALWAYS forces `from_root_body =
///   false` because the user did not type heritage-target member
///   names in the carrier's own literal body.
#[allow(clippy::too_many_arguments)] // explicit `from_root_body` threading
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_interface_with_extends_ctx_ref<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
    recursion_guard: &mut Vec<String>,
) {
    let current_name = recursion_guard
        .last()
        .cloned()
        .unwrap_or_else(|| "<anonymous>".to_string());
    if component_meta_core_trace_enabled() {
        component_meta_core_trace_event(
            "core_interface_resolution",
            format!(
                "file={} phase=start name={} depth={} members={} extends={} from_root_body={}",
                ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                current_name,
                recursion_guard.len(),
                members.len(),
                extends.len(),
                from_root_body,
            ),
        );
    }
    // Resolve own members — propagate caller flag.
    resolve_type_literal_members_with_ctx(
        members,
        base_offset,
        result,
        ctx.source,
        from_root_body,
        Some(ctx),
    );

    // Resolve extends — try heritage AST first for utility type support,
    // then fall back to string-based lookup.
    //
    // Every descent below crosses a heritage boundary; named-target
    // lookups dispatch with `from_root_body = false`. The utility
    // type dispatch (`try_resolve_heritage_utility_type`) treats its
    // argument as heritage internally and does not need a flag.
    for (i, base_name) in extends.iter().enumerate() {
        if recursion_guard.contains(base_name) {
            continue;
        }
        recursion_guard.push(base_name.clone());

        // When a heritage clause has type arguments (e.g., `extends Pick<T, 'k'>`),
        // the name-based lookup below won't find it since "Pick" isn't a local type.
        // Resolve the full heritage expression through the type system which handles
        // all TypeScript utility types (Pick, Omit, Partial, Required, Readonly,
        // Record, Extract, Exclude, etc.) in a single code path.
        if let Some(h) = heritage.get(i) {
            if has_immediate_vue_ignore_comment(ctx.source, h.span().start) {
                recursion_guard.pop();
                continue;
            }
            if let Some(type_args) = &h.type_arguments {
                if !type_args.params.is_empty()
                    && try_resolve_heritage_utility_type(
                        base_name.as_str(),
                        type_args,
                        base_offset,
                        result,
                        ctx,
                    )
                {
                    if result.has_call_signature {
                        result.has_call_signature = true;
                    }
                    recursion_guard.pop();
                    continue;
                }
                // Not a recognized utility type — fall through to name-based lookup
            }
        }

        let type_args = heritage.get(i).and_then(|h| h.type_arguments.as_deref());
        if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
            base_name.as_str(),
            type_args,
            base_offset,
            ctx,
            false,
            recursion_guard,
        ) {
            result.props.extend(resolved.props.iter().cloned());
            result
                .call_signatures
                .extend(resolved.call_signatures.iter().cloned());
            if resolved.has_call_signature {
                result.has_call_signature = true;
            }
        }

        recursion_guard.pop();
    }

    if component_meta_core_trace_enabled() {
        component_meta_core_trace_event(
            "core_interface_resolution",
            format!(
                "file={} phase=end name={} depth={} props={} emits={}",
                ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                current_name,
                recursion_guard.len(),
                result.props.len(),
                result.call_signatures.len()
            ),
        );
    }
}

/// Heritage utility-type dispatch (`extends Pick<T, K>`,
/// `extends Omit<T, K>`, etc.).
///
/// Every internal descent into `T` (the first type argument) is a
/// heritage-like descent — the user did NOT type the resulting member
/// names in the carrier's literal body, so each descent enters with
/// `from_root_body = false`.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn try_resolve_heritage_utility_type<'ctx, 'a: 'ctx>(
    name: &str,
    type_args: &'ctx TSTypeParameterInstantiation<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> bool {
    match name {
        "Pick" if type_args.params.len() >= 2 => {
            let mut inner = ResolvedElements::default();
            resolve_type_elements_inner_with_ctx_ref(
                &type_args.params[0],
                base_offset,
                &mut inner,
                ctx,
                false,
            );
            let keys = extract_string_literal_keys_with_ctx(&type_args.params[1], Some(ctx));
            inner
                .props
                .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
            inner.call_signatures.retain(|e| keys.contains(&e.name));
            result.props.extend(inner.props);
            result.call_signatures.extend(inner.call_signatures);
            true
        }
        "Omit" if type_args.params.len() >= 2 => {
            let mut inner = ResolvedElements::default();
            resolve_type_elements_inner_with_ctx_ref(
                &type_args.params[0],
                base_offset,
                &mut inner,
                ctx,
                false,
            );
            let keys = extract_string_literal_keys_with_ctx(&type_args.params[1], Some(ctx));
            inner
                .props
                .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
            inner.call_signatures.retain(|e| !keys.contains(&e.name));
            result.props.extend(inner.props);
            result.call_signatures.extend(inner.call_signatures);
            true
        }
        "Partial" | "Required" | "Readonly" if !type_args.params.is_empty() => {
            let mut inner = ResolvedElements::default();
            resolve_type_elements_inner_with_ctx_ref(
                &type_args.params[0],
                base_offset,
                &mut inner,
                ctx,
                false,
            );
            if name == "Partial" {
                for p in &mut inner.props {
                    p.optional = true;
                }
            } else if name == "Required" {
                for p in &mut inner.props {
                    p.optional = false;
                }
            }
            result.props.extend(inner.props);
            result.call_signatures.extend(inner.call_signatures);
            true
        }
        "Record" if type_args.params.len() >= 2 => {
            result.root_runtime_types.push(RuntimeType::Object);
            true
        }
        _ => false,
    }
}

/// Resolve a class shape including super-class and implements heritage.
///
/// `from_root_body` is the caller's macro-T own-body flag.
/// - Own members (the class body) propagate the caller's flag.
/// - Super-class and `implements` descent ALWAYS forces
///   `from_root_body = false` (heritage boundary).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_class_with_heritage_ctx_ref<'ctx, 'a: 'ctx>(
    class: &'ctx Class<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
    recursion_guard: &mut Vec<String>,
) {
    resolve_class_members(
        &class.body.body,
        base_offset,
        result,
        ctx.source,
        from_root_body,
    );

    if let Some(super_class) = &class.super_class {
        if let Some(base_name) = get_expression_reference_name(super_class) {
            if !recursion_guard.contains(&base_name) {
                recursion_guard.push(base_name.clone());
                if let Some(type_args) = class.super_type_arguments.as_deref() {
                    if !type_args.params.is_empty()
                        && try_resolve_heritage_utility_type(
                            base_name.as_str(),
                            type_args,
                            base_offset,
                            result,
                            ctx,
                        )
                    {
                        recursion_guard.pop();
                    } else {
                        resolve_named_class_heritage_target(
                            base_name.as_str(),
                            class.super_type_arguments.as_deref(),
                            base_offset,
                            result,
                            ctx,
                            recursion_guard,
                        );
                        recursion_guard.pop();
                    }
                } else {
                    resolve_named_class_heritage_target(
                        base_name.as_str(),
                        None,
                        base_offset,
                        result,
                        ctx,
                        recursion_guard,
                    );
                    recursion_guard.pop();
                }
            }
        }
    }

    for clause in &class.implements {
        let base_name = get_type_reference_name(&clause.expression);
        if recursion_guard.contains(&base_name) {
            continue;
        }
        recursion_guard.push(base_name.clone());
        if let Some(type_args) = clause.type_arguments.as_deref() {
            if !type_args.params.is_empty()
                && try_resolve_heritage_utility_type(
                    base_name.as_str(),
                    type_args,
                    base_offset,
                    result,
                    ctx,
                )
            {
                recursion_guard.pop();
                continue;
            }
        }
        resolve_named_class_heritage_target(
            base_name.as_str(),
            clause.type_arguments.as_deref(),
            base_offset,
            result,
            ctx,
            recursion_guard,
        );
        recursion_guard.pop();
    }

    result.dedup_props();
}

/// Resolve a named class heritage target (super-class or implements
/// clause). This is always a heritage descent — internally dispatch
/// with `from_root_body = false`.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_named_class_heritage_target<'ctx, 'a: 'ctx>(
    name: &str,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
        name,
        type_args,
        base_offset,
        ctx,
        false,
        recursion_guard,
    ) {
        result.props.extend(resolved.props.iter().cloned());
        result
            .call_signatures
            .extend(resolved.call_signatures.iter().cloned());
        if resolved.has_call_signature {
            result.has_call_signature = true;
        }
    }
}

pub(super) fn resolve_class_members(
    members: &[ClassElement],
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
    from_root_body: bool,
) {
    for member in members {
        match member {
            ClassElement::PropertyDefinition(prop) => {
                if let Some(resolved) =
                    resolve_class_property_definition(prop, base_offset, source, from_root_body)
                {
                    result.props.push(resolved);
                }
            }
            ClassElement::MethodDefinition(method) => {
                if let Some(resolved) =
                    resolve_class_method_definition(method, base_offset, source, from_root_body)
                {
                    result.props.push(resolved);
                }
            }
            ClassElement::AccessorProperty(prop) => {
                if let Some(resolved) =
                    resolve_class_accessor_property(prop, base_offset, source, from_root_body)
                {
                    result.props.push(resolved);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn resolve_class_property_definition(
    prop: &PropertyDefinition,
    base_offset: u32,
    source: &[u8],
    from_root_body: bool,
) -> Option<ResolvedProp> {
    if prop.r#static {
        return None;
    }

    let key = get_property_key_span(&prop.key, base_offset)?;
    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .or_else(|| prop.value.as_ref().map(infer_runtime_type_from_expression))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });
    let type_text = prop
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(source, ann.type_annotation.span().into()));

    Some(ResolvedProp {
        span: Span {
            start: prop.span.start + base_offset,
            end: prop.span.end + base_offset,
        },
        key,
        key_name: get_property_key_name(&prop.key),
        optional: prop.optional,
        types,
        visibility: visibility_from_accessibility(prop.accessibility),
        type_span,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
        declared_in_macro_type_arg: from_root_body,
    })
}

pub(super) fn resolve_class_method_definition(
    method: &MethodDefinition,
    base_offset: u32,
    source: &[u8],
    from_root_body: bool,
) -> Option<ResolvedProp> {
    if method.r#static || method.kind == MethodDefinitionKind::Constructor {
        return None;
    }

    let key = get_property_key_span(&method.key, base_offset)?;
    let type_text = callable_signature_text(
        source,
        &method.value.params.items,
        method
            .value
            .return_type
            .as_ref()
            .map(|return_type| &return_type.type_annotation),
    );
    Some(ResolvedProp {
        span: Span {
            start: method.span.start + base_offset,
            end: method.span.end + base_offset,
        },
        key,
        key_name: get_property_key_name(&method.key),
        optional: method.optional,
        types: vec![RuntimeType::Function],
        visibility: visibility_from_accessibility(method.accessibility),
        type_span: None,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
        declared_in_macro_type_arg: from_root_body,
    })
}

pub(super) fn resolve_class_accessor_property(
    prop: &AccessorProperty,
    base_offset: u32,
    source: &[u8],
    from_root_body: bool,
) -> Option<ResolvedProp> {
    if prop.r#static {
        return None;
    }

    let key = get_property_key_span(&prop.key, base_offset)?;
    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .or_else(|| prop.value.as_ref().map(infer_runtime_type_from_expression))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });
    let type_text = prop
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(source, ann.type_annotation.span().into()));

    Some(ResolvedProp {
        span: Span {
            start: prop.span.start + base_offset,
            end: prop.span.end + base_offset,
        },
        key,
        key_name: get_property_key_name(&prop.key),
        optional: false,
        types,
        visibility: visibility_from_accessibility(prop.accessibility),
        type_span,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
        declared_in_macro_type_arg: from_root_body,
    })
}

pub(super) fn infer_runtime_type_from_expression(expr: &Expression<'_>) -> Vec<RuntimeType> {
    match expr {
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => vec![RuntimeType::String],
        Expression::NumericLiteral(_) => vec![RuntimeType::Number],
        Expression::BooleanLiteral(_) => vec![RuntimeType::Boolean],
        Expression::ArrayExpression(_) => vec![RuntimeType::Array],
        Expression::ObjectExpression(_) => vec![RuntimeType::Object],
        Expression::NullLiteral(_) => vec![RuntimeType::Null],
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
            vec![RuntimeType::Function]
        }
        _ => vec![RuntimeType::Unknown],
    }
}

pub(super) fn visibility_from_accessibility(
    accessibility: Option<TSAccessibility>,
) -> ResolvedMemberVisibility {
    match accessibility {
        Some(TSAccessibility::Private) => ResolvedMemberVisibility::Private,
        Some(TSAccessibility::Protected) => ResolvedMemberVisibility::Protected,
        _ => ResolvedMemberVisibility::Public,
    }
}

pub(super) fn get_expression_reference_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// Inner resolution function that uses the context for type reference lookup.
///
/// Recursion depth is tracked via a module-local thread-local counter
/// (see [`RESOLUTION_DEPTH`]) rather than a shared `Rc<Cell<u16>>` field on the
/// context. Removing that field from `TypeResolutionContext` unblocks the
/// host-owned cache migration (the resolver context no longer carries `!Send`
/// interior-mutability state). Depth guarding still bails at
/// [`PARSER_SYNTACTIC_DEPTH_LIMIT`] to prevent stack overflow on deeply nested
/// generic types (syntactic stack-safety, not a semantic budget).
pub(super) fn resolve_type_elements_inner_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) {
    let Some(_guard) = ResolutionDepthGuard::try_enter() else {
        return;
    };
    resolve_type_elements_inner_with_ctx_guarded(node, base_offset, result, ctx, from_root_body);
}

pub(super) fn resolve_type_elements_inner_with_ctx_guarded<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) {
    match node {
        // { prop: Type } — own-body literal, propagate caller flag.
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members_with_ctx(
                &lit.members,
                base_offset,
                result,
                ctx.source,
                from_root_body,
                Some(&*ctx),
            );
        }

        // Parenthesized: (Type) — transparent, propagate caller flag.
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner_with_ctx(
                &paren.type_annotation,
                base_offset,
                result,
                ctx,
                from_root_body,
            );
        }

        // Union: Type1 | Type2 — each arm preserves the caller's
        // `from_root_body` (unions enumerate options, not heritage).
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner_with_ctx(ty, base_offset, result, ctx, from_root_body);
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2 — TSTypeLiteral arms are
        // own-body literals (preserve caller's flag); any non-literal
        // arm (TSTypeReference, etc.) is a surface-merging
        // heritage-like construct from the caller's perspective and
        // descends with `from_root_body = false`.
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                if has_immediate_vue_ignore_comment(ctx.source, ty.span().start) {
                    continue;
                }
                let arm_from_root_body = matches!(ty, TSType::TSTypeLiteral(_)) && from_root_body;
                resolve_type_elements_inner_with_ctx(
                    ty,
                    base_offset,
                    result,
                    ctx,
                    arm_from_root_body,
                );
            }
            result.dedup_props();
        }

        TSType::TSMappedType(mapped) => {
            resolve_mapped_type_with_ctx(mapped, base_offset, result, &*ctx, from_root_body);
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => {
            // Get the type name for lookup
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            // 0. Check per-surface type blocklist — skip expansion entirely
            if ctx.is_type_blocked(&type_name) {
                return;
            }

            // 1. Check local type aliases — alias body is structurally
            //    part of the caller's expression; propagate caller flag.
            if let Some((aliased_type, type_params)) = ctx.find_type_alias(type_name_bytes) {
                let mut child = instantiate_type_params_ctx(
                    ctx,
                    type_params,
                    type_ref.type_arguments.as_deref(),
                );
                let before_props = result.props.len();
                let before_calls = result.call_signatures.len();
                resolve_type_elements_inner_with_ctx(
                    aliased_type,
                    base_offset,
                    result,
                    &mut child,
                    from_root_body,
                );
                // Empty simple alias → copy companion surface of the target
                // (AlertDialogEmits = DialogRootEmits via host external map).
                if result.props.len() == before_props
                    && result.call_signatures.len() == before_calls
                    && !result.has_call_signature
                {
                    if let TSType::TSTypeReference(inner_ref) = aliased_type {
                        if inner_ref.type_arguments.is_none() {
                            let ref_name = get_type_reference_name(&inner_ref.type_name);
                            if let Some(companion) = child.companion_types.get(ref_name.as_str()) {
                                result.props.extend(companion.props.iter().cloned());
                                result
                                    .call_signatures
                                    .extend(companion.call_signatures.iter().cloned());
                                if companion.has_call_signature {
                                    result.has_call_signature = true;
                                }
                            }
                        }
                    }
                }
                ctx.diagnostics.append(&mut child.diagnostics);
                return;
            }

            // 2. Check local interfaces (with extends support) —
            //    `resolve_interface_with_extends_ctx` internally
            //    distinguishes own-members (caller flag) from
            //    heritage descent (false).
            if let Some((interface_members, iface_extends, iface_heritage, iface_type_params)) =
                ctx.find_interface(type_name_bytes)
            {
                let extends_owned: Vec<String> = iface_extends.to_vec();
                let mut guard = vec![type_name.clone()];
                let mut child = instantiate_type_params_ctx(
                    ctx,
                    iface_type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_interface_with_extends_ctx(
                    interface_members,
                    &extends_owned,
                    iface_heritage,
                    base_offset,
                    result,
                    &mut child,
                    from_root_body,
                    &mut guard,
                );
                ctx.diagnostics.append(&mut child.diagnostics);
                return;
            }

            // 3. Check local classes (instance-side shape with heritage).
            if let Some(class_decl) = ctx.find_class(type_name_bytes) {
                let mut guard = vec![type_name.clone()];
                let child = instantiate_type_params_ctx(
                    ctx,
                    class_decl.type_parameters.as_deref(),
                    type_ref.type_arguments.as_deref(),
                );
                resolve_class_with_heritage_ctx_ref(
                    class_decl,
                    base_offset,
                    result,
                    &child,
                    from_root_body,
                    &mut guard,
                );
                return;
            }

            // 4. Check generic type parameter constraints — constraint
            //    body is structurally substituted for the type
            //    parameter; propagate caller flag.
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx(
                    constraint,
                    base_offset,
                    result,
                    ctx,
                    from_root_body,
                );
                return;
            }

            // 5. Check companion <script> block's pre-resolved types.
            //    Companion definitions were resolved with
            //    `from_root_body = false` (see `extract_companion_types`).
            //    When the macro T directly references one
            //    (`defineProps<CompanionFoo>()`), the user wrote the
            //    name in the macro T slot, so the companion's
            //    structural members are the macro T's body — when the
            //    caller's `from_root_body` is `true` we re-stamp the
            //    cloned props' fact to `true`. When the caller is
            //    `false` (we are already inside heritage descent),
            //    the cached `false` is correct.
            if let Some(companion) = ctx.companion_types.get(type_name.as_str()) {
                if from_root_body {
                    result
                        .props
                        .extend(companion.props.iter().cloned().map(stamp_prop_in_root_body));
                } else {
                    result.props.extend(companion.props.iter().cloned());
                }
                result
                    .call_signatures
                    .extend(companion.call_signatures.iter().cloned());
                if companion.has_call_signature {
                    result.has_call_signature = true;
                }
                return;
            }

            // 6. Handle built-in TypeScript utility types — the first
            //    type-argument of `Pick`/`Omit`/`Partial`/`Required`/
            //    `Readonly` is a heritage-like descent: the user did
            //    NOT type the resulting member NAMES in the macro T's
            //    own body — the names came from `T`. Descend with
            //    `from_root_body = false`.
            if let Some(args) = &type_ref.type_arguments {
                match type_name.as_str() {
                    "Omit" if args.params.len() >= 2 => {
                        // Omit<T, K>: resolve T, then remove keys in K
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                            false,
                        );
                        let keys =
                            extract_string_literal_keys_with_ctx(&args.params[1], Some(&*ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                        inner.call_signatures.retain(|e| !keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.call_signatures.extend(inner.call_signatures);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                        return;
                    }
                    "Pick" if args.params.len() >= 2 => {
                        // Pick<T, K>: resolve T, then keep only keys in K
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                            false,
                        );
                        let keys =
                            extract_string_literal_keys_with_ctx(&args.params[1], Some(&*ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                        inner.call_signatures.retain(|e| keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.call_signatures.extend(inner.call_signatures);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                        return;
                    }
                    "Partial" | "Required" | "Readonly" if !args.params.is_empty() => {
                        // These preserve structure, just change modifiers
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            result,
                            ctx,
                            false,
                        );
                        return;
                    }
                    _ => {}
                }
            }

            // 6. Couldn't resolve - add diagnostic
            // Note: We don't add to result.props here because we can't determine the structure
            ctx.diagnostics.push(ResolutionDiagnostic {
                span: Span {
                    start: type_ref.span.start + base_offset,
                    end: type_ref.span.end + base_offset,
                },
                kind: ResolutionDiagnosticKind::UnresolvedTypeReference,
                location: DiagnosticLocation::TypeResolution,
            });
        }

        // Type query: typeof X — look up in companion types. `typeof`
        // is NOT macro T's body; even when the caller's
        // `from_root_body` is `true`, the structural members come
        // from a value declaration the user didn't type names of.
        // Companion's cached `false` is forwarded unchanged.
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let type_name = ident.name.as_str();
                if let Some(companion) = ctx.companion_types.get(type_name) {
                    result.props.extend(companion.props.iter().cloned());
                    result
                        .call_signatures
                        .extend(companion.call_signatures.iter().cloned());
                    if companion.has_call_signature {
                        result.has_call_signature = true;
                    }
                }
            }
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Inner resolution function that uses an immutable context (doesn't collect diagnostics).
///
/// Uses the module-local [`RESOLUTION_DEPTH`] thread-local counter — see
/// [`resolve_type_elements_inner_with_ctx`] for the rationale.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_type_elements_inner_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) {
    let Some(_guard) = ResolutionDepthGuard::try_enter() else {
        return;
    };
    resolve_type_elements_inner_with_ctx_ref_guarded(
        node,
        base_offset,
        result,
        ctx,
        from_root_body,
    );
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_type_elements_inner_with_ctx_ref_guarded<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) {
    match node {
        // { prop: Type } — own-body literal, propagate caller flag.
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members_with_ctx(
                &lit.members,
                base_offset,
                result,
                ctx.source,
                from_root_body,
                Some(ctx),
            );
        }

        // Parenthesized: (Type) — transparent, propagate caller flag.
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner_with_ctx_ref(
                &paren.type_annotation,
                base_offset,
                result,
                ctx,
                from_root_body,
            );
        }

        // Union: Type1 | Type2 — each arm preserves the caller's
        // `from_root_body` (unions enumerate options, not heritage).
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner_with_ctx_ref(
                    ty,
                    base_offset,
                    result,
                    ctx,
                    from_root_body,
                );
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2 — TSTypeLiteral arms are
        // own-body literals (preserve caller's flag); any non-literal
        // arm (TSTypeReference, etc.) is a surface-merging
        // heritage-like construct from the caller's perspective and
        // descends with `from_root_body = false`.
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                if has_immediate_vue_ignore_comment(ctx.source, ty.span().start) {
                    continue;
                }
                let arm_from_root_body = matches!(ty, TSType::TSTypeLiteral(_)) && from_root_body;
                resolve_type_elements_inner_with_ctx_ref(
                    ty,
                    base_offset,
                    result,
                    ctx,
                    arm_from_root_body,
                );
            }
            result.dedup_props();
        }

        TSType::TSMappedType(mapped) => {
            resolve_mapped_type_with_ctx(mapped, base_offset, result, ctx, from_root_body);
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => {
            // Get the type name for lookup
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            // 0. Check per-surface type blocklist — skip expansion entirely
            if ctx.is_type_blocked(&type_name) {
                return;
            }

            // Named local type (alias / interface / class /
            // companion). The named target's body IS the user's
            // expression — propagate caller flag; the named
            // resolver internally distinguishes own-members from
            // heritage descent.
            let mut guard = vec![type_name.clone()];
            if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
                type_name.as_str(),
                type_ref.type_arguments.as_deref(),
                base_offset,
                ctx,
                from_root_body,
                &mut guard,
            ) {
                result.props.extend(resolved.props.iter().cloned());
                result
                    .call_signatures
                    .extend(resolved.call_signatures.iter().cloned());
                if resolved.has_call_signature {
                    result.has_call_signature = true;
                }
                return;
            }

            // 4. Check generic type parameter constraints — propagate
            //    caller flag (constraint body substitutes the type
            //    parameter structurally).
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx_ref(
                    constraint,
                    base_offset,
                    result,
                    ctx,
                    from_root_body,
                );
                return;
            }

            // 6. Handle built-in TypeScript utility types — the first
            //    type-argument of `Pick`/`Omit`/`Partial`/`Required`/
            //    `Readonly` is a heritage-like descent: descend with
            //    `from_root_body = false`.
            if let Some(args) = &type_ref.type_arguments {
                match type_name.as_str() {
                    "Omit" if args.params.len() >= 2 => {
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                            false,
                        );
                        let keys = extract_string_literal_keys_with_ctx(&args.params[1], Some(ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                        inner.call_signatures.retain(|e| !keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.call_signatures.extend(inner.call_signatures);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                    }
                    "Pick" if args.params.len() >= 2 => {
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                            false,
                        );
                        let keys = extract_string_literal_keys_with_ctx(&args.params[1], Some(ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                        inner.call_signatures.retain(|e| keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.call_signatures.extend(inner.call_signatures);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                    }
                    "Partial" | "Required" | "Readonly" if !args.params.is_empty() => {
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            result,
                            ctx,
                            false,
                        );
                    }
                    _ => {}
                }
            }

            // 6. Couldn't resolve - skip silently (no diagnostics in immutable version)
        }

        // Type query: typeof X — look up in companion types. `typeof`
        // is NOT macro T's body; companion's cached `false` is
        // forwarded unchanged.
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let type_name = ident.name.as_str();
                if let Some(companion) = ctx.companion_types.get(type_name) {
                    result.props.extend(companion.props.iter().cloned());
                    result
                        .call_signatures
                        .extend(companion.call_signatures.iter().cloned());
                    if companion.has_call_signature {
                        result.has_call_signature = true;
                    }
                }
            }
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}
