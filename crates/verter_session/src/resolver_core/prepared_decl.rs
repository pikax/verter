#[cfg(test)]
use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::prepared::PreparedExternalDep;
use verter_semantic::analysis::type_solver::{
    PreparedTypeDecl, PreparedValueDecl, ResolvedRootIdentity,
};
use verter_type_expr::TopLevelOwnerId;

use super::shallow_file_state::ClassifiedTypeDeps;
use super::{ExportTarget, ShallowFileState};
use crate::decl_body_memo::{DemandOutcome, LoweredTypeDecl, LoweredValueDecl};
use crate::identity_interner::IdentityInterner;

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
type PreparedTypeDeclSlots = Arc<FxHashMap<verter_type_expr::DeclKey, PreparedTypeDeclSlot>>;
type PreparedValueDeclSlot = Arc<PreparedDeclSlot<PreparedValueDecl>>;
type PreparedValueDeclSlots = Arc<FxHashMap<verter_type_expr::DeclKey, PreparedValueDeclSlot>>;
type OwnerNameResolutionBases =
    Arc<FxHashMap<verter_type_expr::TopLevelOwnerId, Arc<OnceLock<SharedNameResolutionBase>>>>;

/// Per-FILE shared `name_resolution` base table (interned `Arc<str>` names →
/// resolved root identities): built once per prepared-decl cache and shared
/// via `Arc` by every non-namespaced prepared decl of the defining file. See
/// `PreparedTypeDecl::name_resolution` for the sharing + interning contract.
type SharedNameResolutionBase = Arc<FxHashMap<Arc<str>, ResolvedRootIdentity>>;

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
    Failed(PreparationFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationFailure {
    MissingExternalOwner { local_name: String },
    AuthoredOrdinalOverflow { count: usize },
}

/// Projection-facing prepared-declaration lookup.
///
/// Strict prepared declarations are the only values admitted to the shared
/// cache. When strict preparation fails solely because an exact authored
/// declaration references an unresolved import, projection may consume an
/// ephemeral declaration that retains the known authored shape while carrying
/// typed unresolved-owner debt. The declaration is never written into a slot;
/// the projection must run the normal resolver and classify the result partial
/// only if that exact debt remains unresolved at query exit.
#[derive(Debug, Clone)]
pub enum PreparedTypeDeclResolution {
    Complete(Arc<PreparedTypeDecl>),
    AuthoredPartial {
        root_identity: ResolvedRootIdentity,
        declaration: Arc<PreparedTypeDecl>,
        failure: PreparationFailure,
    },
    Missing,
    Failed {
        root_identity: ResolvedRootIdentity,
        failure: PreparationFailure,
    },
}

impl PreparedTypeDeclResolution {
    #[must_use]
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
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
    fn into_result(self) -> Result<Option<T>, PreparationFailure> {
        match self {
            PreparedDeclOutcome::Ready(value) => Ok(value),
            PreparedDeclOutcome::LeaseMiss => {
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                Ok(None)
            }
            PreparedDeclOutcome::Failed(failure) => Err(failure),
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
/// One entry per resolvable import, keyed by the importer's exact
/// `(TopLevelOwnerId, local name)`. Each import is canonicalized SYMMETRICALLY
/// across BOTH rails: the
/// type-export authority (`resolve_named_type_export_target` /
/// `resolve_imported_type_root`) AND the value-export authority
/// (`resolve_value_export_target`). The value stores the authoritative final
/// `(canonical, owner, symbol)`, including for a cold direct target. An absent
/// entry is an unresolved owner and produces `MissingExternalOwner`; consumers
/// never substitute the importer owner or rematch by name.
#[derive(Debug, Clone, Default)]
pub struct ImportCanonicalization {
    /// `(local owner, local name) → FINAL (canonical, owner, symbol)` for
    /// every resolvable import, canonicalized through the authoritative type or
    /// value route rail.
    pub final_resolution: FxHashMap<verter_type_expr::DeclKey, ResolvedRootIdentity>,
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
/// a missing entry is a typed preparation failure. Preparation never invents
/// an owner for an unresolved direct or barrel target.
fn canonicalize_import_target(
    import_canonicalization: &ImportCanonicalization,
    local_name: &str,
    local_owner: verter_type_expr::TopLevelOwnerId,
) -> Result<ResolvedRootIdentity, PreparationFailure> {
    import_canonicalization
        .final_resolution
        .get(&verter_type_expr::DeclKey::new(local_owner, local_name))
        .cloned()
        .ok_or_else(|| PreparationFailure::MissingExternalOwner {
            local_name: local_name.to_string(),
        })
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
    interner: &IdentityInterner,
) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
    prepare_local_type_decl_in(
        canonical_id,
        state,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol_name,
        dep_edges,
        import_canonicalization,
        interner,
    )
}

pub(crate) fn prepare_local_type_decl_in(
    canonical_id: &str,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
    prepare_local_type_decl_outcome(
        &interner.intern(canonical_id),
        state,
        owner,
        symbol_name,
        dep_edges,
        import_canonicalization,
        interner,
    )
    .into_result()
}

/// Lease-aware variant of [`prepare_local_type_decl`]: distinguishes a genuine
/// absence (`Ready(None)`, cacheable) from a broken decl-body lease pin
/// (`LeaseMiss`, no-warm) so the write-once slot cache
/// ([`PreparedTypeDeclCache::get`]) refuses to persist a transient body-less
/// result as genuine declaration absence. `prepare_local_type_decl` collapses
/// the two for direct/standalone callers that do not admit into that slot.
pub(crate) fn prepare_local_type_decl_outcome(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> PreparedDeclOutcome<PreparedTypeDecl> {
    prepare_local_type_decl_outcome_with_base(
        canonical_id,
        state,
        owner,
        symbol_name,
        dep_edges,
        import_canonicalization,
        None,
        interner,
    )
}

/// [`prepare_local_type_decl_outcome`] with an optional caller-owned per-file
/// TYPE-space `name_resolution` base table
/// ([`build_type_name_resolution_base`]): a `Some` caller (the prepared-decl
/// cache, which builds the base once per bundle) shares it via `Arc` across
/// every non-namespaced decl of the file; `None` builds the table fresh —
/// the pre-split per-call behavior for standalone entry points.
fn prepare_local_type_decl_outcome_with_base(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    shared_name_resolution_base: Option<&SharedNameResolutionBase>,
    interner: &IdentityInterner,
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
        if state.has_type_symbol_in(owner, symbol_name) {
            match state.type_decl_outcome_in(owner, symbol_name) {
                DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
                DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
                DemandOutcome::Ready(Some(lowered)) => {
                    (lowered, state.type_deps_in(owner, symbol_name), None)
                }
            }
        } else {
            match state.augmentation_type_decl_outcome_in(&global_scope, owner, symbol_name) {
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
    if state.is_import_local_in(owner, symbol_name) && !state.has_type_symbol_in(owner, symbol_name)
    {
        return PreparedDeclOutcome::Ready(None);
    }

    match prepare_type_decl_from_lowered(
        canonical_id,
        state,
        owner,
        symbol_name,
        lowered.as_ref(),
        deps.as_deref(),
        dep_edges,
        origin,
        import_canonicalization,
        shared_name_resolution_base,
        interner,
    ) {
        Ok(prepared) => PreparedDeclOutcome::Ready(Some(prepared)),
        Err(failure) => PreparedDeclOutcome::Failed(failure),
    }
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
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
    prepare_augmentation_type_decl_in(
        canonical_id,
        state,
        scope,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol_name,
        dep_edges,
        import_canonicalization,
        interner,
    )
}

pub fn prepare_augmentation_type_decl_in(
    canonical_id: &str,
    state: &ShallowFileState,
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
    prepare_augmentation_type_decl_outcome_in(
        &interner.intern(canonical_id),
        state,
        scope,
        owner,
        symbol_name,
        dep_edges,
        import_canonicalization,
        interner,
    )
    .into_result()
}

/// Lease-aware augmentation preparation. A broken-lease augmenter body
/// surfaces the DISTINCT `LeaseMiss` so cross-file stitching cannot warm-admit
/// an under-merged surface.
pub(crate) fn prepare_augmentation_type_decl_outcome_in(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> PreparedDeclOutcome<PreparedTypeDecl> {
    let lowered = match state.augmentation_type_decl_outcome_in(scope, owner, symbol_name) {
        DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
        DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
        DemandOutcome::Ready(Some(lowered)) => lowered,
    };
    // Augmentation bodies share the containing file's exact import namespace.
    // The canonicalization must therefore be the one built with the containing
    // prepared bundle under the active StoreView; an empty/default map would
    // discard an imported base symbol's authoritative target owner.
    match prepare_type_decl_from_lowered(
        canonical_id,
        state,
        owner,
        symbol_name,
        lowered.as_ref(),
        None,
        dep_edges,
        Some(scope),
        import_canonicalization,
        None,
        interner,
    ) {
        Ok(prepared) => PreparedDeclOutcome::Ready(Some(prepared)),
        Err(failure) => PreparedDeclOutcome::Failed(failure),
    }
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
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    lowered: &LoweredTypeDecl,
    deps: Option<&ClassifiedTypeDeps>,
    dep_edges: Option<&FxHashMap<String, String>>,
    origin: Option<&verter_semantic::analysis::type_eval::AugmentationScopeKind>,
    import_canonicalization: &ImportCanonicalization,
    shared_name_resolution_base: Option<&SharedNameResolutionBase>,
    interner: &IdentityInterner,
) -> Result<PreparedTypeDecl, PreparationFailure> {
    #[cfg(test)]
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| {
        count.set(count.get().saturating_add(1));
    });

    let empty_deps = ClassifiedTypeDeps::default();
    let deps = deps.unwrap_or(&empty_deps);
    let external_deps = deps
        .external_deps
        .iter()
        .map(|dep| {
            let identity = import_canonicalization
                .final_resolution
                .get(&verter_type_expr::DeclKey::new(
                    owner,
                    dep.local_name.as_str(),
                ))
                .ok_or_else(|| PreparationFailure::MissingExternalOwner {
                    local_name: dep.local_name.clone(),
                })?;
            Ok(PreparedExternalDep {
                canonical_id: identity.canonical_id.to_string(),
                owner: identity.owner,
                symbol_name: identity.symbol_name.to_string(),
            })
        })
        .collect::<Result<Vec<_>, PreparationFailure>>()?;

    // name_resolution: bare names in the body → resolved identities. The
    // file-symbol + import entries are a per-FILE artifact (identical for
    // every declaration of this file), so a non-namespaced decl SHARES the
    // caller's base table; only a namespaced decl builds a private table to
    // add its declaration-scoped sibling bindings. Key, identity symbol, and
    // identity canonical all share pooled allocations minted through the
    // store-owned interner — a local entry costs zero fresh string copies
    // once the pool is warm.
    let name_resolution = if !symbol_name.contains('.') {
        match shared_name_resolution_base {
            Some(base) => Arc::clone(base),
            None => Arc::new(build_type_name_resolution_base(
                canonical_id,
                state,
                owner,
                dep_edges,
                import_canonicalization,
                interner,
            )?),
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
        insert_file_symbol_resolutions(&mut table, canonical_id, state, owner, interner);
        add_namespace_sibling_resolutions(
            &mut table,
            state,
            owner,
            symbol_name,
            canonical_id,
            origin,
            interner,
        );
        // The declaration-specific external dependency frontier was validated
        // above. Populate this private namespace lookup with only identities
        // that have an exact canonical owner; unrelated unresolved imports
        // stay absent instead of failing the declaration a second time.
        insert_resolvable_type_space_imports(
            &mut table,
            state,
            owner,
            import_canonicalization,
            interner,
        );
        Arc::new(table)
    };

    finish_prepared_type_decl(
        canonical_id,
        state,
        owner,
        symbol_name,
        lowered,
        deps,
        external_deps,
        name_resolution,
        interner,
    )
}

/// Assemble the common prepared-declaration shell from producer-owned facts.
/// Both strict cache preparation and the projection-only authored-partial
/// path route here, so binder, member-index, heritage, and invalidation facts
/// cannot drift between the two representations.
#[allow(clippy::too_many_arguments)]
fn finish_prepared_type_decl(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    lowered: &LoweredTypeDecl,
    deps: &ClassifiedTypeDeps,
    external_deps: Vec<PreparedExternalDep>,
    name_resolution: SharedNameResolutionBase,
    interner: &IdentityInterner,
) -> Result<PreparedTypeDecl, PreparationFailure> {
    let mut prepared = PreparedTypeDecl::new(
        ResolvedRootIdentity::new_in_owner(
            Arc::clone(canonical_id),
            owner,
            interner.intern(symbol_name),
        ),
        lowered.kind,
    );
    // A merged interface carries its ordered contributor SLOTS so body
    // lowering interns a `MergedDecl` peer-merge carrier rather than
    // collapsing to a bare intersection.
    if lowered.body.is_merged() {
        prepared
            .set_merged_contributors(lowered.body.contributors().len())
            .map_err(|overflow| PreparationFailure::AuthoredOrdinalOverflow {
                count: overflow.count,
            })?;
    }
    prepared.type_parameters = lowered.narrow_type_parameters.clone();
    prepared.vue_ignored_heritage = Arc::clone(&lowered.vue_ignored_heritage);
    prepared.local_deps = deps.local_deps.clone();
    prepared.external_deps = external_deps;
    prepared.name_resolution = name_resolution;

    // Populate cache deps for invalidation
    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.as_ref().to_string(), hash_u64));
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
    Ok(prepared)
}

/// Insert the same-file symbol entries of the `name_resolution` table: every
/// TYPE and VALUE symbol name resolves to its own identity in the defining
/// file. The first pass of BOTH base tables (type-space and value-space) —
/// imports insert after (and therefore win per key) in both spaces.
fn insert_file_symbol_resolutions(
    table: &mut FxHashMap<Arc<str>, ResolvedRootIdentity>,
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    interner: &IdentityInterner,
) {
    for key in state.decl_bodies().header_index().type_headers.keys() {
        if key.owner != owner {
            continue;
        }
        let name = interner.intern(key.name.as_ref());
        table.insert(
            Arc::clone(&name),
            ResolvedRootIdentity::new_in_owner(Arc::clone(canonical_id), owner, name),
        );
    }
    for key in state.decl_bodies().header_index().value_headers.keys() {
        if key.owner != owner {
            continue;
        }
        let name = interner.intern(key.name.as_ref());
        table.insert(
            Arc::clone(&name),
            ResolvedRootIdentity::new_in_owner(Arc::clone(canonical_id), owner, name),
        );
    }
}

/// Insert only import identities whose exact owner was canonicalized.
///
/// Used by the shared TYPE-space lookup base and by the projection-only
/// authored-partial carrier. A missing entry stays absent; the exact
/// declaration's [`ClassifiedTypeDeps::external_deps`] is the strict
/// dependency frontier, so an unrelated unresolved import must not prevent a
/// declaration from preparing. No source owner, ordinary owner, or name-only
/// fallback is ever invented.
fn insert_resolvable_type_space_imports(
    table: &mut FxHashMap<Arc<str>, ResolvedRootIdentity>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) {
    for (local, _target) in state.owner_import_targets.iter() {
        if local.owner != owner || state.has_type_symbol_in(owner, local.name.as_ref()) {
            continue;
        }
        let Some(resolved) = import_canonicalization
            .final_resolution
            .get(&verter_type_expr::DeclKey::new(owner, local.name.as_ref()))
        else {
            continue;
        };
        table.insert(interner.intern(local.name.as_ref()), resolved.clone());
    }
}

/// Build an ephemeral projection carrier for an exact authored declaration
/// whose strict preparation failed with `MissingExternalOwner`.
///
/// The carrier copies the same producer-owned declaration facts as a strict
/// prepared declaration, but retains only canonicalized external identities.
/// It is returned directly to the active projection and is never admitted to
/// the write-once prepared slot.
fn prepare_authored_partial_type_decl(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
    if !state.has_type_symbol_in(owner, symbol_name) {
        return Ok(None);
    }
    let lowered = match state.type_decl_outcome_in(owner, symbol_name) {
        DemandOutcome::LeaseMiss | DemandOutcome::Ready(None) => return Ok(None),
        DemandOutcome::Ready(Some(lowered)) => lowered,
    };
    if state.is_import_local_in(owner, symbol_name) {
        return Ok(None);
    }

    let classified = state.type_deps_in(owner, symbol_name);
    let empty_deps = ClassifiedTypeDeps::default();
    let deps = classified.as_deref().unwrap_or(&empty_deps);
    let external_deps =
        deps.external_deps
            .iter()
            .filter_map(|dep| {
                let identity = import_canonicalization.final_resolution.get(
                    &verter_type_expr::DeclKey::new(owner, dep.local_name.as_str()),
                )?;
                Some(PreparedExternalDep {
                    canonical_id: identity.canonical_id.to_string(),
                    owner: identity.owner,
                    symbol_name: identity.symbol_name.to_string(),
                })
            })
            .collect();

    let mut table = FxHashMap::default();
    table.reserve(
        state.type_symbol_names().count()
            + state.value_symbol_names().count()
            + state.owner_import_targets.len(),
    );
    insert_file_symbol_resolutions(&mut table, canonical_id, state, owner, interner);
    if symbol_name.contains('.') {
        add_namespace_sibling_resolutions(
            &mut table,
            state,
            owner,
            symbol_name,
            canonical_id,
            None,
            interner,
        );
    }
    insert_resolvable_type_space_imports(
        &mut table,
        state,
        owner,
        import_canonicalization,
        interner,
    );

    finish_prepared_type_decl(
        canonical_id,
        state,
        owner,
        symbol_name,
        lowered.as_ref(),
        deps,
        external_deps,
        Arc::new(table),
        interner,
    )
    .map(Some)
}

/// Insert the VALUE-space import entries — same FINAL-definition
/// canonicalization as the type space, WITHOUT the type-symbol shadow skip:
/// in a prepared value decl's annotation scope the import binding wins over
/// a same-named local symbol.
fn insert_value_space_import_resolutions(
    table: &mut FxHashMap<Arc<str>, ResolvedRootIdentity>,
    _canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    _dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<(), PreparationFailure> {
    for (local, _target) in state.owner_import_targets.iter() {
        if local.owner != owner {
            continue;
        }
        let local_name = local.name.as_ref();
        let resolved = canonicalize_import_target(import_canonicalization, local_name, owner)?;
        table.insert(interner.intern(local_name), resolved);
    }
    Ok(())
}

/// Build the per-FILE TYPE-space `name_resolution` base table: file symbols,
/// then imports (type-symbol-shadowed). This is the COMPLETE table of every
/// non-namespaced prepared type decl of the file — the prepared-decl cache
/// builds it once per bundle and shares it via `Arc` across those decls; a
/// namespaced decl re-runs the same passes around its sibling bindings.
fn build_type_name_resolution_base(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    _dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<FxHashMap<Arc<str>, ResolvedRootIdentity>, PreparationFailure> {
    let mut table = FxHashMap::default();
    insert_file_symbol_resolutions(&mut table, canonical_id, state, owner, interner);
    insert_resolvable_type_space_imports(
        &mut table,
        state,
        owner,
        import_canonicalization,
        interner,
    );
    Ok(table)
}

/// Build the per-FILE VALUE-space `name_resolution` base table: file symbols,
/// then imports (unshadowed). The value space has no per-declaration bindings
/// at all, so this is the COMPLETE table of EVERY prepared value decl of the
/// file.
fn build_value_name_resolution_base(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<FxHashMap<Arc<str>, ResolvedRootIdentity>, PreparationFailure> {
    let mut table = FxHashMap::default();
    insert_file_symbol_resolutions(&mut table, canonical_id, state, owner, interner);
    insert_value_space_import_resolutions(
        &mut table,
        canonical_id,
        state,
        owner,
        dep_edges,
        import_canonicalization,
        interner,
    )?;
    Ok(table)
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
    name_resolution: &mut FxHashMap<Arc<str>, ResolvedRootIdentity>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    canonical_id: &Arc<str>,
    origin: Option<&verter_semantic::analysis::type_eval::AugmentationScopeKind>,
    interner: &IdentityInterner,
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
            for key in state
                .decl_bodies()
                .header_index()
                .type_headers
                .keys()
                .chain(state.decl_bodies().header_index().value_headers.keys())
                .filter(|key| key.owner == owner)
            {
                let dep_name = key.name.as_ref();
                if let Some(member) = dep_name.strip_prefix(&dotted_prefix) {
                    if !member.contains('.') {
                        name_resolution.insert(
                            interner.intern(member),
                            ResolvedRootIdentity::new_in_owner(
                                Arc::clone(canonical_id),
                                owner,
                                interner.intern(dep_name),
                            ),
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
                            interner.intern(member),
                            ResolvedRootIdentity::new_in_owner(
                                Arc::clone(canonical_id),
                                owner,
                                interner.intern(name),
                            ),
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
    interner: &IdentityInterner,
) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
    let Some(ExportTarget::Local { owner, symbol_name }) = state.export_target(exported_name)
    else {
        return Ok(None);
    };

    // The direct/standalone prep entry carries no precomputed barrel-final
    // canonicalization (that is threaded by the host-materialised caches); a
    // re-export hop falls back to the barrel here. Production resolution goes
    // through the caches, which thread the real canonicalization.
    let Some(mut prepared) = prepare_local_type_decl_in(
        canonical_id,
        state,
        *owner,
        symbol_name,
        dep_edges,
        &ImportCanonicalization::default(),
        interner,
    )?
    else {
        return Ok(None);
    };
    prepared.exported_name = Some(exported_name.to_string());
    prepared.provenance.route_kind = Some("direct".to_string());
    Ok(Some(prepared))
}

/// Prepare a local value declaration from a canonical shallow file state.
pub fn prepare_local_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Option<PreparedValueDecl> {
    prepare_local_value_decl_outcome(
        &interner.intern(canonical_id),
        state,
        symbol_name,
        dep_edges,
        import_canonicalization,
        interner,
    )
    .into_result()
    .ok()
    .flatten()
}

/// Lease-aware variant of [`prepare_local_value_decl`] — see
/// [`prepare_local_type_decl_outcome`] for the no-warm contract that keeps a
/// broken-lease demand from committing a body-less value decl into the
/// write-once slot cache ([`PreparedValueDeclCache::get`]).
pub(crate) fn prepare_local_value_decl_outcome(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> PreparedDeclOutcome<PreparedValueDecl> {
    prepare_local_value_decl_outcome_in(
        canonical_id,
        state,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol_name,
        dep_edges,
        import_canonicalization,
        interner,
    )
}

pub(crate) fn prepare_local_value_decl_outcome_in(
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> PreparedDeclOutcome<PreparedValueDecl> {
    prepare_local_value_decl_outcome_with_base(
        canonical_id,
        state,
        owner,
        symbol_name,
        dep_edges,
        import_canonicalization,
        None,
        interner,
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
    canonical_id: &Arc<str>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    shared_name_resolution_base: Option<&SharedNameResolutionBase>,
    interner: &IdentityInterner,
) -> PreparedDeclOutcome<PreparedValueDecl> {
    let lowered: Arc<LoweredValueDecl> = match state.value_decl_outcome_in(owner, symbol_name) {
        DemandOutcome::LeaseMiss => return PreparedDeclOutcome::LeaseMiss,
        DemandOutcome::Ready(None) => return PreparedDeclOutcome::Ready(None),
        DemandOutcome::Ready(Some(lowered)) => lowered,
    };
    if state.is_import_local_in(owner, symbol_name) {
        return PreparedDeclOutcome::Ready(None);
    }

    let mut prepared = PreparedValueDecl::new(
        ResolvedRootIdentity::new_in_owner(
            Arc::clone(canonical_id),
            owner,
            interner.intern(symbol_name),
        ),
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
    // fallback applies), shared across every value decl of the file. Keys and
    // identities are interned `Arc<str>` allocations minted through the
    // store-owned pool.
    prepared.name_resolution = match shared_name_resolution_base {
        Some(base) => Arc::clone(base),
        None => match build_value_name_resolution_base(
            canonical_id,
            state,
            owner,
            dep_edges,
            import_canonicalization,
            interner,
        ) {
            Ok(base) => Arc::new(base),
            Err(failure) => return PreparedDeclOutcome::Failed(failure),
        },
    };

    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.as_ref().to_string(), hash_u64));

    PreparedDeclOutcome::Ready(Some(prepared))
}

/// Prepare a named exported value declaration after routing has selected the
/// defining file.
pub fn prepare_exported_value_decl(
    canonical_id: &str,
    state: &ShallowFileState,
    exported_name: &str,
    dep_edges: Option<&FxHashMap<String, String>>,
    interner: &IdentityInterner,
) -> Option<PreparedValueDecl> {
    let ExportTarget::Local { owner, symbol_name } = state.export_target(exported_name)? else {
        return None;
    };
    let canonical_id = interner.intern(canonical_id);
    let mut prepared = prepare_local_value_decl_outcome_in(
        &canonical_id,
        state,
        *owner,
        symbol_name,
        dep_edges,
        &ImportCanonicalization::default(),
        interner,
    )
    .into_result()
    .ok()
    .flatten()?;
    prepared.exported_name = Some(exported_name.to_string());
    Some(prepared)
}

#[derive(Clone)]
pub struct PreparedTypeDeclCache {
    canonical_id: Arc<str>,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
    /// Handle to the store-owned identity intern pool (per-store
    /// lifetime) — the cold build below mints every identity through it.
    interner: Arc<IdentityInterner>,
    slots: PreparedTypeDeclSlots,
    /// Per-FILE TYPE-space `name_resolution` base table, built lazily ONCE
    /// per cache (`OnceLock` collapses concurrent initializers) and shared
    /// via `Arc` by every non-namespaced prepared type decl this cache
    /// builds — the per-declaration table rebuild this replaces walked every
    /// file symbol + import per decl.
    name_resolution_bases: OwnerNameResolutionBases,
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

    fn prepare_augmentation_type_decl_outcome_in(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> PreparedDeclOutcome<PreparedTypeDecl> {
        prepare_augmentation_type_decl_outcome_in(
            &self.canonical_id,
            self.state.as_ref(),
            scope,
            owner,
            symbol_name,
            (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            &self.import_canonicalization,
            &self.interner,
        )
    }

    pub fn contains_key(&self, symbol_name: &str) -> bool {
        self.contains_key_in(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
    }

    pub fn contains_key_in(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> bool {
        self.slots
            .contains_key(&verter_type_expr::DeclKey::new(owner, symbol_name))
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

    pub fn get(
        &self,
        symbol_name: &str,
    ) -> Result<Option<Arc<PreparedTypeDecl>>, PreparationFailure> {
        self.get_in(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
    }

    pub fn get_in(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<Option<Arc<PreparedTypeDecl>>, PreparationFailure> {
        let key = verter_type_expr::DeclKey::new(owner, symbol_name);
        let Some(slot) = self.slots.get(&key) else {
            return Ok(None);
        };
        // Warm fast path — no gate.
        if let Some(cached) = slot.value.get() {
            return Ok(cached.clone());
        }
        // Cold: serialise the build under the resettable in-flight gate
        // (cooperative wait, never a spin), then re-check warm — a concurrent
        // winner may have committed while we blocked, so we reuse its result
        // rather than rebuild (single-flight for the successful case).
        let _gate = slot.build_gate.lock();
        if let Some(cached) = slot.value.get() {
            return Ok(cached.clone());
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
        let base_cell = self
            .name_resolution_bases
            .get(&owner)
            .expect("every prepared slot owner has a name-resolution base");
        let name_resolution_base = if let Some(base) = base_cell.get() {
            base
        } else {
            let built = Arc::new(build_type_name_resolution_base(
                &self.canonical_id,
                self.state.as_ref(),
                owner,
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
                &self.import_canonicalization,
                &self.interner,
            )?);
            base_cell.get_or_init(|| built)
        };
        match prepare_local_type_decl_outcome_with_base(
            &self.canonical_id,
            self.state.as_ref(),
            owner,
            symbol_name,
            (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            &self.import_canonicalization,
            Some(name_resolution_base),
            &self.interner,
        ) {
            PreparedDeclOutcome::LeaseMiss => {
                // Broken decl-body lease: leave the write-once slot VACANT
                // (retry on the next live-lease demand) AND mark the
                // generalized non-cacheability rail so an enclosing traced
                // compute refuses admission.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                Ok(None)
            }
            PreparedDeclOutcome::Ready(value) => {
                let committed = value.map(Arc::new);
                let _ = slot.value.set(committed.clone());
                Ok(committed)
            }
            PreparedDeclOutcome::Failed(failure) => Err(failure),
        }
    }

    /// Projection lookup that preserves a recoverable typed preparation
    /// failure as an ephemeral exact-owner authored declaration.
    ///
    /// [`Self::get_in`] remains the strict cache API. This method never commits
    /// an `AuthoredPartial` value into the write-once slot. Callers must retain
    /// the typed unresolved-owner debt through normal reference resolution and
    /// mark the result partial/non-cacheable only when the debt is still live at
    /// query exit.
    pub fn get_in_for_projection(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> PreparedTypeDeclResolution {
        let root_identity = ResolvedRootIdentity::new_in_owner(
            Arc::clone(&self.canonical_id),
            owner,
            self.interner.intern(symbol_name),
        );
        match self.get_in(owner, symbol_name) {
            Ok(Some(declaration)) => PreparedTypeDeclResolution::Complete(declaration),
            Ok(None) => PreparedTypeDeclResolution::Missing,
            Err(failure @ PreparationFailure::MissingExternalOwner { .. }) => {
                match prepare_authored_partial_type_decl(
                    &self.canonical_id,
                    self.state.as_ref(),
                    owner,
                    symbol_name,
                    &self.import_canonicalization,
                    &self.interner,
                ) {
                    Ok(Some(declaration)) => PreparedTypeDeclResolution::AuthoredPartial {
                        root_identity,
                        declaration: Arc::new(declaration),
                        failure,
                    },
                    Ok(None) => PreparedTypeDeclResolution::Failed {
                        root_identity,
                        failure,
                    },
                    Err(recovery_failure) => PreparedTypeDeclResolution::Failed {
                        root_identity,
                        failure: recovery_failure,
                    },
                }
            }
            Err(failure) => PreparedTypeDeclResolution::Failed {
                root_identity,
                failure,
            },
        }
    }

    /// Test observability: whether the write-once slot for `symbol_name` has a
    /// COMMITTED entry (never a validity signal). A broken-lease demand must
    /// leave the slot VACANT — no wrong-empty `None` warm-admitted for a real
    /// symbol — so this returns `false` after a lease-miss.
    #[cfg(test)]
    pub(crate) fn slot_committed_for_test(&self, symbol_name: &str) -> bool {
        self.slots
            .get(&verter_type_expr::DeclKey::new(
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol_name,
            ))
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
    /// Handle to the store-owned identity intern pool (per-store
    /// lifetime) — the cold build below mints every identity through it.
    interner: Arc<IdentityInterner>,
    slots: PreparedValueDeclSlots,
    /// Per-FILE VALUE-space `name_resolution` base table — see
    /// [`PreparedTypeDeclCache::name_resolution_base`]; the value space has
    /// no per-declaration bindings, so EVERY prepared value decl shares it.
    name_resolution_bases: OwnerNameResolutionBases,
}

impl PreparedValueDeclCache {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key(&self, symbol_name: &str) -> bool {
        self.contains_key_in(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
    }

    pub fn contains_key_in(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> bool {
        self.slots
            .contains_key(&verter_type_expr::DeclKey::new(owner, symbol_name))
    }

    pub fn get(&self, symbol_name: &str) -> Option<Arc<PreparedValueDecl>> {
        self.get_in(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
    }

    pub fn get_in(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        let slot = self
            .slots
            .get(&verter_type_expr::DeclKey::new(owner, symbol_name))?;
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
        let base_cell = self
            .name_resolution_bases
            .get(&owner)
            .expect("every prepared slot owner has a name-resolution base");
        let name_resolution_base = if let Some(base) = base_cell.get() {
            base
        } else {
            let built = match build_value_name_resolution_base(
                &self.canonical_id,
                self.state.as_ref(),
                owner,
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
                &self.import_canonicalization,
                &self.interner,
            ) {
                Ok(base) => Arc::new(base),
                Err(_) => return None,
            };
            base_cell.get_or_init(|| built)
        };
        match prepare_local_value_decl_outcome_with_base(
            &self.canonical_id,
            self.state.as_ref(),
            owner,
            symbol_name,
            (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
            &self.import_canonicalization,
            Some(name_resolution_base),
            &self.interner,
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
            PreparedDeclOutcome::Failed(_) => None,
        }
    }

    /// Test observability — see [`PreparedTypeDeclCache::slot_committed_for_test`].
    #[cfg(test)]
    pub(crate) fn slot_committed_for_test(&self, symbol_name: &str) -> bool {
        self.slots
            .get(&verter_type_expr::DeclKey::new(
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol_name,
            ))
            .is_some_and(|slot| slot.value.get().is_some())
    }
}

/// Owner-exact declaration-scope surfaces retained by a prepared bundle.
///
/// The lexical owner is the key in [`PreparedDeclBundle::owner_scopes`], so
/// every map here can stay string-keyed without aliasing a same-name binding
/// from another script region.
#[derive(Clone, Default)]
pub struct PreparedOwnerScope {
    /// Resolved imports visible in this lexical owner.
    pub import_bindings: FxHashMap<String, ImportBinding>,
    /// Same-file type names visible in this lexical owner.
    pub scope_type_names: FxHashSet<String>,
    /// Same-file value names visible in this lexical owner.
    pub scope_value_names: FxHashSet<String>,
    /// Script-setup generic parameters visible in this lexical owner.
    pub script_setup_type_bindings: FxHashMap<String, TypeParamBinding>,
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
    /// Exact declaration-scope surfaces partitioned by lexical owner.
    /// There is deliberately no ordinary-owner fallback: an absent owner has
    /// an empty scope rather than inheriting module-zero declarations.
    pub owner_scopes: FxHashMap<TopLevelOwnerId, PreparedOwnerScope>,
}

impl PreparedDeclBundle {
    /// Exact declaration scope for `owner`.
    #[must_use]
    pub fn owner_scope(&self, owner: TopLevelOwnerId) -> Option<&PreparedOwnerScope> {
        self.owner_scopes.get(&owner)
    }

    /// Prepare an augmentation contributor against the same pinned state,
    /// dependency edges, and exact import canonicalization as this bundle.
    pub(crate) fn prepare_augmentation_type_decl_outcome_in(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        owner: TopLevelOwnerId,
        symbol_name: &str,
    ) -> PreparedDeclOutcome<PreparedTypeDecl> {
        self.prepared_type_decls
            .prepare_augmentation_type_decl_outcome_in(scope, owner, symbol_name)
    }

    /// Plain result-shaped sibling for locator replay. Lease misses retain the
    /// existing non-cacheability fan-out performed by `into_result`.
    pub(crate) fn prepare_augmentation_type_decl_in(
        &self,
        scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
        owner: TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<Option<PreparedTypeDecl>, PreparationFailure> {
        self.prepare_augmentation_type_decl_outcome_in(scope, owner, symbol_name)
            .into_result()
    }
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
    interner: &Arc<IdentityInterner>,
) -> PreparedDeclBundle {
    let dep_edges = Arc::new(dep_edges);
    let import_canonicalization = Arc::new(import_canonicalization);

    let header_index = state.decl_bodies().header_index();
    let mut owner_scopes: FxHashMap<TopLevelOwnerId, PreparedOwnerScope> = FxHashMap::default();

    // Same-file inventories are already keyed by exact lexical owner.
    for key in header_index.type_headers.keys() {
        owner_scopes
            .entry(key.owner)
            .or_default()
            .scope_type_names
            .insert(key.name.to_string());
    }
    for key in header_index.value_headers.keys() {
        owner_scopes
            .entry(key.owner)
            .or_default()
            .scope_value_names
            .insert(key.name.to_string());
    }
    // Synthesised value declarations are ordinary-file declarations and do
    // not live in the parser header table.
    for name in state.value_symbol_names() {
        owner_scopes
            .entry(TopLevelOwnerId::ordinary_file())
            .or_default()
            .scope_value_names
            .insert(name.to_string());
    }

    // Build import bindings from the authoritative owner-qualified import
    // table. One binding per exact `(owner, local-name)` key.
    for (local_key, target) in state.owner_import_targets.iter() {
        let resolved_id = if target.canonical_id.is_empty() {
            dep_edges.get(&target.source_specifier).cloned()
        } else {
            Some(target.canonical_id.clone())
        };
        if let Some(resolved_id) = resolved_id {
            owner_scopes
                .entry(local_key.owner)
                .or_default()
                .import_bindings
                .insert(
                    local_key.name.to_string(),
                    ImportBinding {
                        canonical_id: resolved_id,
                        exported_name: target.imported_name.clone(),
                    },
                );
        }
    }

    // Vue `<script setup generic>` parameters belong exclusively to the
    // setup/instance lexical owner. They must never shadow names in module
    // script or any other carrier region.
    if !script_setup_type_bindings.is_empty() {
        owner_scopes
            .entry(TopLevelOwnerId::instance(0))
            .or_default()
            .script_setup_type_bindings = script_setup_type_bindings;
    }

    let owner_whole_hash = state.whole_hash;
    PreparedDeclBundle {
        owner_whole_hash,
        prepared_type_decls: build_prepared_type_decl_cache(
            canonical_id,
            Arc::clone(&state),
            Arc::clone(&dep_edges),
            Arc::clone(&import_canonicalization),
            interner,
        ),
        prepared_value_decls: build_prepared_value_decl_cache(
            canonical_id,
            Arc::clone(&state),
            Arc::clone(&dep_edges),
            Arc::clone(&import_canonicalization),
            interner,
        ),
        dep_edges,
        owner_scopes,
    }
}

/// Build the host-owned prepared type declaration cache for one defining file.
pub fn build_prepared_type_decl_cache(
    canonical_id: &str,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
    interner: &Arc<IdentityInterner>,
) -> PreparedTypeDeclCache {
    let mut slots: FxHashMap<verter_type_expr::DeclKey, PreparedTypeDeclSlot> = state
        .decl_bodies()
        .header_index()
        .type_headers
        .keys()
        .cloned()
        .map(|key| (key, Arc::new(PreparedDeclSlot::new())))
        .collect();
    // Global-augmentation declarations (`declare global { interface N {} }`)
    // are resolvable by bare name through `prepare_local_type_decl`'s global
    // fallback, so they need a prepared-decl slot even though they never enter
    // the file surface. (A name that IS a file symbol already has a slot and
    // takes precedence.)
    for (scope, key) in state.augmentation_type_decl_keys() {
        if matches!(
            scope,
            verter_semantic::analysis::type_eval::AugmentationScopeKind::Global
        ) && !slots.contains_key(key)
            && !state.is_import_local_in(key.owner, key.name.as_ref())
        {
            slots.insert(key.clone(), Arc::new(PreparedDeclSlot::new()));
        }
    }
    let name_resolution_bases = slots
        .keys()
        .map(|key| key.owner)
        .collect::<FxHashSet<_>>()
        .into_iter()
        .map(|owner| (owner, Arc::new(OnceLock::new())))
        .collect();

    PreparedTypeDeclCache {
        // Pool the canonical ONCE: every identity minted for this file (and
        // the sibling value cache) then shares the pooled allocation.
        canonical_id: interner.intern(canonical_id),
        state,
        dep_edges,
        import_canonicalization,
        interner: Arc::clone(interner),
        slots: Arc::new(slots),
        name_resolution_bases: Arc::new(name_resolution_bases),
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
    interner: &Arc<IdentityInterner>,
) -> PreparedValueDeclCache {
    let slots: FxHashMap<verter_type_expr::DeclKey, PreparedValueDeclSlot> = state
        .decl_bodies()
        .header_index()
        .value_headers
        .keys()
        .filter(|key| !state.is_import_local_in(key.owner, key.name.as_ref()))
        .cloned()
        .map(|key| (key, Arc::new(PreparedDeclSlot::new())))
        .collect();
    let name_resolution_bases = slots
        .keys()
        .map(|key| key.owner)
        .collect::<FxHashSet<_>>()
        .into_iter()
        .map(|owner| (owner, Arc::new(OnceLock::new())))
        .collect();

    PreparedValueDeclCache {
        canonical_id: interner.intern(canonical_id),
        state,
        dep_edges,
        import_canonicalization,
        interner: Arc::clone(interner),
        slots: Arc::new(slots),
        name_resolution_bases: Arc::new(name_resolution_bases),
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
