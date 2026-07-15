//! The `<svelte:head>` PROJECTION + EMISSION.
//!
//! A `<svelte:head>` emits `$.head('<hash>', ($$anchor) => { <body> })` where `<hash>` is the
//! official `hash(filename)` (djb2-XOR — see [`svelte_hash`](super::naming)) and `<body>` is the
//! head's fragment: its NON-title children (`<meta>` / `<link>` / …) ride the shared
//! `from_html`/`$.append` region machinery, and its `<title>` (when present) is the
//! `$.document.title = <rhs>` write emitted in the callback's after_update slot (`$.effect`
//! when the title is static / constant-foldable, `$.deferred_template_effect` when it references
//! state — the official `TitleElement` `has_state` decision).
//!
//! The `<title>` RHS is built from the typed title chunks the same way the official
//! `build_template_chunk` + `TitleElement` build it: a lone expression folds when statically
//! known (else the raw value + the `is_defined`/`?? ''` outer wrap); a multi-chunk title becomes
//! a template literal with the per-interpolation `?? ''` coalesce. The head itself is EXCLUDED
//! from the enclosing body skeleton (`is_non_body_special`) and emits its `$.head(...)` in the
//! fragment's pre-walk non-rendering init stream.

use super::client::ClientEmitter;
use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::{
    concise_arrow_expr_body, escape_template_text, js_single_quoted,
};
use super::client_component_emit::CallbackPlacement;
use super::client_effect::Memoizer;
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{ClientHead, ClientNode, ClientTitleEffect};
use super::ir::{ExprId, HeadTitleIr, NodeId, SpecialElementIr, TitleChunkIr};
use super::reactive_fold::{ChunkFold, NullishCoalesce};
use verter_span::Span;

