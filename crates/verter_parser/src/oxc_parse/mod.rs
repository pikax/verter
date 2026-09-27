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

/// The most native stack oxc 0.151's parser, or a walk of oxc's over what
/// it parsed, spends per level of [`nesting`]'s bound, with twice the
/// margin. Measured on an unoptimized build (the larger frames): a parse
/// level takes at most about 3.4 KiB (an object literal's; a type argument
/// 3.2 KiB, a parenthesis 3.0 KiB, a `!` 0.2 KiB), `clone_in`, the
/// costliest walk, 4.4 KiB (an object literal's or a type argument's), the
/// semantic builder 1.1 KiB and a `Visit` walk 0.6 KiB; an optimized build
/// spends at most 2.4 KiB (`examples/oxc_deep_parse.rs` in this crate,
/// `examples/oxc_deep_walk.rs` in `verter_semantic`).
pub const PARSE_STACK_BYTES_PER_LEVEL: usize = 9 * 1024;

/// Stack the parse needs besides its per-level recursion.
const PARSE_STACK_BASE_BYTES: usize = 512 * 1024;

/// An upper bound of how deeply `source`'s syntax tree nests (see
/// [`nesting`]).
pub fn syntax_nesting(source: &str, source_type: SourceType) -> Nesting {
    #[cfg(test)]
    scan_probe::scanned(source.len());
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
///
/// A program walked by many separate walk stacks (each nested function's
/// body lowered on its own) shares one scan through a cell its owner keeps
/// ([`Self::sharing`]): scanning the program again for every nested
/// function made the walks' containment cost the square of the nesting.
pub struct ProgramWalkStack<'p> {
    program: &'p Program<'p>,
    nesting: ProgramNesting<'p>,
    /// Whether the walks run inside [`Self::within`]'s containment, sized
    /// for the whole program, which bounds a walk of any node in it.
    inside: std::cell::Cell<bool>,
}

/// Where a [`ProgramWalkStack`] keeps its program's scan.
enum ProgramNesting<'p> {
    Owned(std::cell::OnceCell<Nesting>),
    Shared(&'p std::cell::OnceCell<Nesting>),
}

impl<'p> ProgramWalkStack<'p> {
    pub fn new(program: &'p Program<'p>) -> Self {
        Self {
            program,
            nesting: ProgramNesting::Owned(std::cell::OnceCell::new()),
            inside: std::cell::Cell::new(false),
        }
    }

    /// The containment of walks over `program`, its scan kept in `nesting`
    /// (owned with the program, so every walk stack over it scans it at
    /// most once).
    pub fn sharing(program: &'p Program<'p>, nesting: &'p std::cell::OnceCell<Nesting>) -> Self {
        Self {
            program,
            nesting: ProgramNesting::Shared(nesting),
            inside: std::cell::Cell::new(false),
        }
    }

    /// Run `work` on `owner`, whose walks of nodes of the program run under
    /// the containment `stack` reads from it, on the stack the whole
    /// program can need: every walk inside it runs in place, where each
    /// would otherwise take a stack segment sized for the program of its
    /// own.
    pub fn within<T, R>(
        owner: &mut T,
        stack: impl Fn(&T) -> &ProgramWalkStack<'_>,
        work: impl FnOnce(&mut T) -> R,
    ) -> R {
        let walks = stack(owner);
        if walks.inside.get() {
            return work(owner);
        }
        let nesting = walks.program_nesting();
        with_nesting_stack(nesting, move || {
            stack(owner).inside.set(true);
            let result = work(owner);
            stack(owner).inside.set(false);
            result
        })
    }

    fn program_nesting(&self) -> Nesting {
        let cell = match &self.nesting {
            ProgramNesting::Owned(cell) => cell,
            ProgramNesting::Shared(cell) => *cell,
        };
        *cell.get_or_init(|| syntax_nesting(self.program.source_text, self.program.source_type))
    }

    /// The program whose nodes walk under this containment.
    pub fn program(&self) -> &'p Program<'p> {
        self.program
    }

    /// Run `walk`, a walk of oxc's over the node at `span`, with the stack
    /// it can need.
    pub fn with_node_stack<R>(&self, span: Span, walk: impl FnOnce() -> R) -> R {
        if self.inside.get() {
            return walk();
        }
        let by_length = (span.size() as usize)
            .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
            .saturating_add(PARSE_STACK_BASE_BYTES);
        if stacker::remaining_stack().is_some_and(|remaining| remaining >= by_length) {
            return walk();
        }
        with_nesting_stack(self.program_nesting(), walk)
    }
}

