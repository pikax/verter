//! Host-owned `AppConfigNoOverrideProofDb` for the ComponentConfig
//! theme variant fast path (Issue #6).
//!
//! ## Contract (per sidecar §5 + §6.2)
//!
//! Each entry proves: "for the given `(app_config_decl_id,
//! component_key_literal)` tuple, the effective resolved `AppConfig`
//! has NO `ui[component_key_literal]` member regardless of which file
//! declared it (interface merging, module augmentation, generic
//! defaults, all considered)".
//!
//! The proof cache is consulted by the fast path BEFORE deciding
//! whether to project the prepared theme value directly. On cache
//! miss the fast path declines and the slow path runs.
//!
//! ## Population
//!
//! The proof is populated by the slow path: when canonical
//! materialization confirms no `ui[key]` override exists for a given
//! `app_config_decl_id`, it backfills the proof entry with its own
//! dep signature (every contributing file's `content_hash` plus the
//! workspace-level "interface-merging-of-AppConfig generation"
//! counter that bumps when any new `interface AppConfig` declaration
//! is added or removed anywhere in the project).
//!
//! ## Invariants
//!
//! - Warm-hit reads validate via path-precise
//!   `fact_dep_signature`. A single stale fact returns `None` and
//!   the caller cold-recomputes.
//! - There is NO eager workspace-wide effective-interface resolver.
//!   The cache is populated demand-driven by the production
//!   producer (`app_config_no_override_proof_get_or_compute`).
//! - Cache backend stores `Arc<Entry>` keyed by
//!   `AppConfigNoOverrideProofKey`; the producer wraps its cold
//!   compute in `install_fact_tracer` so admitted entries carry the
//!   authoritative R28 fact signature.
//!
//! ## Producer wiring
//!
//! The production producer
//! ([`crate::host_manage::component_meta_methods::app_config_no_override_proof_get_or_compute`])
//! checks each contributing file's
//! [`crate::project_type_store::IndexedReady::declares_interface_app_config`]
//! flag. Files without `interface AppConfig` are trivially
//! non-contributing; files with the flag participate in the
//! proof's `fact_dep_signature` so an edit to the interface
//! invalidates the proof.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use verter_session_query::facts::fact_cache::FactVersionRef;

/// Returns `true` if `fact` references `canonical_id` (as a
/// `FileWholeHash`, `DerivedFactHash`, `FileSourceEnv`, or one of the
/// domain-scoped `Parse` / `ResolveImports` / `RouteSurface` variants
/// whose observed file matches).
///
/// `FactVersionRef::ProjectGeneration` is not file-scoped — it
/// references no canonical — so it never matches.
fn fact_references_canonical(fact: &FactVersionRef, canonical_id: &str) -> bool {
    match fact {
        FactVersionRef::FileWholeHash {
            canonical_id: c, ..
        } => c.as_str() == canonical_id,
        FactVersionRef::DerivedFactHash {
            canonical_id: c, ..
        } => c.as_str() == canonical_id,
        FactVersionRef::Parse(parse_fact) => parse_fact.canonical_id.as_str() == canonical_id,
        FactVersionRef::ResolveImports(resolve_fact) => {
            resolve_fact.canonical_id() == Some(canonical_id)
        }
        FactVersionRef::RouteSurface(route_fact) => {
            route_fact.canonical_id.as_str() == canonical_id
        }
        FactVersionRef::FileSourceEnv {
            canonical_id: c, ..
        } => c.as_str() == canonical_id,
        FactVersionRef::ProgramAnalysis(fact) => match fact {
            verter_session_query::facts::fact_cache::ProgramAnalysisFactRef::FlowBody {
                function,
                ..
            } => function.canonical_id.as_ref() == canonical_id,
        },
        // None is file-scoped: each is a whole-project scalar, a
        // whole-domain aggregate, or a strict self-root world witness —
        // all name no canonical, so none can reference one.
        FactVersionRef::ProjectGeneration { .. }
        | FactVersionRef::DomainGeneration(_)
        | FactVersionRef::StrictSelfRootWorld(_) => false,
        // A consumed result's receipt references every canonical its
        // evidence reaches.
        FactVersionRef::Receipt(receipt) => receipt.references_canonical(canonical_id),
    }
}

