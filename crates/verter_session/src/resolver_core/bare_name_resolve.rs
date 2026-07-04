//! Host-owned bare-name root-identity resolution for the dispatch path.
//!
//! This module provides the free-function equivalent of the
//! `SessionSolverHost::root_identity` logic, extracted so the project
//! semantic dispatcher can resolve a bare `TypeExpr::Ref` to a stable
//! `(canonical_id, symbol_name)` pair without constructing a
//! `SessionSolverHost`. It reads directly from the ctx's cached
//! shallow-file-state, prepared-decl bundles, and resolver stack —
//! the same substrate `SessionSolverHost` wraps.
//!
//! Authority chain:
//! 1. Declaration-scope payload (prepared-decl bundle's script-setup type
//!    bindings, scope type/value names, import bindings).
//! 2. Host's cached `IndexedReady` shallow state for the scope's canonical.
//! 3. Import-target + barrel/re-export walk via
//!    `ResolverContext::resolve_imported_type_root`.
//! 4. Namespace-qualified `Ns.Member` dereference through the prefix's
//!    import binding.
//! 5. Public export-target resolution via
//!    `ResolverContext::resolve_named_type_export_target` /
//!    `resolve_value_export_target`.
//!
//! `DeclarationScopePayload` lives here (not in `solver_host.rs`) so it
//! survives the §5.8 deletion of `solver_host.rs`; the dispatch path is
//! the long-lived consumer.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_semantic::analysis::type_solver::PreparedTypeDecl;

use super::prepared_decl::{ImportBinding, TypeParamBinding};
use crate::resolver_core::ResolverContext;

/// Declaration-scope context used by bare-name root-identity resolution.
///
/// Derived from a [`PreparedDeclBundle`](super::prepared_decl::PreparedDeclBundle)
/// — carries the scope-local type/value names + script-setup type
/// parameter bindings + import bindings in a compact form the
/// resolver can consult without re-walking the bundle.
///
/// `scope_type_bindings` is keyed to [`TypeParamBinding`], NOT to
/// `Arc<PreparedTypeDecl>`; script-setup generic parameters carry
/// their declaration-site `extends` / default expressions directly
/// without an intermediate prepared-decl wrapper.
#[derive(Debug)]
pub(crate) struct DeclarationScopePayload {
    pub(crate) scope_type_names: FxHashSet<String>,
    pub(crate) scope_value_names: FxHashSet<String>,
    pub(crate) scope_type_bindings: FxHashMap<String, TypeParamBinding>,
    pub(crate) import_bindings: FxHashMap<String, ImportBinding>,
}

impl DeclarationScopePayload {
    pub(crate) fn from_bundle(
        bundle: &crate::resolver_core::prepared_decl::PreparedDeclBundle,
    ) -> Self {
        // Include script-setup generic param names in type_names so the
        // resolver recognises them as in-scope.
        let mut scope_type_names = bundle.scope_type_names.clone();
        for param_name in bundle.script_setup_type_bindings.keys() {
            scope_type_names.insert(param_name.clone());
        }

        Self {
            scope_type_names,
            scope_value_names: bundle.scope_value_names.clone(),
            scope_type_bindings: bundle.script_setup_type_bindings.clone(),
            import_bindings: bundle.import_bindings.clone(),
        }
    }
}

