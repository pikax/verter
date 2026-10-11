//! Every way syntax nests, deep, answered through Verter on a stack
//! smaller than any it runs on in production. A nest's flow evaluation —
//! its expressions, statements, calls and nested function values — runs in
//! place on the thread that asks (the caller's, 1 MiB at the least a host
//! asks from), so each form below is evaluated on a thread of the stack its
//! test names, in a fresh process of its own ([`in_a_fresh_process`]: the
//! thread has exactly that stack, and an overflow fails the one test). The
//! oxc-only parse of each form is recorded in
//! `docs/evidence/signature-kernel/oxc-deep-parse.md`; these are the same
//! forms, as a return or a type the probe reads.
//!
//! What each form needs of the caller's stack was measured by bisection,
//! to 32 KiB, unoptimized: a probe's fixed work — the host, the upsert, the
//! dispatch around the evaluation — takes 416 KiB for a `pf` return read
//! and 480 KiB for a probe with a conditional type around it, at any depth
//! of the forms whose evaluators keep explicit stacks (parentheses, `!`,
//! template holes, object and array literals, blocks, labels, patterns,
//! `await`, enum chains and initializers, each 416 KiB at 1,000 levels;
//! nested function values and declarators 480 KiB at 200; `else if` chains
//! 480 KiB at 800). A nest of calls, callbacks or a receiver chain takes
//! 608 KiB at 250 levels and a value-rooted member chain 672 KiB at 500 or
//! 1,000 — a larger fixed cost, not a level's. A return's conditional
//! expression is the one form still spending stack per level: 544 KiB at
//! 500 levels, 672 KiB at 1,000 — about a quarter KiB a level, a residual
//! recursion on the arm-by-arm read. Each test runs on [`SMALL_STACK`]
//! (640 KiB) or [`WIDE_STACK`] (896 KiB), at least 160 KiB past its form's
//! need; the per-level cost that fails it is that headroom over its depth,
//! stated at each test. A level evaluated in place costs far more than any
//! of them: about 53 KiB for a nested function, several KiB for an operand
//! or a call route.
//!
//! A declaration's body lowers on a declaration-lowering worker, on its
//! own 8 MiB, which no caller's thread bounds, and the script's binding
//! usages and a declaration's call positions are walked at index time on
//! the scheduler's: those forms stay at 10,000 levels, where a native level
//! per nesting level overflows the worker, with the caller's side of each
//! read on [`SMALL_STACK`] (416 KiB of it needed).
//!
//! Each answer is TypeScript 7.0.2's for the same module and probe
//! (`--noEmit --strict`), measured at the depth the checker was asked; a
//! depth the checker answers at answers the same at every depth below it.
//!
//! A form whose demand slice outgrows the flow-slice plan budget
//! (`FlowSliceBudget::max_selected_nodes`), or whose evaluation outgrows
//! the connected-demand work budget, answers a typed budget failure, not a
//! type. Those forms are checked for their answer under the budgets, and
//! for the typed failure past them — under ONE budget lowered (the plan's
//! to a few hundred nodes, or the work's to a fraction of production's,
//! the other at its production default) so the trip is reached at
//! [`BUDGET_DEPTH`] rather than at ten thousand, and the axis that tripped
//! is read off the request's audit: the planner's refusal is a structured
//! event, and a work trip is the typed failure with no such event.

use super::checker_probe_lane_tests::{
    default_probe_host, flow_return_audited_on_host, flow_return_outcome_on_host,
    in_a_fresh_process, mismatches, test_path, PROBE_FILE,
};
use crate::host_flow_return_audit::FlowReturnError;
use verter_audit::{FlowSliceBudgetAxisTag, StructuredAuditEvent};
use verter_session_query::flow::peeker::FlowSliceBudget;
use verter_type_engine::semantic_query::FlowReturnFailure;
use verter_type_expr::facts::InferenceUnavailableReason;

/// The stack a form whose fixed need is 416 or 480 KiB runs on: 160 KiB
/// past the larger, and 224 KiB past a `pf` return read.
pub(super) const SMALL_STACK: usize = 640 << 10;

/// The stack the call, callback, receiver and member-chain forms (608 to
/// 672 KiB needed) and the conditional nest (672 KiB at 1,000 levels) run
/// on: 224 KiB past the dearest.
const WIDE_STACK: usize = 896 << 10;

/// The stack `keyof` over a mapped-type nest reads on: 800 KiB needed at
/// 1,000 levels (and at 300), so 1 MiB — the least a host asks from — is
/// 224 KiB past it.
const MAPPED_STACK: usize = 1 << 20;

/// The depth every expression and statement nest is checked at.
const DEPTH: usize = 1_000;

/// The depth a nest of function values is checked at: evaluating each
/// nested function in place costs about 53 KiB a level, so five levels of
/// that overflow [`SMALL_STACK`].
const NESTED_FUNCTIONS: usize = 200;

/// The depth a nest of calls is checked at: a call's route is the dearest
/// frame the evaluator keeps, and the connected demand charges each call
/// once, so a nest this deep answers in a fraction of a second.
const CALLS: usize = 250;

/// The depth a nest whose production-size answer is a budget failure is
/// checked at, under [`PLAN_BUDGET`] or [`WORK_BUDGET`].
const BUDGET_DEPTH: usize = 400;

/// The flow-slice plan budget the plan-failure forms run under: a nest of
/// [`BUDGET_DEPTH`] statements selects more nodes than this.
const PLAN_BUDGET: u32 = 256;

/// The connected-demand work budget a `keyof` chain runs under, a fortieth
/// of production's: a chain of [`BUDGET_DEPTH`] levels charges more than
/// this (its work grows with the square of its depth: 192,301 units at
/// 200 levels, 861,701 at 400).
const WORK_BUDGET: usize = 100_000;

/// The connected-demand work budget a mapped-type nest is read under: the
/// nest's work grows by four units a level (813 at 200, 1,613 at 400,
/// 4,013 at 1,000), so [`BUDGET_DEPTH`] levels charge past this, where a
/// nest a quarter as deep answers.
const MAPPED_WORK_BUDGET: usize = 1_000;

/// The depth a form that lowers inside a type alias, or is walked at index
/// time — on a worker's 8 MiB — is checked at.
const WORKER_DEPTH: usize = 10_000;

/// `f` on a thread of `stack` bytes.
fn on_a_stack<T: Send + 'static>(stack: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(f)
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

/// `probe` over `source` answers `checker`, read on a thread of `stack`
/// bytes in a fresh process: the test `test` ([`test_path!`]) entire.
fn answers_on(
    test: &str,
    stack: usize,
    source: String,
    probe: &'static str,
    checker: &'static str,
) {
    in_a_fresh_process(test, || {
        assert_eq!(
            on_a_stack(stack, move || mismatches(&source, &[(probe, checker)])),
            Vec::<String>::new()
        );
    });
}

/// A probe host whose audit captures the request's structured events, on
/// which a budget trip's axis can be read.
fn footprint_host() -> std::sync::Arc<crate::VerterHost> {
    std::sync::Arc::new(crate::VerterHost::new_standalone(crate::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        ..crate::HostConfig::default()
    }))
}

/// The typed outcome of `pf`'s body-derived return over `source`, and the
/// structured events its request recorded.
fn audited_return_on(
    host: &std::sync::Arc<crate::VerterHost>,
    source: &str,
) -> (
    Result<Option<verter_type_engine::semantic_query::FlowReturnDegradation>, FlowReturnError>,
    Vec<StructuredAuditEvent>,
) {
    let audited = flow_return_audited_on_host(host, source, "pf");
    let events = audited
        .audit()
        .footprint
        .as_ref()
        .expect("the probe host captures the request's footprint")
        .structured_events
        .clone();
    (
        audited.into_result().map(|result| result.degradation()),
        events,
    )
}

/// Whether `events` hold the planner's refusal at `limit` selected nodes.
fn planner_refused_at(events: &[StructuredAuditEvent], limit: u32) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            StructuredAuditEvent::FlowSliceBudgetExceeded {
                axis: FlowSliceBudgetAxisTag::SelectedNodes,
                limit: refused,
                observed,
            } if *refused == limit && *observed > limit
        )
    })
}

