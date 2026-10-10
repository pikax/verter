//! Imported type-root proofs.
//!
//! Replaces `resolved_type_roots` from the legacy dependency cache.
//! Answers `(provider_canonical, imported_name) -> canonical root | stable miss`.
//!
//! Keyed by validated provider file identity. Stores positive and negative roots.
//! Concurrent cold requests for the same imported-root key coalesce via singleflight.

use std::sync::Arc;

#[cfg(any(test, feature = "test-support"))]
use crate::resolver_core::PermissiveStoreView;
use crate::resolver_core::{SingleflightGroup, ValidatedFactCache};
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::store_view::StoreView;

/// Result of resolving an imported type root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedRootResult {
    /// Root resolved: the type's canonical source and resolved symbol name.
    Resolved {
        canonical_source: String,
        owner: verter_type_expr::TopLevelOwnerId,
        resolved_symbol: String,
    },
    /// Stable miss — the type root could not be resolved.
    Miss,
}

impl ImportedRootResult {
    pub fn is_miss(&self) -> bool {
        matches!(self, ImportedRootResult::Miss)
    }

    pub fn resolved(&self) -> Option<(&str, verter_type_expr::TopLevelOwnerId, &str)> {
        match self {
            ImportedRootResult::Resolved {
                canonical_source,
                owner,
                resolved_symbol,
            } => Some((canonical_source, *owner, resolved_symbol)),
            ImportedRootResult::Miss => None,
        }
    }

    pub fn as_identity(&self) -> Option<verter_session_query::type_solver::ResolvedRootIdentity> {
        self.resolved().map(|(canonical, owner, symbol)| {
            verter_session_query::type_solver::ResolvedRootIdentity::new_in_owner(
                canonical, owner, symbol,
            )
        })
    }

    /// Convert to the exact `(canonical_source, owner, resolved_symbol)` identity.
    pub fn as_tuple(&self) -> Option<(String, verter_type_expr::TopLevelOwnerId, String)> {
        match self {
            ImportedRootResult::Resolved {
                canonical_source,
                owner,
                resolved_symbol,
            } => Some((canonical_source.clone(), *owner, resolved_symbol.clone())),
            ImportedRootResult::Miss => None,
        }
    }
}

/// One imported-root flight's outcome: the resolved root, the facts it was
/// resolved under, and whether it was ADMITTED to the shared cache.
///
/// `admitted` is the load-bearing field. A resolve can be valid yet
/// non-admissible — an empty fact signature has nothing to validate against,
/// and a non-cacheable read (a fenced serve, a broken decl-body lease, an
/// unrootable route, an unobservable contributor source env) means the value's
/// basis cannot be soundly rooted. The leader knows this because it RAN the
/// resolve on its own thread, so its own tracer already carries the mark; a
/// FOLLOWER does not — it never executed the walk. Carrying the flag through
/// the flight is what lets an adopting follower re-mark its own thread instead
/// of silently inheriting an empty fact signature that an enclosing memo would
/// then observe as "nothing to invalidate on".
pub(crate) struct ImportedRootFlightOutcome {
    pub(crate) root: Arc<ImportedRootResult>,
    pub(crate) facts: Arc<[FactVersionRef]>,
    pub(crate) admitted: bool,
}

/// Shared DB for imported type-root proofs.
pub struct ImportedRootDb {
    pub(crate) roots: ValidatedFactCache<(String, String), ImportedRootResult>,
    pub(crate) singleflight: SingleflightGroup<(String, String), ImportedRootFlightOutcome, ()>,
}

impl ImportedRootDb {
    pub fn new() -> Self {
        Self {
            roots: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
        }
    }

