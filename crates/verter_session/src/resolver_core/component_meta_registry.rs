//! Component-meta registry publication.
//!
//! This module owns registry queueing, route merging, and publication
//! policy for routed component-meta type publication.
//!
//! See architectural rule 10: "Component-meta publication stays in resolver_core."

use std::collections::VecDeque;
use std::sync::Arc;

use crate::resolver_core::ResolverContext;
use crate::resolver_core::RouteDemand;
use crate::types::FileAnalysisSnapshot;
use verter_type_expr::{FunctionExpr, ObjectMember, PrimitiveName, TypeExpr};

/// Issue #7 / capture-token counters for route-demand
/// emission. Recorded inside [`enqueue_component_meta_registry_ref`]
/// so every enqueue site reports the route variant it pushed onto
/// the queue.
///
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(any(test, debug_assertions))]
pub(crate) const ROUTE_DEMAND_EMITTED_WHOLE_COUNTER: &str = "route_demand_emitted::Whole";
#[cfg(any(test, debug_assertions))]
pub(crate) const ROUTE_DEMAND_EMITTED_PICK_COUNTER: &str = "route_demand_emitted::Pick";
#[cfg(any(test, debug_assertions))]
pub(crate) const ROUTE_DEMAND_EMITTED_MEMBER_PATH_COUNTER: &str =
    "route_demand_emitted::MemberPath";
#[cfg(any(test, debug_assertions))]
pub(crate) const ROUTE_DEMAND_EMITTED_OMIT_COUNTER: &str = "route_demand_emitted::Omit";

/// Map a `RouteDemand` variant to its capture-token counter name.
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(any(test, debug_assertions))]
fn route_demand_counter_name(route: &RouteDemand) -> &'static str {
    match route {
        RouteDemand::Whole => ROUTE_DEMAND_EMITTED_WHOLE_COUNTER,
        RouteDemand::Pick(_) => ROUTE_DEMAND_EMITTED_PICK_COUNTER,
        RouteDemand::MemberPath(_) => ROUTE_DEMAND_EMITTED_MEMBER_PATH_COUNTER,
        RouteDemand::Omit(_) => ROUTE_DEMAND_EMITTED_OMIT_COUNTER,
    }
}

/// Work item for the unified registry publication queue.
///
/// Combines initial entries and transitive references into a single
/// queue that the resolver-core registry publisher processes uniformly.
#[derive(Debug, Clone)]
pub enum RegistryWorkItem {
    /// Initial registry entry from the component's direct type analysis.
    InitialEntry {
        index: usize,
        declaration_source: String,
        requested_name: String,
    },
    /// Transitive type reference discovered from props/emits/slots surfaces.
    TransitiveRef {
        name: String,
        source_hint: Option<String>,
        route: crate::resolver_core::RouteDemand,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingComponentMetaRegistryRef {
    pub(crate) name: String,
    pub(crate) source_hint: Option<String>,
    pub(crate) exported_name: Option<String>,
    pub(crate) route: RouteDemand,
}

pub(crate) fn upsert_component_meta_registry_entry(
    owner_canonical: &str,
    resolved_type_registry: &mut Vec<
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
    >,
    resolved_type_registry_meta: &mut Vec<crate::resolver_core::ResolvedTypeRegistryMeta>,
    published_names: &mut rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: String,
    type_expr: verter_type_expr::TypeExpr,
    declaration: crate::resolver_core::ResolvedTypeDeclaration,
    collection_expr: Option<&verter_type_expr::TypeExpr>,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) {
    let declaration_source_hint =
        (!declaration.canonical_source.is_empty()).then(|| declaration.canonical_source.clone());
    let collect_nested_refs = should_collect_component_meta_registry_nested_refs(
        owner_canonical,
        declaration_source_hint.as_deref(),
    );
    if let Some(index) = resolved_type_registry
        .iter()
        .position(|entry| entry.name == name)
    {
        let existing = resolved_type_registry[index].type_expr.clone();
        let preferred =
            merge_component_meta_registry_candidates(Some(existing.clone()), Some(type_expr))
                .unwrap_or(existing.clone());
        if preferred != existing {
            resolved_type_registry[index].type_expr = preferred.clone();
            if let Some(meta) = resolved_type_registry_meta.get_mut(index) {
                *meta = crate::resolver_core::ResolvedTypeRegistryMeta {
                    name: name.clone(),
                    declaration,
                };
            }
            if collect_nested_refs {
                collect_component_meta_registry_refs(
                    collection_expr.unwrap_or(&preferred),
                    published_names,
                    queued_names,
                    referenced_names,
                    declaration_source_hint.as_deref(),
                    false,
                    cursor,
                );
            }
        }
        return;
    }

    if collect_nested_refs {
        collect_component_meta_registry_refs(
            collection_expr.unwrap_or(&type_expr),
            published_names,
            queued_names,
            referenced_names,
            declaration_source_hint.as_deref(),
            false,
            cursor,
        );
    }
    resolved_type_registry.push(
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
            name: name.clone(),
            type_expr,
            type_expansion: None,
        },
    );
    resolved_type_registry_meta.push(crate::resolver_core::ResolvedTypeRegistryMeta {
        name: name.clone(),
        declaration,
    });
    published_names.insert(name);
}

pub(crate) fn should_collect_component_meta_registry_nested_refs(
    owner_canonical: &str,
    source_hint: Option<&str>,
) -> bool {
    match source_hint.filter(|source| !source.is_empty()) {
        Some(source) => source == owner_canonical,
        None => true,
    }
}

pub(crate) fn owner_component_meta_registry_import_root(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    _snapshot: &FileAnalysisSnapshot,
    local_name: &str,
) -> Option<(String, String)> {
    ctx.resolve_owner_direct_import(owner_canonical, local_name)
}

/// Issue #7 / extract the route's root name when `expr` is
/// either an `IndexedAccess` (`Foo['variants']['variant']`) or a
/// utility wrapper (`Pick<Foo, ...>` / `Omit<Foo, ...>`).
fn component_meta_registry_route_root_name(expr: &TypeExpr) -> Option<String> {
    if let Some((root, _)) = component_meta_registry_public_utility_route(expr) {
        return Some(root);
    }
    if let Some((root, _)) = component_meta_registry_public_indexed_access_route(expr) {
        return Some(root);
    }
    None
}

/// Issue #7 / true when the named alias's prepared body
/// resolves (modulo single alias-of-alias indirection) to a `Ref {
/// name: "ComponentConfig", type_arguments: nonempty }`.
///
/// Returns `false` for:
/// - missing prepared decl
/// - body is a `TypeParameter`
/// - body is a non-generic `Ref` to anything other than another local
///   alias whose body satisfies the rule (alias-of-alias depth 1)
/// - body is a `Ref` to `ComponentConfig` with no type arguments
pub(crate) fn component_meta_registry_owner_local_component_config_alias_name(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    name: &str,
) -> bool {
    /// Strip leading `Parenthesized` wrappers iteratively (no recursion;
    /// satisfies `no_unbounded_recursion_in_resolver_core`).
    fn unwrap_paren(mut expr: &TypeExpr) -> &TypeExpr {
        while let TypeExpr::Parenthesized(inner) = expr {
            expr = inner;
        }
        expr
    }

    let Some(prepared) = ctx.prepared_type_decl(owner_canonical, name) else {
        return false;
    };
    if matches!(prepared.body, TypeExpr::TypeParameter(_)) {
        return false;
    }
    let body = unwrap_paren(&prepared.body);
    if let TypeExpr::Ref {
        name: ref_name,
        type_arguments,
    } = body
    {
        if ref_name.as_ref() == "ComponentConfig" && !type_arguments.is_empty() {
            return true;
        }
        // Single alias-of-alias indirection — follow once.
        if type_arguments.is_empty() {
            if let Some(next) = ctx.prepared_type_decl(owner_canonical, ref_name.as_ref()) {
                if matches!(next.body, TypeExpr::TypeParameter(_)) {
                    return false;
                }
                if let TypeExpr::Ref {
                    name: nested_name,
                    type_arguments: nested_args,
                } = unwrap_paren(&next.body)
                {
                    return nested_name.as_ref() == "ComponentConfig" && !nested_args.is_empty();
                }
            }
        }
    }
    false
}

/// Issue #7 / predicate for the registry public-route
/// rewrite. Returns `Some(root_name)` when ALL of:
/// - the route root has no import binding on the owner (so the alias
///   resolves to a workspace-local declaration)
/// - the prepared body is not a `TypeParameter`
/// - the alias body is `ComponentConfig<...>` (or alias-of-alias to
///   that)
/// - `ComponentConfig` itself is not imported in the owner's scope
///
/// On owner-local hit, the caller MUST emit `RouteDemand::Whole`
/// instead of the standard `MemberPath`/`Pick` route.
pub(crate) fn component_meta_registry_public_route_owner_local_root(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    expr: &TypeExpr,
    source_hint: Option<&str>,
) -> Option<String> {
    // Source hint must be either None (component-local) or match the
    // owner — anything else is from a different owner's scope.
    if let Some(source) = source_hint.filter(|source| !source.is_empty()) {
        if source != owner_canonical {
            return None;
        }
    }

    // Extract the route's root name (either utility or indexed
    // access).
    let root_name = component_meta_registry_route_root_name(expr)?;

    // Owner-local rule: no import binding on `root_name`. If the
    // resolver routes the name to an external file, it is imported.
    if owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, root_name.as_str())
        .is_some()
    {
        return None;
    }

    // ComponentConfig itself must NOT be imported in the owner's
    // scope.
    if owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, "ComponentConfig")
        .is_some()
    {
        return None;
    }

    // Alias body must be `ComponentConfig<...>`.
    if !component_meta_registry_owner_local_component_config_alias_name(
        ctx,
        owner_canonical,
        root_name.as_str(),
    ) {
        return None;
    }

    Some(root_name)
}

pub(crate) fn enqueue_component_meta_registry_ref(
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: &str,
    source_hint: Option<&str>,
    exported_name: Option<&str>,
    route: RouteDemand,
) {
    if published_names.contains(name) {
        return;
    }
    // Issue #7 / record the route-demand variant on every
    // queue admission. Tests assert
    // `route_demand_emitted::Whole >= 1` and
    // `route_demand_emitted::Pick + ::MemberPath == 0` for owner-local
    // ComponentConfig aliases (and the inverse for external imports).
    // Route-demand counter recording — test/debug instrumentation only;
    // gated to match the capture-token module (absent in release).
    #[cfg(any(test, debug_assertions))]
    let counter_name = route_demand_counter_name(&route);
    #[cfg(any(test, debug_assertions))]
    crate::capture_token::with_active_capture(|t| t.record_counter(counter_name, 1));
    let source_hint = source_hint
        .filter(|source| !source.is_empty())
        .map(str::to_string);
    let exported_name = exported_name
        .filter(|exported| !exported.is_empty())
        .map(str::to_string);
    if !queued_names.insert(name.to_string()) {
        if let Some(existing) = referenced_names.iter_mut().find(|pending| {
            pending.name == name
                && pending.source_hint == source_hint
                && pending.exported_name == exported_name
                && component_meta_registry_can_merge_pending_route(&pending.route, &route)
        }) {
            existing.route = crate::resolver_core::merge_route_demands(&existing.route, &route);
        } else {
            referenced_names.push_back(PendingComponentMetaRegistryRef {
                name: name.to_string(),
                source_hint,
                exported_name,
                route,
            });
        }
        return;
    }
    referenced_names.push_back(PendingComponentMetaRegistryRef {
        name: name.to_string(),
        source_hint,
        exported_name,
        route,
    });
}

