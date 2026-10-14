//! Borrowed-form eval-program parse cell.
//!
//! `ParsedEvalProgram` owns an OXC allocator + source and the `Program`
//! AST parsed from them, as a `self_cell` owner/dependent pair so the
//! borrowed AST never outlives its arena. `ParsedEvalProgram::parse` is
//! the scheduler-bound parse entry for the borrowed lowering input (see
//! the `no_direct_oxc_parser_calls_outside_scheduler_path` architecture
//! guard).
//!
//! A walk of the retained program runs under a walk-stack lease; a refused
//! lease is returned BY VALUE ([`WalkStackRefused`], carried upward in a
//! [`WalkedRead`]) so the request-side consumer of the read — never this
//! module, which may run on a lowering worker — marks the result it feeds
//! partial.

use std::{cell::OnceCell, rc::Rc, sync::Arc};
use verter_semantic::analysis::function_program::{
    build_function_program_index_with_nodes, FunctionProgramNodes, ResolvedFunctionNode,
};
use verter_session_query::function_program::{FunctionProgramEntry, FunctionProgramIndex};

type CachedEvalProgramAst<'a> = oxc_ast::ast::Program<'a>;

/// A walk-stack lease for walks of a retained program was refused: the
/// walks did not run, and nothing they would have produced exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkStackRefused;

/// A read over a retained program together with whether a walk it depends
/// on was refused its walk-stack lease. The value of a refused read is the
/// fail-closed answer the source serves without caching it; the consumer
/// that folds the read into a request result marks that result partial.
/// A composition keeps the refusal of every read it consulted.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkedRead<T> {
    pub value: T,
    pub refusal: Option<WalkStackRefused>,
}

impl<T> WalkedRead<T> {
    /// A read whose walks all ran.
    #[inline]
    pub(crate) fn clean(value: T) -> Self {
        Self {
            value,
            refusal: None,
        }
    }

    /// A read whose walk was refused; `value` is its fail-closed answer.
    #[inline]
    pub(crate) fn refused(value: T) -> Self {
        Self {
            value,
            refusal: Some(WalkStackRefused),
        }
    }

    /// Transform the value, keeping the evidence.
    #[inline]
    pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> WalkedRead<U> {
        WalkedRead {
            value: f(self.value),
            refusal: self.refusal,
        }
    }

    /// Take the value, folding this read's evidence into `refusal`.
    #[inline]
    pub(crate) fn absorb_into(self, refusal: &mut Option<WalkStackRefused>) -> T {
        *refusal = refusal.or(self.refusal);
        self.value
    }

    /// Keep the evidence of a read consulted before this one.
    #[inline]
    pub(crate) fn with_prior_refusal(mut self, prior: Option<WalkStackRefused>) -> Self {
        self.refusal = prior.or(self.refusal);
        self
    }
}

impl<T> WalkedRead<Option<T>> {
    /// The answer of a lease-only run over a retained program: no program
    /// (or no answer) is a clean absence; a refused walk is a refused
    /// absence.
    #[inline]
    pub(crate) fn from_program_walk(answer: Option<Result<Option<T>, WalkStackRefused>>) -> Self {
        match answer {
            None => Self::clean(None),
            Some(Ok(value)) => Self::clean(value),
            Some(Err(WalkStackRefused)) => Self::refused(None),
        }
    }
}

struct ParsedEvalProgramOwner {
    allocator: oxc_allocator::Allocator,
    source: Arc<str>,
    source_type: oxc_span::SourceType,
}

self_cell::self_cell!(
    struct ParsedEvalProgramCell {
        owner: ParsedEvalProgramOwner,

        #[covariant]
        dependent: CachedEvalProgramAst,
    }
);

struct IndexedProgramFunctions<'a> {
    index: Arc<FunctionProgramIndex>,
    nodes: FunctionProgramNodes<'a>,
}

self_cell::self_cell!(
    struct IndexedProgramFunctionsCell {
        owner: Rc<ParsedEvalProgramCell>,
        #[covariant]
        dependent: IndexedProgramFunctions,
    }
);

