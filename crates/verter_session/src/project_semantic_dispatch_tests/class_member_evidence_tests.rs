//! A read of a class member whose declaring elements name nothing (a
//! property typed by keywords, literals, or unions, arrays and tuples of
//! them) depends only on those elements and the class's `extends` clause.
//! It roots on the class's `Member` fact for the name instead of the class
//! file's whole hash: the fact stays put across an edit to any other member
//! and moves with an edit to the elements declaring the name, to how many
//! elements declare it, or to the `extends` clause. Direct imported property
//! heads additionally retain exact source route and normal target scope proof.
//! Closed annotated single methods/accessors have source-independent signature
//! coordinates; unproved bindings, body-derived returns and overload sets keep
//! whole-content reads.
//!
//! The expected answers are TypeScript's declared-type reads of these
//! fixtures: a property reads its annotation, an own declaration shadows
//! the base's, and a member the class does not declare reads from the class
//! it extends.

use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};
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

fn flow_return_key<M: verter_audit::observe::ObserveExec>(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities<M>>,
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
        normalized_type_args: Arc::<[_]>::from(Vec::new().into_boxed_slice()).into(),
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
    complete: bool,
    self_roots: Vec<Arc<str>>,
    facts: Vec<FactVersionRef>,
    signature: Vec<FactVersionRef>,
    callable_locations: Vec<Option<verter_span::Span>>,
    return_identity_hash: Option<[u8; 16]>,
}

