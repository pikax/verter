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

use rustc_hash::{FxHashMap, FxHashSet};
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
            // Issue #1 (partial): when the binding's raw type is an
            // `IndexedAccess` whose deref chain transits through an
            // imported declaration, force the symbolic raw form back
            // onto the published `type_expr` and skip the expansion
            // walk. The eager evaluator may have widened the indexed
            // access through an open `[k: string]: any` index
            // signature; the consumer is better served by the
            // navigable `AppProps['avatar']` member-path contract.
            if slot_binding_should_preserve_symbolic_raw_type(binding.raw_type.as_deref(), &mut ctx)
            {
                if let Some(restored) = parse_indexed_access_from_raw(binding.raw_type.as_deref()) {
                    if binding.type_expr != restored {
                        binding.type_expr = restored;
                        changed = true;
                    }
                }
                continue;
            }
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
    host: &'a VerterHost,
    /// Cycle-guard active set keyed on `(DeclIdentity, NormalizedTypeArgs)`
    /// per Invariant #20. Bare-name keying is forbidden because:
    /// * Two `Foo`s in different files would collide under name keying.
    /// * `Pick<X, 'a'>` and `Pick<X, 'b'>` would collide under name-only
    ///   keying — but they are distinct instantiations and must navigate
    ///   independently.
    /// * Anonymous-type cycles re-entering through identical structural
    ///   shapes need an identity that is stable across syntactic clones.
    active_refs: FxHashSet<(DeclIdentity, NormalizedTypeArgs)>,
    /// Greatest observed depth of `active_refs` during the walk. Used
    /// to surface a `policy_active_refs_max_depth` counter under
    /// `CaptureToken` for cycle-guard fixtures.
    active_refs_max_depth: u64,
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

    /// Build a `DeclIdentity` for `(canonical_source, name)` by reading
    /// the file's `whole_hash` from the host's shallow file state. Files
    /// that have no shallow state (e.g. synthetic test fixtures, the
    /// owner SFC before parse, or registry-meta entries pointing at
    /// not-yet-loaded paths) fall back to `HashValue::default()` — the
    /// `(canonical_source, name)` pair is still a deterministic identity
    /// for the cycle guard within a single policy invocation.
    fn decl_identity_for(&self, canonical_source: &str, name: &str) -> DeclIdentity {
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

    // Re-entry on the same key bails with `semanticMiss`: the prior
    // invocation is still resolving this declaration, and continuing
    // would recurse forever. The published shape is `Unknown` so the
    // outer surface stays observable while the recursive arm is
    // transparently signalled as unresolved.
    if ctx.active_refs.contains(&guard_key) {
        return Some(TypeExpr::Unknown {
            raw: String::from("semanticMiss"),
        });
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

/// Selective `Pick` / symbolic `Omit` handling when the target
/// declaration's source resolves to a package-backed canonical id.
/// Returns:
/// - `Some(...)` when the target is package-backed AND the helper
///   produced a definitive result (Pick: object with picked members;
///   Omit: symbolic Ref preserved).
/// - `None` when the target is workspace-owned, the keys can't be
///   extracted, or the target body doesn't fit the helper's contract;
///   the caller falls through to the standard rewrite chain so the
///   canonical reuse path keeps ownership.
fn rewrite_pick_or_omit_for_package_backed(
    utility_name: &str,
    type_arguments: &[TypeExpr],
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<TypeExpr> {
    use crate::meta_resolve::materialize::utility_types::{
        selective_pick_expansion_for_package_backed, symbolic_omit_for_package_backed,
    };
    let target_arg = peel_paren(&type_arguments[0]);
    let keys_arg = type_arguments[1].clone();
    let TypeExpr::Ref {
        name: target_name,
        type_arguments: target_type_args,
    } = target_arg
    else {
        return None;
    };
    if !target_type_args.is_empty() {
        // Generic-parameterised target — let the standard chain
        // handle it (the helper works on bare alias bodies).
        return None;
    }
    let DeclLookup {
        canonical_source,
        body,
    } = ctx.locate_declaration(target_name.as_ref())?;
    if !canonical_source.contains("/node_modules/") {
        // Workspace-owned target: defer to the canonical reuse path
        // (Phase 11 / §6.5).
        return None;
    }
    if utility_name == "Omit" {
        // Symbolic preservation — return the original
        // `Omit<target, keys>` shape unchanged. No member of the
        // target is enumerated.
        return Some(symbolic_omit_for_package_backed(
            type_arguments[0].clone(),
            keys_arg,
        ));
    }
    // Pick: extract the literal key set and materialise selectively.
    let keys = extract_pick_omit_string_literal_keys(&keys_arg)?;
    if keys.is_empty() {
        return None;
    }
    selective_pick_expansion_for_package_backed(&body, &keys, target_name.as_ref())
}

/// Extract a flat `Vec<String>` of string-literal keys from a
/// `Pick<T, K>` / `Omit<T, K>` second type argument. The shape is
/// either a single `Literal::String` or a `Union` of literal strings;
/// any other shape returns `None` so the caller falls through.
fn extract_pick_omit_string_literal_keys(expr: &TypeExpr) -> Option<Vec<String>> {
    use verter_semantic::analysis::type_expr::LiteralValue;
    match peel_paren(expr) {
        TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
        TypeExpr::Union(arms) => {
            let mut out = Vec::with_capacity(arms.len());
            for arm in arms.iter() {
                match peel_paren(arm) {
                    TypeExpr::Literal(LiteralValue::String(value)) => out.push(value.clone()),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
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
// Slot-binding indexed-access symbolic preservation (Issue #1, partial)
// ---------------------------------------------------------------------------

/// Force the slot binding's `type_expr` back to the symbolic
/// `IndexedAccess` shape encoded in `raw_type` when the indexed access
/// transits through an imported declaration. The eager evaluator may
/// have widened the access through an open `[k: string]: any` index
/// signature; the navigable member-path contract is the better public
/// surface.
fn slot_binding_should_preserve_symbolic_raw_type(
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
    raw_indexed_access_root_is_imported(&parsed, ctx)
}

/// Parse the slot binding's `raw_type` annotation back to a `TypeExpr`.
/// Returns `None` for empty/missing raw types or when the parsed shape
/// is not an `IndexedAccess` (only IndexedAccess is restored by the
/// slot-binding guard).
fn parse_indexed_access_from_raw(raw_type: Option<&str>) -> Option<TypeExpr> {
    let raw = raw_type?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(trimmed);
    if matches!(peel_paren(&parsed), TypeExpr::IndexedAccess { .. }) {
        Some(parsed)
    } else {
        None
    }
}

/// Returns true when `expr` is an `IndexedAccess` whose deref chain
/// transits through a Ref to an imported declaration. The "indexed
/// root" is the chain starting from the indexed access's `object` and
/// the property body that the access selects from the root's
/// declaration body.
fn raw_indexed_access_root_is_imported(expr: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    let TypeExpr::IndexedAccess { object, index } = peel_paren(expr) else {
        return false;
    };
    // Index must be a string literal — that is the member-path the
    // policy can statically inspect inside the root's declaration body.
    let TypeExpr::Literal(LiteralValue::String(member)) = peel_paren(index) else {
        return false;
    };
    // Peel through `Object & { … }` and `Object` shapes when the user
    // wrote the indexed access on a literal object (covered by
    // `reduce_indexed_access_over_object_surface`); for the slot
    // binding case we expect a Ref to a declaration.
    let TypeExpr::Ref { name, .. } = peel_paren(object) else {
        return false;
    };
    let Some(DeclLookup {
        canonical_source,
        body,
    }) = ctx.locate_declaration(name.as_ref())
    else {
        return false;
    };
    // The root's declaration body must be an Object whose `member`
    // property type contains an imported reference (or itself resolves
    // to an imported declaration).
    let property_type = match peel_paren(&body) {
        TypeExpr::Object(obj) => obj.properties.iter().find_map(|m| match m {
            ObjectMember::Property(p) if p.name == *member => Some(p.ty.clone()),
            _ => None,
        }),
        _ => None,
    };
    let Some(property_type) = property_type else {
        return false;
    };
    let _ = canonical_source; // root's own location is not the trigger
    type_expr_contains_imported_ref(&property_type, ctx)
}

/// Walks `expr` and returns true on the first `Ref` whose declaration
/// resolves to an imported (non-owner) declaration. Refs whose
/// declarations cannot be located are ignored — they cannot be proven
/// imported.
fn type_expr_contains_imported_ref(expr: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => type_expr_contains_imported_ref(inner, ctx),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(DeclLookup {
                canonical_source, ..
            }) = ctx.locate_declaration(name.as_ref())
            {
                if canonical_source != ctx.owner_canonical {
                    return true;
                }
            }
            type_arguments
                .iter()
                .any(|arg| type_expr_contains_imported_ref(arg, ctx))
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(|ty| type_expr_contains_imported_ref(ty, ctx)),
        TypeExpr::Array { element, .. } => type_expr_contains_imported_ref(element, ctx),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_contains_imported_ref(&element.ty, ctx)),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_contains_imported_ref(object, ctx)
                || type_expr_contains_imported_ref(index, ctx)
        }
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => type_expr_contains_imported_ref(&prop.ty, ctx),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|parameter| type_expr_contains_imported_ref(&parameter.ty, ctx))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|rt| type_expr_contains_imported_ref(rt, ctx))
            }
            ObjectMember::IndexSignature(sig) => {
                type_expr_contains_imported_ref(&sig.key_type, ctx)
                    || type_expr_contains_imported_ref(&sig.value_type, ctx)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| type_expr_contains_imported_ref(&parameter.ty, ctx))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|rt| type_expr_contains_imported_ref(rt, ctx))
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| type_expr_contains_imported_ref(&parameter.ty, ctx))
                || function
                    .return_type
                    .as_deref()
                    .is_some_and(|rt| type_expr_contains_imported_ref(rt, ctx))
        }
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_imported_ref(inner, ctx)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_contains_imported_ref(check, ctx)
                || type_expr_contains_imported_ref(extends, ctx)
                || type_expr_contains_imported_ref(true_type, ctx)
                || type_expr_contains_imported_ref(false_type, ctx)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_contains_imported_ref(source, ctx)
                || type_expr_contains_imported_ref(value, ctx)
                || name_type
                    .as_deref()
                    .is_some_and(|nt| type_expr_contains_imported_ref(nt, ctx))
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|e| type_expr_contains_imported_ref(e, ctx)),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. } => false,
    }
}

