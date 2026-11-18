//! Unit tests for the provided slots module.

use verter_audit::observe::Uncaptured;

use super::attribute_operations::project_attribute_operations;
use super::branches::{project_template_branches, TemplateBranchProjection};
use super::checking_fragment::{CheckingPiece, TemplateBlockId};
use super::component_use::{project_component_uses, ComponentUseProjection};
use super::provided_slots::*;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput, ProjectionPlan};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

struct Fixture {
    source: String,
    plan: ProjectionPlan,
    branches: TemplateBranchProjection,
    uses: ComponentUseProjection,
}

impl Fixture {
    fn new(template: &str) -> Self {
        let source = format!(
            "<script setup lang=\"ts\">\nimport Child from './Child.vue';\nimport Other from './Other.vue';\n</script>\n<template>\n{template}\n</template>\n"
        );
        let parsed = parse_sfc(&source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///src/Slots.vue",
            source: &source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let ast = parsed.template_ast().expect("template");
        let topology = build_template_topology(
            &source,
            ast,
            TopologyRecoveryFacts::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .expect("topology");
        let branches = project_template_branches(&plan, &topology);
        let attributes = project_attribute_operations(&plan, &parsed, &source);
        let uses = project_component_uses(&plan, &attributes);
        Self {
            source,
            plan,
            branches,
            uses,
        }
    }

    fn slots(&self) -> ProvidedSlotProjection {
        project_provided_slots(&self.plan, &self.branches, &self.uses)
    }

    fn flat(&self, fragment: &super::checking_fragment::CheckingFragment) -> String {
        fragment.flatten(&self.source, &|id| {
            self.plan.expression(id).expect("admitted").spelling.clone()
        })
    }

    fn head(&self, slots: &ProvidedSlotProjection, index: usize) -> (String, String) {
        let head = slots
            .body_head(slots.obligations[index].block)
            .expect("a projected body has a head");
        (self.flat(&head.before), self.flat(&head.open))
    }

    fn witness(&self, index: usize) -> &str {
        &self.uses.witnesses[index].binding
    }
}

#[test]
fn context_declares_the_params_from_the_specialized_slot_props() {
    let f = Fixture::new("<Child><template #row=\"{ row }\">{{ row }}</template></Child>");
    let slots = f.slots();
    assert!(slots.complete && !slots.pending);
    assert_eq!(slots.obligations.len(), 1);
    assert_eq!(slots.obligations[0].name, SlotName::Static("row".into()));
    let w = f.witness(0);
    assert_eq!(
        slots.contexts[0].props_type,
        format!("__VerterUseSlotProps<typeof {w}, \"row\">")
    );
    let (before, open) = f.head(&slots, 0);
    assert_eq!(before, format!("__VerterSlotName({w}, \"row\");\n"));
    assert_eq!(
        open,
        format!("{{\nconst {{ row }} = null! as __VerterUseSlotProps<typeof {w}, \"row\">;\n")
    );
    assert_eq!(slots.preludes(), vec![PROVIDED_SLOT_PRELUDE]);
}

#[test]
fn the_name_check_maps_to_the_authored_slot_name() {
    let f = Fixture::new("<Child><template #rwo=\"p\">{{ p }}</template></Child>");
    let slots = f.slots();
    let range = slots.obligations[0].name_range.expect("a written name");
    assert_eq!(&f.source[range.start as usize..range.end as usize], "rwo");
    let head = slots.body_head(slots.obligations[0].block).unwrap();
    assert!(head.before.pieces.iter().any(|piece| matches!(
        piece,
        CheckingPiece::Alias { text, source } if text == "\"rwo\"" && *source == range
    )));
}

#[test]
fn omitting_a_declared_slot_adds_nothing() {
    let f = Fixture::new("<Child><template #a=\"p\">{{ p }}</template></Child>");
    let slots = f.slots();
    assert!(slots.complete);
    assert_eq!(slots.policy, SlotPresencePolicy::VueBaseline);
    assert_eq!(slots.obligations.len(), 1);
    let none = Fixture::new("<Child />").slots();
    assert!(none.complete && none.obligations.is_empty() && none.preludes().is_empty());
}

#[test]
fn implicit_default_and_tag_level_slots_are_provided_slots() {
    let f =
        Fixture::new("<Child>{{ text }}</Child>\n<Other v-slot=\"{ item }\">{{ item }}</Other>");
    let slots = f.slots();
    assert_eq!(slots.obligations.len(), 2);
    assert_eq!(slots.obligations[0].name, SlotName::ImplicitDefault);
    assert_eq!(slots.contexts[0].params, None);
    let (before, open) = f.head(&slots, 0);
    assert_eq!(
        before,
        format!("__VerterSlotName({}, \"default\");\n", f.witness(0))
    );
    assert_eq!(open, "{\n");
    assert_eq!(
        slots.obligations[1].name,
        SlotName::Static("default".into())
    );
    let (_, open) = f.head(&slots, 1);
    assert!(open.starts_with("{\nconst { item } = null! as "));
}

#[test]
fn same_name_templates_each_keep_their_own_block_and_name_range() {
    let f = Fixture::new(
        "<Child>\n<template v-if=\"c\" #a=\"{ x }\">{{ x }}</template>\n<template v-else #a=\"{ y }\">{{ y }}</template>\n</Child>",
    );
    let slots = f.slots();
    assert!(slots.complete);
    assert_eq!(slots.obligations.len(), 2);
    assert_ne!(
        slots.obligations[0].name_range,
        slots.obligations[1].name_range
    );
    let (_, first) = f.head(&slots, 0);
    let (_, second) = f.head(&slots, 1);
    assert!(first.contains("{ x }") && second.contains("{ y }"));
}

#[test]
fn slot_before_default_content_keeps_its_own_name() {
    let f = Fixture::new("<Child>loose {{ t }}<template #a=\"p\">{{ p }}</template></Child>");
    let slots = f.slots();
    let names: Vec<_> = slots.obligations.iter().map(|o| o.name.clone()).collect();
    assert!(names.contains(&SlotName::ImplicitDefault));
    assert!(names.contains(&SlotName::Static("a".into())));
}

#[test]
fn a_dynamic_name_is_checked_and_its_props_follow_the_name_type() {
    let f = Fixture::new("<Child><template #[which]=\"p\">{{ p }}</template></Child>");
    let slots = f.slots();
    let SlotName::Dynamic(_) = &slots.obligations[0].name else {
        panic!("a dynamic name");
    };
    let block = slots.obligations[0].block;
    let (before, _) = f.head(&slots, 0);
    assert_eq!(
        before,
        format!(
            "const __VerterSlotKey_{} = __VerterSlotName({}, (which));\n",
            block.0,
            f.witness(0)
        )
    );
    assert!(slots.contexts[0].props_type.starts_with(&format!(
        "string extends typeof __VerterSlotKey_{} ? any : ",
        block.0
    )));
}

#[test]
fn nested_slot_variables_are_blocks_of_their_own() {
    let f = Fixture::new(
        "<Child><template #a=\"{ row }\"><Other><template #b=\"{ row }\">{{ row }}</template></Other></template></Child>",
    );
    let slots = f.slots();
    assert_eq!(slots.obligations.len(), 2);
    let (_, outer) = f.head(&slots, 0);
    let (_, inner) = f.head(&slots, 1);
    assert!(outer.starts_with("{\nconst { row } = null! as ") && outer.ends_with(";\n"));
    assert!(inner.starts_with("{\nconst { row } = null! as ") && inner.contains("\"b\""));
    assert_ne!(slots.obligations[0].block, slots.obligations[1].block);
    assert_ne!(slots.contexts[0].witness, slots.contexts[1].witness);
}

#[test]
fn generic_slot_functions_keep_their_params_pattern_verbatim() {
    let f = Fixture::new(
        "<Child><template #a=\"{ format, rows }: { format: <T>(v: T) => T; rows: number[] }\">{{ rows }}</template></Child>",
    );
    let slots = f.slots();
    let (_, open) = f.head(&slots, 0);
    assert!(open.contains(
        "const { format, rows }: { format: <T>(v: T) => T; rows: number[] } = null! as "
    ));
}

#[test]
fn the_head_references_nothing_but_the_instance_and_the_params() {
    let f = Fixture::new("<Child><template #a=\"p\">{{ p }}</template></Child>");
    let slots = f.slots();
    let (before, open) = f.head(&slots, 0);
    let w = f.witness(0);
    assert_eq!(before, format!("__VerterSlotName({w}, \"a\");\n"));
    assert_eq!(
        open,
        format!("{{\nconst p = null! as __VerterUseSlotProps<typeof {w}, \"a\">;\n")
    );
    assert!(slots.statements().is_empty());
}

#[test]
fn the_pending_value_contributes_nothing() {
    let f = Fixture::new("<Child><template #a=\"p\">{{ p }}</template></Child>");
    let pending = ProvidedSlotProjection::pending(f.plan.snapshot.clone());
    assert!(pending.pending && !pending.complete);
    assert!(pending.preludes().is_empty());
    assert!(pending.body_head(TemplateBlockId(1)).is_none());
}
