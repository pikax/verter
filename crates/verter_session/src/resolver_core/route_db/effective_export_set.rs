//! Effective export set (post-augmentation stitching) — R29 + G1.
//!
//! `EffectiveExportSet` cold-path computation stitches module augmentations
//! into the resolved export surface for a provider canonical. The
//! `effective_export_sets` sister table on [`super::RouteDb`] caches the
//! post-augmentation result keyed by `(provider, project_identity,
//! resolve_env_hash, lib_env_hash, session_scope)` (R21 — route surface
//! depends on libs because module augmentations live in libs; R6 — the
//! session dimension is the CONTENT-FREE session scope, never the overlay
//! fingerprint).
//!
//! This module owns the `EffectiveExportSet` cache-key/scope/entry types, the
//! cold-compute augmentation-stitch orchestrator, and the per-target fact-key
//! and audit-event constructors. The bare-route surface and barrel surface live
//! in the parent [`super`] module; the two share the parent's private
//! `effective_export_sets` storage through ancestor-module visibility.

use std::sync::Arc;

use verter_semantic::facts::registry::{InternedName, SymbolSpace};

use super::RouteDb;
use crate::file_artifact_store::{
    AugmentationTargetKey, AugmentationTargetKind, AugmenterEntry, AugmenterSet, FileArtifactKey,
    FileArtifactStore, ProjectIdentity,
};
use crate::resolver_core::{FactVersionRef, RouteSurfaceFactRef, StoreView};
use crate::types::Hash16;

/// Content-free session-scope dimension for [`EffectiveExportSetKey`] (R6).
///
/// [`EffectiveExportSetKey`] is a QUERY-IDENTITY cache key, so R6 forbids any
/// content/version-derived value in it: overlay CONTENT identity is rooted on
/// the VALUE via the `ModuleAugmentationIndexShape` augmenter-set fingerprint
/// fact + per-contributor `FileWholeHash` anchors, revalidated against the live
/// view on every warm hit. This dimension carries ONLY the orthogonal,
/// content-free SCOPE identity — `Base` for a base read, `Session(scope_id)`
/// for a session read (the content-free [`crate::resolver_core::StoreViewCompatToken::session`]).
///
/// It keeps base and session reads in DISTINCT slots (a base warm entry can
/// never satisfy a session lookup, and vice-versa) WITHOUT smuggling the
/// overlay-set content fingerprint into the key. That fingerprint legitimately
/// keys the CONTENT-ADDRESSED augmentation index
/// ([`crate::file_artifact_store::AugmentationPopulation::Session`]) — a
/// content-addressed compute cache, not a query-identity cache — and flows into
/// the cold producer only as a compute input (the index scan + discriminator),
/// never into this query-identity key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectiveExportSetScope {
    /// Base read — no session overlay.
    Base,
    /// Session read, keyed by the content-free session scope id
    /// ([`crate::resolver_core::StoreViewCompatToken::session`]).
    Session(u64),
}

impl EffectiveExportSetScope {
    /// Derive the content-free scope from a view's `compat_token().session`:
    /// `None` → [`Self::Base`], `Some(id)` → [`Self::Session`]. This is the
    /// SINGLE derivation the cold producer and the route-surface validator
    /// share so they always agree on a session's slot.
    #[must_use]
    pub fn from_session(session: Option<u64>) -> Self {
        match session {
            Some(id) => Self::Session(id),
            None => Self::Base,
        }
    }
}

/// Key for the per-provider effective export surface (R29 + R21).
///
/// Scoped to `(provider, project, resolve_env, lib_env, session_scope)`.
/// `lib_env_hash` enters because module augmentations live in libs (R21).
/// `session_scope` (overlay isolation) keeps a base read's augmenter set
/// (`Base`) in a distinct slot from a session read's (`Session(scope_id)`,
/// base ∪ overlay) — without it a base-populated warm entry would satisfy a
/// session lookup (the "base-as-session" hazard) on a shared `RouteDb`. It is a
/// CONTENT-FREE scope dimension (R6): the overlay-set content fingerprint is
/// NEVER in this key — overlay content identity is validated on the VALUE's
/// `fact_dep_signature` (the `ModuleAugmentationIndexShape` fingerprint fact +
/// per-contributor `FileWholeHash` anchors), revalidated on every warm hit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectiveExportSetKey {
    /// Canonical id of the provider whose surface this is.
    pub provider_canonical: String,
    /// Project identity dimension (R21).
    pub project_identity: ProjectIdentity,
    /// Resolve-env hash dimension (R21).
    pub resolve_env_hash: Hash16,
    /// Lib-env hash dimension (R21).
    pub lib_env_hash: Hash16,
    /// Content-free session-scope dimension (overlay isolation, R6).
    pub session_scope: EffectiveExportSetScope,
}

