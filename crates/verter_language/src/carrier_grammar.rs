//! Canonical carrier-grammar registry and joint source/grammar acceptance.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use crate::registered_source_authority::{
    random_bytes, sha256, RegisteredSourceAuthority, RegisteredSourceSnapshot,
    SourceValidationError,
};
use crate::{FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions};

macro_rules! nonzero_version {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u32);

        impl $name {
            pub const fn new(value: u32) -> Option<Self> {
                if value == 0 {
                    None
                } else {
                    Some(Self(value))
                }
            }

            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

nonzero_version!(FrameworkAdapterSemanticVersion);
nonzero_version!(CarrierParserGrammarVersion);
nonzero_version!(CarrierGrammarFingerprintSchemaVersion);

/// Current stable grammar-fingerprint encoding.
pub const CARRIER_GRAMMAR_FINGERPRINT_SCHEMA_VERSION: CarrierGrammarFingerprintSchemaVersion =
    CarrierGrammarFingerprintSchemaVersion(1);

/// Random identity of one grammar registry lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_retention::heap_size::HeapSize)]
pub struct GrammarAuthorityNamespaceId([u8; 16]);

impl GrammarAuthorityNamespaceId {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Stable semantic SHA-256 identity of a canonical carrier grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_retention::heap_size::HeapSize)]
pub struct CarrierGrammarFingerprint([u8; 32]);

impl CarrierGrammarFingerprint {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// One carrier's canonical grammar configuration.
///
/// The carrier's owner builds it — the language crate names no framework:
/// the owning carrier language, the owner's canonical encoding (the bytes the
/// grammar fingerprint covers) and the parse options the owner's frontend
/// parses under. Equality is the owner and the canonical bytes; the parse
/// options are the same input seen by the parser, built in the same owner
/// function.
#[derive(Debug, Clone)]
pub struct CarrierGrammarConfig {
    owner: FileLanguage,
    canonical: Arc<[u8]>,
    parse_options: ParseOptions,
}

impl PartialEq for CarrierGrammarConfig {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && self.canonical == other.canonical
    }
}

impl Eq for CarrierGrammarConfig {}

impl std::hash::Hash for CarrierGrammarConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.owner.hash(state);
        self.canonical.hash(state);
    }
}

impl CarrierGrammarConfig {
    /// A configuration built by the carrier's owner: its canonical encoding
    /// and the parse options its frontend uses.
    #[must_use]
    pub fn from_owner(
        owner: FileLanguage,
        canonical: CarrierGrammarEncoder,
        parse_options: ParseOptions,
    ) -> Self {
        Self {
            owner,
            canonical: canonical.0.into(),
            parse_options,
        }
    }

    /// The carrier language the configuration belongs to.
    #[must_use]
    pub fn owner(&self) -> &FileLanguage {
        &self.owner
    }

    /// The parse options the owner's frontend parses this carrier under.
    #[must_use]
    pub fn parse_options(&self) -> &ParseOptions {
        &self.parse_options
    }
}

/// The canonical encoding an owner writes for its grammar configuration:
/// a tag byte, then length-prefixed strings and counts.
#[derive(Debug, Clone)]
pub struct CarrierGrammarEncoder(Vec<u8>);

impl CarrierGrammarEncoder {
    /// Start an encoding with the owner's configuration tag.
    #[must_use]
    pub fn new(tag: u8) -> Self {
        Self(vec![tag])
    }

    /// Append one length-prefixed string.
    pub fn string(&mut self, value: &str) {
        push_string(&mut self.0, value);
    }

    /// Append one length prefix (the count of the items that follow).
    pub fn count(&mut self, len: usize) {
        push_len(&mut self.0, len);
    }
}

/// Authority-minted canonical registration. All fields remain private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalCarrierGrammar {
    authority: GrammarAuthorityNamespaceId,
    adapter_id: FrameworkAdapterId,
    adapter_semantic_version: FrameworkAdapterSemanticVersion,
    language_id: LanguageId,
    parser_grammar_version: CarrierParserGrammarVersion,
    canonical_config: CarrierGrammarConfig,
    fingerprint: CarrierGrammarFingerprint,
}

impl CanonicalCarrierGrammar {
    pub fn authority(&self) -> GrammarAuthorityNamespaceId {
        self.authority
    }

    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.adapter_id
    }

    pub fn adapter_semantic_version(&self) -> FrameworkAdapterSemanticVersion {
        self.adapter_semantic_version
    }

    pub fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    pub fn parser_grammar_version(&self) -> CarrierParserGrammarVersion {
        self.parser_grammar_version
    }

    pub fn canonical_config(&self) -> &CarrierGrammarConfig {
        &self.canonical_config
    }

    pub fn fingerprint(&self) -> CarrierGrammarFingerprint {
        self.fingerprint
    }
}

