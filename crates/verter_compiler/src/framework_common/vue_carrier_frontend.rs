//! Vue [`CarrierFrontend`]: the Vue carrier identity.
//!
//! Parses through the Vue parser and
//! [`super::vue_bridge::build_vue_parse_artifact`], answers the typed
//! reject, and owns the typed downcasts the Vue backends use to reach the
//! parsed SFC back out of a type-erased artifact. Registered non-store parse
//! routes select it through the immutable catalog.

use std::sync::Arc;

use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierGrammarConfig, CarrierGrammarEncoder,
};
use verter_language::{
    parse_key_for, syntax_profile_id_for, CarrierBlockInventory, CarrierParse, FileLanguage,
    FrameworkAdapterId, InventoryValidationError, LanguageId, ParseKey, ParseOptions, SyntaxReject,
    UnregisteredFrameworkParseArtifact, VUE_SYNTAX_COMPATIBILITY_DOMAIN,
    VUE_SYNTAX_COMPATIBILITY_EPOCH,
};
use verter_parser::parser::types::ParsedSfc;

use super::capability::{CarrierFrontend, FrameworkEpoch, Present};
use super::carrier_registry::RegisteredCarrierFrontend;
use super::catalog::{FrontendCap, TypedCapabilityRegistration};
use super::profile::FrameworkProfileManifest;
use super::vue_bridge::{build_vue_parse_artifact, parse_sfc, VueParseCarrier};

/// Vue carrier frontend: parse, typed reject, adapter identity, and the
/// downcasts back to the parsed SFC.
///
/// Only this frontend's own inherent methods call the raw carrier downcast,
/// and only on Vue artifacts; no capability token is involved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueCarrierFrontend;

/// Admission token issued only after a successful Vue frontend parse.
/// Carries the exact [`ParseKey`] it was issued over, so downstream
/// composition verifies the witness against the artifact it consumes
/// instead of trusting by-convention pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueParseAdmission {
    parse_key: Arc<ParseKey>,
}

impl VueParseAdmission {
    /// The exact parse identity this admission was issued over.
    #[must_use]
    pub(crate) fn parse_key(&self) -> &Arc<ParseKey> {
        &self.parse_key
    }
}

/// The built-in Vue manifest: the Vue 3 line over the `.vue` carrier.
pub const VUE_PROFILE_MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
    family: "vue",
    release: "3",
    admitted_majors: &[3],
    claimed_carriers: &["vue"],
};

/// Typed Vue SFC epoch. Its catalog identity is the profile
/// [`VUE_PROFILE_MANIFEST`] mints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueSfcV3;

impl FrameworkEpoch for VueSfcV3 {
    const MANIFEST: FrameworkProfileManifest = VUE_PROFILE_MANIFEST;
}

impl VueCarrierFrontend {
    /// Issue the parse admission over a registered artifact this frontend
    /// actually parsed: the Vue adapter, the Vue carrier language, and a
    /// readable Vue carrier payload are all required. Crate-private —
    /// reached only from host-integration composition; product backends
    /// never mint it.
    pub(crate) fn admit_registered(
        &self,
        artifact: &super::FrameworkParseArtifact,
    ) -> Option<VueParseAdmission> {
        (artifact.adapter_id() == &self.adapter_id()
            && artifact.language_id() == &self.carrier_language_id()
            && self.parsed_sfc(artifact).is_some())
        .then(|| VueParseAdmission {
            parse_key: Arc::new(artifact.parse_key().clone()),
        })
    }

