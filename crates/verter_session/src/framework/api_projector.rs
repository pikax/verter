#![deny(missing_docs)]
//! The framework-neutral public-API projection seam.
//!
//! Some frameworks project a component into a public-API virtual file the IDE
//! type-checks (a Vue SFC's `ComponentPublicInstance`-based `.ts` surface).
//! [`ComponentApiProjector`] is the per-adapter seam that renders that surface;
//! the host's public-API entry selects the projector by the canonical's
//! resolved [`FileLanguage`](verter_language::FileLanguage) adapter id.
//!
//! The Vue leg is the legacy extraction: it delegates to the deep pipeline
//! body (`render_vue_public_api_legacy`) that consumes cached TSC state and
//! external-type collection. Adapter projectors may consume their cached AST
//! facts and ask the shared framework-surface executor to dereference authored
//! locators; they must not reparse source or introduce a private resolver.

use verter_language::FileLanguage;

use crate::framework::public_contract::ComponentContractAvailability;
use crate::resolver_core::StoreView as _;
use crate::types::{CompileProfile, PublicApiMode, PublicApiProjectionError, TscResponse};
use crate::VerterHost;

/// Opaque proof that a structured component contract was projected from one
/// admitted component-meta result and one separately-cacheable output
/// materialization.
///
/// Consumers can only ask whether the proof remains current against a host;
/// the exact read sets and cache-key axes remain producer-owned.
#[derive(Clone)]
pub struct ComponentApiProjectionWitness {
    owner_canonical: std::sync::Arc<str>,
    owner_whole_hash: verter_semantic::analysis::Hash16,
    result_key: crate::component_meta_result_db::ComponentMetaResultKey,
    producer_project_generation: u64,
    admitted_read_set: crate::fact_signature_helpers::ReadSetSignature,
    output_read_set: crate::fact_signature_helpers::ReadSetSignature,
}

impl std::fmt::Debug for ComponentApiProjectionWitness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComponentApiProjectionWitness")
            .field("owner_canonical", &self.owner_canonical)
            .finish_non_exhaustive()
    }
}

impl ComponentApiProjectionWitness {
    pub(crate) fn from_publication_evidence(
        owner_canonical: &str,
        evidence: crate::host_manage::component_meta_entry::ComponentMetaOutputPublicationEvidence,
    ) -> Option<Self> {
        let final_result = evidence.final_result;
        if final_result.key.owner_canonical.as_ref() != owner_canonical
            || final_result.entry.payload.canonical_id.as_ref() != owner_canonical
            || final_result.entry.payload.whole_hash != final_result.owner_whole_hash
        {
            return None;
        }
        Some(Self {
            owner_canonical: std::sync::Arc::from(owner_canonical),
            owner_whole_hash: final_result.owner_whole_hash,
            result_key: final_result.key,
            producer_project_generation: final_result.entry.validated_at_generation,
            admitted_read_set: final_result.entry.read_set_signature.clone(),
            output_read_set: evidence.output_read_set,
        })
    }

    /// Return whether both producer read sets and every exact result-key axis
    /// still describe the host's current project shape and store view.
    ///
    /// Validation is bracketed by the complete host token so a mutation
    /// landing while either signature is checked fails closed.
    pub fn is_current(&self, host: &VerterHost) -> bool {
        let Some(current_view) = host.resolver_store_view_read().current() else {
            return false;
        };
        let computed_under = current_view.view().validation_token();
        if host.project_type_store().current_project_generation()
            != self.producer_project_generation
        {
            return false;
        }
        let Some(owner_whole_hash) = current_view
            .view()
            .whole_hash(self.owner_canonical.as_ref())
        else {
            return false;
        };
        let result_key = host.component_meta_result_key(
            self.owner_canonical.as_ref(),
            &crate::host_manage::ComponentMetaOptions::default(),
        );
        let admitted_valid = current_view
            .view()
            .validates_fact_signature(&self.admitted_read_set.facts);
        let output_valid = current_view
            .view()
            .validates_fact_signature(&self.output_read_set.facts);
        if owner_whole_hash != self.owner_whole_hash
            || result_key != self.result_key
            || !admitted_valid
            || !output_valid
        {
            return false;
        }
        let live_after = host.current_validation_token();
        !computed_under.externally_superseded_by(&live_after)
            && host.project_type_store().current_project_generation()
                == self.producer_project_generation
            && current_view
                .view()
                .whole_hash(self.owner_canonical.as_ref())
                .is_some_and(|whole_hash| whole_hash == self.owner_whole_hash)
            && host.component_meta_result_key(
                self.owner_canonical.as_ref(),
                &crate::host_manage::ComponentMetaOptions::default(),
            ) == self.result_key
    }