fn leaf_facts(signature: &ReadSetSignature) -> Vec<FactVersionRef> {
    let mut walk = ReceiptWalk::default();
    let mut facts = Vec::new();
    for entry in signature.facts.iter() {
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
    read_flow_return_with_mode::<verter_audit::observe::Uncaptured>(host, function)
}

fn read_flow_return_with_mode<M: verter_audit::observe::ObserveExec>(
    host: &Arc<VerterHost>,
    function: &str,
) -> FlowReturnRead {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::<'_, M>::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = SemanticQueryKey::FlowReturn(Box::new(flow_return_key(&dispatch, function)));
    let (ty, diagnostics, complete, callable_locations, return_identity_hash) =
        match dispatch.execute(key.clone()) {
            QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::FlowReturn(result),
                ..
            }) => (
                host.project_node_to_type_expr_for_test(result.return_type()),
                result
                    .checker_diagnostics()
                    .iter()
                    .map(|diagnostic| diagnostic.code().code())
                    .collect(),
                result.fact().status().is_complete(),
                match dispatch.graph().node_data(result.return_type()).as_deref() {
                    Some(verter_type_engine::semantic_query::SemanticNodeData::Signature {
                        params,
                        signature_span,
                        return_type_span,
                        ..
                    }) => std::iter::once(*signature_span)
                        .chain(std::iter::once(*return_type_span))
                        .chain(params.iter().map(|param| param.span))
                        .collect(),
                    _ => Vec::new(),
                },
                match dispatch.graph().node_data(result.return_type()).as_deref() {
                    Some(verter_type_engine::semantic_query::SemanticNodeData::DeclRef {
                        identity,
                    }) => Some(identity.whole_hash),
                    _ => None,
                },
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
    let signature = dispatch
        .graph()
        .entry_read_set_signature_for_tests(&key)
        .map(|signature| signature.facts.iter().cloned().collect())
        .unwrap_or_default();
    FlowReturnRead {
        ty,
        diagnostics,
        complete,
        self_roots,
        facts,
        signature,
        callable_locations,
        return_identity_hash,
    }
}

fn is_member_fact(fact: &FactVersionRef, class: &str, member: &str) -> bool {
    matches!(
        fact,
        FactVersionRef::Parse(parse)
            if &*parse.canonical_id == CLASS
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

/// Explicit monomorphic signatures need only their selected member text.
#[test]
fn annotated_method_and_accessor_reads_keep_member_evidence() {
    for member in [
        "a(x: number): string { return 's'; }",
        "get a(): string { return 's'; }",
    ] {
        let host = host_with(
            &format!("export class B {{ {member} u = 1; }}"),
            "import { B } from './c'; export function f(b: B) { return b.a; }",
        );
        let cold = read_flow_return(&host, "f");
        assert!(
            cold.facts.iter().any(|fact| is_member_fact(fact, "B", "a")),
            "{member}: missing selected evidence: {:?}",
            cold.facts
        );
        assert!(
            !strict_roots_on_class_file(&cold),
            "{member}: {:?}",
            cold.self_roots
        );
        upsert(
            &host,
            CLASS,
            &format!("export class B {{ {member} u = 22; }}"),
        );
        let edited = read_flow_return(&host, "f");
        let fresh = host_with(
            &format!("export class B {{ {member} u = 22; }}"),
            "import { B } from './c'; export function f(b: B) { return b.a; }",
        );
        let control = read_flow_return(&fresh, "f");
        assert_eq!(edited.ty, cold.ty);
        assert_eq!(edited.ty, control.ty);
        assert_eq!(edited.diagnostics, control.diagnostics);
    }
}

/// Unproved binding scopes, body-derived or generic callable signatures and
/// merged declarations keep their whole-content proof. They cannot use an exact property value to
/// certify the work their other inputs still require.
#[test]
fn member_reads_without_equivalent_proof_keep_content_evidence() {
    let mut failures = Vec::new();
    for (source, receiver) in [
        (
            "export interface Other { o: 1 } export class B { a!: Other; }",
            "B",
        ),
        ("export class B { a() { return 's'; } }", "B"),
        ("export class B { get a() { return 's'; } }", "B"),
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
                matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. } if &**canonical_id == CLASS)
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

/// Validation produces the current dependency facts through the request host;
/// callers need not manually index edited dependencies before asking to reuse.
#[test]
fn selected_member_validation_indexes_edited_dependencies_without_preindexing() {
    let host = host_with(
        "export class B { a!: string; u = 1; }",
        "import { B } from './c'; export function f(b: B) { return b.a; }",
    );
    let read = read_flow_return(&host, "f");
    let adopted = read
        .facts
        .iter()
        .find(|fact| is_member_fact(fact, "B", "a"))
        .expect("selected member evidence");
    for (source, expected) in [
        ("export class B { a!: string; u = 22; }", true),
        ("export class B { a!: number; u = 22; }", false),
        ("export class B { u = 22; }", false),
    ] {
        upsert(&host, CLASS, source);
        let valid =
            host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
                verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates(
                    ctx, adopted,
                )
            });
        assert_eq!(valid, expected, "{source}");
    }
}

#[test]
fn imported_and_global_member_reads_reuse_after_unrelated_edits() {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    for imported in [true, false] {
        for member in [
            "a!: string;",
            "get a(): string { return 's'; }",
            "a(x: number): string { return 's'; }",
        ] {
            let prefix = if imported { "export " } else { "" };
            let reader = if imported {
                "import { B } from './c'; export function f(b: B) { return b.a; }"
            } else {
                "export function f(b: B) { return b.a; }"
            };
            let host = host_with(&format!("{prefix}class B {{ {member} u = 1; }}"), reader);
            let cold = read_flow_return(&host, "f");
            assert!(cold.complete, "cold answer must be exact");
            let count = flow_evaluations_for_tests();
            let warm = read_flow_return(&host, "f");
            assert!(warm.complete);
            assert_eq!(warm.diagnostics, cold.diagnostics);
            assert_eq!(warm.ty, cold.ty);
            assert_eq!(
                flow_evaluations_for_tests(),
                count,
                "warm {imported}: {member}"
            );
            upsert(
                &host,
                CLASS,
                &format!("{prefix}class B {{ z = 0; {member} u = 22; }}"),
            );
            let edited = read_flow_return(&host, "f");
            assert!(edited.complete);
            assert_eq!(edited.diagnostics, cold.diagnostics);
            assert_eq!(edited.ty, cold.ty);
            assert_eq!(
                flow_evaluations_for_tests(),
                count,
                "unrelated edit {imported}: {member}; cold facts: {:?}; cold roots: {:?}; edited facts: {:?}", cold.facts, cold.self_roots, edited.facts
            );
            let fresh = host_with(
                &format!("{prefix}class B {{ z = 0; {member} u = 22; }}"),
                reader,
            );
            assert_eq!(edited.ty, read_flow_return(&fresh, "f").ty);
        }
    }
}

#[test]
fn imported_member_reference_keeps_target_dependencies_and_reuses_its_owner() {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    let reader = "import { B } from './c'; export function f(b: B) { return b.a.value; }";
    let source = "import { Other } from './other'; export class B { a!: Other; u = 1; }";
    let host = host_with(source, reader);
    upsert(
        &host,
        "/ws/class-member-evidence/other.ts",
        "export interface Other { value: string; }",
    );
    let cold = read_flow_return(&host, "f");
    assert!(cold.facts.iter().any(|fact| is_member_fact(fact, "B", "a")));
    assert!(!strict_roots_on_class_file(&cold));
    assert!(cold.complete);
    let count = flow_evaluations_for_tests();
    upsert(&host, CLASS, &source.replace("u = 1", "u = 22"));
    let owner_edited = read_flow_return(&host, "f");
    assert_eq!(
        (
            &owner_edited.ty,
            owner_edited.complete,
            &owner_edited.diagnostics
        ),
        (&cold.ty, cold.complete, &cold.diagnostics)
    );
    assert_eq!(
        flow_evaluations_for_tests(),
        count,
        "owner edit reuses the imported-reference answer; facts: {:?}",
        cold.facts
    );
    upsert(
        &host,
        "/ws/class-member-evidence/other.ts",
        "export interface Other { value: number; }",
    );
    let edited = read_flow_return(&host, "f");
    let fresh = host_with(source, reader);
    upsert(
        &fresh,
        "/ws/class-member-evidence/other.ts",
        "export interface Other { value: number; }",
    );
    let control = read_flow_return(&fresh, "f");
    assert_eq!(
        (&edited.ty, edited.complete, &edited.diagnostics),
        (&control.ty, control.complete, &control.diagnostics)
    );
    assert_ne!(edited.ty, cold.ty, "target content must invalidate");
}

/// A reference-typed member read (`b.a` typed by an imported interface)
/// whose only contact with the dependency is the interned carrier
/// `DeclIdentity` still tracks that dependency's content version: the
/// version read observes its `FileWholeHash` fact, and every answer the
/// host serves — cold, recomputed after a dependency edit, or warm —
/// carries the dependency's live version, never a superseded one.
#[test]
fn imported_reference_version_read_is_tracked_across_dependency_edits() {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    let other = "/ws/class-member-evidence/other.ts";
    let reader = "import { B } from './c'; export function f(b: B) { return b.a; }";
    let source = "import { Other } from './other'; export class B { a!: Other; u = 1; }";
    let host = host_with(source, reader);
    upsert(&host, other, "export interface Other { value: string; }");
    let cold = read_flow_return(&host, "f");
    assert!(cold.complete, "cold answer must be exact");
    let live = host
        .authoritative_current_content_hash(other)
        .expect("the dependency is indexed");
    assert!(
        cold.facts.iter().any(|fact| matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id.as_ref() == other && *hash == live
        )),
        "the version interned into the answer's reference must be observed: {:?}",
        cold.facts
    );
    // A comment-only edit changes the dependency's content version without
    // changing the answer's semantics: the warm entry must miss, and the
    // recomputed answer must carry the live version.
    let count = flow_evaluations_for_tests();
    upsert(
        &host,
        other,
        "// edit\nexport interface Other { value: string; }",
    );
    let live_after = host
        .authoritative_current_content_hash(other)
        .expect("the edited dependency is indexed");
    assert_ne!(live_after, live);
    let edited = read_flow_return(&host, "f");
    assert_eq!(
        (
            edited.complete,
            edited.diagnostics.as_slice(),
            edited.ty.as_ref()
        ),
        (cold.complete, cold.diagnostics.as_slice(), cold.ty.as_ref()),
        "a comment-only edit preserves the answer"
    );
    assert!(
        flow_evaluations_for_tests() > count,
        "a warm answer cannot carry the dependency's superseded version"
    );
    assert_eq!(
        edited.return_identity_hash,
        Some(live_after),
        "the answer served after the edit must intern the live version"
    );
    // With the dependency unchanged again, the answer reuses warm and still
    // serves the live version.
    let steady = flow_evaluations_for_tests();
    let warm = read_flow_return(&host, "f");
    assert_eq!(warm.return_identity_hash, Some(live_after));
    assert_eq!(
        flow_evaluations_for_tests(),
        steady,
        "an unchanged dependency still reuses the warm answer"
    );
}

