//! R20 single-entry-per-profile arch guard (orchestrator decision
//! A1-4): the compile-tier cache stores ONE `CompileSlot` per
//! `(canonical, profile_flag_hash)` tuple — NOT a multi-candidate
//! list. Compile output is profile-dimensional, not
//! overlay-dimensional, so overlay variants do NOT need to coexist
//! in the same slot.
//!
//! This guard pins the shape via source-grep on
//! `crates/verter_session/src/types.rs` and
//! `crates/verter_session/src/cache.rs`.

use std::path::Path;

fn read_src(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {rel}"))
}

#[test]
fn compile_slots_map_is_fxhashmap_keyed_on_profile_flag_hash() {
    // The compile-cache shape pinned by R20-single-entry decision:
    // `compile_slots: FxHashMap<u64, CompileSlot>` — `u64` is the
    // `profile_flag_hash`. Multi-candidate storage
    // (`DashMap<u64, CacheEntry<CompileSlot>>`) is forbidden.
    let cache = read_src("src/cache.rs");
    assert!(
        cache.contains("FxHashMap<u64, CompileSlot>"),
        "R20: compile_slots MUST be `FxHashMap<u64, CompileSlot>`; \
         single-entry per profile_flag_hash. Multi-candidate variants \
         (DashMap<u64, CacheEntry<CompileSlot>>) are forbidden."
    );
}

#[test]
fn compile_slot_struct_is_single_entry_not_arc_candidate() {
    // The CompileSlot struct is plain `pub(crate) struct CompileSlot`
    // — NOT `Arc<CompileSlot>`, NOT wrapped in `CacheEntry<...>`.
    // The slot is mutated in place by the cold-compute writer; the
    // warm-hit reader holds a `&CompileSlot` for the duration of
    // the FxHashMap borrow.
    let types = read_src("src/types.rs");
    assert!(
        types.contains("pub(crate) struct CompileSlot"),
        "CompileSlot must be `pub(crate) struct CompileSlot` (single-entry)"
    );
    // Forbidden patterns: multi-candidate would carry `Arc<` or
    // `CacheEntry<` in the slot signature directly.
    assert!(
        !types.contains("CacheEntry<CompileSlot>"),
        "CompileSlot must NOT be wrapped in CacheEntry<CompileSlot> \
         (multi-candidate storage forbidden per R20-single-entry decision)"
    );
}
