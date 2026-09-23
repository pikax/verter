//! Leader-private carrier persistence/adoption substrate.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::carrier_grammar::AcceptedRegisteredCarrierSource;
use verter_language::registered_source_authority::{
    CanonicalIdentityDigest, RegisteredSourceSnapshotId, WholeSourceHash,
};
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
    /// Number of parse candidates currently retained. A persistence that
    /// does not retain in memory reports zero.
    fn retained_candidate_count(&self) -> usize {
        0
    }
}

/// How many persisted parse candidates ONE canonical file may keep.
///
/// A candidate is a complete carrier parse retained so that a later
/// publication of the SAME content adopts it instead of parsing again. The
/// contents worth adopting are the ones an editor comes back to — the version
/// on disk after the buffer closes, the version an undo restores — and those
/// are the newest one or two the file has published. Every version ever typed
/// is not: keyed by content alone, an edit loop would persist one full parse
/// (arena, geometry and all) per keystroke for the life of the session, since
/// nothing but an adoption of that exact content ever takes an entry out.
/// Retention is therefore per canonical file, newest first: publishing a new
/// content drops the oldest candidate of the same file past this window.
/// Other files' candidates are untouched — the bound is per file so a large
/// workspace does not evict one file's disk version because another file was
/// edited.
const PERSISTED_CANDIDATES_PER_CANONICAL: usize = 2;

#[derive(Default)]
struct InMemoryCandidates {
    by_key: HashMap<PersistentCarrierKey, PersistedCarrierCandidate>,
    /// Every retained key per canonical file, oldest first, so the per-file
    /// window can drop the oldest without scanning the whole map.
    by_canonical: HashMap<CanonicalIdentityDigest, VecDeque<PersistentCarrierKey>>,
}

impl InMemoryCandidates {
    fn take(
        &mut self,
        canonical: CanonicalIdentityDigest,
        key: &PersistentCarrierKey,
    ) -> Option<PersistedCarrierCandidate> {
        let candidate = self.by_key.remove(key)?;
        if let Some(keys) = self.by_canonical.get_mut(&canonical) {
            keys.retain(|retained| retained != key);
            if keys.is_empty() {
                self.by_canonical.remove(&canonical);
            }
        }
        Some(candidate)
    }

