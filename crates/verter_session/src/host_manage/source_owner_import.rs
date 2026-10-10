//! Request-side owner-import coordination over passive owned storage.
use crate::owner_import_surface::{OwnerImportSurface, OwnerImportSurfaceDb};
use std::sync::{atomic::Ordering, Arc};
use verter_session_query::analysis::types::Hash16;

/// **The single publisher of `OwnerResolutionSet`.**
///
/// Private to this module by design, and that is the whole enforcement:
/// nothing outside the owner import surface can name this function, so
/// the owner-scoped resolution node has exactly one authority. Rust
/// privacy (`E0603`) is the rail — there is no name-keyed scanner.
///
/// The node records the owner's CHILD DECISIONS, so observing it roots
/// the surface on one fact per resolved specifier instead of on the union
/// of everything those specifiers transitively reach. It is published and
/// observed inside the surface's own cold fact tracer, so the admitted
/// signature carries it and every warm read revalidates it.
///
/// A `None` publication (the owner has no published decision to stand
/// for) observes nothing: the surface keeps whatever precise facts it
/// already recorded, which is the fail-closed direction.
fn observe_owner_resolution_set(host: &crate::VerterHost, owner_canonical: &str) {
    if let Some(fact) = host.ws().publish_owner_resolution_set(owner_canonical) {
        verter_type_engine::resolver_core::resolver_context::observe_fan_out(fact);
    }
}

pub(crate) struct OwnerImportRequestDriver<'a> {
    db: &'a OwnerImportSurfaceDb,
}
impl<'a> OwnerImportRequestDriver<'a> {
    pub(crate) fn new(db: &'a OwnerImportSurfaceDb) -> Self {
        Self { db }
    }
    #[must_use]
    pub fn get_with_view<V>(
        &self,
        host: &crate::VerterHost,
        owner_canonical: &str,
        expected_owner_whole_hash: Hash16,
        view: &V,
    ) -> Option<Arc<OwnerImportSurface>>
    where
        V: verter_session_query::facts::store_view::StoreView + ?Sized,
    {
        let candidate = self
            .db
            .lookup_owner_hash_candidate(owner_canonical, expected_owner_whole_hash)?;
        if candidate.validated_at_generation
            != host.project_type_store().current_project_generation()
        {
            return None;
        }
        if view.validates_fact_signature(&candidate.read_set_signature.facts) {
            return Some(candidate);
        }
        None
    }

