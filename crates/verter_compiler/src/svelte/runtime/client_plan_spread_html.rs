//! The element-spread `$.attribute_effect` fold + the `{@html}` `$.html` op projection.
//!
//! Extracted from `client_plan.rs` (the file-size guard boundary): these are the
//! [`SupportedClientIr`] projection methods for the two coalesced runtime surfaces a
//! spread element / a `{@html}` tag produces — the single `$.attribute_effect(el, () =>
//! ({ <fold> }))` and the `$.html(node, () => h [, true])`. They drive everything off the
//! typed IR + the shared expression rewriter (no string scan), reusing the shared
//! value-building helpers (`mixed_attr_value`, `rewrite`, `object_key`, `object_property`,
//! `style_object`, `fold_style_directives`). The value/property emission is source-preserving
//! (author parens kept verbatim); the spread operand adds only the BEHAVIORAL sequence wrap
//! (`rewrite_spread_operand` keeps one paren pair for a top-level `SequenceExpression` so it
//! does not split into two object entries), decided from the typed OXC node.

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::{
    fold_style_directives, js_single_quoted, object_key, object_property,
};
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{AttrValue, AttrValuePart, ClientNodeId, ClientRuntimeOp};
use super::ir::{AttrIr, ElementIr, ExprId, IrNode, NodeId, StyleDirectiveValue};
use verter_span::Span;