    /// Adapter this frontend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::vue()
    }

    /// Carrier language this frontend parses.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        // The `.vue` SFC carrier language. A same-adapter non-carrier row
        // (e.g. an external Vue template) is NOT this language and is not
        // routed through the SFC parse path.
        LanguageId::new("vue")
    }

    pub(crate) fn unregistered_carrier_arc(
        &self,
        artifact: &UnregisteredFrameworkParseArtifact,
    ) -> Option<Arc<VueParseCarrier>> {
        verter_language::__carrier_downcast_arc::<VueParseCarrier>(artifact)
    }

    /// Reach the parsed SFC back out of a Vue artifact, or `None` when the
    /// erased payload is not a Vue carrier.
    pub(super) fn parsed_sfc<'a>(
        &self,
        artifact: &'a super::FrameworkParseArtifact,
    ) -> Option<&'a ParsedSfc> {
        artifact
            .carrier_ref::<VueParseCarrier>()
            .map(VueParseCarrier::parsed)
    }

    /// The parse-affecting options stamped on a Vue artifact, or `None` when
    /// the erased payload is not a Vue carrier.
    pub(super) fn parse_options<'a>(
        &self,
        artifact: &'a super::FrameworkParseArtifact,
    ) -> Option<&'a ParseOptions> {
        artifact
            .carrier_ref::<VueParseCarrier>()
            .map(VueParseCarrier::parse_options)
    }

    pub(super) fn unregistered_parsed_sfc<'a>(
        &self,
        artifact: &'a UnregisteredFrameworkParseArtifact,
    ) -> Option<&'a ParsedSfc> {
        verter_language::__carrier_downcast_ref::<VueParseCarrier>(artifact)
            .map(VueParseCarrier::parsed)
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
        // This crate never substitutes a default for an absent delimiter
        // pair — that decision belongs to whoever constructs `ParseOptions`
        // (see `ParseOptions::vue_standard`). It only rejects a request it
        // cannot honor: a literal empty-tuple delimiter pair is un-
        // tokenizable — an empty open AND an empty close both match every
        // position with zero-width consumption, which hangs the tokenizer
        // in a zero-advance loop (verified empirically: >1,000,000 events
        // emitted without terminating). Every other pair, including one
        // that happens to equal the tokenizer's own built-in default, is
        // passed through to `parse_sfc` literally.
        if opts.delimiters.0.is_empty() && opts.delimiters.1.is_empty() {
            let language = FileLanguage::vue();
            let syntax_profile = syntax_profile_id_for(&language, opts)
                .expect("the built-in Vue syntax profile exists");
            let parse_key = parse_key_for(
                source,
                &language,
                VUE_SYNTAX_COMPATIBILITY_DOMAIN,
                VUE_SYNTAX_COMPATIBILITY_EPOCH,
                &syntax_profile,
            )
            .expect("the built-in Vue parse identity exists");
            return Err(verter_language::SyntaxReject::UnsupportedProfile {
                parse_key: Arc::new(parse_key),
                syntax_profile: Arc::new(syntax_profile),
                reason: verter_language::UnsupportedSyntaxProfileReason::UnsupportedOption,
            });
        }
        let delimiters = Some((opts.delimiters.0.as_str(), opts.delimiters.1.as_str()));
        let parsed = Arc::new(parse_sfc(source, delimiters, Some(&opts.custom_elements)));
        Ok(build_vue_parse_artifact(source, parsed, opts))
    }
}

impl RegisteredCarrierFrontend for VueCarrierFrontend {
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
        let inventory =
            super::registered_carrier_projection::project_vue(self, accepted, artifact)?;
        let carrier: Arc<dyn CarrierParse> = self
            .unregistered_carrier_arc(artifact)
            .expect("Vue carrier payload");
        Ok((inventory, carrier))
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
    .expect("the built-in Vue profile manifest is admitted")
    .with_registered_grammar(
        vue_grammar_config("{{", "}}", std::iter::empty::<&str>())
            .expect("default Vue grammar delimiters are non-empty"),
    )
}

/// Why a Vue grammar configuration cannot be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueGrammarConfigError {
    /// An interpolation delimiter is empty: an empty delimiter matches at
    /// every position and cannot be tokenized.
    EmptyDelimiter,
}

