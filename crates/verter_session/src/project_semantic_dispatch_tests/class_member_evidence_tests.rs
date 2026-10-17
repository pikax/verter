//! A read of a class member whose declaring elements name nothing (a
//! property typed by keywords, literals, or unions, arrays and tuples of
//! them) depends only on those elements and the class's `extends` clause.
//! It roots on the class's `Member` fact for the name instead of the class
//! file's whole hash: the fact stays put across an edit to any other member
//! and moves with an edit to the elements declaring the name, to how many
//! elements declare it, or to the `extends` clause. A member whose type
//! names anything keeps the whole-file read.
//!
//! The expected answers are TypeScript's declared-type reads of these
//! fixtures: a property reads its annotation, an own declaration shadows
//! the base's, and a member the class does not declare reads from the class
//! it extends.

use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::fact_cache::{
    signature_entries, FactVersionRef, ReadSetSignature,
};
use verter_session_query::facts::receipt::ReceiptWalk;
use verter_session_query::facts::{FactKey, SymbolSpace};
use verter_type_engine::semantic_query::{FlowReturnKey, SemanticQueryOutput};
use verter_type_expr::facts::{FactPropertyKey, FunctionPartIdentity};
use verter_type_expr::{TopLevelOwnerId, TypeExpr};

const CLASS: &str = "/ws/class-member-evidence/c.ts";
const READER: &str = "/ws/class-member-evidence/p.ts";

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(path.to_string()),
        input_id: path.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(path)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn host_with(class_source: &str, reader_source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, CLASS, class_source);
    upsert(&host, READER, reader_source);
    host
}

/// The class `Member` fact of `class.member` for the current content of
/// [`CLASS`], or `None` when the content produces no such fact.
fn member_fact(host: &VerterHost, class: &str, member: &str) -> Option<[u8; 16]> {
    let _ = host.ensure_indexed_ready_serve(CLASS);
    let artifacts = host
        .current_content_pinned_artifacts(CLASS)
        .expect("the class file's current content is indexed");
    artifacts
        .facts
        .lookup_or_compute(&FactKey::Member {
            exporter: crate::file_artifact_store::InternedName::from(class),
            name: FactPropertyKey::identifier(member),
            space: SymbolSpace::Type,
        })
        .map(|fact| fact.semantic_hash)
}

fn flow_return_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    function: &str,
) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(READER),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(function),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(READER),
        demand: verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
        input: verter_type_engine::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    }
}

/// The body-derived return of `function` in [`READER`], with the strict
/// self-roots and every leaf fact its memo entry holds.
struct FlowReturnRead {
    ty: Option<TypeExpr>,
    diagnostics: Vec<u32>,
    self_roots: Vec<Arc<str>>,
    facts: Vec<FactVersionRef>,
}

fn leaf_facts(signature: &ReadSetSignature) -> Vec<FactVersionRef> {
    let mut walk = ReceiptWalk::default();
    let mut facts = Vec::new();
    for entry in signature_entries(&signature.facts) {
        match entry {
            FactVersionRef::Receipt(receipt) => {
                walk.leaves(receipt, |fact| {
                    facts.push(fact.clone());
                    true
                });
            }
            fact => facts.push(fact.clone()),
        }
    }
    facts
}

fn read_flow_return(host: &Arc<VerterHost>, function: &str) -> FlowReturnRead {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = SemanticQueryKey::FlowReturn(Box::new(flow_return_key(&dispatch, function)));
    let (ty, diagnostics) = match dispatch.execute(key.clone()) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => (
            host.project_node_to_type_expr_for_test(result.return_type()),
            result
                .checker_diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code.code())
                .collect(),
        ),
        other => panic!("`{function}` produced no flow return: {other:?}"),
    };
    let self_roots = dispatch
        .graph()
        .entry_self_root_canonicals_for_tests(&key)
        .map(|roots| roots.to_vec())
        .unwrap_or_default();
    let facts = dispatch
        .graph()
        .entry_read_set_signature_for_tests(&key)
        .map(|signature| leaf_facts(&signature))
        .unwrap_or_default();
    FlowReturnRead {
        ty,
        diagnostics,
        self_roots,
        facts,
    }
}