impl<'a> SupportedClientIr<'a> {
    /// Project a `<svelte:head>` into its [`ClientNode::Head`]: the `$.head('<hash>', …)` scope
    /// hash (the official `hash(filename)`), the optional `<title>` effect, and the non-title
    /// body region.
    pub(super) fn project_svelte_head(
        &self,
        s: &SpecialElementIr,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let body_region =
            s.body_region
                .ok_or(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:head without body region",
                    span: s.span,
                })?;
        // The scope hash is `hash(filename)` over the compile filename (the official
        // `SvelteHead` reads the module-level `filename` state). A head without a filename is
        // not reachable through the corpus / topology gate (both supply a filename); an absent
        // filename hashes the empty string deterministically rather than panicking.
        let hash = super::naming::svelte_hash(self.ir.component.filename.as_deref().unwrap_or(""));
        let title = s
            .head_title
            .as_ref()
            .map(|t| self.build_title_effect(t, s.span))
            .transpose()?;
        Ok(ClientNode::Head(ClientHead {
            hash,
            title,
            body_region,
        }))
    }

    /// Build the `<title>` → `$.document.title = <rhs>` effect from its typed chunks — the
    /// official `TitleElement` + `build_template_chunk`. A lone expression uses the
    /// `values.length === 1` fast path (fold-when-known, else the raw value + the outer
    /// `is_defined`/`?? ''` wrap); a text-or-multi title accumulates a cooked literal / template
    /// literal.
    fn build_title_effect(
        &self,
        title: &HeadTitleIr,
        span: Span,
    ) -> Result<ClientTitleEffect, UnsupportedSvelteRuntimeSurface> {
        // A lone `{expr}` (the official `values.length === 1` branch).
        if let [TitleChunkIr::Expr(e)] = title.chunks.as_slice() {
            return self.build_single_expr_title(*e, span);
        }
        self.build_multi_chunk_title(&title.chunks, span)
    }

    /// The single-lone-expression title (`<title>{t}</title>`): fold when statically known ⇒
    /// `$.effect` + literal; else the raw rewritten value with the `TitleElement` outer
    /// `is_defined`/`?? ''` wrap ⇒ `$.deferred_template_effect` when the value references
    /// state. A `has_call` chunk is MEMOIZED into a `$N` deps-array slot (the official
    /// `TitleElement` `memoizer.add` — a call is hoisted to `[() => call()]` and the RHS
    /// reads the opaque placeholder `$N ?? ''`), forcing the deferred form.
    fn build_single_expr_title(
        &self,
        e: ExprId,
        span: Span,
    ) -> Result<ClientTitleEffect, UnsupportedSvelteRuntimeSurface> {
        // PREPARE FIRST (the official `build_expression → Memoizer.add → evaluate`
        // ordering): a legacy-wrapped chunk is a sequence by the time official
        // evaluates it ⇒ UNKNOWN ⇒ it never constant-folds, and a memoized `$N` is
        // opaque — only a RAW non-call chunk may fold.
        let prepared = self.prepare_template_value(
            super::client_legacy_value::AuthoredExpr(e),
            super::client_legacy_value::AuthoredValueSurface::TitleChunk,
        )?;
        let has_call = prepared.has_call();
        if !has_call && !prepared.is_wrapped() {
            match self.title_chunk_fold(e) {
                // A statically-known value folds to a literal (`has_state` false ⇒ `$.effect`).
                ChunkFold::Fold(folded) => {
                    return Ok(ClientTitleEffect {
                        deferred: false,
                        rhs: js_single_quoted(&folded),
                        deps: Vec::new(),
                    })
                }
                ChunkFold::Live { .. } => {}
                ChunkFold::Refuse(reason) => {
                    return Err(UnsupportedSvelteRuntimeSurface::ConstFoldThrow {
                        reason: reason.label(),
                        span,
                    })
                }
            }
        }
        // The official `Memoizer.add` rule: a `has_call` chunk is hoisted into a `$N`
        // deps-array slot (the memoized `$.deferred_template_effect($N => …, [() => (…)])`
        // form) with the WRAPPED sequence as the dep; a non-call live value stays
        // inline (a wrapped one as the parenthesized sequence). A memoized call also
        // forces the deferred form.
        let deferred = prepared.facts().has_state || has_call;
        let mut memoizer = Memoizer::default();
        let placeholder = memoizer.add(prepared.effect_value(), has_call);
        // The `TitleElement` outer coalesce: a MEMOIZED `$N` is opaque (`is_defined`
        // false) ⇒ always `$N ?? ''`; an INLINE legacy-wrapped chunk is a sequence
        // official never proves defined ⇒ the BARE `?? ''`; a raw value emits RAW
        // when provably defined, else `value ?? ''` (parenthesized for a top-level
        // `&&`/`||`).
        let rhs = if has_call || prepared.is_wrapped() {
            format!("{placeholder} ?? ''")
        } else {
            match self.title_chunk_nullish_wrap(e) {
                NullishCoalesce::None => placeholder,
                NullishCoalesce::Bare => format!("{placeholder} ?? ''"),
                NullishCoalesce::Parenthesized => format!("({placeholder}) ?? ''"),
            }
        };
        Ok(ClientTitleEffect {
            deferred,
            rhs,
            deps: memoizer.into_deps(),
        })
    }

    /// A text-only or multi-chunk title (`<title>Hello</title>` / `<title>page {t}</title>`): the
    /// official `build_template_chunk` general path — accumulate cooked literal text (folding a
    /// known interpolation into it) and live template parts (each `?? ''`-coalesced), collapsing
    /// to a single string literal when nothing stays live, else a template literal (which is
    /// itself provably defined, so it needs no outer wrap).
    fn build_multi_chunk_title(
        &self,
        chunks: &[TitleChunkIr],
        span: Span,
    ) -> Result<ClientTitleEffect, UnsupportedSvelteRuntimeSurface> {
        let mut cooked = String::new();
        let mut quasis: Vec<String> = Vec::new();
        let mut exprs: Vec<String> = Vec::new();
        let mut deferred = false;
        // The shared `Memoizer` hoists each `has_call` chunk into a `$N` deps-array slot,
        // numbered in source order (the official `build_template_chunk` memoize rule).
        let mut memoizer = Memoizer::default();
        for chunk in chunks {
            match chunk {
                TitleChunkIr::Text(text) => cooked.push_str(text),
                TitleChunkIr::Expr(e) => {
                    // PREPARE FIRST (official `build_expression → Memoizer.add →
                    // evaluate`): only a RAW non-call chunk may constant-fold — a
                    // wrapped or memoized chunk stays live.
                    let prepared = self.prepare_template_value(
                        super::client_legacy_value::AuthoredExpr(*e),
                        super::client_legacy_value::AuthoredValueSurface::TitleChunk,
                    )?;
                    let has_call = prepared.has_call();
                    if !has_call && !prepared.is_wrapped() {
                        match self.title_chunk_fold(*e) {
                            // A known interpolation folds into the cooked literal text.
                            ChunkFold::Fold(folded) => {
                                cooked.push_str(&folded);
                                continue;
                            }
                            ChunkFold::Live { .. } => {}
                            ChunkFold::Refuse(reason) => {
                                return Err(UnsupportedSvelteRuntimeSurface::ConstFoldThrow {
                                    reason: reason.label(),
                                    span,
                                })
                            }
                        }
                    }
                    deferred |= prepared.facts().has_state || has_call;
                    let placeholder = memoizer.add(prepared.effect_value(), has_call);
                    // A MEMOIZED `$N` is opaque (`is_defined` false) ⇒ always `$N ?? ''`;
                    // an INLINE legacy-wrapped part is a sequence ⇒ the BARE `?? ''`; a
                    // raw live part uses the computed `is_defined`/precedence wrap.
                    let part = if has_call || prepared.is_wrapped() {
                        format!("{placeholder} ?? ''")
                    } else {
                        match self.title_chunk_nullish_wrap(*e) {
                            NullishCoalesce::None => placeholder,
                            NullishCoalesce::Bare => format!("{placeholder} ?? ''"),
                            NullishCoalesce::Parenthesized => format!("({placeholder}) ?? ''"),
                        }
                    };
                    // Close the current cooked run into a quasi and record the live part.
                    quasis.push(std::mem::take(&mut cooked));
                    exprs.push(part);
                }
            }
        }
        let deps = memoizer.into_deps();
        // Nothing stayed live ⇒ a single string literal (official `expressions.length === 0`).
        if exprs.is_empty() {
            return Ok(ClientTitleEffect {
                deferred,
                rhs: js_single_quoted(&cooked),
                deps,
            });
        }
        // Else a template literal `` `q0${e0}q1…qn` `` (the trailing cooked run is the last quasi).
        quasis.push(cooked);
        let mut rhs = String::from("`");
        for (i, expr) in exprs.iter().enumerate() {
            rhs.push_str(&escape_template_text(&quasis[i]));
            rhs.push_str("${");
            rhs.push_str(expr);
            rhs.push('}');
        }
        rhs.push_str(&escape_template_text(&quasis[exprs.len()]));
        rhs.push('`');
        Ok(ClientTitleEffect {
            deferred,
            rhs,
            deps,
        })
    }

    /// The [`ChunkFold`] of a title interpolation (constant-fold when statically known) — the
    /// shared `build_template_chunk` evaluate-fold rail.
    fn title_chunk_fold(&self, e: ExprId) -> ChunkFold {
        let analyzed = self.ir.analysis.expressions.get(e);
        super::reactive_fold::mixed_chunk_fold(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            self.ir.analysis.scripts.instance_source,
        )
    }

    /// The `?? ''` coalesce decision for a LIVE, NON-memoized title interpolation — the
    /// shared `build_template_chunk` `is_defined`/precedence rail. A memoized (`has_call`)
    /// chunk never routes here: its opaque `$N` placeholder always coalesces `$N ?? ''`.
    fn title_chunk_nullish_wrap(&self, e: ExprId) -> NullishCoalesce {
        let analyzed = self.ir.analysis.expressions.get(e);
        super::reactive_fold::mixed_chunk_nullish_wrap(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            self.ir.analysis.scripts.instance_source,
            false,
        )
    }
}

