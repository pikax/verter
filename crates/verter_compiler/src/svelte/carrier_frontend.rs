//! Svelte [`CarrierFrontend`]: the Svelte carrier identity.
//!
//! Parses through the Svelte parser and
//! [`super::carrier::build_svelte_parse_artifact`], answers the typed
//! reject, and owns the typed downcasts the Svelte backends use to reach the
//! parsed component back out of a type-erased artifact. Registered non-store
//! parse routes select it through the immutable catalog.

use std::sync::Arc;

use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierGrammarConfig, CarrierGrammarEncoder,
};
use verter_language::{
    parse_key_for, syntax_profile_id_for, CarrierBlockInventory, CarrierParse, FileLanguage,
    FrameworkAdapterId, InventoryValidationError, LanguageId, ParseKey, ParseOptions, SyntaxReject,
    UnregisteredFrameworkParseArtifact, SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
    SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
};

use crate::framework_common::capability::{CarrierFrontend, FrameworkEpoch, Present};
use crate::framework_common::carrier_registry::RegisteredCarrierFrontend;
use crate::framework_common::catalog::{FrontendCap, TypedCapabilityRegistration};
use crate::framework_common::profile::FrameworkProfileManifest;
use crate::framework_common::FrameworkParseArtifact;

use super::carrier::{build_svelte_parse_artifact, SvelteParseCarrier};
use super::parser::{parse_svelte, ParsedSvelte};

/// Svelte carrier frontend: parse, typed reject, adapter identity, and the
/// downcasts back to the parsed component.
///
/// Only this frontend's own inherent methods call the raw carrier downcast,
/// and only on Svelte artifacts; no capability token is involved.
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

/// The built-in Svelte manifest: the Svelte 5 line over the `.svelte`
/// carrier.
pub const SVELTE_PROFILE_MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
    family: "svelte",
    release: "5",
    admitted_majors: &[5],
    claimed_carriers: &["svelte"],
};

/// Typed Svelte epoch. Its catalog identity is the profile
/// [`SVELTE_PROFILE_MANIFEST`] mints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvelteSfc5;

impl FrameworkEpoch for SvelteSfc5 {
    const MANIFEST: FrameworkProfileManifest = SVELTE_PROFILE_MANIFEST;
}

impl SvelteCarrierFrontend {
    /// Issue the parse admission over a registered artifact this frontend
    /// actually parsed: the Svelte adapter, the Svelte carrier language,
    /// and a readable Svelte carrier payload are all required.
    /// Crate-private — reached only from host-integration composition;
    /// product backends never mint it.
    pub(crate) fn admit_registered(
        &self,
        artifact: &FrameworkParseArtifact,
    ) -> Option<SvelteParseAdmission> {
        (artifact.adapter_id() == &self.adapter_id()
            && artifact.language_id() == &self.carrier_language_id()
            && self.parsed_svelte(artifact).is_some())
        .then(|| SvelteParseAdmission {
            parse_key: Arc::new(artifact.parse_key().clone()),
        })
    }

    /// Adapter this frontend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::svelte()
    }

    /// Carrier language this frontend parses.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        LanguageId::new("svelte")
    }

    pub(crate) fn unregistered_carrier_arc(
        &self,
        artifact: &UnregisteredFrameworkParseArtifact,
    ) -> Option<Arc<SvelteParseCarrier>> {
        verter_language::__carrier_downcast_arc::<SvelteParseCarrier>(artifact)
    }

    /// Reach the parsed component back out of a Svelte artifact, or `None`
    /// when the artifact is not a Svelte carrier.
    #[must_use]
    pub(crate) fn parsed_svelte<'a>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a ParsedSvelte> {
        self.svelte_carrier(artifact)
            .map(SvelteParseCarrier::parsed)
    }

    pub(crate) fn unregistered_parsed_svelte<'a>(
        &self,
        artifact: &'a UnregisteredFrameworkParseArtifact,
    ) -> Option<&'a ParsedSvelte> {
        verter_language::__carrier_downcast_ref::<SvelteParseCarrier>(artifact)
            .map(SvelteParseCarrier::parsed)
    }

    /// Reach the Svelte carrier payload — the parse plus its retained typed
    /// attribute-value lowering — or `None` for a foreign artifact.
    #[must_use]
    pub(super) fn svelte_carrier<'a>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a SvelteParseCarrier> {
        artifact.carrier_ref::<SvelteParseCarrier>()
    }
}

impl CarrierFrontend for SvelteCarrierFrontend {
    type ParseArtifact = Arc<UnregisteredFrameworkParseArtifact>;
    type SyntaxReject = SyntaxReject;
    type ParseAdmission = SvelteParseAdmission;

