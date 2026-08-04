//! Leader-private carrier persistence/adoption substrate.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use verter_language::carrier_grammar::AcceptedRegisteredCarrierSource;
use verter_language::carrier_versions::{
    CarrierParserVersion, FrameworkParseArtifactSchemaVersion,
};
use verter_language::registered_source_authority::{RegisteredSourceSnapshotId, WholeSourceHash};
use verter_language::{FileLanguage, FrameworkParseArtifact};

use crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort;

use super::{FrameworkArtifactId, PersistentAdoptionRejection};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PersistentCarrierKey {
    source_hash: WholeSourceHash,
    language: FileLanguage,
    grammar_fingerprint: verter_language::carrier_grammar::CarrierGrammarFingerprint,
    parser_version: CarrierParserVersion,
    artifact_schema_version: FrameworkParseArtifactSchemaVersion,
}

impl PersistentCarrierKey {
    fn new(id: &FrameworkArtifactId, accepted: &AcceptedRegisteredCarrierSource) -> Self {
        Self {
            source_hash: accepted.source().content_hash(),
            language: accepted.source().resolved_file_language().clone(),
            grammar_fingerprint: accepted.grammar().fingerprint(),
            parser_version: id.carrier_parser_version,
            artifact_schema_version: id.artifact_schema_version,
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
        self.artifact
            .common
            .inventory
            .validate()
            .map_err(|_| PersistentAdoptionRejection::SourceSpaceInvalid)?;
        if self.artifact.carrier_structure_hash
            != verter_language::compute_carrier_structure_hash(&self.artifact.common.inventory)
        {
            return Err(PersistentAdoptionRejection::ParserValidationFailed);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn corrupt_checksum_for_test(&mut self) {
        self.checksum[0] ^= 0xff;
    }
}

fn candidate_checksum(
    artifact: &FrameworkParseArtifact,
    cohort: PersistedCarrierArtifactCohort,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"verter.persisted-carrier-candidate.v1\0");
    hasher.update(artifact.carrier_structure_hash.as_bytes());
    for word in [
        cohort.vue_parser_version().get(),
        cohort.svelte_parser_version().get(),
        cohort.grammar_fingerprint_schema_version().get(),
        cohort.framework_artifact_schema_version().get(),
        cohort.carrier_source_space_schema_version().get(),
        cohort.carrier_source_map_schema_version().get(),
        cohort.session_current_parser_version().get(),
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
