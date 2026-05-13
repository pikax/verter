#![deny(missing_docs)]
//! Stringly-typed mirrors of session / consumer-crate enums that
//! cannot live in the substrate without creating dependency cycles.
//! Producers implement `From<Domain> for Tag` in their own crate.

use serde::{Deserialize, Serialize};

/// Compile target — mirror of `verter_compiler::CompileTarget`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum CompileTargetTag {
    /// VDOM render-function backend.
    Vdom,
    /// Vapor renderer backend.
    Vapor,
    /// IDE backend (valid TSX/JSX for type checking). Default —
    /// matches the LSP / tsgo path that drives most audited compiles.
    #[default]
    Ide,
}

/// Projection mode — mirror of
/// `verter_session::semantic_query::ProjectionMode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ProjectionModeTag {
    /// Identity — pass-through, no projection. Default for
    /// data-only construction sites.
    #[default]
    Identity,
    /// Navigate — preserve carriers, no expansion.
    Navigate,
    /// Shallow — expose one level of surface members.
    Shallow,
    /// Expanded — recursively materialize.
    Expanded,
    /// Skeleton — open-generic body access for cycle detection.
    Skeleton,
}

/// Augmentation target kind — mirror of
/// `verter_session::file_artifact_store::AugmentationTargetKind`.
///
/// Discriminates the four shapes a `declare module "X" { ... }`
/// augmentation can target: an external specifier, a relative path
/// resolved against the augmenter, a wildcard ambient pattern, or
/// the global block. The concrete target value (specifier text,
/// resolved canonical, wildcard pattern) lives in the parallel
/// optional fields of the audit-event variants that carry this tag,
/// keeping the tag itself `Copy + Hash + Eq`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum AugmentationTargetKindTag {
    /// `declare module "vue" {}` — bare specifier resolved through
    /// the project's module resolver under the resolve env. Default
    /// because it is the most common archetype on real Vue / React
    /// codebases.
    #[default]
    ExternalSpecifier,
    /// `declare module "./local" {}` — relative path resolved
    /// against the augmenter's own canonical.
    ResolvedRelativeCanonical,
    /// `declare module "*.css" {}` — wildcard ambient module
    /// pattern.
    WildcardAmbient,
    /// `declare global { ... }` — augments the global scope.
    GlobalAugmentation,
}

/// Bundler kind — mirror of the unplugin's bundler discriminator.
/// Not `Copy` because the `Other` variant carries an owned name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum BundlerKindTag {
    /// Vite — default bundler for the unplugin path.
    #[default]
    Vite,
    /// Webpack.
    Webpack,
    /// Rollup.
    Rollup,
    /// Esbuild.
    Esbuild,
    /// Rolldown.
    Rolldown,
    /// Other bundler — name preserved verbatim.
    Other(String),
}

/// LSP method — mirror of the LSP request method names emitted by
/// `verter_lsp::handlers`. Not `Copy` because the `Other` variant
/// carries an owned method name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum LspMethodTag {
    /// `textDocument/hover`. Default — most audited LSP paths
    /// originate from hover.
    #[default]
    Hover,
    /// `textDocument/definition`.
    GotoDefinition,
    /// `textDocument/completion`.
    Completion,
    /// `textDocument/references`.
    References,
    /// `textDocument/publishDiagnostics`.
    Diagnostics,
    /// `textDocument/documentSymbol`.
    DocumentSymbols,
    /// `textDocument/semanticTokens`.
    SemanticTokens,
    /// `textDocument/inlayHint`.
    InlayHints,
    /// `textDocument/codeAction`.
    CodeAction,
    /// `textDocument/rename`.
    Rename,
    /// Open-ended escape hatch for methods not enumerated above.
    Other(String),
}
