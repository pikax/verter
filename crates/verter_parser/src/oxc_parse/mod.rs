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
//! otherwise it runs on a stack region reserved for it ([`stack`]), which
//! commits only the stack the parse touches. Where a stack can grow no
//! depth is too deep: the region is sized from the source. A host whose
//! engine keeps a call stack of its own, which nothing grows (a
//! WebAssembly engine), parses under that engine's measured
//! [`EngineStackProfile`], and a source nesting past it is not parsed;
//! nor, anywhere, is a source whose region cannot be reserved. Such a
//! parse returns an empty program, marked fatal, whose one diagnostic is
//! [`stack_unavailable_diagnostic`]'s: typed operational incompleteness,
//! not a syntax error.
//!
//! oxc's own walks over what it parsed (`clone_in`, the semantic builder,
//! the `Visit` and `VisitMut` walkers) recurse once per level of the same
//! syntax tree, spending less per level than the parse. Every call into
//! one runs under the same containment: [`with_program_stack`] for a walk
//! of a whole program, [`with_node_stack`] for a walk of one node of it,
//! [`with_ast_stack`] for a walk of a node given its source text, and
//! [`with_nesting_stack`] where the caller already holds the [`Nesting`]
//! of what it walks. An operation pays for their stack once, at its
//! boundary: [`with_program_walk_stack_lease`] reserves the region its
//! walks can need before any begins, the one step that can fail, and
//! every walk inside it runs on that region without reserving again.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Program};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::config::{NoTokensParserConfig, ParserConfig};
use oxc_parser::{ParseOptions, ParserReturn};
use oxc_span::{SourceType, Span};

mod nesting;
mod stack;

pub use nesting::Nesting;
#[cfg(any(test, feature = "stack-fault-injection"))]
pub use stack::faults;
pub use stack::{refusals_within, StackUnavailable};

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

/// The stack a parse or walk of syntax nesting `depth` levels deep can
/// need.
fn stack_bytes(depth: usize) -> usize {
    depth
        .saturating_mul(PARSE_STACK_BYTES_PER_LEVEL)
        .saturating_add(PARSE_STACK_BASE_BYTES)
}

/// The stack a parse of `source_text` can need.
pub fn parse_stack_bytes(source_text: &str, source_type: SourceType) -> usize {
    stack_bytes(syntax_nesting(source_text, source_type).depth as usize)
}

/// A host whose engine runs the module's calls on a call stack of its own,
/// which no stack region grows and nothing in the module can read: a
/// WebAssembly engine. Its profile is a measured runtime safety profile of
/// oxc's recursion on that engine, for the oxc version, build and engine it
/// was measured on, not a limit of the language: a source nesting past
/// [`Self::nesting`] is not parsed there, as typed operational
/// incompleteness, and every host whose stack can grow parses any depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineStackProfile {
    /// The profile's name.
    pub name: &'static str,
    /// The engine and its configuration the profile was measured on.
    pub engine: &'static str,
    /// The `oxc_parser` version the profile was measured against.
    pub oxc_version: &'static str,
    /// The build the profile was measured on.
    pub build: &'static str,
    /// The call stack the engine gives the module.
    pub stack_bytes: usize,
    /// The call stack kept for the frames around a parse: the host's own
    /// and the module's callers of the parse.
    pub reserved_bytes: usize,
    /// The most call stack oxc's parser, or a walk of oxc's over what it
    /// parsed, spends per level of [`nesting`]'s bound, with twice the
    /// margin.
    pub bytes_per_level: usize,
}

impl EngineStackProfile {
    /// How deeply the scan's bound may nest for oxc to parse and walk the
    /// source within the engine's stack.
    pub const fn nesting(&self) -> usize {
        (self.stack_bytes - self.reserved_bytes) / self.bytes_per_level
    }

    /// Whether syntax whose scan bound is `depth` fits the engine's stack.
    pub const fn admits(&self, depth: usize) -> bool {
        depth <= self.nesting()
    }
}

