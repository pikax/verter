use std::sync::Arc;

use super::*;
use verter_session::{FileLanguage, HostConfig, PublicApiMode, UpsertRequest, VerterHost};

#[test]
fn child_hover_preserves_projection_failure_on_jsonrpc_transport() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/UnsafeEnum.vue".to_string()),
            input_id: "/src/UnsafeEnum.vue".to_string(),
            source: Arc::from(
                r#"<script setup lang="ts">
enum Unsafe { Value = Math.random() }
defineProps<{ value: Unsafe }>()
</script>"#,
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert unsafe enum");
    let projection_error = host
        .get_public_api_with_mode("/src/UnsafeEnum.vue", PublicApiMode::Declaration, None)
        .expect_err("unsafe enum projection");

    let error = transport_child_hover_result("/src/UnsafeEnum.vue", Err(projection_error))
        .expect_err("hover must preserve projection failure");

    assert_eq!(error.message, "hover: public API projection failed");
    assert_eq!(
        error.data,
        Some(serde_json::json!({
            "code": "tsc-generation",
            "detailCode": "unsupported-declaration-shape",
            "subject": { "kind": "macro", "syntaxIndex": 0 },
            "declarationShapeReason": "unsupported-enum-shape",
            "memberOrdinal": null,
            "outcomeKind": null,
            "outcomeReason": null,
            "outcomeDiagnostic": null,
        }))
    );
}

#[test]
fn child_hover_transport_preserves_typed_unsupported_and_authoritative_no_match() {
    use crate::server::component_resolve::ChildHoverOutcome;
    use verter_session::framework::{
        ComponentContractAvailability, ComponentContractUnsupported,
        ComponentContractUnsupportedReason, FrameworkAdapterId,
    };

    let unsupported = ComponentContractAvailability::Unsupported(ComponentContractUnsupported {
        adapter_id: FrameworkAdapterId::vue(),
        reason: ComponentContractUnsupportedReason::ComponentMetaUnavailable,
        diagnostics: Arc::from([]),
    });
    let hover = crate::features::hover::build_child_event_hover("@missing", &unsupported)
        .expect("unsupported is a typed diagnostic hover");
    let transported =
        transport_child_hover_result("/src/Unavailable.vue", Ok(ChildHoverOutcome::Hover(hover)))
            .expect("typed unsupported is not a projection transport error");
    let ChildHoverOutcome::Hover(hover) = transported else {
        panic!("typed unsupported hover was collapsed")
    };
    assert!(crate::features::hover::hover_text(&hover).contains("ComponentMetaUnavailable"));

    let transported = transport_child_hover_result(
        "/src/Supported.vue",
        Ok(ChildHoverOutcome::SurfaceAvailableNoMatch),
    )
    .expect("authoritative no-match is not an error");
    assert!(matches!(
        transported,
        ChildHoverOutcome::SurfaceAvailableNoMatch
    ));
}