/// `pf`'s return over `source`, read on [`SMALL_STACK`] under a flow-slice
/// plan budget of [`PLAN_BUDGET`] selected nodes and the production work
/// budget, is the typed budget failure, and the planner's refusal — at
/// that budget — is the request's recorded reason.
fn assert_returns_past_the_plan_budget(source: String) {
    let (outcome, events) = on_a_stack(SMALL_STACK, move || {
        let host = footprint_host();
        host.project_type_store()
            .flow_slice()
            .set_budget_for_test(FlowSliceBudget {
                max_selected_nodes: PLAN_BUDGET,
                ..FlowSliceBudget::default()
            });
        audited_return_on(&host, &source)
    });
    assert_eq!(outcome, WORK_BUDGET_EXCEEDED);
    assert!(
        planner_refused_at(&events, PLAN_BUDGET),
        "the planner refused the slice at {PLAN_BUDGET} nodes: {events:?}"
    );
}

/// `pf`'s return over `source`, read on a thread of `stack` bytes under
/// a connected-demand work budget of `work` and the production plan
/// budget, is the typed budget failure, and the planner recorded no
/// refusal: the work ledger's trip is the reason.
fn assert_returns_past_a_work_budget(source: String, work: usize, stack: usize) {
    let (outcome, events) = on_a_stack(stack, move || {
        let _work = super::connected_demand::WorkBudgetForTests::install(work);
        audited_return_on(&footprint_host(), &source)
    });
    assert_eq!(outcome, WORK_BUDGET_EXCEEDED);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, StructuredAuditEvent::FlowSliceBudgetExceeded { .. })),
        "the plan budget, at its production default, did not trip: {events:?}"
    );
}

/// The typed outcome of a return that evaluated to its value, undegraded.
const RETURNS_ITS_VALUE: Result<
    Option<verter_type_engine::semantic_query::FlowReturnDegradation>,
    FlowReturnError,
> = Ok(None);

/// The typed outcome of a return whose evaluation outgrew the demand's
/// connected work: no value, and the work budget named.
const WORK_BUDGET_EXCEEDED: Result<
    Option<verter_type_engine::semantic_query::FlowReturnDegradation>,
    FlowReturnError,
> = Err(FlowReturnError::Failure(FlowReturnFailure::Budget(
    InferenceUnavailableReason::WorkBudgetExceeded,
)));

/// The connected work `function`'s body-derived return over `source`
/// charges, evaluated cold on a host of its own — a COMPLETE evaluation:
/// the return has a value, undegraded, and its reduced type matches the
/// checker's `checker`.
fn connected_work_of(source: &str, function: &str, checker: &str) -> usize {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    use std::sync::Arc;
    use verter_type_engine::semantic_query::{
        ProjectionMode, ProjectionReductionContext, QueryResult, ReturnProjectionDemand,
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
    };
    let host = default_probe_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        PROBE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(source),
        crate::FileLanguage::script_ts(),
    );
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(function),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let key =
        dispatch.flow_return_key_with_demand(&identity, ReturnProjectionDemand::whole_return());
    let result = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => result,
        other => panic!("`{function}` must evaluate to a value, got {other:?}"),
    };
    let work = dispatch.connected_demand_usage().work;
    assert_eq!(
        result.degradation(),
        None,
        "`{function}` evaluates completely"
    );
    let expected = checker_syntax::parse(checker).expect("the checker print parses");
    let node = dispatch
        .normalize_node_keeping_declaration_refs_for_tests(
            result.return_type(),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
        .expect("the return reduces completely");
    assert!(
        checker_syntax::matches_node(&dispatch, node, &expected, 0),
        "`{function}` answers `{checker}`, measured `{}`",
        render_node(&dispatch, node, 0)
    );
    work
}

/// The connected work a nest of `depth` levels charges grows with its
/// depth, not its square: each level is charged once, however many
/// consumers read it, so the production work budget admits a nest of any
/// depth it has room for — ten thousand levels, by the growth measured at
/// 100, 200 and 400. Every measured evaluation is complete and answers
/// `checker`; `nest` builds the module and names the function read.
fn assert_work_is_linear(nest: impl Fn(usize) -> (String, &'static str), checker: &'static str) {
    let work = |depth: usize| {
        let (source, function) = nest(depth);
        connected_work_of(&source, function, checker)
    };
    let (at_100, at_200, at_400) = (work(100), work(200), work(400));
    assert!(
        at_100 < at_200 && at_200 < at_400,
        "work at 100 / 200 / 400 levels grows: {at_100} / {at_200} / {at_400}"
    );
    let (first, second) = (at_200 - at_100, at_400 - at_200);
    assert!(
        second <= first * 2 + first / 10,
        "work at 100 / 200 / 400 levels: {at_100} / {at_200} / {at_400}"
    );
    assert!(
        at_400 * 25 <= super::connected_demand::MAX_CONNECTED_PROJECTION_WORK,
        "a nest of 10,000 levels, at the work of 400 ({at_400}) twenty-five times over, \
         stays within the production work budget"
    );
}

fn wrap(depth: usize, open: &str, close: &str) -> String {
    format!("{}1{}", open.repeat(depth), close.repeat(depth))
}

fn returning(value: String) -> String {
    format!("export function pf(b: boolean) {{ return {value}; }}\n")
}

fn conditionals(depth: usize) -> String {
    returning(format!("{}2", "b ? 1 : ".repeat(depth)))
}

fn objects(depth: usize) -> String {
    returning(wrap(depth, "{ v: ", " }"))
}

fn blocks(depth: usize) -> String {
    format!(
        "export function pf() {{ {}return 1;{} }}\n",
        "{ ".repeat(depth),
        " }".repeat(depth)
    )
}

const RETURN: &str = "ReturnType<typeof pf>";

/// Parentheses nested 1,000 deep: 416 KiB needed, so a level costing more
/// than 0.22 KiB overflows.
#[test]
fn parentheses_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        returning(wrap(DEPTH, "(", ")")),
        RETURN,
        "number",
    );
}

/// A chain of member reads off a constructed value (`new C().c.c…`): its
/// lowering and its evaluation read the chain from explicit stacks,
/// deciding once that it is rooted at a value, so the work grows with the
/// chain's length. 672 KiB needed at 500 and at 1,000 links alike, so a
/// link costing more than 0.22 KiB overflows [`WIDE_STACK`]. TypeScript
/// 7.0.2: `C`, under every setting.
#[test]
fn value_rooted_member_chains_long_answer_on_a_small_stack() {
    let source = format!(
        "class C {{ c: C = this; }}\nexport function pf() {{ return new C(){}; }}\n",
        ".c".repeat(DEPTH)
    );
    answers_on(test_path!(), WIDE_STACK, source, RETURN, "C");
}

/// `!` nested 1,000 deep: 416 KiB needed, so a level costing more than
/// 0.22 KiB overflows.
#[test]
fn logical_nots_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        returning(format!("{}1", "!".repeat(DEPTH))),
        RETURN,
        "boolean",
    );
}

/// Template holes nested 1,000 deep: 416 KiB needed, so a level costing
/// more than 0.22 KiB overflows.
#[test]
fn template_holes_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        returning(wrap(DEPTH, "`${", "}`")),
        RETURN,
        "string",
    );
}

/// A return's conditional nested 1,000 deep: 672 KiB needed (544 at 500 —
/// about a quarter KiB a level, the residual recursion of the arm-by-arm
/// read), so a level costing more than 0.45 KiB overflows [`WIDE_STACK`].
#[test]
fn conditionals_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        WIDE_STACK,
        conditionals(DEPTH),
        RETURN,
        "1 | 2",
    );
}

/// Object literals nested 1,000 deep: 416 KiB needed, so a level costing
/// more than 0.22 KiB overflows.
#[test]
fn objects_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        objects(DEPTH),
        "keyof ReturnType<typeof pf>",
        "\"v\"",
    );
}

