//! Discriminating tests for the scope-aware
//! `resolve_ref_to_root_identity` lookup: identity is
//! `ResolvedRootIdentity { canonical_id, symbol_name }`, so the same name in
//! two scopes is not collapsed, and a local declaration shadows a same-name
//! import per JavaScript module scoping.

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

#[test]
fn model_modifiers_props_close_the_script_setup_instance() {
    use verter_session_query::analysis::component_meta::PublicInstanceCompleteness;

    for (call, value_name, modifiers_name) in [
        ("defineModel()", "modelValue", "modelModifiers"),
        ("defineModel('count')", "count", "countModifiers"),
        (
            "defineModel<string, 'trim'>()",
            "modelValue",
            "modelModifiers",
        ),
    ] {
        let project = make_project();
        project
            .upsert_base(
                "/Model.vue",
                &format!("<script setup lang=\"ts\">const model = {call}</script>"),
            )
            .unwrap();
        let output = project
            .host()
            .get_component_meta_output("/Model.vue")
            .unwrap()
            .expect("model component output");
        let (analysis, _, _) = output.into_parts();
        assert_eq!(
            analysis
                .props
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec![value_name, modifiers_name],
            "{call}"
        );
        assert!(!analysis.props[1].required, "{call}");
        assert!(!analysis.props[1].has_default, "{call}");
        assert_eq!(
            analysis
                .models
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec![value_name],
            "{call}"
        );
        assert_eq!(
            analysis
                .events
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec![format!("update:{value_name}")],
            "{call}"
        );
        assert!(analysis.domain_guarantees.enumeration.models, "{call}");
        assert_eq!(analysis.domain_guarantees.public_instance.kind, 1, "{call}");
        assert!(!analysis.domain_guarantees.public_instance.open, "{call}");
        let instance = analysis.public_instance.expect("model instance sidecar");
        assert_eq!(
            instance.completeness,
            PublicInstanceCompleteness::Exact,
            "{call}"
        );
        assert_eq!(
            instance
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec![value_name, modifiers_name],
            "{call}"
        );
    }
}

#[test]
fn public_instance_indirect_options_exports_report_options_api_members() {
    use verter_session_query::analysis::component_meta::{
        PublicInstanceCompleteness, PublicInstanceStructuralFailure,
    };

    for (script, expected_props) in [
        (
            "const opts = { props: ['label'], methods: { hidden() {} } }; export default opts;",
            vec!["label"],
        ),
        (
            "import { defineComponent as dc } from 'vue'; export default dc({ props: ['label'], methods: { hidden() {} } });",
            vec!["label"],
        ),
        ("export default missing;", vec![]),
        ("export default makeOptions();", vec![]),
        ("export default function Component() {}", vec![]),
    ] {
        for setup in [
            "",
            "<script setup>const opts = { props: ['wrong'] }; defineExpose({ visible: 1 });</script>",
        ] {
            let project = make_project();
            project
                .upsert_base(
                    "/App.vue",
                    &format!("<script>{script}</script>{setup}"),
                )
                .unwrap();
            let output = project
                .host()
                .get_component_meta_output("/App.vue")
                .unwrap()
                .expect("component output");
            let (analysis, _, _) = output.into_parts();
            assert!(analysis.options_api, "{script}");
            assert_eq!(
                analysis
                    .props
                    .iter()
                    .map(|prop| prop.name.as_str())
                    .collect::<Vec<_>>(),
                expected_props,
                "{script} {setup}"
            );
            assert!(
                !analysis.domain_guarantees.enumeration.options_member_free,
                "{script}"
            );
            assert_eq!(analysis.domain_guarantees.public_instance.kind, 2, "{script}");
            assert_eq!(
                analysis.domain_guarantees.public_instance.causes,
                vec![103],
                "{script}"
            );
            if !setup.is_empty() {
                let instance = analysis.public_instance.expect("exposed member sidecar");
                assert_eq!(
                    instance.structural_failure,
                    Some(PublicInstanceStructuralFailure::OptionsApiMembers),
                    "{script}"
                );
                assert_eq!(
                    instance.completeness,
                    PublicInstanceCompleteness::Partial,
                    "{script}"
                );
            }
        }
    }
}

/// Discriminating test 4 — scope correctness.
///
/// SFC A imports `Helper` from a sibling file B. A also defines a
/// LOCAL `Helper` inside `defineProps<{ inner: Helper }>()`. The
/// local declaration shadows the import per JS module-scope rules.
///
/// The walker MUST distinguish the two `Helper` identities by
/// `ResolvedRootIdentity` — the local one keys on
/// `(/src/App.vue, "Helper")`, the imported one keys on
/// `(/src/b.ts, "Helper")`.
///
/// The deleted string-keyed walker collected `(String, usize)` pairs
/// by name only, so the two `Helper`s collided. The
/// `ResolvedRootIdentity`-keyed walker resolves each `Ref` to its
/// canonical identity and the two scopes stay distinct.
#[test]
fn macro_participation_distinguishes_local_helper_from_imported_helper() {
    use verter_session_query::type_solver::host::ResolvedRootIdentity;

    let project = make_project();
    project
        .upsert_base(
            "/src/b.ts",
            r#"export interface Helper {
  fromB: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
// Local Helper shadows the import for the macro's type argument scope.
interface Helper {
  inner: number
}
defineProps<Helper>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Establish base host state by issuing a get_component_meta call.
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should resolve through the local Helper, not the import");

    // Verify that resolve_ref_to_root_identity discriminates the two
    // Helper identities for the App.vue scope.
    let host = project.host();
    let local_identity = super::resolve_ref_to_root_identity_for_test(
        host,
        "/src/App.vue",
        verter_type_expr::TopLevelOwnerId::instance(0),
        "Helper",
    )
    .expect("local Helper must resolve to a root identity");
    let imported_identity = super::resolve_ref_to_root_identity_for_test(
        host,
        "/src/b.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Helper",
    )
    .expect("imported Helper must resolve to a root identity in b.ts scope");

    assert_eq!(
        local_identity,
        ResolvedRootIdentity::new_in_owner(
            "/src/App.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Helper",
        ),
        "local Helper must key on App.vue, NOT b.ts"
    );
    assert_eq!(
        imported_identity,
        ResolvedRootIdentity::new("/src/b.ts", "Helper"),
        "imported Helper must key on b.ts"
    );
    assert_ne!(
        local_identity, imported_identity,
        "the two Helper identities MUST NOT collide — naive (String, usize) keying would have collapsed them"
    );
}
