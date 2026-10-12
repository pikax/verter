//! Resolved module bindings, shared by namespace value demands.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::facts::fact_cache::FactVersionRef;
use crate::type_solver::ResolvedRootIdentity;

#[derive(Debug, Clone)]
pub struct ModuleExportTable {
    /// `None` is an ambiguous star binding. Retaining it makes ambiguity
    /// propagate through later barrels; absence alone would lose that fact.
    pub bindings: FxHashMap<Arc<str>, Option<ModuleExportBinding>>,
    pub facts: Arc<[FactVersionRef]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleExportBinding {
    Value(ResolvedRootIdentity),
    Namespace(Arc<str>),
}
