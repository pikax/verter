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
pub use types::{ActionKind, AutofixSafety, CodeAction, FileEdit};

// Reserved homes for planned actions product modules. Each module below is
// declaration-only: its doc line names the node that fills it, and product
// nodes write inside their own reserved module only.
pub mod assists;
pub mod compat_prefix;
pub mod css_custom_property;
pub mod css_logical;
pub mod css_value_assists;
pub mod fix_all;
pub mod fix_intents;
pub mod inline_style_to_class;
pub mod migrations;
pub mod refactors;
pub mod rename_intents;
pub mod scss_module;
pub mod transaction;
pub mod verify;
