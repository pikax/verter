//! Fact-version warm-invalidation oracle for the component-meta result
//! cache.
//!
//! These tests are the successor coverage for the retired
//! `expr_needs_projection_rescue` probe: rather than asserting an
//! implementation-internal probe fired, they assert the OBSERVABLE
//! cache contract that the projector pipeline must uphold —
//!
//! 1. a cold component-meta resolution records its cross-file CARRIER
//!    dep-signature facts onto the published `ComponentMetaResultDb`
//!    entry (facts flow), and
//! 2. editing a carrier the entry depends on INVALIDATES the warm
//!    result, so a re-resolution recomputes the changed shape rather
//!    than serving a stale warm hit (fact-version warm invalidation).
//!
//! Together they prove the dep-signature fan-in keeps the warm cache
//! correct without any reference to the eager-materialiser internals.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;

use crate::types::HostConfig;
use crate::{FileKind, UpsertRequest, VerterHost};

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }))
}

fn upsert(host: &VerterHost, id: &str, source: &str, kind: FileKind) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

fn prop_names(meta: &ComponentMetaAnalysis) -> Vec<String> {
    let mut names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    names.sort_unstable();
    names
}

/// Facts flow: a cold component-meta resolution of `defineProps<Props>()`
/// whose `Props` is imported cross-file records the carrier file's
/// whole-hash fact onto the published cache entry. Without the carrier in
/// the dep-signature a carrier edit could not invalidate the warm result.
///
/// Discrimination: an entry published WITHOUT the carrier dep-signature
/// (the fan-in failing to fold the carrier's dispatch facts) yields a
/// dep-signature missing `/types.ts`, failing the assertion below.
#[test]
fn cold_resolution_records_carrier_facts_into_cached_entry() {
    let host = build_host();
    upsert(
        &host,
        "/types.ts",
        "export interface Props { a: string; b: number }",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/Owner.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        FileKind::VueSfc,
    );

    let _ = host
        .get_component_meta("/Owner.vue")
        .expect("cold component meta resolves");

    let dep_canonicals = crate::component_meta_result_db::ComponentMetaResultDb::dep_signature_for_owner_in_test(
        &host,
        "/Owner.vue",
    );
    assert!(
        dep_canonicals.iter().any(|c| c.as_ref() == "/types.ts"),
        "the published component-meta entry's dep-signature MUST include the \
         cross-file carrier `/types.ts` (the dispatch fan-in folds the carrier's \
         whole-hash fact); observed {dep_canonicals:?}"
    );
}

/// Fact-version warm invalidation: after a cold resolution warms the
/// `ComponentMetaResultDb` entry for `defineProps<Props>()`, editing the
/// imported `Props` carrier must invalidate the warm result — the next
/// resolution recomputes the CHANGED prop set rather than serving the
/// stale warm hit.
///
/// Discrimination: a cache that ignores the recorded carrier fact-version
/// (no warm invalidation on carrier edit) serves the original `[a, b]`
/// props after the edit, failing the post-edit assertion. The
/// dep-signature validation against the live `StoreView` recomputes the
/// entry because the carrier's whole-hash changed.
#[test]
fn carrier_edit_invalidates_warm_component_meta_result() {
    let host = build_host();
    upsert(
        &host,
        "/types.ts",
        "export interface Props { a: string; b: number }",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/Owner.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        FileKind::VueSfc,
    );

    let first = host
        .get_component_meta("/Owner.vue")
        .expect("first component meta");
    assert_eq!(
        prop_names(&first),
        vec!["a".to_string(), "b".to_string()],
        "the original carrier publishes exactly [a, b]"
    );

    // A SECOND resolution without any edit must serve the SAME shape (warm
    // hit, validated against unchanged facts).
    let warm = host
        .get_component_meta("/Owner.vue")
        .expect("warm component meta");
    assert_eq!(
        prop_names(&warm),
        vec!["a".to_string(), "b".to_string()],
        "an unedited re-resolution serves the same [a, b] surface (warm hit)"
    );

    // Edit the carrier: drop `b`, rename `a` -> `renamed`, add `c`.
    upsert(
        &host,
        "/types.ts",
        "export interface Props { renamed: string; c: boolean }",
        FileKind::NonSfc,
    );

    let after = host
        .get_component_meta("/Owner.vue")
        .expect("post-edit component meta");
    assert_eq!(
        prop_names(&after),
        vec!["c".to_string(), "renamed".to_string()],
        "the carrier edit MUST invalidate the warm result — the recorded carrier \
         fact-version no longer validates, so the entry recomputes the changed \
         prop set [c, renamed] (a stale warm hit would still report [a, b]): {:?}",
        prop_names(&after)
    );
}
