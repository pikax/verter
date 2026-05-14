//! Block 1A — `DerivedRawState.cached_meta_payload` fact-validation
//! NEGATIVE discriminator.
//!
//! Pre-1A: with `fact_versions: Vec<FactVersionRef>` and no
//! signature-precise fast-path, eager invalidation in `host_upsert`
//! could conservatively drop the payload cache on ANY edit, so this
//! test would observe an unexpected miss after editing an unrelated
//! file.
//!
//! Post-1A: `StoreView::validates_fact_signature` walks the
//! Arc-stored signature; an unrelated edit that does not appear in
//! the consumer's signature leaves the warm hit intact and the
//! `payload_cache_misses` counter does NOT advance on the next call.

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
#[ignore = "block-1.a RED — closed by same-block implementation"]
fn unrelated_edit_keeps_cached_meta_payload_warm() {
    let mh = metahost();
    mh.upsert_base("/src/types.ts", "export interface Foo { a: number; }\n")
        .expect("types upsert");
    mh.upsert_base(
        "/src/other.ts",
        "export interface Other { x: number; }\n",
    )
    .expect("other upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    )
    .expect("vue upsert");

    let session = mh.open_session().expect("session opens");

    // Prime — cold-encode.
    let _ = session
        .get_component_meta_payload("/src/Comp.vue", |_, _| b"prime".to_vec())
        .expect("prime payload");
    let prov = mh.host().provenance();
    let hits_before = prov.payload_cache_hits.load(Relaxed);
    let misses_before = prov.payload_cache_misses.load(Relaxed);

    // Edit unrelated file; Comp.vue does not import it.
    mh.upsert_base(
        "/src/other.ts",
        "export interface Other { x: number; y: number; }\n",
    )
    .expect("other re-upsert");

    let session2 = mh.open_session().expect("session2 opens");
    let _ = session2
        .get_component_meta_payload("/src/Comp.vue", |_, _| b"after-unrelated".to_vec())
        .expect("post-unrelated payload");

    let hits_after = prov.payload_cache_hits.load(Relaxed);
    let misses_after = prov.payload_cache_misses.load(Relaxed);
    assert!(
        hits_after > hits_before,
        "Block 1A: editing an unrelated file (Comp.vue does NOT import other.ts) MUST \
         keep the cached_meta_payload warm hit alive. \
         hits_before={hits_before} hits_after={hits_after}"
    );
    assert_eq!(
        misses_after, misses_before,
        "Block 1A: misses must NOT advance after an unrelated edit. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
