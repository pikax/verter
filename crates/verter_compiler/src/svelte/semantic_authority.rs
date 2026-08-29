//! Svelte [`FrameworkSemanticAuthority`] adapter.
//!
//! Registers Svelte eval-source and template-fact interpretation against
//! the existing Svelte compiler methods over an already-admitted parse
//! artifact. Catalog rows stay unused by production request routes.

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId};

use crate::framework_common::capability::{FrameworkEpochId, FrameworkSemanticAuthority, Present};
use crate::framework_common::catalog::{SemanticCap, TypedCapabilityRegistration};
use crate::framework_common::CarrierCompiler;
use crate::framework_common::FrameworkParseArtifact;
use crate::framework_common::TemplateFacts;

use super::carrier::SvelteCarrierCompiler;
use super::carrier_frontend::SvelteCarrierFrontend;

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

impl FrameworkSemanticAuthority<FrameworkEpochId> for SvelteSemanticAuthority {
    type EvalSource = Arc<str>;
    type TemplateFacts = TemplateFacts;
    type StyleMeaning = ();
    type SemanticAdmission = SvelteSemanticAdmission;
    type ParseArtifact = FrameworkParseArtifact;

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        SvelteCarrierCompiler.eval_source(source, artifact)
    }

    fn template_facts(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts {
        SvelteCarrierCompiler.template_data(source, artifact)
    }
}

/// Typed Svelte semantic catalog row.
#[must_use]
pub fn svelte_semantic_authority_registration(
) -> TypedCapabilityRegistration<SemanticCap<SvelteSemanticAuthority>> {
    TypedCapabilityRegistration::register_semantic(
        SvelteSemanticAuthority.adapter_id(),
        SvelteSemanticAuthority.carrier_language_id(),
        FrameworkEpochId::new(SvelteCarrierFrontend::EPOCH),
        Present(SvelteSemanticAuthority),
    )
}
