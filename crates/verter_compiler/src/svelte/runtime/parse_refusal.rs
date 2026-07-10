//! Fail-closed refusal of the PARSE-DOMAIN unsupported Svelte runtime surfaces
//! (the ones not carried on the runtime IR): a DUPLICATE attribute / directive
//! (5a, the official `attribute_duplicate` parse error), a top-level `<style>`
//! whose css output mode is unprovable or whose body fails the scoping
//! analysis (an ACCEPTED style hands its parsed + analyzed body forward as
//! the pre-lowering style stage), a `<svelte:options>` axis beyond `runes` /
//! `customElement` / `css="injected"` (including `name`, which official
//! rejects as an unknown attribute), and a dev-mode codegen request (5k). A
//! VALID `customElement` value is supported (the lowering resolves it into
//! the custom-element descriptor); its invalid forms are exact-code official
//! rejects caught before this gate.

use verter_span::Span;

use super::SvelteRuntimeOptions;
use super::UnsupportedSvelteRuntimeSurface;
use crate::svelte::parser::{
    ParsedSvelte, SvelteAttribute, SvelteElement, SvelteElementKind, SvelteNode, SvelteSpecialKind,
    SvelteStyle,
};

/// The pre-lowering style stage a top-level `<style>` produces when the
/// parse-domain gate ACCEPTS it: the detected css output mode plus the parsed
/// and analyzed css body. The pipeline completes it into the full
/// [`ProvenStyleScopePlan`](super::css::types::ProvenStyleScopePlan) once the
/// runtime IR exists (the selector-to-template matcher consumes the IR); a
/// downstream plan failure reports its OWN precise code + span.
pub(super) struct PreparedComponentStyle {
    /// The detected css output mode.
    pub(super) mode: super::css::types::CssMode,
    /// The parsed + analyzed css body (the css-domain half of the plan build).
    pub(super) analyzed: super::css::AnalyzedStyleBody,
}

/// The PARSE-DOMAIN gate (choke point 1 of the refuse-by-default pipeline): refuse
/// the unsupported surfaces the runtime IR does not carry (a `<style>` css body
/// that fails the scoping analysis or an unprovable css output mode, a
/// `<svelte:options>` axis beyond `runes` / `customElement`, and a dev-mode
/// codegen request (5k)) BEFORE lowering. Returns the FIRST refusal found as
/// `Err`; on acceptance returns the component's pre-lowering style stage
/// (`Some` when a top-level `<style>` is present — its mode detection and css
/// analysis passed; the matcher + render complete downstream over the real IR).
pub(super) fn parse_domain_gate(
    source: &str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
) -> Result<Option<PreparedComponentStyle>, UnsupportedSvelteRuntimeSurface> {
    // NOTE: a template-element `attribute_duplicate` and a duplicate `<svelte:options>` are
    // official EXACT-CODE parse errors minted by the parser (the encounter-ordered
    // `parse_reject_facts` rail), caught by the official-reject gate that runs BEFORE this
    // gate — they are NOT refused here (a code-less surface would lose the exact code + the
    // arbitration position).
    // An IMPLICIT `<p>` autoclose (a `<p>` with a DIRECT disallowed block child but NO
    // explicit `</p>`): the official compiler AUTO-CLOSES the `<p>` and re-parents the
    // block child as a sibling (a warning, then ACCEPTS). Modeling that DOM re-parenting
    // is outside the §1.2 core, so it fails closed as an unsupported FEATURE (5x) — the
    // official-reject gate already rejected the EXPLICIT-`</p>` autoclose case, so a
    // `<p>` block-child reaching here is the implicit case.
    if let Some(surface) = refuse_implicit_paragraph_autoclose(&parsed.template) {
        return Err(surface);
    }
    // A dev-mode codegen request: the dev-mode output axis is not emitted (5k).
    if opts.dev_codegen {
        return Err(UnsupportedSvelteRuntimeSurface::DevMode {
            span: Span::new(0, 0),
        });
    }
    // A `<script lang="ts">` / `lang="tsx">` TypeScript script: the TS-strip path is
    // a script-completion follow-up, so a TypeScript script fails closed (5t).
    // (TS-strip was supported; demoted to the §1.2-class plain-JS core.)
    // TODO(follow-up): strip the TS annotations (the `lang="ts"` lowering) before
    // client emission instead of failing closed. Owned by the script-completion
    // block (5t).
    for script in [
        parsed.instance_script.as_ref(),
        parsed.module_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(lang) = &script.lang {
            if matches!(lang.as_str(), "ts" | "tsx" | "typescript") {
                return Err(UnsupportedSvelteRuntimeSurface::TypeScript {
                    span: script.content.unwrap_or(script.tag_open),
                });
            }
        }
    }
    // A top-level `<style>` block: `<style>` is parsed into `parsed.styles`
    // (never a template node), so the IR walk does not see it. Detect the css
    // output mode and run the scoping parse + analysis NOW — css analysis runs
    // before template lowering, so a css failure is reported before any
    // template-lowering failure — and hand
    // the accepted stage to the pipeline (the matcher + scoped render complete
    // over the real runtime IR downstream). Never fails open: an unprovable
    // mode or a failed analysis refuses here; an unprovable matcher outcome
    // refuses downstream.
    let prepared_style = match parsed.styles.first() {
        Some(style) => Some(prepare_style_surface(source, style, parsed, opts)?),
        None => None,
    };
    // The STRICT `<svelte:options>` gate: allow ONLY an ABSENT options element, or at
    // most ONE top-level `<svelte:options>` carrying the supported axes (a boolean
    // `runes` literal — BOTH values are valid mode selections — and a VALID
    // `customElement` value). Fail closed on a duplicate / nested / non-root options
    // element, a non-boolean `runes` value, any other axis (`namespace`/…),
    // a spread / directive, or child content.
    if let Some(surface) = refuse_unsupported_options(source, parsed) {
        return Err(surface);
    }
    Ok(prepared_style)
}

