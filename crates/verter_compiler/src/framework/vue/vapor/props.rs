//! Dynamic DOM props become typed effect rows or one-time statement runs.
use std::borrow::Cow;

use super::effect::VaporEffect;
use super::expr::{ExprProvenance, RecordOptions, Resolution};
use super::ops::{NameKind, Part, Stmt};
use super::scope::VaporScope;
use super::VaporCompiler;
use crate::ast::types::PropFlags;
use crate::framework::vue::structure::{
    VueAttr, VueAttrKind, VueBuiltinDirective, VueCompileNodeId, VueNodeFlags,
};
use crate::template::binding::is_simple_ident;
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::template::vue_syntax::camelize;

impl<'c> VaporCompiler<'c> {
    pub(super) fn process_dynamic_props(
        &mut self,
        node: VueCompileNodeId,
        scope: &mut VaporScope<'c>,
    ) {
        let (element, oxc) = self.element_of(node);
        let inputs = self.inputs;
        let source = inputs.source.text;
        self.process_template_ref(node, scope);
        // Vue 3.6.0-rc.15 `getRuntimeHelper`: an SVG element's
        // attributes are written as attributes, flagged as SVG, because
        // its geometry IDL properties (`width`, `cx`, `viewBox`, ...)
        // are read-only `SVGAnimated*` objects a property write misses.
        // The keys with their own setter, and camelCase `aria*` DOM
        // properties, keep those setters on SVG elements too.
        let facts = inputs.structure.facts(node);
        let svg = facts.flags.has(VueNodeFlags::SVG_NAMESPACE);
        let tag = facts.tag.map_or("", |tag| inputs.structure.text(tag));
        for attr in inputs.structure.attrs(node) {
            if matches!(attr.prop_index, VueAttr::CACHED_PROP) {
                continue;
            }
            let prop = &element.props[attr.prop_index as usize];
            if attr.kind == VueAttrKind::On {
                self.process_event(node, attr, scope);
                continue;
            }
            if attr.kind == VueAttrKind::Static {
                continue;
            }
            let parsed = find_prop_oxc_exp(oxc, attr.prop_index as usize);
            let value = attr
                .value_span
                .map(|span| &source[span.start as usize..span.end as usize])
                .unwrap_or("");
            let start = attr.value_span.map_or(0, |span| span.start);
            match attr.kind {
                VueAttrKind::Builtin(VueBuiltinDirective::Model) => {
                    self.process_model(node, attr, scope);
                    continue;
                }
                VueAttrKind::Builtin(VueBuiltinDirective::Show) => {
                    let node_ref = scope.ensure_node_ref(&mut self.state.counters);
                    let expr = self.record_expression(value, start, parsed);
                    let run = self.state.runs.commit(&mut vec![
                        Part::Text("_applyVShow("),
                        Part::Name(NameKind::Node, node_ref),
                        Part::Text(", () => ("),
                        Part::Run(expr),
                        Part::Text("))"),
                    ]);
                    scope.child_statements.push(Stmt::Run(run));
                    self.state.use_helper(VaporHelper::ApplyVShow);
                    continue;
                }
                VueAttrKind::Builtin(VueBuiltinDirective::Html) => {
                    let node_ref = scope.ensure_node_ref(&mut self.state.counters);
                    let expr = self.record_expression(value, start, parsed);
                    self.add_prop_effect(
                        scope,
                        VaporEffect::Html { node_ref, expr },
                        VaporHelper::SetHtml,
                        false,
                    );
                    continue;
                }
                VueAttrKind::Builtin(VueBuiltinDirective::Text | VueBuiltinDirective::Memo) => {
                    continue
                }
                _ => {}
            }
            let arg = attr
                .arg_span
                .map(|span| &source[span.start as usize..span.end as usize]);
            if attr.kind == VueAttrKind::Bind && arg == Some("key") {
                continue;
            }
            if attr.kind == VueAttrKind::Bind && attr.dynamic_arg {
                if let Some(span) = attr.arg_span {
                    verter_debug_assert!(
                        span.end >= span.start + 2,
                        "dynamic arguments include brackets"
                    );
                    let inner_start = span.start + 1;
                    let inner_end = span.end - 1;
                    let key_ast = oxc
                        .and_then(|element| element.prop(attr.prop_index as usize))
                        .and_then(|prop| prop.arg.as_ref());
                    let key = self.record_expression_with(
                        &source[inner_start as usize..inner_end as usize],
                        inner_start,
                        key_ast,
                        RecordOptions {
                            force_js: inputs.options.force_js,
                            resolution: Resolution::Simple,
                            provenance: ExprProvenance::Authored,
                        },
                    );
                    let value = self.record_expression(value, start, parsed);
                    // A nullish runtime key names no attribute: Vue guards the
                    // computed key with `|| ""`, parenthesizing anything that
                    // is not a bare identifier.
                    let simple_key =
                        is_simple_ident(source[inner_start as usize..inner_end as usize].trim());
                    let (open, close) = if simple_key {
                        ("{ [", " || \"\"]: ")
                    } else {
                        ("{ [(", ") || \"\"]: ")
                    };
                    let expr = self.state.runs.commit(&mut vec![
                        Part::Text(open),
                        Part::Run(key),
                        Part::Text(close),
                        Part::Run(value),
                        Part::Text(" }"),
                    ]);
                    let node_ref = scope.ensure_node_ref(&mut self.state.counters);
                    self.add_prop_effect(
                        scope,
                        VaporEffect::DynamicProps {
                            node_ref,
                            expr,
                            svg,
                        },
                        VaporHelper::SetDynamicProps,
                        false,
                    );
                }
                continue;
            }
            if value.is_empty() && arg.is_none() {
                continue;
            }
            if attr.kind == VueAttrKind::Bind && arg.is_none() {
                let node_ref = scope.ensure_node_ref(&mut self.state.counters);
                let expr = self.record_expression(value, start, parsed);
                self.add_prop_effect(
                    scope,
                    VaporEffect::DynamicProps {
                        node_ref,
                        expr,
                        svg,
                    },
                    VaporHelper::SetDynamicProps,
                    false,
                );
                continue;
            }
            // A directive without a supported setter still reserves its reference.
            let node_ref = scope.ensure_node_ref(&mut self.state.counters);
            let Some(arg) = arg else {
                continue;
            };
            let expr = if attr.value_span.is_none() {
                let camelized = camelize(arg);
                let provenance = match &camelized {
                    Cow::Borrowed(_) => ExprProvenance::Authored,
                    Cow::Owned(_) => ExprProvenance::Generated,
                };
                self.record_expression_with(
                    &camelized,
                    attr.arg_span.expect("argument exists").start,
                    None,
                    RecordOptions {
                        force_js: inputs.options.force_js,
                        resolution: Resolution::Simple,
                        provenance,
                    },
                )
            } else if value.trim().is_empty() {
                self.state.runs.commit(&mut vec![Part::Text("\"\"")])
            } else {
                self.record_expression(value, start, parsed)
            };
            let is_const = self.resolver.all_bindings_const_props(
                parsed.and_then(|expression| expression.bindings.as_ref()),
            );
            let mut force_prop = false;
            let mut force_attr = false;
            for modifier in &prop.modifiers {
                match &source[modifier.start as usize..modifier.end as usize] {
                    "prop" => force_prop = true,
                    "attr" => force_attr = true,
                    _ => {}
                }
            }
            let special = if arg == "class" && element.prop_flag.has(PropFlags::HasDynamicClass) {
                Some((
                    VaporEffect::Class {
                        node_ref,
                        expr,
                        svg,
                    },
                    VaporHelper::SetClass,
                ))
            } else if arg == "style" && element.prop_flag.has(PropFlags::HasDynamicStyle) {
                Some((VaporEffect::Style { node_ref, expr }, VaporHelper::SetStyle))
            } else if arg == "value" && can_set_value_directly(tag) {
                Some((VaporEffect::Value { node_ref, expr }, VaporHelper::SetValue))
            } else if arg == "innerHTML" {
                Some((VaporEffect::Html { node_ref, expr }, VaporHelper::SetHtml))
            } else if arg == "textContent" {
                Some((
                    VaporEffect::ElementText { node_ref, expr },
                    VaporHelper::SetElementText,
                ))
            } else {
                None
            };
            let dom_prop = || {
                (
                    VaporEffect::DomProp {
                        node_ref,
                        attr: arg,
                        expr,
                    },
                    VaporHelper::SetDomProp,
                )
            };
            let attr = || {
                (
                    VaporEffect::Attr {
                        node_ref,
                        attr: arg,
                        expr,
                        svg,
                    },
                    VaporHelper::SetAttr,
                )
            };
            let (effect, helper) = if force_prop {
                special.unwrap_or_else(dom_prop)
            } else if force_attr {
                attr()
            } else if let Some(special) = special {
                special
            } else if is_camel_aria(arg) {
                dom_prop()
            } else if svg || should_set_as_attr(tag, arg) || arg.contains('-') {
                attr()
            } else {
                (
                    VaporEffect::Prop {
                        node_ref,
                        attr: arg,
                        expr,
                    },
                    VaporHelper::SetProp,
                )
            };
            self.add_prop_effect(scope, effect, helper, is_const);
        }
    }

