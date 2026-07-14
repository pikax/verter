//! Borrowed-form eval-program parse cell.
//!
//! `ParsedEvalProgram` owns an OXC allocator + source and the `Program`
//! AST parsed from them, as a `self_cell` owner/dependent pair so the
//! borrowed AST never outlives its arena. `ParsedEvalProgram::parse` is
//! the scheduler-bound parse entry for the borrowed lowering input (see
//! the `no_direct_oxc_parser_calls_outside_scheduler_path` architecture
//! guard); consumers reach the cell through the crate-root re-export
//! (`crate::ParsedEvalProgram`).

use std::sync::Arc;

type CachedEvalProgramAst<'a> = oxc_ast::ast::Program<'a>;

struct ParsedEvalProgramOwner {
    allocator: oxc_allocator::Allocator,
    source: Arc<str>,
    source_type: oxc_span::SourceType,
}

self_cell::self_cell!(
    pub(crate) struct ParsedEvalProgram {
        owner: ParsedEvalProgramOwner,

        #[covariant]
        dependent: CachedEvalProgramAst,
    }
);

impl ParsedEvalProgram {
    pub(crate) fn parse(source: Arc<str>, source_type: oxc_span::SourceType) -> Option<Self> {
        let mut panicked = false;
        let parsed = Self::new(
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
                result.program
            },
        );
        (!panicked).then_some(parsed)
    }

    pub(crate) fn source_bytes(&self) -> &[u8] {
        self.borrow_owner().source.as_bytes()
    }

    /// The exact source text this program was parsed from — for a
    /// `.vue` eval program, the position-preserving extracted script
    /// (script bytes at raw SFC offsets), so every span the program
    /// carries is already SFC-absolute.
    pub(crate) fn source_str(&self) -> &str {
        self.borrow_owner().source.as_ref()
    }

    /// The `SourceType` the parse ran under — the self-consistent type
    /// for any walker consuming this program.
    pub(crate) fn source_type(&self) -> oxc_span::SourceType {
        self.borrow_owner().source_type
    }
}
