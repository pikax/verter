//! Custom-element DESCRIPTOR resolution — the official `read_options`
//! `customElement` value ([`CustomElementDescriptor`]) read from the
//! parser-RETAINED `<svelte:options>` value (or synthesized from the
//! `customElement: true` compile option) at lowering time.
//!
//! The parser RETAINS the accepted value: a Text tag rides the
//! [`OptionsCustomElementTextTag`] descriptor the parser's `read_options`
//! finalization resolved when `validate_custom_element_tag` accepted it, and
//! an expression value rides the [`OptionsCustomElementProbe::resolution`]
//! the parser resolved ONCE at options finalization through
//! [`resolve_custom_element_expr`] — the ONE shared validate+extract engine
//! whose reject side the official-reject gate arbitrates. This module
//! performs NO expression parse and NO source slicing (it does not take the
//! source at all): both value forms consume the SAME retained typed result
//! the parser validated, so the lowered descriptor is BY CONSTRUCTION the
//! value the gate accepted; a validator/extractor divergence is structurally
//! impossible.
//!
//! Resolution runs AFTER the official-reject gate, so every input reaching it
//! is official-ACCEPTED: a valid Text tag, a `null` literal, or a conforming
//! object (`{ tag?, shadow?, props?, extend? }`). An arm the gate should have
//! rejected fails LOUDLY as a lowering diagnostic — never a silent
//! plain-component downgrade (which would emit a divergent module with no
//! `create_custom_element`).
//!
//! Precedence mirrors the official `options.customElementOptions ??
//! custom_element_from_option`: the `<svelte:options customElement>` value wins
//! over the compile option, and a `customElement={null}` value (the Svelte-3
//! backwards-compat spelling) sets NOTHING — it falls back to the compile
//! option.
//!
//! [`OptionsCustomElementProbe::resolution`]: crate::svelte::parser::OptionsCustomElementProbe::resolution
//! [`OptionsCustomElementTextTag`]: crate::svelte::parser::OptionsCustomElementTextTag
//! [`resolve_custom_element_expr`]: crate::svelte::parser::resolve_custom_element_expr

use super::RuntimeLoweringDiagnostic;
use crate::svelte::parser::{
    AcceptedCustomElementValue, CustomElementDescriptor, CustomElementShadow, ParsedSvelte,
    SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue, SvelteElement, SvelteElementKind,
    SvelteNode, SvelteSpecialKind,
};
use verter_span::Span;

/// Resolve the component's custom-element descriptor from the FIRST root
/// `<svelte:options>` element's `customElement` attribute, falling back to the
/// `customElement: true` compile option (`custom_element_option`). `None` means
/// the component compiles as a plain component.
///
/// FALLIBLE only on a broken upstream invariant: a `customElement` value shape
/// the official-reject gate should have rejected (a boolean shorthand, a mixed
/// value, a retained reject, a probe-less expression, a retention-less text
/// tag) returns a lowering diagnostic instead of silently downgrading to a
/// plain component.
pub(super) fn resolve_custom_element(
    parsed: &ParsedSvelte,
    custom_element_option: bool,
) -> Result<Option<CustomElementDescriptor>, RuntimeLoweringDiagnostic> {
    let compile_option_descriptor = || {
        custom_element_option.then(|| CustomElementDescriptor {
            tag: None,
            shadow: CustomElementShadow::Open,
            props: Vec::new(),
            extend: None,
            inject_styles: true,
        })
    };
    // The FIRST ROOT-level options element (upstream's `findIndex` over the root
    // fragment); a nested one is an official reject caught upstream.
    let Some(attr) = first_root_options_custom_element_attr(&parsed.template) else {
        return Ok(compile_option_descriptor());
    };
    let SvelteAttributeKind::Plain { value, .. } = &attr.kind else {
        // A spread / directive `customElement` never reaches here (the caller
        // matched a plain attribute name).
        return Err(invariant_diagnostic(attr.span));
    };
    match value {
        // `customElement="my-el"` — consume the RETAINED descriptor the parser
        // resolved at options finalization when the text tag validated (keyed
        // back by the attribute's text-value span). The gate already accepted
        // this component, so a missing retained entry here is a broken
        // upstream invariant, never a silent raw-source re-slice.
        Some(SvelteAttributeValue::Text(span)) => {
            let retained = parsed
                .options_custom_element_text_tags
                .iter()
                .find(|text_tag| text_tag.text_span == *span);
            match retained {
                Some(text_tag) => Ok(Some(text_tag.descriptor.clone())),
                None => Err(invariant_diagnostic(attr.span)),
            }
        }
        // `customElement={EXPR}` — consume the RETAINED typed resolution the
        // parser produced at options finalization (keyed back by the expression
        // span the probe recorded). The gate already accepted this component,
        // so a missing probe or a retained reject here is a broken upstream
        // invariant, never a silent downgrade.
        Some(SvelteAttributeValue::Expression(span)) => {
            let probe = parsed
                .options_custom_element_probes
                .iter()
                .find(|probe| probe.expr_span == *span);
            match probe.map(|probe| &probe.resolution) {
                // The `null` backwards-compat skip sets NOTHING — the compile
                // option decides.
                Some(Ok(AcceptedCustomElementValue::NullSkip)) => Ok(compile_option_descriptor()),
                Some(Ok(AcceptedCustomElementValue::Descriptor(descriptor))) => {
                    Ok(Some(descriptor.clone()))
                }
                Some(Err(_)) | None => Err(invariant_diagnostic(attr.span)),
            }
        }
        // The boolean shorthand and the mixed value are official rejects
        // (`svelte_options_invalid_customelement` / `svelte_options_invalid_tagname`)
        // caught by the official-reject gate BEFORE lowering.
        None | Some(SvelteAttributeValue::Mixed(_)) => Err(invariant_diagnostic(attr.span)),
    }
}

