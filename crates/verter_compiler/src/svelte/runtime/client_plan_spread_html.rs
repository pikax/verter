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
use super::client_codegen_helpers::{js_single_quoted, object_key};
use super::client_legacy_value::{AuthoredExpr, AuthoredValueSurface, PreparedTemplateValue};
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{
    AttrValue, AttributeEffectItem, ClientNodeId, ClientRuntimeOp, StyleDirectiveObjectEntry,
    StyleDirectiveObjectValue,
};
use super::expr::UnwrappedRootKind;
use super::ir::{AttrIr, ElementIr, ExprId, IrNode, NodeId, StyleDirectiveValue};
use super::synthesized_value::SynthesizedTemplateValue;
use verter_span::Span;

/// Host knobs for the SHARED attribute-effect fold builder: the
/// `<svelte:element>` host synthesizes the analyze-phase empty `class`/`style`
/// entries and skips the class family when its lone-class fast path consumed it;
/// the regular-element spread host uses the default (no synthetics — a spread
/// suppresses them officially).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct AttributeEffectFoldOptions {
    /// Append the analyze-phase synthesized `class: ''` entry.
    pub(super) synth_class: bool,
    /// Append the analyze-phase synthesized `style: ''` entry.
    pub(super) synth_style: bool,
    /// Skip the static `class` attribute + every `class:` directive (consumed
    /// by the `$.set_class` fast path).
    pub(super) skip_class: bool,
}

