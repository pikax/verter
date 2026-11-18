//! Element uses admitted by the projection plan.

use super::*;

fn sfc(template: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nconst x = 1;\n</script>\n<template>\n{template}</template>\n"
    )
}

fn complete_plan(source: &str) -> ProjectionPlan {
    let plan = plan_from_source("file:///elements.vue", source);
    assert!(plan.is_complete(), "{:?}", plan.completeness);
    plan
}

fn native_tags(plan: &ProjectionPlan) -> Vec<&str> {
    plan.elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementUseKind::Native { tag, .. } => Some(tag.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn element_without_checked_operations_is_not_admitted() {
    let plan = complete_plan(&sfc("  <div><span>hi</span></div>\n"));
    assert!(plan.elements.is_empty());
}

#[test]
fn native_element_is_admitted_with_its_tag_span_and_operations() {
    let source = sfc("  <button id=\"go\" :disabled=\"flag\" @click=\"run\">go</button>\n");
    let plan = complete_plan(&source);
    assert_eq!(plan.elements.len(), 1);
    let element = &plan.elements[0];
    assert_eq!(
        element.kind,
        ElementUseKind::Native {
            tag: "button".to_string(),
            namespace: ElementNamespace::Html,
        }
    );
    assert_eq!(
        &source[element.tag_start as usize..element.tag_end as usize],
        "button"
    );
    assert_eq!(
        plan.element(&element.id).map(|found| found.id.clone()),
        Some(element.id.clone())
    );
}

#[test]
fn operations_follow_authored_order() {
    let plan = complete_plan(&sfc(
        "  <input type=\"text\" :value=\"a\" v-bind=\"rest\" class=\"c\" />\n",
    ));
    let names: Vec<(AttributeOpKind, &str)> = plan.elements[0]
        .operations
        .iter()
        .map(|op| (op.kind, op.name.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            (AttributeOpKind::Static, "type"),
            (AttributeOpKind::Bound, "value"),
            (AttributeOpKind::VBind, "v-bind"),
            (AttributeOpKind::Static, "class"),
        ]
    );
    let indices: Vec<u32> = plan.elements[0]
        .operations
        .iter()
        .map(|op| op.index)
        .collect();
    assert_eq!(indices, vec![0, 1, 2, 3]);
}

#[test]
fn slot_outlet_and_directive_template_are_admitted() {
    let plan = complete_plan(&sfc(concat!(
        "  <slot name=\"head\" :row=\"x\" />\n",
        "  <template v-bind=\"attrs\"><b class=\"k\">t</b></template>\n",
    )));
    let kinds: Vec<&ElementUseKind> = plan.elements.iter().map(|element| &element.kind).collect();
    assert!(matches!(kinds[0], ElementUseKind::SlotOutlet));
    assert!(matches!(kinds[1], ElementUseKind::TemplateWrapper));
    assert_eq!(native_tags(&plan), vec!["b"]);
}

#[test]
fn slot_body_template_is_not_an_element_use() {
    let plan = complete_plan(&sfc(
        "  <Comp><template #head=\"{ row }\">{{ row }}</template></Comp>\n",
    ));
    assert!(plan.elements.is_empty());
    assert_eq!(plan.uses.len(), 1);
}

#[test]
fn loop_and_condition_directives_alone_do_not_admit_a_template() {
    let plan = complete_plan(&sfc(
        "  <template v-if=\"x\"><p v-for=\"n in 3\">{{ n }}</p></template>\n",
    ));
    assert!(plan.elements.is_empty());
}

#[test]
fn nothing_inside_v_pre_is_admitted() {
    let plan = complete_plan(&sfc(
        "  <div v-pre><span :a=\"b\" id=\"x\">{{ y }}</span></div>\n  <i id=\"after\"></i>\n",
    ));
    assert_eq!(native_tags(&plan), vec!["i"]);
}

#[test]
fn namespace_follows_the_enclosing_svg_or_math_element() {
    let plan = complete_plan(&sfc(concat!(
        "  <svg width=\"1\"><a href=\"#\"><rect width=\"2\" /></a></svg>\n",
        "  <math display=\"block\"><mi id=\"m\"></mi></math>\n",
        "  <a href=\"#\"></a>\n",
    )));
    let namespaces: Vec<(&str, ElementNamespace)> = plan
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementUseKind::Native { tag, namespace } => Some((tag.as_str(), *namespace)),
            _ => None,
        })
        .collect();
    assert_eq!(
        namespaces,
        vec![
            ("svg", ElementNamespace::Svg),
            ("a", ElementNamespace::Svg),
            ("rect", ElementNamespace::Svg),
            ("math", ElementNamespace::MathMl),
            ("mi", ElementNamespace::MathMl),
            ("a", ElementNamespace::Html),
        ]
    );
}

#[test]
fn identical_elements_get_distinct_ids() {
    let plan = complete_plan(&sfc("  <p class=\"a\"></p>\n  <p class=\"a\"></p>\n"));
    assert_eq!(plan.elements.len(), 2);
    assert_ne!(plan.elements[0].id, plan.elements[1].id);
}

