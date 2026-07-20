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

use crate::decl_body_memo::LoweredValueDecl;

/// One framework's synthesized-default policy.
///
/// The host selects the impl by the canonical's resolved [`FileLanguage`]
/// adapter id and calls [`Self::synthesise`] during shallow-state construction.
pub trait ComponentDefaultSynth: Send + Sync {
    /// Synthesize the component's implicit `default` value symbol's lowered
    /// body, or `None` when this component contributes no synthesized default
    /// (no type-based macros, a userland `export default` already present,
    /// etc.). The host stores it through
    /// [`crate::resolver_core::ShallowFileState`]'s
    /// `insert_synthesised_value_default`, which derives the shallow header from
    /// the lowered body and retains the body for the lazy-memo demand path.
    fn synthesise(&self, cx: ComponentDefaultSynthCtx<'_>) -> Option<LoweredValueDecl>;
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
    fn synthesise(&self, cx: ComponentDefaultSynthCtx<'_>) -> Option<LoweredValueDecl> {
        let ComponentDefaultSynthCtx {
            canonical_id: _,
            language,
            macros,
            script_candidates: _,
        } = cx;
        crate::resolver_core::vue_default_synth::synthesise_vue_default_value_symbol(macros)
            .or_else(|| {
                // EVERY genuine `.vue` carrier is a component: a scriptless /
                // macro-less SFC still IS its own implicit `export default`
                // (the empty-instance component). Without this arm the file
                // has NO `default` on its shallow EXPORT surface, so a barrel
                // `export { default as X } from './X.vue'` route walk — the
                // strict export-surface walk value resolution now shares with
                // the type rail — misses at the terminal hop and fallthrough
                // child routing fails. Mirrors the Svelte leg's
                // always-synthesize contract (an empty candidate set is the
                // empty-default case, never a no-op). The typeinfo SCRATCH
                // surface (routed to this leg by the registry despite its
                // `.ts` classification) keeps the macros-only behavior: a
                // scratch with no inlined macros synthesizes nothing.
                language
                    .is_vue()
                    .then(crate::resolver_core::vue_default_synth::empty_vue_default_value_symbol)
            })
    }
}

/// The Svelte synthesized-default leg.
///
/// Consumes the PARSE-DOMAIN
/// [`SvelteScriptCandidates`](verter_semantic::analysis::framework_facts::svelte::SvelteScriptCandidates)
/// captured by the Svelte script-fact provider's syntax-capture half — never the
/// resolved-validation facts. The candidate payload rides on the ctx's
/// `script_candidates` set keyed by the Svelte adapter id.
///
/// EVERY `.svelte` file is a component, so this leg ALWAYS synthesizes a
/// default — even a pure-markup component with no `$props()` and no exports gets
/// a class-shaped default whose instance carries `$props: {}`. The leg is only
/// invoked for a `.svelte`-classified canonical (the host selects it by adapter
/// id), so an empty candidate set is the empty-default case, never a no-op.
#[derive(Debug, Default)]
pub struct SvelteComponentDefaultSynth;

impl ComponentDefaultSynth for SvelteComponentDefaultSynth {
    fn synthesise(&self, cx: ComponentDefaultSynthCtx<'_>) -> Option<LoweredValueDecl> {
        use verter_semantic::analysis::framework_facts::svelte::SvelteScriptCandidates;
        let empty = SvelteScriptCandidates::default();
        let candidates = cx
            .script_candidates
            .for_adapter(&verter_language::FrameworkAdapterId::svelte())
            .and_then(|entry| entry.payload.downcast_ref::<SvelteScriptCandidates>())
            .unwrap_or(&empty);
        Some(
            crate::resolver_core::svelte_default_synth::synthesise_svelte_default_value_symbol(
                candidates,
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_semantic::analysis::types::{AnalyzedMacro, AnalyzedMacroKind};
    use verter_span::Span;
    use verter_type_expr::locators::{
        AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
    };

    fn type_based_props_macro() -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
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
            parsed_type_argument: Some(MacroPayloadLocator {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from("/App.vue"),
                    owner: verter_type_expr::TopLevelOwnerId::instance(0),
                    symbol: Arc::from("default"),
                    space: LocatorSymbolSpace::Value,
                },
                macro_index: 0,
                payload: MacroPayloadPosition::TypeArgument,
            }),
            parsed_type_argument_scope: Some(verter_type_expr::TypeExprScope::new("")),
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn vue_synth_produces_default_for_type_based_props() {
        let macros = vec![type_based_props_macro()];
        let candidates = FrameworkScriptCandidateSet::default();
        let cx = ComponentDefaultSynthCtx {
            canonical_id: "/App.vue",
            language: &FileLanguage::vue(),
            macros: &macros,
            script_candidates: &candidates,
        };
        let synth = VueComponentDefaultSynth;
        let sym = synth.synthesise(cx).expect("type-based props synthesise");
        assert_eq!(
            sym.kind,
            verter_semantic::analysis::type_eval::ValueDeclKind::Class
        );
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
