//! Context references and parser-node facts recorded by the projection plan.

use super::*;
use crate::ast::types::AstNodeKind;

fn sfc(template: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nconst x = 1;\n</script>\n<template>\n{template}</template>\n"
    )
}

fn complete_plan(source: &str) -> ProjectionPlan {
    let plan = plan_from_source("file:///context.vue", source);
    assert!(plan.is_complete(), "{:?}", plan.completeness);
    plan
}

/// The referenced source text (and shorthand flag) of every occurrence of
/// `kind`, in admission order.
fn references_of<'s>(
    source: &'s str,
    plan: &ProjectionPlan,
    kind: ExpressionKind,
) -> Vec<Vec<(&'s str, bool)>> {
    plan.expressions()
        .iter()
        .filter(|occurrence| occurrence.kind == kind)
        .map(|occurrence| {
            plan.context_references(&occurrence.id)
                .iter()
                .map(|r| (&source[r.start as usize..r.end as usize], r.shorthand))
                .collect()
        })
        .collect()
}

fn references_of_spelling<'s>(
    source: &'s str,
    plan: &ProjectionPlan,
    spelling: &str,
) -> Vec<(&'s str, bool)> {
    let occurrence = plan
        .expressions()
        .iter()
        .find(|occurrence| occurrence.spelling == spelling)
        .unwrap_or_else(|| panic!("no occurrence spelled {spelling:?}"));
    plan.context_references(&occurrence.id)
        .iter()
        .map(|r| (&source[r.start as usize..r.end as usize], r.shorthand))
        .collect()
}

#[test]
fn shorthand_property_reference_is_marked_shorthand() {
    let source = sfc("  {{ t('k', { login, count: total }) }}\n");
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::Interpolation),
        vec![vec![("t", false), ("login", true), ("total", false)]]
    );
}

#[test]
fn loop_alias_and_slot_parameter_are_bound_only_inside_their_scope() {
    let source = sfc(concat!(
        "  <li v-for=\"item in items\">{{ item.name }}</li>\n",
        "  {{ item }}\n",
        "  <Comp v-slot=\"{ row }\">{{ row }}</Comp>\n",
        "  {{ row }}\n",
    ));
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::Interpolation),
        vec![vec![], vec![("item", false)], vec![], vec![("row", false)]]
    );
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::VForSource),
        vec![vec![("items", false)]]
    );
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::SlotParams),
        vec![vec![]]
    );
}

#[test]
fn event_in_a_handler_is_not_referenced() {
    let source = sfc(concat!(
        "  <Comp @update=\"value = $event\" />\n",
        "  <Comp @change=\"last = $event; flush()\" />\n",
    ));
    let plan = complete_plan(&source);
    assert_eq!(
        references_of_spelling(&source, &plan, "value = $event"),
        vec![("value", false)]
    );
    assert_eq!(
        references_of_spelling(&source, &plan, "last = $event; flush()"),
        vec![("last", false), ("flush", false)]
    );
}

#[test]
fn allowed_globals_and_keywords_are_not_referenced() {
    let source = sfc("  {{ Math.max(a, undefined) }}\n");
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::Interpolation),
        vec![vec![("a", false)]]
    );
}

#[test]
fn a_member_chain_references_only_its_root() {
    let source = sfc(concat!(
        "  {{ user.profile.name }}\n",
        "  {{ table[key].value }}\n",
    ));
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::Interpolation),
        vec![
            vec![("user", false)],
            vec![("table", false), ("key", false)]
        ]
    );
}

#[test]
fn valueless_shorthand_references_its_argument_unless_a_loop_binds_it() {
    let source = sfc(concat!(
        "  <Comp :user-name />\n",
        "  <Comp v-for=\"title in titles\" :title />\n",
        "  <Comp :undefined />\n",
        "  <Comp :Math />\n",
    ));
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::AttributeValue),
        vec![vec![("user-name", false)], vec![], vec![], vec![]]
    );
}

#[test]
fn dynamic_slot_name_and_argument_reference_the_outer_scope() {
    let source = sfc(concat!(
        "  <Comp :[prop]=\"value\">\n",
        "    <template #[slotName]=\"{ a }\">{{ a }}</template>\n",
        "  </Comp>\n",
    ));
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::AttributeValue),
        vec![
            vec![("prop", false)],
            vec![("value", false)],
            vec![("slotName", false)],
        ]
    );
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::Interpolation),
        vec![vec![]]
    );
}

