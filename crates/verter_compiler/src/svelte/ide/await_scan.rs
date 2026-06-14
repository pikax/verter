//! Syntactic detection of EXPERIMENTAL Svelte await-expressions (D-bg).
//!
//! The experimental Svelte await-expression is an `await` evaluated OUTSIDE an
//! async-function body — at a reactive position: an instance-script top-level
//! statement, inside `$derived(...)`/`$derived.by(...)`, or in a markup
//! expression. An `await` lexically inside an `async function` / `async (…) =>`
//! body is ORDINARY TypeScript and is NOT flagged.
//!
//! Detection is GRAMMAR-CORRECT: the text is parsed once with OXC (the same
//! front-end the rest of the compiler uses) and an `oxc_ast_visit::Visit` walk
//! collects `AwaitExpression` spans whose nearest enclosing function is NOT
//! async. This is a SYNTACTIC analysis (function-scope nesting only — no type
//! resolution), so the Typed-IR-Only rule does not apply; it only records a
//! typed-unsupported diagnostic and never alters the projected expression. An
//! unparseable fragment yields no diagnostics (fail-open) — the projection's
//! own validity is unaffected.

use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrowFunctionExpression, AwaitExpression, Function, Program};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_syntax::scope::ScopeFlags;

/// Scan `text` (an instance/module script body OR a markup interpolation
/// expression) for experimental await-expression occurrences, returning each
/// occurrence's byte offset (relative to `text`) at which the `await` keyword
/// begins.
///
/// `text` is parsed as a TSX module (top-level await parses under module mode),
/// so the three reactive positions — instance-script top level,
/// `$derived(...)`/`$derived.by(...)` args, and markup expressions — all flow
/// through the same grammar-correct walk. An `await` inside an async function /
/// arrow body (brace OR expression body, any nesting) is shadowed and not
/// reported.
pub(super) fn scan_await_positions(text: &str) -> Vec<u32> {
    if !text.contains("await") {
        return Vec::new();
    }
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parsed = Parser::new(&allocator, text, source_type).parse();
    // A fragment that does not parse cleanly yields no diagnostics (fail-open) —
    // the projection's own validity does not depend on this heuristic.
    if parsed.panicked {
        return Vec::new();
    }
    let mut collector = AwaitCollector {
        async_depth: 0,
        positions: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    collector.positions
}

/// Collects `await` keyword start offsets outside any async-function body.
struct AwaitCollector {
    /// The number of enclosing ASYNC function/arrow bodies — an await is the
    /// experimental form only when this is zero (no async shadow).
    async_depth: u32,
    /// The collected `await` keyword start offsets.
    positions: Vec<u32>,
}

impl<'a> Visit<'a> for AwaitCollector {
    fn visit_program(&mut self, it: &Program<'a>) {
        walk::walk_program(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        // A function body OPENS a fresh async context (an async body shadows
        // inner awaits; a sync body un-shadows them — a sync function nested in
        // an async one is its own non-async scope). Save/restore around the walk.
        let saved = self.async_depth;
        self.async_depth = if it.r#async { saved + 1 } else { 0 };
        walk::walk_function(self, it, flags);
        self.async_depth = saved;
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        let saved = self.async_depth;
        self.async_depth = if it.r#async { saved + 1 } else { 0 };
        walk::walk_arrow_function_expression(self, it);
        self.async_depth = saved;
    }

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        if self.async_depth == 0 {
            self.positions.push(it.span.start);
        }
        walk::walk_await_expression(self, it);
    }
}
