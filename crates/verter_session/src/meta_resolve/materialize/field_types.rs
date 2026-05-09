//! Materialization core: TypeExpr stabilizer + field-type rescue.
//!
//! Domain 7 (field-types portion). Owns:
//! - the eager whole-expression materializer
//!   (`materialize_component_meta_type_expr_until_stable` + `_full`),
//! - the `materialize_component_meta_field_types` driver,
//! - the field-rescue / shallow-symbolic / package-backed predicates that
//!   gate it,
//! - the test-only `MTL_CALL_COUNT` instrumentation that the eager-entry
//!   FAIL-FIRST tests count off (Step 6.2 / D22).
//!
//! Lines 99-1827 of the pre-split `meta_resolve.rs` (with the test-only
//! MTL counter section moved here from later in the shell so the static
//! it counts off lives in the same module). The body is verbatim apart
//! from `pub(crate)` visibility escalation on the formerly-private items
//! the parent shell still calls (matches the foundation
//! extractions: `dispatch_helpers.rs`, `resolved_state.rs`, etc.).

use crate::instant::Instant;
use crate::resolver_core::component_meta_registry::{
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route,
};
use crate::resolver_core::ResolverContext;
use crate::types::FileAnalysisSnapshot;
use std::sync::Arc;

use super::super::dep_signature::accumulate_dispatch_dep_signature;
use super::super::dispatch_helpers::{
    project_expr_class_a_via_dispatch, project_type_surface_expr_via_host_threaded,
};
use super::super::field_state::MacroFieldGraphState;
// `request_host` source moved to
// `host_manage/component_meta_request_impl.rs`. Import rewritten to
// the new home.
use super::super::resolved_state::select_imported_materialization_scope;
use super::super::scoring::compare_type_expr_improvement;
use super::macro_shapes::expr_needs_projection_rescue;
use crate::host_manage::component_meta_request_impl::ResolvedMacroMeta;

// `component_meta_registry_should_keep_raw_symbolic_non_object_alias`
// and `preserve_package_backed_symbolic_refs_node` live in the
// `registry_materialize` sibling;
// `type_node_needs_member_route_materialization` lives in the
// `graph_predicates` sibling.
use super::super::graph_predicates::type_node_needs_member_route_materialization;
use super::super::registry_materialize::{
    component_meta_registry_should_keep_raw_symbolic_non_object_alias,
    preserve_package_backed_symbolic_refs_node,
};

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    materialize_component_meta_type_expr_until_stable_full(
        expr,
        scope_canonical_id,
        mode,
        query_engine,
    )
    .type_expr
}

/// Materialize a `TypeExpr` and return both the result and the
/// producing `SemanticNodeId` + accumulated dep_signature
/// ([`MaterializedTypeExpr`]; D31 / D32). Sidecar-capture call sites
/// (Step 9 surface-id propagation) read `.node_id`; the session merges
/// `.dep_signature` into `ResolvedComponentMetaState.fact_versions`
/// before publish (Step 6.6.A).
///
/// The main entry [`materialize_component_meta_type_expr_until_stable`]
/// remains for callers that need only the `TypeExpr` shell — it
/// delegates here and discards `node_id` / `dep_signature`.
///
/// **Body (Step 1.5 final cutover):** the legacy owner-vs-imported
/// scope reconciliation has been removed. Materialization now flows
/// entirely through dispatch:
/// `shallow_lower_type_expr` → `raise_and_reduce(mode)`. Step 1.5
/// closed the three substitution-parity gaps that previously required
/// the legacy walker fallback (Pick<X,K>['member'] indexed access,
/// mapped+conditional `infer P` per-key reduction, and method
/// signatures used as `IndexedAccess` bases).
///
/// Per-request memoisation is preserved so repeat queries of the same
/// `(scope, expr, mode)` triple within one component-meta request
/// reuse the prior result instead of re-running the dispatch
/// reduction. Dispatch's own family memo handles cross-request
/// deduplication.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable_full(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::NodeScopeId;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    // Step 6.2 / D22: count every entry into whole-expression
    // materialization. Memo hits + cold builds both increment so the
    // FAIL-FIRST test discriminates the call-ordering contract at the
    // *entry* boundary, not the build closure.
    #[cfg(test)]
    MTL_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // §4.5 items 2-5: per-request memo keyed on `(scope, candidate, mode)`.
    let memo_key = (
        scope_canonical_id.to_string(),
        expr.clone(),
        matches!(mode, crate::semantic_query::ProjectionMode::Navigate),
    );
    #[cfg(test)]
    crate::spike_instrumentation::record_cache_read("materialize_memo");
    if let Some(cached) = query_engine
        .materialize_memo
        .borrow()
        .get(&memo_key)
        .cloned()
    {
        return cached;
    }

    // Step 3 closure: peek ctx-owned MaterializeMemoDb.
    {
        // Loop-5 instrumentation — bump peek for every host-memo
        // read attempt; bump hit only on the cached return path.
        crate::loop5_instrumentation::MATERIALIZE_MEMO_PEEKS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let ctx = query_engine.ctx();
        let arc_key = (
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            mode,
        );
        let host_db = ctx.project_type_store().materialize_memo_db();
        if let Some(cached) = host_db.peek(&arc_key, ctx) {
            crate::loop5_instrumentation::MATERIALIZE_MEMO_HITS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            query_engine
                .materialize_memo
                .borrow_mut()
                .insert(memo_key, cached.clone());
            return cached;
        }
    }

    // Step 1.5 thin dispatch wrapper. Build NodeScopeId for the file
    // scope, then lower → raise_and_reduce in the caller's mode.
    let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
    let ctx = query_engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let env: FxHashMap<String, crate::semantic_query::SemanticNodeId> = FxHashMap::default();
    let whole_hash = ctx
        .shallow_file_state(scope_canonical_id)
        .map(|state| state.whole_hash)
        .unwrap_or_default();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(scope_canonical_id),
        whole_hash,
        local_scope: None,
    };
    let name_resolution = rustc_hash::FxHashMap::default();
    let mut substitutions: Vec<(Arc<str>, crate::semantic_query::SemanticNodeId)> = Vec::new();
    // R15/F11 — capture the scope-shadowing context
    // once for the materialize → lower pipeline so the dispatch
    // fast-path observes the same shadow set the route extraction
    // path uses.
    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
        scope_payload.as_deref(),
    );
    let _us_trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
    if _us_trace {
        eprintln!(
            "[US_LOWER_START] scope={} mode={:?}",
            scope_canonical_id, mode
        );
    }
    let _us_lower_t0 = Instant::now();
    let lowered = dispatch.shallow_lower_type_expr(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_deref(),
        &shadowing,
        &mut substitutions,
        mode,
    );
    let _us_lower_ms = _us_lower_t0.elapsed().as_secs_f64() * 1000.0;
    if _us_trace {
        eprintln!(
            "[US_LOWER_END] scope={} mode={:?} lower_ms={:.1}",
            scope_canonical_id, mode, _us_lower_ms
        );
    }
    let _us_rr_t0 = Instant::now();
    let dispatch_materialized = dispatch.raise_and_reduce(lowered, mode);
    let _us_rr_ms = _us_rr_t0.elapsed().as_secs_f64() * 1000.0;
    if _us_trace {
        eprintln!(
            "[US_RAISE_END] scope={} mode={:?} raise_reduce_ms={:.1}",
            scope_canonical_id, mode, _us_rr_ms
        );
    }

    // Step 6.6.A: accumulate dispatch's dep_signature into the
    // per-request thread-local so compute_component_meta_state_inner
    // can merge the facts into ResolvedComponentMetaState.fact_versions
    // before publish. Each materialize call contributes its own
    // dispatch-side fact set; the accumulator deduplicates.
    accumulate_dispatch_dep_signature(&dispatch_materialized.dep_signature);

    let materialized = MaterializedTypeExpr {
        node_id: dispatch_materialized.node_id,
        type_expr: dispatch_materialized.type_expr,
        dep_signature: dispatch_materialized.dep_signature,
    };

    // Step 3 closure: write-through to ctx-owned MaterializeMemoDb.
    {
        // Loop-5 instrumentation — count every publish attempt. The
        // get_or_compute path is a no-op on a concurrent winner but
        // we count the attempt because the bench is single-threaded.
        crate::loop5_instrumentation::MATERIALIZE_MEMO_PUBLISHES
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let arc_key = (
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            mode,
        );
        let host_db = ctx.project_type_store().materialize_memo_db();
        let captured_value = materialized.clone();
        let captured_canonical = scope_canonical_id.to_string();
        let _ = host_db.get_or_compute(&arc_key, ctx, move || {
            let dep_sig = crate::resolver_core::component_meta_query_engine::engine_dep_signature_for_canonical(
                ctx,
                captured_canonical.as_str(),
            );
            Some((captured_value, dep_sig))
        });
    }

    query_engine
        .materialize_memo
        .borrow_mut()
        .insert(memo_key, materialized.clone());
    materialized
}

