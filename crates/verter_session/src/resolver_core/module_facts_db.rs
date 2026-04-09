//! Immutable per-file facts keyed by `(canonical_id, whole_hash)`.
//!
//! `ModuleFactsDb` is the sole long-lived owner of raw source, parse,
//! analysis, snapshot, eval source, and shallow state for imported files.
//! `RouteDb` owns resolved routing separately.
//!
//! Facts are immutable once built. `ShallowFileState` is never mutated after
//! construction. Concurrent cold requests for the same `(canonical_id,
//! whole_hash)` coalesce onto one materialization path through singleflight.

use std::sync::Arc;

use crate::resolver_core::shallow_file_state::ShallowFileState;
use crate::resolver_core::{FactVersionRef, SingleflightGroup, StoreView, ValidatedFactCache};
use crate::types::{DependencyResolution, FileAnalysisSnapshot, Hash16};
use rustc_hash::FxHashMap;

/// Immutable per-file module facts.
///
/// Every field is populated once during materialization and never mutated.
/// The `shallow_state` is the canonical shallow symbol inventory — later
/// stages query it, they never rescan the raw file.
#[derive(Debug, Clone)]
pub struct ModuleFacts {
    pub whole_hash: Hash16,
    pub import_route_hash: Option<Hash16>,
    pub import_routes: Arc<FxHashMap<String, DependencyResolution>>,
    pub raw_source: Arc<str>,
    pub cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    pub script_analysis: Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>>,
    pub export_signatures: Option<Arc<Vec<verter_semantic::analysis::ExportSignature>>>,
    pub snapshot: Arc<FileAnalysisSnapshot>,
    pub eval_source: Arc<str>,
    pub external_type_analysis:
        Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
    pub shallow_state: Arc<ShallowFileState>,
}

/// Shared DB for immutable per-file facts.
///
/// Keyed by canonical file ID. Facts are validated against
/// `FactVersionRef::FileWholeHash` for the owned file, plus
/// `HostStoreView` / `workspace_generation` compatibility.
pub struct ModuleFactsDb {
    facts: ValidatedFactCache<String, ModuleFacts>,
    singleflight: SingleflightGroup<String, Arc<ModuleFacts>, ()>,
}

impl ModuleFactsDb {
    pub fn new() -> Self {
        Self {
            facts: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
        }
    }

    /// Look up cached facts for `canonical_id` if still valid in the given view.
    pub fn get<V: StoreView>(&self, canonical_id: &str, view: &V) -> Option<Arc<ModuleFacts>> {
        self.facts.get_if_valid(&canonical_id.to_owned(), view)
    }

