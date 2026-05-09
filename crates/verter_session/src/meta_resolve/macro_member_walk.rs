//! Helpers used by the per-macro projector decomposition (§7.1):
//! `defineProps<T>()` root-name collection and the slot-binding
//! registry-collection skip predicate. Production routes through
//! `meta_resolve::projectors::project_evaluated_types`; per-member
//! reduction is owned by the projector via
//! `materialize_component_meta_type_expr_until_stable`.

use crate::types::FileAnalysisSnapshot;

/// Capture-token counter name recorded every time the slot-binding
/// registry-collection skip predicate fires for a slot binding rooted
/// in the owner's own `defineProps<T>()` interface. Used by
/// `component_meta_slot_binding_skip_tests` to discriminate the
/// positive case (counter > 0) from the counterfixtures (counter == 0)
/// via `CaptureToken::start_for_query` / `CaptureToken::end()`.
pub(crate) const SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER: &str =
    "slot_binding_registry_collection_skips";

/// Issue #10 / capture-token counter incremented every
/// time the Pick member-route materialiser actually descends into a
/// callable parameter type. The package-backed suppression predicate
/// (`pick_member_route_should_skip_callable_descent`) bypasses the
/// indexed-access route entirely; when bypassed, the counter does NOT
/// increment for that member. Used by
/// `component_meta_pick_omit_tests::declared_session_meta_preserves_imported_pick_callback_package_param`
/// (asserts `== 0` for package-backed param) and
/// `pick_callback_workspace_local_param_still_descends` (asserts
/// `>= 1` for workspace-local param).
pub(crate) const PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER: &str =
    "pick_member_route_callable_descent_count";

/// Collect the source-level root type names referenced by every
/// type-based `defineProps<T>()` macro in `snapshot.macros`. The
/// result is the set of names that — when found at the root of a
/// slot binding's raw type — make the binding's contribution to the
/// component-meta registry redundant (the defineProps interface is
/// already authoritative for that surface).
///
/// Only top-level `Ref { name }` roots are collected. Inline
/// object-literal type arguments, intersections, unions, and other
/// composite shapes do not produce a root name (the predicate
/// downstream falls back to `false` for those cases).
pub(crate) fn collect_define_props_root_names(
    snapshot: &FileAnalysisSnapshot,
) -> rustc_hash::FxHashSet<String> {
    use verter_semantic::analysis::type_expr::TypeExpr;
    use verter_semantic::analysis::AnalyzedMacroKind;

    fn root_ref_name(ty: &TypeExpr) -> Option<&str> {
        match ty {
            TypeExpr::Ref { name, .. } => Some(name.as_ref()),
            TypeExpr::Parenthesized(inner) => root_ref_name(inner),
            _ => None,
        }
    }

    let mut names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for mac in snapshot.macros.iter() {
        if mac.kind != AnalyzedMacroKind::DefineProps || !mac.is_type_based {
            continue;
        }
        if let Some(arg) = mac.parsed_type_argument.as_deref() {
            if let Some(name) = root_ref_name(arg) {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// Decide whether the slot-binding's raw-type root is the owner's
/// own `defineProps<T>()` interface — in which case the binding's
/// registry-collection contribution is redundant and can be
/// skipped.
///
/// The predicate fires only when the binding's raw type root
/// resolves to a name in `define_props_roots` for the same owner
/// AND the binding does NOT introduce a new prop surface beyond
/// what the defineProps root already exposes:
///
/// - `Props['avatar']` (indexed-access route) → fires when
///   `Props ∈ define_props_roots`.
/// - `Pick<Props, 'avatar' | 'label'>` / `Omit<Props, 'count'>`
///   (utility route) → fires when `Props ∈ define_props_roots`.
/// - `Props & Extra` (intersection broadens surface) → does NOT
///   fire. The intersection's `Extra` arm is reachable only
///   through the registry-collection call.
/// - `Props | Other` (union) → does NOT fire. Same reasoning as
///   intersection.
/// - `ButtonProps['avatar']` where `ButtonProps` is imported (not
///   the owner's defineProps root) → does NOT fire.
/// - `Primitive(_)` / `Object(_)` / fully-expanded fields whose
///   raw type was None → does NOT fire (no work to skip).
pub(crate) fn slot_binding_targets_define_props_root(
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    define_props_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    if define_props_roots.is_empty() {
        return false;
    }

    // Mirror the `expr` selection in
    // `collect_component_meta_registry_public_field_refs`: prefer the
    // parsed `raw_type` when present, otherwise fall back to the
    // (already-expanded) `r#type`.
    let parsed_raw = field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
    let expr: &TypeExpr = parsed_raw.as_ref().unwrap_or(&field.r#type);

    fn unwrap_paren(ty: &TypeExpr) -> &TypeExpr {
        match ty {
            TypeExpr::Parenthesized(inner) => unwrap_paren(inner),
            _ => ty,
        }
    }
    let expr = unwrap_paren(expr);

    // Broadening shapes (intersection / union) must NOT skip — extra
    // arms beyond the defineProps surface would be lost.
    if matches!(expr, TypeExpr::Intersection(_) | TypeExpr::Union(_)) {
        return false;
    }

    // Try the indexed-access / utility route extractors. These return
    // the source-level root name (e.g. `Props` for `Props['avatar']`
    // or `Pick<Props, ...>`) when the expression is structurally a
    // path projection rooted at a single Ref.
    let root_name = crate::resolver_core::component_meta_registry::component_meta_registry_public_utility_route(expr)
        .or_else(|| {
            crate::resolver_core::component_meta_registry::component_meta_registry_public_indexed_access_route(expr)
        })
        .map(|(name, _route)| name);

    if let Some(root) = root_name {
        return define_props_roots.contains(&root);
    }

    false
}
