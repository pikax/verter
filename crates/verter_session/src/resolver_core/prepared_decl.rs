#[cfg(test)]
use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::prepared::PreparedExternalDep;
use verter_semantic::analysis::type_solver::{
    PreparedTypeDecl, PreparedValueDecl, ResolvedRootIdentity,
};

use super::shallow_file_state::ClassifiedTypeDeps;
use super::{ExportTarget, ShallowFileState};
use crate::decl_body_memo::{DemandOutcome, LoweredTypeDecl, LoweredValueDecl};

/// One per-symbol prepared-decl slot: a warm write-once committed value plus a
/// resettable in-flight gate.
///
/// `value` holds the committed result of a GENUINE build (`Some` decl or
/// `None` absence). `build_gate` serialises COLD builds so a successful build
/// runs exactly once under contention (single-flight): waiters block
/// cooperatively on the `parking_lot::Mutex` (never a busy-spin) and, after the
/// winner commits, re-check `value` and reuse it rather than rebuild. A broken
/// decl-body lease pin leaves `value` VACANT and releases the gate, so a later
/// demand under a live lease recomputes — the lease-miss is never persisted as
/// absence (a write-once `OnceLock` alone could not express that).
struct PreparedDeclSlot<T> {
    value: OnceLock<Option<Arc<T>>>,
    build_gate: parking_lot::Mutex<()>,
}

impl<T> PreparedDeclSlot<T> {
    fn new() -> Self {
        Self {
            value: OnceLock::new(),
            build_gate: parking_lot::Mutex::new(()),
        }
    }
}

type PreparedTypeDeclSlot = Arc<PreparedDeclSlot<PreparedTypeDecl>>;
type PreparedTypeDeclSlots = Arc<FxHashMap<String, PreparedTypeDeclSlot>>;
type PreparedValueDeclSlot = Arc<PreparedDeclSlot<PreparedValueDecl>>;
type PreparedValueDeclSlots = Arc<FxHashMap<String, PreparedValueDeclSlot>>;

/// Outcome of a lease-aware prepared-decl build. A genuine `Ready(None)` (the
/// symbol is not inventoried, is an import-local, or lowered to no decl) is a
/// cacheable absence; a `LeaseMiss` (a broken decl-body lease pin — the
/// demanded body lowering ReturnOnly'd, lowering NOTHING) is a TRANSIENT
/// no-warm signal a cache-admitting consumer must NOT persist as absence, so a
/// later demand under a live lease recovers. Never collapse the two at a
/// warm-admission boundary (the write-once prepared-decl slot).
pub(crate) enum PreparedDeclOutcome<T> {
    Ready(Option<T>),
    LeaseMiss,
}

impl<T> PreparedDeclOutcome<T> {
    /// Collapse to the plain `Option` for direct/standalone callers
    /// (`prepare_exported_*`, tests) that do NOT admit into the write-once
    /// slot cache: a lease-miss reads as `None` there (they recompute on the
    /// next call, warm-poisoning nothing).
    ///
    /// The `LeaseMiss` arm marks the generalized non-cacheability rail so an
    /// enclosing traced compute that folds this transient miss refuses its
    /// own shared-cache admission; a `Ready(None)` cacheable absence marks
    /// nothing.
    fn into_option(self) -> Option<T> {
        match self {
            PreparedDeclOutcome::Ready(value) => value,
            PreparedDeclOutcome::LeaseMiss => {
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
        }
    }
}

/// Import binding: maps a local import name to its resolved target.
/// Used by the declaration-scope solver host to resolve cross-file references.
#[derive(Debug, Clone)]
pub struct ImportBinding {
    pub canonical_id: String,
    pub exported_name: String,
}

/// FINAL defining-file canonicalization of a file's import targets, computed
/// at bundle materialization through the SAME route authority the carrier
/// fallback / dispatch fallthrough use, so the eager/prepared `name_resolution`
/// records the FINAL definition rather than the intermediate barrel.
///
/// One entry per re-export-hop import, keyed by the importer's LOCAL name.
/// Each import is canonicalized SYMMETRICALLY across BOTH rails: the
/// type-export authority (`resolve_named_type_export_target` /
/// `resolve_imported_type_root`) AND the value-export authority
/// (`resolve_value_export_target`); the rail that follows a re-export hop to a
/// DIFFERENT final `(canonical, name)` wins (a type re-export resolves on the
/// type rail, a value re-export on the value rail). A `local_name` absent from
/// the map was not a re-export hop on either rail and resolves through the
/// unchanged barrel fallback.
#[derive(Debug, Clone, Default)]
pub struct ImportCanonicalization {
    /// `local_name → FINAL (canonical, name)` for every re-export-hop import,
    /// canonicalized through whichever rail (type or value) followed the hop.
    pub final_resolution: FxHashMap<String, ResolvedRootIdentity>,
}

/// Script-setup generic type-parameter binding for
/// `<script setup lang="ts" generic="T extends Item = Item">`
/// parameters.
///
/// The binding carries the parameter name plus its declaration-site
/// `extends` constraint and `=` default as **unlowered** [`TypeExpr`]
/// values; the dispatch lowering path interns them on demand into
/// [`SemanticNodeData::TypeParam`](crate::semantic_query::SemanticNodeData::TypeParam)
/// via `shallow_lower_type_expr`. `PreparedTypeDecl` would be the
/// wrong category for this data — type parameters do not have alias
/// bodies, scope-local `name_resolution`, or the rest of the
/// prepared-decl surface.
///
/// `ordinal` carries the 0-based clause position into the lowered
/// `SemanticNodeData::TypeParam.param_index`, disambiguating same-name
/// parameters across multiple script-setup declarations within one
/// file.
#[derive(Debug, Clone)]
pub struct TypeParamBinding {
    pub name: Arc<str>,
    /// 0-based position in the
    /// `<script setup generic="T, U, V">` clause, used as
    /// [`SemanticNodeData::TypeParam.param_index`](crate::semantic_query::SemanticNodeData::TypeParam)
    /// so multiple script-setup parameters in one file get distinct
    /// identity tuples.
    pub ordinal: u16,
    pub constraint: Option<Arc<verter_type_expr::TypeExpr>>,
    pub default: Option<Arc<verter_type_expr::TypeExpr>>,
}

/// Canonicalize ONE import target to the FINAL defining-file identity for the
/// prepared/eager `name_resolution`. A re-export-hop import takes its
/// precomputed final `(canonical, name)` from `import_canonicalization`
/// (resolved at bundle materialisation through the shared route authority);
/// every other import falls back to the barrel/direct resolution
/// ([`resolve_import_target`] + the import's own `imported_name`).
fn canonicalize_import_target(
    import_canonicalization: &ImportCanonicalization,
    owner_canonical_id: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    local_name: &str,
    target: &super::shallow_file_state::ImportTarget,
) -> ResolvedRootIdentity {
    if let Some(final_identity) = import_canonicalization.final_resolution.get(local_name) {
        return final_identity.clone();
    }
    let resolved_id = resolve_import_target(
        owner_canonical_id,
        dep_edges,
        &target.source_specifier,
        Some(target.canonical_id.as_str()),
    );
    ResolvedRootIdentity::new(&resolved_id, &target.imported_name)
}

fn resolve_import_target(
    owner_canonical_id: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    source_specifier: &str,
    canonical_id: Option<&str>,
) -> String {
    if let Some(canonical_id) = dep_edges
        .and_then(|edges| edges.get(source_specifier))
        .cloned()
    {
        return canonical_id;
    }

    if let Some(canonical_id) = canonical_id.filter(|canonical_id| !canonical_id.is_empty()) {
        return canonical_id.to_string();
    }

    let relative_last_segment = source_specifier
        .rsplit('/')
        .next()
        .unwrap_or(source_specifier);
    if source_specifier.starts_with('.') && relative_last_segment.contains('.') {
        crate::id::resolve_external(owner_canonical_id, source_specifier)
    } else {
        source_specifier.to_string()
    }
}

/// Prepare a local type declaration from a canonical shallow file state.
///
/// Populates local_deps, external_deps, and name_resolution from the
/// shallow symbol, and auto-builds the member index for object-like bodies.
///
/// `dep_edges` maps import specifiers (e.g. `./types`) to resolved canonical
/// IDs (e.g. `/src/types.ts`). When provided, external deps and
/// `name_resolution` entries use the resolved canonical IDs. When `None`,
/// raw import specifiers are used as-is.
pub fn prepare_local_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) -> Option<PreparedTypeDecl> {
    prepare_local_type_decl_outcome(
        canonical_id,
        state,
        symbol_name,
        dep_edges,
        import_canonicalization,
    )
    .into_option()
}

