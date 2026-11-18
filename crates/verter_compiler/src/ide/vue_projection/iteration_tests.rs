//! Unit tests for the iteration module.

use verter_audit::observe::Uncaptured;

use super::branches::{TemplateBlockKind, TemplateBranchProjection};
use super::iteration::*;
use crate::framework_common::projection_plan::{
    build_projection_plan, ExpressionKind, PlanInput, ProjectionPlan,
};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::ide::vue_projection::branches::project_template_branches;
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, TemplateBlockId};
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

/// The plan, the branch projection and the iteration projection of one
/// carrier, all from one parse.
fn project(
    source: &str,
) -> (
    ProjectionPlan,
    TemplateBranchProjection,
    IterationProjection,
) {
    let parsed = parse_sfc(source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Loops.vue",
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
    let iteration = project_iteration(&plan, &branches);
    (plan, branches, iteration)
}

fn flat(source: &str, plan: &ProjectionPlan, fragment: &CheckingFragment) -> String {
    fragment.flatten(source, &|id| {
        plan.expression(id).expect("admitted").spelling.clone()
    })
}

/// The loop blocks, outermost first.
fn loops(branches: &TemplateBranchProjection) -> Vec<TemplateBlockId> {
    branches
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, TemplateBlockKind::Loop { .. }))
        .map(|block| block.id)
        .collect()
}

const LIST: &str = concat!(
    "<template>\n",
    "  <ul>\n",
    "    <li v-for=\"(item, index) in items\" :key=\"item.id\">{{ item.name }} {{ index + 1 }}</li>\n",
    "  </ul>\n",
    "  <p>{{ item }}</p>\n",
    "</template>\n",
);

#[test]
fn a_loop_checks_its_source_before_its_aliases_exist() {
    let (plan, branches, iteration) = project(LIST);
    assert!(iteration.complete && !iteration.pending);
    let [block] = loops(&branches)[..] else {
        panic!("one loop block");
    };
    let head = iteration.loop_head(block).expect("the loop has a head");
    let binding = format!("__VerterEach{}", block.0);
    assert_eq!(
        flat(LIST, &plan, &head.before),
        format!("const {binding} = __VerterIterate((items));\n")
    );
    assert_eq!(
        flat(LIST, &plan, &head.open),
        format!("for (const [item, index] of {binding}) {{\n")
    );
    assert_eq!(
        iteration.contract(block).expect("contract").source_binding,
        binding
    );
}

#[test]
fn a_loop_without_a_key_or_index_binds_the_value_only() {
    let source = "<template>\n  <i v-for=\"n in 3\">{{ n }}</i>\n</template>\n";
    let (plan, branches, iteration) = project(source);
    let [block] = loops(&branches)[..] else {
        panic!("one loop block");
    };
    let contract = iteration.contract(block).expect("contract");
    assert!(contract.value.is_some() && contract.key.is_none() && contract.index.is_none());
    let head = iteration.loop_head(block).expect("head");
    assert_eq!(
        flat(source, &plan, &head.open),
        format!("for (const [n] of __VerterEach{}) {{\n", block.0)
    );
}

#[test]
fn a_loop_scope_is_a_block_of_its_own_and_outer_names_come_back() {
    let (_, branches, iteration) = project(LIST);
    let [block] = loops(&branches)[..] else {
        panic!("one loop block");
    };
    let scope = iteration
        .scopes
        .iter()
        .find(|scope| scope.block == block)
        .expect("the loop opens a scope");
    let root = branches.root().expect("root");
    let looped = branches.block(block).expect("loop block");
    assert_eq!(scope.lexical_env, looped.lexical_env);
    assert_ne!(
        scope.lexical_env, root.lexical_env,
        "the template after the loop reads the outer scope again"
    );
    assert_eq!(scope.binders.len(), 2);
}

#[test]
fn a_source_naming_the_outer_alias_is_read_outside_the_inner_loop() {
    let source = concat!(
        "<template>\n",
        "  <div v-for=\"item in items\">\n",
        "    <span v-for=\"item in item.children\">{{ item.name }}</span>\n",
        "  </div>\n",
        "</template>\n",
    );
    let (plan, branches, iteration) = project(source);
    assert!(iteration.complete);
    let [outer, inner] = loops(&branches)[..] else {
        panic!("two loop blocks");
    };
    let outer_env = &branches.block(outer).expect("outer").lexical_env;
    let inner_env = &branches.block(inner).expect("inner").lexical_env;
    assert_ne!(outer_env, inner_env);
    let contract = iteration.contract(inner).expect("inner contract");
    let source_env = &plan
        .expression(&contract.source)
        .expect("source")
        .lexical_env;
    assert_eq!(source_env, outer_env, "the source reads the outer alias");
    let alias_env = &plan
        .expression(contract.value.as_ref().expect("alias"))
        .expect("alias occurrence")
        .lexical_env;
    assert_eq!(alias_env, inner_env);
    let head = iteration.loop_head(inner).expect("head");
    assert!(
        !head.before.is_empty(),
        "the source is held before the loop"
    );
}

