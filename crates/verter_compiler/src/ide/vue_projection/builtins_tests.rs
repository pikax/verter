//! Unit tests for the builtins module.

use super::attribute_operations::project_attribute_operations;
use super::builtins::*;
use super::component_resolution::project_component_resolution;
use super::component_use::project_component_uses;
use super::script_setup::project_script_pair;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

#[test]
fn carrier_shape_uses_parsed_block_presence_even_when_contents_are_empty() {
    for (source, expected) in [
        ("", CarrierShape::Empty),
        ("<style>.a { color: red }</style>", CarrierShape::Empty),
        ("<!-- <script setup lang=\"ts\"> -->", CarrierShape::Empty),
        ("<template></template>", CarrierShape::TemplateOnly),
        ("<script lang=\"ts\"></script>", CarrierShape::OptionsOnly),
        (
            "<script setup lang=\"ts\"></script>",
            CarrierShape::ScriptSetup,
        ),
        (
            "<script lang=\"ts\"></script><script setup lang=\"ts\"></script>",
            CarrierShape::ScriptAndSetup,
        ),
    ] {
        let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
        assert_eq!(CarrierShape::of(&parsed), expected, "{source}");
    }
}

#[test]
fn builtins_share_recognition_and_the_existing_component_witness() {
    let source = "<template><teleport to=\"#app\"/><transition/><transition-group/><keep-alive><Child ref=\"child\" value=\"text\"/></keep-alive><suspense/><base-transition/><Ordinary/></template>";
    let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Comp.vue",
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, source);
    let uses = project_component_uses(&plan, &attributes);
    let facts = project_script_pair(None, None, None).expect("scriptless facts");
    let resolution =
        project_component_resolution(&plan, &uses, &facts, "file:///src/Comp.vue", None);
    let builtins = project_builtins(&plan, &resolution, CarrierShape::of(&parsed));
    assert!(builtins.complete && !builtins.pending);
    assert_eq!(builtins.obligations.len(), 6);
    let child = uses
        .witnesses
        .iter()
        .find(|w| w.component == "Child")
        .expect("child witness");
    assert!(child
        .observations
        .iter()
        .any(|o| o.kind == super::component_use::ObservationKind::Instance));
    assert!(child
        .transaction
        .members
        .iter()
        .any(|member| matches!(member,
            super::component_use::TransactionMember::Property { key, .. } if key == "value"
        )));
    for (obligation, name) in builtins.obligations.iter().zip([
        "Teleport",
        "Transition",
        "TransitionGroup",
        "KeepAlive",
        "Suspense",
        "BaseTransition",
    ]) {
        assert!(obligation
            .before()
            .flatten(source, &|_| unreachable!())
            .contains(&format!("typeof import(\"vue\").{name}")));
        assert!(obligation
            .constructor
            .contains("__VerterUseConstruct<any[], InstanceType<typeof "));
        assert!(uses.witnesses.iter().any(|w| w.use_id == obligation.use_id));
    }
}
