//! Vue [`FrameworkSemanticAuthority`] adapter.
//!
//! Owns Vue eval-source and template-fact interpretation over an
//! already-admitted parse artifact. Eval-source is selected by catalog
//! identity (adapter × epoch × Semantic).

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId};

use super::capability::{FrameworkSemanticAuthority, Present};
use super::carrier_compiler::{CarrierCompiler, TemplateFacts};
use super::catalog::{SemanticCap, TypedCapabilityRegistration};
use super::vue_bridge::VueCarrierCompiler;
use super::vue_carrier_frontend::VueSfcV3;
use super::FrameworkParseArtifact;

/// Position-preserving eval source over admitted script regions.
///
/// Script bytes stay at their raw offsets; every other byte is blanked
/// (line terminators preserved). Output length equals input length. It
/// does not inject a newline between adjacent script regions.
pub(crate) fn position_preserving_eval_source(
    source: &str,
    artifact: &FrameworkParseArtifact,
) -> Arc<str> {
    let src = source.as_bytes();
    let mut out: Vec<u8> = src
        .iter()
        .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    for region in artifact.script_regions() {
        let start = region.span.start as usize;
        let end = region.span.end as usize;
        if start <= end && end <= src.len() {
            out[start..end].copy_from_slice(&src[start..end]);
        }
    }
    match String::from_utf8(out) {
        Ok(text) => Arc::from(text),
        Err(_) => Arc::from(source),
    }
}

/// Vue semantic authority: eval-source, template facts, typed identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueSemanticAuthority;

/// Admission token issued only over an already-admitted Vue parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueSemanticAdmission {
    _private: (),
}

impl VueSemanticAuthority {
    /// Adapter this authority answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierCompiler.adapter_id()
    }

    /// Carrier language this authority interprets.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierCompiler.carrier_language_id()
    }
}

impl FrameworkSemanticAuthority<VueSfcV3> for VueSemanticAuthority {
    type EvalSource = Arc<str>;
    type TemplateFacts = TemplateFacts;
    type StyleMeaning = ();
    type SemanticAdmission = VueSemanticAdmission;
    type ParseArtifact = FrameworkParseArtifact;

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        position_preserving_eval_source(source, artifact)
    }

    fn template_facts(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts {
        VueCarrierCompiler.template_data(source, artifact)
    }
}

/// Typed Vue semantic catalog row.
#[must_use]
pub fn vue_semantic_authority_registration(
) -> TypedCapabilityRegistration<SemanticCap<VueSemanticAuthority>> {
    TypedCapabilityRegistration::register_semantic::<VueSfcV3, _>(
        VueSemanticAuthority.adapter_id(),
        VueSemanticAuthority.carrier_language_id(),
        Present(VueSemanticAuthority),
    )
}
