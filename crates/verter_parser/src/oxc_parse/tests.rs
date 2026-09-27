use oxc_allocator::Allocator;
use oxc_span::SourceType;

use super::{nesting, Parser};

/// `source` parsed through [`Parser`] on a thread with a 1 MiB stack: its
/// statement and error counts.
fn parse_on_a_small_stack(source: String) -> (usize, usize) {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            (parsed.program.body.len(), parsed.diagnostics.len())
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the parse returns")
}

/// Every way syntax nests, 10,000 levels deep, parses on a 1 MiB thread:
/// the parse runs on a stack its source's length cannot exhaust. oxc's own
/// parser overflows a 1 MiB stack from 298 (an object literal, unoptimized) to
/// 9,000 (`!`, optimized) levels
/// (`docs/evidence/signature-kernel/oxc-deep-parse.md`).
#[test]
fn every_form_nested_10000_deep_parses_on_a_small_stack() {
    for (form, source) in forms_nested(10_000) {
        assert_eq!(parse_on_a_small_stack(source), (1, 0), "{form}");
    }
}

/// Every way syntax nests, `depth` levels deep: each form's name and a
/// one-statement source.
fn forms_nested(depth: usize) -> [(&'static str, String); 11] {
    let wrap = |open: &str, close: &str| format!("{}1{}", open.repeat(depth), close.repeat(depth));
    let sources = [
        (
            "parentheses",
            format!("export const v = {};", wrap("(", ")")),
        ),
        ("not", format!("export const v = {}1;", "!".repeat(depth))),
        ("generic", format!("type D = {};", wrap("Box<", ">"))),
        (
            "conditional",
            format!("export const v = {}2;", "b ? 1 : ".repeat(depth)),
        ),
        ("array", format!("export const v = {};", wrap("[", "]"))),
        (
            "object",
            format!("export const v = {};", wrap("{ v: ", " }")),
        ),
        (
            "arrow",
            format!("export const v = {}1;", "() => ".repeat(depth)),
        ),
        (
            "template",
            format!("export const v = {};", wrap("`${", "}`")),
        ),
        (
            "keyof",
            format!("type D = {}{{ v: 1 }};", "keyof ".repeat(depth)),
        ),
        (
            "block",
            format!("{}{}", "{ ".repeat(depth), " }".repeat(depth)),
        ),
        (
            "numeric member",
            format!("export const v = 1.{};", ".a".repeat(depth)),
        ),
    ];
    sources
}

/// Every way syntax nests, 10,000 levels deep, walks on a 1 MiB thread
/// under each containment: oxc's `clone_in` and a `Visit` walk of the
/// whole program, and of its one statement, each recurse once per level
/// and run on a stack the source sizes. Without the containment a walk
/// overflows the thread as the parse would.
#[test]
fn every_form_nested_10000_deep_walks_on_a_small_stack() {
    use oxc_allocator::CloneIn;
    use oxc_ast_visit::Visit;
    use oxc_span::GetSpan;
    #[derive(Default)]
    struct Count(usize);
    impl<'a> Visit<'a> for Count {
        fn enter_node(&mut self, _kind: oxc_ast::AstKind<'a>) {
            self.0 += 1;
        }
    }
    for (form, source) in forms_nested(10_000) {
        let walked = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
                let program = &parsed.program;
                let statement = &program.body[0];
                let clones = Allocator::default();
                let cloned =
                    super::with_program_stack(program, || program.clone_in(&clones).body.len());
                let mut whole = Count::default();
                super::with_program_stack(program, || whole.visit_program(program));
                let walks = super::ProgramWalkStack::new(program);
                let mut node = Count::default();
                walks.with_node_stack(statement.span(), || node.visit_statement(statement));
                let mut text = Count::default();
                super::with_span_stack(&source, statement.span(), || {
                    text.visit_statement(statement)
                });
                (
                    cloned,
                    whole.0 > 10_000,
                    node.0 == text.0 && node.0 >= 10_000,
                )
            })
            .expect("spawn the walking thread")
            .join()
            .expect("the walks return");
        assert_eq!(walked, (1, true, true), "{form}");
    }
}

/// Sources nesting `depth` deep through syntax whose nesting no bracket
/// shows, or behind text the scan skips: each form's name, source type
/// and source, which oxc parses without an error.
fn forms_nested_past_the_brackets(depth: usize) -> Vec<(&'static str, SourceType, String)> {
    let ts = SourceType::ts();
    let tsx = SourceType::tsx();
    let chain = ".a".repeat(depth);
    let parens = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let ors = vec!["a"; depth + 1].join(" | ");
    let boxes = format!("{}A{}", "Box<A, ".repeat(depth), ">".repeat(depth));
    let labels = (0..depth).map(|i| format!("l{i}: ")).collect::<String>();
    vec![
        ("if", ts, format!("{}x;", "if (a) ".repeat(depth))),
        ("label", ts, format!("{labels}x;")),
        (
            "do",
            ts,
            format!("{}x;{}", "do ".repeat(depth), " while (a);".repeat(depth)),
        ),
        (
            "else if",
            ts,
            format!("{}x;", "if (a) x;\nelse ".repeat(depth)),
        ),
        (
            "function body after a return type",
            ts,
            format!("function f(): T {{ return {ors} }}"),
        ),
        (
            "arrow body after a return type",
            ts,
            format!("const f = (x): T => {ors};"),
        ),
        ("type arguments of a call", ts, format!("f<{boxes}>();")),
        (
            "type parameter constraint",
            ts,
            format!("function f<T extends {boxes}>() {{}}"),
        ),
        ("JSX member name", tsx, format!("const v = <a{chain} />;")),
        (
            "JSX type arguments",
            tsx,
            format!("const v = <C<{boxes}> />;"),
        ),
        (
            "JSX attribute element holding a quote",
            tsx,
            format!("const v = <a b=<b>it's</b> c={{{parens}}} />;"),
        ),
        (
            "regular expression class",
            ts,
            format!("r = /[///]/; v = x{chain};"),
        ),
        (
            "string line continuation",
            ts,
            format!("s = 'a\\\r\n'; v = {parens};"),
        ),
        ("comment ended by CR", ts, format!("// c\rv = {parens};")),
        (
            "HTML-like comment",
            ts,
            format!("x = 1 <!-- `\nv = {parens};\n// `"),
        ),
        (
            "keyword property name",
            ts,
            format!("v = x.if(b) / x{chain};"),
        ),
        ("identifier `as`", ts, format!("v = as / x{chain};")),
        (
            "object literal divided",
            ts,
            format!("export default {{}} / x{chain};"),
        ),
        (
            "function expression divided",
            ts,
            format!("v = function () {{}} / x{chain};"),
        ),
        (
            "identifier `await`",
            SourceType::cjs(),
            format!("v = await / x{chain};"),
        ),
        (
            "no-break spaces",
            ts,
            format!("v = {}x;", "typeof\u{a0}".repeat(depth)),
        ),
        (
            "no-break space before a regular expression",
            ts,
            format!("function f() {{ return\u{a0}/[`]/ }}\nv = {parens};\n// `"),
        ),
    ]
}

/// The environment variable naming the form of
/// [`forms_nested_past_the_brackets`] a child run of
/// [`every_form_nested_past_the_brackets_parses_and_walks_on_a_small_stack`]
/// parses and walks.
const DEEP_FORM_CHILD: &str = "VERTER_DEEP_FORM_CHILD";

