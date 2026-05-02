//! Component-meta publication policy (Phase 4B of architectural-debt-closure).
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

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::component_meta::{ComponentMetaAnalysis, ResolvedTypeAnalysis};
use verter_semantic::analysis::type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
    ObjectProperty, TupleElement, TypeExpr,
};

use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::ComponentMetaQueryEngine;
use crate::VerterHost;

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
    };

    let mut changed = false;

    for prop in analysis.props.iter_mut() {
        // Pre-step: restore *Props refs the evaluator may have eagerly
        // resolved away. The deleted `imported_props_like_public_raw_type`
        // helper used the raw type annotation as the canonical form for
        // *Props imports — re-instate that contract before the rule walk.
        if restore_props_suffix_from_raw(&mut prop.type_expr, prop.raw_type.as_deref(), &mut ctx) {
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
            if restore_props_suffix_from_raw(
                &mut binding.type_expr,
                binding.raw_type.as_deref(),
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
            accepted.raw_type.as_deref(),
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

/// If the raw type annotation contains imported *Props refs that the
/// evaluator eagerly resolved into structural shapes (e.g. `ButtonProps[]`
/// became `Array<Object{href, disabled, label}>`), restore the symbolic
/// form by parsing the raw type and confirming the parsed shape matches.
///
/// **Only fires for COMPOUND raw types** — bare `Ref(*Props)` raw types
/// are left to the upstream `merge_evaluated_prop_types_into_meta` policy
/// (which already has the bare-Ref escape hatch at host_manage.rs ~8170).
/// Restoring bare Refs here would over-correct cases like `avatar:
/// AvatarProps` where the evaluator's substituted Object body is the
/// intended public shape (see
/// `resolve_component_meta_publishes_transitive_registry_aliases_for_nested_indexed_access_refs`).
///
/// Returns `true` if the type_expr was rewritten.
fn restore_props_suffix_from_raw(
    type_expr: &mut TypeExpr,
    raw_type: Option<&str>,
    ctx: &mut PolicyCtx<'_, '_>,
) -> bool {
    let Some(raw) = raw_type else {
        return false;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(trimmed);

    // Bare Props-suffix Refs stay deferred to the bare-Ref merge escape
    // hatch — see doc comment.
    if is_bare_props_suffix_ref(&parsed) {
        return false;
    }

    let mut props_refs: Vec<(Arc<str>, usize)> = Vec::new();
    collect_props_suffix_refs(&parsed, &mut props_refs);
    if props_refs.is_empty() {
        return false;
    }

    // Confirm every collected *Props ref in the raw type belongs to an
    // imported declaration (project-local OR package-backed). If any ref
    // resolves to the owner itself, we don't substitute — the eager
    // resolution there is correct.
    for (name, _) in props_refs.iter() {
        let lookup = ctx.locate_declaration(name.as_ref());
        let imported = lookup
            .as_ref()
            .map(|d| d.canonical_source != ctx.owner_canonical)
            .unwrap_or(false);
        if !imported {
            return false;
        }
    }

    // If the resolved type_expr already contains all of the *Props refs,
    // nothing to restore — the evaluator preserved the symbolic form.
    let all_present = props_refs
        .iter()
        .all(|(name, arity)| expr_contains_ref(type_expr, name.as_ref(), *arity));
    if all_present {
        return false;
    }

    *type_expr = parsed;
    true
}

/// `Ref { name: "*Props" }` directly, optionally wrapped in `Parenthesized`.
fn is_bare_props_suffix_ref(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => is_bare_props_suffix_ref(inner),
        TypeExpr::Ref { name, .. } => is_props_suffix(name.as_ref()),
        _ => false,
    }
}

/// Collect every `Ref { name, type_arguments }` pair where `name` ends in
/// `"Props"`. Tracks both name and type-argument arity to disambiguate
/// generic vs. non-generic forms.
fn collect_props_suffix_refs(expr: &TypeExpr, out: &mut Vec<(Arc<str>, usize)>) {
    match expr {
        TypeExpr::Parenthesized(inner) => collect_props_suffix_refs(inner, out),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if is_props_suffix(name.as_ref()) {
                let entry = (name.clone(), type_arguments.len());
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            for arg in type_arguments.iter() {
                collect_props_suffix_refs(arg, out);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_props_suffix_refs(ty, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_props_suffix_refs(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_props_suffix_refs(&element.ty, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_props_suffix_refs(object, out);
            collect_props_suffix_refs(index, out);
        }
        _ => {}
    }
}

/// Whether `expr` contains a `Ref { name, type_arguments }` where
/// `name == target` AND `type_arguments.len() == arity`.
fn expr_contains_ref(expr: &TypeExpr, target: &str, arity: usize) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => expr_contains_ref(inner, target, arity),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            (name.as_ref() == target && type_arguments.len() == arity)
                || type_arguments
                    .iter()
                    .any(|arg| expr_contains_ref(arg, target, arity))
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(|ty| expr_contains_ref(ty, target, arity))
        }
        TypeExpr::Array { element, .. } => expr_contains_ref(element, target, arity),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| expr_contains_ref(&element.ty, target, arity)),
        TypeExpr::IndexedAccess { object, index } => {
            expr_contains_ref(object, target, arity) || expr_contains_ref(index, target, arity)
        }
        _ => false,
    }
}

/// Build O(1)-lookup tables over the resolved type registry / meta.
/// `resolved_type_registry` and `resolved_type_registry_meta` are aligned by
/// `name` — the meta entry for `name` lives in the parallel meta vec.
struct PolicyRegistry<'a> {
    /// `name → &TypeExpr` of the resolved registry entry.
    type_by_name: FxHashMap<&'a str, &'a TypeExpr>,
    /// `name → canonical_source` of the declaration.
    canonical_source_by_name: FxHashMap<&'a str, &'a str>,
    /// Distinct, non-empty declaration scopes drawn from the registry meta.
    /// Used as fallback scopes for cross-file declaration lookup of
    /// transitively-referenced types (e.g. types referenced from a macro
    /// argument's declaration but not registered as a top-level macro arg).
    fallback_scopes: Vec<&'a str>,
}

impl<'a> PolicyRegistry<'a> {
    fn build(
        type_registry: &'a [ResolvedTypeAnalysis],
        type_registry_meta: &'a [ResolvedTypeRegistryMeta],
    ) -> Self {
        let mut type_by_name = FxHashMap::default();
        for entry in type_registry.iter() {
            type_by_name.insert(entry.name.as_str(), &entry.type_expr);
        }
        let mut canonical_source_by_name = FxHashMap::default();
        let mut fallback_scopes: Vec<&'a str> = Vec::new();
        for meta in type_registry_meta.iter() {
            let canonical = meta.declaration.canonical_source.as_str();
            canonical_source_by_name.insert(meta.name.as_str(), canonical);
            if !canonical.is_empty() && !fallback_scopes.contains(&canonical) {
                fallback_scopes.push(canonical);
            }
        }
        Self {
            type_by_name,
            canonical_source_by_name,
            fallback_scopes,
        }
    }

    fn registry_body(&self, name: &str) -> Option<&'a TypeExpr> {
        self.type_by_name.get(name).copied()
    }

    fn canonical_source(&self, name: &str) -> Option<&'a str> {
        self.canonical_source_by_name
            .get(name)
            .copied()
            .filter(|s| !s.is_empty())
    }
}