/// Reduce an indexed access on an already-concrete object surface.
/// `{ avatar: T }['avatar']` reduces directly to `T` without consulting
/// the resolver — the object literal is a complete shape so the lookup
/// is purely structural. Used by the slot-binding policy to short-circuit
/// when the materializer's expansion produced a known-shape `Object`
/// where the index-access target is already in the property list.
#[allow(dead_code)]
fn reduce_indexed_access_over_object_surface(
    object: &TypeExpr,
    index: &TypeExpr,
) -> Option<TypeExpr> {
    let TypeExpr::Object(obj) = peel_paren(object) else {
        return None;
    };
    let TypeExpr::Literal(LiteralValue::String(member)) = peel_paren(index) else {
        return None;
    };
    obj.properties.iter().find_map(|m| match m {
        ObjectMember::Property(p) if p.name == *member => Some(p.ty.clone()),
        _ => None,
    })
}

/// Strip leading `Parenthesized` wrappers; mirror the convention used
/// throughout this module.
fn peel_paren(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Parenthesized(inner) => peel_paren(inner),
        _ => expr,
    }
}

// ---------------------------------------------------------------------------
// Cycle-guard normalized type-argument identity
// ---------------------------------------------------------------------------
//
// Recursion guards inside the policy walker (e.g. `rewrite_ref`'s active-ref
// stack) key on `(DeclIdentity, NormalizedTypeArgs)`. Bare-name strings
// collide across scopes (two `Foo`s in different files) and cannot
// distinguish `Pick<X, 'a'>` from `Pick<X, 'b'>` inside a generic
// instantiation chain. The active-refs set on `PolicyCtx` consumes these
// types; `NormalizedTypeArgs::normalize` is the constructor that the cycle
// guard uses to discriminate generic instantiations.

