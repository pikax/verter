//! Population substitution at the augmentation stitch's recorded boundary.
//! Child caches keep their complete evidence; only the consuming read changes.

use std::sync::Arc;
use verter_session_query::analysis::types::Hash16;
use verter_session_query::facts::fact_cache::{FactVersionRef, ParseEnvHash, RouteSurfaceFactRef};
use verter_session_query::facts::fact_read_set::NonCacheablePropagation;
use verter_session_query::facts::policy::FactPolicy;
use verter_session_query::inputs::contributors::SymbolContributors;
use verter_session_query::resolution::{AugmentationTargetKind, AugmenterSet};
use verter_session_query::source::artifact_key::FileArtifactKey;
use verter_session_query::source::env_identity::SourceEnvIdentity;

use crate::project_semantic_dispatch::BuildLocalTaint;
use crate::resolver_core::resolver_context::{
    record_fact_reads, with_delivering_fact_tracer_cell, with_fact_tracer_cell,
    without_fact_observers, RecordedFactReads,
};

/// Exact immutable publications that cover one stitch's source observations.
/// Both raw source arrays are canonical-ordered, so coverage borrows their
/// existing keys without copying or retaining a second source inventory.
pub(super) struct PopulationEvidence {
    pub(super) fact: FactVersionRef,
    augmenters: Arc<AugmenterSet>,
    contributors: SymbolContributors,
}

impl PopulationEvidence {
    pub(super) fn new(
        target: &AugmentationTargetKind,
        name: &str,
        attribution: Arc<str>,
        augmenters: Arc<AugmenterSet>,
        contributors: SymbolContributors,
        contributor_content: Hash16,
    ) -> Self {
        use verter_session_query::source::augmentation_keys::{
            build_augmentation_population_content_fact_key, join_augmentation_population_content,
        };
        let fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: attribution,
            key: build_augmentation_population_content_fact_key(target, name),
            lane: verter_session_query::facts::FactLane::Semantic,
            expected_hash: join_augmentation_population_content(
                augmenters.content_fingerprint,
                contributor_content,
            ),
        });
        Self {
            fact,
            augmenters,
            contributors,
        }
    }

    fn source(&self, canonical: &str) -> Option<(&FileArtifactKey, Hash16)> {
        let augmenter = self
            .augmenters
            .entries
            .binary_search_by(|entry| entry.artifact_key.canonical.as_ref().cmp(canonical))
            .ok()
            .map(|i| {
                let entry = &self.augmenters.entries[i];
                (&entry.artifact_key, entry.source_parse_env_hash)
            });
        let contributor = self
            .contributors
            .entries
            .binary_search_by(|entry| entry.artifact_key.canonical.as_ref().cmp(canonical))
            .ok()
            .map(|i| {
                let entry = &self.contributors.entries[i];
                (&entry.artifact_key, entry.source_parse_env_hash)
            });
        if let (Some(left), Some(right)) = (augmenter, contributor) {
            if left != right {
                return None;
            }
        }
        augmenter.or(contributor)
    }

    pub(super) fn covers_source(&self, key: &FileArtifactKey, env: &SourceEnvIdentity) -> bool {
        self.source(&key.canonical)
            .is_some_and(|(published, parse_env)| {
                published == key
                    && env.parse_env_hash == ParseEnvHash::from_env_hash(parse_env)
                    && env.parse_key == key.parse_key
                    && env.file_language_id == key.file_language_id
            })
    }

    /// Replace only exact covered whole-content/source-env observations.
    /// Conflicts and evidence outside the population survive; receipts and
    /// every nested cache's own carrier are untouched.
    pub(super) fn substitute(
        &self,
        reads: &mut RecordedFactReads,
        observed: &mut BuildLocalTaint,
        policy: FactPolicy,
    ) {
        let mut conflict = false;
        let mut facts: Vec<_> = reads
            .facts
            .iter()
            .filter(|fact| {
                let matches = match fact {
                    FactVersionRef::FileWholeHash { canonical_id, hash } => self
                        .source(canonical_id)
                        .map(|(key, _)| key.content_hash == *hash),
                    FactVersionRef::FileSourceEnv {
                        canonical_id,
                        parse_env_hash,
                        parse_key,
                        file_language_id,
                    } => self.source(canonical_id).map(|(key, parse_env)| {
                        *parse_env_hash == ParseEnvHash::from_env_hash(parse_env)
                            && *parse_key == key.parse_key
                            && *file_language_id == key.file_language_id
                    }),
                    _ => None,
                };
                match matches {
                    Some(true) => false,
                    Some(false) => {
                        conflict = true;
                        true
                    }
                    None => true,
                }
            })
            .cloned()
            .collect();
        facts.push(self.fact.clone());
        reads.facts = policy.set_of(facts);
        observed.observed_self_roots.retain(|(canonical, hash)| {
            match self.source(canonical.as_str()) {
                Some((key, _)) if key.content_hash == *hash => false,
                Some(_) => {
                    conflict = true;
                    true
                }
                None => true,
            }
        });
        if conflict {
            reads.non_cacheable = true;
            observed.cache_suppress = true;
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnobservableSource,
            );
        }
    }
}

/// Capture before substitution, without changing what any nested cache reads.
/// The ordinary outer sentinel receives transitive refusal marks, while the
/// delivering inner scope receives facts and direct local-only marks. The
/// passive recording also preserves nested local-only marks for transaction
/// reuse, without turning a child's declined retention into a parent refusal.
pub(super) fn record<R>(policy: FactPolicy, work: impl FnOnce() -> R) -> (R, RecordedFactReads) {
    let (((value, reads), inner), outer) = without_fact_observers(|| {
        with_fact_tracer_cell(Default::default(), policy, |_| {
            with_delivering_fact_tracer_cell(Default::default(), policy, |_| {
                record_fact_reads(policy, work)
            })
        })
    });
    let transitive = outer.non_cacheable_read_observed();
    let local = inner.non_cacheable_read_observed();
    if transitive {
        if outer.non_cacheable_only_declined_retention() {
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnretainedResolution,
            );
        } else {
            crate::fact_tracing::note_non_cacheable_propagation(
                NonCacheablePropagation::Transitive,
            );
        }
    } else if local {
        crate::fact_tracing::note_non_cacheable_propagation(NonCacheablePropagation::LocalOnly);
    } else if reads.non_cacheable {
        // Only a nested owner declined retention: passive outer recordings
        // must see the mark, and no enclosing cacheability tracer may see it.
        let _ = with_fact_tracer_cell(Default::default(), policy, |_| {
            crate::fact_tracing::note_non_cacheable_propagation(NonCacheablePropagation::LocalOnly);
        });
    }
    (value, reads)
}
