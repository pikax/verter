//! The Svelte IDE `<svelte:*>` special-element + namespace projection
//! (F8/F9/F10).
//!
//! `<svelte:component>` / `<svelte:self>` (F8) project through the
//! `__verter_dynamic_component` prelude checker; `<svelte:fragment>` (F9)
//! projects its children transparently with a void-checked `slot` literal;
//! `<svelte:options namespace="svg|mathml">` (F10) selects the svg/mathml shim
//! entrypoint via the per-file `@jsxImportSource` pragma variant (chosen at the
//! module head) and the options element is stripped. The static `<svelte:head>`
//! / `<svelte:window>` / … family rewrites to a conservative intrinsic carrier.
//!
//! This module is a continuation of [`super`]'s `TemplateProjector` impl —
//! extracted for file size; it accesses the parent module's private projector
//! type + helpers through `use super::*`.

use super::*;
use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, VariableDeclarator};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

impl TemplateProjector<'_, '_> {
    /// Project a `<svelte:*>` special element.
    pub(super) fn project_special_element(&mut self, el: &SvelteElement, kind: SvelteSpecialKind) {
        match kind {
            // F8 `<svelte:component this={C}>` / `<svelte:self>` — SUPPORTED. Both
            // route through the `__verter_dynamic_component` checker over a
            // class-shaped component value (`this`'s expression, or the LOCAL
            // self contract for `<svelte:self>`).
            SvelteSpecialKind::Component => self.project_dynamic_component(el),
            SvelteSpecialKind::SelfRef => self.project_self_reference(el),
            // F9 `<svelte:fragment slot="x">…</svelte:fragment>` — SUPPORTED. The
            // children project UNWRAPPED (transparent like `{#key}`); the `slot`
            // literal is void-checked.
            SvelteSpecialKind::Fragment => self.project_fragment(el),
            // F10 `<svelte:options …>` — compiler metadata, no JSX surface. The
            // `namespace="svg|mathml"` selection is read at the module head (it
            // selects the prelude pragma variant); the element itself is stripped.
            SvelteSpecialKind::Options => self.strip_options_element(el),
            SvelteSpecialKind::Unknown => {
                self.push_diag(el.open_span, UnsupportedKind::Unknown);
                self.neutralize_element(el);
            }
            // Head / Window / Document / Body / Element / Boundary: rewrite the
            // `<svelte:foo>` tag name to a lowercase intrinsic so the JSX
            // intrinsic table types it conservatively, keep attributes.
            _ => {
                self.rewrite_special_to_intrinsic(el);
                for attr in &el.attributes {
                    self.project_attribute(el, attr);
                }
                for child in &el.children {
                    self.project_node(child);
                }
            }
        }
    }

    /// Project a `<svelte:component this={C} …>` (F8) through the
    /// `__verter_dynamic_component` checker.
    ///
    /// The `this` expression is checked class-shaped, and the remaining
    /// attributes are checked against the component's own `$props` by rendering
    /// them as JSX on a synthesized component local. The projection shape
    /// (self-closing example):
    /// ```text
    /// {(() => { const __VerterDyn = __verter_dynamic_component(C);
    ///           return (<__VerterDyn prop={x} />); })()}
    /// ```
    /// A NON-component `this` FAILS the constructor constraint; a wrong `prop`
    /// FAILS the props bag type. A missing / non-expression `this` neutralizes
    /// the element (no bound component — keep the file valid).
    fn project_dynamic_component(&mut self, el: &SvelteElement) {
        let Some(this_expr) = find_this_expression(el) else {
            // No `this={…}` — nothing to check; render children transparently.
            self.neutralize_element(el);
            return;
        };
        // F11 (P1-3): a store-sub in the `this={$store}` value is rewritten; F6: an
        // await-EXPRESSION (`this={await load()}`) is ALSO rewritten through the
        // PromiseLike helper. The F8 IIFE re-emits the `this` value as TEXT
        // (interpolated into `__verter_dynamic_component((…))`), so both rewrites
        // are applied to the sliced text (the original `this` bytes are overwritten
        // wholesale — no independent mapped chunk to compose with). The text path
        // carries no source span, so the INFORMATIONAL await diagnostic is recorded
        // separately against the ORIGINAL absolute `this` expression span — a raw
        // `await` here would leak into the SYNC render IIFE (invalid TSX), so this
        // markup position is await-safe like every other.
        self.record_await_diagnostics_in(this_expr);
        let component = self.rewrite_store_subs_in_text(self.slice(this_expr).trim());
        self.emit_dynamic_component(el, &component, Some(find_this_attr_span(el)));
    }