struct PolicyCtx<'a, 'h> {
    registry: &'a PolicyRegistry<'a>,
    engine: &'a mut ComponentMetaQueryEngine<'h>,
    owner_canonical: &'a str,
}

impl<'a, 'h> PolicyCtx<'a, 'h> {
    /// Locate `name`'s declaration body. The body lookup itself is the
    /// authoritative signal for "this declaration exists" — registry-meta
    /// `canonical_source` only tells us the macro arg's home, not whether a
    /// transitively-referenced type is reachable.
    ///
    /// Returns `(canonical_source, resolved_name, body)`. The first scope
    /// that produces a body wins; the resolver scopes are the owner first
    /// then registry-meta declaration sources. This mirrors the multi-scope
    /// iteration the deleted `rematerialize` helper performed: `Status` is
    /// referenced from `ExternalProps.status` whose declaration lives in
    /// `/types.ts`, so resolving `Status` in `/App.vue` returns nothing but
    /// resolving in `/types.ts` succeeds.
    fn locate_declaration(&mut self, name: &str) -> Option<DeclLookup> {
        // Registry first — pre-resolved data, zero engine work.
        if let Some(body) = self.registry.registry_body(name) {
            let canonical = self
                .registry
                .canonical_source(name)
                .unwrap_or(self.owner_canonical)
                .to_string();
            return Some(DeclLookup {
                canonical_source: canonical,
                body: body.clone(),
            });
        }
        let mut scopes: Vec<String> = vec![self.owner_canonical.to_string()];
        for fallback in self.registry.fallback_scopes.iter() {
            if !scopes.iter().any(|existing| existing.as_str() == *fallback) {
                scopes.push(fallback.to_string());
            }
        }
        for scope in scopes {
            let decl = self.engine.resolve_type_declaration(&scope, name);
            if decl.canonical_source.is_empty() {
                continue;
            }
            let resolved_name = if decl.resolved_name.is_empty() {
                name.to_string()
            } else {
                decl.resolved_name.clone()
            };
            if let Some(body) = self
                .engine
                .named_decl_body(&decl.canonical_source, &resolved_name)
            {
                return Some(DeclLookup {
                    canonical_source: decl.canonical_source,
                    body,
                });
            }
        }
        None
    }
}

