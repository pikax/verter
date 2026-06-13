//! Header-walk ↔ env-walk parity: the shallow declaration-header index
//! must inventory exactly the names the whole-env walk registers, per
//! space and scope, with matching kinds / params / member headers — while
//! lowering zero bodies (it has no lowering entry to call at all; parity
//! is what makes the index a sound addressing authority for the lazy
//! body service).

use super::*;
use crate::analysis::type_eval_build::{build_eval_env, parse_and_build_env};

fn index_for(source: &str) -> DeclHeaderIndex {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.panicked, "fixture must parse");
    build_decl_header_index(&ret.program, source)
}

/// Build both walks over one source and assert the NAME sets agree in
/// every table (the core addressing-parity invariant).
fn assert_name_parity(source: &str) {
    let env = parse_and_build_env(source);
    let index = index_for(source);

    let mut env_types: Vec<&str> = env.type_symbols.keys().map(String::as_str).collect();
    let mut index_types: Vec<&str> = index.type_headers.keys().map(String::as_str).collect();
    env_types.sort_unstable();
    index_types.sort_unstable();
    assert_eq!(
        env_types, index_types,
        "type-symbol names must match the env walk for source:\n{source}"
    );

    let mut env_values: Vec<&str> = env.value_symbols.keys().map(String::as_str).collect();
    let mut index_values: Vec<&str> = index.value_headers.keys().map(String::as_str).collect();
    env_values.sort_unstable();
    index_values.sort_unstable();
    assert_eq!(
        env_values, index_values,
        "value-symbol names must match the env walk for source:\n{source}"
    );

    let mut env_aug_types: Vec<(String, &str)> = env
        .augmentation_scopes
        .keys()
        .map(|(scope, name)| (format!("{scope:?}"), name.as_str()))
        .collect();
    let mut index_aug_types: Vec<(String, &str)> = index
        .augmentation_type_headers
        .iter()
        .flat_map(|(scope, names)| {
            names
                .keys()
                .map(move |name| (format!("{scope:?}"), name.as_str()))
        })
        .collect();
    env_aug_types.sort();
    index_aug_types.sort();
    assert_eq!(
        env_aug_types, index_aug_types,
        "augmentation TYPE names must match the env walk for source:\n{source}"
    );

    let mut env_aug_values: Vec<(String, &str)> = env
        .augmentation_value_scopes
        .keys()
        .map(|(scope, name)| (format!("{scope:?}"), name.as_str()))
        .collect();
    let mut index_aug_values: Vec<(String, &str)> = index
        .augmentation_value_headers
        .iter()
        .flat_map(|(scope, names)| {
            names
                .keys()
                .map(move |name| (format!("{scope:?}"), name.as_str()))
        })
        .collect();
    env_aug_values.sort();
    index_aug_values.sort();
    assert_eq!(
        env_aug_values, index_aug_values,
        "augmentation VALUE names must match the env walk for source:\n{source}"
    );
}

#[test]
fn file_scope_name_parity_across_declaration_forms() {
    assert_name_parity(
        r#"
type Alias = { a: number };
export type ExportedAlias = string;
interface Iface { x: string }
export interface ExportedIface { y?: Alias }
class Klass { a: number; m(): void {} static s: string; private p: number }
export class ExportedKlass {}
function fnDecl(a: number): string { return "x"; }
async function asyncFn() {}
const constVal = { k1: 1, k2: "two" };
let letVal: number = 1;
var varA = 1, varB = 2;
enum ColorEnum { Red, Green }
"#,
    );
}

#[test]
fn merged_interface_contributors_recorded_in_source_order() {
    let source = r#"
interface Merged { a: string }
type Unrelated = number;
interface Merged { b: number }
"#;
    assert_name_parity(source);
    let index = index_for(source);
    let header = index.type_header("Merged").expect("Merged header");
    assert_eq!(
        header.contributors,
        vec![0, 2],
        "both contributing statements must be recorded in source order"
    );
    let member_names: Vec<&str> = header
        .member_headers
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        member_names,
        vec!["a", "b"],
        "member headers must union across contributors in first-seen order"
    );
    assert_eq!(
        index
            .type_header("Unrelated")
            .expect("Unrelated")
            .contributors,
        vec![1]
    );
}

