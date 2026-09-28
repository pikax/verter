//! Every way syntax nests, deep, answered through Verter on its production
//! stacks: the caller on a 1 MiB thread (the smallest a host asks from),
//! the declaration-lowering and scheduler workers on their own 8 MiB. The
//! oxc-only parse of each form is recorded in
//! `docs/evidence/signature-kernel/oxc-deep-parse.md`; these are the same
//! forms, as a return or a type the probe reads.
//!
//! Each answer is TypeScript 7.0.2's for the same module and probe
//! (`--noEmit --strict`), measured at the depth the test uses.
//!
//! A form whose demand slice at 10,000 levels selects more nodes than the
//! flow-slice plan budget admits (`FlowSliceBudget::max_selected_nodes`,
//! 4,096) answers a typed budget failure there, not a type: it is checked
//! for its answer below it (3,000 levels, 1,000 for conditionals), and at 10,000 for returning on the
//! production stacks at all.

use super::checker_probe_lane_tests::{flow_return_outcome_in, mismatches};
use crate::host_flow_return_audit::FlowReturnError;
use crate::semantic_query::FlowReturnFailure;
use verter_type_expr::facts::InferenceUnavailableReason;

/// The depth every form is checked at.
const DEPTH: usize = 10_000;

/// The depth a nest of function values is checked at: evaluating each
/// nested function in place, a native level per function, overflows the
/// production stacks within 200 levels in every nest checked at it, so 2,000
/// levels prove the stack of evaluators with ten times that margin.
const NESTED_FUNCTIONS: usize = 2_000;

/// A depth whose demand slice the flow-slice plan budget admits: a
/// conditional selects its test and both branches at every level, a
/// member value or a block one node.
const UNDER_THE_PLAN_BUDGET: usize = 3_000;
const CONDITIONALS_UNDER_THE_PLAN_BUDGET: usize = 1_000;

/// The mismatches of `probe` over `source` against `checker`, read on a
/// 1 MiB thread.
fn mismatches_on_a_small_stack(
    source: String,
    probe: &'static str,
    checker: &'static str,
) -> Vec<String> {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || mismatches(&source, &[(probe, checker)]))
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

/// The typed outcome of `pf`'s body-derived return over `source`, read on
/// a 1 MiB thread: the degradation of the value it produced, or the typed
/// error it answered instead, a missing `pf` (`Failure(Missing)`) told
/// apart from a budget. It returns either way, never aborting the host.
fn return_on_a_small_stack(
    source: String,
) -> Result<Option<crate::semantic_query::FlowReturnDegradation>, FlowReturnError> {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || flow_return_outcome_in(Default::default(), &source, "pf"))
        .expect("spawn the probing thread")
        .join()
        .expect("the return evaluates")
}

/// The typed outcome of a return whose evaluation outgrew the demand's
/// connected work: no value, and the work budget named.
const WORK_BUDGET_EXCEEDED: Result<
    Option<crate::semantic_query::FlowReturnDegradation>,
    FlowReturnError,
> = Err(FlowReturnError::Failure(FlowReturnFailure::Budget(
    InferenceUnavailableReason::WorkBudgetExceeded,
)));

/// The typed outcome of a return whose inference outgrew the semantic
/// inference depth budget: no value, and the depth budget named.
const DEPTH_BUDGET_EXCEEDED: Result<
    Option<crate::semantic_query::FlowReturnDegradation>,
    FlowReturnError,
> = Err(FlowReturnError::Failure(FlowReturnFailure::Budget(
    InferenceUnavailableReason::DepthBudgetExceeded,
)));

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
fn parentheses_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(wrap(DEPTH, "(", ")"));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "number"),
        Vec::<String>::new()
    );
}

/// A chain of 10,000 member reads off a constructed value
/// (`new C().c.c…`): its lowering and its evaluation read the chain from
/// explicit stacks, deciding once that it is rooted at a value, so the
/// work grows with the chain's length. TypeScript 7.0.2: `C`, under every
/// setting.
#[test]
fn value_rooted_member_chains_10000_long_answer_on_production_stacks() {
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
fn logical_nots_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(format!("{}1", "!".repeat(DEPTH)));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "boolean"),
        Vec::<String>::new()
    );
}

