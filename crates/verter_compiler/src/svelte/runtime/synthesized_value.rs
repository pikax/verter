//! The SEALED synthesized template-value carrier — a DEDICATED module so the
//! fields are truly module-private: no struct-literal or free-fn `String`
//! construction is possible from any other module (rustc enforces the
//! boundary; the routing guard's construction-site inventory re-verifies it
//! structurally). The ONLY construction API is the three typed constructors
//! below, each consuming PREPARED / typed contributors and deriving the
//! rendered composite text ITSELF — authored raw text or a hand-built legacy
//! sequence has no entry through the CURRENT constructor vocabulary; a new
//! in-module constructor or struct literal fails the routing guard's pinned
//! construction-site inventory. This module holds NOTHING else:
//! any helper landing here would share the fields' scope.

use super::client_legacy_value::PreparedTemplateValue;
use super::client_plan_types::{StyleDirectiveObjectEntry, StyleDirectiveObjectValue};

/// A SYNTHESIZED template value — rendered text plus its memoize trigger. It
/// carries NO prepared wrap by construction (the memoizer receives it
/// untouched); any authored sub-expression was prepared before synthesis.
/// SEALED: the fields are private to THIS dedicated module and the accessor
/// vocabulary is RAW-only — no method yields a wrapped rendering and the type
/// holds no wrap-typed state, so the carrier AS DEFINED cannot yield a
/// wrapped rendering; wrap bytes stay out of `text` because the typed
/// constructors derive it themselves (guard-pinned construction sites).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SynthesizedTemplateValue {
    /// The rendered composite text (`$.clsx(<prepared inner>)`, a directive
    /// object, …).
    text: String,
    /// Whether any authored contributor `has_call` (the memoize trigger for the
    /// synthesized whole).
    has_call: bool,
}

impl SynthesizedTemplateValue {
    /// The `$.clsx(<prepared base>)` class-base composite. Official applies
    /// `build_expression` BEFORE `$.clsx`, so any legacy wrap already lives
    /// INSIDE the prepared inline expression; the composite text is derived
    /// HERE from the typed contributor — no caller supplies free-form text.
    pub(super) fn clsx(base: &PreparedTemplateValue) -> Self {
        Self {
            text: format!("$.clsx({})", base.inline_expression()),
            has_call: base.has_call(),
        }
    }

    /// The merged `[$.CLASS]` class-directives object (`{ on, 'a-b': cond }`)
    /// from `(raw directive name, prepared condition)` entries — the key is
    /// quoted HERE, a same-named condition folds to the JS shorthand, and a
    /// condition-less entry folds to the bare key. `None` when no directive
    /// is present (no `[$.CLASS]` entry is synthesized).
    pub(super) fn class_directives(
        entries: &[(String, Option<PreparedTemplateValue>)],
    ) -> Option<Self> {
        if entries.is_empty() {
            return None;
        }
        let has_call = entries
            .iter()
            .any(|(_, cond)| cond.as_ref().is_some_and(PreparedTemplateValue::has_call));
        let body = entries
            .iter()
            .map(|(name, cond)| {
                let key = super::client_codegen_helpers::object_key(name);
                match cond {
                    Some(prepared) => super::client_codegen_helpers::object_property(
                        &key,
                        &prepared.inline_expression(),
                    ),
                    None => key,
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Some(Self {
            text: format!("{{ {body} }}"),
            has_call,
        })
    }

    /// The merged `[$.STYLE]` style-directives value — the `{ … }` object or
    /// the `[{normal}, {important}]` array form — from TYPED entries: each
    /// authored contribution enters as a prepared carrier (or a typed
    /// static/mixed value), the key quoting / value folding / array split all
    /// happen HERE. `None` when no directive is present.
    pub(super) fn style_directives(entries: &[StyleDirectiveObjectEntry]) -> Option<Self> {
        let has_call = entries.iter().any(|entry| match &entry.value {
            StyleDirectiveObjectValue::Prepared(p) => p.has_call(),
            StyleDirectiveObjectValue::StaticText(_) => false,
            StyleDirectiveObjectValue::Mixed(v) => v.has_call(),
        });
        let rendered: Vec<(String, bool)> = entries
            .iter()
            .map(|entry| {
                let key = super::client_codegen_helpers::object_key(&entry.property);
                let value_text = match &entry.value {
                    StyleDirectiveObjectValue::Prepared(p) => p.inline_expression(),
                    StyleDirectiveObjectValue::StaticText(text) => {
                        super::client_codegen_helpers::js_single_quoted(text)
                    }
                    StyleDirectiveObjectValue::Mixed(v) => v.folded_text(),
                };
                (
                    super::client_codegen_helpers::object_property(&key, &value_text),
                    entry.important,
                )
            })
            .collect();
        super::client_codegen_helpers::fold_style_directives_value(&rendered)
            .map(|text| Self { text, has_call })
    }

    /// The official `has_call` memoize trigger of the synthesized whole.
    pub(super) fn has_call(&self) -> bool {
        self.has_call
    }

    /// The rendered composite text — the ONLY serialization this carrier
    /// offers: always RAW. No wrapped form exists on this API, so an emitter
    /// cannot select legacy wrapping for a synthesized value.
    pub(super) fn raw_text(&self) -> &str {
        &self.text
    }
}
