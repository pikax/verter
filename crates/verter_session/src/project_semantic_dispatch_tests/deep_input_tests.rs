//! Every way syntax nests, deep, answered through Verter on a stack
//! smaller than any it runs on in production. A nest's flow evaluation —
//! its expressions, statements, calls and nested function values — runs in
//! place on the thread that asks (the caller's, 1 MiB at the least a host
//! asks from), so each form below is evaluated on a [`SMALL_STACK`] thread,
//! nested [`DEPTH`] levels: an evaluator spending a native level per nesting
//! level overflows that stack within the first few dozen levels (evaluating
//! a nested function in place costs about 53 KiB a level unoptimized; an
//! operand or a call route several KiB), where the explicit stacks the
//! evaluators keep cost none. The oxc-only parse of each form is recorded
//! in `docs/evidence/signature-kernel/oxc-deep-parse.md`; these are the
//! same forms, as a return or a type the probe reads.
//!
//! A declaration's body lowers on a declaration-lowering worker, on its
//! own 8 MiB, which no caller's thread bounds: the forms that nest inside a
//! type alias stay at 10,000 levels, where a native level per nesting
//! level overflows that worker.
//!
//! Each answer is TypeScript 7.0.2's for the same module and probe
//! (`--noEmit --strict`), measured at the depth the checker was asked; a
//! depth the checker answers at answers the same at every depth below it.
//!
//! A form whose demand slice outgrows the flow-slice plan budget
//! (`FlowSliceBudget::max_selected_nodes`), or whose evaluation outgrows
//! the connected-demand work budget, answers a typed budget failure, not a
//! type. Those forms are checked for their answer under the budgets, and
//! for the typed failure past them — under budgets lowered to a few hundred
//! nodes and a fraction of the production work, so the trip is reached at
//! [`BUDGET_DEPTH`] rather than at the production budgets' ten thousand.

use super::checker_probe_lane_tests::{
    default_probe_host, flow_return_outcome_on_host, mismatches,
};
use crate::host_flow_return_audit::FlowReturnError;
use verter_session_query::flow::peeker::FlowSliceBudget;
use verter_type_engine::semantic_query::FlowReturnFailure;
use verter_type_expr::facts::InferenceUnavailableReason;

/// The stack every nest's flow evaluation runs on here: three quarters of
/// the 1 MiB a host asks from at least. A probe's fixed work — the host,
/// the upsert, the dispatch around the evaluation — takes about 450 KiB of
/// it unoptimized (a probe of two levels overflows 448 KiB and answers on
/// 512 KiB), so a native level per nesting level has a quarter MiB to
/// overflow within: a few levels of a nested function evaluated in place,
/// a few dozen of an operand or a call route.
pub(super) const SMALL_STACK: usize = 768 << 10;

/// The depth every expression and statement nest is checked at on the
/// small stack.
const DEPTH: usize = 1_000;

/// The depth a nest of function values is checked at: evaluating each
/// nested function in place costs about 53 KiB a level, so a tenth of this
/// overflows [`SMALL_STACK`].
const NESTED_FUNCTIONS: usize = 200;

/// The depth a nest of calls is checked at: a call's route is the dearest
/// frame the evaluator keeps, and the connected demand charges each call
/// once, so a nest this deep answers in a fraction of a second.
const CALLS: usize = 250;

/// The depth a nest whose production-size answer is a budget failure is
/// checked at, under [`PLAN_BUDGET`] and [`WORK_BUDGET`].
const BUDGET_DEPTH: usize = 400;

/// The flow-slice plan budget the budget-failure forms run under: a nest of
/// [`BUDGET_DEPTH`] statements selects more nodes than this.
const PLAN_BUDGET: u32 = 256;

/// The connected-demand work budget the budget-failure forms run under, a
/// fortieth of production's: a `keyof` chain of [`BUDGET_DEPTH`] levels
/// charges more than this (its work grows with the square of its depth).
const WORK_BUDGET: usize = 100_000;

/// The connected-demand work budget a mapped-type nest is read under: the
/// nest's work grows by four units a level, so [`BUDGET_DEPTH`] levels
/// charge about 1,600 — past this, where a nest a quarter as deep answers.
const MAPPED_WORK_BUDGET: usize = 1_000;

/// The depth a form that lowers inside a type alias — on a
/// declaration-lowering worker's 8 MiB — is checked at.
const WORKER_DEPTH: usize = 10_000;

/// `f` on a [`SMALL_STACK`] thread.
fn on_a_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(f)
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

/// The mismatches of `probe` over `source` against `checker`, read on a
/// [`SMALL_STACK`] thread.
fn mismatches_on_a_small_stack(
    source: String,
    probe: &'static str,
    checker: &'static str,
) -> Vec<String> {
    on_a_small_stack(move || mismatches(&source, &[(probe, checker)]))
}

