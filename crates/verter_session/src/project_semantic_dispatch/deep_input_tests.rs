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
