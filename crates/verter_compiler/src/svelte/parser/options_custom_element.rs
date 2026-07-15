//! The `<svelte:options customElement={EXPR}>` VALUE engine — the faithful port of the official
//! `svelte@5.56.3` parse pipeline for the expression-valued `customElement` axis
//! (`phases/1-parse/read/expression.js` + `read/options.js`), and the retained typed value it
//! produces.
//!
//! Upstream's `read_options` runs INSIDE the parser (at parse finalization) and RETAINS the
//! validated value on the AST (`AST.SvelteOptions['customElement']`). This module mirrors that
//! placement: the byte parser calls [`resolve_custom_element_expr`] ONCE at its options
//! finalization and retains the outcome on the reserved
//! [`OptionsCustomElementProbe::resolution`] slot — the exact official reject code on the `Err`
//! side, the typed accepted value ([`AcceptedCustomElementValue`]: the `null` backwards-compat
//! skip / the typed [`CustomElementDescriptor`]) on the `Ok` side. The official-reject gate
//! consumes the reject side (at the probe's reserved encounter orders); the runtime lowering
//! consumes the accepted side. The expression is parsed exactly once, in one validate+extract
//! walk — exactly as upstream's `read_options` `customElement` branch validates AND builds the
//! retained value in one pass — so a validator/extractor divergence (a shape one walk accepts
//! and the other misreads) is structurally impossible, and no later stage re-parses the
//! expression span from raw source.
//!
//! [`OptionsCustomElementProbe`]: super::template_ast::OptionsCustomElementProbe
//! [`OptionsCustomElementProbe::resolution`]: super::template_ast::OptionsCustomElementProbe::resolution

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKey};
use oxc_span::{GetSpan, SourceType};

use super::template_ast::validate_custom_element_tag;

/// The RESOLVED `<svelte:options customElement>` / compile-option custom-element
/// descriptor — the official `read_options` `AST.SvelteOptions['customElement']`
/// value re-expressed as owned facts. Retained by the PARSER at options
/// finalization (the runtime lowering consumes it after the official-reject
/// gate, so every consumed value is official-ACCEPTED): the tag is a validated
/// string literal, the prop defs are literal-only, and only `shadow`/`extend`
/// carry verbatim expression source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomElementDescriptor {
    /// The custom-element TAG (`customElement="my-el"` / `{ tag: 'my-el' }`).
    /// `Some` emits `customElements.define(tag, $.create_custom_element(…))`;
    /// `None` (the no-tag object / compile-option-`true` forms) emits the bare
    /// `$.create_custom_element(…)` statement — registration is left to the user.
    pub tag: Option<String>,
    /// The resolved `shadow` axis — drives `create_custom_element`'s CONDITIONAL
    /// fifth argument.
    pub shadow: CustomElementShadow,
    /// The `props` axis entries IN SOURCE ORDER (`{ props: { count: { reflect:
    /// true, type: 'Number' } } }`). Explicit entries win over the inferred
    /// `key: {}` entries the emission appends for the component's remaining
    /// `$props()` members.
    pub props: Vec<CustomElementProp>,
    /// The `extend` axis expression VERBATIM source (`(c) => c`), emitted as the
    /// conditional sixth `create_custom_element` argument. Official passes the
    /// expression through unevaluated; so does Verter. Author parens are peeled
    /// (official prints the `remove_parens`-ed AST), except a SequenceExpression
    /// keeps its syntax-required parens — bare `0, fn` would splice into the
    /// argument list as two arguments.
    pub extend: Option<String>,
    /// The resolved custom-element CSS mode: a custom element ALWAYS injects its
    /// styles (the official `inject_styles = css === 'injected' ||
    /// is_custom_element` — the `is_custom_element` half). Recorded for the style
    /// pipeline; style compilation itself is the CSS vertical, and a custom
    /// element with a `<style>` block still fails closed until it lands.
    pub inject_styles: bool,
}

/// The resolved `shadow` axis of a [`CustomElementDescriptor`] — the official
/// `create_custom_element` fifth-argument rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomElementShadow {
    /// `shadow: 'open'` or the absent default — arg5 is `{ mode: 'open' }`.
    Open,
    /// `shadow: 'none'` — arg5 is OMITTED (or `void 0` when an `extend` sixth
    /// argument follows).
    None,
    /// An OBJECT `shadow` value (`{ mode: 'open', delegatesFocus: true }`) —
    /// passed through VERBATIM as arg5 (upstream hands the object literal to
    /// `create_custom_element` unevaluated).
    ObjectInit(String),
}

