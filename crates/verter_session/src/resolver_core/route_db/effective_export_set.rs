//! Legacy effective-export fact validation plus the shared augmentation-index
//! fact-key constructor.
//!
//! Production augmentation stitching is owned by `ProjectSemanticDispatch`.
//! This module retains the typed legacy cache key/value and validation lookup
//! for existing `FactKey::EffectiveExportSet` observations, but deliberately
//! exposes no cold compute/publish funnel.

use std::sync::Arc;

use verter_semantic::facts::registry::{InternedName, SymbolSpace};

use super::RouteDb;
use crate::file_artifact_store::{AugmentationTargetKind, ProjectIdentity};
use crate::resolver_core::{FactVersionRef, StoreView};
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
    /// recorded `fact_dep_signature` still holds; otherwise `None`.
    pub fn get_effective_export_set<V: StoreView>(
        &self,
        key: &EffectiveExportSetKey,
        view: &V,
    ) -> Option<Arc<EffectiveExportSetEntry>> {
        self.effective_export_sets.get_if_valid(key, view)
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
