use crate::{
    DeclarationMetadataResolver, ImportedEvalOverflow, ImportedEvalSource, ImportedTypeAlias,
    ImportedTypeAliasResolveRequest,
};
use rustc_hash::FxHashSet;
use std::collections::BTreeSet;
use verter_analysis::type_eval::EvalEnv;
use verter_analysis::types::ImportBindingKind;
use verter_analysis::{AnalyzedBinding, AnalyzedImport, AnalyzedMacro, MacroTypeDep};

#[derive(Debug)]
pub struct ImportedEvalTraversalBudget {
    owner_canonical_id: String,
    max_type_roots: usize,
    overflow: Option<ImportedEvalOverflow>,
}

impl ImportedEvalTraversalBudget {
    pub fn new(owner_canonical_id: &str, max_type_roots: usize) -> Self {
        Self {
            owner_canonical_id: owner_canonical_id.to_string(),
            max_type_roots,
            overflow: None,
        }
    }

    pub fn is_exhausted(&self) -> bool {
        self.overflow.is_some()
    }

    pub fn overflow(&self) -> Option<ImportedEvalOverflow> {
        self.overflow.clone()
    }

    pub fn set_overflow(&mut self, message: impl Into<String>) {
        if self.overflow.is_none() {
            self.overflow = Some(ImportedEvalOverflow {
                message: message.into(),
            });
        }
    }

