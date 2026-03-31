use crate::{
    CollectedImportedTypeAlias, DeclarationMetadataResolver, ImportedEvalOverflow,
    ImportedEvalSource, ImportedEvalStats, ImportedTypeAlias, ImportedTypeAliasResolveRequest,
};
use rustc_hash::FxHashSet;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use verter_semantic::analysis::type_eval::EvalEnv;
use verter_semantic::analysis::types::ImportBindingKind;
use verter_semantic::analysis::{AnalyzedBinding, AnalyzedImport, AnalyzedMacro, MacroTypeDep};

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
            "component-meta external type resolution step budget exceeded (maxTypeRoots={}) while resolving '{}#{}' for '{}'",
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

    fn load_eval_source_for_merge(&mut self, canonical_id: &str) -> Option<std::sync::Arc<str>>;

    fn required_import_names_for_exported_type(
        &self,
        canonical_id: &str,
        exported_name: &str,
        eval_source: &str,
    ) -> FxHashSet<String> {
        let _ = canonical_id;
        let alloc = oxc_allocator::Allocator::new();
        verter_compiler::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
            exported_name,
            eval_source,
            &alloc,
        )
    }

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

    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        let declaration = self.resolve_imported_type_declaration(dep_canonical, imported_name);
        let mut canonical = if declaration.canonical_source.is_empty() {
            dep_canonical.to_string()
        } else {
            declaration.canonical_source
        };
        let mut exported_name = if declaration.resolved_name.is_empty() {
            imported_name.to_string()
        } else {
            declaration.resolved_name
        };

        let declaration = self.resolve_imported_type_declaration(&canonical, &exported_name);
        if !declaration.canonical_source.is_empty() {
            canonical = declaration.canonical_source;
        }
        if !declaration.resolved_name.is_empty() {
            exported_name = declaration.resolved_name;
        }

        (canonical, exported_name)
    }
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
    ) -> Option<CollectedImportedTypeAlias>;

    fn prepare_imported_type_alias_failure_count(&self) -> u64 {
        0
    }
}

#[derive(Debug, Clone)]
struct PendingImportedTypeAlias {
    local_name: String,
    source_canonical_id: String,
    exported_name: String,
    merge_root_canonical: String,
    merge_root_exported: String,
}

