//! Svelte [`CarrierFrontend`] adapter.
//!
//! Registers the Svelte frontend against the existing Svelte parser and
//! [`super::carrier::build_svelte_parse_artifact`] constructor (via
//! [`SvelteCarrierCompiler::parse`]). Registered non-store parse routes
//! select this backend through the immutable catalog.

use std::sync::Arc;

use verter_language::carrier_grammar::CarrierGrammarConfig;
use verter_language::{
    FrameworkAdapterId, LanguageId, ParseKey, ParseOptions, SyntaxReject,
    UnregisteredFrameworkParseArtifact,
};

use crate::framework_common::capability::{CarrierFrontend, FrameworkEpoch, Present};
use crate::framework_common::catalog::{FrontendCap, TypedCapabilityRegistration};
use crate::framework_common::CarrierCompiler;

use super::carrier::SvelteCarrierCompiler;

/// Svelte carrier frontend: parse, typed reject, adapter identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SvelteCarrierFrontend;

/// Admission token issued only after a successful Svelte frontend parse.
/// Carries the exact [`ParseKey`] it was issued over, so downstream
/// composition verifies the witness against the artifact it consumes
/// instead of trusting by-convention pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvelteParseAdmission {
    parse_key: Arc<ParseKey>,
}

impl SvelteParseAdmission {
    /// The exact parse identity this admission was issued over.
    #[must_use]
    pub(crate) fn parse_key(&self) -> &Arc<ParseKey> {
        &self.parse_key
    }
}

/// Typed Svelte epoch. Catalog identity is derived from [`FrameworkEpoch::ID`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvelteSfc5;

impl FrameworkEpoch for SvelteSfc5 {
    const ID: &'static str = SvelteCarrierFrontend::EPOCH;
}

impl SvelteCarrierFrontend {
    /// Catalog epoch spelling for the Svelte frontend row.
    pub const EPOCH: &'static str = "svelte";

    /// Issue the parse admission over a registered artifact this frontend
    /// actually parsed: the Svelte adapter, the Svelte carrier language,
    /// and a readable Svelte carrier payload are all required.
    /// Crate-private — reached only from host-integration composition;
    /// product backends never mint it.
    pub(crate) fn admit_registered(
        &self,
        artifact: &crate::framework_common::FrameworkParseArtifact,
    ) -> Option<SvelteParseAdmission> {
        (artifact.adapter_id() == &self.adapter_id()
            && artifact.language_id() == &self.carrier_language_id()
            && SvelteCarrierCompiler.parsed_svelte(artifact).is_some())
        .then(|| SvelteParseAdmission {
            parse_key: Arc::new(artifact.parse_key().clone()),
        })
    }

    /// Adapter this frontend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        SvelteCarrierCompiler.adapter_id()
    }

    /// Carrier language this frontend parses.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        SvelteCarrierCompiler.carrier_language_id()
    }
}

impl CarrierFrontend for SvelteCarrierFrontend {
    type ParseArtifact = Arc<UnregisteredFrameworkParseArtifact>;
    type SyntaxReject = SyntaxReject;
    type ParseAdmission = SvelteParseAdmission;

    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        SvelteCarrierCompiler.parse(source, opts)
    }
}

/// Typed Svelte frontend catalog row, carrying the Svelte registered
/// carrier-grammar fact.
#[must_use]
pub fn svelte_carrier_frontend_registration(
) -> TypedCapabilityRegistration<FrontendCap<SvelteCarrierFrontend>> {
    TypedCapabilityRegistration::register_frontend::<SvelteSfc5, _>(
        SvelteCarrierFrontend.adapter_id(),
        SvelteCarrierFrontend.carrier_language_id(),
        Present(SvelteCarrierFrontend),
    )
    .with_registered_grammar(CarrierGrammarConfig::Svelte)
}
