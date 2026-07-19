use super::decl_dependencies::{
    collect_statement_dependency_names, collect_type_dependency_facts, DeclarationPath,
};
use oxc_allocator::Allocator;
use oxc_ast::ast::{Statement, TSType};
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::{DeclKey, TopLevelOwnerId};

fn parse(source: &str) -> oxc_parser::ParserReturn<'_> {
    let allocator = Box::leak(Box::new(Allocator::default()));
    Parser::new(allocator, source, SourceType::ts()).parse()
}

#[test]
fn demanded_statement_dependencies_keep_structural_value_and_carrier_rails_distinct() {
    let source = r#"
export interface Props<T extends Bound = Default> extends NS.Base<Arg> {
  direct: Direct
  query: typeof runtime.member
  mapped: { [K in keyof Source as Rename<K>]: Value<K> }
  method(input: Input): Output
}
"#;
    let parsed = parse(source);
    assert!(!parsed.panicked, "fixture must parse");
    let owner = TopLevelOwnerId::module(7);
    let rows = collect_statement_dependency_names(&parsed.program.body[0], owner);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].0,
        DeclarationPath::root(DeclKey::new(owner, "Props"))
    );
    let facts = &rows[0].1;

    for expected in [
        "Bound",
        "Default",
        "NS.Base",
        "Arg",
        "Direct",
        "runtime.member",
        "Source",
        "Rename",
        "Value",
        "Input",
        "Output",
    ] {
        assert!(
            facts
                .declaration_carrier_paths
                .iter()
                .any(|path| path.legacy_dotted_name() == expected),
            "missing complete carrier dependency {expected}: {facts:?}"
        );
    }
    assert!(facts
        .value_query_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "runtime.member"));
    assert!(facts
        .structural_dependency_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "Arg"));
    assert!(!facts
        .structural_dependency_paths
        .iter()
        .any(|path| path.legacy_dotted_name() == "NS.Base"));
}

#[test]
fn statement_dependency_rows_preserve_namespace_and_default_alias_identity() {
    let source = r#"
namespace Outer { export namespace Inner { export type Item = Dep } }
export default interface Named extends Base { own: Own }
"#;
    let parsed = parse(source);
    let owner = TopLevelOwnerId::instance(3);
    let rows = parsed
        .program
        .body
        .iter()
        .flat_map(|statement| collect_statement_dependency_names(statement, owner))
        .collect::<Vec<_>>();
    let keys = rows
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();

    assert!(keys.contains(&DeclarationPath::new(
        DeclKey::new(owner, "Outer"),
        ["Inner", "Item"]
    )));
    assert!(keys.contains(&DeclarationPath::root(DeclKey::new(owner, "Named"))));
    assert!(keys.contains(&DeclarationPath::root(DeclKey::new(owner, "default"))));
}

#[test]
fn standalone_type_dependency_fact_collector_is_syntax_only() {
    let parsed = parse("type Subject = Pick<NS.Model, keyof Keys>");
    let Statement::TSTypeAliasDeclaration(alias) = &parsed.program.body[0] else {
        panic!("type alias");
    };
    let ty: &TSType<'_> = &alias.type_annotation;
    let facts = collect_type_dependency_facts(ty);
    let paths = facts
        .declaration_carrier_paths
        .iter()
        .map(|path| path.legacy_dotted_name())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"Pick".to_string()));
    assert!(paths.contains(&"NS.Model".to_string()));
    assert!(paths.contains(&"Keys".to_string()));
}
