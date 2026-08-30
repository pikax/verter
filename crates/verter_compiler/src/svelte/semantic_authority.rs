//! Svelte [`FrameworkSemanticAuthority`] adapter.
//!
//! Owns Svelte eval-source and template-fact interpretation over an
//! already-admitted parse artifact. Catalog lookup keys adapter × epoch
//! × Semantic.

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId};

use crate::compile::RawTemplateData;
use crate::framework_common::capability::{FrameworkSemanticAuthority, Present};
use crate::framework_common::catalog::{SemanticCap, TypedCapabilityRegistration};
use crate::framework_common::registered_carrier_projection::TemplateFactsProduct;
use crate::framework_common::CarrierCompiler;
use crate::framework_common::FrameworkParseArtifact;
use crate::svelte::carrier::SvelteParseCarrier;

use super::carrier::SvelteCarrierCompiler;
use super::carrier_frontend::SvelteSfc5;

/// Svelte semantic authority: eval-source, template facts, typed identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SvelteSemanticAuthority;

/// Admission token issued only over an already-admitted Svelte parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvelteSemanticAdmission {
    _private: (),
}

impl SvelteSemanticAuthority {
    /// Issue the semantic admission over an already-admitted Svelte parse:
    /// the parse admission's witnessed [`verter_language::ParseKey`] must
    /// be the artifact's own, and the artifact's epoch must select this
    /// authority's registered semantic catalog row. Crate-private —
    /// reached only from host-integration composition, and only with the
    /// parse admission in hand; product backends never mint it.
    pub(crate) fn admit_over_parse(
        &self,
        parse: &super::carrier_frontend::SvelteParseAdmission,
        artifact: &FrameworkParseArtifact,
    ) -> Option<SvelteSemanticAdmission> {
        if parse.parse_key().as_ref() != artifact.parse_key() {
            return None;
        }
        crate::framework_common::registered_carrier_projection::registered_semantic_for(
            artifact.adapter_id(),
            artifact.epoch(),
        )
        .map(|_| SvelteSemanticAdmission { _private: () })
    }

    /// Adapter this authority answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        SvelteCarrierCompiler.adapter_id()
    }

    /// Carrier language this authority interprets.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        SvelteCarrierCompiler.carrier_language_id()
    }
}

impl FrameworkSemanticAuthority<SvelteSfc5> for SvelteSemanticAuthority {
    type EvalSource = Arc<str>;
    type TemplateFacts = Option<TemplateFactsProduct>;
    type StyleMeaning = ();
    type SemanticAdmission = SvelteSemanticAdmission;
    type ParseArtifact = FrameworkParseArtifact;

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        crate::framework_common::vue_semantic_authority::position_preserving_eval_source(
            source, artifact,
        )
    }

    fn template_facts(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Option<TemplateFactsProduct> {
        let carrier = artifact.carrier_ref::<SvelteParseCarrier>()?;
        let parsed = carrier.parsed();
        let mut data = RawTemplateData::default();
        super::template_facts::collect_component_usages(
            &parsed.template,
            carrier.attribute_expressions(),
            source,
            &mut data,
        );
        super::template_facts::collect_snippet_definitions(&parsed.template, source, &mut data);
        super::template_facts::collect_svelte_directives(&parsed.template, source, &mut data);
        Some(TemplateFactsProduct {
            data,
            diagnostics: Vec::new(),
        })
    }
}

/// Typed Svelte semantic catalog row.
#[must_use]
pub fn svelte_semantic_authority_registration(
) -> TypedCapabilityRegistration<SemanticCap<SvelteSemanticAuthority>> {
    TypedCapabilityRegistration::register_semantic::<SvelteSfc5, _>(
        SvelteSemanticAuthority.adapter_id(),
        SvelteSemanticAuthority.carrier_language_id(),
        Present(SvelteSemanticAuthority),
    )
}