pub(crate) fn type_expr_has_package_backed_object_like_root(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    fn root_name(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> Option<String> {
        use verter_semantic::analysis::type_expr::TypeExpr;

        match expr {
            TypeExpr::Parenthesized(inner) => root_name(inner),
            TypeExpr::IndexedAccess { object, .. } => root_name(object),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if matches!(name.as_ref(), "Pick" | "Omit") && type_arguments.len() == 2 => {
                crate::resolver_core::component_meta_registry::component_meta_registry_ref_name(
                    &type_arguments[0],
                )
                .map(str::to_string)
            }
            TypeExpr::Ref { name, .. } => Some(name.to_string()),
            _ => None,
        }
    }

    let Some(root_name) = root_name(expr) else {
        return false;
    };

    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    // Issue #11 / route the package-backed classification
    // through `WorkspaceRead::is_package_backed` (NOT a path-substring
    // check on `node_modules`). The realpath-based classification
    // correctly handles pnpm-symlinks and workspace-packages-inside-
    // node_modules.
    if !query_engine
        .ctx
        .workspace_is_package_backed(declaration_scope.as_str())
    {
        return false;
    }

    if matches!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        return true;
    }

    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            crate::resolver_core::component_meta_registry::component_meta_registry_has_explicit_object_surface(&body)
        })
}

pub(crate) fn type_expr_is_slots_member_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

    match expr {
        TypeExpr::IndexedAccess { index, .. } => matches!(
            index.as_ref(),
            TypeExpr::Literal(LiteralValue::String(name)) if name.as_str() == "slots"
        ),
        TypeExpr::Parenthesized(inner) => type_expr_is_slots_member_route(inner),
        _ => false,
    }
}

/// Capture-token counter recorded every time the per-field member-route
/// materialization path actually runs (Issue #5). The indexed-
/// access early-out short-circuits the field loop before the member
/// route fires when the published surface is already terminal — for
/// those fields this counter stays at 0. Counterfixtures (conditional
/// indexed root, recursive indexed access, mapped indexed root,
/// `Record<K, never>` non-object surface) take the slow path and the
/// counter increments.
pub(crate) const MEMBER_ROUTE_CALLS_COUNTER: &str = "member_route_calls";

/// / Issue #5 — true when `expr` is a *terminal scalar surface*:
/// a single primitive, a literal, a literal-string union, or `any |
/// scalars`. This is the condition under which a raw `IndexedAccess`
/// route does not need to be re-projected through the registry member
/// route — the published surface is already the final scalar shape.
///
/// Disallowed shapes (return `false`):
/// - any `Object`, `Function`, `Conditional`, `Mapped`, `IndexedAccess`
///   (the published surface is not terminal)
/// - `Unknown { raw: "semanticMiss" }` and other unresolved sentinels
/// - `TypeOf`, `KeyOf`, `Rest`, `TemplateLiteral`, `Tuple`, `Array`
/// - generic parameters / inferred shells
pub(crate) fn type_expr_is_terminal_scalar_surface(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{LiteralValue, PrimitiveName, TypeExpr};

    fn is_scalar_atom(expr: &TypeExpr) -> bool {
        match expr {
            TypeExpr::Parenthesized(inner) => is_scalar_atom(inner),
            TypeExpr::Primitive(name) => matches!(
                name,
                PrimitiveName::String
                    | PrimitiveName::Number
                    | PrimitiveName::Boolean
                    | PrimitiveName::Symbol
                    | PrimitiveName::BigInt
                    | PrimitiveName::Any
                    | PrimitiveName::Null
                    | PrimitiveName::Undefined
                    | PrimitiveName::Void
            ),
            TypeExpr::Literal(value) => matches!(
                value,
                LiteralValue::String(_) | LiteralValue::Number(_) | LiteralValue::Boolean(_),
            ),
            _ => false,
        }
    }

    match expr {
        TypeExpr::Parenthesized(inner) => type_expr_is_terminal_scalar_surface(inner),
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => is_scalar_atom(expr),
        TypeExpr::Union(members) => !members.is_empty() && members.iter().all(is_scalar_atom),
        _ => false,
    }
}

/// / Issue #5 — true when `expr` is an `IndexedAccess` route
/// (or a `Parenthesized` wrapper around one) whose index expression is
/// a literal string or a literal-string union (NOT a generic /
/// type-parameter, NOT a conditional, NOT mapped). This is the syntactic
/// shape under which the indexed-access early-out is legal.
///
/// Disallowed shapes (return `false`):
/// - non-`IndexedAccess` roots
/// - generic / type-parameter index expressions
/// - conditional / mapped / template-literal index expressions
pub(crate) fn type_expr_is_indexed_access_route(
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

    fn index_is_literal_string_or_literal_string_union(index: &TypeExpr) -> bool {
        match index {
            TypeExpr::Parenthesized(inner) => {
                index_is_literal_string_or_literal_string_union(inner)
            }
            TypeExpr::Literal(LiteralValue::String(_)) => true,
            TypeExpr::Union(members) => {
                !members.is_empty()
                    && members.iter().all(|member| {
                        matches!(
                            member,
                            TypeExpr::Literal(LiteralValue::String(_)) | TypeExpr::Parenthesized(_)
                        ) && index_is_literal_string_or_literal_string_union(member)
                    })
            }
            _ => false,
        }
    }

    match raw {
        TypeExpr::Parenthesized(inner) => type_expr_is_indexed_access_route(inner),
        TypeExpr::IndexedAccess { object: _, index } => {
            // The object must NOT be conditional, mapped, or
            // template-literal — those shapes drive the slow path. We
            // also forbid recursive indexed access at the root.
            // (The body of the early-out caller checks that the
            // *published* surface is terminal scalar; the root shape
            // discipline is enforced by both this predicate and the
            // caller's surface check.)
            index_is_literal_string_or_literal_string_union(index)
        }
        _ => false,
    }
}

/// / Issue #5 — true when `expr` is a `non-empty object surface`
/// per §6.3 strict definition: an `Object` whose `properties.len() >= 1`,
/// at least one property's type is NOT `never`, and the object is not
/// solely an index signature whose value type is `never` (e.g.,
/// `Record<string, never>` is NOT a non-empty object surface; `{}` is
/// NOT a non-empty object surface).
///
/// `Parenthesized` wrappers are stripped.
pub(crate) fn type_expr_is_non_empty_object_surface(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, PrimitiveName, TypeExpr};

    fn is_never(expr: &TypeExpr) -> bool {
        match expr {
            TypeExpr::Parenthesized(inner) => is_never(inner),
            TypeExpr::Primitive(PrimitiveName::Never) => true,
            _ => false,
        }
    }

    let stripped = match expr {
        TypeExpr::Parenthesized(inner) => return type_expr_is_non_empty_object_surface(inner),
        other => other,
    };

    let TypeExpr::Object(obj) = stripped else {
        return false;
    };

    if obj.properties.is_empty() {
        return false;
    }

    // True iff at least one Property/Method has a non-never type. An
    // object that is solely an index signature whose value type is
    // `never` (e.g., `Record<string, never>` lowered) does NOT count.
    let mut saw_non_never_property = false;
    let mut saw_only_never_index_sig = true;
    for member in &obj.properties {
        match member {
            ObjectMember::Property(prop) => {
                saw_only_never_index_sig = false;
                if !is_never(&prop.ty) {
                    saw_non_never_property = true;
                }
            }
            ObjectMember::Method(_)
            | ObjectMember::CallSignature(_)
            | ObjectMember::ConstructSignature(_) => {
                saw_only_never_index_sig = false;
                saw_non_never_property = true;
            }
            ObjectMember::IndexSignature(sig) => {
                if !is_never(&sig.value_type) {
                    saw_only_never_index_sig = false;
                    saw_non_never_property = true;
                }
            }
        }
    }

    saw_non_never_property && !saw_only_never_index_sig
}

/// / Issue #5 — true when the root of an `IndexedAccess` route
/// resolves to a workspace-owned declaration (per
/// `WorkspaceRead::is_workspace_owned`, NOT path-substring
/// `node_modules`). The early-out must not fire for package-backed
/// roots — those flow through the existing rescue / project-type-store
/// path. Generic / utility-route roots that wrap a `Ref` are unwrapped
/// to find the underlying base name.
pub(crate) fn raw_indexed_access_root_is_workspace_owned(
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    fn root_name(expr: &TypeExpr) -> Option<&str> {
        match expr {
            TypeExpr::Parenthesized(inner) => root_name(inner),
            TypeExpr::IndexedAccess { object, .. } => root_name(object),
            TypeExpr::Ref { name, .. } => Some(name.as_ref()),
            _ => None,
        }
    }

    let Some(root_name) = root_name(raw) else {
        return false;
    };

    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, root_name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id
    } else {
        declaration.canonical_source.as_str()
    };

    query_engine
        .ctx
        .workspace_is_workspace_owned(declaration_scope)
}