/// Sealed result of one atomic current-source/current-registration check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRegisteredCarrierSource {
    source: RegisteredSourceSnapshot,
    grammar: CanonicalCarrierGrammar,
}

impl AcceptedRegisteredCarrierSource {
    pub fn source(&self) -> &RegisteredSourceSnapshot {
        &self.source
    }

    pub fn grammar(&self) -> &CanonicalCarrierGrammar {
        &self.grammar
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarRegistrationError {
    AuthorityUnavailable,
    UnsupportedCarrierLanguage,
    ConfigLanguageMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierAcceptanceError {
    Source(SourceValidationError),
    AuthorityUnavailable,
    NoRegisteredGrammar,
    GrammarAuthorityNamespaceMismatch,
    GrammarLanguageMismatch,
    GrammarConfigMismatch,
    GrammarFingerprintMismatch,
}

/// Sole live mint and registry for canonical carrier grammars.
#[derive(Debug)]
pub struct CarrierGrammarAuthority {
    namespace: GrammarAuthorityNamespaceId,
    registrations: Mutex<HashMap<FileLanguage, CanonicalCarrierGrammar>>,
}

impl CarrierGrammarAuthority {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            namespace: GrammarAuthorityNamespaceId(random_bytes()?),
            registrations: Mutex::new(HashMap::new()),
        })
    }

    pub fn namespace(&self) -> GrammarAuthorityNamespaceId {
        self.namespace
    }

    /// Canonicalizes and atomically installs the live grammar for one carrier.
    pub fn register_carrier_grammar(
        &self,
        resolved_file_language: FileLanguage,
        adapter_semantic_version: FrameworkAdapterSemanticVersion,
        parser_grammar_version: CarrierParserGrammarVersion,
        config: CarrierGrammarConfig,
    ) -> Result<CanonicalCarrierGrammar, GrammarRegistrationError> {
        let (adapter_id, language_id) = carrier_identity(&resolved_file_language, &config)?;
        let canonical_config = config;
        let fingerprint = grammar_fingerprint(
            &adapter_id,
            adapter_semantic_version,
            &language_id,
            parser_grammar_version,
            &canonical_config,
        );
        let grammar = CanonicalCarrierGrammar {
            authority: self.namespace,
            adapter_id,
            adapter_semantic_version,
            language_id,
            parser_grammar_version,
            canonical_config,
            fingerprint,
        };
        self.registrations
            .lock()
            .map_err(|_| GrammarRegistrationError::AuthorityUnavailable)?
            .insert(resolved_file_language, grammar.clone());
        Ok(grammar)
    }

    /// Atomically validates current source and current grammar registration,
    /// then and only then mints the accepted capability.
    pub fn accept_registered_source(
        &self,
        source_authority: &RegisteredSourceAuthority,
        source: &RegisteredSourceSnapshot,
        config: &CarrierGrammarConfig,
    ) -> Result<AcceptedRegisteredCarrierSource, CarrierAcceptanceError> {
        source_authority
            .with_validated_current(source, || self.accept_validated_source(source, config))
            .map_err(CarrierAcceptanceError::Source)?
    }

