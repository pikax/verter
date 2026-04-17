//! Request-scoped store view wrapper — one captured snapshot per `getComponentMeta`
//! (or sister) request, plus an extension store for canonicals loaded mid-request
//! via `ensure_loaded`.
//!
//! # Why
//!
//! The route DB's `ValidatedFactCache::get_if_valid` validates *every* recorded
//! fact against a `StoreView`. When two `owned_view` snapshots taken mid-request
//! see different `derived_hashes` (e.g. an intervening `ensure_loaded` bumped the
//! epoch — pre-Phase-1 D it did so unconditionally; post-Phase-1 D it does so
//! only on real content changes), the second view rejects the first view's
//! cached entry and the resolver runs again.
//!
//! `RequestStoreView` fixes that by holding one captured snapshot plus an
//! **additive-only** extension map keyed by canonical. Canonicals loaded
//! mid-request integrate into the extension store via the
//! [`CURRENT_REQUEST_VIEW`] thread-local, which is pushed at request entry by
//! [`RequestViewGuard`].
//!
//! # Architectural rules (from `phase1-cache-cluster-plan.md` §2.2)
//!
//! - The captured view + extension store form the single authority for
//!   `whole_hash` / `derived_hash` / `import_route` / `is_evalable` lookups
//!   within one request.
//! - Resolvers must not reach past the view (no live `host.scheduler.try_get_source`,
//!   `module_facts.get_any`, or `host.get_whole_hash` probes) during a request.
//! - The extension store is **additive-only** per canonical: entries never mutate
//!   once written; deletion only happens at request end when the `RequestStoreView`
//!   is dropped.
//!
//! # Scope in this commit (E)
//!
//! This commit lands:
//! - `RequestStoreView` struct + `RequestViewGuard` RAII.
//! - Thread-local `CURRENT_REQUEST_VIEW` with `install` / drop-restore.
//! - Helper methods: `whole_hash`, `derived_hash`, `import_route`, `is_evalable`,
//!   `record_extension`, `touched`.
//! - [`crate::VerterHost::build_request_store_view`] constructor.
//! - [`ensure_loaded`](crate::VerterHost::ensure_loaded) integration: after
//!   reintegration, the current request view's extension store is updated.
//!
//! The full signature rewrite of every `*_in_view(... Option<&HostStoreView>)`
//! to `view: &RequestStoreView` is deferred to Commit I (legacy cleanup).
//! Meanwhile callers continue to pass `Option<&HostStoreView>` — they receive
//! the captured view, and the route DB's `validates` consults the thread-local
//! extension store when a fact references a canonical outside the captured view.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::resolver_core::{DerivedFactKind, FactVersionRef};
use crate::resolver_store::HostStoreView;
use crate::types::{DependencyResolution, Hash16};

/// Per-canonical extension recorded during a request. Contains enough of
/// `HostStoreView`'s per-canonical shape (whole_hash + derived hashes +
/// import_routes) that route-DB `validates` can accept facts referring to
/// canonicals loaded AFTER the view was snapshotted.
#[derive(Debug, Clone, Default)]
pub(crate) struct RequestExtension {
    pub(crate) whole_hash: Hash16,
    pub(crate) derived_hashes: FxHashMap<DerivedFactKind, Hash16>,
    pub(crate) import_routes: FxHashMap<String, DependencyResolution>,
}

/// Shared map of request-scoped external-type-resolution inputs. Keyed by
/// `(canonical_id, whole_hash)` so content-version collisions within a
/// request (e.g. from archived vs current facts) produce distinct entries.
type ExternalInputsMemoMap =
    FxHashMap<(String, Hash16), Arc<crate::host_manage::ExternalTypeResolutionInputs>>;

/// Shared map of request-scoped `current_eval_state_in_view` results. Keyed
/// by normalized canonical; the outer `Option` distinguishes "never seen"
/// (entry absent) from "seen and returned `None`" (entry present, value
/// `None`).
type EvalStateMemoMap = FxHashMap<String, Option<EvalStateMemoEntry>>;

