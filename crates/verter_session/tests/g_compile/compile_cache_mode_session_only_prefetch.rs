//! Session-only compile-tier prefetch routing.
//!
//! `prefetch_compile_tier_observation_targets` pre-populates the
//! compile-tier FACT TRACER (import-route cache + dependency
//! `IndexedReady`) so a `Session` cold compute records a non-empty
//! `fact_dep_signature`. The fact tracer is installed ONLY for the
//! `Session` cache mode; `Content` / `Stateless` compile with no fact
//! rail and never consume the prefetched state. Their compile
//! correctness (external `src=` resolution, macro-type collection, dep
//! sync) is produced independently by `compile_entry`. So the prefetch
//! is pure fact-observation pre-population and must run ONLY for
//! `Session`.
//!
//! This test drives one cold compute of a cross-file SFC (a
//! `defineProps<Foo>()` macro type dependency on a workspace `.ts`) per
//! requested mode and asserts the prefetch invocation counter:
//!   - `Content`  → downgrades to `Stateless` → counter stays `0`,
//!   - `Stateless`→ counter stays `0`,
//!   - `Session`  → counter increments (>= 1).
//!
//! Discrimination: against the pre-change tree, where the prefetch runs
//! on EVERY cold compute regardless of mode, the `Content` and
//! `Stateless` assertions (`== 0`) fail because the counter increments
//! for them too. They pass only once the call is gated to `Session`.

use verter_session::for_tests::{
    compile_tier_prefetch_invocations_for_tests, reset_compile_tier_prefetch_invocations_for_tests,
};
use verter_session::{
    CompileCacheMode, CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn upsert(host: &VerterHost, canonical: &str, source: &str, kind: FileKind) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// A cross-file SFC with a macro type dependency: `defineProps<Foo>()`
/// where `Foo` is imported from a sibling `.ts`. The macro-type-dep is
/// what makes the prefetch resolve + index `./types`.
fn seed_cross_file_sfc(host: &VerterHost) {
    upsert(
        host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
        FileKind::NonSfc,
    );
    upsert(
        host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
        FileKind::VueSfc,
    );
}

/// Drive exactly ONE cold compute of `/src/Comp.vue` under
/// `requested_mode` on a FRESH host. The prefetch invocation counter is
/// per-host, so each fresh host owns its own counter and no cross-test
/// serialization is needed. Returns the observed invocation count.
fn cold_compute_prefetch_count(requested_mode: CompileCacheMode) -> usize {
    let host = VerterHost::new_standalone(HostConfig::default());
    seed_cross_file_sfc(&host);

    let profile = CompileProfile {
        requested_mode,
        ..CompileProfile::default()
    };

    // Reset right before the cold compute so the read reflects only this
    // compute's prefetch invocations.
    reset_compile_tier_prefetch_invocations_for_tests(&host);
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile,
        })
        .expect("cold compute produces a virtual file");

    // Sanity: the macro-type-dep SFC drives the requested mode to the
    // expected actual mode (Content downgrades to Stateless; the others
    // are unchanged). This pins the routing the counter assertion relies
    // on.
    match requested_mode {
        CompileCacheMode::Content => assert_eq!(
            response.actual_mode,
            CompileCacheMode::Stateless,
            "a Content request on a macro-type-dep SFC must downgrade to Stateless"
        ),
        other => assert_eq!(
            response.actual_mode, other,
            "requested mode {other:?} must route to itself for this SFC"
        ),
    }

    compile_tier_prefetch_invocations_for_tests(&host)
}

#[test]
fn content_cold_compute_does_not_prefetch() {
    assert_eq!(
        cold_compute_prefetch_count(CompileCacheMode::Content),
        0,
        "a Content request (→ Stateless) installs no fact tracer, so the compile-tier prefetch \
         (pure fact-observation pre-population) MUST NOT run"
    );
}

#[test]
fn stateless_cold_compute_does_not_prefetch() {
    assert_eq!(
        cold_compute_prefetch_count(CompileCacheMode::Stateless),
        0,
        "a Stateless request installs no fact tracer, so the compile-tier prefetch MUST NOT run"
    );
}

#[test]
fn session_cold_compute_does_prefetch() {
    let count = cold_compute_prefetch_count(CompileCacheMode::Session);
    assert!(
        count >= 1,
        "a Session request installs the compile-tier fact tracer, so the prefetch MUST run to \
         pre-populate the tracer's import-route + IndexedReady state (got {count} invocations)"
    );
}