/// V8 at its default call stack (`--stack-size`, 984 KiB), the stack of
/// Node.js and of Chromium's threads, for the optimized wasm32 module:
/// measured under Node.js 26.5.0 (V8 14.6), a level of an object literal
/// takes 1,188 bytes of engine stack, the costliest form (a parenthesis
/// 1,034, a type argument 1,034, an array 1,159, a template hole 639 a
/// level, a `!` 104), and no level of `clone_in`, `Visit` or the semantic
/// builder takes more; the module's callers of the parse, from
/// `VerterHost.upsert` down, take under 16 KiB of the 256 KiB kept
/// (`docs/evidence/signature-kernel/oxc-deep-parse.md`). Other engines
/// (SpiderMonkey, JavaScriptCore, wasmtime) have not been measured and
/// carry no profile of their own.
pub const V8_DEFAULT_STACK_PROFILE: EngineStackProfile = EngineStackProfile {
    name: "v8-default-stack",
    engine: "V8 14.6 (Node.js 26.5.0), default --stack-size (984 KiB)",
    oxc_version: "0.151.0",
    build: "wasm32-unknown-unknown, release profile (opt-level 3, lto, one codegen unit)",
    stack_bytes: 984 * 1024,
    reserved_bytes: 256 * 1024,
    bytes_per_level: 2 * 1188,
};

/// The engine-stack profile this build parses under: on wasm32 the V8
/// profile, the runtime contract of the WebAssembly host (Node.js and
/// Chromium); `None` everywhere a stack region grows to what a source
/// needs, where no nesting is refused.
pub const HOST_ENGINE_STACK_PROFILE: Option<EngineStackProfile> = if cfg!(target_arch = "wasm32") {
    Some(V8_DEFAULT_STACK_PROFILE)
} else {
    None
};

/// Whether syntax whose scan bound is `depth` fits the engine stack under
/// `profile`.
fn within_engine_stack(profile: Option<EngineStackProfile>, depth: usize) -> bool {
    profile.is_none_or(|profile| profile.admits(depth))
}

/// Every level of nesting takes at least one byte of source, so a source
/// whose every byte could be a level fits the thread's remaining stack
/// without the scan: the many short sources (a template's expressions, a
/// synthesized wrapper) parse and walk in place at once.
fn fits_by_length(length: usize) -> bool {
    fits_by_length_under(HOST_ENGINE_STACK_PROFILE, length)
}

fn fits_by_length_under(profile: Option<EngineStackProfile>, length: usize) -> bool {
    within_engine_stack(profile, length)
        && stack::remaining().is_some_and(|remaining| remaining >= stack_bytes(length))
}

/// Whether a walk of a source `length` bytes long runs without a scan: in
/// place, its every byte a level fitting the thread's stack, or on the
/// region of a walk-stack lease that covers as much.
fn covered_by_length(length: usize) -> bool {
    fits_by_length(length) || stack::lease_covers(stack_bytes(length))
}

/// Run `parse`, a parse of `source_text`, with at least
/// [`parse_stack_bytes`] of stack: in place when the thread has it, on the
/// walk-stack lease's region when the thread holds one covering it, on a
/// region reserved for it otherwise. A source nesting deeper than a stack
/// this host can provide is not parsed.
fn parse_with_stack<R>(
    source_text: &str,
    source_type: SourceType,
    parse: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    parse_with_stack_under(HOST_ENGINE_STACK_PROFILE, source_text, source_type, parse)
}

/// [`parse_with_stack`] under an engine-stack profile.
fn parse_with_stack_under<R>(
    profile: Option<EngineStackProfile>,
    source_text: &str,
    source_type: SourceType,
    parse: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    if fits_by_length_under(profile, source_text.len()) {
        return Ok(parse());
    }
    let depth = syntax_nesting(source_text, source_type).depth as usize;
    if let Some(profile) = profile.filter(|profile| !profile.admits(depth)) {
        return Err(stack::record_refusal(StackUnavailable {
            needed: depth.saturating_mul(profile.bytes_per_level),
        }));
    }
    stack::with_stack(stack_bytes(depth), stack::Reservation::Parse, parse)
        .map_err(stack::record_refusal)
}

