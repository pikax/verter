use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{
    collect_class_dependency_facts, collect_interface_dependency_facts,
    collect_type_dependency_paths, UnsupportedValuePositionKind,
};

#[test]
fn type_path_collection_preserves_qualified_segments() {
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        "type Subject = Pick<NS.Model, keyof Keys>",
        SourceType::ts(),
    )
    .parse();
    assert!(!parsed.fatal_error, "fixture must parse");
    let Statement::TSTypeAliasDeclaration(alias) = &parsed.program.body[0] else {
        panic!("type alias");
    };

    let paths = collect_type_dependency_paths(&alias.type_annotation)
        .into_iter()
        .map(|path| path.legacy_dotted_name())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["Keys", "NS.Model", "Pick"]);
}

#[test]
fn interface_collection_keeps_dependency_roles_distinct() {
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        r#"
interface Subject<T extends Bound = Default> extends NS.Base<Arg> {
  direct: Direct
  query: typeof runtime.member
  method(input: Input): Output
}
"#,
        SourceType::ts(),
    )
    .parse();
    assert!(!parsed.fatal_error, "fixture must parse");
    let Statement::TSInterfaceDeclaration(interface) = &parsed.program.body[0] else {
        panic!("interface");
    };

    let facts = collect_interface_dependency_facts(interface);
    assert!(facts
        .declaration_carrier_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "NS.Base"));
    assert!(!facts
        .structural_dependency_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "NS.Base"));
    for structural in ["Arg", "Input"] {
        assert!(facts
            .structural_dependency_paths
            .iter()
            .any(|path| path.legacy_dotted_name() == structural));
    }
    assert!(facts
        .value_query_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "runtime.member"));
    assert!(facts.value_position_paths.is_empty());
    assert!(facts.unsupported_value_positions.is_empty());
}

#[test]
fn class_collection_publishes_statically_addressable_initializer_value_carriers() {
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        r#"
class Payload {
  ctor = Base
  kind = Namespace.Kind
  literal = 1
  created = make()
}
"#,
        SourceType::ts(),
    )
    .parse();
    assert!(!parsed.fatal_error, "fixture must parse");
    let Statement::ClassDeclaration(class) = &parsed.program.body[0] else {
        panic!("class");
    };

    let facts = collect_class_dependency_facts(class);
    let expected = ["Base", "Namespace.Kind"]
        .into_iter()
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        facts
            .value_position_paths
            .iter()
            .map(|path| path.legacy_dotted_name())
            .collect::<std::collections::BTreeSet<_>>(),
        expected,
    );
    assert_eq!(
        facts
            .declaration_carrier_paths
            .iter()
            .map(|path| path.legacy_dotted_name())
            .collect::<std::collections::BTreeSet<_>>(),
        expected,
    );
    assert!(facts.dependency_paths.is_empty());
    assert!(facts.structural_dependency_paths.is_empty());
    assert!(facts.unsupported_value_positions.is_empty());
}

#[test]
fn class_collection_fails_closed_for_unaddressable_value_positions() {
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        "class Subject extends factory() { [dynamic()]: Own }",
        SourceType::ts(),
    )
    .parse();
    assert!(!parsed.fatal_error, "fixture must parse");
    let Statement::ClassDeclaration(class) = &parsed.program.body[0] else {
        panic!("class");
    };

    let facts = collect_class_dependency_facts(class);
    assert_eq!(
        facts.unsupported_value_positions,
        [
            UnsupportedValuePositionKind::ClassHeritageExpression,
            UnsupportedValuePositionKind::ComputedClassKey,
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn exported_declaration_ast_shape_remains_supported_by_wrapper_callers() {
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        "export interface Subject extends Base { own: Own }",
        SourceType::ts(),
    )
    .parse();
    assert!(!parsed.fatal_error, "fixture must parse");
    let Statement::ExportDeclaration(export) = &parsed.program.body[0] else {
        panic!("export");
    };
    let Declaration::TSInterfaceDeclaration(interface) = &export.declaration else {
        panic!("exported interface");
    };

    let facts = collect_interface_dependency_facts(interface);
    assert!(facts
        .structural_dependency_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "Base"));
}

/// The parsed program, handed to the thread that collects its facts.
struct Handed<'p, 'a>(&'p oxc_ast::ast::Program<'a>);

// SAFETY: the parsing thread blocks on the collecting thread's join while
// the collecting thread holds the program, so one thread touches it at a
// time.
unsafe impl Send for Handed<'_, '_> {}

/// Types nested 10,000 deep (type arguments, function types, object type
/// members and conditional branches) collect their dependency facts on a
/// 1 MiB thread: the collector visits nested types from an explicit stack.
#[test]
fn types_nested_10000_deep_collect_on_a_small_stack() {
    let depth = 10_000;
    let wrap =
        |open: &str, close: &str| format!("{}Leaf{}", open.repeat(depth), close.repeat(depth));
    let sources = [
        ("arguments", format!("type D = {};", wrap("Box<", ">"))),
        (
            "functions",
            format!("type D = {}Leaf;", "(x: Leaf) => ".repeat(depth)),
        ),
        ("objects", format!("type D = {};", wrap("{ v: ", " }"))),
        (
            "conditionals",
            format!(
                "type D = {}Leaf;",
                "Leaf extends Box ? Leaf : ".repeat(depth)
            ),
        ),
    ];
    for (form, source) in sources {
        let paths = std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(move || {
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
                assert!(!parsed.fatal_error, "fixture must parse");
                let handed = Handed(&parsed.program);
                std::thread::scope(|scope| {
                    std::thread::Builder::new()
                        .stack_size(1 << 20)
                        .spawn_scoped(scope, move || {
                            let handed = handed;
                            let Statement::TSTypeAliasDeclaration(alias) = &handed.0.body[0] else {
                                panic!("type alias");
                            };
                            collect_type_dependency_paths(&alias.type_annotation)
                                .into_iter()
                                .map(|path| path.legacy_dotted_name())
                                .collect::<Vec<_>>()
                        })
                        .expect("spawn the collecting thread")
                        .join()
                        .expect("the facts collect")
                })
            })
            .expect("spawn the parsing thread")
            .join()
            .expect("the parse returns");
        let expected: &[&str] = match form {
            "arguments" | "conditionals" => &["Box", "Leaf"],
            _ => &["Leaf"],
        };
        assert_eq!(paths, expected, "{form}");
    }
}