#[test]
fn template_holes_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(wrap(DEPTH, "`${", "}`"));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "string"),
        Vec::<String>::new()
    );
}

#[test]
fn conditionals_nested_1000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(
            conditionals(CONDITIONALS_UNDER_THE_PLAN_BUDGET),
            RETURN,
            "1 | 2"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn objects_nested_3000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(
            objects(UNDER_THE_PLAN_BUDGET),
            "keyof ReturnType<typeof pf>",
            "\"v\""
        ),
        Vec::<String>::new()
    );
}

#[test]
fn blocks_nested_3000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(blocks(UNDER_THE_PLAN_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Conditionals, object literals and blocks nested 10,000 deep return on
/// the production stacks (their demand slices exceed the plan budget, so
/// the return is its typed budget failure).
#[test]
fn conditionals_objects_and_blocks_nested_10000_deep_return_on_production_stacks() {
    for (form, source) in [
        ("conditionals", conditionals(DEPTH)),
        ("objects", objects(DEPTH)),
        ("blocks", blocks(DEPTH)),
    ] {
        assert_eq!(
            return_on_a_small_stack(source),
            WORK_BUDGET_EXCEEDED,
            "{form}"
        );
    }
}

#[test]
#[ignore = "the flow-slice plan budget admits no demand slice of 10,000 nested conditionals"]
fn conditionals_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(conditionals(DEPTH), RETURN, "1 | 2"),
        Vec::<String>::new()
    );
}

#[test]
#[ignore = "the flow-slice plan budget admits no demand slice of 10,000 nested object literals"]
fn objects_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(objects(DEPTH), "keyof ReturnType<typeof pf>", "\"v\""),
        Vec::<String>::new()
    );
}

#[test]
#[ignore = "the flow-slice plan budget admits no demand slice of 10,000 nested blocks"]
fn blocks_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(blocks(DEPTH), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// A conditional nested 1,000 deep as an object member's value evaluates
/// its branches from the evaluator's stack (a return's conditional is read
/// arm by arm instead, for its per-arm freshness).
#[test]
fn a_member_conditional_nested_1000_deep_answers_on_production_stacks() {
    let source = format!(
        "export function pf(b: boolean) {{ return {{ v: {}2 }}; }}\n",
        "b ? 1 : ".repeat(CONDITIONALS_UNDER_THE_PLAN_BUDGET)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, "ReturnType<typeof pf>[\"v\"]", "number"),
        Vec::<String>::new()
    );
}