#[test]
fn namespace_qualified_names_match_env_walk() {
    assert_name_parity(
        r#"
namespace Outer {
    export type Inner = { v: 1 };
    export namespace Nested {
        export interface Deep { d: string }
    }
}
"#,
    );
}

#[test]
fn default_export_forms_match_env_walk() {
    assert_name_parity("export default class Props { a: number }\n");
    assert_name_parity("export default interface Shape { s: string }\n");
    assert_name_parity("export default { plain: 1 };\n");
    assert_name_parity("export default function named() { return 1; }\n");
}

#[test]
fn augmentation_scopes_match_env_walk() {
    assert_name_parity(
        r#"
declare module "vue" {
    interface ComponentCustomProperties { $x: string }
    const injected: number;
    function helper(): void;
    class Widget { w: number }
}
declare global {
    interface Window { pageData: unknown }
}
type FileScope = { f: 1 };
"#,
    );
}

#[test]
fn jsdoc_typedef_names_obey_ts_decl_precedence() {
    let source = r#"
/** @typedef {{a: number}} OnlyTypedef */
/** @typedef {string} Shadowed */
type Shadowed = { real: true };
"#;
    assert_name_parity(source);
    let index = index_for(source);
    assert!(
        index
            .type_header("OnlyTypedef")
            .expect("typedef header")
            .from_jsdoc_typedef,
        "a name declared only via @typedef is marked as such"
    );
    assert!(
        !index
            .type_header("Shadowed")
            .expect("Shadowed")
            .from_jsdoc_typedef,
        "a TS declaration claims the name; the typedef is shadowed"
    );
}

#[test]
fn type_param_headers_match_lowered_group_params() {
    let source = "interface G<T extends string, U = number> { a: T }\n";
    let env = parse_and_build_env(source);
    let index = index_for(source);
    let env_params: Vec<String> = env.type_symbols["G"]
        .primary()
        .type_parameters
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let header_params: Vec<String> = index
        .type_header("G")
        .expect("G")
        .type_params
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(env_params, header_params);
    let g = index.type_header("G").unwrap();
    assert!(
        g.type_params[0].constraint_span.is_some(),
        "T's constraint locator must be recorded"
    );
    assert!(
        g.type_params[1].default_span.is_some(),
        "U's default locator must be recorded"
    );
    assert!(g.type_params[0].default_span.is_none());
}

#[test]
fn member_headers_match_lowered_lookup_object_names() {
    // Alias bodies: literal members, intersection descent; mapped /
    // utility bodies contribute NO syntactic members — same as the
    // lowered body's direct-object projection.
    for (source, name) in [
        ("type T = { a: 1; b?: string; readonly c: boolean };\n", "T"),
        ("type T = { a: 1 } & ({ b: 2 });\n", "T"),
        (
            "type T = Pick<Other, 'x'>;\ninterface Other { x: 1 }\n",
            "T",
        ),
        ("interface I { p: string; m(): void }\n", "I"),
    ] {
        let env = parse_and_build_env(source);
        let index = index_for(source);
        let env_members = env.type_symbols[name].merged_body().merged_member_names();
        let header_members: Vec<String> = index
            .type_header(name)
            .unwrap_or_else(|| panic!("{name} header"))
            .member_headers
            .iter()
            .map(|m| m.name.clone())
            .collect();
        assert_eq!(
            env_members, header_members,
            "member-header names must match the lowered merged_member_names for:\n{source}"
        );
    }
}

