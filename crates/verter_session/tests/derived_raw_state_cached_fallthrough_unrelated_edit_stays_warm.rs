//! Block 1A — `DerivedRawState.cached_fallthrough` NEGATIVE
//! discriminator: an unrelated edit must leave the warm cached
//! fallthrough entry alive.
//!
//! Pre-1A: substrate uses `Vec<FactVersionRef>` and the consumer
//! relies on a manual `.iter().all(view.validates(...))` pass over
//! the entire Vec. Conservative invalidation in `host_upsert`'s
//! eager cascade would drop the entry on unrelated edits; this
//! NEGATIVE assertion would fail. Post-1A: per-domain fast-path
//! validator preserves the warm entry under unrelated edits.

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
fn unrelated_edit_keeps_cached_fallthrough_warm() {
    let mh = metahost();
    mh.upsert_base(
        "/src/types.ts",
        "export interface RootProps { foo: number }\n",
    )
    .expect("types upsert");
    mh.upsert_base(
        "/src/other.ts",
        "export interface Other { bar: number }\n",
    )
    .expect("other upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { RootProps } from './types';\n\
         defineProps<RootProps>();\n\
         </script>\n\
         <template><div /></template>\n",
    )
    .expect("vue upsert");

    // Prime — first resolve warms cached_fallthrough.
    let r1 = mh.host().resolve_fallthrough_surface("/src/Comp.vue");
    assert!(r1.is_some(), "prime resolve should succeed");

    // Edit unrelated file; Comp.vue does NOT import it. The warm
    // cached_fallthrough entry's fact_versions does not reference
    // /src/other.ts, so the per-domain fast-path validator must NOT
    // invalidate it.
    mh.upsert_base(
        "/src/other.ts",
        "export interface Other { bar: string }\n",
    )
    .expect("other re-upsert");

    let r2 = mh.host().resolve_fallthrough_surface("/src/Comp.vue");
    assert!(
        r2.is_some(),
        "Block 1A: unrelated edit must NOT prevent fallthrough resolution from \
         succeeding for /src/Comp.vue"
    );
}