fn component_meta_registry_can_merge_pending_route(
    existing: &RouteDemand,
    incoming: &RouteDemand,
) -> bool {
    let merged = crate::resolver_core::merge_route_demands(existing, incoming);
    route_demand_keeps_exact_deep_member_path(existing, &merged)
        && route_demand_keeps_exact_deep_member_path(incoming, &merged)
}

fn route_demand_keeps_exact_deep_member_path(
    requested: &RouteDemand,
    merged: &RouteDemand,
) -> bool {
    match requested {
        RouteDemand::MemberPath(path) if path.len() > 1 => match merged {
            RouteDemand::Whole => true,
            RouteDemand::MemberPath(merged_path) => merged_path == path,
            _ => false,
        },
        _ => true,
    }
}

pub(crate) fn choose_preferred_component_meta_registry_candidate(
    left: Option<verter_type_expr::TypeExpr>,
    right: Option<verter_type_expr::TypeExpr>,
) -> Option<verter_type_expr::TypeExpr> {
    match (left, right) {
        (Some(left), Some(right)) => {
            let left_non_object = component_meta_registry_has_non_object_top_level_surface(&left);
            let right_non_object = component_meta_registry_has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            if component_meta_registry_indexed_ref_penalty(&left)
                != component_meta_registry_indexed_ref_penalty(&right)
            {
                return Some(
                    if component_meta_registry_indexed_ref_penalty(&left)
                        < component_meta_registry_indexed_ref_penalty(&right)
                    {
                        left
                    } else {
                        right
                    },
                );
            }

            choose_preferred_imported_type_body(Some(left), Some(right))
        }
        (left, right) => choose_preferred_imported_type_body(left, right),
    }
}

/// Maximum recursion depth for nested object merging to prevent stack overflow
/// on deeply nested `ComponentConfig` types.
const MAX_REGISTRY_MERGE_DEPTH: u8 = 8;

pub(crate) fn merge_component_meta_registry_candidates(
    left: Option<verter_type_expr::TypeExpr>,
    right: Option<verter_type_expr::TypeExpr>,
) -> Option<verter_type_expr::TypeExpr> {
    merge_component_meta_registry_candidates_bounded(left, right, 0)
}

fn merge_component_meta_registry_candidates_bounded(
    left: Option<verter_type_expr::TypeExpr>,
    right: Option<verter_type_expr::TypeExpr>,
    depth: u8,
) -> Option<verter_type_expr::TypeExpr> {
    use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

    fn merge_member_types(left: &TypeExpr, right: &TypeExpr, depth: u8) -> TypeExpr {
        if depth >= MAX_REGISTRY_MERGE_DEPTH {
            return left.clone();
        }
        merge_component_meta_registry_candidates_bounded(
            Some(left.clone()),
            Some(right.clone()),
            depth + 1,
        )
        .unwrap_or_else(|| left.clone())
    }

    fn merge_object_members(left: &TypeExpr, right: &TypeExpr, depth: u8) -> Option<TypeExpr> {
        let (TypeExpr::Object(left_obj), TypeExpr::Object(right_obj)) = (left, right) else {
            return None;
        };

        let mut merged_members = left_obj.properties.to_vec();
        for right_member in &right_obj.properties {
            match right_member {
                ObjectMember::Property(right_property) => {
                    if let Some(ObjectMember::Property(existing_property)) =
                        merged_members.iter_mut().find(|member| {
                            matches!(
                                member,
                                ObjectMember::Property(property)
                                    if property.name == right_property.name
                            )
                        })
                    {
                        existing_property.ty =
                            merge_member_types(&existing_property.ty, &right_property.ty, depth);
                        existing_property.optional =
                            existing_property.optional && right_property.optional;
                        existing_property.readonly =
                            existing_property.readonly && right_property.readonly;
                        // Duplicate member (same name in both sides): aggregate
                        // to the MOST-RESTRICTIVE visibility (the shared merge
                        // rule). A merged member is Public only when Public in
                        // BOTH contributors, so a member non-public in either
                        // side stays non-public — never synthesized Public.
                        existing_property.visibility = existing_property
                            .visibility
                            .most_restrictive(right_property.visibility);
                    } else {
                        // RHS-only property: carry the right-hand property's OXC
                        // spans AND its declared accessibility verbatim (rebuild
                        // of an existing member — `with_spans` would default it
                        // to Public).
                        merged_members.push(ObjectMember::Property(
                            ObjectProperty::with_visibility(
                                right_property.name.clone(),
                                right_property.ty.clone(),
                                right_property.optional,
                                right_property.readonly,
                                right_property.visibility,
                                right_property.spans,
                            ),
                        ));
                    }
                }
                ObjectMember::Method(right_method) => {
                    if let Some(ObjectMember::Method(existing_method)) =
                        merged_members.iter_mut().find(|member| {
                            matches!(
                                member,
                                ObjectMember::Method(method)
                                    if method.name == right_method.name
                            )
                        })
                    {
                        // Duplicate method (same name in both sides): aggregate
                        // to the MOST-RESTRICTIVE visibility via the shared merge
                        // rule, exactly as the duplicate-property arm does. A
                        // merged method is Public only when Public in BOTH
                        // contributors, so a method non-public in either side
                        // stays non-public — never synthesized Public. `optional`
                        // ANDs (present-without-`?` in either side ⇒ required);
                        // the existing (left) signature is retained.
                        existing_method.optional =
                            existing_method.optional && right_method.optional;
                        existing_method.visibility = existing_method
                            .visibility
                            .most_restrictive(right_method.visibility);
                    } else {
                        // RHS-only method: carry the right-hand method's OXC spans
                        // AND its declared accessibility verbatim (rebuild of an
                        // existing member — a source-less constructor would
                        // default it to Public).
                        merged_members.push(ObjectMember::Method(
                            verter_type_expr::MethodSignature::with_visibility(
                                right_method.name.clone(),
                                right_method.function.clone(),
                                right_method.optional,
                                right_method.visibility,
                                right_method.spans,
                            ),
                        ));
                    }
                }
                _ => {
                    if !merged_members.contains(right_member) {
                        merged_members.push(right_member.clone());
                    }
                }
            }
        }

        Some(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: merged_members,
        })))
    }

    match (left, right) {
        (Some(left), Some(right)) => merge_object_members(&left, &right, depth).or_else(|| {
            choose_preferred_component_meta_registry_candidate(Some(left), Some(right))
        }),
        (left, right) => choose_preferred_component_meta_registry_candidate(left, right),
    }
}

fn choose_preferred_imported_type_body(
    resolved_body: Option<TypeExpr>,
    resolved_decl_body: Option<TypeExpr>,
) -> Option<TypeExpr> {
    match (resolved_body, resolved_decl_body) {
        (Some(left), Some(right)) => {
            let left_empty_object = is_empty_object_surface(&left);
            let right_empty_object = is_empty_object_surface(&right);
            if left_empty_object != right_empty_object {
                return Some(if left_empty_object { right } else { left });
            }

            let left_surface_props = extracted_surface_property_count(&left);
            let right_surface_props = extracted_surface_property_count(&right);
            if let (Some(left_count), Some(right_count)) = (left_surface_props, right_surface_props)
            {
                if left_count != right_count {
                    return Some(if left_count > right_count {
                        left
                    } else {
                        right
                    });
                }
            }

            let left_method_surface = method_surface_specificity_score(&left);
            let right_method_surface = method_surface_specificity_score(&right);
            if left_method_surface != right_method_surface {
                return Some(if left_method_surface > right_method_surface {
                    left
                } else {
                    right
                });
            }

            let left_top_level_branching = top_level_branching_surface_score(&left);
            let right_top_level_branching = top_level_branching_surface_score(&right);
            if left_top_level_branching != right_top_level_branching {
                return Some(if left_top_level_branching > right_top_level_branching {
                    left
                } else {
                    right
                });
            }

            let left_nested = contains_nested_resolution_targets(&left);
            let right_nested = contains_nested_resolution_targets(&right);
            if left_nested != right_nested {
                return Some(if left_nested { right } else { left });
            }

            let left_non_object = component_meta_registry_has_non_object_top_level_surface(&left);
            let right_non_object = component_meta_registry_has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            let left_bound_generic_penalty = bound_generic_ref_penalty(&left);
            let right_bound_generic_penalty = bound_generic_ref_penalty(&right);
            if left_bound_generic_penalty != right_bound_generic_penalty {
                return Some(
                    if left_bound_generic_penalty < right_bound_generic_penalty {
                        left
                    } else {
                        right
                    },
                );
            }

            if imported_type_body_specificity_score(&right)
                > imported_type_body_specificity_score(&left)
            {
                Some(right)
            } else {
                Some(left)
            }
        }
        (Some(body), None) | (None, Some(body)) => Some(body),
        (None, None) => None,
    }
}

fn is_empty_object_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => is_empty_object_surface(inner),
        TypeExpr::Object(obj) => obj.properties.is_empty(),
        _ => false,
    }
}