/// A function type returning a function type, 10,000 deep, and the same
/// for constructor types: each signature's return lowers from the locator
/// lowering's explicit stack.
#[test]
fn function_and_constructor_types_nested_10000_deep_answer_on_production_stacks() {
    for arrow in ["() => ", "new () => "] {
        let source = format!("type D = {}1;\n", arrow.repeat(DEPTH));
        assert_eq!(
            mismatches_on_a_small_stack(source, "D extends Function ? 1 : 2", "1"),
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

/// A depth whose nested calls the connected-demand work budget admits.
const CALLS_UNDER_THE_WORK_BUDGET: usize = 500;

#[test]
fn calls_nested_500_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(calls(CALLS_UNDER_THE_WORK_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Calls nested 10,000 deep return on the production stacks (their call
/// resolutions exceed the connected-demand work budget, so the return is
/// its typed budget failure).
#[test]
fn calls_nested_10000_deep_return_on_production_stacks() {
    assert_eq!(return_on_a_small_stack(calls(DEPTH)), WORK_BUDGET_EXCEEDED);
}

#[test]
#[ignore = "the connected-demand work budget admits no 10,000 nested call resolutions"]
fn calls_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(calls(DEPTH), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// A depth whose nested array literals the semantic inference depth budget
/// (64 levels) admits.
const ARRAYS_UNDER_THE_INFERENCE_BUDGET: usize = 64;

fn arrays(depth: usize) -> String {
    returning(wrap(depth, "[", "]"))
}

const ARRAY_PROBE: &str = "ReturnType<typeof pf> extends unknown[] ? 1 : 2";

/// Array literals nested 64 deep evaluate from the evaluator's stack (an
/// array literal is a frame of it, each element evaluated from the stack).
#[test]
fn arrays_nested_64_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(arrays(ARRAYS_UNDER_THE_INFERENCE_BUDGET), ARRAY_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Array literals nested 10,000 deep return on the production stacks (the
/// semantic inference depth budget admits 64 levels, so the return is its
/// typed budget failure).
#[test]
fn arrays_nested_10000_deep_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(arrays(DEPTH)),
        DEPTH_BUDGET_EXCEEDED
    );
}

#[test]
#[ignore = "the semantic inference depth budget admits no 10,000 nested array literals"]
fn arrays_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(arrays(DEPTH), ARRAY_PROBE, "1"),
        Vec::<String>::new()
    );
}

#[test]
fn type_arguments_nested_10000_deep_answer_on_production_stacks() {
    let source = format!(
        "interface Box<T> {{ v: T }}\ntype D = {};\n",
        wrap(DEPTH, "Box<", ">")
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, "D extends Box<unknown> ? 1 : 2", "1"),
        Vec::<String>::new()
    );
}

fn keyof_chain(depth: usize) -> String {
    format!("type D = {}{{ v: 1 }};\n", "keyof ".repeat(depth))
}

const KEYOF_PROBE: &str = "D extends string | number | symbol ? 1 : 2";

/// A `keyof` chain 10,000 deep returns on the production stacks (its
/// resolution exceeds the connected-demand work budget, so the return is
/// its typed budget failure).
#[test]
fn keyof_chains_10000_deep_return_on_production_stacks() {
    let source = format!(
        "{}export function pf() {{ return null as unknown as ({KEYOF_PROBE}); }}\n",
        keyof_chain(DEPTH)
    );
    assert_eq!(return_on_a_small_stack(source), WORK_BUDGET_EXCEEDED);
}

#[test]
#[ignore = "the connected-demand work budget admits no 10,000-deep keyof chain"]
fn keyof_chains_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(keyof_chain(DEPTH), KEYOF_PROBE, "1"),
        Vec::<String>::new()
    );
}

fn arrows(depth: usize) -> String {
    returning(format!("{}1", "() => ".repeat(depth)))
}

const ARROW_PROBE: &str = "ReturnType<typeof pf> extends Function ? 1 : 2";

#[test]
fn arrows_nested_10_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(arrows(10), ARROW_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Arrow functions nested 10,000 deep: each body's return reaches the next
/// function value, which its own evaluator evaluates from the drive's
/// stack of evaluators (`flow_return_nested`), not inside the one around
/// it.
#[test]
fn arrows_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(arrows(DEPTH), ARROW_PROBE, "1"),
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
fn module_calls_nested_1000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(module_calls(1_000, false), "typeof v", "1"),
        Vec::<String>::new()
    );
}

/// A module constant initialized by 10,000 nested calls, read in a
/// function, returns on the production stacks (its calls' resolutions
/// exceed the connected-demand work budget, so the return is its typed
/// failure).
#[test]
fn module_calls_nested_10000_deep_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(module_calls(DEPTH, true)),
        WORK_BUDGET_EXCEEDED
    );
}

fn receiver_chain(links: usize) -> String {
    format!(
        "interface B {{ m(): B; }}
export function pf(b: B) {{ return b{}; }}
",
        ".m()".repeat(links)
    )
}

/// A receiver chain the connected-demand work budget admits.
const RECEIVER_CHAINS_UNDER_THE_WORK_BUDGET: usize = 400;

