use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_parser::utils::oxc::script::route_inventory::{
    RouteCapability, RouteImportForm, RouteImportedName, ScriptImportRoute, ScriptLocalExportRoute,
};
use verter_type_expr::{DeclBindingKey, TopLevelOwnerId};

use super::script_shallow_index::{
    build_script_shallow_index, build_script_shallow_index_with_owners, ScriptShallowIndex,
};
use super::TopLevelOwnerTable;

#[test]
fn shallow_script_publish_contains_only_headers_and_routes_from_one_program() {
    let source = r#"
import type { Input as LocalInput } from './types'
export interface Props { value: LocalInput }
export { Props as PublicProps }
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse");

    let ScriptShallowIndex {
        declaration_headers,
        routes,
    } = build_script_shallow_index(&parsed.program, source);
    let owner = TopLevelOwnerId::ordinary_file();

    assert!(declaration_headers
        .type_headers
        .contains_key(&DeclBindingKey::new(owner, "Props")));
    assert_eq!(
        routes.imports,
        [ScriptImportRoute {
            owner,
            local: "LocalInput".into(),
            source: "./types".into(),
            form: RouteImportForm::Named,
            capability: RouteCapability::TypeOnly,
            imported: RouteImportedName::Name("Input".into()),
        }]
    );
    assert_eq!(
        routes.local_exports,
        [
            ScriptLocalExportRoute {
                owner,
                exported: "Props".into(),
                local: "Props".into(),
                capability: RouteCapability::TypeOnly,
            },
            ScriptLocalExportRoute {
                owner,
                exported: "PublicProps".into(),
                local: "Props".into(),
                capability: RouteCapability::TypeAndValue,
            },
        ]
    );
}

#[test]
fn shallow_script_publish_uses_one_exact_owner_table_for_both_indexes() {
    let source = r#"
import { Input } from './types'
export interface Props { value: Input }
export { Props as PublicProps }
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse");
    let module = TopLevelOwnerId::module(2);
    let instance = TopLevelOwnerId::instance(4);
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, module, instance],
    )
    .expect("owner table");

    let index = build_script_shallow_index_with_owners(&parsed.program, source, &owners)
        .expect("owner table exactly covers Program.body");

    assert!(index
        .declaration_headers
        .type_headers
        .contains_key(&DeclBindingKey::new(module, "Props")));
    assert_eq!(index.routes.imports[0].owner, module);
    assert_eq!(index.routes.local_exports[0].owner, module);
    assert_eq!(index.routes.local_exports[1].owner, instance);

    let incomplete = TopLevelOwnerTable::ordinary_file(parsed.program.body.len() - 1);
    let error = build_script_shallow_index_with_owners(&parsed.program, source, &incomplete)
        .expect_err("incomplete owner table must fail before publication");
    assert_eq!(error.statement_count(), parsed.program.body.len());
    assert_eq!(error.owner_count(), incomplete.len());
}