/// Run the PARSE-DOMAIN half of a top-level `<style>`'s scoping pipeline —
/// css output-mode detection plus the css body parse + scoping analysis (the
/// css-domain half of the plan build) — and return the accepted pre-lowering
/// stage, or the PRECISE fail-closed surface:
///
/// - an UNPROVABLE css output mode → [`StyleCssModeUnsupported`];
/// - a css body the analyzer cannot parse / prove → [`StyleCssAnalysis`]
///   (span-precise to the offending construct).
///
/// Both checks run BEFORE template lowering (the css-first diagnostic order:
/// a css failure is reported even when the template would also fail to
/// lower). The selector-to-template matcher and the scoped render complete
/// downstream over the real runtime IR; a matcher-unprovable plan refuses
/// there on [`StyleSelectorUnsupported`].
///
/// [`StyleCssAnalysis`]: UnsupportedSvelteRuntimeSurface::StyleCssAnalysis
/// [`StyleCssModeUnsupported`]: UnsupportedSvelteRuntimeSurface::StyleCssModeUnsupported
/// [`StyleSelectorUnsupported`]: UnsupportedSvelteRuntimeSurface::StyleSelectorUnsupported
fn prepare_style_surface(
    source: &str,
    style: &SvelteStyle,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
) -> Result<PreparedComponentStyle, UnsupportedSvelteRuntimeSurface> {
    // An absent content span (a defensive case — a content-less `<style>` is
    // an official reject caught before this gate) plans an EMPTY body at the
    // open tag's end.
    let content = style
        .content
        .unwrap_or(Span::new(style.tag_open.end, style.tag_open.end));

    // The css output mode is a PARSE-DOMAIN fact (options element + compile
    // option); an unprovable mode fails closed on the mode surface (the
    // `<style>` content, or the open tag when content is absent).
    let Some(mode) = detect_css_mode(source, parsed, opts) else {
        return Err(UnsupportedSvelteRuntimeSurface::StyleCssModeUnsupported {
            span: style.content.unwrap_or(style.tag_open),
        });
    };

    // The css-domain half of the plan build: parse + analyze the css body. A
    // body-parse or scoping-analysis failure threads its PRECISE official
    // css code + span unchanged into the refusal.
    let analyzed = super::css::analyze_style_body(source, content).map_err(|err| {
        UnsupportedSvelteRuntimeSurface::StyleCssAnalysis {
            code: err.code,
            span: err.span,
        }
    })?;

    Ok(PreparedComponentStyle { mode, analyzed })
}

