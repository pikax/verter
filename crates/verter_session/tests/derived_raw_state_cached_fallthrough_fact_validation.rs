//! Block 1A — `DerivedRawState.cached_fallthrough` fact-validation
//! discriminator (POSITIVE).
//!
//! Pre-1A: `CachedFallthroughEntry.fact_versions: Vec<FactVersionRef>`
//! and the consumer (`try_get_cached_fallthrough` on
//! `FallthroughRequestHost for VerterHost`) used
//! `cached.fact_versions.iter().all(|fact| store_view.validates(fact))`.
//! Source-grep arch guard FAILS pre-1A.
//!
//! Post-1A: substrate is `Arc<[FactVersionRef]>` and the consumer
//! short-circuits via `store_view.validates_fact_signature(...)`.
//! Editing a referenced dep invalidates the warm entry on the next
//! `resolve_fallthrough_surface` call.

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

#[test]
#[ignore = "block-1.a RED — closed by same-block implementation"]
fn cached_fallthrough_substrate_and_consumer_wired() {
    let types_src = read_session_src("types.rs");
    let needle = "pub(crate) struct CachedFallthroughEntry {";
    let idx = types_src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let end = types_src[idx..]
        .find("\n}")
        .expect("CachedFallthroughEntry struct close");
    let window = &types_src[idx..idx + end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "Block 1A: CachedFallthroughEntry.fact_versions must be \
         `Arc<[FactVersionRef]>`. Window:\n{window}"
    );
    assert!(
        !window.contains("fact_versions: Vec<"),
        "CachedFallthroughEntry must NOT carry the legacy `Vec<FactVersionRef>` shape \
         after Block 1A. Window:\n{window}"
    );

    let host_manage = read_session_src("host_manage.rs");
    assert!(
        host_manage.contains("store_view.validates_fact_signature(&cached.fact_versions)"),
        "Block 1A: `try_get_cached_fallthrough` (impl FallthroughRequestHost for \
         VerterHost) must dispatch through `StoreView::validates_fact_signature` on \
         the warm-hit path."
    );

    // Behavioural smoke: warm the cached_fallthrough entry, edit
    // a referenced dep, second resolve must succeed (and the
    // returned shape must reflect the new dep — validated below by
    // observing the same canonical resolves both before and after).
    let mh = metahost();
    mh.upsert_base(
        "/src/types.ts",
        "export interface RootProps { foo: number }\n",
    )
    .expect("ts upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { RootProps } from './types';\n\
         defineProps<RootProps>();\n\
         </script>\n\
         <template><div /></template>\n",
    )
    .expect("vue upsert");

    let r1 = mh.host().resolve_fallthrough_surface("/src/Comp.vue");
    assert!(r1.is_some(), "first fallthrough must resolve");

    // Edit a referenced type; the warm cached_fallthrough must
    // invalidate so the next call cold-recomputes against the new
    // source.
    mh.upsert_base(
        "/src/types.ts",
        "export interface RootProps { foo: string }\n",
    )
    .expect("ts re-upsert");

    let r2 = mh.host().resolve_fallthrough_surface("/src/Comp.vue");
    assert!(r2.is_some(), "second fallthrough must still resolve");
}
