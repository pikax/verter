//! Shared helpers for the R3/R26/R28 fact-based validation substrate
//! used by the inner component-meta caches.
//!
//! Every cache entry carries a [`ReadSetSignature`] whose `facts`
//! rail is `Arc<[FactVersionRef]>` — the cold-compute observation set
//! the producer recorded. It is the sole cache-validity rail. Warm-hit
//! reads call [`validate_fact_signature`] which walks every fact
//! through the current [`crate::resolver_core::StoreView`] snapshot; a
//! single mismatch returns `false` and the warm hit misses, falling
//! through to cold recompute.
//!
//! Bubble-up is the dual rule: when a cache cold-compute or warm-hit
//! returns a value to its caller, the entry's fact signature merges
//! into any active outer tracer via
//! [`crate::resolver_core::FactReadSetCell::observe_borrowed_signature`].
//! That keeps the outer compute's observation set complete even when
//! inner reads come from cache.
//!
//! ## Path-precise parse facts (R28)
//!
//! Beneath the self-root (below), two helper shapes carry the Family A
//! path-precise parse facts:
//!
//! - [`fact_signature_for_canonical_member`] — keyed on `(canonical,
//!   exporter, member, space)`. The cold path reads the `Member`
//!   body fingerprint and `MemberPresence` header fact for the
//!   member.
//! - [`fact_signature_for_exported_type`] — keyed on `(canonical,
//!   type_name, space)`. The cold path observes the top-level type
//!   identity via `Export`, `LocalDecl`, and `MemberShape` facts;
//!   adding/removing/renaming a member shifts `MemberShape`.
//!
//! For caches whose key shape does NOT include a member name (e.g.
//! `AppConfigNoOverrideProofKey`), the cold path observes the
//! `SyntacticExportSet` of the contributing canonical instead. These
//! parse facts remain a refinement for cross-file consumers; the
//! self-root below is the always-on same-file edit detector.
//!
//! ## Self-version rooting (provenance-pure)
//!
//! Each of the three central helpers leads with a self-root
//! `FactVersionRef::FileWholeHash` for the defining/key canonical it
//! represents, then adds the path-precise parse facts above. The
//! self-root is the whole-hash fact for the cache entry's OWN keyed
//! canonical: any byte change to that file shifts its whole hash, so
//! a warm read that validates the self-root via
//! [`validate_fact_signature_with_self_roots`] detects a
//! same-canonical content edit and recomputes. The path-precise parse
//! facts still gate sibling-edit reuse — the self-root augments them
//! for correctness-first closure, it does not replace them.
//!
//! The three central helpers are **provenance-pure**: they never
//! consult the authoritative current-content oracle and never re-read
//! current content. The keyed canonical's content identity is a
//! caller-supplied `observed_hash` — the content version the
//! producer's value was actually computed against, captured exactly
//! once at the value source and threaded into the builder. Both the
//! self-root `FileWholeHash` and every `Parse` fact are pinned to that
//! observed version (`Parse` facts via
//! [`parse_fact_ref_for_observed_current_content`], a content-addressed
//! [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//! lookup). Re-reading the canonical's *current* hash inside a
//! signature builder would open a publish race: an `upsert` landing
//! between value-compute and signature-build would root a stale value
//! by a fresh-looking current hash, which then validates on warm
//! reads instead of missing. Each builder returns `None` — refusing
//! shared-cache admission — when the observed version's parse-fact
//! registry cannot be recovered.

use std::sync::Arc;

use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, SymbolSpace};

use crate::cache_runtime::{NonAdmissionReason, SignatureAdmission};
use crate::resolver_core::{
    FactReadSetFinalise, FactVersionRef, ParseFactRef, ResolverContext, StoreView,
    FACT_SIGNATURE_CAP,
};
use crate::semantic_query::{DepSignature, DepVersion};
use crate::types::Hash16;

/// Bracket one cold-compute closure with a push-style fact tracer.
///
/// Installs a fresh [`crate::resolver_core::FactReadSetCell`] onto the
/// TLS tracer stack, runs `f`, pops the tracer, and finalises the
/// observation set. On [`FactReadSetFinalise::Overflow`] emits a
/// [`crate::component_meta_audit::StructuredAuditEvent::FactSignatureOverflow`]
/// and increments the host's per-host
/// [`crate::VerterHost::signature_overflow_at_install`] counter.
///
/// Returns `(return_value, finalise_result, non_cacheable_read_observed)` so
/// callers decide whether to admit the result to cache or treat it as
/// non-cacheable. `non_cacheable_read_observed == true` means the traced
/// compute consumed a FENCED (ReturnOnly, `store_published == false`)
/// `IndexedReady` serve: the result's fact stamps are read from the
/// LIVE post-mutation state while its payload was computed FROM the
/// superseded artifact — an entry the read-side fact rail cannot
/// reject, so every shared-cache admission point MUST refuse it
/// (serve the value to the caller, publish nothing).
pub(crate) fn install_fact_tracer<F, R>(host: &crate::VerterHost, f: F) -> (R, FactReadSetFinalise)
where
    F: FnOnce() -> R,
{
    let (value, read_set) = host.with_fact_tracer(|| {
        #[cfg(test)]
        force_tracer_overflow_observations(host, None);
        f()
    });
    let finalise = read_set.finalise();
    // The overflow audit event + host counter are emitted HERE and ONLY here —
    // at the ONE signature-CONSUMING boundary per compute. The cacheability
    // scope below deliberately uses the non-emitting `would_overflow` peek: an
    // inner overflow fans into every enclosing tracer, so an emitting nested
    // peek would multiply a single overflowing compute's event and counter
    // across each nesting level.
    if matches!(finalise, FactReadSetFinalise::Overflow) {
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::FactSignatureOverflow {
                candidate_size: (FACT_SIGNATURE_CAP as u32).saturating_add(1),
                cap: FACT_SIGNATURE_CAP as u32,
            },
        );
        host.signature_overflow_at_install
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    (value, finalise)
}

