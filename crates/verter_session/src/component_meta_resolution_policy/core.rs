//! Core walker types and rewrite helpers.
//!
//! Hosts the `PolicyRegistry` / `PolicyCtx` / `DeclLookup` types that
//! coordinate the policy walk and the structural-recursion `rewrite_*`
//! helpers consumed by the entrypoint in `mod.rs`.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_semantic::analysis::type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
    ObjectProperty, TupleElement, TypeExpr,
};

use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::ComponentMetaQueryEngine;
use crate::semantic_query::DeclIdentity;
use crate::VerterHost;

use super::cycle_guard::NormalizedTypeArgs;
use super::pick_omit::rewrite_pick_or_omit_for_package_backed;

/// Build O(1)-lookup tables over the resolved type registry / meta.
/// `resolved_type_registry` and `resolved_type_registry_meta` are aligned by
/// `name` — the meta entry for `name` lives in the parallel meta vec.
pub(super) struct PolicyRegistry<'a> {
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
    pub(super) fn build(
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

pub(super) struct PolicyCtx<'a, 'h> {
    pub(super) registry: &'a PolicyRegistry<'a>,
    pub(super) engine: &'a mut ComponentMetaQueryEngine<'h>,
    pub(super) owner_canonical: &'a str,
    pub(super) host: &'a VerterHost,
    /// Cycle-guard active set keyed on `(DeclIdentity, NormalizedTypeArgs)`
    /// per Invariant #20. Bare-name keying is forbidden because:
    /// * Two `Foo`s in different files would collide under name keying.
    /// * `Pick<X, 'a'>` and `Pick<X, 'b'>` would collide under name-only
    ///   keying — but they are distinct instantiations and must navigate
    ///   independently.
    /// * Anonymous-type cycles re-entering through identical structural
    ///   shapes need an identity that is stable across syntactic clones.
    pub(super) active_refs: FxHashSet<(DeclIdentity, NormalizedTypeArgs)>,
    /// Greatest observed depth of `active_refs` during the walk. Used
    /// to surface a `policy_active_refs_max_depth` counter under
    /// `CaptureToken` for cycle-guard fixtures.
    pub(super) active_refs_max_depth: u64,
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
    pub(super) fn locate_declaration(&mut self, name: &str) -> Option<DeclLookup> {
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

    /// Build a `DeclIdentity` for `(canonical_source, name)` by reading
    /// the file's `whole_hash` from the host's shallow file state. Files
    /// that have no shallow state (e.g. synthetic test fixtures, the
    /// owner SFC before parse, or registry-meta entries pointing at
    /// not-yet-loaded paths) fall back to `HashValue::default()` — the
    /// `(canonical_source, name)` pair is still a deterministic identity
    /// for the cycle guard within a single policy invocation.
    pub(super) fn decl_identity_for(&self, canonical_source: &str, name: &str) -> DeclIdentity {
        let whole_hash = self
            .host
            .shallow_file_state(canonical_source)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        DeclIdentity {
            canonical_id: Arc::from(canonical_source),
            whole_hash,
            decl_name: Arc::from(name),
        }
    }
}

pub(super) struct DeclLookup {
    pub(super) canonical_source: String,
    pub(super) body: TypeExpr,
}

/// Returns true if `expr` was mutated.
pub(super) fn rewrite_in_place(expr: &mut TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    if let Some(next) = rewrite_expr(expr, ctx) {
        *expr = next;
        true
    } else {
        false
    }
}

/// Walk `expr` and produce a rewritten clone if any rule fires; otherwise
/// `None`. Caller owns the in-place swap.
pub(super) fn rewrite_expr(expr: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> Option<TypeExpr> {
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

    // Selective `Pick<package_backed, K>` and symbolic
    // `Omit<package_backed, K>`: when the target declaration's
    // canonical source resolves under `/node_modules/`, the helper
    // module owns the materialisation result. Workspace-owned targets
    // fall through to the standard rewrite chain so the canonical
    // reuse path keeps ownership.
    if (name == "Pick" || name == "Omit") && type_arguments.len() == 2 {
        if let Some(rewritten) = rewrite_pick_or_omit_for_package_backed(name, type_arguments, ctx)
        {
            return Some(rewritten);
        }
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

    // Build the `(DeclIdentity, NormalizedTypeArgs)` cycle-guard key. The
    // identity is keyed on resolved declaration source, not the bare
    // name — two `Foo`s in different files produce different identities;
    // `Pick<X, 'a'>` and `Pick<X, 'b'>` produce different normalized
    // type-args so they navigate independently.
    let decl_identity = ctx.decl_identity_for(&canonical_source, name);
    let normalized_args = NormalizedTypeArgs::normalize(type_arguments, ctx);
    let guard_key = (decl_identity, normalized_args);

    // Re-entry on the same key surfaces a `RecursiveRef`: the prior
    // invocation is still resolving this declaration, and continuing
    // would recurse forever. The recursive back-edge preserves the
    // declaration name plus its instantiation arguments so consumers
    // can render the recursion target (e.g. `Tree[]` whose element
    // is a back-edge to `Tree` publishes `RecursiveRef("Tree", [])`).
    // Generic substitutions are part of identity — `Foo<A>` and
    // `Foo<B>` are distinct guard keys and only collapse to a back
    // edge when the *same* substitution recurs.
    if ctx.active_refs.contains(&guard_key) {
        return Some(TypeExpr::recursive_ref(name, type_arguments.to_vec()));
    }

    ctx.active_refs.insert(guard_key.clone());
    if (ctx.active_refs.len() as u64) > ctx.active_refs_max_depth {
        ctx.active_refs_max_depth = ctx.active_refs.len() as u64;
        crate::capture_token::with_active_capture(|t| {
            t.record_counter("policy_active_refs_max_depth", 1)
        });
    }

    let result = rewrite_ref_body_with_guard(&body, name, type_arguments, ctx);

    ctx.active_refs.remove(&guard_key);
    result
}

/// Body-chase logic invoked under the active-refs cycle guard. Consumes
/// the resolved declaration body and either returns the rewritten shape
/// (Rule 3 / project-local non-Props) or falls through to type-argument
/// recursion (Rule 5).
fn rewrite_ref_body_with_guard(
    body: &TypeExpr,
    name: &str,
    type_arguments: &[TypeExpr],
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<TypeExpr> {
    // Rule 3: project-local non-Props with empty type_arguments → chase to
    // body if the body is structurally resolvable (not just another Ref).
    if type_arguments.is_empty() && body_is_resolvable(body) {
        // The body itself may contain other Refs that need policy treatment
        // (e.g. an Object whose property is `Ref(OtherImported)`). Apply
        // the policy to the body before publishing.
        let cloned = match rewrite_expr(body, ctx) {
            Some(rewritten) => rewritten,
            None => body.clone(),
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

pub(super) fn is_props_suffix(name: &str) -> bool {
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

/// Strip leading `Parenthesized` wrappers; mirror the convention used
/// throughout this module.
pub(super) fn peel_paren(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Parenthesized(inner) => peel_paren(inner),
        _ => expr,
    }
}
