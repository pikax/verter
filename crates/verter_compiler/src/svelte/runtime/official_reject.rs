//! The OFFICIAL-REJECT validation gate — the core of the "official rejects ⇒ Verter
//! must reject" parity quadrant.
//!
//! The structural shape allowlists (the element/attr allowlist, the instance-script
//! item allowlist) prove a surface is one Verter EMITS; they do NOT prove the input
//! is one the official `svelte@5.56.3` compiler ACCEPTS. A §1.2-core-shaped input
//! that official COMPILE-ERRORS (a duplicate declaration, a `$`-prefixed binding, a
//! duplicate / mis-`context`-ed `<script>`, an invalid HTML placement, a global
//! `$foo` reference) must therefore ALSO fail closed in Verter — accepting malformed
//! Svelte changes the observable contract from "compile error, no module" to "module
//! exists", which is not behaviorally identical.
//!
//! This module owns:
//! - [`CoreOfficialValidationRule`] — the typed taxonomy of the official-error
//!   classes the §1.2 core surface must reject, with an exhaustive
//!   [`CoreOfficialValidationRule::ALL`] list;
//! - [`official_reject_gate`] — the analysis-domain detector for the rules that
//!   Verter did not previously fail closed (script duplicate / invalid context,
//!   dollar-prefixed bindings, duplicate accepted declarations, invalid HTML
//!   placement, global `$foo` / `$$foo` references) driven EXCLUSIVELY from the typed
//!   parse + the OXC AST (never a raw-source heuristic);
//! - [`CoreOfficialValidationRule::from_unsupported_surface`] — the mapping from the
//!   already-fail-closed [`UnsupportedSvelteRuntimeSurface`] codes that ALSO
//!   correspond to an official-reject class (duplicate attribute, invalid
//!   `<svelte:options>`, magic identifier) so the reject-parity matrix can classify
//!   every committed reject row to one rule.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, CallExpression, Expression, Function, Program, Statement,
};
use rustc_hash::FxHashSet;

use super::expr::{collect_pattern_names, reparse_module, BindTargetFact, ShadowStack};
use super::official_rule::{CoreOfficialValidationRule, OfficialRejection};
use crate::svelte::bind_contract::{bind_target_policy, resolve_runtime_bind, BindTargetPolicy};
use crate::svelte::parser::tokenizer_scan::find_matching_brace_in;
use crate::svelte::parser::{
    CloseTagViolationKind, ParsedSvelte, ScriptBodyGrammar, SvelteAttribute, SvelteAttributeKind,
    SvelteAttributeValue, SvelteBlockKind, SvelteClauseKind, SvelteDirectiveKind, SvelteElement,
    SvelteElementKind, SvelteNode, SvelteParseRejectKind, SvelteSpecialKind,
};

/// Run the OFFICIAL-REJECT analysis gate over a parsed component: detect the
/// official-error classes Verter did not previously fail closed, returning the FIRST
/// rule violated (with its exact official code), or `None` when the component is free of
/// these classes.
///
/// Runs EARLY in `compile_client` (before the unsupported-feature classifier) so a
/// genuinely-malformed input is rejected for being malformed — not later
/// mis-attributed to an unsupported feature. Driven EXCLUSIVELY from the typed parse
/// (`ParsedSvelte`, including its `strict_parse_errors` fact stream) + the OXC AST of
/// each script/template expression; never a raw `<script` byte-scan or a regex over
/// type text.
#[must_use]
pub fn official_reject_gate(source: &str, parsed: &ParsedSvelte) -> Option<OfficialRejection> {
    // ─── PARSE PHASE (official `phases/1-parse`) — the SINGLE parser-owned,
    // encounter-ordered defect stream is the SOLE parse-error arbitration source. Every
    // parse defect (close-tag / strict-parse / script-domain / explicit-`</p>` autoclose)
    // was recorded with an `encounter_order` minted at its DISCOVERY point in the parser's
    // single forward pass; the gate selects the FIRST-discovered (minimum `encounter_order`)
    // unsuppressed defect, matching official, which stops at the first parse error. Source
    // span is the report ANCHOR only — it NEVER arbitrates. ───
    if let Some(rejection) = select_parse_phase_defect(source, parsed) {
        return Some(rejection);
    }

    // ─── ANALYZE PHASE (official `phases/2-analyze`) — runs ONLY on a CLEAN parse (the
    // parse-defect stream above was empty). Ordered by upstream VISITOR/PASS order (NOT
    // span, NOT the parse-vs-analyze phase): official reaches the template `element.js`
    // directive-value check BEFORE the module/instance scope walk's `$`-declaration /
    // global-`$`-reference checks, which in turn run BEFORE the template-walk
    // `attribute_invalid_name` and `node_invalid_placement`. So:
    //   directive_invalid_value  >  $-decl / global-$-ref  >  attribute_invalid_name  >  placement.
    // A co-located `class:on="text"` + (a script `$foo` global / a `let $x` decl) rejects as
    // `directive_invalid_value`; a co-located `$foo` global + `<div 1foo>` rejects as
    // `global_reference_invalid` (the script global beats the attribute-name check). ───

    // (a) The template-walk `directive_invalid_value` — a non-`style:` directive whose value
    // is a STATIC-TEXT chunk (`class:on="x"` / `use:foo="bar"`) rather than a JS expression
    // in curly braces. Official rejects this at the PARSE phase (`element.js` exempts ONLY a
    // `StyleDirective` from the text-value check); Verter's forgiving parser accepts the
    // markup, so the analyze gate mirrors the rejection. Ordered FIRST in the analyze phase:
    // official reaches this directive-value check ahead of the scope walk's `$`-declaration /
    // global-reference checks, so a malformed directive value wins over a concurrent script
    // `$foo` / `let $x` defect (and over the attribute-name / placement scans).
    if let Some(rule) = scan_directive_invalid_value(source, &parsed.template) {
        return Some(OfficialRejection::of(rule));
    }

    // (a.2) The single document/attribute-order bind-validation pass — ONE traversal that, for
    // each `bind:` directive, computes the `BindTargetFact` ONCE and runs the bind-target SHAPE
    // scans (group policy → parens → invalid-expression) ONLY for binds official carries to
    // expression validation: an INTRINSIC host with a valid bind NAME / HOST / host-ATTRIBUTE
    // (via the shared `bind_contract` routing + the `host_attr_gate` authority), OR any
    // non-intrinsic (component / special) host (official has no DOM name/host/host-attr check
    // there and validates straight to expression shape). A name/host/host-attr-INVALID intrinsic
    // bind is a name/host/host-attr official reject BEFORE expression validation, so it is
    // SKIPPED here and fails closed downstream via the existing unsupported channel (the exact
    // name/host/host-attribute codes are deferred to D-29) — never a confidently-WRONG shape
    // code. Ordered AFTER the parse-phase `directive_invalid_value` (a static text/mixed
    // directive value is a parse error official reports first) and BEFORE the scope walk's
    // `$`-reference checks. Structural over the typed fact (NOT a source-text scan).
    if let Some(rejection) = scan_bind_shape_violations(source, &parsed.template) {
        return Some(rejection);
    }

    // The accepted top-level local names (declared in either script) — the referents a
    // `$foo` store-style reference / `bind:this` target may legally name. Collected
    // once for the reference scans.
    let declared = declared_top_level_locals(source, parsed);

    // (b.1) Script name rules (`scope.js`): a `$`-prefixed declaration
    // (`dollar_prefix_invalid`). Scanned over each script's top-level declarators. (A
    // same-lexical-scope `let`/`const` redeclaration is a PARSE-phase `js_parse_error` owned by
    // the body-probe slot, not an analyze rule, so it is not re-detected here.)
    for script_src in script_sources(source, parsed) {
        if let Some(rejection) = scan_script_declaration_rules(script_src) {
            return Some(rejection);
        }
    }

    // (b.2) Global `$foo` / `$$foo` references in script + template + bind + event
    // positions. A `$foo` is a violation only when `foo` is NOT a declared accepted local
    // AND lowercase-initial (`global_reference_invalid`); a `$$foo` (non-reserved) is always
    // `global_reference_invalid`; the reserved magic objects carry their EXACT runes-mode
    // codes (`$$props` → `legacy_props_invalid`, `$$restProps` → `legacy_rest_props_invalid`).
    // This scan ALSO covers a `$`-prefixed `bind:this={$foo}` target (the directive value is
    // one of the scanned template expression sources). (`$$slots`, which official ACCEPTS, is
    // a deferrable unsupported feature — never an official reject here.)
    if let Some(rejection) = scan_global_dollar_references(source, parsed, &declared) {
        return Some(rejection);
    }

    // (b.3) `$inspect.trace(...)` placement (`inspect_trace_invalid_placement`): the
    // ONLY legal position is the first statement of a function body. Official throws
    // this in the analyze-phase `CallExpression` visitor — the same walk as the scope
    // checks above — so it is ordered with the script-scope family, after the binder /
    // global-reference codes and before the template-walk attribute-name / placement
    // scans. Scanned over every script AND template expression source (a misplaced
    // trace in an interpolation / handler value is the same official hard error).
    if let Some(rejection) = scan_inspect_trace_placement(source, parsed) {
        return Some(rejection);
    }

    // (c) The template-walk `attribute_invalid_name` — a plain attribute on an INTRINSIC
    // element (or `<svelte:element>`) whose NAME is not a valid HTML attribute name.
    // Official rejects this at the PARSE phase (`read/element.js`); Verter's forgiving
    // parser accepts the markup, so the analyze gate mirrors the rejection. Ordered AFTER
    // the script-scope / global-reference checks (official reports the script global error
    // before this attribute-name error — a co-located `$foo` global + `<div 1foo>` rejects
    // as `global_reference_invalid`) and BEFORE the placement scan (an invalid name on a
    // nested `<button>` rejects as `attribute_invalid_name`, not the placement defect).
    if let Some(rule) = scan_attribute_invalid_name(&parsed.template) {
        return Some(OfficialRejection::of(rule));
    }

    // (d) The template-walk `node_invalid_placement` — the disallowed-descendant REPAIR
    // families (a nested `<a>` / `<button>`, a heading-in-heading). Runs LAST in the analyze
    // phase (after the directive-value, script-scope / global-reference, and attribute-name
    // checks) and ONLY on a clean parse (the explicit-`</p>` autoclose is now a PARSE defect
    // minted by the parser, so it is excluded from this scan).
    if let Some(rule) = scan_html_placement(&parsed.template, &mut Vec::new()) {
        return Some(OfficialRejection::of(rule));
    }

    None
}

