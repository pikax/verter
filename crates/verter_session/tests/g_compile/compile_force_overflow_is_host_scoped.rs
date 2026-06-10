//! Discriminating regression test: the compile-tier force-overflow
//! fact-injection knob and the `install_fact_tracer` overflow counter are
//! PER-HOST, not process-global.
//!
//! Root cause this pins: the knobs used to live as process-global
//! `static` atomics (`COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS`,
//! `SIGNATURE_OVERFLOW_AT_INSTALL`). They are read/incremented on the
//! PRODUCTION cold-compute path. Under shared-process `cargo test`, a
//! test that armed the global on one host poisoned CONCURRENT compiles on
//! a DIFFERENT host running on another test thread: that host's
//! `Session` cold compute read the armed global, overflowed, and refused
//! to publish its `CompileSlot` — an intermittent failure whose identity
//! shifted by run. `cargo nextest` (process-per-test) masked it.
//!
//! Post-fix the knobs are per-host fields on `VerterHost`. Arming
//! force-overflow on host A no longer affects host B.
//!
//! Discrimination: against the pre-fix tree (process-global statics), the
//! `host B publishes its CompileSlot` assertion FAILS — B's compile reads
//! A's armed global, overflows, and refuses publication; and the per-host
//! `SIGNATURE_OVERFLOW_AT_INSTALL` delta on B would be non-zero. Against
//! the post-fix tree (per-host fields) both assertions hold: A's overflow
//! is confined to A; B publishes a warm slot and observes a zero
//! overflow-counter delta.

use verter_session::for_tests::{
    compile_force_overflow_observations_for_tests, install_fact_tracer_for_tests,
    observe_fan_out_borrowed_for_tests, read_signature_overflow_at_install,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef, FACT_SIGNATURE_CAP};
use verter_session::{
    CompileCacheMode, CompileProfile, FileLanguage, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

const TRIVIAL_SFC: &str = "<script setup lang=\"ts\">\n\
     const n = 1;\n\
     </script>\n\
     <template><div>{{ n }}</div></template>\n";

/// A `Session`-mode compile of `/src/Comp.vue` is the cold path that
/// reads the compile force-overflow knob. `Session` (not `Content` /
/// `Stateless`) is required because the force-overflow injection block
/// only runs inside the `Session` fact-tracer scope.
fn session_profile() -> CompileProfile {
    CompileProfile {
        requested_mode: CompileCacheMode::Session,
        ..CompileProfile::default()
    }
}

fn prime_session_compile(host: &VerterHost, canonical: &str) {
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: session_profile(),
        })
        .expect("cold compute produces a virtual file");
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Session,
        "fixture invariant: the trivial SFC must compile in Session mode so the \
         compile-tier fact tracer (and the force-overflow read) runs"
    );
}

/// Host A arms force-overflow (1100 > FACT_SIGNATURE_CAP). A separate
/// host B then runs a normal Session compile of a trivial SFC and MUST
/// publish its `CompileSlot` — A's armed knob is invisible to B.
///
/// Pre-fix (process-global knob): B's Session cold compute reads A's
/// armed global → overflow → slot refused → both assertions FAIL.
/// Post-fix (per-host field): B is unaffected → slot published + warm.
#[test]
fn force_overflow_armed_on_host_a_does_not_refuse_host_b_compile_slot() {
    let host_a = VerterHost::new_standalone(HostConfig::default());
    let host_b = VerterHost::new_standalone(HostConfig::default());

    upsert_vue(&host_a, "/src/Comp.vue", TRIVIAL_SFC);
    upsert_vue(&host_b, "/src/Comp.vue", TRIVIAL_SFC);

    // Arm force-overflow on host A only. The guard borrows host A; its
    // knob is per-host so host B never observes it.
    let _guard = compile_force_overflow_observations_for_tests(&host_a, 1100);

    // Host B runs a normal Session compile. It MUST publish a slot.
    let profile = session_profile();
    prime_session_compile(&host_b, "/src/Comp.vue");

    assert!(
        host_b
            .compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_some(),
        "host B must publish its CompileSlot fact_dep_signature — host A's armed \
         force-overflow knob is per-host and MUST NOT poison host B's compile. \
         Pre-fix (process-global static) host B's Session cold compute read A's \
         armed global, overflowed, and refused the publish."
    );
    assert!(
        host_b.compile_slot_is_warm("/src/Comp.vue", &profile),
        "host B's published CompileSlot must be warm — a refused (overflowed) \
         publish would leave the slot absent and `compile_slot_is_warm` false."
    );

    // Sanity: host A's knob is genuinely armed — an A compile under the
    // same guard overflows and refuses to publish. This proves the guard
    // is not a no-op (so the host-B assertion above is discriminating,
    // not vacuously passing because the knob never armed anything).
    prime_session_compile(&host_a, "/src/Comp.vue");
    assert!(
        host_a
            .compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_none(),
        "host A (armed) must refuse its CompileSlot publish on overflow — this \
         confirms the force-overflow knob is active, so the host-B \
         non-interference assertion is discriminating."
    );
}