pub trait ImportedEvalOwnerResolver: ImportedEvalCollectorResolver {
    fn collect_required_owner_import_names(
        &self,
        owner_canonical_id: &str,
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

fn normalized_imported_type_root<R: ImportedEvalSourceMergeResolver>(
    resolver: &R,
    dep_canonical: &str,
    imported_name: &str,
    stats: &mut ImportedEvalStats,
) -> (String, String) {
    stats.normalized_imported_type_root_calls += 1;
    resolver.resolve_imported_type_root(dep_canonical, imported_name)
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn record_required_source_merge_inputs_recursive<R: ImportedEvalSourceMergeResolver>(
    resolver: &mut R,
    canonical_id: &str,
    exported_name: &str,
    seen_sources: &mut FxHashSet<String>,
    inputs: &mut Vec<ImportedEvalSource>,
    canonical_dependencies: &mut BTreeSet<String>,
    visited_type_roots: &mut FxHashSet<(String, String)>,
    budget: &mut ImportedEvalTraversalBudget,
    stats: &mut ImportedEvalStats,
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

    let required_import_names = resolver.required_import_names_for_exported_type(
        canonical_id,
        exported_name,
        eval_source.as_ref(),
    );
    if required_import_names.is_empty() || budget.is_exhausted() {
        return;
    }

    let bindings = resolver.import_bindings_for_merge(canonical_id, eval_source.as_ref());
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

            let (next_canonical, next_exported_name) =
                normalized_imported_type_root(resolver, &dep_canonical, &imported_name, stats);

            record_required_source_merge_inputs_recursive(
                resolver,
                &next_canonical,
                &next_exported_name,
                seen_sources,
                inputs,
                canonical_dependencies,
                visited_type_roots,
                budget,
                stats,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_imported_eval_inputs_lazy<R: ImportedEvalCollectorResolver>(
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
    stats: &mut ImportedEvalStats,
) {
    let mut alias_names = FxHashSet::default();
    let mut pending_aliases = Vec::new();
    let mut queued_roots = FxHashSet::default();
    let mut reached_roots = BTreeMap::new();
    let mut reached_merge_roots = Vec::new();
    let mut reached_merge_root_set = FxHashSet::default();
    let mut worklist = VecDeque::new();

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

                let merge_root =
                    normalized_imported_type_root(resolver, &dep_canonical, &imported_name, stats);
                canonical_dependencies.insert(merge_root.0.clone());

                if alias_names.insert(required_alias_name.clone()) {
                    pending_aliases.push(PendingImportedTypeAlias {
                        local_name: required_alias_name,
                        source_canonical_id: dep_canonical.clone(),
                        exported_name: imported_name.clone(),
                        merge_root_canonical: merge_root.0.clone(),
                        merge_root_exported: merge_root.1.clone(),
                    });
                }

                if queued_roots.insert((merge_root.0.clone(), merge_root.1.clone())) {
                    stats.worklist_seed_count += 1;
                    worklist.push_back(merge_root);
                }
            }
        }
    }

    while let Some((root_canonical, root_exported)) = worklist.pop_front() {
        if budget.is_exhausted() {
            break;
        }
        if reached_roots.contains_key(&(root_canonical.clone(), root_exported.clone())) {
            continue;
        }
        if !budget.try_enter_type_root(&root_canonical, &root_exported, reached_roots.len()) {
            canonical_dependencies.insert(root_canonical.clone());
            continue;
        }

        let Some(collected) = resolver.collect_imported_type_alias(
            ImportedTypeAliasResolveRequest {
                owner_canonical_id: owner_canonical_id.to_string(),
                import_source: String::new(),
                local_name: root_exported.clone(),
                imported_name: root_exported.clone(),
                source_canonical_id: root_canonical.clone(),
                exported_name: root_exported.clone(),
            },
            canonical_dependencies,
            budget,
        ) else {
            stats.prepare_imported_type_alias_failures += 1;
            continue;
        };

        stats.worklist_resolved_count += 1;
        let reached_root = (
            collected.alias.merge_root_canonical.clone(),
            collected.alias.merge_root_exported.clone(),
        );
        reached_roots.insert(reached_root.clone(), collected.alias.requires_source_merge);

        if collected.alias.requires_source_merge
            && reached_merge_root_set.insert(reached_root.clone())
        {
            stats.reached_merge_roots_count += 1;
            reached_merge_roots.push(reached_root);
        }

        for dependency in collected.symbol_dependencies {
            if dependency.canonical_id == collected.alias.merge_root_canonical {
                continue;
            }
            canonical_dependencies.insert(dependency.canonical_id);
        }
    }

    for (merge_root_canonical, merge_root_exported) in reached_merge_roots {
        if budget.is_exhausted() {
            break;
        }
        record_required_source_merge_inputs_recursive(
            resolver,
            &merge_root_canonical,
            &merge_root_exported,
            seen_sources,
            inputs,
            canonical_dependencies,
            visited_type_roots,
            budget,
            stats,
        );
    }

    for pending_alias in pending_aliases {
        let Some(requires_source_merge) = reached_roots.get(&(
            pending_alias.merge_root_canonical.clone(),
            pending_alias.merge_root_exported.clone(),
        )) else {
            stats.dropped_unreached_aliases += 1;
            continue;
        };
        type_aliases.push(ImportedTypeAlias {
            local_name: pending_alias.local_name,
            source_canonical_id: pending_alias.source_canonical_id,
            exported_name: pending_alias.exported_name,
            requires_source_merge: *requires_source_merge,
            merge_root_canonical: pending_alias.merge_root_canonical,
            merge_root_exported: pending_alias.merge_root_exported,
        });
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
    stats: &mut ImportedEvalStats,
) {
    collect_imported_eval_inputs_lazy(
        resolver,
        owner_canonical_id,
        imports,
        required_import_names,
        seen_sources,
        inputs,
        type_aliases,
        canonical_dependencies,
        visited_type_roots,
        budget,
        stats,
    );
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
    let mut stats = ImportedEvalStats::default();
    let mut required_import_names = resolver.collect_required_owner_import_names(
        owner_canonical_id,
        owner_snapshot,
        owner_eval_source,
        owner_env,
    );
    required_import_names.extend(
        owner_snapshot
            .macro_type_deps
            .iter()
            .map(|dep| dep.type_name.clone()),
    );
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
        &mut stats,
    );

    stats.imported_sources_count = inputs.len() as u64;
    stats.prepare_imported_type_alias_failures =
        resolver.prepare_imported_type_alias_failure_count();

    crate::ImportedEvalInputs {
        sources: inputs,
        type_aliases,
        canonical_dependencies,
        overflow: budget.overflow(),
        stats,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
    let owned_owner_env;
    let owner_env = if let Some(owner_env) = owner_env_override {
        owner_env
    } else {
        owned_owner_env =
            resolver.load_owner_eval_env(owner_canonical_id, owner_snapshot, &owner_eval_source);
        &owned_owner_env
    };

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
        CollectedImportedTypeAlias, DeclarationMetadataResolver, ImportedEvalSource,
        ImportedEvalStats, ImportedSymbolDependency, ImportedTypeAlias,
        ImportedTypeAliasResolveRequest, ResolvedDeclarationKind, ResolvedExportTarget,
        ResolvedTypeDeclaration,
    };
    use rustc_hash::FxHashSet;
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use verter_semantic::analysis::type_eval::EvalEnv;
    use verter_semantic::analysis::types::ImportBindingKind;
    use verter_semantic::analysis::{
        AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, MacroTypeDep,
    };
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
        root_targets: BTreeMap<(String, String), (String, String)>,
        source_texts: BTreeMap<String, String>,
        merge_bindings: BTreeMap<String, Vec<ImportedEvalBinding>>,
        collected_aliases: BTreeMap<(String, String), CollectedImportedTypeAlias>,
        failed_aliases: FxHashSet<(String, String)>,
        prepared_requests: Vec<ImportedTypeAliasResolveRequest>,
        recorded_sources: Vec<String>,
        owner_eval_source: String,
        owner_env: EvalEnv,
        owner_eval_source_loads: Cell<usize>,
        owner_eval_env_loads: Cell<usize>,
        imported_root_lookups: Cell<usize>,
        declaration_lookups: Cell<usize>,
        prepare_failure_count: Cell<u64>,
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
        ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
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
                let _ = source;
                inputs.push(ImportedEvalSource {
                    canonical_id: canonical_id.to_string(),
                });
            }
        }