#[test]
fn loop_aliases_are_admitted_per_position_in_the_loop_scope() {
    let source = sfc(concat!(
        "  <li v-for=\"(item, index) in items\" :key=\"index\">{{ item }}</li>\n",
        "  <li v-for=\"({ id, label }, i) in rows\">{{ label }}</li>\n",
    ));
    let plan = complete_plan(&source);
    let aliases: Vec<&ExpressionOccurrence> = plan
        .expressions()
        .iter()
        .filter(|occurrence| occurrence.kind == ExpressionKind::VForAlias)
        .collect();
    assert_eq!(
        aliases
            .iter()
            .map(|alias| alias.spelling.as_str())
            .collect::<Vec<_>>(),
        vec!["item", "index", "{ id, label }", "i"]
    );
    for alias in &aliases {
        assert_eq!(
            &source[alias.start as usize..alias.end as usize],
            alias.spelling
        );
        assert!(plan.context_references(&alias.id).is_empty());
    }
    let item_scope = &plan
        .origins
        .iter()
        .find(|origin| origin.name == "item")
        .expect("item origin")
        .lexical_env;
    assert_eq!(&aliases[0].lexical_env, item_scope);
    assert_eq!(&aliases[1].lexical_env, item_scope);
    assert_ne!(&aliases[2].lexical_env, item_scope);
}

#[test]
fn every_admission_records_the_parser_node_that_carries_it() {
    let source = sfc(concat!(
        "  <div v-if=\"ready\">\n",
        "    <Comp v-for=\"row in rows\" :value=\"row\" @save=\"store(row)\">\n",
        "      <template #[slotName]=\"{ a }\">{{ a }}</template>\n",
        "    </Comp>\n",
        "  </div>\n",
        "  {{ x }}\n",
    ));
    let plan = complete_plan(&source);
    let parsed = crate::framework_common::vue_bridge::parse_sfc(&source, None, None);
    let ast = parsed.template_ast().expect("template");
    assert!(!plan.expressions().is_empty());
    for occurrence in plan.expressions() {
        let node = ast
            .nodes
            .get(occurrence.syntax_node as usize)
            .unwrap_or_else(|| panic!("{occurrence:?} names no parser node"));
        let (start, end) = match &node.kind {
            AstNodeKind::Element(el) => (el.tag_open.start, el.tag_open.end),
            AstNodeKind::Interpolation(interp) => (interp.inner_start, interp.inner_end),
            other => panic!("{occurrence:?} names {other:?}"),
        };
        assert!(
            start <= occurrence.start && occurrence.end <= end,
            "{occurrence:?} lies outside its parser node {start}..{end}"
        );
    }
    for component in &plan.uses {
        let AstNodeKind::Element(el) = &ast.nodes[component.syntax_node as usize].kind else {
            panic!("{component:?} names a non-element");
        };
        assert_eq!(open_tag_name(&el.tag_open, &source), "Comp");
    }
}

#[test]
fn parser_node_and_static_ref_are_not_part_of_identity() {
    let with_ref = sfc("  <Comp ref=\"panel\" :value=\"x\" />\n  {{ x }}\n");
    let shifted = sfc("  <div />\n  <Comp :value=\"x\" />\n  {{ x }}\n");
    let base = complete_plan(&with_ref);
    let moved = complete_plan(&shifted);
    assert_eq!(base.uses[0].static_ref.as_deref(), Some("panel"));
    assert_eq!(moved.uses[0].static_ref, None);
    assert_ne!(base.uses[0].syntax_node, moved.uses[0].syntax_node);
    assert_eq!(base.uses[0].id, moved.uses[0].id);
    let ids = |plan: &ProjectionPlan| {
        plan.expressions()
            .iter()
            .map(|occurrence| (occurrence.id.clone(), occurrence.syntax_node))
            .collect::<Vec<_>>()
    };
    let (base_ids, moved_ids) = (ids(&base), ids(&moved));
    assert_eq!(base_ids.len(), moved_ids.len());
    for ((base_id, base_node), (moved_id, moved_node)) in base_ids.iter().zip(&moved_ids) {
        assert_eq!(base_id, moved_id);
        assert_ne!(base_node, moved_node);
    }
}

#[test]
fn an_id_the_plan_did_not_admit_has_no_references() {
    let plan = complete_plan(&sfc("  {{ a }}\n"));
    let other = complete_plan(&sfc("  {{ b }}\n"));
    let foreign = &other.expressions()[0].id;
    assert!(plan.expression(foreign).is_none());
    assert!(plan.context_references(foreign).is_empty());
}

#[test]
fn a_loop_source_references_its_free_reads_in_the_enclosing_scope() {
    let source = sfc(concat!(
        "  <li v-for=\"x in list.filter(i => i.id === selected)\">{{ x }}</li>\n",
        "  <li v-for=\"n in [...a, ...b]\">{{ n }}</li>\n",
        "  <li v-for=\"m in a.filter(limit => limit > 0).map(v => v < limit)\">{{ m }}</li>\n",
        "  <li v-for=\"e in [{ a }]\">{{ e }}</li>\n",
        "  <p v-for=\"row in rows\"><b v-for=\"c in row.filter(y => y < limit)\">{{ c }}</b></p>\n",
        "  <li v-for=\"item in item\">{{ item }}</li>\n",
    ));
    let plan = complete_plan(&source);
    assert_eq!(
        references_of(&source, &plan, ExpressionKind::VForSource),
        vec![
            vec![("list", false), ("selected", false)],
            vec![("a", false), ("b", false)],
            vec![("a", false), ("limit", false)],
            vec![("a", true)],
            vec![("rows", false)],
            vec![("limit", false)],
            vec![("item", false)],
        ]
    );
}