/// Lease-aware variant of [`prepare_local_type_decl`]: distinguishes a genuine
/// absence (`Ready(None)`, cacheable) from a broken decl-body lease pin
/// (`LeaseMiss`, no-warm) so the write-once slot cache
/// ([`PreparedTypeDeclCache::get`]) refuses to persist a transient body-less
/// result as genuine declaration absence. `prepare_local_type_decl` collapses
/// the two for direct/standalone callers that do not admit into that slot.
pub(crate) fn prepare_local_type_decl_outcome(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) -> PreparedDeclOutcome<PreparedTypeDecl> {
    prepare_local_type_decl_outcome_with_base(
        canonical_id,
        state,
        symbol_name,
        dep_edges,
        import_canonicalization,
        None,
    )
}

/// [`prepare_local_type_decl_outcome`] with an optional caller-owned per-file
/// TYPE-space `name_resolution` base table
/// ([`build_type_name_resolution_base`]): a `Some` caller (the prepared-decl
/// cache, which builds the base once per bundle) shares it via `Arc` across
/// every non-namespaced decl of the file; `None` builds the table fresh —
/// the pre-split per-call behavior for standalone entry points.
fn prepare_local_type_decl_outcome_with_base(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    shared_name_resolution_base: Option<&Arc<FxHashMap<String, ResolvedRootIdentity>>>,
) -> PreparedDeclOutcome<PreparedTypeDecl> {
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;
    // A name absent from the file surface but present in the file's own
    // `declare global { ... }` inventory resolves to the merged global
    // declaration. Global augmentations are visible from any scope, so a bare
    // reference (`type Alias = GlobalContract`) reaches the merged surface
    // through the same prepared-decl → `MergedDecl` machinery as a file symbol.
    // Resolve the lowered body (and, for a file-scope symbol, its classified
    // dependency edges) plus the declaration's ORIGIN scope — which
    // namespace-sibling inventory is visible to a namespaced member of this
    // decl: a file-scope symbol (`None` origin → file-scope siblings) first,
    // then the global-augmentation fallback (`Global` origin → global
    // TYPE-augmentation siblings; the body carries no classified deps — it
    // stitches onto another module's surface). A broken-lease body demand
    // surfaces the DISTINCT `LeaseMiss`, never collapsed into a cacheable miss.
    let global_scope = AugmentationScopeKind::Global;
    let (lowered, deps, origin): (Arc<LoweredTypeDecl>, _, Option<&AugmentationScopeKind>) =
        if state.has_type_symbol(symbol_name) {
            match state.type_decl_outcome(symbol_name) {
                DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
                DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
                DemandOutcome::Ready(Some(lowered)) => {
                    (lowered, state.type_deps(symbol_name), None)
                }
            }
        } else {
            match state.augmentation_type_decl_outcome(&global_scope, symbol_name) {
                DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
                DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
                DemandOutcome::Ready(Some(lowered)) => (lowered, None, Some(&global_scope)),
            }
        };
    // Type-space declarations win over a same-named import binding. This is
    // observable for Vue SFCs because normal `<script>` and `<script setup>`
    // are distinct authored scopes even though their shallow facts share one
    // file owner: a normal-script `type Separator = ...` must remain
    // addressable when setup imports the runtime value `Separator`. A pure
    // import local still has no prepared declaration.
    if state.is_import_local(symbol_name) && !state.has_type_symbol(symbol_name) {
        return PreparedDeclOutcome::Ready(None);
    }

    PreparedDeclOutcome::Ready(Some(prepare_type_decl_from_lowered(
        canonical_id,
        state,
        symbol_name,
        lowered.as_ref(),
        deps.as_deref(),
        dep_edges,
        origin,
        import_canonicalization,
        shared_name_resolution_base,
    )))
}