/// Request-scoped view: one captured `HostStoreView` plus an additive extension
/// store. Held via `Arc` so the thread-local [`CURRENT_REQUEST_VIEW`] can hold
/// a `Weak` without extending the lifetime of the view.
///
/// `#[derive(Clone)]` because `ComponentMetaRequestHost::View` and
/// `FallthroughRequestHost::View` require `StoreView + Clone`. The internal
/// `extensions` / `external_inputs_memo` are `Arc<RwLock<...>>`-wrapped so
/// cloning shares state cheaply — clones of a `RequestStoreView` see the
/// same request extension + memo, which matches the request-scoped
/// semantics (a fixed view passed through `with_fixed_view()` is the same
/// request's view).
#[derive(Debug, Clone)]
pub struct RequestStoreView {
    pub(crate) captured: Arc<HostStoreView>,
    extensions: Arc<RwLock<FxHashMap<String, RequestExtension>>>,
    /// Per-request memo over host-scoped analysis results. Keyed by
    /// `(canonical_id, whole_hash)` — the content identity of the analysis.
    ///
    /// This is a lookup memo over the host cache
    /// (`external_type_analysis_cache` and `module_facts`), NOT a parallel
    /// hydration path. It caches the result of *fetching from the host cache*
    /// so repeat callers within one request don't re-pay canonical
    /// normalization, the `module_facts` lock acquire, and the multiple
    /// `Arc::clone`s that the raw fetch costs. The underlying host-scoped
    /// analysis cache remains the single source of truth for the
    /// `Arc<AnalyzedExternalTypeSource>` data.
    external_inputs_memo: Arc<RwLock<ExternalInputsMemoMap>>,
    /// Per-request memo for `current_eval_state_in_view` results. Keyed by
    /// canonical only — the raw source + parse + whole_hash tuple returned
    /// from the host is stable for the lifetime of a request, so a single
    /// entry per canonical is safe.
    ///
    /// Reduces the per-query redundancy observed on SFC self-walks (nuxt-ui
    /// Accordion: 128 probes of its own `current_eval_state`; expected once
    /// per unique canonical per query). The memo is consulted at function
    /// entry and populated on first miss — subsequent probes return the
    /// cached tuple with only a request-view lock + hashmap lookup.
    eval_state_memo: Arc<RwLock<EvalStateMemoMap>>,
}

/// Cached value of `VerterHost::current_eval_state_in_view` shared via the
/// request view's per-canonical memo. Carries the exact tuple returned by
/// the slow path so memo hits are observationally identical to a fresh call.
#[derive(Debug, Clone)]
pub(crate) struct EvalStateMemoEntry {
    pub(crate) raw_source: Arc<str>,
    pub(crate) cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    pub(crate) whole_hash: Hash16,
}

/// Outcome of [`RequestStoreView::touched`] — used by the debug-mode
/// view-coherence invariant assertion (§2.2 item 9).
///
/// Currently only `Tracked` / `Extended` / `Untracked` are distinguished — the
/// release-mode trace event emission and debug-mode panic wiring land with
/// the signature rewrite in Commit I.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TouchOutcome {
    /// Canonical is in the captured view's `whole_hashes`.
    Tracked,
    /// Canonical was loaded mid-request via `ensure_loaded` and lives in the
    /// extension store.
    Extended,
    /// Canonical is unknown to both the captured view and the extension store.
    /// In debug builds this panics (§2.2 item 9); in release it emits a
    /// `request_view_untouched_canonical` trace event.
    Untracked,
}

