//! Canonical export routing facts.
//!
//! Replaces frontier wildcard resolution and export-graph-style routing state.
//! Answers `(module, exported_name) -> defining module + defining symbol | stable miss`.
//!
//! Barrel files get a `BarrelRouteSurface` built lazily on first query — all
//! wildcard specifiers are resolved once. Individual `(barrel, name)` lookups
//! then read the surface in O(1). Route misses are cached as `RouteResult::Miss`.
//!
//! Concurrent cold requests for the same barrel surface or route key coalesce
//! via singleflight.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolver_core::{
    FactVersionRef, PermissiveStoreView, SingleflightGroup, StoreView, ValidatedFactCache,
};

/// Result of resolving a named export route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// Route resolved to a defining file and symbol.
    Resolved {
        defining_canonical: String,
        defining_symbol: String,
    },
    /// Stable miss — symbol is not exported by this provider.
    Miss,
}

impl RouteResult {
    pub fn is_miss(&self) -> bool {
        matches!(self, RouteResult::Miss)
    }

    pub fn resolved(&self) -> Option<(&str, &str)> {
        match self {
            RouteResult::Resolved {
                defining_canonical,
                defining_symbol,
            } => Some((defining_canonical, defining_symbol)),
            RouteResult::Miss => None,
        }
    }
}

