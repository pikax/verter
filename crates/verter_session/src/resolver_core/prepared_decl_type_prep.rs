//! Prepared type-declaration preparation internals.
//!
//! Extracted from `prepared_decl.rs` (same module, sibling file): the
//! lowered-body → `PreparedTypeDecl` assembly (`prepare_type_decl_from_lowered`,
//! the projection-only `prepare_authored_partial_type_decl`) plus the
//! name-resolution-base + import-resolution construction the assembly folds in
//! (`build_type_name_resolution_base` / `build_value_name_resolution_base`,
//! the file-symbol / import-space inserters, and namespace-sibling binding).
//!
//! These reach the shared shell assembler (`finish_prepared_type_decl`) and the
//! per-file build counter through the parent module (`use super::*`).

use super::*;

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
pub(super) fn prepare_type_decl_from_lowered(
    canonical_id: &verter_type_expr::OwnedStr,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    lowered: &LoweredTypeDecl,
    deps: Option<&ClassifiedTypeDeps>,
    dep_edges: Option<&FxHashMap<String, String>>,
    origin: Option<&verter_session_query::declarations::AugmentationScopeKind>,
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
                .get(&verter_type_expr::DeclBindingKey::new(
                    owner,
                    dep.local_name.as_str(),
                ))
                .ok_or_else(|| PreparationFailure::MissingExternalOwner {
                    local_name: dep.local_name.clone(),
                })?;
            Ok(PreparedExternalDep {
                local_name: dep.local_name.clone(),
                source_specifier: dep.source_specifier.clone(),
                imported_name: dep.imported_name.clone(),
                member_path: match &dep.route {
                    verter_type_expr::RouteDemand::MemberPath(path) => path
                        .iter()
                        .filter_map(|part| part.as_string().map(Arc::<str>::from))
                        .collect::<Vec<_>>()
                        .into(),
                    _ => Arc::from([]),
                },
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
            Some(base) => base.clone(),
            None => NameResolutionTable::new(build_type_name_resolution_base(
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
        NameResolutionTable::new(table)
    };
    let name_resolution = match origin {
        Some(verter_session_query::declarations::AugmentationScopeKind::Global) => {
            with_global_block_resolutions(name_resolution, canonical_id, state, owner, interner)
        }
        _ => name_resolution,
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

/// The `name_resolution` table of a declaration written inside one of
/// `owner`'s `declare global` blocks: a name the module's global blocks
/// declare is, inside them, the global declaration — ahead of the module's
/// own declaration or import of the name, which the module's other
/// references read. Such a name binds the global-augmentation scope
/// identity ([`verter_type_expr::TopLevelOwnerId::global_augmentation`])
/// that addresses the module's contribution to the global: a TYPE the
/// module binds, and a VALUE the module binds unless the module binds the
/// name as a type the blocks do not declare (the one entry serves both
/// spaces, and that type stays the module's). A name the module does not
/// bind already reaches the global by the bare-name fallback. A
/// namespace-sibling binding (a qualified identity) stays the innermost
/// scope's. The table is shared unchanged when no name needs rebinding.
pub(super) fn with_global_block_resolutions(
    name_resolution: SharedNameResolutionBase,
    canonical_id: &verter_type_expr::OwnedStr,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    interner: &IdentityInterner,
) -> SharedNameResolutionBase {
    use verter_session_query::declarations::AugmentationScopeKind;
    let lexical = owner.lexical();
    let global = AugmentationScopeKind::Global;
    let declares_type = |name: &str| state.declares_global_augmentation_type_in(lexical, name);
    let types = state
        .augmentation_type_decl_keys()
        .filter(|(scope, key)| {
            matches!(scope, AugmentationScopeKind::Global)
                && key.owner == lexical
                && state.binds_module_scope_type_in(lexical, key.name.as_ref())
        })
        .map(|(_, key)| key.name.as_ref());
    let values = state
        .headers
        .augmentation_value_headers
        .get(&global)
        .into_iter()
        .flat_map(|headers| headers.keys())
        .filter(|key| {
            let name = key.name.as_ref();
            key.owner == lexical
                && (state.has_value_symbol_in(lexical, name)
                    || state.is_import_local_in(lexical, name))
                && (declares_type(name) || !state.binds_module_scope_type_in(lexical, name))
        })
        .map(|key| key.name.as_ref());
    let rebound: Vec<&str> = types
        .chain(values)
        .filter(|name| {
            name_resolution.get(*name).is_none_or(|bound| {
                bound.canonical_id != *canonical_id || bound.symbol_name.as_ref() == *name
            })
        })
        .collect();
    if rebound.is_empty() {
        return name_resolution;
    }
    let mut table = (*name_resolution).clone();
    for name in rebound {
        let name = interner.intern(name);
        table.insert(
            name.clone(),
            ResolvedRootIdentity::new_in_owner(
                canonical_id.clone(),
                lexical.global_augmentation(),
                name,
            ),
        );
    }
    NameResolutionTable::new(table)
}

/// Insert the same-file symbol entries of the `name_resolution` table: every
/// TYPE and VALUE symbol name resolves to its own identity in the defining
/// file. The first pass of BOTH base tables (type-space and value-space) —
/// imports insert after (and therefore win per key) in both spaces.
fn insert_file_symbol_resolutions(
    table: &mut FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
    canonical_id: &verter_type_expr::OwnedStr,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    interner: &IdentityInterner,
) {
    for key in state.headers.type_headers.keys() {
        if key.owner != owner {
            continue;
        }
        let name = interner.intern(key.name.as_ref());
        table.insert(
            name.clone(),
            ResolvedRootIdentity::new_in_owner(canonical_id.clone(), owner, name),
        );
    }
    for key in state.headers.value_headers.keys() {
        if key.owner != owner {
            continue;
        }
        let name = interner.intern(key.name.as_ref());
        table.insert(
            name.clone(),
            ResolvedRootIdentity::new_in_owner(canonical_id.clone(), owner, name),
        );
    }
    // Eager framework-synthesised values (the Vue public-instance `default`)
    // are not parser headers, but they are exact owner-qualified local value
    // declarations. Admit their recorded keys into the same local namespace;
    // never remap them to the ordinary owner or recover them by name.
    for (key, _) in state.synthesised_value_bodies() {
        if key.owner != owner {
            continue;
        }
        let name = interner.intern(key.name.as_ref());
        table.insert(
            name.clone(),
            ResolvedRootIdentity::new_in_owner(canonical_id.clone(), owner, name),
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
    table: &mut FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) {
    for local in state.owner_import_targets.keys() {
        if local.owner != owner || state.has_type_symbol_in(owner, local.name.as_ref()) {
            continue;
        }
        let Some(resolved) =
            import_canonicalization
                .final_resolution
                .get(&verter_type_expr::DeclBindingKey::new(
                    owner,
                    local.name.as_ref(),
                ))
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
pub(super) fn prepare_authored_partial_type_decl(
    canonical_id: &verter_type_expr::OwnedStr,
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
    let external_deps = deps
        .external_deps
        .iter()
        .filter_map(|dep| {
            let identity = import_canonicalization.final_resolution.get(
                &verter_type_expr::DeclBindingKey::new(owner, dep.local_name.as_str()),
            )?;
            Some(PreparedExternalDep {
                local_name: dep.local_name.clone(),
                source_specifier: dep.source_specifier.clone(),
                imported_name: dep.imported_name.clone(),
                member_path: match &dep.route {
                    verter_type_expr::RouteDemand::MemberPath(path) => path
                        .iter()
                        .filter_map(|part| part.as_string().map(Arc::<str>::from))
                        .collect::<Vec<_>>()
                        .into(),
                    _ => Arc::from([]),
                },
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
        NameResolutionTable::new(table),
        interner,
    )
    .map(Some)
}

/// Insert the VALUE-space import entries — same FINAL-definition
/// canonicalization as the type space, WITHOUT the type-symbol shadow skip:
/// in a prepared value decl's annotation scope the import binding wins over
/// a same-named local symbol.
///
/// A single import binding that fails to canonicalize (an unresolvable
/// specifier — no `node_modules` package, an unconfigured project, a genuine
/// broken import) is SKIPPED, exactly like the type-space sibling
/// ([`insert_resolvable_type_space_imports`]) already skips its own
/// unresolvable entries: this unrelated non-canonicalizable binding must not
/// make every local value declaration in the owner unavailable — a single
/// bad import must not poison every sibling declaration's admission
/// (methods, plain values, everything) by aborting the whole per-owner base
/// build. A reference to the skipped name still fails to resolve on its
/// own — this only stops one bad import from taking down the rest of the
/// file.
fn insert_value_space_import_resolutions(
    table: &mut FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
    _canonical_id: &verter_type_expr::OwnedStr,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    _dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) {
    for (local, target) in state.owner_import_targets.iter() {
        if local.owner != owner {
            continue;
        }
        // A bare namespace import is not a declaration identity: only an
        // exact qualified member (`Ns.Member`) can be routed to a defining
        // owner. Qualified resolution is owned by the namespace-member facts
        // path, so the shared value-name base must neither invent a `*`
        // declaration nor let this unrelated non-canonicalizable binding make
        // every local value declaration in the owner unavailable.
        if target.is_namespace {
            continue;
        }
        let local_name = local.name.as_ref();
        let Ok(resolved) = canonicalize_import_target(import_canonicalization, local_name, owner)
        else {
            continue;
        };
        table.insert(interner.intern(local_name), resolved);
    }
}

/// Build the per-FILE TYPE-space `name_resolution` base table: file symbols,
/// then imports (type-symbol-shadowed). This is the COMPLETE table of every
/// non-namespaced prepared type decl of the file — the prepared-decl cache
/// builds it once per bundle and shares it via `Arc` across those decls; a
/// namespaced decl re-runs the same passes around its sibling bindings.
pub(super) fn build_type_name_resolution_base(
    canonical_id: &verter_type_expr::OwnedStr,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    _dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>, PreparationFailure> {
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
pub(super) fn build_value_name_resolution_base(
    canonical_id: &verter_type_expr::OwnedStr,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    dep_edges: Option<&FxHashMap<String, String>>,
    import_canonicalization: &ImportCanonicalization,
    interner: &IdentityInterner,
) -> Result<FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>, PreparationFailure> {
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
    );
    Ok(table)
}

/// Map each member of a namespaced declaration's enclosing namespaces to its
/// qualified identity in `name_resolution`, so a body reference written by
/// the member's name relative to an enclosing namespace resolves as the
/// checker's namespace scope does: inside `namespace NS { type V = M }`,
/// `Ref("M")` is `NS.M`; inside `namespace A.B { type V = C.T }`, `C.T` is
/// `A.B.C.T`, whichever block of `A.B` declares it. Every enclosing
/// namespace is a scope, innermost first: `A.B`'s members, then `A`'s. A
/// member's first segment an inner namespace binds shadows the outer
/// namespaces' members under that segment. A no-op for a non-namespaced
/// declaration (`symbol_name` has no `.`).
///
/// Members are drawn from the inventory visible for the declaration's
/// `origin` scope, and ONLY where that member has a buildable prepared decl —
/// binding a member the dispatch cannot resolve would dangle, and binding one
/// from a different scope would leak across scopes:
///
/// - File-scope decl (`origin = None`): file-scope `namespace NS { ... }` TYPE
///   and VALUE members, indexed under their qualified `NS.M` name in the
///   file-scope inventory. Both are consumable — a type member through the
///   prepared-type cache, a value member through `prepare_local_value_decl`.
/// - Global-augmentation decl (`origin = Global`): global `declare global {
///   namespace NS { ... } }` TYPE members, retained under `(Global, "NS.M")`
///   and never in file-scope `type_symbols`. TYPE-only: a `(Global, "NS.M")`
///   type key is consumable through [`prepare_local_type_decl`]'s global
///   fallback, but a global VALUE member has no prepared-value slot or
///   fallback, so binding it would dangle. Without this scan an unqualified
///   reference inside a global-augmented namespace (e.g. `interface
///   IntrinsicElements { div: Common }` referencing the sibling `Common`)
///   would not resolve — the valid global `JSX` namespace form.
/// - Module-augmentation decl (`origin = Module(spec)`): a `(Module(spec),
///   "NS.M")` member has no prepared-decl slot today (the prepared-decl caches
///   index file-scope + Global-augmentation symbols only), so no module member
///   is consumable. Bind nothing — binding a Global member here would leak
///   across scopes. When module-scope prepared decls become addressable, the
///   matching `(Module(spec), …)` members bind here.
fn add_namespace_sibling_resolutions(
    name_resolution: &mut FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
    state: &ShallowFileState,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
    canonical_id: &verter_type_expr::OwnedStr,
    origin: Option<&verter_session_query::declarations::AugmentationScopeKind>,
    interner: &IdentityInterner,
) {
    use verter_session_query::declarations::AugmentationScopeKind;
    let Some((namespace_prefix, _)) = symbol_name.rsplit_once('.') else {
        return;
    };
    let names: Vec<&str> = match origin {
        // File-scope decl: file-scope TYPE + VALUE members (both consumable
        // through the prepared-type / prepared-value caches).
        None => state
            .headers
            .type_headers
            .keys()
            .chain(state.headers.value_headers.keys())
            .filter(|key| key.owner == owner)
            .map(|key| key.name.as_ref())
            .collect(),
        // Global-augmentation decl: global TYPE members ONLY — a global
        // namespace's VALUE members are not modelled as namespace-scope
        // names (RESTRICT). The key set spans every augmentation scope, so
        // filter to `Global`.
        Some(AugmentationScopeKind::Global) => state
            .augmentation_type_keys()
            .filter(|(scope, _)| matches!(scope, AugmentationScopeKind::Global))
            .map(|(_, name)| name)
            .collect(),
        // Module-augmentation decl: no consumable module-scope member exists
        // today, so bind nothing rather than dangle a module member or leak a
        // global one across scopes.
        Some(AugmentationScopeKind::Module(_)) => return,
    };
    let mut shadowed: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
    let mut namespace = Some(namespace_prefix);
    while let Some(current) = namespace {
        let dotted_prefix = format!("{current}.");
        let mut bound: Vec<&str> = Vec::new();
        for name in &names {
            let Some(member) = name.strip_prefix(&dotted_prefix) else {
                continue;
            };
            let head = member.split('.').next().unwrap_or(member);
            if shadowed.contains(head) {
                continue;
            }
            bound.push(head);
            name_resolution.insert(
                interner.intern(member),
                ResolvedRootIdentity::new_in_owner(
                    canonical_id.clone(),
                    owner,
                    interner.intern(name),
                ),
            );
        }
        // An `import X = N.M` this namespace's body declares binds `X` to the
        // entity `N.M` names, resolved from this namespace outward (`N`
        // relative to each enclosing namespace, innermost first, then the
        // file): `X.T` is bound to that entity's member `T`.
        for (key, target) in &state.headers.namespace_aliases {
            if key.owner != owner {
                continue;
            }
            let Some(alias) = key.name.as_ref().strip_prefix(&dotted_prefix) else {
                continue;
            };
            if alias.contains('.') || shadowed.contains(alias) {
                continue;
            }
            let Some(entity) = alias_entity(&names, current, target) else {
                continue;
            };
            let entity_prefix = format!("{entity}.");
            for name in &names {
                let bound_name = if *name == entity {
                    alias.to_string()
                } else if let Some(member) = name.strip_prefix(&entity_prefix) {
                    format!("{alias}.{member}")
                } else {
                    continue;
                };
                name_resolution.insert(
                    interner.intern(&bound_name),
                    ResolvedRootIdentity::new_in_owner(
                        canonical_id.clone(),
                        owner,
                        interner.intern(name),
                    ),
                );
            }
            bound.push(alias);
        }
        shadowed.extend(bound);
        namespace = current.rsplit_once('.').map(|(outer, _)| outer);
    }
}

/// The qualified entity the target `N.M` of an `import X = N.M` declared in
/// the namespace `namespace` names: `N.M` read as a member of `namespace`,
/// then of each namespace enclosing it, then at file scope — the first
/// under which `names` declares the entity itself or a member of it. `None`
/// when none does.
fn alias_entity(names: &[&str], namespace: &str, target: &str) -> Option<String> {
    let declares = |entity: &str| {
        let prefix = format!("{entity}.");
        names
            .iter()
            .any(|name| *name == entity || name.starts_with(&prefix))
    };
    let mut scope = Some(namespace);
    while let Some(current) = scope {
        let entity = format!("{current}.{target}");
        if declares(&entity) {
            return Some(entity);
        }
        scope = current.rsplit_once('.').map(|(outer, _)| outer);
    }
    declares(target).then(|| target.to_owned())
}