/// Test-only fact-injection hook read at every tracer scope entry
/// ([`install_fact_tracer`] and [`with_cacheability_scope`]). When a knob is
/// non-zero, fan that many synthetic `FileWholeHash` observations into the
/// freshly-installed tracer (and every enclosing one), so the scope
/// deterministically reports overflow once the per-signature cap is exceeded —
/// the in-process equivalent of a compute whose observation set genuinely
/// exceeds [`FACT_SIGNATURE_CAP`], without a pathological workspace fixture.
///
/// `scope` is the entering scope's ADDRESSABLE identity: `Some(_)` for a scope
/// opened through [`named_cacheability_scope`] / [`named_fact_tracer`], `None`
/// for every other (unnamed) scope in the crate.
///
/// TWO knobs, deliberately:
///
/// - the PER-HOST STICKY `force_fact_tracer_overflow_observations` overflows
///   EVERY scope in the flow, named or not — the right tool when the boundary
///   under test is the only one whose overflow can refuse the publication;
/// - the THREAD-SCOPED TARGETED ONE-SHOT
///   (`host_test_force::arm_fact_tracer_overflow_once`) is claimed by the NAMED
///   scope it was armed for, on the arming thread, and overflows that scope
///   ALONE. It is the seam for a flow with TWO tracers where either overflow
///   would independently refuse the same write: the sticky knob is
///   non-discriminating there (the test passes even if the boundary under test
///   drops its overflow), while the one-shot isolates the NAMED scope and proves
///   that boundary's rail on its own.
///
/// The one-shot is claimed by scope IDENTITY, never by scope ORDER. An
/// order-keyed one-shot is silently RETARGETED by any tracer scope added
/// upstream of the scope under test — the test keeps passing while testing a
/// different boundary. Here an unnamed upstream scope passes `None`, claims
/// nothing, and leaves the one-shot armed for its intended claimant; a named
/// scope claims only when it IS the armed target.
///
/// Placed at the SHARED installer so EVERY traced admission boundary runs it
/// rather than relying on a boundary-specific hook — a production site that
/// reverts to a raw, overflow-discarding tracer still fans the observations and
/// still fails the test. The production build compiles it out.
#[cfg(test)]
fn force_tracer_overflow_observations(
    host: &crate::VerterHost,
    scope: Option<crate::host_test_force::TracerScope>,
) {
    let sticky = host
        .test_force
        .force_fact_tracer_overflow_observations
        .load(std::sync::atomic::Ordering::Relaxed);
    let once = crate::host_test_force::claim_fact_tracer_overflow_once(scope);
    for i in 0..sticky.max(once) {
        crate::resolver_core::resolver_context::observe_fan_out(FactVersionRef::FileWholeHash {
            canonical_id: format!("__force_tracer_overflow_{i}.ts"),
            hash: [(i & 0xff) as u8; 16],
        });
    }
}

/// [`with_cacheability_scope`] for a scope that carries an ADDRESSABLE
/// [`TracerScope`](crate::host_test_force::TracerScope) identity, so a test can
/// target it by NAME with the one-shot overflow knob.
///
/// Reached only through the [`named_cacheability_scope`] macro, whose production
/// arm expands to the plain, unnamed opener — the identity exists in test builds
/// alone.
#[cfg(test)]
fn with_cacheability_scope_named<F, R>(
    host: &crate::VerterHost,
    scope: crate::host_test_force::TracerScope,
    f: F,
) -> (R, bool)
where
    F: for<'t> FnOnce(&CacheabilityProbe<'t>) -> R,
{
    let (value, mut read_set) = host.with_fact_tracer_cell(|cell| {
        force_tracer_overflow_observations(host, Some(scope));
        f(&CacheabilityProbe { cell })
    });
    let non_cacheable = read_set.non_cacheable_read_observed() || read_set.would_overflow();
    (value, non_cacheable)
}

/// [`install_fact_tracer_cacheability`] for an ADDRESSABLE scope. See
/// [`with_cacheability_scope_named`].
#[cfg(test)]
pub(crate) fn install_fact_tracer_cacheability_named<F, R>(
    host: &crate::VerterHost,
    scope: crate::host_test_force::TracerScope,
    f: F,
) -> (R, bool)
where
    F: FnOnce() -> R,
{
    with_cacheability_scope_named(host, scope, |_probe| f())
}

/// [`install_fact_tracer`] for an ADDRESSABLE scope. See
/// [`with_cacheability_scope_named`].
#[cfg(test)]
pub(crate) fn install_fact_tracer_named<F, R>(
    host: &crate::VerterHost,
    scope: crate::host_test_force::TracerScope,
    f: F,
) -> (R, FactReadSetFinalise)
where
    F: FnOnce() -> R,
{
    let (value, read_set) = host.with_fact_tracer(|| {
        force_tracer_overflow_observations(host, Some(scope));
        f()
    });
    let finalise = read_set.finalise();
    if matches!(finalise, FactReadSetFinalise::Overflow) {
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::FactSignatureOverflow {
                candidate_size: (FACT_SIGNATURE_CAP as u32).saturating_add(1),
                cap: FACT_SIGNATURE_CAP as u32,
            },
        );
        host.signature_overflow_at_install
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    (value, finalise)
}

/// Open an [`install_fact_tracer_cacheability`] scope that a test can TARGET BY
/// NAME.
///
/// ```ignore
/// named_cacheability_scope!(host, TracerScope::ScriptFactsImportRoute, || { .. })
/// ```
///
/// ZERO production footprint: the non-test arm expands to the plain
/// [`install_fact_tracer_cacheability`] call and DROPS the scope tokens entirely
/// — no argument, no `&'static` datum, no type. The identity exists only where a
/// test can consume it, and the production build is byte-identical to the unnamed
/// call it replaces.
///
/// Naming a scope is what makes the one-shot overflow knob TARGETED instead of
/// positional: the knob is claimed by identity, so a tracer scope added anywhere
/// UPSTREAM cannot silently retarget it (an unnamed scope claims nothing).
#[cfg(test)]
macro_rules! named_cacheability_scope {
    ($host:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer_cacheability_named($host, $scope, $f)
    };
}

#[cfg(not(test))]
macro_rules! named_cacheability_scope {
    ($host:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer_cacheability($host, $f)
    };
}

/// Open an [`install_fact_tracer`] scope that a test can TARGET BY NAME. The
/// signature-CONSUMING sibling of [`named_cacheability_scope`]; same zero
/// production footprint.
#[cfg(test)]
macro_rules! named_fact_tracer {
    ($host:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer_named($host, $scope, $f)
    };
}

#[cfg(not(test))]
macro_rules! named_fact_tracer {
    ($host:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer($host, $f)
    };
}

pub(crate) use {named_cacheability_scope, named_fact_tracer};

/// Proof that the current compute runs inside a CACHEABILITY TRACER SCOPE —
/// the token every shared-cache admission point requires.
///
/// A [`CacheabilityProbe`] can be minted ONLY by [`with_cacheability_scope`],
/// and the borrow it hands out cannot outlive that scope's closure. An
/// admission API that takes `&CacheabilityProbe` therefore CANNOT be reached
/// from a producer that installed no tracer — the untraced-producer class is
/// closed by the type system.
///
/// [`Self::non_cacheable`] reads the scope's verdict-so-far. The tracer
/// accumulates monotonically, so a read taken at the admission point (the END
/// of the value's compute) covers everything the compute consumed — provided
/// the scope ENCLOSES that compute. That is the discipline every producer
/// follows: the scope is the OUTERMOST bracket of the producer body, so key
/// computation, gate classification, lowering, peek, and reduce all lie inside
/// it and a pre-tracer read point cannot exist.
///
/// **`pub` but UNNAMEABLE.** The enclosing module is `pub(crate)`, so no
/// out-of-crate caller can write this type — it is `pub` only so it can appear
/// in the signature of a shared-cache funnel that IS out-of-crate reachable
/// (`RouteDb`, `ImportedRootDb`). Such a caller obtains one the only way anyone
/// does: by opening a real scope (`for_tests::with_cacheability_scope_for_tests`)
/// and receiving the borrow. The `cell` field stays private, so the token
/// cannot be constructed by struct literal either.
pub struct CacheabilityProbe<'t> {
    cell: &'t crate::resolver_core::FactReadSetCell,
}