impl<'a> SupportedClientIr<'a> {
    /// Project the coalesced `$.attribute_effect(el, () => ({ <fold> }))` op for a
    /// SPREAD element. A spread switches the element's WHOLE attribute strategy: every
    /// co-located attribute folds — in SOURCE ORDER for plain attributes / spreads, with
    /// every `class:` directive merged into ONE trailing `[$.CLASS]: { … }` and every
    /// `style:` directive merged into ONE trailing `[$.STYLE]: { … }` (the official
    /// `Element.js` spread path) — into the single object literal the effect's arrow
    /// returns. The fold has NO memoization (the arrow re-runs the WHOLE object each tick,
    /// so a `has_call` value is a bare expression, not a `$N` slot); every expression is
    /// rewritten through the shared rewriter (a live `$state` reads `$.get(n)`, a demoted
    /// one reads bare, a no-default prop reads `$$props.p`).
    pub(super) fn project_attribute_effect_op(
        &self,
        target: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        // The SOURCE-ORDERED plain-attribute / spread fold entries (a static / dynamic /
        // mixed attribute, a plain `class` / `style` attribute, or a `...spread`).
        let mut entries: Vec<String> = Vec::new();
        // The merged `class:` / `style:` directive entries (collected in directive source
        // order, appended LAST — `[$.CLASS]` then `[$.STYLE]`). A style entry carries its
        // `|important` flag (the array-form switch).
        let mut class_dirs: Vec<String> = Vec::new();
        let mut style_dirs: Vec<(String, bool)> = Vec::new();
        for attr in &el.attrs {
            match attr {
                AttrIr::Static { name, value } => {
                    // A static attribute folds as `name: 'lit'` over the producer-DECODED
                    // value (the fold value is a runtime JS string, NOT a baked skeleton
                    // attr; read via `as_str`, never a second decode). A VALUELESS
                    // attribute (`value: None` — `<input {...p} disabled />`) folds
                    // as the RAW boolean `name: true`, distinct from a present empty-string
                    // value (`Some("")` — `disabled=""`) which stays `name: ''`.
                    let v = match value {
                        None => "true".to_string(),
                        Some(v) => js_single_quoted(v.value.as_str()),
                    };
                    entries.push(format!("{}: {v}", object_key(name)));
                }
                AttrIr::Dynamic { name, expr } => {
                    // A dynamic attribute folds as `name: <rewritten>` — RAW (no `$.clsx`
                    // wrap even for a plain `class={…}`, the official spread-fold rule),
                    // no memoization (the arrow is the reactive boundary). A VALUE position —
                    // source-preserving (author parens kept; sequence wrapped once).
                    let v = self.rewrite_value_preserving_source(*expr)?;
                    entries.push(format!("{}: {v}", object_key(name)));
                }
                AttrIr::Mixed { name, parts } => {
                    // A mixed attribute folds as the `` `lit${expr ?? ''}lit` `` template,
                    // built through the shared mixed-value path with NO memoizer.
                    let (value, _) = self.mixed_attr_value(parts)?;
                    let v = self.fold_attr_value_text(&value);
                    entries.push(format!("{}: {v}", object_key(name)));
                }
                AttrIr::Spread { expr } => {
                    // A spread folds as `...<rewritten>` — source-preserving (author parens
                    // kept verbatim). A top-level `SequenceExpression` operand keeps its
                    // wrapping paren pair (`{...(a, b)}` → `...(a, b)`) because dropping it
                    // would split the operand into two object entries — that wrap is
                    // BEHAVIORAL, decided from the typed OXC node, never a string balance-scan.
                    entries.push(format!("...{}", self.rewrite_spread_operand(*expr)?));
                }
                AttrIr::Class { name, condition } => {
                    let key = object_key(name);
                    let entry = match condition {
                        Some(e) => {
                            // A VALUE position — source-preserving (author parens kept;
                            // sequence wrapped once).
                            let cond = self.rewrite_value_preserving_source(*e)?;
                            // Object-property SHORTHAND: `class:on` / `class:on={on}` fold
                            // to `{ on }` when the value text equals the key (the JS
                            // shorthand the official printer emits); else `{ key: cond }`.
                            object_property(&key, &cond)
                        }
                        None => key,
                    };
                    class_dirs.push(entry);
                }
                AttrIr::Style {
                    property,
                    value,
                    important,
                } => {
                    let key = object_key(property);
                    // A `|important` style directive switches the WHOLE `[$.STYLE]` entry
                    // to the `[{ … }, { prop: 'v' }]` array form — a normal directive folds
                    // into the shorthand object. `fold_style_directives` assembles both.
                    let entry = match value {
                        StyleDirectiveValue::Expr(e) => {
                            // A VALUE position — source-preserving (author parens kept;
                            // sequence wrapped once).
                            let v = self.rewrite_value_preserving_source(*e)?;
                            object_property(&key, &v)
                        }
                        // A static-text style value folds as the quoted string literal
                        // (`[$.STYLE]: { color: 'red' }`).
                        StyleDirectiveValue::Text(text) => {
                            object_property(&key, &js_single_quoted(text))
                        }
                        // A MIXED text+interpolation value folds as the template-literal
                        // `[$.STYLE]: { color: `a${x ?? ''}b` }` through the SAME no-memoizer
                        // mixed-value + fold-text path the `AttrIr::Mixed` arm uses.
                        StyleDirectiveValue::Mixed(parts) => {
                            let (mixed, _) = self.mixed_attr_value(parts)?;
                            object_property(&key, &self.fold_attr_value_text(&mixed))
                        }
                    };
                    style_dirs.push((entry, *important));
                }
                _ => {}
            }
        }
        // Append the merged `[$.CLASS]` then `[$.STYLE]` directive entries (after every
        // plain attribute / spread), matching the official spread-fold ordering.
        if !class_dirs.is_empty() {
            entries.push(format!("[$.CLASS]: {{ {} }}", class_dirs.join(", ")));
        }
        if let Some(style_entry) = fold_style_directives(&style_dirs) {
            entries.push(style_entry);
        }
        let fold_body = entries.join(", ");
        // A void / self-closing element (an `<input>`) takes the official trailing
        // `void 0, void 0, void 0, void 0, true` argument tail (the value/defaultValue
        // handling flag); every other allowlist element is a 2-argument call. An `<input>`
        // carrying an authored `defaultValue` / `defaultChecked` reset attribute SUPPRESSES
        // the tail (the official `Element.js` rule).
        let input_trailing = Self::element_takes_attribute_effect_input_tail(el);
        // The SCOPED spread element passes the scope-hash literal as the fold's
        // `css_hash` argument (the official `build_attribute_effect` — "the
        // spread method appends the hash to the end of the class attribute on
        // its own"), read from the SAME shared scope facts the other injection
        // sites consume.
        let css_hash = self
            .css_scope
            .as_ref()
            .and_then(|facts| facts.hash_for(target))
            .map(js_single_quoted);
        Ok(ClientRuntimeOp::AttributeEffect {
            target: ClientNodeId(target.0),
            fold_body,
            input_trailing,
            css_hash,
        })
    }

    /// Rewrite a spread operand to its emitted `...<operand>` text — source-preserving, with
    /// the one BEHAVIORAL transform: a top-level `SequenceExpression` operand keeps a single
    /// wrapping paren pair (`{...(a, b)}` → `...(a, b)`) so it stays one spread value rather
    /// than splitting into two object entries. Author parens around a non-sequence operand
    /// (`{...(c ? a : b)}`) are kept verbatim (a behavior-preserving redundant-paren cosmetic
    /// difference the minifier collapses).
    ///
    /// The sequence-wrap decision is AST-precedence-aware (the typed OXC node decides), never
    /// a string balance-scan (which would corrupt `{...(a, b)}` into two object entries). It
    /// delegates to the shared value-position printer ([`rewrite_value_preserving_source`]) —
    /// the SAME `unwrapped_is_sequence` fact + sequence re-wrap the every other value/property
    /// fold uses — so the spread operand and every other value fold share one path.
    ///
    /// [`rewrite_value_preserving_source`]: SupportedClientIr::rewrite_value_preserving_source
    fn rewrite_spread_operand(
        &self,
        expr: ExprId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        self.rewrite_value_preserving_source(expr)
    }

