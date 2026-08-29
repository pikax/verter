//! Svelte [`FrameworkSemanticAuthority`] adapter.
//!
//! Owns Svelte eval-source and template-fact interpretation over an
//! already-admitted parse artifact. Eval-source is selected by catalog
//! identity (adapter × epoch × Semantic).

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId};

use crate::framework_common::capability::{FrameworkSemanticAuthority, Present};
use crate::framework_common::catalog::{SemanticCap, TypedCapabilityRegistration};
use crate::framework_common::CarrierCompiler;
use crate::framework_common::FrameworkParseArtifact;
use crate::framework_common::TemplateFacts;

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
    type TemplateFacts = TemplateFacts;
    type StyleMeaning = ();
    type SemanticAdmission = SvelteSemanticAdmission;
    type ParseArtifact = FrameworkParseArtifact;

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        crate::framework_common::vue_semantic_authority::position_preserving_eval_source(
            source, artifact,
        )
    }

    fn template_facts(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts {
        SvelteCarrierCompiler.template_data(source, artifact)
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