/// Scan template nodes for a directive with a static-TEXT value on a NON-`style:` directive
/// — the official `directive_invalid_value` parse error. Returns the rule on the FIRST
/// violating directive in document order, or `None`.
///
/// Mirrors the official `phases/1-parse/state/element.js` rule EXACTLY: a directive value is
/// invalid when `first_value.type === 'Text'` OR the value is a multi-chunk mixed value
/// (`value.length > 1`); a `StyleDirective` is the SOLE exemption (it accepts a text value,
/// which folds as a quoted string). Driven from the typed directive value variant, never a
/// raw-source heuristic.
fn scan_directive_invalid_value(
    source: &str,
    nodes: &[SvelteNode],
) -> Option<CoreOfficialValidationRule> {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                for attr in &el.attributes {
                    if let SvelteAttributeKind::Directive(dir) = &attr.kind {
                        // `style:` is the SOLE directive family that accepts a static-text
                        // value; every other directive requires an expression.
                        if dir.kind == SvelteDirectiveKind::Style {
                            continue;
                        }
                        if directive_value_is_invalid_text(source, &dir.value) {
                            return Some(CoreOfficialValidationRule::DirectiveInvalidValue);
                        }
                    }
                }
                if let Some(rule) = scan_directive_invalid_value(source, &el.children) {
                    return Some(rule);
                }
            }
            SvelteNode::Block(block) => {
                if let Some(rule) = scan_directive_invalid_value(source, &block.children) {
                    return Some(rule);
                }
                for clause in &block.clauses {
                    if let Some(rule) = scan_directive_invalid_value(source, &clause.children) {
                        return Some(rule);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a directive's value is a STATIC-TEXT value that the official compiler rejects on
/// a non-`style:` directive (`first_value.type === 'Text'` OR a multi-chunk mixed value).
///
/// - `None` (a valueless shorthand `class:on`) → NOT invalid (the implied same-named
///   expression).
/// - `Text` (a quoted plain-text body `class:on="x"`) → INVALID (the `type === 'Text'` arm).
/// - `Expression` (a bare `class:on={x}`) → NOT invalid.
/// - `Mixed` → invalid UNLESS it is EXACTLY one `{…}` ExpressionTag spanning the whole body
///   (`class:on="{x}"`, the `value.length === 1` shape); any surrounding text / extra chunk
///   (`class:on="a{x}"` / `class:on="{x}{y}"`) is the `value.length > 1` arm → INVALID.
fn directive_value_is_invalid_text(source: &str, value: &Option<SvelteAttributeValue>) -> bool {
    match value {
        None => false,
        Some(SvelteAttributeValue::Expression(_)) => false,
        Some(SvelteAttributeValue::Text(_)) => true,
        Some(SvelteAttributeValue::Mixed(span)) => {
            // A single `{…}` ExpressionTag spanning the whole quoted body (no surrounding
            // text, whitespace included) is the `value.length === 1` shape → valid; anything
            // else (text before/after, a second interpolation) is `value.length > 1`.
            !mixed_value_is_single_expression(source, *span)
        }
    }
}

/// Whether a quoted mixed value body is EXACTLY one `{…}` interpolation spanning the whole
/// body (no bytes before `{` or after the matching `}`) — the official `value.length === 1`
/// ExpressionTag-only shape. Uses the SHARED JS-aware brace scanner so a `}` inside a
/// string / template literal within the interpolation does not close it early.
fn mixed_value_is_single_expression(source: &str, span: verter_span::Span) -> bool {
    mixed_single_expression_inner(source, span).is_some()
}

/// The INNER expression slice of a quoted mixed value body that is EXACTLY one `{…}`
/// interpolation spanning the whole body (no bytes before `{` or after the matching `}`)
/// — the official `value.length === 1` ExpressionTag shape; `None` for any surrounding
/// text / extra chunk. Uses the SHARED JS-aware brace scanner ([`find_matching_brace_in`])
/// so a `}` inside a string / template literal / comment within the interpolation does
/// not close it early.
fn mixed_single_expression_inner(source: &str, span: verter_span::Span) -> Option<&str> {
    let text = &source[span.start as usize..span.end as usize];
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None; // text (or whitespace) before the interpolation
    }
    let close = find_matching_brace_in(bytes, 1);
    // The matching `}` must be the LAST byte of the body (no trailing text).
    if close != bytes.len().saturating_sub(1) || bytes.get(close) != Some(&b'}') {
        return None;
    }
    Some(&text[1..close])
}

/// The inner expression SOURCE of a directive value that is a SINGLE expression — a bare
/// `{expr}` ([`SvelteAttributeValue::Expression`], whose span already excludes the braces)
/// OR a QUOTED single-expression `"{expr}"` ([`SvelteAttributeValue::Mixed`] whose body is
/// exactly one `{…}` ExpressionTag). Returns `None` for a valueless directive, a
/// static-text value, or a multi-chunk mixed value (none of which is a sequence target).
/// The quoted-`Mixed` inner is located through the SHARED JS-aware brace scanner, so a
/// quoted `"{get, set}"` is scanned IDENTICALLY to a bare `{get, set}` — the official
/// compiler stores both as the same lone inner expression.
fn single_expression_source<'s>(
    source: &'s str,
    value: &Option<SvelteAttributeValue>,
) -> Option<&'s str> {
    match value.as_ref()? {
        SvelteAttributeValue::Expression(span) => {
            Some(&source[span.start as usize..span.end as usize])
        }
        SvelteAttributeValue::Mixed(span) => mixed_single_expression_inner(source, *span),
        SvelteAttributeValue::Text(_) => None,
    }
}