    /// Whether the spread element takes the `$.attribute_effect` 7-argument trailing form
    /// (`…, void 0, void 0, void 0, void 0, true`). An `<input>` takes the tail UNLESS it
    /// carries an authored `defaultValue` / `defaultChecked` reset attribute, which opts the
    /// element out of the value/defaultValue reset behavior the tail encodes (the official
    /// `Element.js` rule); every other allowlist element is a 2-argument call. Driven from
    /// the typed element + its typed attribute set, never the raw tag / source.
    fn element_takes_attribute_effect_input_tail(el: &ElementIr) -> bool {
        el.tag == "input" && !Self::has_authored_reset_attr(el)
    }

    /// Whether the element carries an authored PLAIN attribute named EXACTLY `defaultValue`
    /// or `defaultChecked` — the camelCase, CASE-SENSITIVE reset attributes that suppress
    /// the `<input>` `$.attribute_effect` trailing tail. The match is on the RAW authored
    /// attribute name (a lowercase `defaultvalue` does NOT match); only the static / dynamic
    /// / mixed plain-attribute forms count (a spread / bind / `class:` / `style:` directive /
    /// event is never a reset attribute). Read from the typed `el.attrs`, never a string
    /// scan of the source.
    fn has_authored_reset_attr(el: &ElementIr) -> bool {
        el.attrs.iter().any(|attr| {
            let name = match attr {
                AttrIr::Static { name, .. }
                | AttrIr::Dynamic { name, .. }
                | AttrIr::Mixed { name, .. } => name.as_str(),
                _ => return false,
            };
            name == "defaultValue" || name == "defaultChecked"
        })
    }

