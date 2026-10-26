//! Cold augmentation matching belongs to the source request, outside storage guards.
use crate::file_artifact_store::{
    compute_augmenter_set_fingerprint, emit_module_augmentation_index_shape_event,
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactStore,
};
use smallvec::SmallVec;
use std::sync::Arc;
use verter_session_query::analysis::types::Hash16;
use verter_session_query::resolution::AugmenterEntry;
use verter_session_query::resolution::AugmenterSet;

/// One augmenter target resolution as the cold scan consumes it.
#[derive(Debug, Clone)]
pub(crate) struct AugmentationTarget {
    /// The module file the augmenter's specifier names; `None` skips it.
    pub(crate) target: Option<Arc<str>>,
    /// Whether a set computed from this answer may enter the shared index:
    /// only an admitted resolution roots one. An unretained answer is
    /// complete yet was not kept, and a refusal answers no target at all,
    /// so a set either one fed is served to its request and never stored.
    pub(crate) retainable: bool,
}

impl AugmentationTarget {
    /// An admitted answer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn admitted(target: Option<Arc<str>>) -> Self {
        Self {
            target,
            retainable: true,
        }
    }

    /// The scan's reading of a type-route publication: a delivered target
    /// (admitted or unretained) is matched, a refusal matches nothing, and
    /// only an admitted answer is retainable.
    pub(crate) fn from_publication(
        publication: verter_workspace::ResolutionPublication<String>,
    ) -> Self {
        let retainable = matches!(
            publication,
            verter_workspace::ResolutionPublication::Admitted(_)
        );
        Self {
            target: publication.into_delivered().flatten().map(Arc::from),
            retainable,
        }
    }
}

pub(crate) struct AugmentationRequestDriver<'a> {
    db: &'a FileArtifactStore,
}
impl<'a> AugmentationRequestDriver<'a> {
    pub(crate) fn new(db: &'a FileArtifactStore) -> Self {
        Self { db }
    }
    pub(crate) fn ensure_populated<R>(
        &self,
        key: &AugmentationTargetKey,
        resolve_module_file: R,
        overlay_discriminator: Option<Hash16>,
    ) -> Arc<AugmenterSet>
    where
        R: Fn(&str, &str) -> AugmentationTarget,
    {
        if let Some(existing) = self.db.get_augmenter_set(key) {
            return existing;
        }

        // Cold scan — collect (canonical, parse_stable_hash) for
        // every artifact whose augmentations include at least one
        // matching `ModuleAugmentationFact` for the queried target.
        // Dedup by canonical so a file with multiple matching facts
        // contributes only once.
        //
        // The scan filters to base ([`FileArtifactKey::is_base`])
        // artifacts: the augmentation index is keyed by a base
        // resolve-domain identity (`project_identity`,
        // `resolve_env_hash`, `lib_env_hash`). A session-overlay artifact
        // An overlay-scoped key carries session-divergent
        // augmentations and must not poison that base index.
        // Snapshot first, then match off the guard: the resolver invoked
        // by `augmenter_matches_target` for a resolved-file target re-enters
        // the store and inserts into `self.artifacts`, which cannot run
        // while a `self.artifacts.iter()` shard guard is held (see
        // `collect_augmenter_candidates`).
        let candidates = self.db.collect_augmenter_candidates(overlay_discriminator);
        let mut matched: Vec<AugmenterEntry> = Vec::new();
        let mut seen_canonicals: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        // Whether every resolution the scan consumed was admitted.
        let mut retainable = true;
        for candidate in &candidates {
            for fact in candidate.augmentations.iter() {
                let resolved = if matches!(
                    key.target,
                    AugmentationTargetKind::ResolvedRelativeCanonical(_)
                )
                    && verter_session_query::source::augmentation::fact_augments_resolved_module(
                        fact,
                    ) {
                    let answer =
                        resolve_module_file(candidate.canonical.as_ref(), fact.specifier.as_ref());
                    retainable &= answer.retainable;
                    answer.target
                } else {
                    None
                };
                if verter_session_query::source::augmentation::augmenter_matches_target(
                    fact,
                    key,
                    resolved.as_deref(),
                ) {
                    if seen_canonicals.insert(Arc::clone(&candidate.canonical)) {
                        // Capture the EXACT artifact key — the stitch
                        // consumer re-fetches `.augmentations` via
                        // `get_artifacts(&key)` so it reads precisely
                        // the version fingerprinted here.
                        matched.push(AugmenterEntry {
                            artifact_key: candidate.artifact_key.clone(),
                            parse_stable_hash: candidate.parse_stable_hash,
                        });
                    }
                    break;
                }
            }
        }

        // Sort by (canonical, parse_stable_hash) for determinism.
        matched.sort_by(|a, b| {
            a.canonical()
                .as_ref()
                .cmp(b.canonical().as_ref())
                .then_with(|| a.parse_stable_hash.cmp(&b.parse_stable_hash))
        });

        let augmenter_count = matched.len() as u32;
        let fingerprint = compute_augmenter_set_fingerprint(&matched);
        let entries: SmallVec<[AugmenterEntry; 2]> = matched.into_iter().collect();
        let set = Arc::new(AugmenterSet {
            entries,
            fingerprint,
        });

        // A set fed by a resolution the process did not keep (or could not
        // trust) answers this request only: the shared index never holds it.
        if !retainable {
            return set;
        }

        // Passive publication retains the shared route-surface clock bracket and
        // changes artifact generation only when the fingerprint changes.
        let prev = self
            .db
            .populate_augmenter_set(key.clone(), Arc::clone(&set));
        let prev_fingerprint = prev.as_ref().map(|p| p.fingerprint);

        // Emit `ModuleAugmentationIndexShape` typed audit event.
        emit_module_augmentation_index_shape_event(
            key,
            prev_fingerprint,
            fingerprint,
            augmenter_count,
        );

        set
    }
}
