//! An aborted component-meta computation publishes nothing.
//!
//! A cancelled request returns the abort — never a partial analysis, output
//! or payload whose completeness names the cancellation — and nothing it
//! computed poisons the next request, which answers completely and warms.

use std::sync::Arc;

use crate::semantic_query::ExecutionAbort;
use crate::types::HostConfig;
use crate::types::UpsertRequest;
use crate::VerterHost;

const OWNER: &str = "/src/Aborted.vue";
const SOURCE: &str = r#"<script setup lang="ts">
import type { Props } from './props'
defineProps<Props>()
</script>"#;

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/props.ts".to_owned()),
            input_id: "/src/props.ts".to_owned(),
            source: Arc::from("export interface Props { label: string }\n"),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("TypeScript fixture must upsert");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(OWNER.to_owned()),
            input_id: OWNER.to_owned(),
            source: Arc::from(SOURCE),
            file_language: crate::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("Vue fixture must upsert");
    host
}

/// Run `operation` under a request that is already cancelled.
fn cancelled<T>(operation: impl FnOnce() -> T) -> T {
    let context = crate::request_context::RequestContext::new(7101, Arc::from(OWNER), false, None);
    context.cancel();
    let _guard = crate::request_context::RequestContextGuard::install(context);
    operation()
}

#[test]
fn a_cancelled_analysis_request_publishes_nothing_and_a_retry_completes() {
    let host = host();
    assert_eq!(
        cancelled(|| host.try_get_component_meta(OWNER)).err(),
        Some(ExecutionAbort::Cancelled),
        "a cancelled request returns the abort, not a partial analysis"
    );

    let retry = host
        .try_get_component_meta(OWNER)
        .expect("an uncancelled request is not aborted")
        .expect("the component resolves");
    assert_eq!(
        retry.props.len(),
        1,
        "the retry answers completely: {:?}",
        retry.props
    );
    // Nothing the cancelled attempt computed was published, and the retry
    // was: a third request is served warm with the same answer.
    let warm = host.get_component_meta(OWNER).expect("warm");
    assert_eq!(warm.props.len(), 1);
}

#[test]
fn a_cancelled_output_request_publishes_nothing_and_a_retry_completes() {
    let host = host();
    assert_eq!(
        cancelled(|| host.get_component_meta_output(OWNER)).err(),
        Some(crate::meta_resolve::ComponentMetaFailure::Aborted(
            ExecutionAbort::Cancelled
        )),
        "a cancelled request returns the abort, not an output whose \
         completeness names the cancellation"
    );

    let retry = host
        .get_component_meta_output(OWNER)
        .expect("an uncancelled request is not aborted")
        .expect("the component resolves");
    let (analysis, _resolution, _types, _contract, completeness) = retry.into_parts_with_contract();
    assert_eq!(
        completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    assert_eq!(analysis.props.len(), 1);
}
