//! Attribute / directive lowering for the Svelte runtime IR.
//!
//! Lowers an element / component / special-element attribute list into the IR
//! attribute model ([`AttrIr`]): static literals folded into the static template,
//! dynamic / mixed expression attributes, spreads, `class:` / `style:` / `bind:` /
//! `use:` / transition directives, and event handlers.
//!
//! Event handlers follow the official `svelte@5.56.3` model: the
//! `is_event_attribute` rule (`is_expression_attribute(attr) && name.starts_with('on')`,
//! which accepts the quoted single-expression form `onclick="{...}"`), the
//! `is_capture_event` event-name normalization (strip the trailing `capture`), and
//! the `can_delegate_event` delegation decision keyed on the RAW pre-normalization
//! name (so a capture handler is never delegated).

use verter_span::Span;

use super::entity_decode::{decode_attr_entities, DecodedAttrValue};
use super::events::{can_delegate_event, is_passive_event, normalize_event_name};
use super::expr::ScopeId;
use super::ir::{
    AttrIr, EventOrigin, MixedAttrPart, StaticAttrValue, StyleDirectiveValue, TransitionKind,
};
use super::{local_name_span, span_text, spread_expr_span, LoweringCtx};
use crate::svelte::parser::tokenizer_scan::find_matching_brace_in;
use crate::svelte::parser::{
    SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue, SvelteDirective,
    SvelteDirectiveKind,
};

/// The kind of element an attribute list is being lowered for. The host kind
/// decides how an `on*` event attribute lowers — the official `metadata.delegated`
/// rule keys on `parent.type` (`Attribute.js`):
///
/// - [`AttrHost::Element`] — a regular intrinsic element. An `on*` is a DOM
///   [`AttrIr::Event`], delegated iff `can_delegate_event` (the only host that ever
///   delegates).
/// - [`AttrHost::Component`] — a `<Foo>` component (incl. `<svelte:component>` /
///   `<svelte:self>`). An `on*` is a FORWARDED PROP under its ORIGINAL name
///   (`onclick`), passed in the component call's props object — NOT a DOM event,
///   NEVER delegated (official `build_component` pushes it as a plain prop).
/// - [`AttrHost::DynamicElement`] — a `<svelte:element this={…}>`. An `on*` rides
///   the runtime `$.attribute_effect` spread surface (official `build_attribute_effect`),
///   so it is a dynamic ATTRIBUTE, NOT a DOM event op, and NEVER delegated.
/// - [`AttrHost::GlobalSpecial`] — `<svelte:window>` / `<svelte:body>` /
///   `<svelte:document>`. An `on*` is a DIRECT global `$.event` listener
///   ([`AttrIr::Event`] with `delegated = false`), NEVER delegated.
/// - [`AttrHost::OtherSpecial`] — any other `<svelte:*>` (head / options / boundary
///   / fragment): an `on*` falls through to the element-event path (no corpus
///   fixture relies on a different rule, and the official analyzer never delegates
///   it because the parent is not a `RegularElement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AttrHost {
    /// A regular intrinsic element.
    Element,
    /// A component reference (incl. `<svelte:component>` / `<svelte:self>`).
    Component,
    /// A `<svelte:element this={…}>` dynamic element.
    DynamicElement,
    /// A `<svelte:window>` / `<svelte:body>` / `<svelte:document>` global host.
    GlobalSpecial,
    /// Any other `<svelte:*>` special element.
    OtherSpecial,
}

/// Lower an element's attributes / directives into the IR attribute model.
pub(super) fn lower_attributes(
    ctx: &mut LoweringCtx,
    attributes: &[SvelteAttribute],
    scope: ScopeId,
    host: AttrHost,
) -> Vec<AttrIr> {
    let mut out = Vec::new();
    for attr in attributes {
        match &attr.kind {
            SvelteAttributeKind::Plain { name, value, .. } => {
                out.push(lower_plain_attr(
                    ctx,
                    name,
                    value.as_ref(),
                    attr.span,
                    scope,
                    host,
                ));
            }
            SvelteAttributeKind::Spread(span) => {
                // The spread span includes the leading `...`; the EXPRESSION is the
                // part after it (`{...rest}` → `rest`).
                let expr = ctx.push_expr(spread_expr_span(ctx.source, *span), scope);
                out.push(AttrIr::Spread { expr });
            }
            SvelteAttributeKind::Directive(directive) => {
                if let Some(ir) = lower_directive(ctx, directive, attr.span, scope) {
                    out.push(ir);
                }
            }
            // An attribute-position `{@attach expr}` — the expression span was
            // captured cleanly by the tokenizer (after the `@attach` keyword + ws),
            // so the lowering pushes it directly (no body re-slicing).
            SvelteAttributeKind::Attach { expr_span } => {
                let expr = ctx.push_expr(*expr_span, scope);
                out.push(AttrIr::Attach { expr });
            }
        }
    }
    out
}

