//! `DerivedRawState.cached_resolved_meta` fact-validation
//! discriminator (POSITIVE).
//!
//! A `ResolvedComponentMetaCacheEntry.fact_versions:
//! Vec<FactVersionRef>` whose consumer (`try_get_cached_resolved_meta`)
//! used `view.invalid_fact_details(&cached.fact_versions, 6)` instead
//! of the per-domain fast-path validator would FAIL the migration
//! source-grep arch guard.
//!
//! The substrate is `Arc<[FactVersionRef]>` and
//! `try_get_cached_resolved_meta` short-circuits via
//! `view.validates_fact_signature(...)`. Editing a referenced dep
//! triggers an invalidation observable via the
//! `component_meta_resolved_state_recomputes` counter — the second
//! call after the edit must NOT be served from the resolved-meta
//! warm cache because the validator catches the version bump.

use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

fn read_session_src(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

/// Substrate arch guard + consumer wiring: `ResolvedComponentMetaCacheEntry`
/// carries `Arc<[FactVersionRef]>` and the consumer routes through
/// the fast-path validator.
#[test]
fn cached_resolved_meta_substrate_and_consumer_wired() {
    let types_src = read_session_src("types.rs");
    let needle = "pub(crate) struct ResolvedComponentMetaCacheEntry {";
    let idx = types_src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let end = types_src[idx..]
        .find("\n}")
        .expect("ResolvedComponentMetaCacheEntry struct close");
    let window = &types_src[idx..idx + end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "ResolvedComponentMetaCacheEntry.fact_versions must be \
         `Arc<[FactVersionRef]>`. Window:\n{window}"
    );
    assert!(
        !window.contains("fact_versions: Vec<"),
        "ResolvedComponentMetaCacheEntry must NOT carry the legacy `Vec<FactVersionRef>` \
         shape. Window:\n{window}"
    );

    // Consumer-site arch guard: `try_get_cached_resolved_meta`
    // must dispatch through `validates_fact_signature` so the
    // per-domain fast-path is the live invalidation oracle. A
    // regression that reverts to the legacy `.iter().all(view.validates(...))`
    // form or to `invalid_fact_details` as the gating predicate
    // would erase this assertion.
    //
    // The consumer is split into an owned-view
    // wrapper (`try_get_cached_resolved_meta_for_view_fingerprint`)
    // and a view-threading implementation
    // (`try_get_cached_resolved_meta_for_view_fingerprint_with_store_view`).
    // The wrapper is a thin delegation to the `_with_store_view`
    // variant; the architecturally meaningful validation lives in the
    // implementation function. Source-grep the implementation
    // function so the architectural intent ("fact-signature validation
    // happens at the warm-hit gate") is asserted against the live
    // call site.
    let consumer_src = read_session_src("host_manage/component_meta_methods.rs");
    let consumer_needle = "fn try_get_cached_resolved_meta_for_view_fingerprint_with_store_view(";
    let cidx = consumer_src.find(consumer_needle).unwrap_or_else(|| {
        panic!("expected `{consumer_needle}` in host_manage/component_meta_methods.rs")
    });
    let cend = consumer_src[cidx..]
        .find("\n    }\n")
        .expect("try_get_cached_resolved_meta_for_view_fingerprint_with_store_view fn close");
    let cwindow = &consumer_src[cidx..cidx + cend];
    assert!(
        cwindow.contains("view.validates_fact_signature(&cached.fact_versions)"),
        "try_get_cached_resolved_meta_for_view_fingerprint_with_store_view \
         must gate the warm-hit return on `view.validates_fact_signature(...)`. \
         Window:\n{cwindow}"
    );

    // Behavioural assertion: editing a referenced dep bumps a fact
    // version, the warm-hit validator catches it, and the cold-compute
    // path runs again — advancing
    // `component_meta_resolved_state_recomputes`. Asserting only that
    // both calls return `Some` would NOT prove the validator caught
    // the invalidation: a stale warm hit would also return `Some` (with
    // the same shape pre-edit). The recomputes-counter delta is the
    // discriminating signal.
    let mh = metahost();
    mh.upsert_base("/src/types.ts", "export interface Foo { a: number; }\n")
        .expect("ts upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    )
    .expect("vue upsert");

    let m1 = mh.host().get_component_meta("/src/Comp.vue");
    assert!(m1.is_some(), "first call should return a meta");

    let prov = mh.host().provenance();
    let recomputes_before = prov.component_meta_resolved_state_recomputes.load(Relaxed);

    // Edit the referenced type — bumps a fact version.
    mh.upsert_base("/src/types.ts", "export interface Foo { a: string; }\n")
        .expect("ts re-upsert");

    let m2 = mh.host().get_component_meta("/src/Comp.vue");
    assert!(m2.is_some(), "second call after edit must still resolve");

    let recomputes_after = prov.component_meta_resolved_state_recomputes.load(Relaxed);

    assert!(
        recomputes_after > recomputes_before,
        "editing a referenced dep MUST bypass the warm \
         resolved-meta cache. `component_meta_resolved_state_recomputes` \
         did not advance from {recomputes_before} to {recomputes_after}, \
         which means the validator did not catch the version bump and a \
         stale warm hit served the second call."
    );
}