// Helper methods on RequestStoreView. `#[allow(dead_code)]` on the whole block
// because the signature rewrite (Commit I) is where the `_in_view` callers
// start consuming these helpers. Before I: `is_evalable` is reached via
// `VerterHost::is_evalable`; `install` is called from tests + will be called
// from top-level `getComponentMeta` entry after I.
#[allow(dead_code)]
impl RequestStoreView {
    pub(crate) fn new(captured: Arc<HostStoreView>) -> Arc<Self> {
        Arc::new(Self {
            captured,
            extensions: Arc::new(RwLock::new(FxHashMap::default())),
            external_inputs_memo: Arc::new(RwLock::new(FxHashMap::default())),
            eval_state_memo: Arc::new(RwLock::new(FxHashMap::default())),
        })
    }

    /// Look up a memoized `current_eval_state_in_view` tuple for
    /// `canonical_id`. Returns `Some(Some(entry))` for a previously-resolved
    /// successful lookup, `Some(None)` for a memoized missing file, and
    /// `None` when the canonical has not been probed yet this request.
    ///
    /// The two-level `Option` distinguishes "never seen" from "seen and
    /// returned None" so callers can negative-cache missing files within
    /// one request without re-walking the slow path.
    pub(crate) fn eval_state_memo_get(
        &self,
        canonical_id: &str,
    ) -> Option<Option<EvalStateMemoEntry>> {
        self.eval_state_memo.read().get(canonical_id).cloned()
    }

    /// Record the outcome of a `current_eval_state_in_view` call. Safe to
    /// call with `None` — negative caching avoids redundant fallback walks
    /// for files the host cannot resolve within a single request.
    pub(crate) fn record_eval_state(
        &self,
        canonical_id: impl Into<String>,
        entry: Option<EvalStateMemoEntry>,
    ) {
        self.eval_state_memo
            .write()
            .insert(canonical_id.into(), entry);
    }

    /// Look up a previously-memoized `ExternalTypeResolutionInputs` for
    /// `(canonical_id, whole_hash)`. Returns `None` when no memo entry exists
    /// for the exact content version — callers materialize and
    /// [`record_external_inputs`] on miss.
    pub(crate) fn external_inputs_memo_get(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
    ) -> Option<Arc<crate::host_manage::ExternalTypeResolutionInputs>> {
        self.external_inputs_memo
            .read()
            .get(&(canonical_id.to_string(), whole_hash))
            .cloned()
    }

    /// Store the materialized inputs keyed by `(canonical_id, whole_hash)`.
    /// Subsequent callers within this request short-circuit via
    /// [`external_inputs_memo_get`].
    pub(crate) fn record_external_inputs(
        &self,
        canonical_id: impl Into<String>,
        whole_hash: Hash16,
        inputs: Arc<crate::host_manage::ExternalTypeResolutionInputs>,
    ) {
        self.external_inputs_memo
            .write()
            .insert((canonical_id.into(), whole_hash), inputs);
    }

    #[cfg(test)]
    pub(crate) fn external_inputs_memo_len(&self) -> usize {
        self.external_inputs_memo.read().len()
    }

    /// Return the `whole_hash` for `canonical`, consulting the captured view
    /// first, then the extension store. `None` for unknown canonicals.
    pub(crate) fn whole_hash(&self, canonical: &str) -> Option<Hash16> {
        if let Some(hash) = self.captured.whole_hash(canonical) {
            return Some(hash);
        }
        self.extensions
            .read()
            .get(canonical)
            .map(|ext| ext.whole_hash)
    }

    /// Cheap feasibility predicate — `true` iff the request view (captured or
    /// extension) tracks the canonical. Used by the §4.3 predicate sites that
    /// today invoke `current_eval_state_in_view(&candidate, None).is_some()`.
    pub(crate) fn is_evalable(&self, canonical: &str) -> bool {
        self.whole_hash(canonical).is_some()
    }

    /// Lookup a derived fact hash. Checks the captured view, then the extension
    /// store. `None` for unknown `(canonical, kind)` pairs.
    pub(crate) fn derived_hash(&self, canonical: &str, kind: DerivedFactKind) -> Option<Hash16> {
        if let Some(hash) = self.captured.derived_hash(canonical, kind) {
            return Some(hash);
        }
        self.extensions
            .read()
            .get(canonical)
            .and_then(|ext| ext.derived_hashes.get(&kind).copied())
    }

