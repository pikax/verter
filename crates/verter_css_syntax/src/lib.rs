//! Framework-neutral CSS, SCSS, indented Sass, Less, and Stylus syntax authority.
//!
//! The parser emits one lossless event stream to arbitrary sinks. Runtime
//! consumers can process that stream directly; [`LosslessCstSink`] is an
//! optional retained projection. Source bytes stay in [`CssSource`], and hot
//! records contain compact integer kinds and spans rather than owned text.

#[macro_use]
extern crate verter_debug_assert;

mod arena;
pub mod cst;
pub mod diagnostic;
pub mod dialect;
pub mod event;
pub mod inline_style;
mod layout;
pub mod lexer;
pub mod parser;
pub mod selector;
pub mod stage;
pub mod style_ir;
pub mod svelte_compat;
pub mod token;
pub mod version;

pub use cst::{LosslessCst, LosslessCstSink, SyntaxElement, SyntaxNode};
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
#[cfg(any(test, feature = "test-support"))]
pub use parser::css_source_token_reconstructions;
pub use parser::{
    parse_with_sink, CssEntryPoint, CssParseMode, CssSource, SourceSize, SpecialSelectorListPseudo,
};
#[cfg(any(test, feature = "test-support"))]
pub use selector::parse_selector_structure_thread_invocations;
pub use selector::{
    AttributeMatcher, CombinatorKind, ComplexSelector, ComplexSelectorPart, CompoundTail,
    NthExpression, PseudoFunctionKind, SelectorAttribute, SelectorCombinator, SelectorCompleteness,
    SelectorComponent, SelectorComponentKind, SelectorCompound, SelectorFacts,
    SelectorInterpolation, SelectorKind, SelectorList, SelectorPseudo, SelectorStructure,
    SelectorTrust, SvelteNthArg,
};
pub use stage::{
    ExternalStyleProducer, PreprocessedStyle, PreprocessorIdentity, QualifiedStyleResult,
    StyleDependency, StyleDependencyKind, StyleDiagnostic, StyleProducer, StyleSpecifier,
    StyleSpecifierForm, StyleStage,
};
#[cfg(any(test, feature = "test-support"))]
pub use style_ir::parse_style_ir_thread_invocations;
#[cfg(any(test, feature = "test-support"))]
pub use style_ir::set_style_ir_parse_phase_probe;
pub use style_ir::{
    parse_component_value_tree, parse_style_ir, ComponentBlock, ComponentDelimiter,
    ComponentFunction, ComponentToken, ComponentValue, ComponentValueTree, OwnedComponentValueTree,
    StaticClassFact, StyleBlock, StyleBlockKind, StyleCompleteness, StyleDeclaration,
    StyleDirective, StyleMixinOrFunction, StyleRule, StyleStatement, StyleSyntaxIr,
    UnknownStatement, UnknownStatementKind, ValueInterpolation,
};
pub use svelte_compat::{
    parse_style_body, svelte_first_significant_value_span, svelte_nth_of_selector_span,
    svelte_percentage_selector_span, svelte_read_value_text, svelte_reject_from_ir,
    svelte_trailing_type_selector_span, svelte_trim_js_whitespace, CssBodyParseError,
};
pub use token::{
    css_identifier_eq_ignore_ascii_case, decode_css_identifier, DecodedName, SyntaxToken,
    TokenFlags, TokenKind,
};
pub use version::CssSyntaxGrammarVersion;

#[cfg(test)]
extern crate self as verter_css_syntax;

#[cfg(test)]
#[path = "test_allocator.rs"]
mod test_allocator;
#[cfg(test)]
pub(crate) use test_allocator::measure_allocations;

#[cfg(test)]
#[path = "test_cases.rs"]
mod cases;