        fn load_eval_source_for_merge(
            &mut self,
            canonical_id: &str,
        ) -> Option<std::sync::Arc<str>> {
            self.source_texts
                .get(canonical_id)
                .map(|source| Arc::<str>::from(source.as_str()))
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
            self.declaration_lookups
                .set(self.declaration_lookups.get() + 1);
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

        fn resolve_imported_type_root(
            &self,
            dep_canonical: &str,
            imported_name: &str,
        ) -> (String, String) {
            self.imported_root_lookups
                .set(self.imported_root_lookups.get() + 1);
            self.root_targets
                .get(&(dep_canonical.to_string(), imported_name.to_string()))
                .cloned()
                .unwrap_or_else(|| (dep_canonical.to_string(), imported_name.to_string()))
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
        ) -> Option<CollectedImportedTypeAlias> {
            canonical_dependencies.insert(request.source_canonical_id.clone());
            self.prepared_requests.push(request.clone());
            if self.failed_aliases.contains(&(
                request.source_canonical_id.clone(),
                request.exported_name.clone(),
            )) {
                self.prepare_failure_count
                    .set(self.prepare_failure_count.get() + 1);
                return None;
            }
            self.collected_aliases
                .get(&(
                    request.source_canonical_id.clone(),
                    request.exported_name.clone(),
                ))
                .cloned()
                .or_else(|| {
                    Some(CollectedImportedTypeAlias {
                        alias: ImportedTypeAlias {
                            local_name: request.local_name,
                            source_canonical_id: request.source_canonical_id.clone(),
                            exported_name: request.exported_name.clone(),
                            requires_source_merge: true,
                            merge_root_canonical: request.source_canonical_id,
                            merge_root_exported: request.exported_name,
                        },
                        symbol_dependencies: Vec::new(),
                    })
                })
        }

        fn prepare_imported_type_alias_failure_count(&self) -> u64 {
            self.prepare_failure_count.get()
        }
    }

