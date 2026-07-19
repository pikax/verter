use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_semantic::analysis::types::{AnalyzedMacroKind, MacroTypeDep};

use crate::resolver_core::resolve_local_type_declaration;

use super::direct_macro::{
    imported_declaration_surface_is_authoritative, is_direct_local_macro_type_reference,
    is_direct_macro_type_reference, keep_direct_imported_vue_macro, macro_dep_exported_type_name,
    macro_has_authoritative_owner_surface, macro_has_direct_local_type_root,
    should_ignore_external_macro_type, should_seed_direct_macro_registry_entry,
};
use super::{
    macro_kind_needed_for_fallthrough, placeholder_type_declaration,
    skip_macro_declaration_metadata_for_purpose, ComponentMetaEvalOutputs,
    ComponentMetaResolutionPurpose, ComponentMetaResolverHost, ResolvedComponentMetaParts,
    ResolvedMacroMeta, ResolvedTypeRegistryMeta,
};

pub fn resolve_component_meta_parts<H>(
    host: &H,
    owner_canonical: &str,
    snapshot: &H::Snapshot,
    expanded: bool,
    eval_context: Option<&H::EvalContext>,
    purpose: ComponentMetaResolutionPurpose,
) -> ResolvedComponentMetaParts
where
    H: ComponentMetaResolverHost,
{
    let mut resolved_macros = Vec::new();
    let mut resolved_type_registry = Vec::new();
    let mut resolved_type_registry_meta = Vec::new();
    let mut seen_registry_names = FxHashSet::default();
    let mut native_props_cache = super::NativePropProjectionCache::default();
    let mut tracked_deps = BTreeSet::new();

    let eval_outputs = if expanded {
        host.build_eval_outputs(owner_canonical, snapshot, eval_context, purpose)
    } else {
        ComponentMetaEvalOutputs::default()
    };
    tracked_deps.extend(eval_outputs.tracked_dependencies.iter().cloned());

    let imports = host.snapshot_imports(snapshot);
    let macros = host.snapshot_macros(snapshot);
    let projectable_owner_local_roots = if expanded {
        {
            macros
                .iter()
                .map(|mac| host.projectable_owner_local_macro_roots(owner_canonical, mac))
                .collect::<Vec<_>>()
        }
    } else {
        Default::default()
    };
    let projectable_owner_local_surfaces = if expanded {
        {
            projectable_owner_local_roots
                .iter()
                .map(|roots| !roots.is_empty())
                .collect::<Vec<_>>()
        }
    } else {
        Default::default()
    };
    let macro_type_deps: Vec<MacroTypeDep> = host.snapshot_macro_type_deps(snapshot).to_vec();
    for dep in &macro_type_deps {
        if purpose == ComponentMetaResolutionPurpose::Fallthrough
            && !macro_kind_needed_for_fallthrough(dep.macro_kind)
        {
            continue;
        }
        let direct_macro_reference =
            is_direct_macro_type_reference(host, owner_canonical, macros, dep);
        if expanded {
            if let Some(mac) = macros.get(dep.macro_index) {
                let authoritative_owner = macro_has_authoritative_owner_surface(
                    mac,
                    eval_outputs.evaluated_types.as_ref(),
                    dep.macro_index,
                );
                let projectable_owner_local = projectable_owner_local_surfaces
                    .get(dep.macro_index)
                    .copied()
                    .unwrap_or(false);
                // graph-native authoritative-owner-
                // local detection. The legacy
                // `macro_has_authoritative_resolved_local_surface`
                // re-parsed `resolved.expanded` text via the
                // expanded-text projector to decide whether the
                // owner-local resolved-type surface could replace
                // the imported dep. The graph equivalent is simply:
                // does the host's
                // `projectable_owner_local_macro_roots` (already
                // computed into `projectable_owner_local_surfaces`)
                // produce a non-empty set for this macro? If yes,
                // the prepared owner-local surface IS authoritative
                // and the imported dep is suppressible per the same
                // rules below.
                let authoritative_resolved_local = projectable_owner_local;
                // When the dep's type name is not present in the macro's
                // own type_references (e.g. `defineProps<ChildProps>()`
                // where `interface ChildProps extends Omit<ButtonProps, …>`
                // pulls `ButtonProps` in through heritage rather than a
                // direct macro reference), the owner's local projection
                // cannot stand in for the imported surface — suppressing it
                // would drop the heritage dep from the meta output. Only
                // suppress when the dep is macro-visible directly.
                let dep_referenced_in_macro_types = mac
                    .type_references
                    .iter()
                    .any(|type_name| type_name == &dep.type_name);
                let projectable_owner_local_suppresses_dep = projectable_owner_local
                    && dep_referenced_in_macro_types
                    && !(purpose == ComponentMetaResolutionPurpose::Full
                        && dep.macro_kind == AnalyzedMacroKind::DefineEmits);
                let local_type_root = macro_has_direct_local_type_root(mac);
                // For DefineEmits with a non-direct imported type dep
                // (e.g. `type AppEmits = { change: [...] } & ImportedEmits`),
                // the local emit surface is partial — the intersection's
                // imported branch was not resolvable locally. Don't let the
                // partial local surface block the imported dep from merging.
                let define_emits_partial_intersection =
                    dep.macro_kind == AnalyzedMacroKind::DefineEmits && !direct_macro_reference;
                let authoritative_owner_effective = authoritative_owner
                    && dep_referenced_in_macro_types
                    && !define_emits_partial_intersection;
                let skip_non_direct_dep = !direct_macro_reference
                    && (authoritative_resolved_local
                        || projectable_owner_local_suppresses_dep
                        || authoritative_owner_effective);
                let skip_fallthrough_define_emits = purpose
                    == ComponentMetaResolutionPurpose::Fallthrough
                    && dep.macro_kind == AnalyzedMacroKind::DefineEmits
                    && mac.is_type_based
                    && (!direct_macro_reference || authoritative_owner || local_type_root);
                let skip_authoritative_resolved_local = authoritative_resolved_local
                    && !(purpose == ComponentMetaResolutionPurpose::Full
                        && direct_macro_reference
                        && dep.macro_kind == AnalyzedMacroKind::DefineEmits);
                if skip_non_direct_dep
                    || skip_authoritative_resolved_local
                    || skip_fallthrough_define_emits
                {
                    if skip_fallthrough_define_emits {
                        if let Some(dep_canonical) = host
                            .resolve_type_dependency_canonical(owner_canonical, &dep.import_source)
                        {
                            tracked_deps.insert(dep_canonical);
                        }
                    }
                    continue;
                }
            }
        }
        let macro_index = dep.macro_index;
        let dep_exported_name = macro_dep_exported_type_name(imports, dep);
        let dep_canonical = host
            .resolve_type_dependency_canonical(owner_canonical, &dep.import_source)
            .unwrap_or_default();
        let skip_declaration_metadata = skip_macro_declaration_metadata_for_purpose(purpose);
        let mut resolution_deps = BTreeSet::new();
        let mut imported_surface = if expanded && !should_ignore_external_macro_type(dep) {
            host.resolve_imported_macro_surface(
                owner_canonical,
                &dep.import_source,
                dep_exported_name.as_ref(),
                &mut tracked_deps,
                &mut resolution_deps,
                &mut native_props_cache,
            )
        } else {
            None
        };
        let declaration = if skip_declaration_metadata {
            placeholder_type_declaration(dep_exported_name.as_ref(), dep_exported_name.as_ref())
        } else if let Some(surface) = imported_surface.as_ref() {
            surface.declaration.clone()
        } else {
            host.resolve_type_declaration(&dep_canonical, dep_exported_name.as_ref())
        };
        let jsdoc = if skip_declaration_metadata {
            None
        } else {
            host.resolve_jsdoc_block(
                declaration.canonical_source.as_str(),
                declaration.span,
                expanded,
                &mut tracked_deps,
            )
        };

        if !dep_canonical.is_empty() {
            tracked_deps.insert(dep_canonical.clone());
        }
        if !skip_declaration_metadata
            && !declaration.canonical_source.is_empty()
            && declaration.canonical_source != dep_canonical
        {
            tracked_deps.insert(declaration.canonical_source.clone());
        }

        if !expanded {
            resolved_macros.push(ResolvedMacroMeta {
                macro_index,
                macro_kind: dep.macro_kind,
                type_name: dep.type_name.clone(),
                import_source: dep.import_source.clone(),
                surface_is_authoritative: false,
                declaration,
                native_props: Vec::new(),
                jsdoc,
            });
            continue;
        }

        if should_ignore_external_macro_type(dep) {
            resolved_macros.push(ResolvedMacroMeta {
                macro_index,
                macro_kind: dep.macro_kind,
                type_name: dep.type_name.clone(),
                import_source: dep.import_source.clone(),
                surface_is_authoritative: false,
                declaration,
                native_props: Vec::new(),
                jsdoc,
            });
            continue;
        }

        let imported_native_props = imported_surface.take().map(|surface| surface.native_props);
        if let Some(native_props) = imported_native_props {
            // The native-only surface (`native_props`) rides the SAME
            // dispatch resolution that produced the elements payload:
            // keep-all rows built directly from the one-level
            // `TypeInfoSurface` members (visibility carried verbatim) —
            // no parser projection round-trip, no separate re-resolve.
            // The published props/emits/slots/
            // exposed surface is NOT projected here — it is owned by the
            // typeinfo macro-surface path (`vue_macro_dtos`), which
            // `component_meta_resolved_macros` consults at the session
            // boundary. The registry root is seeded SHALLOW
            // (`TypeExpr::named`): consumers re-resolve the named root on
            // demand through the shared resolver (shallow-by-default), and
            // the typeinfo/evaluated path is the single shape authority.
            let package_backed_dep = host.workspace_is_package_backed(dep_canonical.as_str())
                || host.workspace_is_package_backed(declaration.canonical_source.as_str());
            if is_direct_macro_type_reference(host, owner_canonical, macros, dep)
                && !package_backed_dep
                && should_seed_direct_macro_registry_entry(&declaration)
                && seen_registry_names.insert(dep.type_name.clone())
            {
                resolved_type_registry.push(ResolvedTypeAnalysis {
                    name: dep.type_name.clone(),
                    // Shallow-by-default registry seed: the content-free bare
                    // named-reference SOURCE — consumers re-resolve the named
                    // root on demand through the one shared dispatch.
                    type_source: verter_type_expr::facts::SourcePosition::Present(
                        verter_type_expr::facts::SemanticTypeSource::Closed(
                            verter_type_expr::facts::ClosedTypeFact::Leaf(
                                verter_type_expr::facts::LeafTypeFact::Ref(dep.type_name.clone()),
                            ),
                        ),
                    ),
                    type_expansion: None,
                });
                resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                    name: dep.type_name.clone(),
                    declaration: declaration.clone(),
                });
            }
            let projectable_owner_local = projectable_owner_local_surfaces
                .get(dep.macro_index)
                .copied()
                .unwrap_or(false);
            let imported_surface_is_authoritative =
                imported_declaration_surface_is_authoritative(&declaration);
            let keep_direct_imported_vue_macro = keep_direct_imported_vue_macro(
                host,
                owner_canonical,
                projectable_owner_local,
                purpose,
                macros,
                dep,
                &declaration,
            );
            if !projectable_owner_local || keep_direct_imported_vue_macro {
                resolved_macros.push(ResolvedMacroMeta {
                    macro_index,
                    macro_kind: dep.macro_kind,
                    type_name: dep.type_name.clone(),
                    import_source: dep.import_source.clone(),
                    surface_is_authoritative: imported_surface_is_authoritative,
                    declaration,
                    native_props,
                    jsdoc,
                });
            }
        } else {
            // graph-native fallback. When `imported_native_props`
            // is `None` the macro has no resolvable surface; emit empty
            // surfaces and proceed. The previous source-text reparse
            // path (read source then call the source-typed projector)
            // violated the cache-owned recovery rule and is deleted.
            let projectable_owner_local = projectable_owner_local_surfaces
                .get(dep.macro_index)
                .copied()
                .unwrap_or(false);
            let keep_direct_imported_vue_macro = keep_direct_imported_vue_macro(
                host,
                owner_canonical,
                projectable_owner_local,
                purpose,
                macros,
                dep,
                &declaration,
            );
            let package_backed_dep = host.workspace_is_package_backed(dep_canonical.as_str())
                || host.workspace_is_package_backed(declaration.canonical_source.as_str());
            if is_direct_macro_type_reference(host, owner_canonical, macros, dep)
                && !package_backed_dep
                && should_seed_direct_macro_registry_entry(&declaration)
                && seen_registry_names.insert(dep.type_name.clone())
            {
                resolved_type_registry.push(ResolvedTypeAnalysis {
                    name: dep.type_name.clone(),
                    // Shallow-by-default registry seed: the content-free bare
                    // named-reference SOURCE — consumers re-resolve the named
                    // root on demand through the one shared dispatch.
                    type_source: verter_type_expr::facts::SourcePosition::Present(
                        verter_type_expr::facts::SemanticTypeSource::Closed(
                            verter_type_expr::facts::ClosedTypeFact::Leaf(
                                verter_type_expr::facts::LeafTypeFact::Ref(dep.type_name.clone()),
                            ),
                        ),
                    ),
                    type_expansion: None,
                });
                resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                    name: dep.type_name.clone(),
                    declaration: declaration.clone(),
                });
            }
            if !projectable_owner_local || keep_direct_imported_vue_macro {
                resolved_macros.push(ResolvedMacroMeta {
                    macro_index,
                    macro_kind: dep.macro_kind,
                    type_name: dep.type_name.clone(),
                    import_source: dep.import_source.clone(),
                    surface_is_authoritative: false,
                    declaration,
                    native_props: Vec::new(),
                    jsdoc,
                });
            }
        }
    }

    if expanded {
        for (macro_index, mac) in host.snapshot_macros(snapshot).iter().enumerate() {
            if purpose == ComponentMetaResolutionPurpose::Fallthrough
                && !macro_kind_needed_for_fallthrough(mac.kind)
            {
                continue;
            }
            let macro_has_imported_type_deps = macro_type_deps
                .iter()
                .any(|dep| dep.macro_index == macro_index);
            let projectable_owner_local = projectable_owner_local_surfaces
                .get(macro_index)
                .copied()
                .unwrap_or(false);
            // When the owner has cross-file heritage (extends Omit<External>),
            // the source-text path can only resolve same-file members.
            // The prepared surface supplements it with the full shape
            // including inherited members from the cross-file chain.
            let prepared_surface_will_handle = projectable_owner_local
                && macro_has_imported_type_deps
                && matches!(
                    mac.kind,
                    AnalyzedMacroKind::DefineProps
                        | AnalyzedMacroKind::WithDefaults
                        | AnalyzedMacroKind::DefineModel
                        | AnalyzedMacroKind::DefineSlots
                        | AnalyzedMacroKind::DefineEmits
                        | AnalyzedMacroKind::DefineExpose
                );

            // Seed the direct macro-local root into the registry. The
            // owner-local authority entry (pushed below, gated on
            // `host.owner_local_macro_root_has_surface`) marks the macro as
            // owner-local-authoritative for the materialiser; the published
            // props/emits/slots/exposed surface itself is sourced from the
            // typeinfo path keyed on macro_index.
            for (resolved_index, resolved) in mac.resolved_local_types.iter().enumerate() {
                if !is_direct_local_macro_type_reference(
                    mac,
                    resolved_index,
                    resolved.name.as_str(),
                ) {
                    continue;
                }

                // Seed only the direct macro-local root into the registry up
                // front. Additional owner-local helpers are discovered later
                // from the queried root surface during registry append, which
                // keeps publication demand-driven instead of prepublishing the
                // entire same-file helper chain.
                let direct_named_reference = mac
                    .type_references
                    .iter()
                    .any(|type_name| type_name == &resolved.name);
                if resolved_index == 0
                    && direct_named_reference
                    && seen_registry_names.insert(resolved.name.clone())
                {
                    resolved_type_registry.push(ResolvedTypeAnalysis {
                        name: resolved.name.clone(),
                        // The analyzer's synthesized closed shape IS the
                        // entry's source (a primitive folds to a leaf fact;
                        // richer bodies stay shallow named-reference
                        // locators resolved on demand).
                        type_source: verter_type_expr::facts::SourcePosition::Present(
                            verter_type_expr::facts::SemanticTypeSource::Synthesized(
                                resolved.shape.clone(),
                            ),
                        ),
                        type_expansion: None,
                    });
                    resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                        name: resolved.name.clone(),
                        declaration: if skip_macro_declaration_metadata_for_purpose(purpose) {
                            placeholder_type_declaration(
                                resolved.name.as_str(),
                                resolved.name.as_str(),
                            )
                        } else {
                            resolve_local_type_declaration(
                                host,
                                owner_canonical,
                                resolved.name.as_str(),
                                resolved.span,
                            )
                        },
                    });
                }
            }

            // Owner-local authoritative entry. Pushes a `ResolvedMacroMeta`
            // (gated on `owner_local_macro_root_has_surface`) for any macro
            // whose owner-local prepared surface (`projectable_owner_local`) is
            // non-empty — this covers:
            //   - imported-heritage macros (`prepared_surface_will_handle`):
            //     `interface Props extends Omit<ExternalType, K>` resolves its
            //     cross-file heritage through the typeinfo surface.
            //   - purely-local macros (no cross-file deps).
            //   - DefineEmits with no imported deps under fallthrough scope:
            //     the emits inheritance carve-out.
            if prepared_surface_will_handle
                || (projectable_owner_local && !macro_has_imported_type_deps)
                || (mac.kind == AnalyzedMacroKind::DefineEmits
                    && (purpose == ComponentMetaResolutionPurpose::Fallthrough
                        || !macro_has_imported_type_deps))
            {
                for root_name in projectable_owner_local_roots
                    .get(macro_index)
                    .into_iter()
                    .flatten()
                {
                    // Gate on the owner-local root having a non-empty prepared
                    // surface (folds the prior emptiness check that inspected
                    // the projected props/emits/slots/exposed). The published
                    // props/emits/slots/exposed surface itself is owned by the
                    // typeinfo path; here we only decide whether to push an
                    // authoritative entry for this root.
                    if !host.owner_local_macro_root_has_surface(
                        owner_canonical,
                        root_name,
                        mac.kind,
                    ) {
                        continue;
                    }

                    // An entry for this (macro_index, root) already exists (the
                    // imported arm pushed it). The published surface comes from
                    // the typeinfo path keyed on macro_index, so there is
                    // nothing to replace — keep the existing entry (which
                    // carries the imported declaration's `native_props`) and
                    // skip. Otherwise push the authoritative owner-local entry
                    // (`native_props` is empty — owner-local roots have no
                    // class-member visibility surface).
                    if resolved_macros
                        .iter()
                        .any(|meta| meta.macro_index == macro_index && meta.type_name == *root_name)
                    {
                        continue;
                    }

                    let declaration = if skip_macro_declaration_metadata_for_purpose(purpose) {
                        placeholder_type_declaration(root_name.as_str(), root_name.as_str())
                    } else {
                        host.resolve_type_declaration(owner_canonical, root_name.as_str())
                    };
                    let jsdoc = if skip_macro_declaration_metadata_for_purpose(purpose) {
                        None
                    } else {
                        host.resolve_jsdoc_block(
                            owner_canonical,
                            declaration.span,
                            true,
                            &mut tracked_deps,
                        )
                    };
                    resolved_macros.push(ResolvedMacroMeta {
                        macro_index,
                        macro_kind: mac.kind,
                        type_name: root_name.clone(),
                        import_source: String::new(),
                        surface_is_authoritative: true,
                        declaration,
                        native_props: Vec::new(),
                        jsdoc,
                    });
                }
            }

            for root_name in projectable_owner_local_roots
                .get(macro_index)
                .into_iter()
                .flatten()
            {
                if seen_registry_names.insert(root_name.clone()) {
                    let declaration = if skip_macro_declaration_metadata_for_purpose(purpose) {
                        placeholder_type_declaration(root_name.as_str(), root_name.as_str())
                    } else {
                        host.resolve_type_declaration(owner_canonical, root_name.as_str())
                    };
                    resolved_type_registry.push(ResolvedTypeAnalysis {
                        name: root_name.clone(),
                        // Owner-local authoritative placeholder: an EMPTY
                        // synthesized object shape (the projectable
                        // owner-local surface is the shape authority).
                        type_source: verter_type_expr::facts::SourcePosition::Present(
                            verter_type_expr::facts::SemanticTypeSource::Synthesized(
                                verter_type_expr::facts::ResolvedLocalShape::Object(
                                    std::sync::Arc::from([]),
                                ),
                            ),
                        ),
                        type_expansion: None,
                    });
                    resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                        name: root_name.clone(),
                        declaration,
                    });
                }
            }
        }
    }

    host.sync_transitive_macro_type_dependencies(owner_canonical, &tracked_deps);
    let fact_versions = host.current_dependency_fact_versions(owner_canonical, &tracked_deps);
    ResolvedComponentMetaParts {
        resolved_macros,
        resolved_type_registry,
        resolved_type_registry_meta,
        evaluated_types: eval_outputs.evaluated_types,
        tracked_dependencies: tracked_deps,
        fact_versions,
        surface_identities: eval_outputs.surface_identities,
    }
}
