//! The one entry point to oxc's parser.
//!
//! Every production parse goes through [`Parser`], which has the surface
//! of `oxc_parser::Parser` it replaces. Before oxc parses, a linear scan
//! ([`syntax_nesting`]) bounds how deeply the source's syntax tree can
//! nest; a source past [`SYNTAX_NESTING_LIMIT`] is refused with a typed
//! syntax error and an empty program, exactly as oxc returns a source it
//! cannot parse, so every caller's existing parse-failure path handles it.
//! oxc's parser and every pass over its tree recurse once per level with
//! no limit of their own, so without the refusal a deep enough source
//! overflows the thread's stack and aborts the process.

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::config::{NoTokensParserConfig, ParserConfig};
use oxc_parser::{ParseOptions, ParserReturn};
use oxc_span::{SourceType, Span};

mod nesting;

pub use nesting::{Nesting, NestingExceeded};

/// How deeply a source's syntax may nest, in the units [`syntax_nesting`]
/// counts (each bracket, and each operator or postfix link within one).
pub const SYNTAX_NESTING_LIMIT: u32 = 512;

/// The nesting of `source` (see [`nesting`]), or where it first goes past
/// [`SYNTAX_NESTING_LIMIT`].
pub fn syntax_nesting(source: &str, source_type: SourceType) -> Result<Nesting, NestingExceeded> {
    nesting::scan(source, source_type, SYNTAX_NESTING_LIMIT)
}

/// Whether `source` must be refused. Every level the scan counts takes at
/// least one byte, so a source no longer than the limit is never scanned:
/// the many short sources (a template's expressions, a synthesized
/// wrapper) cost nothing.
fn refusal(source: &str, source_type: SourceType) -> Option<NestingExceeded> {
    if source.len() <= SYNTAX_NESTING_LIMIT as usize {
        return None;
    }
    syntax_nesting(source, source_type).err()
}

/// The syntax error a refused source reports.
pub fn nesting_diagnostic(exceeded: NestingExceeded) -> OxcDiagnostic {
    OxcDiagnostic::error(format!(
        "The source nests deeper than {SYNTAX_NESTING_LIMIT} levels of brackets and operators; \
         it is not parsed."
    ))
    .with_label(Span::empty(exceeded.offset))
}

/// `oxc_parser::Parser`, refusing a source that nests past
/// [`SYNTAX_NESTING_LIMIT`].
pub struct Parser<'a, C: ParserConfig = NoTokensParserConfig> {
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
    options: ParseOptions,
    config: C,
}

impl<'a> Parser<'a> {
    pub fn new(allocator: &'a Allocator, source_text: &'a str, source_type: SourceType) -> Self {
        Self {
            allocator,
            source_text,
            source_type,
            options: ParseOptions::default(),
            config: NoTokensParserConfig,
        }
    }
}

impl<'a, C: ParserConfig> Parser<'a, C> {
    #[must_use]
    pub fn with_options(mut self, options: ParseOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn with_config<D: ParserConfig>(self, config: D) -> Parser<'a, D> {
        Parser {
            allocator: self.allocator,
            source_text: self.source_text,
            source_type: self.source_type,
            options: self.options,
            config,
        }
    }

    fn oxc(self, source_text: &'a str) -> oxc_parser::Parser<'a, C> {
        oxc_parser::Parser::new(self.allocator, source_text, self.source_type)
            .with_options(self.options)
            .with_config(self.config)
    }

    /// Parse the source as a program, or refuse it (see [`Parser`]).
    pub fn parse(self) -> ParserReturn<'a> {
        let source_text = self.source_text;
        match refusal(source_text, self.source_type) {
            None => self.oxc(source_text).parse(),
            Some(exceeded) => {
                // What oxc returns for a source it cannot parse: an empty
                // program over the source, the error, and `panicked`.
                let mut refused = self.oxc("").parse();
                refused.program.source_text = source_text;
                refused.errors = vec![nesting_diagnostic(exceeded)];
                refused.panicked = true;
                refused
            }
        }
    }

    /// Parse the source as one expression, or refuse it (see [`Parser`]).
    pub fn parse_expression(self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
        let source_text = self.source_text;
        match refusal(source_text, self.source_type) {
            None => self.oxc(source_text).parse_expression(),
            Some(exceeded) => Err(vec![nesting_diagnostic(exceeded)]),
        }
    }
}

#[cfg(test)]
mod tests;