fn is_member_fact(fact: &FactVersionRef, class: &str, member: &str) -> bool {
    matches!(
        fact,
        FactVersionRef::Parse(parse)
            if parse.canonical_id == CLASS
                && parse.key == FactKey::Member {
                    exporter: crate::file_artifact_store::InternedName::from(class),
                    name: FactPropertyKey::identifier(member),
                    space: SymbolSpace::Type,
                }
    )
}

fn strict_roots_on_class_file(read: &FlowReturnRead) -> bool {
    read.self_roots.iter().any(|root| root.as_ref() == CLASS)
}

/// The `Member` fact of a class member is the fingerprint of exactly the
/// elements declaring the name and the class's `extends` clause: an edit
/// to another member's value, a new member before it (which moves every
/// offset after it) or a method body edit keep it; an edit to the
/// declaring element's type, optionality or visibility, a second element
/// declaring the name, or an edit to the `extends` clause move it; a
/// same-name interface merging into the class leaves no single class for
/// the fact to describe.
#[test]
fn a_class_member_fact_moves_only_with_the_elements_declaring_the_name() {
    let host = host_with(
        "class A0 { }\nclass A1 { }\nclass B extends A0 { a: string = 's'; u = 1; m() { return 1; } }\n",
        "export {};\n",
    );
    let base = member_fact(&host, "B", "a").expect("a class member has a member fact");

    let unchanged = [
        (
            "another member's value",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { a: string = 's'; u = 22; m() { return 1; } }\n",
        ),
        (
            "a new member before the read one",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { x = 0; a: string = 's'; u = 1; m() { return 1; } }\n",
        ),
        (
            "a method body",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { a: string = 's'; u = 1; m() { return 'two'; } }\n",
        ),
        (
            "another declaration of the file",
            "class A0 { z = 1; }\nclass A1 { }\nclass B extends A0 { a: string = 's'; u = 1; m() { return 1; } }\n",
        ),
    ];
    for (edit, source) in unchanged {
        upsert(&host, CLASS, source);
        assert_eq!(
            member_fact(&host, "B", "a"),
            Some(base),
            "an edit to {edit} keeps the member fact"
        );
    }

    let moved = [
        (
            "the declaring element's type",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { a: number = 1; u = 1; m() { return 1; } }\n",
        ),
        (
            "the declaring element's optionality",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { a?: string = 's'; u = 1; m() { return 1; } }\n",
        ),
        (
            "the declaring element's visibility",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { private a: string = 's'; u = 1; m() { return 1; } }\n",
        ),
        (
            "how many elements declare the name",
            "class A0 { }\nclass A1 { }\nclass B extends A0 { a: string = 's'; u = 1; m() { return 1; } constructor(public a: string) { super(); } }\n",
        ),
        (
            "the extends clause",
            "class A0 { }\nclass A1 { }\nclass B extends A1 { a: string = 's'; u = 1; m() { return 1; } }\n",
        ),
        (
            "the extends clause's removal",
            "class A0 { }\nclass A1 { }\nclass B { a: string = 's'; u = 1; m() { return 1; } }\n",
        ),
    ];
    for (edit, source) in moved {
        upsert(&host, CLASS, source);
        let fact = member_fact(&host, "B", "a");
        assert!(fact.is_some(), "a class member keeps a member fact");
        assert_ne!(fact, Some(base), "an edit to {edit} moves the member fact");
    }

    upsert(
        &host,
        CLASS,
        "class B { a: string = 's'; a: number = 1; }\n",
    );
    let ordered = member_fact(&host, "B", "a").expect("duplicate declarations keep a fact");
    upsert(
        &host,
        CLASS,
        "class B { a: number = 1; a: string = 's'; }\n",
    );
    assert_ne!(
        member_fact(&host, "B", "a"),
        Some(ordered),
        "swapping which declaring element comes first moves the member fact"
    );

    // A selected type deeper than the fingerprint budget is never certified.
    let deep = |leaf: &str| {
        format!(
            "class B {{ a!: {}{leaf}{}; }}\n",
            "Array<".repeat(70),
            ">".repeat(70)
        )
    };
    upsert(&host, CLASS, &deep("string"));
    assert_eq!(
        member_fact(&host, "B", "a"),
        None,
        "a budget-exhausted fingerprint publishes no member fact"
    );

    upsert(
        &host,
        CLASS,
        "class A0 { }\nclass A1 { }\nclass B extends A0 { a: string = 's'; u = 1; m() { return 1; } }\ninterface B { z: 1 }\n",
    );
    assert_eq!(
        member_fact(&host, "B", "a"),
        None,
        "a class merged with an interface has no single class to fingerprint"
    );
}

