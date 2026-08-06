//! The owner's import-route resolution witness — the resolve-domain
//! successor to the deleted `DerivedFactKind::ImportRoute` digest.
//!
//! The legacy rail summarised an owner's *resolved* import table into a
//! single hash that a store-view build recomputed for every published
//! owner, re-resolving generation-stale entries to do so. That made the
//! validation surface both O(published owners) and a resolution
//! PRODUCER: the oracle warmed the very owner edge whose recompute it
//! existed to make observable.
//!
//! The witness here is the same information expressed as resolve-domain
//! facts. Resolving an owner's authored specifiers through the shared
//! route-edge policy drives the Engine's sealed resolution transaction,
//! and every admitted resolution carries its own `ReadSetSignature`. Its
//! derived `Decision` fact is the bounded consumer witness; the resolution
//! world's DAG retains the path-precise primitive observations (`PathProbe`,
//! `Realpath`, `Manifest`, `ExactResolution`, `DirectoryMembers`,
//! `RecoveryScope`, `ContextSelection`) that produced the answer.
//!
//! Consequences that the digest could not provide:
//!
//! * a known-miss specifier stops validating the instant its dependency
//!   appears, because the appearance advances exactly the `PathProbe`
//!   the miss observed — the owner's own bytes never move;
//! * an unrelated appearance leaves the witness valid (path-precise, not
//!   a global file-set stamp);
//! * validation is O(observed facts) against the store view's CAPTURED
//!   immutable resolution world, so a store-view build performs no
//!   routing work at all.
//!
//! # Why the collection is scoped, not ambient
//!
//! An admitted resolution's signature contains its derived Decision node,
//! whose DAG edges retain the transaction's complete observations. Fanning
//! every admitted resolution into whatever fact tracer
//! happens to be installed would inflate every read set in the process,
//! because `resolve_for_persistent_state` sits on every route-edge hop.
//! So the collection point is a DEDICATED scope
//! ([`ResolutionWitnessScope`]) that only the witness builder below
//! installs: with no scope active the recorder is one thread-local
//! integer load, and the observation set materialises exactly at the
//! producers that root on import-route currency.
//!
//! An intermediate lane matters as much as the final answer. The shared
//! type-route policy probes the `TypeImport` lane, falls back to
//! `EsmImport`, then re-normalises the runtime target — so a `.d.ts`
//! companion appearing beside an already-resolving `.js` target
//! retargets the edge without changing the final transaction's own
//! observations. The scope therefore records every resolution the
//! builder drives, not just the carrier it returns.
//!
//! The authored specifier inventory is pure parse domain: script
//! imports / reexports / wildcard reexports from the shallow routing
//! surface, plus the SFC `src=` external requests from the scheduler's
//! parse snapshot. No resolved canonical is read from a parse artifact.

use std::cell::{Cell, RefCell};

use crate::resolver_core::FactVersionRef;
use crate::VerterHost;

thread_local! {
    /// Number of [`ResolutionWitnessScope`] frames installed on this
    /// thread. Read on every admitted resolution, so it is a plain
    /// `Cell` rather than a `RefCell` borrow.
    static WITNESS_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// One observation buffer per installed frame. Only touched while
    /// `WITNESS_DEPTH > 0`.
    static WITNESS_FRAMES: RefCell<Vec<Vec<FactVersionRef>>> = const {
        RefCell::new(Vec::new())
    };
}

/// A scope that collects the observation signatures of every resolution
/// admitted on this thread while it is installed.
///
/// Nesting fans into every open frame, mirroring the fact tracer's
/// stack semantics, so a builder nested inside another builder still
/// contributes to both witnesses.
///
/// Two producers install one: the owner-wide witness builder below, and
/// a route walk that wants the witness of exactly the edges IT
/// traversed. The second is the path-precise form — a barrel's whole
/// authored inventory is far broader than any single route through it,
/// and resolving the unvisited siblings would both over-root the entry
/// and defeat the walk's path precision.
pub(crate) struct ResolutionWitnessScope;

impl ResolutionWitnessScope {
    pub(crate) fn enter() -> Self {
        WITNESS_FRAMES.with(|frames| frames.borrow_mut().push(Vec::new()));
        WITNESS_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }

