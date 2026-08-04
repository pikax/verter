//! Framework-neutral CSS, SCSS, and Less lexical and grammar authority.
//!
//! The parser emits one lossless event stream to arbitrary sinks. Runtime
//! consumers can process that stream directly; [`LosslessCstSink`] is an
//! optional retained projection. Source bytes stay in [`CssSource`], and hot
//! records contain compact integer kinds and spans rather than owned text.

pub mod cst;
pub mod diagnostic;
pub mod dialect;
pub mod event;
pub mod lexer;
pub mod parser;
pub mod selector;
pub mod token;

pub use cst::{parse_lossless, LosslessCst, LosslessCstSink, SyntaxElement, SyntaxNode};
pub use diagnostic::{
    CssDiagnostic, CssDiagnosticKind, CssParseFailure, CssSeverity, CssSourceTooLarge,
    CssStructureTooLarge, RecoveryKind, StructureOverflowKind,
};
pub use dialect::CssDialect;
pub use event::{NodeFlags, ParseEvent, ParseEventSink, ParseSummary, SyntaxKind};
pub use lexer::Lexer;
pub use parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource, Parser, SourceSize};
pub use selector::{
    parse_selector_structure, AttributeMatcher, CombinatorKind, NthExpression, PseudoFunctionKind,
    SelectorAttribute, SelectorCombinator, SelectorComponent, SelectorComponentKind, SelectorKind,
    SelectorPseudo, SelectorStructure,
};
pub use token::{
    css_identifier_eq_ignore_ascii_case, decode_css_identifier, DecodedName, SyntaxToken,
    TokenFlags, TokenKind,
};