    /// Owner-controlled warm-or-cold admission. The request driver performs the validated
    /// warm lookup, atomically cleans only the stale content version, invokes
    /// the cold closure, and directs the sole production write. A valid but
    /// non-cacheable result is served from `ReturnOnly` without mutation.
    pub(crate) fn get_or_compute<V, F>(
        &self,
        host: &crate::VerterHost,
        owner_canonical: &str,
        owner_whole_hash: Hash16,
        view: &V,
        compute: F,
    ) -> Option<Arc<OwnerImportSurface>>
    where
        V: verter_session_query::facts::store_view::StoreView + ?Sized,
        F: FnOnce() -> verter_type_engine::cache_runtime::singleflight::ComputeAdmission<
            Arc<OwnerImportSurface>,
            Arc<OwnerImportSurface>,
        >,
    {
        if let Some(cached) = self.get_with_view(host, owner_canonical, owner_whole_hash, view) {
            // R28 fact-bubble-up on the WARM path — mirror of
            // `RouteDb::get_or_resolve_route_observing_facts`'s warm-hit
            // branch. Re-observe the surface's recorded chain deps (owner
            // + leaf `FileWholeHash` facts + route-chain facts — exactly
            // the validated `read_set_signature`, never a broader set)
            // into every active tracer on this thread, so an ENCLOSING
            // traced cold compute folding this warm surface roots the
            // same dependency facts a cold build fans out. Without this,
            // an enclosing entry publishes without the chain deps and a
            // later leaf edit / barrel retarget cannot invalidate it (the
            // typeinfo published-Surface stale-warm hole). No-op when no
            // tracer is installed (R24 warm-hit cost discipline).
            verter_type_engine::fact_signature_helpers::observe_fact_signature(
                &cached.read_set_signature.facts,
            );
            return Some(cached);
        }
        self.db
            .remove_if_owner_hash_matches(owner_canonical, owner_whole_hash);

        let (decision, finalise) = verter_type_engine::fact_signature_helpers::install_fact_tracer(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(host),
            || {
                let decision = compute();
                let surface = match &decision {
                    verter_type_engine::cache_runtime::singleflight::ComputeAdmission::Cacheable(surface) => {
                        Some(surface)
                    }
                    verter_type_engine::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                        value,
                        ..
                    } => Some(value),
                    verter_type_engine::cache_runtime::singleflight::ComputeAdmission::Failed => None,
                };
                // The owner re-observes every producer-supplied direct-chain fact
                // before finalisation. The admitted signature is rebuilt solely from
                // this owner-owned tracer; a caller cannot hand a raw signature to
                // the write.
                if let Some(surface) = surface {
                    for fact in surface.read_set_signature.facts.iter() {
                        verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                            fact.clone(),
                        );
                    }
                    observe_owner_resolution_set(host, &surface.owner_canonical);
                }
                decision
            },
        );
        host.provenance
            .owner_import_surface_fact_tracer_installs
            .fetch_add(1, Ordering::Relaxed);

        let rebind =
            |surface: Arc<OwnerImportSurface>,
             facts: Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>| {
                Arc::new(OwnerImportSurface {
                    owner_canonical: Arc::clone(&surface.owner_canonical),
                    owner_whole_hash: surface.owner_whole_hash,
                    bindings: Arc::clone(&surface.bindings),
                    read_set_signature:
                        verter_session_query::facts::fact_cache::ReadSetSignature::new(facts),
                    validated_at_generation: surface.validated_at_generation,
                })
            };

        match (decision, finalise) {
            (
                verter_type_engine::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    surface,
                ),
                verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(facts),
            ) => {
                let surface = rebind(surface, facts);
                let generation_current = surface.validated_at_generation
                    == host.project_type_store().current_project_generation();
                let identity_current = surface.owner_canonical.as_ref() == owner_canonical
                    && surface.owner_whole_hash == owner_whole_hash;
                // `view` may be a deliberately fixed request snapshot that
                // predates artifacts loaded during this cold walk. Rechecking
                // the newly minted facts against that old snapshot would reject
                // correct cold results. Generation and key identity fence the
                // publish race here; every warm read performs strict fact
                // validation against its own caller view in `get_with_view`.
                if !generation_current || !identity_current {
                    verter_type_engine::cache_runtime::admission::propagate_non_admission(
                        verter_audit::NonAdmissionReason::GenerationSuperseded,
                    );
                    return None;
                }
                if let Err(refusal) = self
                    .db
                    .insert_owned(Arc::clone(&surface.owner_canonical), Arc::clone(&surface))
                {
                    verter_type_engine::cache_runtime::admission::propagate_non_admission(
                        refusal.non_admission_reason(),
                    );
                }
                Some(surface)
            }
            (
                verter_type_engine::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value,
                    reason,
                },
                verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(facts),
            ) => {
                verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                Some(rebind(value, facts))
            }
            (
                verter_type_engine::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    surface,
                )
                | verter_type_engine::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value: surface,
                    ..
                },
                verter_session_query::facts::fact_read_set::FactReadSetFinalise::NonCacheable(
                    facts,
                ),
            ) => {
                host.provenance
                    .owner_import_surface_fenced_serve_refusals
                    .fetch_add(1, Ordering::Relaxed);
                verter_type_engine::cache_runtime::admission::propagate_non_admission(
                    verter_audit::NonAdmissionReason::UnresolvedProvenance,
                );
                Some(rebind(surface, facts))
            }
            (
                verter_type_engine::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    surface,
                )
                | verter_type_engine::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value: surface,
                    ..
                },
                verter_session_query::facts::fact_read_set::FactReadSetFinalise::MutationUnstable,
            ) => {
                // A compaction domain moved mid-scope: a STABILITY
                // failure, refused under its own reason.
                verter_type_engine::cache_runtime::admission::propagate_non_admission(
                    verter_audit::NonAdmissionReason::MutationUnstable,
                );
                Some(surface)
            }
            (verter_type_engine::cache_runtime::singleflight::ComputeAdmission::Failed, _) => None,
        }
    }
}
