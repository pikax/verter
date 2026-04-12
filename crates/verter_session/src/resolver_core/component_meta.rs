use std::borrow::Cow;
use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_semantic::analysis::type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};
use verter_semantic::analysis::types::{
    AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, MacroTypeDep,
};

use crate::resolver_core::{
    component_meta_registry::component_meta_registry_has_non_object_top_level_surface,
    project_macro_surfaces, resolve_local_type_declaration, resolve_type_declaration,
    surface_projector::{
        project_macro_surfaces_from_expanded_text, project_macro_surfaces_from_source_type_name,
    },
    DeclarationMetadataResolver, FactVersionRef, ResolvedNativeProp, ResolvedTypeDeclaration,
};

/// Collect the set of binding names exposed by macros (e.g., `defineExpose` fields).
/// Used as a filter for which `env.value_symbols` entries to expand as bindings
/// during `expand_macro_types`.
pub fn collect_requested_binding_names(macros: &[AnalyzedMacro]) -> FxHashSet<String> {
    macros
        .iter()
        .flat_map(|mac| mac.expose_fields.iter().map(|field| field.name.clone()))
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct ComponentMetaEvalOutputs {
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub tracked_dependencies: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTypeRegistryMeta {
    pub name: String,
    pub declaration: ResolvedTypeDeclaration,
}

#[derive(Debug, Clone)]
pub struct ResolvedMacroMeta {
    pub macro_index: usize,
    pub macro_kind: AnalyzedMacroKind,
    pub type_name: String,
    pub import_source: String,
    pub surface_is_authoritative: bool,
    pub declaration: ResolvedTypeDeclaration,
    pub native_props: Vec<ResolvedNativeProp>,
    pub props: Vec<verter_semantic::analysis::AnalyzedPropField>,
    pub emits: Vec<verter_semantic::analysis::AnalyzedEmitField>,
    pub slots: Vec<verter_semantic::analysis::AnalyzedSlotField>,
    pub jsdoc: Option<ResolvedJsdocBlock>,
}

#[derive(Debug, Clone)]
pub struct ResolvedImportedMacroSurface {
    pub declaration: ResolvedTypeDeclaration,
    pub elements: ResolvedElements,
}

#[derive(Debug, Clone)]
pub struct ResolvedJsdocBlock {
    pub description: Option<String>,
    pub tags: Vec<ResolvedJsdocTag>,
}

#[derive(Debug, Clone)]
pub struct ResolvedJsdocTag {
    pub name: String,
    pub text: Option<String>,
    pub raw_type: Option<String>,
    pub subject_name: Option<String>,
    pub resolved_type: Option<verter_semantic::analysis::type_expr::TypeExpr>,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaParts {
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    pub resolved_type_registry: Vec<ResolvedTypeAnalysis>,
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub tracked_dependencies: BTreeSet<String>,
    pub fact_versions: Vec<FactVersionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentMetaResolutionPurpose {
    Full,
    Fallthrough,
}

pub fn component_meta_resolved_macros(
    snapshot_macros: &[AnalyzedMacro],
    resolved_macros: &[ResolvedMacroMeta],
) -> Vec<verter_semantic::analysis::component_meta::ResolvedMacroInput> {
    resolved_macros
        .iter()
        .filter(|resolved| {
            snapshot_macros
                .get(resolved.macro_index)
                .is_none_or(|mac| !raw_macro_surface_is_authoritative(mac))
        })
        .map(
            |resolved| verter_semantic::analysis::component_meta::ResolvedMacroInput {
                macro_index: resolved.macro_index,
                props: resolved.props.clone(),
                emits: resolved.emits.clone(),
                slots: resolved.slots.clone(),
            },
        )
        .collect()
}

pub fn component_meta_type_registry(
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
) -> Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis> {
    let mut seen = FxHashSet::default();
    let mut registry = Vec::new();

    for entry in resolved_type_registry {
        if seen.insert(entry.name.clone()) {
            registry.push(entry.clone());
        }
    }

    registry
}

pub trait ComponentMetaResolverHost: DeclarationMetadataResolver {
    type Snapshot;
    type EvalContext;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration
    where
        Self: Sized,
    {
        resolve_type_declaration(self, dep_canonical, requested_name)
    }

    fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport];
    fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro];
    fn snapshot_macro_type_deps<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [MacroTypeDep];

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
        purpose: ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs;

    #[allow(clippy::too_many_arguments)]
    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedElements>;

    #[allow(clippy::too_many_arguments)]
    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedImportedMacroSurface>
    where
        Self: Sized,
    {
        let dep_canonical =
            self.resolve_type_dependency_canonical(owner_canonical, import_source)?;
        let declaration = self.resolve_type_declaration(dep_canonical.as_str(), exported_name);
        let elements = self.resolve_macro_elements(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
        )?;
        Some(ResolvedImportedMacroSurface {
            declaration,
            elements,
        })
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock>;

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &BTreeSet<String>,
    );

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef>;
}

fn raw_macro_surface_is_authoritative(mac: &AnalyzedMacro) -> bool {
    match mac.kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => false,
        AnalyzedMacroKind::DefineEmits => false,
        AnalyzedMacroKind::DefineSlots => false,
        AnalyzedMacroKind::DefineExpose => !mac.expose_fields.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

fn skip_macro_declaration_metadata_for_purpose(purpose: ComponentMetaResolutionPurpose) -> bool {
    purpose == ComponentMetaResolutionPurpose::Fallthrough
}

fn placeholder_type_declaration(
    requested_name: &str,
    resolved_name: &str,
) -> ResolvedTypeDeclaration {
    ResolvedTypeDeclaration {
        requested_name: requested_name.to_string(),
        declaration_id: None,
        resolved_name: resolved_name.to_string(),
        canonical_source: String::new(),
        span: verter_span::Span::default(),
        kind: crate::resolver_core::ResolvedDeclarationKind::Unknown,
        text: None,
    }
}

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
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = FxHashSet::default();
    let mut tracked_deps = BTreeSet::new();

    let eval_outputs = if expanded {
        host.build_eval_outputs(owner_canonical, snapshot, eval_context, purpose)
    } else {
        ComponentMetaEvalOutputs::default()
    };
    tracked_deps.extend(eval_outputs.tracked_dependencies.iter().cloned());

    let imports = host.snapshot_imports(snapshot);
    let macros = host.snapshot_macros(snapshot);
    let macro_type_deps: Vec<MacroTypeDep> = host.snapshot_macro_type_deps(snapshot).to_vec();
    for dep in &macro_type_deps {
        if purpose == ComponentMetaResolutionPurpose::Fallthrough
            && !macro_kind_needed_for_fallthrough(dep.macro_kind)
        {
            continue;
        }
        let direct_macro_reference = is_direct_macro_type_reference(macros, dep);
        if expanded {
            if let Some(mac) = macros.get(dep.macro_index) {
                let authoritative_owner = macro_has_authoritative_owner_surface(
                    mac,
                    eval_outputs.evaluated_types.as_ref(),
                    dep.macro_index,
                );
                let authoritative_resolved_local =
                    macro_has_authoritative_resolved_local_surface(mac);
                let local_type_root = macro_has_direct_local_type_root(mac);
                let skip_non_direct_dep = !direct_macro_reference
                    && (authoritative_owner || authoritative_resolved_local);
                let skip_fallthrough_define_emits = purpose
                    == ComponentMetaResolutionPurpose::Fallthrough
                    && dep.macro_kind == AnalyzedMacroKind::DefineEmits
                    && mac.is_type_based
                    && (!direct_macro_reference || authoritative_owner || local_type_root);
                if skip_non_direct_dep
                    || authoritative_resolved_local
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
                &mut cache,
                &mut visiting,
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
                &mut cache,
                &mut visiting,
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
                props: Vec::new(),
                emits: Vec::new(),
                slots: Vec::new(),
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
                props: Vec::new(),
                emits: Vec::new(),
                slots: Vec::new(),
                jsdoc,
            });
            continue;
        }

        let imported_elements = imported_surface
            .take()
            .map(|surface| surface.elements)
            .or_else(|| {
                host.resolve_macro_elements(
                    owner_canonical,
                    &dep.import_source,
                    dep_exported_name.as_ref(),
                    &mut tracked_deps,
                    &mut resolution_deps,
                    &mut cache,
                    &mut visiting,
                )
            });
        if let Some(elements) = imported_elements {
            let declaration_source = if skip_declaration_metadata {
                None
            } else {
                host.read_source(declaration.canonical_source.as_str())
            };
            let projected =
                project_macro_surfaces(declaration_source.as_deref(), dep.macro_kind, &elements);
            let package_backed_dep = dep_canonical.contains("/node_modules/")
                || declaration.canonical_source.contains("/node_modules/");
            if is_direct_macro_type_reference(macros, dep)
                && !package_backed_dep
                && should_seed_direct_macro_registry_entry(&declaration)
                && seen_registry_names.insert(dep.type_name.clone())
            {
                resolved_type_registry.push(ResolvedTypeAnalysis {
                    name: dep.type_name.clone(),
                    type_expr: resolved_elements_to_type_expr_via_type_text(&elements),
                    type_expansion: None,
                });
                resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                    name: dep.type_name.clone(),
                    declaration: declaration.clone(),
                });
            }
            resolved_macros.push(ResolvedMacroMeta {
                macro_index,
                macro_kind: dep.macro_kind,
                type_name: dep.type_name.clone(),
                import_source: dep.import_source.clone(),
                surface_is_authoritative: true,
                declaration,
                native_props: projected.native_props,
                props: projected.props,
                emits: projected.emits,
                slots: projected.slots,
                jsdoc,
            });
        } else {
            resolved_macros.push(ResolvedMacroMeta {
                macro_index,
                macro_kind: dep.macro_kind,
                type_name: dep.type_name.clone(),
                import_source: dep.import_source.clone(),
                surface_is_authoritative: false,
                declaration,
                native_props: Vec::new(),
                props: Vec::new(),
                emits: Vec::new(),
                slots: Vec::new(),
                jsdoc,
            });
        }
    }

    if expanded {
        for (macro_index, mac) in host.snapshot_macros(snapshot).iter().enumerate() {
            if purpose == ComponentMetaResolutionPurpose::Fallthrough
                && !macro_kind_needed_for_fallthrough(mac.kind)
            {
                continue;
            }
            let owner_source = host.read_source(owner_canonical);
            for (resolved_index, resolved) in mac.resolved_local_types.iter().enumerate() {
                if !is_direct_local_macro_type_reference(
                    mac,
                    resolved_index,
                    resolved.name.as_str(),
                ) {
                    continue;
                }
                if !resolved_macros
                    .iter()
                    .any(|meta| meta.macro_index == macro_index && meta.type_name == resolved.name)
                {
                    if let Some(projected) = owner_source
                        .as_deref()
                        .and_then(|source| {
                            let projection_source = source_for_local_type_projection(source);
                            project_macro_surfaces_from_source_type_name(
                                projection_source.as_ref(),
                                mac.kind,
                                resolved.name.as_str(),
                            )
                        })
                        .or_else(|| {
                            project_macro_surfaces_from_expanded_text(mac.kind, &resolved.expanded)
                        })
                    {
                        if !projected.props.is_empty()
                            || !projected.emits.is_empty()
                            || !projected.slots.is_empty()
                            || !projected.native_props.is_empty()
                        {
                            let declaration =
                                if skip_macro_declaration_metadata_for_purpose(purpose) {
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
                                };
                            let jsdoc = if skip_macro_declaration_metadata_for_purpose(purpose) {
                                None
                            } else {
                                host.resolve_jsdoc_block(
                                    owner_canonical,
                                    resolved.span,
                                    true,
                                    &mut tracked_deps,
                                    &mut cache,
                                    &mut visiting,
                                )
                            };
                            resolved_macros.push(ResolvedMacroMeta {
                                macro_index,
                                macro_kind: mac.kind,
                                type_name: resolved.name.clone(),
                                import_source: String::new(),
                                surface_is_authoritative: true,
                                declaration,
                                native_props: projected.native_props,
                                props: projected.props,
                                emits: projected.emits,
                                slots: projected.slots,
                                jsdoc,
                            });
                        }
                    }
                }

                // Seed only the direct macro-local root into the registry up
                // front. Additional owner-local helpers are discovered later
                // from the queried root surface during registry append, which
                // keeps publication demand-driven instead of prepublishing the
                // entire same-file helper chain.
                if resolved_index == 0 && seen_registry_names.insert(resolved.name.clone()) {
                    resolved_type_registry.push(ResolvedTypeAnalysis {
                        name: resolved.name.clone(),
                        type_expr: resolved.type_expr.clone().unwrap_or_else(|| {
                            verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                                &resolved.expanded,
                            )
                        }),
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
    }
}

fn macro_kind_needed_for_fallthrough(kind: AnalyzedMacroKind) -> bool {
    matches!(
        kind,
        AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::WithDefaults
            | AnalyzedMacroKind::DefineModel
            | AnalyzedMacroKind::DefineEmits
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::declaration_metadata::ResolvedExportTarget;
    use std::collections::BTreeMap;
    use verter_compiler::utils::oxc::vue::resolve_type::{
        ResolvedEmit, ResolvedEmitSignature, ResolvedMemberVisibility, ResolvedProp, RuntimeType,
    };
    use verter_semantic::analysis::type_eval::DeclarationId;
    use verter_semantic::analysis::type_expr::PrimitiveName;
    use verter_semantic::analysis::types::{
        AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind, ImportBindingKind,
        ResolvedLocalType,
    };
    use verter_span::Span;

    #[derive(Clone)]
    struct TestSnapshot {
        imports: Vec<AnalyzedImport>,
        macros: Vec<AnalyzedMacro>,
        macro_type_deps: Vec<verter_semantic::analysis::types::MacroTypeDep>,
    }

    struct TestHost {
        source: String,
        external_macro_elements: BTreeMap<(String, String), ResolvedElements>,
        eval_outputs: ComponentMetaEvalOutputs,
    }

    impl crate::resolver_core::DeclarationMetadataResolver for TestHost {
        fn resolve_export_target(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<ResolvedExportTarget> {
            None
        }

        fn get_export_span_follow_reexports(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<Span> {
            None
        }

        fn read_source(&self, _canonical_source: &str) -> Option<String> {
            Some(self.source.clone())
        }

        fn type_declaration_id(
            &self,
            _canonical_source: &str,
            _resolved_name: &str,
        ) -> Option<DeclarationId> {
            None
        }

        fn resolve_type_dependency_canonical(
            &self,
            _from_canonical: &str,
            _import_source: &str,
        ) -> Option<String> {
            None
        }
    }

    impl ComponentMetaResolverHost for TestHost {
        type Snapshot = TestSnapshot;
        type EvalContext = ();

        fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport] {
            &snapshot.imports
        }

        fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro] {
            &snapshot.macros
        }

        fn snapshot_macro_type_deps<'a>(
            &self,
            snapshot: &'a Self::Snapshot,
        ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
            &snapshot.macro_type_deps
        }

        fn build_eval_outputs(
            &self,
            _owner_canonical: &str,
            _snapshot: &Self::Snapshot,
            _eval_context: Option<&Self::EvalContext>,
            _purpose: ComponentMetaResolutionPurpose,
        ) -> ComponentMetaEvalOutputs {
            self.eval_outputs.clone()
        }

        fn resolve_macro_elements(
            &self,
            _owner_canonical: &str,
            import_source: &str,
            exported_name: &str,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
        ) -> Option<ResolvedElements> {
            self.external_macro_elements
                .get(&(import_source.to_string(), exported_name.to_string()))
                .cloned()
        }

        fn resolve_jsdoc_block(
            &self,
            _canonical_source: &str,
            _span: Span,
            _expanded: bool,
            _tracked_deps: &mut BTreeSet<String>,
            _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
        ) -> Option<ResolvedJsdocBlock> {
            None
        }

        fn sync_transitive_macro_type_dependencies(
            &self,
            _canonical_id: &str,
            _tracked_deps: &BTreeSet<String>,
        ) {
        }

        fn current_dependency_fact_versions(
            &self,
            _canonical: &str,
            _tracked_deps: &BTreeSet<String>,
        ) -> Vec<FactVersionRef> {
            Vec::new()
        }
    }

    struct CombinedSurfaceTestHost {
        source: String,
        imported_surface_calls: std::cell::Cell<usize>,
        eval_outputs: ComponentMetaEvalOutputs,
    }

    impl crate::resolver_core::DeclarationMetadataResolver for CombinedSurfaceTestHost {
        fn resolve_export_target(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<ResolvedExportTarget> {
            None
        }

        fn get_export_span_follow_reexports(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<Span> {
            None
        }

        fn read_source(&self, _canonical_source: &str) -> Option<String> {
            Some(self.source.clone())
        }

        fn type_declaration_id(
            &self,
            _canonical_source: &str,
            _resolved_name: &str,
        ) -> Option<DeclarationId> {
            None
        }

        fn resolve_type_dependency_canonical(
            &self,
            _from_canonical: &str,
            _import_source: &str,
        ) -> Option<String> {
            Some("/dep.ts".to_string())
        }
    }

    impl ComponentMetaResolverHost for CombinedSurfaceTestHost {
        type Snapshot = TestSnapshot;
        type EvalContext = ();

        fn resolve_type_declaration(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> ResolvedTypeDeclaration {
            panic!(
                "resolve_component_meta_parts should use the combined imported-macro surface path"
            );
        }

        fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport] {
            &snapshot.imports
        }

        fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro] {
            &snapshot.macros
        }

        fn snapshot_macro_type_deps<'a>(
            &self,
            snapshot: &'a Self::Snapshot,
        ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
            &snapshot.macro_type_deps
        }

        fn build_eval_outputs(
            &self,
            _owner_canonical: &str,
            _snapshot: &Self::Snapshot,
            _eval_context: Option<&Self::EvalContext>,
            _purpose: ComponentMetaResolutionPurpose,
        ) -> ComponentMetaEvalOutputs {
            self.eval_outputs.clone()
        }

        fn resolve_macro_elements(
            &self,
            _owner_canonical: &str,
            _import_source: &str,
            _exported_name: &str,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
        ) -> Option<ResolvedElements> {
            panic!("resolve_component_meta_parts should not separately ask for imported macro elements");
        }

        fn resolve_imported_macro_surface(
            &self,
            _owner_canonical: &str,
            _import_source: &str,
            exported_name: &str,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
        ) -> Option<ResolvedImportedMacroSurface> {
            self.imported_surface_calls
                .set(self.imported_surface_calls.get() + 1);
            Some(ResolvedImportedMacroSurface {
                declaration: ResolvedTypeDeclaration {
                    requested_name: exported_name.to_string(),
                    declaration_id: None,
                    resolved_name: "Props".to_string(),
                    canonical_source: "/dep.ts".to_string(),
                    span: Span::new(0, 29),
                    kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
                    text: Some("export interface Props { label: string }".to_string()),
                },
                elements: ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 29),
                        key: Span::new(24, 29),
                        key_name: Some("label".to_string()),
                        optional: false,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: Some(Span::new(31, 37)),
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    emits: vec![ResolvedEmit {
                        span: Span::new(0, 24),
                        name: "save".to_string(),
                        name_span: None,
                        signature: ResolvedEmitSignature::Tuple {
                            tuple_text: "[value: string]".to_string(),
                        },
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            })
        }

        fn resolve_jsdoc_block(
            &self,
            _canonical_source: &str,
            _span: Span,
            _expanded: bool,
            _tracked_deps: &mut BTreeSet<String>,
            _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
        ) -> Option<ResolvedJsdocBlock> {
            None
        }

        fn sync_transitive_macro_type_dependencies(
            &self,
            _canonical_id: &str,
            _tracked_deps: &BTreeSet<String>,
        ) {
        }

        fn current_dependency_fact_versions(
            &self,
            _canonical: &str,
            _tracked_deps: &BTreeSet<String>,
        ) -> Vec<FactVersionRef> {
            Vec::new()
        }
    }

    #[test]
    fn resolve_component_meta_parts_prefers_combined_imported_macro_surface() {
        let host = CombinedSurfaceTestHost {
            source: "export interface Props { label: string }".to_string(),
            imported_surface_calls: std::cell::Cell::new(0),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: vec![AnalyzedImport {
                source: "./dep".to_string(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "Props".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: Some("Props".to_string()),
                    is_type_only: true,
                    vue_api: None,
                    span: Span::new(0, 5),
                }],
                span: Span::new(0, 26),
                resolved_canonical_id: Some("/dep.ts".to_string()),
            }],
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                span: Span::new(0, 20),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                macro_index: 0,
                import_source: "./dep".to_string(),
                type_name: "Props".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_span: Span::new(0, 20),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert_eq!(
            host.imported_surface_calls.get(),
            1,
            "expanded imported-macro resolution should use the combined surface path exactly once",
        );
        assert_eq!(resolved.resolved_macros.len(), 1);
        assert_eq!(resolved.resolved_macros[0].props[0].name, "label");
        assert_eq!(
            resolved.resolved_macros[0].declaration.canonical_source, "/dep.ts",
            "combined imported-macro resolution should still preserve declaration ownership",
        );
    }

    #[test]
    fn resolve_component_meta_parts_fallthrough_reuses_combined_imported_macro_surface() {
        let host = CombinedSurfaceTestHost {
            source: "export type Emits = { save: [value: string] }".to_string(),
            imported_surface_calls: std::cell::Cell::new(0),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: vec![AnalyzedImport {
                source: "./dep".to_string(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "Emits".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: Some("Emits".to_string()),
                    is_type_only: true,
                    vue_api: None,
                    span: Span::new(0, 5),
                }],
                span: Span::new(0, 26),
                resolved_canonical_id: Some("/dep.ts".to_string()),
            }],
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec!["Emits".to_string()],
                binding_name: Some("emit".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                span: Span::new(0, 20),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                macro_index: 0,
                import_source: "./dep".to_string(),
                type_name: "Emits".to_string(),
                macro_kind: AnalyzedMacroKind::DefineEmits,
                macro_span: Span::new(0, 20),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Fallthrough,
        );

        assert_eq!(
            host.imported_surface_calls.get(),
            1,
            "fallthrough imported-macro resolution should still use the combined surface path",
        );
        assert_eq!(resolved.resolved_macros.len(), 1);
        assert_eq!(resolved.resolved_macros[0].emits.len(), 1);
        assert_eq!(resolved.resolved_macros[0].emits[0].name, "save");
        assert_eq!(
            resolved.resolved_macros[0].declaration.canonical_source, "",
            "fallthrough should still skip declaration ownership materialization",
        );
    }

    #[test]
    fn resolve_component_meta_parts_fallthrough_skips_imported_define_emits_when_eval_shape_exists()
    {
        let host = CombinedSurfaceTestHost {
            source: "export type Emits = { save: [value: string] }".to_string(),
            imported_surface_calls: std::cell::Cell::new(0),
            eval_outputs: ComponentMetaEvalOutputs {
                evaluated_types: Some(
                    verter_semantic::analysis::type_expand::ExpandedComponentTypes {
                        props: Vec::new(),
                        define_props: Vec::new(),
                        define_emits: vec![
                            verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                                macro_index: 0,
                                result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                    verter_semantic::analysis::type_expand::ExpandedObjectShape {
                                        properties: vec![
                                            verter_semantic::analysis::type_expand::ExpandedProperty {
                                                name: "save".to_string(),
                                                ty: verter_semantic::analysis::type_expr::TypeExpr::Tuple {
                                                    elements: std::sync::Arc::from(vec![
                                                        verter_semantic::analysis::type_expr::TupleElement {
                                                            label: Some("value".to_string()),
                                                            ty: verter_semantic::analysis::type_expr::TypeExpr::Primitive(
                                                                verter_semantic::analysis::type_expr::PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            rest: false,
                                                        },
                                                    ]),
                                                    readonly: false,
                                                },
                                                optional: false,
                                                readonly: false,
                                            },
                                        ],
                                        index_signatures: Vec::new(),
                                        call_signatures: Vec::new(),
                                    },
                                ),
                            },
                        ],
                        emits: Vec::new(),
                        define_slots: Vec::new(),
                        slot_bindings: Vec::new(),
                        bindings: Vec::new(),
                    },
                ),
                tracked_dependencies: BTreeSet::new(),
            },
        };
        let snapshot = TestSnapshot {
            imports: vec![AnalyzedImport {
                source: "./dep".to_string(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "Emits".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: Some("Emits".to_string()),
                    is_type_only: true,
                    vue_api: None,
                    span: Span::new(0, 5),
                }],
                span: Span::new(0, 26),
                resolved_canonical_id: Some("/dep.ts".to_string()),
            }],
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec!["Emits".to_string()],
                binding_name: Some("emit".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                span: Span::new(0, 20),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                macro_index: 0,
                import_source: "./dep".to_string(),
                type_name: "Emits".to_string(),
                macro_kind: AnalyzedMacroKind::DefineEmits,
                macro_span: Span::new(0, 20),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Fallthrough,
        );

        assert_eq!(
            host.imported_surface_calls.get(),
            0,
            "fallthrough should keep type-based imported defineEmits on the evaluated shape path when that shape is already authoritative",
        );
        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "Emits"),
            "fallthrough should not materialize an imported defineEmits surface when evaluated_types already provide the declared events"
        );
        let evaluated = resolved
            .evaluated_types
            .as_ref()
            .expect("the authoritative evaluated defineEmits shape should be preserved");
        assert_eq!(
            evaluated.define_emits.len(),
            1,
            "the evaluated defineEmits shape should still drive downstream extraction",
        );
    }

    #[test]
    fn resolve_component_meta_parts_fallthrough_skips_imported_define_emits_for_local_wrapper_root()
    {
        let source = r#"
import type { RootEmits } from './dep'

interface Emits extends RootEmits {}

defineEmits<Emits>()
"#;
        let host = CombinedSurfaceTestHost {
            source: source.to_string(),
            imported_surface_calls: std::cell::Cell::new(0),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: vec![AnalyzedImport {
                source: "./dep".to_string(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "RootEmits".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: Some("RootEmits".to_string()),
                    is_type_only: true,
                    vue_api: None,
                    span: Span::new(0, 9),
                }],
                span: Span::new(0, 34),
                resolved_canonical_id: Some("/dep.ts".to_string()),
            }],
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec!["Emits".to_string()],
                binding_name: Some("emit".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Emits".to_string(),
                    expanded: "interface Emits extends RootEmits {}".to_string(),
                    type_expr: None,
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                macro_index: 0,
                import_source: "./dep".to_string(),
                type_name: "RootEmits".to_string(),
                macro_kind: AnalyzedMacroKind::DefineEmits,
                macro_span: Span::new(0, source.len() as u32),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Fallthrough,
        );

        assert_eq!(
            host.imported_surface_calls.get(),
            0,
            "fallthrough should leave transitive imported defineEmits deps lazy when an owner-local wrapper root will drive object-shape projection later",
        );
        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "RootEmits"),
            "fallthrough should keep the imported defineEmits root off resolved_macros when the owner-local wrapper is the requested route",
        );
    }

    #[test]
    fn local_resolved_macro_types_project_into_resolved_macro_surfaces() {
        let source =
            "type AccordionEmits = { 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }";
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::new(),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec!["AccordionEmits".to_string()],
                binding_name: Some("emit".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "AccordionEmits".to_string(),
                    expanded:
                        "{ 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }"
                            .to_string(),
                    type_expr: None,
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: Vec::new(),
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/Accordion.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert_eq!(resolved.resolved_macros.len(), 1);
        assert_eq!(resolved.resolved_macros[0].macro_index, 0);
        assert_eq!(
            resolved.resolved_macros[0].macro_kind,
            AnalyzedMacroKind::DefineEmits
        );
        assert_eq!(resolved.resolved_macros[0].type_name, "AccordionEmits");
        assert_eq!(
            resolved.resolved_macros[0].declaration.resolved_name,
            "AccordionEmits"
        );
        assert_eq!(resolved.resolved_macros[0].emits.len(), 1);
        assert_eq!(
            resolved.resolved_macros[0].emits[0].name,
            "update:modelValue"
        );
        assert_eq!(
            resolved.resolved_macros[0].emits[0].payload_type.as_deref(),
            Some("[value: (T extends 'single' ? string : string[]) | undefined]")
        );
    }

    #[test]
    fn local_resolved_slot_types_project_symbolic_pick_bindings() {
        let source = r#"
interface CalendarCellTriggerProps {
  day: Date
  month: number
}

interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}

defineSlots<CalendarSlots>()
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::new(),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineSlots,
                is_type_based: true,
                type_references: vec!["CalendarSlots".to_string()],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "CalendarSlots".to_string(),
                    expanded: "{ day?: (props: { day: Date }) => any }".to_string(),
                    type_expr: None,
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: Vec::new(),
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert_eq!(resolved.resolved_macros.len(), 1);
        assert_eq!(resolved.resolved_macros[0].slots.len(), 1);
        assert_eq!(resolved.resolved_macros[0].slots[0].bindings.len(), 1);
        assert_eq!(
            resolved.resolved_macros[0].slots[0].bindings[0]
                .type_annotation
                .as_deref(),
            Some("CalendarCellTriggerProps['day']")
        );
    }

    #[test]
    fn resolve_component_meta_parts_does_not_preseed_transitive_imported_macro_deps() {
        let source = r#"
type Props = {
  item?: LocalItem
}

type LocalItem = {
  label?: string
}
"#;
        let mut external_macro_elements = BTreeMap::new();
        external_macro_elements.insert(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("href".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                }],
                ..ResolvedElements::default()
            },
        );
        external_macro_elements.insert(
            ("./types".to_string(), "ImportedKeys".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("value".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("'href' | 'target'".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                }],
                ..ResolvedElements::default()
            },
        );
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements,
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "{ item?: LocalItem }".to_string(),
                    type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "item".to_string(),
                            ty: TypeExpr::Ref {
                                name: "LocalItem".into(),
                                type_arguments: Vec::new().into(),
                            },
                            optional: true,
                            readonly: false,
                        })],
                    }))),
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![
                verter_semantic::analysis::types::MacroTypeDep {
                    type_name: "ImportedBase".to_string(),
                    import_source: "./types".to_string(),
                    macro_kind: AnalyzedMacroKind::DefineProps,
                    macro_index: 0,
                    macro_span: Span::new(0, source.len() as u32),
                },
                verter_semantic::analysis::types::MacroTypeDep {
                    type_name: "ImportedKeys".to_string(),
                    import_source: "./types".to_string(),
                    macro_kind: AnalyzedMacroKind::DefineProps,
                    macro_index: 0,
                    macro_span: Span::new(0, source.len() as u32),
                },
            ],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );
        let registry_names: Vec<&str> = resolved
            .resolved_type_registry
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();

        assert_eq!(
            registry_names,
            vec!["Props"],
            "transitive imported macro deps should stay out of the initial registry seed; later append can publish only what the owner surface actually requests"
        );
    }

    #[test]
    fn resolve_component_meta_parts_skips_transitive_imported_macro_resolution_when_eval_surface_is_authoritative(
    ) {
        let source = r#"
type Props = Pick<ImportedBase, 'href'>
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "ImportedBase".to_string()),
                ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("href".to_string()),
                        optional: true,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs {
                evaluated_types: Some(verter_semantic::analysis::type_expand::ExpandedComponentTypes {
                    define_props: vec![
                        verter_semantic::analysis::type_expand::ExpandedMacroProps {
                            macro_index: 0,
                            result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                verter_semantic::analysis::type_expand::ExpandedObjectShape {
                                    properties: vec![
                                        verter_semantic::analysis::type_expand::ExpandedProperty {
                                            name: "href".to_string(),
                                            ty: TypeExpr::Primitive(PrimitiveName::String),
                                            optional: true,
                                            readonly: false,
                                        },
                                    ],
                                    index_signatures: Vec::new(),
                                    call_signatures: Vec::new(),
                                },
                            ),
                        },
                    ],
                    ..Default::default()
                }),
                tracked_dependencies: BTreeSet::new(),
            },
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "{ href?: string }".to_string(),
                    type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "href".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: true,
                            readonly: false,
                        })],
                    }))),
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedBase".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "ImportedBase"),
            "authoritative owner-evaluated surfaces should keep transitive imported macro deps off resolved_macros: {:?}",
            resolved
                .resolved_macros
                .iter()
                .map(|entry| entry.type_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolve_component_meta_parts_skips_transitive_imported_macro_resolution_when_raw_surface_is_authoritative(
    ) {
        let source = r#"
type Props = Pick<ImportedBase, 'href'>
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "ImportedBase".to_string()),
                ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("href".to_string()),
                        optional: true,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "href".to_string(),
                    is_optional: true,
                    span: Span::new(0, 0),
                    type_annotation: Some("string".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "{ href?: string }".to_string(),
                    type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "href".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: true,
                            readonly: false,
                        })],
                    }))),
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedBase".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "ImportedBase"),
            "authoritative owner-local raw fields should keep transitive imported macro deps off resolved_macros: {:?}",
            resolved
                .resolved_macros
                .iter()
                .map(|entry| entry.type_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolve_component_meta_parts_skips_transitive_imported_macro_resolution_when_resolved_local_surface_is_authoritative(
    ) {
        let source = r#"
type Props = {
  href?: ImportedBase['href']
}
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "ImportedBase".to_string()),
                ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("href".to_string()),
                        optional: true,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "{ href?: ImportedBase['href'] }".to_string(),
                    type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "href".to_string(),
                            ty: TypeExpr::IndexedAccess {
                                object: std::sync::Arc::new(TypeExpr::Ref {
                                    name: "ImportedBase".into(),
                                    type_arguments: Vec::new().into(),
                                }),
                                index: std::sync::Arc::new(TypeExpr::string_literal("href")),
                            },
                            optional: true,
                            readonly: false,
                        })],
                    }))),
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedBase".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "ImportedBase"),
            "authoritative resolved-local surfaces should keep transitive imported macro deps off resolved_macros: {:?}",
            resolved
                .resolved_macros
                .iter()
                .map(|entry| entry.type_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolve_component_meta_parts_skips_direct_imported_macro_resolution_when_resolved_local_surface_is_authoritative(
    ) {
        let source = r#"
type Props = {
  tooltip?: ImportedBase
}
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "ImportedBase".to_string()),
                ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("label".to_string()),
                        optional: true,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string(), "ImportedBase".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "{ tooltip?: ImportedBase }".to_string(),
                    type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "tooltip".to_string(),
                            ty: TypeExpr::Ref {
                                name: "ImportedBase".into(),
                                type_arguments: Vec::new().into(),
                            },
                            optional: true,
                            readonly: false,
                        })],
                    }))),
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedBase".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "ImportedBase"),
            "authoritative resolved-local surfaces should keep nested direct imported macro deps off resolved_macros: {:?}",
            resolved
                .resolved_macros
                .iter()
                .map(|entry| entry.type_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolve_component_meta_parts_skips_direct_imported_macro_resolution_when_local_wrapper_type_expr_is_available(
    ) {
        let source = r#"
type Props = Omit<ImportedBase, 'hidden'>
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "ImportedBase".to_string()),
                ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("label".to_string()),
                        optional: true,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string(), "ImportedBase".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "Omit<ImportedBase, 'hidden'>".to_string(),
                    type_expr: Some(
                        verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                            "Omit<ImportedBase, 'hidden'>",
                        ),
                    ),
                    span: Span::new(0, source.len() as u32),
                }],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedBase".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );

        assert!(
            resolved
                .resolved_macros
                .iter()
                .all(|entry| entry.type_name != "ImportedBase"),
            "local wrapper type_expr should keep direct imported macro deps off resolved_macros: {:?}",
            resolved
                .resolved_macros
                .iter()
                .map(|entry| entry.type_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resolve_component_meta_parts_keeps_direct_imported_macro_root_seeded() {
        let host = TestHost {
            source: String::new(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "Props".to_string()),
                ResolvedElements {
                    props: vec![ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("label".to_string()),
                        optional: false,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                    }],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                span: Span::new(0, 1),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "Props".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, 1),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );
        let registry_names: Vec<&str> = resolved
            .resolved_type_registry
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();

        assert_eq!(
            registry_names,
            vec!["Props"],
            "the direct imported macro root should still seed the initial registry"
        );
    }

    #[test]
    fn resolve_component_meta_parts_skips_direct_non_object_imported_macro_seed() {
        let host = TestHost {
            source: "export type StringOrVNode = string | VNode | (() => VNode);".to_string(),
            external_macro_elements: BTreeMap::from([(
                ("./types".to_string(), "StringOrVNode".to_string()),
                ResolvedElements {
                    props: vec![
                        ResolvedProp {
                            span: Span::new(0, 0),
                            key: Span::new(0, 0),
                            key_name: Some("component".to_string()),
                            optional: true,
                            types: vec![RuntimeType::Object],
                            visibility: ResolvedMemberVisibility::Public,
                            type_span: None,
                            type_text: Some("object".to_string()),
                            map_local: false,
                            span_is_absolute: true,
                        },
                        ResolvedProp {
                            span: Span::new(0, 0),
                            key: Span::new(0, 0),
                            key_name: Some("children".to_string()),
                            optional: true,
                            types: vec![RuntimeType::String],
                            visibility: ResolvedMemberVisibility::Public,
                            type_span: None,
                            type_text: Some("string".to_string()),
                            map_local: false,
                            span_is_absolute: true,
                        },
                    ],
                    ..ResolvedElements::default()
                },
            )]),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["StringOrVNode".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                span: Span::new(0, 1),
            }],
            macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
                type_name: "StringOrVNode".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, 1),
            }],
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );
        assert!(
            resolved.resolved_type_registry.is_empty(),
            "direct non-object imported aliases should stay out of the initial registry seed"
        );
    }

    #[test]
    fn resolve_component_meta_parts_keeps_non_root_local_helpers_off_resolved_macros() {
        let source = r#"
type Props = Helper

interface Helper {
  label?: string
}
"#;
        let host = TestHost {
            source: source.to_string(),
            external_macro_elements: BTreeMap::new(),
            eval_outputs: ComponentMetaEvalOutputs::default(),
        };
        let snapshot = TestSnapshot {
            imports: Vec::new(),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: vec![
                    ResolvedLocalType {
                        name: "Props".to_string(),
                        expanded: "{ label?: string }".to_string(),
                        type_expr: Some(TypeExpr::Ref {
                            name: "Helper".into(),
                            type_arguments: Vec::new().into(),
                        }),
                        span: Span::new(0, "type Props = Helper".len() as u32),
                    },
                    ResolvedLocalType {
                        name: "Helper".to_string(),
                        expanded: "{ label?: string }".to_string(),
                        type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                            properties: vec![ObjectMember::Property(ObjectProperty {
                                name: "label".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: true,
                                readonly: false,
                            })],
                        }))),
                        span: Span::new(0, source.len() as u32),
                    },
                ],
                span: Span::new(0, source.len() as u32),
            }],
            macro_type_deps: Vec::new(),
        };

        let resolved = resolve_component_meta_parts(
            &host,
            "/src/App.vue",
            &snapshot,
            true,
            None,
            ComponentMetaResolutionPurpose::Full,
        );
        let resolved_names: Vec<&str> = resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect();

        assert_eq!(
            resolved_names,
            vec!["Props"],
            "only the direct local macro root should project into resolved_macros; helper companions should stay lazy",
        );
    }
}