/// The typed outcome of `pf`'s body-derived return over `source`, read on
/// a [`SMALL_STACK`] thread under the lowered [`PLAN_BUDGET`] and
/// [`WORK_BUDGET`]: the degradation of the value it produced, or the typed
/// error it answered instead, a missing `pf` (`Failure(Missing)`) told
/// apart from a budget. It returns either way, never aborting the host.
fn return_under_small_budgets(
    source: String,
) -> Result<Option<verter_type_engine::semantic_query::FlowReturnDegradation>, FlowReturnError> {
    return_under_a_work_budget(source, WORK_BUDGET)
}

/// [`return_under_small_budgets`] under a connected-demand work budget of
/// `work` in place of [`WORK_BUDGET`].
fn return_under_a_work_budget(
    source: String,
    work: usize,
) -> Result<Option<verter_type_engine::semantic_query::FlowReturnDegradation>, FlowReturnError> {
    on_a_small_stack(move || {
        let _work = super::connected_demand::WorkBudgetForTests::install(work);
        let host = default_probe_host();
        host.project_type_store()
            .flow_slice()
            .set_budget_for_test(FlowSliceBudget {
                max_selected_nodes: PLAN_BUDGET,
                ..FlowSliceBudget::default()
            });
        flow_return_outcome_on_host(&host, &source, "pf")
    })
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
/// charges, evaluated cold on a host of its own.
fn connected_work_of(source: &str, function: &str) -> usize {
    use std::sync::Arc;
    use verter_type_engine::semantic_query::{
        ReturnProjectionDemand, SemanticQueryApi, SemanticQueryKey,
    };
    let host = default_probe_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        super::checker_probe_lane_tests::PROBE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(source),
        crate::FileLanguage::script_ts(),
    );
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(super::checker_probe_lane_tests::PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(function),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let key =
        dispatch.flow_return_key_with_demand(&identity, ReturnProjectionDemand::whole_return());
    let _ = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)));
    dispatch.connected_demand_usage().work
}

/// The connected work a nest of `depth` levels charges grows with its
/// depth, not its square: each level is charged once, however many
/// consumers read it, so the work budget admits a nest of any depth it has
/// room for. `nest` builds the module and names the function read.
fn assert_work_is_linear(nest: impl Fn(usize) -> (String, &'static str)) {
    let work = |depth: usize| {
        let (source, function) = nest(depth);
        connected_work_of(&source, function)
    };
    let (at_100, at_200, at_400) = (work(100), work(200), work(400));
    let (first, second) = (at_200 - at_100, at_400 - at_200);
    assert!(
        at_200 > at_100 && second <= first * 2 + first / 10,
        "work at 100 / 200 / 400 levels: {at_100} / {at_200} / {at_400}"
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

#[test]
fn parentheses_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(DEPTH, "(", ")"));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "number"),
        Vec::<String>::new()
    );
}

/// A chain of member reads off a constructed value (`new C().c.c…`): its
/// lowering and its evaluation read the chain from explicit stacks,
/// deciding once that it is rooted at a value, so the work grows with the
/// chain's length. TypeScript 7.0.2: `C`, under every setting.
#[test]
fn value_rooted_member_chains_long_answer_on_a_small_stack() {
    let source = format!(
        "class C {{ c: C = this; }}\nexport function pf() {{ return new C(){}; }}\n",
        ".c".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "C"),
        Vec::<String>::new()
    );
}

#[test]
fn logical_nots_nested_deep_answer_on_a_small_stack() {
    let source = returning(format!("{}1", "!".repeat(DEPTH)));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "boolean"),
        Vec::<String>::new()
    );
}

#[test]
fn template_holes_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(DEPTH, "`${", "}`"));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "string"),
        Vec::<String>::new()
    );
}

#[test]
fn conditionals_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(conditionals(DEPTH), RETURN, "1 | 2"),
        Vec::<String>::new()
    );
}

#[test]
fn objects_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(objects(DEPTH), "keyof ReturnType<typeof pf>", "\"v\""),
        Vec::<String>::new()
    );
}

#[test]
fn blocks_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(blocks(DEPTH), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Conditionals, object literals and blocks nested past the plan budget
/// return its typed budget failure on the small stack.
#[test]
fn conditionals_objects_and_blocks_nested_past_the_plan_budget_return() {
    for (form, source) in [
        ("conditionals", conditionals(BUDGET_DEPTH)),
        ("objects", objects(BUDGET_DEPTH)),
        ("blocks", blocks(BUDGET_DEPTH)),
    ] {
        assert_eq!(
            return_under_small_budgets(source),
            WORK_BUDGET_EXCEEDED,
            "{form}"
        );
    }
}

/// A conditional nested deep as an object member's value evaluates its
/// branches from the evaluator's stack (a return's conditional is read arm
/// by arm instead, for its per-arm freshness).
#[test]
fn a_member_conditional_nested_deep_answers_on_a_small_stack() {
    let source = format!(
        "export function pf(b: boolean) {{ return {{ v: {}2 }}; }}\n",
        "b ? 1 : ".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, "ReturnType<typeof pf>[\"v\"]", "number"),
        Vec::<String>::new()
    );
}