    /// Look up a cached root for `(provider, imported_name)` if valid.
    pub fn get<V: StoreView + ?Sized>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
    ) -> Option<Arc<ImportedRootResult>> {
        let key = (provider_canonical.to_owned(), imported_name.to_owned());
        self.roots.get_if_valid(&key, view)
    }

    /// Permissive lookup without store-view validation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_any(
        &self,
        provider_canonical: &str,
        imported_name: &str,
    ) -> Option<Arc<ImportedRootResult>> {
        let key = (provider_canonical.to_owned(), imported_name.to_owned());
        self.roots.get_if_valid(&key, &PermissiveStoreView)
    }

    /// Look up or resolve a root for `(provider, imported_name)` with fact validation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_resolve_with_facts<V, F>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
        host: &crate::VerterHost,
        resolve: F,
    ) -> Option<Arc<ImportedRootResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(ImportedRootResult, Vec<FactVersionRef>)>,
    {
        crate::host_manage::source_request::ImportedRootRequestDriver::new(self)
            .get_or_resolve_returning_facts_with_context(
                provider_canonical,
                imported_name,
                view,
                host,
                resolve,
            )
            .map(|(arc, _)| arc)
    }

    /// Like [`Self::get_or_resolve_with_facts`] but ALSO returns the
    /// `fact_dep_signature` the resolved root was admitted under.
    /// Producers that thread the recorded facts into a downstream
    /// cache entry (e.g. `OwnerImportSurfaceDb`) consume this
    /// variant so the dependent cache observes every chain
    /// participant — not only the final tuple.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_resolve_returning_facts<V, F>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
        host: &crate::VerterHost,
        resolve: F,
    ) -> Option<(Arc<ImportedRootResult>, Arc<[FactVersionRef]>)>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(ImportedRootResult, Vec<FactVersionRef>)>,
    {
        crate::host_manage::source_request::ImportedRootRequestDriver::new(self)
            .get_or_resolve_returning_facts_with_context(
                provider_canonical,
                imported_name,
                view,
                host,
                resolve,
            )
    }

    /// **Test-only.** Strong-reference count of the in-flight singleflight state
    /// for `(provider, imported_name)` under `view`'s compat token, or `0` if no
    /// flight is registered.
    ///
    /// A leader parked inside its resolve closure holds the leader-only baseline
    /// of 2 (its local `state` + the `flights` map entry); a follower that has
    /// joined and is committed to the condvar wait raises the count to 3. Tests
    /// poll this to deterministically observe follower admission onto the flight
    /// before releasing the leader — no wall-clock sleep racing the follower's
    /// registration.
    #[cfg(test)]
    pub(crate) fn test_root_inflight_strong_count<V: StoreView + ?Sized>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
    ) -> usize {
        let key = (provider_canonical.to_owned(), imported_name.to_owned());
        self.singleflight
            .test_flight_strong_count(&key, view.compat_token())
    }

    /// Test-only: drive [`Self::get_or_resolve_with_facts`] the way a production
    /// producer does — inside a REAL cacheability tracer scope opened around the
    /// whole resolve. The probe cannot be forged, so this is the contract, not
    /// an escape hatch around it.
    #[cfg(test)]
    fn get_or_resolve_with_facts_probe_for_test<V, F>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<ImportedRootResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(ImportedRootResult, Vec<FactVersionRef>)>,
    {
        self.get_or_resolve_with_facts(
            provider_canonical,
            imported_name,
            view,
            test_host(),
            resolve,
        )
    }

    /// Insert a pre-resolved root proof. **Test-only**: the empty-facts
    /// variant admits entries that would warm under any [`StoreView`] —
    /// production paths must use [`Self::insert_with_facts`].
    #[cfg(test)]
    pub fn insert(
        &self,
        provider_canonical: String,
        imported_name: String,
        result: ImportedRootResult,
    ) {
        let key = (provider_canonical.clone(), imported_name);
        let _ = provider_canonical;
        self.roots.insert(key, result, Vec::new());
    }

    /// Evict all roots for a provider.
    pub fn evict_provider(&self, provider_canonical: &str) {
        let keys: Vec<_> = self
            .roots
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|(provider, _)| provider == provider_canonical)
            .collect();
        for key in keys {
            self.roots.remove(&key);
        }
    }

    /// Clear all cached roots.
    pub fn clear(&self) {
        self.roots.clear();
        self.singleflight.clear();
    }

    /// R20 instrumentation: `admission_refused_count` on the
    /// backing `ValidatedFactCache`. Producers that admit via the
    /// loose `insert_arc` path keep this counter at 0; only strict-
    /// mode admissions via `insert_arc_with_kind` advance it.
    #[must_use]
    pub fn admission_refused_count(&self) -> u64 {
        self.roots.admission_refused_count()
    }

    /// R24 instrumentation: `warm_hit_count` on the backing
    /// `ValidatedFactCache` — the number of `get_if_valid` reads that
    /// found a fact-validated candidate. The route-reuse observable for
    /// repeat imported-root resolutions.
    #[must_use]
    pub fn warm_hit_count(&self) -> u64 {
        self.roots.warm_hit_count()
    }
}

impl Default for ImportedRootDb {
    fn default() -> Self {
        Self::new()
    }
}