/// Lower a plain attribute: an event-handler attribute (`onclick={…}`), a static
/// literal (`class="x"`), or a dynamic expression (`id={expr}` / shorthand).
fn lower_plain_attr(
    ctx: &mut LoweringCtx,
    name: &str,
    value: Option<&SvelteAttributeValue>,
    span: Span,
    scope: ScopeId,
    host: AttrHost,
) -> AttrIr {
    // The official `is_event_attribute` rule (`compiler/utils/ast.js`):
    // `is_expression_attribute(attr) && name.startsWith('on')`. There is NO
    // lowercase-only filter and NO non-empty filter — `onClick`, `onfoo1`,
    // `onfoo-bar`, `on={h}` (event name `''`), `on1={h}` (name `1`) are ALL events.
    // `is_expression_attribute` accepts the bare `{expr}` value form AND a quoted
    // value that is EXACTLY one expression chunk with no literal text
    // (`onclick="{() => x()}"`). The event NAME is `name.slice(2)` (the official
    // `visit_event_attribute`), then capture-normalized.
    if let Some(raw_event) = name.strip_prefix("on") {
        if let Some(handler_span) = single_expression_value_span(ctx, value) {
            // The official `metadata.delegated` rule (`Attribute.js`):
            // `parent.type === 'RegularElement' && can_delegate_event(name.slice(2))`.
            // Only a regular intrinsic element ever delegates a modern `on*`
            // attribute. The other hosts route the handler elsewhere — a Component
            // forwards it as a plain prop, a `<svelte:element>` runs it through
            // `$.attribute_effect`, and a window/body/document binds a DIRECT global
            // listener — and NONE of those is ever delegated.
            match host {
                AttrHost::Component => {
                    // A component-hosted `on*` is a FORWARDED PROP under its ORIGINAL
                    // attribute name (`onclick`), passed in the component call's props
                    // object — NOT a DOM event, NEVER delegated (official
                    // `build_component`). Model it as a plain dynamic prop so the IR
                    // carries the prop name `onclick` verbatim.
                    let expr = ctx.push_expr(handler_span, scope);
                    return AttrIr::Dynamic {
                        name: name.to_string(),
                        expr,
                    };
                }
                AttrHost::DynamicElement => {
                    // A `<svelte:element>`-hosted `on*` rides the runtime
                    // `$.attribute_effect` spread surface (official
                    // `build_attribute_effect`): a DYNAMIC attribute under its
                    // original name, NOT a DOM event op, NEVER delegated.
                    let expr = ctx.push_expr(handler_span, scope);
                    return AttrIr::Dynamic {
                        name: name.to_string(),
                        expr,
                    };
                }
                AttrHost::Element | AttrHost::GlobalSpecial | AttrHost::OtherSpecial => {
                    let handler = ctx.push_expr(handler_span, scope);
                    // The raw event name is `name.slice(2)` — `onClick` → `Click`,
                    // `onfoo-bar` → `foo-bar`, `on={h}` → `` (empty).
                    // `normalize_event_name` strips the trailing `capture` suffix
                    // (`is_capture_event`); the DELEGATION decision keys on the RAW
                    // pre-normalization name (a `*capture` raw name is never in the
                    // delegated set, so a capture handler is never delegated).
                    let (event_type, capture) = normalize_event_name(raw_event);
                    // ONLY a regular intrinsic element delegates (official
                    // `metadata.delegated` requires `parent.type === 'RegularElement'`).
                    // A window/body/document listener and any other `<svelte:*>` host
                    // are NEVER delegated — they bind a DIRECT `$.event`.
                    let delegated =
                        matches!(host, AttrHost::Element) && can_delegate_event(raw_event);
                    // The MODERN attribute form's passive default is purely
                    // `is_passive_event(event_type)` (official `visit_event_attribute`
                    // passes `is_passive_event(name) ? true : undefined`): `touchstart`
                    // / `touchmove` ⇒ `Some(true)`, every other type ⇒ `None`. (A
                    // modern attribute carries no `|passive` / `|nonpassive` modifier —
                    // that is the legacy directive form only.)
                    let passive = is_passive_event(&event_type).then_some(true);
                    return AttrIr::Event {
                        event_type,
                        handler,
                        delegated,
                        capture,
                        modifiers: Vec::new(),
                        passive,
                        origin: EventOrigin::ModernAttribute,
                    };
                }
            }
        }
        // An `on*` attribute (name longer than `on`) whose value is NOT a single
        // expression is the official `attribute_invalid_event_handler` error
        // (`phases/2-analyze/visitors/shared/element.js`: `name.startsWith('on') &&
        // name.length > 2 && !is_expression_attribute`). A valueless `onclick`, a
        // text `onclick="text"`, a whitespace-surrounded `onclick=" {h} "`, and a
        // multi-chunk `onclick="x{h}"` / `onclick="{h}{h}"` all reach here. A bare
        // `on` (length 2) is NOT an error — it falls through to a normal attribute.
        if name.len() > 2 {
            ctx.errors.push(
                "svelte-runtime-invalid-event-handler",
                format!("event attribute `{name}` must be a JavaScript expression, not a string"),
                span,
            );
            // Return a placeholder static attr; the non-empty error set fails the
            // lowering before the IR is published, so this is never observed.
            return AttrIr::Static {
                name: name.to_string(),
                value: None,
            };
        }
    }
    match value {
        // A static text value is ENTITY-DECODED HERE, at the attribute-IR
        // producer boundary — the IR carries the SEMANTIC value (the official
        // parse-time decoded `Text.data`), so the CSS scope matcher and the
        // client emitters read one shared meaning; emitters re-serialize
        // ESCAPE-ONLY (never a second decode).
        Some(SvelteAttributeValue::Text(span)) => AttrIr::Static {
            name: name.to_string(),
            value: Some(StaticAttrValue {
                value: DecodedAttrValue::decode(span_text(ctx.source, *span)),
            }),
        },
        Some(SvelteAttributeValue::Expression(span)) => {
            let expr = ctx.push_expr(*span, scope);
            AttrIr::Dynamic {
                name: name.to_string(),
                expr,
            }
        }
        Some(SvelteAttributeValue::Mixed(span)) => {
            // A concatenated value (`class="a {b}"`) is split into literal +
            // expression runs, NOT reparsed as one (invalid) JS expression.
            let parts = lower_mixed_attr_parts(ctx, *span, scope);
            AttrIr::Mixed {
                name: name.to_string(),
                parts,
            }
        }
        // A valueless attribute is a static boolean attribute.
        None => AttrIr::Static {
            name: name.to_string(),
            value: None,
        },
    }
}