impl CacheabilityProbe<'_> {
    /// `true` when the enclosing scope's compute MUST NOT warm any shared
    /// cache. TWO INDEPENDENT non-admission conditions fold into it:
    ///
    /// 1. a NON-CACHEABLE READ — a FENCED (ReturnOnly, `store_published ==
    ///    false`) `IndexedReady` serve, a broken decl-body lease
    ///    (`LeaseMiss`), an unrootable / unadmitted import route, or an
    ///    unobservable contributor source env. The value was derived from a
    ///    served-without-publication / transient basis while its fact stamps
    ///    read the LIVE view, so the read-side fact rail cannot reject the
    ///    entry.
    /// 2. a fact-signature OVERFLOW — the compute observed more than
    ///    [`FACT_SIGNATURE_CAP`] distinct facts.
    ///
    /// Both are CACHE-ONLY: the value stays `Complete` and flows to the caller
    /// verbatim; only the shared-cache admission is refused (never
    /// `ResultCompleteness::Partial`).
    ///
    /// # Why an overflow refuses here
    ///
    /// NOT because the entry would be unrootable — at these boundaries it
    /// WOULD be rootable: the entry's `ReadSetSignature.facts` is built from
    /// ANOTHER source (the carrier's `dep_signature` via
    /// `engine_fact_signature_for_materialize_memo`, or the keyed canonical's
    /// observed hash), never from this tracer's finalised set, and that
    /// curated signature is well under the cap. The refusal is a conservative
    /// POLICY: an over-cap observation set means the compute read MORE than
    /// the curated signature enumerates, so we can no longer prove the
    /// signature COVERS everything the value depends on — a warm hit could
    /// validate the curated facts while an unenumerated dependency has moved.
    /// The rail therefore has a real cost (a legitimately fact-heavy compute
    /// is recomputed cold forever); it is not free correctness bookkeeping.
    #[inline]
    pub fn non_cacheable(&self) -> bool {
        // Overflow is peeked, never finalised: no `Arc<[FactVersionRef]>`
        // allocation on the hot cold-member path, and no audit event — the
        // event stays owned by the ONE signature-consuming `install_fact_tracer`
        // boundary per compute (see its emission site).
        self.cell.non_cacheable_read_observed() || self.cell.would_overflow()
    }
}

/// Open a CACHEABILITY TRACER SCOPE around a producer's ENTIRE compute and hand
/// it the scope's [`CacheabilityProbe`].
///
/// **The scope must be the OUTERMOST bracket of the producing function.** An
/// admission into a shared cache is fail-closed only if its whole compute — key
/// computation, gate classification, lowering, peek, and reduce — runs inside a
/// cacheability tracer. A tracer that starts LATE (after the lowering, say)
/// leaves a pre-tracer read point whose fenced serve is never observed, and no
/// downstream re-observation is guaranteed: structural-transit reduction does
/// not descend into composite children, so a nested reference resolved during
/// lowering is never re-read by the reduce.
///
/// Returns `(value, non_cacheable)` — the same verdict [`CacheabilityProbe`]
/// reports, sampled once after the scope pops, for a producer that admits
/// AFTER its compute rather than inside it.
pub fn with_cacheability_scope<F, R>(host: &crate::VerterHost, f: F) -> (R, bool)
where
    F: for<'t> FnOnce(&CacheabilityProbe<'t>) -> R,
{
    let (value, mut read_set) = host.with_fact_tracer_cell(|cell| {
        #[cfg(test)]
        force_tracer_overflow_observations(host, None);
        f(&CacheabilityProbe { cell })
    });
    let non_cacheable = read_set.non_cacheable_read_observed() || read_set.would_overflow();
    (value, non_cacheable)
}

/// [`with_cacheability_scope`] for a producer whose admission happens AFTER the
/// traced compute returns, so it needs the verdict but not the in-scope probe.
///
/// Use this entry at every admission boundary whose entry signature is built
/// from ANOTHER source (the carrier's `dep_signature`, the keyed canonical's
/// observed hash) rather than from this tracer's finalised set — those callers
/// have no other place to see an overflow, and folding it into the verdict here
/// makes it impossible to drop. A caller that DOES consume the finalised
/// signature (building the entry's `ReadSetSignature` from it) uses
/// [`install_fact_tracer`] and routes `Overflow` through
/// [`SignatureAdmission::from_finalise`].
pub(crate) fn install_fact_tracer_cacheability<F, R>(host: &crate::VerterHost, f: F) -> (R, bool)
where
    F: FnOnce() -> R,
{
    with_cacheability_scope(host, |_probe| f())
}

/// Fan `sig` into every active tracer on the current thread's stack.
///
/// Thin wrapper around
/// [`crate::resolver_core::resolver_context::observe_fan_out_borrowed`]
/// with a more intention-revealing name for callers in the
/// fact-cache substrate.
#[inline]
pub(crate) fn observe_fact_signature(sig: &[FactVersionRef]) {
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(sig);
}

/// Record a contributor SOURCE-ENV identity observation
/// ([`FactVersionRef::FileSourceEnv`]) from the EXACT artifact key the
/// contributor read actually used.
///
/// `canonical_id`, `parser_version`, and `file_language_id` are sourced
/// from `artifact_key` itself, never re-derived from a canonical/path
/// at the recording site and never read back from an index entry that
/// could be stale. The `parse_env_hash` dimension is DIFFERENT: it is
/// the canonical's LIVE per-canonical parse env — the SAME dimension
/// the contributor `LowerLocator` body-source key folds — sourced
/// through the shared
/// [`crate::resolver_store::SourceEnvIdentity::live_for_artifact_key`]
/// construction the validate-side snapshot seeding also uses, so
/// record and validate compare the same dimension by construction. The
/// key's own `parse_env_hash` slot must NOT be copied into the fact: a
/// base key carries the zero sentinel there and an overlay-scoped key
/// a session discriminator — neither is an env identity, and a copied
/// sentinel would make a live parse-env move (content unchanged)
/// invisible to the rail. The fact is recorded onto the active fact
/// tracer (via [`ResolverContext::observe`]) and returned so the
/// caller can fold it into a producer-built signature.
///
/// `artifact_key = None` — the read could not supply the exact key it
/// served from — means the contributor's coherent 4-field identity is
/// UNOBSERVABLE: the API returns `None` and records NOTHING (never a
/// fabricated default). The caller must route the surrounding result
/// through `ReturnOnly` (no warm admission), matching the
/// unobservable-fact convention of the sibling
/// [`parse_fact_ref_for_observed_current_content`] builder.
///
/// The recording site is the cross-file module-augmentation contributor
/// fold (`collect_augmentation_contributions`): one observation per
/// contributor body folded into a parent value, so a warm parent hit
/// revalidates each contributor's source-env identity against the live
/// view.
pub(crate) fn observe_file_source_env_from_artifact_key(
    ctx: &dyn ResolverContext,
    artifact_key: Option<&crate::file_artifact_store::FileArtifactKey>,
) -> Option<FactVersionRef> {
    let key = artifact_key?;
    let identity = crate::resolver_store::SourceEnvIdentity::live_for_artifact_key(
        ctx.host_for_fact_tracer_install(),
        key,
    );
    let fact = FactVersionRef::FileSourceEnv {
        canonical_id: key.canonical.as_ref().to_owned(),
        parse_env_hash: identity.parse_env_hash,
        parser_version: identity.parser_version,
        file_language_id: identity.file_language_id,
    };
    ctx.observe(fact.clone());
    Some(fact)
}

