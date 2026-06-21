//! Fail-closed refusal of the PARSE-DOMAIN unsupported Svelte runtime surfaces
//! (the ones not carried on the runtime IR): a DUPLICATE attribute / directive
//! (5a, the official `attribute_duplicate` parse error), a top-level `<style>`
//! (5l), a `<svelte:options>` axis beyond `runes` (5m — including `name`, which
//! official rejects as an unknown attribute; 5h for `customElement`), and a
//! dev-mode codegen request (5k).

use verter_span::Span;

use super::SvelteRuntimeOptions;
use super::UnsupportedSvelteRuntimeSurface;
use crate::svelte::parser::{
    ParsedSvelte, SvelteAttribute, SvelteElement, SvelteElementKind, SvelteNode, SvelteSpecialKind,
};

/// The PARSE-DOMAIN gate (choke point 1 of the refuse-by-default pipeline): refuse
/// the unsupported surfaces the runtime IR does not carry (a top-level `<style>`
/// (5l), a `<svelte:options>` axis beyond `runes` (5m, or 5h for `customElement`),
/// and a dev-mode codegen request (5k)) BEFORE lowering. Returns the FIRST one
/// found, or `None`.
pub(super) fn parse_domain_gate(
    source: &str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
) -> Option<UnsupportedSvelteRuntimeSurface> {
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
        return Some(surface);
    }
    // A dev-mode codegen request: the dev-mode output axis is not emitted (5k).
    if opts.dev_codegen {
        return Some(UnsupportedSvelteRuntimeSurface::DevMode {
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
                return Some(UnsupportedSvelteRuntimeSurface::TypeScript {
                    span: script.content.unwrap_or(script.tag_open),
                });
            }
        }
    }
    // A top-level `<style>` block — CSS scoping is the 5l vertical. `<style>` is
    // parsed into `parsed.styles` (never a template node), so the IR walk does not
    // see it; refuse it here.
    if let Some(style) = parsed.styles.first() {
        return Some(UnsupportedSvelteRuntimeSurface::Style {
            span: style.content.unwrap_or(style.tag_open),
        });
    }
    // The STRICT `<svelte:options>` gate: allow ONLY an ABSENT options element, or at
    // most ONE top-level `<svelte:options runes={true} />` (shorthand `runes` ok). Fail
    // closed on a duplicate / nested / non-root options element, a non-boolean / `false`
    // `runes` value, any other axis (`customElement` → 5h, `namespace`/`css`/… → 5m),
    // a spread / directive, or child content.
    if let Some(surface) = refuse_unsupported_options(source, parsed) {
        return Some(surface);
    }
    None
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
/// inferred from rune usage), or (ii) at most ONE TOP-LEVEL
/// `<svelte:options runes={true} />` (the shorthand `runes` boolean is `true` too).
/// Fail closed on EVERY other form. Returns the typed surface for the first
/// violation, or `None` when the element is absent or is exactly the supported form.
///
/// The strict rules (each a deliberate fail-closed):
/// - a DUPLICATE `<svelte:options>` (two or more anywhere) — official `options_duplicate`;
/// - a NESTED / non-root `<svelte:options>` — official `options_invalid_placement`;
/// - a non-boolean `runes` value (`runes={foo}` / `runes={1}` / `runes="true"`) — only
///   the boolean literal `true` (or shorthand) is the supported runes plumbing;
/// - `runes={false}` — selects LEGACY mode, the legacy-mode vertical (5i) owner;
/// - `customElement` / `tag` (5h) and every OTHER axis (`namespace`, `css`, `name`, …,
///   a spread, a directive) (5m);
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
    // here are the officially-ACCEPTED options axes Verter does not yet support as a FEATURE: the
    // ONLY supported axis is `runes={true}` (or shorthand); `runes={false}` is the legacy-mode
    // owner (5i); a valid `customElement` is the host/custom-element vertical (5h); every OTHER
    // accepted axis (a valid `namespace`/`css`, `immutable`/`accessors`/`preserveWhitespace`) is
    // the 5m options vertical. The non-supported arms below stay as a defensive fail-closed for
    // anything an upstream change might newly accept.
    for attr in &first.attributes {
        let name = options_attr_name(attr);
        match name.as_deref() {
            Some("runes") => match classify_runes_value(source, attr) {
                // The supported runes plumbing: a boolean `true` literal / shorthand.
                RunesValue::True => {}
                // `runes={false}` selects legacy mode — the legacy-mode vertical (5i).
                RunesValue::False => {
                    return Some(UnsupportedSvelteRuntimeSurface::LegacyMode {
                        span: first.open_span,
                    });
                }
                // A non-boolean `runes` value (`runes={foo}` / `runes={1}` /
                // `runes="true"`) — only a boolean literal is the supported axis.
                RunesValue::NonBoolean => {
                    return Some(UnsupportedSvelteRuntimeSurface::OptionsAxis {
                        span: first.open_span,
                    });
                }
            },
            Some("customElement") | Some("tag") => {
                return Some(UnsupportedSvelteRuntimeSurface::HostOrCustomElement {
                    surface: "<svelte:options customElement>",
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
/// name/runes axis, so the caller treats it as an unrecognised 5m axis).
fn options_attr_name(attr: &SvelteAttribute) -> Option<String> {
    use crate::svelte::parser::SvelteAttributeKind;
    match &attr.kind {
        SvelteAttributeKind::Plain { name, .. } => Some(name.clone()),
        // A spread / directive on `<svelte:options>` is never the supported
        // `runes` axis — `None` routes it to the 5m unsupported-options arm.
        SvelteAttributeKind::Spread(_) | SvelteAttributeKind::Directive(_) => None,
    }
}