/// The SINGLE document/attribute-order bind-validation pass: for each `bind:` directive, in
/// document/attribute order, compute the [`BindTargetFact`] ONCE and run the bind-target SHAPE
/// scans — group policy → parens → invalid-expression — returning the FIRST shape rejection,
/// or `None`. Replaces the three former per-category whole-tree scans (which re-parsed the
/// target 3× and, by running group-FIRST across the whole tree, could MISORDER multiple bind
/// errors); the single document-order pass matches official's per-`BindDirective` walk.
///
/// The shape scans run ONLY for binds official carries to expression validation:
/// - an INTRINSIC host whose bind NAME / HOST / host-ATTRIBUTE is valid
///   ([`intrinsic_bind_reaches_shape_validation`] — the shared `bind_contract` routing + the
///   `host_attr_gate` authority). A name/host/host-attr-INVALID intrinsic bind is a
///   name/host/host-attr official reject BEFORE expression validation, so it is SKIPPED here
///   and fails closed downstream via the existing unsupported channel (the exact
///   name/host/host-attribute codes are deferred to D-29) — never a wrong shape code;
/// - ANY non-intrinsic (component / special) host — official has no DOM name/host/host-attr
///   check there and validates straight to expression shape, so the shape scans always run
///   (this PRESERVES the official-matching shape codes for component / special-element binds,
///   which 5f owns; it never OPENS such a host — a shape reject is fail-closed).
///
/// Within a valid bind, the order is group policy (the data-driven `IdentifierOrMemberOnly`
/// [`BindTargetPolicy`] — only `bind:group` — rejects ANY sequence target) → author-paren
/// sequence (`bind_invalid_parens`) → structurally-invalid expression
/// (`bind_invalid_expression`), so the more-specific codes win. A TS-wrapped lvalue is EXCLUDED
/// from the invalid-expression arm (the parse-error / D-26 class — `lvalue_contains_ts`). Bare
/// `{expr}` and quoted `"{expr}"` values are unwrapped identically via the shared
/// [`single_expression_source`]; every decision is STRUCTURAL over the typed fact (NEVER a
/// source-text scan). Recurses the same node families the directive-value scan walks.
fn scan_bind_shape_violations(source: &str, nodes: &[SvelteNode]) -> Option<OfficialRejection> {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                let host_is_intrinsic = matches!(el.kind, SvelteElementKind::Intrinsic);
                for attr in &el.attributes {
                    let SvelteAttributeKind::Directive(dir) = &attr.kind else {
                        continue;
                    };
                    if dir.kind != SvelteDirectiveKind::Bind {
                        continue;
                    }
                    // A bare `{expr}` OR a quoted single-expression `"{expr}"`; a static-text /
                    // multi-chunk mixed value is not a bind target (a non-`style:` static-text
                    // value is the parse-phase `directive_invalid_value`, handled earlier).
                    let Some(target_src) = single_expression_source(source, &dir.value) else {
                        continue;
                    };
                    // The bind-target fact, computed ONCE per directive (kills the former
                    // per-category triple reparse) — one parse through the single
                    // `BindTargetFact` constructor, structural over the parsed target.
                    let alloc = oxc_allocator::Allocator::default();
                    let fact = BindTargetFact::from_source(&alloc, target_src);
                    // GATE (intrinsic only): skip the shape scans for a name/host/host-attr-
                    // invalid intrinsic bind — official reports a name/host/host-attr code for
                    // it FIRST, so it fails closed downstream (the unsupported channel; exact
                    // codes deferred to D-29). A non-intrinsic (component / special) host has no
                    // such DOM pre-emption and always reaches the scans.
                    if host_is_intrinsic
                        && !intrinsic_bind_reaches_shape_validation(
                            &dir.local,
                            &el.name,
                            source,
                            &el.attributes,
                        )
                    {
                        continue;
                    }
                    // Within-bind order: group policy → parens → invalid-expression.
                    // (1) `bind:group` (the data-driven identifier/member-only policy) rejects
                    // ANY `SequenceExpression` target with the policy's exact official code,
                    // BEFORE the two-element shape check — so the group code beats parens.
                    if let BindTargetPolicy::IdentifierOrMemberOnly { official_code } =
                        bind_target_policy(&dir.local, &el.name)
                    {
                        if fact.is_sequence {
                            return Some(OfficialRejection::with_code(
                                CoreOfficialValidationRule::BindGroupInvalidExpression,
                                official_code,
                            ));
                        }
                    }
                    // (2) Author parens around a getter/setter sequence (`bind:value={(g,s)}`).
                    if fact.is_parenthesized_sequence {
                        return Some(OfficialRejection::of(
                            CoreOfficialValidationRule::BindInvalidParens,
                        ));
                    }
                    // (3) A structurally-invalid (non-lvalue / non-pair, non-TS) target.
                    if fact.is_invalid_bind_expression {
                        return Some(OfficialRejection::of(
                            CoreOfficialValidationRule::BindInvalidExpression,
                        ));
                    }
                }
                if let Some(rejection) = scan_bind_shape_violations(source, &el.children) {
                    return Some(rejection);
                }
            }
            SvelteNode::Block(block) => {
                if let Some(rejection) = scan_bind_shape_violations(source, &block.children) {
                    return Some(rejection);
                }
                for clause in &block.clauses {
                    if let Some(rejection) = scan_bind_shape_violations(source, &clause.children) {
                        return Some(rejection);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a `bind:` on an INTRINSIC host passes the official NAME / HOST / host-ATTRIBUTE
/// checks (so official carries it to expression-shape validation). `bind:this` is valid on any
/// intrinsic element (host-routed, no DOM-value routing — the SAME `this` discriminant the
/// runtime bind classifier uses for the `This` shape); every other bind must resolve a runtime
/// routing ([`resolve_runtime_bind`] — name + host) AND pass the host-attribute gate
/// ([`super::host_attr_gate::host_attr_gate_passes_parsed`], the SHARED host-attribute
/// authority over the parsed attributes). Typed facts only — never a string heuristic.
fn intrinsic_bind_reaches_shape_validation(
    name: &str,
    tag: &str,
    source: &str,
    attrs: &[SvelteAttribute],
) -> bool {
    if name == "this" {
        return true;
    }
    match resolve_runtime_bind(name, tag) {
        Some(routing) => {
            super::host_attr_gate::host_attr_gate_passes_parsed(name, tag, &routing, source, attrs)
        }
        None => false,
    }
}

/// Scan template nodes for a PLAIN attribute on an INTRINSIC element (or a
/// `<svelte:element>`) whose NAME is not a valid HTML attribute name — the official
/// `attribute_invalid_name` parse error. Returns the rule on the FIRST violating
/// attribute in document order, or `None`.
///
/// Mirrors the official `phases/1-parse/read/element.js` rule: an intrinsic element's
/// attribute name is validated against the regex `/(^[0-9-.])|[\^$@%&#?!|()[\]{}^*+~;]/`
/// (rejected iff it starts with a digit / `-` / `.`, or contains one of the operator
/// characters). A COMPONENT takes quoted prop keys, so its prop names are NOT validated
/// (official accepts `<Foo 1foo="x" />`). Driven from the typed attribute-name string +
/// element kind, never a raw-source heuristic. Recurses the same node families the
/// directive-value scan walks.
fn scan_attribute_invalid_name(nodes: &[SvelteNode]) -> Option<CoreOfficialValidationRule> {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                // Only INTRINSIC elements and `<svelte:element this={…}>` validate attribute
                // names; a component / `<svelte:component>` / `<svelte:self>` / other special
                // takes quoted prop keys (official accepts an invalid prop name there).
                let validates = matches!(
                    el.kind,
                    SvelteElementKind::Intrinsic
                        | SvelteElementKind::Special(SvelteSpecialKind::Element)
                );
                if validates {
                    for attr in &el.attributes {
                        if let SvelteAttributeKind::Plain { name, .. } = &attr.kind {
                            if attribute_name_is_invalid(name) {
                                return Some(CoreOfficialValidationRule::AttributeInvalidName);
                            }
                        }
                    }
                }
                if let Some(rule) = scan_attribute_invalid_name(&el.children) {
                    return Some(rule);
                }
            }
            SvelteNode::Block(block) => {
                if let Some(rule) = scan_attribute_invalid_name(&block.children) {
                    return Some(rule);
                }
                for clause in &block.clauses {
                    if let Some(rule) = scan_attribute_invalid_name(&clause.children) {
                        return Some(rule);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether an attribute NAME is invalid per the official `attribute_invalid_name` rule
/// (the pinned `svelte@5.56.3` regex `/(^[0-9-.])|[\^$@%&#?!|()[\]{}^*+~;]/`): the FIRST
/// character is a digit / `-` / `.`, OR the name CONTAINS one of
/// `^ $ @ % & # ? ! | ( ) [ ] { } * + ~ ;`. A colon name (`foo:bar`), a leading `_`, and
/// mid-name `-` / `.` (`data-x` / `aria-label` / `_foo`) are all VALID. Expressed as a
/// typed char match, never a regex over source text.
fn attribute_name_is_invalid(name: &str) -> bool {
    // FIRST char: a digit / `-` / `.` is invalid (`1foo` / `-foo` / `.foo`). An empty name
    // cannot reach here (the parser never produces a nameless Plain attribute), so the
    // first-char check is on a present byte.
    if let Some(first) = name.chars().next() {
        if first.is_ascii_digit() || first == '-' || first == '.' {
            return true;
        }
    }
    // ANY char in the operator set anywhere in the name is invalid.
    name.chars().any(|c| {
        matches!(
            c,
            '^' | '$'
                | '@'
                | '%'
                | '&'
                | '#'
                | '?'
                | '!'
                | '|'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '*'
                | '+'
                | '~'
                | ';'
        )
    })
}

/// Select the FIRST-discovered (minimum `encounter_order`) unsuppressed PARSE defect from
/// the parser's three encounter-ordered fact rails — the SOLE parse-error arbitration
/// source. `span` (the report anchor on each fact) NEVER participates; arbitration is
/// purely by `encounter_order`, the parser's single forward-pass discovery sequence.
///
/// The rails:
/// - the parser-recorded [`CloseTagViolation`]s — an unclosed intrinsic element
///   (`element_unclosed`), a stray / mismatched close (`element_invalid_closing_tag`), or a
///   void-content close (`void_element_invalid_content`). An `Unclosed` for a `<p>` that is
///   in an IMPLICIT-autoclose situation (a direct disallowed block child, NO explicit
///   `</p>`) is SUPPRESSED — official AUTO-CLOSES it and ACCEPTS, so that `<p>` is a
///   deferrable unsupported FEATURE downstream, never `element_unclosed`;
/// - the strict-parse facts ([`ParsedSvelte::strict_parse_errors`]) — the single broad
///   [`CoreOfficialValidationRule::ParserStrictness`] rule, carrying the exact official code;
/// - the parse-domain reject facts ([`ParsedSvelte::parse_reject_facts`]) — the `<script>`
///   attribute / duplicate-script rejects and the explicit-`</p>` autoclose, each minted at
///   its parser discovery point with its exact official code.
///
/// [`CloseTagViolation`]: crate::svelte::parser::CloseTagViolation
/// [`ParsedSvelte::strict_parse_errors`]: crate::svelte::parser::ParsedSvelte::strict_parse_errors
/// [`ParsedSvelte::parse_reject_facts`]: crate::svelte::parser::ParsedSvelte::parse_reject_facts
fn select_parse_phase_defect(source: &str, parsed: &ParsedSvelte) -> Option<OfficialRejection> {
    // The `<p>` elements in an IMPLICIT-autoclose situation (a direct disallowed block child
    // but NO explicit `</p>`) — their parser-reported `Unclosed` is a FEATURE, not a reject,
    // so it is suppressed below. (The EXPLICIT-`</p>` autoclose is a parse_reject_fact, not
    // an Unclosed.)
    let implicit_autoclose_p_spans =
        collect_implicit_autoclose_paragraph_open_spans(&parsed.template);

    // Track the minimum-`encounter_order` unsuppressed defect across all three rails.
    let mut best: Option<(u32, OfficialRejection)> = None;
    let mut consider = |order: u32, rejection: OfficialRejection| {
        if best.is_none_or(|(o, _)| order < o) {
            best = Some((order, rejection));
        }
    };

    for v in &parsed.close_tag_violations {
        let rule = match v.kind {
            CloseTagViolationKind::Unclosed => {
                // Suppress an `Unclosed` for a `<p>` official auto-closes (the implicit-
                // autoclose feature case) — it is not `element_unclosed`.
                if implicit_autoclose_p_spans.contains(&v.span.start) {
                    continue;
                }
                CoreOfficialValidationRule::ElementUnclosed
            }
            CloseTagViolationKind::InvalidClosingTag => {
                CoreOfficialValidationRule::ElementInvalidClosingTag
            }
            CloseTagViolationKind::VoidElementInvalidContent => {
                CoreOfficialValidationRule::VoidElementInvalidContent
            }
        };
        consider(v.encounter_order, OfficialRejection::of(rule));
    }

    for fact in &parsed.strict_parse_errors {
        consider(
            fact.encounter_order,
            OfficialRejection {
                rule: CoreOfficialValidationRule::ParserStrictness,
                official_code: fact.official_code,
            },
        );
    }

    for fact in &parsed.parse_reject_facts {
        let rule = match fact.kind {
            SvelteParseRejectKind::ScriptReservedAttribute => {
                CoreOfficialValidationRule::ScriptReservedAttribute
            }
            SvelteParseRejectKind::ScriptInvalidContext => {
                CoreOfficialValidationRule::ScriptInvalidContext
            }
            SvelteParseRejectKind::ScriptDuplicate => CoreOfficialValidationRule::ScriptDuplicate,
            SvelteParseRejectKind::StyleDuplicate => CoreOfficialValidationRule::StyleDuplicate,
            SvelteParseRejectKind::AttributeDuplicate => {
                CoreOfficialValidationRule::AttributeDuplicate
            }
            // A duplicate / nested root-only `<svelte:*>` meta element, OR an invalid
            // `<svelte:options>` attribute / child-content (the `read_options` finalization) —
            // all ride the `OptionsInvalid` rule (the meta-element class), carrying the exact site
            // code (`svelte_meta_duplicate` / `svelte_meta_invalid_placement` /
            // `svelte_options_*` / `svelte_meta_invalid_content`) per fact.
            SvelteParseRejectKind::SvelteMetaDuplicate
            | SvelteParseRejectKind::SvelteMetaInvalidPlacement
            | SvelteParseRejectKind::OptionsInvalid => CoreOfficialValidationRule::OptionsInvalid,
            SvelteParseRejectKind::ParagraphAutoclose => {
                CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed
            }
            // The body-parse reject is NOT carried as a parse_reject_fact (the parser does not
            // run OXC); it is filled from the RESERVED body-probe slots below.
            SvelteParseRejectKind::ScriptBodyParse => continue,
        };
        consider(
            fact.encounter_order,
            OfficialRejection::with_code(rule, fact.official_code),
        );
    }

    // FILL the RESERVED script-body-parse slots: parse each script body ONCE with OXC at the
    // probe's grammar (plain `<script>` = JS, `lang="ts"` = TS). A parse FAILURE mints
    // `js_parse_error` at the probe's RESERVED `encounter_order` (the upstream-faithful
    // body-parse position — strictly after the open-tag attribute-duplicate, before the
    // source-order semantic-attr faults — NOT the body span or this execution time). A body
    // that parses CLEAN contributes NO defect.
    for probe in &parsed.script_body_probes {
        let body = &source[probe.body_span.start as usize..probe.body_span.end as usize];
        if script_body_fails_to_parse(body, probe.grammar) {
            consider(
                probe.encounter_order,
                OfficialRejection::with_code(
                    CoreOfficialValidationRule::ScriptBodyParse,
                    "js_parse_error",
                ),
            );
        }
    }

    // FILL the RESERVED style-body-parse slots: run the faithful `read/style.js` CSS body reader
    // from each `<style>`'s content-start. A CSS body parse FAILURE mints the EXACT upstream CSS
    // parse code (`css_expected_identifier` / `css_empty_declaration` / `css_selector_invalid` /
    // `expected_token` / `unexpected_eof`) at the probe's RESERVED `encounter_order` — the
    // upstream `read_style` position, BEFORE the `style_duplicate` check — so a malformed 2nd
    // (or 1st) style body wins the first-error race over the duplicate. A body that parses CLEAN
    // contributes NO defect (the later `style_duplicate` / unsupported-`<style>` rail wins). The
    // reader parses from the ORIGINAL source cursor (NOT an isolated slice): upstream's nested
    // CSS readers run PAST `</style>` into the rest of the source, and that decides the code.
    for probe in &parsed.style_body_probes {
        if let Some(code) =
            super::css_reject::css_body_parse_error(source, probe.content_start as usize)
        {
            consider(
                probe.encounter_order,
                OfficialRejection::with_code(CoreOfficialValidationRule::StyleBodyParse, code),
            );
        }
    }

    // ARBITRATE the RESOLVED `<svelte:options customElement={EXPR}>` validation slots: the
    // parser already ran the one validate+extract engine at options finalization and RETAINED
    // the typed outcome on each probe — this loop only routes a retained reject code to its
    // upstream-faithful position (no re-parse). A SYNTACTIC attribute-expression PARSE fault — a
    // malformed prefix (`js_parse_error`) OR a clean prefix with trailing junk before the `}`
    // (`expected_token`) — mints AT THE PARSE POSITION (`parse_encounter_order` — upstream's
    // `read_expression` runs during the `<svelte:options>` attribute loop, so it beats a LATER
    // template defect / duplicate attribute and loses to an EARLIER one); a parseable-and-fully-
    // consumed-but-invalid expression mints the EXACT `svelte_options_*` code AT THE FINALIZATION
    // POSITION (`encounter_order` — upstream's `read_options` runs after the whole template parse,
    // losing to ANY template parse defect). A retained ACCEPT (a `null` literal, a valid object)
    // contributes NO defect — the native client path lowers its retained descriptor.
    for probe in &parsed.options_custom_element_probes {
        if let Err(code) = &probe.resolution {
            let code = *code;
            let order = if is_options_ce_attribute_parse_fault(code) {
                probe.parse_encounter_order
            } else {
                probe.encounter_order
            };
            consider(
                order,
                OfficialRejection::with_code(CoreOfficialValidationRule::OptionsInvalid, code),
            );
        }
    }

    best.map(|(_, rejection)| rejection)
}

/// Whether a `customElement={EXPR}` disposition code is a SYNTACTIC attribute-expression PARSE fault
/// — one upstream raises during the `<svelte:options>` attribute loop (`read_expression`), so it
/// rides the attribute's source position (`parse_encounter_order`) rather than the `read_options`
/// finalization position. The two parse-phase codes are `js_parse_error` (a malformed prefix /
/// empty inner) and `expected_token` (a clean prefix with trailing junk before the `}` — the
/// missing brace). Every `svelte_options_*` code is a finalization VALIDATION fault (NOT this).
fn is_options_ce_attribute_parse_fault(code: &str) -> bool {
    matches!(code, "js_parse_error" | "expected_token")
}

/// Whether a `<script>` body FAILS to parse the way upstream's Acorn parse does — the
/// body-probe fill. A plain `<script>` parses as JS (`SourceType::mjs()` — module JS, no TS,
/// no JSX, the Acorn-equivalent: TS-only syntax in a plain script is a parse error); a
/// `lang="ts"` body parses as TS (`SourceType::ts()`). A panic OR a non-empty parser error
/// set is a failure (`js_parse_error`).
///
/// Plus the ONE parse-phase error OXC's PARSER defers to its binder but Acorn raises at parse:
/// a same-scope LEXICAL (`let`/`const`) REDECLARATION (`let a; let a`). It is detected
/// structurally on the parsed program's TOP-LEVEL declarators (the §1.2-core script surface is
/// top-level `let`/`const`), so it stays a body-slot `js_parse_error` — never a later analyze
/// fallback. Driven from the typed [`ScriptBodyProbe`] grammar + the typed AST, never a text
/// heuristic.
fn script_body_fails_to_parse(body: &str, grammar: ScriptBodyGrammar) -> bool {
    let alloc = Allocator::default();
    let source_type = match grammar {
        ScriptBodyGrammar::Js => oxc_span::SourceType::mjs(),
        ScriptBodyGrammar::Ts => oxc_span::SourceType::ts(),
    };
    let parsed = oxc_parser::Parser::new(&alloc, alloc.alloc_str(body), source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return true;
    }
    top_level_lexical_redeclaration(&parsed.program)
}

/// Whether the program's TOP-LEVEL declarations contain a same-scope LEXICAL redeclaration —
/// a name bound by `let` / `const` that is also bound by ANOTHER top-level `let` / `const` /
/// `var` declarator (the ECMAScript early SyntaxError Acorn raises at parse but OXC's parser
/// defers to its binder). `var`/`var` re-binding of the same name (legal in JS) is NOT a
/// redeclaration.
///
/// SCOPE (deliberate, NOT an over-claim): this detects ONLY the `let` / `const` redeclaration
/// reachable in the §1.2-core SUPPORTED script surface (top-level `let` / `const` — `$state` /
/// props-destructure / `bind:this` locals). A redeclaration involving a top-level FUNCTION /
/// CLASS / IMPORT declaration (`function f(){} function f(){}`, `class A{} class A{}`,
/// `import x; let x`) — which upstream also `js_parse_error`s — is NOT detected here and does not
/// need to be: a top-level function / class / import is itself OUTSIDE the §1.2-core allowlist, so
/// such a component fails closed as an unsupported FEATURE BEFORE this body-probe code matters. So
/// no REACHABLE official-reject in the supported surface is missed (characterized by
/// `redeclaration_scope_is_let_const_only_function_collisions_fail_closed`).
fn top_level_lexical_redeclaration(program: &Program) -> bool {
    use oxc_ast::ast::VariableDeclarationKind;
    // (name, was_lexical) in source order across the top-level variable declarators.
    let mut bound: Vec<(String, bool)> = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        let lexical = matches!(
            decl.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Const
        );
        for d in &decl.declarations {
            let mut names = Vec::new();
            collect_pattern_names(&d.id, &mut names);
            for name in names {
                // A collision is a redeclaration error when EITHER the prior or the current
                // binding is lexical (`let`/`const`); two `var`s of the same name are legal.
                if let Some((_, prior_lexical)) = bound.iter().find(|(n, _)| *n == name) {
                    if *prior_lexical || lexical {
                        return true;
                    }
                }
                bound.push((name, lexical));
            }
        }
    }
    false
}

/// The module + instance script content sources (the inner text of each `<script>`), in
/// MODULE-then-INSTANCE order — matching upstream's analyze pass, which constructs the module
/// scope before the instance scope (`phases/2-analyze/index.js`) and walks
/// `[module, instance, template]`. So a module-script defect (a global `$foo` reference, a
/// `$`-prefixed binding) is reported BEFORE an instance-script defect. Empty when neither
/// script is present.
fn script_sources<'a>(source: &'a str, parsed: &ParsedSvelte) -> Vec<&'a str> {
    let mut out = Vec::new();
    for content in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        out.push(&source[content.start as usize..content.end as usize]);
    }
    out
}

/// Scan ONE script's top-level declarations for a `$` / `$$`-prefixed binding NAME (a
/// declaration-position binding official's `validate_identifier_name` binder rejects —
/// `dollar_prefix_invalid`; the official message names BOTH forms: "The $ prefix is
/// reserved, and cannot be used for variables and imports"). Driven from the OXC AST of
/// the reparsed script. Returns an [`OfficialRejection`], or `None`.
///
/// A SAME-lexical-scope duplicate declaration (`let a; let a`) is NOT detected here — it is a
/// PARSE-phase error Acorn (and the OXC body-probe) rejects, owned by the body-probe
/// `js_parse_error` slot (a clean body never reaches the analyze phase). So this scan is the
/// `$`-prefix binder check only.
fn scan_script_declaration_rules(script_source: &str) -> Option<OfficialRejection> {
    use oxc_ast::ast::{ImportDeclarationSpecifier, ImportOrExportKind};

    let alloc = Allocator::default();
    let program = reparse_module(&alloc, script_source)?;

    // The top-level binding names official's binder validates: every `let`/`const`/
    // `var` declarator pattern name, plus every VALUE import specifier's LOCAL binding
    // (default / named-`as` local / namespace), in source order. A type-only import
    // (`import type …` / a per-specifier `type` modifier) binds no VALUE — the TS
    // strip removes it — so it is not scanned.
    let mut decl_names: Vec<String> = Vec::new();
    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    collect_pattern_names(&d.id, &mut decl_names);
                }
            }
            Statement::ImportDeclaration(import) => {
                if !matches!(import.import_kind, ImportOrExportKind::Value) {
                    continue;
                }
                let Some(specifiers) = &import.specifiers else {
                    continue;
                };
                for spec in specifiers {
                    let local = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            if matches!(s.import_kind, ImportOrExportKind::Type) {
                                continue; // `import { type Foo as $x }` — type-only
                            }
                            &s.local
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
                    };
                    decl_names.push(local.name.to_string());
                }
            }
            _ => {}
        }
    }

    // A `$` / `$$`-prefixed binding name in ANY top-level declarator pattern / import
    // local position. Official `validate_identifier_name` errors at the binder for a
    // `$`-prefixed binding at the top level (`dollar_prefix_invalid`). (A clean body
    // that reaches here parses fine, so a same-scope redeclaration would already have
    // failed the body-probe.)
    if decl_names.iter().any(|n| n.starts_with('$')) {
        return Some(OfficialRejection::of(
            CoreOfficialValidationRule::DollarPrefixInvalid,
        ));
    }

    None
}

