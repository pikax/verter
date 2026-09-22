//! Leader-private retained store of immutable carrier stable units.
//!
//! A carrier parse artifact is an immutable STABLE UNIT: what makes it the
//! same unit is the content/grammar/parse identity of the source
//! ([`StableUnitKey`]), never the version-bearing registered-source snapshot
//! it happened to be parsed from. [`FrameworkArtifactId`] is snapshot-bound by
//! design — it names one publication of one registered generation — so it is
//! not the reuse identity, and the key below deliberately drops the
//! incarnation/generation axes it carries.
//!
//! Reusing a unit is therefore a READ. Retrieval never consumes the retained
//! entry, so a source that keeps arriving at new generations with unchanged
//! bytes (undo/redo, save-with-no-change, workspace re-scan) parses once and
//! every later generation adopts. A unit that fails validation is explicitly
//! [`CarrierStableUnitStore::discard`]ed rather than being silently dropped by
//! the read, so a rejected unit is never re-offered and a good one is never
//! lost to the first reader.
//!
//! Retention is bounded: [`InMemoryStableUnitStore`] holds at most a
//! caller-chosen number of units and evicts the least recently used one. An
//! evicted unit is re-parsed on its next visit — it is never served stale.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::carrier_grammar::AcceptedRegisteredCarrierSource;
use verter_language::registered_source_authority::{RegisteredSourceSnapshotId, WholeSourceHash};
use verter_language::FileLanguage;

use crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort;

use super::{FrameworkArtifactId, PersistentAdoptionRejection};

/// Identity of one immutable carrier stable unit.
///
/// Every axis here is a property of the bytes and of how they are parsed. The
/// registered snapshot's `file_incarnation`/`generation` are intentionally
/// absent: two generations of identical bytes are two publications of ONE
/// unit, and keying on them would re-parse every revisit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StableUnitKey {
    source_hash: WholeSourceHash,
    language: FileLanguage,
    grammar_fingerprint: verter_language::carrier_grammar::CarrierGrammarFingerprint,
    parse_key: verter_language::ParseKey,
    build_toolchain_fingerprint: crate::build_toolchain_fingerprint::BuildToolchainFingerprint,
}