/// Convert a [`DepSignature`] into a [`Vec<FactVersionRef>`] — the
/// bridge that fans a dispatch sub-query's recorded dependency set
/// into the active fact tracer.
///
/// Per-version mapping (no generation dep is silently dropped):
///
/// - `WholeHash` → `FileWholeHash`.
/// - `ProjectGeneration` → `FactVersionRef::ProjectGeneration` — the
///   project-wide generation a sub-result depended on. Dropping it
///   would let an outer entry that observed the sub-result through the
///   tracer validate against a superseded project shape.
/// - `RouteGeneration` is **not expressible** as a `FactVersionRef` —
///   there is no `FactVersionRef::RouteGeneration` variant (route
///   generation has no authoritative validating source) — so it is
///   skipped. No production path constructs `DepVersion::RouteGeneration`;
///   this arm is the defensive floor.
pub(crate) fn dep_signature_to_fact_signature(sig: &DepSignature) -> Vec<FactVersionRef> {
    sig.iter()
        .filter_map(|(canon, ver)| match ver {
            DepVersion::WholeHash(h) => Some(FactVersionRef::FileWholeHash {
                canonical_id: canon.as_ref().to_string(),
                hash: *h,
            }),
            DepVersion::ProjectGeneration(generation) => Some(FactVersionRef::ProjectGeneration {
                generation: *generation,
            }),
            DepVersion::RouteGeneration(_) => None,
        })
        .collect()
}

/// Read the current value of `host`'s per-host
/// [`crate::VerterHost::signature_overflow_at_install`] counter.
///
/// Exposed for integration tests that verify overflow telemetry —
/// reached through the `for_tests::read_signature_overflow_at_install`
/// re-export in `lib.rs` (see
/// `tests/cases/g_fact/fact_read_set_finalise_overflow.rs`). The `for_tests`
/// shim is gated `cfg(any(test, feature = "test-support"))`; this accessor
/// matches so it is not a dead symbol in release.
#[cfg(any(test, feature = "test-support"))]
#[inline]
pub(crate) fn read_signature_overflow_at_install(host: &crate::VerterHost) -> u64 {
    host.signature_overflow_at_install
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// Walk every `FactVersionRef` in `signature` against the current
/// resolver-store view; return `false` on the first mismatch.
///
/// `O(signature.len())`; zero allocation on the empty path. Empty
/// signatures trivially validate (callers that never observed a fact
/// have no R3 oracle to consult — typical for cache entries produced
/// outside an installed tracer scope; the cache stays correct under
/// the legacy whole-hash regime).
///
/// This is the lazy (non-strict) validator. Production warm reads use
/// [`validate_fact_signature_with_self_roots`]; the only consumer of
/// the lazy form is the `cfg(any(test, feature = "test-support"))`-gated
/// `AppConfigNoOverrideProofDb::peek` plus the substrate test suite, so
/// it is gated to match (no dead surface in release).
#[cfg(any(test, feature = "test-support"))]
#[inline]
#[track_caller]
pub(crate) fn validate_fact_signature(
    ctx: &dyn ResolverContext,
    signature: &[FactVersionRef],
) -> bool {
    if signature.is_empty() {
        return true;
    }
    // Context-aware dispatch.
    //
    // Request-bound contexts (`HostResolverContext`,
    // `SessionResolverContext`) expose the request-entry-snapshotted
    // overlay-aware view through `store_view()` (borrowed; layered
    // overlay+base). Bare-host contexts (`impl ResolverContext for
    // VerterHost`) cannot return a borrowed view (the host owns no
    // long-lived snapshot) so they must rebuild an owned
    // `HostStoreView` through `resolver_store_view()`.
    //
    // Two branches that perform validation INSIDE the matched arm —
    // returning a `&dyn StoreView` from a single `let view = ...`
    // call would borrow a temporary in the bare-host arm and drop
    // before validation runs.
    if ctx.is_request_bound() {
        let view = ctx.store_view();
        signature.iter().all(|fact| view.validates(fact))
    } else {
        let view = ctx.resolver_store_view();
        signature.iter().all(|fact| view.validates(fact))
    }
}

/// Walk `signature` against the current resolver-store view, but
/// validate any `FileWholeHash` fact whose canonical appears in
/// `self_root_canonicals` **strictly** — an untracked or mismatched
/// self-root canonical fails validation.
///
/// This is the validation entry point for a query-identity cache
/// whose `read_set_signature` carries a self-root `FileWholeHash` for
/// its own keyed canonical (the signature shape produced by the three
/// central fact-signature helpers above). [`validate_fact_signature`]
/// alone routes a `FileWholeHash` through the lazy
/// [`crate::resolver_core::StoreView::validates`] rule, whose
/// untracked-file arm optimistically accepts: that is correct for a
/// cross-file *dependency* fact (loaded after the view snapshot) but
/// wrong for a *self-root*, where an untracked keyed canonical means
/// the entry's own file is gone and the entry must miss.
///
/// `self_root_canonicals` is the explicit self-root-vs-dependency
/// distinction: a `FileWholeHash` whose canonical is listed is a
/// self-root and routes through the strict
/// [`crate::resolver_core::StoreView::validates_self_root_whole_hash`];
/// every other fact (including a `FileWholeHash` for a non-listed
/// cross-file dependency) routes through the lazy `validates`, so
/// cross-file lazy permissiveness is preserved. Empty signatures
/// trivially validate.
///
/// This is the warm-read validation entry point for the
/// component-meta query-identity caches: each cache passes its own
/// keyed canonical(s) as `self_root_canonicals`, so a same-canonical
/// content edit — or a keyed canonical that became untracked — fails
/// validation strictly and the warm read recomputes.
#[inline]
#[track_caller]
pub(crate) fn validate_fact_signature_with_self_roots(
    ctx: &dyn ResolverContext,
    signature: &[FactVersionRef],
    self_root_canonicals: &[&str],
) -> bool {
    if signature.is_empty() {
        return true;
    }
    // Context-aware dispatch.
    //
    // Same dispatch rationale as [`validate_fact_signature`] above —
    // request-bound contexts validate against the borrowed
    // overlay-aware view; bare-host contexts rebuild an owned view.
    // Both arms apply the strict `validates_self_root_whole_hash`
    // rule for canonicals listed in `self_root_canonicals` (a keyed
    // canonical that became untracked fails the warm-read validation
    // strictly).
    if ctx.is_request_bound() {
        let view = ctx.store_view();
        signature.iter().all(|fact| match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if self_root_canonicals.contains(&canonical_id.as_str()) =>
            {
                view.validates_self_root_whole_hash(canonical_id, hash)
            }
            other => view.validates(other),
        })
    } else {
        let view = ctx.resolver_store_view();
        signature.iter().all(|fact| match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if self_root_canonicals.contains(&canonical_id.as_str()) =>
            {
                view.validates_self_root_whole_hash(canonical_id, hash)
            }
            other => view.validates(other),
        })
    }
}

