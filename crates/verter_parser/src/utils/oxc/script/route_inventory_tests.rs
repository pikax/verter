use super::route_inventory::{
    build_script_route_inventory, build_script_route_inventory_with_owners, RouteCapability,
    RouteImportForm, RouteImportedName, ScriptExportAssignmentRoute, ScriptImportRoute,
    ScriptLocalExportRoute, ScriptReexportRoute, ScriptRouteCounts, ScriptSideEffectImport,
    ScriptWildcardRoute,
};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::TopLevelOwnerId;

fn parse(source: &str) -> oxc_parser::ParserReturn<'_> {
    let allocator = Box::leak(Box::new(Allocator::default()));
    Parser::new(allocator, source, SourceType::ts()).parse()
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