/// A bare imported reference returned as the answer (no member read) keeps
/// the same tracked-version contract: the interned declaration version is
/// observed, a dependency edit forces a recompute, and the served identity
/// is the live one.
#[test]
fn bare_imported_reference_version_is_tracked_across_dependency_edits() {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    let other = "/ws/class-member-evidence/other.ts";
    let reader = "import { Other } from './other'; export function f(o: Other) { return o; }";
    let host = host_with("export class B { u = 1; }", reader);
    upsert(&host, other, "export interface Other { value: string; }");
    let cold = read_flow_return(&host, "f");
    assert!(cold.complete, "cold answer must be exact");
    let live = host
        .authoritative_current_content_hash(other)
        .expect("the dependency is indexed");
    assert!(
        cold.facts.iter().any(|fact| matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id.as_ref() == other && *hash == live
        )),
        "the version interned into the bare reference must be observed: {:?}",
        cold.facts
    );
    let count = flow_evaluations_for_tests();
    upsert(
        &host,
        other,
        "// edit\nexport interface Other { value: string; }",
    );
    let live_after = host
        .authoritative_current_content_hash(other)
        .expect("the edited dependency is indexed");
    assert_ne!(live_after, live);
    let edited = read_flow_return(&host, "f");
    assert!(
        flow_evaluations_for_tests() > count,
        "warm answer kept a superseded version"
    );
    assert_eq!(edited.return_identity_hash, Some(live_after));
    let fresh = host_with("export class B { u = 1; }", reader);
    upsert(
        &fresh,
        other,
        "// edit\nexport interface Other { value: string; }",
    );
    assert_eq!(edited.ty, read_flow_return(&fresh, "f").ty);
}