/// Blocks nested 1,000 deep: 416 KiB needed, so a level costing more than
/// 0.22 KiB overflows.
#[test]
fn blocks_nested_deep_answer_on_a_small_stack() {
    answers_on(test_path!(), SMALL_STACK, blocks(DEPTH), RETURN, "number");
}

/// Conditionals, object literals and blocks nested past the plan budget
/// return its typed budget failure, the planner's refusal recorded.
#[test]
fn conditionals_objects_and_blocks_nested_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        for source in [
            conditionals(BUDGET_DEPTH),
            objects(BUDGET_DEPTH),
            blocks(BUDGET_DEPTH),
        ] {
            assert_returns_past_the_plan_budget(source);
        }
    });
}

/// A conditional nested 1,000 deep as an object member's value evaluates
/// its branches from the evaluator's stack (a return's conditional is read
/// arm by arm instead, for its per-arm freshness): 416 KiB needed, so a
/// level costing more than 0.22 KiB overflows.
#[test]
fn a_member_conditional_nested_deep_answers_on_a_small_stack() {
    let source = format!(
        "export function pf(b: boolean) {{ return {{ v: {}2 }}; }}\n",
        "b ? 1 : ".repeat(DEPTH)
    );
    answers_on(
        test_path!(),
        SMALL_STACK,
        source,
        "ReturnType<typeof pf>[\"v\"]",
        "number",
    );
}

/// A function type returning a function type, 10,000 deep, and the same
/// for constructor types: each signature's return lowers from the locator
/// lowering's explicit stack, on the declaration-lowering worker; the
/// caller's side reads on [`SMALL_STACK`] (416 KiB needed).
#[test]
fn function_and_constructor_types_nested_10000_deep_answer_on_a_worker() {
    in_a_fresh_process(test_path!(), || {
        for arrow in ["() => ", "new () => "] {
            let source = format!("type D = {}1;\n", arrow.repeat(WORKER_DEPTH));
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(
                    &source,
                    &[("D extends Function ? 1 : 2", "1")]
                )),
                Vec::<String>::new(),
                "{arrow}"
            );
        }
    });
}

/// Nested generic calls, `f(f(…f(1)…))`: each call's callee operand and
/// the frame-lowered argument its executor route types evaluate from the
/// evaluator's stack.
fn calls(depth: usize) -> String {
    format!(
        "function f<T>(v: T): T {{ return v; }}\nexport function pf() {{ return {}1{}; }}\n",
        "f(".repeat(depth),
        ")".repeat(depth)
    )
}

/// Calls nested 250 deep: 608 KiB needed, so a level costing more than
/// 1.15 KiB overflows [`WIDE_STACK`].
#[test]
fn calls_nested_deep_answer_on_a_small_stack() {
    answers_on(test_path!(), WIDE_STACK, calls(CALLS), RETURN, "number");
}

/// Each call of a nest is charged once to the connected demand, however
/// many consumers read it: the work grows with the nest (7 units a call),
/// so the work budget admits every nest it has room for.
#[test]
fn calls_nested_deep_charge_linear_work() {
    assert_work_is_linear(|depth| (calls(depth), "pf"), "number");
}

fn arrays(depth: usize) -> String {
    returning(wrap(depth, "[", "]"))
}

/// `pf`'s return over `source`: how many mutable arrays it nests, and
/// whether the innermost one's element is `number`. Read from a loop: the
/// checker-print comparison descends a bounded depth of its own.
fn array_nest_of(source: &str, probe: &str) -> (usize, bool) {
    super::checker_probe_lane_tests::with_probe(source, probe, |dispatch, node| {
        let graph = dispatch.graph();
        let mut node = node;
        let mut depth = 0;
        while let Some(verter_type_engine::semantic_query::SemanticNodeData::Array {
            element,
            readonly: false,
        }) = graph.node_data(node).as_deref()
        {
            depth += 1;
            node = *element;
        }
        let number = matches!(
            graph.node_data(node).as_deref(),
            Some(
                verter_type_engine::semantic_query::SemanticNodeData::Primitive(
                    verter_type_engine::semantic_query::PrimitiveKind::Number
                )
            )
        );
        (depth, number)
    })
}

/// [`array_nest_of`] for `probe` over `source`, read on [`SMALL_STACK`] in
/// a fresh process, is `(depth, true)`.
fn array_nest_is(test: &str, source: String, probe: &'static str, depth: usize) {
    in_a_fresh_process(test, || {
        assert_eq!(
            on_a_stack(SMALL_STACK, move || array_nest_of(&source, probe)),
            (depth, true)
        );
    });
}

/// Array literals nested 65 deep, past any depth a shallow per-expression
/// inference bounds: the planner opens a site per element and the
/// evaluator evaluates each array literal as a frame of its stack, so the
/// return is `number` under 65 array dimensions, as the checker's is.
#[test]
fn arrays_nested_65_deep_answer_on_a_small_stack() {
    array_nest_is(test_path!(), arrays(65), RETURN, 65);
}

/// Array literals nested 1,000 deep: the content half lowers each literal
/// as a frame of its stack, element by element, so no level costs a native
/// one (416 KiB needed: a level costing more than 0.22 KiB overflows).
#[test]
fn arrays_nested_deep_answer_on_a_small_stack() {
    array_nest_is(test_path!(), arrays(DEPTH), RETURN, DEPTH);
}

/// Array literals nested past the plan budget return its typed budget
/// failure, the planner's refusal recorded.
#[test]
fn arrays_nested_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_the_plan_budget(arrays(BUDGET_DEPTH));
    });
}

fn module_const_arrays(depth: usize) -> String {
    format!("export const x = {};\n", wrap(depth, "[", "]"))
}

/// A module-level `const` initialized with array literals nested 65 deep
/// declares `number` under 65 array dimensions, as the checker's
/// declaration does (TypeScript 7.0.2, all four settings): the shallow
/// declaration inference infers the nest from its explicit stacks.
#[test]
fn module_const_arrays_nested_65_deep_answer_on_a_small_stack() {
    array_nest_is(test_path!(), module_const_arrays(65), "typeof x", 65);
}

/// A thousand levels, within the inference's work budget: no level costs a
/// native one.
#[test]
fn module_const_arrays_nested_deep_answer_on_a_small_stack() {
    array_nest_is(test_path!(), module_const_arrays(DEPTH), "typeof x", DEPTH);
}

/// Type arguments nested 10,000 deep lower from the locator lowering's
/// explicit stack, on the declaration-lowering worker; the caller's side
/// reads on [`SMALL_STACK`] (416 KiB needed).
#[test]
fn type_arguments_nested_10000_deep_answer_on_a_worker() {
    let source = format!(
        "interface Box<T> {{ v: T }}\ntype D = {};\n",
        wrap(WORKER_DEPTH, "Box<", ">")
    );
    answers_on(
        test_path!(),
        SMALL_STACK,
        source,
        "D extends Box<unknown> ? 1 : 2",
        "1",
    );
}

/// Expression nests 10,000 deep in declarations no demand evaluates: the
/// script's binding usages and a declaration's call positions are walked
/// at index time, from explicit stacks, on the scheduler's workers (8 MiB
/// each), where a native level per nesting level overflows. A call nest
/// and a receiver chain sit in module constants, parentheses in a function
/// body; the module's analysis is read, and `pf`'s return, whose body
/// holds none of them, on [`SMALL_STACK`].
#[test]
fn expression_nests_10000_deep_index_on_a_worker() {
    in_a_fresh_process(test_path!(), || {
        on_a_stack(SMALL_STACK, || {
            let source = format!(
                "function f<T>(v: T): T {{ return v; }}\n\
                 interface B {{ m(): B; }}\ndeclare const b: B;\n\
                 const calls = {}1{};\n\
                 const chain = b{};\n\
                 function parens() {{ return {}; }}\n\
                 export function pf() {{ return 1; }}\n",
                "f(".repeat(WORKER_DEPTH),
                ")".repeat(WORKER_DEPTH),
                ".m()".repeat(WORKER_DEPTH),
                wrap(WORKER_DEPTH, "(", ")"),
            );
            let host = default_probe_host();
            assert_eq!(
                flow_return_outcome_on_host(&host, &source, "pf"),
                RETURNS_ITS_VALUE
            );
            let analysis = host
                .get_analysis(PROBE_FILE)
                .expect("the module's analysis is served");
            assert!(
                analysis
                    .bindings
                    .iter()
                    .any(|binding| binding.name == "calls"),
                "the analysis binds the module's constants"
            );
        });
    });
}