#[test]
fn every_name_an_alias_pattern_binds_has_an_origin_and_a_range() {
    let source = concat!(
        "<template>\n",
        "  <li v-for=\"({ id, meta: { tag }, ...rest }, [first, second = 2]) in rows\">{{ id }}</li>\n",
        "</template>\n",
    );
    let (plan, branches, iteration) = project(source);
    assert!(iteration.complete, "the pattern is admitted");
    let [block] = loops(&branches)[..] else {
        panic!("one loop block");
    };
    let origins = iteration
        .origins
        .iter()
        .find(|origins| origins.block == block)
        .expect("origins of the loop");
    let names: Vec<(&str, IterationPosition)> = origins
        .bindings
        .iter()
        .map(|binding| (binding.name.as_str(), binding.position))
        .collect();
    assert_eq!(
        names,
        [
            ("id", IterationPosition::Value),
            ("tag", IterationPosition::Value),
            ("rest", IterationPosition::Value),
            ("first", IterationPosition::Key),
            ("second", IterationPosition::Key),
        ]
    );
    for binding in &origins.bindings {
        assert_eq!(
            &source[binding.range.start as usize..binding.range.end as usize],
            binding.name,
            "the range is the authored name"
        );
        assert!(
            plan.origins
                .iter()
                .any(|origin| origin.id == binding.origin),
            "the origin is the plan's"
        );
    }
    let scope = iteration
        .scopes
        .iter()
        .find(|scope| scope.block == block)
        .expect("scope");
    assert_eq!(scope.binders.len(), origins.bindings.len());
}

#[test]
fn a_same_node_condition_and_the_key_are_placed_by_their_scope() {
    let source = concat!(
        "<template>\n",
        "  <li v-for=\"item in items\" v-if=\"item.visible\" :key=\"item.id\">x</li>\n",
        "</template>\n",
    );
    let (plan, branches, _) = project(source);
    let [block] = loops(&branches)[..] else {
        panic!("one loop block");
    };
    let loop_env = &branches.block(block).expect("loop").lexical_env;
    let env_of = |spelling: &str, kind: ExpressionKind| {
        plan.expressions()
            .iter()
            .find(|occurrence| occurrence.kind == kind && occurrence.spelling == spelling)
            .map(|occurrence| occurrence.lexical_env.clone())
            .unwrap_or_else(|| panic!("`{spelling}` is admitted"))
    };
    assert_ne!(
        &env_of("item.visible", ExpressionKind::BranchCondition),
        loop_env,
        "the condition runs before the loop's aliases exist"
    );
    assert_eq!(
        &env_of("item.id", ExpressionKind::AttributeValue),
        loop_env,
        "the key reads the alias"
    );
}

#[test]
fn the_iteration_prelude_is_declared_only_when_a_loop_is() {
    let (_, _, with_loop) = project(LIST);
    assert_eq!(with_loop.preludes(), vec![ITERATION_PRELUDE]);
    let (_, _, without) = project("<template>\n  <p>{{ a }}</p>\n</template>\n");
    assert!(without.preludes().is_empty());
    assert!(without.contracts.is_empty() && without.complete && !without.pending);
}

#[test]
fn the_prelude_has_one_overload_per_source_kind_and_none_for_maybe_nullish() {
    let text = ITERATION_PRELUDE.text;
    for expected in [
        "(source: readonly T[]): [T, number, number][]",
        "(source: string): [string, number, number][]",
        "(source: number): [number, number, number][]",
        "(source: Iterable<T>): [T, number, number][]",
        "(source: null | undefined)",
        "[T[keyof T], string, number][]",
    ] {
        assert!(text.contains(expected), "missing `{expected}`:\n{text}");
    }
}

#[test]
fn a_pending_projection_contributes_nothing() {
    let (plan, _, _) = project(LIST);
    let pending = IterationProjection::pending(plan.snapshot.clone());
    assert!(pending.pending && !pending.complete);
    assert!(pending.loop_head(TemplateBlockId(1)).is_none());
    assert!(pending.preludes().is_empty());
}