#[test]
fn precise_member_cosmetic_names_refresh_without_changing_semantic_evidence() {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    let reader = "import { B } from './c'; export function f(b: B) { return b.a; }";
    for member in [
        "a(left: number): string { return 's'; }",
        "a!: [left: string];",
    ] {
        let source = format!("export class B {{ {member} u = 1; }}");
        let host = host_with(&source, reader);
        let cold = read_flow_return(&host, "f");
        assert!(cold.complete);
        let semantic = cold
            .facts
            .iter()
            .find_map(|fact| match fact {
                FactVersionRef::Parse(fact)
                    if is_member_fact(&FactVersionRef::Parse(fact.clone()), "B", "a")
                        && fact.lane == verter_session_query::facts::FactLane::Semantic =>
                {
                    Some(fact.expected_hash)
                }
                _ => None,
            })
            .unwrap();
        let changed = source.replace("left", "right");
        upsert(&host, CLASS, &changed);
        let edited = read_flow_return(&host, "f");
        assert_eq!(
            member_fact(&host, "B", "a"),
            Some(semantic),
            "R16 semantic evidence ignores cosmetic names"
        );
        let fresh = read_flow_return(&host_with(&changed, reader), "f");
        assert_eq!(
            (&edited.ty, edited.complete, &edited.diagnostics),
            (&fresh.ty, fresh.complete, &fresh.diagnostics),
            "graph-preserved names must refresh: {member}"
        );
        if member.contains("return") {
            let count = flow_evaluations_for_tests();
            upsert(
                &host,
                CLASS,
                &changed.replace("return 's'", "return 'longer'"),
            );
            let body_edited = read_flow_return(&host, "f");
            assert_eq!(
                (
                    body_edited.ty,
                    body_edited.complete,
                    body_edited.diagnostics
                ),
                (edited.ty, edited.complete, edited.diagnostics)
            );
            assert_eq!(
                flow_evaluations_for_tests(),
                count,
                "an explicitly annotated method body does not change its display signature"
            );
        }
    }
}

