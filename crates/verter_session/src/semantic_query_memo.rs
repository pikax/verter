//! Host-owned semantic-query memo table (Phase 2.2 core).
//!
//! This module provides the concrete backing store for
//! [`SemanticQueryKey`](crate::semantic_query::SemanticQueryKey) →
//! [`SemanticNodeId`](crate::semantic_query::SemanticNodeId) memoization
//! and the stable storage for
//! [`SemanticNodeData`](crate::semantic_query::SemanticNodeData).
//!
//! ## Contract
//!
//! - One shared memo per [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore).
//! - Entries are keyed by `SemanticQueryKey`; cold winners compute the
//!   node, store it, and return its id. Joiners on the same key observe
//!   the same id (no duplicated cold work).
//! - [`SemanticNodeId`] is stable for the lifetime of the memo. Node data
//!   is stored in an append-only arena so readers can hold a long-lived
//!   id without worrying about resizing.
//! - **Same-path recursion** returns `QueryResult::Recursive(self_id)`
//!   so cycles dedup rather than re-entering.
//! - **Distinct top-level waiters** block cooperatively on a per-entry
//!   [`Condvar`] pairing (see [`InflightEntry`]).
//! - Cancelled, budget-exceeded, or partial results **never** promote to a
//!   warm memo entry; they surface as [`QueryError`] variants and the
//!   in-flight admission is removed so the next caller starts fresh.
//! - Entries are immutable once stored. Node data never retains borrowed
//!   OXC AST pointers — callers materialize semantic data before calling
//!   [`SemanticGraphStore::intern_node`].

use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;

use crate::semantic_query::{
    CacheRead, DepSignature, QueryError, QueryResult, SemanticGraphRead, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
};

// ──────────────────────────────────────────────────────────────────────────
// Node arena — append-only, stable ids
// ──────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct NodeArena {
    nodes: Mutex<Vec<Arc<SemanticNodeData>>>,
}

