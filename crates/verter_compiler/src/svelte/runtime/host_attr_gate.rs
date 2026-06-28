//! The official HOST-ATTRIBUTE bind gate — the SINGLE authority that decides
//! whether a host element's typed attributes satisfy the official requirement for a
//! `bind:` directive.
//!
//! Several Svelte binds are valid ONLY when the host element carries a specific
//! STATIC attribute; `svelte@5.56.3` raises a COMPILE ERROR otherwise. The runtime
//! bind router only sees `(name, tag)`, so without this gate an invalid bind would
//! emit a divergent / runtime-broken module. This gate is consumed by BOTH the
//! emitter's bind classifier ([`super::client_shapes::classify_dom_value_bind`]) AND
//! the structural topology recorder ([`super::topology`]) so the oracle and the
//! emitter never disagree about which host shapes are refused.
//!
//! The decision is driven entirely from the typed [`AttrIr`] inventory (the
//! Typed-IR-Only rule, never a source-text scan); the static-text view is
//! ENTITY-DECODED via the shared attribute decoder
//! ([`decode_attr_entities`](super::entity_decode::decode_attr_entities)).

use super::ir::AttrIr;
use crate::svelte::bind_contract::{RuntimeBindRouting, RuntimeHelper};
use crate::svelte::parser::{SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue};

/// The typed presence/value of ONE host-element attribute, read from the host's
/// `ElementIr` attribute inventory — the input to the official host-attribute
/// bind gates ([`host_attr_gate_passes`]). Driven entirely from the typed
/// [`AttrIr`] (never a source-text scan).
///
/// The static-text view is ENTITY-DECODED: the official host-attribute gates
/// compare against the attribute's decoded `Text.data` (`decode_character_references`),
/// so a `type="check&#98;ox"` decodes to `"checkbox"` and `bind:checked` passes —
/// matching svelte@5.56.3. The decode reuses the shared
/// [`decode_attr_entities`](super::entity_decode::decode_attr_entities) attribute
/// decoder (the same path the mixed-attribute / `style:` literal lowering uses), so
/// there is ONE decode authority. (Decoding owns the string, so this carrier is not
/// `Copy`.)
#[derive(Debug, Clone, PartialEq, Eq)]
enum HostAttr {
    /// The attribute is not present on the host.
    Absent,
    /// A STATIC attribute (a compile-time-constant value). `value` is the
    /// ENTITY-DECODED literal (`Some("checkbox")`) or `None` for a valueless boolean
    /// attribute (`contenteditable`).
    Static(Option<String>),
    /// A DYNAMIC / MIXED attribute (`type={t}` / `multiple="a{b}"`) — its value is
    /// a reactive expression, NOT a compile-time constant. The official gates that
    /// require a STATIC attribute reject this.
    Dynamic,
}

/// Read the typed presence/value of the host attribute `name` from the host
/// element's `AttrIr` inventory. A `Static`/`Dynamic`/`Mixed` attribute maps to
/// the corresponding [`HostAttr`]; a `bind:` / event / directive of the same name
/// is NOT a plain attribute and does not count. A STATIC value is ENTITY-DECODED
/// (the official `Text.data` view the gates compare). Decided over the typed IR,
/// never a source scan.
fn host_attr(attrs: &[AttrIr], name: &str) -> HostAttr {
    for attr in attrs {
        match attr {
            AttrIr::Static { name: n, value } if n == name => {
                return HostAttr::Static(
                    value
                        .as_ref()
                        .map(|v| super::entity_decode::decode_attr_entities(&v.value)),
                );
            }
            AttrIr::Dynamic { name: n, .. } | AttrIr::Mixed { name: n, .. } if n == name => {
                return HostAttr::Dynamic;
            }
            _ => {}
        }
    }
    HostAttr::Absent
}