/// Bubble `signature` into **all** active fact tracers on the current
/// thread's stack (fan-out). Called by both cold-compute and warm-hit
/// paths so every outer tracer scope sees every transitive fact the
/// inner cache hit / produced.
#[inline]
pub(crate) fn bubble_fact_signature(_ctx: &dyn ResolverContext, signature: &[FactVersionRef]) {
    if signature.is_empty() {
        return;
    }
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(signature);
}

/// Variant of [`bubble_fact_signature`] for warm-hit paths that
/// don't carry a `ResolverContext` reference (e.g. the semantic
/// graph store's fast-path warm hit). Fans into all active TLS
/// tracer scopes when any are installed; no-op otherwise.
#[inline]
pub(crate) fn bubble_fact_signature_via_tls(signature: &[FactVersionRef]) {
    if signature.is_empty() {
        return;
    }
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(signature);
}

/// Sentinel hash returned when the producer requests a fact that the
/// FileFacts registry hasn't materialised yet (e.g. cold-compute
/// races a parse that hasn't published yet). Validator reads against
/// the registry's actual hash; a sentinel records "MUST be absent"
/// semantics so a later population (or its absence) is still
/// discriminating.
#[inline]
fn zero_hash() -> Hash16 {
    [0u8; 16]
}

/// Build a [`ParseFactRef`] for `(canonical_id, key, lane)` pinned to a
/// caller-supplied **observed** content hash — a provenance-pure parse
/// fact that records the file identity a producer actually observed,
/// not whatever content is current at signature-build time.
///
/// This does NOT consult
/// [`ResolverContext::authoritative_current_content_hash`] and never
/// reads whatever content is current at signature-build time. It
/// performs a
/// content-addressed [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
/// lookup keyed on the passed `observed_content_hash`: the parse
/// registry it reads is the one published for exactly that content
/// version. The emitted fact's `expected_hash` is therefore the fact
/// hash that was live when the producer observed the file, regardless
/// of any edit that has landed since.
///
/// ## Two-identity recovery — raw owner vs analysis canonical
///
/// `canonical_id` is the **raw** owner the caller observed. Two ids are
/// in play and they MUST NOT be conflated:
///
/// * The **artifact-store lookup** is keyed by
///   `normalized_analysis_canonical(canonical_id)` — every
///   `FileArtifactStore` artifact (base via [`ResolverContext::ensure_indexed_ready_serve`],
///   overlay via the overlay materialiser) is published under the
///   normalised analysis canonical as `FileArtifactKey::canonical`. A
///   lookup keyed by the raw owner misses the artifact whenever
///   `normalize(raw) != raw` (a runtime `.js` with a `.d.ts`
///   companion) — the recovery would then return `None` even though the
///   observed parse facts exist.
/// * The emitted **`ParseFactRef.canonical_id`** stays the RAW owner the
///   caller passed. The parse-domain validator
///   ([`crate::resolver_core::StoreView::validates_parse_domain`]) keys
///   the per-file `FileFacts` snapshot by the canonical the view tracks:
///   an overlay-bearing canonical is re-rooted in
///   [`crate::resolver_store::HostStoreView::with_session_overlay`] under
///   the RAW overlay owner, and the materialize-memo signature builder
///   (`engine_fact_signature_for_materialize_memo`) requires the parse
///   fact's id to equal the observation's raw scope id. Normalising the
///   emitted id would break both.
///
/// `None` is returned when no artifact is cached for the
/// `(analysis_canonical, observed_content_hash)` identity — the observed
/// version's parse facts cannot be recovered, so the caller must refuse
/// shared-cache admission rather than emit a fact rooted on a guessed
/// hash.
///
/// This is the construction primitive for a cache entry whose value
/// was materialised against a specific observed file version: the
/// entry's parse fact MUST be pinned to that same observed version so
/// a warm read after a content edit genuinely misses, instead of
/// validating the observed-version fact against post-edit content. The
/// bare `ParseFactRef` (not a wrapped `FactVersionRef`) is returned so
/// a producer that roots the same canonical multiple ways can place it
/// exactly once.
pub(crate) fn parse_fact_ref_for_observed_current_content(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    observed_content_hash: Hash16,
    key: FactKey,
    lane: FactLane,
) -> Option<ParseFactRef> {
    // Content-addressed by `(analysis_canonical, observed_content_hash)`
    // — explicitly NOT view-dependent. The looked-up `FileFacts`
    // registry is parse-domain and content-derived: a base artifact
    // (base key) and a session-overlay artifact (overlay-scoped key)
    // for the SAME content version carry an identical parse-fact
    // registry, so the `parse_env_hash` key dimension is irrelevant
    // here. `get_artifacts_for_content` scans for a `FileArtifacts`
    // matching the `(canonical, content_hash)` pair regardless of
    // `parse_env_hash`, so a producer recovers the same observed parse
    // fact whether or not it ran under a session view.
    //
    // The lookup is keyed by the NORMALISED analysis canonical — the
    // `FileArtifactKey::canonical` identity every artifact is published
    // under. Keying by the raw `canonical_id` misses the artifact when
    // `normalize(raw) != raw` (a `.js` with a `.d.ts` companion); the
    // emitted `ParseFactRef.canonical_id` below stays the raw owner the
    // validator expects.
    let analysis_canonical = ctx.normalized_analysis_canonical(canonical_id);
    let artifacts = ctx
        .project_type_store()
        .indexed()
        .get_artifacts_for_content(analysis_canonical.as_ref(), observed_content_hash)?;
    // Body-sensitive `Export` / `LocalDecl` facts are LAZY: the
    // lookup demands exactly the named declaration's body through the
    // artifact's memo on first observation (`lookup_or_compute`);
    // eager header facts answer without lowering.
    let expected_hash = match artifacts.facts.lookup_or_compute(&key) {
        Some(fact) => match lane {
            FactLane::Semantic => fact.semantic_hash,
            FactLane::Display => fact.display_hash,
        },
        None => zero_hash(),
    };
    Some(ParseFactRef {
        canonical_id: canonical_id.to_string(),
        key,
        lane,
        expected_hash,
    })
}