/// The handler-expression span of an attribute value that the official
/// `is_expression_attribute` accepts as a SINGLE expression — used to decide
/// whether an `on*` attribute is an event (including the quoted-single-expression
/// form `onclick="{…}"`).
///
/// Mirrors `is_expression_attribute(attr)`:
/// - a bare `{expr}` value (`SvelteAttributeValue::Expression`) → its span;
/// - a quoted value (`SvelteAttributeValue::Mixed`) whose chunk array is EXACTLY
///   one `ExpressionTag` (`value.length === 1`) — i.e. the `{…}` interpolation
///   spans the WHOLE quoted body with NO surrounding text, not even whitespace
///   (`onclick="{() => x()}"`) → the interpolation's inner span.
///
/// Any value with ANY surrounding text (including whitespace — `onclick=" {h} "`),
/// more than one interpolation (`onclick="{h}{h}"`), text-and-expression
/// (`onclick="x{h}"`), or no interpolation (plain text / boolean) is NOT a single
/// expression → `None`. For an `on*` attribute longer than `on`, the caller turns
/// that `None` into the official `attribute_invalid_event_handler` compile error.
fn single_expression_value_span(
    ctx: &LoweringCtx,
    value: Option<&SvelteAttributeValue>,
) -> Option<Span> {
    match value? {
        SvelteAttributeValue::Expression(span) => Some(*span),
        SvelteAttributeValue::Mixed(span) => {
            // The body qualifies iff it is EXACTLY one `{…}` interpolation with no
            // bytes before `{` or after the matching `}` (the official
            // `value.length === 1` ExpressionTag-only shape — any surrounding text,
            // whitespace included, makes it a >1-chunk value). Returns the inner span.
            let text = span_text(ctx.source, *span);
            let bytes = text.as_bytes();
            if bytes.first() != Some(&b'{') {
                return None; // text (or whitespace) before the interpolation
            }
            let close = find_matching_brace_in(bytes, 1);
            // The matching `}` must be the LAST byte of the body (no trailing text).
            if close != bytes.len().saturating_sub(1) || bytes.get(close) != Some(&b'}') {
                return None;
            }
            Some(Span::new(span.start + 1, span.start + (close as u32)))
        }
        SvelteAttributeValue::Text(_) => None,
    }
}

