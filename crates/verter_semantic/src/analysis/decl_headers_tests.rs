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

#[test]
fn owner_scoped_headers_do_not_merge_same_name_declarations() {
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_type_expr::{DeclKey, TopLevelOwnerId};

    let source = r#"
interface Shared { moduleA: string }
interface Shared { instance: number }
interface Shared { moduleB: boolean }
namespace Ns { export class C { value!: string } }
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse");
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, instance, module, instance],
    )
    .expect("validated owner table");

    let index = build_decl_header_index_with_owners(&parsed.program, source, &owners);
    let module_header = index
        .type_headers
        .get(&DeclKey::new(module, "Shared"))
        .expect("module Shared");
    let instance_header = index
        .type_headers
        .get(&DeclKey::new(instance, "Shared"))
        .expect("instance Shared");

    assert_eq!(module_header.contributors.len(), 2);
    assert_eq!(module_header.contributors[0].anchor.owner, module);
    assert_eq!(module_header.contributors[0].anchor.owner_local_ordinal, 0);
    assert_eq!(module_header.contributors[1].anchor.owner_local_ordinal, 1);
    assert_eq!(instance_header.contributors.len(), 1);
    assert_eq!(instance_header.contributors[0].anchor.owner, instance);
    assert_eq!(
        instance_header.contributors[0].anchor.owner_local_ordinal,
        0
    );

    let namespaced = DeclKey::new(instance, "Ns.C");
    assert!(index.type_headers.contains_key(&namespaced));
    assert!(index.value_headers.contains_key(&namespaced));
}

#[test]
fn ordinary_header_entry_point_uses_module_zero_owner() {
    use verter_type_expr::{DeclKey, TopLevelOwnerId};

    let index = index_for("interface Props { value: string }");
    let key = DeclKey::new(TopLevelOwnerId::ordinary_file(), "Props");
    let header = index.type_headers.get(&key).expect("ordinary Props");
    assert_eq!(header.contributors[0].anchor.owner, key.owner);
    assert_eq!(header.contributors[0].anchor.owner_local_ordinal, 0);
}

#[test]
fn default_export_aliases_are_scoped_by_lexical_owner() {
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_type_expr::{DeclKey, TopLevelOwnerId};

    let source = r#"
export default interface ModuleDefault { module: string }
export default interface InstanceDefault { instance: number }
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse");
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, instance],
    )
    .expect("validated owner table");

    let index = build_decl_header_index_with_owners(&parsed.program, source, &owners);
    let module_default = index
        .type_headers
        .get(&DeclKey::new(module, "default"))
        .expect("module default alias");
    let instance_default = index
        .type_headers
        .get(&DeclKey::new(instance, "default"))
        .expect("instance default alias");
    assert_eq!(module_default.contributors[0].anchor.owner, module);
    assert_eq!(instance_default.contributors[0].anchor.owner, instance);
    assert_ne!(module_default.name_span, instance_default.name_span);
}

#[test]
fn jsdoc_typedef_headers_use_attachment_or_explicit_region_owner() {
    use crate::analysis::top_level_owners::{TopLevelOwnerRegion, TopLevelOwnerTable};
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_span::Span;
    use verter_type_expr::{DeclKey, TopLevelOwnerId};

    let source = r#"
/** @typedef {string} Shared */
const moduleMarker = 0;
/** @typedef {number} Shared */
const instanceMarker = 0;
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, instance],
    )
    .expect("validated owner table");
    let index = build_decl_header_index_with_owners(&parsed.program, source, &owners);

    let module_header = index
        .type_headers
        .get(&DeclKey::new(module, "Shared"))
        .expect("module typedef");
    let instance_header = index
        .type_headers
        .get(&DeclKey::new(instance, "Shared"))
        .expect("instance typedef");
    let module_typedef = module_header.jsdoc_typedef.expect("module locator");
    let instance_typedef = instance_header.jsdoc_typedef.expect("instance locator");
    assert_eq!(module_typedef.owner_local_ordinal, Some(0));
    assert_eq!(instance_typedef.owner_local_ordinal, Some(0));
    assert_ne!(module_typedef.comment_span, instance_typedef.comment_span);

    let regional_source = "/** @typedef {boolean} Regional */";
    let regional_allocator = Allocator::default();
    let regional = Parser::new(&regional_allocator, regional_source, SourceType::ts()).parse();
    let regional_owners = TopLevelOwnerTable::try_from_statement_owners(
        regional.program.body.len(),
        std::iter::empty(),
    )
    .expect("empty statement table")
    .try_with_regions([TopLevelOwnerRegion {
        owner: instance,
        span: Span::new(0, regional_source.len() as u32),
    }])
    .expect("explicit owner region");
    let regional_index =
        build_decl_header_index_with_owners(&regional.program, regional_source, &regional_owners);
    assert!(regional_index
        .type_headers
        .contains_key(&DeclKey::new(instance, "Regional")));

    let unowned = TopLevelOwnerTable::try_from_statement_owners(1, [instance])
        .expect("single carrier owner is not an implicit region");
    let unowned_source = "const marker = 0;\n/** @typedef {boolean} Unowned */";
    let unowned_allocator = Allocator::default();
    let unowned_program = Parser::new(&unowned_allocator, unowned_source, SourceType::ts()).parse();
    let unowned_index =
        build_decl_header_index_with_owners(&unowned_program.program, unowned_source, &unowned);
    assert!(!unowned_index
        .type_headers
        .contains_key(&DeclKey::new(instance, "Unowned")));

    let ordinary = index_for("/** @typedef {string} Ordinary */");
    assert!(ordinary
        .type_headers
        .contains_key(&DeclKey::new(TopLevelOwnerId::ordinary_file(), "Ordinary")));
}

