//! Tests for the [`ComponentMetaHost`] / [`ComponentMetaSession`] seam —
//! extracted sibling of `component_meta_host.rs` (file-size split; the
//! `#[path]` attach keeps the `component_meta_host::tests::*` test paths).

use super::*;

fn make_host() -> ComponentMetaHost {
    // Tests use `cpu_threads = 1` to avoid CPU oversubscription
    // when many parallel test threads each spin up their own
    // Rayon pools.
    ComponentMetaHost::new_standalone_with_scheduler_config(
        crate::types::HostConfig::default(),
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    )
}

#[test]
fn upsert_base_and_get_source() {
    let host = make_host();
    host.upsert_base("/src/Foo.vue", "<template><div/></template>")
        .unwrap();
    let session = host.open_session_batch().unwrap();
    let source = session.get_effective_source("/src/Foo.vue").unwrap();
    assert!(source.is_some());
    assert!(source.unwrap().contains("<template>"));
}

#[test]
fn session_overlays_are_isolated() {
    let host = make_host();
    host.upsert_base("/src/Foo.vue", "<template><div/></template>")
        .unwrap();
    let session_a = host.open_session_batch().unwrap();
    let session_b = host.open_session_batch().unwrap();

    session_a
        .upsert("/src/Foo.vue", "<template><span/></template>".to_string())
        .unwrap();

    assert_eq!(
        session_a.get_effective_source("/src/Foo.vue").unwrap(),
        Some("<template><span/></template>".to_string())
    );
    assert_eq!(
        session_b.get_effective_source("/src/Foo.vue").unwrap(),
        Some("<template><div/></template>".to_string())
    );
}

#[test]
fn closing_session_reverts_its_overlays() {
    let host = make_host();
    host.upsert_base("/src/Foo.vue", "<template><div/></template>")
        .unwrap();

    let session_a = host.open_session_batch().unwrap();
    session_a
        .upsert("/src/Foo.vue", "<template><span/></template>".to_string())
        .unwrap();
    session_a.close();

    let session_b = host.open_session_batch().unwrap();
    assert_eq!(
        session_b.get_effective_source("/src/Foo.vue").unwrap(),
        Some("<template><div/></template>".to_string())
    );
}

#[test]
fn shutdown_prevents_further_operations() {
    let host = make_host();
    host.shutdown();
    assert!(host.is_shutdown());
    assert!(host.upsert_base("/src/X.vue", "").is_err());
}

#[test]
fn get_component_meta_returns_none_for_missing() {
    let host = make_host();
    let session = host.open_session_batch().unwrap();
    let result = session.get_component_meta("/nonexistent.vue").unwrap();
    assert!(result.is_none());
}

#[test]
fn get_component_meta_returns_some_for_loaded_sfc() {
    let host = make_host();
    host.upsert_base(
        "/src/Button.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div>{{ msg }}</div></template>",
    )
    .unwrap();
    let session = host.open_session_batch().unwrap();
    let result = session.get_component_meta("/src/Button.vue").unwrap();
    assert!(result.is_some(), "should return meta for loaded SFC");
}

/// FIX-B regression (scalar, `ComponentMetaSession` boundary): a
/// fail-closed output-materialization failure crossing the host boundary
/// stays the TYPED `ComponentMetaHostError::OutputMaterialization`
/// variant — with the failed lane / positional index intact — never the
/// demoted `Host(String)`. Discriminating: with the `From<MetaError>`
/// conversion demoting the variant to `Host(err.to_string())`, the
/// typed match below fails.
#[test]
fn scalar_payload_output_failure_stays_typed_at_component_meta_session_boundary() {
    let host = make_host();
    host.upsert_base(
        "/src/TypedErr.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div /></template>",
    )
    .unwrap();
    let session = host.open_session_batch().unwrap();

    crate::test_only::component_meta_output::force_output_failure_for("/src/TypedErr.vue");

    let err = session
        .get_component_meta_payload("/src/TypedErr.vue", |output| {
            let (analysis, _resolution, _types) = output.into_parts();
            format!("props={}", analysis.props.len()).into_bytes()
        })
        .expect_err("a forced output-materialization failure must fail the scalar call");
    match err {
        ComponentMetaHostError::OutputMaterialization(inner) => {
            assert_eq!(
                inner.lane,
                crate::meta_resolve::ComponentMetaOutputLane::Prop,
                "the typed error carries the failed lane across the host boundary"
            );
            assert_eq!(inner.index, 0, "the positional index survives");
        }
        other => panic!(
            "the typed OutputMaterialization variant must survive the \
             MetaError -> ComponentMetaHostError conversion — never the \
             demoted Host(String); got {other:?}"
        ),
    }
}

