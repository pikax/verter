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
use crate::types::Hash16;

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
#[derive(Debug, Clone)]
pub struct BarrelRouteSurface {
    pub barrel_canonical: String,
    /// specifier → canonical_id
    pub wildcard_edges: FxHashMap<String, String>,
    /// Hash of the barrel file that produced this surface.
    pub whole_hash: Hash16,
    /// Hashes of the wildcard source files at build time.
    pub source_hashes: Vec<(String, Hash16)>,
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
        self.routes.get_if_valid(&key, view)
    }

    /// Permissive route lookup without store-view validation.
    pub fn get_route_any(
        &self,
        provider_canonical: &str,
        exported_name: &str,
    ) -> Option<Arc<RouteResult>> {
        let key = (provider_canonical.to_owned(), exported_name.to_owned());
        self.routes.get_if_valid(&key, &PermissiveStoreView)
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

    fn barrel_validation_facts(&self, surface: &BarrelRouteSurface) -> Vec<FactVersionRef> {
        let mut facts = vec![FactVersionRef::FileWholeHash {
            canonical_id: surface.barrel_canonical.clone(),
            hash: surface.whole_hash,
        }];
        for (source_canonical, source_hash) in &surface.source_hashes {
            facts.push(FactVersionRef::FileWholeHash {
                canonical_id: source_canonical.clone(),
                hash: *source_hash,
            });
        }
        facts
    }
}

impl Default for RouteDb {
    fn default() -> Self {
        Self::new()
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
            whole_hash: [1; 16],
            source_hashes: vec![
                ("foo.ts".to_owned(), [2; 16]),
                ("bar.ts".to_owned(), [3; 16]),
            ],
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
                whole_hash: [1; 16],
                source_hashes: vec![],
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
            whole_hash: [1; 16],
            source_hashes: vec![],
        });

        db.clear();

        assert!(db.get_route("a.ts", "X", &view).is_none());
        assert!(db.get_barrel_surface("b.ts", &view).is_none());
    }
}