/// Split a mixed attribute value span (`a {b} c`) into its ordered literal-text +
/// `{expr}` interpolation parts. Each interpolation's inner expression is lowered
/// through OXC (`push_expr`); each literal run is ENTITY-DECODED (the official
/// `decode_character_references` — `title="&copy; {x} &bogus;"` → the literal
/// `&copy; ` decodes to `© `, the `&bogus;` stays literal, so the runtime value is
/// `'© ' + x + ' &bogus;'`). This is a template attribute-value tokenizer (literal
/// vs `{…}` runs), not a JS expression parse.
///
/// The literal decode is DECODE-ONLY (NO re-escaping): a mixed-attribute value is a
/// runtime STRING the backend concatenates, never re-serialized HTML — so `&lt;`
/// decodes to `<` and stays `<` (verified against svelte@5.56.3), NOT the skeleton
/// re-escape path [`super::entity_decode::escape_decoded_attr`] applies to a static
/// attribute in the `from_html` template.
///
/// The `{…}` close is located through the SHARED JS-aware brace scanner
/// ([`find_matching_brace_in`]) the parser's interpolation tokenizer also uses, so
/// a `}` inside a string / template literal / regex / comment within the
/// interpolation (`class="x {format('}')} y"`) does NOT close the interpolation
/// early — never a second hand-rolled byte-level brace counter.
fn lower_mixed_attr_parts(ctx: &mut LoweringCtx, span: Span, scope: ScopeId) -> Vec<MixedAttrPart> {
    let text = span_text(ctx.source, span);
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut literal_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Flush the literal run before this interpolation, entity-decoded.
            if i > literal_start {
                parts.push(MixedAttrPart::Literal(decode_attr_entities(
                    &text[literal_start..i],
                )));
            }
            // The JS-aware close `}` index (string / regex / comment safe). The
            // inner expression is the byte run `i+1 .. close`.
            let close = find_matching_brace_in(bytes, i + 1);
            let inner_start = span.start + (i as u32) + 1;
            let inner_end = span.start + (close as u32);
            let expr = ctx.push_expr(Span::new(inner_start, inner_end), scope);
            parts.push(MixedAttrPart::Expr(expr));
            // Advance past the close `}` (or to EOF when unterminated).
            i = close.saturating_add(1);
            literal_start = i;
        } else {
            i += 1;
        }
    }
    if literal_start < bytes.len() {
        parts.push(MixedAttrPart::Literal(decode_attr_entities(
            &text[literal_start..],
        )));
    }
    parts
}

