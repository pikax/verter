use super::attribute_operations::project_attribute_operations;
use super::component_use::*;
use super::public_constructor::project_public_constructor;
use super::script_setup::ScriptBlockInput;
use crate::cursor::ScriptLanguage;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

const TABLE_GENERIC: &str = "T, U";

const TABLE_SETUP: &str = r#"import type { VNode } from "vue";
defineProps<{ rows: readonly T[]; project: (row: T) => U } & ({ kind: "list" } | { kind: "grid"; columns: number })>();
defineEmits<{ change: [value: U] }>();
defineModel<U>();
defineSlots<{ default(props: { row: T; value: U }): VNode[] }>();
"#;

/// Parent template of the positive probe: a coupled use (rows, callback,
/// discriminant, model, two listeners, slot) and an independent sibling.
const POSITIVE_TEMPLATE: &str = concat!(
    "  <Table :rows=\"rows\" :project=\"(row) => row.name\" kind=\"list\" v-model=\"selected\" @change=\"(value) => log(value)\" v-on:change=\"log\">\n",
    "    <template #default=\"{ row, value }\">{{ row.id }}{{ value }}</template>\n",
    "  </Table>\n",
    "  <Table :rows=\"ids\" :project=\"(id) => id * 2\" kind=\"grid\" :columns=\"3\" @change=\"total += $event\" />",
);

/// Parent template of the negative probe: collected listeners — a plain
/// one, an event-option one and an optional member — whose parameter
/// contradicts the specialized payload.
const NEGATIVE_TEMPLATE: &str = concat!(
    "  <Table :rows=\"rows\" :project=\"(row) => row.name\" kind=\"list\" @change=\"log\" v-on:change=\"count\" />\n",
    "  <Table :rows=\"rows\" :project=\"(row) => row.name\" kind=\"list\" @change.once=\"count\" />\n",
    "  <Table :rows=\"rows\" :project=\"(row) => row.name\" kind=\"list\" @change=\"log\" v-on:change=\"handlers?.onChange\" />",
);

fn sfc(template: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nimport Table from './components/Table.vue';\n</script>\n<template>\n{template}\n</template>\n"
    )
}

fn project(template: &str) -> ComponentUseProjection {
    let source = sfc(template);
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///uses.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    project_component_uses(&plan, &attributes)
}

fn only(projection: &ComponentUseProjection) -> &ComponentUseWitness {
    assert!(projection.complete);
    assert_eq!(projection.witnesses.len(), 1);
    &projection.witnesses[0]
}

fn member_keys(witness: &ComponentUseWitness) -> Vec<Option<&str>> {
    witness
        .transaction
        .members
        .iter()
        .map(TransactionMember::key)
        .collect()
}

