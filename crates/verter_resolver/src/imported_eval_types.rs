use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ImportedEvalInputs {
    pub sources: Vec<ImportedEvalSource>,
    pub type_aliases: Vec<ImportedTypeAlias>,
    pub canonical_dependencies: BTreeSet<String>,
    pub overflow: Option<ImportedEvalOverflow>,
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
