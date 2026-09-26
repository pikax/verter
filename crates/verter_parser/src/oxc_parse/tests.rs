use oxc_allocator::Allocator;
use oxc_span::SourceType;

use super::{nesting, syntax_nesting, Parser, SYNTAX_NESTING_LIMIT};

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
fn realistic_sources_nest_far_below_the_limit() {
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

#[test]
fn a_source_past_the_limit_is_refused_as_a_syntax_error() {
    let allocator = Allocator::default();
    let deep = format!(
        "export const v = {}1{};",
        "(".repeat(10_000),
        ")".repeat(10_000)
    );
    let refused = Parser::new(&allocator, &deep, SourceType::ts()).parse();
    assert!(refused.panicked);
    assert!(refused.program.body.is_empty());
    assert_eq!(refused.program.source_text, deep);
    assert_eq!(refused.errors.len(), 1);
    assert!(refused.errors[0].to_string().contains("nests deeper than"));
    assert!(
        Parser::new(&allocator, &deep[17..deep.len() - 1], SourceType::ts())
            .parse_expression()
            .is_err()
    );
    // `=` is one link and each parenthesis one level.
    let at_limit = format!(
        "export const v = {}1{};",
        "(".repeat(SYNTAX_NESTING_LIMIT as usize - 1),
        ")".repeat(SYNTAX_NESTING_LIMIT as usize - 1)
    );
    assert_eq!(
        syntax_nesting(&at_limit, SourceType::ts()).map(|nesting| nesting.depth),
        Ok(SYNTAX_NESTING_LIMIT)
    );
    let parsed = Parser::new(&allocator, &at_limit, SourceType::ts()).parse();
    assert!(!parsed.panicked && parsed.errors.is_empty());
}

/// Every parse in the workspace's crates goes through [`Parser`]: a direct
/// `oxc_parser::Parser` would parse a source past the limit and overflow.
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