fn keyof_chain(depth: usize) -> String {
    format!("type D = {}{{ v: 1 }};\n", "keyof ".repeat(depth))
}

const KEYOF_PROBE: &str = "D extends string | number | symbol ? 1 : 2";

/// A `keyof` chain whose resolution exceeds the connected-demand work
/// budget returns its typed budget failure, the plan budget at its
/// production default.
#[test]
fn keyof_chains_past_the_work_budget_return() {
    in_a_fresh_process(test_path!(), || {
        let source = format!(
            "{}export function pf() {{ return null as unknown as ({KEYOF_PROBE}); }}\n",
            keyof_chain(BUDGET_DEPTH)
        );
        assert_returns_past_a_work_budget(source, WORK_BUDGET, SMALL_STACK);
    });
}

#[test]
#[ignore = "the connected-demand work budget admits no 10,000-deep keyof chain"]
fn keyof_chains_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches(&keyof_chain(WORKER_DEPTH), &[(KEYOF_PROBE, "1")]),
        Vec::<String>::new()
    );
}

fn arrows(depth: usize) -> String {
    returning(format!("{}1", "() => ".repeat(depth)))
}

const ARROW_PROBE: &str = "ReturnType<typeof pf> extends Function ? 1 : 2";

#[test]
fn arrows_nested_10_deep_answer_on_a_small_stack() {
    answers_on(test_path!(), SMALL_STACK, arrows(10), ARROW_PROBE, "1");
}

/// Arrow functions nested 200 deep: each body's return reaches the next
/// function value, which its own evaluator evaluates from the drive's
/// stack of evaluators (`flow_return_nested`), not inside the one around
/// it. 480 KiB needed, so a level costing more than 0.8 KiB overflows.
#[test]
fn arrows_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        arrows(NESTED_FUNCTIONS),
        ARROW_PROBE,
        "1",
    );
}

/// A module constant initialized by nested generic calls: its value
/// evaluates the calls' indexed records from an explicit stack, and the
/// module's binding usages collect from one.
fn module_calls(depth: usize, read_in_a_function: bool) -> String {
    let calls = format!("{}1{}", "f(".repeat(depth), ")".repeat(depth));
    if read_in_a_function {
        format!(
            "function f<T>(v: T): T {{ return v; }}
const v = {calls};
export function pf() {{ return v; }}
"
        )
    } else {
        format!(
            "function f<T>(v: T): T {{ return v; }}
export const v = {calls};
"
        )
    }
}

/// A module constant initialized by 250 nested calls, read as `typeof v`:
/// 480 KiB needed, so a level costing more than 0.64 KiB overflows.
#[test]
fn module_calls_nested_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        module_calls(CALLS, false),
        "typeof v",
        "1",
    );
}

/// A module constant initialized by nested calls, read in a function,
/// returns its value, and each call's resolution is charged once to the
/// connected demand: the work grows with the nest.
#[test]
fn module_calls_nested_deep_read_charge_linear_work() {
    in_a_fresh_process(test_path!(), || {
        assert_eq!(
            on_a_stack(WIDE_STACK, || flow_return_outcome_on_host(
                &default_probe_host(),
                &module_calls(CALLS, true),
                "pf"
            )),
            RETURNS_ITS_VALUE
        );
        assert_work_is_linear(|depth| (module_calls(depth, true), "pf"), "number");
    });
}

fn receiver_chain(links: usize) -> String {
    format!(
        "interface B {{ m(): B; }}
export function pf(b: B) {{ return b{}; }}
",
        ".m()".repeat(links)
    )
}

/// A receiver chain, `b.m().m()…`: each call is a member call on the
/// value of the call before it, lowered and evaluated from explicit
/// stacks, the value its receiver. 608 KiB needed at 250 links, so a link
/// costing more than 1.15 KiB overflows [`WIDE_STACK`]; each call is
/// charged once to the connected demand (2 units a link), so the work
/// grows with the chain. TypeScript 7.0.2: `B` for 2, 3 and 400 links,
/// under every setting.
#[test]
fn receiver_chains_deep_answer_on_a_small_stack_and_charge_linear_work() {
    in_a_fresh_process(test_path!(), || {
        for links in [2, 3, CALLS] {
            assert_eq!(
                on_a_stack(WIDE_STACK, move || mismatches(
                    &receiver_chain(links),
                    &[(RETURN, "B")]
                )),
                Vec::<String>::new(),
                "{links} links"
            );
        }
        assert_work_is_linear(|links| (receiver_chain(links), "pf"), "B");
    });
}

fn module_receiver_chain(links: usize) -> String {
    format!(
        "interface B {{ m(): B; }}
declare const b: B;
export const v = b{};
",
        ".m()".repeat(links)
    )
}

/// A module constant initialized by a receiver chain reads as the chain's
/// last call: its indexed initializer reads each call's callee as a member
/// of the value of the call before it, from an explicit stack. 608 KiB
/// needed at 250 links, so a link costing more than 1.15 KiB overflows
/// [`WIDE_STACK`]. TypeScript 7.0.2: `B` for 2, 3 and 400 links, under
/// every setting.
#[test]
fn module_receiver_chains_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for links in [2, 3, CALLS] {
            assert_eq!(
                on_a_stack(WIDE_STACK, move || mismatches(
                    &module_receiver_chain(links),
                    &[("typeof v", "B")]
                )),
                Vec::<String>::new(),
                "{links} links"
            );
        }
    });
}

/// A module constant initialized by a receiver chain, read in a function,
/// returns its value, each call charged once to the connected demand: the
/// work grows with the chain.
#[test]
fn module_receiver_chains_deep_read_charge_linear_work() {
    let read = |links: usize| {
        (
            format!(
                "{}export function pf() {{ return v; }}
",
                module_receiver_chain(links)
            ),
            "pf",
        )
    };
    in_a_fresh_process(test_path!(), || {
        assert_eq!(
            on_a_stack(WIDE_STACK, move || flow_return_outcome_on_host(
                &default_probe_host(),
                &read(CALLS).0,
                "pf"
            )),
            RETURNS_ITS_VALUE
        );
        assert_work_is_linear(read, "B");
    });
}

/// Callbacks nested 200 deep, each an arrow passed to a callee that
/// declares its type (`a(() => a(() => … 1))`): each callback is typed
/// under its parameter's declared signature, so its body is never
/// evaluated — a parity row the nesting cannot make dear, 608 KiB needed.
/// TypeScript 7.0.2: `number`, under every `strictNullChecks` ×
/// `noImplicitAny` setting.
#[test]
fn callbacks_nested_deep_answer_on_a_small_stack() {
    let source = format!(
        "declare function a(f: () => number): number;\n{}",
        returning(wrap(NESTED_FUNCTIONS, "a(() => ", ")"))
    );
    answers_on(test_path!(), WIDE_STACK, source, RETURN, "number");
}

/// Callbacks nested through a generic callee, which infers each call's
/// type argument from its callback's return: the call's executor route
/// types each callback under its parameter, suspending at it while its body
/// evaluates from the stack of evaluators.
fn generic_callbacks(depth: usize) -> String {
    format!(
        "declare function a<T>(f: () => T): T;\n{}",
        returning(wrap(depth, "a(() => ", ")"))
    )
}

