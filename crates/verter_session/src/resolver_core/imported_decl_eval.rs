use verter_semantic::analysis::type_eval::{EvalEnv, TypeDeclInfo};
use verter_semantic::analysis::{AnalyzedBinding, AnalyzedImport, AnalyzedMacro, MacroTypeDep};

use crate::resolver_core::ImportedEvalOwnerSnapshot;

#[derive(Debug, Clone)]
pub struct PreparedImportedDeclContext {
    pub imports: Vec<AnalyzedImport>,
    pub macros: Vec<AnalyzedMacro>,
    pub bindings: Vec<AnalyzedBinding>,
    pub macro_type_deps: Vec<MacroTypeDep>,
    pub eval_source: String,
    pub env: EvalEnv,
    pub decl: TypeDeclInfo,
}

impl PreparedImportedDeclContext {
    pub fn owner_snapshot(&self) -> ImportedEvalOwnerSnapshot<'_> {
        ImportedEvalOwnerSnapshot {
            imports: &self.imports,
            macros: &self.macros,
            bindings: &self.bindings,
            macro_type_deps: &self.macro_type_deps,
        }
    }
}