/// Scan for a GLOBAL `$foo` / `$$foo` reference in any script or template expression
/// position. Driven from the OXC AST of each expression source, scope-awarely. Returns an
/// [`OfficialRejection`] carrying the EXACT site-specific code.
fn scan_global_dollar_references(
    source: &str,
    parsed: &ParsedSvelte,
    declared: &FxHashSet<String>,
) -> Option<OfficialRejection> {
    // The script bodies + every template interpolation / directive / attribute
    // expression source.
    let mut sources: Vec<String> = script_sources(source, parsed)
        .into_iter()
        .map(str::to_string)
        .collect();
    collect_template_expression_sources(source, &parsed.template, &mut sources);

    for src in &sources {
        if let Some(rejection) = scan_dollar_refs_in_expression(src, declared) {
            return Some(rejection);
        }
    }
    None
}

/// Scan ONE expression / statement source for a global `$`-prefixed identifier
/// reference, scope-awarely (a local binding of the same name is not a global ref).
fn scan_dollar_refs_in_expression(
    src: &str,
    declared: &FxHashSet<String>,
) -> Option<OfficialRejection> {
    let alloc = Allocator::default();
    // Parse as a statement source; a bare expression source is wrapped so it parses.
    let program = reparse_module(&alloc, src).or_else(|| {
        let wrapped = format!("({src});");
        reparse_module(&alloc, &wrapped)
    })?;
    let mut scan = DollarRefScan {
        declared,
        scopes: ShadowStack::default(),
        found: None,
    };
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    scan.found
}