    /// Parse carrier `source` into the framework-neutral artifact.
    ///
    /// Recoverable malformed syntax is NOT a rejection: the parser collects
    /// diagnostics inline, so ordinary malformed input still returns `Ok`
    /// with the problem recorded on the artifact's mapped diagnostic
    /// channel. `Err(SyntaxReject)` is reserved for a request this frontend
    /// cannot honor at all (an explicitly unsupported parse-option
    /// combination) and is returned BEFORE any artifact is constructed.
    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        let language = FileLanguage::svelte();
        let syntax_profile = syntax_profile_id_for(&language, opts)
            .expect("the built-in Svelte language has a syntax profile");
        let parse_key = parse_key_for(
            source,
            &language,
            SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
            SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
            &syntax_profile,
        )
        .expect("the built-in Svelte language has a parse identity");
        let syntax_profile = Arc::new(syntax_profile);
        let parse_key = Arc::new(parse_key);
        // Svelte's official `loose` parse mode is not implemented by this
        // frontend (capability-matrix: `SVELTE-PARSE-LOCAL` is "strict
        // parser diagnostics/recovery only; ... loose is unsupported
        // fail-closed"). Reject before parsing — never silently downgrade
        // to strict parsing.
        if opts.svelte_loose {
            return Err(SyntaxReject::UnsupportedProfile {
                parse_key,
                syntax_profile,
                reason: verter_language::UnsupportedSyntaxProfileReason::UnsupportedOption,
            });
        }
        // Every OTHER strict-parse / close-tag defect is a parser-owned
        // RECOVERY point: the tokenizer is intentionally infallible and
        // always produces a faithful tree, which is correct for the IDE
        // projection (it owns its own error recovery) — see
        // `SvelteStrictParseError`'s doc. Refusing publication here would
        // make the carrier unusable for exactly the states an editor spends
        // most of its time in (an unclosed `<script>` mid-typing, a stray
        // close tag). The CLIENT-runtime "Verter emits a `Main` ⇔ official
        // ACCEPTS" contract is enforced separately and later, at
        // `official_reject_gate` (compile time) — never at this parse/publish
        // seam.
        let parsed = Arc::new(parse_svelte(source));
        Ok(build_svelte_parse_artifact(
            source,
            parsed,
            parse_key,
            syntax_profile,
        ))
    }
}

impl RegisteredCarrierFrontend for SvelteCarrierFrontend {
    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        CarrierFrontend::parse(self, source, opts)
    }

    fn project_geometry(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<UnregisteredFrameworkParseArtifact>,
    ) -> Result<(CarrierBlockInventory, Arc<dyn CarrierParse>), InventoryValidationError> {
        let inventory = crate::framework_common::registered_carrier_projection::project_svelte(
            self, accepted, artifact,
        )?;
        let carrier: Arc<dyn CarrierParse> = self
            .unregistered_carrier_arc(artifact)
            .expect("Svelte carrier payload");
        Ok((inventory, carrier))
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
    .expect("the built-in Svelte profile manifest is admitted")
    .with_registered_grammar(svelte_grammar_config())
}

/// The `.svelte` carrier's canonical grammar configuration. The carrier
/// has no grammar options: the canonical encoding is tag `2` alone and the
/// frontend parses under the default (strict) options.
#[must_use]
pub fn svelte_grammar_config() -> CarrierGrammarConfig {
    CarrierGrammarConfig::from_owner(
        FileLanguage::svelte(),
        CarrierGrammarEncoder::new(2),
        ParseOptions::default(),
    )
}

#[cfg(test)]
mod grammar_tests {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
    };
    use verter_language::FileLanguage;

    use super::svelte_grammar_config;

    #[test]
    fn canonical_svelte_fingerprint_encoding_matches_the_v1_golden() {
        let fingerprint = *CarrierGrammarAuthority::new()
            .expect("grammar authority")
            .register_carrier_grammar(
                FileLanguage::svelte(),
                FrameworkAdapterSemanticVersion::new(1).expect("nonzero"),
                CarrierParserGrammarVersion::new(1).expect("nonzero"),
                svelte_grammar_config(),
            )
            .expect("Svelte grammar registration")
            .fingerprint()
            .as_bytes();
        assert_eq!(
            fingerprint,
            [
                0xd7, 0xa5, 0x37, 0xab, 0x6a, 0x6a, 0x21, 0xda, 0xa7, 0x3f, 0x24, 0x11, 0x0a, 0x96,
                0xf8, 0xea, 0x0b, 0x7f, 0x06, 0x4a, 0x36, 0x60, 0x50, 0xc1, 0x26, 0x7a, 0xcf, 0x28,
                0x10, 0x4b, 0xce, 0x91,
            ]
        );
    }
}