#[test]
fn enum_headers_live_in_their_own_table_not_value_headers() {
    let source = "enum E { A, B }\nexport enum F { C = 'c' }\n";
    let index = index_for(source);
    assert!(
        index.value_headers.is_empty(),
        "env walk registers no enum value symbols"
    );
    assert_eq!(index.enum_headers["E"].member_names, vec!["A", "B"]);
    assert_eq!(index.enum_headers["F"].member_names, vec!["C"]);
}

#[test]
fn merged_enum_unions_member_names_across_declarations() {
    // TS enum declaration merging: a later same-name `enum` CONTRIBUTES
    // its members to the merged surface. The shallow header must UNION
    // every declaration's members in source order across BOTH arms — the
    // top-level `Statement::TSEnumDeclaration` arm (`enum E`) and the
    // exported `Declaration::TSEnumDeclaration` arm (`export enum F`).
    // Dropping a later declaration's members under-states the surface and
    // under-invalidates a warm consumer.
    let source = "enum E { A }\nenum E { B }\nexport enum F { X }\nexport enum F { Y }\n";
    let index = index_for(source);
    assert_eq!(
        index.enum_headers["E"].member_names,
        vec!["A", "B"],
        "merged enum E must union both declarations' members in source order"
    );
    assert_eq!(
        index.enum_headers["F"].member_names,
        vec!["X", "Y"],
        "merged exported enum F must union both declarations' members in source order"
    );
    // Every contributing statement is recorded in source order.
    assert_eq!(index.enum_headers["E"].contributors, vec![0, 1]);
    assert_eq!(index.enum_headers["F"].contributors, vec![2, 3]);
}

#[test]
fn merged_enum_dedups_repeated_member_name_defensively() {
    // A malformed double-decl that repeats a member name must not
    // double-count: the union keeps one occurrence in first-seen source
    // order.
    let source = "enum E { A, B }\nenum E { B, C }\n";
    let index = index_for(source);
    assert_eq!(
        index.enum_headers["E"].member_names,
        vec!["A", "B", "C"],
        "a repeated member name across declarations is deduped, order preserved"
    );
    assert_eq!(index.enum_headers["E"].contributors, vec![0, 1]);
}

#[test]
fn object_literal_value_headers_record_last_wins_keys() {
    let source = "const obj = { a: 1, b: 2, a: 3 } as const;\n";
    let index = index_for(source);
    let names: Vec<&str> = index.value_headers["obj"]
        .object_member_headers
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["b", "a"],
        "duplicate keys keep the LAST occurrence's position"
    );
}

#[test]
fn selective_statement_lowering_matches_whole_env_group() {
    // Lower ONLY the demanded symbol's contributing statements through
    // the shared statement arm and compare the produced group against
    // the whole-env walk's group — the core selective-lowering parity.
    use crate::analysis::type_eval::EvalEnv;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source = r#"
interface Merged { a: string }
type Unrelated = { u: 1 };
interface Merged { b: number }
"#;
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let index = build_decl_header_index(&ret.program, source);
    let whole = build_eval_env(&ret.program, source);

    let mut scratch = EvalEnv::new();
    for stmt_index in &index.type_header("Merged").expect("Merged").contributors {
        crate::analysis::type_eval_build::lower_top_level_statement(
            &ret.program.body[*stmt_index as usize],
            source,
            &mut scratch,
        );
    }
    let selective_group = &scratch.type_symbols["Merged"];
    let whole_group = &whole.type_symbols["Merged"];
    assert_eq!(
        selective_group.contributors().len(),
        whole_group.contributors().len()
    );
    assert!(selective_group.merged_body().is_merged());
    assert_eq!(
        selective_group.merged_body().merged_member_names(),
        whole_group.merged_body().merged_member_names()
    );
    // NEGATIVE: the un-demanded sibling was never lowered.
    assert!(
        !scratch.type_symbols.contains_key("Unrelated"),
        "selective lowering must not lower the un-demanded statement"
    );
    assert_eq!(scratch.total_decl_count(), 2);
}