/// Detect the component's css OUTPUT MODE — the official `inject_styles =
/// css === 'injected' || is_custom_element` rule over the parse-domain facts:
///
/// - a custom element (the `<svelte:options customElement>` value or the
///   `customElement: true` compile option, with the official
///   `customElementOptions ?? custom_element_from_option` precedence) ⇒
///   [`Injected`](super::css::types::CssMode::Injected);
/// - a `<svelte:options css>` attribute ⇒ `Injected` when its value is the
///   official-accepted static `"injected"` — a Text value OR a single static
///   STRING-LITERAL expression (`css={'injected'}` / `css={"injected"}`), the
///   upstream `get_static_value` rule (the ONLY value upstream accepts on the
///   options element; every other shape is an official reject caught before
///   this gate — defensively `None` here);
/// - otherwise the `External` default.
///
/// `None` means the mode is UNPROVABLE (a broken upstream invariant) — the
/// caller fails closed.
fn detect_css_mode(
    source: &str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
) -> Option<super::css::types::CssMode> {
    use super::css::types::CssMode;
    use crate::svelte::parser::SvelteAttributeKind;

    // The custom-element half of the official rule (the resolver mirrors the
    // `customElementOptions ?? custom_element_from_option` precedence,
    // including the `customElement={null}` fallback). A resolution failure is
    // a broken invariant — unprovable.
    match super::custom_element::resolve_custom_element(parsed, opts.custom_element) {
        Ok(Some(_)) => return Some(CssMode::Injected),
        Ok(None) => {}
        Err(_) => return None,
    }

    // The `css === 'injected'` half: the first ROOT `<svelte:options>`
    // element's `css` attribute. Upstream accepts ONLY the static `"injected"`
    // (a Text value or a single string-literal expression) on the options
    // element, so a surviving attribute IS the injected mode; re-run the ONE
    // shared static-value check defensively (the same authority the options
    // official-reject classification uses — a Text / string-literal-expression
    // value resolves, a dynamic or mixed value never does) and treat any other
    // surviving shape as unprovable.
    let mut options_elements = Vec::new();
    collect_options_elements(&parsed.template, 0, &mut options_elements);
    let css_attr = options_elements
        .iter()
        .find(|(_, depth)| *depth == 0)
        .and_then(|(element, _)| {
            element.attributes.iter().find_map(|attr| match &attr.kind {
                SvelteAttributeKind::Plain { name, value, .. } if name == "css" => Some(value),
                _ => None,
            })
        });
    match css_attr {
        Some(value) if crate::svelte::parser::tokenizer::options_css_is_injected(source, value) => {
            Some(CssMode::Injected)
        }
        Some(_) => None,
        None => Some(CssMode::External),
    }
}

/// Recursively refuse the FIRST `<p>` element (document order, descending element
/// children + block bodies) in an IMPLICIT-autoclose situation: a DIRECT disallowed
/// block child (`<div>` / `<h1>` / `<p>` …) but NO surviving explicit `</p>` close.
/// Returns the typed `ParagraphAutoclose` feature surface, or `None`. (The official
/// compiler auto-closes such a `<p>` and ACCEPTS; the EXPLICIT-`</p>` case is an
/// official reject handled by the official-reject gate.)
fn refuse_implicit_paragraph_autoclose(
    nodes: &[SvelteNode],
) -> Option<UnsupportedSvelteRuntimeSurface> {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                if el.kind == SvelteElementKind::Intrinsic
                    && el.name.eq_ignore_ascii_case("p")
                    && el.close_span.is_none()
                {
                    if let Some(child) =
                        super::official_reject::paragraph_direct_autoclose_child(el)
                    {
                        return Some(UnsupportedSvelteRuntimeSurface::ParagraphAutoclose {
                            child,
                            span: el.open_span,
                        });
                    }
                }
                if let Some(surface) = refuse_implicit_paragraph_autoclose(&el.children) {
                    return Some(surface);
                }
            }
            SvelteNode::Block(block) => {
                if let Some(surface) = refuse_implicit_paragraph_autoclose(&block.children) {
                    return Some(surface);
                }
                for clause in &block.clauses {
                    if let Some(surface) = refuse_implicit_paragraph_autoclose(&clause.children) {
                        return Some(surface);
                    }
                }
            }
            SvelteNode::Text(_) | SvelteNode::Comment(_) | SvelteNode::Interpolation(_) => {}
            SvelteNode::Tag(_) => {}
        }
    }
    None
}