    /// Lookup an import-route resolution for the given canonical + specifier.
    pub(crate) fn import_route(
        &self,
        canonical: &str,
        specifier: &str,
    ) -> Option<DependencyResolution> {
        if let Some(route) = self.captured.import_route(canonical, specifier) {
            return Some(route);
        }
        self.extensions
            .read()
            .get(canonical)
            .and_then(|ext| ext.import_routes.get(specifier).cloned())
    }

    /// Classify whether `canonical` is known to this view. Intended for the
    /// debug-mode invariant assertion at every resolver entry — on
    /// `Untracked`, `cfg(debug_assertions)` panics, release emits a trace
    /// event. Callers that legitimately need to load a canonical must call
    /// [`crate::VerterHost::ensure_loaded`] first (which records the canonical
    /// into the extension store via the [`CURRENT_REQUEST_VIEW`] hook) or
    /// fail closed.
    pub(crate) fn touched(&self, canonical: &str) -> TouchOutcome {
        if self.captured.whole_hash(canonical).is_some() {
            return TouchOutcome::Tracked;
        }
        if self.extensions.read().contains_key(canonical) {
            return TouchOutcome::Extended;
        }
        TouchOutcome::Untracked
    }

    /// Record a canonical loaded mid-request into the extension store.
    /// Idempotent: overwrites any existing entry under the same canonical
    /// (second write should match first write since the canonical's facts
    /// don't change within a single request — see §2.2 additive-only rule).
    pub(crate) fn record_extension(
        &self,
        canonical: impl Into<String>,
        whole_hash: Hash16,
        derived_hashes: FxHashMap<DerivedFactKind, Hash16>,
        import_routes: FxHashMap<String, DependencyResolution>,
    ) {
        self.extensions.write().insert(
            canonical.into(),
            RequestExtension {
                whole_hash,
                derived_hashes,
                import_routes,
            },
        );
    }

    /// Check if a route DB fact is valid against the captured view OR the
    /// extension store. Mirrors `HostStoreView::validates` but adds the
    /// extension fallback for canonicals loaded mid-request.
    pub(crate) fn validates_fact(&self, fact: &FactVersionRef) -> bool {
        if self.captured_validates(fact) {
            return true;
        }
        self.extension_validates(fact)
    }

    fn captured_validates(&self, fact: &FactVersionRef) -> bool {
        use crate::resolver_core::StoreView;
        // Rely on the existing HostStoreView impl. Returns `true` on untracked
        // canonicals for whole_hash + DirectSource (permissive fallback).
        // Rejects untracked Route/ImportRoute — that's the gap the extension
        // store closes.
        self.captured.validates(fact)
    }

