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

use super::checker_probe_lane_tests::{degradation_in, mismatches};

/// The depth every form is checked at.
const DEPTH: usize = 10_000;

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

/// Whether `pf`'s body-derived return over `source` produced a value, read
/// on a 1 MiB thread: it returns either way, never aborting the host.
fn returns_on_a_small_stack(source: String) -> bool {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || degradation_in(Default::default(), &source, "pf").is_ok())
        .expect("spawn the probing thread")
        .join()
        .expect("the return evaluates")
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
fn parentheses_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(wrap(DEPTH, "(", ")"));
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "number"),
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
        assert!(!returns_on_a_small_stack(source), "{form}");
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
    assert!(!returns_on_a_small_stack(calls(DEPTH)));
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
    assert!(!returns_on_a_small_stack(arrays(DEPTH)));
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
    assert!(!returns_on_a_small_stack(source));
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
    assert!(!returns_on_a_small_stack(module_calls(DEPTH, true)));
}

/// A receiver chain, `b.m().m()…`: its calls are walked from an explicit
/// stack. 10,000 links return what three links return, on the production
/// stacks (a member call on a member call's result is an unmodeled
/// position either way).
#[test]
fn receiver_chains_10000_deep_return_on_production_stacks() {
    let chain = |depth: usize| {
        let source = format!(
            "interface B {{ m(): B; }}
export function pf(b: B) {{ return b{}; }}
",
            ".m()".repeat(depth)
        );
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || degradation_in(Default::default(), &source, "pf"))
            .expect("spawn the probing thread")
            .join()
            .expect("the return evaluates")
    };
    assert_eq!(chain(DEPTH), chain(3));
}

