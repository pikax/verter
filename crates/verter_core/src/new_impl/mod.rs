//! AST-based Vue SFC and template parser.
//!
//! This module implements an alternative parsing pipeline that produces a
//! concrete AST (arena-allocated) instead of the event-stream approach used
//! by `crate::syntax`. The pipeline is:
//!
//! ```text
//! TokenizerEvent stream
//!     ↓
//! syntax::Syntax          (event dispatcher + SFC root detection)
//!     ├── Root nodes      (RootNodeScript, RootNodeStyle, RootNodeTemplate, …)
//!     └── ast::builder    (TemplateAstBuilder → TemplateAst)
//!         └── ast::types  (AstNode arena with O(1) parent/sibling navigation)
//! ```
//!
//! - **`types`** — Shared low-level types: `NodeTag`, `NodeId`, `NodeProp`.
//! - **`ast`** — Arena-based AST with pre-computed codegen metadata.
//! - **`syntax`** — Tokenizer event dispatcher that builds root nodes and the
//!   template AST, handling SFC root detection, close-tag validation, directive
//!   classification, and diagnostic collection.

pub mod ast;
pub mod compile;
pub mod script;
pub mod style;
pub mod syntax;
pub mod template;
pub mod types;

#[cfg(test)]
pub(crate) mod test_helpers {
    use smallvec::SmallVec;

    use super::syntax::types::{RootNodeTemplate, RootNodeTemplateContent};
    use super::types::NodeTag;

    pub fn make_root() -> RootNodeTemplate {
        RootNodeTemplate {
            tag_open: NodeTag {
                start: 0,
                end: 0,
                name_end: 0,
            },
            tag_close: None,
            lang: None,
            attributes: Vec::new(),
            content: Some(RootNodeTemplateContent {
                start: 0,
                end: 0,
                children: SmallVec::new(),
            }),
        }
    }

    pub fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
        NodeTag {
            start,
            end,
            name_end,
        }
    }
}