/// The scope-aware scan state for a global `$`-prefixed reference.
struct DollarRefScan<'a> {
    declared: &'a FxHashSet<String>,
    scopes: ShadowStack,
    found: Option<OfficialRejection>,
}

impl DollarRefScan<'_> {
    /// The EXACT official code a `$`-prefixed reference `name` violates, or `None` when it
    /// is not a global-reference violation.
    ///
    /// Mirrors the official `analyze` store-subscription guard
    /// (`phases/2-analyze/index.js`): a `$`-prefixed reference is checked UNLESS it is a
    /// recognised RUNE (`is_rune(name)`). The reserved magic objects carry their EXACT
    /// runes-mode codes (`$$props` → `legacy_props_invalid`, `$$restProps` →
    /// `legacy_rest_props_invalid`); a general `$$foo` (double-dollar) or an undeclared
    /// lowercase-initial `$foo` store subscription is `global_reference_invalid`.
    fn global_violation_code(&self, name: &str) -> Option<&'static str> {
        if !name.starts_with('$') || name == "$" {
            return None;
        }
        // A recognised Svelte RUNE root reference (`$state` / `$derived` / `$props` /
        // `$effect` / `$bindable` / `$inspect` / `$host`, plus their `.raw` / `.by` /
        // `.pre` / … member keypaths reached through the root identifier) is NOT a
        // global store reference — official excludes it via `is_rune(name)`. A
        // shadowed rune name (a local of the same name) is also not a global ref.
        if super::rune_scan::RUNE_ROOT_NAMES.contains(&name) {
            return None;
        }
        if self.scopes.is_shadowed(name) || self.declared.contains(name) {
            // A locally-declared `$`-prefixed binding is invalid too, but that is the
            // `DollarPrefixInvalid` class owned by the declaration scan; here we only
            // flag an UNDECLARED global reference.
            return None;
        }
        // `$$slots` is ACCEPTED by official (it is a valid auto-injected magic object);
        // Verter refuses it only as an unsupported FEATURE (the deferrable 5w
        // magic-identifier path), NEVER an official reject. So it is not a violation
        // here — fall through to the magic-identifier refusal downstream.
        if name == "$$slots" {
            return None;
        }
        // `$$props` / `$$restProps` are an OFFICIAL REJECT in runes mode, each with its OWN
        // exact code.
        if name == "$$props" {
            return Some("legacy_props_invalid");
        }
        if name == "$$restProps" {
            return Some("legacy_rest_props_invalid");
        }
        // A general `$$foo` (double-dollar) is `global_reference_invalid`.
        if name.as_bytes().get(1) == Some(&b'$') {
            return Some("global_reference_invalid");
        }
        // A `$foo` (single-dollar) is a violation only when `foo` is undeclared AND
        // lowercase-initial (the official non-existent-store-subscription rule).
        let store = &name[1..];
        if self.declared.contains(store) || self.scopes.is_shadowed(store) {
            return None;
        }
        if store.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
            Some("global_reference_invalid")
        } else {
            None
        }
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for DollarRefScan<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = FxHashSet::default();
        super::expr::collect_direct_decls(&it.body, &mut frame);
        super::expr::collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        oxc_ast_visit::walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.scopes.push(super::expr::function_scope_names(it));
        oxc_ast_visit::walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scopes.push(super::expr::arrow_scope_names(it));
        oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(super::expr::block_scope_names(it));
        oxc_ast_visit::walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        if self.found.is_none() {
            if let Some(code) = self.global_violation_code(it.name.as_str()) {
                self.found = Some(OfficialRejection::with_code(
                    CoreOfficialValidationRule::GlobalReferenceInvalid,
                    code,
                ));
            }
        }
        oxc_ast_visit::walk::walk_identifier_reference(self, it);
    }
}