use crate::semantic_query::DeclIdentity;
use smallvec::SmallVec;
use verter_semantic::analysis::type_expr::LiteralValue;

/// Hash of an opaque anonymous shape used to discriminate cycle-guard keys
/// across structurally-identical inline type expressions. Phases that need
/// to distinguish anonymous shapes attach a stable hash; identity equality
/// of the hash is what `NormalizedTypeArg` checks.
pub(crate) type ShapeHash = u64;

/// Hash of a `LiteralValue` used as a cheap discriminator for literal-arg
/// cycle-guard keys (e.g. `Pick<X, 'a'>` vs `Pick<X, 'b'>`).
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NormalizedTypeArgs(SmallVec<[NormalizedTypeArg; 4]>);

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
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn from_normalized<I>(args: I) -> Self
    where
        I: IntoIterator<Item = NormalizedTypeArg>,
    {
        Self(args.into_iter().collect())
    }

    /// Resolver-aware constructor — normalizes a slice of `TypeExpr`
    /// arguments into the cycle-guard key form. Each input maps to one
    /// `NormalizedTypeArg`:
    ///
    /// * `Ref { name, args }` resolves through the policy's declaration
    ///   lookup and produces `Decl(DeclIdentity)` keyed on the resolved
    ///   declaration's canonical source. Refs the resolver cannot
    ///   locate fall back to `AnonymousShape(hash)` so unresolved
    ///   references still discriminate by name.
    /// * `Literal(v)` produces `Literal(hash_literal(v))` so
    ///   `Pick<X, 'a'>` and `Pick<X, 'b'>` produce distinct keys.
    /// * `Infer` / unknown-shape arguments produce
    ///   `AnonymousShape(structural_hash)` — a stable hash of the
    ///   `TypeExpr` (which derives `Hash`) so structurally-identical
    ///   inline shapes share an identity but distinct shapes do not
    ///   collide.
    ///
    /// The function is `&mut PolicyCtx` because resolving a `Ref`
    /// argument may consult the same declaration cache the cycle guard
    /// itself uses.
    ///
    /// Visibility is `pub(super)` so the constructor stays inside the
    /// `component_meta_resolution_policy` module — the consumer is
    /// `rewrite_ref` plus structural normalization helpers in the same
    /// file. `PolicyCtx` is module-private; widening the constructor's
    /// visibility past the module boundary would leak the policy's
    /// resolver state through the type signature.
    #[must_use]
    fn normalize(args: &[TypeExpr], ctx: &mut PolicyCtx<'_, '_>) -> Self {
        let mut out: SmallVec<[NormalizedTypeArg; 4]> = SmallVec::new();
        for arg in args {
            out.push(normalize_one_arg(arg, ctx));
        }
        Self(out)
    }

    /// Number of arguments (zero for `empty`).
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// True when no arguments are present.
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterator over the normalized arguments in positional order.
    #[allow(dead_code)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &NormalizedTypeArg> {
        self.0.iter()
    }
}

