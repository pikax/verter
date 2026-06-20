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
use crate::decl_body_memo::{LoweredTypeDecl, LoweredValueDecl};

type PreparedTypeDeclSlot = Arc<OnceLock<Option<Arc<PreparedTypeDecl>>>>;
type PreparedTypeDeclSlots = Arc<FxHashMap<String, PreparedTypeDeclSlot>>;
type PreparedValueDeclSlot = Arc<OnceLock<Option<Arc<PreparedValueDecl>>>>;
type PreparedValueDeclSlots = Arc<FxHashMap<String, PreparedValueDeclSlot>>;

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
    // stitches onto another module's surface).
    let global_scope = AugmentationScopeKind::Global;
    let (lowered, deps, origin): (_, _, Option<&AugmentationScopeKind>) =
        if state.has_type_symbol(symbol_name) {
            (
                state.type_decl(symbol_name)?,
                state.type_deps(symbol_name),
                None,
            )
        } else {
            (
                state.augmentation_type_decl(&global_scope, symbol_name)?,
                None,
                Some(&global_scope),
            )
        };
    if state.is_import_local(symbol_name) {
        return None;
    }

    Some(prepare_type_decl_from_lowered(
        canonical_id,
        state,
        symbol_name,
        lowered.as_ref(),
        deps.as_deref(),
        dep_edges,
        origin,
        import_canonicalization,
    ))
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
    let lowered = state.augmentation_type_decl(scope, symbol_name)?;
    // Augmentation-scope decls are off the barrel-final import path (their
    // bodies stitch onto another module's surface, not a re-export hop), so the
    // barrel fallback applies for any import they reference.
    Some(prepare_type_decl_from_lowered(
        canonical_id,
        state,
        symbol_name,
        lowered.as_ref(),
        None,
        dep_edges,
        Some(scope),
        &ImportCanonicalization::default(),
    ))
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
) -> PreparedTypeDecl {
    #[cfg(test)]
    PREPARED_TYPE_DECL_BUILD_COUNT.with(|count| {
        count.set(count.get().saturating_add(1));
    });

    let mut prepared = PreparedTypeDecl::new(
        ResolvedRootIdentity::new(canonical_id, symbol_name),
        lowered.kind,
        lowered.body.lookup_object().into_owned(),
    );
    // A merged interface carries its ordered contributor bodies so body
    // lowering interns a `MergedDecl` peer-merge carrier rather than collapsing
    // to a bare intersection.
    if lowered.body.is_merged() {
        prepared.merged_contributors = lowered.body.contributors().to_vec();
    }
    prepared.type_parameters = lowered.type_parameters.clone();
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

    // Build name_resolution: maps bare names in the body to resolved identities
    // Local deps resolve to the same file
    for dep_name in state.type_symbol_names() {
        prepared.name_resolution.insert(
            dep_name.to_string(),
            ResolvedRootIdentity::new(canonical_id, dep_name),
        );
    }
    for dep_name in state.value_symbol_names() {
        prepared.name_resolution.insert(
            dep_name.to_string(),
            ResolvedRootIdentity::new(canonical_id, dep_name),
        );
    }
    // Namespace-member scoping: inside `namespace NS { ... }`, an unqualified
    // reference to a sibling member `M` resolves to `NS.M` (TS namespace
    // scope). The shallow inventory indexes the member under its QUALIFIED name
    // (`NS.M`), so a sibling body `Ref("M")` would otherwise miss. For a
    // namespaced decl (whose `symbol_name` carries an `NS.` prefix), map each
    // DIRECT sibling's bare name to its qualified identity, drawn from the
    // inventory visible for the decl's `origin` scope. Inserted AFTER the
    // unqualified loops so the sibling binding takes precedence over an outer
    // same-named type — the TS namespace-scope rule.
    add_namespace_sibling_resolutions(
        &mut prepared.name_resolution,
        state,
        symbol_name,
        canonical_id,
        origin,
    );
    // External deps resolve through import bindings → the FINAL defining
    // file. When the import is a re-export hop, the canonicalization
    // (precomputed at bundle materialisation through the SAME route authority
    // the carrier fallback / dispatch fallthrough use) carries the final
    // `(canonical, name)`; otherwise the barrel fallback applies. This keeps
    // the eager fast-path's stored identity at the FINAL definition rather than
    // the intermediate barrel.
    for (local_name, target) in state.import_targets.iter() {
        let resolved = canonicalize_import_target(
            import_canonicalization,
            canonical_id,
            dep_edges,
            local_name,
            target,
        );
        prepared
            .name_resolution
            .insert(local_name.clone(), resolved);
    }

    // Populate cache deps for invalidation
    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.to_string(), hash_u64));
    prepared.cache_deps.local_closure_participants = deps.local_deps.clone();

    prepared.build_member_index();
    prepared.classify_wrapper_shape();
    prepared.classify_projection();
    prepared
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
    let lowered: Arc<LoweredValueDecl> = state.value_decl(symbol_name)?;
    if state.is_import_local(symbol_name) {
        return None;
    }

    let mut prepared = PreparedValueDecl::new(
        ResolvedRootIdentity::new(canonical_id, symbol_name),
        lowered.kind,
    );
    prepared.type_annotation = lowered.type_annotation.clone();
    prepared.signatures = lowered.signatures.clone();
    prepared.object_shape = lowered.object_shape.clone();
    prepared.enum_members = lowered.enum_members.clone();

    for local_name in state.type_symbol_names() {
        prepared.name_resolution.insert(
            local_name.to_string(),
            ResolvedRootIdentity::new(canonical_id, local_name),
        );
    }
    for local_name in state.value_symbol_names() {
        prepared.name_resolution.insert(
            local_name.to_string(),
            ResolvedRootIdentity::new(canonical_id, local_name),
        );
    }

    // Build name_resolution for type annotations / `typeof` references that
    // reference imported or local symbols in the defining file. A re-export-hop
    // import canonicalizes to the FINAL defining file (the barrel-final
    // canonicalization, same authority as the type rail); otherwise the barrel
    // fallback applies.
    for (local_name, target) in state.import_targets.iter() {
        let resolved = canonicalize_import_target(
            import_canonicalization,
            canonical_id,
            dep_edges,
            local_name,
            target,
        );
        prepared
            .name_resolution
            .insert(local_name.clone(), resolved);
    }

    let hash_u64 = u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default());
    prepared.cache_deps.defining_file = Some((canonical_id.to_string(), hash_u64));

    Some(prepared)
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
        slot.get_or_init(|| {
            prepare_local_type_decl(
                self.canonical_id.as_ref(),
                self.state.as_ref(),
                symbol_name,
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
                &self.import_canonicalization,
            )
            .map(Arc::new)
        })
        .clone()
    }
}

