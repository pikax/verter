//! `ComponentMetaResultDb` fact-validation POSITIVE discriminator.
//!
//! A coarse design that carried only a `dep_signature: DepSignature`
//! whole-hash oracle would revalidate warm hits only on owner
//! whole-hash + transitive whole-hashes: editing a referenced dep whose
//! whole-hash bumped would invalidate the warm hit, but no fact-precise
//! per-domain validator discriminator would exist at this cache layer.
//! This test cannot compile without `fact_dep_signature` on
//! `ComponentMetaResultEntry`.
//!
//! The entry carries `fact_dep_signature:
//! Arc<[FactVersionRef]>` populated from the cold resolver's curated
//! observation set, and `ComponentMetaResultDb::get_with_view`
//! validates the signature through
//! `StoreView::validates_fact_signature` before returning the entry.
//! Editing a referenced type forces the validator to reject the
//! stored signature; the new
//! `component_meta_result_cache_misses` provenance counter advances
//! on the second call, discriminating cache-bypass via the validator
//! from any other miss path.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::component_meta_result_db::ComponentMetaResultEntry;
use verter_session::resolver_core::FactVersionRef;
use verter_session::{CompileErrorPolicy, HostConfig};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

#[test]

fn editing_dep_invalidates_component_meta_result_warm_hit() {
    // Structural arch guard: ensure the type carries the
    // `read_set_signature` carrier and the carrier exposes the
    // `facts: Arc<[FactVersionRef]>` rail.
    fn _assert_field_present<P>(entry: &ComponentMetaResultEntry<P>) -> &[FactVersionRef] {
        &entry.read_set_signature.facts
    }

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

    // Prime — cold compute populates the cache entry, the entry's
    // `fact_dep_signature` records the observed cross-file facts.
    let m1 = mh.host().get_component_meta("/src/Comp.vue");
    assert!(
        m1.is_some(),
        "first call must return a component-meta result"
    );

    let prov = mh.host().provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Warm sanity: an identical second call (no edits) must
    // round-trip the cache (hits advance, misses do not). This is the
    // baseline before the dep edit below; if this advances misses
    // instead of hits, the discriminator is broken.
    let _warm = mh.host().get_component_meta("/src/Comp.vue");
    let hits_after_warm = prov.component_meta_result_cache_hits.load(Relaxed);
    assert!(
        hits_after_warm > hits_before,
        "identical second call must hit the warm `ComponentMetaResultDb` cache. \
         hits_before={hits_before} hits_after_warm={hits_after_warm}"
    );

    // Edit the referenced type — the new content shifts the
    // `Foo` member-body fact under R28, so the entry's
    // `fact_dep_signature` will no longer validate under the live
    // view.
    mh.upsert_base("/src/types.ts", "export interface Foo { a: string; }\n")
        .expect("ts re-upsert");

    let m2 = mh.host().get_component_meta("/src/Comp.vue");
    assert!(m2.is_some(), "second call after edit must still resolve");

    let misses_after_edit = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        misses_after_edit > misses_before,
        "editing a referenced type MUST advance \
         `component_meta_result_cache_misses` — the validator caught \
         the dep version bump and the cache returned None. \
         misses_before={misses_before} misses_after_edit={misses_after_edit}"
    );
}
