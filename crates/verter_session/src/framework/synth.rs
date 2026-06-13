#![deny(missing_docs)]
//! The framework-neutral component-default synthesis seam.
//!
//! Some frameworks have no literal `export default` in their carrier script —
//! the default export is synthesized from the framework's macro calls (a Vue
//! `<script setup>` SFC's `default` instance is produced from `defineProps` /
//! `defineEmits` / `defineSlots`). [`ComponentDefaultSynth`] is the per-adapter
//! seam that fabricates that synthesized `default` value symbol; the host
//! dispatches to the adapter selected by the canonical's resolved
//! [`FileLanguage`] during shallow-state construction.
//!
//! Every input is PARSE-DOMAIN only ([`ComponentDefaultSynthCtx`] carries the
//! canonical, the resolved language, the parse-domain macro list, and the
//! syntax-capture candidate set — no resolver, no capability snapshot, no
//! `StoreView`). The synthesis runs once per content hash inside shallow-state
//! construction, so a resolved-domain input would violate the syntax-only
//! boundary (guard `component_default_synth_parse_domain_only`).

use verter_language::FileLanguage;
use verter_semantic::analysis::framework_facts::FrameworkScriptCandidateSet;
use verter_semantic::analysis::types::AnalyzedMacro;

use crate::resolver_core::shallow_file_state::ShallowValueSymbol;

/// One framework's synthesized-default policy.
///
/// The host selects the impl by the canonical's resolved [`FileLanguage`]
/// adapter id and calls [`Self::synthesise`] during shallow-state construction.
pub trait ComponentDefaultSynth: Send + Sync {
    /// Synthesize the component's implicit `default` value symbol, or `None`
    /// when this component contributes no synthesized default (no type-based
    /// macros, a userland `export default` already present, etc.).
    fn synthesise(&self, cx: ComponentDefaultSynthCtx<'_>) -> Option<ShallowValueSymbol>;
}

/// The PARSE-DOMAIN-only synthesis context.
///
/// Whole-struct destructure pinned by `component_default_synth_parse_domain_only`
/// — every field is parse-domain (no resolver, no capability bits, no
/// `StoreView`).
pub struct ComponentDefaultSynthCtx<'a> {
    /// The canonical id of the component file.
    pub canonical_id: &'a str,
    /// The file's resolved language row (the synth-leg selection key).
    pub language: &'a FileLanguage,
    /// The parse-domain macro list captured during shallow analysis.
    pub macros: &'a [AnalyzedMacro],
    /// The syntax-capture script-fact candidate set (empty when no provider was
    /// active for the file — Vue's synth ignores it; the seam carries it for
    /// later framework verticals).
    pub script_candidates: &'a FrameworkScriptCandidateSet,
}

/// The Vue synthesized-default leg.
///
/// Wraps the pure [`crate::resolver_core::vue_default_synth::synthesise_vue_default_value_symbol`]
/// — the synthesis logic is unchanged; only the path-suffix eligibility gate
/// dissolves into the host's registry language selection (the host only invokes
/// this leg for a `.vue`-classified canonical).
#[derive(Debug, Default)]
pub struct VueComponentDefaultSynth;

impl ComponentDefaultSynth for VueComponentDefaultSynth {
    fn synthesise(&self, cx: ComponentDefaultSynthCtx<'_>) -> Option<ShallowValueSymbol> {
        let ComponentDefaultSynthCtx {
            canonical_id: _,
            language: _,
            macros,
            script_candidates: _,
        } = cx;
        crate::resolver_core::vue_default_synth::synthesise_vue_default_value_symbol(macros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_semantic::analysis::types::{AnalyzedMacro, AnalyzedMacroKind};
    use verter_span::Span;

    fn type_based_props_macro(type_text: &str) -> AnalyzedMacro {
        let parsed =
            verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(type_text, None);
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: Some(Arc::new(parsed)),
            parsed_type_argument_scope: Some(verter_type_expr::TypeExprScope::new("")),
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn vue_synth_produces_default_for_type_based_props() {
        let macros = vec![type_based_props_macro("{ msg: string }")];
        let candidates = FrameworkScriptCandidateSet::default();
        let cx = ComponentDefaultSynthCtx {
            canonical_id: "/App.vue",
            language: &FileLanguage::vue(),
            macros: &macros,
            script_candidates: &candidates,
        };
        let synth = VueComponentDefaultSynth;
        let sym = synth.synthesise(cx).expect("type-based props synthesise");
        assert!(sym.is_synthesised_component_default);
    }

    #[test]
    fn vue_synth_returns_none_without_type_based_macros() {
        let candidates = FrameworkScriptCandidateSet::default();
        let cx = ComponentDefaultSynthCtx {
            canonical_id: "/App.vue",
            language: &FileLanguage::vue(),
            macros: &[],
            script_candidates: &candidates,
        };
        let synth = VueComponentDefaultSynth;
        assert!(synth.synthesise(cx).is_none());
    }
}
