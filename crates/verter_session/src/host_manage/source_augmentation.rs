//! Cold augmentation matching belongs to the source request, outside storage guards.
use crate::file_artifact_store::{
    compute_augmenter_set_fingerprint, emit_module_augmentation_index_shape_event,
    AugmentationTargetKey, AugmentationTargetKind, AugmenterEntry, AugmenterSet, FileArtifactStore,
};
use smallvec::SmallVec;
use std::sync::Arc;
use verter_session_query::analysis::types::Hash16;

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
        resolve_relative_canonical: R,
        overlay_discriminator: Option<Hash16>,
    ) -> Arc<AugmenterSet>
    where
        R: Fn(&str, &str) -> Option<Arc<str>>,
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
        // by `augmenter_matches_target` for a relative target re-enters
        // the store and inserts into `self.artifacts`, which cannot run
        // while a `self.artifacts.iter()` shard guard is held (see
        // `collect_augmenter_candidates`).
        let candidates = self.db.collect_augmenter_candidates(overlay_discriminator);
        let mut matched: Vec<AugmenterEntry> = Vec::new();
        let mut seen_canonicals: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        for candidate in &candidates {
            for fact in candidate.augmentations.iter() {
                let relative = if matches!(
                    key.target,
                    AugmentationTargetKind::ResolvedRelativeCanonical(_)
                ) && verter_session_query::resolution::is_relative_specifier(
                    fact.specifier.as_ref(),
                ) {
                    resolve_relative_canonical(
                        candidate.canonical.as_ref(),
                        fact.specifier.as_ref(),
                    )
                } else {
                    None
                };
                if crate::file_artifact_store::augmenter_matches_target(
                    fact,
                    key,
                    relative.as_deref(),
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
