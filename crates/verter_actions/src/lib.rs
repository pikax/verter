//! Vue SFC code actions for Verter.
//!
//! `verter_actions` provides quick fixes, refactoring, and source actions.
//! It receives diagnostics + context and produces code edits.
//!
//! LSP-independent — the LSP converts `CodeAction` / `FileEdit` to LSP types.
//!
//! # Architecture
//!
//! ```text
//! LintDiagnostic + ActionContext → ActionProvider → Vec<CodeAction>
//! ```

mod engine;
mod provider;
pub mod providers;
mod types;

pub use engine::ActionEngine;
pub use provider::{ActionContext, ActionProvider};
pub use types::{ActionKind, CodeAction, FileEdit};