impl StableUnitKey {
    pub(crate) fn new(
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Self {
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

/// A retained immutable stable unit, handed out by reference-counted clone.
///
/// Holding one does not remove it from the store: a second reader of the same
/// unit gets the same artifact.
pub(crate) struct RetainedStableUnit {
    pub(crate) cohort: PersistedCarrierArtifactCohort,
    source_hash: WholeSourceHash,
    language: FileLanguage,
    grammar_fingerprint: verter_language::carrier_grammar::CarrierGrammarFingerprint,
    /// The registered snapshot this unit was PARSED from. Later adopters ride
    /// their own snapshot; this field is validation evidence about the bytes,
    /// never the provenance a consumer sees.
    origin_source: RegisteredSourceSnapshotId,
    pub(crate) artifact: Arc<FrameworkParseArtifact>,
    checksum: [u8; 32],
}

impl Clone for RetainedStableUnit {
    fn clone(&self) -> Self {
        Self {
            cohort: self.cohort,
            source_hash: self.source_hash,
            language: self.language.clone(),
            grammar_fingerprint: self.grammar_fingerprint,
            origin_source: self.origin_source.clone(),
            artifact: Arc::clone(&self.artifact),
            checksum: self.checksum,
        }
    }
}

impl RetainedStableUnit {
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
            || self.origin_source.content_hash() != accepted.source().content_hash()
        {
            return Err(PersistentAdoptionRejection::SourceFactMismatch);
        }
        if self.checksum != unit_checksum(&self.artifact, self.cohort) {
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
        self.checksum = unit_checksum(&self.artifact, self.cohort);
    }
}

fn unit_checksum(
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

/// The single reuse authority for immutable carrier stable units.
///
/// [`Self::retained`] is a non-consuming read; removal happens only through
/// [`Self::discard`] (the unit failed validation) or through the
/// implementation's own bounded-retention eviction.
pub(crate) trait CarrierStableUnitStore: Send + Sync {
    fn retained(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Option<RetainedStableUnit>;
    fn retain(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<FrameworkParseArtifact>,
        cohort: PersistedCarrierArtifactCohort,
    );
    fn discard(&self, id: &FrameworkArtifactId, accepted: &AcceptedRegisteredCarrierSource);
}

/// Bounded least-recently-used retention of immutable stable units.
///
/// `capacity` is a unit COUNT, not a byte budget: a unit's cost is one
/// already-parsed carrier artifact, and the store's job is to keep an edit
/// session's revisit window addressable without growing once per source
/// generation ever seen. A capacity of zero retains nothing.
struct BoundedUnits {
    capacity: usize,
    units: HashMap<StableUnitKey, RetainedStableUnit>,
    /// Least-recently-used first. Holds exactly the keys of `units`.
    recency: VecDeque<StableUnitKey>,
}

impl BoundedUnits {
    fn touch(&mut self, key: &StableUnitKey) {
        if let Some(position) = self.recency.iter().position(|held| held == key) {
            let key = self
                .recency
                .remove(position)
                .expect("position came from this deque");
            self.recency.push_back(key);
        }
    }

    fn insert(&mut self, key: StableUnitKey, unit: RetainedStableUnit) {
        if self.capacity == 0 {
            return;
        }
        if self.units.insert(key.clone(), unit).is_some() {
            self.touch(&key);
            return;
        }
        self.recency.push_back(key);
        while self.recency.len() > self.capacity {
            let Some(evicted) = self.recency.pop_front() else {
                break;
            };
            self.units.remove(&evicted);
        }
    }

    fn remove(&mut self, key: &StableUnitKey) {
        if self.units.remove(key).is_some() {
            if let Some(position) = self.recency.iter().position(|held| held == key) {
                self.recency.remove(position);
            }
        }
    }
}

pub(crate) struct InMemoryStableUnitStore {
    units: Mutex<BoundedUnits>,
}

impl InMemoryStableUnitStore {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            units: Mutex::new(BoundedUnits {
                capacity,
                units: HashMap::new(),
                recency: VecDeque::new(),
            }),
        }
    }
}

impl Default for InMemoryStableUnitStore {
    fn default() -> Self {
        Self::with_capacity(super::DEFAULT_STABLE_UNIT_RETENTION)
    }
}

impl CarrierStableUnitStore for InMemoryStableUnitStore {
    fn retained(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
    ) -> Option<RetainedStableUnit> {
        let key = StableUnitKey::new(id, accepted);
        let mut units = self.units.lock().ok()?;
        let unit = units.units.get(&key).cloned()?;
        units.touch(&key);
        Some(unit)
    }

    fn retain(
        &self,
        id: &FrameworkArtifactId,
        accepted: &AcceptedRegisteredCarrierSource,
        artifact: &Arc<FrameworkParseArtifact>,
        cohort: PersistedCarrierArtifactCohort,
    ) {
        let unit = RetainedStableUnit {
            cohort,
            source_hash: accepted.source().content_hash(),
            language: accepted.source().resolved_file_language().clone(),
            grammar_fingerprint: accepted.grammar().fingerprint(),
            origin_source: accepted.source().snapshot_id().clone(),
            artifact: Arc::clone(artifact),
            checksum: unit_checksum(artifact, cohort),
        };
        if let Ok(mut units) = self.units.lock() {
            units.insert(StableUnitKey::new(id, accepted), unit);
        }
    }

    fn discard(&self, id: &FrameworkArtifactId, accepted: &AcceptedRegisteredCarrierSource) {
        if let Ok(mut units) = self.units.lock() {
            units.remove(&StableUnitKey::new(id, accepted));
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
    fn framework_and_stable_unit_ids_carry_parse_and_build_identity() {
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
        let unit_key = StableUnitKey::new(&id, &accepted);

        assert_eq!(id.parse_key, parse_key);
        assert_eq!(unit_key.parse_key, parse_key);
        assert_eq!(
            unit_key.build_toolchain_fingerprint,
            crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
        );
        assert_eq!(
            unit_key.language,
            *accepted.source().resolved_file_language()
        );
        assert_eq!(
            unit_key.grammar_fingerprint,
            accepted.grammar().fingerprint()
        );
    }

    /// The reuse key drops the snapshot's incarnation/generation axes: two
    /// registered generations of identical bytes address ONE stable unit.
    #[test]
    fn two_generations_of_identical_bytes_share_one_stable_unit_key() {
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let language = FileLanguage::vue();
        let config = CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap();
        grammar_authority
            .register_carrier_grammar(
                language.clone(),
                FrameworkAdapterSemanticVersion::new(1).unwrap(),
                CarrierParserGrammarVersion::new(1).unwrap(),
                config.clone(),
            )
            .unwrap();
        let bytes: Arc<str> = Arc::from("<template><p>same</p></template>");
        let mut keys = Vec::new();
        let mut artifact_ids = Vec::new();
        for generation in 1..=2u64 {
            let source = source_authority
                .register_source(
                    CanonicalFileId::new("file:///Fixture.vue"),
                    FileIncarnation::new(1),
                    SourceGeneration::new(generation),
                    language.clone(),
                    Arc::clone(&bytes),
                )
                .unwrap();
            let accepted = grammar_authority
                .accept_registered_source(&source_authority, &source, &config)
                .unwrap();
            let id = FrameworkArtifactId::derive(
                &accepted,
                super::super::parse_key_for_accepted(&accepted),
            );
            keys.push(StableUnitKey::new(&id, &accepted));
            artifact_ids.push(id);
        }

        assert_ne!(
            artifact_ids[0], artifact_ids[1],
            "publication identity stays snapshot-bound"
        );
        assert_eq!(keys[0], keys[1], "reuse identity is the stable unit");
    }
}