    fn extension_validates(&self, fact: &FactVersionRef) -> bool {
        let extensions = self.extensions.read();
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash } => extensions
                .get(canonical_id)
                .map(|ext| &ext.whole_hash == hash)
                .unwrap_or(false),
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                DerivedFactKind::DirectSource => extensions
                    .get(canonical_id)
                    .map(|ext| &ext.whole_hash == hash)
                    .unwrap_or(false),
                _ => extensions
                    .get(canonical_id)
                    .and_then(|ext| ext.derived_hashes.get(kind))
                    .map(|current| current == hash)
                    .unwrap_or(false),
            },
        }
    }

    /// Install this view as the current request's thread-local handle.
    /// The returned [`RequestViewGuard`] restores the previous value on drop.
    pub(crate) fn install(self: &Arc<Self>) -> RequestViewGuard {
        let prev = CURRENT_REQUEST_VIEW.with(|cell| cell.borrow().clone());
        CURRENT_REQUEST_VIEW.with(|cell| {
            *cell.borrow_mut() = Some(Arc::downgrade(self));
        });
        RequestViewGuard { _prev: prev }
    }

    #[cfg(test)]
    pub(crate) fn extension_count(&self) -> usize {
        self.extensions.read().len()
    }

    /// Forward to the captured view. Used by route DB + `_in_view` callers that
    /// previously took a `&HostStoreView` directly.
    pub(crate) fn mutation_epoch(&self) -> u64 {
        self.captured.mutation_epoch()
    }

    /// Check whether `canonical_id` is tracked by the captured view OR the
    /// extension store. Mirrors `HostStoreView::tracks_whole_hash` with
    /// extension-aware fallback.
    pub(crate) fn tracks_whole_hash(&self, canonical_id: &str) -> bool {
        if self.captured.tracks_whole_hash(canonical_id) {
            return true;
        }
        self.extensions.read().contains_key(canonical_id)
    }

    /// Accept `hash` for `canonical_id` when either the captured view tracks
    /// that `(canonical, hash)` pair or the extension store does. The
    /// captured view returns `true` for untracked canonicals (permissive
    /// semantics on `HostStoreView::accepts_whole_hash`); if it returns
    /// `false` the hash genuinely mismatched a tracked entry, and the
    /// extension store only overrides that verdict on an exact match.
    pub(crate) fn accepts_whole_hash(&self, canonical_id: &str, hash: Hash16) -> bool {
        if self.captured.accepts_whole_hash(canonical_id, hash) {
            return true;
        }
        // Captured view REJECTED (tracked canonical with mismatched hash).
        // Only an exact extension-store match can override.
        self.extensions
            .read()
            .get(canonical_id)
            .is_some_and(|ext| ext.whole_hash == hash)
    }

    /// Diagnostic helper: delegates to `HostStoreView::invalid_fact_details`
    /// on the captured view. Extension-store mismatches are unlikely in
    /// practice (the extension is additive); the captured-view path surfaces
    /// the same class of violations symmetrically.
    pub(crate) fn invalid_fact_details(
        &self,
        facts: &[crate::resolver_core::FactVersionRef],
        limit: usize,
    ) -> Vec<String> {
        self.captured.invalid_fact_details(facts, limit)
    }

    /// Delegates to the captured `HostStoreView::validates_all`.
    pub(crate) fn validates_all(&self, facts: &[crate::resolver_core::FactVersionRef]) -> bool {
        facts.iter().all(|fact| self.validates_fact(fact))
    }
}

/// Allow `RequestStoreView` to be passed directly anywhere a `StoreView` is
/// accepted (e.g. `ModuleFactsDb::get`, route-DB validators). Delegates to
/// [`RequestStoreView::validates_fact`] — captured view first, extension
/// store fallback — so mid-request loaded canonicals pass route-DB
/// validation instead of being rejected as "untracked".
///
/// `compat_token` and `checks_archive` forward to the captured view so that
/// all route-DB singleflight and archive-strictness semantics stay aligned
/// with the snapshot the request was keyed against.
impl crate::resolver_core::StoreView for RequestStoreView {
    fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        self.captured.compat_token()
    }

    fn validates(&self, fact: &FactVersionRef) -> bool {
        self.validates_fact(fact)
    }

    fn checks_archive(&self) -> bool {
        self.captured.checks_archive()
    }

    /// Strict archived validation — mirror `HostStoreView::validates_archived`
    /// with extension-store fallback. Critical for the no-stale-archive
    /// contract: untracked canonicals (no fact in either captured view or
    /// extension) MUST be rejected for archived entries, otherwise stale
    /// soft-invalidated data leaks past workspace content changes.
    fn validates_archived(&self, fact: &FactVersionRef) -> bool {
        if self.captured.validates_archived(fact) {
            return true;
        }
        // Extension store strict mirror.
        let extensions = self.extensions.read();
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash } => extensions
                .get(canonical_id)
                .is_some_and(|ext| &ext.whole_hash == hash),
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                crate::resolver_core::DerivedFactKind::DirectSource => extensions
                    .get(canonical_id)
                    .is_some_and(|ext| &ext.whole_hash == hash),
                _ => extensions
                    .get(canonical_id)
                    .and_then(|ext| ext.derived_hashes.get(kind))
                    .is_some_and(|current| current == hash),
            },
        }
    }

    fn tracks_file(&self, canonical_id: &str) -> bool {
        // Only the CAPTURED view counts as "tracked" for validation-fact
        // inclusion. Extension-store entries are additive / request-private
        // and tracking them here would add ImportRoute facts whose hash the
        // captured cache doesn't know about — causing false misses on the
        // host-scoped component_meta cache.
        self.captured.tracks_file(canonical_id)
    }
}

