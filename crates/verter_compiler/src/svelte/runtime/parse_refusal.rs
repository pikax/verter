//! Fail-closed refusal of the PARSE-DOMAIN unsupported Svelte runtime surfaces
//! (the ones not carried on the runtime IR): a DUPLICATE attribute / directive
//! (the official `attribute_duplicate` parse error), a top-level `<style>`
//! whose css output mode is unprovable or whose body fails the scoping
//! analysis (an ACCEPTED style hands its parsed + analyzed body forward as
//! the pre-lowering style stage), and a dev-mode codegen request. The
//! `<svelte:options>` element itself carries NO parse-domain refusal here:
//! its officially-accepted axes — `runes`, `customElement`, static
//! `css="injected"`, `namespace`, `preserveWhitespace`, `preserveComments`,
//! and `discloseVersion` — are folded by the shared compile-options resolver,
//! while every malformed form (a duplicate / nested placement, a spread /
//! directive, a non-boolean `runes`, an invalid `namespace` / `css`, an
//! unknown attribute such as `name`, the deprecated `tag`) is an exact-code
//! official reject caught before this gate. A VALID `customElement` value is
//! supported (the lowering resolves it into the custom-element descriptor).

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
/// that fails the scoping analysis or an unprovable css output mode, and a
/// dev-mode codegen request) BEFORE lowering. The `<svelte:options>` axes fold in
/// the shared compile-options resolver (accepted) or reject upstream (malformed) —
/// this gate carries no options-axis refusal. Returns the FIRST refusal found as
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
    // Script dialect is handled by the canonical parser-wide grammar retained on
    // `ScriptAnalysis`. An exact `lang="ts"` component reaches the
    // source-preserving TS-erasure path; every other lang value remains JS grammar
    // and a TS-only body is rejected upstream as `js_parse_error`.
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
    // The `<svelte:options>` element itself needs no refusal here: every MALFORMED
    // options form (a duplicate / nested / non-root placement, child content, a
    // spread / directive, a non-boolean `runes`, an invalid `namespace` / `css`, an
    // unknown attribute, the deprecated `tag`) is an EXACT-CODE parser fact the
    // official-reject gate rejects BEFORE this gate. The officially-ACCEPTED axes
    // (`runes` / `customElement` / static `css="injected"` / `namespace` /
    // `preserveWhitespace` / `preserveComments` / `discloseVersion` / `immutable` /
    // `accessors`) are folded by the shared compile-options resolver
    // ([`resolve_svelte_compile_options`]), which supports `namespace` /
    // `preserveWhitespace` / `preserveComments` / `discloseVersion` and fails closed
    // on `immutable` / `accessors` — so the options element carries no residual
    // parse-domain refusal.
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

/// Recursively collect every `<svelte:options>` element under `nodes`, recording its
/// DEPTH (0 = a top-level root node, >0 = nested inside an element / block). A nested
/// options element is an invalid placement; a second one anywhere is a duplicate.
pub(super) fn collect_options_elements<'a>(
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
pub(super) fn options_attr_name(attr: &SvelteAttribute) -> Option<String> {
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
