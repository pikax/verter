//! Characterisation test for the cross-file dep-signature threading
//! invariant.
//!
//! Discrimination property:
//!
//! Each admit must thread the gate-observed dep facts (the cycle BFS
//! fence and the package-backed declaration scope) so the cache
//! entry's `fact_dep_signature` invalidates on cross-file edits. The
//! failure mode this guards against: `member_shape_peek_or_compute`
//! admits a `ShapeCacheEntry` via a gate-shortcut path (package-backed,
//! cycle, non-reducible) with `dep_signature: Arc::from(Vec::new())`,
//! which self-roots ONLY on the scope file's `whole_hash` — a
//! content edit to the IMPORTED helper file that the gates touched
//! during compute does NOT invalidate the cache entry.
//!
//! Setup: an owner component references an imported type. The type
//! resolution path goes through the projector's shallow gates and
//! admits a `ShapeCacheDb` entry with a cross-file dep on the
//! helper file. Edit the helper, re-query, and assert the cache
//! reports a miss (driven by `fact_dep_signature` invalidation) —
//! NOT a stale warm hit.
//!
//! Without the cross-file dep threading, this test FAILS: the empty
//! dep_signature self-roots the entry on owner.vue only, and the
//! helper-file edit does not invalidate. With it, the admit's
//! dep_signature carries the helper file's whole_hash, and the
//! helper edit triggers fact-validation failure on the next warm-read.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig, UpsertRequest};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

// The asserted provenance counter deltas
// (`component_meta_result_cache_misses` / `_cache_hits`) are read off
// THIS test's own `VerterHost`: `ComponentMetaHost::new_standalone`
// builds a fresh host with a fresh `Arc<MetaProvenance>` and an
// instance-local `MemoryWorkspace`. The deltas are host-local, so the
// test runs in parallel with no shared-process serialization.
#[test]
fn admit_threads_cross_file_dep_signature_for_imported_helper() {
    let mh = metahost();

    // Setup: a helper file with a type that the owner references. The
    // helper type is a generic operator shape (`Pick`) so the projector
    // runs its shallow gates (cycle / package-backed / reducible-
    // operator) during compute — those gates touch the helper file via
    // `resolve_type_declaration` + body lookup.
    mh.upsert_base(
        "/src/helper.ts",
        "export interface Big {\n\
         \ta: number;\n\
         \tb: string;\n\
         }\n\
         export type Picked = Pick<Big, 'a'>;\n",
    )
    .expect("helper.ts upsert");

    mh.upsert_base(
        "/src/Owner.vue",
        "<script setup lang=\"ts\">\n\
         import type { Picked } from './helper';\n\
         defineProps<Picked>();\n\
         </script>\n",
    )
    .expect("Owner.vue upsert");

    // Prime — cold compute publishes the shape and admits to the
    // shape cache. The admit's `fact_dep_signature` includes the
    // helper file's whole_hash.
    let prime = mh.host().get_component_meta("/src/Owner.vue");
    assert!(prime.is_some(), "prime call must resolve");

    let prov = mh.host().provenance();

    // Sanity: a second identical call hits the warm cache.
    let warm_hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let _ = mh.host().get_component_meta("/src/Owner.vue");
    let warm_hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
    assert!(
        warm_hits_after > warm_hits_before,
        "sanity: warm path must hit before the helper edit \
         (hits {warm_hits_before} -> {warm_hits_after})"
    );

    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Now EDIT the helper file: change `Pick<Big, 'a'>` to
    // `Pick<Big, 'b'>`. Owner.vue is NOT touched. Without cross-file
    // dep threading, the shape cache entry self-roots only on Owner.vue
    // and survives this edit silently — the next call would publish the
    // STALE 'a'-keyed shape. With it, the entry's fact_dep_signature
    // carries helper.ts's whole_hash; the edit invalidates.
    let req = UpsertRequest {
        canonical_id: Some("/src/helper.ts".to_string()),
        input_id: "/src/helper.ts".to_string(),
        source: std::sync::Arc::from(
            "export interface Big {\n\
             \ta: number;\n\
             \tb: string;\n\
             }\n\
             export type Picked = Pick<Big, 'b'>;\n",
        ),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/src/helper.ts")
            .static_resolution(),
        aliases: Vec::new(),
    };
    let _ = mh.host().upsert(req).expect("helper.ts re-upsert");

    // Next query on the owner: the warm component-meta result will
    // miss (its fact_dep_signature includes helper.ts whose facts
    // changed). This is the existing ComponentMetaResultDb
    // invalidation path. The discriminating assertion below is on
    // the published shape: the new published surface must reflect
    // `b` not `a`.
    let post_edit = mh.host().get_component_meta("/src/Owner.vue");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);

    assert!(
        misses_after > misses_before,
        "after a helper-file edit the owner's warm hit MUST miss \
         via fact-validation (misses_before={misses_before} \
         misses_after={misses_after})"
    );

    // Bonus: the published shape must reflect the helper's new content
    // (props keyed on 'b', not 'a').
    let meta = post_edit.expect("post-edit query must resolve");
    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names.contains(&"b".to_string()),
        "post-edit published surface must reflect helper's new \
         Pick<Big, 'b'> — actual prop names: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"a".to_string()),
        "post-edit published surface must NOT carry the stale \
         'a' prop — actual prop names: {prop_names:?}"
    );
}
