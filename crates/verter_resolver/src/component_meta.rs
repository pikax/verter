use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_analysis::component_meta::ResolvedTypeAnalysis;
use verter_analysis::type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};
use verter_analysis::types::{AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, MacroTypeDep};
use verter_core::utils::oxc::vue::resolve_type::ResolvedElements;

use crate::{
    project_macro_surfaces, resolve_local_type_declaration, resolve_type_declaration,
    surface_projector::{
        project_macro_surfaces_from_expanded_text, project_macro_surfaces_from_source_type_name,
    },
    DeclarationMetadataResolver, FactVersionRef, ResolvedNativeProp, ResolvedTypeDeclaration,
};

#[derive(Debug, Clone)]
pub struct ComponentMetaEvalOutputs<I> {
    pub evaluated_types: Option<verter_analysis::type_expand::ExpandedComponentTypes>,
    pub cached_eval_inputs: Option<Arc<I>>,
    pub tracked_dependencies: BTreeSet<String>,
}

impl<I> Default for ComponentMetaEvalOutputs<I> {
    fn default() -> Self {
        Self {
            evaluated_types: None,
            cached_eval_inputs: None,
            tracked_dependencies: BTreeSet::new(),
        }
    }
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
    pub declaration: ResolvedTypeDeclaration,
    pub native_props: Vec<ResolvedNativeProp>,
    pub props: Vec<verter_analysis::AnalyzedPropField>,
    pub emits: Vec<verter_analysis::AnalyzedEmitField>,
    pub slots: Vec<verter_analysis::AnalyzedSlotField>,
    pub jsdoc: Option<ResolvedJsdocBlock>,
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
    pub resolved_type: Option<verter_analysis::type_expr::TypeExpr>,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaParts<I> {
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    pub resolved_type_registry: Vec<ResolvedTypeAnalysis>,
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    pub evaluated_types: Option<verter_analysis::type_expand::ExpandedComponentTypes>,
    pub cached_eval_inputs: Option<Arc<I>>,
    pub fact_versions: Vec<FactVersionRef>,
}

pub fn component_meta_resolved_macros(
    snapshot_macros: &[AnalyzedMacro],
    resolved_macros: &[ResolvedMacroMeta],
) -> Vec<verter_analysis::component_meta::ResolvedMacroInput> {
    resolved_macros
        .iter()
        .filter(|resolved| {
            snapshot_macros
                .get(resolved.macro_index)
                .is_none_or(|mac| !raw_macro_surface_is_authoritative(mac))
        })
        .map(
            |resolved| verter_analysis::component_meta::ResolvedMacroInput {
                macro_index: resolved.macro_index,
                props: resolved.props.clone(),
                emits: resolved.emits.clone(),
                slots: resolved.slots.clone(),
            },
        )
        .collect()
}

pub fn component_meta_type_registry(
    resolved_type_registry: &[verter_analysis::component_meta::ResolvedTypeAnalysis],
) -> Vec<verter_analysis::component_meta::ResolvedTypeAnalysis> {
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
    type ImportedInputs;

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
    ) -> ComponentMetaEvalOutputs<Self::ImportedInputs>;