/// AUDITED output-failure record fidelity: with `audit_enabled +
/// footprint_capture`, a forced output-materialization failure on the
/// audited session entry must return the REAL `ActiveStored` audit
/// record the resolution published — retrieved by the request id the
/// output entry threads through its error terminal — paired with the
/// typed `OutputMaterialization` outcome, and must DRAIN it from the
/// bounded store (no orphan left behind).
///
/// Discriminating: with the error terminal dropping the request id, the
/// wrapper fabricates a `cheap_component_meta_record` (`request_id: 0`,
/// `FilteredNoop`) while the real record rots in the store — the
/// nonzero-id, `ActiveStored`, and empty-store asserts all fail RED.
#[test]
fn output_failure_audited_entry_returns_the_real_stored_record_not_a_fabricated_noop() {
    let host = ComponentMetaHost::new_standalone_with_scheduler_config(
        crate::types::HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: true,
            ..crate::types::HostConfig::default()
        },
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    );
    host.upsert_base(
        "/src/AuditOutputFailure.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div /></template>",
    )
    .unwrap();
    let session = host.open_session_batch().unwrap();

    crate::test_only::component_meta_output::force_output_failure_for(
        "/src/AuditOutputFailure.vue",
    );

    let (outcome, record) = session
        .get_component_meta_with_audit("/src/AuditOutputFailure.vue")
        .into_parts();
    let err = outcome.expect_err("the forced output failure surfaces as the typed error");
    assert!(
        matches!(err, ComponentMetaHostError::OutputMaterialization(_)),
        "the audited error outcome stays the typed OutputMaterialization; got {err:?}"
    );

    // The carrier holds the REAL record the resolution published — the
    // error path is audited IDENTICALLY to success.
    assert_ne!(
        record.request_id, 0,
        "the returned record is the real stored record, never the \
         fabricated zero-id cheap record"
    );
    assert_eq!(
        record.capture_state,
        verter_audit::AuditCaptureState::ActiveStored,
        "the real record was collected and published (never FilteredNoop)"
    );
    assert_eq!(record.canonical_id, "/src/AuditOutputFailure.vue");
    assert_eq!(
        record.kind,
        verter_audit::RequestKind::ComponentMeta,
        "the record describes the component-meta resolution"
    );

    // The wrapper DRAINED the record: nothing remains in the host's
    // bounded store — this call was the fresh host's only audited
    // activity, so any residual entry is exactly the orphan this
    // regression guards against.
    let base_host = host.inner.project.host();
    assert!(
        base_host.audit_records.is_empty(),
        "no orphan audit record may remain in the store after the \
         audited error return; {} record(s) left behind",
        base_host.audit_records.len(),
    );
    assert!(
        base_host.take_audit_record(record.request_id).is_none(),
        "the returned record was drained (a second take yields None)"
    );
}