/// Prepare a type declaration retained in an ambient-augmentation scope
/// (`declare module "X" { ... }` / `declare global { ... }`).
///
/// The augmenter file's `declare module`/`declare global` inner declarations
/// never enter file-scope `symbols`; they live in
/// [`ShallowFileState::augmentation_scopes`] keyed by `(scope, name)`. The
/// cross-file augmentation stitch lowers each augmenter's contributed body
/// through the SAME `PreparedTypeDecl` → `MergedDecl` machinery as a file
/// symbol, so it builds the prepared decl from the augmentation symbol's body
/// and the augmenter file's own `name_resolution` (its file symbols + import
/// bindings). Returns `None` when the augmenter has no contributor for
/// `(scope, symbol_name)`.
pub fn prepare_augmentation_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedTypeDecl> {
    prepare_augmentation_type_decl_outcome(canonical_id, state, scope, symbol_name, dep_edges)
        .into_option()
}

/// Lease-aware variant of [`prepare_augmentation_type_decl`]: the cross-file
/// augmentation stitch uses this so a broken-lease augmenter body surfaces the
/// DISTINCT `LeaseMiss` (folded into the fold's `source_env_unobservable`
/// no-warm rail) instead of a silent skip that would warm-admit an
/// under-merged surface. `prepare_augmentation_type_decl` collapses the two for
/// the locator-shape anchor-scope caller (already protected by the preceding
/// `deref_locator_body` lease-miss → `cache_suppress` rail).
pub(crate) fn prepare_augmentation_type_decl_outcome(
    canonical_id: &str,
    state: &ShallowFileState,
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> PreparedDeclOutcome<PreparedTypeDecl> {
    let lowered = match state.augmentation_type_decl_outcome(scope, symbol_name) {
        DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
        DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
        DemandOutcome::Ready(Some(lowered)) => lowered,
    };
    // Augmentation-scope decls are off the barrel-final import path (their
    // bodies stitch onto another module's surface, not a re-export hop), so the
    // barrel fallback applies for any import they reference. No shared base:
    // the stitch prepares one decl per (augmenter, name) demand, and the
    // default canonicalization differs from the bundle-owned base's.
    PreparedDeclOutcome::Ready(Some(prepare_type_decl_from_lowered(
        canonical_id,
        state,
        symbol_name,
        lowered.as_ref(),
        None,
        dep_edges,
        Some(scope),
        &ImportCanonicalization::default(),
        None,
    )))
}

/// Build a [`PreparedTypeDecl`] from an already-lowered
/// [`LoweredTypeDecl`] body plus its (optional) classified dependency
/// edges. Shared by [`prepare_local_type_decl`] (file-scope symbol) and
/// [`prepare_augmentation_type_decl`] (augmentation-scope symbol): both
/// categories lower their body through the identical name-resolution +
/// merged-contributor path, differing only in WHERE the body was looked
/// up (augmentation bodies carry no classified deps; `deps` is `None`).
///
/// `origin` is the declaration's ambient-augmentation scope (`None` for a
/// file-scope symbol), threaded to [`add_namespace_sibling_resolutions`] so a
/// namespaced member binds bare sibling names ONLY from the inventory visible
/// for that origin.
///
/// `shared_name_resolution_base` is the per-FILE type-space base table
/// ([`build_type_name_resolution_base`]) when the caller owns one (the
/// prepared-decl cache builds it once per bundle): a NON-namespaced
/// declaration's `name_resolution` is exactly that base, so it is shared via
/// `Arc` instead of being rebuilt per declaration. A namespaced declaration
/// (dotted `symbol_name`) additionally binds its declaration-scoped sibling
/// names, so it builds its own private table (same insertion order as the
/// base: file symbols, then siblings, then imports — imports stay the
/// per-key winners over siblings). `None` (standalone/augmentation entry
/// points) builds the table fresh — the pre-split per-call behavior.
#[allow(clippy::too_many_arguments)]
fn prepare_type_decl_from_lowered(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    lowered: &LoweredTypeDecl,
    deps: Option<&ClassifiedTypeDeps>,
    dep_edges: Option<&FxHashMap<String, String>>,
    origin: Option<&verter_semantic::analysis::type_eval::AugmentationScopeKind>,
    import_canonicalization: &ImportCanonicalization,
    shared_name_resolution_base: Option<&Arc<FxHashMap<String, ResolvedRootIdentity>>>,
) -> PreparedTypeDecl {
    #[cfg(test)]
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| {
        count.set(count.get().saturating_add(1));
    });

    let mut prepared = PreparedTypeDecl::new(
        ResolvedRootIdentity::new(canonical_id, symbol_name),
        lowered.kind,
    );
    // A merged interface carries its ordered contributor SLOTS so body
    // lowering interns a `MergedDecl` peer-merge carrier rather than
    // collapsing to a bare intersection.
    if lowered.body.is_merged() {
        prepared.set_merged_contributors(lowered.body.contributors().len());
    }
    prepared.type_parameters = lowered.narrow_type_parameters.clone();
    let empty_deps = ClassifiedTypeDeps::default();
    let deps = deps.unwrap_or(&empty_deps);
    prepared.local_deps = deps.local_deps.clone();
    prepared.external_deps = deps
        .external_deps
        .iter()
        .map(|dep| {
            let resolved_id = resolve_import_target(
                canonical_id,
                dep_edges,
                &dep.source_specifier,
                dep.canonical_id.as_deref(),
            );
            PreparedExternalDep {
                canonical_id: resolved_id,
                symbol_name: dep.imported_name.clone(),
            }
        })
        .collect();

    // name_resolution: bare names in the body → resolved identities. The
    // file-symbol + import entries are a per-FILE artifact (identical for
    // every declaration of this file), so a non-namespaced decl SHARES the
    // caller's base table; only a namespaced decl builds a private table to
    // add its declaration-scoped sibling bindings.
    prepared.name_resolution = if !symbol_name.contains('.') {
        match shared_name_resolution_base {
            Some(base) => Arc::clone(base),
            None => Arc::new(build_type_name_resolution_base(
                canonical_id,
                state,
                dep_edges,
                import_canonicalization,
            )),
        }
    } else {
        // Namespace-member scoping: inside `namespace NS { ... }`, an
        // unqualified reference to a sibling member `M` resolves to `NS.M`
        // (TS namespace scope). The shallow inventory indexes the member
        // under its QUALIFIED name (`NS.M`), so a sibling body `Ref("M")`
        // would otherwise miss. For a namespaced decl (whose `symbol_name`
        // carries an `NS.` prefix), map each DIRECT sibling's bare name to
        // its qualified identity, drawn from the inventory visible for the
        // decl's `origin` scope. Inserted AFTER the file-symbol pass so the
        // sibling binding takes precedence over an outer same-named type —
        // the TS namespace-scope rule — and BEFORE the import pass, which
        // stays the per-key winner for names the file's type namespace does
        // not declare.
        let mut table = FxHashMap::default();
        // Reserve the summed inventory sizes up front (tight upper bound for
        // the base entries: shadowed imports and cross-space name collisions
        // only shrink it) — header-index walks, no allocation.
        table.reserve(
            state.type_symbol_names().count()
                + state.value_symbol_names().count()
                + state.import_targets.len(),
        );
        insert_file_symbol_resolutions(&mut table, canonical_id, state);
        add_namespace_sibling_resolutions(&mut table, state, symbol_name, canonical_id, origin);
        insert_type_space_import_resolutions(
            &mut table,
            canonical_id,
            state,
            dep_edges,
            import_canonicalization,
        );
        Arc::new(table)
    };

    // Populate cache deps for invalidation
    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.to_string(), hash_u64));
    prepared.cache_deps.local_closure_participants = deps.local_deps.clone();

    // The classification FACTS (member index / wrapper shape / projection
    // class) were produced ONCE at lazy decl-body lowering time from the
    // transient contributor bodies (the shared `verter_semantic` classifiers
    // over the same lowering that minted the fingerprint) and stored on the
    // memo record — the prepared surface COPIES them. No re-classification,
    // no locator deref, no dispatch, no eager `Instantiate` at prepare time.
    prepared.member_index = lowered.member_index.clone();
    prepared.wrapper_shape = lowered.wrapper_shape.clone();
    prepared.projection_class = lowered.projection_class.clone();
    prepared.heritage_bases = Arc::clone(&lowered.heritage_bases);
    prepared.key_domain_closedness = lowered.key_domain_closedness.clone();
    prepared
}