    /// Project a `<svelte:self …>` (F8) against the LOCAL self-component
    /// contract.
    ///
    /// The self component is a synthesized class-shaped value `__verter_self`
    /// whose `$props` is `__VerterSelfProps` — a module-scope type derived
    /// SYNTACTICALLY from THIS component's own `$props()` annotation (LOCAL, no
    /// component-metadata resolution). The element then routes through the same
    /// dynamic-component checker, so a wrong self-prop FAILS against the local
    /// contract.
    fn project_self_reference(&mut self, el: &SvelteElement) {
        // `<svelte:self>` has no `this` attribute — bind the synthesized self
        // value `__verter_self` (declared in the render-scope preamble).
        self.needs_self_contract = true;
        self.emit_dynamic_component(el, "__verter_self", None);
    }

    /// Emit the shared dynamic-component projection for `<svelte:component>` /
    /// `<svelte:self>`: wrap the element in an IIFE binding the component to a
    /// local checked by `__verter_dynamic_component`, render the remaining
    /// attributes + children on that local, and strip the `this` attribute.
    ///
    /// `component` is the checked component expression text (the `this` value,
    /// or `__verter_self`). `this_attr` is the span of the `this={…}` attribute
    /// to strip (None for `<svelte:self>`, which has no `this`).
    fn emit_dynamic_component(
        &mut self,
        el: &SvelteElement,
        component: &str,
        this_attr: Option<Span>,
    ) {
        const LOCAL: &str = "__VerterDyn";
        // Strip the `this={…}` attribute (it is not a JSX attribute).
        if let Some(span) = this_attr {
            remove_span(self.ct, span);
        }
        // Overwrite the whole open-tag head `<svelte:component` / `<svelte:self`
        // (the `<` THROUGH the name) with the IIFE opener + the renamed JSX tag
        // `<__VerterDyn`. One overwrite avoids any left/right insertion-ordering
        // interplay at a template-leading element's boundary. The component
        // expression is PARENTHESIZED so a `this={a, b}` sequence expression
        // stays ONE argument to the helper (a bare `(a, b)` would split into two).
        self.ct.overwrite(
            el.open_span.start,
            el.name_span.end,
            &format!(
                "{{(() => {{ const {LOCAL} = __verter_dynamic_component(({component})); return (<{LOCAL}"
            ),
        );
        // Project the remaining attributes (events, binds, directives, …) and
        // children — they render on the synthesized local exactly as on a
        // regular component tag.
        for attr in &el.attributes {
            // Skip the `this` attribute — it was stripped above.
            if Some(attr.span) == this_attr {
                continue;
            }
            self.project_attribute(el, attr);
        }
        for child in &el.children {
            self.project_node(child);
        }
        // Rewrite the matching close tag to the local and close the IIFE AFTER
        // it. A self-closing element closes the IIFE right after the open tag.
        if el.self_closing {
            self.ct.append_left(el.open_span.end, "); })()}");
        } else if let Some((close_start, close_end)) = self.matching_close_tag_span(el) {
            // Rewrite the `</name>` name to the local, then close the IIFE after
            // the full close tag.
            self.rewrite_close_at(close_start, el.name.len(), LOCAL);
            self.ct.append_left(close_end, "); })()}");
        } else {
            // Unterminated element — close the IIFE after the open tag so the
            // projected module stays a valid expression.
            self.ct.append_left(el.open_span.end, "); })()}");
        }
    }