/// A receiver chain, `b.m().m()…`: each call is a member call on the
/// value of the call before it, lowered and evaluated from explicit
/// stacks, the value its receiver. TypeScript 7.0.2: `B` for 2, 3 and 400
/// links, under every setting; 10,000 links return the connected-demand
/// work budget's typed failure on the production stacks.
#[test]
fn receiver_chains_10000_deep_return_on_production_stacks() {
    for links in [2, 3, RECEIVER_CHAINS_UNDER_THE_WORK_BUDGET] {
        assert_eq!(
            mismatches_on_a_small_stack(receiver_chain(links), RETURN, "B"),
            Vec::<String>::new(),
            "{links} links"
        );
    }
    assert_eq!(
        return_on_a_small_stack(receiver_chain(DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
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
fn module_receiver_chains_answer() {
    for links in [2, 3, RECEIVER_CHAINS_UNDER_THE_WORK_BUDGET] {
        assert_eq!(
            mismatches_on_a_small_stack(module_receiver_chain(links), "typeof v", "B"),
            Vec::<String>::new(),
            "{links} links"
        );
    }
}

/// A module constant initialized by a receiver chain 10,000 links long,
/// read in a function, returns the connected-demand work budget's typed
/// failure on the production stacks.
#[test]
fn module_receiver_chains_10000_deep_read_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(format!(
            "{}export function pf() {{ return v; }}
",
            module_receiver_chain(DEPTH)
        )),
        WORK_BUDGET_EXCEEDED
    );
}

/// Callbacks nested 10,000 deep, each an arrow passed to a callee that
/// declares its type (`a(() => a(() => … 1))`): each callback is an
/// argument its call evaluates from the evaluator's stack, its body from the
/// stack of evaluators. TypeScript 7.0.2: `number`, under every
/// `strictNullChecks` × `noImplicitAny` setting.
#[test]
fn callbacks_nested_10000_deep_answer_on_production_stacks() {
    let source = format!(
        "declare function a(f: () => number): number;\n{}",
        returning(wrap(DEPTH, "a(() => ", ")"))
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

/// A depth whose generic callbacks the connected-demand work budget admits.
const GENERIC_CALLBACKS_UNDER_THE_WORK_BUDGET: usize = 400;

/// TypeScript 7.0.2: `number`, under every setting.
#[test]
fn generic_callbacks_nested_400_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(
            generic_callbacks(GENERIC_CALLBACKS_UNDER_THE_WORK_BUDGET),
            RETURN,
            "number"
        ),
        Vec::<String>::new()
    );
}

/// Generic callbacks nested 2,000 deep return on the production stacks
/// (their call resolutions exceed the connected-demand work budget, which
/// 1,000 levels fit and 1,100 exceed, so the return is its typed budget
/// failure); typing a callback in place instead of suspending the route
/// overflows within 200 levels.
#[test]
fn generic_callbacks_nested_2000_deep_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(generic_callbacks(NESTED_FUNCTIONS)),
        WORK_BUDGET_EXCEEDED
    );
}