    /// The observations recorded into this frame so far.
    pub(crate) fn collected(&self) -> Vec<FactVersionRef> {
        WITNESS_FRAMES.with(|frames| frames.borrow().last().cloned().unwrap_or_default())
    }
}

impl Drop for ResolutionWitnessScope {
    fn drop(&mut self) {
        WITNESS_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        WITNESS_FRAMES.with(|frames| {
            frames.borrow_mut().pop();
        });
    }
}

/// Record an admitted resolution's sealed Decision signature into every open
/// [`ResolutionWitnessScope`].
///
/// This is the ONE collection point for the resolve-domain rooting rail.
/// It is called from the two Engine entry points the session resolves
/// persistent state through, and it is a thread-local integer load —
/// nothing more — whenever no witness is being built, which is the
/// overwhelming majority of resolutions.
pub(crate) fn record_resolution_witness<T>(
    publication: &verter_workspace::ResolutionPublication<T>,
) {
    if WITNESS_DEPTH.with(Cell::get) == 0 {
        return;
    }
    // A `Refused` publication carries no witness by construction; the
    // builder observes the refusal through the returned publication and
    // declines to root at all.
    let verter_workspace::ResolutionPublication::Admitted(admitted) = publication else {
        return;
    };
    let facts = &admitted.signature().facts;
    if facts.is_empty() {
        return;
    }
    WITNESS_FRAMES.with(|frames| {
        for frame in frames.borrow_mut().iter_mut() {
            frame.extend_from_slice(facts);
        }
    });
}

impl VerterHost {
    /// The owner's complete import-route resolution witness.
    ///
    /// Resolves every authored specifier the owner declares through the
    /// shared route-edge policy and returns the union of the admitted
    /// transactions' observations. `None` means the witness is not
    /// rootable — either the owner has no readable parse surface, or at
    /// least one specifier's resolution was REFUSED (the transaction
    /// could not admit a complete signature), or the union exceeds the
    /// signature bound. A caller that cannot root its entry must not
    /// publish it.
    ///
    /// An owner with a readable surface and no specifiers at all yields
    /// an EMPTY witness, which is a legitimate rooting: there is no
    /// import-route input for the entry to depend on.
    pub(crate) fn owner_import_route_witness(
        &self,
        canonical_id: &str,
    ) -> Option<Vec<FactVersionRef>> {
        let specifiers = self.authored_import_specifiers(canonical_id)?;
        self.import_route_witness_for_lanes(canonical_id, &specifiers)
    }

    /// Coverage-checked variant: the witness for an EXPLICIT specifier
    /// set (the unresolved-wildcard rooting loop supplies the sources it
    /// actually traversed).
    ///
    /// Every listed specifier is resolved, so the returned witness
    /// necessarily observes each one. A refusal on any of them yields
    /// `None` — the old coverage check ("is this source present in the
    /// hashed table?") is structural here rather than a lookup, because
    /// the witness is built FROM the requested sources.
    pub(crate) fn import_route_witness_for_specifiers(
        &self,
        canonical_id: &str,
        specifiers: &[String],
    ) -> Option<Vec<FactVersionRef>> {
        let lanes: Vec<(String, Option<verter_workspace::ResolveRequestKind>)> = specifiers
            .iter()
            .map(|specifier| (specifier.clone(), None))
            .collect();
        self.import_route_witness_for_lanes(canonical_id, &lanes)
    }

