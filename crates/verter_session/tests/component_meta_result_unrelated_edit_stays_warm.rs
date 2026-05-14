//! Block 1.B — `ComponentMetaResultDb` fact-validation NEGATIVE
//! discriminator: an UNRELATED file edit must NOT invalidate the warm
//! hit.
//!
//! Pre-1.B: with only the legacy `dep_signature` whole-hash oracle,
//! the cache stayed warm only because the eager-invalidation
//! cascade in `host_upsert::upsert` cleared cross-file dependents.
//! After Block 4 retires eager invalidation, fact-validation is the
//! sole correctness oracle — and that oracle must keep unrelated
//! edits OUT of the validation set so warm hits survive non-dep
//! activity.
//!
//! Post-1.B: editing an UNRELATED file (no edge from the owner)
//! leaves the warm `fact_dep_signature` intact. The
//! `component_meta_result_cache_hits` counter advances; the
//! `component_meta_result_cache_misses` counter does NOT.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

#[test]
#[ignore = "block-1.b RED — closed by same-block implementation"]
fn unrelated_edit_keeps_component_meta_result_warm() {
    let mh = metahost();
    mh.upsert_base("/src/types.ts", "export interface Foo { a: number; }\n")
        .expect("types upsert");
    mh.upsert_base("/src/other.ts", "export interface Other { x: number; }\n")
        .expect("other upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    )
    .expect("vue upsert");

    // Prime — cold compute populates the cache entry.
    let _ = mh.host().get_component_meta("/src/Comp.vue");

    let prov = mh.host().provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Edit an unrelated file the owner does NOT import. Note: the
    // upsert pipeline still triggers the eager-invalidation
    // cascade today (Block 4 retires it). The behavioural assertion
    // here is whether the warm hit survives via fact-validation —
    // i.e. the captured signature does not include this unrelated
    // file's whole-hash, so the validator continues to accept the
    // entry. Once eager invalidation lands behind a
    // `register_facts_for_new_content_without_eviction` hook (see
    // `component_meta_result_eager_invalidation_defeating.rs`),
    // this assertion holds in isolation.
    mh.upsert_base(
        "/src/other.ts",
        "export interface Other { x: number; y: number; }\n",
    )
    .expect("other re-upsert");

    let _ = mh.host().get_component_meta("/src/Comp.vue");

    let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        hits_after > hits_before,
        "Block 1.B: editing an unrelated file (Comp.vue does NOT import \
         other.ts) MUST keep the ComponentMetaResultDb warm hit alive. \
         hits_before={hits_before} hits_after={hits_after}"
    );
    assert_eq!(
        misses_after, misses_before,
        "Block 1.B: misses must NOT advance after an unrelated edit. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