/// Run `walk`, a walk of oxc's over parsed syntax, with at least `needed`
/// bytes of stack: in place, or on the region of the walk-stack lease the
/// operation around it holds ([`with_walk_stack_lease`]), which cannot
/// fail. A walk no lease covers reserves a region of its own; the parse of
/// the syntax had as much or more, so that reservation fails only on an
/// address space exhausted since, which ends the process as any failed
/// allocation does.
#[track_caller]
fn walk_with_stack<R>(needed: usize, walk: impl FnOnce() -> R) -> R {
    match stack::with_stack(needed, stack::Reservation::Walk, walk) {
        Ok(result) => result,
        Err(unavailable) => std::alloc::handle_alloc_error(
            std::alloc::Layout::from_size_align(unavailable.needed.max(1), 16)
                .unwrap_or(std::alloc::Layout::new::<u128>()),
        ),
    }
}

/// Run `walk` holding a walk-stack lease of `needed` bytes, the walk's own
/// stack, so the walk reserves nothing past it. A refused lease is
/// [`StackUnavailable`] and `walk` does not run: the refusal is recorded
/// for the operation around the walk that records its refusals
/// ([`refusals_within`]), which reports it as typed incompleteness. Test
/// builds record by its call site a leased walk that could reserve while no
/// operation records ([`faults::take_unleased_walks`]): its refusal would
/// reach no operation.
#[track_caller]
fn leased_walk<R>(needed: usize, walk: impl FnOnce() -> R) -> Result<R, StackUnavailable> {
    #[cfg(any(test, feature = "stack-fault-injection"))]
    if !stack::recording() && !stack::lease_covers(needed) {
        stack::faults::unleased_walk(std::panic::Location::caller());
    }
    stack::with_walk_stack_lease(needed, || walk_with_stack(needed, walk))
}

/// [`with_ast_stack`] under a walk-stack lease of its own ([`leased_walk`]):
/// the walk is its operation's boundary.
#[track_caller]
pub fn leased_ast_walk<R>(
    source_text: &str,
    source_type: SourceType,
    walk: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    let needed = if covered_by_length(source_text.len()) {
        stack_bytes(source_text.len())
    } else {
        parse_stack_bytes(source_text, source_type)
    };
    leased_walk(needed, walk)
}

/// [`with_program_stack`] under a walk-stack lease of its own
/// ([`leased_walk`]): the walk is its operation's boundary.
#[track_caller]
pub fn leased_program_walk<R>(
    program: &Program<'_>,
    walk: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    leased_ast_walk(program.source_text, program.source_type, walk)
}

/// [`with_span_stack`] under a walk-stack lease of its own
/// ([`leased_walk`]): the walk is its operation's boundary.
#[track_caller]
pub fn leased_span_walk<R>(
    source_text: &str,
    span: Span,
    walk: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    let text = source_text
        .get(span.start as usize..span.end as usize)
        .unwrap_or(source_text);
    let needed = if covered_by_length(text.len()) {
        stack_bytes(text.len())
    } else {
        stack_bytes(
            syntax_nesting(text, SourceType::ts())
                .depth
                .max(syntax_nesting(text, SourceType::tsx()).depth) as usize,
        )
    };
    leased_walk(needed, walk)
}

/// Run `operation` holding a walk-stack lease for syntax nesting as
/// `nesting` measured: a region reserved, before `operation` begins, for
/// the stack every walk of oxc's inside it can need, which each such walk
/// runs on without reserving again. The lease is the operation's one
/// fallible step: when its region cannot be reserved, `operation` does not
/// run and the result is [`StackUnavailable`], typed operational
/// incompleteness. A thread already holding a lease that covers `nesting`,
/// or whose own stack does, reserves nothing.
pub fn with_walk_stack_lease<R>(
    nesting: Nesting,
    operation: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    stack::with_walk_stack_lease(stack_bytes(nesting.depth as usize), operation)
}