/// Lower a directive attribute into the IR attribute model.
///
/// A SHORTHAND directive (no `={…}` value — `class:active` / `style:color` /
/// `bind:value`) synthesizes the implied same-named identifier expression (the
/// directive `local` IS a JS identifier present in the attribute span), so the
/// op population sees a real [`ExprId`] instead of a dropped `None`. `use:fn`
/// likewise synthesizes the action reference (`fn`) when no argument is given.
fn lower_directive(
    ctx: &mut LoweringCtx,
    directive: &SvelteDirective,
    attr_span: Span,
    scope: ScopeId,
) -> Option<AttrIr> {
    // A directive's value expression is the inner expression of its `={…}` (bare
    // expression) value, OR — for a QUOTED single-expression value
    // (`class:active="{foo}"` / `bind:value="{foo}"` / `use:a="{foo}"`) — the inner
    // expression UNWRAPPED from the quotes + braces, mirroring the `on*` attribute
    // path. The official parser stores a directive's quoted single-`{…}` value as
    // that lone ExpressionTag (the inner expression), so the IR must carry `foo`
    // (an identifier), NOT `{foo}` (which reparses as an object literal). The
    // `single_expression_value_span` helper returns the inner span for a quoted
    // EXACTLY-one-`{…}` value; any other value (a bare `={…}`, a mixed value, or a
    // plain text value) keeps its whole span.
    let value_expr = |ctx: &mut LoweringCtx| {
        let v = directive.value.as_ref()?;
        let span = match v {
            SvelteAttributeValue::Expression(span) => *span,
            // A quoted value that is EXACTLY one `{…}` chunk unwraps to the inner
            // expression span (the official single-ExpressionTag form); a multi-chunk
            // quoted value keeps its whole span (it is a concatenation expression the
            // backend builds, not a lone identifier).
            SvelteAttributeValue::Mixed(span) => {
                single_expression_value_span(ctx, Some(v)).unwrap_or(*span)
            }
            SvelteAttributeValue::Text(span) => *span,
        };
        Some(ctx.push_expr(span, scope))
    };
    // The implied shorthand expression: the directive's local name as an
    // identifier reference (`class:active` ⇒ `active`). Located precisely within
    // the attribute span (the local name appears after the `prefix:`), so it
    // reparses as a real identifier reference rather than a dropped `None`.
    let shorthand_expr = |ctx: &mut LoweringCtx| {
        local_name_span(ctx.source, attr_span, &directive.local).map(|s| ctx.push_expr(s, scope))
    };
    match directive.kind {
        SvelteDirectiveKind::Bind => {
            // Shorthand `bind:value` ⇒ the bound expression is `value`.
            let expr = value_expr(ctx).or_else(|| shorthand_expr(ctx));
            Some(AttrIr::Bind {
                target: directive.local.clone(),
                expr,
            })
        }
        SvelteDirectiveKind::Class => {
            // Shorthand `class:active` ⇒ the condition is `active`.
            let condition = value_expr(ctx).or_else(|| shorthand_expr(ctx));
            Some(AttrIr::Class {
                name: directive.local.clone(),
                condition,
            })
        }
        SvelteDirectiveKind::Style => {
            // `style:` is the SOLE directive family that accepts a STATIC-TEXT value
            // (`style:color="red"` → the quoted string `{ color: 'red' }`); a bare `={x}`
            // / quoted single-`{x}` / shorthand value lowers to an expression. (A non-style
            // directive with a text value is rejected upstream by the official-reject gate
            // as `directive_invalid_value`, so it never reaches here.)
            let value = match &directive.value {
                // A static-text value carries the DECODED text (the fold emits the
                // single-quoted literal); NOT a synthetic string-literal expr.
                Some(SvelteAttributeValue::Text(span)) => {
                    StyleDirectiveValue::Text(decode_attr_entities(span_text(ctx.source, *span)))
                }
                // A MIXED quoted body that is NOT a single `{x}` chunk (`style:color="a{x}b"`
                // / `style:color="{x}{y}"`) is a text+interpolation concatenation — lower it
                // through the shared mixed-attr-parts splitter into the ordered literal /
                // expression run (the projector folds the template-literal). A SINGLE-`{x}`
                // quoted body (`style:color="{x}"`) is the `value.length === 1` shape and
                // stays an `Expr` (handled by the `_` arm below via
                // `single_expression_value_span`).
                Some(v @ SvelteAttributeValue::Mixed(span))
                    if single_expression_value_span(ctx, Some(v)).is_none() =>
                {
                    StyleDirectiveValue::Mixed(lower_mixed_attr_parts(ctx, *span, scope))
                }
                // A bare `={x}` / quoted single-`{x}` value, OR the shorthand `style:color`
                // (the implied same-named `color` reference) lowers to an expression.
                _ => {
                    let expr = value_expr(ctx).or_else(|| shorthand_expr(ctx));
                    match expr {
                        Some(expr) => StyleDirectiveValue::Expr(expr),
                        // A `style:color` shorthand whose local-name span could not be
                        // located (defensive): fall back to a text value of the local name.
                        None => StyleDirectiveValue::Text(directive.local.clone()),
                    }
                }
            };
            Some(AttrIr::Style {
                property: directive.local.clone(),
                value,
                important: directive.modifiers.iter().any(|m| m == "important"),
            })
        }
        SvelteDirectiveKind::On => {
            let handler = value_expr(ctx)?;
            // A LEGACY `on:` directive is NEVER delegated — the official
            // `OnDirective.js` always calls `build_event(…, /*delegated*/ false)`,
            // emitting a direct `$.event(...)`. Only a MODERN `onclick={…}`
            // attribute participates in delegation (`Attribute.js` sets
            // `metadata.delegated`). The legacy `on:click|capture` modifier sets the
            // capture phase; the event NAME is the directive local (the `|capture`
            // modifier is not part of the name, and the `*capture` SUFFIX form does
            // not apply to legacy directives — capture is a modifier there).
            let capture = directive.modifiers.iter().any(|m| m == "capture");
            // The LEGACY directive form's passive option derives from its modifiers
            // ONLY (official `OnDirective.js`: `passive = includes('passive') ||
            // (includes('nonpassive') ? false : undefined)`): `|passive` ⇒ `Some(true)`,
            // else `|nonpassive` ⇒ `Some(false)`, else `None`. It does NOT consult
            // `is_passive_event` — a legacy `on:touchstart` emits no passive arg.
            let passive = if directive.modifiers.iter().any(|m| m == "passive") {
                Some(true)
            } else if directive.modifiers.iter().any(|m| m == "nonpassive") {
                Some(false)
            } else {
                None
            };
            Some(AttrIr::Event {
                event_type: directive.local.clone(),
                handler,
                delegated: false,
                capture,
                modifiers: directive.modifiers.clone(),
                passive,
                origin: EventOrigin::LegacyDirective,
            })
        }
        SvelteDirectiveKind::Use => {
            // `use:fn` ⇒ the action reference is the synthesized `fn`;
            // `use:fn={arg}` carries `arg` as the action argument. A bare `use:fn`
            // must still emit an Action op (the action reference is the local name).
            let action = shorthand_expr(ctx)?;
            let arg = value_expr(ctx);
            Some(AttrIr::Use { expr: action, arg })
        }
        // A `transition:` / `in:` / `out:` directive. The `|global` modifier is the
        // official `TRANSITION_GLOBAL` flag bit; `|local` (or no modifier) is the
        // default — recorded at LOWERING so the projection computes the FLAG integer
        // from typed data, never a re-scan of the modifier list.
        SvelteDirectiveKind::Transition => Some(AttrIr::Transition {
            kind: TransitionKind::Transition,
            name: directive.local.clone(),
            expr: value_expr(ctx),
            global: directive.modifiers.iter().any(|m| m == "global"),
        }),
        SvelteDirectiveKind::In => Some(AttrIr::Transition {
            kind: TransitionKind::In,
            name: directive.local.clone(),
            expr: value_expr(ctx),
            global: directive.modifiers.iter().any(|m| m == "global"),
        }),
        SvelteDirectiveKind::Out => Some(AttrIr::Transition {
            kind: TransitionKind::Out,
            name: directive.local.clone(),
            expr: value_expr(ctx),
            global: directive.modifiers.iter().any(|m| m == "global"),
        }),
        // `animate:` is its OWN attribute family (the `$.animation` helper), NOT a
        // transition kind — keyed-each placement is validated by the client surface.
        SvelteDirectiveKind::Animate => Some(AttrIr::Animate {
            name: directive.local.clone(),
            expr: value_expr(ctx),
        }),
        SvelteDirectiveKind::Let => Some(AttrIr::Let {
            name: directive.local.clone(),
            expr: value_expr(ctx),
        }),
        SvelteDirectiveKind::Unknown => {
            ctx.errors.push(
                "svelte-runtime-unknown-directive",
                format!("unrecognised directive `{}`", directive.local),
                Span::new(0, 0),
            );
            None
        }
    }
}