/// Emit a self-root `FileWholeHash` for `canonical_id` pinned to a
/// caller-supplied **observed** content hash.
///
/// A self-root is the whole-hash fact for a cache entry's OWN keyed
/// canonical. The hash is NOT re-read from current content: it is the
/// content version the producer observed at the value source and
/// threaded into the signature builder. Pinning the self-root to the
/// observed version is what makes the value and its signature root on
/// one content identity — a re-read of the canonical's *current* hash
/// would root a stale value on post-edit content when an `upsert`
/// lands in the publish race window.
#[inline]
fn observed_self_root_fact(canonical_id: &str, observed_hash: Hash16) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical_id.to_string(),
        hash: observed_hash,
    }
}

/// Build a provenance-pure, path-precise signature for a cache whose
/// validity depends on a single MEMBER of an exporter type.
///
/// The builder is **provenance-pure**: it never consults the
/// authoritative current-content oracle and never re-reads current
/// content. The keyed canonical's content identity is supplied by the
/// caller as `observed_hash` — the content version the producer's
/// value was actually computed against, captured once at the value
/// source. The signature leads with a self-root `FileWholeHash` pinned
/// to that observed hash, then adds the path-precise parse facts:
/// `MemberPresence(exporter, member, space)` (header fact — bumps on
/// add/remove/rename/kind-change) and `Member(exporter, member,
/// space)` (body fingerprint — bumps on body edit). Both parse facts
/// are content-addressed against `observed_hash` via
/// [`parse_fact_ref_for_observed_current_content`] — they record the
/// fact hashes live when the producer observed the file, not whatever
/// is current at signature-build time.
///
/// Returns [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::UnresolvedProvenance`] when the observed
/// version's parse-fact registry cannot be recovered (no
/// content-addressed artifact for `(canonical_id, observed_hash)`).
/// The caller still returns the freshly-computed value, it only
/// forgoes the shared cache.
///
/// Use this helper for caches keyed on `(canonical, exporter,
/// member, space)` — slot-binding member reads and member-keyed
/// dispatch member projection.
///
/// Test-only: no production producer composes a member-keyed signature
/// this way (its former dedicated walker-DB consumer was deleted). The
/// `query_identity_self_root_substrate_tests` substrate suite exercises
/// this helper to characterise the observed-hash self-root prepend for
/// member-keyed scopes, matching the `fact_signature_for_canonical_surface`
/// precedent.
#[cfg(test)]
pub(crate) fn fact_signature_for_canonical_member(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exporter: &str,
    member: &str,
    space: SymbolSpace,
    observed_hash: Hash16,
) -> SignatureAdmission {
    let exporter_name = InternedName::from(exporter);
    let member_name = InternedName::from(member);
    let presence_key = FactKey::MemberPresence {
        exporter: exporter_name.clone(),
        name: member_name.clone(),
        space,
    };
    let body_key = FactKey::Member {
        exporter: exporter_name,
        name: member_name,
        space,
    };
    // Lead with the observed-hash self-root `FileWholeHash`, then add
    // the path-precise `MemberPresence` / `Member` parse facts pinned
    // to the SAME observed content version.
    let presence_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        presence_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let body_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        body_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let entries: Vec<FactVersionRef> = vec![
        observed_self_root_fact(canonical_id, observed_hash),
        FactVersionRef::Parse(presence_fact),
        FactVersionRef::Parse(body_fact),
    ];
    SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(entries)))
}

/// Build a provenance-pure signature for a cache whose validity
/// depends on the IDENTITY of a top-level type declared at
/// `canonical_id` — the Family A producer pattern for caches keyed on
/// `(canonical, type_name)`.
///
/// The builder is **provenance-pure**: it never consults the
/// authoritative current-content oracle and never re-reads current
/// content. The keyed canonical's content identity is supplied by the
/// caller as `observed_hash` — the content version the producer's
/// value was computed against, captured once at the value source. The
/// signature leads with a self-root `FileWholeHash` pinned to that
/// observed hash, then adds the top-level-identity parse facts:
/// - `Export(name, space)` — present iff the type is exported under
///   that name.
/// - `LocalDecl(name, space)` — present iff the type is declared
///   locally (non-exported).
/// - `MemberShape(exporter=name, space)` — the ordered member list
///   fingerprint; bumps when members are added/removed/renamed.
///
/// All three parse facts are content-addressed against `observed_hash`
/// via [`parse_fact_ref_for_observed_current_content`].
///
/// Returns [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::UnresolvedProvenance`] when the observed
/// version's parse-fact registry cannot be recovered. The caller
/// still returns the freshly-computed value.
pub(crate) fn fact_signature_for_exported_type(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
    space: SymbolSpace,
    observed_hash: Hash16,
) -> SignatureAdmission {
    let name = InternedName::from(type_name);
    let export_key = FactKey::Export {
        name: name.clone(),
        space,
    };
    let local_decl_key = FactKey::LocalDecl {
        name: name.clone(),
        space,
    };
    let member_shape_key = FactKey::MemberShape {
        exporter: name,
        space,
    };
    // Lead with the observed-hash self-root `FileWholeHash`, then add
    // the top-level-identity `Export` / `LocalDecl` / `MemberShape`
    // parse facts pinned to the SAME observed content version.
    let export_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        export_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let local_decl_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        local_decl_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let member_shape_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        member_shape_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let entries: Vec<FactVersionRef> = vec![
        observed_self_root_fact(canonical_id, observed_hash),
        FactVersionRef::Parse(export_fact),
        FactVersionRef::Parse(local_decl_fact),
        FactVersionRef::Parse(member_shape_fact),
    ];
    SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(entries)))
}

/// Build a provenance-pure, whole-canonical signature for a cache
/// whose cold-compute reads the file's surface fingerprint (e.g. a
/// binding-walker that enumerates every export).
///
/// The builder is **provenance-pure**: it never re-reads current
/// content. The keyed canonical's content identity is supplied by the
/// caller as `observed_hash` — the content version the value was
/// computed against, captured once at the value source. The signature
/// leads with a self-root `FileWholeHash` pinned to that observed
/// hash, then observes the `SyntacticExportSet` parse fact
/// content-addressed against the SAME observed version via
/// [`parse_fact_ref_for_observed_current_content`]. Returns `None`
/// when the observed version's parse-fact registry cannot be
/// recovered, refusing shared-cache admission.
///
/// Test-only: no production producer composes a whole-surface
/// signature this way. The `query_identity_self_root_substrate_tests`
/// substrate suite exercises this helper to characterise the
/// observed-hash self-root prepend.
///
/// Returns [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::UnresolvedProvenance`] when the observed
/// version's parse-fact registry cannot be recovered.
#[cfg(test)]
pub(crate) fn fact_signature_for_canonical_surface(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    observed_hash: Hash16,
) -> SignatureAdmission {
    // Lead with the observed-hash self-root `FileWholeHash`, then add
    // the `SyntacticExportSet` surface parse fact pinned to the SAME
    // observed content version.
    let surface_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        FactKey::SyntacticExportSet,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let entries: Vec<FactVersionRef> = vec![
        observed_self_root_fact(canonical_id, observed_hash),
        FactVersionRef::Parse(surface_fact),
    ];
    SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(entries)))
}

