//! Project-store-owned identity string intern pool.
//!
//! Deduplicates the identity-carrying strings the session layer mints at
//! high volume — canonical file ids and symbol names flowing into
//! [`verter_semantic::analysis::type_solver::host::ResolvedRootIdentity`]
//! and the prepared-declaration surface — so every identity for the same
//! `(path, name)` shares one `Arc<str>` allocation instead of cloning a
//! fresh `String`.
//!
//! Ownership: exactly one pool per
//! [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)
//! (per-store lifetime). Never a process global, never request-local —
//! minting boundaries receive a handle to the store's pool.
//!
//! Identity semantics: the pool is an ALLOCATION dedup only. Equality and
//! hashing of interned values remain content-based everywhere (`Arc<str>`
//! derives delegate to `str`); no pointer identity or intern ordinal ever
//! enters a cache key.
//!
//! Bounding: the pool is bounded by RETAINED PAYLOAD BYTES (the summed
//! `str` lengths it holds), not entry count. When an insert would exceed
//! the budget the pool evicts — first dropping entries only the pool still
//! references (`Arc` strong count of 1), then, if still over budget,
//! releasing its remaining references wholesale. Eviction only drops the
//! POOL's reference: every `Arc<str>` previously handed out stays alive
//! and valid; the pool merely loses the ability to dedup against it until
//! the string is interned again. A string longer than the whole budget is
//! returned un-pooled.

use std::collections::HashSet;
use std::sync::Arc;

/// Store-owned intern pool for identity-carrying strings.
///
/// See the module docs for ownership, identity, and bounding semantics.
/// The map deliberately keeps the std `HashSet` DEFAULT build hasher
/// (`RandomState` = SipHash): identity strings include workspace paths
/// derived from user input, so the pool map stays HashDoS-resistant —
/// never swap in a fast non-resistant hasher here.
pub struct IdentityInterner {
    max_retained_bytes: usize,
    inner: parking_lot::Mutex<InternerShard>,
}

#[derive(Default)]
struct InternerShard {
    entries: HashSet<Arc<str>>,
    /// Summed payload (`str`) lengths of pooled entries — the bound
    /// currency. `Arc` header overhead is a small constant per entry and
    /// intentionally outside the accounting.
    retained_bytes: usize,
}

impl IdentityInterner {
    /// Default retained-payload budget. Sized for a large workspace's
    /// unique canonical ids (~100 B each) plus symbol names (~16 B each)
    /// with generous headroom: the 179-component corpus retains well
    /// under 2 MiB of unique identity payload.
    pub const DEFAULT_MAX_RETAINED_BYTES: usize = 4 * 1024 * 1024;

    #[must_use]
    pub fn new(max_retained_bytes: usize) -> Self {
        Self {
            max_retained_bytes,
            inner: parking_lot::Mutex::new(InternerShard::default()),
        }
    }

    #[must_use]
    pub fn with_default_budget() -> Self {
        Self::new(Self::DEFAULT_MAX_RETAINED_BYTES)
    }

    /// The pooled `Arc<str>` for `s`'s content, admitting a new shared
    /// allocation on first sight. Steady state a hit is one map lookup
    /// and one refcount bump — no allocation.
    #[must_use]
    pub fn intern(&self, s: &str) -> Arc<str> {
        let mut inner = self.inner.lock();
        if let Some(existing) = inner.entries.get(s) {
            return Arc::clone(existing);
        }
        let value: Arc<str> = Arc::from(s);
        self.admit_locked(&mut inner, &value);
        value
    }

    /// Intern an EXISTING `Arc<str>` without copying: a pool miss admits
    /// the caller's own allocation; a hit returns the pooled one so
    /// content-equal identities converge onto a single allocation.
    #[must_use]
    pub fn intern_arc(&self, s: &Arc<str>) -> Arc<str> {
        let mut inner = self.inner.lock();
        if let Some(existing) = inner.entries.get(s.as_ref()) {
            return Arc::clone(existing);
        }
        self.admit_locked(&mut inner, s);
        Arc::clone(s)
    }

    /// Currently retained payload bytes (always ≤ the budget).
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.inner.lock().retained_bytes
    }

    /// Number of pooled entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().entries.is_empty()
    }

    /// Admit `value` under the retained-bytes budget, evicting first when
    /// the insert would overflow. A value longer than the WHOLE budget is
    /// never pooled (the caller still gets its allocation — it simply
    /// isn't deduplicated).
    fn admit_locked(&self, inner: &mut InternerShard, value: &Arc<str>) {
        let len = value.len();
        if len > self.max_retained_bytes {
            return;
        }
        if inner.retained_bytes + len > self.max_retained_bytes {
            Self::evict_locked(inner, self.max_retained_bytes - len);
        }
        if inner.entries.insert(Arc::clone(value)) {
            inner.retained_bytes += len;
        }
    }

    /// Two-pass eviction bringing retained bytes ≤ `target`.
    ///
    /// The first pass drops entries ONLY the pool still references
    /// (`Arc` strong count of 1 under the pool lock — no external handle
    /// can exist or be minted concurrently for such an entry), actually
    /// freeing their memory. A second pass (still over target) releases
    /// every remaining pool reference wholesale: extant `Arc`s handed
    /// out earlier stay alive and valid — the pool only loses dedup
    /// ability for them until they are interned again.
    fn evict_locked(inner: &mut InternerShard, target: usize) {
        let mut retained = inner.retained_bytes;
        inner.entries.retain(|entry| {
            if retained <= target {
                return true;
            }
            if Arc::strong_count(entry) == 1 {
                retained -= entry.len();
                false
            } else {
                true
            }
        });
        inner.retained_bytes = retained;
        if inner.retained_bytes > target {
            inner.entries.clear();
            inner.retained_bytes = 0;
        }
    }

    /// Test observability over the concrete map type (the SipHash
    /// compile-witness in the module tests reads it).
    #[cfg(test)]
    pub(crate) fn with_entries_for_test<R>(
        &self,
        f: impl FnOnce(&HashSet<Arc<str>, std::collections::hash_map::RandomState>) -> R,
    ) -> R {
        f(&self.inner.lock().entries)
    }
}

