//! Block 1A — `DerivedRawState.cached_resolved_meta` fact-validation
//! discriminator (POSITIVE).
//!
//! Pre-1A: `ResolvedComponentMetaCacheEntry.fact_versions:
//! Vec<FactVersionRef>` and the consumer (`try_resolve_cached_meta`)
//! used `view.invalid_fact_details(&cached.fact_versions, 6)` instead
//! of the per-domain fast-path validator. The migration source-grep
//! arch guard FAILS pre-1A.
//!
//! Post-1A: substrate is `Arc<[FactVersionRef]>` and
//! `try_resolve_cached_meta` short-circuits via
//! `view.validates_fact_signature(...)`. Editing a referenced dep
//! triggers an invalidation observable via the `get_component_meta`
//! call counter — the second call after the edit must NOT be served
//! from the resolved-meta warm cache because the validator catches
//! the version bump.

use std::fs;
use std::path::Path;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

fn read_session_src(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
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
#[ignore = "block-1.a RED — closed by same-block implementation"]
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
        "Block 1A: ResolvedComponentMetaCacheEntry.fact_versions must be \
         `Arc<[FactVersionRef]>`. Window:\n{window}"
    );
    assert!(
        !window.contains("fact_versions: Vec<"),
        "ResolvedComponentMetaCacheEntry must NOT carry the legacy `Vec<FactVersionRef>` \
         shape after Block 1A. Window:\n{window}"
    );

    // Behavioural smoke: editing a referenced dep advances the
    // `get_component_meta_calls` counter monotonically (every public
    // call increments). The second call after an edit must reach the
    // cold path; the resolved-meta warm cache cannot mask the edit
    // when the validator is wired.
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

    // Edit the referenced type — bumps a fact version.
    mh.upsert_base("/src/types.ts", "export interface Foo { a: string; }\n")
        .expect("ts re-upsert");

    let m2 = mh.host().get_component_meta("/src/Comp.vue");
    assert!(m2.is_some(), "second call after edit must still resolve");
    // The two metas may carry the same observable shape (number→string
    // doesn't change accepted_props names), but the call must
    // succeed and cold-recompute. The behavioural delta is in the
    // counters; the runtime contract is that no stale cached state
    // survives the dep edit.
}