/// Empty signature constructor for cache entries published outside
/// any observable cold-compute pass (e.g. test fixtures, synthetic
/// publish paths). Validator trivially accepts; readers fall back to
/// the existing whole-hash regime in the legacy producer.
#[inline]
pub(crate) fn empty_fact_signature() -> Arc<[FactVersionRef]> {
    Arc::from(Vec::<FactVersionRef>::new())
}

/// The pair a structural-carrier signature producer returns: the
/// path-precise `FactVersionRef` signature plus the explicit self-root
/// canonical set the warm-read validator checks **strictly**. Produced
/// by `materialize_structure_read_set` (the `MaterializeStructureDb`
/// carrier) and `ref_cycle_read_set` (the `RefCycleResultDb` carrier).
/// `materialize_structure_read_set` returns
/// `Result<StructuralCarrierReadSet, NonAdmissionReason>` so refusal
/// modes (`SelfRootConflict`, `RouteGenerationDependency`) reach the
/// caller verbatim; `ref_cycle_read_set` returns
/// `Option<StructuralCarrierReadSet>` because its single refusal mode
/// is a torn self-root observation (`SelfRootConflict`), which the
/// caller attributes at the call site.
pub(crate) type StructuralCarrierReadSet = (Arc<[FactVersionRef]>, Arc<[Arc<str>]>);

/// A cache entry's dependency signature — the path-precise fact
/// signature captured by an `install_fact_tracer` scope.
///
/// `facts` is the sole cache-validity rail: the fact-tracer
/// observation set the producer recorded. Warm-hit reads validate
/// `facts` against the live store view (self-roots strict, cross-file
/// dependency facts lazy) and bubble it into any active outer tracer.
///
/// The carrier is `pub(crate)`. Cache entries store a single
/// `read_set_signature: ReadSetSignature` field. The shared cold-build
/// helper builds the carrier when the tracer finalises; warm-hit paths
/// call `validate_with_self_roots(ctx, &self_roots)` BEFORE
/// `bubble(ctx)`.
///
/// Invariants:
/// - `validate_with_self_roots(ctx, &self_roots)` returns true only
///   when `facts` validates, with every `FileWholeHash` fact for a
///   listed self-root canonical validated **strictly**. An empty
///   carrier with no self-roots validates vacuously.
/// - `bubble(ctx)` fans `facts` into every active outer tracer on the
///   current TLS stack.
/// - `canonical_ids()` returns the canonical IDs referenced by
///   `facts`, deduplicated by string identity. The reverse index
///   registers a (canonical → entry) mapping for each yielded ID.
/// - `is_overflow()` returns true when the producer's tracer finalised
///   with `FactReadSetFinalise::Overflow` — the materialised result
///   is valid but the path-precise signature is too large to admit
///   safely. Cache consumers route overflowed values through
///   `ComputeAdmission::ReturnOnly` (return without admitting).
#[derive(Clone, Debug)]
pub struct ReadSetSignature {
    pub facts: Arc<[FactVersionRef]>,
    /// Marks the carrier as constructed from a tracer that returned
    /// `FactReadSetFinalise::Overflow`. The materialised value is
    /// valid; the signature is too large to admit. The cooperative
    /// admission path routes the value through
    /// `ComputeAdmission::ReturnOnly` and the in-flight slot
    /// broadcasts the value to joiners.
    pub overflowed: bool,
}

impl ReadSetSignature {
    /// Construct a carrier from the traced path-precise fact set.
    #[inline]
    pub fn new(facts: Arc<[FactVersionRef]>) -> Self {
        Self {
            facts,
            overflowed: false,
        }
    }

    /// Construct an overflow carrier. The fact rail is empty; the
    /// `overflowed` flag is set. Cooperative admission consumers
    /// route values bearing this carrier through
    /// `ComputeAdmission::ReturnOnly`.
    #[inline]
    pub fn overflow() -> Self {
        Self {
            facts: empty_fact_signature(),
            overflowed: true,
        }
    }

    /// Empty carrier. The fact rail is empty; the `overflowed` flag is
    /// false. Used for synthetic publishes that pre-date the
    /// fact-tracer substrate.
    #[inline]
    pub fn empty() -> Self {
        Self {
            facts: empty_fact_signature(),
            overflowed: false,
        }
    }

    /// Validate the fact rail against the host's live state, validating
    /// every `FileWholeHash` fact whose canonical is listed in
    /// `self_root_canonicals` **strictly**.
    ///
    /// Returns `true` only when `facts` validates. Any `FileWholeHash`
    /// for a listed self-root canonical routes through the strict
    /// [`crate::resolver_core::StoreView::validates_self_root_whole_hash`]
    /// (an untracked or hash-mismatched self-root fails); every other
    /// fact — including a `FileWholeHash` for a non-listed cross-file
    /// dependency — keeps the lazy
    /// [`crate::resolver_core::StoreView::validates`] permissiveness.
    /// An overflow carrier always fails; an empty carrier with no
    /// self-roots validates vacuously.
    ///
    /// This is the strict warm-read validation entry point for a
    /// query-identity cache whose entry records its keyed (or
    /// file-derived input) canonicals as `self_root_canonicals`: a
    /// same-canonical content edit, or a self-root canonical the live
    /// store view no longer tracks, fails validation.
    #[inline]
    #[track_caller]
    pub(crate) fn validate_with_self_roots(
        &self,
        ctx: &dyn ResolverContext,
        self_root_canonicals: &[Arc<str>],
    ) -> bool {
        if self.overflowed {
            return false;
        }
        let self_root_refs: Vec<&str> = self_root_canonicals.iter().map(Arc::as_ref).collect();
        validate_fact_signature_with_self_roots(ctx, &self.facts, &self_root_refs)
    }