    /// Revalidate an existing sealed acceptance against both live authorities
    /// without minting a replacement capability.
    pub fn validate_accepted_current(
        &self,
        source_authority: &RegisteredSourceAuthority,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Result<(), CarrierAcceptanceError> {
        source_authority
            .with_validated_current(accepted.source(), || {
                let registrations = self
                    .registrations
                    .lock()
                    .map_err(|_| CarrierAcceptanceError::AuthorityUnavailable)?;
                let live = registrations
                    .get(accepted.source().resolved_file_language())
                    .ok_or(CarrierAcceptanceError::NoRegisteredGrammar)?;
                if live != accepted.grammar() {
                    return Err(CarrierAcceptanceError::GrammarConfigMismatch);
                }
                Ok(())
            })
            .map_err(CarrierAcceptanceError::Source)?
    }

    fn accept_validated_source(
        &self,
        source: &RegisteredSourceSnapshot,
        config: &CarrierGrammarConfig,
    ) -> Result<AcceptedRegisteredCarrierSource, CarrierAcceptanceError> {
        let registrations = self
            .registrations
            .lock()
            .map_err(|_| CarrierAcceptanceError::AuthorityUnavailable)?;
        let grammar = registrations
            .get(source.resolved_file_language())
            .ok_or(CarrierAcceptanceError::NoRegisteredGrammar)?;
        if grammar.authority != self.namespace {
            return Err(CarrierAcceptanceError::GrammarAuthorityNamespaceMismatch);
        }
        let (adapter_id, language_id) =
            carrier_identity(source.resolved_file_language(), &grammar.canonical_config)
                .map_err(|_| CarrierAcceptanceError::GrammarLanguageMismatch)?;
        if grammar.adapter_id != adapter_id || grammar.language_id != language_id {
            return Err(CarrierAcceptanceError::GrammarLanguageMismatch);
        }
        if grammar.canonical_config != *config {
            return Err(CarrierAcceptanceError::GrammarConfigMismatch);
        }
        let expected_fingerprint = grammar_fingerprint(
            &grammar.adapter_id,
            grammar.adapter_semantic_version,
            &grammar.language_id,
            grammar.parser_grammar_version,
            &grammar.canonical_config,
        );
        if grammar.fingerprint != expected_fingerprint {
            return Err(CarrierAcceptanceError::GrammarFingerprintMismatch);
        }
        Ok(AcceptedRegisteredCarrierSource {
            source: source.clone(),
            grammar: grammar.clone(),
        })
    }
}

fn carrier_identity(
    language: &FileLanguage,
    config: &CarrierGrammarConfig,
) -> Result<(FrameworkAdapterId, LanguageId), GrammarRegistrationError> {
    let (adapter_id, language_id) = match language {
        FileLanguage::Framework {
            adapter_id,
            language_id,
        } => (adapter_id.clone(), language_id.clone()),
        _ => return Err(GrammarRegistrationError::UnsupportedCarrierLanguage),
    };
    if config.owner() != language {
        return Err(GrammarRegistrationError::ConfigLanguageMismatch);
    }
    Ok((adapter_id, language_id))
}

fn grammar_fingerprint(
    adapter_id: &FrameworkAdapterId,
    adapter_semantic_version: FrameworkAdapterSemanticVersion,
    language_id: &LanguageId,
    parser_grammar_version: CarrierParserGrammarVersion,
    config: &CarrierGrammarConfig,
) -> CarrierGrammarFingerprint {
    let mut encoding = Vec::new();
    encoding.extend_from_slice(b"verter.carrier-grammar-fingerprint\0");
    encoding.extend_from_slice(
        &CARRIER_GRAMMAR_FINGERPRINT_SCHEMA_VERSION
            .get()
            .to_be_bytes(),
    );
    push_string(&mut encoding, adapter_id.as_str());
    encoding.extend_from_slice(&adapter_semantic_version.get().to_be_bytes());
    push_string(&mut encoding, language_id.as_str());
    encoding.extend_from_slice(&parser_grammar_version.get().to_be_bytes());
    encoding.extend_from_slice(&config.canonical);
    CarrierGrammarFingerprint(sha256(&[&encoding]))
}

fn push_string(encoding: &mut Vec<u8>, value: &str) {
    push_len(encoding, value.len());
    encoding.extend_from_slice(value.as_bytes());
}

fn push_len(encoding: &mut Vec<u8>, len: usize) {
    let len = u32::try_from(len).expect("canonical grammar field exceeds u32 length");
    encoding.extend_from_slice(&len.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use super::*;
    use crate::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };

    fn version(value: u32) -> CarrierParserGrammarVersion {
        CarrierParserGrammarVersion::new(value).expect("nonzero grammar version")
    }

    fn adapter_version(value: u32) -> FrameworkAdapterSemanticVersion {
        FrameworkAdapterSemanticVersion::new(value).expect("nonzero adapter version")
    }

    /// A fixture configuration for the `.vue` carrier, shaped like its
    /// owner's: the language crate builds none of its own.
    fn vue_config(open: &str, close: &str, custom_elements: &[&str]) -> CarrierGrammarConfig {
        let mut canonical = CarrierGrammarEncoder::new(1);
        canonical.string(open);
        canonical.string(close);
        canonical.count(custom_elements.len());
        for name in custom_elements {
            canonical.string(name);
        }
        CarrierGrammarConfig::from_owner(
            crate::FileLanguage::vue(),
            canonical,
            ParseOptions {
                delimiters: (open.to_string(), close.to_string()),
                custom_elements: custom_elements.iter().map(ToString::to_string).collect(),
                svelte_loose: false,
            },
        )
    }

    /// A fixture configuration for the `.svelte` carrier.
    fn svelte_config() -> CarrierGrammarConfig {
        CarrierGrammarConfig::from_owner(
            crate::FileLanguage::svelte(),
            CarrierGrammarEncoder::new(2),
            ParseOptions::default(),
        )
    }

    fn source() -> (RegisteredSourceAuthority, RegisteredSourceSnapshot) {
        let authority = RegisteredSourceAuthority::new().expect("source authority");
        let snapshot = authority
            .register_source(
                CanonicalFileId::new("file:///workspace/App.vue"),
                FileIncarnation::new(3),
                SourceGeneration::new(9),
                crate::FileLanguage::vue(),
                Arc::from("<template>{{ value }}</template>"),
            )
            .expect("registered source");
        (authority, snapshot)
    }

    #[test]
    fn grammar_fingerprint_is_stable_across_authority_lifetimes() {
        let first = CarrierGrammarAuthority::new().expect("first grammar authority");
        let second = CarrierGrammarAuthority::new().expect("second grammar authority");
        let config = vue_config("{{", "}}", &["x-", "math-"]);
        let first_grammar = first
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(4),
                config.clone(),
            )
            .expect("first registration");
        let second_grammar = second
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(4),
                config,
            )
            .expect("second registration");

