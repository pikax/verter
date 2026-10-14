//! A script's module routing inventory: imports, re-exports, local exports and export
//! assignments with their routing capabilities. Built by the parser front-end.

use verter_type_expr::TopLevelOwnerId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RouteCapability {
    TypeOnly,
    ValueOnly,
    TypeAndValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RouteImportForm {
    Named,
    Default,
    Namespace,
    /// `import x = require("m")`: the module's `export =` value, or its
    /// namespace when it has none.
    ImportEquals,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RouteImportedName {
    Namespace,
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptImportRoute {
    pub owner: TopLevelOwnerId,
    pub local: String,
    pub source: String,
    pub form: RouteImportForm,
    pub capability: RouteCapability,
    pub imported: RouteImportedName,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptSideEffectImport {
    pub owner: TopLevelOwnerId,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptReexportRoute {
    pub owner: TopLevelOwnerId,
    pub exported: String,
    pub source: String,
    pub imported: String,
    pub capability: RouteCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptWildcardRoute {
    pub owner: TopLevelOwnerId,
    pub source: String,
    pub capability: RouteCapability,
    pub exported_namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptLocalExportRoute {
    pub owner: TopLevelOwnerId,
    pub exported: String,
    pub local: String,
    pub capability: RouteCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptExportAssignmentRoute {
    pub owner: TopLevelOwnerId,
    pub local: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ScriptRouteCounts {
    pub top_level_statement_count: usize,
    pub import_binding_count: usize,
    pub bindingless_import_count: usize,
    pub direct_reexport_count: usize,
    pub wildcard_reexport_count: usize,
    pub local_export_count: usize,
    pub export_assignment_count: usize,
}

/// The files one source file pulls into its TypeScript program, in the
/// order the program visits them: triple-slash `path` references, then
/// `types` references, then `lib` references, then module names — import
/// and re-export specifiers in statement order (including the bare
/// specifiers an ambient `declare module "m" { … }` body imports), then
/// dynamic `import("m")` calls and `import("m")` types in source order,
/// then the names of `declare module "m"` augmentations. Authored text
/// only: resolving a name is the program-order authority's job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProgramReferences {
    /// `/// <reference path="…" />`, as written.
    pub paths: Vec<String>,
    /// `/// <reference types="…" />` package names.
    pub types: Vec<String>,
    /// `/// <reference lib="…" />` library names.
    pub libs: Vec<String>,
    /// Module names in TypeScript's import collection order.
    pub module_names: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptRouteInventory {
    /// Top-level module syntax, including empty exports with no route rows.
    /// Computed from the retained AST, never from raw-source token guesses.
    #[serde(default)]
    pub has_module_syntax: bool,
    pub imports: Vec<ScriptImportRoute>,
    pub bindingless_imports: Vec<ScriptSideEffectImport>,
    pub reexports: Vec<ScriptReexportRoute>,
    pub wildcard_reexports: Vec<ScriptWildcardRoute>,
    pub local_exports: Vec<ScriptLocalExportRoute>,
    pub export_assignments: Vec<ScriptExportAssignmentRoute>,
    pub counts: ScriptRouteCounts,
    /// The files this source pulls into its program, for program file
    /// order. Not part of the route surface: no route hash reads it.
    #[serde(default)]
    pub program_references: ProgramReferences,
}