/// The STRICT `<svelte:options>` gate. Allow ONLY: (i) NO options element (mode
/// inferred from rune usage), or (ii) at most ONE TOP-LEVEL `<svelte:options>`
/// carrying the supported axes — a boolean `runes` literal (the shorthand `runes`
/// is `true`; `runes={false}` forces legacy mode) and a VALID `customElement`
/// value (resolved into the custom-element descriptor at lowering). Fail closed
/// on EVERY other form.
/// Returns the typed surface for the first violation, or `None` when the element
/// is absent or carries only supported axes.
///
/// The strict rules (each a deliberate fail-closed):
/// - a DUPLICATE `<svelte:options>` (two or more anywhere) — official `options_duplicate`;
/// - a NESTED / non-root `<svelte:options>` — official `options_invalid_placement`;
/// - a non-boolean `runes` value (`runes={foo}` / `runes={1}` / `runes="true"`) — only
///   a boolean literal (or the shorthand) is the supported runes plumbing; BOTH
///   boolean values are valid mode selections (`runes={false}` forces legacy mode,
///   whose not-yet-lowered surfaces classify per surface downstream);
/// - `tag` (always an official reject upstream; defensive here) and every OTHER
///   axis (`namespace`, `name`, …, a spread, a directive) — `css` is supported
///   (the injected-mode selector [`detect_css_mode`] consumes);
/// - child content (a `<svelte:options>` is a self-closing marker; content is invalid).
fn refuse_unsupported_options(
    source: &str,
    parsed: &ParsedSvelte,
) -> Option<UnsupportedSvelteRuntimeSurface> {
    // (1) Collect every `<svelte:options>` element with its depth (0 = top-level root,
    // >0 = nested). Walking the WHOLE tree catches a nested options element.
    let mut found: Vec<(&SvelteElement, usize)> = Vec::new();
    collect_options_elements(&parsed.template, 0, &mut found);

    // No options element — the supported absent case (mode inferred from rune usage).
    let &(first, first_depth) = found.first()?;

    // NOTE: a DUPLICATE `<svelte:options>` is an official EXACT-CODE parse error
    // (`svelte_meta_duplicate`) minted by the parser and caught by the official-reject gate
    // that runs BEFORE this gate — it is not refused here.

    // (3) A NESTED / non-root options element — official `svelte_meta_invalid_placement`,
    // now an EXACT-CODE parser fact caught by the official-reject gate BEFORE this gate, so a
    // nested options never reaches here. The depth check stays as a defensive fail-closed.
    if first_depth != 0 {
        return Some(UnsupportedSvelteRuntimeSurface::OptionsAxis {
            span: first.open_span,
        });
    }

    // (4) Child content on the options marker is invalid (it is a self-closing axis
    // carrier, never a container).
    if !first.children.is_empty() {
        return Some(UnsupportedSvelteRuntimeSurface::OptionsAxis {
            span: first.open_span,
        });
    }

    // (5) Classify the single root options element's attributes. By the time this gate runs the
    // options element is official-ACCEPTED (every official-rejected options attribute / child
    // content — `name`/an unknown attribute, a bad `namespace`/`css`, a non-boolean `runes`/`tag`,
    // an invalid `customElement`, a spread/directive, child content — is an EXACT-CODE
    // `OptionsInvalid` parser fact the official-reject gate caught BEFORE this gate; see
    // `official_reject.rs` + the parser `read_options` finalization). So the only inputs reaching
    // here are the officially-ACCEPTED options axes: a boolean `runes` literal (BOTH values —
    // `runes={true}` forces runes mode, `runes={false}` forces legacy mode; the legacy
    // component's unsupported surfaces are classified per surface downstream), a valid
    // `customElement` value, and the static `css="injected"` output-mode axis are SUPPORTED;
    // every OTHER accepted axis (a valid `namespace`,
    // `immutable`/`accessors`/`preserveWhitespace`) is a later options vertical. The
    // non-supported arms below stay as a defensive fail-closed for anything an upstream change
    // might newly accept.
    for attr in &first.attributes {
        let name = options_attr_name(attr);
        match name.as_deref() {
            Some("runes") => match classify_runes_value(source, attr) {
                // The supported runes plumbing: BOTH boolean literals are valid
                // MODE SELECTIONS — `runes={true}` (or shorthand) forces runes
                // mode, `runes={false}` forces legacy mode. Neither is a parse
                // refusal; a legacy component's not-yet-lowered surfaces are
                // classified per surface at runtime surface classification.
                RunesValue::True | RunesValue::False => {}
                // A non-boolean `runes` value (`runes={foo}` / `runes={1}` /
                // `runes="true"`) — only a boolean literal is the supported axis.
                RunesValue::NonBoolean => {
                    return Some(UnsupportedSvelteRuntimeSurface::OptionsAxis {
                        span: first.open_span,
                    });
                }
            },
            // A VALID `customElement` value is SUPPORTED: every invalid form
            // (boolean shorthand, a bad tag, a malformed object, a non-object /
            // non-`null` expression) is an EXACT-CODE `OptionsInvalid` parser fact
            // the official-reject gate caught BEFORE this gate, so the value here
            // is official-ACCEPTED and the lowering resolves it into the
            // [`CustomElementDescriptor`](crate::svelte::parser::CustomElementDescriptor).
            Some("customElement") => {}
            // A surviving `css` axis is SUPPORTED: official accepts ONLY the
            // static `"injected"` value on the options element (anything else —
            // including `"external"` — is the exact-code
            // `svelte_options_invalid_attribute_value` reject minted upstream),
            // and [`detect_css_mode`] consumes it as the injected output mode
            // (re-checking the static value defensively).
            Some("css") => {}
            // The deprecated `tag` axis is ALWAYS an official reject
            // (`svelte_options_deprecated_tag`, minted by the parser and caught by
            // the official-reject gate) — it never reaches this gate; the arm
            // stays as a defensive fail-closed.
            Some("tag") => {
                return Some(UnsupportedSvelteRuntimeSurface::OptionsAxis {
                    span: first.open_span,
                });
            }
            _ => {
                return Some(UnsupportedSvelteRuntimeSurface::OptionsAxis {
                    span: first.open_span,
                });
            }
        }
    }
    None
}