#[test]
fn precise_callable_locations_are_independent_of_member_offsets() {
    let reader = "import { B } from './c'; export function f(b: B) { return b.a; }";
    let source = "export class B { a(x?: number): string { return 's'; } }";
    let host = host_with(source, reader);
    let cold = read_flow_return(&host, "f");
    let changed = source.replace("{ a", "{ unrelated = 1; a");
    upsert(&host, CLASS, &changed);
    let warm = read_flow_return(&host, "f");
    let fresh = read_flow_return(&host_with(&changed, reader), "f");
    assert!(cold.complete && warm.complete && fresh.complete);
    assert!(!fresh.callable_locations.is_empty(), "callable control");
    assert!(
        fresh.callable_locations.iter().all(Option::is_none),
        "scope-free precise signature locations must be source-independent: {:?}",
        fresh.callable_locations
    );
    assert_eq!(warm.callable_locations, fresh.callable_locations);
    assert_eq!(warm.diagnostics, fresh.diagnostics);
    match fresh.ty {
        Some(TypeExpr::Function(ref function)) => assert!(function.parameters[0].optional),
        ref other => panic!("callable answer: {other:?}"),
    }
}

#[test]
fn overload_groups_retain_authored_content_and_order() {
    let reader = "import { B } from './c'; export function f(b: B) { return b.a; }";
    let source = "export class B { a(x: 1): string; a(x: 2): number; a(x: 1 | 2): string | number { return x; } }";
    let host = host_with(source, reader);
    let cold = read_flow_return(&host, "f");
    assert!(cold.facts.iter().any(|fact| matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id.as_ref() == CLASS)));
    assert!(!cold.facts.iter().any(|fact| is_member_fact(fact, "B", "a")));
    let warm = read_flow_return(&host, "f");
    let fresh = read_flow_return(&host_with(source, reader), "f");
    assert_eq!(
        (&cold.ty, cold.complete, &cold.diagnostics),
        (&warm.ty, warm.complete, &warm.diagnostics)
    );
    assert_eq!(
        (warm.ty, warm.complete, warm.diagnostics),
        (fresh.ty, fresh.complete, fresh.diagnostics)
    );
}

#[test]
fn standalone_augmentation_content_port_observes_the_served_snapshot() {
    use verter_type_engine::resolver_core::request_ports::{IndexedInputs, OwnedLowering};
    let host = host_with(
        "export {}; declare global { interface X { a: string } }",
        "",
    );
    host.ensure_indexed_ready_serve(CLASS).unwrap();
    let facts = host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
        let identity = IndexedInputs::artifact_key_for_current_content(ctx, CLASS).unwrap();
        let (answer, reads) =
            verter_type_engine::resolver_core::resolver_context::with_fact_tracer_cell(
                Default::default(),
                |_| OwnedLowering::augmenter_artifact_answer(ctx, &identity, identity.content_hash),
            );
        assert!(!answer.unwrap().augmentations.is_empty());
        match reads.finalise() {
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(facts) => facts,
            other => panic!("exact served contributor read refused: {other:?}"),
        }
    });
    assert!(facts.iter().any(|fact| matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id.as_ref() == CLASS)), "the standalone content port must certify the payload it exposed");
    upsert(
        &host,
        CLASS,
        "export {}; declare global { interface X { a: number } }",
    );
    host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
        assert!(!verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates_fact_signature(ctx, verter_session_query::facts::fact_set::FactsRef::Set(&facts)));
    });
}