    /// The fold text for an [`AttrValue`] used as a fold entry value — the SAME shape the
    /// reactive-attr emitter builds, but with NO memoizer (the `$.attribute_effect` arrow
    /// re-runs the whole object, so a `has_call` part is a bare expression, not a `$N`
    /// slot). A `Const` is verbatim, a `Single` is its bare rewritten expression, a
    /// `Mixed` is the `` `lit${expr ?? ''}lit` `` template.
    ///
    /// Shared with the standalone `$.set_style` directive fold (`project_set_style_op`'s
    /// `StyleDirectiveValue::Mixed` arm) — a mixed-style directive value folds through
    /// the SAME no-memoizer fold-text the spread `[$.STYLE]` arm uses.
    pub(super) fn fold_attr_value_text(&self, value: &AttrValue) -> String {
        match value {
            AttrValue::Const(text) => text.clone(),
            AttrValue::Single { rewritten, .. } => rewritten.clone(),
            AttrValue::Mixed(parts) => {
                let mut tmpl = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(text) => {
                            tmpl.push_str(&super::client_codegen_helpers::escape_template_text(
                                text,
                            ));
                        }
                        AttrValuePart::Expr {
                            rewritten,
                            coalesce,
                            ..
                        } => {
                            use super::reactive_fold::NullishCoalesce;
                            match coalesce {
                                NullishCoalesce::None => {
                                    tmpl.push_str(&format!("${{{rewritten}}}"))
                                }
                                NullishCoalesce::Bare => {
                                    tmpl.push_str(&format!("${{{rewritten} ?? ''}}"));
                                }
                                NullishCoalesce::Parenthesized => {
                                    tmpl.push_str(&format!("${{({rewritten}) ?? ''}}"));
                                }
                            }
                        }
                    }
                }
                tmpl.push('`');
                tmpl
            }
        }
    }

    /// Project the `$.html(node, () => h [, true])` op for a `{@html}` node. The payload
    /// (the second argument) is a `() => <rewritten-expr>` thunk, ELIDED to the bare
    /// rewritten callee when the payload is a DIRECT non-optional zero-argument identifier
    /// call WHOSE CALLEE REWRITES UNCHANGED (`{@html render()}` with a plain / local /
    /// demoted `render` → `render`, the official thunk elision). A call whose callee
    /// rewrites to a member (`{@html render()}` with `render` a no-default `$props()`
    /// binding → `$$props.render`) does NOT elide — it stays the thunk over the rewritten
    /// whole expression (`() => $$props.render()`). The only-child topology (the `{@html}`
    /// is the SOLE controlled child of its parent element) selects the parent-targeted
    /// `, true` + `$.reset(parent)` form.
    pub(super) fn project_html_op(
        &self,
        node_id: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let expr = self.html_payload_expr(node_id)?;
        let analyzed = self.ir.analysis.expressions.get(expr);
        // Thunk elision: a DIRECT, non-optional, zero-argument identifier call elides the
        // `() => …` thunk to the bare callee ONLY when the callee rewrites UNCHANGED (a
        // plain / local / demoted id, where `$.get` / `$$props` rewriting is a no-op). A
        // prop / signal callee rewrites to a member or `$.get(...)` call, so it stays the
        // thunk over the whole rewritten expression. The callee fact is harvested from the
        // analysis parse; the callee's REWRITTEN form is computed through the shared
        // rewriter (its own name source — never a raw-source heuristic, never the
        // un-rewritten callee).
        let payload = match &analyzed.direct_zero_arg_call_callee {
            Some(callee_name) => {
                let rewritten_callee = self.rewrite_identifier(callee_name, analyzed.scope)?;
                if &rewritten_callee == callee_name {
                    // Plain / local / demoted callee — elide to the bare callee.
                    rewritten_callee
                } else {
                    // The callee rewrote to a member / getter — keep the thunk, but render it
                    // as the REWRITTEN-CALLEE zero-arg call `() => <rewritten_callee>()`, NOT
                    // the blind whole-source rewrite (which would keep any author parens
                    // around the callee — `(render)()` → `() => ($$props.render)()`). Since the
                    // payload is a direct zero-arg call, its shape is exactly `<callee>()`, so
                    // rebuilding the call from the harvested+peeled callee drops the author
                    // parens to match official (`() => $$props.render()`).
                    format!("() => {rewritten_callee}()")
                }
            }
            // The non-call thunk is a CONCISE-ARROW-BODY value position — source-preserving
            // (author parens kept verbatim), with the BEHAVIORAL sequence wrap (a bare `{@html
            // a, b}` becomes `() => (a, b)`, keeping `b` from leaking as a 3rd positional
            // `$.html` arg) AND the concise-arrow-body object wrap (a leading-`{` body becomes
            // `() => ({ … })` so it returns the object instead of parsing a block body).
            None => format!("() => {}", self.rewrite_arrow_body_value(expr)?),
        };
        let only_child = self.html_is_only_controlled_child(node_id);
        Ok(ClientRuntimeOp::Html {
            target: ClientNodeId(node_id.0),
            payload,
            only_child,
        })
    }

    /// Rewrite a bare IDENTIFIER NAME (its own source) through the shared expression
    /// rewriter, in `scope` — so a signal callee reads `$.get(name)`, a no-default prop
    /// reads `$$props.name`, and a plain / local / demoted id stays `name`. Used by the
    /// `{@html}` elision decision to compare the rewritten callee against the bare name.
    fn rewrite_identifier(
        &self,
        name: &str,
        scope: super::expr::ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        super::expr_rewrite::rewrite_expression_full(
            name,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// The raw-markup expression id of a `{@html}` node (a `TagIr::Html` tag or a
    /// `RawHtml`-form interpolation).
    fn html_payload_expr(
        &self,
        node_id: NodeId,
    ) -> Result<ExprId, UnsupportedSvelteRuntimeSurface> {
        match self.ir.node(node_id) {
            IrNode::Tag(super::ir::TagIr::Html { expr }) => Ok(*expr),
            IrNode::Interpolation {
                expr,
                escape: super::ir::EscapeMode::Raw,
                ..
            } => Ok(*expr),
            _ => Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "non-html-node",
                span: Span::new(node_id.0, node_id.0),
            }),
        }
    }

    /// Whether a `{@html}` node is the SOLE controlled child of its parent element (the
    /// official `is_controlled` case: the runtime walks it without a `<!>` anchor, so the
    /// `$.html` operates on the PARENT element var with the trailing `true` argument). A
    /// root-level `{@html}` (no parent element) is NOT controlled (it anchors to a
    /// `$.comment()` / fragment node). Driven from the typed IR geometry.
    pub(super) fn html_is_only_controlled_child(&self, node_id: NodeId) -> bool {
        // Find the parent element whose cleaned children are EXACTLY this `{@html}` node.
        for node in &self.ir.nodes {
            let IrNode::Element(el) = node else {
                continue;
            };
            if !el.children.contains(&node_id) {
                continue;
            }
            // The element's cleaned children must be EXACTLY one node — this `{@html}`.
            let items = super::whitespace::clean_nodes(
                self.ir,
                &el.children,
                super::whitespace::CleanContext::region_root(),
            );
            return matches!(
                items.as_slice(),
                [super::whitespace::CleanItem::Node(only)] if *only == node_id
            );
        }
        false
    }
}