    /// Project a `<svelte:fragment slot="x">…</svelte:fragment>` (F9) —
    /// transparent children + a void-checked `slot` literal.
    ///
    /// The fragment wrapper carries no rendered surface (it is a slot-grouping
    /// construct): its children project UNWRAPPED (like `{#key}`), the open +
    /// close tags are removed, and the `slot` value is void-checked once so a
    /// dynamic `slot={expr}` stays type-checked. Slot-NAME precision is NOT
    /// claimed here (an owner-gated `$slots` contract).
    fn project_fragment(&mut self, el: &SvelteElement) {
        // Void-check the `slot` value (literal or expression) — emitted in place
        // of the open tag, then children render transparently as siblings.
        //
        // A DYNAMIC `slot={expr}` is a MARKUP-EXPRESSION position: the value is
        // emitted into `{__verter_void(EXPR)}` inside the SYNC render fn, so a
        // store-sub (`slot={$name}`) is rewritten to its read helper AND a markup
        // await (`slot={await load()}`) is rewritten to `__verter_await_expr(…)`
        // — a raw `await` here would be INVALID TSX. Both rewrites route the same
        // TEXT path as every other re-emitted markup expression; the INFORMATIONAL
        // await diagnostic anchors on the original absolute expression span.
        let void_check = match find_slot_value(self.source, el) {
            Some(SlotValue::Expression(span)) => {
                self.record_await_diagnostics_in(span);
                let expr = self.rewrite_store_subs_in_text(self.slice(span));
                format!("{{__verter_void({expr})}}")
            }
            Some(SlotValue::Literal(text)) => format!("{{__verter_void({text})}}"),
            None => String::new(),
        };
        // Replace the whole open tag with the void-check (no `<svelte:fragment`
        // residue). A self-closing fragment has no children.
        self.ct
            .overwrite(el.open_span.start, el.open_span.end, &void_check);
        for child in &el.children {
            self.project_node(child);
        }
        // Remove the matching `</svelte:fragment>` close tag.
        if !el.self_closing {
            self.remove_close_tag(el);
        }
    }

    /// Strip a `<svelte:options …>` element (F10) — it is compiler metadata with
    /// no JSX/type surface. The `namespace` selection is read at the module head
    /// (`detect_jsx_namespace`); here the element + its children are removed.
    fn strip_options_element(&mut self, el: &SvelteElement) {
        remove_span(self.ct, el.open_span);
        if !el.self_closing {
            // Remove any (unusual) children and the close tag.
            for child in &el.children {
                if let Some(span) = node_span(child) {
                    remove_span(self.ct, span);
                }
            }
            self.remove_close_tag(el);
        }
    }

    /// Remove the matching close tag `</name>` of `el` (depth-aware), leaving no
    /// residue. A no-op for a self-closing element.
    fn remove_close_tag(&mut self, el: &SvelteElement) {
        if el.self_closing {
            return;
        }
        if let Some((start, end)) = self.matching_close_tag_span(el) {
            self.ct.remove(start, end);
        }
    }

    /// Rewrite a `<svelte:foo ...>` open + close to a lowercase `<div>`
    /// intrinsic carrier (conservative typing) keeping the attribute run.
    fn rewrite_special_to_intrinsic(&mut self, el: &SvelteElement) {
        // Overwrite `svelte:foo` name with `div` so the element types through
        // the intrinsic table, on BOTH the open AND the matching close tag —
        // an `</svelte:window>` residue would be invalid TSX.
        self.ct
            .overwrite(el.name_span.start, el.name_span.end, "div");
        self.rewrite_close_tag_name(el, "div");
    }

    /// Project an element to an empty void-checked fragment (its expressions
    /// are preserved in children but the element wrapper is neutralized).
    fn neutralize_element(&mut self, el: &SvelteElement) {
        // Rewrite the tag name to a fragment-safe `div` and keep children, on
        // BOTH the open AND close tag.
        if el.name_span.start < el.name_span.end {
            self.ct
                .overwrite(el.name_span.start, el.name_span.end, "div");
        }
        self.rewrite_close_tag_name(el, "div");
        for child in &el.children {
            self.project_node(child);
        }
    }
}

/// The expression span of the `this={…}` attribute on a `<svelte:component>`
/// (the dynamic-component value). `None` when absent / non-expression.
pub(super) fn find_this_expression(el: &SvelteElement) -> Option<Span> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain {
            name,
            value: Some(SvelteAttributeValue::Expression(expr)),
            ..
        } if name == "this" => Some(*expr),
        _ => None,
    })
}