impl NodeArena {
    fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        let mut nodes = self.nodes.lock();
        let id = SemanticNodeId(nodes.len() as u64);
        nodes.push(Arc::new(data));
        id
    }

    fn get(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        let nodes = self.nodes.lock();
        nodes.get(id.0 as usize).cloned()
    }

    fn len(&self) -> usize {
        self.nodes.lock().len()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// In-flight admission — per-entry Mutex + Condvar pair
// ──────────────────────────────────────────────────────────────────────────

/// In-flight admission state for one cold build.
///
/// The inner mutex guards `state` exclusively; `ready` is signalled when
/// the winner publishes. Joiners wait on `ready` via `wait_while`, so they
/// do not busy-retry.
struct InflightEntry {
    state: Mutex<InflightState>,
    ready: Condvar,
}

#[derive(Default)]
struct InflightState {
    /// `None` while building; `Some` after the winner publishes.
    completed: Option<QueryResult<SemanticNodeId>>,
    /// Dep signature the winner observed.
    dep_signature: Option<DepSignature>,
    /// `true` once some thread owns the build. Subsequent threads wait on
    /// `ready` rather than trying to own it themselves.
    claimed: bool,
}

impl InflightEntry {
    fn new() -> Self {
        Self {
            state: Mutex::new(InflightState::default()),
            ready: Condvar::new(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Semantic graph store
// ──────────────────────────────────────────────────────────────────────────

/// Host-owned semantic-query memo table + node arena. One instance per
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore).
///
/// This store alone does not execute queries — it is the cache substrate.
/// Concrete resolution happens inside a dispatcher that owns the solver /
/// resolver knowledge.
#[derive(Default)]
pub struct SemanticGraphStore {
    arena: NodeArena,
    entries: Mutex<FxHashMap<SemanticQueryKey, MemoEntry>>,
    inflight: Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
}

#[derive(Clone)]
struct MemoEntry {
    result: QueryResult<SemanticNodeId>,
    dep_signature: DepSignature,
}

thread_local! {
    /// Per-thread set of query keys currently being executed. Used to
    /// detect same-path recursion so callers return a sentinel instead of
    /// self-awaiting.
    static IN_FLIGHT_ON_THIS_THREAD: RefCell<Vec<SemanticQueryKey>> =
        const { RefCell::new(Vec::new()) };
}

impl SemanticGraphStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a new immutable [`SemanticNodeData`] and return its stable id.
    pub fn intern_node(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.arena.push(data)
    }

    /// Read the resolved payload for a semantic node id. Returns `None` if
    /// the id has not been interned.
    pub fn node_data(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.arena.get(id)
    }

    /// Number of interned semantic nodes. Useful for tests and counters.
    pub fn node_count(&self) -> usize {
        self.arena.len()
    }

    /// Number of warm memo entries. Useful for tests and counters.
    pub fn memo_entry_count(&self) -> usize {
        self.entries.lock().len()
    }

    /// Warm-lookup a key. Returns the memoized result + its recorded
    /// dependency signature when present.
    pub fn get(&self, key: &SemanticQueryKey) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let entries = self.entries.lock();
        entries.get(key).cloned().map(|entry| CacheRead {
            value: entry.result,
            dep_signature: entry.dep_signature,
        })
    }

    /// Cooperative execution entry point. Semantics:
    ///
    /// 1. If the key is already warm, return the cached result and signature.
    /// 2. If the current thread is already building this exact key further
    ///    up its own stack, return
    ///    [`QueryResult::Recursive(sentinel)`](QueryResult::Recursive) —
    ///    **never self-await.**
    /// 3. If another thread is building the key, block cooperatively on the
    ///    per-entry condvar until it publishes.
    /// 4. Otherwise claim ownership, invoke `build`, publish the result,
    ///    and wake joiners.
    ///
    /// `recursion_sentinel` produces a fallback [`SemanticNodeId`] when
    /// same-path recursion is detected.
    pub fn execute_cooperative<F, R>(
        &self,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
    ) -> CacheRead<QueryResult<SemanticNodeId>>
    where
        F: FnOnce() -> (QueryResult<SemanticNodeId>, DepSignature),
        R: FnOnce() -> SemanticNodeId,
    {
        // 1. Warm memo hit.
        if let Some(hit) = self.get(&key) {
            return hit;
        }

        // 2. Same-path recursion detection — bail with a sentinel.
        let is_self_recursive =
            IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow().iter().any(|k| k == &key));
        if is_self_recursive {
            return CacheRead {
                value: QueryResult::Recursive(recursion_sentinel()),
                dep_signature: empty_signature(),
            };
        }

        // 3. Register or join the in-flight entry.
        let inflight = {
            let mut table = self.inflight.lock();
            table
                .entry(key.clone())
                .or_insert_with(|| Arc::new(InflightEntry::new()))
                .clone()
        };

        // Claim ownership or wait for the winner to publish.
        let should_build = {
            let mut state = inflight.state.lock();
            if state.claimed {
                // Cooperative wait — block on the per-entry condvar until
                // `completed` is set. Joiners never busy-spin.
                inflight
                    .ready
                    .wait_while(&mut state, |s| s.completed.is_none());
                let result = state.completed.clone().expect("winner must have published");
                let dep_signature = state.dep_signature.clone().unwrap_or_else(empty_signature);
                return CacheRead {
                    value: result,
                    dep_signature,
                };
            }
            state.claimed = true;
            true
        };
        debug_assert!(should_build);

        // 4. Execute the cold build with this key pushed onto the
        //    per-thread recursion stack so descendants see us.
        IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow_mut().push(key.clone()));
        let (result, dep_signature) = build();
        IN_FLIGHT_ON_THIS_THREAD.with(|slot| {
            let mut v = slot.borrow_mut();
            if let Some(pos) = v.iter().rposition(|k| k == &key) {
                v.remove(pos);
            }
        });

        // 5. Warm-publish only successful values; errors and recursion
        //    sentinels never become shared-cache entries.
        let publishable = matches!(&result, QueryResult::Value(_));
        if publishable {
            let mut entries = self.entries.lock();
            entries.insert(
                key.clone(),
                MemoEntry {
                    result: result.clone(),
                    dep_signature: dep_signature.clone(),
                },
            );
        }

        // 6. Finalize in-flight and wake joiners.
        {
            let mut state = inflight.state.lock();
            state.completed = Some(result.clone());
            state.dep_signature = Some(dep_signature.clone());
        }
        inflight.ready.notify_all();

        // 7. Retire the in-flight entry for non-publishable results so the
        //    next caller starts fresh.
        if !publishable {
            let mut table = self.inflight.lock();
            table.remove(&key);
        }

        CacheRead {
            value: result,
            dep_signature,
        }
    }
}