/// Generic callbacks nested 200 deep: 608 KiB needed, so a level costing
/// more than 1.4 KiB overflows [`WIDE_STACK`] (typing a callback in place
/// instead of suspending the route costs tens); each call resolution is
/// charged once to the connected demand, so the work grows with the nest.
/// TypeScript 7.0.2: `number`, under every setting.
#[test]
fn generic_callbacks_nested_deep_answer_on_a_small_stack_and_charge_linear_work() {
    in_a_fresh_process(test_path!(), || {
        assert_eq!(
            on_a_stack(WIDE_STACK, || mismatches(
                &generic_callbacks(NESTED_FUNCTIONS),
                &[(RETURN, "number")]
            )),
            Vec::<String>::new()
        );
        assert_work_is_linear(|depth| (generic_callbacks(depth), "pf"), "number");
    });
}

/// Immediately invoked functions nested 200 deep, arrows with block bodies
/// and function expressions: each call's callee operand is a nested
/// function value evaluated from the stack of evaluators. 480 KiB needed,
/// so a level costing more than 0.8 KiB overflows. TypeScript 7.0.2:
/// `number`, under every setting.
#[test]
fn immediately_invoked_functions_nested_deep_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for (open, close) in [
            ("(() => { return ", "; })()"),
            ("(function () { return ", "; })()"),
        ] {
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(
                    &returning(wrap(NESTED_FUNCTIONS, open, close)),
                    &[(RETURN, "number")]
                )),
                Vec::<String>::new(),
                "{open}"
            );
        }
    });
}

const OBJECT_PROBE: &str = "ReturnType<typeof pf> extends object ? 1 : 2";

/// Object literals whose method returns the next, 200 deep: a literal's
/// methods evaluate as children of its own step. 480 KiB needed, so a
/// level costing more than 0.8 KiB overflows. TypeScript 7.0.2: `1`,
/// under every setting.
#[test]
fn object_methods_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(NESTED_FUNCTIONS, "{ m() { return ", "; } }"));
    answers_on(test_path!(), SMALL_STACK, source, OBJECT_PROBE, "1");
}

/// Class expressions whose method returns the next, 200 deep: a class's
/// member functions evaluate as children of its own step. 480 KiB needed,
/// so a level costing more than 0.8 KiB overflows. TypeScript 7.0.2: `1`,
/// under every setting.
#[test]
fn class_methods_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(NESTED_FUNCTIONS, "class { m() { return ", "; } }"));
    answers_on(test_path!(), SMALL_STACK, source, ARROW_PROBE, "1");
}

/// Arrows nested 200 deep, each returning the next from inside a block, a
/// labeled statement, an `if` arm and a `switch` clause: a branch
/// statement's regions are frames of its region's run, so the returned
/// function is evaluated from the stack of evaluators. 480 KiB needed, so
/// a level costing more than 0.8 KiB overflows. TypeScript 7.0.2: `1`,
/// under every setting.
#[test]
fn arrows_returned_from_statements_nested_deep_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for (open, close) in [
            ("() => { { return ", "; } }"),
            ("() => { l: { return ", "; } }"),
            ("() => { if (b) { return ", "; } throw 0; }"),
            ("() => { switch (b) { case true: return ", "; } throw 0; }"),
        ] {
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(
                    &returning(wrap(NESTED_FUNCTIONS, open, close)),
                    &[(ARROW_PROBE, "1")]
                )),
                Vec::<String>::new(),
                "{open}"
            );
        }
    });
}

/// Arrows nested 200 deep, each a declarator's initializer the body
/// returns: the declarator suspends at its initializer's nested function
/// value. 480 KiB needed, so a level costing more than 0.8 KiB overflows.
/// TypeScript 7.0.2: `1`, under every setting.
#[test]
fn arrows_initializing_declarators_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(
        NESTED_FUNCTIONS,
        "() => { const f = ",
        "; return f; }",
    ));
    answers_on(test_path!(), SMALL_STACK, source, ARROW_PROBE, "1");
}

/// A destructuring declarator nested 1,000 patterns deep lowers, binds and
/// drops from explicit stacks: 416 KiB needed, so a level costing more
/// than 0.22 KiB overflows. Every element of an `any` source is `any`:
/// TypeScript 7.0.2 answers `1`, under every setting.
#[test]
fn destructuring_patterns_nested_deep_answer_on_a_small_stack() {
    let source = format!(
        "export function pf(x: any) {{ const {}v{} = x; return v; }}\n",
        "{ v: ".repeat(DEPTH),
        " }".repeat(DEPTH)
    );
    answers_on(
        test_path!(),
        SMALL_STACK,
        source,
        "0 extends (1 & ReturnType<typeof pf>) ? 1 : 2",
        "1",
    );
}

/// `await` nested 1,000 deep lowers and evaluates from explicit stacks:
/// 480 KiB needed, so a level costing more than 0.16 KiB overflows.
/// TypeScript 7.0.2: `Promise<number>`, under every setting.
#[test]
fn awaits_nested_deep_answer_on_a_small_stack() {
    let source = format!(
        "export async function pf() {{ return {}1; }}\n",
        "await ".repeat(DEPTH)
    );
    answers_on(test_path!(), SMALL_STACK, source, RETURN, "Promise<number>");
}

/// Statements nested past the plan budget return its typed budget failure,
/// the planner's refusal recorded: each lowers and evaluates as frames of
/// its region's lowering and run.
fn statements_nested_past_the_plan_budget_return(forms: [(&str, &str); 3]) {
    for (open, close) in forms {
        let source = format!(
            "export function pf(b: boolean, x: number) {{ {}return 1;{} return 2; }}\n",
            open.repeat(BUDGET_DEPTH),
            close.repeat(BUDGET_DEPTH)
        );
        assert_returns_past_the_plan_budget(source);
    }
}

/// [`statements_nested_past_the_plan_budget_return`] for `if`, `switch` and
/// `try`.
#[test]
fn branch_statements_nested_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        statements_nested_past_the_plan_budget_return([
            ("if (b) { ", " }"),
            ("switch (x) { case 1: ", " }"),
            ("try { ", " } catch { }"),
        ]);
    });
}

/// [`statements_nested_past_the_plan_budget_return`] for `while`, `for` and
/// `do`.
#[test]
fn loop_statements_nested_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        statements_nested_past_the_plan_budget_return([
            ("while (b) { ", " }"),
            ("for (;;) { ", " }"),
            ("do { ", " } while (b);"),
        ]);
    });
}

/// The source parse identity a content-addressed key names is derived once
/// per artifact: the derivations a nest's evaluation makes do not grow with
/// its depth (deriving it per nested function hashed the whole source once
/// per function, the square of the nesting).
#[test]
fn a_nest_derives_its_source_parse_identity_a_fixed_number_of_times() {
    let derivations = |depth: usize| {
        let before = verter_session_query::source::framework_parse::source_parse_identity_derivations_for_tests();
        assert_eq!(
            mismatches(&arrows(depth), &[(ARROW_PROBE, "1")]),
            Vec::<String>::new()
        );
        verter_session_query::source::framework_parse::source_parse_identity_derivations_for_tests()
            - before
    };
    assert_eq!(derivations(20), derivations(200));
}

/// Template literal types nested 10,000 deep: a template in a template's
/// hole splices into it from an explicit stack, on the
/// declaration-lowering worker; the caller's side reads on
/// [`SMALL_STACK`] (416 KiB needed). TypeScript 7.0.2: `"1"`, under every
/// setting.
#[test]
fn template_literal_types_nested_10000_deep_answer_on_a_worker() {
    let source = format!("type D = {};\n", wrap(WORKER_DEPTH, "`${", "}`"));
    answers_on(test_path!(), SMALL_STACK, source, "D", "\"1\"");
}

fn mapped_types(depth: usize) -> String {
    format!("type D = {};\n", wrap(depth, "{ [K in \"a\"]: ", " }"))
}