    fn add_prop_effect(
        &mut self,
        scope: &mut VaporScope<'c>,
        effect: VaporEffect<'c>,
        helper: VaporHelper,
        is_const: bool,
    ) {
        if is_const {
            let mut parts = Vec::new();
            super::output::record_effect(&effect, &mut parts);
            scope
                .child_statements
                .push(Stmt::Run(self.state.runs.commit(&mut parts)));
        } else {
            scope.own_effects.push(self.state.add_effect(effect));
        }
        self.state.use_helper(helper);
    }
}

/// Vue 3.6.0-rc.15 `canSetValueDirectly`: every element but `<progress>`
/// and custom elements takes `value` through `setValue`.
fn can_set_value_directly(tag: &str) -> bool {
    !tag.eq_ignore_ascii_case("progress") && !tag.contains('-')
}

/// A camelCase ARIA reflection (`ariaLabel`) is a DOM property.
fn is_camel_aria(key: &str) -> bool {
    key.as_bytes()
        .windows(5)
        .any(|window| window.starts_with(b"aria") && window[4].is_ascii_uppercase())
}

/// Vue 3.6.0-rc.15 `shouldSetAsAttr`: DOM properties that are read-only or
/// differ from their attribute on some elements are written as attributes.
fn should_set_as_attr(tag: &str, key: &str) -> bool {
    let is = |name: &str| tag.eq_ignore_ascii_case(name);
    match key {
        "spellcheck" | "draggable" | "translate" | "autocorrect" | "form" => true,
        "list" => is("input"),
        "type" => is("textarea"),
        "width" | "height" => is("img") || is("video") || is("canvas") || is("source"),
        "sandbox" => is("iframe"),
        _ => false,
    }
}