#[test]
fn element_ids_are_stable_under_edits_elsewhere() {
    let base = "  <p class=\"a\"></p>\n  <section><em :t=\"x\"></em></section>\n";
    let edited = format!("  <h1 id=\"new\"></h1>\n  {{{{ x }}}}\n{base}");
    let before = complete_plan(&sfc(base));
    let after = complete_plan(&sfc(&edited));
    let ids = |plan: &ProjectionPlan, tag: &str| -> Vec<ElementUseId> {
        plan.elements
            .iter()
            .filter(|element| {
                matches!(&element.kind, ElementUseKind::Native { tag: t, .. } if t == tag)
            })
            .map(|element| element.id.clone())
            .collect()
    };
    assert_eq!(ids(&before, "p"), ids(&after, "p"));
    assert_eq!(ids(&before, "em"), ids(&after, "em"));
}

#[test]
fn component_uses_stay_out_of_the_element_population() {
    let plan = complete_plan(&sfc(
        "  <Comp id=\"c\" :a=\"x\" /><Transition name=\"f\"><div class=\"d\" /></Transition>\n",
    ));
    assert_eq!(plan.uses.len(), 2);
    assert_eq!(native_tags(&plan), vec!["div"]);
}

#[test]
fn element_expressions_are_admitted_and_refer_to_the_context() {
    let source = sfc("  <a :href=\"base + path\">x</a>\n");
    let plan = complete_plan(&source);
    let value = plan.elements[0].operations[0]
        .expression
        .as_ref()
        .expect("value");
    let occurrence = plan.expression(value).expect("occurrence");
    assert_eq!(occurrence.spelling, "base + path");
    assert_eq!(occurrence.lexical_env, plan.elements[0].lexical_env);
    let referenced: Vec<&str> = plan
        .context_references(value)
        .iter()
        .map(|r| &source[r.start as usize..r.end as usize])
        .collect();
    assert_eq!(referenced, vec!["base", "path"]);
}

#[test]
fn unparsable_element_expression_keeps_the_plan_incomplete() {
    let plan = plan_from_source("file:///bad.vue", &sfc("  <a :href=\"base +\">x</a>\n"));
    assert!(!plan.is_complete());
}

#[test]
fn element_static_ref_is_recorded() {
    let plan = complete_plan(&sfc("  <canvas ref=\"stage\" width=\"3\"></canvas>\n"));
    assert_eq!(plan.elements[0].static_ref.as_deref(), Some("stage"));
}

#[test]
fn ref_only_elements_keep_static_names_and_admitted_dynamic_references() {
    let source = sfc("<svg><a ref=\"link\" /><circle :ref=\"el => accept(el)\" /></svg>");
    let plan = complete_plan(&source);
    assert_eq!(native_tags(&plan), ["a", "circle"]);
    assert_eq!(plan.elements[0].static_ref.as_deref(), Some("link"));
    assert!(plan.elements[1].static_ref.is_none());
    let op = &plan.elements[1].operations[0];
    assert_eq!(op.name, "ref");
    assert_eq!(op.kind, AttributeOpKind::Bound);
    let value = op.expression.as_ref().expect("dynamic ref is admitted");
    let occurrence = plan.expression(value).unwrap();
    assert_eq!(occurrence.spelling, "el => accept(el)");
    assert_eq!(
        &source[occurrence.start as usize..occurrence.end as usize],
        occurrence.spelling
    );
    let references: Vec<_> = plan
        .context_references(value)
        .iter()
        .map(|reference| &source[reference.start as usize..reference.end as usize])
        .collect();
    assert_eq!(references, ["accept"]);
}

#[test]
fn element_in_a_loop_records_the_loop_environment() {
    let plan = complete_plan(&sfc(
        "  <ul><li v-for=\"n in 3\" :k=\"n\"></li></ul>\n  <li :k=\"x\"></li>\n",
    ));
    assert_eq!(plan.elements.len(), 2);
    assert_ne!(plan.elements[0].lexical_env, plan.elements[1].lexical_env);
}

#[test]
fn element_ids_in_a_loop_survive_an_identical_loop_inserted_before_it() {
    let original = "  <div v-for=\"n in rows\"><input :value=\"n.a\" /></div>
";
    let edited = format!(
        "  <div v-for=\"n in rows\"><input :value=\"n.b\" /></div>
{original}"
    );
    let before = complete_plan(&sfc(original));
    let after = complete_plan(&sfc(&edited));
    let surviving = &before.elements[0];
    assert!(after.element(&surviving.id).is_some());
}

fn plan_with_custom_elements(source: &str, prefixes: &[String]) -> ProjectionPlan {
    let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, Some(prefixes));
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///custom.vue",
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: Some(prefixes),
    });
    assert!(plan.is_complete(), "{:?}", plan.completeness);
    plan
}

#[test]
fn tag_matching_the_custom_element_policy_is_a_custom_element_use() {
    let source = sfc("  <my-widget size=\"2\" :open=\"x\" /><ion-card a=\"1\" />\n");
    let plan = plan_with_custom_elements(&source, &["my-".to_string()]);
    assert_eq!(plan.elements.len(), 1);
    let element = &plan.elements[0];
    assert_eq!(
        element.kind,
        ElementUseKind::CustomElement {
            tag: "my-widget".to_string()
        }
    );
    assert_eq!(
        &source[element.tag_start as usize..element.tag_end as usize],
        "my-widget"
    );
    assert_eq!(element.operations.len(), 2);
    assert_eq!(
        plan.uses.len(),
        1,
        "a tag outside the policy stays a component use"
    );
}

#[test]
fn without_a_policy_a_dashed_tag_is_a_component_use() {
    let plan = complete_plan(&sfc("  <my-widget size=\"2\" />\n"));
    assert_eq!(plan.uses.len(), 1);
    assert!(plan.elements.is_empty());
}