/// A read through a parameter of an imported class, of a member whose type
/// names nothing, roots its function's return on the class's `Member` fact
/// for that name, never on the class file: the class file is not among the
/// return's strict self-roots. A member whose type names another
/// declaration keeps the whole-file read and its strict root.
#[test]
fn a_reference_free_member_read_roots_on_the_member_fact_not_the_class_file() {
    let host = host_with(
        "export interface Other { o: 1 }\nexport class B { a: string = 's'; r!: Other; u = 1; }\n",
        "import { B } from './c';\n\
         export function f(b: B) { return b.a; }\n\
         export function g(b: B) { return b.r; }\n",
    );

    let precise = read_flow_return(&host, "f");
    assert_eq!(
        precise.ty,
        Some(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String))
    );
    assert!(
        precise
            .facts
            .iter()
            .any(|fact| is_member_fact(fact, "B", "a")),
        "the read roots on the member fact: {:?}",
        precise.facts
    );
    assert!(
        !strict_roots_on_class_file(&precise),
        "the class file is no strict root of the read: {:?}",
        precise.self_roots
    );

    let whole = read_flow_return(&host, "g");
    assert!(
        !whole
            .facts
            .iter()
            .any(|fact| is_member_fact(fact, "B", "r")),
        "a member whose type names a declaration records no member fact"
    );
    assert!(
        strict_roots_on_class_file(&whole),
        "a member whose type names a declaration keeps the class file as a strict root: {:?}",
        whole.self_roots
    );

    // Validate the observation actually adopted by the read, rather than a
    // separately recomputed hash. Indexing is explicit here: this checks the
    // member fact, not the enclosing query's cross-file warm reuse.
    use verter_session_query::facts::store_view::StoreView;
    let adopted = precise
        .facts
        .iter()
        .find(|fact| is_member_fact(fact, "B", "a"))
        .unwrap();
    for (source, valid) in [
        ("export class B { u = 22; a: string = 's'; }", true),
        ("export class B { a: number = 1; }", false),
        ("export class B { }", false),
        (
            "export class A {} export class B extends A { a: string = 's'; }",
            false,
        ),
        (
            "export class B { a: string = 's'; } interface B { z: 1 }",
            false,
        ),
    ] {
        upsert(&host, CLASS, source);
        let _ = host.ensure_indexed_ready_serve(CLASS);
        assert_eq!(
            host.resolver_store_view_read()
                .into_owned_view()
                .validates(adopted),
            valid,
            "adopted evidence validates against the current class: {source}"
        );
    }
}

