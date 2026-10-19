//! The template-class consumer keeps every construction it reads alive for
//! as long as it reads it. Under counted ownership with zero retention
//! headroom nothing is cached, so a dispatcher result is owned only by the
//! carrier it is delivered in: a classification that dropped the carrier and
//! kept reading its label would read released nodes and publish an
//! unresolved row for a closed literal domain.

use std::sync::Arc;

use super::*;
use crate::types::UpsertRequest;
use crate::{HostConfig, VerterHost};
use verter_language::FileLanguage;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};
use verter_type_engine::semantic_query_memo::graph_fixture::GraphFixture;

fn zero_headroom_harness() -> GraphFixture {
    GraphFixture::with_account(StoreAccount::new(SemanticRetentionAccount::new(
        RetentionLimits {
            aggregate_ceiling_bytes: 0,
            ..RetentionLimits::defaults()
        },
    )))
}

/// The class rows of `src`'s template, classified by a dispatcher over a
/// counted graph with zero retention headroom and no builder scope open.
fn counted_rows(src: &str) -> Vec<ClosedLiteralDomain> {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/Counted.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert")
        .canonical_id;
    let source = host
        .scheduler
        .try_get_source(&canonical)
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
    .expect("raw template data")
    .data;
    let base = host.resolver_store_view_read().into_cold_seed_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_cold_seed(&host, &base, overlay);
    let dispatch = ProjectSemanticDispatch::with_graph_fixture(&ctx, zero_headroom_harness());
    let (_, rows, _) = classify_requested_subjects(
        &dispatch,
        &canonical,
        TemplateClassScriptInputs {
            macros: &data.parse.script_analysis.macros,
            bindings: &data.parse.script_analysis.bindings,
        },
        &raw,
    );
    rows.iter().map(|row| row.domain.clone()).collect()
}

fn strings(values: &[&str]) -> ClosedLiteralDomain {
    ClosedLiteralDomain::Strings(values.iter().map(|value| Arc::from(*value)).collect())
}

#[test]
fn counted_class_classification_reads_only_constructions_it_holds() {
    let cases = [
        (
            "prop typed by an alias of a closed literal union",
            "<script setup lang=\"ts\">type Kind = 'a' | 'b';\
             defineProps<{ kind: Kind }>();</script>\
             <template><div :class=\"kind\" /></template>",
            strings(&["a", "b"]),
        ),
        (
            "binding typed by an alias of an aliased closed literal union",
            "<script setup lang=\"ts\">type Base = 'primary' | 'secondary';\
             type Variant = Base; const variant: Variant = 'primary';</script>\
             <template><div :class=\"variant\" /></template>",
            strings(&["primary", "secondary"]),
        ),
    ];
    for (label, src, expected) in cases {
        assert_eq!(counted_rows(src), vec![expected], "{label}");
    }
}