/// The classified VALUE of a `runes` `<svelte:options>` attribute.
enum RunesValue {
    /// `runes` (shorthand) / `runes={true}` — the supported boolean-true axis.
    True,
    /// `runes={false}` — selects legacy mode.
    False,
    /// A non-boolean value (`runes={foo}` / `runes={1}` / `runes="true"` / a mixed
    /// value) — only a boolean literal is supported.
    NonBoolean,
}

/// Classify a `runes` attribute's value into [`RunesValue`]. The shorthand `runes`
/// (no value) is `True`; `runes={true}` / `runes={false}` read the boolean literal
/// from the expression span; a STRING value (`runes="true"`), a non-literal
/// expression (`runes={foo}` / `runes={1}`), or a mixed value is `NonBoolean`. Driven
/// from the typed attribute value + the expression source slice, never a guess.
fn classify_runes_value(source: &str, attr: &SvelteAttribute) -> RunesValue {
    use crate::svelte::parser::{SvelteAttributeKind, SvelteAttributeValue};
    let SvelteAttributeKind::Plain { value, .. } = &attr.kind else {
        // A spread / directive never reaches here (the caller only matches a plain
        // `runes` name); defensive non-boolean.
        return RunesValue::NonBoolean;
    };
    match value {
        // Shorthand boolean `runes` ⇒ true.
        None => RunesValue::True,
        // `runes={true}` / `runes={false}` — the ONLY supported expression forms are
        // the bare boolean literals.
        Some(SvelteAttributeValue::Expression(span)) => {
            let text = source[span.start as usize..span.end as usize].trim();
            match text {
                "true" => RunesValue::True,
                "false" => RunesValue::False,
                _ => RunesValue::NonBoolean,
            }
        }
        // A quoted string (`runes="true"`) or a mixed value is non-boolean (official
        // requires a `{...}` boolean expression).
        Some(SvelteAttributeValue::Text(_)) | Some(SvelteAttributeValue::Mixed(_)) => {
            RunesValue::NonBoolean
        }
    }
}

/// Recursively collect every `<svelte:options>` element under `nodes`, recording its
/// DEPTH (0 = a top-level root node, >0 = nested inside an element / block). A nested
/// options element is an invalid placement; a second one anywhere is a duplicate.
fn collect_options_elements<'a>(
    nodes: &'a [SvelteNode],
    depth: usize,
    out: &mut Vec<(&'a SvelteElement, usize)>,
) {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                if matches!(
                    el.kind,
                    SvelteElementKind::Special(SvelteSpecialKind::Options)
                ) {
                    out.push((el, depth));
                }
                collect_options_elements(&el.children, depth + 1, out);
            }
            SvelteNode::Block(block) => {
                collect_options_elements(&block.children, depth + 1, out);
                for clause in &block.clauses {
                    collect_options_elements(&clause.children, depth + 1, out);
                }
            }
            _ => {}
        }
    }
}

/// The attribute NAME of a `<svelte:options>` attribute (a plain attribute name),
/// or `None` for a non-plain form (a spread / directive — neither is a supported
/// name/runes axis, so the caller treats it as an unrecognised options axis).
fn options_attr_name(attr: &SvelteAttribute) -> Option<String> {
    use crate::svelte::parser::SvelteAttributeKind;
    match &attr.kind {
        SvelteAttributeKind::Plain { name, .. } => Some(name.clone()),
        // A spread / directive / `{@attach}` on `<svelte:options>` is never the
        // supported `runes` axis — `None` routes it to the unsupported-options arm.
        SvelteAttributeKind::Spread(_)
        | SvelteAttributeKind::Directive(_)
        | SvelteAttributeKind::Attach { .. } => None,
    }
}