/// [`with_walk_stack_lease`] for an operation over `program`'s syntax: its
/// walks of the whole program, or of any node in it.
pub fn with_program_walk_stack_lease<R>(
    program: &Program<'_>,
    operation: impl FnOnce() -> R,
) -> Result<R, StackUnavailable> {
    // A program short enough that every byte could be a level fits the
    // thread's stack by its length, and leases it without the scan.
    let length = program.source_text.len();
    let needed = if fits_by_length(length) {
        stack_bytes(length)
    } else {
        parse_stack_bytes(program.source_text, program.source_type)
    };
    stack::with_walk_stack_lease(needed, operation)
}

/// The diagnostic a parse returns for a source nesting deeper than a stack
/// this host can provide, in place of the program it did not parse.
pub fn stack_unavailable_diagnostic(unavailable: StackUnavailable) -> OxcDiagnostic {
    OxcDiagnostic::error(unavailable.to_string())
        .with_error_code(STACK_UNAVAILABLE_SCOPE, STACK_UNAVAILABLE_CODE)
        .with_note(unavailable.needed.to_string())
}

/// The refusal a parse returned in place of its program: the
/// [`StackUnavailable`] its [`stack_unavailable_diagnostic`] carries, when
/// the parse is fatal and carries one.
pub fn parse_refusal(parsed: &ParserReturn<'_>) -> Option<StackUnavailable> {
    if !parsed.fatal_error {
        return None;
    }
    diagnostics_refusal(parsed.diagnostics.errors())
}

/// The [`StackUnavailable`] a [`stack_unavailable_diagnostic`] among
/// `diagnostics` carries.
pub fn diagnostics_refusal<'d>(
    diagnostics: impl IntoIterator<Item = &'d OxcDiagnostic>,
) -> Option<StackUnavailable> {
    diagnostics
        .into_iter()
        .find(|diagnostic| is_stack_unavailable(diagnostic))
        .map(|diagnostic| StackUnavailable {
            needed: diagnostic
                .note
                .as_deref()
                .and_then(|needed| needed.parse().ok())
                .unwrap_or(usize::MAX),
        })
}

/// Whether `diagnostic` is [`stack_unavailable_diagnostic`]'s: the parse
/// did not run, and its empty program is operational incompleteness, not
/// the source's syntax.
pub fn is_stack_unavailable(diagnostic: &OxcDiagnostic) -> bool {
    diagnostic.code.scope.as_deref() == Some(STACK_UNAVAILABLE_SCOPE)
        && diagnostic.code.number.as_deref() == Some(STACK_UNAVAILABLE_CODE)
}

const STACK_UNAVAILABLE_SCOPE: &str = "verter";
const STACK_UNAVAILABLE_CODE: &str = "stack-unavailable";

/// Run `walk`, a walk of oxc's over syntax parsed from `source_text`, with
/// at least [`parse_stack_bytes`] of stack: in place when the thread has
/// it, on a new stack segment otherwise.
#[track_caller]
pub fn with_ast_stack<R>(
    source_text: &str,
    source_type: SourceType,
    walk: impl FnOnce() -> R,
) -> R {
    if covered_by_length(source_text.len()) {
        return walk_with_stack(stack_bytes(source_text.len()), walk);
    }
    scanned_walk(parse_stack_bytes(source_text, source_type), walk)
}

/// [`with_ast_stack`] for a walk of `program`, or of any node in it.
#[track_caller]
pub fn with_program_stack<R>(program: &Program<'_>, walk: impl FnOnce() -> R) -> R {
    with_ast_stack(program.source_text, program.source_type, walk)
}