struct DeclLookup {
    canonical_source: String,
    body: TypeExpr,
}

/// Returns true if `expr` was mutated.
fn rewrite_in_place(expr: &mut TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    if let Some(next) = rewrite_expr(expr, ctx) {
        *expr = next;
        true
    } else {
        false
    }
}

/// Walk `expr` and produce a rewritten clone if any rule fires; otherwise
/// `None`. Caller owns the in-place swap.
fn rewrite_expr(expr: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> Option<TypeExpr> {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => rewrite_ref(name.as_ref(), type_arguments.as_ref(), ctx),

        TypeExpr::IndexedAccess { object, index } => {
            // Rule 2: member-path-on-Props stays symbolic.
            if indexed_access_targets_props_suffix(object) {
                return None;
            }
            // Rule 5: recurse into both arms.
            let new_obj = rewrite_expr(object, ctx);
            let new_idx = rewrite_expr(index, ctx);
            if new_obj.is_none() && new_idx.is_none() {
                return None;
            }
            Some(TypeExpr::IndexedAccess {
                object: new_obj.map(Arc::new).unwrap_or_else(|| object.clone()),
                index: new_idx.map(Arc::new).unwrap_or_else(|| index.clone()),
            })
        }

        // Rule 5: structural recursion.
        TypeExpr::Array { element, readonly } => {
            let new_element = rewrite_expr(element, ctx)?;
            Some(TypeExpr::Array {
                element: Arc::new(new_element),
                readonly: *readonly,
            })
        }
        TypeExpr::Tuple { elements, readonly } => {
            let mut next: Option<Vec<TupleElement>> = None;
            for (idx, element) in elements.iter().enumerate() {
                if let Some(rewritten) = rewrite_expr(&element.ty, ctx) {
                    let cloned = next.get_or_insert_with(|| elements.iter().cloned().collect());
                    cloned[idx].ty = rewritten;
                }
            }
            next.map(|elements| TypeExpr::Tuple {
                elements: Arc::from(elements),
                readonly: *readonly,
            })
        }
        TypeExpr::Union(types) => rewrite_homogeneous(types, ctx).map(TypeExpr::Union),
        TypeExpr::Intersection(types) => {
            rewrite_homogeneous(types, ctx).map(TypeExpr::Intersection)
        }
        TypeExpr::Object(obj) => {
            rewrite_object(obj, ctx).map(|next| TypeExpr::Object(Arc::new(next)))
        }
        TypeExpr::Function(func) => {
            rewrite_function(func, ctx).map(|next| TypeExpr::Function(Arc::new(next)))
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let new_check = rewrite_expr(check, ctx);
            let new_extends = rewrite_expr(extends, ctx);
            let new_true = rewrite_expr(true_type, ctx);
            let new_false = rewrite_expr(false_type, ctx);
            if new_check.is_none()
                && new_extends.is_none()
                && new_true.is_none()
                && new_false.is_none()
            {
                return None;
            }
            Some(TypeExpr::Conditional {
                check: new_check.map(Arc::new).unwrap_or_else(|| check.clone()),
                extends: new_extends.map(Arc::new).unwrap_or_else(|| extends.clone()),
                true_type: new_true.map(Arc::new).unwrap_or_else(|| true_type.clone()),
                false_type: new_false
                    .map(Arc::new)
                    .unwrap_or_else(|| false_type.clone()),
            })
        }
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let new_source = rewrite_expr(source, ctx);
            let new_value = rewrite_expr(value, ctx);
            let new_name_type = name_type.as_ref().and_then(|n| rewrite_expr(n, ctx));
            if new_source.is_none() && new_value.is_none() && new_name_type.is_none() {
                return None;
            }
            Some(TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: new_source.map(Arc::new).unwrap_or_else(|| source.clone()),
                value: new_value.map(Arc::new).unwrap_or_else(|| value.clone()),
                optional: *optional,
                readonly: *readonly,
                name_type: match (new_name_type, name_type) {
                    (Some(rewritten), _) => Some(Arc::new(rewritten)),
                    (None, original) => original.clone(),
                },
            })
        }
        TypeExpr::KeyOf(inner) => {
            let new_inner = rewrite_expr(inner, ctx)?;
            Some(TypeExpr::KeyOf(Arc::new(new_inner)))
        }
        TypeExpr::Rest(inner) => {
            let new_inner = rewrite_expr(inner, ctx)?;
            Some(TypeExpr::Rest(Arc::new(new_inner)))
        }
        TypeExpr::Parenthesized(inner) => {
            let new_inner = rewrite_expr(inner, ctx)?;
            Some(TypeExpr::Parenthesized(Arc::new(new_inner)))
        }

        // Terminals — no rewrite possible.
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Unknown { .. } => None,
    }
}