/// FIX-A regression (batch, `ComponentMetaSession` boundary — the shape
/// the NAPI `getComponentMetaBatch` binding consumes): a forced per-item
/// output-materialization failure fails the whole batch CALL with the
/// TYPED error (scalar ≡ batch — exactly as the scalar payload call
/// throws), never a per-item shape change and never a silent missing
/// slot; with no failure armed, the same batch returns the pre-cut
/// `Vec<Option<Vec<u8>>>` shape where `None` is reserved EXCLUSIVELY
/// for a genuinely missing canonical.
#[test]
fn batch_payload_output_failure_fails_the_call_typed_and_absence_stays_none_slot() {
    let host = make_host();
    host.upsert_base(
        "/src/BatchOk.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: string }>()\n</script>\n<template><div /></template>",
    )
    .unwrap();
    host.upsert_base(
        "/src/BatchFail.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ b: number }>()\n</script>\n<template><div /></template>",
    )
    .unwrap();
    let session = host.open_session_batch().unwrap();
    let ids = vec![
        "/src/BatchOk.vue".to_string(),
        "/src/BatchFail.vue".to_string(),
        "/src/BatchMissing.vue".to_string(), // never upserted
    ];
    let encode = |output: crate::meta_resolve::ComponentMetaOutput| {
        let (analysis, _resolution, _types) = output.into_parts();
        format!("props={}", analysis.props.len()).into_bytes()
    };

    // (1) Forced per-item failure FIRST (before any warm payload can be
    // admitted): the whole batch CALL errors with the TYPED variant
    // (lane + positional index intact) — a real failure is never
    // collapsed onto the missing sentinel or a slot shape.
    crate::test_only::component_meta_output::force_output_failure_for("/src/BatchFail.vue");
    let err = session
        .get_component_meta_batch_payloads(&ids, encode)
        .expect_err("a forced output-materialization failure must fail the batch call");
    match err {
        ComponentMetaHostError::OutputMaterialization(inner) => {
            assert_eq!(
                inner.lane,
                crate::meta_resolve::ComponentMetaOutputLane::Prop,
                "the call-level error carries the typed lane"
            );
        }
        other => panic!(
            "the batch call-level error must be the typed \
             OutputMaterialization variant (scalar ≡ batch); got {other:?}"
        ),
    }

    // (2) Knob consumed — the SAME batch recovers to the pre-cut shape:
    // one Option slot per input, `None` EXCLUSIVELY for the genuinely
    // missing canonical (the failure above admitted no poisoned
    // payload).
    let slots = session
        .get_component_meta_batch_payloads(&ids, encode)
        .expect("no forced failure: the batch call succeeds");
    assert_eq!(slots.len(), 3);
    assert!(
        matches!(&slots[0], Some(bytes) if bytes.as_slice() == b"props=1"),
        "successful slot carries the encoded payload; got {:?}",
        slots[0]
    );
    assert!(slots[1].is_some(), "second component resolves");
    assert!(
        slots[2].is_none(),
        "a genuinely missing canonical keeps the None sentinel; got {:?}",
        slots[2]
    );
}

/// FIX-A regression (output batch, `ComponentMetaSession` boundary — the
/// shape the WASM `getComponentMetaBatch` binding consumes): identical
/// call-level failure semantics on the output-envelope batch.
#[test]
fn output_batch_failure_fails_the_call_typed_and_absence_stays_none_slot() {
    let host = make_host();
    host.upsert_base(
        "/src/OutOk.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: string }>()\n</script>\n<template><div /></template>",
    )
    .unwrap();
    let session = host.open_session_batch().unwrap();
    let ids = vec![
        "/src/OutOk.vue".to_string(),
        "/src/OutMissing.vue".to_string(), // never upserted
    ];

    crate::test_only::component_meta_output::force_output_failure_for("/src/OutOk.vue");
    let err = session
        .get_component_meta_output_batch(&ids)
        .expect_err("a forced output-materialization failure must fail the call");
    assert!(
        matches!(err, ComponentMetaHostError::OutputMaterialization(_)),
        "the output-batch call-level error stays typed; got {err:?}"
    );

    // Knob consumed — the same batch recovers: `None` reserved
    // EXCLUSIVELY for the genuinely missing canonical.
    let slots = session
        .get_component_meta_output_batch(&ids)
        .expect("no forced failure: the output batch call succeeds");
    assert_eq!(slots.len(), 2);
    assert!(slots[0].is_some(), "resolved component fills its slot");
    assert!(
        slots[1].is_none(),
        "a genuinely missing canonical keeps the None sentinel"
    );
}