    impl ImportedEvalOwnerResolver for TestCollectorResolver {
        fn collect_required_owner_import_names(
            &self,
            _owner_canonical_id: &str,
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

    fn collected_alias(
        local_name: &str,
        source_canonical_id: &str,
        exported_name: &str,
        merge_root_canonical: &str,
        merge_root_exported: &str,
        requires_source_merge: bool,
        symbol_dependencies: Vec<ImportedSymbolDependency>,
    ) -> CollectedImportedTypeAlias {
        CollectedImportedTypeAlias {
            alias: ImportedTypeAlias {
                local_name: local_name.to_string(),
                source_canonical_id: source_canonical_id.to_string(),
                exported_name: exported_name.to_string(),
                requires_source_merge,
                merge_root_canonical: merge_root_canonical.to_string(),
                merge_root_exported: merge_root_exported.to_string(),
            },
            symbol_dependencies,
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
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/real.ts".to_string(), "User".to_string()),
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
        let mut stats = ImportedEvalStats::default();

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
            &mut stats,
        );

        assert_eq!(resolver.prepared_requests.len(), 1);
        assert_eq!(resolver.prepared_requests[0].local_name, "User");
        assert_eq!(resolver.prepared_requests[0].imported_name, "User");
        assert_eq!(
            resolver.prepared_requests[0].source_canonical_id,
            "/src/real.ts"
        );
        assert_eq!(resolver.prepared_requests[0].exported_name, "User");
        assert_eq!(resolver.recorded_sources, vec!["/src/real.ts".to_string()]);
        assert_eq!(type_aliases.len(), 1);
        assert_eq!(type_aliases[0].local_name, "Types.User");
        assert_eq!(type_aliases[0].source_canonical_id, "/src/dep.ts");
        assert_eq!(type_aliases[0].merge_root_canonical, "/src/real.ts");
        assert_eq!(type_aliases[0].merge_root_exported, "User");
        assert_eq!(inputs.len(), 1);
        assert!(canonical_dependencies.contains("/src/dep.ts"));
        assert!(canonical_dependencies.contains("/src/real.ts"));
    }

    #[test]
    fn collect_imported_eval_inputs_keeps_discovered_symbol_dependencies_attached_lazily() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![
                binding("Foo", ImportBindingKind::Named, Some("Foo"), true),
                binding("Unused", ImportBindingKind::Named, Some("Unused"), true),
            ],
            true,
        )];
        let required_import_names = FxHashSet::from_iter(["Foo".to_string()].into_iter());
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.collected_aliases.insert(
            ("/src/dep.ts".to_string(), "Foo".to_string()),
            collected_alias(
                "Foo",
                "/src/dep.ts",
                "Foo",
                "/src/dep.ts",
                "Foo",
                false,
                vec![ImportedSymbolDependency {
                    local_name: "Bar".to_string(),
                    canonical_id: "/src/bar.ts".to_string(),
                    exported_name: "Bar".to_string(),
                }],
            ),
        );
        resolver.collected_aliases.insert(
            ("/src/bar.ts".to_string(), "Bar".to_string()),
            collected_alias(
                "Bar",
                "/src/bar.ts",
                "Bar",
                "/src/bar.ts",
                "Bar",
                false,
                Vec::new(),
            ),
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut type_aliases = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let mut stats = ImportedEvalStats::default();

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
            &mut stats,
        );

