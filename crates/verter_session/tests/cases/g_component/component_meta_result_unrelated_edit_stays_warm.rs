//! `ComponentMetaResultDb` fact-validation NEGATIVE discriminator: an
//! UNRELATED file edit must NOT invalidate the warm hit.
//!
//! The owner-upsert path has no eager reverse-dependent invalidation
//! cascade — a file edit never physically clears a downstream owner's
//! warm result. Fact-validation is the sole correctness oracle, and
//! that oracle must keep unrelated edits OUT of the validation set so
//! warm hits survive non-dep activity.
//!
//! Editing an UNRELATED file (no edge from the owner) leaves the warm
//! `fact_dep_signature` intact. The `component_meta_result_cache_hits`
//! counter advances; the `component_meta_result_cache_misses` counter
//! does NOT.

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

    // Edit an unrelated file the owner does NOT import. The
    // owner-upsert path has no eager reverse-dependent cascade — a
    // dependency edit never physically clears a downstream owner's
    // warm result. The behavioural assertion here is that the warm
    // hit survives via fact-validation: the captured signature does
    // not include this unrelated file's whole-hash, so the validator
    // continues to accept the entry.
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
        "editing an unrelated file (Comp.vue does NOT import \
         other.ts) MUST keep the ComponentMetaResultDb warm hit alive. \
         hits_before={hits_before} hits_after={hits_after}"
    );
    assert_eq!(
        misses_after, misses_before,
        "misses must NOT advance after an unrelated edit. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