fn contains_nested_resolution_targets(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers are intrinsic terminal leaves; no nested
        // resolution targets reach through them.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::TypeParameter(_) => false,
        TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => contains_nested_resolution_targets(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| contains_nested_resolution_targets(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Object(_) => false,
        // A constructor type, like a function type, is treated as a terminal
        // here (its signature is not walked for nested resolution targets) —
        // identical to the `Function` arm.
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => false,
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Infer { .. } => false,
    }
}

fn extracted_surface_property_count(expr: &TypeExpr) -> Option<usize> {
    match expr {
        TypeExpr::Parenthesized(inner) => extracted_surface_property_count(inner),
        TypeExpr::Object(obj) => Some(
            obj.properties
                .iter()
                .filter(|member| {
                    matches!(member, ObjectMember::Property(_) | ObjectMember::Method(_))
                })
                .count(),
        ),
        TypeExpr::Intersection(types) => {
            let mut total = 0usize;
            let mut saw_surface = false;
            for ty in types.iter() {
                let count = extracted_surface_property_count(ty)?;
                total += count;
                saw_surface = true;
            }
            saw_surface.then_some(total)
        }
        _ => None,
    }
}

fn method_surface_specificity_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Parenthesized(inner) => method_surface_specificity_score(inner),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Method(method) => {
                    2 + method_surface_specificity_score(&TypeExpr::Function(Arc::new(
                        method.function.clone(),
                    )))
                }
                ObjectMember::Property(prop) => {
                    // A bare constructor type is an equally-specific callable
                    // surface as a function type — both earn the bonus.
                    usize::from(matches!(
                        prop.ty,
                        TypeExpr::Function(_) | TypeExpr::ConstructorType(_)
                    )) + method_surface_specificity_score(&prop.ty)
                }
                ObjectMember::IndexSignature(sig) => {
                    method_surface_specificity_score(&sig.key_type)
                        + method_surface_specificity_score(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    method_surface_specificity_score(&TypeExpr::Function(Arc::new(func.clone())))
                }
            })
            .sum(),
        // A constructor type carries the same `FunctionExpr` payload as a
        // function type and is an equally-specific callable surface, so it is
        // scored identically.
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            func.parameters
                .iter()
                .map(|param| method_surface_specificity_score(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(method_surface_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Array { element, .. } | TypeExpr::KeyOf(element) | TypeExpr::Rest(element) => {
            method_surface_specificity_score(element)
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| method_surface_specificity_score(&element.ty))
            .sum(),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().map(method_surface_specificity_score).sum(),
        TypeExpr::IndexedAccess { object, index } => {
            method_surface_specificity_score(object) + method_surface_specificity_score(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            method_surface_specificity_score(check)
                + method_surface_specificity_score(extends)
                + method_surface_specificity_score(true_type)
                + method_surface_specificity_score(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            method_surface_specificity_score(source)
                + method_surface_specificity_score(value)
                + name_type
                    .as_deref()
                    .map(method_surface_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers carry no method surface — score 0.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => 0,
    }
}

fn bound_generic_ref_penalty(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers reference no bound generic; no penalty.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => 0,
        TypeExpr::TypeOf(_) => 1,
        TypeExpr::TypeParameter(param) => {
            param
                .constraint
                .as_deref()
                .map(bound_generic_ref_penalty)
                .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(bound_generic_ref_penalty)
                    .unwrap_or_default()
        }
        TypeExpr::Ref { type_arguments, .. } => {
            usize::from(!type_arguments.is_empty())
                + type_arguments
                    .iter()
                    .map(bound_generic_ref_penalty)
                    .sum::<usize>()
        }
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => bound_generic_ref_penalty(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| bound_generic_ref_penalty(&element.ty))
            .sum(),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().map(bound_generic_ref_penalty).sum(),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Property(prop) => bound_generic_ref_penalty(&prop.ty),
                ObjectMember::IndexSignature(sig) => {
                    bound_generic_ref_penalty(&sig.key_type)
                        + bound_generic_ref_penalty(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.parameters
                        .iter()
                        .map(|param| bound_generic_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + func
                            .return_type
                            .as_deref()
                            .map(bound_generic_ref_penalty)
                            .unwrap_or_default()
                        + func
                            .type_parameters
                            .iter()
                            .map(|param| {
                                param
                                    .constraint
                                    .as_deref()
                                    .map(bound_generic_ref_penalty)
                                    .unwrap_or_default()
                                    + param
                                        .default
                                        .as_deref()
                                        .map(bound_generic_ref_penalty)
                                        .unwrap_or_default()
                            })
                            .sum::<usize>()
                }
                ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .map(|param| bound_generic_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + method
                            .function
                            .return_type
                            .as_deref()
                            .map(bound_generic_ref_penalty)
                            .unwrap_or_default()
                        + method
                            .function
                            .type_parameters
                            .iter()
                            .map(|param| {
                                param
                                    .constraint
                                    .as_deref()
                                    .map(bound_generic_ref_penalty)
                                    .unwrap_or_default()
                                    + param
                                        .default
                                        .as_deref()
                                        .map(bound_generic_ref_penalty)
                                        .unwrap_or_default()
                            })
                            .sum::<usize>()
                }
            })
            .sum(),
        // A constructor type's signature is penalised identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            func.parameters
                .iter()
                .map(|param| bound_generic_ref_penalty(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(bound_generic_ref_penalty)
                    .unwrap_or_default()
                + func
                    .type_parameters
                    .iter()
                    .map(|param| {
                        param
                            .constraint
                            .as_deref()
                            .map(bound_generic_ref_penalty)
                            .unwrap_or_default()
                            + param
                                .default
                                .as_deref()
                                .map(bound_generic_ref_penalty)
                                .unwrap_or_default()
                    })
                    .sum::<usize>()
        }
        TypeExpr::IndexedAccess { object, index } => {
            bound_generic_ref_penalty(object) + bound_generic_ref_penalty(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            bound_generic_ref_penalty(check)
                + bound_generic_ref_penalty(extends)
                + bound_generic_ref_penalty(true_type)
                + bound_generic_ref_penalty(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            bound_generic_ref_penalty(source)
                + bound_generic_ref_penalty(value)
                + name_type
                    .as_deref()
                    .map(bound_generic_ref_penalty)
                    .unwrap_or_default()
        }
    }
}

fn top_level_branching_surface_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Parenthesized(inner) => top_level_branching_surface_score(inner),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            let mut score = 0usize;
            for ty in types.iter() {
                match ty {
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    TypeExpr::Unknown { .. } => {}
                    _ => score += 1,
                }
            }
            if score >= 2 {
                score
            } else {
                0
            }
        }
        _ => 0,
    }
}

const SPECIFICITY_UNKNOWN: usize = 0;
const SPECIFICITY_TYPEOF: usize = 4;
const SPECIFICITY_TERMINAL: usize = 8;
const SPECIFICITY_REF_BASE: usize = 16;
const SPECIFICITY_TEMPLATE_LITERAL_BASE: usize = 20;
const SPECIFICITY_WRAPPER_BASE: usize = 24;
const SPECIFICITY_INDEXED_ACCESS_BASE: usize = 28;
const SPECIFICITY_MAPPED_BASE: usize = 32;
const SPECIFICITY_TUPLE_BASE: usize = 40;
const SPECIFICITY_FUNCTION_BASE: usize = 48;
const SPECIFICITY_UNION_BASE: usize = 56;
const SPECIFICITY_INTERSECTION_BASE: usize = 64;
const SPECIFICITY_OBJECT_BASE: usize = 96;
const SPECIFICITY_OBJECT_PROPERTY: usize = 12;
const SPECIFICITY_INDEX_SIGNATURE: usize = 6;
const SPECIFICITY_CALL_LIKE_MEMBER: usize = 10;

fn imported_type_body_specificity_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Unknown { .. } => SPECIFICITY_UNKNOWN,
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => SPECIFICITY_TERMINAL,
        TypeExpr::TypeOf(_) => SPECIFICITY_TYPEOF,
        TypeExpr::TypeParameter(param) => {
            SPECIFICITY_REF_BASE
                + param
                    .constraint
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Ref { type_arguments, .. } => {
            SPECIFICITY_REF_BASE
                + type_arguments
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => {
            SPECIFICITY_WRAPPER_BASE + imported_type_body_specificity_score(element)
        }
        TypeExpr::Tuple { elements, .. } => {
            SPECIFICITY_TUPLE_BASE
                + elements
                    .iter()
                    .map(|element| imported_type_body_specificity_score(&element.ty))
                    .sum::<usize>()
        }
        TypeExpr::Union(types) => {
            SPECIFICITY_UNION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Intersection(types) => {
            SPECIFICITY_INTERSECTION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Object(obj) => {
            SPECIFICITY_OBJECT_BASE
                + obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(prop) => {
                            SPECIFICITY_OBJECT_PROPERTY
                                + imported_type_body_specificity_score(&prop.ty)
                        }
                        ObjectMember::IndexSignature(sig) => {
                            SPECIFICITY_INDEX_SIGNATURE
                                + imported_type_body_specificity_score(&sig.key_type)
                                + imported_type_body_specificity_score(&sig.value_type)
                        }
                        ObjectMember::CallSignature(func)
                        | ObjectMember::ConstructSignature(func) => {
                            SPECIFICITY_CALL_LIKE_MEMBER + imported_function_specificity_score(func)
                        }
                        ObjectMember::Method(method) => {
                            SPECIFICITY_CALL_LIKE_MEMBER
                                + imported_function_specificity_score(&method.function)
                        }
                    })
                    .sum::<usize>()
        }
        // A constructor type scores as a function surface (same `FunctionExpr`
        // payload, same callable-specificity contribution).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            SPECIFICITY_FUNCTION_BASE + imported_function_specificity_score(func)
        }
        TypeExpr::IndexedAccess { object, index } => {
            SPECIFICITY_INDEXED_ACCESS_BASE
                + imported_type_body_specificity_score(object)
                + imported_type_body_specificity_score(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            SPECIFICITY_WRAPPER_BASE
                + imported_type_body_specificity_score(check)
                + imported_type_body_specificity_score(extends)
                + imported_type_body_specificity_score(true_type)
                + imported_type_body_specificity_score(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            SPECIFICITY_MAPPED_BASE
                + imported_type_body_specificity_score(source)
                + imported_type_body_specificity_score(value)
                + name_type
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            SPECIFICITY_TEMPLATE_LITERAL_BASE
                + expressions
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Infer { .. } => SPECIFICITY_TYPEOF,
        TypeExpr::RecursiveRef { .. } => SPECIFICITY_REF_BASE,
        // Synthetic carrier — intrinsic terminal leaf, same specificity
        // as primitive / literal terminals.
        TypeExpr::SyntheticSlotBinding(_) => SPECIFICITY_TERMINAL,
    }
}

fn imported_function_specificity_score(func: &FunctionExpr) -> usize {
    let params = func
        .parameters
        .iter()
        .map(|param| imported_type_body_specificity_score(&param.ty))
        .sum::<usize>();
    let ret = func
        .return_type
        .as_deref()
        .map(imported_type_body_specificity_score)
        .unwrap_or_default();
    let generics = func
        .type_parameters
        .iter()
        .map(|param| {
            param
                .constraint
                .as_deref()
                .map(imported_type_body_specificity_score)
                .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        })
        .sum::<usize>();
    params + ret + generics
}

pub(crate) fn component_meta_registry_has_non_object_top_level_surface(
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            component_meta_registry_has_non_object_top_level_surface(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types
                .iter()
                .any(component_meta_registry_has_non_object_top_level_surface)
                || types.iter().any(|ty| !matches!(ty, TypeExpr::Object(_)))
        }
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Object(_) => false,
        _ => false,
    }
}

pub(crate) fn component_meta_registry_has_explicit_object_surface(
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            component_meta_registry_has_explicit_object_surface(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(component_meta_registry_has_explicit_object_surface),
        TypeExpr::Object(_) => true,
        _ => false,
    }
}

pub(crate) fn component_meta_registry_raw_member_path_surface(
    expr: &verter_type_expr::TypeExpr,
    path: &[String],
) -> Option<verter_type_expr::TypeExpr> {
    use verter_type_expr::{MemberVisibility, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

    /// Navigate into a `TypeExpr::Object` by a single property name,
    /// unwrapping `Parenthesized`. Returns the matched source
    /// `ObjectProperty` (value + declared visibility) so the member-path
    /// wrapper can thread the navigated member's visibility rather than
    /// silently re-minting it as `Public`. Returns `None` if `expr` is not
    /// an Object or no member matches.
    fn navigate_object_member<'a>(
        expr: &'a TypeExpr,
        member_name: &str,
    ) -> Option<&'a ObjectProperty> {
        match expr {
            TypeExpr::Parenthesized(inner) => navigate_object_member(inner, member_name),
            TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                ObjectMember::Property(property) if property.name == member_name => Some(property),
                _ => None,
            }),
            _ => None,
        }
    }

    if path.is_empty() {
        return Some(expr.clone());
    }

    let mut leaf = expr;
    // Record each hop's declared visibility (aligned with `path`) so the
    // rebuilt wrapper preserves the source member's accessibility.
    let mut hop_visibilities: Vec<MemberVisibility> = Vec::with_capacity(path.len());
    for member_name in path {
        let property = navigate_object_member(leaf, member_name)?;
        hop_visibilities.push(property.visibility);
        leaf = &property.ty;
    }

    Some(
        path.iter().zip(hop_visibilities).rev().fold(
            leaf.clone(),
            |child, (member_name, visibility)| {
                // Nested-object wrapper for one navigation hop. The member name
                // comes from the path; its visibility is the source member's
                // declared accessibility (threaded above) so a non-public hop is
                // never re-minted as `Public`.
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(
                        ObjectProperty::synthetic_with_visibility(
                            member_name.clone(),
                            child,
                            true,
                            false,
                            visibility,
                        ),
                    )],
                }))
            },
        ),
    )
}