#[test]
#[ignore = "the connected-demand work budget admits no 10,000 nested generic callback inferences"]
fn generic_callbacks_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(generic_callbacks(DEPTH), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Immediately invoked functions nested 2,000 deep, arrows with block
/// bodies and function expressions: each call's callee operand is a nested
/// function value evaluated from the stack of evaluators. TypeScript 7.0.2:
/// `number`, under every setting.
#[test]
fn immediately_invoked_functions_nested_2000_deep_answer_on_production_stacks() {
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

/// Object literals whose method returns the next, 2,000 deep: a literal's
/// methods evaluate as children of its own step. TypeScript 7.0.2: `1`,
/// under every setting.
#[test]
fn object_methods_nested_2000_deep_answer_on_production_stacks() {
    let source = returning(wrap(NESTED_FUNCTIONS, "{ m() { return ", "; } }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, OBJECT_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Class expressions whose method returns the next, 2,000 deep: a class's
/// member functions evaluate as children of its own step. TypeScript 7.0.2:
/// `1`, under every setting.
#[test]
fn class_methods_nested_2000_deep_answer_on_production_stacks() {
    let source = returning(wrap(NESTED_FUNCTIONS, "class { m() { return ", "; } }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, ARROW_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Arrows nested 2,000 deep, each returning the next from inside a branch
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

/// [`arrows_returned_from`] a block and a labeled statement.
#[test]
fn arrows_returned_from_blocks_and_labels_nested_2000_deep_answer_on_production_stacks() {
    arrows_returned_from("() => { { return ", "; } }");
    arrows_returned_from("() => { l: { return ", "; } }");
}

/// [`arrows_returned_from`] an `if` arm.
#[test]
fn arrows_returned_from_if_arms_nested_2000_deep_answer_on_production_stacks() {
    arrows_returned_from("() => { if (b) { return ", "; } throw 0; }");
}

/// [`arrows_returned_from`] a `switch` clause.
#[test]
fn arrows_returned_from_switch_clauses_nested_2000_deep_answer_on_production_stacks() {
    arrows_returned_from("() => { switch (b) { case true: return ", "; } throw 0; }");
}

/// Arrows nested 2,000 deep, each a declarator's initializer the body
/// returns: the declarator suspends at its initializer's nested function
/// value. TypeScript 7.0.2: `1`, under every setting.
#[test]
fn arrows_initializing_declarators_nested_2000_deep_answer_on_production_stacks() {
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

/// A destructuring declarator nested 10,000 patterns deep lowers, binds and
/// drops from explicit stacks. Every element of an `any` source is `any`:
/// TypeScript 7.0.2 answers `1`, under every setting.
#[test]
fn destructuring_patterns_nested_10000_deep_answer_on_production_stacks() {
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

/// `await` nested 10,000 deep lowers and evaluates from explicit stacks.
/// TypeScript 7.0.2: `Promise<number>`, under every setting.
#[test]
fn awaits_nested_10000_deep_answer_on_production_stacks() {
    let source = format!(
        "export async function pf() {{ return {}1; }}\n",
        "await ".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "Promise<number>"),
        Vec::<String>::new()
    );
}

/// Statements nested 10,000 deep return on the production stacks: each
/// lowers and evaluates as frames of its region's lowering and run (their
/// demand slices exceed the plan budget, so the return is its typed budget
/// failure).
fn statements_nested_10000_deep_return(forms: [(&str, &str, &str); 3]) {
    for (form, open, close) in forms {
        let source = format!(
            "export function pf(b: boolean, x: number) {{ {}return 1;{} return 2; }}\n",
            open.repeat(DEPTH),
            close.repeat(DEPTH)
        );
        assert_eq!(
            return_on_a_small_stack(source),
            WORK_BUDGET_EXCEEDED,
            "{form}"
        );
    }
}

/// [`statements_nested_10000_deep_return`] for `if`, `switch` and `try`.
#[test]
fn branch_statements_nested_10000_deep_return_on_production_stacks() {
    statements_nested_10000_deep_return([
        ("if", "if (b) { ", " }"),
        ("switch", "switch (x) { case 1: ", " }"),
        ("try", "try { ", " } catch { }"),
    ]);
}

/// [`statements_nested_10000_deep_return`] for `while`, `for` and `do`.
#[test]
fn loop_statements_nested_10000_deep_return_on_production_stacks() {
    statements_nested_10000_deep_return([
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
        let before = crate::file_artifact_store::source_parse_identity_derivations_for_tests();
        assert_eq!(
            mismatches(&arrows(depth), &[(ARROW_PROBE, "1")]),
            Vec::<String>::new()
        );
        crate::file_artifact_store::source_parse_identity_derivations_for_tests() - before
    };
    assert_eq!(derivations(20), derivations(200));
}

/// Template literal types nested 10,000 deep: a template in a template's
/// hole splices into it from an explicit stack. TypeScript 7.0.2: `"1"`,
/// under every setting.
#[test]
fn template_literal_types_nested_10000_deep_answer_on_production_stacks() {
    let source = format!("type D = {};\n", wrap(DEPTH, "`${", "}`"));
    assert_eq!(
        mismatches_on_a_small_stack(source, "D", "\"1\""),
        Vec::<String>::new()
    );
}

fn mapped_types(depth: usize) -> String {
    format!("type D = {};\n", wrap(depth, "{ [K in \"a\"]: ", " }"))
}

/// A mapped-type nest the connected-demand work budget admits.
const MAPPED_TYPES_UNDER_THE_WORK_BUDGET: usize = 1_000;

/// Mapped types nested deep lower as frames of the locator lowering's
/// stack (their binder frames a shared chain) and are judged open by a
/// walk whose levels carry only the arm they take. 10,000 deep the probe
/// returns the connected-demand work budget's typed failure.
#[test]
fn mapped_types_nested_10000_deep_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(format!(
            "{}export function pf() {{ const p: keyof D = null as any; return p; }}\n",
            mapped_types(DEPTH)
        )),
        WORK_BUDGET_EXCEEDED
    );
}

/// A mapped-type nest under the connected-demand work budget answers:
/// TypeScript 7.0.2 answers `"a"`, under every setting.
#[test]
fn mapped_types_nested_1000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(
            mapped_types(MAPPED_TYPES_UNDER_THE_WORK_BUDGET),
            "keyof D",
            "\"a\""
        ),
        Vec::<String>::new()
    );
}

/// Mapped types nested 10,000 deep. TypeScript 7.0.2: `"a"`, under every
/// setting.
#[test]
#[ignore = "the connected-demand work budget admits no 10,000-deep mapped-type nest"]
fn mapped_types_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(mapped_types(DEPTH), "keyof D", "\"a\""),
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
        "export namespace A { ".repeat(DEPTH - 1),
        " }".repeat(DEPTH),
        "A.".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, "D", "1"),
        Vec::<String>::new()
    );
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

/// A statement wrapped in 10,000 labels. TypeScript 7.0.2: `number`, under
/// every setting.
#[test]
#[ignore = "the flow-slice plan budget admits no demand slice of 10,000 nested labels"]
fn label_chains_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(label_chain(DEPTH), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// A statement wrapped in 10,000 labels returns on the production stacks:
/// the parse, and the copy of its program the binding index makes, run on
/// the stack the scan bounds (a braceless label nests its statement a level
/// under it), and the return is the plan's typed budget failure.
#[test]
fn label_chains_10000_deep_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(label_chain(DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
}

/// A statement wrapped in 4,000 labels, which the flow-slice plan budget
/// admits. TypeScript 7.0.2: `number`, under every setting.
#[test]
fn label_chains_4000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(label_chain(4_000), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// An `else if` chain 10,000 long. TypeScript 7.0.2: `1 | 2` (reporting
/// TS2563, body too large for control-flow analysis), under every setting.
#[test]
#[ignore = "the flow-slice plan budget admits no demand slice of 10,001 return sites"]
fn else_if_chains_10000_long_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(else_if_chain(DEPTH), RETURN, "1 | 2"),
        Vec::<String>::new()
    );
}

/// An `else if` chain 10,000 long returns on the production stacks: the
/// parse, and the copy of its program the binding index makes, run on the
/// stack the scan bounds (an `else` continues its `if` past the `;` ending
/// the `if`'s body), and the return is the plan's typed budget failure.
#[test]
fn else_if_chains_10000_long_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(else_if_chain(DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
}

/// `else if` chains 250, 300 and 800 long: each return site is work the
/// connected demand pays for, not a fixed ceiling. TypeScript 7.0.2: `1 |
/// 2`, under every setting.
#[test]
fn else_if_chains_250_300_and_800_long_answer_on_production_stacks() {
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
fn switches_with_300_and_800_returning_cases_answer_on_production_stacks() {
    for cases in [300, 800] {
        assert_eq!(
            mismatches_on_a_small_stack(switch_returns(cases), RETURN, switch_returns_union(cases)),
            Vec::<String>::new(),
            "{cases} cases"
        );
    }
}

/// A `switch` with 10,000 returning cases returns the connected-demand work
/// budget's typed failure on the production stacks.
#[test]
fn switches_with_10000_returning_cases_return_on_production_stacks() {
    assert_eq!(
        return_on_a_small_stack(switch_returns(DEPTH)),
        WORK_BUDGET_EXCEEDED
    );
}

/// A nest whose demand slice the flow-slice plan budget admits: the
/// unreachable and dead-path chains below carry one and two returns per
/// level.
const UNDER_THE_SLICE_BUDGET: usize = 120;

/// Unreachable code nested deep, each level a block behind a `return`: each
/// unreachable region evaluates as a frame of its enclosing region's run.
/// 10,000 deep the return is the plan's typed budget failure; under the
/// slice budget TypeScript 7.0.2 answers `number`, under every setting.
#[test]
fn unreachable_code_nested_10000_deep_returns_on_production_stacks() {
    let source = |depth: usize| {
        format!(
            "export function pf() {{ {}return 1;{} }}\n",
            "return 1; { ".repeat(depth),
            " }".repeat(depth)
        )
    };
    assert_eq!(return_on_a_small_stack(source(DEPTH)), WORK_BUDGET_EXCEEDED);
    assert_eq!(
        mismatches_on_a_small_stack(source(UNDER_THE_SLICE_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Code past an exhaustive `switch` nested deep: each region's statements
/// past the dead path evaluate on its own dead tail, a frame of the run.
/// 10,000 deep the return is the plan's typed budget failure; under the
/// slice budget TypeScript 7.0.2 answers `number`, under every setting.
#[test]
fn code_past_exhaustive_switches_nested_10000_deep_returns_on_production_stacks() {
    let source = |depth: usize| {
        format!(
            "export function pf(x: \"a\" | \"b\") {{ {}return 1;{} }}\n",
            "switch (x) { case \"a\": return 1; case \"b\": return 1; } { ".repeat(depth),
            " }".repeat(depth)
        )
    };
    assert_eq!(return_on_a_small_stack(source(DEPTH)), WORK_BUDGET_EXCEEDED);
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
        let walked = crate::mapper_binder_registry::type_expr_visits_for_tests();
        let copied = super::locator_shape::binder_frame_clones_for_tests();
        assert_eq!(
            mismatches(&mapped_types(depth), &[("keyof D", "\"a\"")]),
            Vec::<String>::new()
        );
        (
            crate::mapper_binder_registry::type_expr_visits_for_tests() - walked,
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

/// A dead loop nest the flow-slice plan budget admits.
const DEAD_LOOPS_UNDER_THE_PLAN_BUDGET: usize = 1_000;

/// Loops behind a literal `false` test nested deep: each dead body
/// evaluates as a frame of the run, on a dead path its pass restores. 10,000
/// deep the demand slice exceeds the plan budget (the return is its typed
/// budget failure); under it TypeScript 7.0.2 answers `1 | 2`, under every
/// setting.
#[test]
fn dead_loops_nested_10000_deep_return_on_production_stacks() {
    for open in ["while (false) { ", "for (; false; ) { "] {
        let source = |depth: usize| {
            format!(
                "export function pf() {{ {}return 1;{} return 2; }}\n",
                open.repeat(depth),
                " }".repeat(depth)
            )
        };
        assert_eq!(
            return_on_a_small_stack(source(DEPTH)),
            WORK_BUDGET_EXCEEDED,
            "{open}"
        );
        assert_eq!(
            mismatches_on_a_small_stack(source(DEAD_LOOPS_UNDER_THE_PLAN_BUDGET), RETURN, "1 | 2"),
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

/// Enums 10,000 long, each member reading the member of the enum before
/// it: `enum E0 { A = 1 } enum E1 { A = E0.A } … enum E10000 { A =
/// E9999.A }`.
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

/// An enum's member reading through a chain of 10,000 enums is evaluated
/// from an explicit stack. TypeScript 7.0.2: `E10000.A extends 1 ? "y" :
/// "n"` is `"y"`, under every `strictNullChecks` × `noImplicitAny` setting.
#[test]
fn enum_reference_chains_10000_long_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(
            enum_chain(DEPTH),
            "E10000.A extends 1 ? \"y\" : \"n\"",
            "\"y\""
        ),
        Vec::<String>::new()
    );
}

/// One enum of 10,000 members, each the member before it plus one, the
/// first another enum's member. TypeScript 7.0.2: `E.A9999 extends 10000 ?
/// "y" : "n"` is `"y"`, under every setting.
#[test]
fn enum_member_chains_10000_long_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(
            enum_member_chain(DEPTH),
            "E.A9999 extends 10000 ? \"y\" : \"n\"",
            "\"y\""
        ),
        Vec::<String>::new()
    );
}

/// Enum initializers 10,000 deep: an addition nested in parentheses to the
/// right, a left-leaning chain of additions, and a chain of additions of
/// another enum's member. TypeScript 7.0.2: each member extends `10001`
/// (`"y"`), under every setting.
#[test]
fn enum_initializers_nested_10000_deep_answer_on_production_stacks() {
    for (form, initializer) in [
        ("parenthesized", wrap(DEPTH, "(1 + ", ")")),
        ("left-leaning", format!("{}1", "1 + ".repeat(DEPTH))),
        ("member reads", format!("{}1", "X.A + ".repeat(DEPTH))),
    ] {
        let source = format!("enum X {{ A = 1 }}\nenum D {{ A = {initializer} }}\n");
        assert_eq!(
            mismatches_on_a_small_stack(source, "D.A extends 10001 ? \"y\" : \"n\"", "\"y\""),
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
