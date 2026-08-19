//! Leader-private carrier persistence/adoption substrate.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::carrier_grammar::AcceptedRegisteredCarrierSource;
use verter_language::registered_source_authority::{RegisteredSourceSnapshotId, WholeSourceHash};
use verter_language::FileLanguage;

use crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort;

use super::{FrameworkArtifactId, PersistentAdoptionRejection};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PersistentCarrierKey {
    source_hash: WholeSourceHash,
    language: FileLanguage,
    grammar_fingerprint: verter_language::carrier_grammar::CarrierGrammarFingerprint,
    parse_key: verter_language::ParseKey,
    build_toolchain_fingerprint: crate::build_toolchain_fingerprint::BuildToolchainFingerprint,
}

impl PersistentCarrierKey {
    fn new(id: &FrameworkArtifactId, accepted: &AcceptedRegisteredCarrierSource) -> Self {
        Self {
            source_hash: accepted.source().content_hash(),
            language: accepted.source().resolved_file_language().clone(),
            grammar_fingerprint: accepted.grammar().fingerprint(),
            parse_key: id.parse_key.clone(),
            build_toolchain_fingerprint:
                crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint(),
        }
    }
}

pub(crate) struct PersistedCarrierCandidate {
    pub(crate) cohort: PersistedCarrierArtifactCohort,
    source_hash: WholeSourceHash,
    language: FileLanguage,
    grammar_fingerprint: verter_language::carrier_grammar::CarrierGrammarFingerprint,
    source: RegisteredSourceSnapshotId,
    pub(crate) artifact: Arc<FrameworkParseArtifact>,
    checksum: [u8; 32],
}

impl PersistedCarrierCandidate {
    pub(crate) fn validate(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        expected_id: &FrameworkArtifactId,
        expected_cohort: PersistedCarrierArtifactCohort,
    ) -> Result<(), PersistentAdoptionRejection> {
        if self.cohort != expected_cohort {
            return Err(PersistentAdoptionRejection::CohortMismatch);
        }
        if self.grammar_fingerprint != accepted.grammar().fingerprint() {
            return Err(PersistentAdoptionRejection::StableGrammarMismatch);
        }
        if self.source_hash != accepted.source().content_hash()
            || self.language != *accepted.source().resolved_file_language()
            || self.source.content_hash() != accepted.source().content_hash()
        {
            return Err(PersistentAdoptionRejection::SourceFactMismatch);
        }
        if self.checksum != candidate_checksum(&self.artifact, self.cohort) {
            return Err(PersistentAdoptionRejection::ChecksumMismatch);
        }
        if self.artifact.parse_key() != &expected_id.parse_key
            || self.artifact.adapter_id() != &expected_id.adapter_id
            || self.artifact.language_id() != &expected_id.language_id
        {
            return Err(PersistentAdoptionRejection::ParserValidationFailed);
        }
        self.artifact
            .inventory()
            .validate()
            .map_err(|_| PersistentAdoptionRejection::SourceSpaceInvalid)?;
        if self.artifact.carrier_structure_hash()
            != verter_language::compute_carrier_structure_hash(self.artifact.inventory())
        {
            return Err(PersistentAdoptionRejection::ParserValidationFailed);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn corrupt_checksum_for_test(&mut self) {
        self.checksum[0] ^= 0xff;
    }

    #[cfg(test)]
    pub(crate) fn replace_artifact_for_test(&mut self, artifact: Arc<FrameworkParseArtifact>) {
        self.artifact = artifact;
        self.checksum = candidate_checksum(&self.artifact, self.cohort);
    }
}

fn candidate_checksum(
    artifact: &FrameworkParseArtifact,
    cohort: PersistedCarrierArtifactCohort,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"verter.persisted-carrier-candidate.v1\0");
    hasher.update(artifact.carrier_structure_hash().as_bytes());
    hasher.update(artifact.parse_key().digest().as_bytes());
    hasher.update(cohort.build_toolchain_fingerprint().as_bytes());
    for word in [
        cohort.grammar_fingerprint_schema_version().get(),
        cohort.carrier_source_space_schema_version().get(),
        cohort.carrier_source_map_schema_version().get(),
        cohort.carrier_cache_serialization_version().get(),
    ] {
        hasher.update(word.to_le_bytes());
    }
    hasher.finalize().into()
}

pub(crate) trait CarrierPersistence: Send + Sync {
    fn take_candidate(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Option<PersistedCarrierCandidate>;
    fn store_success(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<FrameworkParseArtifact>,
        cohort: PersistedCarrierArtifactCohort,
    );
}

#[derive(Default)]
pub(crate) struct InMemoryCarrierPersistence {
    candidates: Mutex<HashMap<PersistentCarrierKey, PersistedCarrierCandidate>>,
}

impl CarrierPersistence for InMemoryCarrierPersistence {
    fn take_candidate(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Option<PersistedCarrierCandidate> {
        self.candidates
            .lock()
            .ok()?
            .remove(&PersistentCarrierKey::new(id, accepted))
    }

    fn store_success(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<FrameworkParseArtifact>,
        cohort: PersistedCarrierArtifactCohort,
    ) {
        let candidate = PersistedCarrierCandidate {
            cohort,
            source_hash: accepted.source().content_hash(),
            language: accepted.source().resolved_file_language().clone(),
            grammar_fingerprint: accepted.grammar().fingerprint(),
            source: accepted.source().snapshot_id().clone(),
            artifact: Arc::clone(artifact),
            checksum: candidate_checksum(artifact, cohort),
        };
        if let Ok(mut candidates) = self.candidates.lock() {
            candidates.insert(PersistentCarrierKey::new(id, accepted), candidate);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
        FrameworkAdapterSemanticVersion,
    };
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };

    #[test]
    fn framework_and_persistent_ids_carry_parse_and_build_identity() {
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let language = FileLanguage::vue();
        let config = CarrierGrammarConfig::vue("{{", "}}", ["fixture-box"]).unwrap();
        grammar_authority
            .register_carrier_grammar(
                language.clone(),
                FrameworkAdapterSemanticVersion::new(1).unwrap(),
                CarrierParserGrammarVersion::new(1).unwrap(),
                config.clone(),
            )
            .unwrap();
        let source = source_authority
            .register_source(
                CanonicalFileId::new("file:///Fixture.vue"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                language,
                Arc::from("<template><fixture-box /></template>"),
            )
            .unwrap();
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &source, &config)
            .unwrap();
        let parse_key = super::super::parse_key_for_accepted(&accepted);
        let id = FrameworkArtifactId::derive(&accepted, parse_key.clone());
        let persistent = PersistentCarrierKey::new(&id, &accepted);

        assert_eq!(id.parse_key, parse_key);
        assert_eq!(persistent.parse_key, parse_key);
        assert_eq!(
            persistent.build_toolchain_fingerprint,
            crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
        );
        assert_eq!(
            persistent.language,
            *accepted.source().resolved_file_language()
        );
        assert_eq!(
            persistent.grammar_fingerprint,
            accepted.grammar().fingerprint()
        );
    }
}
