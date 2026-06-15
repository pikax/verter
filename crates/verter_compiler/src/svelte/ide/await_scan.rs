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
use oxc_span::{GetSpan, SourceType};
use oxc_syntax::scope::ScopeFlags;

use crate::code_transform::CodeTransform;

/// Rewrite every experimental await-expression in `text` (a markup-expression
/// fragment scanned relative to `base`) to the checkable form `await ARG` →
/// `__verter_await_expr(ARG)` through `CodeTransform` ops on `ct`.
///
/// This is the SINGLE SOURCE OF TRUTH for the await byte-rewrite — EVERY markup
/// await position routes through it, so they all emit byte-identical wrapper ops:
/// the span-based markup-expression path (`rewrite_await_exprs_in`, over the
/// projector's main transform with `base = span.start`); the TEXT-path
/// markup-expression entries (the F8 dynamic-component `this`, the hoisted
/// `{@const}`/`{@let}` inner, a `bind:` target — over a throwaway transform with
/// `base = 0`); AND the block-binding PATTERN-DEFAULT path (via
/// [`rewrite_pattern_default_awaits_on`], which scans inside a parse-wrapper and
/// delegates the byte ops here). `__verter_render` STAYS SYNC, so a raw `await`
/// left at ANY markup position would be INVALID TSX; routing every entry through
/// this helper makes the "no raw `await` at any markup position" guarantee TRUE,
/// not just documented. The `[keyword_start, arg_start)` run is overwritten with
/// the wrapper open (replacing `await ` + interleaving whitespace) and a `)` is
/// inserted at `await_end` (== `ARG`'s end), preserving the original `ARG` bytes
/// for hover / mapping. Diagnostics are NOT emitted here — the caller records
/// them against the absolute source span (see `record_await_diagnostics_in`).
pub(super) fn rewrite_await_exprs_on(ct: &mut CodeTransform, base: u32, text: &str) {
    apply_await_rewrite(
        ct,
        scan_await_positions(text).into_iter().map(|at| {
            // The unwrapped span path applies each position verbatim at `base + off`.
            AwaitPosition {
                keyword_start: base + at.keyword_start,
                await_end: base + at.await_end,
                arg_start: base + at.arg_start,
            }
        }),
    );
}

/// Apply the await byte-rewrite to every position in `positions` (offsets already
/// translated to the target transform's coordinate space). This is the ONLY place
/// that emits the `__verter_await_expr(` wrapper ops — `rewrite_await_exprs_on`
/// (the span / text path) and `rewrite_pattern_default_awaits_on` (the
/// parse-wrapper path) both funnel their translated positions here, so there is
/// exactly one byte-rewrite implementation. The `[keyword_start, arg_start)` run
/// is overwritten with the wrapper open and a `)` is inserted at `await_end`.
fn apply_await_rewrite(ct: &mut CodeTransform, positions: impl Iterator<Item = AwaitPosition>) {
    for at in positions {
        ct.overwrite(at.keyword_start, at.arg_start, "__verter_await_expr(");
        ct.append_left(at.await_end, ")");
    }
}

/// The wrapper prefix the pattern-default scan parses `pattern_text` inside — the
/// SAME `const [` … `] = null as any;` wrapper the store-default scanner uses, so
/// a bare identifier, a destructuring pattern, and a comma-separated param list
/// all parse as one declarator's binding pattern (default initializers parse as
/// expressions). Mirrors `store_scan::PATTERN_WRAPPER_PREFIX_LEN`.
const PATTERN_WRAPPER_PREFIX_LEN: u32 = "const [".len() as u32;