    pub fn try_enter_type_root(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
        current_root_count: usize,
    ) -> bool {
        if self.is_exhausted() {
            return false;
        }

        if current_root_count < self.max_type_roots {
            return true;
        }

        self.set_overflow(format!(
            "component-meta imported type merge budget exceeded (maxTypeRoots={}) while resolving '{}#{}' for '{}'",
            self.max_type_roots, canonical_id, exported_name, self.owner_canonical_id,
        ));
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedEvalBinding {
    pub local_name: String,
    pub imported_name: Option<String>,
    pub source: String,
    pub resolved_canonical_id: Option<String>,
    pub is_namespace: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ImportedEvalOwnerSnapshot<'a> {
    pub imports: &'a [AnalyzedImport],
    pub macros: &'a [AnalyzedMacro],
    pub bindings: &'a [AnalyzedBinding],
    pub macro_type_deps: &'a [MacroTypeDep],
}

#[allow(clippy::obfuscated_if_else)]
pub fn required_type_alias_names_for_import_binding(
    local_binding_name: &str,
    is_namespace: bool,
    required_import_names: &FxHashSet<String>,
) -> Vec<String> {
    if is_namespace {
        let prefix = format!("{local_binding_name}.");
        return required_import_names
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .cloned()
            .collect();
    }

    if required_import_names.contains(local_binding_name) {
        vec![local_binding_name.to_string()]
    } else {
        Vec::new()
    }
}

pub fn imported_member_name_for_type_alias(
    local_binding_name: &str,
    imported_name: Option<&str>,
    is_namespace: bool,
    required_alias_name: &str,
) -> Option<String> {
    if is_namespace {
        let prefix = format!("{local_binding_name}.");
        return required_alias_name
            .strip_prefix(&prefix)
            .map(str::to_string)
            .filter(|name| !name.is_empty());
    }

    Some(imported_name.unwrap_or(local_binding_name).to_string())
}

pub trait ImportedEvalSourceMergeResolver {
    fn record_eval_input_source(
        &mut self,
        canonical_id: &str,
        seen_sources: &mut FxHashSet<String>,
        inputs: &mut Vec<ImportedEvalSource>,
        canonical_dependencies: &mut BTreeSet<String>,
    );

    fn load_eval_source_for_merge(&mut self, canonical_id: &str) -> Option<String>;

    fn import_bindings_for_merge(
        &mut self,
        canonical_id: &str,
        eval_source: &str,
    ) -> Vec<ImportedEvalBinding>;

    fn resolve_import_binding_dependency(
        &self,
        owner_canonical_id: &str,
        binding: &ImportedEvalBinding,
    ) -> Option<String>;

    fn resolve_imported_type_declaration(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> crate::ResolvedTypeDeclaration;
}

pub trait ImportedEvalCollectorResolver:
    DeclarationMetadataResolver + ImportedEvalSourceMergeResolver
{
    fn resolve_imported_type_dependency(
        &self,
        owner_canonical_id: &str,
        import: &AnalyzedImport,
    ) -> Option<String>;

    fn collect_imported_type_alias(
        &mut self,
        request: ImportedTypeAliasResolveRequest,
        canonical_dependencies: &mut BTreeSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
    ) -> Option<ImportedTypeAlias>;
}

pub trait ImportedEvalOwnerResolver: ImportedEvalCollectorResolver {
    fn collect_required_owner_import_names(
        &self,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        owner_eval_source: &str,
        owner_env: &EvalEnv,
    ) -> FxHashSet<String>;

    fn track_direct_eval_dependencies(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        canonical_dependencies: &mut BTreeSet<String>,
    );
}

pub trait ImportedEvalOwnerContextResolver: ImportedEvalOwnerResolver {
    fn load_owner_eval_source(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    ) -> String;

    fn load_owner_eval_env(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        owner_eval_source: &str,
    ) -> EvalEnv;
}

#[allow(clippy::too_many_arguments)]
pub fn record_required_source_merge_inputs_recursive<R: ImportedEvalSourceMergeResolver>(
    resolver: &mut R,
    canonical_id: &str,
    exported_name: &str,
    seen_sources: &mut FxHashSet<String>,
    inputs: &mut Vec<ImportedEvalSource>,
    canonical_dependencies: &mut BTreeSet<String>,
    visited_type_roots: &mut FxHashSet<(String, String)>,
    budget: &mut ImportedEvalTraversalBudget,
) {
    if budget.is_exhausted() {
        return;
    }

    let visit_key = (canonical_id.to_string(), exported_name.to_string());
    if visited_type_roots.contains(&visit_key) {
        return;
    }
    if !budget.try_enter_type_root(canonical_id, exported_name, visited_type_roots.len()) {
        canonical_dependencies.insert(canonical_id.to_string());
        return;
    }
    visited_type_roots.insert(visit_key);

    resolver.record_eval_input_source(canonical_id, seen_sources, inputs, canonical_dependencies);

    let Some(eval_source) = resolver.load_eval_source_for_merge(canonical_id) else {
        return;
    };

    let alloc = oxc_allocator::Allocator::new();
    let required_import_names =
        verter_core::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
            exported_name,
            eval_source.as_str(),
            &alloc,
        );
    if required_import_names.is_empty() || budget.is_exhausted() {
        return;
    }

    let bindings = resolver.import_bindings_for_merge(canonical_id, &eval_source);
    for binding in &bindings {
        if budget.is_exhausted() {
            break;
        }

        let required_alias_names = required_type_alias_names_for_import_binding(
            binding.local_name.as_str(),
            binding.is_namespace,
            &required_import_names,
        );
        if required_alias_names.is_empty() {
            continue;
        }

        let Some(dep_canonical) = resolver.resolve_import_binding_dependency(canonical_id, binding)
        else {
            continue;
        };

        canonical_dependencies.insert(dep_canonical.clone());
        for required_alias_name in required_alias_names {
            if budget.is_exhausted() {
                break;
            }

            let Some(imported_name) = imported_member_name_for_type_alias(
                binding.local_name.as_str(),
                binding.imported_name.as_deref(),
                binding.is_namespace,
                &required_alias_name,
            ) else {
                continue;
            };

            let declaration =
                resolver.resolve_imported_type_declaration(&dep_canonical, &imported_name);
            let next_canonical = if declaration.canonical_source.is_empty() {
                dep_canonical.clone()
            } else {
                declaration.canonical_source
            };
            let next_exported_name = if declaration.resolved_name.is_empty() {
                imported_name
            } else {
                declaration.resolved_name
            };

            record_required_source_merge_inputs_recursive(
                resolver,
                &next_canonical,
                &next_exported_name,
                seen_sources,
                inputs,
                canonical_dependencies,
                visited_type_roots,
                budget,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn collect_imported_eval_inputs<R: ImportedEvalCollectorResolver>(
    resolver: &mut R,
    owner_canonical_id: &str,
    imports: &[AnalyzedImport],
    required_import_names: &FxHashSet<String>,
    seen_sources: &mut FxHashSet<String>,
    inputs: &mut Vec<ImportedEvalSource>,
    type_aliases: &mut Vec<ImportedTypeAlias>,
    canonical_dependencies: &mut BTreeSet<String>,
    visited_type_roots: &mut FxHashSet<(String, String)>,
    budget: &mut ImportedEvalTraversalBudget,
) {
    let mut alias_names = FxHashSet::default();

    for import in imports {
        if budget.is_exhausted() {
            break;
        }
        for binding in &import.bindings {
            if budget.is_exhausted() {
                break;
            }
            let required_alias_names = required_type_alias_names_for_import_binding(
                binding.name.as_str(),
                matches!(binding.kind, ImportBindingKind::Namespace),
                required_import_names,
            );
            if required_alias_names.is_empty() {
                continue;
            }

            let Some(dep_canonical) =
                resolver.resolve_imported_type_dependency(owner_canonical_id, import)
            else {
                continue;
            };

            canonical_dependencies.insert(dep_canonical.clone());
            for required_alias_name in required_alias_names {
                let Some(imported_name) = imported_member_name_for_type_alias(
                    binding.name.as_str(),
                    binding.imported_name.as_deref(),
                    matches!(binding.kind, ImportBindingKind::Namespace),
                    &required_alias_name,
                ) else {
                    continue;
                };

                let declaration =
                    resolver.resolve_imported_type_declaration(&dep_canonical, &imported_name);
                let source_canonical_id = if declaration.canonical_source.is_empty() {
                    dep_canonical.clone()
                } else {
                    declaration.canonical_source
                };
                let exported_name = if declaration.resolved_name.is_empty() {
                    imported_name.clone()
                } else {
                    declaration.resolved_name
                };

                if alias_names.insert(required_alias_name.clone()) {
                    if let Some(alias) = resolver.collect_imported_type_alias(
                        ImportedTypeAliasResolveRequest {
                            owner_canonical_id: owner_canonical_id.to_string(),
                            import_source: import.source.clone(),
                            local_name: required_alias_name.clone(),
                            imported_name,
                            source_canonical_id: source_canonical_id.clone(),
                            exported_name: exported_name.clone(),
                        },
                        canonical_dependencies,
                        budget,
                    ) {
                        if alias.requires_source_merge {
                            record_required_source_merge_inputs_recursive(
                                resolver,
                                &source_canonical_id,
                                &exported_name,
                                seen_sources,
                                inputs,
                                canonical_dependencies,
                                visited_type_roots,
                                budget,
                            );
                        }
                        type_aliases.push(alias);
                    }
                }
            }
        }
    }
}

pub fn build_imported_eval_inputs<R: ImportedEvalOwnerResolver>(
    resolver: &mut R,
    owner_canonical_id: &str,
    owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    owner_eval_source: &str,
    owner_env: &EvalEnv,
    additional_required_import_names: Option<&FxHashSet<String>>,
    budget: &mut ImportedEvalTraversalBudget,
) -> crate::ImportedEvalInputs {
    let mut seen = FxHashSet::default();
    let mut inputs = Vec::new();
    let mut type_aliases = Vec::new();
    let mut canonical_dependencies = BTreeSet::new();
    let mut visited_type_roots = FxHashSet::default();
    let mut required_import_names =
        resolver.collect_required_owner_import_names(owner_snapshot, owner_eval_source, owner_env);
    if let Some(additional) = additional_required_import_names {
        required_import_names.extend(additional.iter().cloned());
    }

    resolver.track_direct_eval_dependencies(
        owner_canonical_id,
        owner_snapshot,
        &mut canonical_dependencies,
    );

    collect_imported_eval_inputs(
        resolver,
        owner_canonical_id,
        owner_snapshot.imports,
        &required_import_names,
        &mut seen,
        &mut inputs,
        &mut type_aliases,
        &mut canonical_dependencies,
        &mut visited_type_roots,
        budget,
    );

    crate::ImportedEvalInputs {
        sources: inputs,
        type_aliases,
        canonical_dependencies,
        overflow: budget.overflow(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_imported_eval_inputs_with_owner_context<R: ImportedEvalOwnerContextResolver>(
    resolver: &mut R,
    owner_canonical_id: &str,
    owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    owner_eval_source: Option<&str>,
    owner_env_override: Option<&EvalEnv>,
    additional_required_import_names: Option<&FxHashSet<String>>,
    budget: &mut ImportedEvalTraversalBudget,
) -> crate::ImportedEvalInputs {
    let owner_eval_source = owner_eval_source
        .map(str::to_string)
        .unwrap_or_else(|| resolver.load_owner_eval_source(owner_canonical_id, owner_snapshot));
    let owner_env = owner_env_override.cloned().unwrap_or_else(|| {
        resolver.load_owner_eval_env(owner_canonical_id, owner_snapshot, &owner_eval_source)
    });

    build_imported_eval_inputs(
        resolver,
        owner_canonical_id,
        owner_snapshot,
        owner_eval_source.as_str(),
        &owner_env,
        additional_required_import_names,
        budget,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_imported_eval_inputs, build_imported_eval_inputs_with_owner_context,
        collect_imported_eval_inputs, imported_member_name_for_type_alias,
        record_required_source_merge_inputs_recursive,
        required_type_alias_names_for_import_binding, ImportedEvalBinding,
        ImportedEvalCollectorResolver, ImportedEvalOwnerContextResolver, ImportedEvalOwnerResolver,
        ImportedEvalOwnerSnapshot, ImportedEvalSourceMergeResolver, ImportedEvalTraversalBudget,
    };
    use crate::{
        DeclarationMetadataResolver, ImportedEvalSource, ImportedTypeAlias,
        ImportedTypeAliasResolveRequest, ResolvedDeclarationKind, ResolvedExportTarget,
        ResolvedTypeDeclaration,
    };
    use rustc_hash::FxHashSet;
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use verter_analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
    use verter_analysis::type_expr::{PrimitiveName, TypeExpr};
    use verter_analysis::types::ImportBindingKind;
    use verter_analysis::{AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, MacroTypeDep};
    use verter_span::Span;

    #[test]
    fn namespace_alias_names_only_keep_matching_members() {
        let required = FxHashSet::from_iter(
            [
                "ns.foo".to_string(),
                "ns.bar".to_string(),
                "other.baz".to_string(),
            ]
            .into_iter(),
        );

        let mut actual = required_type_alias_names_for_import_binding("ns", true, &required);
        actual.sort();

        assert_eq!(actual, vec!["ns.bar".to_string(), "ns.foo".to_string()]);
    }

    #[test]
    fn namespace_member_name_strips_prefix() {
        assert_eq!(
            imported_member_name_for_type_alias("ns", None, true, "ns.deep.Type"),
            Some("deep.Type".to_string())
        );
        assert_eq!(
            imported_member_name_for_type_alias("ns", None, true, "other.Type"),
            None
        );
    }

    #[test]
    fn traversal_budget_overflows_after_limit() {
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 1);

        assert!(budget.try_enter_type_root("/src/types.ts", "Props", 0));
        assert!(!budget.try_enter_type_root("/src/other.ts", "Other", 1));
        assert!(budget.is_exhausted());
        assert!(budget
            .overflow()
            .is_some_and(|overflow| overflow.message.contains("maxTypeRoots=1")));
    }

    #[derive(Default)]
    struct TestCollectorResolver {
        import_targets: BTreeMap<String, String>,
        export_targets: BTreeMap<(String, String), ResolvedExportTarget>,
        declarations: BTreeMap<(String, String), ResolvedTypeDeclaration>,
        source_texts: BTreeMap<String, String>,
        merge_bindings: BTreeMap<String, Vec<ImportedEvalBinding>>,
        prepared_requests: Vec<ImportedTypeAliasResolveRequest>,
        recorded_sources: Vec<String>,
        owner_eval_source: String,
        owner_env: EvalEnv,
        owner_eval_source_loads: Cell<usize>,
        owner_eval_env_loads: Cell<usize>,
    }

    impl DeclarationMetadataResolver for TestCollectorResolver {
        fn resolve_export_target(
            &self,
            dep_canonical: &str,
            requested_name: &str,
        ) -> Option<ResolvedExportTarget> {
            self.export_targets
                .get(&(dep_canonical.to_string(), requested_name.to_string()))
                .cloned()
        }

        fn get_export_span_follow_reexports(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<Span> {
            None
        }

        fn read_source(&self, _canonical_source: &str) -> Option<String> {
            None
        }

        fn type_declaration_id(
            &self,
            _canonical_source: &str,
            _resolved_name: &str,
        ) -> Option<verter_analysis::type_eval::DeclarationId> {
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

    impl ImportedEvalSourceMergeResolver for TestCollectorResolver {
        fn record_eval_input_source(
            &mut self,
            canonical_id: &str,
            seen_sources: &mut FxHashSet<String>,
            inputs: &mut Vec<ImportedEvalSource>,
            canonical_dependencies: &mut BTreeSet<String>,
        ) {
            canonical_dependencies.insert(canonical_id.to_string());
            if !seen_sources.insert(canonical_id.to_string()) {
                return;
            }
            self.recorded_sources.push(canonical_id.to_string());
            if let Some(source) = self.source_texts.get(canonical_id) {
                inputs.push(ImportedEvalSource {
                    canonical_id: canonical_id.to_string(),
                    source: Arc::from(source.as_str()),
                });
            }
        }

        fn load_eval_source_for_merge(&mut self, canonical_id: &str) -> Option<String> {
            self.source_texts.get(canonical_id).cloned()
        }

        fn import_bindings_for_merge(
            &mut self,
            canonical_id: &str,
            _eval_source: &str,
        ) -> Vec<ImportedEvalBinding> {
            self.merge_bindings
                .get(canonical_id)
                .cloned()
                .unwrap_or_default()
        }

        fn resolve_import_binding_dependency(
            &self,
            owner_canonical_id: &str,
            binding: &ImportedEvalBinding,
        ) -> Option<String> {
            binding.resolved_canonical_id.clone().or_else(|| {
                self.import_targets
                    .get(&format!("{owner_canonical_id}:{}", binding.source))
                    .cloned()
            })
        }

        fn resolve_imported_type_declaration(
            &self,
            dep_canonical: &str,
            imported_name: &str,
        ) -> ResolvedTypeDeclaration {
            self.declarations
                .get(&(dep_canonical.to_string(), imported_name.to_string()))
                .cloned()
                .unwrap_or_else(|| ResolvedTypeDeclaration {
                    requested_name: imported_name.to_string(),
                    declaration_id: None,
                    resolved_name: imported_name.to_string(),
                    canonical_source: dep_canonical.to_string(),
                    span: Span::new(0, 0),
                    kind: ResolvedDeclarationKind::Unknown,
                    text: None,
                })
        }
    }

    impl ImportedEvalCollectorResolver for TestCollectorResolver {
        fn resolve_imported_type_dependency(
            &self,
            owner_canonical_id: &str,
            import: &AnalyzedImport,
        ) -> Option<String> {
            self.import_targets
                .get(&format!("{owner_canonical_id}:{}", import.source))
                .cloned()
                .or_else(|| self.import_targets.get(&import.source).cloned())
        }

        fn collect_imported_type_alias(
            &mut self,
            request: ImportedTypeAliasResolveRequest,
            canonical_dependencies: &mut BTreeSet<String>,
            _budget: &mut ImportedEvalTraversalBudget,
        ) -> Option<ImportedTypeAlias> {
            canonical_dependencies.insert(request.source_canonical_id.clone());
            self.prepared_requests.push(request.clone());
            Some(ImportedTypeAlias {
                local_name: request.local_name,
                source_canonical_id: request.source_canonical_id,
                exported_name: request.exported_name,
                decl: TypeDeclInfo {
                    name: "Alias".to_string(),
                    declaration_id: 0,
                    kind: TypeDeclKind::Alias,
                    type_parameters: Vec::new(),
                    body: TypeExpr::Primitive(PrimitiveName::String),
                },
                requires_source_merge: true,
            })
        }
    }

    impl ImportedEvalOwnerResolver for TestCollectorResolver {
        fn collect_required_owner_import_names(
            &self,
            _owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
            _owner_eval_source: &str,
            _owner_env: &EvalEnv,
        ) -> FxHashSet<String> {
            FxHashSet::from_iter(["Types.User".to_string()].into_iter())
        }

        fn track_direct_eval_dependencies(
            &self,
            owner_canonical_id: &str,
            owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
            canonical_dependencies: &mut BTreeSet<String>,
        ) {
            canonical_dependencies.insert(owner_canonical_id.to_string());
            for dep in owner_snapshot.macro_type_deps {
                canonical_dependencies.insert(dep.import_source.clone());
            }
        }
    }

    impl ImportedEvalOwnerContextResolver for TestCollectorResolver {
        fn load_owner_eval_source(
            &self,
            _owner_canonical_id: &str,
            _owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        ) -> String {
            self.owner_eval_source_loads
                .set(self.owner_eval_source_loads.get() + 1);
            self.owner_eval_source.clone()
        }

        fn load_owner_eval_env(
            &self,
            _owner_canonical_id: &str,
            _owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
            _owner_eval_source: &str,
        ) -> EvalEnv {
            self.owner_eval_env_loads
                .set(self.owner_eval_env_loads.get() + 1);
            self.owner_env.clone()
        }
    }

    fn analyzed_import(
        source: &str,
        bindings: Vec<AnalyzedImportBinding>,
        is_type_only: bool,
    ) -> AnalyzedImport {
        AnalyzedImport {
            source: source.to_string(),
            is_type_only,
            bindings,
            span: Span::new(0, 0),
            resolved_canonical_id: None,
        }
    }

    fn binding(
        name: &str,
        kind: ImportBindingKind,
        imported_name: Option<&str>,
        is_type_only: bool,
    ) -> AnalyzedImportBinding {
        AnalyzedImportBinding {
            name: name.to_string(),
            kind,
            imported_name: imported_name.map(str::to_string),
            is_type_only,
            vue_api: None,
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn collect_imported_eval_inputs_routes_namespace_aliases_through_resolver() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![binding("Types", ImportBindingKind::Namespace, None, true)],
            true,
        )];
        let required_import_names = FxHashSet::from_iter(["Types.User".to_string()].into_iter());
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.export_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ResolvedExportTarget {
                source_canonical_id: Some("/src/real.ts".to_string()),
                source_name: "User".to_string(),
            },
        );
        resolver.declarations.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ResolvedTypeDeclaration {
                requested_name: "User".to_string(),
                declaration_id: None,
                resolved_name: "User".to_string(),
                canonical_source: "/src/real.ts".to_string(),
                span: Span::new(0, 0),
                kind: ResolvedDeclarationKind::Interface,
                text: None,
            },
        );
        resolver.source_texts.insert(
            "/src/real.ts".to_string(),
            "export interface User {}".to_string(),
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut type_aliases = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);

        collect_imported_eval_inputs(
            &mut resolver,
            "/src/App.vue",
            &imports,
            &required_import_names,
            &mut seen_sources,
            &mut inputs,
            &mut type_aliases,
            &mut canonical_dependencies,
            &mut visited_type_roots,
            &mut budget,
        );

        assert_eq!(resolver.prepared_requests.len(), 1);
        assert_eq!(resolver.prepared_requests[0].local_name, "Types.User");
        assert_eq!(resolver.prepared_requests[0].imported_name, "User");
        assert_eq!(
            resolver.prepared_requests[0].source_canonical_id,
            "/src/real.ts"
        );
        assert_eq!(resolver.recorded_sources, vec!["/src/real.ts".to_string()]);
        assert_eq!(type_aliases.len(), 1);
        assert_eq!(inputs.len(), 1);
        assert!(canonical_dependencies.contains("/src/dep.ts"));
        assert!(canonical_dependencies.contains("/src/real.ts"));
    }

    #[test]
    fn build_imported_eval_inputs_routes_owner_requirements_through_resolver() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![binding("Types", ImportBindingKind::Namespace, None, true)],
            true,
        )];
        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: &imports,
            macros: &[] as &[AnalyzedMacro],
            bindings: &[],
            macro_type_deps: &[] as &[MacroTypeDep],
        };
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.declarations.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ResolvedTypeDeclaration {
                requested_name: "User".to_string(),
                declaration_id: None,
                resolved_name: "User".to_string(),
                canonical_source: "/src/real.ts".to_string(),
                span: Span::new(0, 0),
                kind: ResolvedDeclarationKind::Interface,
                text: None,
            },
        );
        resolver.source_texts.insert(
            "/src/real.ts".to_string(),
            "export interface User {}".to_string(),
        );

        let owner_env = EvalEnv::new();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);

        let inputs = build_imported_eval_inputs(
            &mut resolver,
            "/src/App.vue",
            &owner_snapshot,
            "defineProps<Types.User>()",
            &owner_env,
            None,
            &mut budget,
        );

        assert_eq!(inputs.type_aliases.len(), 1);
        assert_eq!(inputs.sources.len(), 1);
        assert!(inputs.canonical_dependencies.contains("/src/App.vue"));
        assert!(inputs.canonical_dependencies.contains("/src/dep.ts"));
        assert!(inputs.canonical_dependencies.contains("/src/real.ts"));
    }

    #[test]
    fn build_imported_eval_inputs_with_owner_context_loads_missing_source_and_env() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![binding("Types", ImportBindingKind::Namespace, None, true)],
            true,
        )];
        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: &imports,
            macros: &[] as &[AnalyzedMacro],
            bindings: &[],
            macro_type_deps: &[] as &[MacroTypeDep],
        };
        let mut resolver = TestCollectorResolver::default();
        resolver.owner_eval_source = "defineProps<Types.User>()".to_string();
        resolver.owner_env = EvalEnv::new();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.declarations.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ResolvedTypeDeclaration {
                requested_name: "User".to_string(),
                declaration_id: None,
                resolved_name: "User".to_string(),
                canonical_source: "/src/real.ts".to_string(),
                span: Span::new(0, 0),
                kind: ResolvedDeclarationKind::Interface,
                text: None,
            },
        );
        resolver.source_texts.insert(
            "/src/real.ts".to_string(),
            "export interface User {}".to_string(),
        );
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);

        let inputs = build_imported_eval_inputs_with_owner_context(
            &mut resolver,
            "/src/App.vue",
            &owner_snapshot,
            None,
            None,
            None,
            &mut budget,
        );

        assert_eq!(resolver.owner_eval_source_loads.get(), 1);
        assert_eq!(resolver.owner_eval_env_loads.get(), 1);
        assert_eq!(inputs.type_aliases.len(), 1);
        assert_eq!(inputs.sources.len(), 1);
    }

    #[test]
    fn source_merge_recurses_through_local_alias_chain() {
        let mut resolver = TestCollectorResolver::default();
        resolver.source_texts.insert(
            "/src/root.ts".to_string(),
            r#"import type { User as ImportedUser } from "./dep";
type Local = ImportedUser;
export interface Props extends Local {}"#
                .to_string(),
        );
        resolver.source_texts.insert(
            "/src/real.ts".to_string(),
            "export interface User {}".to_string(),
        );
        resolver.merge_bindings.insert(
            "/src/root.ts".to_string(),
            vec![ImportedEvalBinding {
                local_name: "ImportedUser".to_string(),
                imported_name: Some("User".to_string()),
                source: "./dep".to_string(),
                resolved_canonical_id: Some("/src/dep.ts".to_string()),
                is_namespace: false,
            }],
        );
        resolver.declarations.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ResolvedTypeDeclaration {
                requested_name: "User".to_string(),
                declaration_id: None,
                resolved_name: "User".to_string(),
                canonical_source: "/src/real.ts".to_string(),
                span: Span::new(0, 0),
                kind: ResolvedDeclarationKind::Interface,
                text: None,
            },
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);

        record_required_source_merge_inputs_recursive(
            &mut resolver,
            "/src/root.ts",
            "Props",
            &mut seen_sources,
            &mut inputs,
            &mut canonical_dependencies,
            &mut visited_type_roots,
            &mut budget,
        );

        assert_eq!(
            resolver.recorded_sources,
            vec!["/src/root.ts".to_string(), "/src/real.ts".to_string()]
        );
        assert_eq!(inputs.len(), 2);
        assert!(canonical_dependencies.contains("/src/root.ts"));
        assert!(canonical_dependencies.contains("/src/dep.ts"));
        assert!(canonical_dependencies.contains("/src/real.ts"));
    }
}