/// Types requiring a binding, accessors, methods and merged declarations
/// keep their content proof. They cannot use an exact property value to
/// certify the work their other inputs still require.
#[test]
fn member_reads_without_equivalent_proof_keep_content_evidence() {
    let mut failures = Vec::new();
    for (source, receiver) in [
        (
            "export interface Other { o: 1 } export class B { a!: Other; }",
            "B",
        ),
        ("export class B { a(): string { return 's'; } }", "B"),
        ("export class B { get a(): string { return 's'; } }", "B"),
        ("export class B<T> { a!: T; }", "B<string>"),
        ("export class B { a!: this; }", "B"),
        (
            "declare const value: string; export class B { a!: typeof value; }",
            "B",
        ),
        (
            "export class B { a: string = 's'; } interface B { z: 1 }",
            "B",
        ),
    ] {
        let host = host_with(
            source,
            &format!(
                "import {{ B }} from './c'; export function f(b: {receiver}) {{ return b.a; }}"
            ),
        );
        let read = read_flow_return(&host, "f");
        // A substituted argument can carry the caller's node scope. The
        // fallback must still retain the class's whole-content evidence;
        // node provenance alone does not enumerate all dependency facts.
        let has_content_fact = if receiver == "B<string>" {
            read.facts.iter().any(|fact| {
                matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == CLASS)
            })
        } else {
            read.self_roots.iter().any(|root| root.as_ref() == CLASS)
        };
        if !has_content_fact || read.facts.iter().any(|fact| is_member_fact(fact, "B", "a")) {
            failures.push(format!(
                "{source}: roots {:?}, facts {:?}",
                read.self_roots, read.facts
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The member cutoff must not cut off its caller's work: a later generic
/// call still diagnoses a rejected argument, and writes still narrow the
/// member read. Expected returns and TS2345 are checked with TypeScript 7.0.2.
#[test]
fn member_cutoff_preserves_writes_and_cold_warm_edited_diagnostics() {
    const READER_SOURCE: &str = "import { B } from './c';\n\
        declare function con<T extends string>(x: T): T;\n\
        export function f(b: B) { return con(b.a); }\n\
        export function g(b: B) { b.w = 's'; return b.w; }\n";
    let string = Some(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
    let host = host_with(
        "export class B { a: string = 's'; w!: string | number; }",
        READER_SOURCE,
    );
    for (source, diagnostics) in [
        (
            "export class B { a: string = 's'; w!: string | number; }",
            vec![],
        ),
        (
            "export class B { a: string = 's'; w!: string | number; u = 22; }",
            vec![],
        ),
        (
            "export class B { a: number = 1; w!: string | number; u = 22; }",
            vec![2345],
        ),
    ] {
        upsert(&host, CLASS, source);
        let fresh = host_with(source, READER_SOURCE);
        for (function, expected) in [("f", diagnostics), ("g", vec![])] {
            for read in [
                read_flow_return(&host, function),
                read_flow_return(&host, function),
                read_flow_return(&fresh, function),
            ] {
                assert_eq!(read.ty, string, "{function}: {source}");
                assert_eq!(read.diagnostics, expected, "{function}: {source}");
            }
        }
    }
    upsert(
        &host,
        READER,
        &READER_SOURCE.replace("b.w = 's'", "b.w = 1"),
    );
    assert_eq!(
        read_flow_return(&host, "g").ty,
        Some(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)),
        "an affecting write invalidates the enclosing return"
    );
}

/// Edits that change what a reference-free member read answers reach it: an
/// edit to the declaring element's type, the element's removal (the read
/// then follows the `extends` clause to the base that declares the name), an
/// edit to the `extends` clause, and an own redeclaration shadowing the base
/// again. Each answer is read off the host that served the earlier ones and
/// equals what TypeScript reads for that content.
#[test]
fn edits_that_change_a_reference_free_member_read_reach_it() {
    let host = host_with(
        "export class A { a = 'x' as const; }\nexport class Z { a = 'z' as const; }\nexport class B extends A { a: string = 's'; u = 1; }\n",
        "import { B } from './c';\nexport function f(b: B) { return b.a; }\n",
    );
    let string = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let number = TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number);
    assert_eq!(read_flow_return(&host, "f").ty, Some(string.clone()));

    upsert(
        &host,
        CLASS,
        "export class A { a = 'x' as const; }\nexport class Z { a = 'z' as const; }\nexport class B extends A { a: string = 's'; u = 22; }\n",
    );
    assert_eq!(
        read_flow_return(&host, "f").ty,
        Some(string.clone()),
        "another member's edit keeps the answer"
    );

    upsert(
        &host,
        CLASS,
        "export class A { a = 'x' as const; }\nexport class Z { a = 'z' as const; }\nexport class B extends A { a: number = 1; u = 22; }\n",
    );
    assert_eq!(
        read_flow_return(&host, "f").ty,
        Some(number),
        "the declaring element's type edit reaches the read"
    );

    upsert(
        &host,
        CLASS,
        "export class A { a = 'x' as const; }\nexport class Z { a = 'z' as const; }\nexport class B extends A { u = 22; }\n",
    );
    assert_eq!(
        read_flow_return(&host, "f").ty,
        Some(TypeExpr::string_literal("x")),
        "with the own declaration gone the read follows the extends clause"
    );

    upsert(
        &host,
        CLASS,
        "export class A { a = 'x' as const; }\nexport class Z { a = 'z' as const; }\nexport class B extends Z { u = 22; }\n",
    );
    assert_eq!(
        read_flow_return(&host, "f").ty,
        Some(TypeExpr::string_literal("z")),
        "an edit to the extends clause reaches the inherited read"
    );

    upsert(
        &host,
        CLASS,
        "export class A { a = 'x' as const; }\nexport class Z { a = 'z' as const; }\nexport class B extends Z { a: 'own' = 'own'; u = 22; }\n",
    );
    assert_eq!(
        read_flow_return(&host, "f").ty,
        Some(TypeExpr::string_literal("own")),
        "an own redeclaration shadows the base again"
    );
}
