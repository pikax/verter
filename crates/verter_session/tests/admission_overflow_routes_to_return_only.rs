//! Discriminating test: an overflowed cold-compute on the
//! compile-tier producer does NOT cache the result. A second cold
//! call MUST re-build the virtual file, exposing the
//! refuse-publish-on-overflow contract via the observable side
//! effect (cold-compute is invoked twice, not once).
//!
//! Pre-fix the empty-signature slot stayed warm, so the second call
//! returned the cached result and the compile counter only advanced
//! once. Post-fix the second call re-enters cold compute and the
//! counter advances twice.
//!
//! Counter source: `session_metrics::compile_count_by_profile` if
//! enabled, otherwise the `compile_slot_is_warm` predicate
//! discriminates: pre-fix it stays true after the first call;
//! post-fix it stays false because no slot was published.

use verter_session::for_tests::compile_force_overflow_observations_for_tests;
use verter_session::{
    CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn prime_compile(host: &VerterHost, canonical: &str) {
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Script),
        compile_profile: CompileProfile::default(),
    });
}

/// Discriminator: two cold calls with the tracer overflowing must
/// each re-build the virtual file. Pre-fix, the first overflow's
/// empty-signature slot stayed warm and the second call hit warm
/// (1 cold-compute). Post-fix, no slot is admitted and the second
/// call cold-recomputes (2 cold-computes).
#[test]
fn cold_compute_with_overflow_signature_does_not_publish_compile_slot() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();

    // First cold compute under forced overflow — no slot lands.
    {
        let _guard = compile_force_overflow_observations_for_tests(1100);
        prime_compile(&host, "/src/Comp.vue");
    }
    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after the first overflowed cold compute the slot MUST NOT be warm — \
         no compile_slots entry was admitted."
    );
    // The carrier-level invariant: no entry in compile_slots.
    assert!(
        host.compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_none(),
        "no slot is present in compile_slots after the first overflowed cold \
         compute. Pre-fix the slot landed with an empty signature; post-fix \
         the producer refuses the insert."
    );

    // Second cold compute under forced overflow — still no slot.
    // Discriminator: pre-fix the prior empty-signature slot would
    // have served warm here, skipping the cold compute entirely;
    // post-fix the producer cold-recomputes and refuses publication
    // again.
    {
        let _guard = compile_force_overflow_observations_for_tests(1100);
        prime_compile(&host, "/src/Comp.vue");
    }
    assert!(
        host.compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_none(),
        "the second overflowed cold compute MUST also refuse publication. A \
         present slot here means the first call's empty-signature slot served \
         warm — the pre-fix collapse defect masquerading as a warm hit."
    );
}
