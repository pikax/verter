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
//! Bounding: the pool holds NO private byte quota. Every pooled entry's
//! payload is reserved against the one shared
//! [`SemanticRetentionAccount`](crate::semantic_retention_account::SemanticRetentionAccount),
//! so the pool's growth competes with every other host-owned store for
//! the same ratified aggregate ceiling instead of carrying a parallel
//! budget beside it.
//!
//! Pressure handling: when the account refuses a payload, the pool SHEDS
//! — it drops entries only the pool still references (`Arc` strong count
//! of 1 under the pool lock), which actually frees their memory and
//! releases their charges, then retries the reservation once. If the
//! account still refuses, the string is returned UN-POOLED: the caller
//! keeps a correct allocation, the pool simply loses dedup for it.
//! Shedding only drops the POOL's reference — every `Arc<str>` previously
//! handed out stays alive and valid.

use std::collections::HashMap;
use std::sync::Arc;

use crate::semantic_retention_account::{
    ChargeClass, RetentionAdmission, RetentionCharge, SemanticRetentionAccount,
};

/// Store-owned intern pool for identity-carrying strings.
///
/// See the module docs for ownership, identity, and bounding semantics.
/// The map deliberately keeps the std `HashMap` DEFAULT build hasher
/// (`RandomState` = SipHash): identity strings include workspace paths
/// derived from user input, so the pool map stays HashDoS-resistant —
/// never swap in a fast non-resistant hasher here.
pub struct IdentityInterner {
    account: Arc<SemanticRetentionAccount>,
    inner: parking_lot::Mutex<InternerShard>,
}

#[derive(Default)]
struct InternerShard {
    /// The pooled strings, each mapped to the retention charge that
    /// keeps its payload accounted.
    ///
    /// The KEY is the pooled `Arc<str>` — an immutable, content-hashed
    /// identity — and the charge is the VALUE, so the map's key type
    /// stays free of the account's interior mutability. Holding the
    /// charge beside its string is what makes the pool's release
    /// exactly-once: a removal drops both together, and there is no
    /// separate bookkeeping step a shed path could skip.
    entries: HashMap<Arc<str>, RetentionCharge>,
    /// Summed payload (`str`) lengths of pooled entries — diagnostics
    /// only. The authoritative bound is the shared account.
    retained_bytes: usize,
}

impl IdentityInterner {
    /// Construct a pool charging `account`.
    #[must_use]
    pub fn new(account: Arc<SemanticRetentionAccount>) -> Self {
        Self {
            account,
            inner: parking_lot::Mutex::new(InternerShard::default()),
        }
    }

    /// Construct a pool charging the shared process-local account.
    #[must_use]
    pub fn with_process_local_account() -> Self {
        Self::new(SemanticRetentionAccount::process_local())
    }