/// [`with_span_stack`] for a walk of the node at `span` in `source_text`
/// that does not enter the bodies at `nested` (spans in the same text, the
/// bodies of the functions nested in the node): the stack is sized from the
/// node's text with each of those bodies taken out, so a function's walk
/// costs its own syntax only, not every function nested inside it.
pub fn with_own_syntax_stack<R>(
    source_text: &str,
    span: Span,
    nested: impl IntoIterator<Item = Span>,
    walk: impl FnOnce() -> R,
) -> R {
    match own_syntax_text(source_text, span, nested) {
        Some(own) => with_source_stack(&own, walk),
        None => with_source_stack(source_text, walk),
    }
}

/// The text of the node at `span` with each body at `nested` taken out
/// (see [`with_own_syntax_stack`]); `None` when the span lies outside
/// the text.
fn own_syntax_text(
    source_text: &str,
    span: Span,
    nested: impl IntoIterator<Item = Span>,
) -> Option<String> {
    let text = source_text.get(span.start as usize..span.end as usize)?;
    let mut nested: Vec<Span> = nested
        .into_iter()
        .filter(|inner| {
            inner.start >= span.start && inner.end <= span.end && inner.start < inner.end
        })
        .collect();
    nested.sort_by_key(|inner| inner.start);
    // Each body taken out leaves one token (`0`), the syntax around it
    // unchanged.
    let mut own = String::with_capacity(text.len());
    let mut at = span.start;
    for inner in nested {
        if inner.start < at {
            continue;
        }
        own.push_str(&source_text[at as usize..inner.start as usize]);
        own.push('0');
        at = inner.end;
    }
    own.push_str(&source_text[at as usize..span.end as usize]);
    Some(own)
}

/// Counts the bytes [`syntax_nesting`] scans on this thread; test-only.
#[cfg(test)]
pub(crate) mod scan_probe {
    use std::cell::Cell;
    thread_local! {
        static SCANNED: Cell<usize> = const { Cell::new(0) };
    }
    pub(crate) fn scanned(bytes: usize) {
        SCANNED.with(|count| count.set(count.get() + bytes));
    }
    /// The bytes scanned since the last call.
    pub(crate) fn take() -> usize {
        SCANNED.with(|count| count.replace(0))
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
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
    options: ParseOptions,
}

impl<'a> Parser<'a> {
    pub fn new(allocator: &'a Allocator, source_text: &'a str, source_type: SourceType) -> Self {
        Self {
            inner: oxc_parser::Parser::new(allocator, source_text, source_type),
            allocator,
            source_text,
            source_type,
            options: ParseOptions::default(),
        }
    }
}

impl<'a, C: ParserConfig> Parser<'a, C> {
    #[must_use]
    pub fn with_options(self, options: ParseOptions) -> Self {
        Self {
            inner: self.inner.with_options(options),
            options,
            ..self
        }
    }

    #[must_use]
    pub fn with_config<D: ParserConfig>(self, config: D) -> Parser<'a, D> {
        Parser {
            inner: self.inner.with_config(config),
            allocator: self.allocator,
            source_text: self.source_text,
            source_type: self.source_type,
            options: self.options,
        }
    }

    /// Parse the source as a program.
    pub fn parse(self) -> ParserReturn<'a> {
        let Self {
            inner,
            source_text,
            source_type,
            ..
        } = self;
        with_ast_stack(source_text, source_type, move || inner.parse())
    }

    /// Parse the source as one expression: the expression at its start, as
    /// oxc parsed it before 0.151, leaving any content after it to the
    /// caller. oxc 0.151's `parse_expression` instead rejects a source with
    /// content after the expression — a lone `Unexpected token` at that
    /// content — so that case parses the source before the token.
    pub fn parse_expression(self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
        let Self {
            inner,
            allocator,
            source_text,
            source_type,
            options,
        } = self;
        with_ast_stack(source_text, source_type, move || {
            let diagnostics = match inner.parse_expression() {
                Ok(expression) => return Ok(expression),
                Err(diagnostics) => diagnostics,
            };
            match trailing_content_start(source_text, &diagnostics) {
                Some(end) => oxc_parser::Parser::new(allocator, &source_text[..end], source_type)
                    .with_options(options)
                    .parse_expression()
                    .map_err(Vec::from),
                None => Err(diagnostics.into_vec()),
            }
        })
    }
}

/// Where the content after a parsed expression starts, when `diagnostics`
/// is oxc's rejection of that content alone: one `Unexpected token` at it.
fn trailing_content_start(source_text: &str, diagnostics: &[OxcDiagnostic]) -> Option<usize> {
    let [diagnostic] = diagnostics else {
        return None;
    };
    let [label] = diagnostic.labels.as_slice() else {
        return None;
    };
    let start = label.offset() as usize;
    (diagnostic.message == "Unexpected token"
        && start > 0
        && start < source_text.len()
        && source_text.is_char_boundary(start))
    .then_some(start)
}

#[cfg(test)]
mod tests;