/// Read the typed presence/value of the host attribute `name` from a PARSED element's
/// [`SvelteAttribute`] inventory — the same `Absent` / `Static` / `Dynamic` classification
/// [`host_attr`] derives from the lowered [`AttrIr`], read instead from the pre-IR parse (the
/// official-reject gate runs before IR lowering). A plain attribute's value maps as: a
/// valueless boolean (`<select multiple>`) → `Static(None)`; a quoted/unquoted TEXT value →
/// `Static(Some(decoded))` (ENTITY-DECODED via the SAME [`decode_attr_entities`]
/// (super::entity_decode::decode_attr_entities) decoder the lowered view uses); an `{expr}`
/// expression or a `"a{b}"` mixed value → `Dynamic`. A `bind:` / event / directive of the same
/// name is NOT a plain attribute and does not count. Structural over the typed parse, never a
/// raw-source heuristic.
fn parsed_host_attr(source: &str, attrs: &[SvelteAttribute], name: &str) -> HostAttr {
    for attr in attrs {
        if let SvelteAttributeKind::Plain { name: n, value, .. } = &attr.kind {
            if n == name {
                return match value {
                    // A valueless boolean attribute (`<select multiple>`).
                    None => HostAttr::Static(None),
                    Some(SvelteAttributeValue::Text(span)) => {
                        HostAttr::Static(Some(super::entity_decode::decode_attr_entities(
                            &source[span.start as usize..span.end as usize],
                        )))
                    }
                    Some(SvelteAttributeValue::Expression(_))
                    | Some(SvelteAttributeValue::Mixed(_)) => HostAttr::Dynamic,
                };
            }
        }
    }
    HostAttr::Absent
}

/// Whether the host element's typed attributes satisfy the official HOST-ATTRIBUTE
/// requirement for this `(name, tag, routing)` bind. Returns `false` (⇒ the caller
/// fails the bind closed) when the host is missing / has the wrong shape of a
/// required attribute — exactly the cases svelte@5.56.3 raises a COMPILE ERROR for.
///
/// The gates (verified empirically against svelte@5.56.3):
/// - The `<input type>` requirement — "'type' attribute must be a static text value
///   if input uses two-way binding". For an `<input>` bind, a `type` attribute, when
///   PRESENT, must be a STATIC TEXT VALUE (`Static(Some)`): a valueless `type`
///   (`Static(None)`) is rejected for EVERY input bind, and a DYNAMIC `type={t}` is
///   rejected for every input bind EXCEPT `bind:value` (where a dynamic type is
///   ALLOWED — official still emits `$.bind_value`). An ABSENT `type` is allowed
///   (the `checked` gate below separately requires `type="checkbox"`). This is the
///   first input-bind gate, BEFORE the helper-specific checks.
/// - `bind:checked` ADDITIONALLY REQUIRES the (entity-decoded) static `type` to be
///   exactly `"checkbox"` — "`bind:checked` can only be used with
///   `<input type="checkbox">`". A missing / dynamic / other-value `type` fails.
/// - the contenteditable binds (`innerHTML` / `innerText` / `textContent`, the
///   [`RuntimeHelper::ContentEditable`] routing) REQUIRE a STATIC `contenteditable`
///   attribute present on the host — "'contenteditable' attribute is required for
///   textContent, innerHTML and innerText two-way bindings" — AND it may NOT be
///   dynamic — "'contenteditable' attribute cannot be dynamic if element uses two-way
///   binding". A missing OR dynamic `contenteditable` fails (a static valueless
///   `contenteditable` or a static `contenteditable="true"` passes).
/// - `bind:value` on a `<select>` with a `multiple` attribute REQUIRES it to be
///   STATIC — "'multiple' attribute must be static if select uses two-way binding".
///   A dynamic `multiple={m}` fails; a static or absent `multiple` passes.
///
/// Every other bind has no host-attribute requirement and passes unconditionally.
/// Driven entirely from the typed [`AttrIr`] inventory (the Typed-IR-Only rule); the
/// static-text view is ENTITY-DECODED via [`host_attr`].
///
/// This is the SINGLE host-attribute gate authority: the emitter's bind classifier
/// ([`super::client_shapes::classify_dom_value_bind`]), the structural topology recorder
/// ([`super::topology`]), AND the official-reject gate's bind-validation pass
/// ([`super::official_reject`]) route an accepted-bind decision through ONE decision core
/// ([`host_attr_gate_decision`]) — each supplying its own attribute view (the lowered
/// [`AttrIr`] inventory via [`host_attr_gate_passes`], or the pre-IR parsed
/// [`SvelteAttribute`] inventory via [`host_attr_gate_passes_parsed`]) — so the phases never
/// disagree about which host shapes are refused.
pub(super) fn host_attr_gate_passes(
    name: &str,
    tag: &str,
    routing: &RuntimeBindRouting,
    host_attrs: &[AttrIr],
) -> bool {
    host_attr_gate_decision(name, tag, routing, |q| host_attr(host_attrs, q))
}