pub(crate) fn component_meta_registry_expr_references_name(
    expr: &verter_type_expr::TypeExpr,
    target_name: &str,
) -> bool {
    use verter_type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        }
        | TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            name.as_ref() == target_name
                || type_arguments
                    .iter()
                    .any(|arg| component_meta_registry_expr_references_name(arg, target_name))
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::Rest(element)
        | TypeExpr::KeyOf(element) => {
            component_meta_registry_expr_references_name(element, target_name)
        }
        TypeExpr::IndexedAccess { object, index } => {
            component_meta_registry_expr_references_name(object, target_name)
                || component_meta_registry_expr_references_name(index, target_name)
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| component_meta_registry_expr_references_name(&element.ty, target_name)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(|ty| component_meta_registry_expr_references_name(ty, target_name)),
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => {
                component_meta_registry_expr_references_name(&property.ty, target_name)
            }
            ObjectMember::IndexSignature(signature) => {
                component_meta_registry_expr_references_name(&signature.key_type, target_name)
                    || component_meta_registry_expr_references_name(
                        &signature.value_type,
                        target_name,
                    )
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function.parameters.iter().any(|param| {
                    component_meta_registry_expr_references_name(&param.ty, target_name)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    component_meta_registry_expr_references_name(return_type, target_name)
                })
            }
            ObjectMember::Method(method) => {
                method.function.parameters.iter().any(|param| {
                    component_meta_registry_expr_references_name(&param.ty, target_name)
                }) || method
                    .function
                    .return_type
                    .as_deref()
                    .is_some_and(|return_type| {
                        component_meta_registry_expr_references_name(return_type, target_name)
                    })
            }
        }),
        // A constructor type may reference the target name in its parameters /
        // return exactly like a function type (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function
                .parameters
                .iter()
                .any(|param| component_meta_registry_expr_references_name(&param.ty, target_name))
                || function.return_type.as_deref().is_some_and(|return_type| {
                    component_meta_registry_expr_references_name(return_type, target_name)
                })
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            component_meta_registry_expr_references_name(check, target_name)
                || component_meta_registry_expr_references_name(extends, target_name)
                || component_meta_registry_expr_references_name(true_type, target_name)
                || component_meta_registry_expr_references_name(false_type, target_name)
        }
        TypeExpr::Mapped {
            source,
            name_type,
            value,
            ..
        } => {
            component_meta_registry_expr_references_name(source, target_name)
                || name_type.as_deref().is_some_and(|name_type| {
                    component_meta_registry_expr_references_name(name_type, target_name)
                })
                || component_meta_registry_expr_references_name(value, target_name)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|expr| component_meta_registry_expr_references_name(expr, target_name)),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers reference no public type name — their
        // identity is closed (the binding_name is intrinsic, not a
        // registry-lookup target).
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => false,
    }
}

pub(crate) fn component_meta_registry_indexed_ref_penalty(
    expr: &verter_type_expr::TypeExpr,
) -> usize {
    use verter_type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::IndexedAccess { object, index } => {
            let local_penalty = matches!(object.as_ref(), TypeExpr::Ref { .. }) as usize;
            local_penalty
                + component_meta_registry_indexed_ref_penalty(object)
                + component_meta_registry_indexed_ref_penalty(index)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .map(component_meta_registry_indexed_ref_penalty)
            .sum(),
        TypeExpr::Array { element, .. }
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element) => component_meta_registry_indexed_ref_penalty(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| component_meta_registry_indexed_ref_penalty(&element.ty))
            .sum(),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Property(prop) => {
                    component_meta_registry_indexed_ref_penalty(&prop.ty)
                }
                ObjectMember::IndexSignature(sig) => {
                    component_meta_registry_indexed_ref_penalty(&sig.key_type)
                        + component_meta_registry_indexed_ref_penalty(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.parameters
                        .iter()
                        .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + func
                            .return_type
                            .as_deref()
                            .map(component_meta_registry_indexed_ref_penalty)
                            .unwrap_or(0)
                }
                ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + method
                            .function
                            .return_type
                            .as_deref()
                            .map(component_meta_registry_indexed_ref_penalty)
                            .unwrap_or(0)
                }
            })
            .sum(),
        // A constructor type is penalised identically to a function type (same
        // `FunctionExpr` payload walked for indexed refs).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            func.parameters
                .iter()
                .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(component_meta_registry_indexed_ref_penalty)
                    .unwrap_or(0)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            component_meta_registry_indexed_ref_penalty(check)
                + component_meta_registry_indexed_ref_penalty(extends)
                + component_meta_registry_indexed_ref_penalty(true_type)
                + component_meta_registry_indexed_ref_penalty(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            component_meta_registry_indexed_ref_penalty(source)
                + component_meta_registry_indexed_ref_penalty(value)
                + name_type
                    .as_deref()
                    .map(component_meta_registry_indexed_ref_penalty)
                    .unwrap_or(0)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .map(component_meta_registry_indexed_ref_penalty)
            .sum(),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers carry no indexed-ref penalty.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => 0,
    }
}