        let prepared: Vec<_> = resolver
            .prepared_requests
            .iter()
            .map(|request| format!("{}#{}", request.source_canonical_id, request.exported_name))
            .collect();
        assert_eq!(
            prepared,
            vec!["/src/dep.ts#Foo".to_string()],
            "discovered symbol dependencies should stay attached to the reached parent root during lazy collection"
        );
        assert_eq!(type_aliases.len(), 1);
        assert_eq!(type_aliases[0].local_name, "Foo");
        assert!(
            !type_aliases
                .iter()
                .any(|alias| alias.local_name == "Unused"),
            "unused owner imports must never become imported eval aliases"
        );
        assert!(
            canonical_dependencies.contains("/src/bar.ts"),
            "discovered symbol dependencies should be tracked as canonical dependencies"
        );
        assert!(
            inputs.is_empty(),
            "lazy dependency expansion should not collect merge sources when no reached symbol needs source merge"
        );
        assert_eq!(
            stats.worklist_enqueued_from_symbol_deps_count, 0,
            "symbol dependencies should not become separate lazy worklist roots"
        );
    }

    #[test]
    fn collect_imported_eval_inputs_keeps_symbol_dependencies_attached_to_their_parent_root() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![binding("Foo", ImportBindingKind::Named, Some("Foo"), true)],
            true,
        )];
        let required_import_names = FxHashSet::from_iter(["Foo".to_string()].into_iter());
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.collected_aliases.insert(
            ("/src/dep.ts".to_string(), "Foo".to_string()),
            collected_alias(
                "Foo",
                "/src/dep.ts",
                "Foo",
                "/src/dep.ts",
                "Foo",
                false,
                vec![ImportedSymbolDependency {
                    local_name: "Bar".to_string(),
                    canonical_id: "/src/bar.ts".to_string(),
                    exported_name: "Bar".to_string(),
                }],
            ),
        );
        resolver.collected_aliases.insert(
            ("/src/bar.ts".to_string(), "Bar".to_string()),
            collected_alias(
                "Bar",
                "/src/bar.ts",
                "Bar",
                "/src/bar.ts",
                "Bar",
                false,
                Vec::new(),
            ),
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut type_aliases = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let mut stats = ImportedEvalStats::default();

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
            &mut stats,
        );

        let prepared: Vec<_> = resolver
            .prepared_requests
            .iter()
            .map(|request| format!("{}#{}", request.source_canonical_id, request.exported_name))
            .collect();
        assert_eq!(
            prepared,
            vec!["/src/dep.ts#Foo".to_string()],
            "symbol dependencies should stay attached to the reached root instead of becoming sibling worklist roots"
        );
        assert_eq!(
            stats.worklist_resolved_count, 1,
            "lazy collection should only resolve the owner-reached root here"
        );
        assert_eq!(
            stats.worklist_enqueued_from_symbol_deps_count, 0,
            "symbol dependencies should not seed a second deepening frontier during collection"
        );
        assert!(
            canonical_dependencies.contains("/src/bar.ts"),
            "the parent alias should still track its child dependency for invalidation"
        );
        assert_eq!(type_aliases.len(), 1);
        assert_eq!(type_aliases[0].local_name, "Foo");
    }

    #[test]
    fn collect_imported_eval_inputs_skips_same_file_local_support_symbols_in_worklist() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![binding("Foo", ImportBindingKind::Named, Some("Foo"), true)],
            true,
        )];
        let required_import_names = FxHashSet::from_iter(["Foo".to_string()].into_iter());
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.collected_aliases.insert(
            ("/src/dep.ts".to_string(), "Foo".to_string()),
            collected_alias(
                "Foo",
                "/src/dep.ts",
                "Foo",
                "/src/dep.ts",
                "Foo",
                false,
                vec![
                    ImportedSymbolDependency {
                        local_name: "LocalHelper".to_string(),
                        canonical_id: "/src/dep.ts".to_string(),
                        exported_name: "LocalHelper".to_string(),
                    },
                    ImportedSymbolDependency {
                        local_name: "Bar".to_string(),
                        canonical_id: "/src/bar.ts".to_string(),
                        exported_name: "Bar".to_string(),
                    },
                ],
            ),
        );
        resolver.collected_aliases.insert(
            ("/src/bar.ts".to_string(), "Bar".to_string()),
            collected_alias(
                "Bar",
                "/src/bar.ts",
                "Bar",
                "/src/bar.ts",
                "Bar",
                false,
                Vec::new(),
            ),
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut type_aliases = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let mut stats = ImportedEvalStats::default();

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
            &mut stats,
        );

        let prepared: Vec<_> = resolver
            .prepared_requests
            .iter()
            .map(|request| format!("{}#{}", request.source_canonical_id, request.exported_name))
            .collect();
        assert_eq!(
            prepared,
            vec!["/src/dep.ts#Foo".to_string()],
            "same-file support symbols should stay attached to the reached alias and must not become separate worklist roots"
        );
        assert!(
            !prepared.contains(&"/src/dep.ts#LocalHelper".to_string()),
            "same-file helper symbols should not be enqueued as cross-file dependency work items"
        );
    }

    #[test]
    fn build_imported_eval_inputs_reports_lazy_worklist_stats() {
        let imports = vec![analyzed_import(
            "./dep",
            vec![binding("Foo", ImportBindingKind::Named, Some("Foo"), true)],
            true,
        )];
        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: &imports,
            macros: &[],
            bindings: &[],
            macro_type_deps: &[],
        };
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "Foo".to_string()),
            ("/src/real.ts".to_string(), "Foo".to_string()),
        );
        resolver.collected_aliases.insert(
            ("/src/real.ts".to_string(), "Foo".to_string()),
            collected_alias(
                "Foo",
                "/src/real.ts",
                "Foo",
                "/src/real.ts",
                "Foo",
                true,
                vec![ImportedSymbolDependency {
                    local_name: "Bar".to_string(),
                    canonical_id: "/src/bar.ts".to_string(),
                    exported_name: "Bar".to_string(),
                }],
            ),
        );
        resolver.collected_aliases.insert(
            ("/src/bar.ts".to_string(), "Bar".to_string()),
            collected_alias(
                "Bar",
                "/src/bar.ts",
                "Bar",
                "/src/bar.ts",
                "Bar",
                false,
                Vec::new(),
            ),
        );
        resolver.source_texts.insert(
            "/src/real.ts".to_string(),
            "export interface Foo { value: string }".to_string(),
        );

        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let inputs = build_imported_eval_inputs(
            &mut resolver,
            "/src/App.vue",
            &owner_snapshot,
            "",
            &EvalEnv::default(),
            Some(&FxHashSet::from_iter(["Foo".to_string()].into_iter())),
            &mut budget,
        );

        assert_eq!(
            inputs.stats.worklist_seed_count, 1,
            "only owner-reachable imported bindings should seed the worklist"
        );
        assert_eq!(
            inputs.stats.worklist_resolved_count, 1,
            "the lazy worklist should resolve only the owner-reached root"
        );
        assert_eq!(
            inputs.stats.worklist_enqueued_from_symbol_deps_count, 0,
            "symbol dependencies should stay attached to the reached root instead of becoming new worklist entries"
        );
        assert_eq!(
            inputs.stats.reached_merge_roots_count, 1,
            "only the reached Foo root should require merge-backed source collection"
        );
        assert_eq!(
            inputs.stats.imported_sources_count, 1,
            "merge-backed collection should record exactly the reached merge root source"
        );
        assert_eq!(
            inputs.stats.normalized_imported_type_root_calls, 1,
            "the owner import should normalize once and reuse that root metadata downstream"
        );
        assert_eq!(
            inputs.stats.prepare_imported_type_alias_failures, 0,
            "successful lazy worklist collection should not report prepare failures"
        );
        assert_eq!(
            resolver.prepared_requests.len(),
            1,
            "lazy collection should prepare only the reached root here"
        );
        assert!(
            !inputs.sources.is_empty(),
            "reached merge roots should still materialize their defining sources"
        );
    }

    #[test]
    fn build_imported_eval_inputs_seeds_macro_type_deps_without_source_heuristics() {
        let imports = vec![analyzed_import(
            "./types",
            vec![binding(
                "ButtonProps",
                ImportBindingKind::Named,
                Some("ButtonProps"),
                true,
            )],
            true,
        )];
        let mut resolver = TestCollectorResolver::default();
        resolver.import_targets.insert(
            "/src/App.vue:./types".to_string(),
            "/src/types.ts".to_string(),
        );
        resolver.collected_aliases.insert(
            ("/src/types.ts".to_string(), "ButtonProps".to_string()),
            collected_alias(
                "ButtonProps",
                "/src/types.ts",
                "ButtonProps",
                "/src/types.ts",
                "ButtonProps",
                false,
                Vec::new(),
            ),
        );
        resolver.owner_eval_source = "defineProps<ButtonProps>()".to_string();

        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: &imports,
            macros: &[],
            bindings: &[],
            macro_type_deps: &[MacroTypeDep {
                type_name: "ButtonProps".to_string(),
                import_source: "./types".to_string(),
                macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::default(),
            }],
        };

        let owner_eval_source = resolver.owner_eval_source.clone();
        let owner_env = resolver.owner_env.clone();

        let inputs = build_imported_eval_inputs(
            &mut resolver,
            "/src/App.vue",
            &owner_snapshot,
            owner_eval_source.as_str(),
            &owner_env,
            None,
            &mut ImportedEvalTraversalBudget::new("/src/App.vue", 8),
        );

        assert!(inputs.type_aliases.iter().any(|alias| {
            alias.local_name == "ButtonProps"
                && alias.source_canonical_id == "/src/types.ts"
                && alias.exported_name == "ButtonProps"
        }));
    }

    #[test]
    fn build_imported_eval_inputs_counts_prepare_failures_without_aborting_other_roots() {
        let imports = vec![
            analyzed_import(
                "./dep",
                vec![binding(
                    "Broken",
                    ImportBindingKind::Named,
                    Some("Broken"),
                    true,
                )],
                true,
            ),
            analyzed_import(
                "./dep",
                vec![binding(
                    "Good",
                    ImportBindingKind::Named,
                    Some("Good"),
                    true,
                )],
                true,
            ),
        ];
        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: &imports,
            macros: &[],
            bindings: &[],
            macro_type_deps: &[],
        };
        let mut resolver = TestCollectorResolver::default();
        resolver
            .import_targets
            .insert("/src/App.vue:./dep".to_string(), "/src/dep.ts".to_string());
        resolver
            .failed_aliases
            .insert(("/src/dep.ts".to_string(), "Broken".to_string()));
        resolver.collected_aliases.insert(
            ("/src/dep.ts".to_string(), "Good".to_string()),
            collected_alias(
                "Good",
                "/src/dep.ts",
                "Good",
                "/src/dep.ts",
                "Good",
                false,
                Vec::new(),
            ),
        );

        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let inputs = build_imported_eval_inputs(
            &mut resolver,
            "/src/App.vue",
            &owner_snapshot,
            "",
            &EvalEnv::default(),
            Some(&FxHashSet::from_iter(
                ["Broken".to_string(), "Good".to_string()].into_iter(),
            )),
            &mut budget,
        );

        assert_eq!(
            inputs.stats.prepare_imported_type_alias_failures, 1,
            "per-symbol failures should be counted so regressions are visible during validation"
        );
        assert_eq!(
            inputs.stats.worklist_resolved_count, 1,
            "the successful symbol should still resolve even when another root fails"
        );
        assert_eq!(
            inputs.type_aliases.len(),
            1,
            "failing roots should be skipped without aborting the rest of the worklist"
        );
        assert_eq!(inputs.type_aliases[0].local_name, "Good");
        assert!(
            !inputs
                .type_aliases
                .iter()
                .any(|alias| alias.local_name == "Broken"),
            "failed roots must not leak partially prepared aliases into the result set"
        );
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
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/real.ts".to_string(), "User".to_string()),
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
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/real.ts".to_string(), "User".to_string()),
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
    fn build_imported_eval_inputs_with_owner_context_uses_owner_overrides_without_loading() {
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
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/real.ts".to_string(), "User".to_string()),
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
        let owner_env = EvalEnv::new();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);

        let inputs = build_imported_eval_inputs_with_owner_context(
            &mut resolver,
            "/src/App.vue",
            &owner_snapshot,
            Some("defineProps<Types.User>()"),
            Some(&owner_env),
            None,
            &mut budget,
        );

        assert_eq!(resolver.owner_eval_source_loads.get(), 0);
        assert_eq!(resolver.owner_eval_env_loads.get(), 0);
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
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/real.ts".to_string(), "User".to_string()),
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let mut stats = ImportedEvalStats::default();

        record_required_source_merge_inputs_recursive(
            &mut resolver,
            "/src/root.ts",
            "Props",
            &mut seen_sources,
            &mut inputs,
            &mut canonical_dependencies,
            &mut visited_type_roots,
            &mut budget,
            &mut stats,
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

    #[test]
    fn source_merge_prefers_resolve_imported_type_root_fast_path() {
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
            "export interface RealUser {}".to_string(),
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
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/real.ts".to_string(), "RealUser".to_string()),
        );

        let mut seen_sources = FxHashSet::default();
        let mut inputs = Vec::new();
        let mut canonical_dependencies = BTreeSet::new();
        let mut visited_type_roots = FxHashSet::default();
        let mut budget = ImportedEvalTraversalBudget::new("/src/App.vue", 8);
        let mut stats = ImportedEvalStats::default();

        record_required_source_merge_inputs_recursive(
            &mut resolver,
            "/src/root.ts",
            "Props",
            &mut seen_sources,
            &mut inputs,
            &mut canonical_dependencies,
            &mut visited_type_roots,
            &mut budget,
            &mut stats,
        );

        assert_eq!(
            resolver.recorded_sources,
            vec!["/src/root.ts".to_string(), "/src/real.ts".to_string()]
        );
        assert_eq!(
            resolver.imported_root_lookups.get(),
            1,
            "source merge should use the resolver's direct root lookup once for the imported alias"
        );
        assert_eq!(
            resolver.declaration_lookups.get(),
            0,
            "source merge should not force full declaration metadata when a direct root lookup is available"
        );
        assert_eq!(inputs.len(), 2);
        assert!(canonical_dependencies.contains("/src/dep.ts"));
        assert!(canonical_dependencies.contains("/src/real.ts"));
    }
}