/// Resolve a bare identifier to a `(canonical_id, symbol_name)` root
/// identity within `scope_canonical_id`.
///
/// Mirrors `SessionSolverHost::root_identity`:
/// 1. If the name lives in the scope's declaration-scope payload
///    (script-setup type param, scope-local type/value), it resolves to
///    the scope's own canonical id.
/// 2. Else fall back to the scope's cached `IndexedReady` — local
///    symbols, locally exported symbols, import targets from the
///    shallow state.
/// 3. Else walk prefix imports for namespace-qualified names.
/// 4. Else ask the ctx's export-target resolvers.
///
/// Returns `None` when the name cannot be located through any
/// ctx-owned state.
pub(crate) fn resolve_bare_name_in_scope(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    scope_payload: Option<&DeclarationScopePayload>,
    name: &str,
) -> Option<ResolvedRootIdentity> {
    // 1. Declaration-scope payload lookup (scope-local type/value,
    //    script-setup type bindings).
    if let Some(payload) = scope_payload {
        if payload.scope_type_bindings.contains_key(name)
            || payload.scope_type_names.contains(name)
            || payload.scope_value_names.contains(name)
        {
            return Some(ResolvedRootIdentity::new(scope_canonical_id, name));
        }
    }

    if scope_canonical_id.is_empty() {
        return None;
    }

    // 2. Scope's cached IndexedReady — local symbols + local exports.
    // Structurally read-only: the resolved identity feeds enclosing
    // builds whose admission gates consume the fenced-serve chokepoint
    // flag, so the serve status is not re-checked here.
    if let Some(entry) = ctx
        .ensure_indexed_ready_serve(scope_canonical_id)
        .map(|serve| serve.indexed)
    {
        if symbol_exists_in_facts(&entry, name) {
            return Some(ResolvedRootIdentity::new(scope_canonical_id, name));
        }
        if matches!(
            entry.shallow_state.export_target(name),
            Some(crate::resolver_core::ExportTarget::Local { .. })
        ) {
            return Some(ResolvedRootIdentity::new(scope_canonical_id, name));
        }
        // A `declare global { interface Name { ... } }` declaration in this
        // file is not a file-surface symbol, but the name resolves to the
        // merged global declaration. The prepared-decl builder + `ResolveDecl`
        // both fall back to the global augmentation inventory under the same
        // `(canonical, name)` identity.
        if entry.shallow_state.has_global_augmentation(name) {
            return Some(ResolvedRootIdentity::new(scope_canonical_id, name));
        }
    }

    // 3. Import-target walk (shallow state + prepared-bundle bindings).
    if let Some(resolved) =
        resolve_import_binding_from_facts(ctx, scope_canonical_id, scope_payload, name)
    {
        return Some(resolved);
    }

    // 4. Namespace-qualified: `Ns.Member`.
    if let Some(resolved) =
        resolve_namespace_member_from_facts(ctx, scope_canonical_id, scope_payload, name)
    {
        return Some(resolved);
    }

    // 5. Cross-owner export target.
    if let Some((canonical_id, exported_name)) =
        ctx.resolve_named_type_export_target(scope_canonical_id, name)
    {
        return Some(ResolvedRootIdentity::new(&canonical_id, &exported_name));
    }

    None
}

fn symbol_exists_in_facts(
    entry: &crate::project_type_store::IndexedReady,
    symbol_name: &str,
) -> bool {
    // Local PRESENCE through the CENTRALIZED effective header lookup so a rune
    // module's ambient `$state`/`$derived`/… (value or type) resolves at the
    // bare-name surface without this site knowing about the rune prelude. A
    // plain `.ts` is unaffected (the effective lookup is rune-module-gated and
    // reduces to the header-index probe).
    entry
        .shallow_state
        .effective_type_header_present(symbol_name)
        || entry
            .shallow_state
            .effective_value_header_present(symbol_name)
}

/// Resolve a local import binding for `local_name` declared in
/// `canonical_id` through three fall-through layers (shallow facts →
/// scope payload → prepared-decl bundle).
///
/// # Macro Type Traversal Rule — unresolved imports
///
/// When `local_name` cannot be resolved (no shallow import target,
/// no scope-payload binding, no prepared-decl entry), every layer
/// returns `None` and the caller short-circuits to a miss / opaque
/// sentinel via the dispatch layer. **No synthetic placeholder root
/// is invented for the unresolved specifier.** This is the explicit
/// `CRITICAL` Macro Type Traversal contract from `CLAUDE.md`: only
/// follow the import graph reachable from the requested type's
/// declaration graph; never treat plain imports as implicit exports
/// or synthesise an external root for an absent specifier.
///
/// Concretely: a fixture importing `import type { Foo } from
/// './types'` where `./types` is not in the workspace produces a
/// `None` here, the lowering pipeline encodes the absence as an
/// `Opaque(QueryError::Miss)` sentinel, and downstream projection
/// publishes the partial without inventing a stub for `Foo`. The
/// component-meta result remains well-formed (other props
/// resolve), but any field whose type transitively depended on
/// `Foo` carries the opaque sentinel.
fn resolve_import_binding_from_facts(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    scope_payload: Option<&DeclarationScopePayload>,
    local_name: &str,
) -> Option<ResolvedRootIdentity> {
    // A) Try the shallow-state import_targets map first (the cached
    //    parse facts are the canonical authority). Structurally
    //    read-only (see the scope read above).
    if let Some(entry) = ctx
        .ensure_indexed_ready_serve(canonical_id)
        .map(|serve| serve.indexed)
    {
        let state = &entry.shallow_state;
        if let Some(target) = state.import_target(local_name) {
            let resolved_id = if target.canonical_id.is_empty() {
                ctx.resolve_type_dependency_canonical(canonical_id, &target.source_specifier)?
            } else {
                target.canonical_id.clone()
            };
            return Some(resolve_imported_type_root_identity(
                ctx,
                &resolved_id,
                &target.imported_name,
            ));
        }
    }

    // B) Fallback to the scope payload's import bindings (which the
    //    prepared-decl builder may have discovered through script-setup
    //    manifest paths not visible to the raw shallow state).
    if let Some(payload) = scope_payload {
        if let Some(binding) = payload.import_bindings.get(local_name) {
            return Some(resolve_imported_type_root_identity(
                ctx,
                &binding.canonical_id,
                &binding.exported_name,
            ));
        }
    }

    // C) Final fallback: fetch the prepared-decl bundle directly.
    let bundle = ctx.prepared_decl_bundle(canonical_id)?;
    let binding = bundle.import_bindings.get(local_name)?;
    Some(resolve_imported_type_root_identity(
        ctx,
        &binding.canonical_id,
        &binding.exported_name,
    ))
}

