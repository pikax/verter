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

use super::prepared_decl::{ImportBinding, PreparedTypeDeclResolution, TypeParamBinding};
use crate::resolver_core::ResolverContext;

/// Declaration-scope context used by bare-name root-identity resolution.
///
/// A shared VIEW over a
/// [`PreparedDeclBundle`](super::prepared_decl::PreparedDeclBundle) —
/// exposes the scope-local type/value names + script-setup type
/// parameter bindings + import bindings the resolver consults, reading
/// straight through the bundle `Arc` (construction is one refcount
/// bump; the bundle's maps are never copied).
///
/// [`Self::scope_type_bindings`] is keyed to [`TypeParamBinding`], NOT
/// to `Arc<PreparedTypeDecl>`; script-setup generic parameters carry
/// their declaration-site `extends` / default expressions directly
/// without an intermediate prepared-decl wrapper.
///
/// In-scope predicate contract: [`Self::scope_type_names`] is the
/// bundle's RAW same-file set — script-setup generic params are NOT
/// unioned in. Every in-scope check consults
/// [`Self::scope_type_bindings`] alongside it (the resolver rail below,
/// the dispatch gates, and [`ScopeShadowing`](crate::resolver_core::scope_shadowing::ScopeShadowing)
/// all check both sources), so the materialized union the payload used
/// to carry is redundant.
pub(crate) struct DeclarationScopePayload {
    bundle: Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>,
    owner: verter_type_expr::TopLevelOwnerId,
}

impl std::fmt::Debug for DeclarationScopePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The bundle itself is not `Debug`; print the payload's
        // consulted surfaces (same shape the pre-view struct printed).
        f.debug_struct("DeclarationScopePayload")
            .field("scope_type_names", self.scope_type_names())
            .field("scope_value_names", self.scope_value_names())
            .field("scope_type_bindings", self.scope_type_bindings())
            .field("import_bindings", self.import_bindings())
            .finish()
    }
}