/// The FIRST root-level `<svelte:options>` element's plain `customElement`
/// attribute, or `None`.
fn first_root_options_custom_element_attr(nodes: &[SvelteNode]) -> Option<&SvelteAttribute> {
    let options = nodes.iter().find_map(|node| match node {
        SvelteNode::Element(el) if is_options_element(el) => Some(el),
        _ => None,
    })?;
    options.attributes.iter().find(|attr| {
        matches!(&attr.kind, SvelteAttributeKind::Plain { name, .. } if name == "customElement")
    })
}

/// Whether an element is the `<svelte:options>` special.
fn is_options_element(el: &SvelteElement) -> bool {
    matches!(
        el.kind,
        SvelteElementKind::Special(SvelteSpecialKind::Options)
    )
}

/// The broken-upstream-invariant diagnostic: a `customElement` value shape the
/// official-reject gate should have rejected reached descriptor resolution.
fn invariant_diagnostic(span: Span) -> RuntimeLoweringDiagnostic {
    RuntimeLoweringDiagnostic {
        code: "svelte-runtime-custom-element-descriptor",
        message: "the `<svelte:options customElement>` value reached descriptor resolution in a \
                  shape the official-reject gate accepts only after validation — a gate/resolver \
                  divergence"
            .to_string(),
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svelte::parser::parse_svelte;

    const STRING_TAG_SRC: &str = "<svelte:options customElement=\"my-el\" />\n<p>hi</p>";

    #[test]
    fn string_tag_lowering_consumes_only_the_retained_descriptor() {
        // Positive: resolution returns the parser-RETAINED string-tag descriptor.
        let parsed = parse_svelte(STRING_TAG_SRC);
        let descriptor = resolve_custom_element(&parsed, false)
            .expect("a retained valid string tag resolves")
            .expect("a string-tag customElement yields a descriptor");
        assert_eq!(descriptor.tag.as_deref(), Some("my-el"));
        assert_eq!(descriptor.shadow, CustomElementShadow::Open);
        assert!(descriptor.props.is_empty());
        assert_eq!(descriptor.extend, None);
        assert!(descriptor.inject_styles);
        // Discriminating negative: with the retained entry REMOVED, resolution must FAIL
        // LOUDLY (the descriptor-invariant diagnostic) — proving the lowering consumes the
        // parser RETENTION, not a raw-source re-slice of the attribute span (which
        // `resolve_custom_element` can no longer perform: it does not take the source).
        let mut tampered = parse_svelte(STRING_TAG_SRC);
        tampered.options_custom_element_text_tags.clear();
        let err = resolve_custom_element(&tampered, false)
            .expect_err("a missing retained text-tag descriptor is a loud invariant failure");
        assert_eq!(err.code, "svelte-runtime-custom-element-descriptor");
    }
}