/// A retained eval-program parse: the `self_cell` owner/dependent pair plus
/// the parse-outcome facts walkers need (`had_errors`).
pub struct ParsedEvalProgram {
    cell: Rc<ParsedEvalProgramCell>,
    functions: OnceCell<IndexedProgramFunctionsCell>,
    /// The program's nesting scan, shared by every walk stack over it
    /// (each nested function's skeleton and slice content), computed at
    /// most once and dropped with the program.
    nesting: OnceCell<verter_semantic::analysis::walk_stack::Nesting>,
    /// The parse produced RECOVERABLE errors (`ParserReturn::errors` was
    /// non-empty). An error-recovered AST can silently DROP real code, so
    /// provers of non-usage (e.g. macro-usage liveness) must fail open when
    /// this is set.
    had_errors: bool,
}

/// The first chunk an empty arena takes: `oxc_allocator`'s
/// `FIRST_ALLOCATION_GOAL` (16 KiB, `src/arena/create.rs`). Any chunk the
/// arena adds later is at least this size.
const ARENA_FIRST_CHUNK_BYTES: usize = 16 * 1024;

/// Arena bytes per source byte a parse needs at most for the dense
/// TypeScript a small file holds: declarations with method and function
/// signatures parse to 19 to 23 arena bytes per source byte, so this bound
/// leaves a margin above the densest.
const ARENA_BYTES_PER_SOURCE_BYTE_BOUND: usize = 32;

/// The arena a retained parse of `source_len` bytes allocates into.
///
/// The arena lives as long as the snapshot's lease, so its unused tail is
/// retained memory, and a 16 KiB first chunk is many times a small file's
/// AST. A file whose bounded AST fits below that chunk starts from a chunk
/// of that bound instead. The bound must hold: a parse that outgrows a
/// pre-sized chunk adds a full default chunk beside it and retains more
/// than an empty arena would have.
fn retained_arena(source_len: usize) -> oxc_allocator::Allocator {
    let bound = source_len.saturating_mul(ARENA_BYTES_PER_SOURCE_BYTE_BOUND);
    if bound < ARENA_FIRST_CHUNK_BYTES {
        oxc_allocator::Allocator::with_capacity(bound)
    } else {
        oxc_allocator::Allocator::new()
    }
}

impl ParsedEvalProgram {
    #[cfg(test)]
    pub(crate) fn parse(source: Arc<str>, source_type: oxc_span::SourceType) -> Option<Self> {
        Self::parse_outcome(source, source_type).ok()
    }

    /// Parse `source`: the program, or, for a fatal parse, the stack
    /// refusal it carries when it was refused for want of stack rather
    /// than for its syntax.
    pub(crate) fn parse_outcome(
        source: Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> Result<Self, Option<verter_parser::oxc_parse::StackUnavailable>> {
        verter_audit::attribute_n!(EvalProgramParse, source.len());
        let mut panicked = false;
        let mut refused = None;
        let mut had_errors = false;
        let cell = ParsedEvalProgramCell::new(
            ParsedEvalProgramOwner {
                allocator: retained_arena(source.len()),
                source,
                source_type,
            },
            |owner| {
                let result = verter_parser::oxc_parse::Parser::new(
                    &owner.allocator,
                    owner.source.as_ref(),
                    owner.source_type,
                )
                .with_options(oxc_parser::ParseOptions {
                    parse_regular_expression: false,
                    ..oxc_parser::ParseOptions::default()
                })
                .parse();
                panicked = result.fatal_error;
                refused = verter_parser::oxc_parse::parse_refusal(&result);
                had_errors = !result.diagnostics.is_empty();
                // The retained arena is the dominant per-file live footprint;
                // `used_bytes()` walks the chunk list, which is why the amount
                // must never be evaluated when attribution is off.
                verter_audit::attribute_n!(ParseArenaUsed, owner.allocator.used_bytes());
                verter_audit::attribute_max!(ParseArenaCapacity, owner.allocator.capacity());
                result.program
            },
        );
        if panicked {
            return Err(refused);
        }
        Ok(Self {
            cell: Rc::new(cell),
            functions: OnceCell::new(),
            nesting: OnceCell::new(),
            had_errors,
        })
    }

    /// The parsed program AST, borrowed from the retained arena.
    pub fn borrow_dependent(&self) -> &CachedEvalProgramAst<'_> {
        self.cell.borrow_dependent()
    }

