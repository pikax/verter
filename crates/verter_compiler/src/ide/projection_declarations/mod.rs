//! The declaration product of one carrier: what a distributable `.d.ts` holds.

/// Module identity a declaration is published under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarationModuleIdentity {
    pub canonical_id: String,
    /// File name of the published declaration, e.g. `Comp.vue.d.ts`.
    pub declaration_file: String,
}

/// Why a dependency cannot be published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureRefusal {
    PrivateCheckingPath { text: String },
    InaccessibleCapture { name: String },
    WorkspaceOnlyImport { specifier: String },
}

/// Names the declaration needs and how each travels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationDependencyClosure {
    /// Declarations lifted into the published file, as text, in order.
    pub lifted: Vec<String>,
    /// Imports kept, as authored specifier and clause.
    pub retained_imports: Vec<String>,
    pub refused: Vec<ClosureRefusal>,
}

/// One correspondence from published text to authored source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarationMapping {
    pub generated_start: u32,
    pub generated_end: u32,
    pub source_start: u32,
    pub source_end: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationMapProjection {
    pub mappings: Vec<DeclarationMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationDiagnostic {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// The typed product in the projection backend's `Declarations` slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationProjectionProduct {
    pub module: DeclarationModuleIdentity,
    /// TypeScript source the engine emits the declaration from. It holds the
    /// public contract only: no setup body, no template scope.
    pub declaration_source: String,
    pub closure: DeclarationDependencyClosure,
    pub map: DeclarationMapProjection,
    /// Non-empty means nothing is published for this module.
    pub diagnostics: Vec<DeclarationDiagnostic>,
}

impl DeclarationProjectionProduct {
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        self.diagnostics.is_empty() && self.closure.refused.is_empty()
    }
}

mod construction;
pub(crate) use construction::{build_declaration_product, DeclarationContractInput};

#[cfg(test)]
mod tests;