/// Walk `expr` and enqueue every nominal `Ref` reachable through the
/// supplied [`ProjectionCursor`].
///
/// **Cursor contract** (per the G1 + G2 path-precision gates): when the
/// cursor is at a whole-surface node (`is_whole_surface()`), descent
/// is unbounded — preserves pre-path-precision behaviour. When the cursor
/// carries a narrowed filter (Pick → `Include`, Omit → `Exclude`,
/// or an explicit `ProjectionNode::children` map):
///
/// - **Object arm** (G2): `Property` and `Method` (named members)
///   gate per-member descent on `cursor.admits_key(name)`. Anonymous
///   structural members — `IndexSignature`, `CallSignature`,
///   `ConstructSignature` — are skipped entirely under a narrowed
///   cursor because named-key narrowing (Pick/Omit) produces a
///   property-only surface that does not carry index/callable
///   shapes.
///
/// - **Conditional arm** (G1): the predicate sides (`check`,
///   `extends`) are type-level operands, NOT part of the published
///   value surface. Under a narrowed cursor we walk only the
///   result-side branches (`true_type`, `false_type`) — both,
///   because openness is not tracked here (treat as open: distribute
///   the remaining demand into both branches). Whole-surface walks
///   all four.
///
/// - **Mapped arm** (G1): the `source` (the mapped-source key
///   domain `T` in `{ [K in keyof T]: V }`) and the `name_type`
///   (the `as` remapping) are type-level metadata. Under a narrowed
///   cursor we walk only `value` (the produced value type for the
///   requested keys). Whole-surface walks `source` + `value` +
///   `name_type`.
pub(crate) fn collect_component_meta_registry_refs(
    expr: &verter_type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    allow_plain_member_refs: bool,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) {
    use verter_type_expr::TypeExpr;

    if let Some((root_name, route)) = component_meta_registry_public_utility_route(expr) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            None,
            route,
        );
        return;
    }

    if let Some((root_name, route)) = component_meta_registry_public_indexed_access_route(expr) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            None,
            route,
        );
        return;
    }

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            collect_component_meta_registry_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
                cursor,
            );
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_component_meta_registry_refs(
                    &element.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                    cursor,
                );
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            if !allow_plain_member_refs {
                return;
            }
            for ty in types.iter() {
                collect_component_meta_registry_refs(
                    ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                    cursor,
                );
            }
        }
        // Registry publication stays shallow: object/function member types remain
        // inline on the owning helper instead of spawning separate registry
        // entries for every nested support type. Routed helper refs still need
        // to preserve their member path here; imported deep member-path roots
        // are filtered later in `meta_resolve` once the consuming field surface
        // has already been projected concretely.
        TypeExpr::Object(obj) => {
            use verter_type_expr::ObjectMember;

            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        // Path-precise gate. If
                        // the cursor's key filter rejects this
                        // property's name, skip the member's nested
                        // refs entirely — that sibling is OUTSIDE
                        // the published surface the consumer walks.
                        // Whole-surface cursors admit every key so
                        // pre-path-precision top-level callers see no
                        // behaviour change.
                        if !cursor.admits_key(prop.name.as_str()) {
                            continue;
                        }
                        collect_component_meta_registry_member_surface_refs(
                            &prop.ty,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        // G2 path-precision gate for
                        // non-Property members. `IndexSignature`
                        // (`[key: K]: V`) is anonymous; it
                        // applies structurally to every key
                        // matching `K`. Under a narrowed cursor
                        // (Pick/Omit/explicit-children) the
                        // published surface enumerates named keys
                        // only — the index signature's `V` is NOT
                        // in the narrowed value surface. Skip its
                        // nested refs unless the cursor is
                        // genuinely whole-surface.
                        if !cursor.is_whole_surface() {
                            continue;
                        }
                        collect_component_meta_registry_member_surface_refs(
                            &sig.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                        collect_component_meta_registry_member_surface_refs(
                            &sig.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        // G2 path-precision gate.
                        // `CallSignature` / `ConstructSignature`
                        // are anonymous callable shapes; named-
                        // key narrowing (`Pick<Foo, "k">`)
                        // produces a property-only surface that
                        // does NOT carry the callable shape.
                        // Skip nested refs unless whole-surface.
                        if !cursor.is_whole_surface() {
                            continue;
                        }
                        collect_component_meta_registry_function_surface_refs(
                            func,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                    ObjectMember::Method(method) => {
                        // G2 path-precision gate.
                        // Methods are named members; apply the
                        // same `admits_key` gate as the Property
                        // arm so a `Pick<Foo, "methodA">`-style
                        // narrowing prunes `methodB`'s nested
                        // refs.
                        if !cursor.admits_key(method.name.as_str()) {
                            continue;
                        }
                        collect_component_meta_registry_function_surface_refs(
                            &method.function,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                }
            }
        }
        // A constructor type's signature surface is collected identically to a
        // function type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            collect_component_meta_registry_function_surface_refs(
                func,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_component_meta_registry_refs(
                object,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
                cursor,
            );
            collect_component_meta_registry_refs(
                index,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
                cursor,
            );
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            if !allow_plain_member_refs {
                return;
            }
            // G1 path-precision gate. The doc-comment on
            // this fn promises the Conditional arm gates on
            // `is_whole_surface()`; the pre-G1 impl unconditionally
            // recursed into all four branches, leaking unselected
            // type-level predicate sides into a narrowed surface.
            //
            // - Whole-surface (no narrowing): walk all four (check +
            //   extends + true_type + false_type). Preserves
            //   pre-path-precision top-level callers' refs and audit
            //   behaviour.
            // - Narrowed (Include/Exclude/explicit-children at this
            //   hop): the cursor enumerates a value-surface; the
            //   conditional's `check`/`extends` are type-level
            //   predicate operands, NOT part of the published-value
            //   surface. We only enqueue refs reachable through the
            //   result-side branches (`true_type` + `false_type`)
            //   because we can't statically tell which branch a path
            //   resolves to (treat as open: distribute remaining
            //   demand into both). Cf. CLAUDE.md "Macro Type
            //   Traversal Rule": open conditionals distribute the
            //   remaining path into both branches.
            if cursor.is_whole_surface() {
                collect_component_meta_registry_refs(
                    check,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                    cursor,
                );
                collect_component_meta_registry_refs(
                    extends,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                    cursor,
                );
            }
            collect_component_meta_registry_refs(
                true_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
                cursor,
            );
            collect_component_meta_registry_refs(
                false_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
                cursor,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            if !allow_plain_member_refs {
                return;
            }
            // G1 path-precision gate. The doc-comment on
            // this fn promises the Mapped arm gates on
            // `is_whole_surface()`; the pre-G1 impl unconditionally
            // recursed into source + value + name_type, leaking the
            // mapped-source key-domain into a narrowed surface even
            // when only specific keys are requested.
            //
            // - Whole-surface: walk source + value (+ name_type).
            //   Preserves pre-path-precision refs.
            // - Narrowed: walk only `value` — the published-value
            //   surface produced by `{ [K in keyof T]: V }` for the
            //   requested keys is `V`; the mapped-source `T` itself
            //   (and the `as` name_type remapping) is type-level
            //   metadata outside the value surface.
            if cursor.is_whole_surface() {
                collect_component_meta_registry_refs(
                    source,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                    cursor,
                );
            }
            collect_component_meta_registry_refs(
                value,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
                cursor,
            );
            if cursor.is_whole_surface() {
                if let Some(name_type) = name_type.as_deref() {
                    collect_component_meta_registry_refs(
                        name_type,
                        published_names,
                        queued_names,
                        output,
                        source_hint,
                        allow_plain_member_refs,
                        cursor,
                    );
                }
            }
        }
        TypeExpr::TypeParameter(_) => {}
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        // Synthetic carrier — never enqueues as a public type
        // reference; its `binding_name` is intrinsic, not a workspace
        // alias.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => {}
    }
}

pub(crate) fn collect_component_meta_registry_function_surface_refs(
    func: &verter_type_expr::FunctionExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    for param in &func.parameters {
        collect_component_meta_registry_member_surface_refs(
            &param.ty,
            published_names,
            queued_names,
            output,
            source_hint,
            false,
        );
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_component_meta_registry_member_surface_refs(
            return_type,
            published_names,
            queued_names,
            output,
            source_hint,
            false,
        );
    }
}

pub(crate) fn collect_component_meta_registry_public_field_refs(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    // Typed-IR-Only Resolver Rule: when the post-expansion
    // `field.r#type` carries no actionable route (no `IndexedAccess`,
    // `Pick`, etc. shape the registry can route on), fall back to the
    // analyzer-populated shallow form — the bare annotation the user
    // wrote, e.g. `TypeExpr::Ref { name: "Props" }` or
    // `TypeExpr::IndexedAccess { object: Ref { name: "Props" }, … }`.
    // No reparse of `raw_type`.
    let shallow_recovery =
        (!component_meta_registry_field_expr_has_actionable_route(&field.r#type))
            .then_some(field.shallow_type_expr.as_ref())
            .flatten()
            .filter(|shallow| {
                let deep_indexed_path = component_meta_registry_public_indexed_access_route(
                    shallow,
                )
                .is_some_and(|(_, route)| {
                    matches!(
                        route,
                        RouteDemand::MemberPath(ref path) if path.len() > 1,
                    )
                });
                !deep_indexed_path
                    || component_meta_registry_has_explicit_object_surface(&field.r#type)
            });
    let expr = shallow_recovery.unwrap_or(&field.r#type);

    // Issue #7 / owner-local ComponentConfig alias rewrite.
    // When the indexed-access or utility route's root resolves to an
    // owner-local alias whose body is `ComponentConfig<...>`, emit a
    // `RouteDemand::Whole(root)` instead of `MemberPath`/`Pick`. The
    // registry materialises the alias once and reuses the result for
    // every later projection.
    //
    // External imports preserve `MemberPath`/`Pick` (the predicate
    // declines when the route root has an import binding).
    if let Some(owner_local_root) = component_meta_registry_public_route_owner_local_root(
        ctx,
        owner_canonical,
        snapshot,
        expr,
        source_hint,
    ) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            owner_local_root.as_str(),
            source_hint,
            None,
            RouteDemand::Whole,
        );
        return;
    }

    let skip_direct_plain_ref = component_meta_registry_ref_name(expr).is_some_and(|name| {
        ctx.prepared_type_decl(owner_canonical, name)
            .is_some_and(|prepared| {
                matches!(prepared.body, verter_type_expr::TypeExpr::TypeParameter(_),)
            })
            || owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name)
                .and_then(|(canonical_id, exported_name)| {
                    (!canonical_id.is_empty()
                        && !ctx.workspace_is_package_backed(canonical_id.as_str()))
                    .then(|| ctx.prepared_type_decl(canonical_id.as_str(), exported_name.as_str()))
                })
                .flatten()
                .is_some_and(|prepared| {
                    component_meta_registry_has_non_object_top_level_surface(&prepared.body)
                        && !component_meta_registry_has_explicit_object_surface(&prepared.body)
                })
            || owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name)
                .is_some_and(|(canonical_id, _)| {
                    ctx.workspace_is_package_backed(canonical_id.as_str())
                })
            || ctx.workspace_is_package_backed(
                ctx.resolve_type_declaration_for_dep(owner_canonical, name)
                    .canonical_source
                    .as_str(),
            )
    });
    let skip_imported_generic_non_object_ref = component_meta_registry_direct_public_ref(expr)
        .is_some_and(|(name, type_arguments)| {
            if type_arguments.is_empty() {
                return false;
            }
            let Some((canonical_id, exported_name)) =
                owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name)
            else {
                return false;
            };
            if canonical_id.is_empty() || ctx.workspace_is_package_backed(canonical_id.as_str()) {
                return false;
            }
            ctx.prepared_type_decl(canonical_id.as_str(), exported_name.as_str())
                .is_some_and(|prepared| {
                    component_meta_registry_has_non_object_top_level_surface(&prepared.body)
                        && !component_meta_registry_has_explicit_object_surface(&prepared.body)
                })
        });
    let direct_ref = component_meta_registry_direct_public_ref(expr);
    if (skip_direct_plain_ref || skip_imported_generic_non_object_ref)
        && direct_ref.is_some_and(|(name, _)| {
            let local_type_parameter =
                ctx.prepared_type_decl(owner_canonical, name)
                    .is_some_and(|prepared| {
                        matches!(prepared.body, verter_type_expr::TypeExpr::TypeParameter(_),)
                    });
            let import_root =
                owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name);
            let package_backed = import_root.as_ref().is_some_and(|(canonical_id, _)| {
                ctx.workspace_is_package_backed(canonical_id.as_str())
            }) || ctx.workspace_is_package_backed(
                ctx.resolve_type_declaration_for_dep(owner_canonical, name)
                    .canonical_source
                    .as_str(),
            );
            !local_type_parameter && !package_backed
        })
    {
        if let Some((name, _)) = direct_ref {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name,
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
    }
    if !skip_direct_plain_ref && !skip_imported_generic_non_object_ref {
        collect_component_meta_registry_public_surface_refs(
            expr,
            published_names,
            queued_names,
            output,
            source_hint,
        );
    }

    collect_component_meta_registry_public_indexed_access_roots(
        ctx,
        owner_canonical,
        expr,
        published_names,
        queued_names,
        output,
        source_hint,
    );
}

pub(crate) fn component_meta_registry_ref_name(expr: &verter_type_expr::TypeExpr) -> Option<&str> {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => Some(name.as_ref()),
        TypeExpr::Parenthesized(inner) => component_meta_registry_ref_name(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_direct_public_ref(
    expr: &verter_type_expr::TypeExpr,
) -> Option<(&str, &[verter_type_expr::TypeExpr])> {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => Some((name.as_ref(), type_arguments.as_ref())),
        TypeExpr::Parenthesized(inner) => component_meta_registry_direct_public_ref(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_field_expr_has_actionable_route(
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    component_meta_registry_direct_public_ref(expr).is_some()
        || component_meta_registry_public_utility_route(expr).is_some()
        || component_meta_registry_public_indexed_access_route(expr).is_some()
}

pub(crate) fn component_meta_registry_string_literal_keys(
    expr: &verter_type_expr::TypeExpr,
) -> Option<Vec<String>> {
    use verter_type_expr::{LiteralValue, TypeExpr};

    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
        TypeExpr::Union(types) => {
            let mut keys = Vec::new();
            for ty in types.iter() {
                keys.extend(component_meta_registry_string_literal_keys(ty)?);
            }
            keys.sort();
            keys.dedup();
            Some(keys)
        }
        TypeExpr::Parenthesized(inner) => component_meta_registry_string_literal_keys(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_public_utility_route(
    expr: &verter_type_expr::TypeExpr,
) -> Option<(String, RouteDemand)> {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => component_meta_registry_public_utility_route(inner),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.len() == 2 && matches!(name.as_ref(), "Pick" | "Omit") => {
            // Allow `Pick`/`Omit` over BOTH bare Refs (`Pick<Foo, K>`)
            // and INSTANTIATED generic Refs (`Pick<Foo<T>, K>`). The
            // route consumer surfaces the inner ref's declared
            // members under `symbol_name` — for bare and instantiated
            // forms alike, the published prop NAMES come from the
            // declaration body. Type-parameter substitutions affect
            // member VALUES (e.g. `items?: T` body shape), not member
            // names, and the route applies a name-set filter (Pick /
            // Omit). Allowing the inner Ref to carry `type_arguments`
            // closes the cross-package + instantiated-generic gap
            // (CP1) — pre-fix `component_meta_registry_ref_name`
            // required `type_arguments.is_empty()` and the route
            // extractor returned `None` for every utility wrap over
            // an instantiated generic, falling through to a path
            // that flattened the inner generic's body without
            // applying the Pick/Omit filter.
            let root_name =
                component_meta_registry_utility_inner_ref_name(&type_arguments[0])?.to_string();
            let members = component_meta_registry_string_literal_keys(&type_arguments[1])?;
            if members.is_empty() {
                return None;
            }
            let route = if name.as_ref() == "Pick" {
                RouteDemand::Pick(members)
            } else {
                RouteDemand::Omit(members)
            };
            Some((root_name, route))
        }
        _ => None,
    }
}

/// Extract the inner Ref's name for `Pick<X, K>` / `Omit<X, K>` route
/// extraction. Mirrors [`component_meta_registry_ref_name`] but
/// permits the Ref to carry `type_arguments` — the route consumer
/// dispatches the inner ref's declared members under `symbol_name`,
/// and the Pick/Omit filter operates on member NAMES (not member
/// types), so the type arguments do not affect route discriminability.
/// Used exclusively by the public utility route extractor (above).
/// Other callers of `component_meta_registry_ref_name` enforce the
/// `type_arguments.is_empty()` invariant for different reasons —
/// e.g. indexed-access chain roots that MUST be bare; those
/// constraints stay verbatim.
///
/// Iterative walk through `Parenthesized` shells — the loop is bounded
/// by the number of nested parentheses (a structural property of the
/// caller-supplied AST). No self-recursion, so the
/// `no_unbounded_recursion_in_resolver_core` architecture guard does
/// not need an entry for this helper.
fn component_meta_registry_utility_inner_ref_name(
    expr: &verter_type_expr::TypeExpr,
) -> Option<&str> {
    use verter_type_expr::TypeExpr;

    let mut cursor = expr;
    loop {
        match cursor {
            TypeExpr::Ref { name, .. } => return Some(name.as_ref()),
            TypeExpr::Parenthesized(inner) => cursor = inner.as_ref(),
            _ => return None,
        }
    }
}

pub(crate) fn component_meta_registry_public_indexed_access_route(
    expr: &verter_type_expr::TypeExpr,
) -> Option<(String, RouteDemand)> {
    use verter_type_expr::TypeExpr;

    fn collect_path(expr: &TypeExpr, path: &mut Vec<String>) -> Option<String> {
        use verter_type_expr::{LiteralValue, TypeExpr};

        match expr {
            TypeExpr::IndexedAccess { object, index } => {
                let root = collect_path(object, path)?;
                let TypeExpr::Literal(LiteralValue::String(member)) = index.as_ref() else {
                    return None;
                };
                path.push(member.clone());
                Some(root)
            }
            TypeExpr::Parenthesized(inner) => collect_path(inner, path),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => Some(name.to_string()),
            _ => None,
        }
    }

    let mut path = Vec::new();
    let root = collect_path(expr, &mut path)?;
    if path.is_empty() {
        return None;
    }
    Some((root, RouteDemand::MemberPath(path)))
}

pub(crate) fn collect_component_meta_registry_public_surface_refs(
    expr: &verter_type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
        TypeExpr::Parenthesized(element) => {
            collect_component_meta_registry_public_surface_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Array { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. } => {}
        TypeExpr::Object(_)
        | TypeExpr::Function(_)
        // A constructor type, like a function/object type, enqueues no top-level
        // public-surface ref here (its inner signature is walked elsewhere).
        | TypeExpr::ConstructorType(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carrier — never enqueues as a public surface ref.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => {}
    }
}

pub(crate) fn collect_component_meta_registry_public_indexed_access_roots(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    let Some((root_name, route)) = component_meta_registry_public_indexed_access_route(expr) else {
        return;
    };
    let Some(prepared) = ctx.prepared_type_decl(owner_canonical, root_name.as_str()) else {
        return;
    };
    if matches!(prepared.body, verter_type_expr::TypeExpr::TypeParameter(_),) {
        return;
    }
    enqueue_component_meta_registry_ref(
        published_names,
        queued_names,
        output,
        root_name.as_str(),
        source_hint,
        None,
        route,
    );
}

pub(crate) fn collect_component_meta_registry_member_surface_refs(
    expr: &verter_type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    allow_plain_refs: bool,
) {
    use verter_type_expr::{ObjectMember, TypeExpr};

    if let Some((root_name, route)) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
    {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            None,
            route,
        );
        return;
    }

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } if allow_plain_refs => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_component_meta_registry_member_surface_refs(
                object,
                published_names,
                queued_names,
                output,
                source_hint,
                true,
            );
            collect_component_meta_registry_member_surface_refs(
                index,
                published_names,
                queued_names,
                output,
                source_hint,
                true,
            );
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            collect_component_meta_registry_member_surface_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_component_meta_registry_member_surface_refs(
                    &element.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            for ty in types.iter() {
                collect_component_meta_registry_member_surface_refs(
                    ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_component_meta_registry_member_surface_refs(
                check,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                extends,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                true_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                false_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_component_meta_registry_member_surface_refs(
                source,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                value,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_component_meta_registry_member_surface_refs(
                    name_type,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        // A constructor type's signature surface is collected identically to a
        // function type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            collect_component_meta_registry_function_surface_refs(
                func,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_component_meta_registry_member_surface_refs(
                            &prop.ty,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_component_meta_registry_member_surface_refs(
                            &sig.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                        collect_component_meta_registry_member_surface_refs(
                            &sig.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_component_meta_registry_function_surface_refs(
                            func,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_component_meta_registry_function_surface_refs(
                            &method.function,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                }
            }
        }
        TypeExpr::TypeParameter(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        // Synthetic carrier — never enqueues as a member surface ref.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Ref { .. } => {}
    }
}

// + §6.10 sub-task 4 — both legacy
// member-path helpers retired. The shared object-member navigation
// logic is inlined into the body of
// `component_meta_registry_raw_member_path_surface` (its only
// surviving caller). The retired symbols are listed in the
// `RETIRED_SYMBOLS` array of the static-grep gate test.

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use super::{
        choose_preferred_imported_type_body, collect_component_meta_registry_refs,
        component_meta_registry_field_expr_has_actionable_route,
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_raw_member_path_surface, enqueue_component_meta_registry_ref,
        imported_type_body_specificity_score, merge_component_meta_registry_candidates,
        method_surface_specificity_score, owner_component_meta_registry_import_root, RouteDemand,
    };
    use crate::types::{AnalysisLevel, DependencyResolution, HostConfig};
    use crate::VerterHost;
    use verter_type_expr::{
        FunctionExpr, FunctionParam, LiteralValue, MethodSignature, ObjectExpr, ObjectMember,
        ObjectProperty, PrimitiveName, TypeExpr, ValueRef,
    };

    fn object_with_props(names: &[&str]) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: names
                .iter()
                .map(|name| {
                    ObjectMember::Property(ObjectProperty::synthetic_public(
                        (*name).to_string(),
                        TypeExpr::Primitive(PrimitiveName::String),
                        false,
                        false,
                    ))
                })
                .collect(),
        }))
    }

    fn empty_object() -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: Vec::new(),
        }))
    }

    /// `{ cb: <callable> }` where the property value is a callable surface
    /// (`() => void`). Built for either a `Function` or a `ConstructorType`
    /// property value carrying the same `FunctionExpr` payload.
    fn object_with_callable_prop(constructor: bool) -> TypeExpr {
        let func = Arc::new(FunctionExpr::synthetic(
            Vec::new(),
            Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
            Vec::new(),
        ));
        let cb = if constructor {
            TypeExpr::ConstructorType(func)
        } else {
            TypeExpr::Function(func)
        };
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "cb".to_string(),
                cb,
                false,
                false,
            ))],
        }))
    }

    /// A constructor-type property is an equally-specific callable surface as a
    /// function-type property, so the specificity score must be IDENTICAL.
    /// Discriminating: the `Object`-property branch counted
    /// `matches!(prop.ty, TypeExpr::Function(_))` only; a constructor-type
    /// property missed that callable-surface bonus and scored one lower.
    #[test]
    fn constructor_type_prop_scores_like_function_prop() {
        let function_obj = object_with_callable_prop(false);
        let constructor_obj = object_with_callable_prop(true);
        assert_eq!(
            method_surface_specificity_score(&constructor_obj),
            method_surface_specificity_score(&function_obj),
            "a constructor-type property is an equally-specific callable surface \
             as a function-type property",
        );
    }

    #[test]
    fn indexed_access_route_preserves_full_member_path() {
        let expr = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
            "Button['variants']['color']",
            None,
        );

        assert_eq!(
            component_meta_registry_public_indexed_access_route(&expr),
            Some((
                "Button".to_string(),
                RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]),
            ))
        );
    }

    #[test]
    fn collect_registry_refs_preserves_indexed_access_member_path() {
        let expr =
            verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload("Button['ui']", None);
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
        );

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            Some("/src/Button.vue"),
            false,
            proj.cursor(),
        );

        let pending = output
            .pop_front()
            .expect("indexed-access helper should enqueue a registry ref");
        assert_eq!(pending.name, "Button");
        assert_eq!(pending.source_hint.as_deref(), Some("/src/Button.vue"));
        assert_eq!(pending.exported_name, None);
        assert_eq!(
            pending.route,
            RouteDemand::MemberPath(vec!["ui".to_string()]),
            "indexed-access helper refs should preserve the requested member path instead of widening to Whole",
        );
        assert!(
            output.is_empty(),
            "indexed-access helper refs should enqueue only the routed root helper"
        );
    }

    #[test]
    fn enqueue_registry_ref_keeps_deep_member_paths_separate_from_top_level_picks() {
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();

        enqueue_component_meta_registry_ref(
            &published_names,
            &mut queued_names,
            &mut output,
            "Button",
            Some("/src/Button.vue"),
            None,
            RouteDemand::Pick(vec!["slots".to_string()]),
        );
        enqueue_component_meta_registry_ref(
            &published_names,
            &mut queued_names,
            &mut output,
            "Button",
            Some("/src/Button.vue"),
            None,
            RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]),
        );

        assert_eq!(
            output.len(),
            2,
            "deep member-path requests must not be collapsed into a top-level pick for the same root"
        );
        assert_eq!(
            output[0].route,
            RouteDemand::Pick(vec!["slots".to_string()]),
        );
        assert_eq!(
            output[1].route,
            RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]),
        );
    }

    #[test]
    fn merge_registry_candidates_combines_partial_object_routes() {
        let left = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "slots".to_string(),
                object_with_props(&["base", "label"]),
                true,
                false,
            ))],
        }));
        let right = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "variants".to_string(),
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "color".to_string(),
                        TypeExpr::union(vec![
                            TypeExpr::string_literal("primary"),
                            TypeExpr::string_literal("secondary"),
                        ]),
                        false,
                        false,
                    ))],
                })),
                true,
                false,
            ))],
        }));

        let merged = merge_component_meta_registry_candidates(Some(left), Some(right))
            .expect("partial route candidates should merge");
        let TypeExpr::Object(shape) = &merged else {
            panic!("merged partial route candidates should stay object-shaped");
        };

        assert_eq!(
            shape.properties.len(),
            2,
            "merged object should have exactly 2 properties (slots, variants)"
        );
        assert!(shape.properties.iter().any(|member| {
            matches!(member, ObjectMember::Property(property) if property.name == "slots")
        }));
        let variants = shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "variants" => {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("merged surface should keep variants");
        let TypeExpr::Object(variants_shape) = variants else {
            panic!("variants should stay object-shaped after merge, got {variants:?}");
        };
        assert_eq!(
            variants_shape.properties.len(),
            1,
            "variants should have exactly 1 property (color)"
        );
        assert!(variants_shape.properties.iter().any(|member| {
            matches!(member, ObjectMember::Property(property) if property.name == "color")
        }));
    }

    fn property_with_visibility(
        name: &str,
        vis: verter_type_expr::MemberVisibility,
    ) -> ObjectMember {
        ObjectMember::Property(ObjectProperty::with_visibility(
            name.to_string(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
            vis,
            verter_type_expr::MemberSpans::default(),
        ))
    }

    fn merged_property_visibility(
        merged: &TypeExpr,
        name: &str,
    ) -> verter_type_expr::MemberVisibility {
        let TypeExpr::Object(obj) = merged else {
            panic!("merged candidate should be object-shaped, got {merged:?}");
        };
        obj.properties
            .iter()
            .find_map(|m| match m {
                ObjectMember::Property(p) if p.name == name => Some(p.visibility),
                _ => None,
            })
            .unwrap_or_else(|| panic!("merged object must contain `{name}`"))
    }

    /// F3 (RHS-only member): registry object merge must carry an RHS-only
    /// property's declared accessibility verbatim — a private member
    /// contributed only by the right side stays Private on the merged surface.
    ///
    /// Discriminating: against the tree where the RHS-only push uses
    /// `with_spans`, the merged `right_only` member is Public and this FAILS.
    #[test]
    fn merge_registry_preserves_rhs_only_member_visibility() {
        use verter_type_expr::MemberVisibility;
        let left = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![property_with_visibility(
                "left_only",
                MemberVisibility::Public,
            )],
        }));
        let right = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![property_with_visibility(
                "right_only",
                MemberVisibility::Private,
            )],
        }));

        let merged = merge_component_meta_registry_candidates(Some(left), Some(right))
            .expect("object candidates merge");
        assert_eq!(
            merged_property_visibility(&merged, "right_only"),
            MemberVisibility::Private,
            "an RHS-only private member must stay Private through the registry merge",
        );
        assert_eq!(
            merged_property_visibility(&merged, "left_only"),
            MemberVisibility::Public,
            "the LHS public member is unchanged",
        );
    }

    /// F3 (duplicate member): when both sides declare a member of the same
    /// name, the merged member takes the MOST-RESTRICTIVE visibility (the shared
    /// rule) — Public only when Public in BOTH sides. This is arm-order
    /// independent.
    ///
    /// Discriminating: against the tree where the duplicate-member branch leaves
    /// `existing_property.visibility` untouched (keeping the LEFT side's
    /// visibility), `(Public-left, Private-right)` merges to Public and the
    /// arm-order-independence assertion FAILS.
    #[test]
    fn merge_registry_duplicate_member_takes_most_restrictive_visibility() {
        use verter_type_expr::MemberVisibility;

        let merge_dup = |left_vis, right_vis| {
            let left = TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![property_with_visibility("dup", left_vis)],
            }));
            let right = TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![property_with_visibility("dup", right_vis)],
            }));
            let merged = merge_component_meta_registry_candidates(Some(left), Some(right))
                .expect("object candidates merge");
            merged_property_visibility(&merged, "dup")
        };

        // Public in both -> Public.
        assert_eq!(
            merge_dup(MemberVisibility::Public, MemberVisibility::Public),
            MemberVisibility::Public,
        );
        // Any non-public side -> non-public (most restrictive), arm-order
        // independent.
        assert_eq!(
            merge_dup(MemberVisibility::Public, MemberVisibility::Private),
            MemberVisibility::Private,
            "public-left + private-right must merge to Private",
        );
        assert_eq!(
            merge_dup(MemberVisibility::Private, MemberVisibility::Public),
            MemberVisibility::Private,
            "private-left + public-right must merge to Private (arm-order independent)",
        );
        assert_eq!(
            merge_dup(MemberVisibility::Protected, MemberVisibility::Private),
            MemberVisibility::Private,
            "protected + private -> Private",
        );
        assert_eq!(
            merge_dup(MemberVisibility::Public, MemberVisibility::Protected),
            MemberVisibility::Protected,
        );
    }

    #[test]
    fn field_expr_actionable_route_recognizes_direct_generic_refs() {
        let expr = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
            "GetModelValue<T, VK, true>",
            None,
        );

        assert!(
            component_meta_registry_field_expr_has_actionable_route(&expr),
            "direct public generic refs should reuse the existing symbolic field expr instead of reparsing raw_type",
        );
    }

    #[test]
    fn field_expr_actionable_route_rejects_expanded_object_surfaces() {
        let expr = object_with_props(&["base", "label"]);

        assert!(
            !component_meta_registry_field_expr_has_actionable_route(&expr),
            "expanded object surfaces still need raw_type reparsing when the raw annotation carries a routed helper like Button['ui']",
        );
    }

    #[test]
    fn owner_import_root_resolves_named_imports_through_barrels() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { AvatarProps } from './types'

