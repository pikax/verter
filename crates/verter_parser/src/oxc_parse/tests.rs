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
            (parsed.program.body.len(), parsed.errors.len())
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the parse returns")
}

/// Every way syntax nests, 10,000 levels deep, parses on a 1 MiB thread:
/// the parse runs on a stack its source's length cannot exhaust. oxc's own
/// parser overflows a 1 MiB stack from 274 (`Box<…>`, unoptimized) to
/// 3,712 (`!`, optimized) levels
/// (`docs/evidence/signature-kernel/oxc-deep-parse.md`).
#[test]
fn every_form_nested_10000_deep_parses_on_a_small_stack() {
    for (form, source) in forms_nested(10_000) {
        assert_eq!(parse_on_a_small_stack(source), (1, 0), "{form}");
    }
}

/// Every way syntax nests, `depth` levels deep: each form's name and a
/// one-statement source.
fn forms_nested(depth: usize) -> [(&'static str, String); 10] {
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
            !parsed.panicked && parsed.errors.is_empty()
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
const CONTAINMENTS: [&str; 6] = [
    "with_program_stack",
    "with_node_stack",
    "with_ast_stack",
    "with_source_stack",
    "with_span_stack",
    "with_nesting_stack",
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
