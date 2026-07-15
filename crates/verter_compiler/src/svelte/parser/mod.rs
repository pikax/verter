//! The Svelte parser: a byte tokenizer producing the [`ParsedSvelte`] template
//! AST.
//!
//! The parser accepts the FULL current Svelte syntax (Svelte 5.56.x) WITHOUT
//! crash — every matrix row parses, regardless of its SUPPORTED/OUT-OF-SCOPE
//! disposition (a projector concern, never a parser one). It performs NO type
//! lowering (the thin-adapters guard bans it): expression interiors are
//! span-recorded and left to the projector. The ONE expression the parser DOES
//! resolve is the `<svelte:options customElement={EXPR}>` VALUE — mirroring
//! upstream, whose `read_options` runs inside the parser and retains the
//! validated value on the AST (see [`options_custom_element`]).

mod block_head;
pub mod options_custom_element;
mod strict_facts;
pub mod template_ast;
#[cfg(test)]
mod template_ast_tests;
pub mod tokenizer;
pub(crate) mod tokenizer_scan;

pub use options_custom_element::{
    resolve_custom_element_expr, AcceptedCustomElementValue, CustomElementDescriptor,
    CustomElementProp, CustomElementShadow,
};
pub use template_ast::{
    forced_runes_option, validate_custom_element_tag, CloseTagViolation, CloseTagViolationKind,
    OptionsCustomElementProbe, OptionsCustomElementTextTag, ParsedSvelte, ScriptBodyGrammar,
    ScriptBodyProbe, StyleBodyProbe, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue,
    SvelteBlock, SvelteBlockClause, SvelteBlockKind, SvelteClauseKind, SvelteDirective,
    SvelteDirectiveKind, SvelteElement, SvelteElementKind, SvelteNode, SvelteParseDiagnostic,
    SvelteParseRejectFact, SvelteParseRejectKind, SvelteScript, SvelteSpecialKind,
    SvelteStrictParseError, SvelteStrictParseErrorKind, SvelteStyle, SvelteTag, SvelteTagKind,
};
pub use tokenizer::parse_svelte;