/// Rewrite a `TypeExpr::Ref { name, type_arguments }` per rules 1, 3, 4, 5.
fn rewrite_ref(
    name: &str,
    type_arguments: &[TypeExpr],
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<TypeExpr> {
    // Rule 4: Props-suffix bare alias / generic stays symbolic — name-only
    // check, no declaration lookup needed.
    if is_props_suffix(name) {
        return rewrite_type_arguments(name, type_arguments, ctx, /*recurse*/ false);
    }

    // For non-Props refs, check whether the declaration is reachable. If
    // not, leave the Ref alone (Rule 5 only recurses into type_arguments).
    let lookup = ctx.locate_declaration(name);
    let Some(DeclLookup {
        canonical_source,
        body,
    }) = lookup
    else {
        return rewrite_type_arguments(name, type_arguments, ctx, /*recurse*/ true);
    };

    // Rule 1: package-backed Refs stay symbolic.
    if canonical_source.contains("/node_modules/") {
        return rewrite_type_arguments(name, type_arguments, ctx, /*recurse*/ false);
    }

    // Rule 3: project-local non-Props with empty type_arguments → chase to
    // body if the body is structurally resolvable (not just another Ref).
    if type_arguments.is_empty() && body_is_resolvable(&body) {
        // The body itself may contain other Refs that need policy treatment
        // (e.g. an Object whose property is `Ref(OtherImported)`). Apply
        // the policy to the body before publishing.
        let cloned = match rewrite_expr(&body, ctx) {
            Some(rewritten) => rewritten,
            None => body,
        };
        return Some(cloned);
    }

    // Rule 5: recurse into type_arguments only.
    rewrite_type_arguments(name, type_arguments, ctx, /*recurse*/ true)
}

/// If `recurse` is true, rewrite each type argument and rebuild the Ref when
/// any change occurred. If false, return None (Ref kept as-is).
fn rewrite_type_arguments(
    name: &str,
    type_arguments: &[TypeExpr],
    ctx: &mut PolicyCtx<'_, '_>,
    recurse: bool,
) -> Option<TypeExpr> {
    if !recurse || type_arguments.is_empty() {
        return None;
    }
    let mut next: Option<Vec<TypeExpr>> = None;
    for (idx, arg) in type_arguments.iter().enumerate() {
        if let Some(rewritten) = rewrite_expr(arg, ctx) {
            let cloned = next.get_or_insert_with(|| type_arguments.to_vec());
            cloned[idx] = rewritten;
        }
    }
    next.map(|args| TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(args),
    })
}