/// Scan for a `$inspect.trace(...)` call OUTSIDE its single legal position — the
/// official `inspect_trace_invalid_placement` hard error ("`$inspect.trace(...)` must
/// be the first statement of a function body"). Covers every script source AND every
/// template expression source (the same inputs the global-`$`-reference scan walks).
/// Returns the rejection on the first violating source, or `None`.
fn scan_inspect_trace_placement(source: &str, parsed: &ParsedSvelte) -> Option<OfficialRejection> {
    let mut sources: Vec<String> = script_sources(source, parsed)
        .into_iter()
        .map(str::to_string)
        .collect();
    collect_template_expression_sources(source, &parsed.template, &mut sources);

    for src in &sources {
        if source_has_misplaced_inspect_trace(src) {
            return Some(OfficialRejection::of(
                CoreOfficialValidationRule::InspectTraceInvalidPlacement,
            ));
        }
    }
    None
}

/// Whether ONE expression / statement source contains a `$inspect.trace(...)` call in
/// an ILLEGAL position. Driven from the OXC AST: the walker records the span of every
/// UNSHADOWED trace call plus an allow-set of the trace calls sitting in the ONE legal
/// position (the `expression` of an `ExpressionStatement` that is `statements[0]` of a
/// NON-generator function body — a declaration/expression `Function` or a BLOCK-bodied
/// arrow); any trace span outside the allow-set is the violation. The scan is
/// SCOPE-AWARE (it mirrors the [`DollarRefScan`] `ShadowStack`): a `$inspect`
/// PARAMETER is VALID Svelte (`($inspect) => …` / `function get($inspect) { … }` are
/// accepted by official — only a `const $inspect` LOCAL is `dollar_prefix_invalid`), so
/// `$inspect.trace()` under a local `$inspect` binding is an ORDINARY method call and is
/// ignored entirely.
fn source_has_misplaced_inspect_trace(src: &str) -> bool {
    let alloc = Allocator::default();
    // Parse as a statement source; a bare expression source is wrapped so it parses.
    let Some(program) = reparse_module(&alloc, src).or_else(|| {
        let wrapped = format!("({src});");
        reparse_module(&alloc, &wrapped)
    }) else {
        return false;
    };
    let mut scan = InspectTracePlacementScan::default();
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    scan.trace_spans
        .iter()
        .any(|span| !scan.legal_spans.contains(span))
}

