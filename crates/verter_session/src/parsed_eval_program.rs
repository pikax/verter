//! Borrowed-form eval-program parse cell.
//!
//! `ParsedEvalProgram` owns an OXC allocator + source and the `Program`
//! AST parsed from them, as a `self_cell` owner/dependent pair so the
//! borrowed AST never outlives its arena. `ParsedEvalProgram::parse` is
//! the scheduler-bound parse entry for the borrowed lowering input (see
//! the `no_direct_oxc_parser_calls_outside_scheduler_path` architecture
//! guard); consumers reach the cell through the crate-root re-export
//! (`crate::ParsedEvalProgram`).

use std::{cell::OnceCell, rc::Rc, sync::Arc};
use verter_semantic::analysis::function_program::{
    build_function_program_index_with_nodes, FunctionProgramEntry, FunctionProgramIndex,
    FunctionProgramNodes, ResolvedFunctionNode,
};

type CachedEvalProgramAst<'a> = oxc_ast::ast::Program<'a>;

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
pub(crate) struct ParsedEvalProgram {
    cell: Rc<ParsedEvalProgramCell>,
    functions: OnceCell<IndexedProgramFunctionsCell>,
    /// The parse produced RECOVERABLE errors (`ParserReturn::errors` was
    /// non-empty). An error-recovered AST can silently DROP real code, so
    /// provers of non-usage (e.g. macro-usage liveness) must fail open when
    /// this is set.
    had_errors: bool,
}

impl ParsedEvalProgram {
    pub(crate) fn parse(source: Arc<str>, source_type: oxc_span::SourceType) -> Option<Self> {
        verter_audit::attribute_n!(EvalProgramParse, source.len());
        let mut panicked = false;
        let mut had_errors = false;
        let cell = ParsedEvalProgramCell::new(
            ParsedEvalProgramOwner {
                allocator: oxc_allocator::Allocator::new(),
                source,
                source_type,
            },
            |owner| {
                let result = oxc_parser::Parser::new(
                    &owner.allocator,
                    owner.source.as_ref(),
                    owner.source_type,
                )
                .with_options(oxc_parser::ParseOptions {
                    parse_regular_expression: false,
                    ..oxc_parser::ParseOptions::default()
                })
                .parse();
                panicked = result.panicked;
                had_errors = !result.errors.is_empty();
                // The retained arena is the dominant per-file live footprint;
                // `used_bytes()` walks the chunk list, which is why the amount
                // must never be evaluated when attribution is off.
                verter_audit::attribute_n!(ParseArenaUsed, owner.allocator.used_bytes());
                verter_audit::attribute_max!(ParseArenaCapacity, owner.allocator.capacity());
                result.program
            },
        );
        (!panicked).then_some(Self {
            cell: Rc::new(cell),
            functions: OnceCell::new(),
            had_errors,
        })
    }

    /// The parsed program AST, borrowed from the retained arena.
    pub(crate) fn borrow_dependent(&self) -> &CachedEvalProgramAst<'_> {
        self.cell.borrow_dependent()
    }

    /// Demand the content-free index and register arena addresses once under
    /// this retained parse owner. Neither the arena nor its node table leaves it.
    pub(crate) fn function_program_index(
        &self,
        owners: &verter_semantic::analysis::top_level_owners::TopLevelOwnerTable,
        canonical: Arc<str>,
        parse_env_hash: &crate::types::Hash16,
    ) -> Arc<FunctionProgramIndex> {
        let cell = self.functions.get_or_init(|| {
            IndexedProgramFunctionsCell::new(Rc::clone(&self.cell), |owner| {
                let (index, nodes) = build_function_program_index_with_nodes(
                    owner.borrow_dependent(),
                    owner.borrow_owner().source.as_ref(),
                    owners,
                    canonical,
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
    pub(crate) fn with_indexed_function<R>(
        &self,
        entry: &FunctionProgramEntry,
        lower: impl for<'a> FnOnce(ResolvedFunctionNode<'a>, &'a FunctionProgramEntry) -> R,
    ) -> Option<R> {
        let cell = self.functions.get()?;
        let retained = cell.borrow_dependent();
        let indexed = retained.index.get(&entry.key)?.entry();
        if indexed.locator != entry.locator
            || indexed.span != entry.span
            || indexed.body_span != entry.body_span
            || indexed.flow_body_exact_hash != entry.flow_body_exact_hash
        {
            return None;
        }
        Some(lower(retained.nodes.get(&entry.key)?, indexed))
    }

    pub(crate) fn with_indexed_call<R>(
        &self,
        span: verter_span::Span,
        lower: impl for<'a> FnOnce(&'a oxc_ast::ast::CallExpression<'a>) -> R,
    ) -> Option<R> {
        let cell = self.functions.get()?;
        Some(lower(cell.borrow_dependent().nodes.call(span)?))
    }

    /// Whether the parse recovered from errors (`ParserReturn::errors`
    /// non-empty). See the field docs — non-usage provers fail open on this.
    pub(crate) fn had_errors(&self) -> bool {
        self.had_errors
    }

    /// The exact source text this program was parsed from — for a
    /// `.vue` eval program, the position-preserving extracted script
    /// (script bytes at raw SFC offsets), so every span the program
    /// carries is already SFC-absolute.
    pub(crate) fn source_str(&self) -> &str {
        self.cell.borrow_owner().source.as_ref()
    }

    /// The `SourceType` the parse ran under — the self-consistent type
    /// for any walker consuming this program.
    pub(crate) fn source_type(&self) -> oxc_span::SourceType {
        self.cell.borrow_owner().source_type
    }
}
