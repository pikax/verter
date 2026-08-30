//! Vue [`CarrierFrontend`] adapter.
//!
//! Registers the Vue frontend against the existing Vue parser and
//! [`super::vue_bridge::build_vue_parse_artifact`] constructor. Registered
//! non-store parse routes select this backend through the immutable catalog.

use std::sync::Arc;

use verter_language::carrier_grammar::CarrierGrammarConfig;
use verter_language::{
    FrameworkAdapterId, LanguageId, ParseOptions, SyntaxReject, UnregisteredFrameworkParseArtifact,
};

use super::capability::{CarrierFrontend, FrameworkEpoch, Present};
use super::catalog::{FrontendCap, TypedCapabilityRegistration};
use super::vue_bridge::VueCarrierCompiler;
use super::CarrierCompiler;

/// Vue carrier frontend: parse, typed reject, adapter identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueCarrierFrontend;

/// Admission token issued only after a successful Vue frontend parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueParseAdmission {
    _private: (),
}

/// Typed Vue SFC epoch. Catalog identity is derived from [`FrameworkEpoch::ID`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueSfcV3;

impl FrameworkEpoch for VueSfcV3 {
    const ID: &'static str = VueCarrierFrontend::EPOCH;
}

impl VueCarrierFrontend {
    /// Catalog epoch spelling for the Vue SFC frontend row.
    pub const EPOCH: &'static str = "vue";

    /// Adapter this frontend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierCompiler.adapter_id()
    }

    /// Carrier language this frontend parses.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierCompiler.carrier_language_id()
    }
}

impl CarrierFrontend for VueCarrierFrontend {
    type ParseArtifact = Arc<UnregisteredFrameworkParseArtifact>;
    type SyntaxReject = SyntaxReject;
    type ParseAdmission = VueParseAdmission;

    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        VueCarrierCompiler.parse(source, opts)
    }
}

/// Typed Vue frontend catalog row, carrying the Vue registered
/// carrier-grammar fact (default interpolation delimiters, no custom
/// elements).
#[must_use]
pub fn vue_carrier_frontend_registration(
) -> TypedCapabilityRegistration<FrontendCap<VueCarrierFrontend>> {
    TypedCapabilityRegistration::register_frontend::<VueSfcV3, _>(
        VueCarrierFrontend.adapter_id(),
        VueCarrierFrontend.carrier_language_id(),
        Present(VueCarrierFrontend),
    )
    .with_registered_grammar(
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>())
            .expect("default Vue grammar delimiters are non-empty"),
    )
}
