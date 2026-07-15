//! Vue SFC diagnostic engine for Verter.
//!
//! `verter_diagnostics` is a pure function: analysis data in, diagnostics out.
//! It depends only on `verter_semantic::analysis` and can be used:
//! - By `verter_lsp` for real-time diagnostics
//! - As a standalone CLI tool for CI/CD
//! - By third-party tools consuming analysis snapshots
//!
//! Detection only — rules produce diagnostics, external sources (TSGO) enrich them.
//! Fixes live in `verter_actions`.
//!
//! # Architecture
//!
//! The diagnostic engine uses a trait-based rule system with a single-pass DFS visitor.
//! Each rule implements [`LintRule`] with optional hooks for template elements,
//! directives, script bindings, style blocks, and cross-file analysis.
//!
//! ```text
//! AnalysisSnapshot → LintVisitor → [Rule1, Rule2, ...] → DiagnosticSet
//! ```

pub mod casing;
mod comment_directives;
mod config;
mod context;
pub mod cross_file;
mod diagnostic;
mod diagnostic_set;
mod linter;
pub mod rules;
mod visitor;

pub use comment_directives::parse_comment_directives;
pub use config::{
    discover_lint_config, parse_rule_severity, strip_json_comments, strip_trailing_commas,
    LintConfig, LintPreset, ProjectLintConfig, ProjectSsrConfig, ResolvedLintConfig,
    VerterProjectConfig,
};
pub use context::LintContext;
pub use cross_file::{
    build_cross_file_snapshot, find_unknown_models, find_unknown_props, kebab_to_camel,
    ChildComponentInfo, CrossFileSnapshot, UnknownModelEntry, UnknownPropEntry,
};
pub use diagnostic::{
    Certainty, DiagnosticSpanKind, DiagnosticTag, EvidenceSnippet, LintDiagnostic, RelatedFile,
    Severity,
};
pub use diagnostic_set::DiagnosticSet;
pub use linter::Linter;
pub use rules::{FileContext, LintRule};
pub use visitor::LintVisitor;

#[cfg(test)]
pub(crate) mod test_support;