/// FACT_SIGNATURE_CAP + 1 distinct facts — observing them inside an
/// `install_fact_tracer` scope drives the finalise to `Overflow` and
/// bumps the host's `signature_overflow_at_install` counter.
fn overflow_facts() -> Vec<FactVersionRef> {
    (0u32..=(FACT_SIGNATURE_CAP as u32))
        .map(|i| {
            let mut hash = [0u8; 16];
            hash[0] = (i & 0xFF) as u8;
            hash[1] = ((i >> 8) & 0xFF) as u8;
            hash[2] = ((i >> 16) & 0xFF) as u8;
            FactVersionRef::FileWholeHash {
                canonical_id: format!("host_scoped_overflow_fact_{i}.ts"),
                hash,
            }
        })
        .collect()
}

/// The `install_fact_tracer` overflow counter is per-host: triggering an
/// overflow on host A leaves host B's `signature_overflow_at_install`
/// delta at exactly 0.
///
/// Pre-fix (process-global `SIGNATURE_OVERFLOW_AT_INSTALL`): A's overflow
/// bumps the shared counter B reads, so B's delta is non-zero. Post-fix
/// (per-host field): A's overflow is confined to A; B's delta is 0.
#[test]
fn signature_overflow_counter_is_host_scoped() {
    let host_a = VerterHost::new_standalone(HostConfig::default());
    let host_b = VerterHost::new_standalone(HostConfig::default());

    let b_before = read_signature_overflow_at_install(&host_b);
    let a_before = read_signature_overflow_at_install(&host_a);

    // Overflow host A's tracer via `install_fact_tracer` — the path that
    // increments `signature_overflow_at_install`. 1024+1 distinct facts
    // guarantees `FactReadSetFinalise::Overflow`.
    let facts = overflow_facts();
    let (_value, finalise) = install_fact_tracer_for_tests(&host_a, || {
        observe_fan_out_borrowed_for_tests(&facts);
    });
    assert!(
        matches!(finalise, FactReadSetFinalise::Overflow),
        "fixture invariant: 1024+1 facts must overflow host A's tracer — \
         otherwise the counter delta is vacuous."
    );

    let a_after = read_signature_overflow_at_install(&host_a);
    assert!(
        a_after > a_before,
        "fixture invariant: host A's overflow MUST advance host A's per-host \
         counter (before={a_before}, after={a_after})."
    );

    // Host B never overflowed anything. Its per-host counter delta MUST
    // be 0 — host A's overflow is confined to host A.
    let b_after = read_signature_overflow_at_install(&host_b);
    assert_eq!(
        b_after, b_before,
        "host B's per-host overflow counter delta MUST be 0 — host A's overflow \
         is confined to host A. Pre-fix (process-global static) A's overflow \
         bumped the shared counter B reads, so B's delta was non-zero."
    );
}