/// One contribution from an augmenter into a provider's effective
/// export surface.
///
/// Equality + hash are by `(augmented_name, space, contributor_canonical)`
/// so a downstream cache that hashes the entry can detect order-stable
/// changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectiveExportEntry {
    /// Augmented name added by the contributor.
    pub augmented_name: InternedName,
    /// Symbol space of the contribution.
    pub space: SymbolSpace,
    /// Canonical id of the file that emitted this augmentation.
    pub contributor_canonical: Arc<str>,
}

/// Cached effective export set after augmentation stitching (R29 +
/// G1).
///
/// `entries` is sorted by `(augmented_name, space, contributor_canonical)`
/// so the post-stitch surface is determinate under augmenter set
/// reordering. `fact_dep_signature` records the
/// `ModuleAugmentationIndexShape` fact for the queried target plus
/// per-augmenter file-version anchors — invalidating the consumer when
/// the augmenter set changes (G1) OR when one augmenter's content
/// changes.
#[derive(Debug, Clone)]
pub struct EffectiveExportSetEntry {
    /// Stitched effective contributions sorted by
    /// `(augmented_name, space, contributor_canonical)`.
    pub entries: Arc<[EffectiveExportEntry]>,
    /// Number of augmenters that contributed to this surface.
    pub augmenter_count: u32,
    /// Fingerprint of the augmenter set at stitch time. The
    /// downstream consumer's `fact_dep_signature` records a
    /// `RouteSurfaceFactRef::ModuleAugmentationIndexShape` carrying
    /// this hash as `expected_hash`.
    pub augmenter_set_fingerprint: Hash16,
    /// Fact-dep signature for this candidate. Multi-candidate cache
    /// slots store one signature per candidate.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

impl RouteDb {
    // ────────────────────────────────────────────────────────────
    // Effective export set (post-augmentation stitching) — R29 + G1
    // ────────────────────────────────────────────────────────────

    /// Warm-hit lookup for an effective export surface, validated
    /// against the current view. Returns the cached entry if the
    /// recorded `fact_dep_signature` still holds; otherwise `None`
    /// (caller routes through [`Self::get_or_compute_effective_export_set`]
    /// for the cold path).
    pub fn get_effective_export_set<V: StoreView>(
        &self,
        key: &EffectiveExportSetKey,
        view: &V,
    ) -> Option<Arc<EffectiveExportSetEntry>> {
        self.effective_export_sets.get_if_valid(key, view)
    }