impl<'a> ClientEmitter<'a> {
    /// Emit a projected `<svelte:head>` at its source position: `$.head('<hash>', ($$anchor) => {
    /// <body region + title after_update> })`. The head takes NO anchor argument — the `$$anchor`
    /// is created inside the callback. The non-title body rides the shared region-callback
    /// emitter; the title effect is threaded as the callback region's after_update statement (the
    /// official `TitleElement` `after_update` placement — between the body ops and the `$.append`).
    pub(super) fn emit_svelte_head(
        &mut self,
        out: &mut super::output::SvelteRuntimeOutput,
        node: NodeId,
    ) {
        let ClientNode::Head(h) = self.client_node(node) else {
            return;
        };
        let h: ClientHead = h.clone();
        // The `<title>` after_update statement: `$.effect` (static) / `$.deferred_template_effect`
        // (stateful) wrapping the `$.document.title = <rhs>` write. A title bearing memoized
        // `has_call` chunks emits the deps-array form `$.deferred_template_effect(($0, …) =>
        // { … }, [() => dep0, …])` (the official `TitleElement` `memoizer.apply()` params +
        // `memoizer.sync_values()` deps). Empty for a title-less head.
        let after_update = match &h.title {
            Some(title) => {
                let body = format!("{{$.document.title = {};}}", title.rhs);
                if title.deps.is_empty() {
                    let wrapper = if title.deferred {
                        "$.deferred_template_effect"
                    } else {
                        "$.effect"
                    };
                    format!("{wrapper}(() => {body});")
                } else {
                    // The memoized deps-array form: params `$0 … $N-1`, each dep a `() =>
                    // <expr>` concise arrow (routed through the shared unconditional wrap).
                    let params = (0..title.deps.len())
                        .map(|i| format!("${i}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let deps_array = title
                        .deps
                        .iter()
                        .map(|d| format!("() => {}", concise_arrow_expr_body(d)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("$.deferred_template_effect(({params}) => {body}, [{deps_array}]);")
                }
            }
            None => String::new(),
        };
        out.push('\t');
        out.push_str(&format!("$.head('{}', ", h.hash));
        self.emit_region_callback_with_after_update(
            out,
            h.body_region,
            &["$$anchor".to_string()],
            &[],
            &after_update,
            CallbackPlacement::InlineArg,
        );
        out.push_str(");\n");
    }
}