/// Resolve one `TypeExpr` argument into its `NormalizedTypeArg` shape.
fn normalize_one_arg(arg: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> NormalizedTypeArg {
    match arg {
        TypeExpr::Parenthesized(inner) => normalize_one_arg(inner, ctx),
        TypeExpr::Ref { name, .. } => {
            // Try to resolve the ref to a declaration identity. When
            // the lookup succeeds the canonical source uniquely keys
            // the argument; otherwise fall back to a structural hash
            // so the cycle guard still discriminates by name.
            if let Some(DeclLookup {
                canonical_source, ..
            }) = ctx.locate_declaration(name.as_ref())
            {
                NormalizedTypeArg::Decl(ctx.decl_identity_for(&canonical_source, name.as_ref()))
            } else {
                NormalizedTypeArg::AnonymousShape(hash_type_expr(arg))
            }
        }
        TypeExpr::Literal(value) => NormalizedTypeArg::Literal(hash_literal(value)),
        TypeExpr::Infer { .. } => NormalizedTypeArg::None,
        _ => NormalizedTypeArg::AnonymousShape(hash_type_expr(arg)),
    }
}

/// Compute a deterministic 64-bit hash of a `TypeExpr`. The structural
/// `Hash` derivation on `TypeExpr` is stable across builds because all
/// component hashers are deterministic; this function routes through
/// `xxh3` so the digest is the same shape used elsewhere in the cycle
/// guard's identity space.
fn hash_type_expr(expr: &TypeExpr) -> ShapeHash {
    use std::hash::Hash;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    expr.hash(&mut bridge);
    hasher.digest()
}

impl Default for NormalizedTypeArgs {
    fn default() -> Self {
        Self::empty()
    }
}

/// Stable hash of a `LiteralValue` for use inside `NormalizedTypeArg::Literal`.
/// Mirrors the manual `Hash` impl on `LiteralValue` — same input bytes
/// produce the same digest across builds.
#[must_use]
pub(crate) fn hash_literal(value: &LiteralValue) -> LiteralHash {
    use std::hash::Hash;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    value.hash(&mut bridge);
    hasher.digest()
}

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

#[cfg(test)]
#[path = "component_meta_resolution_policy_cycle_tests.rs"]
mod cycle_tests;

// ---------------------------------------------------------------------------
// policy_helpers — Issue #11 shared symbolic-preservation predicate
// ---------------------------------------------------------------------------

pub(crate) mod policy_helpers {
    //! Shared predicate consumed by all three callsites in the
    //! component-meta materialize pipeline that decide between
    //! symbolic preservation vs. canonical materialize for an
    //! imported bare ref:
    //!
    //! - `meta_resolve/materialize/field_types.rs::top_level_imported_ref_can_stay_symbolic`
    //! - `resolver_core/component_meta_query_engine/shallow_preserve.rs::should_preserve_imported_bare_ref`
    //! - `meta_resolve/materialize/macro_shapes.rs::named_ref_can_use_prepared_projection`
    //!
    //! Per Issue #11 / §6.5: workspace-owned direct-member
    //! interface/class refs (generic or non-generic) that are not on
    //! the recursion/cycle stack and not in a route-preservation
    //! context MUST materialize canonically — their cache key
    //! `(target_decl_id, normalized_type_args)` is shared across all
    //! callers per CLAUDE.md "generic substitutions are part of
    //! semantic meaning". This contract is mis-expressed at three
    //! sites with three near-equivalent guards on integration; the
    //! helper consolidates it.
    //!
    //! Symbolic preservation is reserved for:
    //! 1. package-backed refs (per `WorkspaceRead::is_package_backed`)
    //! 2. explicit shallow-preservation list entries
    //! 3. recursion / cycle boundaries (target's
    //!    `(DeclId, NormalizedTypeArgs)` already on `active_refs`)
    //! 4. lazy-route expression contexts
    //! 5. slot-binding indexed-access expressions (Issue #1)
    //! 6. terminal indexed-access leaves already published (Issue #5)
    //!
    //! Path-substring checks on `node_modules` are BANNED — the
    //! helper consumes `WorkspaceRead::is_workspace_owned` /
    //! `is_package_backed` exclusively (which route through the
    //! resolver's realpath-based classification). The pnpm-symlink
    //! and workspace-package-inside-node_modules cases are correctly
    //! classified as workspace-owned, NOT package-backed.

    use verter_semantic::analysis::type_eval::TypeDeclKind;
    use verter_semantic::analysis::type_solver::prepared::{
        PreparedProjectionClass, PreparedTypeDecl,
    };

    /// Context bundle for the helper. Exposes the narrow capabilities
    /// the predicate needs without leaking `&mut ComponentMetaQueryEngine`
    /// or `&dyn ResolverContext` through the public helper signature.
    pub(crate) struct PolicyContext<'a> {
        /// Whether `target_canonical_id` is workspace-owned per
        /// `WorkspaceRead::is_workspace_owned` (NOT a path-substring
        /// check on `node_modules`).
        pub is_workspace_owned: &'a dyn Fn(&str) -> bool,
        /// Whether `target_canonical_id` is package-backed per
        /// `WorkspaceRead::is_package_backed` (NOT a path-substring
        /// check on `node_modules`).
        pub is_package_backed: &'a dyn Fn(&str) -> bool,
        /// Caller-provided route-preservation flag. True when the
        /// imported ref is observed inside a lazy-route expression,
        /// a slot-binding indexed-access, or a terminal
        /// indexed-access leaf already published. The caller is
        /// responsible for setting this correctly per their callsite
        /// context.
        pub route_preservation_context: bool,
        /// Caller-provided cycle-active flag. True when the target's
        /// `(DeclId, NormalizedTypeArgs)` identity is already on the
        /// caller's active recursion stack. The caller is responsible
        /// for setting this correctly per their callsite's recursion
        /// guard infrastructure.
        pub cycle_active_for_target: bool,
        /// Caller-provided shallow-preservation list flag. True when
        /// the target's resolved name is on an explicit
        /// shallow-preservation list (e.g., Vue runtime types kept
        /// symbolic by name). The caller is responsible for setting
        /// this correctly per their callsite.
        pub shallow_preserve_list_entry: bool,
    }

    /// Phase 11 / Issue #11 — shared predicate that decides whether
    /// an imported bare ref MUST materialize canonically (returns
    /// `true`) or MAY stay symbolic (returns `false`).
    ///
    /// - `target_canonical_id`: the target declaration's canonical
    ///   source id (after import-route resolution).
    /// - `prepared_body`: the resolved `PreparedTypeDecl` for the
    ///   target (or `None` if no prepared decl is available yet —
    ///   that case treats the body as not direct-member-eligible).
    /// - `ctx`: the callsite-provided `PolicyContext` bundle.
    ///
    /// ## Decision flow
    ///
    /// 1. If `ctx.is_package_backed(target_canonical_id)` →
    ///    return `false` (preserve symbolic, disallowed shape #1).
    /// 2. If `ctx.shallow_preserve_list_entry` →
    ///    return `false` (disallowed shape #2).
    /// 3. If `ctx.cycle_active_for_target` →
    ///    return `false` (disallowed shape #3).
    /// 4. If `ctx.route_preservation_context` →
    ///    return `false` (disallowed shapes #4-#6).
    /// 5. If NOT `ctx.is_workspace_owned(target_canonical_id)` →
    ///    return `false` (the helper only fires for workspace-owned
    ///    targets; everything else is conservative).
    /// 6. If `prepared_body` describes a direct-member interface or
    ///    class (interface/class kind, OR
    ///    `PreparedProjectionClass::DirectMembers`, OR a non-empty
    ///    `member_index`) → return `true` (canonical materialize is
    ///    required).
    /// 7. Otherwise → return `false` (preserve symbolic, conservative
    ///    default).
    ///
    /// Per §6.5, generic targets are eligible — the cache key
    /// `(target_decl_id, normalized_type_args)` is the responsibility
    /// of the materialization layer, not this predicate.
    #[must_use]
    pub(crate) fn imported_ref_must_materialize_canonically(
        target_canonical_id: &str,
        prepared_body: Option<&PreparedTypeDecl>,
        ctx: &PolicyContext<'_>,
    ) -> bool {
        // 1. Package-backed → preserve symbolic.
        if (ctx.is_package_backed)(target_canonical_id) {
            return false;
        }

        // 2. Explicit shallow-preservation list entry → preserve symbolic.
        if ctx.shallow_preserve_list_entry {
            return false;
        }

        // 3. Recursion/cycle stack → preserve symbolic.
        if ctx.cycle_active_for_target {
            return false;
        }

        // 4-6. Route-preservation context → preserve symbolic.
        if ctx.route_preservation_context {
            return false;
        }

        // 5. Not workspace-owned → preserve symbolic (conservative).
        if !(ctx.is_workspace_owned)(target_canonical_id) {
            return false;
        }

        // 6. Workspace-owned direct-member interface/class →
        //    canonical materialize required.
        let Some(prepared) = prepared_body else {
            return false;
        };

        matches!(prepared.kind, TypeDeclKind::Class)
            || matches!(prepared.kind, TypeDeclKind::Interface)
            || matches!(
                prepared.projection_class,
                PreparedProjectionClass::DirectMembers
            )
            || !prepared.member_index.is_empty()
    }
}