/// Insert the same-file symbol entries of the `name_resolution` table: every
/// TYPE and VALUE symbol name resolves to its own identity in the defining
/// file. The first pass of BOTH base tables (type-space and value-space) —
/// imports insert after (and therefore win per key) in both spaces.
fn insert_file_symbol_resolutions(
    table: &mut FxHashMap<String, ResolvedRootIdentity>,
    canonical_id: &str,
    state: &ShallowFileState,
) {
    for dep_name in state.type_symbol_names() {
        table.insert(
            dep_name.to_string(),
            ResolvedRootIdentity::new(canonical_id, dep_name),
        );
    }
    for dep_name in state.value_symbol_names() {
        table.insert(
            dep_name.to_string(),
            ResolvedRootIdentity::new(canonical_id, dep_name),
        );
    }
}

/// Insert the TYPE-space import entries: external deps resolve through import
/// bindings → the FINAL defining file. When the import is a re-export hop,
/// the canonicalization (precomputed at bundle materialisation through the
/// SAME route authority the carrier fallback / dispatch fallthrough use)
/// carries the final `(canonical, name)`; otherwise the barrel fallback
/// applies. This keeps the eager fast-path's stored identity at the FINAL
/// definition rather than the intermediate barrel.
///
/// `PreparedTypeDecl` bodies are lowered in TYPE space: preserve the same
/// local-first precedence used by route-fact classification and the
/// declaration-scope fallback — a same-file type header shadows an import
/// binding with the same local spelling. Value-only imports still populate
/// the table (including `typeof` dependencies).
fn insert_type_space_import_resolutions(
    table: &mut FxHashMap<String, ResolvedRootIdentity>,
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) {
    for (local_name, target) in state.import_targets.iter() {
        if state.has_type_symbol(local_name) {
            continue;
        }
        let resolved = canonicalize_import_target(
            import_canonicalization,
            canonical_id,
            dep_edges,
            local_name,
            target,
        );
        table.insert(local_name.clone(), resolved);
    }
}

/// Insert the VALUE-space import entries — same FINAL-definition
/// canonicalization as the type space, WITHOUT the type-symbol shadow skip:
/// in a prepared value decl's annotation scope the import binding wins over
/// a same-named local symbol.
fn insert_value_space_import_resolutions(
    table: &mut FxHashMap<String, ResolvedRootIdentity>,
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) {
    for (local_name, target) in state.import_targets.iter() {
        let resolved = canonicalize_import_target(
            import_canonicalization,
            canonical_id,
            dep_edges,
            local_name,
            target,
        );
        table.insert(local_name.clone(), resolved);
    }
}