    /// Whether [`Self::validate_with_self_roots`] would actually
    /// **discriminate by view** for `self_root_canonicals` — i.e.
    /// whether the carrier carries at least one self-root fact that the
    /// strict validator routes through
    /// [`crate::resolver_core::StoreView::validates_self_root_whole_hash`].
    ///
    /// `validate_with_self_roots` only rejects a cross-view reuse when a
    /// `FileWholeHash` whose canonical is listed in
    /// `self_root_canonicals` mismatches the live store view. Every
    /// other fact — an empty fact rail, a `FileWholeHash` for a
    /// non-listed cross-file *dependency*, a `ProjectGeneration` rail —
    /// routes through the lazy / permissive path, which an unrelated
    /// overlay validates **vacuously**. So a carrier whose `facts` hold
    /// no `FileWholeHash` for any listed self-root canonical cannot
    /// discriminate a follower running under a different overlay: the
    /// validation passes regardless of the follower's view.
    ///
    /// Returns `true` iff at least one `FileWholeHash` fact in `facts`
    /// has a canonical that appears in `self_root_canonicals`. An
    /// overflow carrier never carries a self-root (`facts` is empty),
    /// an empty `self_root_canonicals` slice can never match, and a
    /// synthetic empty-fact carrier holds no `FileWholeHash` at all —
    /// all three return `false`.
    ///
    /// The in-flight joiner gate uses this to refuse cross-view reuse
    /// of ANY winner whose carrier could only ever validate vacuously —
    /// a tracer-overflow carrier, an unrootable build carrying only
    /// cross-file dependency facts, or a non-suppressed
    /// `QueryResult::Error(Miss)` from a declaration missing under the
    /// winner's overlay. For all of these `validate_with_self_roots` is
    /// not a real view check, so a follower under a possibly-different
    /// overlay must fork and recompute rather than coalesce onto the
    /// winner's view-specific result. The fork is not gated on
    /// `cache_suppress`.
    #[inline]
    pub(crate) fn has_view_discriminating_self_root(
        &self,
        self_root_canonicals: &[Arc<str>],
    ) -> bool {
        if self_root_canonicals.is_empty() {
            return false;
        }
        self.facts.iter().any(|fact| match fact {
            FactVersionRef::FileWholeHash { canonical_id, .. } => self_root_canonicals
                .iter()
                .any(|root| root.as_ref() == canonical_id.as_str()),
            _ => false,
        })
    }

    /// Whether this carrier records the invalidation rail that roots a
    /// `Partial(MissingDependency)` result — the §18.2 admission narrowing.
    ///
    /// A missing-dependency result (`import { X } from './missing'` where
    /// `./missing` does not yet exist) is fact-rooted-cacheable ONLY when
    /// the producer recorded the import-route rail: when the dependency
    /// later appears, the `DerivedFactKind::ImportRoute` rail's hash shifts
    /// and the warm read misses (lazy cross-file invalidation, the normal
    /// rail). The bare presence of arbitrary file facts is NOT sufficient —
    /// a positive `FileWholeHash` would warm-admit a degraded result with
    /// no rail that the dependency's appearance can invalidate. The
    /// `admit_decision` rule consults THIS, never the taint enum class, to
    /// decide `Warm` vs `ReturnOnly` for `Partial(MissingDependency)`.
    #[inline]
    #[must_use]
    pub(crate) fn records_missing_dependency_fact(&self) -> bool {
        self.facts.iter().any(|fact| {
            matches!(
                fact,
                FactVersionRef::DerivedFactHash {
                    kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                    ..
                }
            )
        })
    }

    /// Whether this carrier records the negative-resolution rail that roots
    /// a `Partial(UnresolvedReference)` result — the §18.2 admission
    /// narrowing.
    ///
    /// An unresolved reference over well-formed syntax is
    /// fact-rooted-cacheable ONLY when the resolver recorded the negative
    /// resolved-import fact — a `ResolvedImportClause` / `ResolvedReexportBinding`
    /// whose `resolved_canonical` is the
    /// [`UNRESOLVED_SENTINEL`](crate::resolved_import_facts_producer::UNRESOLVED_SENTINEL).
    /// When the reference later resolves, the producer records a real
    /// canonical, the fact's hash shifts, and the warm read misses. A
    /// POSITIVE `ResolveImports` fact must NOT qualify — it carries no
    /// negative rail, so trusting it would warm-admit a degraded result.
    /// `admit_decision` consults THIS, never the taint enum class.
    #[inline]
    #[must_use]
    pub(crate) fn records_negative_resolution_fact(&self) -> bool {
        self.facts.iter().any(|fact| match fact {
            FactVersionRef::ResolveImports(r) => match &r.key {
                FactKey::ResolvedImportClause {
                    resolved_canonical, ..
                }
                | FactKey::ResolvedReexportBinding {
                    resolved_canonical, ..
                } => {
                    resolved_canonical.as_ref()
                        == crate::resolved_import_facts_producer::UNRESOLVED_SENTINEL
                }
                _ => false,
            },
            _ => false,
        })
    }

    /// Bubble the path-precise fact set into every active outer
    /// tracer on the current TLS stack. No-op when the tracer stack
    /// is empty or `facts` is empty.
    #[inline]
    pub(crate) fn bubble(&self, ctx: &dyn ResolverContext) {
        bubble_fact_signature(ctx, &self.facts);
    }

    /// Bubble via TLS only — for fast-path warm hits that don't
    /// thread a `ResolverContext` reference. Equivalent to
    /// `bubble_fact_signature_via_tls(&self.facts)`.
    #[inline]
    pub fn bubble_via_tls(&self) {
        bubble_fact_signature_via_tls(&self.facts);
    }

    /// Canonical IDs referenced by this carrier's fact rail,
    /// deduplicated by string equality. The reverse index drains via
    /// this iterator.
    ///
    /// A `ProjectGeneration` fact references no canonical and
    /// contributes nothing — it is a project-wide fact validated
    /// on-read, not indexed per-canonical.
    pub fn canonical_ids(&self) -> Vec<Arc<str>> {
        // Small dedup set; cache entries' canonical sets typically
        // hold fewer than 16 entries each. `FxHashSet` over Arc<str>
        // keeps comparison O(1) per insertion when arcs are shared.
        let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        let mut out: Vec<Arc<str>> = Vec::new();
        for fact in self.facts.iter() {
            // `ProjectGeneration` references no canonical — it
            // contributes nothing to the reverse index.
            let Some(canon_str) = fact.canonical_id() else {
                continue;
            };
            let canon: Arc<str> = Arc::from(canon_str);
            if seen.insert(Arc::clone(&canon)) {
                out.push(canon);
            }
        }
        out
    }

    /// True iff the original tracer finalised with `Overflow`. The
    /// entry's value is valid; cache consumers route it through
    /// `ComputeAdmission::ReturnOnly` instead of admitting it.
    #[inline]
    pub fn is_overflow(&self) -> bool {
        self.overflowed
    }

    /// True iff this signature may be promoted into a warm cache entry.
    ///
    /// Cacheability is a function of overflow alone: an overflowed
    /// signature is too large to admit safely and must route through
    /// `ComputeAdmission::ReturnOnly`. **Emptiness is NOT a
    /// non-cacheable condition** — a tracer that observed zero facts
    /// validates vacuously on warm hits and is still safely cacheable.
    /// Producers that need to distinguish "no facts observed" from
    /// "non-empty fact rail" should read `self.facts.is_empty()`
    /// directly.
    #[inline]
    pub fn is_cacheable(&self) -> bool {
        !self.overflowed
    }
}

#[cfg(test)]
#[path = "fact_signature_helpers_tests.rs"]
mod fact_signature_helpers_tests;
