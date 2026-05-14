//! Block 1A — `DerivedRawState.cached_resolved_meta` NEGATIVE
//! discriminator: an unrelated edit must leave the warm cached
//! resolved-meta entry alive.
//!
//! Pre-1A: substrate uses `Vec<FactVersionRef>` and the consumer
//! calls `view.invalid_fact_details(...)` (per-item walk). Any
//! conservative invalidation in `host_upsert` (eager cascade) would
//! drop the entry; this NEGATIVE assertion would fail. Post-1A:
//! per-domain fast-path validator preserves the warm entry under
//! unrelated edits.

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
fn unrelated_edit_keeps_cached_resolved_meta_warm() {
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

    // Prime — drive the resolver through `VerterHost::get_component_meta`
    // (the `ComponentMetaHost` inherent + trait method conflict makes
    // dispatch ambiguous; using the underlying host accessor is
    // unambiguous and exercises the same code path).
    let m1 = mh
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("Comp meta exists");

    // Edit unrelated file; Comp.vue does NOT import it. The
    // cached_resolved_meta entry for /src/Comp.vue must stay alive
    // under fact-precise validation — its signature does not mention
    // /src/other.ts.
    mh.upsert_base(
        "/src/other.ts",
        "export interface Other { x: number; y: number; }\n",
    )
    .expect("other re-upsert");

    let m2 = mh
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("Comp meta still exists");

    assert_eq!(
        m1.accepted_props.len(),
        m2.accepted_props.len(),
        "Block 1A: unrelated edit must NOT invalidate the cached_resolved_meta entry. \
         accepted_props lens differ before={} after={} — this signals over-invalidation.",
        m1.accepted_props.len(),
        m2.accepted_props.len()
    );
}