/// A module constant initialized by a receiver chain 10,000 links long
/// reads as one three links long does, on the production stacks.
#[test]
fn module_receiver_chains_10000_deep_read_on_production_stacks() {
    let chain = |depth: usize| {
        let source = format!(
            "interface B {{ m(): B; }}
declare const b: B;
export const v = b{};
",
            ".m()".repeat(depth)
        );
        mismatches_on_a_small_stack(source, "typeof v", "B")
    };
    assert_eq!(chain(DEPTH), chain(3));
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

/// Generic callbacks nested 10,000 deep return on the production stacks
/// (their call resolutions exceed the connected-demand work budget, so the
/// return is its typed budget failure); typing a callback in place instead
/// of suspending the route overflows.
#[test]
fn generic_callbacks_nested_10000_deep_return_on_production_stacks() {
    assert!(!returns_on_a_small_stack(generic_callbacks(DEPTH)));
}

#[test]
#[ignore = "the connected-demand work budget admits no 10,000 nested generic callback inferences"]
fn generic_callbacks_nested_10000_deep_answer_on_production_stacks() {
    assert_eq!(
        mismatches_on_a_small_stack(generic_callbacks(DEPTH), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Immediately invoked functions nested 10,000 deep, arrows with block
/// bodies and function expressions: each call's callee operand is a nested
/// function value evaluated from the stack of evaluators. TypeScript 7.0.2:
/// `number`, under every setting.
#[test]
fn immediately_invoked_functions_nested_10000_deep_answer_on_production_stacks() {
    for (open, close) in [
        ("(() => { return ", "; })()"),
        ("(function () { return ", "; })()"),
    ] {
        assert_eq!(
            mismatches_on_a_small_stack(returning(wrap(DEPTH, open, close)), RETURN, "number"),
            Vec::<String>::new(),
            "{open}"
        );
    }
}

const OBJECT_PROBE: &str = "ReturnType<typeof pf> extends object ? 1 : 2";

/// Object literals whose method returns the next, 10,000 deep: a literal's
/// methods evaluate as children of its own step. TypeScript 7.0.2: `1`,
/// under every setting.
#[test]
fn object_methods_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(wrap(DEPTH, "{ m() { return ", "; } }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, OBJECT_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Class expressions whose method returns the next, 10,000 deep: a class's
/// member functions evaluate as children of its own step. TypeScript 7.0.2:
/// `1`, under every setting.
#[test]
fn class_methods_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(wrap(DEPTH, "class { m() { return ", "; } }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, ARROW_PROBE, "1"),
        Vec::<String>::new()
    );
}

/// Arrows nested 10,000 deep, each returning the next from inside a block,
/// an `if` arm, a `switch` clause and a labeled statement: a branch
/// statement's regions are frames of its region's run, so the returned
/// function is evaluated from the stack of evaluators. TypeScript 7.0.2:
/// `1`, under every setting.
#[test]
fn arrows_returned_from_branches_nested_10000_deep_answer_on_production_stacks() {
    for (open, close) in [
        ("() => { { return ", "; } }"),
        ("() => { if (b) { return ", "; } throw 0; }"),
        ("() => { switch (b) { case true: return ", "; } throw 0; }"),
        ("() => { l: { return ", "; } }"),
    ] {
        assert_eq!(
            mismatches_on_a_small_stack(returning(wrap(DEPTH, open, close)), ARROW_PROBE, "1"),
            Vec::<String>::new(),
            "{open}"
        );
    }
}

/// Arrows nested 10,000 deep, each a declarator's initializer the body
/// returns: the declarator suspends at its initializer's nested function
/// value. TypeScript 7.0.2: `1`, under every setting.
#[test]
fn arrows_initializing_declarators_nested_10000_deep_answer_on_production_stacks() {
    let source = returning(wrap(DEPTH, "() => { const f = ", "; return f; }"));
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

/// `if`, `switch`, `try`, labeled and loop statements nested 10,000 deep
/// return on the production stacks: each lowers and evaluates as frames of
/// its region's lowering and run (their demand slices exceed the plan
/// budget, so the return is its typed budget failure).
#[test]
fn branch_and_loop_statements_nested_10000_deep_return_on_production_stacks() {
    let body = |open: &str, close: &str| {
        format!(
            "export function pf(b: boolean, x: number) {{ {}return 1;{} return 2; }}\n",
            open.repeat(DEPTH),
            close.repeat(DEPTH)
        )
    };
    for (form, source) in [
        ("if", body("if (b) { ", " }")),
        ("switch", body("switch (x) { case 1: ", " }")),
        ("try", body("try { ", " } catch { }")),
        ("while", body("while (b) { ", " }")),
        ("for", body("for (;;) { ", " }")),
        ("do", body("do { ", " } while (b);")),
    ] {
        assert!(!returns_on_a_small_stack(source), "{form}");
    }
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

/// Template literal types nested 10,000 deep. TypeScript 7.0.2: `"1"`,
/// under every setting.
#[test]
#[ignore = "a template literal type nested in a template literal type evaluates a query a native level per nesting"]
fn template_literal_types_nested_10000_deep_answer_on_production_stacks() {
    let source = format!("type D = {};\n", wrap(DEPTH, "`${", "}`"));
    assert_eq!(
        mismatches_on_a_small_stack(source, "D", "\"1\""),
        Vec::<String>::new()
    );
}

/// Mapped types nested 10,000 deep. TypeScript 7.0.2: `"a"`, under every
/// setting.
#[test]
#[ignore = "a mapped type nested in a mapped type lowers its locator shape a native level per nesting"]
fn mapped_types_nested_10000_deep_answer_on_production_stacks() {
    let source = format!("type D = {};\n", wrap(DEPTH, "{ [K in \"a\"]: ", " }"));
    assert_eq!(
        mismatches_on_a_small_stack(source, "keyof D", "\"a\""),
        Vec::<String>::new()
    );
}

/// Namespaces nested 10,000 deep, read through a 10,000-segment qualified
/// name. TypeScript 7.0.2: `1`, under every setting.
#[test]
#[ignore = "a namespace nested in a namespace indexes its declaration headers a native level per nesting"]
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

/// A statement wrapped in 10,000 labels. TypeScript 7.0.2: `number`, under
/// every setting.
#[test]
#[ignore = "a label chain's parse snapshot is copied a native level per label beyond the parse containment"]
fn label_chains_10000_deep_answer_on_production_stacks() {
    let labels: String = (0..DEPTH).map(|label| format!("l{label}: ")).collect();
    let source = format!("export function pf() {{ {labels}return 1; }}\n");
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "number"),
        Vec::<String>::new()
    );
}

/// An `else if` chain 10,000 long. TypeScript 7.0.2: `1 | 2` (reporting
/// TS2563, body too large for control-flow analysis), under every setting.
#[test]
#[ignore = "an else-if chain's parse snapshot is copied a native level per arm beyond the parse containment"]
fn else_if_chains_10000_long_answer_on_production_stacks() {
    let source = format!(
        "export function pf(b: boolean) {{ {}return 2; }}\n",
        "if (b) return 1; else ".repeat(DEPTH)
    );
    assert_eq!(
        mismatches_on_a_small_stack(source, RETURN, "1 | 2"),
        Vec::<String>::new()
    );
}

/// A nest whose return statements the flow-slice plan's return-site budget
/// (256) admits: the unreachable and dead-path chains below carry one and
/// two returns per level.
const UNDER_THE_RETURN_SITE_BUDGET: usize = 120;

/// Unreachable code nested deep, each level a block behind a `return`: each
/// unreachable region evaluates as a frame of its enclosing region's run.
/// 10,000 deep the return is the plan's typed budget failure; under the
/// return-site budget TypeScript 7.0.2 answers `number`, under every
/// setting.
#[test]
fn unreachable_code_nested_10000_deep_returns_on_production_stacks() {
    let source = |depth: usize| {
        format!(
            "export function pf() {{ {}return 1;{} }}\n",
            "return 1; { ".repeat(depth),
            " }".repeat(depth)
        )
    };
    assert!(!returns_on_a_small_stack(source(DEPTH)));
    assert_eq!(
        mismatches_on_a_small_stack(source(UNDER_THE_RETURN_SITE_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
}

/// Code past an exhaustive `switch` nested deep: each region's statements
/// past the dead path evaluate on its own dead tail, a frame of the run.
/// 10,000 deep the return is the plan's typed budget failure; under the
/// return-site budget TypeScript 7.0.2 answers `number`, under every
/// setting.
#[test]
fn code_past_exhaustive_switches_nested_10000_deep_returns_on_production_stacks() {
    let source = |depth: usize| {
        format!(
            "export function pf(x: \"a\" | \"b\") {{ {}return 1;{} }}\n",
            "switch (x) { case \"a\": return 1; case \"b\": return 1; } { ".repeat(depth),
            " }".repeat(depth)
        )
    };
    assert!(!returns_on_a_small_stack(source(DEPTH)));
    assert_eq!(
        mismatches_on_a_small_stack(source(UNDER_THE_RETURN_SITE_BUDGET), RETURN, "number"),
        Vec::<String>::new()
    );
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
