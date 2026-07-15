//! Structural-shape regression-prevention guards.
//!
//! These guards are NOT pre/post discriminators for this change —
//! `ReadSetSignature` already existed in the parent branch with the
//! same `{ facts, overflowed }` field set, and `CacheEntry<V>`
//! already carried `{ value, signature, self_root_canonicals,
//! validated_at_generation }`. The struct shapes asserted here are
//! the SAME pre- and post-change.
//!
//! The guards are kept (rather than removed) because they cheaply
//! pin the architectural separation: the signature carrier owns
//! facts + overflow only; the cache entry owns the world generation
//! alongside the value. A future change that conflates the two
//! responsibilities (e.g. moving `validated_at_generation` onto
//! `ReadSetSignature`) would silently weaken the carrier's role —
//! the syn-AST assertion below catches that drift.

use std::path::PathBuf;

/// Parse `fact_signature_helpers.rs`'s `ReadSetSignature` struct via
/// `syn::parse_file` and assert it has EXACTLY two fields named
/// `facts` and `overflowed`. There MUST NOT be a
/// `validated_at_generation` field on the carrier — that is the
/// `CacheEntry<V>` responsibility, not the signature's.
///
/// Regression-prevention guard, NOT a discriminator for this
/// change. See file header.
#[test]
fn regression_guard_read_set_signature_has_no_generation_field() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("fact_signature_helpers.rs");
    let src = std::fs::read_to_string(&path).expect("read fact_signature_helpers.rs");
    let parsed = syn::parse_file(&src).expect("syn parse fact_signature_helpers.rs");

    let mut found = false;
    for item in &parsed.items {
        let syn::Item::Struct(s) = item else { continue };
        if s.ident != "ReadSetSignature" {
            continue;
        }
        found = true;
        let syn::Fields::Named(fields) = &s.fields else {
            panic!("ReadSetSignature MUST have named fields");
        };
        let names: Vec<String> = fields
            .named
            .iter()
            .map(|f| f.ident.as_ref().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["facts".to_string(), "overflowed".to_string()],
            "ReadSetSignature MUST have exactly two fields: `facts` (the \
             path-precise fact rail) + `overflowed` (the structural carrier \
             bit that distinguishes overflow from empty). Generation belongs \
             on `CacheEntry<V>`, not on the signature carrier."
        );
        // Belt-and-suspenders: assert no `validated_at_generation` field.
        assert!(
            !names.iter().any(|n| n == "validated_at_generation"),
            "ReadSetSignature MUST NOT carry `validated_at_generation` — \
             generation rides on `CacheEntry<V>` alongside the value. \
             Conflating them onto the signature blurs the responsibility \
             boundary."
        );
    }
    assert!(
        found,
        "ReadSetSignature struct MUST be declared in fact_signature_helpers.rs"
    );
}

/// Parse `cache_runtime/admission.rs` and assert that `CacheEntry<V>`
/// carries exactly the three fields `value`, `signature`,
/// `validated_at_generation` (plus the `self_root_canonicals` rail).
/// Generation lives HERE, not on the signature carrier.
///
/// Regression-prevention guard, NOT a discriminator for this
/// change. See file header.
#[test]
fn regression_guard_cache_entry_carries_generation_distinct_from_signature() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("cache_runtime")
        .join("admission.rs");
    let src = std::fs::read_to_string(&path).expect("read admission.rs");
    let parsed = syn::parse_file(&src).expect("syn parse admission.rs");

    let mut found = false;
    for item in &parsed.items {
        let syn::Item::Struct(s) = item else { continue };
        if s.ident != "CacheEntry" {
            continue;
        }
        found = true;
        let syn::Fields::Named(fields) = &s.fields else {
            panic!("CacheEntry MUST have named fields");
        };
        let names: Vec<String> = fields
            .named
            .iter()
            .map(|f| f.ident.as_ref().unwrap().to_string())
            .collect();
        // CacheEntry<V> carries:
        // - value: V (the cached value)
        // - signature: ReadSetSignature (the validation rail)
        // - self_root_canonicals: Arc<[Arc<str>]> (strict-validation rail)
        // - validated_at_generation: u64 (world generation)
        assert!(
            names.contains(&"value".to_string()),
            "CacheEntry MUST carry a `value` field; found {names:?}"
        );
        assert!(
            names.contains(&"signature".to_string()),
            "CacheEntry MUST carry a `signature: ReadSetSignature` field; found {names:?}"
        );
        assert!(
            names.contains(&"validated_at_generation".to_string()),
            "CacheEntry MUST carry a `validated_at_generation` field DISTINCT from \
             the signature — the signature validates path-precise facts, the \
             generation validates the world snapshot the value was computed under. \
             Found {names:?}"
        );
        // Optional but expected: self_root_canonicals.
        assert!(
            names.contains(&"self_root_canonicals".to_string()),
            "CacheEntry MUST carry a `self_root_canonicals` rail; found {names:?}"
        );
    }
    assert!(
        found,
        "CacheEntry struct MUST be declared in cache_runtime/admission.rs"
    );
}