impl SemanticGraphRead for SemanticGraphStore {
    fn node_data(&self, node: SemanticNodeId) -> Arc<SemanticNodeData> {
        SemanticGraphStore::node_data(self, node).unwrap_or_else(|| {
            // Missing node id — fabricate an Opaque sentinel rather than
            // panicking. Ids are only handed out by `intern_node`, so this
            // is defensive; in debug builds the arena invariant is
            // expected to be consistent.
            Arc::new(SemanticNodeData::Opaque(QueryError::Miss))
        })
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{PrimitiveKind, ResolveDeclKey, ScopeId};

    fn scope(canonical: &str) -> ScopeId {
        ScopeId {
            canonical_id: Arc::from(canonical),
            local_scope: None,
        }
    }

    #[test]
    fn interning_returns_unique_stable_ids() {
        let store = SemanticGraphStore::new();
        let a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        assert_ne!(a, b);
        assert_eq!(a.0 + 1, b.0);
    }

    #[test]
    fn node_data_is_readable_via_graph_read_trait() {
        let store = SemanticGraphStore::new();
        let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let read: &dyn SemanticGraphRead = &store;
        let data = read.node_data(id);
        assert!(matches!(
            *data,
            SemanticNodeData::Primitive(PrimitiveKind::Boolean)
        ));
    }

    #[test]
    fn execute_cooperative_memoizes_winner_result() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });

        let mut call_count = 0u32;
        let _first = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                call_count += 1;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );

        // Second call must be a warm hit. The build closure is not invoked.
        let second = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                call_count += 1;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        );

        match second.value {
            QueryResult::Value(id) => {
                let data = store.node_data(id).unwrap();
                assert!(matches!(
                    *data,
                    SemanticNodeData::Primitive(PrimitiveKind::String)
                ));
            }
            other => panic!("expected warm value, got {other:?}"),
        }
        assert_eq!(call_count, 1, "cold build must run exactly once");
    }

    #[test]
    fn same_path_recursion_returns_sentinel_not_deadlock() {
        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Recursive"),
        });

        let store_ref = Arc::clone(&store);
        let key_ref = key.clone();

        let result = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                // Re-enter the same key from the same stack — this must
                // return a Recursive sentinel, not self-await.
                let inner = store_ref.execute_cooperative(
                    key_ref.clone(),
                    || store_ref.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        panic!("inner build must not run during same-path recursion");
                    },
                );
                match inner.value {
                    QueryResult::Recursive(_) => {
                        let id = store_ref
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
                        (QueryResult::Value(id), empty_signature())
                    }
                    other => panic!("expected Recursive sentinel, got {other:?}"),
                }
            },
        );
        assert!(matches!(result.value, QueryResult::Value(_)));
    }

    #[test]
    fn errors_do_not_warm_shared_memo() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("BadBudget"),
        });

        let first = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Error(QueryError::Miss), empty_signature()),
        );
        assert!(matches!(first.value, QueryResult::Error(_)));
        assert_eq!(
            store.memo_entry_count(),
            0,
            "errors must not promote to warm memo entries"
        );

        let mut re_ran = false;
        let second = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                re_ran = true;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );
        assert!(re_ran, "failed-result keys must not become warm");
        assert!(matches!(second.value, QueryResult::Value(_)));
    }

    #[test]
    fn dep_signature_is_returned_with_warm_hits() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });
        let sig: DepSignature = Arc::from(
            vec![(
                Arc::<str>::from("/w/a.ts"),
                crate::semantic_query::DepVersion::WholeHash([1u8; 16]),
            )]
            .into_boxed_slice(),
        );
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), sig.clone())
            },
        );
        let warm = store.get(&key).unwrap();
        assert_eq!(warm.dep_signature.len(), 1);
        assert_eq!(warm.dep_signature[0].0.as_ref(), "/w/a.ts");
    }

    #[test]
    fn recursive_sentinel_does_not_promote_to_warm_memo() {
        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("R"),
        });

        let id = store.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
        let res = store.execute_cooperative(
            key.clone(),
            || id,
            || (QueryResult::Recursive(id), empty_signature()),
        );
        assert!(matches!(res.value, QueryResult::Recursive(_)));
        assert_eq!(
            store.memo_entry_count(),
            0,
            "recursion sentinels must not promote to warm memo"
        );
    }

    /// Cross-thread waiter joins the in-flight key and observes the
    /// winner's published result. Exercises the `Condvar` pairing.
    #[test]
    fn cross_thread_joiner_waits_on_winner_publish() {
        use std::thread;
        use std::time::Duration;

        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Shared"),
        });

        let start_barrier = Arc::new(std::sync::Barrier::new(2));
        let store_owner = Arc::clone(&store);
        let key_owner = key.clone();
        let barrier_owner = Arc::clone(&start_barrier);

        let winner = thread::spawn(move || {
            store_owner.execute_cooperative(
                key_owner,
                || store_owner.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    barrier_owner.wait();
                    // Hold the build open briefly so the joiner reaches
                    // the condvar wait.
                    thread::sleep(Duration::from_millis(25));
                    let id =
                        store_owner.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(id), empty_signature())
                },
            )
        });

        // Let the winner claim first, then the joiner waits on the
        // condvar.
        start_barrier.wait();
        let joiner = thread::spawn({
            let store = Arc::clone(&store);
            let key = key.clone();
            move || {
                store.execute_cooperative(
                    key,
                    || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        panic!("joiner must never run the cold build");
                    },
                )
            }
        });

        let winner_result = winner.join().unwrap();
        let joiner_result = joiner.join().unwrap();

        // Both must see the winner's node id.
        match (winner_result.value, joiner_result.value) {
            (QueryResult::Value(w), QueryResult::Value(j)) => assert_eq!(w, j),
            other => panic!("unexpected combined result: {other:?}"),
        }
    }
}