#[test]
fn augmentation_contributors_retain_lexical_owner() {
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    use crate::analysis::type_eval::AugmentationScopeKind;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_type_expr::{DeclKey, TopLevelOwnerId};

    let source = r#"
declare module "pkg" { interface Config { module: string } }
declare module "pkg" { interface Config { instance: number } }
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, instance],
    )
    .expect("validated owner table");
    let index = build_decl_header_index_with_owners(&parsed.program, source, &owners);
    let scoped = index
        .augmentation_type_headers
        .get(&AugmentationScopeKind::Module("pkg".to_string()))
        .expect("pkg augmentation");

    let module_header = scoped
        .get(&DeclKey::new(module, "Config"))
        .expect("module Config");
    let instance_header = scoped
        .get(&DeclKey::new(instance, "Config"))
        .expect("instance Config");
    assert_eq!(module_header.contributors.len(), 1);
    assert_eq!(instance_header.contributors.len(), 1);
    assert_eq!(module_header.contributors[0].anchor.owner, module);
    assert_eq!(instance_header.contributors[0].anchor.owner, instance);
}

/// Build both walks over one source and assert the NAME sets agree in
/// every table (the core addressing-parity invariant).
fn assert_name_parity(source: &str) {
    let env = parse_and_build_env(source);
    let index = index_for(source);

    let mut env_types: Vec<&str> = env
        .type_symbols
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    let mut index_types: Vec<&str> = index
        .type_headers
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    env_types.sort_unstable();
    index_types.sort_unstable();
    assert_eq!(
        env_types, index_types,
        "type-symbol names must match the env walk for source:\n{source}"
    );

    let mut env_values: Vec<&str> = env
        .value_symbols
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    let mut index_values: Vec<&str> = index
        .value_headers
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    env_values.sort_unstable();
    index_values.sort_unstable();
    assert_eq!(
        env_values, index_values,
        "value-symbol names must match the env walk for source:\n{source}"
    );

    let mut env_aug_types: Vec<(String, &str)> = env
        .augmentation_scopes
        .keys()
        .map(|(scope, key)| (format!("{scope:?}"), key.name.as_ref()))
        .collect();
    let mut index_aug_types: Vec<(String, &str)> = index
        .augmentation_type_headers
        .iter()
        .flat_map(|(scope, names)| {
            names
                .keys()
                .map(move |key| (format!("{scope:?}"), key.name.as_ref()))
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
        .map(|(scope, key)| (format!("{scope:?}"), key.name.as_ref()))
        .collect();
    let mut index_aug_values: Vec<(String, &str)> = index
        .augmentation_value_headers
        .iter()
        .flat_map(|(scope, names)| {
            names
                .keys()
                .map(move |key| (format!("{scope:?}"), key.name.as_ref()))
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
        header
            .contributors
            .iter()
            .map(|contributor| contributor.anchor.contributor_index)
            .collect::<Vec<_>>(),
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
            .contributors
            .iter()
            .map(|contributor| contributor.anchor.contributor_index)
            .collect::<Vec<_>>(),
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
fn declare_global_namespace_jsx_augmentation_parity() {
    // A `namespace` nested inside `declare global` registers its inner members
    // under their qualified `JSX.X` names in the GLOBAL augmentation scope. The
    // header index and the env walk must register the SAME qualified keys
    // (`JSX.IntrinsicElements`, `JSX.Element`) — otherwise `has_global_
    // augmentation` (header-driven) and the lazy body memo (env-driven) would
    // disagree on the `(scope, name)` identity. A second block re-targeting
    // `JSX.IntrinsicElements` must fold into the same key on both sides.
    assert_name_parity(
        r#"
export {};
declare global {
    namespace JSX {
        interface IntrinsicElements {
            div: { id?: string; className?: string };
            span: { title?: string };
        }
        interface Element {
            __element_brand__: true;
        }
    }
}
declare global {
    namespace JSX {
        interface IntrinsicElements {
            customCard: { variant?: "primary" | "secondary" };
        }
    }
}
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
        .params
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
fn member_header_facts_match_production_index_across_body_shapes() {
    // Alias bodies: literal members, intersection descent; mapped /
    // utility bodies contribute NO syntactic members — the stored
    // member-header FACT inventory must agree with the production
    // parse-time index (names AND flags) for every body shape.
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
        let env_members: Vec<MemberHeader> = env.type_symbols[name]
            .merged_member_header_facts()
            .into_iter()
            .map(|fact| MemberHeader {
                name: fact.name,
                kind: if fact.is_method {
                    MemberHeaderKind::Method
                } else {
                    MemberHeaderKind::Property
                },
                optional: fact.optional,
                readonly: fact.readonly,
            })
            .collect();
        let header_members = &index
            .type_header(name)
            .unwrap_or_else(|| panic!("{name} header"))
            .member_headers;
        assert_eq!(
            &env_members, header_members,
            "member-header facts must match the production index for:\n{source}"
        );
    }
}

#[test]
fn seeded_member_headers_carry_fact_flags_and_match_production_index() {
    // The env-seeded header mirror reads the stored `MemberHeaderFact`
    // inventory — real syntactic flags (method kind, `?`, `readonly`) — not a
    // body walk with hard-coded defaults. Parity target: the production
    // parse-time index over the same source.
    let source = "interface I { p?: string; readonly c: boolean; m(): void }\n";
    let env = parse_and_build_env(source);
    let seeded = DeclHeaderIndex::from_eval_env(&env);
    let production = index_for(source);

    let seeded_headers = &seeded.type_header("I").expect("seeded I").member_headers;
    let production_headers = &production
        .type_header("I")
        .expect("production I")
        .member_headers;
    assert_eq!(
        seeded_headers, production_headers,
        "seeded member headers must equal the production parse-time headers (names AND flags)"
    );

    // Explicit per-flag discrimination (not just parity): the fact carries
    // the real syntactic flags.
    let by_name = |name: &str| {
        seeded_headers
            .iter()
            .find(|h| h.name == name)
            .unwrap_or_else(|| panic!("member {name}"))
    };
    assert!(by_name("p").optional, "`p?` must be optional");
    assert_eq!(by_name("p").kind, MemberHeaderKind::Property);
    assert!(!by_name("p").readonly);
    assert!(by_name("c").readonly, "`readonly c` must be readonly");
    assert!(!by_name("c").optional);
    assert_eq!(
        by_name("m").kind,
        MemberHeaderKind::Method,
        "`m()` must be a Method header"
    );
    assert!(!by_name("m").optional);

    // Negative: exactly the three declared members — nothing fabricated.
    let mut names: Vec<&str> = seeded_headers.iter().map(|h| h.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["c", "m", "p"]);
}

#[test]
fn seeded_member_headers_keep_own_members_under_heritage_and_skip_base_names() {
    // A heritage-carrying contributor (`interface X extends Base { a }`)
    // contributes its OWN members only: the heritage `Ref` is not a member
    // header (inherited members surface through the semantic reducer, never
    // the shallow index). Merged same-name contributors union first-seen.
    let source = "interface Base { b: string }\ninterface X extends Base { a: number }\ninterface X { m(): void }\n";
    let env = parse_and_build_env(source);
    let seeded = DeclHeaderIndex::from_eval_env(&env);

    let x = seeded.type_header("X").expect("X header");
    let names: Vec<&str> = x.member_headers.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(
        names,
        ["a", "m"],
        "own members across merged contributors, first-seen order, no base names"
    );
    assert!(
        !names.contains(&"b") && !names.contains(&"Base"),
        "heritage members/names must not enter the shallow header index"
    );
    // Production-index parity for the merged + heritage case.
    let production = index_for(source);
    assert_eq!(
        x.member_headers,
        production
            .type_header("X")
            .expect("production X")
            .member_headers
    );
}

#[test]
fn seeded_enum_headers_read_the_stored_member_names_fact() {
    // Enum member names reach the seeded index through the stored
    // `EnumMemberNamesFact` (the presence rail), including unfoldable-value
    // members; merged same-name enums union first-seen.
    let source = "enum E { A, B = compute() }\nenum E { C = 'c' }\n";
    let env = parse_and_build_env(source);

    // The stored per-contributor facts exist and union across contributors.
    let group = &env.value_symbols["E"];
    let fact = group
        .merged_enum_member_names_fact()
        .expect("enum name fact");
    assert_eq!(fact.names.as_ref(), ["A", "B", "C"]);

    let seeded = DeclHeaderIndex::from_eval_env(&env);
    assert_eq!(
        seeded.enum_headers["E"].member_names,
        ["A", "B", "C"],
        "seeded enum headers read the stored fact (presence rail superset)"
    );
    // Production parity.
    let production = index_for(source);
    assert_eq!(
        seeded.enum_headers["E"].member_names,
        production.enum_headers["E"].member_names
    );
}

#[test]
fn enum_registers_dual_space_headers_with_members_in_enum_table() {
    use crate::analysis::type_eval::{TypeDeclKind, ValueDeclKind};

    let source = "enum E { A, B }\nexport enum F { C = 'c' }\n";
    let index = index_for(source);

    // An `enum` is a dual-space symbol (like a class): it resolves through
    // the shared type + value demand path, so it registers BOTH a value
    // header (kind Enum) and a type header (kind Alias — the projected-type
    // union). The member NAMES live ONLY in the dedicated enum table (the
    // member-presence facts rail); the value header's object-member list
    // stays EMPTY so the facts rail never double-emits enum members as plain
    // value-object members.
    assert_eq!(index.value_headers["E"].kind, ValueDeclKind::Enum);
    assert_eq!(index.value_headers["F"].kind, ValueDeclKind::Enum);
    assert!(
        index.value_headers["E"].object_member_headers.is_empty(),
        "enum members live in enum_headers, never as value-object members"
    );
    assert_eq!(index.type_headers["E"].kind, TypeDeclKind::Alias);
    assert_eq!(index.type_headers["F"].kind, TypeDeclKind::Alias);
    assert!(
        index.type_headers["E"].member_headers.is_empty(),
        "the enum's type is a projected-type union, not an object with members"
    );

    // The member-name authority remains the dedicated enum table.
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
    assert_eq!(
        index.enum_headers["E"]
            .contributors
            .iter()
            .map(|contributor| contributor.anchor.contributor_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        index.enum_headers["F"]
            .contributors
            .iter()
            .map(|contributor| contributor.anchor.contributor_index)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
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
    assert_eq!(
        index.enum_headers["E"]
            .contributors
            .iter()
            .map(|contributor| contributor.anchor.contributor_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn from_eval_env_populates_enum_headers_matching_index_enum() {
    // Post-merge an `enum` is a VALUE symbol (kind Enum) carrying its
    // `enum_members`, so the env-seeded `from_eval_env` mirror (the test/debug
    // seeding path) must ALSO populate the dedicated `enum_headers` table with
    // the SAME member-name inventory the production `index_enum` (AST) path
    // records — including the UNIONED member set for a merged enum. An empty
    // `enum_headers` would give a seeded `ShallowFileState` the enum's bodies
    // but an empty `enum_symbol_names()`, breaking the parse-stable-hash
    // enum-header fold and the enum `MemberPresence` fact emission for seeded
    // artifacts.
    let source =
        "enum E { A, B }\nenum M { X }\nenum M { Y }\nexport enum S { Idle = 'idle', Active = 'active' }\n";
    let env = parse_and_build_env(source);
    let seeded = DeclHeaderIndex::from_eval_env(&env);
    let production = index_for(source);

    // Same enum-symbol key set (the `enum_symbol_names()` authority).
    let mut seeded_keys: Vec<&str> = seeded
        .enum_headers
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    let mut prod_keys: Vec<&str> = production
        .enum_headers
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    seeded_keys.sort_unstable();
    prod_keys.sort_unstable();
    assert_eq!(
        seeded_keys, prod_keys,
        "from_eval_env must register the SAME enum symbols as index_enum"
    );
    assert!(
        !seeded_keys.is_empty(),
        "the seeded mirror must actually carry enum headers"
    );

    // Same member-name inventory per enum (the `enum_member_names()` authority),
    // including the merged union.
    for name in ["E", "M", "S"] {
        assert_eq!(
            seeded.enum_headers[name].member_names, production.enum_headers[name].member_names,
            "seeded enum_headers[{name}].member_names must match index_enum"
        );
    }
    // Concretely: single enum, merged enum (union of both declarations), and a
    // string enum, all in source order.
    assert_eq!(seeded.enum_headers["E"].member_names, vec!["A", "B"]);
    assert_eq!(
        seeded.enum_headers["M"].member_names,
        vec!["X", "Y"],
        "a merged enum must union both declarations' members in source order"
    );
    assert_eq!(
        seeded.enum_headers["S"].member_names,
        vec!["Idle", "Active"]
    );

    // The enum's members live ONLY in `enum_headers`, never double-registered
    // as value-object members (consistent with the production path).
    assert!(
        seeded.value_headers["E"].object_member_headers.is_empty(),
        "enum members must not leak into the value header's object members"
    );
}

#[test]
fn from_eval_env_enum_headers_include_unfoldable_and_computed_member_names() {
    // The env-seeded `from_eval_env` mirror must seed `enum_headers` with the
    // FULL member-NAME set the production `index_enum` records — EVERY
    // statically-named member, INCLUDING members whose VALUE Verter cannot
    // statically fold (`A = 1 << 2`, and the bare `B` after it whose
    // auto-increment value is deferred) and computed STRING member names
    // (`["X"]`). Seeding from the FOLDABLE value subset (`merged_enum_members`)
    // would UNDER-COUNT: `E` would see only `C`/`D` (the unfoldable `A` and
    // deferred `B` dropped) and `N` NOTHING (computed names dropped) — diverging
    // from `index_enum` and under-emitting the seeded enum `MemberPresence`
    // facts / `parse_stable_hash` enum-header fold. The member-NAME set is the
    // PRESENCE-rail authority, independent of value foldability.
    let source = "export enum E { A = 1 << 2, B, C = 5, D }\n\
                  export enum N { [\"X\"] = 1, [\"Y\"] = 2 }\n";
    let env = parse_and_build_env(source);
    let seeded = DeclHeaderIndex::from_eval_env(&env);
    let production = index_for(source);

    // Same enum-symbol key set as the production walk.
    let mut seeded_keys: Vec<&str> = seeded
        .enum_headers
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    let mut prod_keys: Vec<&str> = production
        .enum_headers
        .keys()
        .map(|key| key.name.as_ref())
        .collect();
    seeded_keys.sort_unstable();
    prod_keys.sort_unstable();
    assert_eq!(seeded_keys, prod_keys);

    // The FULL member-name inventory per enum must match `index_enum` exactly —
    // unfoldable-value members (`A`/`B`) and computed-string names (`X`/`Y`)
    // included.
    assert_eq!(
        seeded.enum_headers["E"].member_names, production.enum_headers["E"].member_names,
        "seeded enum_headers[E] must carry EVERY static member name (deferred-value A/B included), matching index_enum"
    );
    assert_eq!(
        seeded.enum_headers["E"].member_names,
        vec!["A", "B", "C", "D"],
        "the presence rail carries all names regardless of value foldability"
    );
    assert_eq!(
        seeded.enum_headers["N"].member_names, production.enum_headers["N"].member_names,
        "seeded enum_headers[N] must carry computed-string member names (X/Y), matching index_enum"
    );
    assert_eq!(seeded.enum_headers["N"].member_names, vec!["X", "Y"]);
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
    let build_ctx =
        crate::analysis::type_eval_build::BuildEvalEnvContext::new("inline:selective-parity");
    let whole = build_eval_env(&ret.program, source, &build_ctx);

    let mut scratch = EvalEnv::new();
    for contributor in &index.type_header("Merged").expect("Merged").contributors {
        // Selective lowering passes the statement's ORIGINAL top-level index
        // (the recorded contributor locator), never a renumbered position.
        crate::analysis::type_eval_build::lower_top_level_statement(
            &ret.program.body[contributor.anchor.contributor_index as usize],
            crate::analysis::type_eval_build::StatementLowerCtx::from_contributor_anchor(
                &build_ctx,
                contributor.anchor,
            ),
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
        selective_group.merged_member_header_facts(),
        whole_group.merged_member_header_facts()
    );
    // NEGATIVE: the un-demanded sibling was never lowered.
    assert!(
        !scratch.type_symbols.contains_key("Unrelated"),
        "selective lowering must not lower the un-demanded statement"
    );
    assert_eq!(scratch.total_decl_count(), 2);
}
