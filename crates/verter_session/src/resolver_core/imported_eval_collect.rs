use crate::resolver_core::{
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

pub trait ImportedEvalResolver: DeclarationMetadataResolver {
    // --- Source merge methods ---

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
    ) -> crate::resolver_core::ResolvedTypeDeclaration;

    /// Run the BFS frontier engine for a set of merge roots to pre-warm
    /// caches before the recursive merge-input loop.
    ///
    /// `roots` is a slice of `(canonical_id, exported_name)` pairs
    /// representing the reached merge roots that will be walked.
    ///
    /// The default implementation returns `None`, meaning the frontier
    /// pass was unavailable. Callers must surface overflow instead of
    /// silently falling back to recursive merge-root discovery.
    fn run_frontier_for_merge_roots(
        &mut self,
        _roots: &[(String, String)],
    ) -> Option<crate::resolver_core::ExternalTypeFrontier> {
        None
    }

    fn merge_root_frontier_failure_message(&self) -> Option<String> {
        None
    }

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

    // --- Collector methods ---

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

    // --- Owner methods ---

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

    // --- Owner context methods ---

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

#[derive(Debug, Clone)]
struct PendingImportedTypeAlias {
    local_name: String,
    source_canonical_id: String,
    exported_name: String,
    merge_root_canonical: String,
    merge_root_exported: String,
}

fn normalized_imported_type_root<R: ImportedEvalResolver>(
    resolver: &R,
    dep_canonical: &str,
    imported_name: &str,
    stats: &mut ImportedEvalStats,
) -> (String, String) {
    stats.normalized_imported_type_root_calls += 1;
    resolver.resolve_imported_type_root(dep_canonical, imported_name)
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn record_required_source_merge_inputs_recursive<R: ImportedEvalResolver>(
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
fn collect_imported_eval_inputs_lazy<R: ImportedEvalResolver>(
    resolver: &mut R,
    owner_canonical_id: &str,
    imports: &[AnalyzedImport],
    required_import_names: &FxHashSet<String>,
    seen_sources: &mut FxHashSet<String>,
    inputs: &mut Vec<ImportedEvalSource>,
    type_aliases: &mut Vec<ImportedTypeAlias>,
    canonical_dependencies: &mut BTreeSet<String>,
    _visited_type_roots: &mut FxHashSet<(String, String)>,
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

    if !reached_merge_roots.is_empty() && !budget.is_exhausted() {
        if let Some(frontier) = resolver.run_frontier_for_merge_roots(&reached_merge_roots) {
            record_merge_inputs_from_frontier(
                resolver,
                &frontier,
                seen_sources,
                inputs,
                canonical_dependencies,
            );
        } else {
            let message = resolver.merge_root_frontier_failure_message().unwrap_or_else(|| {
                format!(
                    "component-meta merge-root frontier unavailable while resolving merge inputs for '{}'",
                    owner_canonical_id,
                )
            });
            budget.set_overflow(message);
        }
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
pub fn collect_imported_eval_inputs<R: ImportedEvalResolver>(
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
pub fn build_imported_eval_inputs<R: ImportedEvalResolver>(
    resolver: &mut R,
    owner_canonical_id: &str,
    owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    owner_eval_source: &str,
    owner_env: &EvalEnv,
    additional_required_import_names: Option<&FxHashSet<String>>,
    budget: &mut ImportedEvalTraversalBudget,
) -> crate::resolver_core::ImportedEvalInputs {
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

    crate::resolver_core::ImportedEvalInputs {
        sources: inputs,
        type_aliases,
        canonical_dependencies,
        overflow: budget.overflow(),
        stats,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn build_imported_eval_inputs_with_owner_context<R: ImportedEvalResolver>(
    resolver: &mut R,
    owner_canonical_id: &str,
    owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    owner_eval_source: Option<&str>,
    owner_env_override: Option<&EvalEnv>,
    additional_required_import_names: Option<&FxHashSet<String>>,
    budget: &mut ImportedEvalTraversalBudget,
) -> crate::resolver_core::ImportedEvalInputs {
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
        owner_env,
        additional_required_import_names,
        budget,
    )
}

/// Frontier-aware merge root collection.
///
/// Instead of recursively walking the import graph per merge root, this
/// function uses a pre-computed frontier to discover which canonical files
/// participate in the merge. It then iterates over those files to record
/// eval input sources.
///
/// The frontier must have already been run (seeded with the merge roots
/// and executed via `frontier.run(host)`).
pub fn record_merge_inputs_from_frontier<R: ImportedEvalResolver>(
    resolver: &mut R,
    frontier: &crate::resolver_core::ExternalTypeFrontier,
    seen_sources: &mut FxHashSet<String>,
    inputs: &mut Vec<ImportedEvalSource>,
    canonical_dependencies: &mut BTreeSet<String>,
) {
    // Record all canonical files the frontier touched as dependencies
    for canonical_id in frontier.touched_canonical_ids() {
        canonical_dependencies.insert(canonical_id.clone());
        resolver.record_eval_input_source(
            &canonical_id,
            seen_sources,
            inputs,
            canonical_dependencies,
        );
    }
}