fn resolve_namespace_member_from_facts(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    scope_payload: Option<&DeclarationScopePayload>,
    symbol_name: &str,
) -> Option<ResolvedRootIdentity> {
    let dot_pos = symbol_name.find('.')?;
    let prefix = &symbol_name[..dot_pos];
    let member = &symbol_name[dot_pos + 1..];
    let binding = resolve_import_binding_from_facts(ctx, canonical_id, scope_payload, prefix)?;

    // Structurally read-only (see the scope read above).
    if let Some(target_entry) = ctx
        .ensure_indexed_ready_serve(&binding.canonical_id)
        .map(|serve| serve.indexed)
    {
        if symbol_exists_in_facts(&target_entry, member) {
            return Some(ResolvedRootIdentity::new(&binding.canonical_id, member));
        }

        if let Some(crate::resolver_core::ExportTarget::Local { symbol_name }) =
            target_entry.shallow_state.export_target(member)
        {
            return Some(ResolvedRootIdentity::new(
                &binding.canonical_id,
                symbol_name,
            ));
        }
    }

    if let Some((resolved_canonical_id, exported_name)) =
        ctx.resolve_named_type_export_target(&binding.canonical_id, member)
    {
        return Some(ResolvedRootIdentity::new(
            &resolved_canonical_id,
            &exported_name,
        ));
    }

    ctx.resolve_value_export_target(&binding.canonical_id, member)
        .map(|target| ResolvedRootIdentity::new(&target.canonical_id, &target.name))
}

fn resolve_imported_type_root_identity(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exported_name: &str,
) -> ResolvedRootIdentity {
    if canonical_id.is_empty() {
        return ResolvedRootIdentity::new(canonical_id, exported_name);
    }

    // Facts-returning form + tracer record: bare-name resolution feeds
    // memoized builds (the `LowerLocator` ref-head fallback in particular),
    // so the route-chain facts must enter the active tracer — otherwise a
    // barrel retarget with the owner unchanged false-warms the enclosing
    // cache entry. A no-op when no tracer is installed.
    let ((resolved_canonical_id, resolved_symbol_name), route_facts) =
        ctx.resolve_imported_type_root_with_facts(canonical_id, exported_name);
    ctx.observe_borrowed_signature(&route_facts);
    ResolvedRootIdentity::new(resolved_canonical_id, resolved_symbol_name)
}

/// Resolve a `PreparedTypeDecl` for a root identity using ctx-owned
/// caches. Mirrors `SessionSolverHost::resolve_prepared_type_decl`:
///
/// 1. Direct `prepared_type_decl` lookup at the root's canonical.
/// 2. Import-root walk via `resolve_imported_type_root`, then retry
///    `prepared_type_decl` at the resolved `(canonical, name)`.
///
/// **Script-setup type-parameter bindings are NOT reachable through
/// this function.** Script-setup parameters are not type-aliases and
/// therefore not `PreparedTypeDecl`s; the lowering hot path reads
/// `scope_payload.scope_type_bindings.get(name)` directly to obtain a
/// [`TypeParamBinding`](crate::resolver_core::prepared_decl::TypeParamBinding)
/// and emits a `SemanticNodeData::TypeParam` without going through
/// this function.
///
/// Used by the dispatch path (walker / build_instantiate) so dispatch
/// does not need to construct a `SessionSolverHost` just to reach the
/// prepared-decl cache.
pub(crate) fn resolve_prepared_type_decl_via_host(
    ctx: &dyn ResolverContext,
    _scope_canonical_id: Option<&str>,
    _scope_payload: Option<&DeclarationScopePayload>,
    root_identity: &ResolvedRootIdentity,
) -> Option<Arc<PreparedTypeDecl>> {
    if let Some(prepared) =
        ctx.prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
    {
        return Some(prepared);
    }

    if root_identity.canonical_id.is_empty() {
        return None;
    }

    // Facts-returning form + tracer record (see
    // `resolve_imported_type_root_identity` above): this retry hop also
    // feeds memoized builds, so its route proof must be observed.
    let ((final_canonical_id, final_symbol_name), route_facts) = ctx
        .resolve_imported_type_root_with_facts(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
        );
    ctx.observe_borrowed_signature(&route_facts);
    if final_canonical_id == root_identity.canonical_id
        && final_symbol_name == root_identity.symbol_name
    {
        return None;
    }

    ctx.prepared_type_decl(&final_canonical_id, &final_symbol_name)
}