// The materialiser's registry-route branch handles
// route shapes (`Pick<T, K>`, `Omit<T, K>`, `T['k']`) through
// dispatch's canonical projection, so the alias-body walk-through is
// not needed. The removed symbols are listed in the `RETIRED_SYMBOLS`
// array of the static-grep gate test.

pub(crate) fn parsed_field_raw_type(
    field: &verter_semantic::analysis::type_expand::ExpandedField,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
        .filter(|expr| !expr.is_unknown())
}

pub(crate) fn interface_body_has_members_needing_materialization(
    body: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
    fn member_type_needs_materialization(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::Conditional { .. } => true,
            TypeExpr::Parenthesized(inner) => member_type_needs_materialization(inner),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                types.iter().any(member_type_needs_materialization)
            }
            _ => false,
        }
    }
    match body {
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => member_type_needs_materialization(&prop.ty),
            _ => false,
        }),
        TypeExpr::Intersection(types) => types
            .iter()
            .any(interface_body_has_members_needing_materialization),
        _ => false,
    }
}

pub(crate) fn top_level_imported_ref_can_stay_symbolic(
    scope_canonical_id: &str,
    name: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        name.to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());

    // Issue #11 / consult the shared symbolic-preservation
    // helper before any kind-specific branching. Workspace-owned
    // direct-member interface/class refs (generic or non-generic) MUST
    // materialize canonically (cache key includes
    // `(target_decl_id, normalized_type_args)`), so the helper returns
    // `true` and this predicate returns `false` (do NOT preserve
    // symbolic). Symbolic preservation is reserved for: package-backed
    // refs, explicit shallow-preservation list entries, recursion/cycle
    // boundaries, and route-preservation expressions.
    let prepared = query_engine
        .ctx
        .prepared_type_decl(target_scope.as_str(), target_name.as_str());
    let policy_ctx = crate::component_meta_resolution_policy::policy_helpers::PolicyContext {
        is_workspace_owned: &|canonical| query_engine.ctx.workspace_is_workspace_owned(canonical),
        is_package_backed: &|canonical| query_engine.ctx.workspace_is_package_backed(canonical),
        // Top-level ref site: the field-rescue caller handles
        // route-preservation (lazy route, slot-binding) at its own
        // site; top-level imported bare refs are NOT route-
        // preservation contexts here.
        route_preservation_context: false,
        // Top-level entry: the helper's own cycle guard relies on
        // the caller's recursion stack; this site has no active
        // refs (it is the entry point of the field rescue).
        cycle_active_for_target: false,
        // Top-level ref site: the field-rescue caller does not
        // maintain an explicit shallow-preservation list at this
        // boundary (the existing `should_preserve_imported_bare_ref`
        // path handles package-backed shapes via
        // `is_package_backed`).
        shallow_preserve_list_entry: false,
    };
    let must_materialize = crate::component_meta_resolution_policy::policy_helpers::imported_ref_must_materialize_canonically(
        target_scope.as_str(),
        prepared.as_deref(),
        &policy_ctx,
    );
    if must_materialize {
        return false;
    }

    let target_declaration = query_engine
        .resolve_direct_prepared_type_declaration_metadata(
            target_scope.as_str(),
            target_name.as_str(),
        )
        .unwrap_or(declaration);

    if matches!(
        target_declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        // Interfaces with members that need materialization (IndexedAccess,
        // Mapped types) should not stay symbolic — the consumer needs the
        // concrete member shapes. Retained for non-workspace-owned
        // (e.g. package-backed) Interface/Class refs after the Issue #11
        // helper has decided this is not a canonical-reuse case.
        let body_needs_materialization = query_engine
            .named_decl_body(target_scope.as_str(), target_name.as_str())
            .is_some_and(|body| interface_body_has_members_needing_materialization(&body));
        if !body_needs_materialization {
            return true;
        }
    }

    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &body,
                target_scope.as_str(),
                query_engine,
            )
        })
}

pub(crate) fn field_should_preserve_shallow_symbolic_raw_type(
    scope_canonical_id: &str,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let Some(raw) = parsed_field_raw_type(field) else {
        return false;
    };

    match &raw {
        verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => top_level_imported_ref_can_stay_symbolic(
            scope_canonical_id,
            name.as_ref(),
            query_engine,
        ),
        _ if component_meta_registry_public_utility_route(&raw).is_some() => {
            type_expr_has_package_backed_object_like_root(&raw, scope_canonical_id, query_engine)
        }
        verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess { .. } => {
            type_expr_has_package_backed_object_like_root(&raw, scope_canonical_id, query_engine)
        }
        verter_semantic::analysis::type_expr::TypeExpr::TypeOf(_)
        | verter_semantic::analysis::type_expr::TypeExpr::TypeParameter(_) => false,
        _ => {
            query_engine.should_preserve_shallow_field_expr(scope_canonical_id, &raw)
                && !lowered_needs_member_route_materialization(
                    &raw,
                    scope_canonical_id,
                    query_engine,
                )
        }
    }
}

/// Migration helper. Lowers `expr` to a Navigate-mode
/// `SemanticNodeId` and dispatches to J1's graph-native
/// [`type_node_needs_member_route_materialization`] predicate. The
/// cycle-BFS dep-signature facts collected during the predicate's walk
/// are accumulated into the per-request thread-local dispatch
/// accumulator so the caller's completion fence remains complete
/// (matches legacy behaviour: the deleted TypeExpr predicate routed
/// through the deleted F-era TypeExpr cycle adapter which accumulated
/// the same way).
///
/// Returns `false` (conservative: not needed) when lowering fails —
/// matches the deleted TypeExpr predicate's behaviour for shapes the
/// dispatcher cannot lower.
pub(crate) fn lowered_needs_member_route_materialization(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let ctx = query_engine.ctx;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return false;
    };
    let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let result = type_node_needs_member_route_materialization(ctx, node, &mut local_fence, 0);
    if !local_fence.is_empty() {
        accumulate_dispatch_dep_signature(&Arc::from(local_fence.into_boxed_slice()));
    }
    result
}