impl<'a> SupportedClientIr<'a> {
    /// Project the coalesced `$.attribute_effect(el, (params) => ({ <fold> }), [deps])`
    /// op for a SPREAD element. A spread switches the element's WHOLE attribute
    /// strategy: every co-located attribute folds — in SOURCE ORDER for plain
    /// attributes / spreads, with every `class:` directive merged into ONE trailing
    /// `[$.CLASS]` and every `style:` directive merged into ONE trailing `[$.STYLE]`
    /// (the official `Element.js` spread path) — into TYPED
    /// [`AttributeEffectItem`]s. Each authored value is PREPARED through the sole
    /// entry (`BuildExpression` for co-located values / `style:` inner values, RAW
    /// for spread operands / `class:` conditions); the EMITTER renders the items
    /// through one ordered per-effect memoizer (official `Memoizer`), so a
    /// `has_call` value hoists into a `$N` arrow param + dependency.
    pub(super) fn project_attribute_effect_op(
        &self,
        target: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        let items =
            self.attribute_effect_items(&el.attrs, AttributeEffectFoldOptions::default())?;
        // A void / self-closing element (an `<input>`) takes the official trailing
        // `true` remove-defaults argument (suppressed by an authored `defaultValue` /
        // `defaultChecked` reset attribute); every other allowlist element omits it.
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
            items,
            input_trailing,
            css_hash,
        })
    }

    /// Build the SOURCE-ORDERED typed fold items of one `$.attribute_effect` over a
    /// typed attribute inventory — the ONE shared item builder serving the
    /// regular-element spread fold and the `<svelte:element>` fold (the
    /// `<svelte:element>` host passes `skip_class_directives` when its lone-class
    /// fast path consumed them). `class:` / `style:` directives merge into ONE
    /// trailing `[$.CLASS]` / `[$.STYLE]` synthesized object each (class inner
    /// conditions RAW, style inner values WRAPPED — official
    /// `build_class_directives_object` / `build_style_directives_object`).
    pub(super) fn attribute_effect_items(
        &self,
        attrs: &[AttrIr],
        opts: AttributeEffectFoldOptions,
    ) -> Result<Vec<AttributeEffectItem>, UnsupportedSvelteRuntimeSurface> {
        let mut items: Vec<AttributeEffectItem> = Vec::new();
        // The merged `class:` / `style:` directive entries (collected in directive
        // source order, appended LAST — `[$.CLASS]` then `[$.STYLE]`) — TYPED
        // contributor lists the sealed synthesis constructors consume (key
        // quoting / shorthand folding / `|important` array split / `has_call`
        // aggregation all live in the constructors).
        let mut class_dirs: Vec<(String, Option<PreparedTemplateValue>)> = Vec::new();
        let mut style_dirs: Vec<StyleDirectiveObjectEntry> = Vec::new();
        for attr in attrs {
            match attr {
                AttrIr::Static { name, value } => {
                    // On the `<svelte:element>` lone-class fast path the static
                    // `class` is the `$.set_class` pieces' BASE value — it does
                    // not fold (case-insensitive name, the official
                    // `toLowerCase()` rule).
                    if opts.skip_class && name.eq_ignore_ascii_case("class") {
                        continue;
                    }
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
                    items.push(AttributeEffectItem::Entry(format!(
                        "{}: {v}",
                        object_key(name)
                    )));
                }
                AttrIr::Dynamic { name, expr } => {
                    // A co-located ordinary attribute value routes through the shared
                    // prepared builder (`build_attribute_value` → `build_expression`,
                    // then the per-effect memoize on `has_call`). An EVENT attribute
                    // (`on*` name) whose value is a FUNCTION expression hoists to a
                    // stable local instead (the official handler-stability rule) —
                    // a function expression never triggers the wrap.
                    let prepared = self.prepare_template_value(
                        AuthoredExpr(*expr),
                        AuthoredValueSurface::AttributeEffectValue,
                    )?;
                    if name.starts_with("on")
                        && matches!(prepared.facts().root_kind, UnwrappedRootKind::Function)
                    {
                        items.push(AttributeEffectItem::Event {
                            prop: name.clone(),
                            handler: prepared,
                        });
                    } else {
                        items.push(AttributeEffectItem::Attr {
                            prop: object_key(name),
                            value: AttrValue::single_authored(prepared),
                        });
                    }
                }
                AttrIr::Mixed { name, parts } => {
                    // A mixed attribute folds as the `` `lit${expr ?? ''}lit` ``
                    // template with each authored chunk PREPARED + memoized on
                    // `has_call` (the emitter's per-effect memoizer).
                    let (value, _) =
                        self.mixed_attr_value(parts, AuthoredValueSurface::AttributeEffectValue)?;
                    items.push(AttributeEffectItem::Attr {
                        prop: object_key(name),
                        value,
                    });
                }
                AttrIr::Spread { expr } => {
                    // A spread operand is RAW with respect to `build_expression` (the
                    // Raw policy row) but MEMOIZABLE — official `SpreadAttribute`
                    // visits raw and the attribute-effect `Memoizer` still receives
                    // it. The value-position printer keeps a top-level
                    // `SequenceExpression` operand's one paren pair (`{...(a, b)}`)
                    // so it stays one spread value.
                    let prepared = self.prepare_template_value(
                        AuthoredExpr(*expr),
                        AuthoredValueSurface::ElementSpreadOperand,
                    )?;
                    items.push(AttributeEffectItem::Spread { value: prepared });
                }
                AttrIr::Class { name, condition } if !opts.skip_class => {
                    // A `class:` condition is RAW (official visits it without
                    // `build_expression`); the synthesized object memoizes as a
                    // whole on the merged `has_call`. The key quoting + the
                    // object-property SHORTHAND (`class:on` / `class:on={on}`
                    // fold to `{ on }` when the value text equals the key) live
                    // in the sealed `class_directives` constructor.
                    let cond = match condition {
                        Some(e) => Some(self.prepare_template_value(
                            AuthoredExpr(*e),
                            AuthoredValueSurface::ClassDirectiveCondition,
                        )?),
                        None => None,
                    };
                    class_dirs.push((name.clone(), cond));
                }
                AttrIr::Class { .. } => {}
                AttrIr::Style {
                    property,
                    value,
                    important,
                } => {
                    // A `style:` inner value WRAPS (official
                    // `build_style_directives_object` routes each value through
                    // `build_attribute_value` → `build_expression`); a `|important`
                    // directive switches the WHOLE `[$.STYLE]` entry to the
                    // `[{ … }, { prop: 'v' }]` array form (the split lives in the
                    // sealed `style_directives` constructor).
                    let typed = match value {
                        StyleDirectiveValue::Expr(e) => {
                            StyleDirectiveObjectValue::Prepared(self.prepare_template_value(
                                AuthoredExpr(*e),
                                AuthoredValueSurface::StyleDirectiveValue,
                            )?)
                        }
                        // A static-text style value folds as the quoted string literal
                        // (`[$.STYLE]: { color: 'red' }`), quoted by the constructor.
                        StyleDirectiveValue::Text(text) => {
                            StyleDirectiveObjectValue::StaticText(text.clone())
                        }
                        // A MIXED text+interpolation value folds as the template-literal
                        // `[$.STYLE]: { color: `a${(…) ?? ''}b` }` with each authored
                        // chunk PREPARED (wrapping inline); the whole object memoizes.
                        StyleDirectiveValue::Mixed(parts) => {
                            let (mixed, _) = self.mixed_attr_value(
                                parts,
                                AuthoredValueSurface::StyleDirectiveValue,
                            )?;
                            StyleDirectiveObjectValue::Mixed(mixed)
                        }
                    };
                    style_dirs.push(StyleDirectiveObjectEntry {
                        property: property.clone(),
                        value: typed,
                        important: *important,
                    });
                }
                _ => {}
            }
        }
        // The analyze-phase SYNTHESIZED empty `class` / `style` attributes (official
        // `phases/2-analyze/index.js`) append AFTER the real attributes and BEFORE
        // the `[$.CLASS]` / `[$.STYLE]` directive entries.
        if opts.synth_class {
            items.push(AttributeEffectItem::Entry("class: ''".to_string()));
        }
        if opts.synth_style {
            items.push(AttributeEffectItem::Entry("style: ''".to_string()));
        }
        // Append the merged `[$.CLASS]` then `[$.STYLE]` directive entries (after every
        // plain attribute / spread), matching the official spread-fold ordering —
        // synthesized through the sealed typed constructors (no free-form text).
        if let Some(obj) = SynthesizedTemplateValue::class_directives(&class_dirs) {
            items.push(AttributeEffectItem::ClassDirectives(obj));
        }
        if let Some(obj) = SynthesizedTemplateValue::style_directives(&style_dirs) {
            items.push(AttributeEffectItem::StyleDirectives(obj));
        }
        Ok(items)
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
        // PREPARE FIRST (official `build_expression` order): a legacy-wrapped
        // payload's getter returns the prepared sequence — a wrapped call is
        // NEVER elided (its zero-arg unthunk lives INSIDE the sequence's
        // `$.untrack(callee)` instead).
        let payload = self.prepare_template_value(
            super::client_legacy_value::AuthoredExpr(expr),
            super::client_legacy_value::AuthoredValueSurface::HtmlPayload,
        )?;
        // Thunk elision on the RAW carrier only: a DIRECT, non-optional,
        // zero-argument identifier call elides the `() => …` thunk to the bare
        // callee ONLY when the callee rewrites UNCHANGED (a plain / local /
        // demoted id, where `$.get` / `$$props` rewriting is a no-op). A
        // prop / signal callee rewrites to a member or `$.get(...)` call, so it
        // stays the thunk over the whole rewritten expression. The callee fact is
        // harvested from the analysis parse; the callee's REWRITTEN form is
        // computed through the shared rewriter (its own name source — never a
        // raw-source heuristic, never the un-rewritten callee).
        let analyzed = self.ir.analysis.expressions.get(expr);
        let getter_form = if payload.is_wrapped() {
            super::client_plan_types::HtmlGetterForm::PreparedThunk
        } else {
            match &analyzed.direct_zero_arg_call_callee {
                Some(callee_name) => {
                    let rewritten_callee = self.rewrite_identifier(callee_name, analyzed.scope)?;
                    if &rewritten_callee == callee_name {
                        // Plain / local / demoted callee — elide to the bare callee.
                        super::client_plan_types::HtmlGetterForm::ElidedCallee(rewritten_callee)
                    } else {
                        // The callee rewrote to a member / getter — keep the thunk, but
                        // render it as the REWRITTEN-CALLEE zero-arg call
                        // `() => <rewritten_callee>()`, NOT the blind whole-source
                        // rewrite (which would keep any author parens around the
                        // callee — `(render)()` → `() => ($$props.render)()`). Since
                        // the payload is a direct zero-arg call, its shape is exactly
                        // `<callee>()`, so rebuilding the call from the
                        // harvested+peeled callee drops the author parens to match
                        // official (`() => $$props.render()`).
                        super::client_plan_types::HtmlGetterForm::RebuiltCallThunk(rewritten_callee)
                    }
                }
                None => super::client_plan_types::HtmlGetterForm::PreparedThunk,
            }
        };
        let only_child = self.html_is_only_controlled_child(node_id);
        Ok(ClientRuntimeOp::Html {
            target: ClientNodeId(node_id.0),
            payload,
            getter_form,
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