/// [`with_ast_stack`] for a walk of the node at `span` in `program`,
/// sized from the node's own text: a short node walks in place at once, and
/// a long one costs a scan of its text, no more than the walk itself.
#[track_caller]
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
#[track_caller]
pub fn with_source_stack<R>(source_text: &str, walk: impl FnOnce() -> R) -> R {
    if covered_by_length(source_text.len()) {
        return walk_with_stack(stack_bytes(source_text.len()), walk);
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
#[track_caller]
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
    #[track_caller]
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
    #[track_caller]
    pub fn with_node_stack<R>(&self, span: Span, walk: impl FnOnce() -> R) -> R {
        if self.inside.get() {
            return walk();
        }
        if covered_by_length(span.size() as usize) {
            return walk_with_stack(stack_bytes(span.size() as usize), walk);
        }
        with_nesting_stack(self.program_nesting(), walk)
    }
}

/// [`with_span_stack`] for a walk of the node at `span` in `source_text`
/// that does not enter the bodies at `nested` (spans in the same text, the
/// bodies of the functions nested in the node): the stack is sized from the
/// node's text with each of those bodies taken out, so a function's walk
/// costs its own syntax only, not every function nested inside it.
#[track_caller]
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
#[track_caller]
pub fn with_nesting_stack<R>(nesting: Nesting, walk: impl FnOnce() -> R) -> R {
    scanned_walk(stack_bytes(nesting.depth as usize), walk)
}

/// [`walk_with_stack`] for a walk too long to fit the thread's stack by its
/// length, sized from its scan: one that can reserve a region when no
/// lease covers it, which test builds record by its call site
/// ([`faults::take_unleased_walks`]) whether or not this thread's stack
/// has the bytes.
#[track_caller]
fn scanned_walk<R>(needed: usize, walk: impl FnOnce() -> R) -> R {
    #[cfg(any(test, feature = "stack-fault-injection"))]
    if !stack::lease_covers(needed) {
        stack::faults::unleased_walk(std::panic::Location::caller());
    }
    walk_with_stack(needed, walk)
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

    /// Parse the source as a program. A source nesting deeper than a stack
    /// this host can provide is not parsed: its program is empty and its
    /// one diagnostic is [`stack_unavailable_diagnostic`]'s.
    pub fn parse(self) -> ParserReturn<'a> {
        let Self {
            inner,
            allocator,
            source_text,
            source_type,
            options,
        } = self;
        parse_with_stack(source_text, source_type, move || inner.parse())
            .unwrap_or_else(|unavailable| unparsed(allocator, source_type, options, unavailable))
    }

    /// Parse the source as one expression: the expression at its start, as
    /// oxc parsed it before 0.151, leaving any content after it to the
    /// caller. oxc 0.151's `parse_expression` instead rejects a source with
    /// content after the expression — a lone `Unexpected token` at that
    /// content — so that case parses the source before the token. A source
    /// nesting deeper than a stack this host can provide is not parsed: its
    /// one diagnostic is [`stack_unavailable_diagnostic`]'s.
    pub fn parse_expression(self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
        let Self {
            inner,
            allocator,
            source_text,
            source_type,
            options,
        } = self;
        parse_with_stack(source_text, source_type, move || {
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
        .unwrap_or_else(|unavailable| Err(vec![stack_unavailable_diagnostic(unavailable)]))
    }
}

/// What a parse that could not have its stack returns: an empty program
/// whose one diagnostic is [`stack_unavailable_diagnostic`]'s.
fn unparsed<'a>(
    allocator: &'a Allocator,
    source_type: SourceType,
    options: ParseOptions,
    unavailable: StackUnavailable,
) -> ParserReturn<'a> {
    let mut empty = oxc_parser::Parser::new(allocator, "", source_type)
        .with_options(options)
        .parse();
    empty
        .diagnostics
        .push(stack_unavailable_diagnostic(unavailable));
    // The program is not the source's: nothing may be read from it.
    empty.fatal_error = true;
    empty
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
