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

use crate::resolver_core::FactVersionRef;

/// Returns `true` if `fact` references `canonical_id` (as a
/// `FileWholeHash`, `DerivedFactHash`, or one of the domain-scoped
/// `Parse` / `ResolveImports` / `RouteSurface` variants whose
/// observed file matches).
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
            resolve_fact.canonical_id.as_str() == canonical_id
        }
        FactVersionRef::RouteSurface(route_fact) => {
            route_fact.canonical_id.as_str() == canonical_id
        }
        FactVersionRef::ProjectGeneration { .. } => false,
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
/// ([`crate::resolver_core::StoreView::validates`]) is the sole
/// cache-validity oracle.
#[derive(Clone)]
pub struct AppConfigNoOverrideProofEntry {
    /// R3/R26/R28 path-precise dep signature. Captured by the
    /// production producer's `install_fact_tracer` scope; bubbles
    /// into outer fact tracers via
    /// [`crate::fact_signature_helpers::bubble_fact_signature`] on
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
    /// **Block 1.H:** validation is path-precise only. Each warm-hit
    /// read calls [`crate::fact_signature_helpers::validate_fact_signature`]
    /// against the live store view through `ctx`. A single
    /// mismatched fact returns `None` and the caller cold-recomputes.
    /// On a successful warm hit, the path-precise observation set
    /// bubbles into any active outer fact tracer.
    ///
    /// Called from the production producer
    /// (`component_meta_caches::app_config_no_override_proof_get_or_compute`)
    /// on the cold path; the warm-hit fast path inside the
    /// component-meta ComponentConfig resolver consumes the proof
    /// via the same `peek` surface.
    pub(crate) fn peek(
        &self,
        key: &AppConfigNoOverrideProofKey,
        ctx: &dyn crate::resolver_core::ResolverContext,
    ) -> Option<Arc<AppConfigNoOverrideProofEntry>> {
        let entry = self.entries.get(key)?;
        if crate::fact_signature_helpers::validate_fact_signature(ctx, &entry.fact_dep_signature) {
            crate::fact_signature_helpers::bubble_fact_signature(ctx, &entry.fact_dep_signature);
            Some(Arc::clone(entry.value()))
        } else {
            // Stale; drop reference to allow eviction by other paths.
            drop(entry);
            None
        }
    }

    /// Publish a freshly-computed proof entry. Called by the
    /// production producer
    /// (`app_config_no_override_proof_get_or_compute`) when its
    /// `install_fact_tracer` scope finalised successfully.
    ///
    /// `fact_dep_signature` MUST be the
    /// [`crate::resolver_core::FactReadSetFinalise::Ok`] payload
    /// produced by the producer's tracer. Legacy `DepSignature`
    /// derivation is no longer performed at publish time — the
    /// producer is the single authority for the entry's
    /// validation contract.
    pub fn publish(
        &self,
        key: AppConfigNoOverrideProofKey,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) {
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

impl crate::invalidation_domain::ParticipatesInInvalidation for AppConfigNoOverrideProofDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        // `AppConfigNoOverrideProofDb` participates in
        // [FileContent, AppConfigInterfaceMerge]. The proof's dep
        // signature includes the merge-generation; either a content
        // edit on a flagged file or a workspace-level
        // `interface AppConfig` shape change must invalidate it.
        &[FileContent, AppConfigInterfaceMerge]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, AppConfigInterfaceMerge | ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for AppConfigNoOverrideProofDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}