/// The `.vue` carrier's canonical grammar configuration: the interpolation
/// delimiters and the custom-element prefixes, sorted and deduplicated.
///
/// The canonical encoding is tag `1`, the open and close delimiters, the
/// custom-element count and each name; the parse options carry the same
/// delimiters and the same canonical name list, so the parse key and the
/// grammar fingerprint read one input.
pub fn vue_grammar_config<I, S>(
    open: &str,
    close: &str,
    custom_elements: I,
) -> Result<CarrierGrammarConfig, VueGrammarConfigError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if open.is_empty() || close.is_empty() {
        return Err(VueGrammarConfigError::EmptyDelimiter);
    }
    // Vue's registered custom-element matcher is byte-exact `starts_with`,
    // so the canonical order compares authored bytes.
    let mut names: Vec<String> = custom_elements
        .into_iter()
        .map(|name| name.as_ref().to_string())
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut canonical = CarrierGrammarEncoder::new(1);
    canonical.string(open);
    canonical.string(close);
    canonical.count(names.len());
    for name in &names {
        canonical.string(name);
    }
    Ok(CarrierGrammarConfig::from_owner(
        FileLanguage::vue(),
        canonical,
        ParseOptions {
            delimiters: (open.to_string(), close.to_string()),
            custom_elements: names,
            svelte_loose: false,
        },
    ))
}

#[cfg(test)]
mod grammar_tests {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
    };
    use verter_language::FileLanguage;

    use super::{vue_grammar_config, VueGrammarConfigError};

    fn fingerprint(config: verter_language::carrier_grammar::CarrierGrammarConfig) -> [u8; 32] {
        *CarrierGrammarAuthority::new()
            .expect("grammar authority")
            .register_carrier_grammar(
                FileLanguage::vue(),
                FrameworkAdapterSemanticVersion::new(1).expect("nonzero"),
                CarrierParserGrammarVersion::new(1).expect("nonzero"),
                config,
            )
            .expect("Vue grammar registration")
            .fingerprint()
            .as_bytes()
    }

    #[test]
    fn canonical_vue_fingerprint_encoding_matches_the_v1_golden() {
        assert_eq!(
            fingerprint(vue_grammar_config("{{", "}}", std::iter::empty::<&str>()).unwrap()),
            [
                0x7c, 0x27, 0x8e, 0x6e, 0xa2, 0x7c, 0x54, 0x34, 0xe9, 0x30, 0x09, 0x9e, 0x69, 0xf7,
                0x44, 0xbc, 0xe6, 0xf6, 0x3d, 0x39, 0xb8, 0xc7, 0xf6, 0x23, 0xf8, 0x9f, 0x13, 0x66,
                0x8d, 0xe6, 0xcb, 0x31,
            ]
        );
    }

    #[test]
    fn canonical_vue_fingerprint_with_custom_elements_matches_the_v1_golden() {
        assert_eq!(
            fingerprint(vue_grammar_config("{{", "}}", ["x-", "math-"]).unwrap()),
            [
                0x89, 0xf4, 0xc0, 0x70, 0xc9, 0xc1, 0x11, 0xe1, 0x7d, 0x85, 0x2b, 0x9b, 0xb5, 0xa3,
                0x69, 0x66, 0x99, 0x3c, 0x7c, 0x05, 0x4a, 0xc3, 0x4b, 0x7c, 0x10, 0xc1, 0xd0, 0xd6,
                0xa3, 0x82, 0x7d, 0x39,
            ]
        );
    }

    #[test]
    fn vue_custom_elements_are_sorted_and_deduplicated_before_fingerprinting() {
        let unordered = vue_grammar_config("{{", "}}", ["z-", "a-", "z-"]).unwrap();
        let ordered = vue_grammar_config("{{", "}}", ["a-", "z-"]).unwrap();
        assert_eq!(unordered.parse_options().custom_elements, ["a-", "z-"]);
        assert_eq!(fingerprint(unordered), fingerprint(ordered));
    }

    #[test]
    fn empty_vue_delimiters_are_rejected() {
        assert_eq!(
            vue_grammar_config("", "}}", std::iter::empty::<&str>()).unwrap_err(),
            VueGrammarConfigError::EmptyDelimiter
        );
        assert_eq!(
            vue_grammar_config("{{", "", std::iter::empty::<&str>()).unwrap_err(),
            VueGrammarConfigError::EmptyDelimiter
        );
    }
}
