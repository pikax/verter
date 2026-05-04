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