export interface Props {
  avatar?: AvatarProps
}

defineProps<Props>()
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from("export * from './Alert.vue'\nexport * from './Avatar.vue'\n"),
        );
        ws.inject_file(
            "/src/Alert.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AlertProps {
  title?: string
}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/types.ts",
            vec![
                DependencyResolution {
                    specifier: "./Alert.vue".to_string(),
                    resolved_canonical_id: Some("/src/Alert.vue".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                DependencyResolution {
                    specifier: "./Avatar.vue".to_string(),
                    resolved_canonical_id: Some("/src/Avatar.vue".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let snapshot = host
            .get_raw_analysis_snapshot("/src/App.vue")
            .expect("app snapshot should exist");

        let resolved = owner_component_meta_registry_import_root(
            &host,
            "/src/App.vue",
            &snapshot,
            "AvatarProps",
        );

        assert_eq!(
            resolved,
            Some(("/src/Avatar.vue".to_string(), "AvatarProps".to_string())),
            "registry import roots should collapse direct named owner imports to the canonical defining file instead of keeping the barrel canonical",
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_prefers_more_specific_shapes() {
        let resolved_body = Some(TypeExpr::named("Props"));
        let decl_body = Some(object_with_props(&["label", "count"]));

        let chosen = choose_preferred_imported_type_body(resolved_body, decl_body.clone());

        assert_eq!(
            chosen, decl_body,
            "the body with the richer concrete surface should win"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_keeps_existing_body_on_equal_specificity() {
        let left = object_with_props(&["label"]);
        let right = object_with_props(&["count"]);

        let chosen = choose_preferred_imported_type_body(Some(left.clone()), Some(right));

        assert_eq!(
            chosen,
            Some(left),
            "equal scores should preserve the first successful resolution"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_rejects_empty_object_placeholders() {
        let resolved_body = Some(empty_object());
        let decl_body = Some(TypeExpr::union(vec![
            TypeExpr::Literal(LiteralValue::String("to".to_string())),
            TypeExpr::Literal(LiteralValue::String("replace".to_string())),
        ]));

        let chosen = choose_preferred_imported_type_body(resolved_body, decl_body.clone());

        assert_eq!(
            chosen, decl_body,
            "empty-object placeholders must not outrank concrete literal-union aliases"
        );
    }

    #[test]
    fn imported_type_body_specificity_prefers_object_surfaces_over_refs_and_typeof() {
        let typeof_score = imported_type_body_specificity_score(&TypeExpr::TypeOf(ValueRef {
            path: vec!["theme".to_string()],
        }));
        let ref_score = imported_type_body_specificity_score(&TypeExpr::named("Props"));
        let object_score = imported_type_body_specificity_score(&object_with_props(&["label"]));

        assert!(
            typeof_score < ref_score && ref_score < object_score,
            "specificity ordering should keep typeof < ref < object, got typeof={typeof_score} ref={ref_score} object={object_score}"
        );
    }

    #[test]
    fn imported_type_body_specificity_rewards_richer_object_surfaces() {
        let small = imported_type_body_specificity_score(&object_with_props(&["label"]));
        let large = imported_type_body_specificity_score(&object_with_props(&["label", "count"]));

        assert!(
            large > small,
            "object surfaces with more top-level members should score higher, got small={small} large={large}"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_prefers_richer_object_surface_with_nested_members() {
        let resolved_body = Some(object_with_props(&["next"]));
        let decl_body = Some(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "base".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    true,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "current".to_string(),
                    TypeExpr::named("T"),
                    true,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "next".to_string(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    true,
                    false,
                )),
            ],
        })));

        let chosen = choose_preferred_imported_type_body(resolved_body, decl_body.clone());

        assert_eq!(
            chosen, decl_body,
            "a richer concrete object surface should beat a smaller local-eval object even when one member type stays symbolic"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_keeps_meaningful_top_level_union_surface() {
        let flattened_object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "path".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            ))],
        }));
        let symbolic_union = TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::named("St"),
            TypeExpr::named("vt"),
        ]);

        let preferred = choose_preferred_imported_type_body(
            Some(flattened_object.clone()),
            Some(symbolic_union.clone()),
        )
        .expect("preferred body should exist");

        assert_eq!(preferred, symbolic_union);
        assert_ne!(preferred, flattened_object);
    }

    #[test]
    fn choose_preferred_imported_type_body_prefers_method_signatures_over_function_properties() {
        let function = FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("props".to_string()),
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "ui".to_string(),
                        TypeExpr::Primitive(PrimitiveName::String),
                        false,
                        false,
                    ))],
                })),
                false,
                false,
            )],
            Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
            vec![],
        );
        let property_object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "default".to_string(),
                TypeExpr::Function(Arc::new(function.clone())),
                true,
                false,
            ))],
        }));
        let method_object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Method(MethodSignature::synthetic_public(
                "default".to_string(),
                function,
                true,
            ))],
        }));

        let preferred =
            choose_preferred_imported_type_body(Some(property_object), Some(method_object.clone()))
                .expect("preferred body should exist");

        assert_eq!(preferred, method_object);
    }

    #[test]
    fn raw_member_path_surface_projects_explicit_object_members_without_widening() {
        let raw = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "ui".to_string(),
                    TypeExpr::Object(Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                            "base".to_string(),
                            TypeExpr::Primitive(PrimitiveName::String),
                            true,
                            false,
                        ))],
                    })),
                    true,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "label".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    true,
                    false,
                )),
            ],
        }));

        let projected = component_meta_registry_raw_member_path_surface(&raw, &["ui".to_string()])
            .expect("explicit object surface should project the requested member path");

        let TypeExpr::Object(shape) = &projected else {
            panic!("member path projection should stay object-shaped, got {projected:?}");
        };
        let member_names: Vec<_> = shape
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            vec!["ui"],
            "raw member path projection should stay on the requested member instead of widening to siblings"
        );
    }

    // -----------------------------------------------------------------
    // G1 — Conditional / Mapped walker gates on
    // `cursor.is_whole_surface()`. Under a narrowed cursor the
    // type-level predicate sides (Conditional.check/extends,
    // Mapped.source/name_type) must NOT contribute refs.
    // -----------------------------------------------------------------

    /// Helper: build an explicit `Include` cursor at the root, with
    /// `Registry` surface kind. The cursor is narrowed → not
    /// whole-surface.
    fn narrowed_include_projection(
        keys: &[&str],
    ) -> crate::meta_resolve::projection_demand::SurfaceProjection {
        let mut node =
            crate::meta_resolve::projection_demand::ProjectionNode::whole_surface_expanded();
        node.key_filter = crate::meta_resolve::projection_demand::KeyFilter::Include(
            keys.iter()
                .map(|k| Arc::<str>::from(*k))
                .collect::<Vec<_>>()
                .into(),
        );
        crate::meta_resolve::projection_demand::SurfaceProjection {
            surface: crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
            root: node,
        }
    }

    fn ref_named(name: &str) -> TypeExpr {
        TypeExpr::Ref {
            name: name.to_string().into(),
            type_arguments: Arc::from([]),
        }
    }

    fn drain_names(output: &VecDeque<super::PendingComponentMetaRegistryRef>) -> Vec<String> {
        output.iter().map(|p| p.name.clone()).collect()
    }

    #[test]
    fn g1_conditional_under_narrowed_cursor_skips_check_extends() {
        // Conditional { check: CheckRef, extends: ExtendsRef,
        //              true_type: TrueRef, false_type: FalseRef }
        // Narrowed cursor (Include("a")) — predicate sides MUST NOT
        // be walked; result branches MUST be walked.
        let expr = TypeExpr::Conditional {
            check: Arc::new(ref_named("CheckRef")),
            extends: Arc::new(ref_named("ExtendsRef")),
            true_type: Arc::new(ref_named("TrueRef")),
            false_type: Arc::new(ref_named("FalseRef")),
        };
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = narrowed_include_projection(&["a"]);

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true, // allow_plain_member_refs — Conditional gate runs after this
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            !names.iter().any(|n| n == "CheckRef"),
            "G1: narrowed cursor must NOT walk Conditional.check (was: {names:?})"
        );
        assert!(
            !names.iter().any(|n| n == "ExtendsRef"),
            "G1: narrowed cursor must NOT walk Conditional.extends (was: {names:?})"
        );
        assert!(
            names.iter().any(|n| n == "TrueRef"),
            "G1: narrowed cursor MUST walk Conditional.true_type (open: distribute remaining demand) (was: {names:?})"
        );
        assert!(
            names.iter().any(|n| n == "FalseRef"),
            "G1: narrowed cursor MUST walk Conditional.false_type (open: distribute remaining demand) (was: {names:?})"
        );
    }

    #[test]
    fn g1_conditional_under_whole_surface_walks_all_four() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(ref_named("CheckRef")),
            extends: Arc::new(ref_named("ExtendsRef")),
            true_type: Arc::new(ref_named("TrueRef")),
            false_type: Arc::new(ref_named("FalseRef")),
        };
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
        );

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        for n in ["CheckRef", "ExtendsRef", "TrueRef", "FalseRef"] {
            assert!(
                names.iter().any(|name| name == n),
                "G1: whole-surface MUST walk every Conditional branch including {n} (was: {names:?})"
            );
        }
    }

    #[test]
    fn g1_mapped_under_narrowed_cursor_skips_source_and_name_type() {
        // Mapped { source: SourceRef, value: ValueRef, name_type: Some(NameTypeRef) }
        // Narrowed cursor — source/name_type are type-level, must
        // not be walked. Value MUST be walked.
        let expr = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(ref_named("SourceRef")),
            value: Arc::new(ref_named("ValueRef")),
            name_type: Some(Arc::new(ref_named("NameTypeRef"))),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
        };
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = narrowed_include_projection(&["a"]);

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            !names.iter().any(|n| n == "SourceRef"),
            "G1: narrowed cursor must NOT walk Mapped.source (was: {names:?})"
        );
        assert!(
            !names.iter().any(|n| n == "NameTypeRef"),
            "G1: narrowed cursor must NOT walk Mapped.name_type (was: {names:?})"
        );
        assert!(
            names.iter().any(|n| n == "ValueRef"),
            "G1: narrowed cursor MUST walk Mapped.value (the produced value type) (was: {names:?})"
        );
    }

    #[test]
    fn g1_mapped_under_whole_surface_walks_source_value_name_type() {
        let expr = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(ref_named("SourceRef")),
            value: Arc::new(ref_named("ValueRef")),
            name_type: Some(Arc::new(ref_named("NameTypeRef"))),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
        };
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
        );

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        for n in ["SourceRef", "ValueRef", "NameTypeRef"] {
            assert!(
                names.iter().any(|name| name == n),
                "G1: whole-surface MUST walk every Mapped branch including {n} (was: {names:?})"
            );
        }
    }

    // -----------------------------------------------------------------
    // G2 — non-Property ObjectMember arms (Method,
    // IndexSignature, CallSignature, ConstructSignature) apply the
    // path-precision gate.
    // -----------------------------------------------------------------

    fn object_with_member(member: ObjectMember) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![member],
        }))
    }

    fn fn_returning(name: &str) -> FunctionExpr {
        // Wrap the named ref in an IndexedAccess so the registry's
        // routed-helper path enqueues the root name. A bare `Ref`
        // would be filtered by `allow_plain_refs=false` in
        // `collect_component_meta_registry_function_surface_refs`.
        FunctionExpr::synthetic(
            Vec::new(),
            Some(Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(ref_named(name)),
                index: Arc::new(TypeExpr::Literal(LiteralValue::String("x".to_string()))),
            })),
            Vec::new(),
        )
    }

    #[test]
    fn g2_method_under_narrowed_cursor_gates_on_admits_key() {
        // Pick<Foo, "methodA"> equivalent — Include("methodA"). The
        // Method arm must skip `methodB` and walk `methodA`'s nested
        // refs.
        let method_a = ObjectMember::Method(MethodSignature::synthetic_public(
            "methodA".to_string(),
            fn_returning("MethodAReturnRef"),
            false,
        ));
        let method_b = ObjectMember::Method(MethodSignature::synthetic_public(
            "methodB".to_string(),
            fn_returning("MethodBReturnRef"),
            false,
        ));
        let expr = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![method_a, method_b],
        }));
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = narrowed_include_projection(&["methodA"]);

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            names.iter().any(|n| n == "MethodAReturnRef"),
            "G2: narrowed cursor MUST walk admitted Method's nested refs (was: {names:?})"
        );
        assert!(
            !names.iter().any(|n| n == "MethodBReturnRef"),
            "G2: narrowed cursor MUST NOT walk rejected Method's nested refs (was: {names:?})"
        );
    }

    #[test]
    fn g2_index_signature_skipped_under_narrowed_cursor() {
        let sig = ObjectMember::IndexSignature(verter_type_expr::IndexSignature::synthetic(
            "key".to_string(),
            ref_named("IndexKeyRef"),
            ref_named("IndexValueRef"),
            false,
        ));
        let expr = object_with_member(sig);
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = narrowed_include_projection(&["a"]);

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            !names.iter().any(|n| n == "IndexKeyRef"),
            "G2: narrowed cursor must NOT walk IndexSignature.key_type (was: {names:?})"
        );
        assert!(
            !names.iter().any(|n| n == "IndexValueRef"),
            "G2: narrowed cursor must NOT walk IndexSignature.value_type (was: {names:?})"
        );
    }

    #[test]
    fn g2_index_signature_walked_under_whole_surface() {
        let sig = ObjectMember::IndexSignature(verter_type_expr::IndexSignature::synthetic(
            "key".to_string(),
            ref_named("IndexKeyRef"),
            ref_named("IndexValueRef"),
            false,
        ));
        let expr = object_with_member(sig);
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
        );

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            names.iter().any(|n| n == "IndexKeyRef"),
            "G2: whole-surface MUST walk IndexSignature.key_type (was: {names:?})"
        );
        assert!(
            names.iter().any(|n| n == "IndexValueRef"),
            "G2: whole-surface MUST walk IndexSignature.value_type (was: {names:?})"
        );
    }

    #[test]
    fn g2_call_signature_skipped_under_narrowed_cursor() {
        let call = ObjectMember::CallSignature(fn_returning("CallReturnRef"));
        let expr = object_with_member(call);
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = narrowed_include_projection(&["a"]);

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            !names.iter().any(|n| n == "CallReturnRef"),
            "G2: narrowed cursor must NOT walk CallSignature's nested refs (was: {names:?})"
        );
    }

    #[test]
    fn g2_call_signature_walked_under_whole_surface() {
        let call = ObjectMember::CallSignature(fn_returning("CallReturnRef"));
        let expr = object_with_member(call);
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();
        let proj = crate::meta_resolve::projection_demand::SurfaceProjection::whole_surface(
            crate::meta_resolve::projection_demand::PublishedSurfaceKind::Registry,
        );

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            None,
            true,
            proj.cursor(),
        );

        let names = drain_names(&output);
        assert!(
            names.iter().any(|n| n == "CallReturnRef"),
            "G2: whole-surface MUST walk CallSignature's nested refs (was: {names:?})"
        );
    }
}