/// The full span of the `this={…}` attribute (the run to strip from the open
/// tag). Falls back to an empty span when absent.
pub(super) fn find_this_attr_span(el: &SvelteElement) -> Span {
    el.attributes
        .iter()
        .find(|a| matches!(&a.kind, SvelteAttributeKind::Plain { name, .. } if name == "this"))
        .map(|a| a.span)
        .unwrap_or_else(|| Span::new(el.open_span.start, el.open_span.start))
}

/// The `slot` attribute value of a `<svelte:fragment slot=…>` (F9), classified
/// for void-checking. A static `slot="x"` yields a JS-ESCAPED string-literal
/// `Literal` (a valid TSX string — `serde_json::to_string` escapes
/// quotes/backslashes; a raw double-quote wrap is invalid TSX). A dynamic
/// `slot={expr}` yields the `Expression` SPAN (so the caller can route the markup
/// rewrite — store-subs / awaits — and record diagnostics against the absolute
/// span, NOT pre-format the raw bytes). `None` when there is no `slot` attribute
/// (an empty fragment) or for a `Mixed` text+interpolation literal (`slot="a{b}"`)
/// — its raw span is not a valid standalone expression and slot-NAME precision is
/// not claimed here, so no void-check is emitted rather than invalid TSX residue.
pub(super) enum SlotValue {
    /// A JS-escaped string literal (a static `slot="x"`), ready to emit verbatim.
    Literal(String),
    /// The source span of a dynamic `slot={expr}` value (the caller routes the
    /// markup rewrite + diagnostics).
    Expression(Span),
}

pub(super) fn find_slot_value(source: &str, el: &SvelteElement) -> Option<SlotValue> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name, value, .. } if name == "slot" => match value {
            // Static literal — JS-escape the raw text so quotes/backslashes produce
            // a VALID TSX string literal. `serde_json::to_string` yields a JSON
            // string, which is a valid JS/TS string literal.
            Some(SvelteAttributeValue::Text(span)) => {
                serde_json::to_string(&source[span.start as usize..span.end as usize])
                    .ok()
                    .map(SlotValue::Literal)
            }
            // A dynamic `slot={expr}` — return the SPAN so the caller routes the
            // markup rewrite (store-subs / awaits) and records diagnostics.
            Some(SvelteAttributeValue::Expression(span)) => Some(SlotValue::Expression(*span)),
            // A `Mixed` text+interpolation literal is not a valid standalone
            // expression — skip the void-check (no invalid residue).
            Some(SvelteAttributeValue::Mixed(_)) | None => None,
        },
        _ => None,
    })
}

/// Detect the JSX namespace (F10) from a top-level `<svelte:options
/// namespace="svg|mathml">` element in the template. Returns the default HTML
/// namespace when absent / unrecognised. Only a top-level options element
/// counts (Svelte requires `<svelte:options>` at component root).
pub(super) fn detect_jsx_namespace(source: &str, nodes: &[SvelteNode]) -> SvelteJsxNamespace {
    for node in nodes {
        if let SvelteNode::Element(el) = node {
            if matches!(
                el.kind,
                SvelteElementKind::Special(SvelteSpecialKind::Options)
            ) {
                if let Some(text) = find_namespace_option(source, el) {
                    return SvelteJsxNamespace::from_options_literal(&text);
                }
            }
        }
    }
    SvelteJsxNamespace::Html
}