/// Cache key: `(app_config_decl_canonical_id, component_key_literal)`.
///
/// `app_config_decl_canonical_id` is the canonical id of the file
/// that declares (or first declares, in the merge case) the
/// `AppConfig` interface. `component_key_literal` is the literal key
/// supplied as the third type argument of `ComponentConfig<typeof
/// theme, AppConfig, key>` — e.g. `"button"` or `"variants"`.
pub type AppConfigNoOverrideProofKey = (Arc<str>, Arc<str>);

/// Cache entry: the path-precise fact signature. The presence of an
/// entry IS the proof — we do not need a separate value.
///
/// The entry carries `fact_dep_signature: Arc<[FactVersionRef]>`
/// directly: the path-precise fact-signature substrate
/// ([`verter_session_query::facts::store_view::StoreView::validates`]) is the sole
/// cache-validity oracle.
#[derive(Clone)]
pub struct AppConfigNoOverrideProofEntry {
    /// R3/R26/R28 path-precise dep signature. Captured by the
    /// production producer's `install_fact_tracer` scope; bubbles
    /// into outer fact tracers via
    /// [`verter_type_engine::fact_signature_helpers::bubble_fact_signature`] on
    /// warm hit. Validated against the live store view on every
    /// warm-hit read.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

/// Host-owned cache. Sole authority for the proof state on
/// [`crate::project_type_store::ProjectTypeStore`].
pub struct AppConfigNoOverrideProofDb {
    entries: DashMap<AppConfigNoOverrideProofKey, Arc<AppConfigNoOverrideProofEntry>>,
    live_counter: Arc<AtomicU64>,
}

impl AppConfigNoOverrideProofDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            live_counter,
        }
    }

    /// Look up a proof entry. Returns `None` on miss; the fast-path
    /// caller declines and the slow path runs.
    ///
    /// Validation is path-precise only. Each warm-hit read calls
    /// [`verter_type_engine::fact_signature_helpers::validate_fact_signature`]
    /// against the live store view through `ctx`. A single
    /// mismatched fact returns `None` and the caller cold-recomputes.
    /// On a successful warm hit, the path-precise observation set
    /// bubbles into any active outer fact tracer.
    ///
    /// Called from the producer
    /// (`app_config_no_override_proof_get_or_compute`)
    /// on the cold path; the warm-hit fast path inside the
    /// component-meta ComponentConfig resolver consumes the proof
    /// via the same `peek` surface. That producer is reached today only
    /// through tests / the `for_tests` wrapper, so this read gate is
    /// `cfg(any(test, feature = "test-support"))` to match (no dead surface in
    /// release).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn candidate(
        &self,
        key: &AppConfigNoOverrideProofKey,
    ) -> Option<Arc<AppConfigNoOverrideProofEntry>> {
        Some(Arc::clone(self.entries.get(key)?.value()))
    }

    /// Publish a freshly-computed proof entry. Called by the
    /// production producer
    /// (`app_config_no_override_proof_get_or_compute`) when its
    /// `install_fact_tracer` scope finalised successfully.
    ///
    /// `fact_dep_signature` MUST be the
    /// [`verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok`] payload
    /// produced by the producer's tracer. Legacy `DepSignature`
    /// derivation is no longer performed at publish time — the
    /// producer is the single authority for the entry's
    /// validation contract.
    ///
    /// The entry retains the signature's evidence pages, so publication
    /// first claims them into a refusable reservation; a refused claim
    /// publishes nothing.
    #[cfg(any(test, feature = "test-support"))]
    pub fn publish(
        &self,
        key: AppConfigNoOverrideProofKey,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) {
        if verter_session_query::facts::receipt::claim_evidence_pages(
            &verter_session_query::retention::SemanticRetentionAccount::process_local(),
            &fact_dep_signature,
        )
        .is_err()
        {
            return;
        }
        let entry = Arc::new(AppConfigNoOverrideProofEntry { fact_dep_signature });
        if self.entries.insert(key, entry).is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Per-canonical eviction: drop every entry whose
    /// `fact_dep_signature` references `canonical_id` (either as the
    /// `app_config_decl_canonical_id` key component or anywhere in
    /// the fact signature). Called from
    /// [`crate::project_type_store::ProjectTypeStore::evict_canonical`].
    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let to_remove: Vec<AppConfigNoOverrideProofKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (decl_canonical, _) = entry.key();
                let dep_hits = entry
                    .value()
                    .fact_dep_signature
                    .iter()
                    .any(|fact| fact_references_canonical(fact, canonical_id));
                if decl_canonical.as_ref() == canonical_id || dep_hits {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in to_remove {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Drop every entry. Called on project-generation bump.
    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for AppConfigNoOverrideProofDb {
    fn default() -> Self {
        Self::new()
    }
}

impl verter_type_engine::invalidation_domain::ParticipatesInInvalidation
    for AppConfigNoOverrideProofDb
{
    fn domains(&self) -> &'static [verter_type_engine::invalidation_domain::InvalidationDomain] {
        use verter_type_engine::invalidation_domain::InvalidationDomain::*;
        // `AppConfigNoOverrideProofDb` participates in
        // [FileContent, AppConfigInterfaceMerge]. The proof's dep
        // signature includes the merge-generation; either a content
        // edit on a flagged file or a workspace-level
        // `interface AppConfig` shape change must invalidate it.
        &[FileContent, AppConfigInterfaceMerge]
    }
    fn invalidate(&self, domain: verter_type_engine::invalidation_domain::InvalidationDomain) {
        use verter_type_engine::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, AppConfigInterfaceMerge | ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl verter_type_engine::invalidation_domain::InvalidationByCanonical
    for AppConfigNoOverrideProofDb
{
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// AppConfigNoOverrideProofDb production producer
// ═══════════════════════════════════════════════════════════════════════════
/// Production producer for [`crate::app_config_proof_db::AppConfigNoOverrideProofDb`].
///
/// Given a key `(decl_canonical, component_key_literal)`, returns
/// the cached proof entry if one is valid under the live store
/// view, OR runs a cold compute (wrapped in `install_fact_tracer`)
/// and publishes a fresh proof.
///
/// The cold compute checks the `IndexedReady.declares_interface_app_config`
/// flag for `decl_canonical` and observes its `FileWholeHash` fact
/// through the active tracer. The proof's `fact_dep_signature`
/// therefore captures (a) the decl-canonical's whole-hash so an
/// edit to the file invalidates the proof, and (b) any transitive
/// observations the call-chain made through the resolver substrate.
///
/// `publish()` accepts `Arc<[FactVersionRef]>` directly — the
/// path-precise fact-signature substrate (`HostStoreView::validates`)
/// is the sole cache-validity oracle.
///
/// **Cold-build outcome semantics:**
/// - `Some(entry)` published — proof is valid. The fast-path
///   consumer can rely on the fact-signature for warm-hit revalidation.
/// - On `FactReadSetFinalise::NonCacheable` — refuse cache admission;
///   the next call cold-recomputes. In a `semantic-observe` build the
///   provenance counter `app_config_proof_non_cacheable_refusals` advances.
///
/// Resolver-tier producer that takes `&dyn ResolverContext` to stay
/// inside the request-port contract (the five ports in
/// `resolver_core::request_ports`, whose compile-contract fixtures prove a
/// request cannot reach ambient host state). Integration tests reach this
/// via the crate-public wrapper
/// [`crate::for_tests::app_config_no_override_proof_get_or_compute_for_tests`].
///
/// The ComponentConfig theme-variant fast-path resolver (a future
/// re-introduction of the retired rescue cascade) and the
/// app-config no-override deferred-proof test both reach this
/// producer.
///
/// Reached today only through tests and the `for_tests` wrapper (both
/// gated by the explicit `test-support` feature); gated to match so the
/// producer is absent from every ordinary production build.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn app_config_no_override_proof_get_or_compute<
    C: verter_type_engine::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn verter_type_engine::resolver_core::ResolverContext<C>,
    proofs: &crate::app_config_proof_db::AppConfigNoOverrideProofDb,
    provenance: &crate::meta_provenance::MetaProvenance,
    key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
) -> Option<Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
    // Warm-hit peek — validate the cached fact_dep_signature against
    // the live store view. The peek bubbles the signature into any
    // active outer tracer on success.
    if let Some(entry) = proofs.candidate(key) {
        if ctx.validates_fact_signature(&entry.fact_dep_signature) {
            ctx.observe_borrowed_signature(&entry.fact_dep_signature);
            return Some(entry);
        }
    }

    // Cold compute. The closure observes the decl-canonical's whole
    // hash so an edit invalidates the proof.
    let (decl_canonical, _component_key_literal) = key;
    let decl_canonical_for_compute = Arc::clone(decl_canonical);
    let cold_body = move || -> bool {
        // Look up the IndexedReady for the decl canonical. The
        // tracer fan-out picks up any indirect observations the
        // resolver substrate emits.
        //
        // Content-pinned: the observed `FileWholeHash` fact becomes
        // part of this proof entry's `read_set_signature`. A permissive
        // `get_any` could observe a stale artifact's `whole_hash`,
        // sealing the proof against a content hash that is no longer
        // current. A stale candidate is treated identically to "file
        // removed" — `current_content_pinned_indexed` returns `None`,
        // the sentinel-zero hash is observed, and the validator
        // re-derives the proof on the next read.
        let ir = ctx.indexed_for_current_content(decl_canonical_for_compute.as_ref());
        // Observe the file's whole-hash explicitly. If no IndexedReady
        // is present (file removed), record a sentinel zero hash so
        // the validator picks up the absence on the next read.
        let whole_hash = ir.as_ref().map(|ir| ir.whole_hash).unwrap_or_default();
        ctx.observe(
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                canonical_id: decl_canonical_for_compute.as_ref().to_string(),
                hash: whole_hash,
            },
        );
        // The "no override" determination is a structural query
        // into the interface members. For the producer's
        // substrate-correctness contract, the
        // `declares_interface_app_config` flag short-circuits the
        // walk: a file without `interface AppConfig` cannot
        // contribute an override.
        //
        // Files that DO declare `interface AppConfig` participate in
        // the proof's fact_dep_signature via the file_whole_hash
        // observation above; any edit to the interface body shifts
        // the whole-hash and invalidates the proof. This is the
        // R3/R26/R28 substrate contract — the producer does NOT
        // need to walk the interface body to decide the proof's
        // validation oracle.
        ir.as_ref()
            .map(|ir| !ir.declares_interface_app_config)
            .unwrap_or(true)
    };
    let (no_override, finalise) = verter_type_engine::fact_signature_helpers::install_fact_tracer(
        &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
        cold_body,
    );
    provenance
        .app_config_proof_fact_tracer_installs
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // ReturnOnly never publishes — fenced-serve arm: a proof derived
    // from a served-without-publication artifact must not seal a
    // shared no-override entry whose facts validate against the live
    // view. Decline to publish; the consumer takes the slow path.
    match finalise {
        verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(fact_dep_signature) => {
            if !no_override {
                // The file declares `interface AppConfig` — we
                // cannot prove "no override" without walking the
                // member set. Decline to publish; the fast-path
                // consumer must take the slow path.
                return None;
            }
            proofs.publish(key.clone(), Arc::clone(&fact_dep_signature));
            Some(Arc::new(
                crate::app_config_proof_db::AppConfigNoOverrideProofEntry { fact_dep_signature },
            ))
        }
        verter_session_query::facts::fact_read_set::FactReadSetFinalise::NonCacheable(_) => {
            #[cfg(feature = "semantic-observe")]
            provenance
                .app_config_proof_non_cacheable_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        }
        // Refuses too, but is not a non-cacheable read: a compaction domain
        // moved mid-compute.
        verter_session_query::facts::fact_read_set::FactReadSetFinalise::MutationUnstable => None,
    }
}