/// Rewrite every experimental await-expression inside the DEFAULT-VALUE
/// expressions of a block-binding PATTERN TEXT (`{ x = await load() }` /
/// `($item = await thing())`) through `CodeTransform` ops on `ct`, returning
/// whether ANY rewrite was applied.
///
/// A binding-pattern default is a MARKUP-EXPRESSION position — the pattern is
/// sliced into a SYNC arrow head (`xs.map(({ x = await load() }) => …)`), so a
/// raw `await` there would be INVALID TSX (`__verter_render` stays sync). The
/// pattern text alone is not a parseable module, so it is scanned inside the SAME
/// `const [{pattern}] = null as any;` wrapper the store-default scanner uses (the
/// default initializer then parses as a top-level expression and its await is the
/// experimental form); each found offset is translated back past the wrapper
/// prefix and the byte ops are emitted by the SHARED [`apply_await_rewrite`] — the
/// one byte-rewrite implementation `rewrite_await_exprs_on` also uses — so this
/// path keeps NO parallel overwrite of its own. Positions landing inside the
/// wrapper machinery (defensive) are dropped.
pub(super) fn rewrite_pattern_default_awaits_on(
    ct: &mut CodeTransform,
    pattern_text: &str,
) -> bool {
    if !pattern_text.contains("await") {
        return false;
    }
    let wrapped = format!("const [{pattern_text}] = null as any;");
    let translated: Vec<AwaitPosition> = scan_await_positions(&wrapped)
        .into_iter()
        .filter(|at| at.keyword_start >= PATTERN_WRAPPER_PREFIX_LEN)
        .map(|at| AwaitPosition {
            keyword_start: at.keyword_start - PATTERN_WRAPPER_PREFIX_LEN,
            await_end: at.await_end - PATTERN_WRAPPER_PREFIX_LEN,
            arg_start: at.arg_start - PATTERN_WRAPPER_PREFIX_LEN,
        })
        .collect();
    let rewrote = !translated.is_empty();
    apply_await_rewrite(ct, translated.into_iter());
    rewrote
}

/// The `await`-keyword byte offsets (relative to `pattern_text`) of every
/// experimental await-expression in a block-binding PATTERN's default-value
/// expressions. Scanned inside the SAME `const [{pattern}] = null as any;`
/// wrapper as [`rewrite_pattern_default_awaits_on`], with each offset translated
/// back past the wrapper prefix. Used to record the INFORMATIONAL diagnostics
/// against the absolute source span (the rewrite itself is byte-only).
pub(super) fn pattern_default_await_keyword_offsets(pattern_text: &str) -> Vec<u32> {
    if !pattern_text.contains("await") {
        return Vec::new();
    }
    let wrapped = format!("const [{pattern_text}] = null as any;");
    scan_await_positions(&wrapped)
        .into_iter()
        .filter(|at| at.keyword_start >= PATTERN_WRAPPER_PREFIX_LEN)
        .map(|at| at.keyword_start - PATTERN_WRAPPER_PREFIX_LEN)
        .collect()
}

/// One detected experimental await-expression: the byte offsets (relative to the
/// scanned `text`) of the `await` keyword start, the whole `await ARG`
/// expression end, and the start of the awaited `ARG`.
///
/// The markup projector rewrites `await ARG` → `__verter_await_expr(ARG)` from
/// these spans: it overwrites `[keyword_start, arg_start)` with the wrapper open
/// (replacing `await ` + interleaving whitespace) and inserts `)` at `await_end`
/// (== `ARG`'s end), preserving the original `ARG` bytes for hover / mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AwaitPosition {
    /// The byte offset of the `await` keyword start.
    pub keyword_start: u32,
    /// The byte offset of the end of the whole `await ARG` expression (one past
    /// the awaited argument's last byte).
    pub await_end: u32,
    /// The byte offset of the awaited `ARG`'s first byte.
    pub arg_start: u32,
}

/// Scan `text` (an instance/module script body OR a markup interpolation
/// expression) for experimental await-expression occurrences, returning each
/// occurrence's keyword + argument spans (relative to `text`).
///
/// `text` is parsed as a TSX module (top-level await parses under module mode),
/// so the three reactive positions — instance-script top level,
/// `$derived(...)`/`$derived.by(...)` args, and markup expressions — all flow
/// through the same grammar-correct walk. An `await` inside an async function /
/// arrow body (brace OR expression body, any nesting) is shadowed and not
/// reported.
pub(super) fn scan_await_positions(text: &str) -> Vec<AwaitPosition> {
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

/// Collects await-expression positions outside any async-function body.
struct AwaitCollector {
    /// The number of enclosing ASYNC function/arrow bodies — an await is the
    /// experimental form only when this is zero (no async shadow).
    async_depth: u32,
    /// The collected await-expression positions.
    positions: Vec<AwaitPosition>,
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
            self.positions.push(AwaitPosition {
                keyword_start: it.span.start,
                await_end: it.span.end,
                arg_start: it.argument.span().start,
            });
        }
        walk::walk_await_expression(self, it);
    }
}
