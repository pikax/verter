//! The store view a fact reference validates under, and the coalescing token of its
//! external validity.
use crate::facts::fact_cache::DerivedFactKind;
use crate::facts::fact_cache::FactVersionRef;
use crate::facts::fact_cache::ParseFactRef;
use crate::facts::fact_cache::ProgramAnalysisFactRef;
use crate::facts::fact_cache::ResolveImportsFactRef;
use crate::facts::fact_cache::RouteSurfaceFactRef;

use std::hash::Hash;

pub type ResolverHash16 = crate::analysis::types::Hash16;

/// Lane-identity token for singleflight / stability-request
/// deduplication.
///
/// This token is the SOLE identity `run_stable_request` (and the
/// `SingleflightGroup` lanes it drives) coalesce on, and a FOLLOWER
/// receives the LEADER's stable result WITHOUT revalidating it against
/// the follower's own view. The token must therefore be a COMPLETE
/// validity oracle: two requests may coalesce onto one lane ONLY if
/// their views are validation-equivalent.
///
/// `epoch` + `session` alone are NOT complete — a view's EXTERNAL
/// validity can change (env-hash / project-identity / project-generation /
/// overlay) WITHOUT moving the `store_view_epoch`. `validity_fingerprint`
/// closes that hole: the production
/// [`crate::resolver_store::HostStoreView`] folds the EXTERNAL-supersession
/// dimensions of its `StoreViewValidationToken` into it (the SAME oracle
/// the executors' promotion fence `is_stable` compares), so two views that
/// would externally-supersede each other get distinct lane identities and
/// never wrongly coalesce. Test / permissive stubs leave it `0` (their
/// views are validation-trivial).
///
/// The additive `artifact_generation` /
/// `load_generation` are DELIBERATELY EXCLUDED from the fold: a cold
/// compute advances those generations as its OWN work (publishing
/// artifacts, loading dependencies), so two concurrent identical cold
/// requests legitimately observe different additive generations. Folding
/// them would split those identical requests across distinct lanes and
/// spawn multiple cold winners instead of one leader + N-1 dedup-joining
/// followers — the same self-fencing the promotion oracle avoids. A
/// follower on the same external lane IS validation-equivalent: the leader
/// only promotes when the external dimensions are coherent.
///
/// `epoch` and `session` are retained as separate fields because callers
/// read them directly (e.g. the route-surface validator inspects
/// `session` to reject session views; the snapshot identity threads
/// `epoch`). `validity_fingerprint` is additive: it tightens lane
/// identity without changing what those reads observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewCompatToken {
    pub epoch: u64,
    pub session: Option<u64>,
    /// Fold of the EXTERNAL-supersession dimensions of the
    /// `StoreViewValidationToken` the view was built under (epoch,
    /// project-generation, env-hash, project-identity, overlay). `0` for
    /// validation-trivial stub views. Folds every external validity-
    /// affecting dimension that `epoch` alone does not cover — and excludes
    /// the additive artifact / load generations a cold
    /// compute advances as its own work — so the singleflight / stability
    /// coalescing lane is the SAME oracle the promotion fence applies.
    pub validity_fingerprint: u64,
}