#[test]
fn target_population_validation_materializes_edits_without_certifying_content() {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    let aug = "/ws/class-member-evidence/aug.ts";
    let reader = "import { B } from './c'; import './aug'; export function f(b: B) { return b.a; }";
    let before = "import './c'; declare module './c' { interface B { z: string } } export {};";
    let after = before.replace("z: string", "z: number");
    let host = host_with("export class B { a!: string; }", reader);
    upsert(&host, aug, before);
    host.ensure_indexed_ready_serve(aug).unwrap();
    let cold = read_flow_return(&host, "f");
    assert!(cold.complete);
    let population = cold.facts.iter().find(|fact| matches!(fact,
        FactVersionRef::RouteSurface(fact) if matches!(&fact.key,
            FactKey::ModuleAugmentationIndexShape { target_kind_tag: verter_session_query::facts::registry::AugmentationTargetKindTag::ResolvedRelativeCanonical,
                resolved_relative_canonical: Some(target), wildcard_pattern: None, .. } if target.as_ref() == CLASS))).unwrap();
    let count = flow_evaluations_for_tests();
    upsert(&host, aug, &after);
    host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
        assert!(
            verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates(
                ctx, population
            ),
            "unchanged target-wide membership validates through canonical current-fact production"
        );
    });
    assert_eq!(
        flow_evaluations_for_tests(),
        count,
        "population preparation does not evaluate the consumer"
    );
    let edited = read_flow_return(&host, "f");
    assert!(
        flow_evaluations_for_tests() > count,
        "changed contributor content must reject the old consumer despite unchanged population"
    );
    let fresh = host_with("export class B { a!: string; }", reader);
    upsert(&fresh, aug, &after);
    fresh.ensure_indexed_ready_serve(aug).unwrap();
    let control = read_flow_return(&fresh, "f");
    assert_eq!(
        (edited.ty, edited.complete, edited.diagnostics),
        (control.ty, control.complete, control.diagnostics)
    );
}

#[test]
fn target_population_tracks_unindexed_membership_add_remove_and_retarget() {
    let aug = "/ws/class-member-evidence/aug.ts";
    let reader = "import { B } from './c'; import './aug'; export function f(b: B) { return b.a; }";
    let matching = "import './c'; declare module './c' { interface B { z: string } } export {};";
    let absent = "import './c'; export {};";
    let retargeted =
        "import './c'; declare module './other' { interface B { z: string } } export {};";
    let renamed = "import './c'; declare module './c' { interface Q { z: string } } export {};";
    for (before, after) in [
        (absent, matching),
        (matching, absent),
        (matching, retargeted),
        (matching, renamed),
    ] {
        let host = host_with("export class B { a!: string; }", reader);
        upsert(&host, aug, before);
        host.ensure_indexed_ready_serve(aug).unwrap();
        let cold = read_flow_return(&host, "f");
        assert!(cold.complete);
        let population = cold.facts.iter().find(|fact| matches!(fact,
            FactVersionRef::RouteSurface(fact) if matches!(&fact.key,
                FactKey::ModuleAugmentationIndexShape { target_kind_tag: verter_session_query::facts::registry::AugmentationTargetKindTag::ResolvedRelativeCanonical,
                    resolved_relative_canonical: Some(target), wildcard_pattern: None, .. } if target.as_ref() == CLASS))).unwrap();
        upsert(&host, aug, after);
        host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
            assert!(!verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates(ctx, population), "target population must reject changed binding membership without caller indexing: {before} -> {after}");
            assert!(!verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates_fact_signature(ctx, verter_session_query::facts::fact_set::FactsRef::Slice(&cold.signature)), "the whole retained receipt must reject changed target population");
        });
        let edited = read_flow_return(&host, "f");
        let fresh = host_with("export class B { a!: string; }", reader);
        upsert(&fresh, aug, after);
        let control = read_flow_return(&fresh, "f");
        assert_eq!(
            (edited.ty, edited.complete, edited.diagnostics),
            (control.ty, control.complete, control.diagnostics)
        );
    }
}