/// Mapped types nested deep lower as frames of the locator lowering's
/// stack (their binder frames a shared chain) and are judged open by a
/// walk whose levels carry only the arm they take. Past the
/// connected-demand work budget ([`MAPPED_WORK_BUDGET`]: the nest's work
/// grows by four units a level) the probe returns the budget's typed
/// failure, the plan budget at its production default — read on
/// [`MAPPED_STACK`], as reading `keyof D` over the nest needs.
#[test]
fn mapped_types_nested_past_the_work_budget_return() {
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_a_work_budget(
            format!(
                "{}export function pf() {{ const p: keyof D = null as any; return p; }}\n",
                mapped_types(BUDGET_DEPTH)
            ),
            MAPPED_WORK_BUDGET,
            MAPPED_STACK,
        );
    });
}

/// A mapped-type nest under the connected-demand work budget answers on
/// [`MAPPED_STACK`]: TypeScript 7.0.2 answers `"a"`, under every setting.
/// Reading `keyof D` over the nest takes 800 KiB of the caller's stack at
/// 1,000 levels as at 300 — a fixed cost of the open judgement, not a
/// level's — so a level costing more than 0.22 KiB overflows.
#[test]
fn mapped_types_nested_1000_deep_answer_on_a_one_mebibyte_stack() {
    answers_on(
        test_path!(),
        MAPPED_STACK,
        mapped_types(1_000),
        "keyof D",
        "\"a\"",
    );
}

/// Mapped types nested 10,000 deep. TypeScript 7.0.2: `"a"`, under every
/// setting.
#[test]
#[ignore = "the connected-demand work budget admits no 10,000-deep mapped-type nest"]
fn mapped_types_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches(&mapped_types(WORKER_DEPTH), &[("keyof D", "\"a\"")]),
        Vec::<String>::new()
    );
}

/// Namespaces nested 10,000 deep, read through a 10,000-segment qualified
/// name: every walk registering a namespace's members descends the nest
/// from an explicit stack, on the worker that indexes the module; the
/// caller's side reads on [`SMALL_STACK`] (416 KiB needed). TypeScript
/// 7.0.2: `1`, under every setting.
#[test]
fn namespaces_nested_10000_deep_answer_on_a_worker() {
    let source = format!(
        "namespace A {{ {}export type V = 1;{}\ntype D = {}V;\n",
        "export namespace A { ".repeat(WORKER_DEPTH - 1),
        " }".repeat(WORKER_DEPTH),
        "A.".repeat(WORKER_DEPTH)
    );
    answers_on(test_path!(), SMALL_STACK, source, "D", "1");
}

fn label_chain(depth: usize) -> String {
    let labels: String = (0..depth).map(|label| format!("l{label}: ")).collect();
    format!("export function pf() {{ {labels}return 1; }}\n")
}

fn else_if_chain(length: usize) -> String {
    format!(
        "export function pf(b: boolean) {{ {}return 2; }}\n",
        "if (b) return 1; else ".repeat(length)
    )
}

/// A statement wrapped in labels past the plan budget returns its typed
/// budget failure, the planner's refusal recorded: the parse, and the copy
/// of its program the binding index makes, run on the stack the scan
/// bounds (a braceless label nests its statement a level under it).
#[test]
fn label_chains_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_the_plan_budget(label_chain(BUDGET_DEPTH));
    });
}

/// A statement wrapped in 1,000 labels, which the flow-slice plan budget
/// admits: 416 KiB needed, so a level costing more than 0.22 KiB
/// overflows. TypeScript 7.0.2: `number`, under every setting.
#[test]
fn label_chains_deep_answer_on_a_small_stack() {
    answers_on(
        test_path!(),
        SMALL_STACK,
        label_chain(DEPTH),
        RETURN,
        "number",
    );
}

/// An `else if` chain 10,000 long. TypeScript 7.0.2: `1 | 2` (reporting
/// TS2563, body too large for control-flow analysis), under every setting.
#[test]
#[ignore = "the flow-slice plan budget admits no demand slice of 10,001 return sites"]
fn else_if_chains_10000_long_answer_on_production_stacks() {
    assert_eq!(
        mismatches(&else_if_chain(WORKER_DEPTH), &[(RETURN, "1 | 2")]),
        Vec::<String>::new()
    );
}

/// An `else if` chain past the plan budget returns its typed budget
/// failure, the planner's refusal recorded: the parse, and the copy of its
/// program the binding index makes, run on the stack the scan bounds (an
/// `else` continues its `if` past the `;` ending the `if`'s body).
#[test]
fn else_if_chains_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_the_plan_budget(else_if_chain(BUDGET_DEPTH));
    });
}

/// `else if` chains 250, 300 and 800 long: each return site is work the
/// connected demand pays for, not a fixed ceiling. 480 KiB needed at 800,
/// so an arm costing more than 0.2 KiB overflows. TypeScript 7.0.2: `1 |
/// 2`, under every setting.
#[test]
fn else_if_chains_250_300_and_800_long_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for length in [250, 300, 800] {
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(
                    &else_if_chain(length),
                    &[(RETURN, "1 | 2")]
                )),
                Vec::<String>::new(),
                "{length} arms"
            );
        }
    });
}

/// A `switch` whose `cases` cases each return their own literal, and whose
/// `default` returns `-1`.
fn switch_returns(cases: usize) -> String {
    let arms: String = (0..cases)
        .map(|case| format!("case {case}: return {case}; "))
        .collect();
    format!("export function pf(x: number) {{ switch (x) {{ {arms}default: return -1; }} }}\n")
}

/// The union `-1 | 0 | 1 | … | cases - 1` of [`switch_returns`]'s literals.
fn switch_returns_union(cases: usize) -> &'static str {
    let union: Vec<String> = std::iter::once("-1".to_owned())
        .chain((0..cases).map(|case| case.to_string()))
        .collect();
    Box::leak(union.join(" | ").into_boxed_str())
}

/// A `switch` with 300 and with 800 returning cases and a returning
/// `default`: the return is the union of every case's literal. TypeScript
/// 7.0.2: `-1 | 0 | 1 | … | 299` and `-1 | 0 | 1 | … | 799`, under every
/// setting.
#[test]
fn switches_with_300_and_800_returning_cases_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for cases in [300, 800] {
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(
                    &switch_returns(cases),
                    &[(RETURN, switch_returns_union(cases))]
                )),
                Vec::<String>::new(),
                "{cases} cases"
            );
        }
    });
}

/// A `switch` with more returning cases than the plan budget admits returns
/// the typed budget failure, the planner's refusal recorded.
#[test]
fn switches_with_returning_cases_past_the_plan_budget_return() {
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_the_plan_budget(switch_returns(BUDGET_DEPTH));
    });
}

/// A nest whose demand slice the flow-slice plan budget admits: the
/// unreachable and dead-path chains below carry one and two returns per
/// level.
const UNDER_THE_SLICE_BUDGET: usize = 120;

/// Unreachable code nested deep, each level a block behind a `return`: each
/// unreachable region evaluates as a frame of its enclosing region's run.
/// Past the plan budget the return is its typed budget failure; under the
/// slice budget TypeScript 7.0.2 answers `number`, under every setting.
#[test]
fn unreachable_code_nested_deep_returns_on_a_small_stack() {
    let source = |depth: usize| {
        format!(
            "export function pf() {{ {}return 1;{} }}\n",
            "return 1; { ".repeat(depth),
            " }".repeat(depth)
        )
    };
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_the_plan_budget(source(BUDGET_DEPTH));
        assert_eq!(
            on_a_stack(SMALL_STACK, move || mismatches(
                &source(UNDER_THE_SLICE_BUDGET),
                &[(RETURN, "number")]
            )),
            Vec::<String>::new()
        );
    });
}

/// Code past an exhaustive `switch` nested deep: each region's statements
/// past the dead path evaluate on its own dead tail, a frame of the run.
/// Past the plan budget the return is its typed budget failure; under the
/// slice budget TypeScript 7.0.2 answers `number`, under every setting.
#[test]
fn code_past_exhaustive_switches_nested_deep_returns_on_a_small_stack() {
    let source = |depth: usize| {
        format!(
            "export function pf(x: \"a\" | \"b\") {{ {}return 1;{} }}\n",
            "switch (x) { case \"a\": return 1; case \"b\": return 1; } { ".repeat(depth),
            " }".repeat(depth)
        )
    };
    in_a_fresh_process(test_path!(), || {
        assert_returns_past_the_plan_budget(source(BUDGET_DEPTH));
        assert_eq!(
            on_a_stack(SMALL_STACK, move || mismatches(
                &source(UNDER_THE_SLICE_BUDGET),
                &[(RETURN, "number")]
            )),
            Vec::<String>::new()
        );
    });
}

