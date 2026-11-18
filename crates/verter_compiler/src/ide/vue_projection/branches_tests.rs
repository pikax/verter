//! Unit tests for the branches module.

use verter_audit::observe::Uncaptured;

use super::branches::*;
use crate::framework_common::projection_plan::{
    build_projection_plan, AdmittedExpressionId, PlanInput, ProjectionPlan,
};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, TemplateBlockId};
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

/// The plan and the branch projection of one carrier, both from one parse.
fn project(source: &str) -> (ProjectionPlan, TemplateBranchProjection) {
    let parsed = parse_sfc(source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Branches.vue",
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let ast = parsed.template_ast().expect("the carrier has a template");
    let topology = build_template_topology(
        source,
        ast,
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("the topology builds");
    let branches = project_template_branches(&plan, &topology);
    (plan, branches)
}

fn spelling(plan: &ProjectionPlan, id: &AdmittedExpressionId) -> String {
    plan.expression(id).expect("admitted").spelling.clone()
}

/// The guards on the path into `block`, as `+cond` (holds) or `-cond`
/// (fails), in path order.
fn guards(plan: &ProjectionPlan, branches: &TemplateBranchProjection, block: u32) -> Vec<String> {
    branches
        .path(TemplateBlockId(block))
        .expect("block has a path")
        .steps
        .iter()
        .map(|step| {
            let sign = match step.polarity {
                GuardPolarity::Holds => '+',
                GuardPolarity::Fails => '-',
            };
            format!("{sign}{}", spelling(plan, &step.expression))
        })
        .collect()
}

fn flat(source: &str, plan: &ProjectionPlan, fragment: &CheckingFragment) -> String {
    fragment.flatten(source, &|id| spelling(plan, id))
}

const CHAIN: &str = concat!(
    "<template>\n",
    "  <p>{{ top }}</p>\n",
    "  <div v-if=\"user\">\n",
    "    <span v-if=\"user.name\">{{ user.name.length }}</span>\n",
    "    <span v-else>{{ fallback }}</span>\n",
    "  </div>\n",
    "  <p v-else-if=\"kind === 'b'\">{{ kind }}</p>\n",
    "  <p v-else>{{ other }}</p>\n",
    "</template>\n",
);

#[test]
fn blocks_follow_the_authored_nesting_and_order() {
    let (plan, branches) = project(CHAIN);
    assert!(branches.complete);
    let root = branches.root().expect("root block");
    assert_eq!(root.kind, TemplateBlockKind::Root);
    let shape: Vec<String> = branches
        .blocks
        .iter()
        .map(|block| {
            let items: Vec<String> = block
                .items
                .iter()
                .map(|item| match item {
                    TemplateItem::Interpolation(id) => spelling(&plan, id),
                    TemplateItem::Block(child) => format!("#{}", child.0),
                    other => format!("{other:?}"),
                })
                .collect();
            format!(
                "{}<{:?}>[{}]",
                block.id.0,
                block.parent.map(|p| p.0),
                items.join(", ")
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            "0<None>[top, #1, #4, #5]",
            "1<Some(0)>[#2, #3]",
            "2<Some(1)>[user.name.length]",
            "3<Some(1)>[fallback]",
            "4<Some(0)>[kind]",
            "5<Some(0)>[other]",
        ]
    );
    for block in &branches.blocks[1..] {
        assert_eq!(block.lexical_env, root.lexical_env, "arms open no scope");
    }
}

#[test]
fn a_nested_else_is_dominated_by_its_ancestor_guard_and_its_failed_sibling() {
    let (plan, branches) = project(CHAIN);
    assert_eq!(guards(&plan, &branches, 0), Vec::<String>::new());
    assert_eq!(guards(&plan, &branches, 2), vec!["+user", "+user.name"]);
    assert_eq!(guards(&plan, &branches, 3), vec!["+user", "-user.name"]);
}

#[test]
fn else_if_and_else_keep_every_preceding_false_branch_in_authored_order() {
    let (plan, branches) = project(CHAIN);
    assert_eq!(guards(&plan, &branches, 4), vec!["-user", "+kind === 'b'"]);
    assert_eq!(guards(&plan, &branches, 5), vec!["-user", "-kind === 'b'"]);
}

#[test]
fn arm_heads_open_if_else_if_and_else_with_each_condition_once() {
    let (plan, branches) = project(CHAIN);
    let open = |block: u32| {
        let head = branches
            .arm_head(TemplateBlockId(block))
            .expect("an arm has a head");
        assert!(head.before.is_empty());
        flat(CHAIN, &plan, &head.open)
    };
    assert_eq!(open(1), "if (user) {\n");
    assert_eq!(open(4), "else if (kind === 'b') {\n");
    assert_eq!(open(5), "else {\n");
    assert_eq!(branches.arm_head(TemplateBlockId(0)), None);
}

#[test]
fn interpolations_are_placed_at_their_items() {
    let (plan, branches) = project(CHAIN);
    let placed: Vec<String> = branches
        .statements()
        .iter()
        .map(|statement| flat(CHAIN, &plan, &statement.fragment))
        .collect();
    assert_eq!(
        placed,
        vec![
            "(top);\n",
            "(user.name.length);\n",
            "(fallback);\n",
            "(kind);\n",
            "(other);\n"
        ]
    );
    let other = branches.blocks[5].items[0].clone();
    assert_eq!(branches.place_of(&other), Some((TemplateBlockId(5), 0)));
}

#[test]
fn a_comment_between_arms_keeps_one_chain() {
    let source = concat!(
        "<template>\n  <div>\n",
        "    <p v-if=\"kind === 'a'\">a</p>\n",
        "    <!-- note -->\n",
        "    <p v-else-if=\"kind === 'b'\">b</p>\n",
        "    <p v-else>c</p>\n",
        "  </div>\n</template>\n",
    );
    let (plan, branches) = project(source);
    assert_eq!(
        guards(&plan, &branches, 3),
        vec!["-kind === 'a'", "-kind === 'b'"]
    );
}

#[test]
fn a_chain_that_starts_at_an_else_if_opens_with_if() {
    let source = concat!(
        "<template>\n  <article>\n",
        "    <Detail v-else-if=\"item.kind === 'main'\" :item=\"item\" />\n",
        "  </article>\n</template>\n",
    );
    let (plan, branches) = project(source);
    let arm = branches
        .blocks
        .iter()
        .find(|block| matches!(block.kind, TemplateBlockKind::Arm { .. }))
        .expect("the arm is a block");
    let head = branches.arm_head(arm.id).expect("head");
    assert_eq!(
        flat(source, &plan, &head.open),
        "if (item.kind === 'main') {\n"
    );
}

#[test]
fn loops_and_slot_bodies_are_blocks_with_their_own_scope() {
    let source = concat!(
        "<template>\n",
        "  <li v-for=\"item in items\">{{ item }}</li>\n",
        "  <Child><template #row=\"{ row }\">{{ row }}</template></Child>\n",
        "</template>\n",
    );
    let (plan, branches) = project(source);
    let root = branches.root().expect("root");
    let kinds: Vec<&TemplateBlockKind> = branches.blocks.iter().map(|block| &block.kind).collect();
    assert!(matches!(
        kinds[1],
        TemplateBlockKind::Loop { source } if spelling(&plan, source) == "items"
    ));
    assert!(matches!(
        kinds[2],
        TemplateBlockKind::SlotBody { slot: 0, .. }
    ));
    assert_ne!(branches.blocks[1].lexical_env, root.lexical_env);
    assert_ne!(branches.blocks[2].lexical_env, root.lexical_env);
    assert_eq!(guards(&plan, &branches, 1), Vec::<String>::new());
}

#[test]
fn the_pending_value_is_distinct_from_an_empty_template() {
    let (plan, branches) = project("<template><p>static</p></template>\n");
    assert!(branches.blocks.is_empty() && branches.complete);
    assert!(!TemplateBranchProjection::pending(plan.snapshot.clone()).complete);
}

#[test]
fn runtime_root_inventory_keeps_nested_native_children_inside_one_root() {
    let (_, branches) = project("<template><div><span/></div></template>");
    let nodes = branches.runtime_roots();
    assert_eq!(nodes[0].children.len(), 1);
    let div = &nodes[nodes[0].children[0]];
    assert_eq!(
        div.native.as_ref().map(|(tag, _)| tag.as_str()),
        Some("div")
    );
    assert!(div.children.is_empty());
    assert_eq!(nodes.len(), 2);
}

#[test]
fn runtime_root_inventory_distinguishes_flat_native_siblings() {
    let (_, branches) = project("<template><div/><span/></template>");
    let nodes = branches.runtime_roots();
    assert_eq!(nodes[0].children.len(), 2);
    assert!(nodes[0]
        .children
        .iter()
        .all(|&index| nodes[index].native.is_some()));
}

#[test]
fn runtime_root_inventory_retains_significant_text_and_comments() {
    use crate::template::topology::VueTemplateNodeKind;
    let (_, branches) = project("<template>  <!-- note --><div/>significant text</template>");
    let nodes = branches.runtime_roots();
    let kinds: Vec<_> = nodes[0]
        .children
        .iter()
        .map(|&index| nodes[index].kind)
        .collect();
    assert_eq!(
        kinds,
        [
            VueTemplateNodeKind::Comment,
            VueTemplateNodeKind::Element,
            VueTemplateNodeKind::Text
        ]
    );
}