pub fn resolved_elements_to_type_expr_via_type_text(
    resolved: &ResolvedElements,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let properties = resolved
        .props
        .iter()
        .map(|prop| {
            let ty = prop
                .type_text
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
                .unwrap_or(TypeExpr::Unknown {
                    raw: "unknown".to_string(),
                });
            ObjectMember::Property(ObjectProperty {
                name: prop
                    .key_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                ty,
                optional: prop.optional,
                readonly: false,
            })
        })
        .collect();

    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
}

fn should_ignore_external_macro_type(dep: &MacroTypeDep) -> bool {
    dep.macro_kind == AnalyzedMacroKind::DefineSlots
        && dep.import_source == "vue"
        && dep.type_name == "Slot"
}

fn is_direct_macro_type_reference(macros: &[AnalyzedMacro], dep: &MacroTypeDep) -> bool {
    macros.get(dep.macro_index).is_some_and(|mac| {
        mac.type_references
            .iter()
            .any(|type_name| type_name == &dep.type_name)
    })
}

fn is_direct_local_macro_type_reference(
    mac: &AnalyzedMacro,
    resolved_index: usize,
    resolved_name: &str,
) -> bool {
    resolved_index == 0
        || mac
            .type_references
            .iter()
            .any(|type_name| type_name == resolved_name)
}

