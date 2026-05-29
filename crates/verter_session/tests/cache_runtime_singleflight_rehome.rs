//! Structural guard: the cooperative get-or-compute admission
//! primitive lives at `crates/verter_session/src/cache_runtime/`,
//! not at the crate root.
//!
//! The singleflight primitive (`ComputeAdmission` + the
//! `cooperative_*` entry points) and its sibling thread-coordinated
//! discriminator tests are owned by the `cache_runtime` substrate
//! module. The crate-root `cooperative_admission.rs` /
//! `cooperative_admission_tests.rs` files MUST NOT exist — a
//! re-introduction would mean a second copy of the primitive (a
//! forwarder shim or a stray fork), which this gate forbids.
//!
//! The companion behavioural discriminators that assert the
//! primitive's three-variant `ComputeAdmission<V, Entry>` shape and
//! its admission semantics live in
//! `tests/block_1_i_discriminators.rs::cooperative_return_only_not_shared_to_joiners`,
//! which reads the canonical `cache_runtime/singleflight.rs` path.

use std::path::PathBuf;

fn session_crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The primitive must live under `cache_runtime/`, and the legacy
/// crate-root home must be gone. This test FAILS on a tree where the
/// primitive still sits at the crate root (its previous home) and
/// PASSES once it has been rehomed into the substrate module — it
/// discriminates the two layouts.
#[test]
fn singleflight_primitive_lives_under_cache_runtime() {
    let root = session_crate_root();

    let legacy_module = root.join("src/cooperative_admission.rs");
    let legacy_tests = root.join("src/cooperative_admission_tests.rs");
    let rehomed_module = root.join("src/cache_runtime/singleflight.rs");
    let rehomed_tests = root.join("src/cache_runtime/singleflight_tests.rs");

    assert!(
        !legacy_module.exists(),
        "`src/cooperative_admission.rs` must NOT exist — the singleflight \
         admission primitive is owned by `src/cache_runtime/singleflight.rs`. \
         A file at the crate root means a stray second copy / forwarder shim.",
    );
    assert!(
        !legacy_tests.exists(),
        "`src/cooperative_admission_tests.rs` must NOT exist — the \
         discriminator tests moved to `src/cache_runtime/singleflight_tests.rs`.",
    );
    assert!(
        rehomed_module.exists(),
        "`src/cache_runtime/singleflight.rs` must exist — it is the canonical \
         home of the cooperative get-or-compute admission primitive.",
    );
    assert!(
        rehomed_tests.exists(),
        "`src/cache_runtime/singleflight_tests.rs` must exist — it is the \
         sibling thread-coordinated discriminator suite for the primitive.",
    );
}

/// The rehomed module must still own the verbatim primitive API: the
/// three-variant `ComputeAdmission<V, Entry>` enum and the three
/// `cooperative_*` entry points. This pins that the rehome preserved
/// the live surface (it did not collapse the type parameters or drop
/// an entry point), reading the canonical path so the assertion fails
/// if the file is missing or its API drifted.
#[test]
fn rehomed_singleflight_owns_the_verbatim_primitive_api() {
    let src =
        std::fs::read_to_string(session_crate_root().join("src/cache_runtime/singleflight.rs"))
            .expect("read cache_runtime/singleflight.rs");

    assert!(
        src.contains("pub enum ComputeAdmission<V, Entry> {"),
        "singleflight must own `ComputeAdmission<V, Entry>` with its \
         two type parameters — the stored carrier (`Entry`) and the \
         projected value (`V`) are semantically distinct and must not \
         be collapsed.",
    );
    for variant in &["Cacheable(Entry)", "ReturnOnly(V)", "Failed"] {
        assert!(
            src.contains(variant),
            "ComputeAdmission must declare the `{variant}` variant.",
        );
    }
    for entry_point in &[
        "pub fn cooperative_get_or_insert<",
        "pub fn cooperative_get_or_insert_with_post_publish<",
        "pub fn cooperative_admit_with_post_publish<",
    ] {
        assert!(
            src.contains(entry_point),
            "singleflight must expose `{entry_point}…` — one of the three \
             cooperative admission entry points the primitive owns.",
        );
    }
}