/// The placement-scan state: every UNSHADOWED `$inspect.trace(...)` call span, plus the
/// allow-set of spans in the one legal (function-body-first-statement) position. Carries
/// the same lexical [`ShadowStack`] as [`DollarRefScan`] so a param-shadowed `$inspect`
/// (a valid Svelte parameter) is treated as an ordinary local and ignored.
#[derive(Default)]
struct InspectTracePlacementScan {
    /// The span of EVERY UNSHADOWED `$inspect.trace(...)` call encountered, in walk order.
    trace_spans: Vec<(u32, u32)>,
    /// The spans of the trace calls that are the first statement of a function body.
    legal_spans: FxHashSet<(u32, u32)>,
    /// The active lexical shadow frames (function/arrow params + block-local names).
    scopes: ShadowStack,
}

impl InspectTracePlacementScan {
    /// Record the ONE legal trace position of a function body: `statements[0]` being
    /// an `ExpressionStatement` whose expression is the trace call. A PARENTHESIZED
    /// wrapper (`($inspect.trace());`) is transparent — official accepts it in the
    /// same position — so parens are unwrapped (recursively) and the INNER call's
    /// span is recorded, matching what `visit_call_expression` records. A locally
    /// SHADOWED `$inspect` is an ordinary call — not the rune trace — so nothing is
    /// recorded (the caller pushes the owning function's scope frame first).
    fn allow_first_statement(&mut self, statements: &[Statement<'_>]) {
        if self.scopes.is_shadowed("$inspect") {
            return;
        }
        let Some(Statement::ExpressionStatement(es)) = statements.first() else {
            return;
        };
        let mut expr = &es.expression;
        while let Expression::ParenthesizedExpression(paren) = expr {
            expr = &paren.expression;
        }
        let Expression::CallExpression(call) = expr else {
            return;
        };
        if call_is_inspect_trace(call) {
            self.legal_spans.insert((call.span.start, call.span.end));
        }
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for InspectTracePlacementScan {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = FxHashSet::default();
        super::expr::collect_direct_decls(&it.body, &mut frame);
        super::expr::collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        oxc_ast_visit::walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        // Push THIS function's scope (params + own id + body-local decls) FIRST so
        // `allow_first_statement` sees a `$inspect` PARAM as a shadow. A
        // declaration/expression `Function` body (incl. a class method's value) hosts
        // the legal first-statement position.
        self.scopes.push(super::expr::function_scope_names(it));
        // A GENERATOR body is NOT a legal trace host — official svelte@5.56.3 rejects a
        // generator-body first-statement trace with `inspect_trace_generator`. Only a
        // NON-generator function body hosts the legal first-statement position, so a
        // generator's first statement is never admitted to the allow-set and its trace
        // rejects via the placement rule (both fail-closed). Async is fine — only
        // generators are excluded.
        if !it.r#generator {
            if let Some(body) = &it.body {
                self.allow_first_statement(&body.statements);
            }
        }
        oxc_ast_visit::walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(super::expr::arrow_scope_names(it));
        // Only a BLOCK-bodied arrow has a function BODY; a concise (expression) body
        // is an EXPRESSION position — never legal. (An arrow is never a generator.)
        if !it.r#expression {
            self.allow_first_statement(&it.body.statements);
        }
        oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(super::expr::block_scope_names(it));
        oxc_ast_visit::walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        // Only an UNSHADOWED `$inspect.trace()` is the rune trace; a param/local-shadowed
        // `$inspect` is an ordinary object method call.
        if call_is_inspect_trace(it) && !self.scopes.is_shadowed("$inspect") {
            self.trace_spans.push((it.span.start, it.span.end));
        }
        oxc_ast_visit::walk::walk_call_expression(self, it);
    }
}

/// Whether a call is `$inspect.trace(...)` — a `CallExpression` whose callee is the
/// static `.trace` member on the bare `$inspect` identifier. Typed-AST only.
fn call_is_inspect_trace(call: &CallExpression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return false;
    };
    // The member OBJECT is paren-transparent (`($inspect).trace()`) — official treats
    // redundant parens around the `$inspect` receiver as transparent.
    let mut object = &member.object;
    while let Expression::ParenthesizedExpression(paren) = object {
        object = &paren.expression;
    }
    member.property.name.as_str() == "trace"
        && matches!(object, Expression::Identifier(id) if id.name.as_str() == "$inspect")
}

/// The set of declared TOP-LEVEL local names across the instance + module scripts (an
/// accepted `bind:this` target / `$foo`-store referent must be one of these). Driven
/// from the OXC AST of each script's top-level declarators.
fn declared_top_level_locals(source: &str, parsed: &ParsedSvelte) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    for script_src in script_sources(source, parsed) {
        let alloc = Allocator::default();
        let Some(program) = reparse_module(&alloc, script_src) else {
            continue;
        };
        for stmt in &program.body {
            if let Statement::VariableDeclaration(decl) = stmt {
                for d in &decl.declarations {
                    let mut decl_names = Vec::new();
                    collect_pattern_names(&d.id, &mut decl_names);
                    for n in decl_names {
                        names.insert(n);
                    }
                }
            }
        }
    }
    names
}

/// The text value an attribute / directive value span carries (a quoted body, an
/// expression inner, or a mixed value), or `None` for a valueless directive.
fn directive_value_text(source: &str, value: &Option<SvelteAttributeValue>) -> Option<String> {
    let span = match value.as_ref()? {
        SvelteAttributeValue::Text(span)
        | SvelteAttributeValue::Expression(span)
        | SvelteAttributeValue::Mixed(span) => span,
    };
    Some(source[span.start as usize..span.end as usize].to_string())
}

/// Collect every template EXPRESSION source (interpolations, directive expressions,
/// attribute expression values, spreads, and block/clause HEAD expressions) under
/// `nodes` into `out`. Used by the global-`$`-ref scan and the `$inspect.trace()`
/// placement scan to cover every template / bind / event / block-head position.
fn collect_template_expression_sources(source: &str, nodes: &[SvelteNode], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            SvelteNode::Interpolation(span) => {
                out.push(source[span.start as usize..span.end as usize].to_string());
            }
            SvelteNode::Element(el) => {
                for attr in &el.attributes {
                    push_attribute_expression_sources(source, attr, out);
                }
                collect_template_expression_sources(source, &el.children, out);
            }
            SvelteNode::Block(block) => {
                // The block's HEAD expression (`{#if expr}` condition, `{#each list …}`
                // list, `{#await expr}` subject, `{#key expr}` key) is an EXPRESSION
                // position, not the body — collect it so the reference / placement scans
                // see it (official rejects a misplaced trace / a `$`-global there too).
                if let Some(head) = &block.head_expr {
                    out.push(source[head.start as usize..head.end as usize].to_string());
                }
                // The `{#each list as item (KEY)}` KEY is a SEPARATE expression position
                // (not folded into `head_expr`), so collect it too.
                if let SvelteBlockKind::Each { key: Some(key), .. } = &block.kind {
                    out.push(source[key.start as usize..key.end as usize].to_string());
                }
                collect_template_expression_sources(source, &block.children, out);
                for clause in &block.clauses {
                    // Only an `{:else if}` clause head is an EXPRESSION (a condition). A
                    // `{:then v}` / `{:catch e}` `clause.expr` is a BINDING PATTERN, not an
                    // expression — feeding it to the reference scan would false-reject a
                    // `$`-prefixed await binding (`{:then $foo}`, which official accepts).
                    // TODO(follow-up): a `{:then}` / `{:catch}` binding pattern can still
                    // carry global `$` references in a DEFAULT initializer (`{:then {x =
                    // $foo}}` → official `global_reference_invalid`), and a clause-bound
                    // `$`-name referenced in the clause BODY (`{:then $foo}<b>{$foo}</b>`)
                    // is official-accepted but currently false-rejected because the bound
                    // name is not registered as a declared local. Both are PRE-EXISTING
                    // await-clause global-`$`-scan gaps (unchanged by this block; the
                    // pattern spans were never scanned before). The proper fix registers
                    // the clause bindings as declared locals and scans only the pattern's
                    // default-initializer expressions — a scope-aware pattern walk, not a
                    // whole-span reparse.
                    if matches!(clause.kind, SvelteClauseKind::ElseIf) {
                        if let Some(expr) = &clause.expr {
                            out.push(source[expr.start as usize..expr.end as usize].to_string());
                        }
                    }
                    collect_template_expression_sources(source, &clause.children, out);
                }
            }
            _ => {}
        }
    }
}