fn member<'w>(witness: &'w ComponentUseWitness, key: &str) -> &'w MemberValue {
    witness
        .transaction
        .members
        .iter()
        .find_map(|member| match member {
            TransactionMember::Property { key: k, value, .. } if k == key => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no construction member {key}"))
}

fn constructions(rendered: &str) -> usize {
    rendered
        .matches(&format!("new ({USE_CONSTRUCTOR}("))
        .count()
}

/// Every authored channel of one use — data, callback, discriminant, model
/// value and listener — is a member of its one construction; the use
/// renders exactly one construction, and it names the component directly.
#[test]
fn component_use_single_witness_carries_every_channel_once() {
    let projection = project(
        "  <Table :rows=\"rows\" :project=\"(row) => row.name\" kind=\"list\" v-model=\"selected\" @change=\"log\">\n    <template #default=\"{ row }\">{{ row.id }}</template>\n  </Table>",
    );
    let witness = only(&projection);
    assert_eq!(
        member_keys(witness),
        vec![
            Some("rows"),
            Some("project"),
            Some("kind"),
            Some("modelValue"),
            Some("onChange"),
        ]
    );
    assert!(witness.transaction.validations.is_empty());
    let rendered = witness.render();
    assert_eq!(constructions(&rendered), 1);
    assert!(rendered.starts_with(&format!(
        "const {} = new ({USE_CONSTRUCTOR}(Table))({{ \"rows\": (rows), ",
        witness.binding
    )));
    assert!(!rendered.contains("instantiateComponent"));
    assert_eq!(constructions(&projection.render()), 1);
    assert_eq!(projection.render().matches(USE_PRELUDE).count(), 1);
}

/// Slot props, listener contracts, model write types and the ref instance
/// all read the use's witness, never the uninstantiated component.
#[test]
fn component_use_observations_read_the_specialized_witness() {
    let projection = project(
        "  <Table :rows=\"rows\" :project=\"(row) => row.name\" v-model=\"selected\" @change=\"log\">\n    <template #default=\"{ row }\">{{ row.id }}</template>\n    <template #footer>x</template>\n  </Table>",
    );
    let witness = only(&projection);
    let owner = format!("typeof {}", witness.binding);
    let kinds: Vec<&ObservationKind> = witness.observations.iter().map(|o| &o.kind).collect();
    assert_eq!(
        kinds,
        vec![
            &ObservationKind::Slot {
                name: "default".to_string()
            },
            &ObservationKind::Slot {
                name: "footer".to_string()
            },
            &ObservationKind::Event {
                key: "onChange".to_string(),
                fallback: None,
            },
            &ObservationKind::Model {
                name: "modelValue".to_string()
            },
            &ObservationKind::Instance,
        ]
    );
    for observation in &witness.observations {
        assert!(
            observation.type_text == owner
                || observation.type_text.contains(&format!("<{owner}, ")),
            "observation must read the witness: {}",
            observation.type_text
        );
        assert!(!observation.type_text.contains("typeof Table"));
        assert!(!observation.type_text.contains("InstanceType"));
    }
    assert_eq!(
        witness.observations[0].type_text,
        format!("__VerterUseSlotProps<{owner}, \"default\">")
    );
    assert_eq!(
        witness.observations[3].type_text,
        format!("__VerterUseModel<{owner}, \"onUpdate:modelValue\">")
    );
}

/// Unannotated callbacks — a bound callback prop and a function-expression
/// listener — stay inside the construction, so TypeScript types their
/// parameters from the one specialization; only an inline statement runs
/// as a `$event` handler checked against the specialized contract.
#[test]
fn component_use_contextual_callbacks_stay_in_the_construction() {
    let projection = project(
        "  <Table :rows=\"rows\" :project=\"(row) => row.name\" @change=\"(value) => log(value)\" v-on:change=\"total += $event\" />",
    );
    let witness = only(&projection);
    assert_eq!(
        member(witness, "project"),
        &MemberValue::Expression {
            id: member(witness, "project").expression().unwrap().clone(),
            spelling: "(row) => row.name".to_string(),
        }
    );
    assert!(matches!(
        member(witness, "onChange"),
        MemberValue::Expression { spelling, .. } if spelling == "(value) => log(value)"
    ));
    // Rendered bare, so the target contract types the parameters.
    assert!(witness.render().contains(
        "\"rows\": (rows), \"project\": ((row) => row.name), \"onChange\": ((value) => log(value)) });"
    ));
    assert_eq!(witness.transaction.validations.len(), 1);
    let check = &witness.transaction.validations[0];
    assert_eq!(check.contract, CheckContract::Listener);
    assert!(matches!(
        &check.value,
        MemberValue::InlineHandler { spelling, statements: false, .. } if spelling == "total += $event"
    ));
    assert!(witness.render().ends_with(&format!(
        "const {b}_check0: __VerterUseListener<typeof {b}, \"onChange\"> = ($event) => (total += $event);\n",
        b = witness.binding
    )));
}

/// Every actual callable is validated against the listener contract the
/// runtime reads: an event-option key falls back to its unsuffixed key
/// instead of reaching the construction untyped, an optional member is the
/// handler itself, a `@vue:` hook outside the reserved lifecycle set is an
/// ordinary listener, a prop-spelled handler is an observed listener, and an
/// inline expression returns its value while a statement list is a block.
#[test]
fn component_use_listener_contracts_follow_the_runtime_listener_key() {
    let projection = project(concat!(
        "  <Table @change.once=\"count\" @change.capture.passive=\"(v) => v\" :onSave=\"save\"",
        " @change=\"props?.onChange\" v-on:change=\"flag === true\" @vue:foo=\"hook\"",
        " @vue:mounted=\"hook\" @close=\"a = 1; b = 2\" />",
    ));
    let witness = only(&projection);
    assert_eq!(
        member_keys(witness),
        vec![Some("onSave"), Some("onChange"), Some("onVnodeFoo")]
    );
    assert!(matches!(
        member(witness, "onChange"),
        MemberValue::Expression { spelling, .. } if spelling == "props?.onChange"
    ));
    let checks: Vec<(&str, Option<&str>, &str)> = witness
        .transaction
        .validations
        .iter()
        .map(|check| {
            assert_eq!(check.contract, CheckContract::Listener);
            (
                check.key.as_str(),
                check.fallback.as_deref(),
                match &check.value {
                    MemberValue::Expression { spelling, .. }
                    | MemberValue::InlineHandler { spelling, .. } => spelling.as_str(),
                    MemberValue::StaticText(_) => panic!("authored handler"),
                },
            )
        })
        .collect();
    assert_eq!(
        checks,
        vec![
            ("onChangeOnce", Some("onChange"), "count"),
            ("onChangeCapturePassive", Some("onChange"), "(v) => v"),
            ("onChange", None, "flag === true"),
            ("onClose", None, "a = 1; b = 2"),
        ]
    );
    let reasons: Vec<(u32, ExclusionReason)> = witness
        .transaction
        .excluded
        .iter()
        .map(|e| (e.op_index, e.reason))
        .collect();
    assert_eq!(reasons, vec![(6, ExclusionReason::Reserved)]);
    let rendered = witness.render();
    let b = &witness.binding;
    for line in [
        format!("const {b}_check0: __VerterUseListener<typeof {b}, \"onChangeOnce\", \"onChange\"> = (count);\n"),
        format!("const {b}_check1: __VerterUseListener<typeof {b}, \"onChangeCapturePassive\", \"onChange\"> = ((v) => v);\n"),
        format!("const {b}_check2: __VerterUseListener<typeof {b}, \"onChange\"> = ($event) => (flag === true);\n"),
        format!("const {b}_check3: __VerterUseListener<typeof {b}, \"onClose\"> = ($event) => {{ a = 1; b = 2; }};\n"),
    ] {
        assert!(rendered.contains(&line), "missing {line} in {rendered}");
    }
    assert!(rendered.contains("\"onChange\": (props?.onChange)"));
    assert!(!rendered.contains("\"onChangeOnce\": "));
    let events: Vec<(&str, Option<&str>)> = witness
        .observations
        .iter()
        .filter_map(|o| match &o.kind {
            ObservationKind::Event { key, fallback } => Some((key.as_str(), fallback.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(
        events,
        vec![
            ("onChangeOnce", Some("onChange")),
            ("onChangeCapturePassive", Some("onChange")),
            ("onSave", None),
            ("onChange", None),
            ("onVnodeFoo", None),
            ("onClose", None),
        ]
    );
    assert!(witness.observations.iter().any(|o| o.type_text
        == format!("__VerterUseListener<typeof {b}, \"onChangeOnce\", \"onChange\">")));
}

/// A static discriminant is a string-literal member of the construction,
/// so TypeScript keeps the literal and the matching union branch; a bound
/// literal keeps its authored expression.
#[test]
fn component_use_static_discriminant_stays_a_literal() {
    let witness_of = |template: &str| project(template).witnesses[0].clone();
    let static_kind = witness_of("  <Table :rows=\"ids\" kind=\"grid\" :columns=\"3\" />");
    assert_eq!(
        member(&static_kind, "kind"),
        &MemberValue::StaticText("grid".to_string())
    );
    assert!(static_kind
        .render()
        .contains("\"kind\": \"grid\", \"columns\": (3) });"));
    let bound_kind = witness_of("  <Table :rows=\"ids\" :kind=\"'grid'\" />");
    assert!(bound_kind.render().contains("\"kind\": ('grid')"));
    let escaped = witness_of("  <Table title='a\"b' />");
    assert!(escaped.render().contains("\"title\": \"a\\\"b\""));
}

/// A listener key accumulates: its first handler reaches the construction
/// after every spread, and each further handler is a validation check
/// against the specialized listener contract, never a second property or a
/// fabricated handler array.
#[test]
fn component_use_collected_listeners_validate_against_the_specialized_contract() {
    let projection = project(
        "  <Table @change=\"log\" v-bind=\"attrs\" :onChange=\"count\" v-on:change=\"(value) => log(value)\" />",
    );
    let witness = only(&projection);
    assert_eq!(member_keys(witness), vec![None, Some("onChange")]);
    assert!(matches!(
        member(witness, "onChange"),
        MemberValue::Expression { spelling, .. } if spelling == "log"
    ));
    let checks: Vec<(&str, CheckContract, &str)> = witness
        .transaction
        .validations
        .iter()
        .map(|check| {
            let MemberValue::Expression { spelling, .. } = &check.value else {
                panic!("authored handler")
            };
            (check.key.as_str(), check.contract, spelling.as_str())
        })
        .collect();
    assert_eq!(
        checks,
        vec![
            ("onChange", CheckContract::Listener, "count"),
            ("onChange", CheckContract::Listener, "(value) => log(value)"),
        ]
    );
    let rendered = witness.render();
    assert_eq!(rendered.matches("\"onChange\": ").count(), 1);
    assert!(!rendered.contains('['));
    assert!(rendered.contains(&format!(
        "const {b}_check0: __VerterUseListener<typeof {b}, \"onChange\"> = (count);\n",
        b = witness.binding
    )));
}

/// The specialization key is per use and offset-free: editing one use's
/// contributor changes that use's key, while an unrelated sibling keeps
/// its id, key and checking text even though every one of its source
/// offsets shifts.
#[test]
fn component_use_specialization_is_per_use_and_offset_free() {
    let before =
        project("  <Table :rows=\"a\" />\n  <Table :rows=\"ids\" :project=\"(id) => id\" />");
    let after = project(
        "  <Table :rows=\"aLongerName\" />\n  <Table :rows=\"ids\" :project=\"(id) => id\" />",
    );
    assert!(before.complete && after.complete);
    let (edited_before, edited_after) = (&before.witnesses[0], &after.witnesses[0]);
    assert_ne!(edited_before.specialization, edited_after.specialization);
    let (sibling_before, sibling_after) = (&before.witnesses[1], &after.witnesses[1]);
    assert_eq!(sibling_before.use_id, sibling_after.use_id);
    assert_eq!(sibling_before.specialization, sibling_after.specialization);
    assert_eq!(sibling_before.render(), sibling_after.render());
    // Removing a contributor of the sibling does change its key.
    let dropped = project("  <Table :rows=\"a\" />\n  <Table :rows=\"ids\" />");
    assert_ne!(
        sibling_before.specialization,
        dropped.witnesses[1].specialization
    );
}

/// Sibling uses of one component — and a dynamic `<component :is>` —
/// each own their construction and binding, so one use's specialization
/// never reaches another.
#[test]
fn component_use_sibling_uses_keep_independent_witnesses() {
    let projection = project(POSITIVE_TEMPLATE);
    assert!(projection.complete);
    let [first, second] = projection.witnesses.as_slice() else {
        panic!("two uses")
    };
    assert_ne!(first.binding, second.binding);
    assert_ne!(first.use_id, second.use_id);
    for witness in [first, second] {
        let rendered = witness.render();
        assert_eq!(constructions(&rendered), 1);
        let other = if witness.binding == first.binding {
            second
        } else {
            first
        };
        assert!(!rendered.contains(&other.binding));
    }
    assert!(member_keys(second).contains(&Some("columns")));
    assert!(!member_keys(first).contains(&Some("columns")));
    let dynamic = project("  <component :is=\"current\" :rows=\"ids\" />");
    let dynamic = only(&dynamic);
    assert_eq!(dynamic.component, "(current)");
    assert_eq!(member_keys(dynamic), vec![Some("rows")]);
    let kebab = project("  <data-table :rows=\"ids\" />");
    assert_eq!(only(&kebab).component, "DataTable");
}

/// Operations that are not authored inference contributors never reach
/// the construction, and each records why.
#[test]
fn component_use_non_contributors_are_excluded_with_reasons() {
    let projection = project(
        "  <Table title=\"a\" :title=\"t\" :[k]=\"v\" v-on=\"handlers\" :key=\"id\" .value=\"x\" v-model.trim=\"text\" v-focus />",
    );
    let witness = only(&projection);
    assert_eq!(
        member_keys(witness),
        vec![Some("title"), Some("modelValue")]
    );
    let reasons: Vec<(u32, ExclusionReason)> = witness
        .transaction
        .excluded
        .iter()
        .map(|e| (e.op_index, e.reason))
        .collect();
    for expected in [
        (1, ExclusionReason::Overridden),
        (2, ExclusionReason::DynamicKey),
        (3, ExclusionReason::ListenerObject),
        (4, ExclusionReason::Reserved),
        (5, ExclusionReason::DomBinding),
        (6, ExclusionReason::ModelUpdate),
        (6, ExclusionReason::ModelModifiers),
        (7, ExclusionReason::NoProperty),
    ] {
        assert!(
            reasons.contains(&expected),
            "missing {expected:?} in {reasons:?}"
        );
    }
    assert_eq!(reasons.len(), 8);
}

/// Products are deterministic, and a plan with an unadmitted expression or
/// a use whose component cannot be named stays incomplete.
#[test]
fn component_use_products_are_deterministic_and_incomplete_plans_stay_incomplete() {
    assert_eq!(project(POSITIVE_TEMPLATE), project(POSITIVE_TEMPLATE));
    assert!(!project("  <Table :rows=\"rows +\" />").complete);
    let unnamed = project("  <component :is=\"\" :rows=\"ids\" />");
    assert!(!unnamed.complete);
    assert!(unnamed.witnesses.is_empty());
}

#[test]
fn table_probe_fixture_is_the_rendered_declaration() {
    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP18/probes/components/Table.vue.ts"
    ));
    let contract = project_public_constructor(
        None,
        Some(ScriptBlockInput {
            content: TABLE_SETUP,
            content_start: 0,
            lang: Some(ScriptLanguage::TypeScript),
        }),
        Some(TABLE_GENERIC),
    )
    .expect("projects");
    let rendered = contract.declaration().expect("setup renders a constructor");
    assert!(
        FIXTURE.replace("\r\n", "\n").ends_with(&rendered),
        "the probe component must end with the rendered declaration:\n{rendered}"
    );
}

/// The tsc probes carry the product's own rendering: each probe contains
/// its template's rendered projection byte for byte, and every observation
/// the positive probe reads is the product's observation type.
#[test]
fn probe_fixtures_are_the_rendered_products() {
    const POSITIVE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP18/probes/positive.ts"
    ));
    const NEGATIVE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP18/probes/negative.ts"
    ));
    let positive = project(POSITIVE_TEMPLATE);
    let fixture = POSITIVE.replace("\r\n", "\n");
    assert!(
        fixture.contains(&positive.render()),
        "positive probe must contain the rendered projection:\n{}",
        positive.render()
    );
    for observation in &positive.witnesses[0].observations {
        assert!(
            fixture.contains(&observation.type_text),
            "positive probe must read {}",
            observation.type_text
        );
    }
    let negative = project(NEGATIVE_TEMPLATE);
    assert!(
        NEGATIVE.replace("\r\n", "\n").contains(&negative.render()),
        "negative probe must contain the rendered projection:\n{}",
        negative.render()
    );
}