/// The explicit `runes` mode forced by a top-level `<svelte:options runes={…}>`
/// (F12 legacy detection). Returns `Some(true)` for `runes` / `runes={true}`,
/// `Some(false)` for `runes={false}`, and `None` when no `<svelte:options runes>`
/// is present (the caller then falls back to rune-USAGE detection). Only a
/// top-level options element counts (Svelte requires it at component root).
pub(super) fn detect_forced_runes_option(source: &str, nodes: &[SvelteNode]) -> Option<bool> {
    for node in nodes {
        if let SvelteNode::Element(el) = node {
            if matches!(
                el.kind,
                SvelteElementKind::Special(SvelteSpecialKind::Options)
            ) {
                if let Some(v) = find_runes_option(source, el) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The `runes` option value on a `<svelte:options>` element: a valueless
/// `runes` boolean-shorthand is `true`; `runes={true}` / `runes={false}` read the
/// literal; any other form is treated as absent (`None`).
fn find_runes_option(source: &str, el: &SvelteElement) -> Option<bool> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name, value, .. } if name == "runes" => match value {
            // `runes` (no value) — boolean shorthand ⇒ true.
            None => Some(true),
            // `runes={true}` / `runes={false}` — read the expression literal.
            Some(SvelteAttributeValue::Expression(span)) => {
                let text = source[span.start as usize..span.end as usize].trim();
                match text {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    })
}

/// The `namespace="…"` literal value on a `<svelte:options>` element.
fn find_namespace_option(source: &str, el: &SvelteElement) -> Option<String> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain {
            name,
            value: Some(SvelteAttributeValue::Text(span)),
            ..
        } if name == "namespace" => {
            Some(source[span.start as usize..span.end as usize].to_string())
        }
        _ => None,
    })
}

/// Extract the props TYPE annotation from an instance script's `$props()`
/// declaration — the LOCAL self-props contract source (F8). SYNTACTIC only (no
/// resolver): the script is parsed once with OXC (the same front-end the rest of
/// the compiler uses) and the FIRST genuine `$props()` rune-call declarator is
/// matched, else `None` (a permissive contract).
///
/// A genuine props rune is a `CallExpression` whose callee is the bare
/// identifier `$props` (NOT a member access like `$props.id()`, and NOT a
/// `$props` substring inside a string/comment — the grammar-correct parse
/// excludes both). Two annotation forms contribute the contract type:
///
/// - generic `$props<T>()` → the `<T>` type-argument text;
/// - annotated `let … : T = $props()` → the declarator's type-annotation text.
///
/// A catastrophically-unparseable fragment (`parsed.panicked`) yields `None`
/// (fail-open). A RECOVERABLE parse error (`parsed.errors` non-empty, AST still
/// produced) is NOT treated as fatal here: the grammar-correct AST still pins
/// the real `$props()` rune declarator precisely, and the self-props contract is
/// a LOCAL convenience over which the projection's own TSX validity does not
/// depend (a wrong contract only weakens the self-prop check, never produces
/// invalid TSX). Tightening to bail on any recoverable error would only degrade
/// otherwise-resolvable self-contracts to permissive — strictly less precision
/// for no validity gain.
pub(super) fn extract_props_annotation(script: &str) -> Option<String> {
    if !script.contains("$props") {
        return None;
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, script, SourceType::tsx()).parse();
    if parsed.panicked {
        return None;
    }
    let mut collector = PropsRuneCollector {
        source: script,
        annotation: None,
    };
    collector.visit_program(&parsed.program);
    collector.annotation
}

/// `true` when `expr` is the bare `$props` identifier callee of a props rune
/// call — NOT a member access (`$props.id()`).
fn is_props_rune_callee(expr: &Expression) -> bool {
    matches!(expr, Expression::Identifier(id) if id.name == "$props")
}

/// Collects the FIRST genuine `$props()` rune call's contract annotation text.
struct PropsRuneCollector<'s> {
    source: &'s str,
    annotation: Option<String>,
}

impl<'a> Visit<'a> for PropsRuneCollector<'_> {
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        if self.annotation.is_some() {
            return;
        }
        if let Some(Expression::CallExpression(call)) = &decl.init {
            if is_props_rune_callee(&call.callee) {
                // Generic form `$props<T>()` — the first type argument is the
                // contract type.
                if let Some(targs) = &call.type_arguments {
                    if let Some(first) = targs.params.first() {
                        let span = first.span();
                        let text = self.source[span.start as usize..span.end as usize].trim();
                        if !text.is_empty() {
                            self.annotation = Some(text.to_string());
                            return;
                        }
                    }
                }
                // Annotated form `let … : T = $props()` — the declarator's
                // type annotation is the contract type.
                if let Some(ann) = &decl.type_annotation {
                    let span = ann.type_annotation.span();
                    let text = self.source[span.start as usize..span.end as usize].trim();
                    if !text.is_empty() {
                        self.annotation = Some(text.to_string());
                        return;
                    }
                }
            }
        }
        walk::walk_variable_declarator(self, decl);
    }
}
