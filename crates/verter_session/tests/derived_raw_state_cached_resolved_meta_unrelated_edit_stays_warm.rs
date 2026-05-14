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
fn unrelated_edit_keeps_cached_resolved_meta_warm() {
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

    // Prime — drive the resolver through `VerterHost::get_component_meta`
    // (the `ComponentMetaHost` inherent + trait method conflict makes
    // dispatch ambiguous; using the underlying host accessor is
    // unambiguous and exercises the same code path).
    let m1 = mh
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("Comp meta exists");

    // Capture the cold-compute counter AFTER the prime call so we can
    // observe in isolation whether the post-edit `get_component_meta`
    // had to recompute the resolved-meta state.
    // `component_meta_resolved_state_recomputes` increments inside
    // `compute_component_meta_state` (host_manage/component_meta_methods.rs:597-599),
    // i.e. on every cold compute. Warm-hit paths bypass that
    // function via `try_get_cached_resolved_meta`'s
    // `validates_fact_signature` short-circuit and therefore do NOT
    // advance the counter.
    let prov = mh.host().provenance();
    let recomputes_before = prov.component_meta_resolved_state_recomputes.load(Relaxed);

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

    let recomputes_after = prov.component_meta_resolved_state_recomputes.load(Relaxed);

    // Discriminator (per codex P2): comparing `accepted_props.len()`
    // would NOT prove the cached resolved-meta entry stayed warm — a
    // regression that eager-invalidates and cold-recomputes the
    // resolved-meta state would still return the same prop count and
    // keep this test green. The recomputes counter is the only
    // observable that distinguishes "served from warm cache" from
    // "cold-recomputed but produced the same shape".
    assert_eq!(
        recomputes_after, recomputes_before,
        "Block 1A: unrelated edit must NOT invalidate the cached_resolved_meta entry. \
         `component_meta_resolved_state_recomputes` advanced from {recomputes_before} \
         to {recomputes_after}, signalling that the cold-compute path ran despite \
         the edit not touching any dep observed by Comp.vue's fact signature."
    );

    // The accepted_props lens still match across the two calls — this
    // is a sanity assertion, not the discriminating one.
    assert_eq!(
        m1.accepted_props.len(),
        m2.accepted_props.len(),
        "Sanity: unrelated edit must not change the published prop set. \
         before={} after={}",
        m1.accepted_props.len(),
        m2.accepted_props.len()
    );
}
