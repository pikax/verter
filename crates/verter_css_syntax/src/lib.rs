//! Framework-neutral CSS, SCSS, indented Sass, Less, and Stylus syntax authority.
//!
//! The parser emits one lossless event stream to arbitrary sinks. Runtime
//! consumers can process that stream directly; [`LosslessCstSink`] is an
//! optional retained projection. Source bytes stay in [`CssSource`], and hot
//! records contain compact integer kinds and spans rather than owned text.

#[macro_use]
extern crate verter_debug_assert;

pub mod cst;
pub mod diagnostic;
pub mod dialect;
pub mod event;
pub mod inline_style;
mod layout;
pub mod lexer;
pub mod parser;
pub mod selector;
pub mod style_ir;
pub mod svelte_compat;
pub mod token;
pub mod version;

pub use cst::{parse_lossless, LosslessCst, LosslessCstSink, SyntaxElement, SyntaxNode};
pub use diagnostic::{
    CssDiagnostic, CssDiagnosticKind, CssParseFailure, CssSeverity, CssSourceTooLarge,
    CssStructureTooLarge, RecoveryKind, StructureOverflowKind,
};
pub use dialect::CssDialect;
pub use event::{NodeFlags, ParseEvent, ParseEventSink, ParseSummary, SyntaxKind};
#[cfg(any(test, feature = "test-support"))]
pub use inline_style::parse_inline_style_declarations_thread_invocations;
pub use inline_style::{parse_inline_style_declarations, InlineStyleDeclaration};
pub use lexer::Lexer;
pub use parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource, Parser, SourceSize};
pub use selector::{
    parse_selector_structure, AttributeMatcher, CombinatorKind, ComplexSelector,
    ComplexSelectorPart, NthExpression, PseudoFunctionKind, SelectorAttribute, SelectorCombinator,
    SelectorCompleteness, SelectorComponent, SelectorComponentKind, SelectorCompound,
    SelectorFacts, SelectorInterpolation, SelectorKind, SelectorList, SelectorPseudo,
    SelectorStructure, SelectorTrust,
};
pub use style_ir::{
    parse_component_value_tree, parse_style_ir, ComponentBlock, ComponentDelimiter,
    ComponentFunction, ComponentToken, ComponentValue, ComponentValueTree, StaticClassFact,
    StyleBlock, StyleBlockKind, StyleCompleteness, StyleDeclaration, StyleDirective,
    StyleMixinOrFunction, StyleRule, StyleStatement, StyleSyntaxIr, StyleSyntaxIrSink,
    UnknownStatement, UnknownStatementKind, ValueInterpolation,
};
pub use svelte_compat::style_body_reject_code;
pub use token::{
    css_identifier_eq_ignore_ascii_case, decode_css_identifier, DecodedName, SyntaxToken,
    TokenFlags, TokenKind,
};
pub use version::CssSyntaxGrammarVersion;