    #[allow(clippy::too_many_arguments)]
    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedElements>;

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut BTreeSet<String>,
        cache: &mut crate::ExternalTypeBodyCache,
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

pub fn resolve_component_meta_parts<H>(
    host: &H,
    owner_canonical: &str,
    snapshot: &H::Snapshot,
    expanded: bool,
    eval_context: Option<&H::EvalContext>,
) -> ResolvedComponentMetaParts<H::ImportedInputs>
where
    H: ComponentMetaResolverHost,
{
    let mut resolved_macros = Vec::new();
    let mut resolved_type_registry = Vec::new();
    let mut resolved_type_registry_meta = Vec::new();
    let mut seen_registry_names = FxHashSet::default();
    let mut cache = crate::ExternalTypeBodyCache::default();
    let mut visiting = FxHashSet::default();
    let mut tracked_deps = BTreeSet::new();

    let eval_outputs = if expanded {
        host.build_eval_outputs(owner_canonical, snapshot, eval_context)
    } else {
        ComponentMetaEvalOutputs::default()
    };
    tracked_deps.extend(eval_outputs.tracked_dependencies.iter().cloned());

    let imports = host.snapshot_imports(snapshot);
    let macro_type_deps: Vec<MacroTypeDep> = host.snapshot_macro_type_deps(snapshot).to_vec();
    for dep in &macro_type_deps {
        let macro_index = dep.macro_index;
        let dep_exported_name = macro_dep_exported_type_name(imports, dep);
        let dep_canonical = host
            .resolve_type_dependency_canonical(owner_canonical, &dep.import_source)
            .unwrap_or_default();
        let declaration = host.resolve_type_declaration(&dep_canonical, dep_exported_name.as_ref());
        let jsdoc = host.resolve_jsdoc_block(
            declaration.canonical_source.as_str(),
            declaration.span,
            expanded,
            &mut tracked_deps,
            &mut cache,
            &mut visiting,
        );

        if !dep_canonical.is_empty() {
            tracked_deps.insert(dep_canonical.clone());
        }
        if !declaration.canonical_source.is_empty() && declaration.canonical_source != dep_canonical
        {
            tracked_deps.insert(declaration.canonical_source.clone());
        }

        if !expanded {
            resolved_macros.push(ResolvedMacroMeta {
                macro_index,
                macro_kind: dep.macro_kind,
                type_name: dep.type_name.clone(),
                import_source: dep.import_source.clone(),
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
                declaration,
                native_props: Vec::new(),
                props: Vec::new(),
                emits: Vec::new(),
                slots: Vec::new(),
                jsdoc,
            });
            continue;
        }

        let mut resolution_deps = BTreeSet::new();
        if let Some(elements) = host.resolve_macro_elements(
            owner_canonical,
            &dep.import_source,
            dep_exported_name.as_ref(),
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
        ) {
            let declaration_source = host.read_source(declaration.canonical_source.as_str());
            let mut projected =
                project_macro_surfaces(declaration_source.as_deref(), dep.macro_kind, &elements);
            if dep.macro_kind == AnalyzedMacroKind::DefineSlots {
                if let Some(source_projected) = declaration_source.as_deref().and_then(|source| {
                    project_macro_surfaces_from_source_type_name(
                        source,
                        dep.macro_kind,
                        declaration.resolved_name.as_str(),
                    )
                }) {
                    if source_projected.slots.len() > projected.slots.len() {
                        projected.slots = source_projected.slots;
                    }
                }
            }
            if seen_registry_names.insert(dep.type_name.clone()) {
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
            let owner_source = host.read_source(owner_canonical);
            for resolved in &mac.resolved_local_types {
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
                            let declaration = resolve_local_type_declaration(
                                host,
                                owner_canonical,
                                resolved.name.as_str(),
                                resolved.span,
                            );
                            let jsdoc = host.resolve_jsdoc_block(
                                owner_canonical,
                                resolved.span,
                                true,
                                &mut tracked_deps,
                                &mut cache,
                                &mut visiting,
                            );
                            resolved_macros.push(ResolvedMacroMeta {
                                macro_index,
                                macro_kind: mac.kind,
                                type_name: resolved.name.clone(),
                                import_source: String::new(),
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

                if seen_registry_names.insert(resolved.name.clone()) {
                    resolved_type_registry.push(ResolvedTypeAnalysis {
                        name: resolved.name.clone(),
                        type_expr: resolved.type_expr.clone().unwrap_or_else(|| {
                            verter_analysis::type_expr_lower::parse_type_annotation(
                                &resolved.expanded,
                            )
                        }),
                        type_expansion: None,
                    });
                    resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                        name: resolved.name.clone(),
                        declaration: resolve_local_type_declaration(
                            host,
                            owner_canonical,
                            resolved.name.as_str(),
                            resolved.span,
                        ),
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
        cached_eval_inputs: eval_outputs.cached_eval_inputs,
        fact_versions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declaration_metadata::ResolvedExportTarget;
    use verter_analysis::type_eval::DeclarationId;
    use verter_analysis::types::{
        AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, ResolvedLocalType,
    };
    use verter_span::Span;

    #[derive(Clone)]
    struct TestSnapshot {
        imports: Vec<AnalyzedImport>,
        macros: Vec<AnalyzedMacro>,
        macro_type_deps: Vec<verter_analysis::types::MacroTypeDep>,
    }

    struct TestHost {
        source: String,
    }

    impl crate::DeclarationMetadataResolver for TestHost {
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
        type ImportedInputs = ();

        fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport] {
            &snapshot.imports
        }

        fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro] {
            &snapshot.macros
        }

        fn snapshot_macro_type_deps<'a>(
            &self,
            snapshot: &'a Self::Snapshot,
        ) -> &'a [verter_analysis::types::MacroTypeDep] {
            &snapshot.macro_type_deps
        }

        fn build_eval_outputs(
            &self,
            _owner_canonical: &str,
            _snapshot: &Self::Snapshot,
            _eval_context: Option<&Self::EvalContext>,
        ) -> ComponentMetaEvalOutputs<Self::ImportedInputs> {
            ComponentMetaEvalOutputs::default()
        }

        fn resolve_macro_elements(
            &self,
            _owner_canonical: &str,
            _import_source: &str,
            _exported_name: &str,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut crate::ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
        ) -> Option<ResolvedElements> {
            None
        }

        fn resolve_jsdoc_block(
            &self,
            _canonical_source: &str,
            _span: Span,
            _expanded: bool,
            _tracked_deps: &mut BTreeSet<String>,
            _cache: &mut crate::ExternalTypeBodyCache,
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
    fn local_resolved_macro_types_project_into_resolved_macro_surfaces() {
        let source =
            "type AccordionEmits = { 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }";
        let host = TestHost {
            source: source.to_string(),
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

        let resolved =
            resolve_component_meta_parts(&host, "/src/Accordion.vue", &snapshot, true, None);

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

        let resolved = resolve_component_meta_parts(&host, "/src/App.vue", &snapshot, true, None);

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
}

pub fn resolved_elements_to_type_expr_via_type_text(
    resolved: &ResolvedElements,
) -> verter_analysis::type_expr::TypeExpr {
    let properties = resolved
        .props
        .iter()
        .map(|prop| {
            let ty = prop
                .type_text
                .as_deref()
                .map(verter_analysis::type_expr_lower::parse_type_annotation)
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
                verter_analysis::types::ImportBindingKind::Namespace
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
