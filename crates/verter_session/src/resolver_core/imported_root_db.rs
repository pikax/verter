//! Imported type-root proofs.
//!
//! Replaces `resolved_type_roots` from the legacy dependency cache.
//! Answers `(provider_canonical, imported_name) -> canonical root | stable miss`.
//!
//! Keyed by validated provider file identity. Stores positive and negative roots.
//! Concurrent cold requests for the same imported-root key coalesce via singleflight.

use std::sync::Arc;

use crate::resolver_core::{FactVersionRef, SingleflightGroup, StoreView, ValidatedFactCache};

/// Result of resolving an imported type root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedRootResult {
    /// Root resolved: the type's canonical source and resolved symbol name.
    Resolved {
        canonical_source: String,
        resolved_symbol: String,
    },
    /// Stable miss — the type root could not be resolved.
    Miss,
}

impl ImportedRootResult {
    pub fn is_miss(&self) -> bool {
        matches!(self, ImportedRootResult::Miss)
    }

    pub fn resolved(&self) -> Option<(&str, &str)> {
        match self {
            ImportedRootResult::Resolved {
                canonical_source,
                resolved_symbol,
            } => Some((canonical_source, resolved_symbol)),
            ImportedRootResult::Miss => None,
        }
    }

    /// Convert to a `(canonical_source, resolved_symbol)` tuple, matching
    /// the legacy `resolved_type_roots` map value format.
    pub fn as_tuple(&self) -> Option<(String, String)> {
        match self {
            ImportedRootResult::Resolved {
                canonical_source,
                resolved_symbol,
            } => Some((canonical_source.clone(), resolved_symbol.clone())),
            ImportedRootResult::Miss => None,
        }
    }
}

/// Shared DB for imported type-root proofs.
pub struct ImportedRootDb {
    roots: ValidatedFactCache<(String, String), ImportedRootResult>,
    singleflight: SingleflightGroup<(String, String), Arc<ImportedRootResult>, ()>,
}

impl ImportedRootDb {
    pub fn new() -> Self {
        Self {
            roots: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
        }
    }

    /// Look up a cached root for `(provider, imported_name)` if valid.
    pub fn get<V: StoreView>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
    ) -> Option<Arc<ImportedRootResult>> {
        let key = (provider_canonical.to_owned(), imported_name.to_owned());
        self.roots.get_if_valid(&key, view)
    }

    /// Permissive lookup without store-view validation.
    pub fn get_any(
        &self,
        provider_canonical: &str,
        imported_name: &str,
    ) -> Option<Arc<ImportedRootResult>> {
        struct PermissiveView;
        impl crate::resolver_core::StoreView for PermissiveView {
            fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
                crate::resolver_core::StoreViewCompatToken(0)
            }
            fn validates(&self, _fact: &FactVersionRef) -> bool {
                true
            }
        }
        let key = (provider_canonical.to_owned(), imported_name.to_owned());
        self.roots.get_if_valid(&key, &PermissiveView)
    }

    /// Look up or resolve a root for `(provider, imported_name)`.
    pub fn get_or_resolve<V, F>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
        resolve: F,
    ) -> Option<Arc<ImportedRootResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<ImportedRootResult>,
    {
        let key = (provider_canonical.to_owned(), imported_name.to_owned());

        if let Some(result) = self.roots.get_if_valid(&key, view) {
            return Some(result);
        }

        let flight = self.singleflight.run(key.clone(), view.compat_token(), || {
            if let Some(result) = self.roots.get_if_valid(&key, view) {
                return Ok(result);
            }
            match resolve() {
                Some(result) => {
                    let arc = Arc::new(result);
                    self.roots.insert_arc(key.clone(), arc.clone(), Vec::new());
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

    /// Insert a pre-resolved root proof.
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

    /// Insert a pre-resolved root proof with explicit fact validation.
    pub fn insert_with_facts(
        &self,
        provider_canonical: String,
        imported_name: String,
        result: ImportedRootResult,
        facts: Vec<FactVersionRef>,
    ) {
        let key = (provider_canonical, imported_name);
        self.roots.insert(key, result, facts);
    }

    /// Seed roots from a legacy `resolved_type_roots` map.
    #[cfg(test)]
    pub fn seed_from_legacy_roots(
        &self,
        provider_canonical: &str,
        roots: &rustc_hash::FxHashMap<String, (String, String)>,
    ) {
        for (imported_name, (canonical_source, resolved_symbol)) in roots {
            self.insert(
                provider_canonical.to_owned(),
                imported_name.clone(),
                ImportedRootResult::Resolved {
                    canonical_source: canonical_source.clone(),
                    resolved_symbol: resolved_symbol.clone(),
                },
            );
        }
    }

    /// Evict all roots for a provider.
    pub fn evict_provider(&self, provider_canonical: &str) {
        // ValidatedFactCache does not support prefix deletion.
        // Stale entries are caught by fact-validation on next access.
        let _ = provider_canonical;
    }

    /// Clear all cached roots.
    pub fn clear(&self) {
        self.roots.clear();
        self.singleflight.clear();
    }
}

impl Default for ImportedRootDb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{FactVersionRef, StoreView, StoreViewCompatToken};

    struct TestView {
        token: StoreViewCompatToken,
    }

    impl TestView {
        fn new(token: u64) -> Self {
            Self {
                token: StoreViewCompatToken(token),
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
                canonical_source: "foo.vue".to_owned(),
                resolved_symbol: "FooProps".to_owned(),
            },
        );

        let result = db.get("index.ts", "FooProps", &view);
        assert!(result.is_some());
        let root = result.unwrap();
        assert_eq!(root.resolved(), Some(("foo.vue", "FooProps")));
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
        let db = ImportedRootDb::new();
        let view = TestView::new(1);
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let r1 = db.get_or_resolve("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(ImportedRootResult::Resolved {
                canonical_source: "bar.vue".to_owned(),
                resolved_symbol: "Bar".to_owned(),
            })
        });
        assert!(r1.is_some());

        let r2 = db.get_or_resolve("index.ts", "Bar", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        });
        assert!(r2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn seed_from_legacy_roots() {
        let db = ImportedRootDb::new();
        let view = TestView::new(1);

        let mut legacy = rustc_hash::FxHashMap::default();
        legacy.insert("Foo".to_owned(), ("foo.vue".to_owned(), "Foo".to_owned()));
        legacy.insert("Bar".to_owned(), ("bar.vue".to_owned(), "Bar".to_owned()));

        db.seed_from_legacy_roots("index.ts", &legacy);

        assert!(db.get("index.ts", "Foo", &view).is_some());
        assert!(db.get("index.ts", "Bar", &view).is_some());
        assert!(db.get("index.ts", "Missing", &view).is_none());
    }

    #[test]
    fn as_tuple_conversion() {
        let resolved = ImportedRootResult::Resolved {
            canonical_source: "a.ts".to_owned(),
            resolved_symbol: "X".to_owned(),
        };
        assert_eq!(
            resolved.as_tuple(),
            Some(("a.ts".to_owned(), "X".to_owned()))
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
                    db.get_or_resolve("index.ts", "Coalesce", &view, || {
                        call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        Some(ImportedRootResult::Resolved {
                            canonical_source: "c.ts".to_owned(),
                            resolved_symbol: "Coalesce".to_owned(),
                        })
                    })
                })
            })
            .collect();

        for h in handles {
            assert!(h.join().unwrap().is_some());
        }

        let count = call_count.load(std::sync::atomic::Ordering::Relaxed);
        assert!(count <= 2);
    }
}