/// Pre-resolved wildcard route surface for a barrel file.
///
/// Maps each wildcard `source_specifier` to its resolved `canonical_id`.
/// Built lazily on first barrel query, then reused for all subsequent queries.
///
/// Version rooting lives in `fact_dep_signature` (a sorted, deduplicated
/// list of `FactVersionRef` entries the producer observed while
/// computing the surface). Concurrent file versions of the same
/// `barrel_canonical` coexist as distinct candidates inside the
/// multi-candidate `ValidatedFactCache` slot — each candidate's
/// signature validates against the current `StoreView`.
#[derive(Debug, Clone)]
pub struct BarrelRouteSurface {
    /// The barrel canonical this surface was built for.
    pub barrel_canonical: String,
    /// specifier → canonical_id
    pub wildcard_edges: FxHashMap<String, String>,
    /// Fact dependencies recorded while the surface was built — the
    /// validation signature for this candidate. Multi-candidate cache
    /// slots store one signature per candidate so concurrent file
    /// versions or overlay variants coexist without overwriting each
    /// other.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

/// Shared DB for canonical export routing facts.
pub struct RouteDb {
    /// `(provider_canonical, exported_name)` → route result.
    routes: ValidatedFactCache<(String, String), RouteResult>,
    route_singleflight: SingleflightGroup<(String, String), Arc<RouteResult>, ()>,
    /// `barrel_canonical` → full wildcard route surface (lazy, built once).
    barrel_surfaces: ValidatedFactCache<String, BarrelRouteSurface>,
    barrel_singleflight: SingleflightGroup<String, Arc<BarrelRouteSurface>, ()>,
}

impl RouteDb {
    pub fn new() -> Self {
        Self {
            routes: ValidatedFactCache::default(),
            route_singleflight: SingleflightGroup::default(),
            barrel_surfaces: ValidatedFactCache::default(),
            barrel_singleflight: SingleflightGroup::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Route lookups
    // -----------------------------------------------------------------------

    /// Look up a cached route for `(provider, name)` if valid in the view.
    pub fn get_route<V: StoreView>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
    ) -> Option<Arc<RouteResult>> {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        let result = self.routes.get_if_valid(&key, view);
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .route_db
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .route_db
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        result
    }

    /// Permissive route lookup without store-view validation.
    pub fn get_route_any(
        &self,
        provider_canonical: &str,
        exported_name: &str,
    ) -> Option<Arc<RouteResult>> {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        let result = self.routes.get_if_valid(&key, &PermissiveStoreView);
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .route_db
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .route_db
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        result
    }

    /// Look up or materialize a route for `(provider, name)`.
    pub fn get_or_resolve_route<V, F>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<RouteResult>,
    {
        self.get_or_resolve_route_with_facts(provider_canonical, exported_name, view, || {
            resolve().map(|result| (result, Vec::new()))
        })
    }

    /// Look up or materialize a route for `(provider, name)` with fact validation.
    pub fn get_or_resolve_route_with_facts<V, F>(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());

        if let Some(result) = self.routes.get_if_valid(&key, view) {
            return Some(result);
        }

        let flight = self
            .route_singleflight
            .run(key.clone(), view.compat_token(), || {
                if let Some(result) = self.routes.get_if_valid(&key, view) {
                    return Ok(result);
                }
                match resolve() {
                    Some((result, facts)) => {
                        let arc = Arc::new(result);
                        self.routes.insert_arc(key.clone(), arc.clone(), facts);
                        Ok(arc)
                    }
                    None => Err(()),
                }
            });

        match flight {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Insert a pre-resolved route. **Test-only**: the empty-facts variant
    /// admits entries that would warm under any [`StoreView`] — production
    /// paths must use [`Self::insert_route_with_facts`].
    #[cfg(test)]
    pub fn insert_route(
        &self,
        provider_canonical: String,
        exported_name: String,
        result: RouteResult,
    ) {
        let key = (provider_canonical, exported_name);
        self.routes.insert(key, result, Vec::new());
    }

    /// Insert a pre-resolved route with explicit fact validation.
    pub fn insert_route_with_facts(
        &self,
        provider_canonical: String,
        exported_name: String,
        result: RouteResult,
        facts: Vec<FactVersionRef>,
    ) {
        let key = (provider_canonical, exported_name);
        self.routes.insert(key, result, facts);
    }

    /// Evict all routes for a provider.
    pub fn evict_provider(&self, provider_canonical: &str) {
        let route_keys: Vec<_> = self
            .routes
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|(provider, _)| provider == provider_canonical)
            .collect();
        for key in route_keys {
            self.routes.remove(&key);
        }

        self.barrel_surfaces.remove(&provider_canonical.to_owned());
    }

    // -----------------------------------------------------------------------
    // Barrel surface lookups
    // -----------------------------------------------------------------------

    /// Look up a cached barrel surface if valid in the view.
    pub fn get_barrel_surface<V: StoreView>(
        &self,
        barrel_canonical: &str,
        view: &V,
    ) -> Option<Arc<BarrelRouteSurface>> {
        self.barrel_surfaces
            .get_if_valid(&barrel_canonical.to_owned(), view)
    }

    /// Look up or build a barrel surface.
    pub fn get_or_build_barrel_surface<V, F>(
        &self,
        barrel_canonical: &str,
        view: &V,
        build: F,
    ) -> Option<Arc<BarrelRouteSurface>>
    where
        V: StoreView,
        F: FnOnce() -> Option<BarrelRouteSurface>,
    {
        let key = barrel_canonical.to_owned();

        if let Some(surface) = self.barrel_surfaces.get_if_valid(&key, view) {
            return Some(surface);
        }

        let flight = self
            .barrel_singleflight
            .run(key.clone(), view.compat_token(), || {
                if let Some(surface) = self.barrel_surfaces.get_if_valid(&key, view) {
                    return Ok(surface);
                }
                match build() {
                    Some(surface) => {
                        let arc = Arc::new(surface);
                        let facts = self.barrel_validation_facts(&arc);
                        self.barrel_surfaces
                            .insert_arc(key.clone(), arc.clone(), facts);
                        Ok(arc)
                    }
                    None => Err(()),
                }
            });

        match flight {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Insert a pre-built barrel surface.
    pub fn insert_barrel_surface(&self, surface: BarrelRouteSurface) {
        let key = surface.barrel_canonical.clone();
        let facts = self.barrel_validation_facts(&surface);
        self.barrel_surfaces.insert(key, surface, facts);
    }

    // -----------------------------------------------------------------------
    // Clearing
    // -----------------------------------------------------------------------

    /// Clear all cached routes and barrel surfaces.
    pub fn clear(&self) {
        self.routes.clear();
        self.route_singleflight.clear();
        self.barrel_surfaces.clear();
        self.barrel_singleflight.clear();
    }

    // -----------------------------------------------------------------------
    // Fact construction
    // -----------------------------------------------------------------------

    /// Return the cached `fact_dep_signature` for a barrel surface as
    /// a fresh `Vec<FactVersionRef>` suitable for re-admission into a
    /// downstream `ValidatedFactCache`.
    ///
    /// The post-Stage-6c contract: the signature is already the
    /// validation oracle for the surface — it was finalised at
    /// admission time. This helper exists for callers that need to
    /// thread the existing signature into a higher-tier
    /// `insert_arc(..., facts)` call (the `ValidatedFactCache` API
    /// takes `Vec<FactVersionRef>`, not the immutable `Arc<[...]>`
    /// the candidate stores). For warm-hit observation onto the
    /// active tracer use `observe_borrowed_signature(...)` instead.
    fn barrel_validation_facts(&self, surface: &BarrelRouteSurface) -> Vec<FactVersionRef> {
        surface.fact_dep_signature.as_ref().to_vec()
    }
}

impl Default for RouteDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for RouteDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.clear();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for RouteDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        // Routes are keyed on (resolver_owner_canonical, specifier);
        // a content edit on a provider canonical evicts every route
        // routed through that provider via `evict_provider`. Returns
        // 0 because the underlying primitive does not surface a count;
        // the cascade outcome is verified via the per-DB unit tests.
        self.evict_provider(canonical_id);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{FactVersionRef, StoreView, StoreViewCompatToken};

    #[derive(Debug)]
    struct TestView {
        token: StoreViewCompatToken,
    }

    impl TestView {
        fn accepting_all(token: u64) -> Self {
            Self {
                token: StoreViewCompatToken {
                    epoch: token,
                    session: None,
                },
            }
        }
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }

        fn validates(&self, _fact: &FactVersionRef) -> bool {
            true // Accept all facts in tests.
        }
    }

    #[test]
    fn insert_and_get_route() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            "index.ts".to_owned(),
            "Foo".to_owned(),
            RouteResult::Resolved {
                defining_canonical: "foo.ts".to_owned(),
                defining_symbol: "Foo".to_owned(),
            },
        );