fn macro_has_authoritative_evaluated_surface(
    evaluated: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    macro_kind: AnalyzedMacroKind,
    macro_index: usize,
) -> bool {
    let Some(evaluated) = evaluated else {
        return false;
    };

    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => evaluated
            .define_props
            .iter()
            .find(|entry| entry.macro_index == macro_index)
            .is_some_and(|entry| !entry.result.value.properties.is_empty()),
        AnalyzedMacroKind::DefineEmits => evaluated
            .define_emits
            .iter()
            .find(|entry| entry.macro_index == macro_index)
            .is_some_and(|entry| {
                !entry.result.value.properties.is_empty()
                    || !entry.result.value.call_signatures.is_empty()
            }),
        AnalyzedMacroKind::DefineSlots => evaluated
            .define_slots
            .iter()
            .find(|entry| entry.macro_index == macro_index)
            .is_some_and(|entry| !entry.result.value.properties.is_empty()),
        AnalyzedMacroKind::DefineExpose => !evaluated.bindings.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

fn macro_has_authoritative_owner_surface(
    mac: &AnalyzedMacro,
    evaluated: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> bool {
    if macro_has_authoritative_evaluated_surface(evaluated, mac.kind, macro_index) {
        return true;
    }

    match mac.kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => !mac.prop_fields.is_empty(),
        AnalyzedMacroKind::DefineEmits => !mac.emit_fields.is_empty(),
        AnalyzedMacroKind::DefineSlots => !mac.slot_fields.is_empty(),
        AnalyzedMacroKind::DefineExpose => !mac.expose_fields.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

fn macro_has_authoritative_resolved_local_surface(mac: &AnalyzedMacro) -> bool {
    mac.resolved_local_types
        .iter()
        .enumerate()
        .filter(|(resolved_index, resolved)| {
            is_direct_local_macro_type_reference(mac, *resolved_index, resolved.name.as_str())
        })
        .any(|(_, resolved)| {
            project_macro_surfaces_from_expanded_text(mac.kind, &resolved.expanded).is_some_and(
                |projected| {
                    !projected.native_props.is_empty()
                        || !projected.props.is_empty()
                        || !projected.emits.is_empty()
                        || !projected.slots.is_empty()
                },
            ) || resolved_local_type_expr_can_drive_authoritative_projection(mac.kind, resolved)
        })
}

fn macro_has_direct_local_type_root(mac: &AnalyzedMacro) -> bool {
    mac.resolved_local_types
        .iter()
        .enumerate()
        .any(|(resolved_index, resolved)| {
            is_direct_local_macro_type_reference(mac, resolved_index, resolved.name.as_str())
        })
}

fn resolved_local_type_expr_can_drive_authoritative_projection(
    macro_kind: AnalyzedMacroKind,
    resolved: &verter_semantic::analysis::types::ResolvedLocalType,
) -> bool {
    matches!(
        macro_kind,
        AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::WithDefaults
            | AnalyzedMacroKind::DefineModel
    ) && resolved.type_expr.is_some()
}

fn macro_dep_exported_type_name<'a>(
    imports: &'a [AnalyzedImport],
    dep: &'a MacroTypeDep,
) -> Cow<'a, str> {
    for import in imports
        .iter()
        .filter(|import| import.source == dep.import_source)
    {
        for binding in &import.bindings {
            if dep.type_name == binding.name {
                return Cow::Owned(
                    binding
                        .imported_name
                        .clone()
                        .unwrap_or_else(|| binding.name.clone()),
                );
            }

            if matches!(
                binding.kind,
                verter_semantic::analysis::types::ImportBindingKind::Namespace
            ) {
                let prefix = format!("{}.", binding.name);
                if let Some(member_name) = dep.type_name.strip_prefix(&prefix) {
                    return Cow::Owned(member_name.to_string());
                }
            }
        }
    }

    Cow::Borrowed(dep.type_name.as_str())
}

fn source_for_local_type_projection(source: &str) -> Cow<'_, str> {
    if !source.contains("<script") {
        return Cow::Borrowed(source);
    }

    let mut cursor = 0usize;
    let mut extracted = String::new();
    while let Some(start_rel) = source[cursor..].find("<script") {
        let tag_start = cursor + start_rel;
        let Some(tag_end_rel) = source[tag_start..].find('>') else {
            break;
        };
        let content_start = tag_start + tag_end_rel + 1;
        let Some(close_rel) = source[content_start..].find("</script>") else {
            break;
        };
        let content_end = content_start + close_rel;
        let content = source[content_start..content_end].trim();
        if !content.is_empty() {
            if !extracted.is_empty() {
                extracted.push('\n');
            }
            extracted.push_str(content);
            extracted.push('\n');
        }
        cursor = content_end + "</script>".len();
    }

    if extracted.is_empty() {
        Cow::Borrowed(source)
    } else {
        Cow::Owned(extracted)
    }
}

fn should_seed_direct_macro_registry_entry(declaration: &ResolvedTypeDeclaration) -> bool {
    if declaration.kind != crate::resolver_core::ResolvedDeclarationKind::TypeAlias {
        return true;
    }
    let Some(text) = declaration.text.as_deref() else {
        return true;
    };
    let Some(body_text) = type_alias_body_text(text) else {
        return true;
    };
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(body_text);
    !component_meta_registry_has_non_object_top_level_surface(&parsed)
}

fn type_alias_body_text(text: &str) -> Option<&str> {
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => in_string = Some(ch),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '=' if angle_depth == 0
                && paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0 =>
            {
                return Some(
                    text[idx + ch.len_utf8()..]
                        .trim()
                        .trim_end_matches(';')
                        .trim(),
                );
            }
            _ => {}
        }
    }

    None
}
