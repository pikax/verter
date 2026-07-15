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
//! `tests/cases/g_block/block_1_i_discriminators.rs::cooperative_return_only_not_shared_to_joiners`,
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
fn rehomed_singleflight_exposes_the_typed_admission_contract() {
    use verter_audit::NonAdmissionReason;
    use verter_session::for_tests::ComputeAdmission;

    let cacheable: ComputeAdmission<&'static str, u32> = ComputeAdmission::Cacheable(17);
    let return_only: ComputeAdmission<&'static str, u32> = ComputeAdmission::ReturnOnly {
        value: "served",
        reason: NonAdmissionReason::PartialResult,
    };
    let failed: ComputeAdmission<&'static str, u32> = ComputeAdmission::Failed;

    assert!(matches!(cacheable, ComputeAdmission::Cacheable(17)));
    assert!(matches!(
        return_only,
        ComputeAdmission::ReturnOnly {
            value: "served",
            reason: NonAdmissionReason::PartialResult,
        }
    ));
    assert!(matches!(failed, ComputeAdmission::Failed));
}