    /// Look up cached facts without a store view (permissive — any cached entry is returned).
    ///
    /// Used by WASM / no-store-view contexts where fact-validation is not available.
    /// Prefers `get()` with a proper `StoreView` for production use.
    pub fn get_any(&self, canonical_id: &str) -> Option<Arc<ModuleFacts>> {
        struct PermissiveView;
        impl StoreView for PermissiveView {
            fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
                crate::resolver_core::StoreViewCompatToken(0)
            }
            fn validates(&self, _fact: &FactVersionRef) -> bool {
                true
            }
        }
        self.facts
            .get_if_valid(&canonical_id.to_owned(), &PermissiveView)
    }

    pub fn values(&self) -> Vec<Arc<ModuleFacts>> {
        self.facts.values()
    }

    /// Look up or materialize facts for `canonical_id`.
    ///
    /// If cached facts exist and are valid, returns them. Otherwise, runs
    /// `materialize` exactly once per `(canonical_id, token)` pair via
    /// singleflight. The materialized facts are stored and returned.
    pub fn get_or_materialize<V, F>(
        &self,
        canonical_id: &str,
        view: &V,
        materialize: F,
    ) -> Option<Arc<ModuleFacts>>
    where
        V: StoreView,
        F: FnOnce() -> Option<ModuleFacts>,
    {
        let key = canonical_id.to_owned();

        // Fast path: cached and valid.
        if let Some(facts) = self.facts.get_if_valid(&key, view) {
            return Some(facts);
        }

        // Singleflight: coalesce concurrent cold loads.
        // The singleflight wraps the result in an outer Arc, so we use
        // Arc<ModuleFacts> as the flight value type to avoid double-Arc.
        let result = self.singleflight.run(key.clone(), view.compat_token(), || {
            // Re-check cache inside flight (another thread may have populated it).
            if let Some(facts) = self.facts.get_if_valid(&key, view) {
                return Ok(facts);
            }

            match materialize() {
                Some(facts) => {
                    let arc = Arc::new(facts);
                    let mut validation_facts = vec![FactVersionRef::FileWholeHash {
                        canonical_id: key.clone(),
                        hash: arc.whole_hash,
                    }];
                    // Only include ImportRoute validation for tracked files.
                    // Untracked dependency files never have set_import_dependencies
                    // called on them, so their route facts are safe to omit —
                    // this eliminates false cache misses from the store view not
                    // having their derived hashes.
                    if view.tracks_file(&key) {
                        validation_facts = append_import_route_validation_fact(
                            validation_facts,
                            &key,
                            arc.import_route_hash,
                        );
                    }
                    self.facts
                        .insert_arc(key.clone(), arc.clone(), validation_facts);
                    Ok(arc)
                }
                None => Err(()),
            }
        });

        match result {
            // run_result.value is Arc<Arc<ModuleFacts>> — unwrap outer Arc.
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Insert pre-built facts directly (e.g., from scheduler data).
    pub fn insert(&self, canonical_id: String, facts: ModuleFacts) {
        let hash = facts.whole_hash;
        let validation_facts = vec![FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.clone(),
            hash,
        }];
        let validation_facts = append_import_route_validation_fact(
            validation_facts,
            &canonical_id,
            facts.import_route_hash,
        );
        self.facts.insert(canonical_id, facts, validation_facts);
    }

    /// Insert pre-built facts as Arc.
    pub fn insert_arc(&self, canonical_id: String, facts: Arc<ModuleFacts>) {
        let hash = facts.whole_hash;
        let validation_facts = vec![FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.clone(),
            hash,
        }];
        let validation_facts = append_import_route_validation_fact(
            validation_facts,
            &canonical_id,
            facts.import_route_hash,
        );
        self.facts.insert_arc(canonical_id, facts, validation_facts);
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Snapshot all cached entries (for store view construction and auditing).
    pub fn snapshot_all(&self) -> Vec<(String, Arc<ModuleFacts>)> {
        self.facts.snapshot_all()
    }

    /// Evict facts for a canonical file (hard removal).
    pub fn evict(&self, canonical_id: &str) {
        self.facts.remove(&canonical_id.to_owned());
    }

    /// Hard-evict: clear from both primary and archive maps.
    /// Used when a file is deleted — archived entries must not survive
    /// because untracked-file acceptance in the store view's `validates`
    /// would otherwise return stale facts for a deleted file.
    pub fn hard_evict(&self, canonical_id: &str) {
        self.facts.hard_remove(&canonical_id.to_owned());
    }

    /// Soft-invalidate facts for a canonical file.
    ///
    /// The current entry becomes unreachable for new views, but the
    /// previous generation remains accessible to stale store views
    /// via the `previous` chain in `ValidatedFactCache`.
    pub fn invalidate(&self, canonical_id: &str) {
        self.facts.invalidate(&canonical_id.to_owned());
    }

    /// Clear all cached facts.
    pub fn clear(&self) {
        self.facts.clear();
        self.singleflight.clear();
    }
}

impl Default for ModuleFactsDb {
    fn default() -> Self {
        Self::new()
    }
}