/// Build the per-FILE TYPE-space `name_resolution` base table: file symbols,
/// then imports (type-symbol-shadowed). This is the COMPLETE table of every
/// non-namespaced prepared type decl of the file — the prepared-decl cache
/// builds it once per bundle and shares it via `Arc` across those decls; a
/// namespaced decl re-runs the same passes around its sibling bindings.
fn build_type_name_resolution_base(
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) -> FxHashMap<String, ResolvedRootIdentity> {
    let mut table = FxHashMap::default();
    insert_file_symbol_resolutions(&mut table, canonical_id, state);
    insert_type_space_import_resolutions(
        &mut table,
        canonical_id,
        state,
        dep_edges,
        import_canonicalization,
    );
    table
}

/// Build the per-FILE VALUE-space `name_resolution` base table: file symbols,
/// then imports (unshadowed). The value space has no per-declaration bindings
/// at all, so this is the COMPLETE table of EVERY prepared value decl of the
/// file.
fn build_value_name_resolution_base(
    canonical_id: &str,
    state: &ShallowFileState,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) -> FxHashMap<String, ResolvedRootIdentity> {
    let mut table = FxHashMap::default();
    insert_file_symbol_resolutions(&mut table, canonical_id, state);
    insert_value_space_import_resolutions(
        &mut table,
        canonical_id,
        state,
        dep_edges,
        import_canonicalization,
    );
    table
}

/// Map each DIRECT sibling member of a namespaced declaration's enclosing
/// namespace to its qualified identity in `name_resolution`, so an unqualified
/// body reference (`Ref("M")` inside `namespace NS { type V = M }`) resolves to
/// `NS.M`. A no-op for a non-namespaced declaration (`symbol_name` has no `.`).
/// Only single-segment direct siblings bind (a deeper-nested `NS.Sub.X` is not
/// reachable as a bare name from `NS`'s own member bodies).
///
/// Siblings are drawn from the inventory visible for the declaration's `origin`
/// scope, and ONLY where that sibling has a buildable prepared decl — binding a
/// sibling the dispatch cannot resolve would dangle, and binding one from a
/// different scope would leak across scopes:
///
/// - File-scope decl (`origin = None`): file-scope `namespace NS { ... }` TYPE
///   and VALUE members, indexed under their qualified `NS.M` name in the
///   file-scope inventory. Both are consumable — a type sibling through the
///   prepared-type cache, a value sibling through `prepare_local_value_decl`.
/// - Global-augmentation decl (`origin = Global`): global `declare global {
///   namespace NS { ... } }` TYPE members, retained under `(Global, "NS.M")`
///   and never in file-scope `type_symbols`. TYPE-only: a `(Global, "NS.M")`
///   type key is consumable through [`prepare_local_type_decl`]'s global
///   fallback, but a global VALUE sibling has no prepared-value slot or
///   fallback, so binding it would dangle. Without this scan an unqualified
///   reference inside a global-augmented namespace (e.g. `interface
///   IntrinsicElements { div: Common }` referencing the sibling `Common`)
///   would not resolve — the valid global `JSX` namespace form.
/// - Module-augmentation decl (`origin = Module(spec)`): a `(Module(spec),
///   "NS.M")` sibling has no prepared-decl slot today (the prepared-decl caches
///   index file-scope + Global-augmentation symbols only), so no module sibling
///   is consumable. Bind nothing — binding a Global sibling here would leak
///   across scopes. When module-scope prepared decls become addressable, the
///   matching `(Module(spec), …)` siblings bind here.
fn add_namespace_sibling_resolutions(
    name_resolution: &mut FxHashMap<String, ResolvedRootIdentity>,
    state: &ShallowFileState,
    symbol_name: &str,
    canonical_id: &str,
    origin: Option<&verter_semantic::analysis::type_eval::AugmentationScopeKind>,
) {
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;
    let Some((namespace_prefix, _)) = symbol_name.rsplit_once('.') else {
        return;
    };
    let dotted_prefix = format!("{namespace_prefix}.");

    match origin {
        // File-scope decl: bind file-scope TYPE + VALUE siblings (both
        // consumable through the prepared-type / prepared-value caches).
        None => {
            for dep_name in state.type_symbol_names().chain(state.value_symbol_names()) {
                if let Some(member) = dep_name.strip_prefix(&dotted_prefix) {
                    if !member.contains('.') {
                        name_resolution.insert(
                            member.to_string(),
                            ResolvedRootIdentity::new(canonical_id, dep_name),
                        );
                    }
                }
            }
        }
        // Global-augmentation decl: bind global TYPE siblings ONLY — a global
        // VALUE sibling is not consumable (no prepared-value slot/fallback), so
        // binding it would dangle (RESTRICT). The key set spans every
        // augmentation scope, so filter to `Global`.
        Some(AugmentationScopeKind::Global) => {
            for (scope, name) in state.augmentation_type_keys() {
                if !matches!(scope, AugmentationScopeKind::Global) {
                    continue;
                }
                if let Some(member) = name.strip_prefix(&dotted_prefix) {
                    if !member.contains('.') {
                        name_resolution.insert(
                            member.to_string(),
                            ResolvedRootIdentity::new(canonical_id, name),
                        );
                    }
                }
            }
        }
        // Module-augmentation decl: no consumable module-scope sibling exists
        // today, so bind nothing rather than dangle a module sibling or leak a
        // global one across scopes.
        Some(AugmentationScopeKind::Module(_)) => {}
    }
}

/// Prepare a named exported type declaration after routing has selected the
/// defining file.
pub fn prepare_exported_type_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedTypeDecl> {
    let ExportTarget::Local { symbol_name } = state.export_target(exported_name)? else {
        return None;
    };

    // The direct/standalone prep entry carries no precomputed barrel-final
    // canonicalization (that is threaded by the host-materialised caches); a
    // re-export hop falls back to the barrel here. Production resolution goes
    // through the caches, which thread the real canonicalization.
    let mut prepared = prepare_local_type_decl(
        canonical_id,
        state,
        symbol_name,
        dep_edges,
        &ImportCanonicalization::default(),
    )?;
    prepared.exported_name = Some(exported_name.to_string());
    prepared.provenance.route_kind = Some("direct".to_string());
    Some(prepared)
}

