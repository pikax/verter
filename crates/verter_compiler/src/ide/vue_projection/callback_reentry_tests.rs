//! Unit tests for the callback reentry module.

use verter_audit::observe::Uncaptured;

use super::callback_reentry::*;
use crate::framework_common::projection_plan::{
    build_projection_plan, AdmittedExpressionId, AttributeOpKind, PlanInput, ProjectionPlan,
};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::ide::vue_projection::branches::{project_template_branches, GuardPolarity};
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, CheckingPiece};
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

struct Carrier {
    source: String,
    plan: ProjectionPlan,
    reentry: CallbackReentryPlan,
}

fn carrier(template: &str) -> Carrier {
    let source = format!("<template>\n{template}</template>\n");
    let parsed = parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Reentry.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let ast = parsed.template_ast().expect("the carrier has a template");
    let topology = build_template_topology(
        &source,
        ast,
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("the topology builds");
    let branches = project_template_branches(&plan, &topology);
    let reentry = project_callback_reentry(&plan, &branches);
    Carrier {
        source,
        plan,
        reentry,
    }
}

impl Carrier {
    /// The value of the `index`th `v-on` operation over the component uses.
    fn listener(&self, index: usize) -> AdmittedExpressionId {
        self.plan
            .uses
            .iter()
            .flat_map(|use_| &use_.operations)
            .filter(|op| op.kind == AttributeOpKind::VOn)
            .nth(index)
            .and_then(|op| op.expression.clone())
            .expect("a component listener")
    }

    fn handler(&self) -> AdmittedExpressionId {
        self.listener(0)
    }

    fn flat(&self, fragment: &CheckingFragment) -> String {
        fragment.flatten(&self.source, &|id| {
            self.plan.expression(id).expect("admitted").spelling.clone()
        })
    }

    fn inline(&self, statements: bool) -> String {
        self.flat(&self.reentry.inline_handler(&self.handler(), statements))
    }
}

fn mapped_once(fragment: &CheckingFragment, id: &AdmittedExpressionId) -> usize {
    fragment
        .pieces
        .iter()
        .filter(|piece| matches!(piece, CheckingPiece::Expression(held) if held == id))
        .count()
}

#[test]
fn an_inline_handler_in_a_guarded_branch_replays_the_guard_and_returns_its_value() {
    let carrier = carrier("  <Child v-if=\"user\" @save=\"user.name.trim()\" />\n");
    assert!(!carrier.reentry.pending);
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("the inline handler is a callback");
    assert_eq!(callback.kind, CallbackKind::InlineExpression);
    assert_eq!(callback.replayed.len(), 1);
    assert_eq!(callback.replayed[0].polarity, GuardPolarity::Holds);
    assert!(callback.skipped.is_empty());
    assert_eq!(
        carrier.inline(false),
        "($event) => { void $event;\nif (!(user)) throw undefined;\nreturn (user.name.trim());\n}"
    );
}

#[test]
fn an_inline_handler_that_returns_a_promise_keeps_it() {
    let carrier = carrier("  <Child v-if=\"user\" @save=\"load(user.id)\" />\n");
    assert_eq!(
        carrier.inline(false),
        "($event) => { void $event;\nif (!(user)) throw undefined;\nreturn (load(user.id));\n}"
    );
}

#[test]
fn a_statement_handler_runs_its_statements_after_the_guards() {
    let carrier = carrier("  <Child v-if=\"user\" @save=\"user = null; user.name\" />\n");
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("callback");
    assert_eq!(callback.kind, CallbackKind::InlineStatements);
    assert_eq!(
        carrier.inline(true),
        "($event) => { void $event;\nif (!(user)) throw undefined;\nuser = null; user.name;\n}"
    );
}

#[test]
fn an_else_branch_replays_the_failed_conditions_and_nested_guards_in_path_order() {
    let carrier = carrier(concat!(
        "  <div v-if=\"user\">\n",
        "    <Child v-if=\"user.name\" @save=\"a + 1\" />\n",
        "  </div>\n",
        "  <Other v-else-if=\"kind === 'b'\" @save=\"b + 1\" />\n",
        "  <Last v-else @save=\"c + 1\" />\n",
    ));
    assert_eq!(carrier.reentry.callbacks.len(), 3);
    let flat = |index: usize| {
        carrier.flat(
            &carrier
                .reentry
                .inline_handler(&carrier.listener(index), false),
        )
    };
    assert_eq!(
        flat(0),
        "($event) => { void $event;\nif (!(user)) throw undefined;\nif (!(user.name)) throw undefined;\nreturn (a + 1);\n}"
    );
    assert_eq!(
        flat(1),
        "($event) => { void $event;\nif (user) throw undefined;\nif (!(kind === 'b')) throw undefined;\nreturn (b + 1);\n}"
    );
    assert_eq!(
        flat(2),
        "($event) => { void $event;\nif (user) throw undefined;\nif (kind === 'b') throw undefined;\nreturn (c + 1);\n}"
    );
}

#[test]
fn an_authored_function_and_a_handler_reference_get_no_replayed_guard() {
    for handler in [
        "() => { user = null }",
        "function () { return user }",
        "onSave",
    ] {
        let carrier = carrier(&format!("  <Child v-if=\"user\" @save=\"{handler}\" />\n"));
        assert!(
            carrier.reentry.callbacks.is_empty(),
            "`{handler}` keeps its own closure"
        );
        assert!(carrier.reentry.projections.is_empty());
    }
}

#[test]
fn a_handler_outside_any_branch_has_the_plain_form() {
    let carrier = carrier("  <Child @save=\"count + 1\" />\n");
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("callback");
    assert!(callback.replayed.is_empty());
    assert!(carrier.reentry.projections.is_empty());
    assert_eq!(
        carrier.inline(false),
        "($event) => (void $event, count + 1)"
    );
    let statements = self::carrier("  <Child @save=\"a = 1; b = 2\" />\n");
    assert_eq!(
        statements.inline(true),
        "($event) => { void $event;\na = 1; b = 2;\n}"
    );
}

#[test]
fn a_pending_plan_contributes_the_plain_form() {
    let carrier = carrier("  <Child v-if=\"user\" @save=\"user.name.trim()\" />\n");
    let pending = CallbackReentryPlan::pending(carrier.plan.snapshot.clone());
    assert!(pending.pending && pending.callbacks.is_empty());
    assert_eq!(
        carrier.flat(&pending.inline_handler(&carrier.handler(), false)),
        "($event) => (void $event, user.name.trim())"
    );
}

#[test]
fn a_guard_is_replayed_and_the_handler_is_mapped_once() {
    let carrier = carrier("  <Child v-if=\"user\" @save=\"user.name.trim()\" />\n");
    let handler = carrier.handler();
    let fragment = carrier.reentry.inline_handler(&handler, false);
    assert_eq!(mapped_once(&fragment, &handler), 1);
    let guard = &carrier
        .reentry
        .callback(&handler)
        .expect("callback")
        .replayed[0]
        .expression;
    assert_eq!(mapped_once(&fragment, guard), 0);
    assert!(fragment
        .pieces
        .iter()
        .any(|piece| matches!(piece, CheckingPiece::Replay(held) if held == guard)));
}

#[test]
fn a_guard_whose_name_a_loop_rebinds_is_skipped_not_replayed() {
    let carrier = carrier(concat!(
        "  <div v-if=\"item\">\n",
        "    <Row v-for=\"item in items\" @save=\"pick(item.id)\" />\n",
        "  </div>\n",
    ));
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("callback");
    assert!(callback.replayed.is_empty());
    assert_eq!(callback.skipped.len(), 1);
    assert_eq!(
        callback.skipped[0].1,
        GuardSkip::Rebound {
            name: "item".to_string()
        }
    );
    assert_eq!(
        carrier.inline(false),
        "($event) => (void $event, pick(item.id))"
    );
}

#[test]
fn a_guard_the_loop_does_not_rebind_is_still_replayed() {
    let carrier = carrier(concat!(
        "  <div v-if=\"user\">\n",
        "    <Row v-for=\"item in items\" @save=\"item.id + user.id\" />\n",
        "  </div>\n",
    ));
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("callback");
    assert_eq!(callback.replayed.len(), 1);
    assert!(callback.skipped.is_empty());
}

#[test]
fn a_guard_written_inside_the_loop_reads_the_loop_binding() {
    let carrier = carrier(concat!(
        "  <div v-for=\"item in items\">\n",
        "    <Row v-if=\"item.ok\" @save=\"pick(item.id)\" />\n",
        "  </div>\n",
    ));
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("callback");
    assert_eq!(callback.replayed.len(), 1);
    assert!(callback.skipped.is_empty());
}

#[test]
fn a_model_update_replays_the_guards_that_dominate_it() {
    let carrier = carrier("  <Child v-if=\"user\" v-model=\"user.name\" />\n");
    let model = carrier
        .plan
        .uses
        .iter()
        .flat_map(|use_| &use_.operations)
        .find(|op| op.kind == AttributeOpKind::Model)
        .and_then(|op| op.expression.clone())
        .expect("a model operation");
    let callback = carrier.reentry.callback(&model).expect("callback");
    assert_eq!(callback.kind, CallbackKind::ModelUpdate);
    assert_eq!(
        carrier.flat(&carrier.reentry.replayed_guards(&model)),
        "if (!(user)) throw undefined;\n"
    );
}

#[test]
fn a_string_that_spells_a_loop_name_is_not_a_read_of_it() {
    let carrier = carrier(concat!(
        "  <div v-if=\"kind === 'item'\">\n",
        "    <Row v-for=\"item in items\" @save=\"pick(item.id)\" />\n",
        "  </div>\n",
    ));
    let callback = carrier
        .reentry
        .callback(&carrier.handler())
        .expect("callback");
    assert_eq!(callback.replayed.len(), 1);
    assert!(callback.skipped.is_empty());
}
