//! Element attribute / class / style op-projection helpers for the narrow client
//! plan, extracted from `client_plan` to keep it under the file-size guard.
//!
//! These [`SupportedClientIr`] methods project an accepted element's dynamic plain
//! attribute / non-static-property / `class` / `style` surfaces into their narrow
//! [`ClientRuntimeOp`]s (`ReactiveAttr` / `SetClass` / `SetStyle`), reading each
//! contributor's STRUCTURED [`AttrValue`] (+ `has_state` / `has_call`) through the
//! shared attribute-value + source-preserving rewrite helpers the sibling modules own.

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::{
    escape_template_text, js_single_quoted, object_key, object_property, style_object,
};
use super::client_legacy_value::{AuthoredExpr, AuthoredValueSurface};
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{
    AttrValue, AttrValuePart, ClientDynAttrEmit, ClientNodeId, ClientRuntimeOp,
    PlannedTemplateValue,
};
use super::client_shapes::ClientDynamicAttrShape;
use super::entity_decode::escape_decoded_attr;
use super::ir::{
    AttrIr, IrNode, NodeId, NonStaticPropertyKind, NonStaticPropertyValue, StyleDirectiveValue,
};
use super::synthesized_value::SynthesizedTemplateValue;
use verter_span::Span;

impl<'a> SupportedClientIr<'a> {
    /// The IR element node for a target [`NodeId`] (a non-element target is a
    /// classifier/plan divergence — fail closed defensively).
    pub(super) fn element_for(
        &self,
        target: NodeId,
    ) -> Result<&super::ir::ElementIr, UnsupportedSvelteRuntimeSurface> {
        match self.ir.node(target) {
            IrNode::Element(el) => Ok(el),
            _ => Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                name: "non-element-attr-target".to_string(),
                span: Span::new(0, 0),
            }),
        }
    }

    /// Project a dynamic PLAIN attribute (`AttrIr::Dynamic` / `AttrIr::Mixed`,
    /// `AttrOpKind::Plain`) into its narrow [`ClientRuntimeOp::ReactiveAttr`]. The
    /// emission shape is re-derived from the (deterministic) name classifier — a DOM
    /// property write vs `$.set_attribute`. The value is the WHOLE attribute value
    /// (read from the element's `Dynamic` / `Mixed` attr, not the single op expr) —
    /// a `Dynamic` single expression or a `Mixed` `` `lit${expr ?? ''}lit` ``
    /// template literal. `has_state` is the official `metadata.expression.has_state`.
    pub(super) fn project_reactive_attr_op(
        &self,
        target: NodeId,
        name: &str,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        // The element's `Dynamic` / `Mixed` attribute under this name → its STRUCTURED
        // value (each expression carrying its `has_call` fact for the emit-time
        // memoizer) + `has_state`.
        let (value, has_state) =
            self.attr_value_for(el, name, AuthoredValueSurface::AttributeValue)?;
        // The write is REACTIVE when it references state OR `has_call` (a `has_call`
        // value is memoized into a `$N` placeholder that only the effect can bind — the
        // official `Memoizer.add` rule, which forces even a pure `String(plain_let)`
        // into the render `$.template_effect`).
        let reactive = has_state || value.has_call();
        // Re-derive the emission shape from the name (deterministic, matches the
        // classifier's recorded fact). The span is unused on the accept path.
        let shape = super::client_shapes::classify_dynamic_attr_shape(name, Span::new(0, 0))?;
        let emit = match shape {
            ClientDynamicAttrShape::SetAttribute { name } => {
                ClientDynAttrEmit::SetAttribute { name, value }
            }
            ClientDynamicAttrShape::DomProperty { prop } => {
                ClientDynAttrEmit::Property { prop, value }
            }
            // A PLAIN-kind op is never `autofocus` (autofocus is a `NonStaticProperty`
            // op, projected by `project_non_static_property_op`) — defensively refuse
            // rather than mis-emit it as a reactive write.
            ClientDynamicAttrShape::Autofocus
            // Class / style never reach here (they are `AttrOpKind::Class` / `Style`).
            | ClientDynamicAttrShape::Class
            | ClientDynamicAttrShape::Style => {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: name.to_string(),
                    span: Span::new(0, 0),
                });
            }
        };
        Ok(ClientRuntimeOp::ReactiveAttr {
            target: ClientNodeId(target.0),
            emit,
            reactive,
        })
    }

    /// Project a `NonStaticProperty` op (`autofocus` / media `muted`, static or
    /// dynamic) into its narrow [`ClientRuntimeOp::ReactiveAttr`]. `autofocus` →
    /// init-only `$.autofocus(node, value)`; a DOM property (`muted`) → `node.<name> =
    /// value`. The init value is `true` (a valueless attr), a literal, or the rewritten
    /// expression.
    pub(super) fn project_non_static_property_op(
        &self,
        target: NodeId,
        property: &super::ir::NonStaticPropertyOp,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        // The STRUCTURED value + whether it is reactive. A `Mixed` value retains its
        // FULL ordered literal+expr run; each expression carries `has_call` for the
        // emit-time memoizer.
        let (value, has_state) = self.non_static_property_value(&property.value)?;
        // `autofocus` is init-only regardless of state; a property write (`muted`)
        // joins the effect when its value is stateful OR `has_call` (the official rule
        // — a `has_call` value is memoized and can only live in the effect).
        let init_only = matches!(property.kind, NonStaticPropertyKind::Autofocus);
        let reactive = (has_state || value.has_call()) && !init_only;
        let emit = match property.kind {
            // `autofocus` is ALWAYS init-only `$.autofocus(node, value)` — even a
            // dynamic value (`autofocus={v}`) is read once at init, so it is NEVER
            // memoized; flatten the structured value to a plain emit string.
            NonStaticPropertyKind::Autofocus => ClientDynAttrEmit::Autofocus {
                value: self.flatten_init_attr_value(&value),
            },
            // A DOM property write (`video.muted = value`) — carries the structured
            // value so a `has_call` reactive value memoizes at emit time.
            NonStaticPropertyKind::DomProperty => ClientDynAttrEmit::Property {
                prop: super::client_allowlist::normalize_attribute(&property.name),
                value,
            },
        };
        Ok(ClientRuntimeOp::ReactiveAttr {
            target: ClientNodeId(target.0),
            emit,
            reactive,
        })
    }

    /// Build the STRUCTURED value of a non-static-property op (`autofocus` / `muted`),
    /// plus its `has_state`. A `Boolean` valueless attr is the constant `true`; a
    /// static literal is a quoted constant; a single `Expr` carries its `has_call`; a
    /// `Mixed` value retains its full literal+expr run (each expr with `has_call`).
    fn non_static_property_value(
        &self,
        value: &NonStaticPropertyValue,
    ) -> Result<(AttrValue, bool), UnsupportedSvelteRuntimeSurface> {
        match value {
            NonStaticPropertyValue::Boolean => Ok((AttrValue::Const("true".to_string()), false)),
            NonStaticPropertyValue::Literal(text) => {
                Ok((AttrValue::Const(js_single_quoted(text)), false))
            }
            NonStaticPropertyValue::Expr(expr) => {
                // A VALUE position (`defaultValue={…}`; the `$.autofocus(node, value)`
                // init likewise) — prepared through the sole authored-value entry:
                // official applies `build_expression` to the property value whether
                // it lands in the effect (`video.muted = $0`) or the one-shot init
                // (`$.autofocus(node, (…))`).
                let prepared = self.prepare_template_value(
                    AuthoredExpr(*expr),
                    AuthoredValueSurface::AttributeValue,
                )?;
                let has_state = prepared.facts().has_state;
                Ok((AttrValue::single_authored(prepared), has_state))
            }
            NonStaticPropertyValue::Mixed(parts) => {
                self.mixed_attr_value(parts, AuthoredValueSurface::AttributeValue)
            }
        }
    }

    /// Flatten a structured [`AttrValue`] for an INIT-only (`$.autofocus`) emit, where
    /// no effect-side memoizer runs. A `Single` value emits its (legacy-wrap-prepared)
    /// inline expression; a `Const` emits verbatim; a `Mixed` value builds the
    /// `` `lit${expr ?? ''}lit` `` template inline (no memoization, since an init-only
    /// value is read once — a prepared legacy wrap still embeds parenthesized, the
    /// official `$.autofocus(node, (…, $.untrack(…)))` shape).
    fn flatten_init_attr_value(&self, value: &AttrValue) -> String {
        match value {
            AttrValue::Const(text) => text.clone(),
            AttrValue::Single(PlannedTemplateValue::Authored(p)) => p.inline_expression(),
            AttrValue::Single(PlannedTemplateValue::Synthesized(s)) => s.raw_text().to_string(),
            AttrValue::Mixed(parts) => {
                let mut tmpl = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(text) => tmpl.push_str(&escape_template_text(text)),
                        AttrValuePart::Expr { value, .. } => {
                            let v = value.inline_expression();
                            tmpl.push_str(&format!("${{{v} ?? ''}}"));
                        }
                    }
                }
                tmpl.push('`');
                tmpl
            }
        }
    }

    /// Project the coalesced `$.set_class(node, is_html, value, css_hash, prev, next)`
    /// op for a regular element — the shared [`project_set_class_pieces`] merge over the
    /// element's attribute set, wrapped with the target node id. The emitter assembles
    /// the final call with the real DOM var (`is_html = 1`) + accumulator name.
    ///
    /// [`project_set_class_pieces`]: Self::project_set_class_pieces
    pub(super) fn project_set_class_op(
        &self,
        target: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        let pieces = self.project_set_class_pieces(target, &el.attrs)?;
        Ok(ClientRuntimeOp::SetClass {
            target: ClientNodeId(target.0),
            value: pieces.value,
            css_hash: pieces.css_hash,
            directives: pieces.directives,
            directives_has_call: pieces.directives_has_call,
            reactive: pieces.reactive,
            accumulator_stem: pieces.accumulator_stem,
        })
    }

    /// Project the SEMANTIC pieces of one coalesced `$.set_class` write over an
    /// element's typed attribute set. Merges the `class={…}` base attribute (if any —
    /// a missing base is the directive-synthesized `''`) with EVERY `class:` directive
    /// into ONE call, matching the official `build_set_class`.
    ///
    /// `target` is the HOST node — the DYNAMIC scope-class injection site reads
    /// its scope hash from the shared [`CssScopeFacts`] and applies the official
    /// `build_set_class` 3-way: a LITERAL empty/missing base becomes the bare
    /// hash literal; a LITERAL string base appends ` <hash>` (attr-escaped); any
    /// other base (a dynamic expression / mixed template / boolean `true`)
    /// leaves the value untouched and passes the hash as the `css_hash` arg.
    /// When no hash lands in `css_hash`, directives still force the official
    /// `null` placeholder (`!css_hash && next`). Host-independent: SHARED by the
    /// regular-element class op ([`Self::project_set_class_op`]) and the
    /// `<svelte:element>` lone-class fast path — one class-merge substrate; the emitters
    /// assemble the final call with their real host expression + `is_html` flag +
    /// accumulator name.
    ///
    /// [`CssScopeFacts`]: super::css::types::CssScopeFacts
    pub(super) fn project_set_class_pieces(
        &self,
        target: NodeId,
        attrs: &[AttrIr],
    ) -> Result<super::client_plan_types::SetClassPieces, UnsupportedSvelteRuntimeSurface> {
        // The scope hash for THIS element — `Some` iff the selector-to-template
        // matcher marked the host node scoped (the shared per-element read).
        let scope_hash = self
            .css_scope
            .as_ref()
            .and_then(|facts| facts.hash_for(target));
        // Whether the scope hash was folded INTO the value literal (the first
        // two arms of the official 3-way); when it was not and the element is
        // scoped, the hash rides the `css_hash` argument instead.
        let mut hash_in_value = false;
        // The base `class` attribute (a `Static` / `Dynamic` / `Mixed` named `class` —
        // matched case-insensitively, the official `get_attribute_name` normalization),
        // and every `class:` directive, in source order.
        let mut base_value: Option<AttrValue> = None;
        let mut base_has_state = false;
        let mut directives: Vec<(String, String)> = Vec::new();
        let mut dir_has_state = false;
        let mut directives_has_call = false;
        for attr in attrs {
            match attr {
                AttrIr::Static { name, value } if name.eq_ignore_ascii_case("class") => {
                    // A static `class` consumed as the `$.set_class` BASE value is a
                    // runtime JS-STRING argument (NOT a baked skeleton attr) carrying
                    // the DECODED semantic value the producer boundary stored
                    // (`class="a&amp;b"` → base `'a&b'` — read via `as_str`, never a
                    // second decode). A VALUELESS `class` (`value: None` —
                    // `<div class class:on={c}>`) is the RAW boolean base `true`,
                    // distinct from a present empty-string `class=""` (`Some("")`)
                    // which stays `''`.
                    //
                    // The DYNAMIC scope-class injection's literal arms (official
                    // `build_set_class`): a scoped element's EMPTY literal becomes
                    // the bare hash; a non-empty literal appends ` <hash>` over the
                    // ATTR-ESCAPED value (`escape_html(value, true)` — the official
                    // literal-arm escape, ESCAPE-ONLY over the already-decoded
                    // value). The boolean `true` base is NOT a string literal — its
                    // hash rides the `css_hash` arg.
                    base_value = Some(match value {
                        None => AttrValue::Const("true".to_string()),
                        Some(v) => match scope_hash {
                            Some(hash) if v.value.is_empty() => {
                                hash_in_value = true;
                                AttrValue::Const(js_single_quoted(hash))
                            }
                            Some(hash) => {
                                hash_in_value = true;
                                AttrValue::Const(js_single_quoted(&format!(
                                    "{} {}",
                                    escape_decoded_attr(&v.value),
                                    hash
                                )))
                            }
                            None => AttrValue::Const(js_single_quoted(v.value.as_str())),
                        },
                    });
                }
                AttrIr::Dynamic { name, expr } if name.eq_ignore_ascii_case("class") => {
                    // The `$.set_class` BASE: (1) PREPARE the authored expression
                    // (official applies `build_expression` BEFORE `$.clsx`), (2) render
                    // its inline expression, (3) synthesize `$.clsx(<prepared>)` when
                    // `needs_clsx` fires, (4) the memoizer memoizes the synthesized
                    // whole. Preparation is NEVER called on the resulting `$.clsx(...)`.
                    //
                    // Official `Attribute.js` sets `needs_clsx` for a single-expression
                    // `class={…}` UNLESS the value is a `Literal` / `TemplateLiteral` /
                    // `BinaryExpression`: a `class={a + b}` string-concatenation, a
                    // `class={'x'}` literal, and a `` class={`a${b}`} `` template emit
                    // the value RAW (no `$.clsx` wrap); every other shape IS wrapped.
                    let prepared = self.prepare_template_value(
                        AuthoredExpr(*expr),
                        AuthoredValueSurface::ClassBase,
                    )?;
                    base_has_state |= prepared.facts().has_state;
                    base_value = Some(AttrValue::Single(
                        if super::reactive_analysis::class_value_needs_clsx(
                            prepared.facts().root_kind,
                        ) {
                            PlannedTemplateValue::Synthesized(SynthesizedTemplateValue::clsx(
                                &prepared,
                            ))
                        } else {
                            PlannedTemplateValue::Authored(prepared)
                        },
                    ));
                }
                AttrIr::Mixed { name, parts } if name.eq_ignore_ascii_case("class") => {
                    // A MIXED-string class (`class="a {x} b"`) is already a string
                    // template — official `needs_clsx` is FALSE for it, so it is NOT
                    // wrapped in `$.clsx` (verified against svelte@5.56.3). The
                    // structured value memoizes each EXPRESSION PART at emit time, not
                    // the whole rendered template.
                    let (mixed, st) =
                        self.mixed_attr_value(parts, AuthoredValueSurface::ClassBase)?;
                    base_value = Some(mixed);
                    base_has_state |= st;
                }
                AttrIr::Class { name, condition } => {
                    let cond = match condition {
                        Some(e) => {
                            // A `class:` condition is a RAW semantic role — it routes
                            // through the policy entry point and yields a raw carrier
                            // (official `build_class_directives_object` visits raw).
                            let prepared = self.prepare_template_value(
                                AuthoredExpr(*e),
                                AuthoredValueSurface::ClassDirectiveCondition,
                            )?;
                            dir_has_state |= prepared.facts().has_state;
                            directives_has_call |= prepared.has_call();
                            prepared.inline_expression()
                        }
                        // A value-less shorthand `class:foo` with no synthesized
                        // condition is a defensive empty (lowering always synthesizes
                        // one) — skip it.
                        None => continue,
                    };
                    directives.push((object_key(name), cond));
                }
                _ => {}
            }
        }
        let has_directives = !directives.is_empty();
        // The `value` arg: the structured base value, or the directive-synthesized
        // `''` when only directives are present — which, on a SCOPED element, is
        // the official synthetic empty class whose empty literal becomes the bare
        // hash (the `value === '' → value = hash` arm).
        let value = base_value.unwrap_or_else(|| match scope_hash {
            Some(hash) => {
                hash_in_value = true;
                AttrValue::Const(js_single_quoted(hash))
            }
            None => AttrValue::Const("''".to_string()),
        });
        let directives_has_call = directives_has_call && has_directives;
        // The op is REACTIVE when any contributor references state OR `has_call` (the
        // base or any directive) — the official rule that forces the effect +
        // memoization (and the accumulator) even over a pure-call/plain-let surface.
        let reactive = base_has_state || dir_has_state || value.has_call() || directives_has_call;
        // The `css_hash` arg (official `build_set_class`): the hash literal when
        // the element is scoped and the hash did NOT fold into the value (a
        // dynamic/mixed/boolean-true base); else the `null` placeholder when
        // directives are present (`!css_hash && next`); else absent.
        let css_hash = match scope_hash {
            Some(hash) if !hash_in_value => Some(js_single_quoted(hash)),
            _ => has_directives.then(|| "null".to_string()),
        };
        // The directives object `{ foo: cond, ... }` (a same-named condition
        // collapses to the official printer's shorthand `{ foo }` via the
        // shared `object_property`); absent when no directives.
        let directives_obj = has_directives.then(|| {
            let entries = directives
                .iter()
                .map(|(k, v)| object_property(k, v))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {entries} }}")
        });
        // The reactive-directive path needs the `let classes;` accumulator (used for
        // BOTH the `prev` arg and the `<name> =` assignment); a non-reactive directive
        // path passes `{}` as `prev` (no accumulator).
        let accumulator_stem = (has_directives && reactive).then_some("classes");
        Ok(super::client_plan_types::SetClassPieces {
            value,
            css_hash,
            directives: directives_obj,
            directives_has_call,
            reactive,
            accumulator_stem,
        })
    }

    /// Project the coalesced `$.set_style(node, value, prev, next)` op for an element
    /// . Merges the `style={…}` base attribute (if any) with EVERY `style:`
    /// directive into ONE call, matching the official `build_set_style`. The
    /// `|important` directives split into the `[normal, important]` array `next`;
    /// custom / hyphenated property keys are quoted.
    pub(super) fn project_set_style_op(
        &self,
        target: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        let mut base_value: Option<AttrValue> = None;
        let mut base_has_state = false;
        // Normal + important directive entries (key already quoted as needed).
        let mut normal: Vec<(String, String)> = Vec::new();
        let mut important: Vec<(String, String)> = Vec::new();
        let mut dir_has_state = false;
        let mut directives_has_call = false;
        for attr in &el.attrs {
            match attr {
                AttrIr::Static { name, value } if name.eq_ignore_ascii_case("style") => {
                    // A static `style` consumed as the `$.set_style` BASE value is a
                    // runtime JS-STRING argument (NOT a baked skeleton attr) carrying
                    // the producer-DECODED value (`style="q:'&quot;'"` → base
                    // `'q:\'"\''` — read via `as_str`, never a second decode). A
                    // VALUELESS `style` (`value: None` — `<div style style:color={c}>`) is the
                    // RAW boolean base `true`, distinct from a present empty-string `style=""`
                    // (`Some("")`) which stays `''`.
                    base_value = Some(match value {
                        None => AttrValue::Const("true".to_string()),
                        Some(v) => AttrValue::Const(js_single_quoted(v.value.as_str())),
                    });
                }
                AttrIr::Dynamic { name, expr } if name.eq_ignore_ascii_case("style") => {
                    // The `$.set_style` BASE value is a VALUE position, PREPARED
                    // through the sole authored-value entry (official routes it
                    // through `build_expression` then memoizes on `has_call`).
                    let prepared = self.prepare_template_value(
                        AuthoredExpr(*expr),
                        AuthoredValueSurface::StyleBase,
                    )?;
                    base_has_state |= prepared.facts().has_state;
                    base_value = Some(AttrValue::single_authored(prepared));
                }
                AttrIr::Mixed { name, parts } if name.eq_ignore_ascii_case("style") => {
                    // The structured mixed value memoizes each EXPRESSION PART at emit
                    // time, not the whole rendered template.
                    let (mixed, st) =
                        self.mixed_attr_value(parts, AuthoredValueSurface::StyleBase)?;
                    base_value = Some(mixed);
                    base_has_state |= st;
                }
                AttrIr::Style {
                    property,
                    value,
                    important: is_important,
                } => {
                    let v = match value {
                        StyleDirectiveValue::Expr(e) => {
                            // A `style:` inner value WRAPS (official
                            // `build_style_directives_object` routes each value
                            // through `build_attribute_value` → `build_expression`);
                            // the synthesized directive OBJECT then memoizes as a
                            // whole on the merged `has_call`.
                            let prepared = self.prepare_template_value(
                                AuthoredExpr(*e),
                                AuthoredValueSurface::StyleDirectiveValue,
                            )?;
                            dir_has_state |= prepared.facts().has_state;
                            directives_has_call |= prepared.has_call();
                            prepared.inline_expression()
                        }
                        // A static-text style value folds as a quoted string literal
                        // (`{ color: 'red' }`) — no state / call flags.
                        StyleDirectiveValue::Text(text) => js_single_quoted(text),
                        // A MIXED text+interpolation value (`style:color="a{x}b"`) folds as
                        // the template-literal `` `a${x ?? ''}b` `` with each authored
                        // chunk PREPARED (wrapping inline) and NO per-chunk memoizer —
                        // official memoizes the whole directive object, not the chunks.
                        StyleDirectiveValue::Mixed(parts) => {
                            let (mixed, st) = self.mixed_attr_value(
                                parts,
                                AuthoredValueSurface::StyleDirectiveValue,
                            )?;
                            dir_has_state |= st;
                            // A `has_call` interpolation inside the template forces the
                            // effect (the official memoizer rule), exactly as a base mixed
                            // value does.
                            directives_has_call |= mixed.has_call();
                            mixed.folded_text()
                        }
                    };
                    let entry = (object_key(property), v);
                    if *is_important {
                        important.push(entry);
                    } else {
                        normal.push(entry);
                    }
                }
                _ => {}
            }
        }
        let has_directives = !normal.is_empty() || !important.is_empty();
        // The `value` arg: the structured base value, or `''` when only directives are
        // present (the official `build_set_style` passes an empty-string base then).
        let value = base_value.unwrap_or_else(|| AttrValue::Const("''".to_string()));
        let directives_has_call = directives_has_call && has_directives;
        // The op is REACTIVE when any contributor references state OR `has_call`.
        let reactive = base_has_state || dir_has_state || value.has_call() || directives_has_call;
        // The directives object, or the `[normal, important]` array when any
        // `|important` directive is present (the official `build_style_directives_object`).
        let directives_obj = has_directives.then(|| {
            let normal_obj = style_object(&normal);
            if important.is_empty() {
                normal_obj
            } else {
                format!("[{}, {}]", normal_obj, style_object(&important))
            }
        });
        let accumulator_stem = (has_directives && reactive).then_some("styles");
        Ok(ClientRuntimeOp::SetStyle {
            target: ClientNodeId(target.0),
            value,
            directives: directives_obj,
            directives_has_call,
            reactive,
            accumulator_stem,
        })
    }
}
