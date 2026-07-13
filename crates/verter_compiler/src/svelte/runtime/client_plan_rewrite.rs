//! The template-expression REWRITE half of the Svelte client plan builder.
//!
//! Extracted from `client_plan.rs` (the file-size guard boundary): these are the
//! [`SupportedClientIr`] methods that rewrite a template expression (an `ExprId` or a
//! raw source string) to its emitted client form through the FALLIBLE
//! source-preserving rewriter ([`expr_rewrite`]), threading the per-instance proxy-init
//! map so a template-side reassignment matches the official `should_proxy(rhs)`. They
//! cover the generic rewrite ([`rewrite`](SupportedClientIr::rewrite) /
//! [`rewrite_source`](SupportedClientIr::rewrite_source)), the plain-JS function-pair
//! lane ([`rewrite_source_plain_js`](SupportedClientIr::rewrite_source_plain_js)), the
//! value/property printer with the BEHAVIORAL top-level-sequence paren wrap
//! ([`rewrite_value_preserving_source`](SupportedClientIr::rewrite_value_preserving_source)),
//! and the concise-arrow-body embedding
//! ([`rewrite_arrow_body_value`](SupportedClientIr::rewrite_arrow_body_value)). The
//! rewrite is the SOLE source-derived edit; the surrounding synthesized scaffolding is
//! produced elsewhere in the plan builder.

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_plan::SupportedClientIr;
use super::expr::ScopeId;
use super::expr_rewrite;
use super::ir::ExprId;

impl<'a> SupportedClientIr<'a> {
    /// Rewrite one template expression to its emitted client form through the
    /// FALLIBLE rewriter, threading the per-instance proxy-init map (so a
    /// template-side reassignment matches the official `should_proxy(rhs)`).
    pub(super) fn rewrite(
        &self,
        expr_id: ExprId,
        _scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        expr_rewrite::rewrite_expression_full(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// Rewrite a RAW expression SOURCE STRING (not a pre-analyzed `ExprId`) to its
    /// emitted client form in `scope`, through the same FALLIBLE source-preserving
    /// rewriter as [`rewrite`](Self::rewrite). Used for a function-pair bind's two
    /// `{get, set}` element sources, which are sliced from the bind expression's source
    /// and rewritten INDEPENDENTLY (each as a value expression, so a signal read/write
    /// inside an inline arrow lowers while a bare function identifier passes through).
    pub(super) fn rewrite_source(
        &self,
        source: &str,
        scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        expr_rewrite::rewrite_expression_full(
            source,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// Rewrite the EXPRESSION OF a top-level instance-script STATEMENT (the
    /// effect-statement carrier's `$effect(...)` / `$effect.pre(...)` /
    /// `$effect.root(...)` / `$effect.tracking()` call source) through the shared
    /// FALLIBLE rewriter in the STATEMENT role
    /// ([`rewrite_statement_expression_full`](expr_rewrite::rewrite_statement_expression_full)):
    /// the top-level call is the expression of an `ExpressionStatement` — the ONE
    /// official-legal position for the statement-only user-effect members
    /// (`effect_invalid_placement`) — so it lowers instead of refusing as a value
    /// position. `carrier_head_trivia` is the carrier's pre-rendered
    /// transparent-wrapper head trivia, re-emitted inside the emitted helper
    /// call (the canonical call-internal slot). Everything else (nested
    /// statement admission, signal rewrites, the await gate) is identical to
    /// [`rewrite_source`](Self::rewrite_source).
    pub(super) fn rewrite_statement_source(
        &self,
        source: &str,
        carrier_head_trivia: &str,
        scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        expr_rewrite::rewrite_statement_expression_full(
            source,
            carrier_head_trivia,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// Rewrite the INIT of an instance-script effect-rune declarator (the
    /// `$effect.root(fn)` / `$effect.tracking()` carrier payload) through the
    /// shared FALLIBLE rewriter in the VALUE role
    /// ([`rewrite_init_expression_full`](expr_rewrite::rewrite_init_expression_full)),
    /// threading the carrier's pre-rendered transparent-wrapper head trivia
    /// into the top-level family invocation-head rewrite (the canonical
    /// call-internal slot). Everything else is identical to
    /// [`rewrite_source`](Self::rewrite_source).
    pub(super) fn rewrite_rune_init_source(
        &self,
        source: &str,
        carrier_head_trivia: &str,
        scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        expr_rewrite::rewrite_init_expression_full(
            source,
            carrier_head_trivia,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// Rewrite a FUNCTION-PAIR bind element SOURCE STRING through the PLAIN-JS rewrite
    /// lane ([`rewrite_expression_plain_js`](expr_rewrite::rewrite_expression_plain_js)):
    /// the element is parsed as `SourceType::mjs()` and NOT TS-stripped, mirroring
    /// official svelte@5.56.3's plain-JS parse of a binding expression. Used ONLY for the
    /// two `{get, set}` elements of a DOM function-pair bind (already accepted +
    /// extracted by `parse_plain_svelte_function_pair`); each element is rewritten
    /// INDEPENDENTLY as a value expression (signal reads/writes inside an inline arrow
    /// lower; a bare function identifier passes through). This is distinct from
    /// [`rewrite_source`](Self::rewrite_source) (the TSX + strip lane used for
    /// instance-script `function` declarations) — the dialect change is SCOPED to
    /// function-pair elements, not the broader expression-rewrite surface.
    pub(super) fn rewrite_source_plain_js(
        &self,
        source: &str,
        scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        expr_rewrite::rewrite_expression_plain_js(
            source,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// Rewrite one template expression for a VALUE / PROPERTY position — source-preserving,
    /// with the one BEHAVIORAL value-position transform the official `b.thunk` / `b.spread` /
    /// property-value printer also performs: re-wrapping EXACTLY a top-level
    /// `SequenceExpression` in one paren pair so it stays a single value.
    ///
    /// Concretely: rewrite the WHOLE expression source through the shared source-preserving
    /// expression rewriter (signal/prop reads lowered, TS stripped, author parens +
    /// whitespace kept verbatim), then wrap the result in one paren pair IFF the unwrapped
    /// root is a `SequenceExpression`. The sequence wrap is BEHAVIORAL: a bare `a, b` must
    /// stay ONE value, so the official printer (and Verter) wrap a top-level sequence in one
    /// paren pair — without it a `{@html a, b}` would emit `() => a, b`, splitting `b` into a
    /// positional argument (structurally broken). Author parens around a non-sequence value
    /// (`(c ? a : b)`, `(o.x)`) are kept verbatim — the official printer drops them, but that
    /// is a behavior-preserving redundant-paren COSMETIC difference the minifier collapses.
    ///
    /// This is the value/property-position printer ONLY (it adds the sequence wrap). The
    /// generic [`rewrite`] is the same source-preserving rewriter WITHOUT the sequence wrap,
    /// used at lvalue / bind / event / other-sensitive sites.
    ///
    /// [`rewrite`]: Self::rewrite
    pub(super) fn rewrite_value_preserving_source(
        &self,
        expr_id: ExprId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        let rewritten = expr_rewrite::rewrite_expression_full(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)?;
        Ok(if analyzed.unwrapped_is_sequence {
            // A BARE author sequence (`a, b`) must stay one value: the official printer wraps
            // a top-level `SequenceExpression` in one paren pair so it does not split into
            // positional arguments / object entries (`{@html a, b}` -> `() => (a, b)`). This
            // is BEHAVIORAL, not cosmetic: source-preservation alone would emit `() => a, b`
            // (a broken argument count).
            format!("({rewritten})")
        } else {
            rewritten
        })
    }
}