/// One explicit `props` entry of a [`CustomElementDescriptor`] — the official
/// `{ [name]: { attribute?, reflect?, type? } }` definition, validated to
/// literal-only fields by [`resolve_custom_element_expr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomElementProp {
    /// The entry NAME — the identifier key of the `props` object member. The
    /// emission looks it up as a LOCAL binding name (the official
    /// `analysis.instance.scope.get(name)`); an aliased member surfaces its
    /// SOURCE key.
    pub name: String,
    /// The `attribute` override (`{ attribute: 'data-count' }`) — a validated
    /// string literal.
    pub attribute: Option<String>,
    /// The `reflect` flag (`{ reflect: true }`) — a validated boolean literal.
    pub reflect: bool,
    /// The `type` hint (`'String'`/`'Number'`/`'Boolean'`/`'Array'`/`'Object'`)
    /// — a validated string literal from the closed official set.
    pub type_hint: Option<String>,
}

/// The official-ACCEPTED `<svelte:options customElement={EXPR}>` value — what upstream's
/// `read_options` retains when the expression validates clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptedCustomElementValue {
    /// The `null` literal — the Svelte-3 backwards-compat spelling. It sets NOTHING
    /// (upstream `break`s without touching `component_options.customElement`), so the
    /// `customElement: true` compile option still decides.
    NullSkip,
    /// A conforming descriptor object (`{ tag?, shadow?, props?, extend? }`), extracted in the
    /// SAME walk that validated it.
    Descriptor(CustomElementDescriptor),
}

/// Resolve a `<svelte:options customElement={EXPR}>` expression exactly as upstream — the ONE
/// validate+extract walk, run ONCE by the parser's options finalization and retained on the
/// probe. `Err(code)` is the EXACT official reject code (the official-reject gate's side);
/// `Ok(value)` is the ACCEPTED retained value (the runtime lowering's side): the `null`
/// backwards-compat skip, or the typed descriptor extracted from the SAME walk that validated
/// it. `expr_src` is the raw `{…}` inner expression text.
///
/// Upstream parses the `customElement` value through `read_expression` (`read/expression.js`) →
/// `read_attribute_value`'s mustache reader, which performs a CURSOR parse: `parse_expression_at`
/// parses ONE prefix expression starting at the `{`, records `node.end`, sets `parser.index =
/// node.end`, `allow_whitespace`s, then `eat('}', true)`. So there are THREE outcomes, in upstream's
/// order on the encounter timeline:
/// - the prefix expression FAILS to parse (`{<}`, `{ tag: }`, `{}` empty, `{1 + }`): the acorn parse
///   throws `js_parse_error` (BEFORE `read_options`).
/// - the prefix expression PARSES but TRAILING non-`}` content follows it (`{1 2}`, `{foo bar}`,
///   `{a.b c}`, `{1;2}`): the required `eat('}', true)` fails on the trailing token →
///   `expected_token`. (A comma/sequence `{1,2}` or call `{(1)(2)}` is ONE expression that consumes
///   the whole inner — no trailing junk — so it does NOT take this arm.)
/// - a SINGLE clean expression consumes the whole inner (only whitespace/comments before the `}`):
///   it reaches `read_options` `customElement` VALIDATION (`read/options.js`, parse finalization) —
///   `null` / a conforming object ACCEPTS; anything else is `svelte_options_*`.
///
/// OXC's `parse_expression` already models the cursor: it parses the prefix expression and reports
/// its `span().end`, IGNORING trailing tokens (it does not require full consumption), exactly as
/// acorn's `parse_expression_at` does — so the prefix end is `expr.span().end` and the
/// "remaining is only trivia" test is the faithful `allow_whitespace` + `eat('}')` check.
/// (Both parse with parens PRESERVED — acorn under `preserveParens: true`, OXC's default — so the
/// cursor span of a parenthesized prefix includes its parens; the peel below never widens what
/// counts as trailing junk.)
///
/// Upstream's `read_expression` then returns `remove_parens(node)` — a DEEP recursive walk
/// replacing every `ParenthesizedExpression` with its inner expression — so by the time
/// `read_options` classifies the value, author parens are transparent at EVERY depth
/// (`({ tag: 'x' })`, `{ tag: ('x') }`, `reflect: (true)` all read through, while a parenthesized
/// INVALID value keeps its exact reject code). This walk mirrors that with [`peel_parens`] at each
/// classification site.
///
/// The parser already handled the boolean-shorthand (`svelte_options_invalid_customelement`) and
/// Text-value (`validate_tag`) cases — this is the `value[0].expression` branch only, where `v`
/// is the mustache expression, so upstream's `value = [v]` makes `value[0].type !== 'Text'`.
pub fn resolve_custom_element_expr(
    expr_src: &str,
) -> Result<AcceptedCustomElementValue, &'static str> {
    let alloc = Allocator::default();
    let parsed = oxc_parser::Parser::new(&alloc, alloc.alloc_str(expr_src), SourceType::mjs())
        .parse_expression();
    let Ok(expr) = parsed else {
        // FIRST CHECK — the PREFIX expression does not PARSE. Upstream reads the attribute
        // expression with acorn during element parsing — BEFORE `read_options` — so a
        // syntactically-malformed `customElement={EXPR}` (including an EMPTY / whitespace-only inner
        // and an incomplete prefix like `1 +`) is `js_parse_error`, NOT a `read_options` code.
        // (Verified pinned svelte@5.56.3: `{{ tag: }}`, `{<}`, `{}`, `{1 + }` all raise
        // `js_parse_error`.)
        return Err("js_parse_error");
    };

    // SECOND CHECK — the prefix expression parsed, but upstream's `eat('}', true)` runs at
    // `node.end` after `allow_whitespace`. If non-trivia content remains between the prefix
    // expression's end and the `}`, the required `}` is missing → `expected_token`. (Verified pinned:
    // `{1 2}` / `{foo bar}` / `{a.b c}` / `{1;2}` → `expected_token`.)
    let prefix_end = (expr.span().end as usize).min(expr_src.len());
    if !is_only_expression_trivia(&expr_src[prefix_end..]) {
        return Err("expected_token");
    }

    // VALIDATION+EXTRACTION — a single clean expression consuming the whole inner: run the
    // `read_options` `customElement` branch (upstream validates AND builds the retained value in
    // this one walk). The top-level value is PEELED first — upstream's `read_expression` hands
    // `read_options` the `remove_parens` result, so `({ tag: 'x' })` / `(null)` classify as
    // their inner expressions.
    match peel_parens(&expr) {
        // `value[0].expression.type === 'Literal' && value === null` → backwards-compat skip
        // (ACCEPT, retains nothing). OXC models a `null` literal as `Expression::NullLiteral`.
        Expression::NullLiteral(_) => Ok(AcceptedCustomElementValue::NullSkip),
        // An ObjectExpression — validate its `tag` / `props` / `shadow` members and extract the
        // descriptor.
        Expression::ObjectExpression(obj) => {
            resolve_custom_element_object(obj, expr_src).map(AcceptedCustomElementValue::Descriptor)
        }
        // Any OTHER expression (a number, string, identifier, call, …) is not an ObjectExpression
        // and not `null` → `svelte_options_invalid_customelement`.
        _ => Err("svelte_options_invalid_customelement"),
    }
}

