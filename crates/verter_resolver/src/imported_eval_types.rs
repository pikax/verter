use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ImportedEvalInputs {
    pub sources: Vec<ImportedEvalSource>,
    pub type_aliases: Vec<ImportedTypeAlias>,
    pub canonical_dependencies: BTreeSet<String>,
    pub overflow: Option<ImportedEvalOverflow>,
    pub stats: ImportedEvalStats,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedEvalStats {
    pub worklist_seed_count: u64,
    pub worklist_resolved_count: u64,
    pub worklist_enqueued_from_symbol_deps_count: u64,
    pub reached_merge_roots_count: u64,
    pub imported_sources_count: u64,
    pub normalized_imported_type_root_calls: u64,
    pub prepare_imported_type_alias_failures: u64,
    pub dropped_unreached_aliases: u64,
}

#[derive(Debug, Clone)]
pub struct ImportedEvalSource {
    pub canonical_id: String,
    pub source: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportedSymbolDependency {
    pub local_name: String,
    pub canonical_id: String,
    pub exported_name: String,
}

#[derive(Debug, Clone)]
pub struct ImportedTypeAlias {
    pub local_name: String,
    pub source_canonical_id: String,
    pub exported_name: String,
    pub requires_source_merge: bool,
    pub merge_root_canonical: String,
    pub merge_root_exported: String,
}

#[derive(Debug, Clone)]
pub struct CollectedImportedTypeAlias {
    pub alias: ImportedTypeAlias,
    pub symbol_dependencies: Vec<ImportedSymbolDependency>,
}

#[derive(Debug, Clone)]
pub struct ImportedEvalOverflow {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ComputedEvaluatedTypes {
    pub evaluated_types: Option<verter_analysis::type_expand::ExpandedComponentTypes>,
    pub discovered_dependencies: BTreeSet<String>,
}