    /// Look up or compute the effective export surface for a provider
    /// under the given env, stitching module augmentations from the
    /// host's augmentation index.
    ///
    /// `target` classifies the queried specifier into one of the four
    /// `AugmentationTargetKind` archetypes (R29). The cold path:
    ///
    /// 1. Builds `AugmentationTargetKey` from `key` + `target`.
    /// 2. Calls
    ///    [`FileArtifactStore::ensure_augmentation_index_populated`]
    ///    to materialise the augmenter set (and emit a
    ///    `ModuleAugmentationIndexShape` event on first install).
    /// 3. Iterates each augmenter's parse-domain
    ///    [`ModuleAugmentationFact`](verter_semantic::facts::ModuleAugmentationFact)
    ///    entries that match the target, stitches `(augmented_name,
    ///    space)` contributions into the effective set sorted by
    ///    `(augmented_name, space, contributor_canonical)`.
    /// 4. Records a
    ///    [`FactVersionRef::RouteSurface`] entry for
    ///    [`FactKey::ModuleAugmentationIndexShape`](verter_semantic::facts::FactKey)
    ///    (with `expected_hash = AugmenterSet.fingerprint`) so a future
    ///    augmenter-set change invalidates the consumer (per G1),
    ///    plus a per-contributor `FileWholeHash` so an edit to an
    ///    augmenting file's content also invalidates the consumer.
    /// 5. Emits a typed
    ///    [`StructuredAuditEvent::ModuleAugmentationStitched`](crate::component_meta_audit::StructuredAuditEvent)
    ///    audit event for the cold compute.
    ///
    /// `resolve_relative_canonical` is the caller-supplied resolver
    /// hook used for the `ResolvedRelativeCanonical` target archetype.
    ///
    /// `session_view` is the active overlay-bearing view (or `None` for a
    /// base read). The CONTENT-ADDRESSED augmentation-index population identity
    /// plus the overlay discriminator are derived from it through the shared
    /// [`crate::session_view::augmentation_population_for_view`] — the SAME
    /// derivation the body stitch uses — so a session read scans its own
    /// overlay augmenters (matched by the session overlay discriminator)
    /// UNIONED with base. The QUERY-IDENTITY `EffectiveExportSetKey` slot is
    /// keyed by the CONTENT-FREE session scope (`view.compat_token().session`),
    /// NOT the overlay fingerprint (R6); overlay content identity is validated
    /// on the value's facts. A `None` (or overlay-free) view stays base-only.
    /// The session branch never returns a base-only augmenter set presented
    /// under a session scope.
    pub fn get_or_compute_effective_export_set<V, FH, RR>(
        &self,
        key: EffectiveExportSetKey,
        target: AugmentationTargetKind,
        view: &V,
        session_view: Option<&dyn crate::session_view::SessionView>,
        artifact_store: &FileArtifactStore,
        contributor_whole_hash: FH,
        resolve_relative_canonical: RR,
    ) -> Arc<EffectiveExportSetEntry>
    where
        V: StoreView,
        FH: Fn(&str) -> Option<Hash16>,
        RR: Fn(&str, &str) -> Option<Arc<str>>,
    {
        // CONTENT-ADDRESSED population identity (overlay-aware augmentation
        // index): derived from the active overlay-bearing view through the
        // shared `augmentation_population_for_view`, the SAME derivation the
        // body stitch uses. A session view keys its augmenter set under
        // `Session(overlay-set fingerprint)` and the cold scan unions the
        // session's overlay augmenters (matched by the overlay discriminator)
        // with base; a base/overlay-free view stays `Base` with a `None`
        // discriminator. This fingerprint is a COMPUTE INPUT ONLY — it keys the
        // content-addressed `AugmentationTargetKey` index and matches overlay
        // artifacts; it NEVER enters the query-identity `EffectiveExportSetKey`
        // (R6).
        let (population, overlay_discriminator) =
            crate::session_view::augmentation_population_for_view(session_view);

        // The QUERY-IDENTITY slot dimension is the CONTENT-FREE session scope
        // (R6): the orthogonal session identity (`compat_token().session`),
        // never the overlay-set content fingerprint. Overwrite it (the scope is
        // OWNED by the derivation, never the caller) so the cache slot, the warm
        // lookup, and the route-surface validator all agree, keeping base and
        // session reads in distinct `EffectiveExportSetKey` slots. Overlay
        // CONTENT identity is rooted on the value's `fact_dep_signature`
        // (the `ModuleAugmentationIndexShape` fingerprint fact + per-contributor
        // `FileWholeHash` anchors), revalidated on every warm hit.
        let mut key = key;
        key.session_scope = EffectiveExportSetScope::from_session(view.compat_token().session);

        if let Some(existing) = self.effective_export_sets.get_if_valid(&key, view) {
            return existing;
        }

        let flight =
            self.effective_export_singleflight
                .run(key.clone(), view.compat_token(), || {
                    if let Some(existing) = self.effective_export_sets.get_if_valid(&key, view) {
                        return Ok(existing);
                    }

                    let augmentation_target_key = AugmentationTargetKey {
                        project_identity: key.project_identity,
                        resolve_env_hash: key.resolve_env_hash,
                        lib_env_hash: key.lib_env_hash,
                        population,
                        target: target.clone(),
                    };
                    let augmenter_set = artifact_store.ensure_augmentation_index_populated(
                        &augmentation_target_key,
                        &resolve_relative_canonical,
                        overlay_discriminator,
                    );

                    // Stitch each augmenter's contributions for the
                    // queried target. The augmenter's `.augmentations`
                    // are re-fetched by the EXACT `FileArtifactKey`
                    // captured at index-population time
                    // (`get_artifacts(&key)`), never a content-agnostic
                    // canonical-only scan: with lazy cache invalidation
                    // a stale pre-edit version of the augmenter can
                    // linger alongside the current one, and a
                    // canonical-only scan could surface a different
                    // version than the one the augmenter-set
                    // fingerprint was computed over.
                    //
                    // Stale-key self-heal. The captured exact key can
                    // still go stale: when an augmenter is reparsed
                    // under a NEW `FileArtifactKey` but its
                    // `parse_stable_hash` is unchanged (a member-body
                    // edit that leaves the decl skeleton intact), the
                    // augmenter-set fingerprint does not move, so the
                    // cached `AugmenterSet` is not invalidated and its
                    // `AugmenterEntry.artifact_key` keeps pointing at
                    // the PRE-edit content hash. A same-canonical edit
                    // routed through `FileArtifactStore::insert` drains
                    // that pre-edit key, so `get_artifacts` on the
                    // captured key misses. Silently dropping the
                    // augmenter there would shrink the stitched surface
                    // while keeping the (now wrong) fingerprint/count.
                    // On a miss we therefore re-derive the augmenter's
                    // CURRENT exact key from the scheduler-authoritative
                    // content hash (`contributor_whole_hash`, the same
                    // scheduler oracle the per-contributor `FileWholeHash`
                    // anchors below are built from — never a
                    // content-agnostic `get_artifacts_any` /
                    // `content_hash_for_canonical` scan) and read with
                    // that. Refreshed keys are written back into the
                    // cached `AugmenterSet` after the loop so subsequent
                    // reads hit the fast exact-key path. This routes
                    // through the SHARED
                    // `FileArtifactStore::augmenter_artifacts_self_healing`
                    // helper — the SAME healing path the `MergedDecl` body
                    // stitch uses, so the two cannot diverge.
                    let mut stitched: Vec<EffectiveExportEntry> = Vec::new();
                    let mut refreshed_keys: Vec<(usize, FileArtifactKey)> = Vec::new();
                    for (idx, augmenter) in augmenter_set.entries.iter().enumerate() {
                        let augmenter_canonical = augmenter.canonical();
                        // A genuine miss after the current-key re-fetch
                        // (the augmenter's `IndexedReady` is not
                        // materialised under its current content hash)
                        // is a principled skip — never a content-agnostic
                        // fallback scan. When the scheduler oracle has no
                        // current hash, only the captured key is tried.
                        let art = match contributor_whole_hash(augmenter_canonical.as_ref()) {
                            Some(current_hash) => artifact_store.augmenter_artifacts_self_healing(
                                &augmenter.artifact_key,
                                current_hash,
                            ),
                            None => artifact_store
                                .get_artifacts(&augmenter.artifact_key)
                                .map(|art| (art, None)),
                        };
                        let Some((art, refreshed_key)) = art else {
                            continue;
                        };
                        if let Some(refreshed_key) = refreshed_key {
                            refreshed_keys.push((idx, refreshed_key));
                        }
                        for fact in art.augmentations.iter() {
                            if !crate::file_artifact_store::augmenter_matches_target(
                                fact,
                                &augmentation_target_key,
                                augmenter_canonical.as_ref(),
                                &resolve_relative_canonical,
                            ) {
                                continue;
                            }
                            stitched.push(EffectiveExportEntry {
                                augmented_name: fact.augmented_name.clone(),
                                space: fact.space,
                                contributor_canonical: Arc::clone(augmenter_canonical),
                            });
                        }
                    }

                    // Write any refreshed exact keys back into the
                    // cached `AugmenterSet`. The augmenter-set
                    // fingerprint is folded over `parse_stable_hash`
                    // values (never `content_hash`), and a stale key is
                    // only refreshed when the augmenter's
                    // `parse_stable_hash` is unchanged — so the rebuilt
                    // set carries the SAME fingerprint and the same
                    // per-entry `parse_stable_hash` values; only the
                    // `artifact_key` content-hash dimension advances.
                    // Re-publishing under the identical fingerprint
                    // keeps every recorded `ModuleAugmentationIndexShape`
                    // signature valid while making the next stitch hit
                    // the fast exact-key path.
                    let augmenter_set = if refreshed_keys.is_empty() {
                        augmenter_set
                    } else {
                        let mut entries = augmenter_set.entries.clone();
                        for (idx, current_key) in refreshed_keys {
                            entries[idx] = AugmenterEntry {
                                artifact_key: current_key,
                                parse_stable_hash: entries[idx].parse_stable_hash,
                            };
                        }
                        let refreshed_set = Arc::new(AugmenterSet {
                            entries,
                            fingerprint: augmenter_set.fingerprint,
                        });
                        artifact_store.populate_augmenter_set(
                            augmentation_target_key.clone(),
                            Arc::clone(&refreshed_set),
                        );
                        refreshed_set
                    };
                    stitched.sort_by(|a, b| {
                        a.augmented_name
                            .as_ref()
                            .cmp(b.augmented_name.as_ref())
                            .then_with(|| compare_symbol_space(a.space, b.space))
                            .then_with(|| {
                                a.contributor_canonical
                                    .as_ref()
                                    .cmp(b.contributor_canonical.as_ref())
                            })
                    });

                    // Build the validation signature: the
                    // augmentation-index-shape fact + per-contributor
                    // file-whole-hash anchors.
                    let mut facts: Vec<FactVersionRef> = Vec::new();
                    facts.push(FactVersionRef::RouteSurface(RouteSurfaceFactRef {
                        canonical_id: key.provider_canonical.clone(),
                        key: build_module_augmentation_index_shape_fact_key(&target),
                        lane: verter_semantic::facts::FactLane::Semantic,
                        expected_hash: augmenter_set.fingerprint,
                    }));
                    for augmenter in augmenter_set.entries.iter() {
                        let augmenter_canonical = augmenter.canonical();
                        if let Some(hash) = contributor_whole_hash(augmenter_canonical.as_ref()) {
                            facts.push(FactVersionRef::FileWholeHash {
                                canonical_id: augmenter_canonical.as_ref().to_owned(),
                                hash,
                            });
                        }
                    }
                    let signature: Arc<[FactVersionRef]> =
                        Arc::from(facts.clone().into_boxed_slice());

                    let augmenter_count = augmenter_set.entries.len() as u32;
                    let entry = Arc::new(EffectiveExportSetEntry {
                        entries: Arc::from(stitched.into_boxed_slice()),
                        augmenter_count,
                        augmenter_set_fingerprint: augmenter_set.fingerprint,
                        fact_dep_signature: signature,
                    });
                    // Strict admission. The `EffectiveExportSet`
                    // cold-build always pushes at least the
                    // `ModuleAugmentationIndexShape` fact above, so
                    // `facts` is non-empty by construction here —
                    // strict admission is unconditionally safe.
                    self.effective_export_sets.insert_arc_with_kind(
                        key.clone(),
                        Arc::clone(&entry),
                        facts,
                        "route_db.effective_export_sets",
                    );

                    // Emit the cold-path audit event.
                    emit_module_augmentation_stitched_event(
                        &target,
                        augmenter_count,
                        augmenter_set.fingerprint,
                    );

                    Ok(entry)
                });

        match flight {
            Ok(run_result) => (*run_result.value).clone(),
            Err(()) => {
                // Singleflight returned Err only when the inner
                // closure does — our closure always Ok's. This arm
                // remains as a defensive fall-through so the type
                // signature stays infallible to callers.
                Arc::new(EffectiveExportSetEntry {
                    entries: Arc::from(Vec::<EffectiveExportEntry>::new().into_boxed_slice()),
                    augmenter_count: 0,
                    augmenter_set_fingerprint: [0u8; 16],
                    fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
                })
            }
        }
    }