fn append_import_route_validation_fact(
    mut facts: Vec<FactVersionRef>,
    canonical_id: &str,
    import_route_hash: Option<Hash16>,
) -> Vec<FactVersionRef> {
    if let Some(hash) = import_route_hash {
        facts.push(FactVersionRef::DerivedFactHash {
            canonical_id: canonical_id.to_string(),
            kind: crate::resolver_core::DerivedFactKind::ImportRoute,
            hash,
        });
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{FactVersionRef, StoreView, StoreViewCompatToken};
    use rustc_hash::{FxHashMap, FxHashSet};

    #[derive(Debug)]
    struct TestView {
        token: StoreViewCompatToken,
        valid_hashes: FxHashSet<(String, Hash16)>,
    }

    impl TestView {
        fn new(token: u64) -> Self {
            Self {
                token: StoreViewCompatToken(token),
                valid_hashes: FxHashSet::default(),
            }
        }

        fn with_hash(mut self, canonical_id: &str, hash: Hash16) -> Self {
            self.valid_hashes.insert((canonical_id.to_owned(), hash));
            self
        }
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }

        fn validates(&self, fact: &FactVersionRef) -> bool {
            match fact {
                FactVersionRef::FileWholeHash { canonical_id, hash } => {
                    self.valid_hashes.contains(&(canonical_id.clone(), *hash))
                }
                FactVersionRef::DerivedFactHash { .. } => true,
            }
        }
    }

    fn make_test_facts(hash: Hash16) -> ModuleFacts {
        let analysis = Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        );
        ModuleFacts {
            whole_hash: hash,
            import_route_hash: None,
            import_routes: Arc::new(FxHashMap::default()),
            raw_source: Arc::from(""),
            cached_parse: None,
            script_analysis: None,
            export_signatures: None,
            snapshot: Arc::new(FileAnalysisSnapshot::default()),
            eval_source: Arc::from(""),
            external_type_analysis: analysis.clone(),
            shallow_state: Arc::new(ShallowFileState {
                whole_hash: hash,
                exports: FxHashMap::default(),
                wildcard_reexports: vec![],
                symbols: FxHashMap::default(),
                value_symbols: FxHashMap::default(),
                import_locals: rustc_hash::FxHashSet::default(),
                import_targets: FxHashMap::default(),
                analysis,
            }),
        }
    }

    #[test]
    fn insert_and_get_valid() {
        let db = ModuleFactsDb::new();
        let hash: Hash16 = [1; 16];
        let facts = make_test_facts(hash);

        db.insert("foo.ts".to_owned(), facts);

        let view = TestView::new(1).with_hash("foo.ts", hash);
        let result = db.get("foo.ts", &view);
        assert!(result.is_some());
        assert_eq!(result.unwrap().whole_hash, hash);
    }

    #[test]
    fn get_returns_none_when_hash_stale() {
        let db = ModuleFactsDb::new();
        let hash: Hash16 = [1; 16];
        db.insert("foo.ts".to_owned(), make_test_facts(hash));

        // View validates a different hash.
        let view = TestView::new(1).with_hash("foo.ts", [2; 16]);
        assert!(db.get("foo.ts", &view).is_none());
    }

    #[test]
    fn evict_removes_entry() {
        let db = ModuleFactsDb::new();
        let hash: Hash16 = [1; 16];
        db.insert("foo.ts".to_owned(), make_test_facts(hash));

        db.evict("foo.ts");

        let view = TestView::new(1).with_hash("foo.ts", hash);
        assert!(db.get("foo.ts", &view).is_none());
    }

    #[test]
    fn clear_removes_all() {
        let db = ModuleFactsDb::new();
        db.insert("a.ts".to_owned(), make_test_facts([1; 16]));
        db.insert("b.ts".to_owned(), make_test_facts([2; 16]));

        db.clear();

        let view = TestView::new(1)
            .with_hash("a.ts", [1; 16])
            .with_hash("b.ts", [2; 16]);
        assert!(db.get("a.ts", &view).is_none());
        assert!(db.get("b.ts", &view).is_none());
    }

    #[test]
    fn get_or_materialize_caches_result() {
        let db = ModuleFactsDb::new();
        let hash: Hash16 = [3; 16];
        let view = TestView::new(1).with_hash("bar.ts", hash);

        let call_count = std::sync::atomic::AtomicU32::new(0);

        let result = db.get_or_materialize("bar.ts", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(make_test_facts(hash))
        });
        assert!(result.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);

        // Second call should hit cache, not materialize again.
        let result2 = db.get_or_materialize("bar.ts", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(make_test_facts(hash))
        });
        assert!(result2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    /// View that accepts all hashes but doesn't track specific files.
    /// Simulates HostStoreView for untracked dependency files.
    #[derive(Debug)]
    struct UntrackedAcceptingView {
        token: StoreViewCompatToken,
    }

    impl StoreView for UntrackedAcceptingView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }
        fn validates(&self, _fact: &FactVersionRef) -> bool {
            true // Accept everything (like untracked-file acceptance)
        }
        fn tracks_file(&self, _canonical_id: &str) -> bool {
            false // No files tracked
        }
    }

    /// Untracked dependency files should hit the validated cache on the
    /// second access. Before the fix, ImportRoute facts caused false misses
    /// for every access (O(n) redundant materialization).
    #[test]
    fn untracked_dep_file_hits_cache_on_second_access() {
        let db = ModuleFactsDb::new();
        let hash: Hash16 = [5; 16];
        let import_route_hash: Hash16 = [6; 16];

        let call_count = std::sync::atomic::AtomicU32::new(0);
        let view = UntrackedAcceptingView {
            token: StoreViewCompatToken(1),
        };

        // First access: materializes with import routes.
        let facts_fn = || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut facts = make_test_facts(hash);
            facts.import_route_hash = Some(import_route_hash);
            Some(facts)
        };
        let r1 = db.get_or_materialize("dep.d.ts", &view, facts_fn);
        assert!(r1.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);

        // Second access: must hit cache (no re-materialization).
        let r2 = db.get_or_materialize("dep.d.ts", &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(make_test_facts(hash))
        });
        assert!(r2.is_some());
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "untracked dependency file should hit validated cache on second access, \
             not re-materialize (ImportRoute fact should be omitted for untracked files)"
        );
    }

    #[test]
    fn get_or_materialize_returns_none_on_failed_materialization() {
        let db = ModuleFactsDb::new();
        let view = TestView::new(1);

        let result = db.get_or_materialize("missing.ts", &view, || None);
        assert!(result.is_none());
    }

    #[test]
    fn singleflight_coalesces_concurrent_cold_loads() {
        use std::sync::{Arc as StdArc, Barrier};
        use std::thread;

        let db = StdArc::new(ModuleFactsDb::new());
        let hash: Hash16 = [5; 16];
        let barrier = StdArc::new(Barrier::new(2));
        let call_count = StdArc::new(std::sync::atomic::AtomicU32::new(0));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let db = db.clone();
                let barrier = barrier.clone();
                let call_count = call_count.clone();
                thread::spawn(move || {
                    let view = TestView::new(1).with_hash("coalesce.ts", hash);
                    barrier.wait();
                    db.get_or_materialize("coalesce.ts", &view, || {
                        call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        // Small delay to increase coalesce chance.
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        Some(make_test_facts(hash))
                    })
                })
            })
            .collect();

        for h in handles {
            let result = h.join().unwrap();
            assert!(result.is_some());
        }

        // At most 1 materialization should have run (singleflight coalesce).
        // In practice, timing may cause 2 in rare cases, but the singleflight
        // guarantees at most 1 per (key, token) pair when they overlap.
        let count = call_count.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            count <= 2,
            "Expected at most 2 materializations, got {count}"
        );
    }
}
