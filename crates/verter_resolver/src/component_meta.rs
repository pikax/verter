use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_analysis::component_meta::ResolvedTypeAnalysis;
use verter_analysis::type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};
use verter_analysis::types::{AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, MacroTypeDep};
use verter_core::utils::oxc::vue::resolve_type::ResolvedElements;

use crate::{
    project_macro_surfaces, resolve_local_type_declaration, resolve_type_declaration,
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

pub trait ComponentMetaResolverHost: DeclarationMetadataResolver {
    type Snapshot;
    type EvalContext;
    type ImportedInputs;

    fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport];
    fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro];
    fn snapshot_macro_type_deps<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [MacroTypeDep];

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
    ) -> ComponentMetaEvalOutputs<Self::ImportedInputs>;

    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut FxHashMap<(String, String), Option<ResolvedElements>>,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedElements>;

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut BTreeSet<String>,
        cache: &mut FxHashMap<(String, String), Option<ResolvedElements>>,
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
    let mut cache = FxHashMap::default();
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
        let declaration =
            resolve_type_declaration(host, &dep_canonical, dep_exported_name.as_ref());
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
            let projected = project_macro_surfaces(
                host.read_source(declaration.canonical_source.as_str())
                    .as_deref(),
                dep.macro_kind,
                &elements,
            );
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
        for mac in host.snapshot_macros(snapshot) {
            for resolved in &mac.resolved_local_types {
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

    TypeExpr::Object(ObjectExpr { properties })
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