    /// Insert a pre-built `EffectiveExportSetEntry` directly. Test-only
    /// helper for asserting cache-state assumptions without driving a
    /// full cold compute.
    #[cfg(test)]
    pub fn insert_effective_export_set(
        &self,
        key: EffectiveExportSetKey,
        entry: EffectiveExportSetEntry,
        facts: Vec<FactVersionRef>,
    ) {
        self.effective_export_sets.insert(key, entry, facts);
    }

    /// Number of slots in the effective-export-set table.
    #[must_use]
    pub fn effective_export_set_len(&self) -> usize {
        self.effective_export_sets.len()
    }

    /// View-free permissive lookup of the cached
    /// [`EffectiveExportSetEntry::augmenter_set_fingerprint`] for
    /// the supplied key. Used by the route-surface-domain validator
    /// (R26) so a consumer that recorded
    /// `RouteSurfaceFactRef::EffectiveExportSet` with
    /// `expected_hash = augmenter_set_fingerprint` can revalidate on
    /// read without re-entering the view's `validates` dispatch
    /// (the validator itself runs inside the view, so calling the
    /// view-aware `get_effective_export_set` would recurse).
    ///
    /// Returns `Some(fingerprint)` when an entry exists for the
    /// composed `(provider, project, resolve_env, lib_env)` slot,
    /// `None` otherwise. Multi-candidate slots return the
    /// last-admitted candidate's fingerprint (consistent with
    /// `ValidatedFactCache::snapshot_all` last-writer-wins
    /// semantics for permissive reads).
    #[must_use]
    pub fn lookup_effective_export_set_fingerprint(
        &self,
        key: &EffectiveExportSetKey,
    ) -> Option<Hash16> {
        self.effective_export_sets
            .lookup_any_candidate(key)
            .map(|entry| entry.augmenter_set_fingerprint)
    }
}

/// Build the parse-domain `FactKey::ModuleAugmentationIndexShape`
/// payload that an `EffectiveExportSet` consumer observes for the
/// queried target. The parallel optional fields hold the concrete
/// target value; the `target_kind_tag` discriminates.
pub(crate) fn build_module_augmentation_index_shape_fact_key(
    target: &AugmentationTargetKind,
) -> verter_semantic::facts::FactKey {
    use verter_semantic::facts::registry::AugmentationTargetKindTag;
    match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(spec.clone()),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            }
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ResolvedRelativeCanonical,
                external_specifier: None,
                resolved_relative_canonical: Some(Arc::clone(canon)),
                wildcard_pattern: None,
            }
        }
        AugmentationTargetKind::WildcardAmbient(pat) => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::WildcardAmbient,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: Some(pat.clone()),
            }
        }
        AugmentationTargetKind::GlobalAugmentation => {
            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::GlobalAugmentation,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            }
        }
    }
}