pub trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;

    /// Validate a fact reference under this view. Implementers MUST
    /// supply this method; the trait does NOT provide a default
    /// because legacy substrate variants (`FileWholeHash`,
    /// `DerivedFactHash`) need an implementer-specific check.
    /// Per-domain implementers route the per-domain variants here
    /// via the matching `validates_*_domain` methods.
    fn validates(&self, fact: &FactVersionRef) -> bool;

    /// Validate a parse-domain fact reference (R26). Default impl
    /// returns `false`; implementers that emit parse-domain facts
    /// override.
    fn validates_parse_domain(&self, _fact: &ParseFactRef) -> bool {
        false
    }

    /// Validate a resolve-imports-domain fact reference (R26).
    /// Default impl returns `false`; the resolver implementer
    /// overrides.
    fn validates_resolve_imports_domain(&self, _fact: &ResolveImportsFactRef) -> bool {
        false
    }

    /// Validate a route-surface-domain fact reference (R26). Default
    /// impl returns `false`; the `RouteDb` implementer overrides.
    fn validates_route_surface_domain(&self, _fact: &RouteSurfaceFactRef) -> bool {
        false
    }

    /// Validate a program-analysis-domain fact reference (R26).
    /// Default impl returns `false` (fail closed); the production
    /// [`crate::resolver_store::HostStoreView`] overrides with the live
    /// `FunctionProgramIndex` whole-body hash comparison.
    fn validates_program_analysis_domain(&self, _fact: &ProgramAnalysisFactRef) -> bool {
        false
    }

    /// Validate a recorded contributor source-env identity
    /// ([`FactVersionRef::FileSourceEnv`]) STRICTLY against the
    /// view-current artifact identity.
    ///
    /// Returns `true` only when the view tracks a current artifact
    /// identity for `canonical_id` whose `parse_env_hash`,
    /// `parse_key`, and `file_language_id` all equal the recorded
    /// values. A differing, missing, tombstoned, or untracked
    /// contributor identity rejects — there is deliberately NO
    /// untracked-file optimistic accept here (unlike the lazy
    /// `FileWholeHash` arm): a contributor whose source-env identity
    /// the view cannot confirm must miss and recompute. Content
    /// validity stays on the separate `FileWholeHash` fact.
    ///
    /// Default impl returns `false` (fail closed); the production
    /// [`crate::resolver_store::HostStoreView`] overrides with the
    /// snapshot comparison.
    fn validates_file_source_env(
        &self,
        _canonical_id: &str,
        _parse_env_hash: crate::facts::fact_cache::ParseEnvHash,
        _parse_key: &verter_language::ParseKey,
        _file_language_id: &verter_language::FileLanguage,
    ) -> bool {
        false
    }

    /// Validate a **self-root** `FileWholeHash` fact strictly.
    ///
    /// A self-root is the whole-hash fact for a query-identity cache
    /// entry's OWN keyed canonical (as opposed to a cross-file
    /// dependency fact). [`Self::validates`] applies a lazy
    /// "untracked file → optimistically accept" rule to a plain
    /// `FileWholeHash`: a file loaded as a dependency after the view
    /// snapshot has no tracked hash, and forcing every such dependency
    /// through a permissive recheck would be expensive. That
    /// permissiveness is unsafe for a self-root: an untracked self-root
    /// canonical means the cache entry's own file is gone (or its
    /// content is unknown to this view), which must FAIL validation —
    /// otherwise the entry survives a same-canonical content edit.
    ///
    /// This method is the strict counterpart: an untracked or
    /// hash-mismatched self-root canonical returns `false`. The default
    /// impl delegates to [`Self::validates`] so non-production
    /// `StoreView` stubs keep their existing behavior; the production
    /// [`crate::resolver_store::HostStoreView`] overrides it to reject
    /// the untracked case. Callers that hold the explicit self-root
    /// canonical set route through
    /// [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`].
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        self.validates(&FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: *hash,
        })
    }

    /// Exact O(1) identity of the strict self-root world represented by this
    /// view, or `None` when the view cannot vouch for one.
    fn strict_self_root_world_identity(
        &self,
    ) -> Option<crate::facts::fact_cache::StrictSelfRootWorld> {
        None
    }

    /// Whether `canonical_id` has a versioned authority that can safely be
    /// represented by a strict-self-root world witness.
    fn strict_self_root_is_witnessable(&self, _canonical_id: &str) -> bool {
        false
    }

    /// Strictly validate every observed root in one stable authority world and
    /// mint its terminal witness. The before/after identity comparison closes
    /// transitions that straddle the validation loop.
    fn mint_strict_self_root_world(
        &self,
        roots: &[(&str, ResolverHash16)],
    ) -> Option<crate::facts::fact_cache::StrictSelfRootWorld> {
        let before = self.strict_self_root_world_identity()?;
        if !roots.iter().all(|(canonical, hash)| {
            self.strict_self_root_is_witnessable(canonical)
                && self.validates_self_root_whole_hash(canonical, hash)
        }) {
            return None;
        }
        (self.strict_self_root_world_identity() == Some(before)).then_some(before)
    }

    /// Whether the view tracks a specific file through its captured roots.
    ///
    /// Used by self-root attribution and route-derived cache paths to
    /// distinguish an absent canonical from a hash mismatch.
    fn tracks_file(&self, _canonical_id: &str) -> bool {
        false
    }

    /// Direct read of a view's parse-domain `DerivedFactHash` for a
    /// `(canonical, kind)` pair.
    ///
    /// Returns `Some(hash)` when the captured roots answer the pair (currently
    /// `Route`), `None` otherwise. Used by per-rejection attribution helpers
    /// (e.g. `attribute_prepared_decl_bundle_rejection`) to
    /// distinguish "entry absent" from "entry present, hash differs"
    /// without re-probing the validator with synthetic hashes.
    ///
    /// Default returns `None` so test-only / permissive views inherit
    /// "no derived fact" semantics; production `HostStoreView` overrides it.
    fn derived_hash_for(
        &self,
        _canonical_id: &str,
        _kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        None
    }

    /// This view's contribution to a fact tracer's compaction basis,
    /// captured ONCE at tracer installation.
    ///
    /// The projection exists so a tracer scope never READS a store view:
    /// composing a basis needs the two composite stamps' key dimensions
    /// and the resolution-root identity, which only a view holds, but
    /// building a view per tracer scope is an `O(store-view read)` cost on
    /// the installation path and — far hotter — on every admission
    /// boundary's movement re-check. A caller that already HOLDS a bound
    /// view borrows it here for free; the live half is composed from the
    /// host's atomics. See
    /// [`AggregateGenerations::from_seed`](crate::facts::fact_cache::AggregateGenerations::from_seed).
    ///
    /// The default is [`AggregateBasisSeed::Unvouched`]: a view that does
    /// not answer vouches for nothing, so scopes it seeds compact nothing
    /// and detect no movement. That is the fail-safe direction — the
    /// alternative, a fabricated stamp, is a witness the wrong view can
    /// satisfy.
    #[inline]
    fn aggregate_basis_seed(&self) -> crate::facts::fact_cache::AggregateBasisSeed {
        crate::facts::fact_cache::AggregateBasisSeed::Unvouched
    }

    /// **The single whole-signature validation entry point.** Every warm
    /// read that validates a stored signature goes through here.
    ///
    /// Returns `Ok(())` when every fact validates, or `Err(index)` naming
    /// the FIRST rejecting fact — the attribution the rejection-reporting
    /// readers need, so they do not re-run the loop to find it.
    ///
    /// `self_root_canonicals` names the canonicals whose `FileWholeHash`
    /// facts are the entry's OWN roots. Those route through the strict
    /// [`Self::validates_self_root_whole_hash`] — an untracked keyed
    /// canonical means the entry's own file is gone and the entry must
    /// miss — while every other fact, INCLUDING a `FileWholeHash` for a
    /// non-listed cross-file dependency, routes through the lazy
    /// [`Self::validates`], preserving cross-file permissiveness. An
    /// empty slice is therefore exactly plain whole-signature validation,
    /// which is why [`Self::validates_fact_signature`] can be a wrapper
    /// rather than a second rule.
    ///
    /// **Why one method and not eleven loops.** The default body IS
    /// `sig.iter().all(...)`, so a caller that inlines it is
    /// indistinguishable TODAY. It stops being indistinguishable the
    /// moment a view needs a rule the per-fact predicate cannot express —
    /// a whole-signature overlay snapshot or lease, a mixed-domain
    /// aggregate refusal — because a view can only state such a rule
    /// HERE. An inlined loop silently opts its cache out of it, and the
    /// symptom is a stale serve at one cache and not the others.
    ///
    /// Implementers override THIS. The two `bool` forms below are thin
    /// wrappers and exist so no caller has to spell the `Result`.
    #[inline]
    fn validate_fact_signature(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> Result<(), usize> {
        let mut walk = crate::facts::fact_cache::ReceiptWalk::default();
        for (index, fact) in sig.iter().enumerate() {
            let ok =
                crate::facts::fact_cache::validates_through_receipts(fact, &mut walk, |leaf| {
                    match leaf {
                        FactVersionRef::FileWholeHash { canonical_id, hash }
                            if self_root_canonicals.contains(&canonical_id.as_str()) =>
                        {
                            self.validates_self_root_whole_hash(canonical_id, hash)
                        }
                        other => self.validates(other),
                    }
                });
            if !ok {
                return Err(index);
            }
        }
        Ok(())
    }

    /// Validate every fact in `sig` under this view; `true` iff all
    /// validate. Empty signatures trivially return `true`.
    ///
    /// Wrapper over [`Self::validate_fact_signature`] with no self-roots.
    #[inline]
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool {
        self.validate_fact_signature(sig, &[]).is_ok()
    }

    /// Validate `sig`, treating every `FileWholeHash` whose canonical is
    /// listed in `self_root_canonicals` as a STRICT self-root.
    ///
    /// Wrapper over [`Self::validate_fact_signature`].
    #[inline]
    fn validates_fact_signature_with_self_roots(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> bool {
        self.validate_fact_signature(sig, self_root_canonicals)
            .is_ok()
    }

    /// Promote a lazily-materialised canonical's route facts into the
    /// request-scoped completion overlay.
    ///
    /// Called by the cold prepared-decl-bundle materialiser for
    /// declaration files (`.d.ts` / `.d.mts` / `.d.cts`) whose
    /// `IndexedReady` materialised AFTER the request-entry
    /// [`crate::resolver_store::HostStoreView`] snapshot was built —
    /// entries published after that snapshot are invisible to the
    /// view, so every subsequent warm-validation read of the bundle's
    /// stored derived-fact hashes would route through the base view's
    /// untracked-canonical reject and trigger a fresh cold rebuild.
    /// With promotion the next read sees the canonical as tracked, the
    /// warm validation matches, and the bundle's cold/warm ratio
    /// collapses from O(N) cold rebuilds to the expected 1:N (one cold
    /// + N-1 warm).
    ///
    /// The producer-side caller is responsible for the epoch guard
    /// (skip the call if the host's `current_store_view_epoch` no
    /// longer matches the base view's `mutation_epoch`) — keeping
    /// the trait off the concrete `VerterHost` type to preserve the
    /// request-port boundary (the six ports in `request_ports`).
    ///
    /// Implementers writing into a per-request overlay must:
    /// - Insert `whole_hash` into the overlay's `whole_hashes` map
    ///   (so `validates_self_root_whole_hash` accepts the bundle's
    ///   `FileWholeHash` self-root).
    /// - Insert `route_hash` into the overlay's `derived_hashes` under
    ///   the `Route` kind when `Some`.
    ///
    /// The owner's import-route dependency is deliberately NOT promoted:
    /// it is a resolve-domain resolution witness validated against the
    /// base view's captured immutable resolution world, not a
    /// per-canonical derived hash the overlay can carry.
    ///
    /// Default impl is no-op so non-request views (the bare
    /// [`crate::resolver_store::HostStoreView`], test-only
    /// [`PermissiveStoreView`], etc.) inherit "no overlay" semantics
    /// — they have no per-request append-only side maps to mutate.
    fn promote_route_completion(
        &self,
        _canonical: &str,
        _whole_hash: crate::analysis::types::Hash16,
        _route_hash: Option<crate::analysis::types::Hash16>,
    ) {
    }
}

impl crate::facts::fact_cache::FactVersionValidator for dyn StoreView + '_ {
    #[inline]
    fn validates_fact_version(&self, fact: &FactVersionRef) -> bool {
        StoreView::validates(self, fact)
    }

    /// The view's own whole-signature rule, one receipt walk shared by
    /// the signature, never a fresh walk per fact.
    #[inline]
    fn validates_fact_signature(&self, facts: &[FactVersionRef]) -> bool {
        StoreView::validates_fact_signature(self, facts)
    }
}