/// Every form of [`forms_nested_past_the_brackets`], 10,000 deep, parses and
/// walks on a 1 MiB thread: the scan bounds syntax that nests with no
/// bracket, and text it skips ends where oxc ends it, so the parse and
/// oxc's walks run on a stack the source sizes. Each form runs in a child
/// process, so a form the scan under-counts overflows its child, which the
/// test names, rather than this process.
#[test]
fn every_form_nested_past_the_brackets_parses_and_walks_on_a_small_stack() {
    use oxc_allocator::CloneIn;
    use oxc_ast_visit::Visit;
    use oxc_span::GetSpan;
    #[derive(Default)]
    struct Count(usize);
    impl<'a> Visit<'a> for Count {
        fn enter_node(&mut self, _kind: oxc_ast::AstKind<'a>) {
            self.0 += 1;
        }
    }
    if let Some(name) = std::env::var_os(DEEP_FORM_CHILD) {
        let (_, source_type, source) = forms_nested_past_the_brackets(10_000)
            .into_iter()
            .find(|(form, _, _)| *form == name)
            .expect("a form of the list");
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &source, source_type).parse();
                assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
                let program = &parsed.program;
                let clones = Allocator::default();
                super::with_program_stack(program, || program.clone_in(&clones));
                let mut whole = Count::default();
                super::with_program_stack(program, || whole.visit_program(program));
                let walks = super::ProgramWalkStack::new(program);
                for statement in &program.body {
                    let mut node = Count::default();
                    walks.with_node_stack(statement.span(), || node.visit_statement(statement));
                }
            })
            .expect("spawn the walking thread")
            .join()
            .expect("the parse and walks return");
        println!("{DEEP_FORM_CHILD}: parsed and walked");
        return;
    }
    let failures: Vec<String> = forms_nested_past_the_brackets(10_000)
        .into_iter()
        .filter_map(|(form, _, _)| {
            let output =
                std::process::Command::new(std::env::current_exe().expect("this test binary"))
                    .args([
                        "--exact",
                        "oxc_parse::tests::every_form_nested_past_the_brackets_parses_and_walks_on_a_small_stack",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env(DEEP_FORM_CHILD, form)
                    .output()
                    .expect("run the form in a child process");
            let stderr = String::from_utf8_lossy(&output.stderr);
            let walked = String::from_utf8_lossy(&output.stdout)
                .contains(&format!("{DEEP_FORM_CHILD}: parsed and walked"));
            (!output.status.success() || !walked).then(|| {
                let reason = stderr
                    .lines()
                    .find(|line| line.contains("overflowed") || line.contains("panicked"))
                    .unwrap_or("");
                format!("{form}: {:?} {reason}", output.status)
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An unclosed nest parses to its syntax error, not an overflow: each open
/// bracket is one level and one byte, oxc's costliest per byte.
#[test]
fn an_unclosed_200000_deep_nest_parses_to_a_syntax_error_on_a_small_stack() {
    let (_, errors) = parse_on_a_small_stack(format!("export const v = {}1", "[".repeat(200_000)));
    assert!(errors > 0, "an unclosed nest is a syntax error");
}

/// A parsed expression takes the same stack.
#[test]
fn a_10000_deep_expression_parses_on_a_small_stack() {
    let parsed = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let source = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
            let allocator = Allocator::default();
            Parser::new(&allocator, &source, SourceType::ts())
                .parse_expression()
                .is_ok()
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the parse returns");
    assert!(parsed);
}

/// Every parse in the workspace's crates goes through [`Parser`]: a direct
/// `oxc_parser::Parser` parses on whatever stack its thread has left and
/// overflows it on a deep enough source.
/// `verter_type_expr_oxc` sits below this crate and uses oxc's parser only
/// in its tests (a dev-dependency); benchmarks are not shipped.
#[test]
fn no_crate_parses_around_the_guard() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut direct = Vec::new();
    let mut stack = Vec::new();
    for entry in std::fs::read_dir(crates)
        .expect("read the crates directory")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "verter_bench" || name == "verter_type_expr_oxc" {
            continue;
        }
        stack.push(entry.path().join("src"));
    }
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path.ends_with("oxc_parse") {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read a source file");
            for (index, line) in source.lines().enumerate() {
                let imports_parser = line.contains("use oxc_parser::{")
                    && line
                        .split(['{', '}', ','])
                        .any(|item| item.trim() == "Parser");
                let names_parser = line.match_indices("oxc_parser::Parser").any(|(at, path)| {
                    !line[at + path.len()..]
                        .starts_with(|next: char| next.is_ascii_alphanumeric() || next == '_')
                });
                if names_parser || imports_parser {
                    direct.push(format!("{}:{}: {}", path.display(), index + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        direct.is_empty(),
        "parse through `verter_parser::oxc_parse::Parser`:\n{}",
        direct.join("\n")
    );
    let manifest = std::fs::read_to_string(crates.join("verter_type_expr_oxc/Cargo.toml"))
        .expect("read verter_type_expr_oxc's manifest");
    let dependencies = manifest
        .split("[dev-dependencies]")
        .next()
        .expect("the manifest's head");
    assert!(
        !dependencies.contains("oxc_parser"),
        "verter_type_expr_oxc parses only in its tests"
    );
}

/// The scan's bound over a source ([`nesting`]).
fn depth(source: &str, source_type: SourceType) -> u32 {
    nesting::scan(source, source_type, u32::MAX)
        .expect("no limit")
        .depth
}

fn ts(source: &str) -> u32 {
    depth(source, SourceType::ts())
}

#[test]
fn brackets_and_links_nest() {
    assert_eq!(ts("a"), 0);
    assert_eq!(ts("a + b + c"), 2);
    assert_eq!(ts("f(g(h(1)))"), 3);
    assert_eq!(ts("f()()()()"), 4);
    assert_eq!(ts("a[0][1][2]"), 3);
    assert_eq!(ts("x.y.z"), 2);
    assert_eq!(ts("!!!x"), 3);
    assert_eq!(ts("b ? 1 : b ? 2 : 3"), 2);
    assert_eq!(ts("type A = Box<Box<Box<1>>>"), 4);
    assert_eq!(ts(&format!("{}1{}", "[".repeat(40), "]".repeat(40))), 40);
}

/// A numeric literal ends where the lexical grammar ends it, so the member
/// accesses after it nest: `1..a` is `1.` then `.a`, `1.e3.a` an exponent
/// then `.a`, and `0x1f.a` a hex literal then `.a`.
#[test]
fn a_numeric_literal_ends_at_its_token_boundary() {
    let chain = ".a".repeat(500);
    for number in [
        "1.",
        "1.5",
        ".5",
        "1.e3",
        "1e+3",
        "1E-3",
        "0x1f",
        "0o17",
        "0b1",
        "1_000",
        "1n",
        "0x1fn",
        "017",
        "1_0.2_5e1_0",
    ] {
        assert!(
            ts(&format!("const v = {number}{chain}")) >= 500,
            "{number}: {}",
            ts(&format!("const v = {number}{chain}"))
        );
    }
    // The literal's own characters are one operand.
    assert_eq!(ts("x = 1.5e+10"), 1);
    assert_eq!(ts("x = 0x1f_ffn"), 1);
    assert_eq!(ts("x = 1_000.25"), 1);
}

/// The cases of `cases` (a name, a source type, a source and how deeply
/// its syntax nests) whose scan bound is shallower than their nesting.
fn under_counted(cases: Vec<(&str, SourceType, String, u32)>) -> Vec<String> {
    cases
        .into_iter()
        .filter_map(|(name, source_type, source, nests)| {
            let bound = depth(&source, source_type);
            (bound < nests).then(|| format!("{name}: bound {bound} under {nests}"))
        })
        .collect()
}

const N: usize = 300;

/// A statement nested as the body of a braceless `if`, `while`, `for`,
/// `with`, `do`, `else` or label nests one level per head, though no
/// bracket encloses it, and an `else` continues its `if` past the `;` that
/// ends the `if`'s body.
#[test]
fn braceless_statement_bodies_nest() {
    let n = N as u32;
    let script = SourceType::cjs();
    let labels = (0..N).map(|i| format!("l{i}: ")).collect::<String>();
    let failures = under_counted(vec![
        (
            "if",
            SourceType::ts(),
            format!("{}x;", "if (a) ".repeat(N)),
            n,
        ),
        (
            "if, one per line",
            SourceType::ts(),
            format!("{}x;", "if (a)\n".repeat(N)),
            n,
        ),
        (
            "while",
            SourceType::ts(),
            format!("{}x;", "while (a) ".repeat(N)),
            n,
        ),
        (
            "for",
            SourceType::ts(),
            format!("{}x;", "for (;;) ".repeat(N)),
            n,
        ),
        ("with", script, format!("{}x;", "with (a) ".repeat(N)), n),
        ("label", SourceType::ts(), format!("{labels}x;"), n),
        (
            "do",
            SourceType::ts(),
            format!("{}x;{}", "do ".repeat(N), " while (a);".repeat(N)),
            n,
        ),
        (
            "else if, bodies ended by `;`",
            SourceType::ts(),
            format!("{}x;", "if (a) x;\nelse ".repeat(N)),
            n,
        ),
        (
            "else if, comma bodies",
            SourceType::ts(),
            format!("{}x;", "if (a) x, y;\nelse ".repeat(N)),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A function's body, and an arrow's expression body, after a return type
/// annotation are expressions: their `|`, `&` and `>` nest as operators and
/// their line breaks end statements as a script's do.
#[test]
fn a_body_after_a_return_type_is_an_expression() {
    let n = N as u32;
    let ors = vec!["a"; N + 1].join(" | ");
    let greater = vec!["a"; N + 1].join(" > ");
    let lines = vec!["a"; N + 1].join("\n+ ");
    let failures = under_counted(vec![
        (
            "function body",
            SourceType::ts(),
            format!("function f(): T {{ return {ors} }}"),
            n,
        ),
        (
            "method body",
            SourceType::ts(),
            format!("class C {{ m(): T {{ return {ors} }} }}"),
            n,
        ),
        (
            "object method body",
            SourceType::ts(),
            format!("const o = {{ m(): T {{ return {ors} }} }}"),
            n,
        ),
        (
            "arrow expression body",
            SourceType::ts(),
            format!("const f = (x): T => {ors}"),
            n,
        ),
        (
            "arrow comparison body",
            SourceType::ts(),
            format!("const f = (x): T => {greater}"),
            n,
        ),
        (
            "arrow body across lines",
            SourceType::ts(),
            format!("const f = (x): T => {lines}"),
            n,
        ),
        (
            "after a function expression's body",
            SourceType::ts(),
            format!("x = function (): T {{}} | {ors}"),
            n,
        ),
        (
            "tsx arrow body",
            SourceType::tsx(),
            format!("const f = (x): T => {ors}"),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A `<` in an expression may open type arguments or parameters, whose
/// commas separate nested siblings of one list: a comma does not start
/// the chain over while a `<` of its segment is open.
#[test]
fn type_arguments_in_an_expression_nest_across_their_commas() {
    let n = N as u32;
    let nested = format!("{}A{}", "Box<A, ".repeat(N), ">".repeat(N));
    let failures = under_counted(vec![
        ("call", SourceType::ts(), format!("f<{nested}>();"), n),
        ("new", SourceType::ts(), format!("new Map<{nested}>();"), n),
        ("as", SourceType::ts(), format!("x as {nested};"), n),
        (
            "function type parameter",
            SourceType::ts(),
            format!("function f<T extends {nested}>() {{}}"),
            n,
        ),
        (
            "class type parameter",
            SourceType::ts(),
            format!("class C<T extends {nested}> {{}}"),
            n,
        ),
        (
            "interface type parameter",
            SourceType::ts(),
            format!("interface I<T extends {nested}> {{}}"),
            n,
        ),
        (
            "alias type parameter",
            SourceType::ts(),
            format!("type A<T extends {nested}> = T;"),
            n,
        ),
        (
            "heritage",
            SourceType::ts(),
            format!("class C extends B<{nested}> {{}}"),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A JSX element's name nests once per member access, its type arguments
/// as a type's do, an element given as an attribute's value under the
/// element, and a tag's comments and whitespace are skipped.
#[test]
fn jsx_names_type_arguments_and_attribute_elements_nest() {
    let n = N as u32;
    let tsx = SourceType::tsx();
    let nested = format!("{}A{}", "Box<A, ".repeat(N), ">".repeat(N));
    let parens = format!("{}1{}", "(".repeat(N), ")".repeat(N));
    let failures = under_counted(vec![
        (
            "member name",
            tsx,
            format!("const v = <a{} />;", ".a".repeat(N)),
            n,
        ),
        (
            "member closing name",
            tsx,
            format!("const v = <a{0}></a{0}>;", ".a".repeat(N)),
            n,
        ),
        (
            "type arguments",
            tsx,
            format!("const v = <C<{nested}> />;"),
            n,
        ),
        (
            "attribute element",
            tsx,
            format!("const v = {}{};", "<a b=".repeat(N), " />".repeat(N)),
            n,
        ),
        (
            "space after `<`",
            tsx,
            format!("const v = {}x{};", "< div>".repeat(N), "</div>".repeat(N)),
            n,
        ),
        (
            "comment in a tag",
            tsx,
            format!(
                "const v = {}x{};",
                "<div /* > */ a={1}>".repeat(N),
                "</div>".repeat(N)
            ),
            n,
        ),
        (
            "attribute element holding a quote",
            tsx,
            format!("const v = <a b=<b>it's</b> c={{{parens}}} />;"),
            n,
        ),
        (
            "space after `<` before text holding a quote",
            tsx,
            format!("const v = < div>it's {{{parens}}}</div>;"),
            n,
        ),
        (
            "comment holding a quote in a tag",
            tsx,
            format!("const v = <div /* it's */>{{{parens}}}</div>;"),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What the scan skips as a string, comment, regular expression or
/// template text ends where the lexical grammar ends it, so the code after
/// it is scanned: a `/` inside a regular expression's class, a string's
/// `\` before a CRLF line break, a line comment ended by a CR or a
/// U+2028, and an HTML-like comment outside a module.
#[test]
fn skipped_text_ends_at_its_token_boundary() {
    let n = N as u32;
    let chain = ".a".repeat(N);
    let parens = format!("{}1{}", "(".repeat(N), ")".repeat(N));
    let failures = under_counted(vec![
        (
            "regular expression class holding `//`",
            SourceType::ts(),
            format!("r = /[///]/; v = x{chain};"),
            n,
        ),
        (
            "string line continuation before CRLF",
            SourceType::ts(),
            format!("s = 'a\\\r\n'; v = {parens};"),
            n,
        ),
        (
            "line comment ended by CR",
            SourceType::ts(),
            format!("// c\rv = {parens};"),
            n,
        ),
        (
            "line comment ended by U+2028",
            SourceType::ts(),
            format!("// c\u{2028}v = {parens};"),
            n,
        ),
        (
            "HTML-like comment holding a backtick",
            SourceType::ts(),
            format!("x = 1 <!-- `\nv = {parens};\n// `"),
            n,
        ),
        (
            "HTML-like close comment holding a backtick",
            SourceType::cjs(),
            format!("x = 1\n--> `\nv = {parens};\n// `"),
            n,
        ),
        (
            "nested template holes",
            SourceType::ts(),
            format!("v = {}1{};", "`${".repeat(N), "}`".repeat(N)),
            n,
        ),
        (
            "tagged template chain",
            SourceType::ts(),
            format!("v = t{};", "``".repeat(N)),
            n,
        ),
        (
            "brackets in strings and comments",
            SourceType::ts(),
            format!("v = '(((' + /* ((( */ \"[[[\" + x{chain};"),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where the previous token decides whether `/` divides or starts a
/// regular expression, a keyword read as a property name, an identifier
/// spelled like a type operator and an object literal's `}` divide; where
/// the token alone cannot decide (`await`, `yield` and `of` may be
/// identifiers, and the scan does not always tell a function or class
/// expression's body from a declaration's), the rest of the source is
/// bounded by its length.
#[test]
fn a_division_is_not_read_as_a_regular_expression() {
    let n = N as u32;
    let chain = ".a".repeat(N);
    let failures = under_counted(vec![
        (
            "keyword as a property name",
            SourceType::ts(),
            format!("v = x.if(b) / x{chain};"),
            n,
        ),
        (
            "keyword as a private name",
            SourceType::ts(),
            format!("class C {{ m() {{ v = this.#if(b) / x{chain}; }} }}"),
            n,
        ),
        (
            "identifier `as`",
            SourceType::ts(),
            format!("v = as / x{chain};"),
            n,
        ),
        (
            "identifier `keyof`",
            SourceType::ts(),
            format!("v = keyof / x{chain};"),
            n,
        ),
        (
            "object literal default export",
            SourceType::ts(),
            format!("export default {{}} / x{chain};"),
            n,
        ),
        (
            "object literal member",
            SourceType::ts(),
            format!("v = {{ a: {{}} / x{chain} }};"),
            n,
        ),
        (
            "function expression",
            SourceType::ts(),
            format!("v = function () {{}} / x{chain};"),
            n,
        ),
        (
            "class expression",
            SourceType::ts(),
            format!("v = class {{}} / x{chain};"),
            n,
        ),
        (
            "identifier `await`",
            SourceType::cjs(),
            format!("v = await / x{chain};"),
            n,
        ),
        (
            "identifier `yield`",
            SourceType::cjs(),
            format!("v = yield / x{chain};"),
            n,
        ),
        (
            "identifier `of`",
            SourceType::ts(),
            format!("v = of / x{chain};"),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where the previous token decides whether `/` divides, the rest of the
/// source keeps its own depth: an object literal a module exports by
/// default, a keyword read as a property name and a declaration's body
/// each decide it.
#[test]
fn a_decided_slash_keeps_the_rest_of_the_source_shallow() {
    let rest = "x;\n".repeat(1_000);
    for decided in [
        "export default {} / 2;",
        "v = { a: {} / 2 };",
        "v = x.of / 2;",
        "v = x.await / 2;",
        "function f() {}\n/re/.test(x);",
        "class C { m() {} }\n/re/.test(x);",
        "if (a) {}\n/re/.test(x);",
    ] {
        assert!(ts(&format!("{decided}\n{rest}")) < 10, "{decided}");
    }
}

/// A non-ASCII space or line terminator separates tokens as an ASCII one
/// does: `typeof\u{a0}typeof\u{a0}x` is two prefix operators, not one
/// identifier, and a regular expression after `return\u{a0}` is one.
#[test]
fn unicode_spaces_separate_tokens() {
    let n = N as u32;
    let parens = format!("{}1{}", "(".repeat(N), ")".repeat(N));
    let failures = under_counted(vec![
        (
            "no-break space",
            SourceType::ts(),
            format!("v = {}x;", "typeof\u{a0}".repeat(N)),
            n,
        ),
        (
            "ideographic space",
            SourceType::ts(),
            format!("v = {}x;", "typeof\u{3000}".repeat(N)),
            n,
        ),
        (
            "byte order mark",
            SourceType::ts(),
            format!("v = {}x;", "typeof\u{feff}".repeat(N)),
            n,
        ),
        (
            "line separator",
            SourceType::ts(),
            format!("v = {}x;", "typeof\u{2028}".repeat(N)),
            n,
        ),
        (
            "no-break space before a regular expression",
            SourceType::ts(),
            format!("function f() {{ return\u{a0}/[`]/ }}\nv = {parens};\n// `"),
            n,
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn siblings_and_statements_start_over() {
    assert_eq!(ts("f(a + b, c + d, e + f)"), 2);
    assert_eq!(ts("a + b; c + d; e + f;"), 1);
    // A line break ends a statement whose next line starts a new one.
    assert_eq!(ts("x = a + b\ny = c + d\nz = e + f\n"), 2);
    // It does not where the next line continues the expression.
    assert_eq!(ts("x = a\n+ b\n+ c\n"), 3);
    let many = (0..500)
        .map(|i| format!("let v{i} = a.b.c(d)\n"))
        .collect::<String>();
    assert_eq!(ts(&many), 4);
}

#[test]
fn an_else_if_chain_nests_once_per_branch() {
    let chain = (0..50)
        .map(|i| format!("if (x === {i}) {{ f() }}\nelse "))
        .collect::<String>();
    let chain = format!("{chain}{{ g() }}");
    assert!(ts(&chain) >= 50, "{}", ts(&chain));
}

#[test]
fn a_union_is_flat_only_where_the_source_is_a_type() {
    let members = (0..5000)
        .map(|i| format!("'m{i}'"))
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(ts(&format!("type U = {members};")) <= 2);
    assert!(
        ts(&format!(
            "type U =\n  | {members}\n  | 'last'\nexport const x = 1"
        )) <= 2
    );
    assert!(ts(&format!("interface I {{ u: {members}; v: {members} }}")) <= 3);
    assert!(
        depth(
            &format!("export declare function f(u: {members}): {members};"),
            SourceType::d_ts()
        ) <= 3
    );
    // An annotation's union is a type: a parameter's, a variable's, a class
    // member's, a return type's, up to an initializer.
    assert!(ts(&format!("function f(x:\n  | {members}\n) {{ return x }}")) <= 4);
    assert!(ts(&format!("let v: {members} = 'm1'")) <= 4);
    assert!(
        ts(&format!(
            "class C {{ p?: {members}; m(): {members} {{ return 'm1' }} }}"
        )) <= 5
    );
    assert!(ts(&format!("const g = (x: {members}): {members} => x")) <= 5);
    // An object literal's or a default value's `|` is an expression.
    let ors = vec!["1"; 500].join(" | ");
    assert!(ts(&format!("const o = {{ a: {ors} }}")) >= 500);
    assert!(ts(&format!("function f(x: number = {ors}) {{}}")) >= 500);
    assert!(ts(&format!("const t = b ? {ors} : 2")) >= 500);
    // Qualified members are siblings under the union, not a chain.
    let qualified = (0..500)
        .map(|i| format!("Kind.K{i}"))
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(ts(&format!("type U = {qualified};")) <= 3);
    // A union in each branch of a conditional chain does not hide the chain.
    let conditional = format!("type D = {}4;", "1 extends 2 ? 3 | 5 : ".repeat(300));
    assert!(ts(&conditional) >= 300);
    // An expression's `|` nests.
    assert!(ts(&format!("const x = {}", vec!["1"; 5000].join(" | "))) >= 5000);
    // A declaration file's initializer and enum members are expressions.
    assert!(
        depth(
            &format!("declare enum E {{ A = {} }}", vec!["1"; 500].join(" | ")),
            SourceType::d_ts()
        ) >= 500
    );
    // A statement after a type alias without a semicolon is not a type.
    assert!(ts(&format!("type A = B\n[{}]", vec!["1"; 500].join(" | "))) >= 500);
}

#[test]
fn strings_comments_regexes_and_templates_are_skipped() {
    assert_eq!(ts("'((((((' + \"[[[[[\""), 1);
    assert_eq!(ts("// ((((((\n/* [[[[[ */ a"), 0);
    assert!(ts("const r = /[(]+\\(/g") <= 4);
    assert_eq!(ts("x = a / b / c"), 3);
    assert_eq!(ts("`(((( ${a + b} ((((`"), 3);
    // A regular expression's own groups still count.
    assert!(ts(&format!("r = /{}a{}/", "(".repeat(300), ")".repeat(300))) >= 300);
}

#[test]
fn jsx_elements_nest_but_siblings_and_text_do_not() {
    let tsx = SourceType::tsx();
    let siblings = (0..500)
        .map(|_| "<li>item, (text); </li>")
        .collect::<String>();
    assert!(depth(&format!("const v = <ul>{siblings}</ul>"), tsx) <= 6);
    let nested = format!("const v = {}x{}", "<div>".repeat(300), "</div>".repeat(300));
    assert!(depth(&nested, tsx) >= 300);
    // A TSX arrow function's type parameters are not an element:
    // what follows them is scanned as script.
    assert!(depth("const f = <T,>(x: T) => x\nconst g = ((((((1))))))", tsx) >= 6);
}

#[test]
fn realistic_sources_nest_shallowly() {
    let source = r#"
import { ref, computed } from 'vue'
export default defineComponent({
  props: { items: { type: Array as PropType<Item[]>, required: true } },
  setup(props, { emit }) {
    const selected = ref<string | null>(null)
    const visible = computed(() => props.items.filter((item) => item.visible && !item.hidden).map((item) => item.id))
    function select(id: string) {
      if (selected.value === id) { selected.value = null } else if (id) { selected.value = id } else { emit('clear') }
    }
    return { selected, visible, select }
  },
})
"#;
    assert!(ts(source) < 40, "{}", ts(source));
}

/// oxc alone, on an ordinary 8 MiB thread with no containment: 10,000 nested
/// type arguments.
fn oxc_parses_10000_nested_type_arguments_on_an_8_mib_thread() -> bool {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let source = format!("type D = {}1{};", "Box<".repeat(10_000), ">".repeat(10_000));
            let allocator = Allocator::default();
            let parsed = oxc_parser::Parser::new(&allocator, &source, SourceType::ts()).parse();
            !parsed.fatal_error && parsed.diagnostics.is_empty()
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the parse returns")
}

/// The environment variable that makes this test binary run the oxc-only
/// parse itself ([`oxc_still_overflows_on_10000_nested_type_arguments`]).
const OXC_CANARY_CHILD: &str = "VERTER_OXC_DEEP_PARSE_CANARY_CHILD";

/// Canary: oxc's parser returns an AST for 10,000 nested type arguments on
/// an ordinary 8 MiB thread. It does not today — its recursive descent
/// overflows the thread and aborts the process
/// (`docs/evidence/signature-kernel/oxc-deep-parse.md`), which is why every
/// parse goes through [`Parser`]'s stack containment. When it passes, oxc
/// parses this depth by itself and the containment can be revisited.
#[test]
#[ignore = "oxc parser recursion limit; passes once oxc parses this depth"]
fn oxc_parses_deep_nesting_on_an_ordinary_stack() {
    assert!(oxc_parses_10000_nested_type_arguments_on_an_8_mib_thread());
}

/// The same oxc-only parse, run in a child process so its abort is
/// observed rather than fatal: it still overflows. The day this fails
/// because the child succeeded, un-ignore
/// [`oxc_parses_deep_nesting_on_an_ordinary_stack`] and revisit the
/// containment.
#[test]
fn oxc_still_overflows_on_10000_nested_type_arguments() {
    if std::env::var_os(OXC_CANARY_CHILD).is_some() {
        oxc_parses_10000_nested_type_arguments_on_an_8_mib_thread();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "oxc_parse::tests::oxc_still_overflows_on_10000_nested_type_arguments",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(OXC_CANARY_CHILD, "1")
        .output()
        .expect("run the oxc-only parse in a child process");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains("has overflowed its stack"),
        "oxc now parses 10,000 nested type arguments on an 8 MiB thread \
         (status {:?}); un-ignore `oxc_parses_deep_nesting_on_an_ordinary_stack` \
         and revisit the parse containment",
        output.status
    );
}

/// The walks of oxc's (a `Visit` or `VisitMut` entry, `clone_in`, the
/// semantic builder) that recurse once per level of a syntax tree.
const WALK_ENTRIES: [&str; 5] = [
    ".clone_in(",
    "SemanticBuilder::new(",
    ".visit_",
    "walk::walk_",
    "walk_mut::walk_",
];

/// The containments a walk of oxc's runs under.
const CONTAINMENTS: [&str; 7] = [
    "with_program_stack",
    "with_node_stack",
    "with_ast_stack",
    "with_source_stack",
    "with_span_stack",
    "with_nesting_stack",
    "with_own_syntax_stack",
];

/// `source` with its comments, strings and character literals blanked
/// (their bytes turned to spaces), so its brackets are the code's.
fn code_only(source: &str) -> Vec<u8> {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        let to = to.min(out.len());
        for byte in &mut out[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = source[i..].find('\n').map_or(bytes.len(), |at| i + at);
                blank(&mut out, i, end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = source[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |at| i + 2 + at + 2);
                blank(&mut out, i, end);
                i = end;
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'"' | b'#'))
                && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) =>
            {
                let hashes = bytes[i + 1..].iter().take_while(|&&b| b == b'#').count();
                if bytes.get(i + 1 + hashes) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close = format!("\"{}", "#".repeat(hashes));
                let body = i + 2 + hashes;
                let end = source[body..]
                    .find(&close)
                    .map_or(bytes.len(), |at| body + at + close.len());
                blank(&mut out, i, end);
                i = end;
            }
            b'"' => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                blank(&mut out, i, end + 1);
                i = end + 1;
            }
            b'\'' => {
                // A character literal (`'x'`, `'\n'`, `'\u{..}'`); a lifetime
                // has no closing quote.
                let rest = &source[i + 1..];
                let len = if rest.starts_with('\\') {
                    rest.find('\'').filter(|&at| at > 1).map(|at| at + 1)
                } else {
                    rest.chars()
                        .next()
                        .filter(|c| rest[c.len_utf8()..].starts_with('\''))
                        .map(|c| c.len_utf8() + 1)
                };
                match len {
                    Some(len) => {
                        blank(&mut out, i, i + 1 + len);
                        i += 1 + len;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// One production source file the guard reads: its code (comments,
/// strings and character literals blanked) up to its inline test module.
struct GuardFile {
    path: std::path::PathBuf,
    source: String,
    code: Vec<u8>,
}

/// Whether the code at `at` in `files[file]` runs contained: inside a
/// closure or argument of one of the [`CONTAINMENTS`] called in its own
/// function; in a step of a walk (a `visit_…` method, entered only from a
/// walk, as a call to one from anywhere else is itself an entry this guard
/// checks); or in a helper every call of which in the workspace runs
/// contained (a helper only walk steps or containments call; a private
/// one's calls are its file's, a public one's the workspace's). `callers`
/// holds the helpers being traced, so a recursive helper is not assumed to
/// contain itself.
fn contained(files: &[GuardFile], file: usize, at: usize, callers: &mut Vec<String>) -> bool {
    let code = &files[file].code;
    let (mut parens, mut braces) = (0usize, 0usize);
    let mut i = at;
    while i > 0 {
        i -= 1;
        match code[i] {
            b')' => parens += 1,
            b'(' if parens > 0 => parens -= 1,
            b'(' => {
                let callee = code[..i].trim_ascii_end();
                if CONTAINMENTS
                    .iter()
                    .any(|name| callee.ends_with(name.as_bytes()))
                {
                    return true;
                }
            }
            b'}' => braces += 1,
            b'{' if braces > 0 => braces -= 1,
            b'{' => {
                // The body of the enclosing function ends the search.
                let head_start = code[..i]
                    .iter()
                    .rposition(|&b| matches!(b, b';' | b'{' | b'}'))
                    .map_or(0, |at| at + 1);
                let head = String::from_utf8_lossy(&code[head_start..i]);
                let public = head
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .any(|word| word == "pub");
                let mut words = head
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .filter(|word| !word.is_empty());
                if words.any(|word| word == "fn") {
                    let Some(name) = words.next() else {
                        return false;
                    };
                    if name.starts_with("visit_") {
                        return true;
                    }
                    if callers.iter().any(|caller| caller == name) {
                        return false;
                    }
                    callers.push(name.to_string());
                    // A private helper is called only from its own file.
                    let calls: Vec<(usize, usize)> = calls_of(files, name)
                        .into_iter()
                        .filter(|&(caller, _)| public || caller == file)
                        .collect();
                    let contained = !calls.is_empty()
                        && calls
                            .into_iter()
                            .all(|(file, call)| contained(files, file, call, callers));
                    callers.pop();
                    return contained;
                }
            }
            _ => {}
        }
    }
    false
}

/// Where the workspace's production code uses a function named `name`: a
/// call, or the function passed as a value (not where it defines or
/// imports one).
fn calls_of(files: &[GuardFile], name: &str) -> Vec<(usize, usize)> {
    let mut calls = Vec::new();
    for (file, guard_file) in files.iter().enumerate() {
        let text = String::from_utf8_lossy(&guard_file.code);
        for (at, _) in text.match_indices(name) {
            let before = text[..at].chars().next_back();
            let after = &text[at + name.len()..];
            // The statement the name sits in, from the end of the one before.
            let statement_start = text[..at].rfind([';', '}']).map_or(0, |at| at + 1);
            let statement = text[statement_start..at].trim_start();
            let imported = ["use ", "pub use ", "pub(crate) use ", "pub(super) use "]
                .iter()
                .any(|import| statement.starts_with(import));
            if before.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
                && !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                && !after.trim_start().starts_with('!')
                && !text[..at].trim_end().ends_with("fn")
                && !imported
            {
                calls.push((file, at));
            }
        }
    }
    calls
}

/// Every walk of oxc's in the workspace's crates runs under one of this
/// module's containments: a walk recurses once per level of the tree it
/// walks, and on a thread's fixed stack a deep enough tree overflows it.
/// Test code walks on its own test thread and is not checked.
#[test]
fn no_crate_walks_oxc_syntax_around_the_containment() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut files = Vec::new();
    let mut stack = Vec::new();
    for entry in std::fs::read_dir(crates)
        .expect("read the crates directory")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "verter_bench" || name == "verter_type_expr_oxc" {
            continue;
        }
        stack.push(entry.path().join("src"));
    }
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" && !name.ends_with("_tests") && name != "oxc_parse" {
                    stack.push(path);
                }
                continue;
            }
            if !name.ends_with(".rs")
                || name == "tests.rs"
                || name.ends_with("_tests.rs")
                || name == "test_support.rs"
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read a source file");
            let mut code = code_only(&source);
            // An inline test module walks on its test thread.
            if let Some(end) = String::from_utf8_lossy(&code).find("#[cfg(test)]\nmod ") {
                code.truncate(end);
            }
            files.push(GuardFile { path, source, code });
        }
    }
    let mut bypasses = Vec::new();
    for (file, guard_file) in files.iter().enumerate() {
        let visits = guard_file.source.contains("oxc_ast_visit");
        let text = String::from_utf8_lossy(&guard_file.code);
        for walk in WALK_ENTRIES {
            for (at, _) in text.match_indices(walk) {
                // A visitor steps its own walk through `self`.
                let step = match walk {
                    ".visit_" => text[..at]
                        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .next()
                        .is_some_and(|receiver| receiver == "self"),
                    "walk::walk_" | "walk_mut::walk_" => text[at..]
                        .split_once('(')
                        .is_some_and(|(_, arguments)| arguments.trim_start().starts_with("self")),
                    _ => false,
                };
                let visitor_walk = walk != ".clone_in(" && walk != "SemanticBuilder::new(";
                if step || (visitor_walk && !visits) {
                    continue;
                }
                if !contained(&files, file, at, &mut Vec::new()) {
                    let line = text[..at].matches('\n').count();
                    bypasses.push(format!(
                        "{}:{}: {}",
                        guard_file.path.display(),
                        line + 1,
                        guard_file.source.lines().nth(line).unwrap_or("").trim()
                    ));
                }
            }
        }
    }
    assert!(
        bypasses.is_empty(),
        "walk oxc syntax under `with_program_stack`, `with_node_stack`, `with_ast_stack`, \
         `with_source_stack`, `with_span_stack` or `with_nesting_stack` ({} sites):\n{}",
        bypasses.len(),
        bypasses.join("\n")
    );
}

/// Walk stacks sharing one program's cell scan the program once between
/// them, however many long nodes they walk.
#[test]
fn walk_stacks_sharing_a_program_scan_it_once() {
    let scanned = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let source = format!("export const v = {}1;", "() => ".repeat(2_000));
            let allocator = oxc_allocator::Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            let cell = std::cell::OnceCell::new();
            super::scan_probe::take();
            for _ in 0..3 {
                let walks = super::ProgramWalkStack::sharing(&parsed.program, &cell);
                walks.with_node_stack(parsed.program.span, || ());
            }
            super::scan_probe::take()
        })
        .expect("spawn the walking thread")
        .join()
        .expect("the walks return");
    assert_eq!(
        scanned,
        format!("export const v = {}1;", "() => ".repeat(2_000)).len()
    );
}

/// Inside [`super::ProgramWalkStack::within`], a walk of any node of the
/// program runs on the stack the containment grew once, never on a
/// segment of its own.
#[test]
fn walks_within_a_program_containment_run_in_place() {
    struct Owner<'p> {
        walks: super::ProgramWalkStack<'p>,
    }
    let in_place = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let source = format!("export const v = {}1;", "() => ".repeat(2_000));
            let allocator = oxc_allocator::Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            let mut owner = Owner {
                walks: super::ProgramWalkStack::new(&parsed.program),
            };
            super::ProgramWalkStack::within(
                &mut owner,
                |owner| &owner.walks,
                |owner| {
                    let outer = super::stack::remaining().expect("the stack is known");
                    let inner = owner
                        .walks
                        .with_node_stack(parsed.program.span, super::stack::remaining)
                        .expect("the stack is known");
                    // The same segment: the walk ran a few frames deeper.
                    outer >= inner && outer - inner < 64 * 1024
                },
            )
        })
        .expect("spawn the walking thread")
        .join()
        .expect("the walks return");
    assert!(in_place);
}

/// A function's own syntax is its text with each nested body taken out.
#[test]
fn own_syntax_takes_the_nested_bodies_out() {
    let source = "f(() => { a(); }, function g() { return 1; })";
    let body = |text: &str| {
        let start = source.find(text).unwrap() as u32;
        oxc_span::Span::new(start, start + text.len() as u32)
    };
    let own = super::own_syntax_text(
        source,
        oxc_span::Span::new(0, source.len() as u32),
        [body("{ return 1; }"), body("{ a(); }")],
    );
    assert_eq!(own.as_deref(), Some("f(() => 0, function g() 0)"));
}

/// This process's commit charge, in bytes.
#[cfg(windows)]
fn committed_bytes() -> usize {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: `counters` is a live `PROCESS_MEMORY_COUNTERS_EX` whose `cb`
    // holds its size, which the call may fill as its prefix
    // `PROCESS_MEMORY_COUNTERS`.
    let read = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    assert_ne!(read, 0, "read the process memory counters");
    counters.PrivateUsage
}

/// The environment variable that makes this test binary measure a grown
/// stack's commit itself
/// ([`a_grown_stack_commits_what_the_walk_touches_not_what_it_reserves`]).
#[cfg(windows)]
const COMMIT_CHILD: &str = "VERTER_GROWN_STACK_COMMIT_CHILD";

/// A stack grown for a deep source reserves what its nesting can need and
/// commits what the walk on it touches: a 1,000,000-level bound reserves
/// about 8.6 GiB, and a walk a few frames deep commits a few pages of it.
/// Committing the reservation up front made a handful of deep sources
/// exhaust the system's commit limit. The measure runs in a child process,
/// so no other test's allocations move it.
#[cfg(windows)]
#[test]
fn a_grown_stack_commits_what_the_walk_touches_not_what_it_reserves() {
    if std::env::var_os(COMMIT_CHILD).is_some() {
        let committed = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                let before = committed_bytes();
                let during =
                    super::with_nesting_stack(super::Nesting { depth: 1_000_000 }, committed_bytes);
                during.saturating_sub(before)
            })
            .expect("spawn the measuring thread")
            .join()
            .expect("the measure returns");
        println!("{COMMIT_CHILD}: {committed}");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "oxc_parse::tests::a_grown_stack_commits_what_the_walk_touches_not_what_it_reserves",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(COMMIT_CHILD, "1")
        .output()
        .expect("measure in a child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let committed: usize = stdout
        .lines()
        .find_map(|line| line.split_once(&format!("{COMMIT_CHILD}: ")))
        .and_then(|(_, bytes)| bytes.trim().parse().ok())
        .unwrap_or_else(|| {
            panic!(
                "the child measured nothing ({:?}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert!(
        committed < 64 << 20,
        "a grown stack committed {committed} bytes"
    );
}

/// A stack no host can reserve is a typed failure, not a panic: the work
/// does not run.
#[test]
fn a_stack_that_cannot_be_reserved_is_a_typed_failure() {
    let needed = 1usize << (usize::BITS - 2);
    let mut ran = false;
    let result = super::stack::with_stack(needed, super::stack::Reservation::Walk, || ran = true);
    assert_eq!(result, Err(super::StackUnavailable { needed }));
    assert!(!ran);
}

/// A parse that cannot have its stack returns an empty program whose one
/// diagnostic says so.
#[test]
fn a_parse_without_its_stack_returns_the_typed_diagnostic() {
    let allocator = Allocator::default();
    let unparsed = super::unparsed(
        &allocator,
        SourceType::ts(),
        oxc_parser::ParseOptions::default(),
        super::StackUnavailable { needed: 1 << 40 },
    );
    assert!(unparsed.program.body.is_empty());
    assert!(unparsed.fatal_error, "the program is not the source's");
    assert_eq!(unparsed.diagnostics.len(), 1);
    let diagnostic = unparsed.diagnostics.errors().next().expect("one error");
    assert!(super::is_stack_unavailable(diagnostic), "{diagnostic:?}");
    assert_eq!(
        super::parse_refusal(&unparsed),
        Some(super::StackUnavailable { needed: 1 << 40 })
    );
}

/// Run `work` on a fresh 1 MiB thread, its reservation count and fault
/// injection its own.
fn on_a_small_thread<R: Send + 'static>(work: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(work)
        .expect("spawn the thread")
        .join()
        .expect("the work returns")
}

/// A source 10,000 parentheses deep: its parse and walks need a region.
fn deep_source() -> String {
    format!(
        "export const v = {}1{};",
        "(".repeat(10_000),
        ")".repeat(10_000)
    )
}

/// A parse whose region cannot be reserved returns the typed diagnostic in
/// place of the program, marked fatal, and the thread goes on: the same
/// parse retried, and a shallow one, parse.
#[test]
fn a_parse_whose_region_cannot_be_reserved_is_typed_and_a_retry_parses() {
    let (refused, retried, shallow) = on_a_small_thread(|| {
        let source = deep_source();
        let allocator = Allocator::default();
        super::faults::fail_next_reservations(1);
        let refused = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let refused = (
            refused.fatal_error,
            refused.program.body.len(),
            refused
                .diagnostics
                .errors()
                .all(super::is_stack_unavailable),
            refused.diagnostics.len(),
        );
        let retried = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let shallow = Parser::new(&allocator, "export const v = (1);", SourceType::ts()).parse();
        (
            refused,
            (retried.program.body.len(), retried.diagnostics.len()),
            (shallow.program.body.len(), shallow.diagnostics.len()),
        )
    });
    assert_eq!(refused, (true, 0, true, 1));
    assert_eq!(retried, (1, 0));
    assert_eq!(shallow, (1, 0));
}

/// A walk-stack lease whose region cannot be reserved is the operation's
/// typed failure, and the operation's walks never start; the lease
/// retried runs them.
#[test]
fn a_walk_stack_lease_that_cannot_be_reserved_starts_no_walk() {
    use oxc_allocator::CloneIn;
    let (refused, started, retried) = on_a_small_thread(|| {
        let source = deep_source();
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let program = &parsed.program;
        let clones = Allocator::default();
        let started = std::cell::Cell::new(false);
        super::faults::fail_next_reservations(1);
        let refused = super::with_program_walk_stack_lease(program, || {
            started.set(true);
            super::with_program_stack(program, || program.clone_in(&clones).body.len())
        });
        let retried = super::with_program_walk_stack_lease(program, || {
            super::with_program_stack(program, || program.clone_in(&clones).body.len())
        });
        (refused.is_err(), started.get(), retried)
    });
    assert!(refused, "the lease is refused");
    assert!(!started, "no walk starts under a refused lease");
    assert_eq!(retried, Ok(1));
}

/// Every walk inside a walk-stack lease, of the whole program, of its
/// statement, by its text, and nested under another, runs on the lease's
/// region: the operation reserves once. Without the lease each reserves a
/// region of its own.
#[test]
fn walks_inside_a_walk_stack_lease_reserve_nothing_more() {
    use oxc_allocator::CloneIn;
    use oxc_ast_visit::Visit;
    use oxc_span::GetSpan;
    #[derive(Default)]
    struct Count(usize);
    impl<'a> Visit<'a> for Count {
        fn enter_node(&mut self, _kind: oxc_ast::AstKind<'a>) {
            self.0 += 1;
        }
    }
    let walks = |leased: bool| {
        on_a_small_thread(move || {
            let source = deep_source();
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            let program = &parsed.program;
            let statement = &program.body[0];
            let clones = Allocator::default();
            let work = || {
                let cloned =
                    super::with_program_stack(program, || program.clone_in(&clones).body.len());
                let mut whole = Count::default();
                super::with_program_stack(program, || whole.visit_program(program));
                let mut nested = Count::default();
                super::with_node_stack(program, statement.span(), || {
                    super::with_span_stack(&source, statement.span(), || {
                        nested.visit_statement(statement)
                    })
                });
                let node_walks = super::ProgramWalkStack::new(program);
                let mut node = Count::default();
                node_walks.with_node_stack(statement.span(), || node.visit_statement(statement));
                (cloned, whole.0 > 10_000, nested.0 == node.0)
            };
            super::faults::take_reservations();
            let walked = if leased {
                super::with_program_walk_stack_lease(program, work).expect("the lease")
            } else {
                work()
            };
            (walked, super::faults::take_reservations())
        })
    };
    assert_eq!(walks(true), ((1, true, true), 1));
    let (walked, reserved) = walks(false);
    assert_eq!(walked, (1, true, true));
    assert!(reserved > 1, "{reserved}");
}

/// A walk that panics on the lease's region, and an operation that panics
/// out of its lease, leave the thread on its own stack with its enclosing
/// lease: the next walks run as before, and a lease ended by a panic
/// covers nothing after it.
#[test]
fn a_panic_on_a_lease_region_restores_the_enclosing_stack_and_lease() {
    use oxc_allocator::CloneIn;
    let outcome = on_a_small_thread(|| {
        let source = deep_source();
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let program = &parsed.program;
        let clones = Allocator::default();
        let before = super::stack::remaining();
        let inside = super::with_program_walk_stack_lease(program, || {
            let lease_before = super::stack::remaining();
            let walk = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::with_program_stack(program, || -> usize { panic!("the walk fails") })
            }));
            let restored = super::stack::remaining() == lease_before;
            super::faults::take_reservations();
            let again = super::with_program_stack(program, || program.clone_in(&clones).body.len());
            (
                walk.is_err(),
                restored,
                again,
                super::faults::take_reservations(),
            )
        })
        .expect("the lease");
        let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::with_program_walk_stack_lease(program, || -> usize {
                panic!("the operation fails")
            })
        }));
        let needed = super::parse_stack_bytes(&source, SourceType::ts());
        (
            inside,
            escaped.is_err(),
            super::stack::lease_covers(needed),
            super::stack::remaining() == before,
        )
    });
    assert_eq!(outcome, ((true, true, 1, 0), true, false, true));
}

/// The V8 engine-stack profile parses a source whose scan bound is its
/// nesting and refuses one level more, typed, before oxc runs.
#[test]
fn the_engine_stack_profile_admits_its_nesting_and_refuses_one_more() {
    let profile = super::V8_DEFAULT_STACK_PROFILE;
    let limit = profile.nesting();
    let nest = |depth: usize| format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    assert_eq!(ts(&nest(limit)), limit as u32);
    let parsed = |depth: usize| {
        let source = nest(depth);
        on_a_small_thread(move || {
            let ran = std::cell::Cell::new(false);
            let result =
                super::parse_with_stack_under(Some(profile), &source, SourceType::ts(), || {
                    ran.set(true)
                });
            (result.is_ok(), ran.get())
        })
    };
    assert_eq!(parsed(limit - 1), (true, true));
    assert_eq!(parsed(limit), (true, true));
    assert_eq!(parsed(limit + 1), (false, false));
    assert_eq!(parsed(limit * 100), (false, false));
}

/// A host whose stack grows carries no engine-stack profile: it refuses no
/// depth.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_host_whose_stack_grows_refuses_no_depth() {
    assert_eq!(super::HOST_ENGINE_STACK_PROFILE, None);
    let parsed = on_a_small_thread(|| {
        let source = format!("{}1{}", "[".repeat(200_000), "]".repeat(200_000));
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
        (parsed.program.body.len(), parsed.diagnostics.len())
    });
    assert_eq!(parsed, (1, 0));
}

/// The V8 engine-stack profile was measured against the `oxc_parser`
/// locked for the workspace: a different oxc spends a different stack per
/// level, and the profile is re-measured
/// (`docs/evidence/signature-kernel/oxc-deep-parse.md`, "WebAssembly")
/// before its version moves.
#[test]
fn the_engine_stack_profile_was_measured_against_the_locked_oxc() {
    let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(lock).expect("read the workspace's Cargo.lock");
    let locked = lock
        .split("[[package]]")
        .find(|package| package.contains("\nname = \"oxc_parser\"\n"))
        .and_then(|package| {
            package
                .lines()
                .find_map(|line| line.strip_prefix("version = \""))
                .map(|version| version.trim_end_matches('"').to_string())
        })
        .expect("oxc_parser is locked");
    assert_eq!(
        locked,
        super::V8_DEFAULT_STACK_PROFILE.oxc_version,
        "oxc_parser moved to {locked}: re-measure the engine-stack profile's per-level cost \
         (`oxc_deep_parse_wasi.mjs` and the wasm bisection in oxc-deep-parse.md) and update \
         `V8_DEFAULT_STACK_PROFILE`"
    );
}

/// Run `work` under `bytes` more of stack in use, spent a frame at a time.
fn with_stack_spent<R>(bytes: usize, work: impl FnOnce() -> R) -> R {
    fn spend(bytes: usize, work: &mut dyn FnMut()) {
        let frame = [0u8; 64 * 1024];
        std::hint::black_box(&frame);
        if bytes <= frame.len() {
            work();
        } else {
            spend(bytes - frame.len(), work);
        }
    }
    let mut work = Some(work);
    let mut result = None;
    spend(bytes, &mut || {
        result = Some((work.take().expect("runs once"))())
    });
    result.expect("the work ran")
}

/// A walk the lease covers, reached deep on a region reserved past the
/// lease while work on the lease's region waits under it, runs on a region
/// of its own: the lease's region is never re-entered under its own
/// suspended work.
#[test]
fn a_covered_walk_under_a_region_past_the_lease_does_not_reenter_the_lease() {
    use oxc_allocator::CloneIn;
    let (walked, reserved) = on_a_small_thread(|| {
        let leased_source = deep_source();
        let deeper_source = format!(
            "export const w = {}1{};",
            "(".repeat(11_000),
            ")".repeat(11_000)
        );
        let allocator = Allocator::default();
        let leased = Parser::new(&allocator, &leased_source, SourceType::ts()).parse();
        let deeper = Parser::new(&allocator, &deeper_source, SourceType::ts()).parse();
        let (leased, deeper) = (&leased.program, &deeper.program);
        let clones = Allocator::default();
        super::faults::take_reservations();
        let walked = super::with_program_walk_stack_lease(leased, || {
            // On the lease's region, a walk of a deeper program reserves a
            // region of its own; deep on that one, a walk of the leased
            // program has less left than it needs.
            super::with_program_stack(leased, || {
                super::with_program_stack(deeper, || {
                    with_stack_spent(24 << 20, || {
                        super::with_program_stack(leased, || leased.clone_in(&clones).body.len())
                    }) + deeper.clone_in(&clones).body.len()
                })
            })
        })
        .expect("the lease");
        (walked, super::faults::take_reservations())
    });
    assert_eq!(walked, 2);
    assert_eq!(reserved, 3, "the lease, the deeper walk, the walk under it");
}

/// The oxc syntax types a function recursing over the syntax tree takes by
/// reference.
const OXC_SYNTAX_TYPES: [&str; 31] = [
    "Expression",
    "Statement",
    "TSType",
    "BindingPattern",
    "ChainElement",
    "Argument",
    "ArrayExpressionElement",
    "ObjectPropertyKind",
    "PropertyKey",
    "AssignmentTarget",
    "SimpleAssignmentTarget",
    "TSTypeName",
    "JSXElement",
    "JSXChild",
    "Declaration",
    "ForStatementLeft",
    "FunctionBody",
    "Class",
    "ClassElement",
    "TSSignature",
    "Program",
    "BindingPatternKind",
    "AssignmentTargetMaybeDefault",
    "JSXExpression",
    "TemplateLiteral",
    "ObjectExpression",
    "CallExpression",
    "ArrowFunctionExpression",
    "Function",
    "JSXAttributeItem",
    "TSTypeParameterInstantiation",
];

/// Whether a function's parameters take oxc syntax by reference: `&T<`,
/// `&'a T<`, `&mut T<`, `&oxc_ast::ast::T<`.
fn takes_oxc_syntax(parameters: &str) -> bool {
    OXC_SYNTAX_TYPES.iter().any(|name| {
        parameters
            .match_indices(&format!("{name}<"))
            .any(|(at, _)| {
                let before = &parameters[..at];
                if before
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    return false;
                }
                let mut before = before.trim_end();
                before = before
                    .strip_suffix("oxc_ast::ast::")
                    .unwrap_or(before)
                    .trim_end();
                before = before.strip_suffix("mut").unwrap_or(before).trim_end();
                if let Some(tick) = before.rfind('\'') {
                    if before[tick + 1..]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        before = before[..tick].trim_end();
                    }
                }
                before.ends_with('&')
            })
    })
}

/// Whether `body` calls the function `name`: `name(`, `self.name(` or
/// `Self::name(`, not a method of that name on another value.
fn calls_itself(body: &str, name: &str) -> bool {
    body.match_indices(name).any(|(at, _)| {
        let after = body[at + name.len()..].trim_start();
        if !after.starts_with('(') {
            return false;
        }
        let before = &body[..at];
        if before.ends_with("self.") || before.ends_with("Self::") {
            return true;
        }
        !before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':')
    })
}

/// Every function in `code` that takes oxc syntax by reference and calls
/// itself: its name.
fn self_recursions_over_oxc_syntax(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices("fn ") {
        if code[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        let rest = &code[at + 3..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let Some(open) = rest.find('(') else {
            continue;
        };
        let mut depth = 0usize;
        let mut close = None;
        for (offset, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            continue;
        };
        if !takes_oxc_syntax(&rest[open..close]) {
            continue;
        }
        let Some(body_open) = rest[close..].find(['{', ';']).map(|offset| close + offset) else {
            continue;
        };
        if rest.as_bytes()[body_open] == b';' {
            continue;
        }
        let mut depth = 0usize;
        let mut body_end = rest.len();
        for (offset, c) in rest[body_open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = body_open + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        if calls_itself(&rest[body_open + 1..body_end], &name) {
            found.push(name);
        }
    }
    found
}

/// Verter's own functions that recurse over oxc syntax spend native stack
/// once per level of it, as oxc's walks do, but no containment guard sees
/// them: they belong on explicit work stacks. The census in
/// `hand_written_recursions.txt` lists the ones that do (a function over
/// oxc syntax that calls itself directly; mutual recursion is not found);
/// a new one fails here. Move it to an explicit stack, or, when its depth
/// is bounded by something other than the source's nesting, list it with
/// that bound. An entry that no longer recurses is removed from the list.
#[test]
fn hand_written_recursions_over_oxc_syntax_do_not_grow() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut live = std::collections::BTreeSet::new();
    let mut stack = Vec::new();
    for entry in std::fs::read_dir(crates)
        .expect("read the crates directory")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "verter_bench" {
            continue;
        }
        stack.push(entry.path().join("src"));
    }
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" && !name.ends_with("_tests") {
                    stack.push(path);
                }
                continue;
            }
            if !name.ends_with(".rs")
                || name == "tests.rs"
                || name.ends_with("_tests.rs")
                || name == "test_support.rs"
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read a source file");
            let mut code = String::from_utf8_lossy(&code_only(&source)).into_owned();
            if let Some(end) = code.find("#[cfg(test)]\nmod ") {
                code.truncate(end);
            }
            let relative = path
                .strip_prefix(crates)
                .expect("under the crates directory")
                .to_string_lossy()
                .replace('\\', "/");
            for function in self_recursions_over_oxc_syntax(&code) {
                live.insert(format!("{relative} {function}"));
            }
        }
    }
    let recorded: std::collections::BTreeSet<String> = include_str!("hand_written_recursions.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split(" #").next().unwrap_or(line).trim().to_string())
        .collect();
    let new: Vec<_> = live.difference(&recorded).collect();
    let gone: Vec<_> = recorded.difference(&live).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "new recursions over oxc syntax (walk them from an explicit stack):\n{}\n\
         listed recursions that no longer recurse (remove them from \
         hand_written_recursions.txt):\n{}",
        new.iter()
            .map(|entry| entry.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        gone.iter()
            .map(|entry| entry.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