/// Whether the bytes after the prefix expression's end (up to the `customElement={…}` closing brace)
/// are ONLY trivia — the whitespace `allow_whitespace` consumes plus the `/* … */` and `// …`
/// comments acorn's `read_expression` skips past before the required `eat('}', true)`. Any other
/// remaining content is the trailing token that makes the `}` missing (`expected_token`).
///
/// This is a TRIVIA scan, NOT an expression / type re-parse: it never interprets the remaining text
/// as a type or splits it on operators — it only classifies whitespace / comment runs and stops at
/// the first real token.
///
/// An unterminated `/* … */` inside the value (`customElement={1 /* unterminated} />`) never
/// reaches this scan as the deciding factor: the `customElement={…}` inner span is delimited by the
/// COMMENT-AWARE brace matcher [`find_matching_brace_in`], whose `/*`-to-EOF skip consumes the `}`
/// into the comment, so no closing `}` is found and the span runs to true EOF. The required
/// `eat('}', true)` is therefore missing → `expected_token` (the upstream compiler reaches its
/// `js_parse_error` along a different path; both REJECT, and the exact-code divergence is recorded
/// in the parser-parity debt ledger — see `docs/arch/svelte-native-compiler-plan.md`).
///
/// [`find_matching_brace_in`]: super::tokenizer_scan::find_matching_brace_in
fn is_only_expression_trivia(rest: &str) -> bool {
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            // a block comment `/* … */` — skip to its close (or to the end if unterminated).
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            // a line comment `// …` — skip to end-of-line (or to the end).
            i += 2;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else {
            return false; // a real trailing token — the `}` is missing.
        }
    }
    true
}

