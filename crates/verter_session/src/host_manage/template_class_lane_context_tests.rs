//! Hermetic regression for the template-class lane's resolver-context binding.
//!
//! The lane (`VerterHost::build_template_class_semantic_facts`) chooses its
//! resolver context from CACHE PRESENCE: when the owner's `IndexedReady` is
//! already published at the requested whole hash and the publication scope is
//! base-publishable it takes one branch, otherwise it composes a cold-seed
//! session view. Both branches must bind a request-bound context.
//!
//! It is not survivable at runtime: the builder's `classify_binding` demands
//! `ResolverContext::prepared_value_decl` for every template `:class` subject
//! that is a script binding, and only a request-bound context can serve a
//! prepared declaration. The sealed builder signature rejects a
//! non-request-bound context.
//!
//! The fixture is the minimal shape of that demand — a locally-vendored SFC
//! whose template binds `:class` to a `computed` script binding — driven so the
//! owner's `IndexedReady` is already present when the lane runs, and asserted
//! on the recorded lane binding rather than on the returned facts.

use std::sync::Arc;

use super::take_template_class_lane_bindings;
use crate::types::UpsertRequest;
use crate::{HostConfig, VerterHost};
use verter_language::FileLanguage;

/// Minimal vendored reproduction of the corpus `Avatar.vue` shape: a
/// `<script setup>` binding consumed by a template `:class`, which is what
/// makes the lane demand a prepared value declaration.
const CLASS_BOUND_SFC: &str = r#"<template>
  <span :class="rootClass">x</span>
</template>
<script setup lang="ts">
import { computed } from 'vue'
const rootClass = computed(() => 'a b')
</script>
"#;

fn upsert_vue(host: &VerterHost, id: &str, src: &str) -> String {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .expect("upsert")
    .canonical_id
}

fn template_class_facts_for(host: &VerterHost, canonical: &str) {
    let source = host
        .scheduler
        .try_get_source(canonical)
        .expect("source snapshot");
    let data = source
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("host source data");
    let raw = crate::parse::compile_template_data(
        &data.file_language,
        source.source.as_ref(),
        data.framework_parse.as_deref(),
        true,
        &host.provenance,
    )
    .expect("raw template data");
    let _ = host.build_template_class_semantic_facts(
        canonical,
        data.parse.whole_hash,
        Arc::clone(&source.source),
        crate::project_semantic_dispatch::template_class_facts::TemplateClassScriptInputs {
            macros: &data.parse.script_analysis.macros,
            bindings: &data.parse.script_analysis.bindings,
        },
        &raw,
        crate::project_semantic_dispatch::template_class_facts::TemplateClassPublicationScope::BasePublishable,
    );
}

/// The indexed-present branch must bind a REQUEST-BOUND resolver context.
///
/// With the owner's `IndexedReady` already published, the base branch must
/// preserve the request-bound prepared-declaration authority.
#[test]
fn indexed_present_template_class_lane_binds_a_request_bound_context() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = upsert_vue(&host, "/Comp.vue", CLASS_BOUND_SFC);

    // Publish the owner's `IndexedReady` so the lane's cache-presence probe
    // takes the indexed-present branch. Without this the lane composes the
    // cold-seed session context and the defect is not reached at all.
    assert!(
        host.ensure_indexed_ready_serve(&canonical).is_some(),
        "fixture precondition: the owner must have a published IndexedReady"
    );

    let _ = take_template_class_lane_bindings();
    template_class_facts_for(&host, &canonical);
    let bindings = take_template_class_lane_bindings();

    // Anti-vacuity: the lane must actually have run, and must have run through
    // the indexed-present branch. A fixture that silently stopped reaching the
    // branch would otherwise pass by observing nothing.
    assert_eq!(
        bindings.len(),
        1,
        "expected exactly one template-class lane binding, got {bindings:?}"
    );
    assert!(
        bindings[0].indexed_present,
        "fixture precondition: the lane must take the indexed-present branch, got {bindings:?}"
    );
    assert!(
        bindings[0].request_bound,
        "the indexed-present template-class lane bound a NON-request-bound resolver context. \
         The resolver-tier builder demands prepared declarations (classify_binding -> \
         ResolverContext::prepared_value_decl), which only a request-bound context can serve."
    );
}

/// Control: the cold-seed branch must stay request-bound too. This proves the
/// assertion discriminates a branch rather than accepting any lane binding.
#[test]
fn cold_seed_template_class_lane_binds_a_request_bound_context() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = upsert_vue(&host, "/Comp.vue", CLASS_BOUND_SFC);

    let _ = take_template_class_lane_bindings();
    // No `ensure_indexed_ready_serve` — the owner's artifact is absent, so the
    // cache-presence probe falls through to the cold-seed session branch.
    template_class_facts_for(&host, &canonical);
    let bindings = take_template_class_lane_bindings();

    assert_eq!(
        bindings.len(),
        1,
        "expected exactly one template-class lane binding, got {bindings:?}"
    );
    assert!(
        !bindings[0].indexed_present,
        "control precondition: the lane must take the cold-seed branch, got {bindings:?}"
    );
    assert!(
        bindings[0].request_bound,
        "the cold-seed template-class lane bound a NON-request-bound resolver context"
    );
}
