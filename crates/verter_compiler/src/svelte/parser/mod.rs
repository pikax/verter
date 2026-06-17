//! The Svelte parser: a byte tokenizer producing the [`ParsedSvelte`] template
//! AST.
//!
//! The parser accepts the FULL current Svelte syntax (Svelte 5.56.x) WITHOUT
//! crash — every matrix row parses, regardless of its SUPPORTED/OUT-OF-SCOPE
//! disposition (a projector concern, never a parser one). It performs NO type
//! lowering (the thin-adapters guard bans it): expression interiors are
//! span-recorded and left to the projector.

mod block_head;
pub mod template_ast;
#[cfg(test)]
mod template_ast_tests;
pub mod tokenizer;

pub use template_ast::{
    ParsedSvelte, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue, SvelteBlock,
    SvelteBlockClause, SvelteBlockKind, SvelteClauseKind, SvelteDirective, SvelteDirectiveKind,
    SvelteElement, SvelteElementKind, SvelteNode, SvelteParseDiagnostic, SvelteScript,
    SvelteSpecialKind, SvelteStyle, SvelteTag, SvelteTagKind,
};
pub use tokenizer::parse_svelte;