/// Apply rewrite to each element of `types`. Returns `Some(new arc-slice)` if
/// any element rewrote, else `None`.
fn rewrite_homogeneous(
    types: &Arc<[TypeExpr]>,
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<Arc<[TypeExpr]>> {
    let mut next: Option<Vec<TypeExpr>> = None;
    for (idx, ty) in types.iter().enumerate() {
        if let Some(rewritten) = rewrite_expr(ty, ctx) {
            let cloned = next.get_or_insert_with(|| types.to_vec());
            cloned[idx] = rewritten;
        }
    }
    next.map(Arc::from)
}

/// Apply rewrite to each member's type. Returns `Some(new ObjectExpr)` if any
/// changed, else `None`.
fn rewrite_object(obj: &Arc<ObjectExpr>, ctx: &mut PolicyCtx<'_, '_>) -> Option<ObjectExpr> {
    let mut next: Option<Vec<ObjectMember>> = None;
    for (idx, member) in obj.properties.iter().enumerate() {
        if let Some(rewritten) = rewrite_object_member(member, ctx) {
            let cloned = next.get_or_insert_with(|| obj.properties.clone());
            cloned[idx] = rewritten;
        }
    }
    next.map(|properties| ObjectExpr { properties })
}

fn rewrite_object_member(
    member: &ObjectMember,
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<ObjectMember> {
    match member {
        ObjectMember::Property(prop) => {
            let new_ty = rewrite_expr(&prop.ty, ctx)?;
            Some(ObjectMember::Property(ObjectProperty {
                name: prop.name.clone(),
                ty: new_ty,
                optional: prop.optional,
                readonly: prop.readonly,
            }))
        }
        ObjectMember::IndexSignature(sig) => {
            let new_key = rewrite_expr(&sig.key_type, ctx);
            let new_value = rewrite_expr(&sig.value_type, ctx);
            if new_key.is_none() && new_value.is_none() {
                return None;
            }
            Some(ObjectMember::IndexSignature(IndexSignature {
                key_name: sig.key_name.clone(),
                key_type: new_key.unwrap_or_else(|| sig.key_type.clone()),
                value_type: new_value.unwrap_or_else(|| sig.value_type.clone()),
                readonly: sig.readonly,
            }))
        }
        ObjectMember::CallSignature(func) => {
            rewrite_function(func, ctx).map(ObjectMember::CallSignature)
        }
        ObjectMember::ConstructSignature(func) => {
            rewrite_function(func, ctx).map(ObjectMember::ConstructSignature)
        }
        ObjectMember::Method(method) => {
            let new_func = rewrite_function(&method.function, ctx)?;
            Some(ObjectMember::Method(MethodSignature {
                name: method.name.clone(),
                function: new_func,
                optional: method.optional,
            }))
        }
    }
}

fn rewrite_function(func: &FunctionExpr, ctx: &mut PolicyCtx<'_, '_>) -> Option<FunctionExpr> {
    let mut next_params: Option<Vec<FunctionParam>> = None;
    for (idx, param) in func.parameters.iter().enumerate() {
        if let Some(rewritten) = rewrite_expr(&param.ty, ctx) {
            let cloned = next_params.get_or_insert_with(|| func.parameters.clone());
            cloned[idx].ty = rewritten;
        }
    }
    let new_return = func
        .return_type
        .as_ref()
        .and_then(|rt| rewrite_expr(rt, ctx));
    if next_params.is_none() && new_return.is_none() {
        return None;
    }
    Some(FunctionExpr {
        parameters: next_params.unwrap_or_else(|| func.parameters.clone()),
        return_type: match (new_return, &func.return_type) {
            (Some(rewritten), _) => Some(Arc::new(rewritten)),
            (None, original) => original.clone(),
        },
        type_parameters: func.type_parameters.clone(),
    })
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

fn is_props_suffix(name: &str) -> bool {
    name.ends_with("Props")
}

/// Walk through `Parenthesized` / Union / Intersection wrappers; return true
/// if any leaf is a Ref-with-Props-suffix-name.
fn indexed_access_targets_props_suffix(object: &TypeExpr) -> bool {
    match object {
        TypeExpr::Parenthesized(inner) => indexed_access_targets_props_suffix(inner),
        TypeExpr::Ref { name, .. } => is_props_suffix(name.as_ref()),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(indexed_access_targets_props_suffix)
        }
        _ => false,
    }
}

/// A registry body is "resolvable" if it is a structural shape (Object /
/// Union / Intersection / Array / Tuple / Function / Primitive / Literal /
/// Conditional / Mapped / TemplateLiteral / KeyOf / etc.) — anything other
/// than a bare Ref (which would just chase to another symbolic).
///
/// Bodies that are themselves IndexedAccess on a *Props alias should NOT
/// resolve eagerly — they are kept symbolic because that is the registry
/// authoritative form.
fn body_is_resolvable(body: &TypeExpr) -> bool {
    match body {
        TypeExpr::Parenthesized(inner) => body_is_resolvable(inner),
        TypeExpr::Ref { .. } => false,
        TypeExpr::IndexedAccess { object, .. } => !indexed_access_targets_props_suffix(object),
        TypeExpr::Unknown { .. } | TypeExpr::Infer { .. } | TypeExpr::TypeParameter(_) => false,
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Object(_)
        | TypeExpr::Function(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Rest(_)
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

// ---------------------------------------------------------------------------
// Cycle-guard normalized type-argument identity
// ---------------------------------------------------------------------------
//
// Recursion guards inside the policy walker (e.g. `rewrite_ref`'s active-ref
// stack) must key on `(DeclId, NormalizedTypeArgs)` rather than bare name
// strings: bare names collide across scopes and cannot distinguish
// `Pick<X, 'a'>` from `Pick<X, 'b'>` inside a generic instantiation chain.
//
// The TYPE definitions land here so consumers can begin keying on them; the
// actual swap of the existing bare-name `active_refs` set into a
// `(DeclIdentity, NormalizedTypeArgs)` set is owned by the policy guard
// bundle that consumes this surface. `#[allow(dead_code)]` covers the gap
// between "type defined" and "type consumed" — these items have a known
// downstream consumer that lands in a sibling bundle.

use crate::semantic_query::DeclIdentity;
use smallvec::SmallVec;
use verter_semantic::analysis::type_expr::LiteralValue;

/// Hash of an opaque anonymous shape used to discriminate cycle-guard keys
/// across structurally-identical inline type expressions. Phases that need
/// to distinguish anonymous shapes attach a stable hash; identity equality
/// of the hash is what `NormalizedTypeArg` checks.
#[allow(dead_code)]
pub(crate) type ShapeHash = u64;

/// Hash of a `LiteralValue` used as a cheap discriminator for literal-arg
/// cycle-guard keys (e.g. `Pick<X, 'a'>` vs `Pick<X, 'b'>`).
#[allow(dead_code)]
pub(crate) type LiteralHash = u64;

/// One normalized type argument for the cycle-guard key. The four variants
/// cover every shape the cycle guard observes:
///
/// - `Decl(DeclIdentity)` — a named declaration reference (resolved by the
///   resolver). Two args resolve to the same `DeclIdentity` when their
///   bare-name lookups land on the same declaration.
/// - `Literal(LiteralHash)` — a literal value (`'a'`, `42`, `true`).
///   Different literal values produce different hashes, so `Pick<X, 'a'>`
///   and `Pick<X, 'b'>` produce different `NormalizedTypeArgs`.
/// - `AnonymousShape(ShapeHash)` — an inline anonymous shape that has no
///   declaration identity (e.g. `{ a: string }` passed inline as a type
///   argument). The hash carries enough structural information to
///   distinguish unrelated inline shapes.
/// - `None` — empty / missing argument slot. Reserved for ambient
///   reductions where the caller wants to discriminate between "no arg"
///   and "a real arg that happens to hash to zero".
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NormalizedTypeArg {
    Decl(DeclIdentity),
    Literal(LiteralHash),
    AnonymousShape(ShapeHash),
    None,
}

/// Normalized form of a type-argument list, used as the second component
/// of cycle-guard keys (the first is the resolved `DeclIdentity` of the
/// declaration being entered).
///
/// Normalization is deterministic: two argument lists that resolve to the
/// same declaration identities and literal values produce the same
/// `NormalizedTypeArgs` value, regardless of the syntactic shape of the
/// original `TypeExpr` arguments. The ordering of arguments IS preserved
/// (positional arguments) — different positions are different keys.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NormalizedTypeArgs(SmallVec<[NormalizedTypeArg; 4]>);

#[allow(dead_code)]
impl NormalizedTypeArgs {
    /// Build an empty `NormalizedTypeArgs`. Used as the cycle-guard key
    /// for a declaration entered with no type arguments.
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self(SmallVec::new())
    }

    /// Build a `NormalizedTypeArgs` from an iterator of normalized
    /// arguments. Caller is responsible for resolving each input
    /// `TypeExpr` to its `NormalizedTypeArg` (the resolver-aware
    /// constructor lives in the bundle that consumes this type).
    #[must_use]
    pub(crate) fn from_normalized<I>(args: I) -> Self
    where
        I: IntoIterator<Item = NormalizedTypeArg>,
    {
        Self(args.into_iter().collect())
    }

    /// Number of arguments (zero for `empty`).
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// True when no arguments are present.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterator over the normalized arguments in positional order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &NormalizedTypeArg> {
        self.0.iter()
    }
}

impl Default for NormalizedTypeArgs {
    fn default() -> Self {
        Self::empty()
    }
}

/// Stable hash of a `LiteralValue` for use inside `NormalizedTypeArg::Literal`.
/// Mirrors the manual `Hash` impl on `LiteralValue` — same input bytes
/// produce the same digest across builds.
#[allow(dead_code)]
#[must_use]
pub(crate) fn hash_literal(value: &LiteralValue) -> LiteralHash {
    use std::hash::Hash;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    value.hash(&mut bridge);
    hasher.digest()
}

#[allow(dead_code)]
struct LiteralHashBridge<'a>(&'a mut xxhash_rust::xxh3::Xxh3);

impl<'a> std::hash::Hasher for LiteralHashBridge<'a> {
    fn finish(&self) -> u64 {
        self.0.digest()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

#[cfg(test)]
#[path = "component_meta_resolution_policy_tests.rs"]
mod tests;