/// Migration helper. Lowers `materialized` and `raw`
/// TypeExpr inputs to Navigate-mode `SemanticNodeId`s, dispatches to
/// J4's graph-native [`preserve_package_backed_symbolic_refs_node`],
/// and raises the result back to TypeExpr.
///
/// Returns `materialized.clone()` (matches the deleted TypeExpr
/// predicate's `_ => materialized.clone()` arm) when either lowering
/// fails or the raise back to TypeExpr fails — preserves existing
/// behaviour for shapes the dispatcher cannot lower deterministically.
pub(crate) fn lowered_preserve_package_backed_symbolic_refs(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let ctx = engine.ctx;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let Some(materialized_node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        materialized,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return materialized.clone();
    };
    let Some(raw_node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        raw,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return materialized.clone();
    };
    let preserved_node =
        preserve_package_backed_symbolic_refs_node(ctx, materialized_node, raw_node, 0);
    if preserved_node == materialized_node {
        return materialized.clone();
    }
    dispatch
        .raise_node_to_type_expr(preserved_node)
        .unwrap_or_else(|| materialized.clone())
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_field_types(
    scope_canonical_id: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new(
        &crate::loop5_instrumentation::MATERIALIZE_FIELD_TYPES_CALLS,
        &crate::loop5_instrumentation::MATERIALIZE_FIELD_TYPES_NS,
    );
    let _ft_t0 = Instant::now();
    if std::env::var("VERTER_PROGRESS_STREAM").is_ok() {
        eprintln!(
            "[FIELD_TYPES_ENTER] owner={} props={} slots={} bindings={}",
            scope_canonical_id,
            evaluated_types.props.len(),
            evaluated_types.slot_bindings.len(),
            evaluated_types.bindings.len()
        );
    }
    fn rescue_field(
        scope_canonical_id: &str,
        field: &verter_semantic::analysis::type_expand::ExpandedField,
        field_state: &mut MacroFieldGraphState<'_>,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        mode: crate::semantic_query::ProjectionMode,
    ) {
        let trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
        let t0 = Instant::now();
        let needs_rescue = expr_needs_projection_rescue(
            query_engine,
            scope_canonical_id,
            field_state.published_type(),
        );
        if trace {
            eprintln!(
                "[RF_NEEDS_RESCUE] field={} mode={:?} needs={} ms={:.1}",
                field.name,
                mode,
                needs_rescue,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        if !needs_rescue {
            return;
        }

        let t_scope = Instant::now();
        let materialize_scope_canonical_id = select_imported_materialization_scope(
            field_state.published_type(),
            scope_canonical_id,
            query_engine,
        )
        .or_else(|| {
            parsed_field_raw_type(field).as_ref().and_then(|raw| {
                select_imported_materialization_scope(raw, scope_canonical_id, query_engine)
            })
        })
        .unwrap_or_else(|| scope_canonical_id.to_string());
        if trace {
            eprintln!(
                "[RF_SCOPE_SELECT] field={} scope_picked={} ms={:.1}",
                field.name,
                materialize_scope_canonical_id,
                t_scope.elapsed().as_secs_f64() * 1000.0
            );
        }

        let t_us = Instant::now();
        let rescued = materialize_component_meta_type_expr_until_stable(
            field_state.published_type(),
            materialize_scope_canonical_id.as_str(),
            mode,
            query_engine,
        );
        if trace {
            eprintln!(
                "[RF_UNTIL_STABLE] field={} mode={:?} ms={:.1}",
                field.name,
                mode,
                t_us.elapsed().as_secs_f64() * 1000.0
            );
        }
        if rescued != *field_state.published_type() {
            field_state.set_current_type(rescued);
        }
    }

    /// Call the J1 `_node` predicate via the
    /// field-state's lazy-lowered current_node. Returns `false` when
    /// lowering fails (matches the legacy TypeExpr predicate's
    /// "conservative not-needed" fallback when no canonical node id
    /// exists for the input).
    fn current_needs_member_route_materialization(
        ctx: &dyn ResolverContext,
        field_state: &mut MacroFieldGraphState<'_>,
    ) -> bool {
        let Some(node) = field_state.current_node() else {
            return false;
        };
        let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        type_node_needs_member_route_materialization(ctx, node, &mut local_fence, 0)
    }

    /// Call the J1 `_node` predicate via the
    /// field-state's lazy-lowered raw_node. Returns `false` when
    /// lowering fails.
    fn raw_needs_member_route_materialization(
        ctx: &dyn ResolverContext,
        field_state: &mut MacroFieldGraphState<'_>,
        raw: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        let Some(node) = field_state.raw_node(raw) else {
            return false;
        };
        let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        type_node_needs_member_route_materialization(ctx, node, &mut local_fence, 0)
    }

    fn route_leaf_beats_wrapper_object(
        candidate: &verter_semantic::analysis::type_expr::TypeExpr,
        current: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        let TypeExpr::Object(object) = current else {
            return false;
        };
        let [ObjectMember::Property(property)] = object.properties.as_slice() else {
            return false;
        };
        property.ty == *candidate
    }

    fn type_expr_contains_named_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::RecursiveRef { name, .. } => name.as_ref() == target_name,
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => type_expr_contains_named_recursive_ref(inner, target_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| type_expr_contains_named_recursive_ref(&element.ty, target_name)),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| type_expr_contains_named_recursive_ref(ty, target_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                ObjectMember::Property(property) => {
                    type_expr_contains_named_recursive_ref(&property.ty, target_name)
                }
                ObjectMember::Method(method) => {
                    method.function.parameters.iter().any(|parameter| {
                        type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                    }) || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| {
                            type_expr_contains_named_recursive_ref(return_type, target_name)
                        })
                }
                ObjectMember::IndexSignature(signature) => {
                    type_expr_contains_named_recursive_ref(&signature.key_type, target_name)
                        || type_expr_contains_named_recursive_ref(
                            &signature.value_type,
                            target_name,
                        )
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function.parameters.iter().any(|parameter| {
                        type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        type_expr_contains_named_recursive_ref(return_type, target_name)
                    })
                }
            }),
            TypeExpr::Function(function) => {
                function.parameters.iter().any(|parameter| {
                    type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    type_expr_contains_named_recursive_ref(return_type, target_name)
                })
            }
            TypeExpr::IndexedAccess { object, index } => {
                type_expr_contains_named_recursive_ref(object, target_name)
                    || type_expr_contains_named_recursive_ref(index, target_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                type_expr_contains_named_recursive_ref(check, target_name)
                    || type_expr_contains_named_recursive_ref(extends, target_name)
                    || type_expr_contains_named_recursive_ref(true_type, target_name)
                    || type_expr_contains_named_recursive_ref(false_type, target_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                type_expr_contains_named_recursive_ref(source, target_name)
                    || type_expr_contains_named_recursive_ref(value, target_name)
                    || name_type.as_deref().is_some_and(|name_type| {
                        type_expr_contains_named_recursive_ref(name_type, target_name)
                    })
            }
            TypeExpr::TemplateLiteral { expressions, .. } => expressions
                .iter()
                .any(|expr| type_expr_contains_named_recursive_ref(expr, target_name)),
            TypeExpr::Ref { type_arguments, .. } => type_arguments
                .iter()
                .any(|arg| type_expr_contains_named_recursive_ref(arg, target_name)),
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => false,
        }
    }

    /// Issue #9 — recognises a replacement body whose top-level shape
    /// is `Pick<…>` or `Omit<…>` and whose type-argument tree mentions
    /// `RecursiveRef { name == target_name }`. The combination is
    /// irreducible: `Pick`/`Omit` builtins re-derive their result by
    /// re-substituting the target into themselves, so any single-layer
    /// expansion produces another `Pick`/`Omit` of the same target.
    /// The materializer publishes a `semanticMiss` sentinel for these
    /// shapes rather than expanding into a nested cycle.
    fn pick_or_omit_with_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> bool {
        use verter_semantic::analysis::type_expr::TypeExpr;
        match expr {
            TypeExpr::Parenthesized(inner) => pick_or_omit_with_recursive_ref(inner, target_name),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == "Pick" || name.as_ref() == "Omit" => type_arguments
                .iter()
                .any(|arg| type_expr_contains_named_recursive_ref(arg, target_name)),
            _ => false,
        }
    }

    fn expand_named_recursive_refs_one_layer(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
        replacement: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use std::sync::Arc;
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::RecursiveRef { name, .. } if name.as_ref() == target_name => {
                replacement.clone()
            }
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(expand_named_recursive_refs_one_layer(
                    element,
                    target_name,
                    replacement,
                )),
                readonly: *readonly,
            },
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: expand_named_recursive_refs_one_layer(
                                &element.ty,
                                target_name,
                                replacement,
                            ),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect(),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(
                types
                    .iter()
                    .map(|ty| expand_named_recursive_refs_one_layer(ty, target_name, replacement))
                    .collect(),
            ),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(
                types
                    .iter()
                    .map(|ty| expand_named_recursive_refs_one_layer(ty, target_name, replacement))
                    .collect(),
            ),
            TypeExpr::Object(object) => {
                let mut next = object.as_ref().clone();
                for member in &mut next.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty = expand_named_recursive_refs_one_layer(
                                &property.ty,
                                target_name,
                                replacement,
                            );
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = expand_named_recursive_refs_one_layer(
                                    &parameter.ty,
                                    target_name,
                                    replacement,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                                    return_type,
                                    target_name,
                                    replacement,
                                ));
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = expand_named_recursive_refs_one_layer(
                                &signature.key_type,
                                target_name,
                                replacement,
                            );
                            signature.value_type = expand_named_recursive_refs_one_layer(
                                &signature.value_type,
                                target_name,
                                replacement,
                            );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = expand_named_recursive_refs_one_layer(
                                    &parameter.ty,
                                    target_name,
                                    replacement,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                                    return_type,
                                    target_name,
                                    replacement,
                                ));
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(next))
            }
            TypeExpr::Function(function) => {
                let mut next = function.as_ref().clone();
                for parameter in &mut next.parameters {
                    parameter.ty = expand_named_recursive_refs_one_layer(
                        &parameter.ty,
                        target_name,
                        replacement,
                    );
                }
                if let Some(return_type) = next.return_type.as_mut() {
                    *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                        return_type,
                        target_name,
                        replacement,
                    ));
                }
                TypeExpr::Function(Arc::new(next))
            }
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(expand_named_recursive_refs_one_layer(
                    object,
                    target_name,
                    replacement,
                )),
                index: Arc::new(expand_named_recursive_refs_one_layer(
                    index,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(expand_named_recursive_refs_one_layer(
                    check,
                    target_name,
                    replacement,
                )),
                extends: Arc::new(expand_named_recursive_refs_one_layer(
                    extends,
                    target_name,
                    replacement,
                )),
                true_type: Arc::new(expand_named_recursive_refs_one_layer(
                    true_type,
                    target_name,
                    replacement,
                )),
                false_type: Arc::new(expand_named_recursive_refs_one_layer(
                    false_type,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                optional,
                readonly,
                name_type,
                value,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(expand_named_recursive_refs_one_layer(
                    source,
                    target_name,
                    replacement,
                )),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(|name_type| {
                    Arc::new(expand_named_recursive_refs_one_layer(
                        name_type,
                        target_name,
                        replacement,
                    ))
                }),
                value: Arc::new(expand_named_recursive_refs_one_layer(
                    value,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: expressions
                    .iter()
                    .map(|expr| {
                        expand_named_recursive_refs_one_layer(expr, target_name, replacement)
                    })
                    .collect(),
            },
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                // Ref does not name the recursive target by definition
                // (the `RecursiveRef` arm at the top would have matched
                // first); we still recurse into the ref's type
                // arguments so a `Pick<RecursiveRef<R>, …>` shape
                // expands the inner backedge correctly.
                if type_arguments.is_empty() {
                    return expr.clone();
                }
                let mut next = Vec::with_capacity(type_arguments.len());
                for arg in type_arguments.iter() {
                    next.push(expand_named_recursive_refs_one_layer(
                        arg,
                        target_name,
                        replacement,
                    ));
                }
                TypeExpr::Ref {
                    name: name.clone(),
                    type_arguments: Arc::from(next),
                }
            }
            TypeExpr::RecursiveRef { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => expr.clone(),
        }
    }

    fn rewrite_named_self_refs_to_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use std::sync::Arc;
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == target_name && type_arguments.is_empty() => {
                TypeExpr::recursive_ref(target_name, Vec::new())
            }
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    element,
                    target_name,
                )),
                readonly: *readonly,
            },
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: rewrite_named_self_refs_to_recursive_ref(&element.ty, target_name),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect(),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(
                types
                    .iter()
                    .map(|ty| rewrite_named_self_refs_to_recursive_ref(ty, target_name))
                    .collect(),
            ),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(
                types
                    .iter()
                    .map(|ty| rewrite_named_self_refs_to_recursive_ref(ty, target_name))
                    .collect(),
            ),
            TypeExpr::Object(object) => {
                let mut next = object.as_ref().clone();
                for member in &mut next.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty =
                                rewrite_named_self_refs_to_recursive_ref(&property.ty, target_name);
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = rewrite_named_self_refs_to_recursive_ref(
                                    &parameter.ty,
                                    target_name,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                                    return_type,
                                    target_name,
                                ));
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = rewrite_named_self_refs_to_recursive_ref(
                                &signature.key_type,
                                target_name,
                            );
                            signature.value_type = rewrite_named_self_refs_to_recursive_ref(
                                &signature.value_type,
                                target_name,
                            );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = rewrite_named_self_refs_to_recursive_ref(
                                    &parameter.ty,
                                    target_name,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                                    return_type,
                                    target_name,
                                ));
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(next))
            }
            TypeExpr::Function(function) => {
                let mut next = function.as_ref().clone();
                for parameter in &mut next.parameters {
                    parameter.ty =
                        rewrite_named_self_refs_to_recursive_ref(&parameter.ty, target_name);
                }
                if let Some(return_type) = next.return_type.as_mut() {
                    *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                        return_type,
                        target_name,
                    ));
                }
                TypeExpr::Function(Arc::new(next))
            }
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    object,
                    target_name,
                )),
                index: Arc::new(rewrite_named_self_refs_to_recursive_ref(index, target_name)),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(rewrite_named_self_refs_to_recursive_ref(check, target_name)),
                extends: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    extends,
                    target_name,
                )),
                true_type: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    true_type,
                    target_name,
                )),
                false_type: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    false_type,
                    target_name,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    source,
                    target_name,
                )),
                value: Arc::new(rewrite_named_self_refs_to_recursive_ref(value, target_name)),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(|name_type| {
                    Arc::new(rewrite_named_self_refs_to_recursive_ref(
                        name_type,
                        target_name,
                    ))
                }),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: expressions
                    .iter()
                    .map(|expr| rewrite_named_self_refs_to_recursive_ref(expr, target_name))
                    .collect(),
            },
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                // The empty-type-arguments self-ref case is handled by
                // the dedicated arm above. Here we either:
                // * rewrite a self-ref *with* type arguments to a
                //   parameterized `RecursiveRef` (preserves
                //   `Pick<R<T>, 'x'>`-style shapes when R is the
                //   recursive target), or
                // * descend into the type arguments to surface
                //   self-refs nested inside other generics
                //   (`Pick<R, 'x'>` where R is the target — the inner
                //   R gets rewritten to `RecursiveRef`).
                let mut next = Vec::with_capacity(type_arguments.len());
                for arg in type_arguments.iter() {
                    next.push(rewrite_named_self_refs_to_recursive_ref(arg, target_name));
                }
                if name.as_ref() == target_name {
                    TypeExpr::recursive_ref(target_name, next)
                } else if type_arguments.is_empty() {
                    expr.clone()
                } else {
                    TypeExpr::Ref {
                        name: name.clone(),
                        type_arguments: Arc::from(next),
                    }
                }
            }
            TypeExpr::RecursiveRef { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => expr.clone(),
        }
    }

    fn indexed_access_alias_body_transport(
        scope_canonical_id: &str,
        raw: &verter_semantic::analysis::type_expr::TypeExpr,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        let (root_symbol, route) = component_meta_registry_public_indexed_access_route(raw)?;
        let crate::resolver_core::RouteDemand::MemberPath(path) = route else {
            return None;
        };
        let [member_name] = path.as_slice() else {
            return None;
        };

        let member_ty = query_engine.prepared_member_raw_type(
            scope_canonical_id,
            root_symbol.as_str(),
            member_name,
        )?;
        let verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } = member_ty
        else {
            return None;
        };
        if !type_arguments.is_empty() {
            return None;
        }

        let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
        let declaration_scope = if declaration.canonical_source.is_empty() {
            scope_canonical_id.to_string()
        } else {
            declaration.canonical_source
        };
        let declaration_name = if declaration.resolved_name.is_empty() {
            name.as_ref().to_string()
        } else {
            declaration.resolved_name
        };
        let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
            declaration_scope.as_str(),
            declaration_name.as_str(),
        );
        let body = query_engine.named_decl_body(target_scope.as_str(), target_name.as_str())?;
        let replacement = rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str());
        matches!(
            replacement,
            verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
        )
        .then_some(replacement)
    }

    // `slot_binding_scope_hints` carries `(slot.binding) -> declaration scope`
    // pairs populated from `resolved.declaration.canonical_source` for
    // every cross-file `defineSlots` macro binding. Read by the slot-binding
    // rescue loop below to skip rescue when the binding originated from an
    // imported helper type body.
    let mut slot_binding_scope_hints = rustc_hash::FxHashMap::<String, Vec<String>>::default();
    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }
        if mac.kind != verter_semantic::analysis::AnalyzedMacroKind::DefineSlots {
            continue;
        }
        for resolved in resolved_macros.iter().filter(|resolved| {
            resolved.macro_index == macro_index
                && resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
        }) {
            let declaration_scope = resolved.declaration.canonical_source.as_str();
            if declaration_scope.is_empty() {
                continue;
            }
            for slot in &resolved.slots {
                for binding in &slot.bindings {
                    let entry = slot_binding_scope_hints
                        .entry(format!("{}.{}", slot.name, binding.name))
                        .or_default();
                    if !entry.iter().any(|scope| scope == declaration_scope) {
                        entry.push(declaration_scope.to_string());
                    }
                }
            }
        }
    }

    for field in &mut evaluated_types.props {
        // Loop-9 block 2: preserve_raw + early-out predicates.
        // Returns `true` to signal "skip the rest of this iteration"
        // (the legacy `continue` semantics). The TimerGuard captures
        // wall-clock for predicate evaluation regardless of which
        // branch fires.
        let early_out = {
            let _t = crate::loop5_instrumentation::TimerGuard::new(
                &crate::loop5_instrumentation::FIELD_PROPS_PRESERVE_AND_EARLY_OUTS_CALLS,
                &crate::loop5_instrumentation::FIELD_PROPS_PRESERVE_AND_EARLY_OUTS_NS,
            );
            let preserve_raw = field_should_preserve_shallow_symbolic_raw_type(
                scope_canonical_id,
                field,
                query_engine,
            );
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "FIELD_MATERIALIZE owner={} field={} raw={:?} current={:?} preserve_raw={}",
                    scope_canonical_id, field.name, field.raw_type, field.r#type, preserve_raw,
                ));
            }
            if preserve_raw {
                true
            } else if let Some(raw) = parsed_field_raw_type(field).as_ref() {
                // Issue #5 / early-out (PRE-rescue): indexed-access
                // route + terminal-scalar surface skips the rescue.
                if type_expr_is_indexed_access_route(raw)
                    && type_expr_is_terminal_scalar_surface(&field.r#type)
                    && raw_indexed_access_root_is_workspace_owned(
                        raw,
                        scope_canonical_id,
                        query_engine,
                    )
                {
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "FIELD_MATERIALIZE_INDEXED_TERMINAL_EARLY_OUT owner={} field={} raw={:?} published={:?}",
                            scope_canonical_id, field.name, raw, field.r#type,
                        ));
                    }
                    true
                } else if let super::component_config_fast_path::FastPathOutcome::Hit(candidate) =
                    super::component_config_fast_path::component_config_theme_variant_fast_path(
                        raw,
                        scope_canonical_id,
                        query_engine.ctx,
                    )
                {
                    // Issue #6 / ComponentConfig theme variant fast
                    // path: publish the projected value and skip.
                    field.r#type = candidate;
                    crate::capture_token::with_active_capture(|t| {
                        t.record_counter(
                            super::component_config_fast_path::COMPONENT_CONFIG_FAST_PATH_HITS_COUNTER,
                            1,
                        )
                    });
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "FIELD_MATERIALIZE_COMPONENT_CONFIG_FAST_PATH_HIT owner={} field={} raw={:?} published={:?}",
                            scope_canonical_id, field.name, raw, field.r#type,
                        ));
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if early_out {
            continue;
        }

        // Loop-9 block 3: MacroFieldGraphState wrap + define_props
        // candidate scan + rescue_field. No early-outs: timer is a
        // straightforward scope guard.
        let ctx = query_engine.ctx;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        {
            let _t = crate::loop5_instrumentation::TimerGuard::new(
                &crate::loop5_instrumentation::FIELD_PROPS_RESCUE_FIELD_CALLS,
                &crate::loop5_instrumentation::FIELD_PROPS_RESCUE_FIELD_NS,
            );
            if let Some(candidate) = evaluated_types
                .define_props
                .iter()
                .flat_map(|define_props| define_props.result.value.properties.iter())
                .find(|property| property.name == field.name)
                .map(|property| property.ty.clone())
            {
                if compare_type_expr_improvement(&candidate, field_state.published_type())
                    && !expr_needs_projection_rescue(query_engine, scope_canonical_id, &candidate)
                {
                    field_state.set_current_type(candidate);
                }
            }
            rescue_field(
                scope_canonical_id,
                field,
                &mut field_state,
                query_engine,
                crate::semantic_query::ProjectionMode::Expanded,
            );
        }

        // Loop-9 block 4: needs_member_route predicate checks +
        // post-rescue early-out path. Computes the early-out flag
        // inside a timed scope, then performs the publish/continue
        // OUTSIDE the timer (since `publish()` consumes the
        // field_state and we want the scope to drop the timer
        // first). Returns `(early_out, raw_needs_member_route)`.
        let (early_out_kind, raw_needs_member_route) = {
            let _t = crate::loop5_instrumentation::TimerGuard::new(
                &crate::loop5_instrumentation::FIELD_PROPS_NEEDS_MEMBER_ROUTE_CALLS,
                &crate::loop5_instrumentation::FIELD_PROPS_NEEDS_MEMBER_ROUTE_NS,
            );
            // Migrate predicate to graph-native J1 _node
            // version via field_state.raw_node().
            let raw_needs_member_route = parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                raw_needs_member_route_materialization(ctx, &mut field_state, raw)
                    || component_meta_registry_public_utility_route(raw).is_some()
            });
            let raw_is_unpreserved_top_level_ref =
                parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                    matches!(
                        raw,
                        verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
                            if type_arguments.is_empty()
                    )
                });
            if crate::host_manage::component_meta_debug_enabled() {
                let current_needs =
                    current_needs_member_route_materialization(ctx, &mut field_state);
                crate::host_manage::component_meta_debug(format!(
                    "FIELD_MATERIALIZE_POST_RESCUE owner={} field={} current={:?} raw_needs_member_route={} raw_is_unpreserved_top_level_ref={} current_needs_member_route={}",
                    scope_canonical_id,
                    field.name,
                    field_state.published_type(),
                    raw_needs_member_route,
                    raw_is_unpreserved_top_level_ref,
                    current_needs,
                ));
            }
            // 0 = no early-out; 1 = first early-out; 2 = slots-route.
            let early_out_kind: u8 = if !(raw_needs_member_route
                || raw_is_unpreserved_top_level_ref
                || current_needs_member_route_materialization(ctx, &mut field_state))
            {
                1
            } else if let Some(raw) = parsed_field_raw_type(field).as_ref() {
                if type_expr_is_slots_member_route(raw)
                    && type_expr_is_non_empty_object_surface(field_state.published_type())
                    && raw_indexed_access_root_is_workspace_owned(
                        raw,
                        scope_canonical_id,
                        query_engine,
                    )
                {
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "FIELD_MATERIALIZE_SLOTS_OBJECT_EARLY_OUT owner={} field={} raw={:?} published={:?}",
                            scope_canonical_id, field.name, raw, field_state.published_type(),
                        ));
                    }
                    2
                } else {
                    0
                }
            } else {
                0
            };
            (early_out_kind, raw_needs_member_route)
        };
        if early_out_kind != 0 {
            field.r#type = field_state.publish();
            continue;
        }

        // Loop-9 block 6: scope selection + 3-way routed-surface
        // candidate scan (`materialize_member_surface_expr` x3 +
        // `project_expr_class_a_via_dispatch` x2). Always increments
        // CALLS once; only does work when raw_needs_member_route
        // && !raw_route_root_is_package_backed.
        let materialize_scope_canonical_id = {
            let _t = crate::loop5_instrumentation::TimerGuard::new(
                &crate::loop5_instrumentation::FIELD_PROPS_ROUTED_SURFACE_CALLS,
                &crate::loop5_instrumentation::FIELD_PROPS_ROUTED_SURFACE_NS,
            );
            let materialize_scope_canonical_id = select_imported_materialization_scope(
                field_state.published_type(),
                scope_canonical_id,
                query_engine,
            )
            .or_else(|| {
                parsed_field_raw_type(field).as_ref().and_then(|raw| {
                    select_imported_materialization_scope(raw, scope_canonical_id, query_engine)
                })
            })
            .unwrap_or_else(|| scope_canonical_id.to_string());
            let raw_route_root_is_package_backed =
                parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                    type_expr_has_package_backed_object_like_root(
                        raw,
                        scope_canonical_id,
                        query_engine,
                    )
                });
            if raw_needs_member_route && !raw_route_root_is_package_backed {
                crate::capture_token::with_active_capture(|t| {
                    t.record_counter(MEMBER_ROUTE_CALLS_COUNTER, 1)
                });
                let routed_surface = query_engine.materialize_member_surface_expr(
                    materialize_scope_canonical_id.as_str(),
                    field_state.published_type(),
                    true,
                );
                if compare_type_expr_improvement(&routed_surface, field_state.published_type()) {
                    field_state.set_current_type(routed_surface);
                }
                if let Some(raw_route_surface) =
                    parsed_field_raw_type(field).as_ref().and_then(|raw| {
                        project_expr_class_a_via_dispatch(
                            query_engine.ctx,
                            materialize_scope_canonical_id.as_str(),
                            raw,
                        )
                    })
                {
                    let raw_route_surface = query_engine.materialize_member_surface_expr(
                        materialize_scope_canonical_id.as_str(),
                        &raw_route_surface,
                        true,
                    );
                    if compare_type_expr_improvement(
                        &raw_route_surface,
                        field_state.published_type(),
                    ) || route_leaf_beats_wrapper_object(
                        &raw_route_surface,
                        field_state.published_type(),
                    ) {
                        field_state.set_current_type(raw_route_surface);
                    }
                }
                if let Some(projected_route_surface) = project_expr_class_a_via_dispatch(
                    query_engine.ctx,
                    materialize_scope_canonical_id.as_str(),
                    field_state.published_type(),
                ) {
                    let projected_route_surface = query_engine.materialize_member_surface_expr(
                        materialize_scope_canonical_id.as_str(),
                        &projected_route_surface,
                        false,
                    );
                    if compare_type_expr_improvement(
                        &projected_route_surface,
                        field_state.published_type(),
                    ) || route_leaf_beats_wrapper_object(
                        &projected_route_surface,
                        field_state.published_type(),
                    ) {
                        field_state.set_current_type(projected_route_surface);
                    }
                }
            }
            materialize_scope_canonical_id
        };

        // Loop-9 block 7: the big match on
        // `field_state.published_type()` — bare-Ref → declaration
        // body lookup + bridge helper, OR
        // `materialize_component_meta_type_expr_until_stable`. This
        // is one of the most likely hot blocks; counter increments
        // once per iteration regardless of which arm fires.
        let _loop9_ref_rescue_timer = crate::loop5_instrumentation::TimerGuard::new(
            &crate::loop5_instrumentation::FIELD_PROPS_REF_RESCUE_MATCH_CALLS,
            &crate::loop5_instrumentation::FIELD_PROPS_REF_RESCUE_MATCH_NS,
        );
        let rescued = match field_state.published_type() {
            verter_semantic::analysis::type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let name = name.clone();
                let declaration = query_engine.resolve_type_declaration(
                    materialize_scope_canonical_id.as_str(),
                    name.as_ref(),
                );
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    materialize_scope_canonical_id.clone()
                } else {
                    declaration.canonical_source.clone()
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref().to_string()
                } else {
                    declaration.resolved_name.clone()
                };
                let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
                    declaration_scope.as_str(),
                    declaration_name.as_str(),
                );
                let rescued = query_engine
                    .named_decl_body(target_scope.as_str(), target_name.as_str())
                    .map(|body| {
                        rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str())
                    })
                    .or_else(|| {
                        // (): the Class B
                        // migration completed by routing through the
                        // bridge helper `project_type_surface_expr_via_host_threaded`.
                        // The §5.14.1 pre-flight gate observes zero
                        // external engine-method callers; the bridge
                        // is the single production callsite shape.
                        // The semantic target is
                        // `dispatch.execute(Instantiate { args: [],
                        // body_mode: Expanded })` per sub-
                        project_type_surface_expr_via_host_threaded(
                            query_engine,
                            target_scope.as_str(),
                            target_name.as_str(),
                        )
                    })
                    .unwrap_or_else(|| {
                        materialize_component_meta_type_expr_until_stable(
                            field_state.published_type(),
                            materialize_scope_canonical_id.as_str(),
                            crate::semantic_query::ProjectionMode::Expanded,
                            query_engine,
                        )
                    });
                if crate::host_manage::component_meta_debug_enabled() {
                    crate::host_manage::component_meta_debug(format!(
                        "FIELD_MATERIALIZE_REF owner={} field={} current_ref={} materialize_scope={} target_scope={} target_name={} rescued={:?}",
                        scope_canonical_id,
                        field.name,
                        name,
                        materialize_scope_canonical_id,
                        target_scope,
                        target_name,
                        rescued,
                    ));
                }
                rescued
            }
            _ => materialize_component_meta_type_expr_until_stable(
                field_state.published_type(),
                materialize_scope_canonical_id.as_str(),
                crate::semantic_query::ProjectionMode::Expanded,
                query_engine,
            ),
        };
        if compare_type_expr_improvement(&rescued, field_state.published_type()) {
            field_state.set_current_type(rescued);
        }
        drop(_loop9_ref_rescue_timer);

        // Loop-9 block 8: raw-ref body lookup + recursive-ref
        // expansion + `indexed_access_alias_body_transport`.
        let _loop9_raw_ref_transport_timer = crate::loop5_instrumentation::TimerGuard::new(
            &crate::loop5_instrumentation::FIELD_PROPS_RAW_REF_TRANSPORT_CALLS,
            &crate::loop5_instrumentation::FIELD_PROPS_RAW_REF_TRANSPORT_NS,
        );
        // Track whether the raw-ref branch handled the field (legacy
        // `continue` semantics). Set TRUE when the legacy code would
        // have `continue`d before the final indexed-access transport
        // path. We still must `publish()` after `continue`; using a
        // local bool lets us re-route through publish().
        let mut raw_ref_branch_handled = false;
        if let Some(verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        }) = parsed_field_raw_type(field)
        {
            if type_arguments.is_empty() {
                let declaration =
                    query_engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    declaration.canonical_source
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref().to_string()
                } else {
                    declaration.resolved_name
                };
                let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
                    declaration_scope.as_str(),
                    declaration_name.as_str(),
                );
                if let Some(body) =
                    query_engine.named_decl_body(target_scope.as_str(), target_name.as_str())
                {
                    let replacement =
                        rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str());
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "FIELD_RAW_REF_BODY owner={} field={} target_scope={} target_name={} body={:?} replacement={:?}",
                            scope_canonical_id,
                            field.name,
                            target_scope,
                            target_name,
                            body,
                            replacement,
                        ));
                    }
                    // Issue #9 guard: a body whose top-level shape is
                    // `Pick<…>` or `Omit<…>` and whose recursion arms
                    // mention `RecursiveRef { name == target_name }` is
                    // an irreducible cycle — chasing further would
                    // recurse without bound. Publish a `semanticMiss`
                    // sentinel keyed on the resolved declaration
                    // identity so the outer macro keeps a discoverable
                    // shape while the recursive arm is signalled as
                    // unresolved.
                    if pick_or_omit_with_recursive_ref(&replacement, target_name.as_str()) {
                        field_state.set_current_type(
                            verter_semantic::analysis::type_expr::TypeExpr::Unknown {
                                raw: String::from("semanticMiss"),
                            },
                        );
                        raw_ref_branch_handled = true;
                    } else if matches!(
                        replacement,
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) && !matches!(
                        field_state.published_type(),
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) {
                        field_state.set_current_type(replacement);
                        raw_ref_branch_handled = true;
                    } else if matches!(
                        field_state.published_type(),
                        verter_semantic::analysis::type_expr::TypeExpr::Ref {
                            name: ref_name,
                            type_arguments,
                        } if type_arguments.is_empty()
                            && ref_name.as_ref() == target_name.as_str()
                    ) {
                        // When the field type is a bare Ref to the target
                        // type, apply the body replacement directly
                        // (initial expansion).
                        field_state.set_current_type(replacement);
                        raw_ref_branch_handled = true;
                    } else if type_expr_contains_named_recursive_ref(
                        field_state.published_type(),
                        target_name.as_str(),
                    ) {
                        let expanded = expand_named_recursive_refs_one_layer(
                            field_state.published_type(),
                            target_name.as_str(),
                            &replacement,
                        );
                        if expanded != *field_state.published_type() {
                            field_state.set_current_type(expanded);
                        }
                    } else {
                        // The define_props macro shape projection may have
                        // expanded the top-level type one layer (producing an
                        // Object body) while leaving nested self-references
                        // as bare `Ref{target}` instead of `RecursiveRef`.
                        // Rewrite those bare self-refs to `RecursiveRef` and
                        // expand one more layer so the transport carries the
                        // same two-level shape the RecursiveRef path would
                        // produce.
                        let with_recursive_refs = rewrite_named_self_refs_to_recursive_ref(
                            field_state.published_type(),
                            target_name.as_str(),
                        );
                        if with_recursive_refs != *field_state.published_type() {
                            let expanded = expand_named_recursive_refs_one_layer(
                                &with_recursive_refs,
                                target_name.as_str(),
                                &replacement,
                            );
                            if expanded != *field_state.published_type() {
                                field_state.set_current_type(expanded);
                            }
                        }
                    }
                }
            }
        }
        if !raw_ref_branch_handled {
            if let Some(raw) = parsed_field_raw_type(field).as_ref() {
                if let Some(replacement) =
                    indexed_access_alias_body_transport(scope_canonical_id, raw, query_engine)
                {
                    if !matches!(
                        field_state.published_type(),
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) {
                        field_state.set_current_type(replacement);
                    }
                }
            }
        }
        // Final publish + write-back to the field.
        field.r#type = field_state.publish();
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE_FINAL owner={} field={} final={:?}",
                scope_canonical_id, field.name, field.r#type,
            ));
        }
    }
    // Loop-9 block 9: define_props sync + emits + slot_bindings +
    // bindings final loops. Combined into a single timer because
    // each tail loop is small relative to the per-prop main loop;
    // CALLS increments once per request.
    let _loop9_tail_loops = crate::loop5_instrumentation::TimerGuard::new(
        &crate::loop5_instrumentation::FIELD_TAIL_LOOPS_CALLS,
        &crate::loop5_instrumentation::FIELD_TAIL_LOOPS_NS,
    );
    let finalized_prop_types = evaluated_types
        .props
        .iter()
        .map(|field| (field.name.clone(), field.r#type.clone()))
        .collect::<rustc_hash::FxHashMap<_, _>>();
    for define_props in &mut evaluated_types.define_props {
        for property in &mut define_props.result.value.properties {
            if let Some(finalized) = finalized_prop_types.get(&property.name) {
                property.ty = finalized.clone();
            }
        }
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_DEFINE_PROPS_SYNC owner={} props={:?}",
                scope_canonical_id,
                define_props
                    .result
                    .value
                    .properties
                    .iter()
                    .map(|property| (property.name.clone(), property.ty.clone()))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    if std::env::var("VERTER_PROGRESS_STREAM").is_ok() {
        eprintln!(
            "[PROPS_DONE] owner={} elapsed_ms_total={:.1}",
            scope_canonical_id,
            _ft_t0.elapsed().as_secs_f64() * 1000.0
        );
    }
    if std::env::var("VERTER_PROGRESS_STREAM").is_ok() {
        eprintln!(
            "[EMITS_ENTER] owner={} count={}",
            scope_canonical_id,
            evaluated_types.emits.len()
        );
    }
    for field in &mut evaluated_types.emits {
        // Wrap field.r#type in MacroFieldGraphState.
        let ctx = query_engine.ctx;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        rescue_field(
            scope_canonical_id,
            field,
            &mut field_state,
            query_engine,
            crate::semantic_query::ProjectionMode::Expanded,
        );
        field.r#type = field_state.publish();
    }
    let _slot_bindings_t0 = Instant::now();
    let _trace_sb = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
    if _trace_sb {
        eprintln!(
            "[SLOTS_ENTER] owner={} count={}",
            scope_canonical_id,
            evaluated_types.slot_bindings.len()
        );
    }
    for (_sb_idx, field) in evaluated_types.slot_bindings.iter_mut().enumerate() {
        if _trace_sb {
            eprintln!(
                "[SB_OUTER_START] idx={} name={} elapsed_ms={:.1}",
                _sb_idx,
                field.name,
                _slot_bindings_t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let _sb_outer_t0 = Instant::now();
        // Wrap field.r#type in MacroFieldGraphState.
        let ctx = query_engine.ctx;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        if _trace_sb {
            let preview = format!("{:?}", field.r#type);
            let preview = if preview.len() > 600 {
                format!("{}... [{} chars total]", &preview[..600], preview.len())
            } else {
                preview
            };
            eprintln!(
                "[SB_RESCUE_START] idx={} name={} mode=Navigate type={}",
                _sb_idx, field.name, preview
            );
        }
        let _rescue_t0 = Instant::now();
        // Slot-binding rescue uses Navigate mode (path-precise) per
        // CLAUDE.md macro-traversal rule. The published_type for a
        // slot_binding is typically `IndexedAccess { Ref<X<...>>, k }` —
        // the IndexedAccess `[k]` IS the terminal projection. Expanded
        // mode would walk the entire generic instantiation including
        // third-party heritage (e.g., `extends UIMessage<...>` from the
        // `ai` package), producing minutes of fan-out per binding.
        //
        // Skip rescue when this slot binding originated from an
        // imported helper type body (a `defineSlots<X>()` whose `X`
        // is declared in a different file than the owner SFC) and
        // the graph-native synthesis already published an
        // exact-symbolic form. The synthesizer is the authority for
        // symbolic shapes that cross the import boundary
        // (`Button['ui']` from `ButtonSlots` imported from
        // `./button-types`, `Conditional` / `Mapped` on imported
        // helpers); the rescue path would re-expand these through
        // `project_expr_surface_expr` and erase the source-text
        // identity downstream consumers re-resolve from. Per the
        // slot-binding contract: imported helper member-paths stay
        // lazy; consumers navigate them via dispatch when they need
        // the resolved members.
        //
        // The `slot_binding_scope_hints` map carries
        // `(slot.binding) -> declaration scope` pairs populated
        // upstream from `resolved.declaration.canonical_source` for
        // every cross-file `defineSlots` / `defineEmits` macro
        // binding. A non-empty hint set whose entries all differ
        // from the owner canonical means "this binding originated
        // from an imported type body".
        //
        // Local helpers (test
        // `public_component_meta_materializes_local_component_config_*`)
        // have either no scope hints (the slot type is declared in
        // the owner SFC's own `<script>` block) or hints that point
        // back to the owner's own canonical id, so they still route
        // through the rescue path and materialise into concrete
        // object surfaces.
        let synthesis_published_symbolic = matches!(
            field.exactness,
            verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
        );
        let root_is_imported_helper = synthesis_published_symbolic
            && slot_binding_scope_hints
                .get(&field.name)
                .is_some_and(|hints| {
                    !hints.is_empty()
                        && hints
                            .iter()
                            .all(|scope| scope.as_str() != scope_canonical_id)
                });
        if !root_is_imported_helper {
            rescue_field(
                scope_canonical_id,
                field,
                &mut field_state,
                query_engine,
                crate::semantic_query::ProjectionMode::Navigate,
            );
        }
        if _trace_sb {
            eprintln!(
                "[SB_RESCUE_END] idx={} name={} rescue_ms={:.1}",
                _sb_idx,
                field.name,
                _rescue_t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        // Imported-helper symbolic shapes skip the scope-hint
        // materialisation loop. The graph-native synthesizer is the
        // authority for symbolic forms that cross the import boundary
        // (Conditional / IndexedAccess / Mapped on imported helper
        // routes); the scope-hint loop would route the symbolic shape
        // through `materialize_component_meta_type_expr_until_stable`
        // and re-materialise the helper away. Local helpers still
        // route through the loop so their bindings materialise into
        // concrete object surfaces.
        if root_is_imported_helper {
            field.r#type = field_state.publish();
            continue;
        }
        let Some(scope_hints) = slot_binding_scope_hints.get(&field.name) else {
            field.r#type = field_state.publish();
            continue;
        };
        for scope_hint in scope_hints {
            let parsed_raw = field
                .raw_type
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
            for (cand_idx, candidate) in [
                Some(field_state.published_type().clone()),
                parsed_raw.clone(),
                parsed_raw.as_ref().and_then(|raw| {
                    project_expr_class_a_via_dispatch(query_engine.ctx, scope_hint, raw)
                }),
                project_expr_class_a_via_dispatch(
                    query_engine.ctx,
                    scope_hint,
                    field_state.published_type(),
                ),
            ]
            .into_iter()
            .flatten()
            .enumerate()
            {
                let trace = std::env::var("VERTER_PROGRESS_STREAM").is_ok();
                if trace {
                    eprintln!(
                        "[SB_START] field={} scope={} cand={} START until_stable",
                        field.name, scope_hint, cand_idx
                    );
                }
                let t0 = Instant::now();
                let rescued = materialize_component_meta_type_expr_until_stable(
                    &candidate,
                    scope_hint,
                    crate::semantic_query::ProjectionMode::Navigate,
                    query_engine,
                );
                if trace {
                    eprintln!(
                        "[SB_END] field={} scope={} cand={} until_stable_ms={:.1}",
                        field.name,
                        scope_hint,
                        cand_idx,
                        t0.elapsed().as_secs_f64() * 1000.0
                    );
                }
                if compare_type_expr_improvement(&rescued, field_state.published_type()) {
                    field_state.set_current_type(rescued);
                }
                if trace {
                    eprintln!(
                        "[SB_START] field={} scope={} cand={} START member_surface",
                        field.name, scope_hint, cand_idx
                    );
                }
                let t1 = Instant::now();
                let surface =
                    query_engine.materialize_member_surface_expr(scope_hint, &candidate, false);
                if trace {
                    eprintln!(
                        "[SB_END] field={} scope={} cand={} member_surface_ms={:.1}",
                        field.name,
                        scope_hint,
                        cand_idx,
                        t1.elapsed().as_secs_f64() * 1000.0
                    );
                }
                if compare_type_expr_improvement(&surface, field_state.published_type()) {
                    field_state.set_current_type(surface);
                }
            }
        }
        field.r#type = field_state.publish();
        if _trace_sb {
            eprintln!(
                "[SB_OUTER_END] idx={} name={} outer_ms={:.1}",
                _sb_idx,
                field.name,
                _sb_outer_t0.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    if _trace_sb {
        eprintln!(
            "[SLOTS_EXIT] owner={} total_ms={:.1}",
            scope_canonical_id,
            _slot_bindings_t0.elapsed().as_secs_f64() * 1000.0
        );
    }
    for field in &mut evaluated_types.bindings {
        // Wrap field.r#type in MacroFieldGraphState.
        let ctx = query_engine.ctx;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        rescue_field(
            scope_canonical_id,
            field,
            &mut field_state,
            query_engine,
            crate::semantic_query::ProjectionMode::Expanded,
        );
        field.r#type = field_state.publish();
    }
}
/// Test-only call counter for `materialize_component_meta_type_expr_until_stable`.
/// Incremented at function entry — memo hits and cold builds both
/// count, since the counter discriminates the *entry* invariant: did
/// the caller route through whole-expression materialization at all?
#[cfg(test)]
pub(crate) static MTL_CALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Test accessor for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn mtl_call_count_for_tests() -> usize {
    MTL_CALL_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Test reset for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_mtl_call_count_for_tests() {
    MTL_CALL_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
}
