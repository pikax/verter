use super::attribute_operations::project_attribute_operations;
use super::component_resolution::{
    project_component_resolution, ComponentSource, DYNAMIC_USE_PRELUDE,
};
use super::component_use::project_component_uses;
use super::script_setup::{project_script_pair, ScriptBlockInput};
use super::script_setup::{ModuleScopeProjection, ScriptProjectionFacts, UniversalSetupBinder};
use crate::cursor::ScriptLanguage;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

fn resolve(
    template: &str,
    bindings: &[&str],
) -> super::component_resolution::ComponentResolutionProjection {
    let source = format!("<template>{template}</template>");
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/RecursiveCard.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let facts = ScriptProjectionFacts {
        module: ModuleScopeProjection {
            normal_script_bindings: bindings.iter().map(|b| (*b).into()).collect(),
            ..Default::default()
        },
        setup: None,
        binder: UniversalSetupBinder::default(),
    };
    project_component_resolution(&plan, &uses, &facts, "file:///src/RecursiveCard.vue", None)
}

#[test]
fn component_resolution_keeps_global_namespace_recursive_and_missing_distinct() {
    let result = resolve(
        "<Local/><Icons.Button/><RecursiveCard/><RegisteredWidget/><MissingThing/>",
        &["Local", "Icons"],
    );
    assert!(result.complete);
    assert_eq!(result.observations.len(), 5);
    assert_eq!(result.observations[0].source, ComponentSource::Local);
    assert_eq!(result.observations[1].source, ComponentSource::Namespace);
    assert_eq!(result.observations[2].source, ComponentSource::Recursive);
    assert_eq!(result.observations[3].source, ComponentSource::Global);
    assert_eq!(result.observations[4].source, ComponentSource::Global);
    assert_eq!(result.observations[2].expression, "__VerterPublicComponent");
    assert!(result
        .render_fallbacks()
        .contains("GlobalComponentType<'MissingThing'>"));
    assert!(result
        .render_fallbacks()
        .contains("globalComponentsNav().MissingThing"));
}

#[test]
fn dynamic_contract_recovers_only_one_shared_discriminated_source() {
    let result = resolve(
        "<component :is=\"choice.component\" v-bind=\"choice.props\"/><component :is=\"other.component\" v-bind=\"choice.props\"/>",
        &["choice", "other"],
    );
    assert!(result.complete);
    assert_eq!(result.dynamic.len(), 2);
    assert_eq!(
        result.dynamic[0].correlated_source.as_deref(),
        Some("choice")
    );
    assert_eq!(result.dynamic[1].correlated_source, None);
    assert!(result.dynamic[0]
        .render(&result.observations[0].witness)
        .contains("__VerterDynamicCorrelated(choice, \"component\", \"props\")"));

    let changed = resolve(
        "<component :is=\"choice.component\" v-bind=\"choice.otherProps\"/>",
        &["choice"],
    );
    assert!(changed.complete);
    assert_eq!(changed.dynamic[0].correlated_source, None);
    assert_ne!(result.snapshot, changed.snapshot);
}

#[test]
fn resolution_probes_use_the_emitted_contract_and_global_fallback() {
    let positive = include_str!("../../../../../tests/sfc-projection/STP32/probes/positive.ts");
    let negative = include_str!("../../../../../tests/sfc-projection/STP32/probes/negative.ts");
    let compact = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .replace(",)", ")")
    };
    assert!(compact(positive).contains(&compact(DYNAMIC_USE_PRELUDE)));
    assert!(compact(negative).contains(&compact(DYNAMIC_USE_PRELUDE)));
    let global = resolve("<RegisteredWidget/>", &[]);
    assert!(positive.contains(&global.render_fallbacks().replace('\'', "\"")));
    let emitted = global.render_fallbacks();
    let start = emitted.find("const RegisteredWidget").unwrap() + "const ".len();
    let nav = crate::ide::script::global_component_nav_probe_offset(
        &emitted,
        start as u32,
        (start + "RegisteredWidget".len()) as u32,
    );
    assert_eq!(
        nav.map(|offset| &emitted[offset as usize..offset as usize + 16]),
        Some("RegisteredWidget")
    );
    let missing = resolve("<MissingThing/>", &[]);
    assert!(negative.contains(&missing.render_fallbacks().replace('\'', "\"")));
    assert!(positive.contains("__VerterDynamicCorrelated(choice, \"component\", \"props\")"));
    assert!(negative.contains("__VerterDynamicCorrelated(wrong, \"component\", \"props\")"));
}

#[test]
fn kebab_fallback_preserves_authored_intrinsic_distinction_and_pascal_intent() {
    let kebab = resolve("<registered-widget/>", &[]);
    assert!(kebab
        .render_fallbacks()
        .contains("___VERTER___GlobalComponentKebabType<'RegisteredWidget', 'registered-widget'>"));
    let mixed = resolve("<registered-widget/><RegisteredWidget/>", &[]);
    assert_eq!(
        mixed
            .render_fallbacks()
            .matches("const RegisteredWidget")
            .count(),
        1
    );
    assert!(mixed
        .render_fallbacks()
        .contains("___VERTER___GlobalComponentType<'RegisteredWidget'>"));
}

#[test]
fn imported_namespace_and_async_setup_binding_use_script_facts() {
    let body = "import * as Icons from './barrel';\nimport { defineAsyncComponent } from 'vue';\nconst AsyncCard = defineAsyncComponent(() => import('./Card.vue'));";
    let source = format!(
        "<script setup lang=\"ts\">{body}</script><template><Icons.Button/><AsyncCard/></template>"
    );
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Parent.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let facts = project_script_pair(
        None,
        Some(ScriptBlockInput {
            content: body,
            content_start: "<script setup lang=\"ts\">".len() as u32,
            lang: Some(ScriptLanguage::TypeScript),
        }),
        None,
    )
    .unwrap();
    let resolved =
        project_component_resolution(&plan, &uses, &facts, "file:///src/Parent.vue", None);
    assert!(resolved.complete);
    assert_eq!(resolved.observations[0].source, ComponentSource::Namespace);
    assert_eq!(resolved.observations[1].source, ComponentSource::Local);
    assert_eq!(resolved.observations[1].expression, "AsyncCard");
}

#[test]
fn resolution_specialization_changes_with_binding_authority_and_snapshot() {
    let local = resolve("<Local/>", &["Local"]);
    let global = resolve("<Local/>", &[]);
    assert_eq!(local.snapshot, global.snapshot);
    assert_ne!(
        local.observations[0].specialization,
        global.observations[0].specialization
    );

    let shifted = resolve("  <Local/>", &["Local"]);
    assert_ne!(local.snapshot, shifted.snapshot);
    assert_ne!(
        local.observations[0].specialization,
        shifted.observations[0].specialization
    );
}

#[test]
fn configured_custom_element_never_enters_component_resolution() {
    let source = "<template><my-widget label=\"native\"/></template>";
    let configured = ["my-".to_string()];
    let parsed = crate::compile::parse_sfc(source, None, Some(&configured));
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Parent.vue",
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, source);
    let uses = project_component_uses(&plan, &attributes);
    let facts = ScriptProjectionFacts {
        module: ModuleScopeProjection::default(),
        setup: None,
        binder: UniversalSetupBinder::default(),
    };
    let resolved = project_component_resolution(
        &plan,
        &uses,
        &facts,
        "file:///src/Parent.vue",
        Some(&configured),
    );
    assert!(resolved.complete);
    assert!(resolved.observations.is_empty());
    assert!(resolved.render_fallbacks().is_empty());
}