        let result = db.get_route("index.ts", "Foo", &view);
        assert!(result.is_some());
        let route = result.unwrap();
        assert!(
            matches!(&*route, RouteResult::Resolved { defining_canonical, .. } if defining_canonical == "foo.ts")
        );
    }

    #[test]
    fn miss_is_cached() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            "index.ts".to_owned(),
            "Missing".to_owned(),
            RouteResult::Miss,
        );

        let result = db.get_route("index.ts", "Missing", &view);
        assert!(result.is_some());
        assert!(result.unwrap().is_miss());
    }

    #[test]
    fn get_or_resolve_route_caches_result() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let result = db.get_or_resolve_route("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(RouteResult::Resolved {
                defining_canonical: "bar.ts".to_owned(),
                defining_symbol: "Bar".to_owned(),
            })
        });
        assert!(result.is_some());

        // Second call should hit cache.
        let result2 = db.get_or_resolve_route("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(RouteResult::Miss)
        });
        assert!(result2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn barrel_surface_insert_and_get() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        let surface = BarrelRouteSurface {
            barrel_canonical: "barrel.ts".to_owned(),
            wildcard_edges: {
                let mut m = FxHashMap::default();
                m.insert("./foo".to_owned(), "foo.ts".to_owned());
                m.insert("./bar".to_owned(), "bar.ts".to_owned());
                m
            },
            fact_dep_signature: Arc::from(
                vec![
                    FactVersionRef::FileWholeHash {
                        canonical_id: "barrel.ts".to_owned(),
                        hash: [1; 16],
                    },
                    FactVersionRef::FileWholeHash {
                        canonical_id: "foo.ts".to_owned(),
                        hash: [2; 16],
                    },
                    FactVersionRef::FileWholeHash {
                        canonical_id: "bar.ts".to_owned(),
                        hash: [3; 16],
                    },
                ]
                .into_boxed_slice(),
            ),
        };

        db.insert_barrel_surface(surface);

        let result = db.get_barrel_surface("barrel.ts", &view);
        assert!(result.is_some());
        let s = result.unwrap();
        assert_eq!(s.wildcard_edges.len(), 2);
        assert_eq!(s.wildcard_edges.get("./foo").unwrap(), "foo.ts");
    }

    #[test]
    fn get_or_build_barrel_surface_caches() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let result = db.get_or_build_barrel_surface("barrel.ts", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(BarrelRouteSurface {
                barrel_canonical: "barrel.ts".to_owned(),
                wildcard_edges: FxHashMap::default(),
                fact_dep_signature: Arc::from(
                    vec![FactVersionRef::FileWholeHash {
                        canonical_id: "barrel.ts".to_owned(),
                        hash: [1; 16],
                    }]
                    .into_boxed_slice(),
                ),
            })
        });
        assert!(result.is_some());

        let result2 = db.get_or_build_barrel_surface("barrel.ts", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        });
        assert!(result2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn clear_removes_all() {
        let db = RouteDb::new();
        let view = TestView::accepting_all(1);

        db.insert_route(
            "a.ts".to_owned(),
            "X".to_owned(),
            RouteResult::Resolved {
                defining_canonical: "x.ts".to_owned(),
                defining_symbol: "X".to_owned(),
            },
        );
        db.insert_barrel_surface(BarrelRouteSurface {
            barrel_canonical: "b.ts".to_owned(),
            wildcard_edges: FxHashMap::default(),
            fact_dep_signature: Arc::from(
                vec![FactVersionRef::FileWholeHash {
                    canonical_id: "b.ts".to_owned(),
                    hash: [1; 16],
                }]
                .into_boxed_slice(),
            ),
        });

        db.clear();

        assert!(db.get_route("a.ts", "X", &view).is_none());
        assert!(db.get_barrel_surface("b.ts", &view).is_none());
    }
}