/// A function type returning a function type, 10,000 deep, and the same
/// for constructor types: each signature's return lowers from the locator
/// lowering's explicit stack, on the declaration-lowering worker.
#[test]
fn function_and_constructor_types_nested_10000_deep_answer_on_a_worker() {
    for arrow in ["() => ", "new () => "] {
        let source = format!("type D = {}1;\n", arrow.repeat(WORKER_DEPTH));
        assert_eq!(
            mismatches(&source, &[("D extends Function ? 1 : 2", "1")]),
            Vec::<String>::new(),
            "{arrow}"
        );
    }
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

#[test]
fn calls_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(calls(CALLS), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Each call of a nest is charged once to the connected demand, however
/// many consumers read it: the work grows with the nest, so the work
/// budget admits every nest it has room for.
#[test]
fn calls_nested_deep_charge_linear_work() {
    assert_work_is_linear(|depth| (calls(depth), "pf"));
}

fn arrays(depth: usize) -> String {
    returning(wrap(depth, "[", "]"))
}

/// `pf`'s return over `source`, read on a [`SMALL_STACK`] thread: how many
/// mutable arrays it nests, and whether the innermost one's element is
/// `number`. Read from a loop: the checker-print comparison descends a
/// bounded depth of its own.
fn array_nest_on_a_small_stack(source: String) -> (usize, bool) {
    array_nest_of_on_a_small_stack(source, RETURN)
}

/// [`array_nest_on_a_small_stack`] for `probe`.
fn array_nest_of_on_a_small_stack(source: String, probe: &'static str) -> (usize, bool) {
    on_a_small_stack(move || array_nest_of(&source, probe))
}

/// [`array_nest_of_on_a_small_stack`] on this thread.
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

/// Array literals nested 65 deep, past any depth a shallow per-expression
/// inference bounds: the planner opens a site per element and the
/// evaluator evaluates each array literal as a frame of its stack, so the
/// return is `number` under 65 array dimensions, as the checker's is.
#[test]
fn arrays_nested_65_deep_answer_on_a_small_stack() {
    assert_eq!(array_nest_on_a_small_stack(arrays(65)), (65, true));
}

/// Array literals nested deep: the content half lowers each literal as a
/// frame of its stack, element by element, so no level costs a native one.
#[test]
fn arrays_nested_deep_answer_on_a_small_stack() {
    assert_eq!(array_nest_on_a_small_stack(arrays(DEPTH)), (DEPTH, true));
}

/// Array literals nested past the plan budget return its typed budget
/// failure on the small stack.
#[test]
fn arrays_nested_past_the_plan_budget_return() {
    assert_eq!(
        return_under_small_budgets(arrays(BUDGET_DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
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
    assert_eq!(
        array_nest_of_on_a_small_stack(module_const_arrays(65), "typeof x"),
        (65, true)
    );
}

/// A thousand levels, within the inference's work budget: no level costs a
/// native one.
#[test]
fn module_const_arrays_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        array_nest_of_on_a_small_stack(module_const_arrays(DEPTH), "typeof x"),
        (DEPTH, true)
    );
}

/// Type arguments nested 10,000 deep lower from the locator lowering's
/// explicit stack, on the declaration-lowering worker.
#[test]
fn type_arguments_nested_10000_deep_answer_on_a_worker() {
    let source = format!(
        "interface Box<T> {{ v: T }}\ntype D = {};\n",
        wrap(WORKER_DEPTH, "Box<", ">")
    );
    assert_eq!(
        mismatches(&source, &[("D extends Box<unknown> ? 1 : 2", "1")]),
        Vec::<String>::new()
    );
}

fn keyof_chain(depth: usize) -> String {
    format!("type D = {}{{ v: 1 }};\n", "keyof ".repeat(depth))
}

const KEYOF_PROBE: &str = "D extends string | number | symbol ? 1 : 2";

/// A `keyof` chain whose resolution exceeds the connected-demand work
/// budget returns its typed budget failure on the small stack.
#[test]
fn keyof_chains_past_the_work_budget_return() {
    let source = format!(
        "{}export function pf() {{ return null as unknown as ({KEYOF_PROBE}); }}\n",
        keyof_chain(BUDGET_DEPTH)
    );
    assert_eq!(return_under_small_budgets(source), WORK_BUDGET_EXCEEDED);
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
    assert_eq!(
        mismatches_on_a_small_stack(arrows(10), ARROW_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Arrow functions nested deep: each body's return reaches the next
/// function value, which its own evaluator evaluates from the drive's
/// stack of evaluators (`flow_return_nested`), not inside the one around
/// it.
#[test]
fn arrows_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(arrows(NESTED_FUNCTIONS), ARROW_PROBE, "1"),
        Vec::<String>::new()
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

#[test]
fn module_calls_nested_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(module_calls(CALLS, false), "typeof v", "1"),
        Vec::<String>::new()
    );
}

/// A module constant initialized by nested calls, read in a function,
/// returns its value on the small stack, and each call's resolution is
/// charged once to the connected demand: the work grows with the nest.
#[test]
fn module_calls_nested_deep_read_charge_linear_work() {
    assert_eq!(
        on_a_small_stack(|| {
            flow_return_outcome_on_host(&default_probe_host(), &module_calls(CALLS, true), "pf")
        }),
        RETURNS_ITS_VALUE
    );
    assert_work_is_linear(|depth| (module_calls(depth, true), "pf"));
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
/// stacks, the value its receiver. TypeScript 7.0.2: `B` for 2, 3 and 400
/// links, under every setting; each call is charged once to the connected
/// demand, so the work grows with the chain.
#[test]
fn receiver_chains_deep_answer_on_a_small_stack_and_charge_linear_work() {
    for links in [2, 3, CALLS] {
        assert_eq!(
            mismatches_on_a_small_stack(receiver_chain(links), RETURN, "B"),
            Vec::<String>::new(),
            "{links} links"
        );
    }
    assert_work_is_linear(|links| (receiver_chain(links), "pf"));
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
/// of the value of the call before it, from an explicit stack. TypeScript
/// 7.0.2: `B` for 2, 3 and 400 links, under every setting.
#[test]
fn module_receiver_chains_answer_on_a_small_stack() {
    for links in [2, 3, CALLS] {
        assert_eq!(
            mismatches_on_a_small_stack(module_receiver_chain(links), "typeof v", "B"),
            Vec::<String>::new(),
            "{links} links"
        );
    }
}

/// A module constant initialized by a receiver chain, read in a function,
/// returns its value on the small stack, each call charged once to the
/// connected demand: the work grows with the chain.
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
    assert_eq!(
        on_a_small_stack(move || flow_return_outcome_on_host(
            &default_probe_host(),
            &read(CALLS).0,
            "pf"
        )),
        RETURNS_ITS_VALUE
    );
    assert_work_is_linear(read);
}

/// Callbacks nested deep, each an arrow passed to a callee that declares
/// its type (`a(() => a(() => … 1))`): each callback is an argument its
/// call evaluates from the evaluator's stack, its body from the stack of
/// evaluators. TypeScript 7.0.2: `number`, under every `strictNullChecks`
/// × `noImplicitAny` setting.
#[test]
fn callbacks_nested_deep_answer_on_a_small_stack() {
    let source = format!(
        "declare function a(f: () => number): number;\n{}",
        returning(wrap(NESTED_FUNCTIONS, "a(() => ", ")"))
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "number"),
        Vec::<String>::new()
    );
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

/// Generic callbacks nested deep answer on the small stack — typing a
/// callback in place instead of suspending the route overflows within a
/// few levels — and each call resolution is charged once to the connected
/// demand: the work grows with the nest. TypeScript 7.0.2: `number`, under
/// every setting.
#[test]
fn generic_callbacks_nested_deep_answer_on_a_small_stack_and_charge_linear_work() {
    assert_eq!(
        mismatches_on_a_small_stack(generic_callbacks(NESTED_FUNCTIONS), RETURN, "number"),
        Vec::<String>::new()
    );
    assert_work_is_linear(|depth| (generic_callbacks(depth), "pf"));
}

/// Immediately invoked functions nested deep, arrows with block bodies and
/// function expressions: each call's callee operand is a nested function
/// value evaluated from the stack of evaluators. TypeScript 7.0.2:
/// `number`, under every setting.
#[test]
fn immediately_invoked_functions_nested_deep_answer_on_a_small_stack() {
    for (open, close) in [
        ("(() => { return ", "; })()"),
        ("(function () { return ", "; })()"),
    ] {
        assert_eq!(
            mismatches_on_a_small_stack(
                returning(wrap(NESTED_FUNCTIONS, open, close)),
                RETURN,
                "number"
            ),
            Vec::<String>::new(),
            "{open}"
        );
    }
}

const OBJECT_PROBE: &str = "ReturnType<typeof pf> extends object ? 1 : 2";

/// Object literals whose method returns the next, nested deep: a literal's
/// methods evaluate as children of its own step. TypeScript 7.0.2: `1`,
/// under every setting.
#[test]
fn object_methods_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(NESTED_FUNCTIONS, "{ m() { return ", "; } }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, OBJECT_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Class expressions whose method returns the next, nested deep: a class's
/// member functions evaluate as children of its own step. TypeScript
/// 7.0.2: `1`, under every setting.
#[test]
fn class_methods_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(NESTED_FUNCTIONS, "class { m() { return ", "; } }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, ARROW_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Arrows nested deep, each returning the next from inside a branch
/// statement: a branch statement's regions are frames of its region's run,
/// so the returned function is evaluated from the stack of evaluators.
/// TypeScript 7.0.2: `1`, under every setting.
fn arrows_returned_from(open: &str, close: &str) {
    assert_eq!(
        mismatches_on_a_small_stack(
            returning(wrap(NESTED_FUNCTIONS, open, close)),
            ARROW_PROBE,
            "1"
        ),
        Vec::<String>::new(),
        "{open}"
    );
}

/// [`arrows_returned_from`] a block, a labeled statement, an `if` arm and
/// a `switch` clause.
#[test]
fn arrows_returned_from_statements_nested_deep_answer_on_a_small_stack() {
    arrows_returned_from("() => { { return ", "; } }");
    arrows_returned_from("() => { l: { return ", "; } }");
    arrows_returned_from("() => { if (b) { return ", "; } throw 0; }");
    arrows_returned_from("() => { switch (b) { case true: return ", "; } throw 0; }");
}

/// Arrows nested deep, each a declarator's initializer the body returns:
/// the declarator suspends at its initializer's nested function value.
/// TypeScript 7.0.2: `1`, under every setting.
#[test]
fn arrows_initializing_declarators_nested_deep_answer_on_a_small_stack() {
    let source = returning(wrap(
        NESTED_FUNCTIONS,
        "() => { const f = ",
        "; return f; }",
    ));
    assert_eq!(
        mismatches_on_a_small_stack(source, ARROW_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// A destructuring declarator nested deep lowers, binds and drops from
/// explicit stacks. Every element of an `any` source is `any`: TypeScript
/// 7.0.2 answers `1`, under every setting.
#[test]
fn destructuring_patterns_nested_deep_answer_on_a_small_stack() {
    let source = format!(
        "export function pf(x: any) {{ const {}v{} = x; return v; }}\n",
        "{ v: ".repeat(DEPTH),
        " }".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, "0 extends (1 & ReturnType<typeof pf>) ? 1 : 2", "1"),
        Vec::<String>::new()
    );
}

/// `await` nested deep lowers and evaluates from explicit stacks.
/// TypeScript 7.0.2: `Promise<number>`, under every setting.
#[test]
fn awaits_nested_deep_answer_on_a_small_stack() {
    let source = format!(
        "export async function pf() {{ return {}1; }}\n",
        "await ".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "Promise<number>"),
        Vec::<String>::new()
    );
}

/// Statements nested past the plan budget return its typed budget failure
/// on the small stack: each lowers and evaluates as frames of its region's
/// lowering and run.
fn statements_nested_past_the_plan_budget_return(forms: [(&str, &str, &str); 3]) {
    for (form, open, close) in forms {
        let source = format!(
            "export function pf(b: boolean, x: number) {{ {}return 1;{} return 2; }}\n",
            open.repeat(BUDGET_DEPTH),
            close.repeat(BUDGET_DEPTH)
        );
        assert_eq!(
            return_under_small_budgets(source),
            WORK_BUDGET_EXCEEDED,
            "{form}"
        );
    }
}

/// [`statements_nested_past_the_plan_budget_return`] for `if`, `switch` and
/// `try`.
#[test]
fn branch_statements_nested_past_the_plan_budget_return() {
    statements_nested_past_the_plan_budget_return([
        ("if", "if (b) { ", " }"),
        ("switch", "switch (x) { case 1: ", " }"),
        ("try", "try { ", " } catch { }"),
    ]);
}

/// [`statements_nested_past_the_plan_budget_return`] for `while`, `for` and
/// `do`.
#[test]
fn loop_statements_nested_past_the_plan_budget_return() {
    statements_nested_past_the_plan_budget_return([
        ("while", "while (b) { ", " }"),
        ("for", "for (;;) { ", " }"),
        ("do", "do { ", " } while (b);"),
    ]);
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
/// declaration-lowering worker. TypeScript 7.0.2: `"1"`, under every
/// setting.
#[test]
fn template_literal_types_nested_10000_deep_answer_on_a_worker() {
    let source = format!("type D = {};\n", wrap(WORKER_DEPTH, "`${", "}`"));
    assert_eq!(mismatches(&source, &[("D", "\"1\"")]), Vec::<String>::new());
}

fn mapped_types(depth: usize) -> String {
    format!("type D = {};\n", wrap(depth, "{ [K in \"a\"]: ", " }"))
}

/// Mapped types nested deep lower as frames of the locator lowering's
/// stack (their binder frames a shared chain) and are judged open by a
/// walk whose levels carry only the arm they take. Past the
/// connected-demand work budget ([`MAPPED_WORK_BUDGET`]: the nest's work
/// grows by four units a level) the probe returns the budget's typed
/// failure.
#[test]
fn mapped_types_nested_past_the_work_budget_return() {
    assert_eq!(
        return_under_a_work_budget(
            format!(
                "{}export function pf() {{ const p: keyof D = null as any; return p; }}\n",
                mapped_types(BUDGET_DEPTH)
            ),
            MAPPED_WORK_BUDGET
        ),
        WORK_BUDGET_EXCEEDED
    );
}

/// A mapped-type nest under the connected-demand work budget answers on a
/// 1 MiB thread, the smallest a host asks from: TypeScript 7.0.2 answers
/// `"a"`, under every setting. (Reading `keyof D` over the nest takes more
/// of the caller's stack than the other probes' fixed work, at any depth:
/// a nest of 300 overflows [`SMALL_STACK`] as a nest of 1,000 does, and
/// both answer on 1 MiB.)
#[test]
fn mapped_types_nested_1000_deep_answer_on_a_one_mebibyte_stack() {
    let failures = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| mismatches(&mapped_types(1_000), &[("keyof D", "\"a\"")]))
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers");
    assert_eq!(failures, Vec::<String>::new());
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
/// from an explicit stack. TypeScript 7.0.2: `1`, under every setting.
#[test]
fn namespaces_nested_10000_deep_answer_on_production_stacks() {
    let source = format!(
        "namespace A {{ {}export type V = 1;{}\ntype D = {}V;\n",
        "export namespace A { ".repeat(WORKER_DEPTH - 1),
        " }".repeat(WORKER_DEPTH),
        "A.".repeat(WORKER_DEPTH)
    );
    assert_eq!(mismatches(&source, &[("D", "1")]), Vec::<String>::new());
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
/// budget failure on the small stack: the parse, and the copy of its
/// program the binding index makes, run on the stack the scan bounds (a
/// braceless label nests its statement a level under it).
#[test]
fn label_chains_past_the_plan_budget_return() {
    assert_eq!(
        return_under_small_budgets(label_chain(BUDGET_DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
}

/// A statement wrapped in labels, which the flow-slice plan budget admits.
/// TypeScript 7.0.2: `number`, under every setting.
#[test]
fn label_chains_deep_answer_on_a_small_stack() {
    assert_eq!(
        mismatches_on_a_small_stack(label_chain(DEPTH), RETURN, "number"),
        Vec::<String>::new()
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
/// failure on the small stack: the parse, and the copy of its program the
/// binding index makes, run on the stack the scan bounds (an `else`
/// continues its `if` past the `;` ending the `if`'s body).
#[test]
fn else_if_chains_past_the_plan_budget_return() {
    assert_eq!(
        return_under_small_budgets(else_if_chain(BUDGET_DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
}

/// `else if` chains 250, 300 and 800 long: each return site is work the
/// connected demand pays for, not a fixed ceiling. TypeScript 7.0.2: `1 |
/// 2`, under every setting.
#[test]
fn else_if_chains_250_300_and_800_long_answer_on_a_small_stack() {
    for length in [250, 300, 800] {
        assert_eq!(
            mismatches_on_a_small_stack(else_if_chain(length), RETURN, "1 | 2"),
            Vec::<String>::new(),
            "{length} arms"
        );
    }
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
    for cases in [300, 800] {
        assert_eq!(
            mismatches_on_a_small_stack(switch_returns(cases), RETURN, switch_returns_union(cases)),
            Vec::<String>::new(),
            "{cases} cases"
        );
    }
}

/// A `switch` with more returning cases than the plan budget admits returns
/// the typed budget failure on the small stack.
#[test]
fn switches_with_returning_cases_past_the_plan_budget_return() {
    assert_eq!(
        return_under_small_budgets(switch_returns(BUDGET_DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
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
    assert_eq!(
        return_under_small_budgets(source(BUDGET_DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
    assert_eq!(
        mismatches_on_a_small_stack(source(UNDER_THE_SLICE_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
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
    assert_eq!(
        return_under_small_budgets(source(BUDGET_DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
    assert_eq!(
        mismatches_on_a_small_stack(source(UNDER_THE_SLICE_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
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
/// the plan budget the return is its typed budget failure; under it
/// TypeScript 7.0.2 answers `1 | 2`, under every setting.
#[test]
fn dead_loops_nested_deep_return_on_a_small_stack() {
    for open in ["while (false) { ", "for (; false; ) { "] {
        let source = |depth: usize| {
            format!(
                "export function pf() {{ {}return 1;{} return 2; }}\n",
                open.repeat(depth),
                " }".repeat(depth)
            )
        };
        assert_eq!(
            return_under_small_budgets(source(BUDGET_DEPTH)),
            WORK_BUDGET_EXCEEDED,
            "{open}"
        );
        assert_eq!(
            mismatches_on_a_small_stack(source(DEPTH), RETURN, "1 | 2"),
            Vec::<String>::new(),
            "{open}"
        );
    }
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

/// An enum's member reading through a chain of enums is evaluated from an
/// explicit stack. TypeScript 7.0.2: `E<length>.A extends 1 ? "y" : "n"`
/// is `"y"`, under every `strictNullChecks` × `noImplicitAny` setting.
#[test]
fn enum_reference_chains_long_answer_on_a_small_stack() {
    let probe = Box::leak(format!("E{DEPTH}.A extends 1 ? \"y\" : \"n\"").into_boxed_str());
    assert_eq!(
        mismatches_on_a_small_stack(enum_chain(DEPTH), probe, "\"y\""),
        Vec::<String>::new()
    );
}

/// One enum of many members, each the member before it plus one, the
/// first another enum's member. TypeScript 7.0.2: `E.A<length - 1> extends
/// <length> ? "y" : "n"` is `"y"`, under every setting.
#[test]
fn enum_member_chains_long_answer_on_a_small_stack() {
    let probe =
        Box::leak(format!("E.A{} extends {DEPTH} ? \"y\" : \"n\"", DEPTH - 1).into_boxed_str());
    assert_eq!(
        mismatches_on_a_small_stack(enum_member_chain(DEPTH), probe, "\"y\""),
        Vec::<String>::new()
    );
}

/// Enum initializers nested deep: an addition nested in parentheses to the
/// right, a left-leaning chain of additions, and a chain of additions of
/// another enum's member. TypeScript 7.0.2: each member extends `<depth> +
/// 1` (`"y"`), under every setting.
#[test]
fn enum_initializers_nested_deep_answer_on_a_small_stack() {
    let probe = Box::leak(format!("D.A extends {} ? \"y\" : \"n\"", DEPTH + 1).into_boxed_str());
    for (form, initializer) in [
        ("parenthesized", wrap(DEPTH, "(1 + ", ")")),
        ("left-leaning", format!("{}1", "1 + ".repeat(DEPTH))),
        ("member reads", format!("{}1", "X.A + ".repeat(DEPTH))),
    ] {
        let source = format!("enum X {{ A = 1 }}\nenum D {{ A = {initializer} }}\n");
        assert_eq!(
            mismatches_on_a_small_stack(source, probe, "\"y\""),
            Vec::<String>::new(),
            "{form}"
        );
    }
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

/// Calibration, run by hand: `CAL_FORM` over `CAL_DEPTH` levels on a thread
/// of `CAL_STACK` KiB, printing the time it took.
#[test]
#[ignore = "a calibration run, driven by environment variables"]
fn calibrate_a_form_on_a_stack() {
    let form = std::env::var("CAL_FORM").expect("CAL_FORM");
    let depth: usize = std::env::var("CAL_DEPTH")
        .expect("CAL_DEPTH")
        .parse()
        .unwrap();
    let stack_kib: usize = std::env::var("CAL_STACK")
        .expect("CAL_STACK")
        .parse()
        .unwrap();
    let (source, probe, checker): (String, &'static str, &'static str) = match form.as_str() {
        "arrows" => (arrows(depth), ARROW_PROBE, "1"),
        "parens" => (returning(wrap(depth, "(", ")")), RETURN, "number"),
        "nots" => (
            returning(format!("{}1", "!".repeat(depth))),
            RETURN,
            "boolean",
        ),
        "holes" => (returning(wrap(depth, "`${", "}`")), RETURN, "string"),
        "awaits" => (
            format!(
                "export async function pf() {{ return {}1; }}\n",
                "await ".repeat(depth)
            ),
            RETURN,
            "Promise<number>",
        ),
        "members" => (
            format!(
                "class C {{ c: C = this; }}\nexport function pf() {{ return new C(){}; }}\n",
                ".c".repeat(depth)
            ),
            RETURN,
            "C",
        ),
        "conditionals" => (conditionals(depth), RETURN, "1 | 2"),
        "objects" => (objects(depth), "keyof ReturnType<typeof pf>", "\"v\""),
        "blocks" => (blocks(depth), RETURN, "number"),
        "arrays" => (arrays(depth), RETURN, "number[]"),
        "calls" => (calls(depth), RETURN, "number"),
        "callbacks" => (
            format!(
                "declare function a(f: () => number): number;\n{}",
                returning(wrap(depth, "a(() => ", ")"))
            ),
            RETURN,
            "number",
        ),
        "generic_callbacks" => (generic_callbacks(depth), RETURN, "number"),
        "iife" => (
            returning(wrap(depth, "(() => { return ", "; })()")),
            RETURN,
            "number",
        ),
        "object_methods" => (
            returning(wrap(depth, "{ m() { return ", "; } }")),
            OBJECT_PROBE,
            "1",
        ),
        "class_methods" => (
            returning(wrap(depth, "class { m() { return ", "; } }")),
            ARROW_PROBE,
            "1",
        ),
        "arrows_if" => (
            returning(wrap(depth, "() => { if (b) { return ", "; } throw 0; }")),
            ARROW_PROBE,
            "1",
        ),
        "arrows_decl" => (
            returning(wrap(depth, "() => { const f = ", "; return f; }")),
            ARROW_PROBE,
            "1",
        ),
        "module_calls" => (module_calls(depth, false), "typeof v", "1"),
        "module_calls_read" => (module_calls(depth, true), RETURN, "1"),
        "receiver" => (receiver_chain(depth), RETURN, "B"),
        "module_receiver" => (module_receiver_chain(depth), "typeof v", "B"),
        "destructuring" => (
            format!(
                "export function pf(x: any) {{ const {}v{} = x; return v; }}\n",
                "{ v: ".repeat(depth),
                " }".repeat(depth)
            ),
            "0 extends (1 & ReturnType<typeof pf>) ? 1 : 2",
            "1",
        ),
        "labels" => (label_chain(depth), RETURN, "number"),
        "else_if" => (else_if_chain(depth), RETURN, "1 | 2"),
        "dead_loops" => (
            format!(
                "export function pf() {{ {}return 1;{} return 2; }}\n",
                "while (false) { ".repeat(depth),
                " }".repeat(depth)
            ),
            RETURN,
            "1 | 2",
        ),
        "enum_chain" => (
            enum_chain(depth),
            Box::leak(format!("E{depth}.A extends 1 ? \"y\" : \"n\"").into_boxed_str()),
            "\"y\"",
        ),
        "enum_members" => (
            enum_member_chain(depth),
            Box::leak(format!("E.A{} extends {depth} ? \"y\" : \"n\"", depth - 1).into_boxed_str()),
            "\"y\"",
        ),
        "enum_init_paren" => (
            format!(
                "enum X {{ A = 1 }}\nenum D {{ A = {} }}\n",
                wrap(depth, "(1 + ", ")")
            ),
            Box::leak(format!("D.A extends {} ? \"y\" : \"n\"", depth + 1).into_boxed_str()),
            "\"y\"",
        ),
        "enum_init_left" => (
            format!(
                "enum X {{ A = 1 }}\nenum D {{ A = {}1 }}\n",
                "1 + ".repeat(depth)
            ),
            Box::leak(format!("D.A extends {} ? \"y\" : \"n\"", depth + 1).into_boxed_str()),
            "\"y\"",
        ),
        "enum_init_member" => (
            format!(
                "enum X {{ A = 1 }}\nenum D {{ A = {}1 }}\n",
                "X.A + ".repeat(depth)
            ),
            Box::leak(format!("D.A extends {} ? \"y\" : \"n\"", depth + 1).into_boxed_str()),
            "\"y\"",
        ),
        "type_args" => (
            format!(
                "interface Box<T> {{ v: T }}\ntype D = {};\n",
                wrap(depth, "Box<", ">")
            ),
            "D extends Box<unknown> ? 1 : 2",
            "1",
        ),
        "fn_types" => (
            format!("type D = {}1;\n", "() => ".repeat(depth)),
            "D extends Function ? 1 : 2",
            "1",
        ),
        "template_types" => (
            format!("type D = {};\n", wrap(depth, "`${", "}`")),
            "D",
            "\"1\"",
        ),
        "mapped" => (mapped_types(depth), "keyof D", "\"a\""),
        "namespaces" => (
            format!(
                "namespace A {{ {}export type V = 1;{}\ntype D = {}V;\n",
                "export namespace A { ".repeat(depth - 1),
                " }".repeat(depth),
                "A.".repeat(depth)
            ),
            "D",
            "1",
        ),
        "keyof" => (keyof_chain(depth), KEYOF_PROBE, "1"),
        "union_chain" => {
            let mut source = String::from("type U0 = 0 | 1;\n");
            for link in 1..=depth {
                source.push_str(&format!("type U{link} = U{} | {};\n", link - 1, link + 1));
            }
            let probe: &'static str = Box::leak(format!("U{depth}").into_boxed_str());
            (source, probe, probe)
        }
        other => panic!("unknown form {other}"),
    };
    if std::env::var_os("CAL_WORK").is_some() {
        let module = format!(
            "{source}\nexport function pf() {{ const __probe: {probe} = null as any; return __probe; }}\n"
        );
        let work = connected_work_of(&module, "pf");
        eprintln!("CALIBRATION {form} depth={depth} work={work}");
        return;
    }
    let started = std::time::Instant::now();
    let failures = std::thread::Builder::new()
        .stack_size(stack_kib << 10)
        .spawn(move || mismatches(&source, &[(probe, checker)]))
        .expect("spawn")
        .join()
        .expect("answers");
    eprintln!(
        "CALIBRATION {form} depth={depth} stack={stack_kib}KiB took {:?}: {failures:?}",
        started.elapsed()
    );
    assert_eq!(failures, Vec::<String>::new());
}
