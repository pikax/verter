//! Runtime guard — R20 admission contract: `insert_arc_with_kind`
//! refuses an empty signature for every production cache kind.
//!
//! The strict admission method
//! `ValidatedFactCache::insert_arc_with_kind(key, value, facts, kind)`
//! gates fact-completeness: an empty `facts` vector causes the
//! admission to be REFUSED — the cache entry is NOT recorded, the
//! `admission_refused_count()` counter advances, and a
//! `FactSignatureAdmissionRefused { cache_kind: <kind> }` structured
//! audit event fires.
//!
//! This complements `admission_guard.rs`'s generic empty-signature
//! refusal test with a PER-KIND enumeration over every production
//! cache that goes through `insert_arc_with_kind`. The discriminating
//! property: each kind's refusal must independently land — both on
//! the counter (one increment per refused admission) and on the
//! cache state (`cache.entries` map stays empty for that key). If a
//! future substrate change accidentally exempted one kind (e.g. by
//! taking a different code path in `insert_arc_inner`), this test
//! would observe a counter advance below the expected total OR a
//! lingering entry under one of the keys.
//!
//! ## Production cache kinds enumerated
//!
//! The six kinds below are exactly the `&'static str` literals
//! passed to `insert_arc_with_kind` in `crates/verter_session/src/`
//! (see `insert_arc_strict_admission_required.rs` for the scanner
//! that pins them down). The arch guard and this runtime test cite
//! the same list — if one drifts, both fail.
//!
//! - `"prepared_decl_bundles"` (`host_manage/prepared_decl.rs`)
//! - `"component_meta.results"` (`host_manage/component_meta_methods.rs`)
//! - `"imported_root_db.roots"` (`resolver_core/imported_root_db.rs`)
//! - `"route_db.routes"` (`resolver_core/route_db.rs`)
//! - `"route_db.barrel_surfaces"` (`resolver_core/route_db.rs`)
//! - `"route_db.effective_export_sets"` (`resolver_core/route_db.rs`)

#![cfg(test)]

use std::sync::Arc;

use verter_session::resolver_core::ValidatedFactCache;

/// Production-source cache kinds passed to `insert_arc_with_kind`.
/// Mirrors the `PRODUCTION_CACHE_KINDS` list in
/// `insert_arc_strict_admission_required.rs` — when one is added or
/// renamed, both must be updated together.
const PRODUCTION_CACHE_KINDS: &[&str] = &[
    "prepared_decl_bundles",
    "component_meta.results",
    "imported_root_db.roots",
    "route_db.routes",
    "route_db.barrel_surfaces",
    "route_db.effective_export_sets",
];

/// For every production cache kind, an empty-signature admission
/// through the strict path is REFUSED:
///
/// - The `entries` map remains empty for that key.
/// - `admission_refused_count()` advances by exactly one per kind.
/// - `signature_overflow_count()` does NOT advance (empty-signature
///   refusal is a separate refusal category from over-cap).
///
/// Discriminating signal: a substrate regression that caused
/// `insert_arc_with_kind` to silently admit an empty signature for
/// any one kind would (a) leave a cache entry under that kind's
/// key, and (b) leave `admission_refused_count()` below the per-kind
/// expected total.
#[test]
fn empty_signature_refused_per_production_cache_kind() {
    for (idx, kind) in PRODUCTION_CACHE_KINDS.iter().enumerate() {
        let cache: ValidatedFactCache<String, u64> = ValidatedFactCache::default();
        let key = format!("k_{idx}");

        // Admission attempt: empty signature, strict mode.
        cache.insert_arc_with_kind(key.clone(), Arc::new(idx as u64), Vec::new(), kind);

        // Assertion 1: the entry was NOT recorded.
        assert_eq!(
            cache.len(),
            0,
            "cache kind `{kind}`: empty-signature admission must not record an entry; \
             got len={}",
            cache.len()
        );

        // Assertion 2: the refusal counter advanced exactly once.
        assert_eq!(
            cache.admission_refused_count(),
            1,
            "cache kind `{kind}`: admission_refused_count must advance by exactly 1 \
             after one empty-signature refusal; got {}",
            cache.admission_refused_count()
        );

        // Assertion 3: no overflow counter advancement.
        assert_eq!(
            cache.signature_overflow_count(),
            0,
            "cache kind `{kind}`: empty-signature refusal must NOT advance \
             signature_overflow_count (different refusal category); got {}",
            cache.signature_overflow_count()
        );
    }
}

/// Discriminating control: a non-empty signature for the SAME cache
/// kind is admitted normally. Without this control, the refusal
/// test could pass trivially by a substrate change that broke the
/// happy path along with the refusal — the cache would always be
/// empty regardless of input.
#[test]
fn non_empty_signature_admits_normally_per_production_cache_kind() {
    use verter_semantic::analysis::Hash16;
    use verter_session::resolver_core::FactVersionRef;

    for (idx, kind) in PRODUCTION_CACHE_KINDS.iter().enumerate() {
        let cache: ValidatedFactCache<String, u64> = ValidatedFactCache::default();
        let key = format!("k_{idx}");
        let mut hash: Hash16 = [0u8; 16];
        hash[0] = idx as u8;
        let fact = FactVersionRef::FileWholeHash {
            canonical_id: format!("/w/file_{idx}.ts"),
            hash,
        };

        cache.insert_arc_with_kind(key.clone(), Arc::new(idx as u64), vec![fact], kind);

        assert_eq!(
            cache.len(),
            1,
            "cache kind `{kind}`: non-empty signature must admit; got len={}",
            cache.len()
        );
        assert_eq!(
            cache.admission_refused_count(),
            0,
            "cache kind `{kind}`: non-empty-signature admission must NOT advance \
             admission_refused_count; got {}",
            cache.admission_refused_count()
        );
        assert_eq!(
            cache.signature_overflow_count(),
            0,
            "cache kind `{kind}`: non-empty under-cap signature must NOT advance \
             signature_overflow_count; got {}",
            cache.signature_overflow_count()
        );
    }
}

/// Discriminating cross-check: repeating the empty-signature refusal
/// for the SAME kind advances the counter linearly. Without this,
/// the per-kind test could pass with a counter that saturated at 1
/// or that was reset per-kind by the substrate.
#[test]
fn repeated_empty_signature_refusals_accumulate_for_one_kind() {
    let cache: ValidatedFactCache<String, u64> = ValidatedFactCache::default();
    let kind = "prepared_decl_bundles";

    for n in 0u32..5 {
        cache.insert_arc_with_kind(format!("k_{n}"), Arc::new(n as u64), Vec::new(), kind);
    }

    assert_eq!(
        cache.len(),
        0,
        "repeated empty-signature admissions must leave the cache empty; got len={}",
        cache.len()
    );
    assert_eq!(
        cache.admission_refused_count(),
        5,
        "five empty-signature refusals must advance admission_refused_count by 5; got {}",
        cache.admission_refused_count()
    );
}
