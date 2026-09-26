//! Standalone reproducer for oxc's parser on deeply nested input.
//!
//! It calls nothing of Verter's: it builds one nested source and parses it
//! with `oxc_parser::Parser` on a thread of the given stack, so a failure
//! here is oxc's own.
//!
//! ```text
//! cargo run -p verter_parser --example oxc_deep_parse [--release] -- <form> <depth> <stack-MiB>
//! ```
//!
//! `<form>` is one of `parentheses`, `not`, `generic`, `conditional`,
//! `array`, `object`, `arrow`, `template`, `keyof`, `block`. On success it
//! prints the statement count and the error count; when oxc's recursive
//! descent exhausts the thread's stack the process aborts with
//! `thread 'oxc-parse' has overflowed its stack`.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn source(form: &str, depth: usize) -> String {
    let wrap = |open: &str, close: &str| format!("{}1{}", open.repeat(depth), close.repeat(depth));
    match form {
        "parentheses" => format!("export const v = {};\n", wrap("(", ")")),
        "not" => format!("export const v = {}1;\n", "!".repeat(depth)),
        "generic" => format!("type D = {};\n", wrap("Box<", ">")),
        "conditional" => format!(
            "declare const b: boolean;\nexport const v = {}2;\n",
            "b ? 1 : ".repeat(depth)
        ),
        "array" => format!("export const v = {};\n", wrap("[", "]")),
        "object" => format!("export const v = {};\n", wrap("{ v: ", " }")),
        "arrow" => format!("export const v = {}1;\n", "() => ".repeat(depth)),
        "template" => format!("export const v = {};\n", wrap("`${", "}`")),
        "keyof" => format!("type D = {}{{ v: 1 }};\n", "keyof ".repeat(depth)),
        "block" => format!("{}{}", "{ ".repeat(depth), " }".repeat(depth)),
        other => panic!("unknown form {other}"),
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [form, depth, stack] = arguments.as_slice() else {
        eprintln!("usage: oxc_deep_parse <form> <depth> <stack-MiB>");
        std::process::exit(2);
    };
    let depth: usize = depth.parse().expect("a depth");
    let stack: usize = stack.parse().expect("a stack size in MiB");
    let source = source(form, depth);
    let (statements, errors) = std::thread::Builder::new()
        .name("oxc-parse".to_string())
        .stack_size(stack * 1024 * 1024)
        .spawn(move || {
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            (parsed.program.body.len(), parsed.errors.len())
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the parse returns");
    println!(
        "oxc_parser 0.126.0 form={form} depth={depth} stack={stack}MiB: parsed, {statements} statements, {errors} errors"
    );
}