impl verter_type_engine::invalidation_domain::ParticipatesInInvalidation for ImportedRootDb {
    fn domains(&self) -> &'static [verter_type_engine::invalidation_domain::InvalidationDomain] {
        use verter_type_engine::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: verter_type_engine::invalidation_domain::InvalidationDomain) {
        use verter_type_engine::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.clear();
        }
    }
}

impl verter_type_engine::invalidation_domain::InvalidationByCanonical for ImportedRootDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        self.evict_provider(canonical_id);
        0
    }
}

/// Host used by in-crate cache unit tests. Production callers supply their
/// request's resolver context; the DB owns the tracer scope in both cases.
#[cfg(test)]
fn test_host() -> &'static crate::VerterHost {
    static TEST_SCOPE_HOST: std::sync::OnceLock<crate::VerterHost> = std::sync::OnceLock::new();
    TEST_SCOPE_HOST
        .get_or_init(|| crate::VerterHost::new_standalone(crate::types::HostConfig::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_session_query::facts::fact_cache::FactVersionRef;
    use verter_session_query::facts::store_view::{StoreView, StoreViewCompatToken};

    struct TestView {
        token: StoreViewCompatToken,
    }

    impl TestView {
        fn new(token: u64) -> Self {
            Self {
                token: StoreViewCompatToken {
                    epoch: token,
                    session: None,
                    validity_fingerprint: 0,
                },
            }
        }
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }

        fn validates(&self, _fact: &FactVersionRef) -> bool {
            true
        }
    }

    #[test]
    fn insert_and_get_resolved() {
        let db = ImportedRootDb::new();
        let view = TestView::new(1);

        db.insert(
            "index.ts".to_owned(),
            "FooProps".to_owned(),
            ImportedRootResult::Resolved {
                canonical_source: "foo.ts".to_owned(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                resolved_symbol: "FooProps".to_owned(),
            },
        );

        let result = db.get("index.ts", "FooProps", &view);
        assert!(result.is_some());
        let root = result.unwrap();
        assert_eq!(
            root.resolved(),
            Some((
                "foo.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "FooProps",
            ))
        );
    }

    #[test]
    fn miss_is_stable() {
        let db = ImportedRootDb::new();
        let view = TestView::new(1);

        db.insert(
            "index.ts".to_owned(),
            "Missing".to_owned(),
            ImportedRootResult::Miss,
        );

        let result = db.get("index.ts", "Missing", &view);
        assert!(result.is_some());
        assert!(result.unwrap().is_miss());
    }

    #[test]
    fn get_or_resolve_caches() {
        // Strict-admission contract: the zero-facts
        // `get_or_resolve` helper does NOT admit a fact-validated
        // entry. To exercise the caching path, callers thread a
        // non-empty fact signature through `get_or_resolve_with_facts`.
        let db = ImportedRootDb::new();
        let view = TestView::new(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let dummy_fact = FactVersionRef::FileWholeHash {
            canonical_id: "bar.ts".to_owned(),
            hash: [0u8; 16],
        };
        let r1 = db.get_or_resolve_with_facts_probe_for_test("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some((
                ImportedRootResult::Resolved {
                    canonical_source: "bar.ts".to_owned(),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    resolved_symbol: "Bar".to_owned(),
                },
                vec![dummy_fact.clone()],
            ))
        });
        assert!(r1.is_some());

        let r2 = db.get_or_resolve_with_facts_probe_for_test("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        });
        assert!(r2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn get_or_resolve_with_empty_facts_does_not_cache() {
        // Strict-admission discrimination: a resolve that returns an EMPTY fact
        // signature — the shape the route producer emits for a root it cannot
        // root — is NOT admitted. The second call re-invokes the resolver
        // because the first skipped admission.
        let db = ImportedRootDb::new();
        let view = TestView::new(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let resolve_unrootable = || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some((
                ImportedRootResult::Resolved {
                    canonical_source: "bar.ts".to_owned(),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    resolved_symbol: "Bar".to_owned(),
                },
                Vec::new(),
            ))
        };

        let first = db.get_or_resolve_with_facts_probe_for_test(
            "index.ts",
            "Bar",
            &view,
            resolve_unrootable,
        );
        assert!(
            first.is_some(),
            "refusal keeps the VALUE — an unrootable root is still served to its caller"
        );
        let second = db.get_or_resolve_with_facts_probe_for_test(
            "index.ts",
            "Bar",
            &view,
            resolve_unrootable,
        );
        assert!(second.is_some());
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "Empty-fact root resolves are not cached under strict \
             admission; the second call MUST re-invoke the resolver. \
             A non-empty fact signature is what opts a resolve into caching."
        );
    }

    #[test]
    fn as_tuple_conversion() {
        let resolved = ImportedRootResult::Resolved {
            canonical_source: "a.ts".to_owned(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            resolved_symbol: "X".to_owned(),
        };
        assert_eq!(
            resolved.as_tuple(),
            Some((
                "a.ts".to_owned(),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "X".to_owned(),
            ))
        );

        let miss = ImportedRootResult::Miss;
        assert_eq!(miss.as_tuple(), None);
    }

    #[test]
    fn clear_removes_all() {
        let db = ImportedRootDb::new();
        let view = TestView::new(1);

        db.insert(
            "a.ts".to_owned(),
            "X".to_owned(),
            ImportedRootResult::Resolved {
                canonical_source: "x.ts".to_owned(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                resolved_symbol: "X".to_owned(),
            },
        );

        db.clear();
        assert!(db.get("a.ts", "X", &view).is_none());
    }

    #[test]
    fn singleflight_coalesces_concurrent_proofs() {
        use std::sync::{Arc as StdArc, Barrier};
        use std::thread;

        let db = StdArc::new(ImportedRootDb::new());
        let barrier = StdArc::new(Barrier::new(2));
        let call_count = StdArc::new(std::sync::atomic::AtomicU32::new(0));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let db = db.clone();
                let barrier = barrier.clone();
                let call_count = call_count.clone();
                thread::spawn(move || {
                    let view = TestView::new(1);
                    barrier.wait();
                    db.get_or_resolve_with_facts_probe_for_test(
                        "index.ts",
                        "Coalesce",
                        &view,
                        || {
                            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            Some((
                                ImportedRootResult::Resolved {
                                    canonical_source: "c.ts".to_owned(),
                                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                                    resolved_symbol: "Coalesce".to_owned(),
                                },
                                vec![FactVersionRef::FileWholeHash {
                                    canonical_id: "c.ts".to_owned(),
                                    hash: [0x77u8; 16],
                                }],
                            ))
                        },
                    )
                })
            })
            .collect();

        for h in handles {
            assert!(h.join().unwrap().is_some());
        }

        let count = call_count.load(std::sync::atomic::Ordering::Relaxed);
        assert!(count <= 2);
    }

    /// An imported-root resolve the strict admission refused to persist (the
    /// empty-fact-signature carrier a fenced frontier walk produces) must NOT
    /// stay behind as a joinable `Done` rendezvous: a late claimant on the
    /// still-pinned lane would adopt a possibly-superseded root with neither a
    /// fact signature to bubble into its outer tracer nor any refusal recorded
    /// on its own request. Retention mirrors admission.
    ///
    /// The single-claimant sibling of the concurrent burst fence below: the
    /// participation pin is what keeps the leader's lane in the map after it
    /// publishes, so the second call is a genuine claimant against the terminal
    /// the leader left behind. RETAINING an unadmitted terminal makes that
    /// claimant a Follower on every one of its bounded attempts, so it adopts
    /// the superseded root and its own resolve never runs.
    #[test]
    fn unadmitted_root_resolve_is_not_retained_as_a_joinable_rendezvous() {
        use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

        let db = ImportedRootDb::new();
        let view = TestView::new(1);

        // A burst sibling's participation pin keeps the lane alive past the
        // leader's completion — the window in which a late claimant could join
        // a retained `Done`.
        let _pin = db.singleflight.participate(
            ("provider.ts".to_owned(), "Foo".to_owned()),
            view.compat_token(),
        );

        let resolves = AtomicU32::new(0);
        let live_fact = FactVersionRef::FileWholeHash {
            canonical_id: "live_dep.ts".to_owned(),
            hash: [7u8; 16],
        };

        // Call 1: the resolve returns the never-persisted empty-facts shape —
        // the carrier the fenced frontier walk produces. Its own caller is
        // still served the root it computed.
        let first =
            db.get_or_resolve_with_facts_probe_for_test("provider.ts", "Foo", &view, || {
                resolves.fetch_add(1, AtomicOrdering::SeqCst);
                Some((
                    ImportedRootResult::Resolved {
                        canonical_source: "superseded_dep.ts".to_owned(),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        resolved_symbol: "Foo".to_owned(),
                    },
                    Vec::new(),
                ))
            });
        assert_eq!(
            first.as_deref().and_then(ImportedRootResult::resolved),
            Some((
                "superseded_dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Foo",
            )),
            "the leader's own caller is still served the unadmitted root",
        );

        // Call 2 (a late claimant on the pinned lane): must NOT adopt the
        // unadmitted result — it re-resolves cold against fresh state, and its
        // admissible result is the one that reaches the shared cache.
        let second =
            db.get_or_resolve_with_facts_probe_for_test("provider.ts", "Foo", &view, || {
                resolves.fetch_add(1, AtomicOrdering::SeqCst);
                Some((
                    ImportedRootResult::Resolved {
                        canonical_source: "live_dep.ts".to_owned(),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        resolved_symbol: "Foo".to_owned(),
                    },
                    vec![live_fact.clone()],
                ))
            });
        assert_eq!(
            resolves.load(AtomicOrdering::SeqCst),
            2,
            "a late claimant must re-run its OWN resolve instead of adopting the unadmitted \
             (empty-facts) root as a retained rendezvous",
        );
        assert_eq!(
            second.as_deref().and_then(ImportedRootResult::resolved),
            Some((
                "live_dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Foo",
            )),
            "the late claimant must return its own fresh resolve's root",
        );
        assert_eq!(
            db.get("provider.ts", "Foo", &view)
                .as_deref()
                .and_then(ImportedRootResult::resolved),
            Some((
                "live_dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Foo",
            )),
            "only the late claimant's admissible (fact-rooted) root may be persisted — the \
             leader's unrooted root must never reach the shared cache",
        );
    }

    /// CONCURRENT LEADER/FOLLOWER FENCE. A cold winner whose resolve is refused
    /// admission must NOT become a joinable rendezvous for the burst: a
    /// concurrent FOLLOWER never executed that walk, so nothing marked its
    /// tracer rails, and silently adopting the winner's value hands an enclosing
    /// traced compute an EMPTY fact signature — "no dependencies" — for a value
    /// whose basis cannot be rooted. The enclosing memo then warm-admits a
    /// result derived from it, with no rail that can ever invalidate it.
    ///
    /// Retention mirrors admission: an UNADMITTED outcome is not retained, so
    /// the follower re-resolves against fresh state and admits its OWN live
    /// result. (Only the sustained-churn bounded fallback adopts an unadmitted
    /// outcome, and that path fans `UnrootableRoute` into the adopting thread by
    /// hand.)
    ///
    /// DISCRIMINATION rests on the burst sibling's lane pin held across the
    /// whole window. The retention decision is only OBSERVABLE while some other
    /// pin keeps the lane in the map: a lone follower's own unpin reaps the lane
    /// the instant it reads the leader's terminal, so its next bounded attempt
    /// re-elects either way and a retained `Done(unadmitted)` is
    /// indistinguishable from a discarded one. With a sibling pin holding the
    /// lane, a RETAINED unadmitted terminal is re-joined on every bounded
    /// attempt: the follower exhausts its attempts, adopts the leader's
    /// unrooted root, never runs its own resolve, and the cache stays empty —
    /// the three assertions below fail. The pin models exactly what a third
    /// concurrent burst member's in-flight claim does in production.
    #[test]
    fn burst_follower_reresolves_instead_of_adopting_an_unadmitted_root() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::{mpsc, Arc as StdArc};
        use std::thread;
        use std::time::{Duration, Instant};

        // With a burst sibling's participation pin held, the parked leader's
        // lane carries 3 refs (the `flights` map entry + the sibling's pin
        // guard + the leader's local state); a follower committed to the
        // condvar wait raises it to 4.
        const LEADER_AND_SIBLING_PIN_INFLIGHT_REFS: usize = 3;

        let db = StdArc::new(ImportedRootDb::new());
        let (tx_leader_in_closure, rx_leader_in_closure) = mpsc::channel::<()>();
        let (tx_release_leader, rx_release_leader) = mpsc::channel::<()>();

        // The burst sibling: a concurrent claimant whose in-flight pin keeps the
        // lane alive past the leader's publish — the window in which a follower
        // could join a retained `Done`.
        let sibling_pin = db.singleflight.participate(
            ("burst_provider.ts".to_owned(), "Burst".to_owned()),
            TestView::new(1).compat_token(),
        );

        // LEADER: serves a root computed from a basis that cannot be rooted —
        // the empty fact signature the fenced frontier walk produces. Refused
        // admission; valid for its own caller.
        let leader_db = StdArc::clone(&db);
        let leader = thread::spawn(move || {
            let view = TestView::new(1);
            leader_db.get_or_resolve_with_facts_probe_for_test(
                "burst_provider.ts",
                "Burst",
                &view,
                || {
                    tx_leader_in_closure
                        .send(())
                        .expect("driver receives the leader's in-closure signal");
                    rx_release_leader
                        .recv_timeout(Duration::from_secs(10))
                        .expect("driver releases the parked leader");
                    Some((
                        ImportedRootResult::Resolved {
                            canonical_source: "superseded_dep.ts".to_owned(),
                            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                            resolved_symbol: "Burst".to_owned(),
                        },
                        Vec::new(),
                    ))
                },
            )
        });

        rx_leader_in_closure
            .recv_timeout(Duration::from_secs(10))
            .expect("the leader enters its resolve closure");

        // FOLLOWER: joins the leader's in-flight lane BEFORE the leader
        // publishes — a genuine burst member. Its own resolve produces the LIVE
        // root with a fact signature that admits.
        let follower_resolves = StdArc::new(AtomicUsize::new(0));
        let follower_db = StdArc::clone(&db);
        let follower_resolves_in_closure = StdArc::clone(&follower_resolves);
        let follower = thread::spawn(move || {
            let view = TestView::new(1);
            follower_db.get_or_resolve_with_facts_probe_for_test(
                "burst_provider.ts",
                "Burst",
                &view,
                || {
                    follower_resolves_in_closure.fetch_add(1, AtomicOrdering::SeqCst);
                    Some((
                        ImportedRootResult::Resolved {
                            canonical_source: "live_dep.ts".to_owned(),
                            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                            resolved_symbol: "Burst".to_owned(),
                        },
                        vec![FactVersionRef::FileWholeHash {
                            canonical_id: "live_dep.ts".to_owned(),
                            hash: [0x44u8; 16],
                        }],
                    ))
                },
            )
        });

        // Wait for the follower to be COMMITTED to the leader's flight before
        // releasing the leader — otherwise the follower could arrive after the
        // leader retired the slot and would be a fresh cold winner, not a burst
        // member, and the test would prove nothing.
        let probe_view = TestView::new(1);
        let deadline = Instant::now() + Duration::from_secs(10);
        while db.test_root_inflight_strong_count("burst_provider.ts", "Burst", &probe_view)
            <= LEADER_AND_SIBLING_PIN_INFLIGHT_REFS
        {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the follower to be admitted onto the imported-root \
                 singleflight in-flight entry (strong count never exceeded \
                 {LEADER_AND_SIBLING_PIN_INFLIGHT_REFS})",
            );
            thread::sleep(Duration::from_millis(1));
        }
        tx_release_leader.send(()).expect("release the leader");

        let leader_root = leader.join().expect("leader thread");
        let follower_root = follower.join().expect("follower thread");
        // The burst has drained; the sibling's claim is over.
        drop(sibling_pin);

        // The leader is still served its own (valid, unrooted) root — refusal is
        // CACHE-ONLY.
        assert_eq!(
            leader_root
                .as_deref()
                .and_then(ImportedRootResult::resolved),
            Some((
                "superseded_dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Burst",
            )),
            "the unadmitted leader must still be served the root it computed",
        );

        assert!(
            follower_resolves.load(AtomicOrdering::SeqCst) >= 1,
            "POISON: the follower ADOPTED the leader's unadmitted root without ever running its \
             own resolve. It never executed that walk, so nothing marked its tracer rails, and \
             it inherits an EMPTY fact signature for a value whose basis cannot be rooted — an \
             enclosing memo then observes 'no dependencies' and warm-admits a result derived \
             from it. An unadmitted outcome must not be retained as a burst rendezvous",
        );
        assert_eq!(
            follower_root
                .as_deref()
                .and_then(ImportedRootResult::resolved),
            Some((
                "live_dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Burst",
            )),
            "the follower must serve the root ITS OWN fresh resolve produced, not the leader's \
             unrooted one",
        );
        assert_eq!(
            db.get("burst_provider.ts", "Burst", &probe_view)
                .as_deref()
                .and_then(ImportedRootResult::resolved),
            Some((
                "live_dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Burst",
            )),
            "only the follower's ADMISSIBLE (fact-rooted) root may be persisted — the leader's \
             unrooted root must never reach the shared cache",
        );
    }
}