thread_local! {
    /// Current request's view, installed by [`RequestStoreView::install`] at
    /// request entry and restored to its previous value when the returned
    /// [`RequestViewGuard`] drops.
    ///
    /// Held as `Weak<RequestStoreView>` so the thread-local doesn't extend the
    /// view's lifetime past the request.
    pub(crate) static CURRENT_REQUEST_VIEW: RefCell<Option<Weak<RequestStoreView>>> =
        const { RefCell::new(None) };
}

/// RAII guard returned by [`RequestStoreView::install`]. Restores the previous
/// thread-local value on drop so nested requests compose safely.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct RequestViewGuard {
    _prev: Option<Weak<RequestStoreView>>,
}

impl Drop for RequestViewGuard {
    fn drop(&mut self) {
        CURRENT_REQUEST_VIEW.with(|cell| {
            *cell.borrow_mut() = self._prev.clone();
        });
    }
}

/// Upgrade the thread-local request view, if one is installed. Returns `None`
/// when the caller is outside any request (top-level loads, background work).
pub(crate) fn current_request_view() -> Option<Arc<RequestStoreView>> {
    CURRENT_REQUEST_VIEW.with(|cell| cell.borrow().as_ref().and_then(Weak::upgrade))
}

/// Effective request view for a helper that takes `Option<&RequestStoreView>`.
///
/// Holds either the ambient (thread-local) view as `Arc<RequestStoreView>` or
/// the borrowed explicit-arg view. Use [`Self::as_view`] inside the helper body
/// to obtain a `&RequestStoreView` without thinking about which case you're in.
///
/// [`Self::OutsideRequest`] indicates the caller is genuinely outside any
/// request — only there is it acceptable to fall back to live host probes
/// (`get_whole_hash`, `scheduler.try_get_source`, etc.).
pub(crate) enum EffectiveView<'a> {
    Ambient(Arc<RequestStoreView>),
    Explicit(&'a RequestStoreView),
    OutsideRequest,
}

impl<'a> EffectiveView<'a> {
    /// Borrow the underlying view, regardless of whether it came from the
    /// ambient thread-local or the explicit arg.
    pub(crate) fn as_view(&self) -> Option<&RequestStoreView> {
        match self {
            EffectiveView::Ambient(view) => Some(&**view),
            EffectiveView::Explicit(view) => Some(view),
            EffectiveView::OutsideRequest => None,
        }
    }
}

/// Resolve the effective request view for a helper that takes
/// `Option<&RequestStoreView>`. Ambient (`current_request_view()`) takes
/// precedence over the explicit arg. Returns [`EffectiveView::OutsideRequest`]
/// only when no request is installed on the current thread.
///
/// Reference shapes already correct in `module_facts_in_request_view`,
/// `is_evalable`. New helpers and restructured ones use this primitive to
/// converge on the ambient-view-first pattern.
pub(crate) fn effective_request_view<'a>(
    explicit: Option<&'a RequestStoreView>,
) -> EffectiveView<'a> {
    if let Some(ambient) = current_request_view() {
        EffectiveView::Ambient(ambient)
    } else if let Some(explicit) = explicit {
        EffectiveView::Explicit(explicit)
    } else {
        EffectiveView::OutsideRequest
    }
}
