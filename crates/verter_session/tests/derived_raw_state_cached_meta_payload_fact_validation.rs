//! Block 1A — `DerivedRawState.cached_meta_payload` fact-validation
//! discriminator (POSITIVE).
//!
//! Pre-1A: `CachedMetaPayload.fact_versions: Vec<FactVersionRef>` and
//! the consumer used `cached.fact_versions.iter().all(|fact|
//! view.validates(fact))`. The behavioural assertion below fails
//! because (a) the substrate cannot cheaply revalidate without per-
//! item iteration and (b) editing a referenced dep does NOT bump the
//! `payload_cache_misses` counter (the encoded-payload cache stays
//! warm despite a stale `Vec`).
//!
//! Post-1A: substrate uses `Arc<[FactVersionRef]>` and the consumer
//! short-circuits via `view.validates_fact_signature(...)`. Editing
//! a referenced dep causes the next `get_component_meta_payload`
//! call to miss; the behavioural delta passes.

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

/// Behavioural assertion: editing a referenced type triggers a miss
/// on the next payload encode. Pre-1A the manual
/// `.iter().all(view.validates(...))` predicate over a `Vec` would
/// still report cache-valid against the stored snapshot; post-1A the
/// `StoreView::validates_fact_signature` fast-path catches the
/// version bump and the payload-cache miss counter advances.
#[test]
#[ignore = "block-1.a RED — closed by same-block implementation"]
fn editing_dep_invalidates_cached_meta_payload() {
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

    let session = mh.open_session().expect("session opens");

    // Prime — first call cold-encodes.
    let first = session
        .get_component_meta_payload("/src/Comp.vue", |_, _| b"prime".to_vec())
        .expect("first payload");
    assert!(first.is_some(), "Comp.vue must resolve a component meta");
    let prov = mh.host().provenance();
    let hits_before = prov.payload_cache_hits.load(Relaxed);
    let misses_before = prov.payload_cache_misses.load(Relaxed);

    // Warm — repeat the same canonical with unchanged inputs.
    let _second = session
        .get_component_meta_payload("/src/Comp.vue", |_, _| b"warm".to_vec())
        .expect("second payload");
    let hits_after_warm = prov.payload_cache_hits.load(Relaxed);
    assert!(
        hits_after_warm > hits_before,
        "Block 1A substrate must allow the second call to hit the cached_meta_payload \
         warm cache when inputs are unchanged. hits_before={hits_before} \
         hits_after_warm={hits_after_warm}"
    );

    // Edit the referenced type body; the new whole-hash bumps the
    // observed fact set and the warm hit must invalidate.
    mh.upsert_base("/src/types.ts", "export interface Foo { a: string; }\n")
        .expect("ts re-upsert");

    let session2 = mh.open_session().expect("session2 opens");
    let _ = session2
        .get_component_meta_payload("/src/Comp.vue", |_, _| b"after-edit".to_vec())
        .expect("third payload");
    let misses_after_edit = prov.payload_cache_misses.load(Relaxed);
    assert!(
        misses_after_edit > misses_before,
        "Block 1A: editing a referenced type must invalidate the cached_meta_payload \
         warm hit (misses must advance). misses_before={misses_before} \
         misses_after_edit={misses_after_edit}"
    );
}
