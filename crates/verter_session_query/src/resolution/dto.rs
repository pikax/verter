//! Core resolver value types: the request/result vocabulary shared by every
//! `ModuleResolverCore` entry point.
//!
//! These are dependency-neutral plain data — no VFS access, no cache
//! authority, no workspace-private identity. `verter_workspace` re-exports
//! these definitions by value for its own callers; this module is their
//! canonical owner.

/// Ownership information for a file within a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOwnership {
    pub project_root: String,
    pub tsconfig_path: Option<String>,
}

/// Resolution kind for an import or external source request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolveRequestKind {
    EsmImport,
    TypeImport,
    RequireCall,
    SfcSrcAttr,
}

/// Which dependency graph is asking for resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

/// Resolution context — determines which target a specifier resolves to.
///
/// Different `(phase, kind)` combinations produce different results for the
/// same specifier. For example:
/// - `(CodegenBlocker, EsmImport)` → runtime entry (`index.js`, `"import"` condition)
/// - `(CodegenBlocker, TypeImport)` → type entry (`index.d.ts`, `"types"` condition)
/// - `(ProviderGraph, *)` → type entry (`"types"` condition)
/// - `(*, RequireCall)` → CJS entry (`"require"` condition)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolutionContext {
    pub phase: ResolvePhase,
    pub kind: ResolveRequestKind,
}

/// Where the resolved file should be exposed to the provider layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTarget {
    SourceFile,
    CarrierPublicApi,
    ShadowSourceFile,
}

/// High-level category describing how the specifier resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    Relative,
    TsConfigPath,
    ProjectReference,
    NodeModules,
    PackageExports,
    PackageImports,
    WorkspaceAlias,
    Bundler,
    PlaygroundMap,
}

/// Input for import resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    pub importer_id: String,
    pub specifier: String,
    pub kind: ResolveRequestKind,
    pub phase: ResolvePhase,
}

/// Output from import resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveResult {
    pub source_id: String,
    pub provider_id: String,
    pub provider_specifier: String,
    pub provider_target: ProviderTarget,
    pub resolution_kind: ResolutionKind,
    pub owner_tsconfig_path: Option<String>,
}

#[cfg(test)]
#[path = "dto_tests.rs"]
mod tests;