/// A fenced augmenter serve is a VALID complete read for the requesting
/// caller: the augmentation population keeps the contributor — the
/// served artifact's identity while its content is still current — so a
/// member only the augmentation supplies keeps answering mid-fence. The
/// fence turns the enclosing answer non-cacheable (the `FencedServe`
/// note), never memberless; only an unavailable source or superseded
/// served content drops the contributor.
#[test]
fn fenced_augmenter_serve_keeps_its_contribution() {
    let aug = "/ws/class-member-evidence/aug.ts";
    let reader = "import { B } from './c'; import './aug'; export function f(b: B) { return b.z; }";
    let augmenter = "import './c'; declare module './c' { interface B { z: string } } export {};";
    let host = host_with("export class B {}", reader);
    upsert(&host, aug, augmenter);
    host.ensure_indexed_ready_serve(aug).unwrap();
    let cold = read_flow_return(&host, "f");
    assert!(cold.complete, "cold answer must be exact");
    let rctx =
        verter_type_engine::request_context::RequestContext::new(1, Arc::from(READER), false, None);
    let _guard = verter_type_engine::request_context::RequestContextGuard::install(rctx);
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let fenced = read_flow_return(&host, "f");
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        (
            fenced.ty.as_ref(),
            fenced.complete,
            fenced.diagnostics.as_slice()
        ),
        (cold.ty.as_ref(), cold.complete, cold.diagnostics.as_slice()),
        "a fenced augmenter serve must keep its contribution"
    );
    assert!(
        !verter_type_engine::request_context::current_request_result_is_partial(),
        "a fenced serve is non-cacheable, NOT partial"
    );
}

#[test]
fn global_population_retracts_unindexed_final_contributor() {
    let reader = "export function f(b: B) { return b.a; }";
    for canonical in [CLASS, "/ws/class-member-evidence/globals.d.ts"] {
        let host = host_with("", reader);
        upsert(&host, canonical, "declare class B { a: string; }");
        let cold = read_flow_return(&host, "f");
        assert!(cold.complete);
        let population = cold.facts.iter().find(|fact| matches!(fact,
            FactVersionRef::RouteSurface(fact) if matches!(&fact.key,
                FactKey::ModuleAugmentationIndexShape { target_kind_tag, external_specifier, wildcard_pattern, .. }
                    if crate::global_contributors::population_fact_decl_name(*target_kind_tag,
                        external_specifier.as_ref().map(|s| s.as_ref()), wildcard_pattern.as_ref().map(|s| s.as_ref())) == Some("B")))).unwrap();
        upsert(&host, canonical, "export {};");
        host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
            assert!(!verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates(ctx, population), "removing the final contributor must update standalone global population without caller indexing: {canonical}");
            assert!(!verter_type_engine::resolver_core::fact_validation_port::FactValidation::validates_fact_signature(ctx, verter_session_query::facts::fact_set::FactsRef::Slice(&cold.signature)));
        });
        let edited = read_flow_return(&host, "f");
        let fresh = host_with("", reader);
        upsert(&fresh, canonical, "export {};");
        let control = read_flow_return(&fresh, "f");
        assert_eq!(
            (edited.ty, edited.complete, edited.diagnostics),
            (control.ty, control.complete, control.diagnostics)
        );
    }
}

#[test]
fn standalone_analysis_and_structure_ports_observe_their_snapshot() {
    use verter_type_engine::resolver_core::request_ports::{IndexedInputs, OwnedLowering};
    let canonical = "/ws/class-member-evidence/Component.vue";
    let host = host_with("", "");
    upsert(
        &host,
        canonical,
        "<script setup lang='ts'>const a = 1;</script><template><div /></template>",
    );
    host.ensure_indexed_ready_serve(canonical).unwrap();
    let _ = host.get_raw_analysis_snapshot(canonical);
    for analysis in [true, false] {
        host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
            let (present, reads) = verter_type_engine::resolver_core::resolver_context::with_fact_tracer_cell(Default::default(), |_| {
                if analysis { IndexedInputs::get_raw_analysis_snapshot(ctx, canonical).is_some() }
                else { OwnedLowering::ordered_sfc_structure(ctx, canonical).is_some() }
            });
            assert!(present, "actual served control: analysis={analysis}");
            let facts = match reads.finalise() {
                verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(facts) => facts,
                other => panic!("snapshot port refused: {other:?}"),
            };
            assert!(facts.iter().any(|fact| matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id.as_ref() == canonical)), "standalone snapshot port must observe exact served content: analysis={analysis}");
        });
    }
}