/// Push the expression sources an attribute carries (a directive's bound/handler
/// expression, an expression-valued plain attribute, a spread) into `out`.
fn push_attribute_expression_sources(source: &str, attr: &SvelteAttribute, out: &mut Vec<String>) {
    match &attr.kind {
        SvelteAttributeKind::Directive(dir) => {
            if let Some(text) = directive_value_text(source, &dir.value) {
                out.push(text);
            }
        }
        SvelteAttributeKind::Plain { value, .. } => {
            if let Some(SvelteAttributeValue::Expression(span)) = value {
                out.push(source[span.start as usize..span.end as usize].to_string());
            }
        }
        SvelteAttributeKind::Spread(span) => {
            out.push(source[span.start as usize..span.end as usize].to_string());
        }
        // An `{@attach expr}` attachment carries one expression (the tokenizer-captured
        // expr span) — scanned like any other attribute expression.
        SvelteAttributeKind::Attach { expr_span } => {
            out.push(source[expr_span.start as usize..expr_span.end as usize].to_string());
        }
    }
}

/// Scan template nodes for an invalid HTML placement (the ANALYZE-phase `node_invalid_placement`
/// check), carrying the ANCESTOR element tag stack (root..parent). Returns the FIRST violating
/// rule in document order, or `None`.
///
/// ONE official mechanism on the §1.2 element universe: the REPAIR families (`a`/`button`/`h1..h6`
/// nesting) — a disallowed DESCENDANT is `node_invalid_placement`. (The `<p>` explicit-`</p>`
/// AUTO-CLOSE family is a PARSE defect — minted by the parser at the surviving `</p>` close as an
/// `element_invalid_closing_tag_autoclosed` fact — NOT a placement check; the implicit-autoclose
/// `<p>` is official-ACCEPTED.)
fn scan_html_placement(
    nodes: &[SvelteNode],
    ancestors: &mut Vec<String>,
) -> Option<CoreOfficialValidationRule> {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                let tag = element_html_tag(el);
                if let Some(tag) = &tag {
                    // REPAIR families: this element disallowed inside an ancestor.
                    if repair_placement_violation(tag, ancestors) {
                        return Some(CoreOfficialValidationRule::NodeInvalidPlacement);
                    }
                }
                // Descend with this element pushed as the new innermost ancestor (only
                // a real HTML element contributes to the ancestor chain).
                if let Some(tag) = tag {
                    ancestors.push(tag);
                    let found = scan_html_placement(&el.children, ancestors);
                    ancestors.pop();
                    if found.is_some() {
                        return found;
                    }
                } else if let Some(rule) = scan_html_placement(&el.children, ancestors) {
                    return Some(rule);
                }
            }
            SvelteNode::Block(block) => {
                if let Some(rule) = scan_html_placement(&block.children, ancestors) {
                    return Some(rule);
                }
                for clause in &block.clauses {
                    if let Some(rule) = scan_html_placement(&clause.children, ancestors) {
                        return Some(rule);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The open-tag-span starts of every `<p>` element in an IMPLICIT-autoclose situation
/// (a DIRECT disallowed block child but NO explicit `</p>` close). Official AUTO-CLOSES
/// such a `<p>` and ACCEPTS it (a warning), so the parser's `Unclosed` violation for it
/// is suppressed (the case is a deferrable unsupported FEATURE, never `element_unclosed`).
fn collect_implicit_autoclose_paragraph_open_spans(nodes: &[SvelteNode]) -> Vec<u32> {
    let mut out = Vec::new();
    collect_implicit_autoclose_paragraphs_into(nodes, &mut out);
    out
}

fn collect_implicit_autoclose_paragraphs_into(nodes: &[SvelteNode], out: &mut Vec<u32>) {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                if element_html_tag(el).as_deref() == Some("p")
                    && el.close_span.is_none()
                    && paragraph_direct_autoclose_child(el).is_some()
                {
                    out.push(el.open_span.start);
                }
                collect_implicit_autoclose_paragraphs_into(&el.children, out);
            }
            SvelteNode::Block(block) => {
                collect_implicit_autoclose_paragraphs_into(&block.children, out);
                for clause in &block.clauses {
                    collect_implicit_autoclose_paragraphs_into(&clause.children, out);
                }
            }
            _ => {}
        }
    }
}

/// The HTML tag name of an element node, or `None` for a non-HTML element (a
/// component, a `<svelte:*>` special element, a custom element) — those do not
/// participate in the HTML auto-repair placement rules ("custom elements can be
/// anything").
fn element_html_tag(el: &SvelteElement) -> Option<String> {
    match el.kind {
        SvelteElementKind::Intrinsic => {
            if el.name.contains('-') {
                None
            } else {
                Some(el.name.to_ascii_lowercase())
            }
        }
        _ => None,
    }
}

/// Whether placing `child` inside the given ancestor chain (innermost last) is an
/// official REPAIR-family placement violation (`node_invalid_placement`). Mirrors
/// `is_tag_valid_with_ancestor` for the §1.2 element universe — the REPAIRED-descendant
/// families (`a`/`button`/`h1..h6`): any ancestor in the chain that disallows `child`
/// as a descendant repairs the HTML. (The `<p>` AUTO-CLOSE family is handled separately
/// in `scan_html_placement`, gated on a surviving explicit `</p>`.)
fn repair_placement_violation(child: &str, ancestors: &[String]) -> bool {
    ancestors
        .iter()
        .rev()
        .any(|ancestor| repair_disallowed_descendant(ancestor, child))
}

/// The official `disallowed_children` REPAIR families restricted to the §1.2 element
/// universe (`a` / `button` / `h1`): each disallows the listed descendants such that
/// the browser repairs the HTML.
fn repair_disallowed_descendant(ancestor: &str, child: &str) -> bool {
    match ancestor {
        "a" => child == "a",
        "button" => child == "button",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            matches!(child, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
        }
        _ => false,
    }
}

/// The disallowed block child that triggers a `<p>` autoclose, if any — the FIRST
/// DIRECT child element of `p` whose lowercased HTML tag is in the official `<p>`
/// autoclose descendant set. Shared with the parse-domain feature gate (the implicit
/// autoclose) so both surfaces read ONE block-child predicate.
pub(super) fn paragraph_direct_autoclose_child(p: &SvelteElement) -> Option<String> {
    p.children.iter().find_map(|child| {
        if let SvelteNode::Element(c) = child {
            let tag = element_html_tag(c)?;
            // The official `<p>` autoclosing-children predicate is the parser-owned shared
            // tag-list (`tokenizer_scan`), so the parser's explicit-`</p>` autoclose mint and
            // this gate-side implicit-autoclose suppression scan read ONE block-child rule.
            crate::svelte::parser::tokenizer_scan::paragraph_autocloses_on_block_child(&tag)
                .then_some(tag)
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "official_reject_tests.rs"]
mod official_reject_tests;
