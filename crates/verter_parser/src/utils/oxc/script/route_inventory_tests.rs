use super::route_inventory::{
    build_script_route_inventory, build_script_route_inventory_with_owners, RouteCapability,
    RouteImportForm, RouteImportedName, ScriptExportAssignmentRoute, ScriptImportRoute,
    ScriptLocalExportRoute, ScriptReexportRoute, ScriptRouteCounts, ScriptSideEffectImport,
    ScriptWildcardRoute,
};
use crate::oxc_parse::Parser;
use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_type_expr::TopLevelOwnerId;

fn parse(source: &str) -> oxc_parser::ParserReturn<'_> {
    let allocator = Box::leak(Box::new(Allocator::default()));
    Parser::new(allocator, source, SourceType::ts()).parse()
}

#[test]
fn module_identity_comes_from_top_level_syntax_even_without_route_rows() {
    for (source, expected) in [
        ("export {};", true),
        ("export = 42;", true),
        ("import x = require('x');", true),
        ("import X = N.X;", false),
        ("export import X = N.X;", true),
        ("namespace N { export interface X {} }", false),
        ("import('x');", false),
        ("if (ok) /; export {}/.test(x);", false),
        ("if (ok) /; export {}/.test(x); export {};", true),
    ] {
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "{source}: {:?}", parsed.errors);
        assert_eq!(
            build_script_route_inventory(&parsed.program).has_module_syntax,
            expected,
            "{source}"
        );
    }
}

#[test]
fn script_route_inventory_captures_only_closed_route_facts() {
    let source = r#"
import type DefaultType from './default'
import { Foo as LocalFoo, type Bar } from './named'
import * as NS from './namespace'
import './side-effect'
import {} from './empty'
export { LocalFoo as PublicFoo, type Bar as PublicBar }
export { Source as Alias, type TypeSource } from './direct'
export * from './star'
export type * from './types'
export * as Bag from './bag'
export interface Shape { value: string }
export class Klass {}
export const value = 1
export default value
export = NS
type Hidden = { ignored: true }
"#;
    let parsed = parse(source);
    assert!(!parsed.panicked, "fixture must parse");
    let inventory = build_script_route_inventory(&parsed.program);
    let owner = TopLevelOwnerId::ordinary_file();

    assert_eq!(
        inventory.counts,
        ScriptRouteCounts {
            top_level_statement_count: parsed.program.body.len(),
            import_binding_count: 4,
            bindingless_import_count: 2,
            direct_reexport_count: 2,
            wildcard_reexport_count: 3,
            local_export_count: 6,
            export_assignment_count: 1,
        }
    );
    assert_eq!(
        inventory.imports,
        [
            ScriptImportRoute {
                owner,
                local: "DefaultType".into(),
                source: "./default".into(),
                form: RouteImportForm::Default,
                capability: RouteCapability::TypeOnly,
                imported: RouteImportedName::Name("default".into()),
            },
            ScriptImportRoute {
                owner,
                local: "LocalFoo".into(),
                source: "./named".into(),
                form: RouteImportForm::Named,
                capability: RouteCapability::TypeAndValue,
                imported: RouteImportedName::Name("Foo".into()),
            },
            ScriptImportRoute {
                owner,
                local: "Bar".into(),
                source: "./named".into(),
                form: RouteImportForm::Named,
                capability: RouteCapability::TypeOnly,
                imported: RouteImportedName::Name("Bar".into()),
            },
            ScriptImportRoute {
                owner,
                local: "NS".into(),
                source: "./namespace".into(),
                form: RouteImportForm::Namespace,
                capability: RouteCapability::TypeAndValue,
                imported: RouteImportedName::Namespace,
            },
        ]
    );
    assert_eq!(
        inventory.bindingless_imports,
        [
            ScriptSideEffectImport {
                owner,
                source: "./side-effect".into(),
            },
            ScriptSideEffectImport {
                owner,
                source: "./empty".into(),
            },
        ]
    );
    assert_eq!(
        inventory.reexports,
        [
            ScriptReexportRoute {
                owner,
                exported: "Alias".into(),
                source: "./direct".into(),
                imported: "Source".into(),
                capability: RouteCapability::TypeAndValue,
            },
            ScriptReexportRoute {
                owner,
                exported: "TypeSource".into(),
                source: "./direct".into(),
                imported: "TypeSource".into(),
                capability: RouteCapability::TypeOnly,
            },
        ]
    );
    assert_eq!(
        inventory.wildcard_reexports,
        [
            ScriptWildcardRoute {
                owner,
                source: "./star".into(),
                capability: RouteCapability::TypeAndValue,
                exported_namespace: None,
            },
            ScriptWildcardRoute {
                owner,
                source: "./types".into(),
                capability: RouteCapability::TypeOnly,
                exported_namespace: None,
            },
            ScriptWildcardRoute {
                owner,
                source: "./bag".into(),
                capability: RouteCapability::TypeAndValue,
                exported_namespace: Some("Bag".into()),
            },
        ]
    );
    assert_eq!(
        inventory.local_exports,
        [
            ScriptLocalExportRoute {
                owner,
                exported: "PublicFoo".into(),
                local: "LocalFoo".into(),
                capability: RouteCapability::TypeAndValue,
            },
            ScriptLocalExportRoute {
                owner,
                exported: "PublicBar".into(),
                local: "Bar".into(),
                capability: RouteCapability::TypeOnly,
            },
            ScriptLocalExportRoute {
                owner,
                exported: "Shape".into(),
                local: "Shape".into(),
                capability: RouteCapability::TypeOnly,
            },
            ScriptLocalExportRoute {
                owner,
                exported: "Klass".into(),
                local: "Klass".into(),
                capability: RouteCapability::TypeAndValue,
            },
            ScriptLocalExportRoute {
                owner,
                exported: "value".into(),
                local: "value".into(),
                capability: RouteCapability::ValueOnly,
            },
            ScriptLocalExportRoute {
                owner,
                exported: "default".into(),
                local: "value".into(),
                capability: RouteCapability::ValueOnly,
            },
        ]
    );
    assert_eq!(
        inventory.export_assignments,
        [ScriptExportAssignmentRoute {
            owner,
            local: "NS".into(),
        }]
    );
}