    /// Lane-aware witness builder. `None` selects the shared type-route
    /// policy; `Some(kind)` replays a specifier through the SAME
    /// workspace lane the recorder produced it under. Exact resolutions
    /// are keyed `(specifier, phase, kind)`, so an SFC `src=` include
    /// resolved through the type-route lane would miss the caller's
    /// `SfcSrcAttr` exact row and observe a different fact than the one
    /// a re-push advances.
    fn import_route_witness_for_lanes(
        &self,
        canonical_id: &str,
        specifiers: &[(String, Option<verter_workspace::ResolveRequestKind>)],
    ) -> Option<Vec<FactVersionRef>> {
        #[cfg(test)]
        if self
            .test_force
            .force_import_route_witness_refusal_for_tests
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return self.decline_import_route_witness();
        }
        let witness = self.observed_import_route_witness(canonical_id, specifiers)?;
        if witness.len() > verter_workspace::FACT_SIGNATURE_CAP {
            // Overflow: the witness cannot represent the complete
            // observation set, so it is not rootable. Never represented
            // by a truncated or empty signature (`.DECISION.md` §3).
            return self.decline_import_route_witness();
        }
        Some(witness)
    }

    /// The deduped observation set, in first-observation order, with the
    /// signature bound NOT yet applied.
    ///
    /// `None` means a specifier's resolution was REFUSED — the other reason
    /// a witness is unrootable, and a genuinely different one: a refusal
    /// carries no observations at all, while an overflow carries too many.
    /// Splitting them here is what lets a fixture assert which of the two it
    /// is exercising instead of asserting the `None` both produce.
    fn observed_import_route_witness(
        &self,
        canonical_id: &str,
        specifiers: &[(String, Option<verter_workspace::ResolveRequestKind>)],
    ) -> Option<Vec<FactVersionRef>> {
        let (refused, observed) = {
            let scope = ResolutionWitnessScope::enter();
            let mut refused = false;
            for (specifier, lane) in specifiers {
                match self.generation_current_route_resolution(canonical_id, specifier, *lane) {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                        // The witness is the point of the call; the
                        // projected target is not consumed here.
                        let _ = admitted.into_result();
                    }
                    verter_workspace::ResolutionPublication::Refused(_) => {
                        refused = true;
                    }
                }
            }
            (refused, scope.collected())
        };
        if refused {
            return self.decline_import_route_witness();
        }

        // Dedup while preserving first-observation order. The consuming
        // producer folds these into its own `FactReadSet`, which sorts
        // and dedups canonically on finalise.
        let mut seen: rustc_hash::FxHashSet<FactVersionRef> =
            rustc_hash::FxHashSet::with_capacity_and_hasher(observed.len(), Default::default());
        let mut witness: Vec<FactVersionRef> = Vec::with_capacity(observed.len());
        for fact in observed {
            if seen.insert(fact.clone()) {
                witness.push(fact);
            }
        }
        Some(witness)
    }

    /// Mark the enclosing compute non-cacheable and report an
    /// unrootable import-route witness.
    fn decline_import_route_witness(&self) -> Option<Vec<FactVersionRef>> {
        crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
            crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
        );
        None
    }

    /// Test-support mirror of [`Self::owner_import_route_witness`] so
    /// fixtures can assert what the production producers root on.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn owner_import_route_witness_for_tests(
        &self,
        canonical_id: &str,
    ) -> Option<Vec<FactVersionRef>> {
        self.owner_import_route_witness(canonical_id)
    }

    /// The size of the owner's deduped observation set BEFORE the signature
    /// bound is applied; `None` when a specifier's resolution was refused.
    ///
    /// A fixture asserting only that the witness is `None` cannot tell
    /// overflow from refusal, so it cannot notice when a change to the bound
    /// or to witness composition silently converts it from one into the
    /// other. This reports the quantity the bound is compared against, so
    /// the fixture can state which case it stages and how much headroom it
    /// holds.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn owner_import_route_observation_count_for_tests(
        &self,
        canonical_id: &str,
    ) -> Option<usize> {
        let specifiers = self.authored_import_specifiers(canonical_id)?;
        self.observed_import_route_witness(canonical_id, &specifiers)
            .map(|witness| witness.len())
    }

    /// Observe the owner's import-route witness onto the ACTIVE fact
    /// tracer, for callers that root through the tracer rather than an
    /// explicit fact vector.
    ///
    /// Returns `false` when the witness is not rootable; the refusal has
    /// already been marked on the tracer by then.
    pub(crate) fn observe_owner_import_route_witness(&self, canonical_id: &str) -> bool {
        match self.owner_import_route_witness(canonical_id) {
            Some(witness) => {
                crate::resolver_core::resolver_context::observe_fan_out_borrowed(&witness);
                true
            }
            None => false,
        }
    }

    /// The owner's AUTHORED import/reexport specifier inventory — pure
    /// parse domain.
    ///
    /// Two parse-domain sources, unioned:
    ///
    /// * the scheduler's parse snapshot — SFC `src=` external requests
    ///   plus the script's import declarations and module references,
    ///   exactly the inventory `build_parsed_edges_from_analysis`
    ///   records as the owner's edges;
    /// * the shallow routing surface of an already-published
    ///   `IndexedReady`, read OBSERVE-ONLY, which additionally carries
    ///   the reexport and wildcard-reexport specifiers.
    ///
    /// Neither read is a resolved-route read, and neither materialises:
    /// `observe_content_pinned_indexed` has no re-index arm, so building
    /// a witness never publishes or refreshes an artifact.
    ///
    /// A third source is the CALLER-DECLARED specifier set: the KEYS of
    /// `DerivedRawState.import_routes`. That table is now exclusively
    /// the caller-supplied authoritative route push
    /// (`set_import_dependencies`), so its keys are the caller's
    /// declared request identities — a request-domain input, not a
    /// resolved-route read (the resolved VALUES are never consulted
    /// here). Without it a purely synthetic bundler specifier with no
    /// authored counterpart in the owner's source contributed nothing to
    /// the witness, so a consumer rooted on the witness would never
    /// observe that specifier retargeting. Each such specifier is
    /// resolved below like any other, and the exact-resolution row the
    /// same push installs is what its `ExactResolution` fact observes.
    ///
    /// `None` when the owner has no readable parse surface at all; an
    /// empty vector is a genuine "no import routes" answer.
    fn authored_import_specifiers(
        &self,
        canonical_id: &str,
    ) -> Option<Vec<(String, Option<verter_workspace::ResolveRequestKind>)>> {
        let mut specifiers: Vec<(String, Option<verter_workspace::ResolveRequestKind>)> =
            Vec::new();
        let mut readable = false;

        if let Some(source) = self.scheduler.try_get_source(canonical_id) {
            if let Some(data) = source.downcast_data::<crate::host_executor::HostSourceData>() {
                readable = true;
                // SFC `src=` external includes. A `src=` target can appear
                // or retarget while the owner's bytes stay put — exactly
                // the class the legacy digest merged in from
                // `DerivedRawState`. Replayed through the recorded
                // `SfcSrcAttr` lane, because exact resolutions are keyed
                // `(specifier, phase, kind)`.
                specifiers.extend(data.parse.external_requests.iter().map(|request| {
                    (
                        request.specifier.clone(),
                        Some(verter_workspace::ResolveRequestKind::SfcSrcAttr),
                    )
                }));
                let analysis = &data.parse.script_analysis;
                specifiers.extend(
                    analysis
                        .imports
                        .iter()
                        .map(|import| (import.source.clone(), None)),
                );
                for module_reference in &analysis.module_references {
                    specifiers.extend(
                        module_reference
                            .literal_specifier
                            .iter()
                            .chain(module_reference.finite_specifiers.iter())
                            .filter(|specifier| !specifier.is_empty())
                            .map(|specifier| (specifier.clone(), None)),
                    );
                }
            }
        }

        // Script-level routing surface from the shallow inventory, when
        // one is already published. Observe-only: never materialises.
        if let Some(indexed) = self.observe_content_pinned_indexed(canonical_id) {
            readable = true;
            let state = &indexed.shallow_state;
            specifiers.extend(
                state
                    .import_targets
                    .values()
                    .map(|target| (target.source_specifier.clone(), None)),
            );
            specifiers.extend(
                state
                    .wildcard_reexports
                    .iter()
                    .map(|wildcard| (wildcard.source_specifier.clone(), None)),
            );
            specifiers.extend(state.exports.values().filter_map(|export| match export {
                crate::resolver_core::shallow_file_state::ExportTarget::Reexport {
                    source_specifier,
                    ..
                } => Some((source_specifier.clone(), None)),
                crate::resolver_core::shallow_file_state::ExportTarget::Local { .. } => None,
            }));
        }

        // Caller-declared specifiers (KEYS only — the resolved values are
        // never read). A caller-pushed route for a specifier the owner
        // does not author is otherwise invisible to the witness.
        if let Some(derived) = self.derived_raw_cache().get(canonical_id) {
            specifiers.extend(
                derived
                    .import_routes
                    .keys()
                    .map(|specifier| (specifier.clone(), None)),
            );
        }

        if !readable {
            return None;
        }
        specifiers.sort();
        specifiers.dedup();
        Some(specifiers)
    }
}