/// A nest of mapped types lowers in work that grows with the nest, not its
/// square: each binder's identity reads the mapped types nested in it once
/// (each hashed its whole value subtree), and each body's binder stack
/// shares the frames around it (each copied them).
#[test]
fn a_mapped_type_nest_lowers_in_linear_work() {
    let work = |depth: usize| {
        let walked = verter_type_engine::mapper_binder_registry::type_expr_visits_for_tests();
        let copied = super::locator_shape::binder_frame_clones_for_tests();
        assert_eq!(
            mismatches(&mapped_types(depth), &[("keyof D", "\"a\"")]),
            Vec::<String>::new()
        );
        (
            verter_type_engine::mapper_binder_registry::type_expr_visits_for_tests() - walked,
            super::locator_shape::binder_frame_clones_for_tests() - copied,
        )
    };
    let ((shallow_walked, shallow_copied), (deep_walked, deep_copied)) = (work(50), work(500));
    assert!(
        deep_walked <= 20 * shallow_walked,
        "a ten-times-deeper nest walked {deep_walked} nodes against {shallow_walked}"
    );
    assert!(
        deep_copied <= 20 * shallow_copied,
        "a ten-times-deeper nest copied {deep_copied} binder frames against {shallow_copied}"
    );
}

/// Loops behind a literal `false` test nested deep: each dead body
/// evaluates as a frame of the run, on a dead path its pass restores. Past
/// the plan budget the return is its typed budget failure; 1,000 deep (416
/// KiB needed, so a level costing more than 0.22 KiB overflows) TypeScript
/// 7.0.2 answers `1 | 2`, under every setting.
#[test]
fn dead_loops_nested_deep_return_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for open in ["while (false) { ", "for (; false; ) { "] {
            let source = |depth: usize| {
                format!(
                    "export function pf() {{ {}return 1;{} return 2; }}\n",
                    open.repeat(depth),
                    " }".repeat(depth)
                )
            };
            assert_returns_past_the_plan_budget(source(BUDGET_DEPTH));
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(
                    &source(DEPTH),
                    &[(RETURN, "1 | 2")]
                )),
                Vec::<String>::new(),
                "{open}"
            );
        }
    });
}

/// Loops nested `depth` deep, the innermost writing `body` over `let x: 0 |
/// 1 = 0` (and `y`, alike).
fn nested_loops(depth: usize, body: &str, result: &str) -> String {
    format!(
        "export function pf(b: boolean) {{ let x: 0 | 1 = 0; let y: 0 | 1 = 0; {}{body}{} return {result}; }}\n",
        "while (b) { ".repeat(depth),
        " }".repeat(depth)
    )
}

/// Nested loops take work polynomial in their depth: a loop pass from a
/// state an earlier pass of the loop started from is that pass, so the
/// nested loops of a pass whose head did not change are not evaluated
/// again (each loop's body ran once per pass of every loop around it, `2 ^
/// depth` times). TypeScript 7.0.2, 20 deep, under every setting: `0` for
/// `x = 0`, `readonly [0, 0]` for `x = 0; y = 0`, `0 | 1` for `x = 1`.
#[test]
fn nested_loops_take_passes_polynomial_in_their_depth() {
    for (body, result, answer) in [
        ("x = 0;", "x", "0"),
        ("x = 0; y = 0;", "[x, y] as const", "readonly [0, 0]"),
        ("x = 1;", "x", "0 | 1"),
    ] {
        let passes = |depth: usize| {
            let before = super::flow_return::loop_passes_for_tests();
            assert_eq!(
                mismatches(&nested_loops(depth, body, result), &[(RETURN, answer)]),
                Vec::<String>::new(),
                "{body}"
            );
            super::flow_return::loop_passes_for_tests() - before
        };
        let (shallow, deep) = (passes(10), passes(20));
        assert!(
            deep <= 5 * shallow,
            "`{body}` 20 deep took {deep} passes against {shallow} 10 deep"
        );
    }
}

/// Enums `length` long, each member reading the member of the enum before
/// it: `enum E0 { A = 1 } enum E1 { A = E0.A } … enum E<length> { A =
/// E<length - 1>.A }`.
fn enum_chain(length: usize) -> String {
    let mut source = String::from("enum E0 { A = 1 }\n");
    for index in 1..=length {
        source.push_str(&format!("enum E{index} {{ A = E{}.A }}\n", index - 1));
    }
    source
}

/// One enum whose members each read the member before them, seeded by
/// another enum's member: `enum X { A = 1 } enum E { A0 = X.A, A1 = A0 +
/// 1, … }`.
fn enum_member_chain(length: usize) -> String {
    let members: Vec<String> = (1..length)
        .map(|index| format!("A{index} = A{} + 1", index - 1))
        .collect();
    format!(
        "enum X {{ A = 1 }}\nenum E {{ A0 = X.A, {} }}\n",
        members.join(", ")
    )
}

/// An enum's member reading through a chain of 1,000 enums is evaluated
/// from an explicit stack: 416 KiB needed, so a link costing more than
/// 0.22 KiB overflows. TypeScript 7.0.2: `E<length>.A extends 1 ? "y" :
/// "n"` is `"y"`, under every `strictNullChecks` × `noImplicitAny` setting.
#[test]
fn enum_reference_chains_long_answer_on_a_small_stack() {
    let probe = Box::leak(format!("E{DEPTH}.A extends 1 ? \"y\" : \"n\"").into_boxed_str());
    answers_on(test_path!(), SMALL_STACK, enum_chain(DEPTH), probe, "\"y\"");
}

/// One enum of 1,000 members, each the member before it plus one, the
/// first another enum's member: 416 KiB needed, so a member costing more
/// than 0.22 KiB overflows. TypeScript 7.0.2: `E.A<length - 1> extends
/// <length> ? "y" : "n"` is `"y"`, under every setting.
#[test]
fn enum_member_chains_long_answer_on_a_small_stack() {
    let probe =
        Box::leak(format!("E.A{} extends {DEPTH} ? \"y\" : \"n\"", DEPTH - 1).into_boxed_str());
    answers_on(
        test_path!(),
        SMALL_STACK,
        enum_member_chain(DEPTH),
        probe,
        "\"y\"",
    );
}

/// Enum initializers nested 1,000 deep: an addition nested in parentheses
/// to the right, a left-leaning chain of additions, and a chain of
/// additions of another enum's member — 416 KiB needed, so a level costing
/// more than 0.22 KiB overflows. TypeScript 7.0.2: each member extends
/// `<depth> + 1` (`"y"`), under every setting.
#[test]
fn enum_initializers_nested_deep_answer_on_a_small_stack() {
    let probe: &'static str =
        Box::leak(format!("D.A extends {} ? \"y\" : \"n\"", DEPTH + 1).into_boxed_str());
    in_a_fresh_process(test_path!(), || {
        for (form, initializer) in [
            ("parenthesized", wrap(DEPTH, "(1 + ", ")")),
            ("left-leaning", format!("{}1", "1 + ".repeat(DEPTH))),
            ("member reads", format!("{}1", "X.A + ".repeat(DEPTH))),
        ] {
            let source = format!("enum X {{ A = 1 }}\nenum D {{ A = {initializer} }}\n");
            let rows: [(&'static str, &'static str); 1] = [(probe, "\"y\"")];
            assert_eq!(
                on_a_stack(SMALL_STACK, move || mismatches(&source, &rows)),
                Vec::<String>::new(),
                "{form}"
            );
        }
    });
}

