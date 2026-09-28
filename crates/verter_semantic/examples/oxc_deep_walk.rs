//! Standalone measure of oxc's own walks over deeply nested input.
//!
//! It calls nothing of Verter's: it builds one nested source, parses it
//! with `oxc_parser::Parser` on a thread whose stack the parse cannot
//! exhaust, then runs one of oxc's walks over the program on a thread of
//! the given stack, so a failure here is the walk's own.
//!
//! ```text
//! cargo run -p verter_semantic --example oxc_deep_walk [--release] -- <walk> <form> <depth> <stack-MiB>
//! ```
//!
//! `<walk>` is `clone` (`CloneIn::clone_in`), `visit` (a `Visit` walk that
//! counts nodes) or `semantic` (`SemanticBuilder::build`). `<form>` is one
//! of the forms `verter_parser`'s `oxc_deep_parse` example builds. When the
//! walk exhausts the thread's stack the process aborts with
//! `thread 'oxc-walk' has overflowed its stack`.

use oxc_allocator::{Allocator, CloneIn};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
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

/// The parsed program, handed to the walking thread.
struct Handed<'p, 'a>(&'p oxc_ast::ast::Program<'a>);

// SAFETY: the parsing thread blocks on the walking thread's join while the
// walking thread holds the program, so exactly one thread touches it (and
// its cells) at a time.
unsafe impl Send for Handed<'_, '_> {}

#[derive(Default)]
struct Count(usize);

impl<'a> Visit<'a> for Count {
    fn enter_node(&mut self, _kind: oxc_ast::AstKind<'a>) {
        self.0 += 1;
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [walk, form, depth, stack] = arguments.as_slice() else {
        eprintln!("usage: oxc_deep_walk <clone|visit|semantic> <form> <depth> <stack-MiB>");
        std::process::exit(2);
    };
    let depth: usize = depth.parse().expect("a depth");
    let stack: usize = stack.parse().expect("a stack size in MiB");
    let source = source(form, depth);
    let walk = walk.clone();
    let nodes = std::thread::Builder::new()
        .name("oxc-parse".to_string())
        .stack_size(4 << 30)
        .spawn(move || {
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            assert!(parsed.diagnostics.is_empty(), "the source parses");
            let handed = Handed(&parsed.program);
            std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .name("oxc-walk".to_string())
                    .stack_size(stack * 1024 * 1024)
                    .spawn_scoped(scope, move || {
                        let handed = handed;
                        let program = handed.0;
                        match walk.as_str() {
                            "clone" => {
                                let clones = Allocator::default();
                                program.clone_in(&clones).body.len()
                            }
                            "visit" => {
                                let mut count = Count::default();
                                count.visit_program(program);
                                count.0
                            }
                            "semantic" => {
                                let built = SemanticBuilder::new().build(program);
                                built.semantic.scoping().symbols_len()
                            }
                            other => panic!("unknown walk {other}"),
                        }
                    })
                    .expect("spawn the walking thread")
                    .join()
                    .expect("the walk returns")
            })
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the parse returns");
    println!("oxc 0.151.0 form={form} depth={depth} stack={stack}MiB: walked, {nodes}");
}
