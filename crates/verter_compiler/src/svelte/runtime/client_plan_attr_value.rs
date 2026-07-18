//! The STRUCTURED dynamic-attribute VALUE builder — the official `build_attribute_value` /
//! `build_template_chunk` value model, extracted from `client_plan` to keep it under the
//! file-size guard.
//!
//! [`SupportedClientIr::attr_value_for`] reads an element's `Dynamic` / `Mixed` attribute into a
//! typed [`AttrValue`] (+ `has_state`); [`SupportedClientIr::mixed_attr_value`] is the mixed
//! (quoted) path — the single-chunk raw value vs the multi-chunk `build_template_chunk`
//! evaluate-fold (each interpolation folded when statically known, else a live `?? ''` part).

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::js_single_quoted;
use super::client_legacy_value::{AuthoredExpr, AuthoredValueSurface};
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{AttrValue, AttrValuePart};
use super::ir::{AttrIr, MixedAttrPart};
use verter_span::Span;

impl<'a> SupportedClientIr<'a> {
    /// Build the STRUCTURED dynamic-attribute value for the attribute named `name` on
    /// element `el` — a [`AttrValue::Single`] for a `Dynamic` single expression, or a
    /// [`AttrValue::Mixed`] for a `Mixed` literal+expr value — plus its `has_state`
    /// (whether the value joins the combined effect). Each expression carries its
    /// `has_call` fact so the emitter memoizes it (the official deps-array rule); the
    /// literal chunks of a mixed value are entity-decoded at IR-lowering time.
    pub(super) fn attr_value_for(
        &self,
        el: &super::ir::ElementIr,
        name: &str,
        surface: AuthoredValueSurface,
    ) -> Result<(AttrValue, bool), UnsupportedSvelteRuntimeSurface> {
        for attr in &el.attrs {
            match attr {
                AttrIr::Dynamic { name: n, expr } if n == name => {
                    // The SOLE authored-value preparation: value-position rewrite +
                    // facts + the surface-policied legacy wrap (official
                    // `build_expression` before the memoize decision); the emit-time
                    // consumers only serialize the carrier.
                    let prepared = self.prepare_template_value(AuthoredExpr(*expr), surface)?;
                    let has_state = prepared.facts().has_state;
                    return Ok((AttrValue::single_authored(prepared), has_state));
                }
                AttrIr::Mixed { name: n, parts } if n == name => {
                    return self.mixed_attr_value(parts, surface);
                }
                _ => {}
            }
        }
        Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
            name: name.to_string(),
            span: Span::new(0, 0),
        })
    }

    /// Build the value of a `Mixed` (quoted) attribute, mirroring official's
    /// `build_attribute_value`. The chunk count decides the path EXACTLY as official's
    /// `value.length` does:
    ///
    /// - ONE chunk (`id="{d}"` / `class="{d}"`) routes the SINGLE-expression path — a raw
    ///   `build_expression` value ([`AttrValue::Single`], no evaluate-fold, no `?? ''`
    ///   wrap), with `has_state` the expression's own. (A lone literal chunk — a quoted
    ///   value with no interpolation cannot reach here as `Mixed`, but is handled as a
    ///   `Const` defensively.) Official's `value.length === 1` branch does NOT call
    ///   `build_template_chunk`, so it never evaluate-folds.
    /// - MULTI chunk (`id="a {d} b"`) routes `build_template_chunk` — each interpolation is
    ///   evaluate-folded when statically KNOWN (`scope.evaluate`), else kept as a live
    ///   `` ${expr ?? ''} `` part; an all-literal result collapses to a single `Const`.
    ///
    /// Returns the structured value + whether ANY surviving (un-folded) part references
    /// state.
    pub(super) fn mixed_attr_value(
        &self,
        parts: &[MixedAttrPart],
        surface: AuthoredValueSurface,
    ) -> Result<(AttrValue, bool), UnsupportedSvelteRuntimeSurface> {
        // SINGLE-chunk quoted value — the official `value.length === 1` branch: the raw
        // single expression, NOT evaluate-folded and NOT `?? ''`-wrapped.
        if parts.len() == 1 {
            return match &parts[0] {
                MixedAttrPart::Literal(text) => {
                    Ok((AttrValue::Const(js_single_quoted(text)), false))
                }
                MixedAttrPart::Expr(e) => {
                    // The single-chunk quoted value is a VALUE position, prepared
                    // through the sole authored-value entry.
                    let prepared = self.prepare_template_value(AuthoredExpr(*e), surface)?;
                    let has_state = prepared.facts().has_state;
                    Ok((AttrValue::single_authored(prepared), has_state))
                }
            };
        }

        // MULTI-chunk value — the official `build_template_chunk` evaluate-fold path.
        let mut value_parts = Vec::with_capacity(parts.len());
        let mut has_state = false;
        for part in parts {
            match part {
                MixedAttrPart::Literal(text) => {
                    value_parts.push(AttrValuePart::Literal(text.clone()));
                }
                MixedAttrPart::Expr(e) => {
                    let analyzed = self.ir.analysis.expressions.get(*e);
                    // A template-literal `${…}` interpolation is a VALUE position,
                    // prepared through the sole authored-value entry (the `?? ''`
                    // coalesce decision below peels parens of its own for its
                    // precedence check, so it is unaffected).
                    let prepared = self.prepare_template_value(AuthoredExpr(*e), surface)?;
                    let has_call = prepared.has_call();
                    // Official `build_template_chunk` constant-folds a KNOWN interpolation
                    // into the cooked literal text (`id="a {d + 1} b"` over a demoted
                    // `$state(5)` → `'a 6 b'`) via `scope.evaluate` — but it evaluates the
                    // chunk AFTER memoization (`shared/utils.js`: `memoize(...)` then
                    // `scope.evaluate(value)`). A `has_call` chunk is replaced by a synthetic
                    // `$N` slot BEFORE the evaluate, and `evaluate($N)` resolves to no binding
                    // ⇒ UNKNOWN ⇒ never folds (so `String(d)` over a demoted `$state` stays a
                    // live `String(d)` effect, NOT a folded literal). Only a NON-`has_call`
                    // chunk can fold; a LEGACY-WRAPPED chunk is a sequence expression by the
                    // time official evaluates it ⇒ likewise UNKNOWN ⇒ never folds (and never
                    // reaches the compile-refusal arm — official emits it live); an unknown
                    // chunk stays live either way.
                    //
                    // The const-fold tri-state contract: `Fold` → the cooked literal;
                    // `Live` (a plain not-foldable chunk OR a ledgered live-fallback) → the
                    // live `?? ''` path; `Refuse` → a deterministic compile refusal (a
                    // compile-time JS throw official also compile-fails — never emit live
                    // code that would crash at runtime).
                    if !has_call && !prepared.is_wrapped() {
                        match super::reactive_fold::mixed_chunk_fold(
                            analyzed.source,
                            analyzed.scope,
                            &self.ir.analysis.bindings,
                            &self.ir.analysis.scopes,
                            self.ir.analysis.scripts.instance_source,
                        ) {
                            super::reactive_fold::ChunkFold::Fold(folded) => {
                                value_parts.push(AttrValuePart::Literal(folded));
                                continue;
                            }
                            // Both a plain not-foldable chunk and a ledgered live-fallback
                            // emit the live expression (below); the ledger reason is recorded
                            // in the checked-in `LiveFallbackReason` table.
                            super::reactive_fold::ChunkFold::Live { .. } => {}
                            super::reactive_fold::ChunkFold::Refuse(reason) => {
                                // The span is unused on the accept-path refusal (matching
                                // the other `mixed_attr_value` refusals); the `ExprId`
                                // arena does not carry a source span.
                                return Err(UnsupportedSvelteRuntimeSurface::ConstFoldThrow {
                                    reason: reason.label(),
                                    span: Span::new(0, 0),
                                });
                            }
                        }
                    }
                    has_state |= prepared.facts().has_state;
                    // The `?? ''` coercion for this LIVE part — official's
                    // `build_template_chunk` `is_defined`/precedence rule. A memoized part
                    // (`has_call`) is a `$N` identifier slot, so the paren decision
                    // collapses; a provably-defined part is emitted RAW (no `?? ''`). An
                    // INLINE legacy-wrapped part is a self-parenthesized sequence official
                    // never proves defined — the BARE `?? ''` always applies to it.
                    let coalesce = if prepared.is_wrapped() && !has_call {
                        super::reactive_fold::NullishCoalesce::Bare
                    } else {
                        super::reactive_fold::mixed_chunk_nullish_wrap(
                            analyzed.source,
                            analyzed.scope,
                            &self.ir.analysis.bindings,
                            &self.ir.analysis.scopes,
                            self.ir.analysis.scripts.instance_source,
                            has_call,
                        )
                    };
                    value_parts.push(AttrValuePart::Expr {
                        value: prepared,
                        coalesce,
                    });
                }
            }
        }
        // If EVERY part is a literal (every interpolation folded to a known constant),
        // the value is a single STRING literal — official `build_template_chunk` emits
        // `b.literal(cooked)` (`'a 6 b'`) when `expressions.length === 0`, NOT a template
        // literal. Concatenate the cooked text and emit a single-quoted `Const`.
        if value_parts
            .iter()
            .all(|p| matches!(p, AttrValuePart::Literal(_)))
        {
            let cooked: String = value_parts
                .iter()
                .map(|p| match p {
                    AttrValuePart::Literal(t) => t.as_str(),
                    AttrValuePart::Expr { .. } => "",
                })
                .collect();
            return Ok((AttrValue::Const(js_single_quoted(&cooked)), has_state));
        }
        Ok((AttrValue::Mixed(value_parts), has_state))
    }
}