#[test]
fn component_meta_with_resolution_keeps_resolved_type_registry_sidecar() {
    let host = make_host();
    host.upsert_base(
        "/src/types.ts",
        r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  appConfig?: A
}"#,
    )
    .unwrap();
    host.upsert_base(
        "/src/theme.ts",
        r#"export default {
  variants: {
color: { primary: '', secondary: '' }
  },
  slots: {
base: '',
label: ''
  }
} as const"#,
    )
    .unwrap();
    host.upsert_base(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    )
    .unwrap();

    let session = host.open_session_batch().unwrap();
    let (_analysis, resolved) = session
        .get_component_meta_with_resolution("/src/Button.vue")
        .unwrap()
        .expect("canonical query should return meta plus resolution sidecar");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("canonical query should keep the resolved Button registry entry");
    let button_ty = crate::test_only::semantic_source_probe::demand_type_expr(
        host.host(),
        "/src/Button.vue",
        button_entry.type_source.present().expect("present source"),
    )
    .unwrap_or_else(|| panic!("Button's published registry source must demand-materialize"));
    let TypeExpr::Object(button_shape) = &button_ty else {
        panic!("expected resolved Button helper to materialize as an object, got {button_ty:?}",);
    };

    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button registry entry should keep variants");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "expected Button.variants to materialize as an object, got {:?}",
            variants_member
        );
    };
    assert!(
        variants_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "color"),
        ),
        "expected Button.variants to expose color, got {:?}",
        variants_member
    );
}

#[test]
fn component_meta_budget_errors_surface_on_new_session_api() {
    let host = ComponentMetaHost::new_standalone_with_scheduler_config(
        crate::types::HostConfig {
            external_resolution_step_budget: Some(40),
            ..crate::types::HostConfig::default()
        },
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    );

    let import_count = 45usize;
    let mut defs_source = String::new();
    for index in 0..import_count {
        defs_source.push_str(&format!(
            "export interface T{index} {{ p{index}: string }}\n"
        ));
    }

    let mut types_source = String::new();
    types_source.push_str("import type { ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" } from './defs'\n");
    types_source.push_str("export interface Props extends ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" {}\n");

    host.upsert_base("/src/defs.ts", &defs_source).unwrap();
    host.upsert_base("/src/types.ts", &types_source).unwrap();
    host.upsert_base(
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
    )
    .unwrap();
    host.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./defs".to_string(),
            resolved_canonical_id: Some("/src/defs.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = host.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .expect("large finite heritage graph should resolve successfully")
        .expect("component meta should be present");

    // All 2005 interfaces should contribute one prop each
    assert_eq!(
        meta.props.len(),
        import_count,
        "large finite heritage graph should produce all {import_count} props"
    );
    // Spot-check the first prop and confirm the highest-numbered prop is
    // still present; projected surfaces are sorted lexicographically.
    assert_eq!(meta.props[0].name, "p0");
    assert!(
        meta.props
            .iter()
            .any(|prop| prop.name == format!("p{}", import_count - 1)),
        "large finite heritage graph should retain p{} somewhere in the deterministic lexical surface order",
        import_count - 1
    );
}

#[test]
fn extracted_external_meta_keeps_fallthrough_on_captured_store_view() {
    let host = make_host();
    host.upsert_base("/src/Link.vue", "<template><a /></template>")
        .unwrap();
    host.upsert_base(
        "/src/Button.vue",
        r#"<script setup lang="ts">
import Link from './Link.vue'
</script>
<template><Link /></template>"#,
    )
    .unwrap();

    let _store_view = host.host().resolver_store_view();
    let resolved = host
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("button resolved state should exist for the captured store view");

    host.upsert_base("/src/Link.vue", "<script setup lang=\"ts\"></script>")
        .unwrap();

    let meta = crate::resolver_core::with_bare_host_ctx_for_test(host.host(), |ctx| {
        extract_component_meta_from_resolved_with_evaluated(
            host.host(),
            ctx,
            "/src/Button.vue",
            &resolved,
            resolved.evaluated_types.as_ref(),
            true,
        )
    });

    assert!(
        matches!(
            meta.fallthrough_surface,
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
        ),
        "captured store views should keep child fallthrough resolution pinned to the resolved snapshot",
    );
}