    /// Demand the content-free index and register arena addresses once under
    /// this retained parse owner. Neither the arena nor its node table leaves it.
    ///
    /// The index's walks of the program run under a walk-stack lease for it,
    /// the one fallible step: a refused lease is [`WalkStackRefused`], returned
    /// for the operation around it, and leaves nothing indexed, so a later
    /// demand whose lease is granted indexes the program.
    pub fn function_program_index(
        &self,
        owners: &verter_session_query::analysis::top_level_owners::TopLevelOwnerTable,
        canonical: Arc<str>,
        parse_env_hash: &verter_session_query::analysis::types::Hash16,
        class_fields: &verter_session_query::declarations::class_fields::ClassFieldValues,
    ) -> Result<Arc<FunctionProgramIndex>, WalkStackRefused> {
        if let Some(cell) = self.functions.get() {
            return Ok(Arc::clone(&cell.borrow_dependent().index));
        }
        self.leased(|| self.index_functions(owners, canonical, parse_env_hash, class_fields))
    }

    /// Run `walks`, walks of this program, under a walk-stack lease for it,
    /// sized from the program's one shared scan: the lease is the walks'
    /// one fallible step. A refused lease is [`WalkStackRefused`], returned by
    /// value: the result that would have read the walks is partial, so no
    /// cache retains what was computed without them, and the request-side
    /// consumer of the read marks it so.
    fn leased<R>(&self, walks: impl FnOnce() -> R) -> Result<R, WalkStackRefused> {
        let program = self.borrow_dependent();
        let nesting = *self.nesting.get_or_init(|| {
            verter_parser::oxc_parse::syntax_nesting(program.source_text, program.source_type)
        });
        verter_parser::oxc_parse::with_walk_stack_lease(nesting, walks)
            .map_err(|_| WalkStackRefused)
    }

    fn index_functions(
        &self,
        owners: &verter_session_query::analysis::top_level_owners::TopLevelOwnerTable,
        canonical: Arc<str>,
        parse_env_hash: &verter_session_query::analysis::types::Hash16,
        class_fields: &verter_session_query::declarations::class_fields::ClassFieldValues,
    ) -> Arc<FunctionProgramIndex> {
        let cell = self.functions.get_or_init(|| {
            IndexedProgramFunctionsCell::new(Rc::clone(&self.cell), |owner| {
                let (index, nodes) = build_function_program_index_with_nodes(
                    owner.borrow_dependent(),
                    owner.borrow_owner().source.as_ref(),
                    owners,
                    canonical,
                    class_fields,
                );
                IndexedProgramFunctions {
                    index: Arc::new(crate::decl_body_memo::fold_flow_body_env_identity(
                        &index,
                        parse_env_hash,
                        owner.borrow_owner().source_type,
                    )),
                    nodes,
                }
            })
        });
        Arc::clone(&cell.borrow_dependent().index)
    }