    #[cfg(test)]
    pub(crate) fn observes_relative_augmentation_for_test(
        &self,
        target_canonical: &str,
        augmenter_canonical: &str,
    ) -> ((bool, bool), (bool, bool)) {
        fn observes(
            signature: &crate::fact_signature_helpers::ReadSetSignature,
            target_canonical: &str,
            augmenter_canonical: &str,
        ) -> (bool, bool) {
            let shape = signature.facts.iter().any(|fact| {
                matches!(
                    fact,
                    crate::resolver_core::FactVersionRef::RouteSurface(route)
                        if matches!(
                            &route.key,
                            verter_semantic::facts::FactKey::ModuleAugmentationIndexShape {
                                target_kind_tag: verter_semantic::facts::registry::AugmentationTargetKindTag::ResolvedRelativeCanonical,
                                resolved_relative_canonical: Some(canonical),
                                ..
                            } if canonical.as_ref() == target_canonical
                        )
                )
            });
            let contributor = signature.facts.iter().any(|fact| {
                matches!(
                    fact,
                    crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                        if canonical_id == augmenter_canonical
                )
            });
            (shape, contributor)
        }

        (
            observes(
                &self.admitted_read_set,
                target_canonical,
                augmenter_canonical,
            ),
            observes(&self.output_read_set, target_canonical, augmenter_canonical),
        )
    }
}

/// One host-composed projector result: declaration response plus mandatory
/// semantic contract availability.
#[derive(Debug, Clone)]
pub struct ComponentApiProjection {
    /// Generated public declaration surface.
    pub response: TscResponse,
    /// Mandatory semantic public contract availability.
    pub contract: ComponentContractAvailability,
    /// Producer-owned validity proof for reusable background publication.
    /// `None` means the response remains usable for this invocation but its
    /// contract must not enter a downstream reusable cache.
    pub publication_witness: Option<ComponentApiProjectionWitness>,
}

/// One framework's public-API projection policy.
///
/// The host selects the impl by the canonical's resolved
/// [`FileLanguage`](verter_language::FileLanguage) adapter id and calls
/// [`Self::render_api`]; `Ok(None)` is the no-projection answer, while a
/// selected carrier's projection failure remains a typed error.
pub trait ComponentApiProjector: Send + Sync {
    /// Render the component's public-API surface for the requested mode.
    ///
    /// `Ok(None)` means the adapter intentionally exposes no public-API
    /// virtual file for this language/mode. Projection refusals return their
    /// exact typed failure.
    fn render_api(
        &self,
        cx: ComponentApiProjectorCtx<'_>,
    ) -> Result<Option<TscResponse>, PublicApiProjectionError>;
}

/// The public-API projection context.
///
/// Carries the canonical, the canonical's RUNTIME-loaded [`FileLanguage`]
/// (the explicit `UpsertRequest.file_language` the source was loaded with —
/// the same authority the pre-registry Vue gate consulted, NOT a static path
/// re-classification), the requested [`PublicApiMode`], the optional compile
/// [`CompileProfile`], and the host handle the Vue legacy flow needs.
pub struct ComponentApiProjectorCtx<'a> {
    /// The host handle the projector renders against.
    pub host: &'a VerterHost,
    /// The ALREADY-alias-resolved canonical id the host classified. The
    /// projector renders against THIS exact target (it does NOT re-resolve the
    /// alias) so classification and rendering operate on one coherent
    /// canonical — a concurrent alias relabel cannot classify one target and
    /// render another.
    pub resolved_canonical: &'a str,
    /// The canonical's runtime-loaded language row, captured for the SAME
    /// `resolved_canonical` — the per-adapter leg matches it against its
    /// descriptor's `carrier_language` so a same-adapter non-carrier language
    /// (e.g. a template row) does not enter the carrier-only public-API flow.
    pub file_language: &'a FileLanguage,
    /// The requested public-API surface mode.
    pub mode: PublicApiMode,
    /// The compile profile, when script/content overrides apply.
    pub profile: Option<&'a CompileProfile>,
    /// The batch-shared cold seed + active session view (crate-private; least
    /// authority). `Some` on every host render path (scalar `N=1` and batch).
    /// Vue consumes it for cross-file macro-type resolution. Svelte uses the
    /// same request seed to dereference AST-captured `$props()` and dispatcher
    /// locators through the shared framework-surface executor.
    pub(crate) render_seed: Option<PublicApiRenderSeed<'a>>,
}

/// The batch-shared cold-seed store view + active session view threaded into a
/// public-API render so a render takes ZERO per-call store-view reads.
///
/// Captured ONCE — per scalar call (`N=1`) or per batch — as a
/// [`crate::resolver_store::BatchFixedView`] and shared across every item: the
/// O(N²) store-view-cliff collapse. Least authority: the raw `BatchFixedView`
/// is intentionally NOT exposed; the projector only needs the cold seed to
/// build its request-bound resolver context.
pub(crate) struct PublicApiRenderSeed<'a> {
    /// The batch-shared OVERLAID cold-seed for the external-type collection /
    /// extraction resolver context. Reused across every item; the cold compute
    /// seeds from it WITHOUT a fresh per-item `resolver_store_view_read()`.
    pub(crate) cold_seed: &'a crate::resolver_store::ColdSeedHostStoreView,
    /// The exact session/profile view the cold seed was rooted through.
    /// Profile-owned block overrides ride this view as one immutable source
    /// overlay, so syntax extraction, semantic macro projection, and revision
    /// fencing all observe the same bytes.
    pub(crate) view: &'a dyn crate::session_view::SessionView,
    /// The batch's captured fixed view itself, so a per-item consumer that
    /// needs a request-bound snapshot (the fallthrough resolver) pins THIS
    /// capture instead of opening its own — the render path takes ZERO
    /// per-item store-view reads, which the O(1) batch gates measure.
    pub(crate) fixed: &'a crate::resolver_store::BatchFixedView,
}