/// Forward [`StoreView`] through a shared reference, including the unsized
/// `&dyn StoreView` form.
///
/// This lets a generic `view: &V where V: StoreView` validator accept a
/// `&crate::resolver_core::fact_validation_port::FactValidationView::new(ctx)` borrow (`&dyn StoreView`) directly — e.g. the
/// fallthrough resolver validates per-element / per-child / per-root
/// node-cache entries through `&crate::resolver_core::fact_validation_port::FactValidationView::new(self.ctx)` so the validation
/// rides the request-bound, currentness-gated `RequestStoreView` rather
/// than a separately-rebuilt raw `HostStoreView`. Every method just
/// re-dispatches to the referent.
impl<T: StoreView + ?Sized> StoreView for &T {
    #[inline]
    fn compat_token(&self) -> StoreViewCompatToken {
        (**self).compat_token()
    }
    #[inline]
    fn validates(&self, fact: &FactVersionRef) -> bool {
        (**self).validates(fact)
    }
    #[inline]
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        (**self).validates_parse_domain(fact)
    }
    #[inline]
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool {
        (**self).validates_resolve_imports_domain(fact)
    }
    #[inline]
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool {
        (**self).validates_route_surface_domain(fact)
    }
    #[inline]
    fn validates_program_analysis_domain(&self, fact: &ProgramAnalysisFactRef) -> bool {
        (**self).validates_program_analysis_domain(fact)
    }
    #[inline]
    fn validates_file_source_env(
        &self,
        canonical_id: &str,
        parse_env_hash: crate::facts::fact_cache::ParseEnvHash,
        parse_key: &verter_language::ParseKey,
        file_language_id: &verter_language::FileLanguage,
    ) -> bool {
        (**self).validates_file_source_env(
            canonical_id,
            parse_env_hash,
            parse_key,
            file_language_id,
        )
    }
    #[inline]
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        (**self).validates_self_root_whole_hash(canonical_id, hash)
    }
    #[inline]
    fn strict_self_root_world_identity(
        &self,
    ) -> Option<crate::facts::fact_cache::StrictSelfRootWorld> {
        (**self).strict_self_root_world_identity()
    }
    #[inline]
    fn strict_self_root_is_witnessable(&self, canonical_id: &str) -> bool {
        (**self).strict_self_root_is_witnessable(canonical_id)
    }
    #[inline]
    fn mint_strict_self_root_world(
        &self,
        roots: &[(&str, ResolverHash16)],
    ) -> Option<crate::facts::fact_cache::StrictSelfRootWorld> {
        (**self).mint_strict_self_root_world(roots)
    }
    #[inline]
    fn tracks_file(&self, canonical_id: &str) -> bool {
        (**self).tracks_file(canonical_id)
    }
    #[inline]
    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        (**self).derived_hash_for(canonical_id, kind)
    }
    #[inline]
    fn aggregate_basis_seed(&self) -> crate::facts::fact_cache::AggregateBasisSeed {
        (**self).aggregate_basis_seed()
    }
    #[inline]
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool {
        (**self).validates_fact_signature(sig)
    }
    #[inline]
    fn validate_fact_signature(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> Result<(), usize> {
        (**self).validate_fact_signature(sig, self_root_canonicals)
    }
    #[inline]
    fn validates_fact_signature_with_self_roots(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> bool {
        (**self).validates_fact_signature_with_self_roots(sig, self_root_canonicals)
    }
    #[inline]
    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: crate::analysis::types::Hash16,
        route_hash: Option<crate::analysis::types::Hash16>,
    ) {
        (**self).promote_route_completion(canonical, whole_hash, route_hash)
    }
}