    fn store(
        &mut self,
        canonical: CanonicalIdentityDigest,
        key: PersistentCarrierKey,
        candidate: PersistedCarrierCandidate,
    ) {
        let keys = self.by_canonical.entry(canonical).or_default();
        // Re-storing a content the file already holds refreshes its place in
        // the window rather than counting it twice.
        keys.retain(|retained| retained != &key);
        keys.push_back(key.clone());
        self.by_key.insert(key, candidate);
        while keys.len() > PERSISTED_CANDIDATES_PER_CANONICAL {
            if let Some(oldest) = keys.pop_front() {
                self.by_key.remove(&oldest);
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct InMemoryCarrierPersistence {
    candidates: Mutex<InMemoryCandidates>,
}

impl CarrierPersistence for InMemoryCarrierPersistence {
    /// Across every canonical file: the object-count half of the store's
    /// retention bound.
    fn retained_candidate_count(&self) -> usize {
        self.candidates
            .lock()
            .map(|candidates| candidates.by_key.len())
            .unwrap_or(0)
    }

    fn take_candidate(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Option<PersistedCarrierCandidate> {
        self.candidates.lock().ok()?.take(
            accepted.source().snapshot_id().canonical_digest(),
            &PersistentCarrierKey::new(id, accepted),
        )
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
            candidates.store(
                accepted.source().snapshot_id().canonical_digest(),
                PersistentCarrierKey::new(id, accepted),
                candidate,
            );
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

    /// Registers `bytes` as generation `generation` of `canonical` and
    /// accepts it under `config`.
    fn fixture_accepted(
        source_authority: &RegisteredSourceAuthority,
        grammar_authority: &CarrierGrammarAuthority,
        config: &CarrierGrammarConfig,
        canonical: &str,
        generation: u64,
        bytes: &str,
    ) -> AcceptedRegisteredCarrierSource {
        let source = source_authority
            .register_source(
                CanonicalFileId::new(canonical),
                FileIncarnation::new(1),
                SourceGeneration::new(generation),
                FileLanguage::vue(),
                Arc::from(bytes),
            )
            .unwrap();
        grammar_authority
            .accept_registered_source(source_authority, &source, config)
            .unwrap()
    }

    /// Per-file candidate window: `store_success` keeps the newest
    /// `PERSISTED_CANDIDATES_PER_CANONICAL` contents of ONE canonical file,
    /// `take_candidate` un-indexes what it takes, another file's candidates
    /// are unaffected, and re-storing a held content refreshes its place
    /// rather than counting it twice.
    ///
    /// On the unbounded code (a plain content-keyed map) the count after
    /// three stores is 3 and the oldest content is still there to take. With
    /// a window whose `take` did NOT un-index, the store after the take would
    /// evict the live `a2` (count 2, not 3, at that step).
    #[test]
    fn in_memory_candidates_keep_the_newest_two_per_canonical_file() {
        let persistence = InMemoryCarrierPersistence::default();
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let config = CarrierGrammarConfig::vue("{{", "}}", ["fixture-box"]).unwrap();
        grammar_authority
            .register_carrier_grammar(
                FileLanguage::vue(),
                FrameworkAdapterSemanticVersion::new(1).unwrap(),
                CarrierParserGrammarVersion::new(1).unwrap(),
                config.clone(),
            )
            .unwrap();
        let cohort = crate::carrier_artifact_cohort::current_persisted_carrier_artifact_cohort();
        let accepted = |canonical: &str, generation: u64, bytes: &str| {
            fixture_accepted(
                &source_authority,
                &grammar_authority,
                &config,
                canonical,
                generation,
                bytes,
            )
        };
        let store = |accepted: &AcceptedRegisteredCarrierSource| {
            let id = FrameworkArtifactId::derive(
                accepted,
                super::super::parse_key_for_accepted(accepted),
            );
            let artifact = Arc::new(
                verter_compiler::framework_common::registered_carrier_projection::project_registered_accepted(
                    accepted,
                )
                .expect("fixture parses")
                .into_framework_parse_artifact(),
            );
            persistence.store_success(&id, accepted, &artifact, cohort);
            id
        };

        let a1 = accepted("file:///A.vue", 1, "<template><p>a1</p></template>");
        let a2 = accepted("file:///A.vue", 2, "<template><p>a2</p></template>");
        let a3 = accepted("file:///A.vue", 3, "<template><p>a3</p></template>");
        let a1_id = store(&a1);
        let a2_id = store(&a2);
        let a3_id = store(&a3);
        assert_eq!(persistence.retained_candidate_count(), 2);
        assert!(
            persistence.take_candidate(&a1_id, &a1).is_none(),
            "the oldest content of the file is past the window"
        );
        assert_eq!(persistence.retained_candidate_count(), 2);

        let b1 = accepted("file:///B.vue", 1, "<template><p>b1</p></template>");
        let b1_id = store(&b1);
        assert_eq!(
            persistence.retained_candidate_count(),
            3,
            "the window is per canonical file"
        );

        assert!(persistence.take_candidate(&a3_id, &a3).is_some());
        assert_eq!(persistence.retained_candidate_count(), 2);
        let a4 = accepted("file:///A.vue", 4, "<template><p>a4</p></template>");
        let a4_id = store(&a4);
        assert_eq!(
            persistence.retained_candidate_count(),
            3,
            "taking a3 un-indexed it, so storing a4 keeps a2 (a2, a4, b1)"
        );
        assert!(persistence.take_candidate(&a2_id, &a2).is_some());
        assert!(persistence.take_candidate(&a4_id, &a4).is_some());
        assert!(persistence.take_candidate(&b1_id, &b1).is_some());
        assert_eq!(persistence.retained_candidate_count(), 0);

        // Re-storing a content the file already holds refreshes it in place.
        store(&a4);
        store(&a4);
        assert_eq!(persistence.retained_candidate_count(), 1);
        assert!(persistence.take_candidate(&a4_id, &a4).is_some());
        assert!(persistence.take_candidate(&a4_id, &a4).is_none());
    }
}
