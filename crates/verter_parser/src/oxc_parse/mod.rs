//! The one entry point to oxc's parser.
//!
//! Every production parse goes through [`Parser`], which has the surface
//! of `oxc_parser::Parser` it replaces and parses exactly what oxc parses.
//! oxc's parser is a recursive descent with no depth limit of its own: it
//! spends native stack per level of nesting, and on a thread's fixed stack
//! a deeply nested source overflows it and aborts the process (the
//! standalone reproducer is `examples/oxc_deep_parse.rs`; the evidence is
//! in `docs/evidence/signature-kernel/oxc-deep-parse.md`).
//!
//! [`Parser`] runs the parse on a stack its source cannot exhaust. A linear
//! scan ([`nesting`]) bounds, from above, how deeply the source's syntax
//! tree nests; the parse needs at most [`PARSE_STACK_BYTES_PER_LEVEL`] per
//! level of that bound. When the calling thread has that much stack left
//! the parse runs in place, which is every source of ordinary depth;
//! otherwise it runs on a stack segment allocated for it (`stacker`). No
//! source is refused, and no depth is too deep: the segment is sized from
//! the source.

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::config::{NoTokensParserConfig, ParserConfig};
use oxc_parser::{ParseOptions, ParserReturn};
use oxc_span::SourceType;

mod nesting;

pub use nesting::Nesting;

/// The most native stack oxc 0.126's parser spends per level of
/// [`nesting`]'s bound, with twice the margin: measured with
/// `examples/oxc_deep_parse.rs` on an unoptimized build (the larger frames),
/// a level takes at most about 3.7 KiB (a nested type argument, which the
/// scan counts once; a parenthesis, bracket or template hole 2.5 KiB, a
/// `!` 0.6 KiB), and an optimized build about half of that.
pub const PARSE_STACK_BYTES_PER_LEVEL: usize = 8 * 1024;

/// Stack the parse needs besides its per-level recursion.
const PARSE_STACK_BASE_BYTES: usize = 512 * 1024;

/// An upper bound of how deeply `source`'s syntax tree nests (see
/// [`nesting`]).
pub fn syntax_nesting(source: &str, source_type: SourceType) -> Nesting {
    match nesting::scan(source, source_type, u32::MAX) {
        Ok(nesting) => nesting,
        Err(_) => unreachable!("no nesting exceeds an unbounded limit"),
    }
}

/// The stack a parse of `source_text` can need.
pub fn parse_stack_bytes(source_text: &str, source_type: SourceType) -> usize {
    (syntax_nesting(source_text, source_type).depth as usize)
        .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
        .saturating_add(PARSE_STACK_BASE_BYTES)
}

/// Run `parse` with at least [`parse_stack_bytes`] of stack: in place when
/// the thread has it, on a new stack segment otherwise.
fn with_parse_stack<R>(source_text: &str, source_type: SourceType, parse: impl FnOnce() -> R) -> R {
    // Every level of nesting takes at least one byte of source, so a source
    // whose every byte could be a level fits the thread's remaining stack
    // without the scan: the many short sources (a template's expressions, a
    // synthesized wrapper) parse in place at once.
    let by_length = source_text
        .len()
        .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
        .saturating_add(PARSE_STACK_BASE_BYTES);
    if stacker::remaining_stack().is_some_and(|remaining| remaining >= by_length) {
        return parse();
    }
    let needed = parse_stack_bytes(source_text, source_type);
    stacker::maybe_grow(needed, needed, parse)
}

/// `oxc_parser::Parser`, parsing on a stack its source cannot exhaust.
pub struct Parser<'a, C: ParserConfig = NoTokensParserConfig> {
    inner: oxc_parser::Parser<'a, C>,
    source_text: &'a str,
    source_type: SourceType,
}

impl<'a> Parser<'a> {
    pub fn new(allocator: &'a Allocator, source_text: &'a str, source_type: SourceType) -> Self {
        Self {
            inner: oxc_parser::Parser::new(allocator, source_text, source_type),
            source_text,
            source_type,
        }
    }
}

impl<'a, C: ParserConfig> Parser<'a, C> {
    #[must_use]
    pub fn with_options(self, options: ParseOptions) -> Self {
        Self {
            inner: self.inner.with_options(options),
            ..self
        }
    }

    #[must_use]
    pub fn with_config<D: ParserConfig>(self, config: D) -> Parser<'a, D> {
        Parser {
            inner: self.inner.with_config(config),
            source_text: self.source_text,
            source_type: self.source_type,
        }
    }

    /// Parse the source as a program.
    pub fn parse(self) -> ParserReturn<'a> {
        let Self {
            inner,
            source_text,
            source_type,
        } = self;
        with_parse_stack(source_text, source_type, move || inner.parse())
    }

    /// Parse the source as one expression.
    pub fn parse_expression(self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
        let Self {
            inner,
            source_text,
            source_type,
        } = self;
        with_parse_stack(source_text, source_type, move || inner.parse_expression())
    }
}

#[cfg(test)]
mod tests;