#[test]
fn script_route_inventory_is_body_independent_and_owner_exact() {
    let before = parse(
        "export interface Shared { before: string }\n\
         export { Shared as Public }\n",
    );
    let after = parse(
        "export interface Shared<T> extends Base<T> { after: T; nested: { x: number } }\n\
         export { Shared as Public }\n",
    );
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let owners = [module, instance];
    let before_inventory =
        build_script_route_inventory_with_owners(&before.program, &owners).expect("owner table");
    let after_inventory =
        build_script_route_inventory_with_owners(&after.program, &owners).expect("owner table");

    assert_eq!(before_inventory, after_inventory);
    assert_eq!(before_inventory.local_exports[0].owner, module);
    assert_eq!(before_inventory.local_exports[1].owner, instance);

    let invalid = build_script_route_inventory_with_owners(&after.program, &[module])
        .expect_err("incomplete owner table must fail");
    assert_eq!(invalid.statement_count(), 2);
    assert_eq!(invalid.owner_count(), 1);
}

#[test]
fn default_export_surfaces_route_through_the_default_symbol() {
    let owner = TopLevelOwnerId::ordinary_file();
    for (source, capability) in [
        (
            "export default class Props { label!: string }",
            RouteCapability::TypeAndValue,
        ),
        (
            "export default interface Props { label: string }",
            RouteCapability::TypeOnly,
        ),
        (
            "export default { label: 'ok' } as const",
            RouteCapability::ValueOnly,
        ),
    ] {
        let parsed = parse(source);
        assert!(!parsed.panicked, "fixture must parse: {source}");

        let inventory = build_script_route_inventory(&parsed.program);
        assert_eq!(
            inventory.local_exports,
            [ScriptLocalExportRoute {
                owner,
                exported: "default".into(),
                local: "default".into(),
                capability,
            }],
            "the route must target the canonical default header: {source}",
        );
    }
}

fn parse_as(source: &str, source_type: SourceType) -> oxc_parser::ParserReturn<'_> {
    let allocator = Box::leak(Box::new(Allocator::default()));
    Parser::new(allocator, source, source_type).parse()
}

fn exported_names(source: &str, source_type: SourceType) -> Vec<String> {
    let parsed = parse_as(source, source_type);
    assert!(parsed.errors.is_empty(), "{source}: {:?}", parsed.errors);
    let mut names: Vec<String> = build_script_route_inventory(&parsed.program)
        .local_exports
        .into_iter()
        .map(|route| route.exported)
        .collect();
    names.sort();
    names
}

#[test]
fn a_declaration_file_module_without_export_declarations_exports_every_declaration() {
    let source = "export function top(): 1;\ndeclare function hidden(): 2;\n\
                  declare namespace N { const c: 3; }\ninterface I {}\ndeclare global { var g: 4; }\n";
    assert_eq!(
        exported_names(source, SourceType::d_ts()),
        ["I", "N", "hidden", "top"]
    );
    // An export declaration ends the implicit exports.
    assert_eq!(
        exported_names(&format!("{source}export {{}};\n"), SourceType::d_ts()),
        ["top"]
    );
    // Outside a declaration file only `export` exports.
    assert_eq!(exported_names(source, SourceType::ts()), ["top"]);
    // A declaration file without module syntax is a global script.
    assert!(exported_names("declare function hidden(): 2;\n", SourceType::d_ts()).is_empty());
}

#[test]
fn an_import_assignment_is_an_import_route() {
    let parsed =
        parse("import N = require('./n');\nimport type T = require('./t');\nimport Q = N.Q;\n");
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let imports = build_script_route_inventory(&parsed.program).imports;
    assert_eq!(
        imports
            .iter()
            .map(|route| (
                route.local.as_str(),
                route.source.as_str(),
                route.form,
                route.capability
            ))
            .collect::<Vec<_>>(),
        [
            (
                "N",
                "./n",
                RouteImportForm::ImportEquals,
                RouteCapability::TypeAndValue
            ),
            (
                "T",
                "./t",
                RouteImportForm::ImportEquals,
                RouteCapability::TypeOnly
            ),
        ]
    );
}

#[test]
fn export_declarations_are_not_export_modifiers() {
    use super::route_inventory::statements_have_export_declarations;
    for (source, expected) in [
        ("export {};", true),
        ("export { a };", true),
        ("export * from './a';", true),
        ("export = a;", true),
        ("export default 1;", true),
        ("export const a = 1;", false),
        ("export function f() {}", false),
        ("export default function f() {}", false),
        ("declare const a: 1;", false),
    ] {
        let parsed = parse(source);
        assert_eq!(
            statements_have_export_declarations(&parsed.program.body),
            expected,
            "{source}"
        );
    }
}