/// Prepare a local value declaration from a canonical shallow file state.
pub fn prepare_local_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) -> Option<PreparedValueDecl> {
    prepare_local_value_decl_outcome(
        canonical_id,
        state,
        symbol_name,
        dep_edges,
        import_canonicalization,
    )
    .into_option()
}

/// Lease-aware variant of [`prepare_local_value_decl`] — see
/// [`prepare_local_type_decl_outcome`] for the no-warm contract that keeps a
/// broken-lease demand from committing a body-less value decl into the
/// write-once slot cache ([`PreparedValueDeclCache::get`]).
pub(crate) fn prepare_local_value_decl_outcome(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
) -> PreparedDeclOutcome<PreparedValueDecl> {
    prepare_local_value_decl_outcome_with_base(
        canonical_id,
        state,
        symbol_name,
        dep_edges,
        import_canonicalization,
        None,
    )
}

/// [`prepare_local_value_decl_outcome`] with an optional caller-owned
/// per-file VALUE-space `name_resolution` base table
/// ([`build_value_name_resolution_base`]). The value space has no
/// per-declaration bindings, so every prepared value decl's table IS the
/// base: a `Some` caller (the prepared-decl cache, which builds the base
/// once per bundle) shares it via `Arc`; `None` builds it fresh — the
/// pre-split per-call behavior for standalone entry points.
fn prepare_local_value_decl_outcome_with_base(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    shared_name_resolution_base: Option<&Arc<FxHashMap<String, ResolvedRootIdentity>>>,
) -> PreparedDeclOutcome<PreparedValueDecl> {
    let lowered: Arc<LoweredValueDecl> = match state.value_decl_outcome(symbol_name) {
        DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
        DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
        DemandOutcome::Ready(Some(lowered)) => lowered,
    };
    if state.is_import_local(symbol_name) {
        return PreparedDeclOutcome::Ready(None);
    }

    let mut prepared = PreparedValueDecl::new(
        ResolvedRootIdentity::new(canonical_id, symbol_name),
        lowered.kind,
    );
    prepared.type_annotation = lowered.type_annotation.clone();
    prepared.signatures = lowered.signatures.clone();
    prepared.object_shape = lowered.object_shape.clone();
    prepared.enum_members = lowered.enum_members.clone();

    // name_resolution for type annotations / `typeof` references that
    // reference imported or local symbols in the defining file — the per-FILE
    // value-space base table (file symbols, then imports; a re-export-hop
    // import canonicalizes to the FINAL defining file through the barrel-final
    // canonicalization, same authority as the type rail; otherwise the barrel
    // fallback applies), shared across every value decl of the file.
    prepared.name_resolution = match shared_name_resolution_base {
        Some(base) => Arc::clone(base),
        None => Arc::new(build_value_name_resolution_base(
            canonical_id,
            state,
            dep_edges,
            import_canonicalization,
        )),
    };

    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.to_string(), hash_u64));

    PreparedDeclOutcome::Ready(Some(prepared))
}

/// Prepare a named exported value declaration after routing has selected the
/// defining file.
pub fn prepare_exported_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
) -> Option<PreparedValueDecl> {
    let ExportTarget::Local { symbol_name } = state.export_target(exported_name)? else {
        return None;
    };

    let mut prepared = prepare_local_value_decl(
        canonical_id,
        state,
        symbol_name,
        dep_edges,
        &ImportCanonicalization::default(),
    )?;
    prepared.exported_name = Some(exported_name.to_string());
    Some(prepared)
}