    /// Execute a pure lowerer against one exact retained function address.
    /// The higher-ranked callback returns only owned output, never an arena borrow.
    /// The containment of walks over this program, sharing its one scan.
    pub(crate) fn walk_stack(&self) -> verter_semantic::analysis::walk_stack::ProgramWalkStack<'_> {
        verter_semantic::analysis::walk_stack::ProgramWalkStack::sharing(
            self.borrow_dependent(),
            &self.nesting,
        )
    }

    pub(crate) fn with_indexed_function<R>(
        &self,
        entry: &FunctionProgramEntry,
        lower: impl for<'a> FnOnce(ResolvedFunctionNode<'a>, &'a FunctionProgramEntry) -> R,
    ) -> Result<Option<R>, WalkStackRefused> {
        let Some(cell) = self.functions.get() else {
            return Ok(None);
        };
        let retained = cell.borrow_dependent();
        let Some(indexed) = retained
            .index
            .get(entry.key())
            .map(|matched| matched.entry())
        else {
            return Ok(None);
        };
        if indexed.locator() != entry.locator()
            || indexed.span() != entry.span()
            || indexed.body_span() != entry.body_span()
            || indexed.flow_body_exact_hash() != entry.flow_body_exact_hash()
        {
            return Ok(None);
        }
        let Some(node) = retained.nodes.get(entry.key()) else {
            return Ok(None);
        };
        self.leased(|| lower(node, indexed)).map(Some)
    }

    /// Lower the indexed call, `new` or tagged template addressed by `span`.
    pub(crate) fn with_indexed_call_site<R>(
        &self,
        span: verter_span::Span,
        lower: impl for<'a> FnOnce(
            verter_semantic::analysis::function_program::IndexedCallSite<'a>,
        ) -> R,
    ) -> Result<Option<R>, WalkStackRefused> {
        let Some(cell) = self.functions.get() else {
            return Ok(None);
        };
        let Some(site) = cell.borrow_dependent().nodes.call_site(span) else {
            return Ok(None);
        };
        self.leased(|| lower(site)).map(Some)
    }

    /// Whether the parse recovered from errors (`ParserReturn::errors`
    /// non-empty). See the field docs — non-usage provers fail open on this.
    pub fn had_errors(&self) -> bool {
        self.had_errors
    }

    /// The exact source text this program was parsed from — for a
    /// `.vue` eval program, the position-preserving extracted script
    /// (script bytes at raw SFC offsets), so every span the program
    /// carries is already SFC-absolute.
    pub fn source_str(&self) -> &str {
        self.cell.borrow_owner().source.as_ref()
    }

    /// The `SourceType` the parse ran under — the self-consistent type
    /// for any walker consuming this program.
    pub fn source_type(&self) -> oxc_span::SourceType {
        self.cell.borrow_owner().source_type
    }

    /// Bytes the retained arena holds, used or not.
    #[cfg(test)]
    fn arena_capacity(&self) -> usize {
        self.cell.borrow_owner().allocator.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capacity an empty arena holds once it has allocated.
    fn default_capacity() -> usize {
        let probe = oxc_allocator::Allocator::new();
        probe.alloc(0u8);
        probe.capacity()
    }

    fn retained_capacity(source: String) -> usize {
        ParsedEvalProgram::parse(Arc::from(source), oxc_span::SourceType::ts())
            .expect("the fixture parses")
            .arena_capacity()
    }

    /// A small file never retains more arena than an empty arena would have
    /// taken, whatever its density: a pre-sized chunk the parse outgrows
    /// would keep both itself and the default chunk added beside it.
    #[test]
    fn a_small_file_never_retains_more_arena_than_the_default_chunk() {
        let default_capacity = default_capacity();
        let interfaces = |n: usize| {
            (0..n)
                .map(|i| {
                    format!("export interface I{i} {{ a{i}: string; b{i}?: number; c{i}(x: number): void }}\n")
                })
                .collect::<String>()
        };
        let functions = |n: usize| {
            (0..n)
                .map(|i| {
                    format!("export function f{i}(a: number, b: string): number {{ return a; }}\n")
                })
                .collect::<String>()
        };
        for source in [
            interfaces(1),
            interfaces(5),
            interfaces(8),
            functions(4),
            functions(8),
        ] {
            let len = source.len();
            let capacity = retained_capacity(source);
            assert!(
                capacity <= default_capacity,
                "a {len}-byte file retains {capacity} arena bytes, above the {default_capacity} an empty arena takes"
            );
        }
    }

    /// A declaration-only file small enough to pre-size retains a chunk of
    /// its bound, not the default chunk.
    #[test]
    fn a_tiny_declaration_file_retains_less_than_the_default_chunk() {
        let default_capacity = default_capacity();
        let capacity =
            retained_capacity("interface BenchRegistry { r0: { id: 0; tag: \"t0\" } }\n".into());
        assert!(
            capacity * 4 < default_capacity,
            "a one-line declaration retains {capacity} arena bytes of {default_capacity}"
        );
    }
}