/// Evaluating an enum's pending members evaluates each initializer, and
/// resolves each reference, once: the work of a chain of references grows
/// with its length, whether the chain runs through enums or through the
/// members of one enum.
#[test]
fn an_enum_reference_chain_evaluates_in_linear_work() {
    let work = |source: String, probe: &'static str| {
        let before = super::enum_type::enum_initializer_work_for_tests();
        assert_eq!(
            mismatches(&source, &[(probe, "\"y\"")]),
            Vec::<String>::new()
        );
        super::enum_type::enum_initializer_work_for_tests() - before
    };
    let through_enums = |length: usize| {
        let probe = Box::leak(format!("E{length}.A extends 1 ? \"y\" : \"n\"").into_boxed_str());
        work(enum_chain(length), probe)
    };
    let through_members = |length: usize| {
        let probe = Box::leak(
            format!("E.A{} extends {length} ? \"y\" : \"n\"", length - 1).into_boxed_str(),
        );
        work(enum_member_chain(length), probe)
    };
    assert!(through_enums(200) > 0);
    assert_eq!(through_enums(400), 2 * through_enums(200));
    assert!(through_members(200) > 0);
    assert_eq!(through_members(400), 2 * through_members(200));
}

/// A module of an interface of `length` members `k0 … k{length-1}` and the
/// union of their names, twice over (`K`).
fn index_key_union(length: usize) -> String {
    let members: String = (0..length).map(|i| format!("  k{i}: {i};\n")).collect();
    let names: Vec<String> = (0..length).map(|i| format!("\"k{i}\"")).collect();
    format!(
        "interface I {{\n{members}}}\ntype K = {union} | {union};\n",
        union = names.join(" | ")
    )
}

/// An access by a union of keys reads each distinct key once, and its key
/// set is deduplicated — across a union's members and against an
/// intersection's — in work linear in the keys: a scan of the keys kept so
/// far made a long key union quadratic.
///
/// Measured on TypeScript 7.0.2 (`--strict`): `I[K] extends number ? "y" :
/// "n"` and `I[keyof I & K] extends number ? "y" : "n"` are `"y"` at both
/// lengths.
///
/// Mutation: deduplicating through a scan of the keys kept so far takes
/// four times the comparisons at twice the length.
#[test]
fn an_access_by_a_key_union_dedups_in_linear_work() {
    let work = |length: usize| {
        let before = super::build::index_key_probes_for_tests();
        let source = index_key_union(length);
        assert_eq!(
            mismatches(
                &source,
                &[
                    ("I[K] extends number ? \"y\" : \"n\"", "\"y\""),
                    ("I[keyof I & K] extends number ? \"y\" : \"n\"", "\"y\""),
                ],
            ),
            Vec::<String>::new()
        );
        super::build::index_key_probes_for_tests() - before
    };
    let short = work(200);
    assert!(short > 0);
    assert_eq!(work(400), 2 * short);
}

/// A chain of `length` aliases down to `T | undefined`, a generic
/// function declared to return its end, and the call's result kept as a
/// `const` and joined with a pinned `"ok"` in a return.
fn alias_chain(length: usize) -> String {
    let mut source = String::from("type A0<T> = T | undefined;\n");
    for level in 1..=length {
        source.push_str(&format!("type A{level}<T> = A{}<T>;\n", level - 1));
    }
    source.push_str(&format!(
        "declare function f<T>(x: T): A{length}<T>;\n\
         export const c = f(\"ok\");\n\
         export function gj(b: boolean) {{ if (b) {{ return f(\"ok\"); }} return \"ok\" as const; }}\n"
    ));
    source
}

/// The evaluated mismatches of `rows` over `source`, read on an 8 MiB
/// thread, a declaration-lowering worker's: instantiating an alias whose
/// body applies another alias still opens the next instantiation one
/// native query deeper, so a chain of aliases takes native stack per alias
/// (the connected demand's query-depth cap bounds how many).
fn evaluated_mismatches_on_a_worker_stack(
    source: String,
    rows: &'static [(&'static str, &'static str)],
) -> Vec<String> {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || super::checker_probe_lane_tests::evaluated_mismatches(&source, rows))
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

const ALIAS_CHAIN_ROWS: &[(&str, &str)] = &[
    ("typeof c", "\"ok\" | undefined"),
    ("ReturnType<typeof gj>", "\"ok\" | undefined"),
];

/// A fresh `"ok"` deposited for `T` is kept at the call when `T` is at
/// the top level of the declared return, through any number of aliases:
/// the binder walk reads the chain from a work list, one alias
/// instantiation at a time, however long it is.
#[test]
fn alias_chains_20_long_keep_a_top_level_deposit() {
    assert_eq!(
        evaluated_mismatches_on_a_worker_stack(alias_chain(20), ALIAS_CHAIN_ROWS),
        Vec::<String>::new()
    );
}

/// `{name}0 = {base}`, then `{name}{n} = {name}{n - 1}` up to `length`.
fn named_chain(name: &str, length: usize, base: &str) -> String {
    let mut source = format!("type {name}0 = {base};\n");
    for level in 1..=length {
        source.push_str(&format!("type {name}{level} = {name}{};\n", level - 1));
    }
    source
}

/// A target signature whose result names `void` through a chain of twelve
/// aliases accepts any source result: `(() => number) extends (() => V12)`
/// is `1` (TypeScript 7.0.2, all four settings). The chain is followed to
/// its end, however long.
#[test]
fn a_void_result_through_a_12_alias_chain_accepts_any_result() {
    let source = named_chain("V", 12, "void");
    assert_eq!(
        evaluated_mismatches_on_a_worker_stack(
            source,
            &[("(() => number) extends (() => V12) ? 1 : 2", "1")]
        ),
        Vec::<String>::new()
    );
}

/// A rest element names its array through a declaration and through a
/// chain of twelve aliases: `[string, 1, 2]` is assignable to
/// `[string, ...R]` for `type R = number[]` and for `R12` down to it
/// (TypeScript 7.0.2, all four settings).
#[test]
fn a_rest_element_names_its_array_through_aliases() {
    for source in [
        "type R = number[];\ntype T = [string, ...R];\n".to_owned(),
        format!(
            "{}type T = [string, ...R12];\n",
            named_chain("R", 12, "number[]")
        ),
    ] {
        assert_eq!(
            evaluated_mismatches_on_a_worker_stack(
                source,
                &[("[string, 1, 2] extends T ? 1 : 2", "1")]
            ),
            Vec::<String>::new()
        );
    }
}

/// A base class whose instance type is an intersection nested twelve times
/// through aliases (`I{n} = I{n - 1} & { k{n}: n }`) is a valid base: its
/// derived class reads every member (TypeScript 7.0.2, all four settings).
#[test]
fn a_base_through_12_nested_intersections_is_valid() {
    let mut source = String::from("type I0 = { k0: 0 };\n");
    for level in 1..=12 {
        source.push_str(&format!(
            "type I{level} = I{} & {{ k{level}: {level} }};\n",
            level - 1
        ));
    }
    source.push_str("declare const Base: new () => I12;\nexport class C extends Base {}\n");
    assert_eq!(
        evaluated_mismatches_on_a_worker_stack(source, &[("C[\"k0\"]", "0"), ("C[\"k12\"]", "12")]),
        Vec::<String>::new()
    );
}

/// An index of named key unions nested twelve times
/// (`K{n} = K{n - 1} | "k{n}"`) enumerates every key:
/// `Box[K12]` reads all thirteen members (TypeScript 7.0.2, all four
/// settings).
#[test]
#[ignore = "an index by a named key union that names another key union evaluates to no value"]
fn an_index_through_12_nested_key_unions_enumerates_every_key() {
    let mut source = String::from("type Box = {");
    for level in 0..=12 {
        source.push_str(&format!(" k{level}: {level};"));
    }
    source.push_str(" };\ntype K0 = \"k0\";\n");
    for level in 1..=12 {
        source.push_str(&format!("type K{level} = K{} | \"k{level}\";\n", level - 1));
    }
    assert_eq!(
        evaluated_mismatches_on_a_worker_stack(
            source,
            &[(
                "Box[K12]",
                "0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12"
            )]
        ),
        Vec::<String>::new()
    );
}