#[cfg(feature = "semantic-observe")]
#[test]
fn neutral_augmentation_suffixes_omit_graph_lowering_work() {
    let read = |count: usize, contribution: &str| {
        let mut reader = "import { B } from './c';".to_string();
        let host = host_with("export class B { a!: string; }", "");
        for i in 0..count {
            let name = format!("/ws/class-member-evidence/aug-{i}.ts");
            upsert(&host, &name, &format!("import './c'; declare module './c' {{ interface B {{ {contribution} }} }} export {{}};"));
            // Establish this fixed cold-work population. Dependency-edit
            // validation tests above deliberately do not pre-index edits.
            host.ensure_indexed_ready_serve(&name).unwrap();
            reader.push_str(&format!("import './aug-{i}';"));
        }
        reader.push_str("export function f(b: B) { return b.a; }");
        upsert(&host, READER, &reader);
        host.with_base_resolver_context::<_, verter_audit::observe::Uncaptured>(|ctx| {
            let (_, population) = verter_type_engine::resolver_core::request_ports::OwnedLowering::augmentation_index(ctx,
                verter_session_query::resolution::AugmentationTargetKind::ResolvedRelativeCanonical(Arc::from(CLASS)));
            assert_eq!(population.entries.len(), count, "the fixed work control must exercise every suffix contributor");
        });
        let before = host.project_type_store().semantic_graph().stats_snapshot();
        let cold = read_flow_return_with_mode::<verter_audit::observe::Captured>(&host, "f");
        let after = host.project_type_store().semantic_graph().stats_snapshot();
        let warm = read_flow_return(&host, "f");
        assert!(cold.complete && warm.complete);
        assert_eq!(cold.ty, warm.ty);
        assert_eq!(cold.diagnostics, warm.diagnostics);
        (cold.ty, after.misses - before.misses)
    };
    let baseline = read(0, "");
    let empty_suffix = read(16, "");
    assert_eq!(baseline.0, empty_suffix.0);
    assert!(
        empty_suffix.1 <= baseline.1 + 1,
        "empty suffix graph computations: {} versus {}",
        empty_suffix.1,
        baseline.1
    );
    let affecting = read(16, "later: number;");
    assert!(
        affecting.1 > empty_suffix.1,
        "nonempty suffixes must still be evaluated: {} versus {}",
        affecting.1,
        empty_suffix.1
    );
}

#[test]
fn nonneutral_augmentation_suffixes_keep_content_and_diagnostics() {
    let aug = "/ws/class-member-evidence/aug.ts";
    let reader = "import { B } from './c'; import './aug'; export function f(b: B) { return b.a; }";
    // The first row is the checker's conflicting subsequent-property
    // declaration boundary (TS2717). Generic headers and heritage are also
    // deliberately outside the empty-interface proof.
    for body in [
        "interface B { a: number }",
        "interface B<T> {}",
        "interface Extra { later: number } interface B extends Extra {}",
    ] {
        let source = format!("import './c'; declare module './c' {{ {body} }} export {{}};");
        let make = || {
            let host = host_with("export class B { a!: string; }", reader);
            upsert(&host, aug, &source);
            host.ensure_indexed_ready_serve(aug).unwrap();
            host
        };
        let host = make();
        let cold = read_flow_return(&host, "f");
        assert!(cold.facts.iter().any(|fact| matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id.as_ref() == aug)), "the later body must still be consumed: {body}");
        assert!(
            !cold.facts.iter().any(|fact| is_member_fact(fact, "B", "a")),
            "unproved suffix cannot certify a precise own-member read: {body}"
        );
        let warm = read_flow_return(&host, "f");
        let fresh = read_flow_return(&make(), "f");
        assert_eq!(
            (&cold.ty, cold.complete, &cold.diagnostics),
            (&warm.ty, warm.complete, &warm.diagnostics)
        );
        assert_eq!(
            (warm.ty, warm.complete, warm.diagnostics),
            (fresh.ty, fresh.complete, fresh.diagnostics)
        );
    }
}
