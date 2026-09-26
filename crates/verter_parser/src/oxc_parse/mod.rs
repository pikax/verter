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
//!
//! oxc's own walks over what it parsed (`clone_in`, the semantic builder,
//! the `Visit` and `VisitMut` walkers) recurse once per level of the same
//! syntax tree, spending less per level than the parse. Every call into
//! one runs under the same containment: [`with_program_stack`] for a walk
//! of a whole program, [`with_node_stack`] for a walk of one node of it,
//! [`with_ast_stack`] for a walk of a node given its source text, and
//! [`with_nesting_stack`] where the caller already holds the [`Nesting`]
//! of what it walks.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Program};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::config::{NoTokensParserConfig, ParserConfig};
use oxc_parser::{ParseOptions, ParserReturn};
use oxc_span::{SourceType, Span};

mod nesting;

pub use nesting::Nesting;

/// The most native stack oxc 0.126's parser, or a walk of oxc's over what
/// it parsed, spends per level of
/// [`nesting`]'s bound, with twice the margin: measured with
/// `examples/oxc_deep_parse.rs` on an unoptimized build (the larger frames),
/// a parse level takes at most about 3.7 KiB (a nested type argument, which
/// the scan counts once; a parenthesis, bracket or template hole 2.5 KiB, a
/// `!` 0.6 KiB), and an optimized build about half of that. A walk spends
/// less: `clone_in`, the deepest of them, about 1.4 KiB a `!`.
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

/// Run `walk`, a parse of `source_text` or a walk of oxc's over syntax
/// parsed from it, with at least [`parse_stack_bytes`] of stack: in place
/// when the thread has it, on a new stack segment otherwise.
pub fn with_ast_stack<R>(
    source_text: &str,
    source_type: SourceType,
    walk: impl FnOnce() -> R,
) -> R {
    // Every level of nesting takes at least one byte of source, so a source
    // whose every byte could be a level fits the thread's remaining stack
    // without the scan: the many short sources (a template's expressions, a
    // synthesized wrapper) parse in place at once.
    let by_length = source_text
        .len()
        .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
        .saturating_add(PARSE_STACK_BASE_BYTES);
    if stacker::remaining_stack().is_some_and(|remaining| remaining >= by_length) {
        return walk();
    }
    let needed = parse_stack_bytes(source_text, source_type);
    stacker::maybe_grow(needed, needed, walk)
}

/// [`with_ast_stack`] for a walk of `program`, or of any node in it.
pub fn with_program_stack<R>(program: &Program<'_>, walk: impl FnOnce() -> R) -> R {
    with_ast_stack(program.source_text, program.source_type, walk)
}

/// [`with_ast_stack`] for a walk of the node at `span` in `program`,
/// sized from the node's own text: a short node walks in place at once, and
/// a long one costs a scan of its text, no more than the walk itself.
pub fn with_node_stack<R>(program: &Program<'_>, span: Span, walk: impl FnOnce() -> R) -> R {
    let text = program
        .source_text
        .get(span.start as usize..span.end as usize)
        .unwrap_or(program.source_text);
    with_ast_stack(text, program.source_type, walk)
}

/// [`with_ast_stack`] for a walk of a node given only its source text, not
/// its program's source type. The scan's source type decides only whether
/// `<` can open a JSX element and whether a declaration file's unions
/// count; the syntax nests no deeper than the larger of a TypeScript and a
/// TSX scan of it, whichever it is.
pub fn with_source_stack<R>(source_text: &str, walk: impl FnOnce() -> R) -> R {
    let by_length = source_text
        .len()
        .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
        .saturating_add(PARSE_STACK_BASE_BYTES);
    if stacker::remaining_stack().is_some_and(|remaining| remaining >= by_length) {
        return walk();
    }
    let nesting = Nesting {
        depth: syntax_nesting(source_text, SourceType::ts())
            .depth
            .max(syntax_nesting(source_text, SourceType::tsx()).depth),
    };
    with_nesting_stack(nesting, walk)
}

/// [`with_source_stack`] for a walk of the node at `span` in `source_text`,
/// the text its spans index (the whole text when the span lies outside
/// it).
pub fn with_span_stack<R>(source_text: &str, span: Span, walk: impl FnOnce() -> R) -> R {
    let text = source_text
        .get(span.start as usize..span.end as usize)
        .unwrap_or(source_text);
    with_source_stack(text, walk)
}

/// The containment of many walks over nodes of one program: a node short
/// enough to fit the thread's remaining stack at a level per byte walks in
/// place, and a longer one on the stack the whole program's [`Nesting`]
/// can need, scanned at most once however many nodes walk. Where
/// [`with_node_stack`] would rescan a node nested in a node already
/// scanned, this scans the program once.
pub struct ProgramWalkStack<'p> {
    program: &'p Program<'p>,
    nesting: std::cell::OnceCell<Nesting>,
}

impl<'p> ProgramWalkStack<'p> {
    pub fn new(program: &'p Program<'p>) -> Self {
        Self {
            program,
            nesting: std::cell::OnceCell::new(),
        }
    }

    /// The program whose nodes walk under this containment.
    pub fn program(&self) -> &'p Program<'p> {
        self.program
    }

    /// Run `walk`, a walk of oxc's over the node at `span`, with the stack
    /// it can need.
    pub fn with_node_stack<R>(&self, span: Span, walk: impl FnOnce() -> R) -> R {
        let by_length = (span.size() as usize)
            .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
            .saturating_add(PARSE_STACK_BASE_BYTES);
        if stacker::remaining_stack().is_some_and(|remaining| remaining >= by_length) {
            return walk();
        }
        let nesting = *self
            .nesting
            .get_or_init(|| syntax_nesting(self.program.source_text, self.program.source_type));
        with_nesting_stack(nesting, walk)
    }
}

/// Run `walk`, a walk of oxc's over syntax that nests as `nesting`
/// measured (a node's, or its program's), with the stack that can need.
pub fn with_nesting_stack<R>(nesting: Nesting, walk: impl FnOnce() -> R) -> R {
    let needed = (nesting.depth as usize)
        .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
        .saturating_add(PARSE_STACK_BASE_BYTES);
    stacker::maybe_grow(needed, needed, walk)
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
        with_ast_stack(source_text, source_type, move || inner.parse())
    }

    /// Parse the source as one expression.
    pub fn parse_expression(self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
        let Self {
            inner,
            source_text,
            source_type,
        } = self;
        with_ast_stack(source_text, source_type, move || inner.parse_expression())
    }
}

#[cfg(test)]
mod tests;