        assert_ne!(first_grammar.authority(), second_grammar.authority());
        assert_eq!(first_grammar.fingerprint(), second_grammar.fingerprint());
    }

    #[test]
    fn every_canonical_grammar_input_discriminates_the_fingerprint() {
        let authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let cases = [
            (adapter_version(1), version(1), vue_config("{{", "}}", &[])),
            (adapter_version(2), version(1), vue_config("{{", "}}", &[])),
            (adapter_version(1), version(2), vue_config("{{", "}}", &[])),
            (adapter_version(1), version(1), vue_config("[[", "]]", &[])),
            (
                adapter_version(1),
                version(1),
                vue_config("{{", "}}", &["x-"]),
            ),
        ];
        let fingerprints = cases
            .into_iter()
            .map(|(adapter, parser, config)| {
                authority
                    .register_carrier_grammar(crate::FileLanguage::vue(), adapter, parser, config)
                    .expect("Vue grammar")
                    .fingerprint()
            })
            .collect::<HashSet<_>>();
        assert_eq!(fingerprints.len(), 5);

        let svelte = authority
            .register_carrier_grammar(
                crate::FileLanguage::svelte(),
                adapter_version(1),
                version(1),
                svelte_config(),
            )
            .expect("Svelte grammar")
            .fingerprint();
        assert!(!fingerprints.contains(&svelte));
    }

    #[test]
    fn adapter_and_language_ids_are_independent_fingerprint_inputs() {
        let adapter_version = adapter_version(1);
        let parser_version = version(1);
        let config = svelte_config();
        let base = grammar_fingerprint(
            &crate::FrameworkAdapterId::new("adapter-a"),
            adapter_version,
            &crate::LanguageId::new("language-a"),
            parser_version,
            &config,
        );
        let other_adapter = grammar_fingerprint(
            &crate::FrameworkAdapterId::new("adapter-b"),
            adapter_version,
            &crate::LanguageId::new("language-a"),
            parser_version,
            &config,
        );
        let other_language = grammar_fingerprint(
            &crate::FrameworkAdapterId::new("adapter-a"),
            adapter_version,
            &crate::LanguageId::new("language-b"),
            parser_version,
            &config,
        );
        assert_ne!(base, other_adapter);
        assert_ne!(base, other_language);
    }

    #[test]
    fn a_config_registered_under_another_owners_language_is_refused() {
        let authority = CarrierGrammarAuthority::new().expect("grammar authority");
        assert_eq!(
            authority.register_carrier_grammar(
                crate::FileLanguage::svelte(),
                adapter_version(1),
                version(1),
                vue_config("{{", "}}", &[]),
            ),
            Err(GrammarRegistrationError::ConfigLanguageMismatch)
        );
        assert_eq!(
            authority.register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(1),
                svelte_config(),
            ),
            Err(GrammarRegistrationError::ConfigLanguageMismatch)
        );
    }

    #[test]
    fn valid_current_source_and_registration_mint_accepted_capability() {
        let (source_authority, snapshot) = source();
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let config = vue_config("{{", "}}", &[]);
        let grammar = grammar_authority
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(1),
                config.clone(),
            )
            .expect("Vue grammar registration");
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &snapshot, &config)
            .expect("accepted registered source");

        assert_eq!(accepted.source().snapshot_id(), snapshot.snapshot_id());
        assert_eq!(accepted.grammar().fingerprint(), grammar.fingerprint());
        assert_eq!(accepted.source().bytes(), snapshot.bytes());
    }

    #[test]
    fn svelte_has_its_own_closed_empty_grammar_registration() {
        let source_authority = RegisteredSourceAuthority::new().expect("source authority");
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///workspace/App.svelte"),
                FileIncarnation::new(3),
                SourceGeneration::new(9),
                crate::FileLanguage::svelte(),
                Arc::from("<p>hello</p>"),
            )
            .expect("registered source");
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        grammar_authority
            .register_carrier_grammar(
                crate::FileLanguage::svelte(),
                adapter_version(1),
                version(1),
                svelte_config(),
            )
            .expect("Svelte grammar registration");
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &snapshot, &svelte_config())
            .expect("accepted Svelte source");
        assert_eq!(
            accepted.source().resolved_file_language(),
            &crate::FileLanguage::svelte()
        );
        assert_eq!(accepted.grammar().canonical_config(), &svelte_config());
    }

    #[test]
    fn caller_config_must_equal_the_current_canonical_registration() {
        let (source_authority, snapshot) = source();
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        grammar_authority
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(1),
                vue_config("{{", "}}", &[]),
            )
            .expect("Vue grammar registration");
        assert_eq!(
            grammar_authority.accept_registered_source(
                &source_authority,
                &snapshot,
                &vue_config("[[", "]]", &[]),
            ),
            Err(CarrierAcceptanceError::GrammarConfigMismatch)
        );
    }

    #[test]
    fn unregistered_live_language_is_rejected() {
        let source_authority = RegisteredSourceAuthority::new().expect("source authority");
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///workspace/App.svelte"),
                FileIncarnation::new(3),
                SourceGeneration::new(9),
                crate::FileLanguage::svelte(),
                Arc::from("<p>hello</p>"),
            )
            .expect("registered source");
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        assert_eq!(
            grammar_authority.accept_registered_source(
                &source_authority,
                &snapshot,
                &svelte_config(),
            ),
            Err(CarrierAcceptanceError::NoRegisteredGrammar)
        );
    }

    #[test]
    fn tampered_live_grammar_namespace_is_rejected() {
        let (source_authority, snapshot) = source();
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let config = vue_config("{{", "}}", &[]);
        grammar_authority
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(1),
                config.clone(),
            )
            .expect("Vue grammar registration");
        grammar_authority
            .registrations
            .lock()
            .expect("grammar registry")
            .get_mut(&crate::FileLanguage::vue())
            .expect("Vue grammar")
            .authority = GrammarAuthorityNamespaceId([0xD8; 16]);
        assert_eq!(
            grammar_authority.accept_registered_source(&source_authority, &snapshot, &config),
            Err(CarrierAcceptanceError::GrammarAuthorityNamespaceMismatch)
        );
    }

    #[test]
    fn tampered_live_grammar_fingerprint_is_rejected() {
        let (source_authority, snapshot) = source();
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let config = vue_config("{{", "}}", &[]);
        grammar_authority
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(1),
                config.clone(),
            )
            .expect("Vue grammar registration");
        grammar_authority
            .registrations
            .lock()
            .expect("grammar registry")
            .get_mut(&crate::FileLanguage::vue())
            .expect("Vue grammar")
            .fingerprint = CarrierGrammarFingerprint([0xE9; 32]);
        assert_eq!(
            grammar_authority.accept_registered_source(&source_authority, &snapshot, &config),
            Err(CarrierAcceptanceError::GrammarFingerprintMismatch)
        );
    }

    #[test]
    fn tampered_live_grammar_language_is_rejected() {
        let (source_authority, snapshot) = source();
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let config = vue_config("{{", "}}", &[]);
        grammar_authority
            .register_carrier_grammar(
                crate::FileLanguage::vue(),
                adapter_version(1),
                version(1),
                config.clone(),
            )
            .expect("Vue grammar registration");
        grammar_authority
            .registrations
            .lock()
            .expect("grammar registry")
            .get_mut(&crate::FileLanguage::vue())
            .expect("Vue grammar")
            .language_id = crate::LanguageId::new("svelte");
        assert_eq!(
            grammar_authority.accept_registered_source(&source_authority, &snapshot, &config),
            Err(CarrierAcceptanceError::GrammarLanguageMismatch)
        );
    }
}