impl DeclarationScopePayload {
    pub(crate) fn from_bundle(
        bundle: &Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Self {
        Self {
            bundle: Arc::clone(bundle),
            owner,
        }
    }

    #[must_use]
    pub(crate) fn owner(&self) -> verter_type_expr::TopLevelOwnerId {
        self.owner
    }

    fn owner_scope(&self) -> Option<&crate::resolver_core::prepared_decl::PreparedOwnerScope> {
        self.bundle.owner_scope(self.owner)
    }

    /// Same-file type names visible in the declaration scope (RAW
    /// bundle set — check [`Self::scope_type_bindings`] too; see the
    /// type-level in-scope predicate contract).
    pub(crate) fn scope_type_names(&self) -> &FxHashSet<String> {
        static EMPTY: std::sync::OnceLock<FxHashSet<String>> = std::sync::OnceLock::new();
        self.owner_scope()
            .map(|scope| &scope.scope_type_names)
            .unwrap_or_else(|| EMPTY.get_or_init(FxHashSet::default))
    }

    /// Same-file value names visible in the declaration scope.
    pub(crate) fn scope_value_names(&self) -> &FxHashSet<String> {
        static EMPTY: std::sync::OnceLock<FxHashSet<String>> = std::sync::OnceLock::new();
        self.owner_scope()
            .map(|scope| &scope.scope_value_names)
            .unwrap_or_else(|| EMPTY.get_or_init(FxHashSet::default))
    }

    /// Script-setup generic type parameter bindings (Vue SFC only;
    /// empty for non-Vue files).
    pub(crate) fn scope_type_bindings(&self) -> &FxHashMap<String, TypeParamBinding> {
        static EMPTY: std::sync::OnceLock<FxHashMap<String, TypeParamBinding>> =
            std::sync::OnceLock::new();
        self.owner_scope()
            .map(|scope| &scope.script_setup_type_bindings)
            .unwrap_or_else(|| EMPTY.get_or_init(FxHashMap::default))
    }

    /// Resolved import bindings: local name → (canonical_id, exported_name).
    pub(crate) fn import_bindings(&self) -> &FxHashMap<String, ImportBinding> {
        static EMPTY: std::sync::OnceLock<FxHashMap<String, ImportBinding>> =
            std::sync::OnceLock::new();
        self.owner_scope()
            .map(|scope| &scope.import_bindings)
            .unwrap_or_else(|| EMPTY.get_or_init(FxHashMap::default))
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
    scope_owner: verter_type_expr::TopLevelOwnerId,
    scope_payload: Option<&DeclarationScopePayload>,
    name: &str,
) -> Option<ResolvedRootIdentity> {
    // Identity mints below go through the store-owned intern pool so a
    // repeated `(scope, name)` resolution reuses one shared allocation.
    let interner = ctx.project_type_store().identity_interner();
    let mint_in_owner = |owner| {
        ResolvedRootIdentity::new_in_owner(
            interner.intern(scope_canonical_id),
            owner,
            interner.intern(name),
        )
    };
    // 1. Declaration-scope payload lookup (scope-local type/value,
    //    script-setup type bindings).
    if let Some(payload) = scope_payload.filter(|payload| payload.owner() == scope_owner) {
        let setup_binding_visible = matches!(
            scope_owner.kind(),
            verter_type_expr::TopLevelOwnerKind::Instance
        ) && payload.scope_type_bindings().contains_key(name);
        let ordinary_bundle_surface = scope_owner
            == verter_type_expr::TopLevelOwnerId::ordinary_file()
            && (payload.scope_type_names().contains(name)
                || payload.scope_value_names().contains(name));
        if setup_binding_visible || ordinary_bundle_surface {
            return Some(mint_in_owner(scope_owner));
        }
    }

    if scope_canonical_id.is_empty() {
        return None;
    }

    // 2. Scope's cached IndexedReady — local symbols + local exports.
    // Structurally read-only: the resolved identity feeds enclosing
    // builds whose admission gates consume the fenced-serve chokepoint
    // flag, so the serve status is not re-checked here.
    let indexed = ctx
        .ensure_indexed_ready_serve(scope_canonical_id)
        .map(|serve| serve.indexed);
    if let Some(entry) = indexed.as_ref() {
        if symbol_exists_in_facts(entry.as_ref(), scope_owner, name) {
            return Some(mint_in_owner(scope_owner));
        }
        if matches!(
            entry.shallow_state.export_target(name),
            Some(crate::resolver_core::ExportTarget::Local { owner, .. }) if *owner == scope_owner
        ) {
            return Some(mint_in_owner(scope_owner));
        }
        // A `declare global { interface Name { ... } }` declaration in this
        // file is not a file-surface symbol, but the name resolves to the
        // merged global declaration. The prepared-decl builder + `ResolveDecl`
        // both fall back to the global augmentation inventory under the same
        // `(canonical, name)` identity.
        if entry.shallow_state.has_global_augmentation(name) {
            return Some(mint_in_owner(scope_owner));
        }
    }

    // 3. Import-target walk (shallow state + prepared-bundle bindings).
    if let Some(resolved) =
        resolve_import_binding_from_facts(ctx, scope_canonical_id, scope_owner, scope_payload, name)
    {
        return Some(resolved);
    }

    // 4. Namespace-qualified: `Ns.Member`.
    if let Some(resolved) = resolve_namespace_member_from_facts(
        ctx,
        scope_canonical_id,
        scope_owner,
        scope_payload,
        name,
    ) {
        return Some(resolved);
    }

    // 5. Canonical SFC lexical-owner chain. An Instance/setup owner may see
    // exactly one validated Module/companion owner after every exact local
    // declaration/import route has missed. The reverse edge does not exist.
    // Re-entering this resolver with the exact parent owner preserves normal
    // module declaration/import rules and threads that owner into the returned
    // DeclKey identity; an absent or ambiguous parent fails closed.
    if let Some(parent_owner) = indexed.as_ref().and_then(|entry| {
        entry
            .shallow_state
            .validated_lexical_parent_owner(scope_owner)
    }) {
        let parent_payload = ctx
            .prepared_decl_bundle(scope_canonical_id)
            .map(|bundle| DeclarationScopePayload::from_bundle(&bundle, parent_owner));
        if let Some(resolved) = resolve_bare_name_in_scope(
            ctx,
            scope_canonical_id,
            parent_owner,
            parent_payload.as_ref(),
            name,
        ) {
            return Some(resolved);
        }
    }

    // 6. Cross-file export target. Same-file cross-owner visibility is owned
    // exclusively by the validated lexical-parent chain above; accepting it
    // here would recreate an arbitrary name-based owner rematch and let Module
    // scope see Instance declarations.
    let (resolved, route_facts) =
        ctx.resolve_imported_type_root_with_facts(scope_canonical_id, name);
    ctx.observe_borrowed_signature(&route_facts);
    if let Some(resolved) = resolved {
        if resolved.canonical_id.as_ref() == scope_canonical_id && resolved.owner != scope_owner {
            return None;
        }
        return Some(resolved);
    }

    None
}

fn symbol_exists_in_facts(
    entry: &crate::project_type_store::IndexedReady,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
) -> bool {
    // Local PRESENCE through the CENTRALIZED effective header lookup so a rune
    // module's ambient `$state`/`$derived`/… (value or type) resolves at the
    // bare-name surface without this site knowing about the rune prelude. A
    // plain `.ts` is unaffected (the effective lookup is rune-module-gated and
    // reduces to the header-index probe).
    entry
        .shallow_state
        .effective_type_header_present_in(owner, symbol_name)
        || entry
            .shallow_state
            .effective_value_header_present_in(owner, symbol_name)
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
    owner: verter_type_expr::TopLevelOwnerId,
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
        if let Some(target) = state.import_target_in(owner, local_name) {
            let resolved_id = if target.canonical_id.is_empty() {
                ctx.resolve_type_dependency_canonical(canonical_id, &target.source_specifier)?
            } else {
                target.canonical_id.clone()
            };
            return resolve_imported_type_root_identity(ctx, &resolved_id, &target.imported_name);
        }
    }

    // B) Fallback to the scope payload's import bindings (which the
    //    prepared-decl builder may have discovered through script-setup
    //    manifest paths not visible to the raw shallow state).
    if let Some(payload) = scope_payload.filter(|payload| payload.owner() == owner) {
        if let Some(binding) = payload.import_bindings().get(local_name) {
            return resolve_imported_type_root_identity(
                ctx,
                &binding.canonical_id,
                &binding.exported_name,
            );
        }
    }

    // C) Final fallback: fetch the prepared-decl bundle directly.
    let bundle = ctx.prepared_decl_bundle(canonical_id)?;
    let binding = bundle.owner_scope(owner)?.import_bindings.get(local_name)?;
    resolve_imported_type_root_identity(ctx, &binding.canonical_id, &binding.exported_name)
}

fn resolve_namespace_member_from_facts(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    _scope_payload: Option<&DeclarationScopePayload>,
    symbol_name: &str,
) -> Option<ResolvedRootIdentity> {
    let dot_pos = symbol_name.find('.')?;
    let prefix = &symbol_name[..dot_pos];
    let member = &symbol_name[dot_pos + 1..];
    let target_canonical =
        resolve_namespace_import_canonical_from_facts(ctx, canonical_id, owner, prefix)?;

    let (resolved, route_facts) =
        ctx.resolve_imported_type_root_with_facts(&target_canonical, member);
    ctx.observe_borrowed_signature(&route_facts);
    if let Some(resolved) = resolved {
        return Some(resolved);
    }

    let interner = ctx.project_type_store().identity_interner();
    ctx.resolve_value_export_target(&target_canonical, member)
        .map(|target| {
            ResolvedRootIdentity::new_in_owner(
                interner.intern(&target.canonical_id),
                target.owner,
                interner.intern(&target.name),
            )
        })
}

/// Resolve the dependency canonical owned by an exact namespace-import
/// binding (`import * as Ns from './dep'`). A namespace alias is a module
/// handle, not an exported declaration name: routing it through ordinary
/// import resolution would incorrectly probe the dependency for an export
/// literally named `Ns` before the qualified member is known.
///
/// The owner-qualified shallow import table is the sole authority. Its
/// ambiguous-binding state is already fail-closed (`import_target_in` returns
/// `None`), and `is_namespace` distinguishes a real module handle from a named
/// or default import. The returned canonical remains the namespace MODULE;
/// [`resolve_namespace_member_from_facts`] sends the member through the shared
/// type/value export resolvers, which own final re-export identity.
fn resolve_namespace_import_canonical_from_facts(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    prefix: &str,
) -> Option<String> {
    let indexed = ctx
        .ensure_indexed_ready_serve(canonical_id)
        .map(|serve| serve.indexed)?;
    let target = indexed
        .shallow_state
        .import_target_in(owner, prefix)
        .filter(|target| target.is_namespace)?;
    if target.canonical_id.is_empty() {
        ctx.resolve_type_dependency_canonical(canonical_id, &target.source_specifier)
    } else {
        Some(target.canonical_id.clone())
    }
}

fn resolve_imported_type_root_identity(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exported_name: &str,
) -> Option<ResolvedRootIdentity> {
    if canonical_id.is_empty() {
        return None;
    }

    // Facts-returning form + tracer record: bare-name resolution feeds
    // memoized builds (the `LowerLocator` ref-head fallback in particular),
    // so the route-chain facts must enter the active tracer — otherwise a
    // barrel retarget with the owner unchanged false-warms the enclosing
    // cache entry. A no-op when no tracer is installed.
    let (resolved, route_facts) =
        ctx.resolve_imported_type_root_with_facts(canonical_id, exported_name);
    ctx.observe_borrowed_signature(&route_facts);
    resolved
}

/// Resolve a prepared-declaration projection outcome for a root identity using
/// ctx-owned caches. Strict declarations remain cache-owned; a recoverable
/// exact authored failure is returned explicitly as `AuthoredPartial`.
///
/// 1. Direct projection lookup at the root's canonical.
/// 2. Import-root walk via `resolve_imported_type_root`, then retry
///    at the resolved exact `(canonical, owner, name)`.
///
/// **Script-setup type-parameter bindings are NOT reachable through
/// this function.** Script-setup parameters are not type-aliases and
/// therefore not `PreparedTypeDecl`s; the lowering hot path reads
/// `scope_payload.scope_type_bindings().get(name)` directly to obtain a
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
) -> PreparedTypeDeclResolution {
    let read = |identity: &ResolvedRootIdentity| {
        let Some(bundle) = ctx.prepared_decl_bundle(identity.canonical_id.as_ref()) else {
            return PreparedTypeDeclResolution::Missing;
        };
        bundle
            .prepared_type_decls
            .get_in_for_projection(identity.owner, identity.symbol_name.as_ref())
    };

    let direct = read(root_identity);
    if !direct.is_missing() {
        return direct;
    }

    if root_identity.canonical_id.is_empty() {
        return PreparedTypeDeclResolution::Missing;
    }

    // Facts-returning form + tracer record (see
    // `resolve_imported_type_root_identity` above): this retry hop also
    // feeds memoized builds, so its route proof must be observed.
    let (final_identity, route_facts) = ctx.resolve_imported_type_root_with_facts(
        &root_identity.canonical_id,
        &root_identity.symbol_name,
    );
    ctx.observe_borrowed_signature(&route_facts);
    let Some(final_identity) = final_identity else {
        return PreparedTypeDeclResolution::Missing;
    };
    if final_identity == *root_identity {
        return PreparedTypeDeclResolution::Missing;
    }

    read(&final_identity)
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
    owner: verter_type_expr::TopLevelOwnerId,
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
            let is_sibling = state.has_type_symbol_in(owner, &qualified)
                || state.has_value_symbol_in(owner, &qualified);
            is_sibling
                .then(|| ResolvedRootIdentity::new_in_owner(scope_canonical_id, owner, qualified))
        }
        // Global-augmentation namespace: a global TYPE sibling ONLY.
        LocalScopeOrigin::Global => {
            use verter_semantic::analysis::type_eval::AugmentationScopeKind;
            let is_global_type_sibling = state.augmentation_type_keys().any(|(scope, sym)| {
                matches!(scope, AugmentationScopeKind::Global) && sym == qualified
            });
            is_global_type_sibling.then(|| {
                ResolvedRootIdentity::new_in_owner(
                    scope_canonical_id,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    qualified,
                )
            })
        }
        // Module-augmentation namespace: no consumable sibling today.
        LocalScopeOrigin::Module => None,
    }
}