/// Resolve an unqualified namespace-member reference to its QUALIFIED sibling
/// identity, reconstructing the sibling visibility from SHALLOW file state.
///
/// This is the content-free local-scope resolver hook the
/// [`LocalScopePayload::Namespace`](crate::semantic_query::LocalScopePayload)
/// carrier feeds: a carrier inside `namespace NS { ... }` stores ONLY the
/// scope id (the prefix + origin); this hook re-prefixes the bare `name` with
/// the namespace prefix and looks `NS.name` up in the SHALLOW type/value
/// headers visible for the scope's origin — NO sibling map is stored on the
/// carrier, NO body is lowered. It mirrors EXACTLY the three origin rules of
/// the eager `add_namespace_sibling_resolutions`:
///
/// - [`File`](crate::semantic_query::LocalScopeOrigin::File): a file-scope
///   `namespace NS { ... }` binds a direct TYPE or VALUE sibling indexed under
///   its qualified `NS.name` name (single-segment members only; a deeper
///   `NS.Sub.X` is not reachable as a bare name from `NS`).
/// - [`Global`](crate::semantic_query::LocalScopeOrigin::Global): a
///   `declare global { namespace NS { ... } }` binds a global TYPE sibling
///   ONLY (a global value sibling has no prepared-value slot).
/// - [`Module`](crate::semantic_query::LocalScopeOrigin::Module): binds
///   nothing (no consumable module-scope sibling is addressable today).
///
/// Returns the resolved `(scope_canonical_id, "NS.name")` identity, or `None`
/// when the namespace has no such direct sibling under the origin's inventory.
///
/// NOTE — scaffolding: this hook is the model + reader for the producer stage
/// that stamps the namespace `LocalScopeId` onto a body `BareRef` carrier. It
/// is NOT yet wired into the live carrier-resolution flow (the eager
/// `add_namespace_sibling_resolutions` path remains the active producer of
/// namespace-sibling `name_resolution` entries); it is exercised in isolation
/// against synthetic shallow state. `allow(dead_code)`: the live caller lands
/// when the structural producer stamps the scope id (a later stage).
#[allow(dead_code)]
pub(crate) fn resolve_namespace_sibling_in_scope(
    payload: &crate::semantic_query::LocalScopePayload,
    state: &crate::resolver_core::ShallowFileState,
    scope_canonical_id: &str,
    name: &str,
) -> Option<ResolvedRootIdentity> {
    use crate::semantic_query::{LocalScopeOrigin, LocalScopePayload};

    let LocalScopePayload::Namespace { prefix, origin } = payload;
    // A bare member name binds to `<prefix>.<name>`; only a DIRECT (single
    // additional segment) sibling is reachable as a bare name.
    if name.contains('.') {
        return None;
    }
    let qualified = format!("{prefix}.{name}");

    match origin {
        // File-scope namespace: a direct TYPE or VALUE sibling, indexed under
        // its qualified `NS.name` name in the file-scope header inventory.
        LocalScopeOrigin::File => {
            let is_sibling = state
                .type_symbol_names()
                .chain(state.value_symbol_names())
                .any(|sym| sym == qualified);
            is_sibling.then(|| ResolvedRootIdentity::new(scope_canonical_id, &qualified))
        }
        // Global-augmentation namespace: a global TYPE sibling ONLY.
        LocalScopeOrigin::Global => {
            use verter_semantic::analysis::type_eval::AugmentationScopeKind;
            let is_global_type_sibling = state.augmentation_type_keys().any(|(scope, sym)| {
                matches!(scope, AugmentationScopeKind::Global) && sym == qualified
            });
            is_global_type_sibling
                .then(|| ResolvedRootIdentity::new(scope_canonical_id, &qualified))
        }
        // Module-augmentation namespace: no consumable sibling today.
        LocalScopeOrigin::Module => None,
    }
}

#[cfg(test)]
#[path = "bare_name_resolve_namespace_tests.rs"]
mod bare_name_resolve_namespace_tests;