impl std::fmt::Debug for IdentityInterner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("IdentityInterner")
            .field("entries", &inner.entries.len())
            .field("retained_bytes", &inner.retained_bytes)
            .field("max_retained_bytes", &self.max_retained_bytes)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn intern_dedupes_content_equal_strings_to_one_allocation() {
        let pool = IdentityInterner::with_default_budget();
        let a = pool.intern("/src/runtime/components/Button.vue");
        // A content-equal lookup from a DIFFERENT source allocation must
        // return the SAME allocation (content-keyed, not pointer-keyed).
        let other_source = String::from("/src/runtime/components/Button.vue");
        let b = pool.intern(&other_source);
        assert!(Arc::ptr_eq(&a, &b), "content-equal interns must share");
        assert_eq!(a.as_ref(), "/src/runtime/components/Button.vue");
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn intern_arc_admits_existing_allocation_without_copy_and_dedupes() {
        let pool = IdentityInterner::with_default_budget();
        let existing: Arc<str> = Arc::from("Props");
        let admitted = pool.intern_arc(&existing);
        // First admission reuses the caller's allocation (no copy).
        assert!(Arc::ptr_eq(&admitted, &existing));
        // A later plain intern of equal content resolves to the same one.
        let again = pool.intern("Props");
        assert!(Arc::ptr_eq(&again, &existing));
    }

    #[test]
    fn retained_bytes_never_exceed_budget_and_extant_arcs_survive_eviction() {
        // Budget fits roughly three of the four 32-byte strings below.
        let pool = IdentityInterner::new(100);
        let strings: Vec<String> = (0..8)
            .map(|i| format!("/src/components/Component{i:02}.vue"))
            .collect();
        let mut held: Vec<Arc<str>> = Vec::new();
        for s in &strings {
            held.push(pool.intern(s));
            assert!(
                pool.retained_bytes() <= 100,
                "retained bytes {} exceeded budget after interning {s}",
                pool.retained_bytes()
            );
        }
        // Every Arc handed out remains alive and content-correct even
        // though the pool evicted to stay under budget.
        for (arc, s) in held.iter().zip(&strings) {
            assert_eq!(arc.as_ref(), s.as_str());
        }
        // The pool still functions after eviction: re-interning yields a
        // usable, content-equal Arc.
        let re = pool.intern(&strings[0]);
        assert_eq!(re.as_ref(), strings[0].as_str());
    }

    #[test]
    fn eviction_drops_pool_only_entries_before_externally_held_ones() {
        let pool = IdentityInterner::new(100);
        // Externally held entry: the pool + this test both hold it.
        let kept = pool.intern("/src/components/HeldAlive000.vue");
        // Pool-only entries: dropped by the test immediately.
        for i in 0..4 {
            drop(pool.intern(&format!("/src/components/PoolOnly{i:03}.vue")));
        }
        // The overflow insert must evict the pool-only entries first, so
        // the externally held entry still dedups afterwards.
        let _trigger = pool.intern("/src/components/OverflowXYZ.vue");
        let again = pool.intern("/src/components/HeldAlive000.vue");
        assert!(
            Arc::ptr_eq(&again, &kept),
            "externally held entry must survive pool-only-first eviction"
        );
    }

    #[test]
    fn oversized_string_is_returned_unpooled_and_pool_stays_bounded() {
        let pool = IdentityInterner::new(16);
        let big = "x".repeat(64);
        let a = pool.intern(&big);
        assert_eq!(a.as_ref(), big.as_str());
        assert_eq!(pool.retained_bytes(), 0, "oversized entry must not pool");
        // Not deduped (each call allocates) — but always correct.
        let b = pool.intern(&big);
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(a, b, "content equality holds regardless of pooling");
    }

    #[test]
    fn pool_map_uses_hashdos_resistant_default_hasher() {
        // Compile-witness: the pool's map type is the std HashSet with the
        // default SipHash `RandomState` build hasher — NOT a fast
        // non-resistant hasher. Widening this signature breaks the witness.
        fn witness(
            entries: &std::collections::HashSet<Arc<str>, std::collections::hash_map::RandomState>,
        ) -> usize {
            entries.len()
        }
        let pool = IdentityInterner::with_default_budget();
        let _ = pool.intern("witness");
        assert_eq!(pool.with_entries_for_test(witness), 1);
    }
}
