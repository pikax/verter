//! Vue SFC linter engine for Verter.
//!
//! `verter_linter` is a pure function: analysis data in, diagnostics out.
//! It depends only on `verter_analysis` and can be used:
//! - By `verter_lsp` for real-time diagnostics
//! - As a standalone CLI tool for CI/CD
//! - By third-party tools consuming analysis snapshots
//!
//! # Architecture
//!
//! The linter uses a trait-based rule system with a single-pass DFS visitor.
//! Each rule implements [`LintRule`] with optional hooks for template elements,
//! directives, script bindings, style blocks, and cross-file analysis.
//!
//! ```text
//! AnalysisSnapshot → LintVisitor → [Rule1, Rule2, ...] → Vec<LintDiagnostic>
//! ```

mod comment_directives;
mod config;
mod context;
mod diagnostic;
mod linter;
pub mod rules;
mod visitor;

pub use comment_directives::parse_comment_directives;
pub use config::{LintConfig, LintPreset};
pub use context::LintContext;
pub use diagnostic::{LintDiagnostic, LintFix, Severity};
pub use linter::Linter;
pub use rules::LintRule;
pub use visitor::LintVisitor;