    /// The pooled `Arc<str>` for `s`'s content, admitting a new shared
    /// allocation on first sight. Steady state a hit is one map lookup
    /// and one refcount bump — no allocation.
    #[must_use]
    pub fn intern(&self, s: &str) -> Arc<str> {
        let mut inner = self.inner.lock();
        if let Some((existing, _)) = inner.entries.get_key_value(s) {
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
        if let Some((existing, _)) = inner.entries.get_key_value(s.as_ref()) {
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

    /// Reserve `value`'s payload against the shared account and pool it.
    ///
    /// On refusal the pool SHEDS its pool-only entries (releasing their
    /// charges) and retries the reservation exactly once. A second
    /// refusal leaves the string un-pooled: the caller keeps a correct
    /// allocation and the pool simply does not dedup it. The pool never
    /// stores an entry whose bytes it did not reserve, so its charged
    /// total always equals its retained payload.
    fn admit_locked(&self, inner: &mut InternerShard, value: &Arc<str>) {
        let len = value.len();
        let charge = match self.account.reserve(ChargeClass::Retained, len) {
            RetentionAdmission::Admitted(charge) => charge,
            RetentionAdmission::Refused(_) => {
                Self::shed_pool_only_locked(inner);
                match self.account.reserve(ChargeClass::Retained, len) {
                    RetentionAdmission::Admitted(charge) => charge,
                    RetentionAdmission::Refused(_) => return,
                }
            }
        };
        if inner.entries.insert(Arc::clone(value), charge).is_none() {
            inner.retained_bytes += len;
        }
    }

    /// Drop every entry ONLY the pool still references (`Arc` strong
    /// count of 1 under the pool lock — no external handle can exist or
    /// be minted concurrently for such an entry), actually freeing their
    /// memory and releasing their charges.
    ///
    /// Entries an external holder still references are left alone: their
    /// bytes stay alive whether or not the pool indexes them, so dropping
    /// the pool's reference would free nothing while destroying dedup.
    /// A shed can therefore legitimately reclaim nothing, which is why
    /// the caller must tolerate a second refusal.
    fn shed_pool_only_locked(inner: &mut InternerShard) {
        let mut shed = 0usize;
        inner.entries.retain(|value, _charge| {
            if Arc::strong_count(value) == 1 {
                shed += value.len();
                false
            } else {
                true
            }
        });
        inner.retained_bytes -= shed;
        verter_audit::attribute_n!(RetentionPoolShed, shed);
    }

    /// Test observability over the concrete map type (the SipHash
    /// compile-witness in the module tests reads it).
    #[cfg(test)]
    pub(crate) fn with_entries_for_test<R>(
        &self,
        f: impl FnOnce(
            &HashMap<Arc<str>, RetentionCharge, std::collections::hash_map::RandomState>,
        ) -> R,
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
            .field("account", &self.account.snapshot())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_retention_account::RetentionLimits;
    use std::sync::Arc;

    /// A private account with a tight ceiling, so a pressure test never
    /// perturbs (or is perturbed by) a concurrently-running test.
    fn pool_with_ceiling(aggregate_ceiling_bytes: usize) -> IdentityInterner {
        IdentityInterner::new(SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes,
            active_ceiling_bytes: aggregate_ceiling_bytes,
            pin_threshold_bytes: aggregate_ceiling_bytes,
            max_entry_bytes: aggregate_ceiling_bytes,
        }))
    }

    #[test]
    fn intern_dedupes_content_equal_strings_to_one_allocation() {
        let pool = IdentityInterner::with_process_local_account();
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
        let pool = IdentityInterner::with_process_local_account();
        let existing: Arc<str> = Arc::from("Props");
        let admitted = pool.intern_arc(&existing);
        // First admission reuses the caller's allocation (no copy).
        assert!(Arc::ptr_eq(&admitted, &existing));
        // A later plain intern of equal content resolves to the same one.
        let again = pool.intern("Props");
        assert!(Arc::ptr_eq(&again, &existing));
    }

    /// The pool carries NO private byte quota: its bound is the shared
    /// account's headroom, and every pooled payload is charged there.
    #[test]
    fn pooled_payload_is_charged_to_the_shared_account() {
        let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
        let pool = IdentityInterner::new(Arc::clone(&account));
        let before = account.snapshot().retained_bytes;
        let held = pool.intern("/src/components/Charged.vue");
        assert_eq!(
            account.snapshot().retained_bytes - before,
            held.len(),
            "the pooled payload must be charged to the shared account"
        );
        drop(pool);
        assert_eq!(
            account.snapshot().retained_bytes,
            before,
            "dropping the pool must release exactly the bytes it charged"
        );
    }

    #[test]
    fn retained_bytes_never_exceed_the_account_ceiling_and_extant_arcs_survive() {
        // Ceiling fits roughly three of the ~32-byte strings below.
        let pool = pool_with_ceiling(100);
        let strings: Vec<String> = (0..8)
            .map(|i| format!("/src/components/Component{i:02}.vue"))
            .collect();
        let mut held: Vec<Arc<str>> = Vec::new();
        for s in &strings {
            held.push(pool.intern(s));
            assert!(
                pool.retained_bytes() <= 100,
                "retained bytes {} exceeded the account ceiling after interning {s}",
                pool.retained_bytes()
            );
        }
        // Every Arc handed out remains alive and content-correct even
        // though the pool shed entries to stay under the ceiling.
        for (arc, s) in held.iter().zip(&strings) {
            assert_eq!(arc.as_ref(), s.as_str());
        }
        // The pool still functions after shedding: re-interning yields a
        // usable, content-equal Arc.
        let re = pool.intern(&strings[0]);
        assert_eq!(re.as_ref(), strings[0].as_str());
    }

    #[test]
    fn shedding_drops_pool_only_entries_before_externally_held_ones() {
        let pool = pool_with_ceiling(100);
        // Externally held entry: the pool + this test both hold it.
        let kept = pool.intern("/src/components/HeldAlive000.vue");
        // Pool-only entries: dropped by the test immediately.
        for i in 0..4 {
            drop(pool.intern(&format!("/src/components/PoolOnly{i:03}.vue")));
        }
        // The overflowing insert must shed the pool-only entries first,
        // so the externally held entry still dedups afterwards.
        let _trigger = pool.intern("/src/components/OverflowXYZ.vue");
        let again = pool.intern("/src/components/HeldAlive000.vue");
        assert!(
            Arc::ptr_eq(&again, &kept),
            "externally held entry must survive pool-only-first shedding"
        );
    }

    #[test]
    fn unpoolable_string_is_returned_correct_and_pool_stays_bounded() {
        let pool = pool_with_ceiling(16);
        let big = "x".repeat(64);
        let a = pool.intern(&big);
        assert_eq!(a.as_ref(), big.as_str());
        assert_eq!(
            pool.retained_bytes(),
            0,
            "a payload the account refuses must not pool"
        );
        // Not deduped (each call allocates) — but always correct.
        let b = pool.intern(&big);
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(a, b, "content equality holds regardless of pooling");
    }

    #[test]
    fn pool_map_uses_hashdos_resistant_default_hasher() {
        // Compile-witness: the pool's map type is the std HashMap with
        // the default SipHash `RandomState` build hasher — NOT a fast
        // non-resistant hasher. Widening this signature breaks the witness.
        fn witness(
            entries: &std::collections::HashMap<
                Arc<str>,
                crate::semantic_retention_account::RetentionCharge,
                std::collections::hash_map::RandomState,
            >,
        ) -> usize {
            entries.len()
        }
        let pool = IdentityInterner::with_process_local_account();
        let _ = pool.intern("witness");
        assert_eq!(pool.with_entries_for_test(witness), 1);
    }
}
