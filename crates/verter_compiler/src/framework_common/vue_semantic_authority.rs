//! Vue [`FrameworkSemanticAuthority`] adapter.
//!
//! Owns Vue eval-source and template-fact interpretation over an
//! already-admitted parse artifact. Catalog lookup keys adapter × epoch
//! × Semantic.

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId};

use crate::compile::types::{
    CodegenOptions, CompileTarget, ResolvedVueCompileOptions, VueMacroSemanticInput,
};
use crate::compile::{compile_from_parsed_legacy, RawTemplateData};

use super::capability::{FrameworkSemanticAuthority, Present};
use super::carrier_compiler::CarrierCompiler;
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
    type TemplateFacts = Option<RawTemplateData>;
    type StyleMeaning = ();
    type SemanticAdmission = VueSemanticAdmission;
    type ParseArtifact = FrameworkParseArtifact;

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        position_preserving_eval_source(source, artifact)
    }

    fn template_facts(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Option<RawTemplateData> {
        let parsed = VueCarrierCompiler.parsed_sfc(artifact)?;
        let core_opts = CodegenOptions {
            target: CompileTarget::TEMPLATE_DATA,
            ..Default::default()
        };
        let verter_opts = ResolvedVueCompileOptions {
            source_map: false,
            ..Default::default()
        };
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_from_parsed_legacy(
            source,
            parsed,
            &core_opts,
            &verter_opts,
            &VueMacroSemanticInput::Unavailable,
            &alloc,
        )
        .ok()?;
        match result.template_data {
            Some(data) => Some(data),
            None if parsed.template_ast().is_none() => Some(RawTemplateData::default()),
            None => None,
        }
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