/// Validate AND extract the `customElement` ObjectExpression, mirroring upstream EXACTLY:
/// every member must be a non-computed identifier-keyed `Property` (else
/// `svelte_options_invalid_customelement`), then the `tag` / `props` / `shadow` / `extend`
/// members are read FIND-FIRST (upstream's `properties.find(([n]) => n === name)`) — validated
/// to their specific codes and retained into the typed descriptor in the same pass. Unknown
/// members are IGNORED (upstream only reads `tag`/`props`/`shadow`/`extend`). Sub-expression
/// spans index `expr_src` (the parsed inner-expression slice), so the `shadow`/`extend`
/// verbatim slices come straight from it.
fn resolve_custom_element_object(
    obj: &ObjectExpression<'_>,
    expr_src: &str,
) -> Result<CustomElementDescriptor, &'static str> {
    let properties = &obj.properties;
    let mut descriptor = CustomElementDescriptor {
        tag: None,
        shadow: CustomElementShadow::Open,
        props: Vec::new(),
        extend: None,
        inject_styles: true,
    };

    // (1) every property must be a plain, non-computed, identifier-keyed `Property`.
    for property in properties {
        let ObjectPropertyKind::ObjectProperty(p) = property else {
            // A SpreadProperty (`{...x}`) is not a `Property` → invalid.
            return Err("svelte_options_invalid_customelement");
        };
        if p.computed || property_key_identifier(&p.key).is_none() {
            return Err("svelte_options_invalid_customelement");
        }
    }

    // (2) `tag` → validate_tag on the static string value (or `None` when not a string literal);
    // a validated tag is retained.
    if let Some(tag_value) = find_property(properties, "tag") {
        let tag = string_literal_value(tag_value);
        if let Some(code) = validate_custom_element_tag(tag) {
            return Err(code);
        }
        descriptor.tag = tag.map(str::to_string);
    }

    // (3) `props` → must be an ObjectExpression of `{ [key]: { attribute?, reflect?, type? } }`;
    // the validated entries are retained in source order.
    if let Some(props_value) = find_property(properties, "props") {
        descriptor.props = resolve_custom_element_props(props_value)?;
    }

    // (4) `shadow` → `"open"` / `"none"` / an ObjectExpression (retained VERBATIM — upstream
    // passes the object literal through unevaluated); otherwise invalid shadow.
    if let Some(shadow_value) = find_property(properties, "shadow") {
        descriptor.shadow = match shadow_value {
            Expression::StringLiteral(s) if s.value == "open" => CustomElementShadow::Open,
            Expression::StringLiteral(s) if s.value == "none" => CustomElementShadow::None,
            Expression::ObjectExpression(init) => {
                CustomElementShadow::ObjectInit(slice_expr(expr_src, init.span()))
            }
            _ => return Err("svelte_options_invalid_customelement_shadow"),
        };
    }

    // (5) `extend` → retained VERBATIM, no validation (upstream passes the expression through
    // unevaluated). The slice comes from the PEELED span (official prints the peeled AST, parens
    // absent) — except a SequenceExpression, the one expression whose bare text is ambiguous in
    // the arg6 slot (`0, fn` would splice as TWO arguments): it keeps its syntax-required parens,
    // exactly as official prints it (`create_custom_element(…, (0, fn))`). A sequence-valued
    // member MUST be parenthesized in source, so the parens are always there to re-emit. Any
    // unknown member is ignored.
    if let Some(extend_value) = find_property(properties, "extend") {
        let slice = slice_expr(expr_src, extend_value.span());
        descriptor.extend = Some(match extend_value {
            Expression::SequenceExpression(_) => format!("({slice})"),
            _ => slice,
        });
    }

    Ok(descriptor)
}