#[cfg(test)]
#[path = "bare_name_resolve_namespace_tests.rs"]
mod bare_name_resolve_namespace_tests;

#[cfg(test)]
mod payload_tests {
    use super::*;
    use crate::resolver_core::prepared_decl::{
        build_prepared_decl_bundle, ImportCanonicalization, TypeParamBinding,
    };
    use crate::resolver_core::ShallowFileState;

    /// `DeclarationScopePayload` is a VIEW over the prepared-decl
    /// bundle: construction shares the bundle's maps through the
    /// bundle `Arc` (one refcount bump), never deep-copies them, and
    /// script-setup generic params stay in `scope_type_bindings`
    /// (the in-scope disjunction checks bindings + names, so the raw
    /// `scope_type_names` set no longer materializes the union).
    #[test]
    fn payload_shares_bundle_maps_instead_of_copying() {
        let source = r#"
export interface Props { label: string }
export const defaults = { label: 'ok' }
"#;
        let state = ShallowFileState::service_backed_for_test(source);
        let interner = Arc::new(crate::identity_interner::IdentityInterner::with_default_budget());
        let mut script_setup: FxHashMap<String, TypeParamBinding> = FxHashMap::default();
        script_setup.insert(
            "T".to_string(),
            TypeParamBinding {
                name: Arc::from("T"),
                ordinal: 0,
                constraint: None,
                default: None,
            },
        );
        let bundle = Arc::new(build_prepared_decl_bundle(
            "/src/Comp.vue.ts",
            Arc::clone(&state),
            FxHashMap::default(),
            script_setup,
            ImportCanonicalization::default(),
            &interner,
        ));

        let module_owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let instance_owner = verter_type_expr::TopLevelOwnerId::instance(0);
        let module_scope = bundle
            .owner_scope(module_owner)
            .expect("ordinary module scope should be prepared");
        let instance_scope = bundle
            .owner_scope(instance_owner)
            .expect("script-setup instance scope should be prepared");
        let module_payload = DeclarationScopePayload::from_bundle(&bundle, module_owner);
        let instance_payload = DeclarationScopePayload::from_bundle(&bundle, instance_owner);

        assert!(
            std::ptr::eq(
                module_payload.scope_type_names(),
                &module_scope.scope_type_names
            ),
            "scope_type_names must be the bundle's own set, not a copy"
        );
        assert!(
            std::ptr::eq(
                module_payload.scope_value_names(),
                &module_scope.scope_value_names
            ),
            "scope_value_names must be the bundle's own set, not a copy"
        );
        assert!(
            std::ptr::eq(
                instance_payload.scope_type_bindings(),
                &instance_scope.script_setup_type_bindings
            ),
            "scope_type_bindings must be the bundle's own map, not a copy"
        );
        assert!(
            std::ptr::eq(
                module_payload.import_bindings(),
                &module_scope.import_bindings
            ),
            "import_bindings must be the bundle's own map, not a copy"
        );

        // Union-removal contract: the script-setup param is visible
        // through the bindings map, NOT through the raw name set.
        assert!(instance_payload.scope_type_bindings().contains_key("T"));
        assert!(!module_payload.scope_type_names().contains("T"));
        assert!(module_payload.scope_type_names().contains("Props"));
        assert!(module_payload.scope_value_names().contains("defaults"));
        assert!(!module_payload.scope_type_bindings().contains_key("T"));
    }
}