/// The official host-attribute gate over a PARSED element's [`SvelteAttribute`] inventory —
/// the pre-IR entry for the official-reject gate's bind-validation pass (it runs before IR
/// lowering, so it has no `AttrIr`). Routes through the SAME [`host_attr_gate_decision`] core
/// as the lowered-IR [`host_attr_gate_passes`], reading each attribute via [`parsed_host_attr`].
pub(super) fn host_attr_gate_passes_parsed(
    name: &str,
    tag: &str,
    routing: &RuntimeBindRouting,
    source: &str,
    attrs: &[SvelteAttribute],
) -> bool {
    host_attr_gate_decision(name, tag, routing, |q| parsed_host_attr(source, attrs, q))
}

/// The shared host-attribute gate DECISION (the official compile-error checks), reading each
/// host attribute through `lookup` so BOTH the lowered-IR caller and the parsed-AST caller
/// route ONE authority. `lookup(name)` returns the attribute's [`HostAttr`] presence/value.
fn host_attr_gate_decision(
    name: &str,
    tag: &str,
    routing: &RuntimeBindRouting,
    lookup: impl Fn(&str) -> HostAttr,
) -> bool {
    // The official `<input type>` requirement applies to EVERY `<input>` bind: a
    // `type` attribute, when PRESENT, must be a STATIC TEXT VALUE. A valueless `type`
    // (`Static(None)`) is invalid for every input bind; a DYNAMIC `type={t}` is
    // invalid for every input bind EXCEPT `bind:value`. An ABSENT `type` is allowed.
    // This runs BEFORE the helper-specific checks below.
    if tag == "input" {
        match lookup("type") {
            // A valueless `type` (`<input type bind:…>`) is invalid for EVERY input
            // bind, including `bind:value`.
            HostAttr::Static(None) => return false,
            // A dynamic `type={t}` is invalid for every input bind EXCEPT `bind:value`
            // (where official tolerates a dynamic type and still emits `$.bind_value`).
            HostAttr::Dynamic if name != "value" => return false,
            // A static-text `type` (`Static(Some)`), an absent `type`, or a dynamic
            // `type` on `bind:value` is allowed here (the `checked` gate below adds the
            // `type="checkbox"` value requirement).
            _ => {}
        }
    }
    // `bind:checked` ⇒ the (entity-decoded) static `type` must be exactly `"checkbox"`
    // (the `Checked` routing is only ever produced for an `<input>` host). The decoded
    // view is the official `Text.data` comparison (`type="check&#98;ox"` ⇒ matches).
    if routing.helper == RuntimeHelper::Checked {
        return matches!(lookup("type"), HostAttr::Static(Some(v)) if v == "checkbox");
    }
    // The contenteditable binds ⇒ a STATIC (non-dynamic) `contenteditable` attribute
    // must be present. A static value (`contenteditable` valueless OR
    // `contenteditable="true"`) passes; a missing or dynamic one fails.
    if routing.helper == RuntimeHelper::ContentEditable {
        return matches!(lookup("contenteditable"), HostAttr::Static(_));
    }
    // `bind:value` on a `<select multiple>` ⇒ the `multiple` attribute (if present)
    // must be STATIC. A dynamic `multiple` fails; static or absent passes. (The
    // `SelectValue` routing is only ever produced for a `<select>` host.)
    if routing.helper == RuntimeHelper::SelectValue && name == "value" {
        return !matches!(lookup("multiple"), HostAttr::Dynamic);
    }
    true
}