/// Emit a typed
/// [`StructuredAuditEvent::ModuleAugmentationStitched`](crate::component_meta_audit::StructuredAuditEvent)
/// for the cold-path compute. Silent no-op when no audit accumulator is
/// installed on the active thread.
fn emit_module_augmentation_stitched_event(
    target: &AugmentationTargetKind,
    augmenter_count: u32,
    fingerprint: Hash16,
) {
    use verter_audit::AugmentationTargetKindTag;
    let (tag, external_specifier, resolved_relative_canonical, wildcard_pattern) = match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => (
            AugmentationTargetKindTag::ExternalSpecifier,
            Some(Arc::<str>::from(spec.as_ref())),
            None,
            None,
        ),
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => (
            AugmentationTargetKindTag::ResolvedRelativeCanonical,
            None,
            Some(Arc::clone(canon)),
            None,
        ),
        AugmentationTargetKind::WildcardAmbient(pat) => (
            AugmentationTargetKindTag::WildcardAmbient,
            None,
            None,
            Some(Arc::<str>::from(pat.as_ref())),
        ),
        AugmentationTargetKind::GlobalAugmentation => (
            AugmentationTargetKindTag::GlobalAugmentation,
            None,
            None,
            None,
        ),
    };
    crate::host_manage::push_structured_event(
        crate::component_meta_audit::StructuredAuditEvent::ModuleAugmentationStitched {
            target_kind_tag: tag,
            external_specifier,
            resolved_relative_canonical,
            wildcard_pattern,
            augmenter_count,
            fingerprint,
        },
    );
}

/// Total ordering over `SymbolSpace` variants for deterministic
/// stitching order. Type < Value < Namespace.
fn compare_symbol_space(a: SymbolSpace, b: SymbolSpace) -> std::cmp::Ordering {
    fn rank(s: SymbolSpace) -> u8 {
        match s {
            SymbolSpace::Type => 0,
            SymbolSpace::Value => 1,
            SymbolSpace::Namespace => 2,
        }
    }
    rank(a).cmp(&rank(b))
}