/// Validate AND extract the `props` member value, mirroring upstream's
/// `svelte_options_invalid_customelement_props` rules: `props` must be an ObjectExpression; each
/// entry's value must be an ObjectExpression of only `type`
/// (`"String"`/`"Number"`/`"Boolean"`/`"Array"`/`"Object"`), `reflect` (boolean), or `attribute`
/// (string) literal members. Entries are retained in source order; a DUPLICATE entry name keeps
/// its FIRST position with the LAST entry's definition (upstream's `ce.props[name] = {…}`
/// object-assign semantics), and a duplicate field within one entry is last-wins the same way.
fn resolve_custom_element_props(
    props: &Expression,
) -> Result<Vec<CustomElementProp>, &'static str> {
    const INVALID: &str = "svelte_options_invalid_customelement_props";
    let Expression::ObjectExpression(obj) = props else {
        return Err(INVALID);
    };
    let mut defs: Vec<CustomElementProp> = Vec::new();
    for property in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = property else {
            return Err(INVALID);
        };
        if p.computed {
            return Err(INVALID);
        }
        let Some(name) = property_key_identifier(&p.key) else {
            return Err(INVALID);
        };
        // Peeled like every other classification site: `count: ({ … })` is the same entry
        // definition as `count: { … }` (upstream's AST carries no parens here).
        let Expression::ObjectExpression(entry) = peel_parens(&p.value) else {
            return Err(INVALID);
        };
        let mut def = CustomElementProp {
            name: name.to_string(),
            attribute: None,
            reflect: false,
            type_hint: None,
        };
        for entry_prop in &entry.properties {
            let ObjectPropertyKind::ObjectProperty(ep) = entry_prop else {
                return Err(INVALID);
            };
            let (Some(key), false) = (property_key_identifier(&ep.key), ep.computed) else {
                return Err(INVALID);
            };
            match key {
                "type" => match string_literal_value(&ep.value) {
                    Some(hint @ ("String" | "Number" | "Boolean" | "Array" | "Object")) => {
                        def.type_hint = Some(hint.to_string());
                    }
                    _ => return Err(INVALID),
                },
                // The boolean-literal check peels (`reflect: (true)` reads through, exactly as
                // the string-literal reads do), while a peeled NON-boolean stays invalid.
                "reflect" => match peel_parens(&ep.value) {
                    Expression::BooleanLiteral(b) => def.reflect = b.value,
                    _ => return Err(INVALID),
                },
                "attribute" => match string_literal_value(&ep.value) {
                    Some(attribute) => def.attribute = Some(attribute.to_string()),
                    None => return Err(INVALID),
                },
                _ => return Err(INVALID),
            }
        }
        // Upstream's `ce.props[name] = {…}`: a duplicate name keeps its FIRST insertion
        // position with the LAST definition.
        match defs.iter_mut().find(|existing| existing.name == def.name) {
            Some(existing) => *existing = def,
            None => defs.push(def),
        }
    }
    Ok(defs)
}

/// Peel every transparent `ParenthesizedExpression` layer off `expr` — the faithful port of
/// upstream's `remove_parens` (`phases/1-parse/acorn.js`), a DEEP recursive walk that replaces
/// each `ParenthesizedExpression` with its inner expression before `read_options` classifies.
/// Applied at every classification site of this walk (the top-level value, each selected
/// member value, each props field value, the string/boolean literal reads), so nesting like
/// `((({ tag: ('x') })))` peels the same as upstream. The peeled node's span EXCLUDES the
/// wrapping parens — exactly what the `shadow`/`extend` verbatim slices want, since official
/// prints the peeled AST (parens absent from the emission).
fn peel_parens<'a, 'b>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    expr
}

/// Slice a sub-expression's VERBATIM source out of the parsed inner-expression text (the AST
/// spans index `expr_src` directly).
fn slice_expr(expr_src: &str, span: oxc_span::Span) -> String {
    expr_src[span.start as usize..span.end as usize].to_string()
}

/// The value of the FIRST object property whose identifier key equals `name`, or `None` —
/// mirroring upstream's `properties.find(([n]) => n === name)?.[1]` over the
/// `[key.name, property.value]` pairs it collected. The caller already asserted (in the loop-(1)
/// pass) that every property is a non-computed identifier-keyed `Property`, so this just selects
/// by name and returns the property VALUE (for a getter/shorthand the value is the function /
/// identifier node, which the downstream `string_literal_value` correctly treats as non-string).
/// The value is PEELED (upstream's AST carries no parens here — `remove_parens` already ran), so
/// `tag`/`props`/`shadow`/`extend` classify their inner expressions, and the `shadow`/`extend`
/// verbatim slices come from the peeled spans.
fn find_property<'a>(
    properties: &'a oxc_allocator::Vec<ObjectPropertyKind<'a>>,
    name: &str,
) -> Option<&'a Expression<'a>> {
    properties.iter().find_map(|property| match property {
        ObjectPropertyKind::ObjectProperty(p) if property_key_identifier(&p.key) == Some(name) => {
            Some(peel_parens(&p.value))
        }
        _ => None,
    })
}

/// The identifier name of a non-computed object-property key (`{ tag: … }` → `"tag"`), or `None`
/// for a computed / non-identifier key. Mirrors upstream's `key.type === 'Identifier'`.
fn property_key_identifier<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// The string value of a STRING-LITERAL expression (`"my-el"` → `Some("my-el")`), or `None` for a
/// non-string-literal — faithful to upstream reading `value` only off a static string Literal.
/// PEELED first (`('my-el')` reads through — upstream's AST carries no parens at this point), so
/// the `tag`/`type`/`attribute` reads accept the parenthesized spellings official accepts.
fn string_literal_value<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match peel_parens(expr) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "options_custom_element_tests.rs"]
mod options_custom_element_tests;