#[derive(Clone)]
pub struct PreparedValueDeclCache {
    canonical_id: Arc<str>,
    state: Arc<ShallowFileState>,
    dep_edges: Arc<FxHashMap<String, String>>,
    import_canonicalization: Arc<ImportCanonicalization>,
    slots: PreparedValueDeclSlots,
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
        slot.get_or_init(|| {
            prepare_local_value_decl(
                self.canonical_id.as_ref(),
                self.state.as_ref(),
                symbol_name,
                (!self.dep_edges.is_empty()).then_some(self.dep_edges.as_ref()),
                &self.import_canonicalization,
            )
            .map(Arc::new)
        })
        .clone()
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
    let mut import_bindings = FxHashMap::default();
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
    let mut slots: FxHashMap<String, Arc<OnceLock<Option<Arc<PreparedTypeDecl>>>>> = state
        .type_symbol_names()
        .filter(|symbol_name| !state.is_import_local(symbol_name))
        .map(|symbol_name| (symbol_name.to_string(), Arc::new(OnceLock::new())))
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
            slots.insert(name.to_string(), Arc::new(OnceLock::new()));
        }
    }

    PreparedTypeDeclCache {
        canonical_id: Arc::from(canonical_id),
        state,
        dep_edges,
        import_canonicalization,
        slots: Arc::new(slots),
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
        .map(|symbol_name| (symbol_name.to_string(), Arc::new(OnceLock::new())))
        .collect();

    PreparedValueDeclCache {
        canonical_id: Arc::from(canonical_id),
        state,
        dep_edges,
        import_canonicalization,
        slots: Arc::new(slots),
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
mod tests {
    use std::sync::Arc;

    use verter_compiler::utils::oxc::script::type_surface::{
        analyze_external_type_source, AnalyzedExternalTypeSource,
    };
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    use verter_semantic::analysis::type_eval_build::parse_and_build_env;
    use verter_semantic::analysis::Hash16;

    use super::*;

    fn make_analysis(source: &str) -> Arc<AnalyzedExternalTypeSource> {
        let alloc = oxc_allocator::Allocator::new();
        Arc::new(analyze_external_type_source(source, &alloc))
    }

    #[test]
    fn prepares_local_exported_type_decl_from_shallow_file_state() {
        let source = "export interface Props { label: string }";
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
            .expect("Props should prepare");

        assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
        assert_eq!(prepared.root_identity.symbol_name, "Props");
        assert_eq!(prepared.exported_name.as_deref(), Some("Props"));

        // Member index should be auto-populated for interface with properties
        assert!(
            prepared.member_index.contains_key("label"),
            "member index should contain 'label' property"
        );
    }

    #[test]
    fn prepares_local_value_decl_from_shallow_file_state() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_value_decl("/src/types.ts", &state, "defaults", None)
            .expect("defaults should prepare");

        assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
        assert_eq!(prepared.root_identity.symbol_name, "defaults");
        assert_eq!(prepared.exported_name.as_deref(), Some("defaults"));
        assert_eq!(prepared.kind, ValueDeclKind::Const);
        assert!(prepared.type_annotation.is_some());
    }

    #[test]
    fn prepared_type_decl_name_resolution_includes_typeof_imports() {
        let source = r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));
        let dep_edges = FxHashMap::from_iter([
            ("./types".to_string(), "/src/types.ts".to_string()),
            ("./theme".to_string(), "/src/theme.ts".to_string()),
        ]);

        let prepared =
            prepare_exported_type_decl("/src/button-types.ts", &state, "Button", Some(&dep_edges))
                .expect("Button should prepare");

        assert_eq!(
            prepared
                .name_resolution
                .get("theme")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/theme.ts", "theme"))
        );
    }

    #[test]
    fn prepared_type_decl_falls_back_to_canonical_relative_targets_without_dep_edges() {
        let source = r#"
import type { ComponentConfig } from './tv.ts'
import type { AppConfig } from './schema.ts'
import theme from './theme.ts'

export type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/Button.vue", &state, "Button", None)
            .expect("Button should prepare");

        assert_eq!(
            prepared
                .name_resolution
                .get("ComponentConfig")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/tv.ts", "ComponentConfig"))
        );
        assert_eq!(
            prepared
                .name_resolution
                .get("AppConfig")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/schema.ts", "AppConfig"))
        );
        assert_eq!(
            prepared
                .name_resolution
                .get("theme")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/theme.ts", "default"))
        );
    }

    #[test]
    fn prepared_value_decl_falls_back_to_canonical_relative_targets_without_dep_edges() {
        let source = r#"
import type { Theme } from './theme.ts'

export const defaults: Theme = {} as Theme
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_value_decl("/src/Button.vue", &state, "defaults", None)
            .expect("defaults should prepare");

        assert_eq!(
            prepared
                .name_resolution
                .get("Theme")
                .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
            Some(("/src/theme.ts", "Theme"))
        );
    }

    #[test]
    fn does_not_prepare_reexport_without_frontier_routing() {
        let source = r#"export { Props } from "./inner""#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        assert!(prepare_exported_type_decl("/src/barrel.ts", &state, "Props", None).is_none());
    }

    #[test]
    fn prepared_type_decl_populates_deps_from_shallow_symbol() {
        let source = r#"
import { Inner } from "./inner"
type Local = { x: number }
export interface Props { child: Inner; data: Local }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
            .expect("Props should prepare");

        // Should have a member index for 'child' and 'data'
        assert!(
            prepared.member_index.contains_key("child"),
            "member index should contain 'child'"
        );
        assert!(
            prepared.member_index.contains_key("data"),
            "member index should contain 'data'"
        );
    }

    #[test]
    fn builds_local_prepared_decl_caches_from_shallow_file_state() {
        let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let type_cache = build_prepared_type_decl_cache(
            "/src/types.ts",
            Arc::new(state.clone()),
            Arc::new(FxHashMap::default()),
            Arc::new(ImportCanonicalization::default()),
        );
        let value_cache = build_prepared_value_decl_cache(
            "/src/types.ts",
            Arc::new(state),
            Arc::new(FxHashMap::default()),
            Arc::new(ImportCanonicalization::default()),
        );

        assert!(type_cache.contains_key("Props"));
        assert!(value_cache.contains_key("defaults"));
    }

    #[test]
    fn prepared_type_decl_build_counter_is_thread_local() {
        reset_prepared_type_decl_build_count_for_tests();

        let source = "export interface Props { label: string }";
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
            .expect("Props should prepare");
        assert_eq!(prepared.root_identity.symbol_name, "Props");
        assert_eq!(prepared_type_decl_build_count_for_tests(), 1);

        let other_thread_count = std::thread::spawn(prepared_type_decl_build_count_for_tests)
            .join()
            .expect("thread-local counter probe should join cleanly");
        assert_eq!(
            other_thread_count, 0,
            "prepared decl build counters should not leak across test threads",
        );
    }

    // Scope-matched namespace-sibling binding: a namespaced decl binds bare
    // sibling names ONLY from the inventory visible for its declaration ORIGIN,
    // and only where that sibling has a buildable prepared decl. The two tests
    // below pin the two regressions the shared (origin-blind) helper introduced.

    #[test]
    fn module_augmentation_namespace_decl_does_not_bind_global_sibling() {
        // A `declare module "ext" { namespace NS { ... } }` decl resolves its
        // OWN module siblings — NEVER a global-augmentation `namespace NS`
        // sibling of the same namespace name. The origin-blind helper scanned
        // `Global` augmentation keys unconditionally and leaked the sibling
        // `GlobalOnly` (declared in `declare global { namespace NS }`) into the
        // Module-scope decl's `name_resolution`, crossing scopes. Module
        // siblings are not consumable today (no Module-scope prepared-decl
        // slot), so the Module arm binds NOTHING.
        use verter_semantic::analysis::type_eval::AugmentationScopeKind;
        let source = r#"
export {};
declare global { namespace NS { type GlobalOnly = { g: string } } }
declare module "ext" { namespace NS { interface Foo { x: GlobalOnly } } }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        // Harness invariant the leak depends on: the module namespace member is
        // retained under `(Module("ext"), "NS.Foo")` and a global sibling under
        // `(Global, "NS.GlobalOnly")`, distinct scopes sharing the `NS.` prefix.
        let prepared = prepare_augmentation_type_decl(
            "/src/aug.ts",
            &state,
            &AugmentationScopeKind::Module("ext".into()),
            "NS.Foo",
            None,
        )
        .expect("NS.Foo should prepare");

        assert!(
            !prepared.name_resolution.contains_key("GlobalOnly"),
            "a module-augmentation namespace decl must NOT bind a \
             global-augmentation sibling of the same namespace name"
        );
    }

    #[test]
    fn global_augmentation_namespace_type_decl_binds_type_sibling_not_value_sibling() {
        // A global-augmentation namespace TYPE decl binds its global TYPE
        // siblings (consumable through `prepare_local_type_decl`'s global
        // fallback) but NOT its global VALUE siblings: no prepared-value slot or
        // value fallback exists for a `(Global, "NS.member")` value key, so a
        // binding would dangle. The origin-blind helper chained the value keys
        // and bound the dangling `VERSION`; the Global arm is TYPE-only.
        let source = r#"
export {};
declare global { namespace JSX {
  type Common = { id?: string };
  export const VERSION: string;
  interface El { x: Common }
} }
"#;
        let analysis = make_analysis(source);
        let env = parse_and_build_env(source);
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));

        let prepared = prepare_local_type_decl(
            "/src/aug.ts",
            &state,
            "JSX.El",
            None,
            &ImportCanonicalization::default(),
        )
        .expect("JSX.El should prepare");

        // Positive control: the global TYPE sibling still binds (proves the
        // Global TYPE scan is retained, not gutted along with the value scan).
        assert_eq!(
            prepared
                .name_resolution
                .get("Common")
                .map(|i| (i.canonical_id.as_str(), i.symbol_name.as_str())),
            Some(("/src/aug.ts", "JSX.Common")),
        );
        // Restrict: the non-consumable global VALUE sibling must NOT be bound.
        assert!(
            !prepared.name_resolution.contains_key("VERSION"),
            "a non-consumable global-augmentation VALUE sibling must NOT be \
             bound into name_resolution"
        );
    }
}