#[derive(Clone)]
pub struct PreparedTypeDeclCache {
    canonical_id: Arc<str>,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
    slots: PreparedTypeDeclSlots,
    /// Per-FILE TYPE-space `name_resolution` base table, built lazily ONCE
    /// per cache (`OnceLock` collapses concurrent initializers) and shared
    /// via `Arc` by every non-namespaced prepared type decl this cache
    /// builds — the per-declaration table rebuild this replaces walked every
    /// file symbol + import per decl.
    name_resolution_base: Arc<OnceLock<Arc<FxHashMap<String, ResolvedRootIdentity>>>>,
    /// Per-cache-instance count of COLD builds admitted through
    /// [`PreparedTypeDeclCache::get`] (post-gate). Instance-scoped so a
    /// concurrent single-flight test asserts exactly ONE build on ITS OWN
    /// cache without a sibling test's cold `.get()` racing a shared global.
    #[cfg(test)]
    cold_build_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl PreparedTypeDeclCache {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key(&self, symbol_name: &str) -> bool {
        self.slots.contains_key(symbol_name)
    }

    /// The defining file's content identity — the `whole_hash` of the
    /// [`ShallowFileState`] every `PreparedTypeDecl` in this cache is
    /// built from.
    ///
    /// Provenance source for a query-identity cache producer whose
    /// value is derived from a `PreparedTypeDecl`: the producer reads
    /// the decl AND this hash from the SAME bundle, so the value and
    /// its self-root fact signature are provably one content version
    /// (untorn against a racing `upsert`). The hash is also view-correct
    /// — an overlay-bearing bundle (materialised through
    /// `prepared_decl_bundle_with_context`) carries the overlay's
    /// `ShallowFileState`, so the hash reflects whatever view the
    /// bundle was built from.
    pub fn defining_content_hash(&self) -> verter_semantic::analysis::Hash16 {
        self.state.whole_hash
    }

    pub fn get(&self, symbol_name: &str) -> Option<Arc<PreparedTypeDecl>> {
        let slot = self.slots.get(symbol_name)?;
        // Warm fast path — no gate.
        if let Some(cached) = slot.value.get() {
            return cached.clone();
        }
        // Cold: serialise the build under the resettable in-flight gate
        // (cooperative wait, never a spin), then re-check warm — a concurrent
        // winner may have committed while we blocked, so we reuse its result
        // rather than rebuild (single-flight for the successful case).
        let _gate = slot.build_gate.lock();
        if let Some(cached) = slot.value.get() {
            return cached.clone();
        }
        // A broken decl-body lease pin surfaces the DISTINCT `LeaseMiss` — fail
        // CLOSED via ReturnOnly: leave `value` VACANT (release the gate without
        // committing). Persisting `None` would falsely warm a REAL symbol as
        // genuine ABSENCE for the bundle's life (an entry the read-side fact
        // rail cannot reject, since content did not change) and could never
        // recover; the vacant slot lets a later demand under a live lease
        // recompute. A genuine result (`Some`) or genuine absence (`Ready(None)`)
        // commits the write-once value.
        #[cfg(test)]
        self.cold_build_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name_resolution_base = self.name_resolution_base.get_or_init(|| {
            Arc::new(build_type_name_resolution_base(
                self.canonical_id.as_ref(),
                self.state.as_ref(),
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
                &self.import_canonicalization,
            ))
        });
        match prepare_local_type_decl_outcome_with_base(
            self.canonical_id.as_ref(),
            self.state.as_ref(),
            symbol_name,
            (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            &self.import_canonicalization,
            Some(name_resolution_base),
        ) {
            PreparedDeclOutcome::LeaseMiss => {
                // Broken decl-body lease: leave the write-once slot VACANT
                // (retry on the next live-lease demand) AND mark the
                // generalized non-cacheability rail so an enclosing traced
                // compute refuses admission.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
            PreparedDeclOutcome::Ready(value) => {
                let committed = value.map(Arc::new);
                let _ = slot.value.set(committed.clone());
                committed
            }
        }
    }

    /// Test observability: whether the write-once slot for `symbol_name` has a
    /// COMMITTED entry (never a validity signal). A broken-lease demand must
    /// leave the slot VACANT — no wrong-empty `None` warm-admitted for a real
    /// symbol — so this returns `false` after a lease-miss.
    #[cfg(test)]
    pub(crate) fn slot_committed_for_test(&self, symbol_name: &str) -> bool {
        self.slots
            .get(symbol_name)
            .is_some_and(|slot| slot.value.get().is_some())
    }

    /// Test observability: this cache instance's own count of COLD builds
    /// admitted through [`get`](Self::get). Instance-scoped so a concurrent
    /// single-flight assertion is hermetic against sibling tests' cold `.get()`.
    #[cfg(test)]
    pub(crate) fn cold_build_count_for_test(&self) -> usize {
        self.cold_build_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub struct PreparedValueDeclCache {
    canonical_id: Arc<str>,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
    slots: PreparedValueDeclSlots,
    /// Per-FILE VALUE-space `name_resolution` base table — see
    /// [`PreparedTypeDeclCache::name_resolution_base`]; the value space has
    /// no per-declaration bindings, so EVERY prepared value decl shares it.
    name_resolution_base: Arc<OnceLock<Arc<FxHashMap<String, ResolvedRootIdentity>>>>,
}

impl PreparedValueDeclCache {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key(&self, symbol_name: &str) -> bool {
        self.slots.contains_key(symbol_name)
    }

    pub fn get(&self, symbol_name: &str) -> Option<Arc<PreparedValueDecl>> {
        let slot = self.slots.get(symbol_name)?;
        // Warm fast path — no gate.
        if let Some(cached) = slot.value.get() {
            return cached.clone();
        }
        // Cold: serialise under the resettable in-flight gate, then re-check
        // warm (single-flight) — same primitive as the type cache above. A
        // `LeaseMiss` leaves the slot VACANT (release the gate without
        // committing) so a false-warm absence is never persisted for a real
        // symbol and a later live-lease demand recomputes; only a genuine
        // result or genuine absence is cacheable.
        let _gate = slot.build_gate.lock();
        if let Some(cached) = slot.value.get() {
            return cached.clone();
        }
        let name_resolution_base = self.name_resolution_base.get_or_init(|| {
            Arc::new(build_value_name_resolution_base(
                self.canonical_id.as_ref(),
                self.state.as_ref(),
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
                &self.import_canonicalization,
            ))
        });
        match prepare_local_value_decl_outcome_with_base(
            self.canonical_id.as_ref(),
            self.state.as_ref(),
            symbol_name,
            (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            &self.import_canonicalization,
            Some(name_resolution_base),
        ) {
            PreparedDeclOutcome::LeaseMiss => {
                // Broken decl-body lease: leave the write-once slot VACANT
                // (retry on the next live-lease demand) AND mark the
                // generalized non-cacheability rail so an enclosing traced
                // compute refuses admission.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
            PreparedDeclOutcome::Ready(value) => {
                let committed = value.map(Arc::new);
                let _ = slot.value.set(committed.clone());
                committed
            }
        }
    }

    /// Test observability — see [`PreparedTypeDeclCache::slot_committed_for_test`].
    #[cfg(test)]
    pub(crate) fn slot_committed_for_test(&self, symbol_name: &str) -> bool {
        self.slots
            .get(symbol_name)
            .is_some_and(|slot| slot.value.get().is_some())
    }
}

/// Atomic declaration-surface bundle for one canonical file.
///
/// Valid as long as its `ImportRoute` and `FileWholeHash` facts match the
/// current store view. Never incrementally merged — rebuilt wholesale when the
/// import graph or file content changes.
#[derive(Clone)]
pub struct PreparedDeclBundle {
    /// The content version (`ShallowFileState::whole_hash`) of the
    /// canonical file this bundle was built from. A consumer that
    /// resolves a declaration through this bundle and roots a cache
    /// entry on the bundle's declaring file reads this — an OBSERVED
    /// identity captured when the bundle was materialised, never a
    /// current-content re-read at the consumer's signature-build time.
    pub owner_whole_hash: crate::resolver_core::ResolverHash16,
    pub prepared_type_decls: PreparedTypeDeclCache,
    pub prepared_value_decls: PreparedValueDeclCache,
    /// The dep_edges snapshot used to build this bundle.
    /// Stored so `SessionSolverHost::with_declaration_scope` can read it
    /// instead of recomputing dependency resolutions from the store view.
    pub dep_edges: Arc<FxHashMap<String, String>>,
    /// Resolved import bindings: local name → (canonical_id, exported_name).
    /// Built from the owner file's import targets + dep_edges during
    /// bundle materialization.
    pub import_bindings: FxHashMap<String, ImportBinding>,
    /// Same-file type names visible in the declaration scope.
    pub scope_type_names: FxHashSet<String>,
    /// Same-file value names visible in the declaration scope.
    pub scope_value_names: FxHashSet<String>,
    /// Script-setup generic type parameter bindings (Vue SFC only).
    /// Empty for non-Vue files. Populated once during bundle materialization
    /// so the solver hot path never calls `current_eval_state`.
    ///
    /// Each entry is a [`TypeParamBinding`] — type parameters are not
    /// type aliases, so they do not flow through `PreparedTypeDecl`.
    pub script_setup_type_bindings: FxHashMap<String, TypeParamBinding>,
}

/// Build an atomic declaration-surface bundle from a shallow file state and
/// resolved dependency edges. The bundle is immutable after construction.
///
/// `script_setup_type_bindings` are supplied by the caller (host_manage) because
/// extracting them requires access to the host's source/parse state, which is a
/// session-level concern. For non-Vue files the caller passes an empty map.
///
/// Each entry is a [`TypeParamBinding`]; type parameters do not flow
/// through `PreparedTypeDecl` because they are not type aliases.
pub fn build_prepared_decl_bundle(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: FxHashMap<String, String>,
    script_setup_type_bindings: FxHashMap<String, TypeParamBinding>,
    import_canonicalization: ImportCanonicalization,
) -> PreparedDeclBundle {
    let dep_edges = Arc::new(dep_edges);
    let import_canonicalization = Arc::new(import_canonicalization);

    // Build import bindings from shallow state import_targets + dep_edges.
    // One binding per import target at most — exact capacity bound.
    let mut import_bindings =
        FxHashMap::with_capacity_and_hasher(state.import_targets.len(), Default::default());
    for (local_name, target) in state.import_targets.iter() {
        let resolved_id = if target.canonical_id.is_empty() {
            dep_edges.get(&target.source_specifier).cloned()
        } else {
            Some(target.canonical_id.clone())
        };
        if let Some(resolved_id) = resolved_id {
            import_bindings.insert(
                local_name.clone(),
                ImportBinding {
                    canonical_id: resolved_id,
                    exported_name: target.imported_name.clone(),
                },
            );
        }
    }

    // Collect same-file symbol name sets (header-level — no body lowering).
    let scope_type_names: FxHashSet<String> =
        state.type_symbol_names().map(str::to_string).collect();
    let scope_value_names: FxHashSet<String> =
        state.value_symbol_names().map(str::to_string).collect();

    let owner_whole_hash = state.whole_hash;
    PreparedDeclBundle {
        owner_whole_hash,
        prepared_type_decls: build_prepared_type_decl_cache(
            canonical_id,
            Arc::clone(&state),
            Arc::clone(&dep_edges),
            Arc::clone(&import_canonicalization),
        ),
        prepared_value_decls: build_prepared_value_decl_cache(
            canonical_id,
            Arc::clone(&state),
            Arc::clone(&dep_edges),
            Arc::clone(&import_canonicalization),
        ),
        dep_edges,
        import_bindings,
        scope_type_names,
        scope_value_names,
        script_setup_type_bindings,
    }
}

/// Build the host-owned prepared type declaration cache for one defining file.
pub fn build_prepared_type_decl_cache(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
) -> PreparedTypeDeclCache {
    let mut slots: FxHashMap<String, PreparedTypeDeclSlot> = state
        .type_symbol_names()
        .map(|symbol_name| (symbol_name.to_string(), Arc::new(PreparedDeclSlot::new())))
        .collect();
    // Global-augmentation declarations (`declare global { interface N {} }`)
    // are resolvable by bare name through `prepare_local_type_decl`'s global
    // fallback, so they need a prepared-decl slot even though they never enter
    // the file surface. (A name that IS a file symbol already has a slot and
    // takes precedence.)
    for (scope, name) in state.augmentation_type_keys() {
        if matches!(
            scope,
            verter_semantic::analysis::type_eval::AugmentationScopeKind::Global
        ) && !slots.contains_key(name)
            && !state.is_import_local(name)
        {
            slots.insert(name.to_string(), Arc::new(PreparedDeclSlot::new()));
        }
    }

    PreparedTypeDeclCache {
        canonical_id: Arc::from(canonical_id),
        state,
        dep_edges,
        import_canonicalization,
        slots: Arc::new(slots),
        name_resolution_base: Arc::new(OnceLock::new()),
        #[cfg(test)]
        cold_build_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }
}

/// Build the host-owned prepared value declaration cache for one defining file.
pub fn build_prepared_value_decl_cache(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
) -> PreparedValueDeclCache {
    let slots = state
        .value_symbol_names()
        .filter(|symbol_name| !state.is_import_local(symbol_name))
        .map(|symbol_name| (symbol_name.to_string(), Arc::new(PreparedDeclSlot::new())))
        .collect();

    PreparedValueDeclCache {
        canonical_id: Arc::from(canonical_id),
        state,
        dep_edges,
        import_canonicalization,
        slots: Arc::new(slots),
        name_resolution_base: Arc::new(OnceLock::new()),
    }
}

#[cfg(test)]
thread_local! {
    static PREPARED_TYPE_DECL_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_prepared_type_decl_build_count_for_tests() {
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn prepared_type_decl_build_count_for_tests() -> usize {
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| count.get())
}

#[cfg(test)]
#[path = "prepared_decl_tests.rs"]
mod tests;
