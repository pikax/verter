//! Discriminating regression tests for the typed-form sidecar contract.
//!
//! These tests pin the contract that every `ResolvedProp` and
//! `ResolvedEmit` whose annotation is observable via OXC has a populated
//! `type_expr` (the lowered annotation) AND a paired `type_expr_scope`
//! (the canonical_id of the file whose parse produced the typed
//! expression). A regression that drops `type_expr` population (so it
//! defaults to `None` because no producer populates it) must trip
//! `assert_typed_form_populated()` immediately.
//!
//! Coverage matrix:
//! 1. Local-SFC parse path: bare alias `Ref` preserved (shallow-by-default).
//! 2. External resolution path: bare alias `Ref` preserved with the
//!    sibling file's canonical_id stamped as scope.
//! 3. External resolution path: synthesized member `a: X['a']` lowered
//!    as `IndexedAccess { object: Ref { name: "X" }, index: Literal("a") }`.
//! 4. Parser-boundary invariant panic: artificially zeroing `type_expr`
//!    on a populated `ResolvedElements` makes
//!    `assert_typed_form_populated()` report the violation.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::{LiteralValue, TypeExpr, TypeExprScope};

use super::resolve_type::{
    build_type_context, resolve_external_type_with_canonical, resolve_type_elements_with_ctx_ref,
    ResolvedElements, ResolvedEmitSignature,
};

/// Parse an SFC-script-style fragment, build a `TypeResolutionContext`
/// bound to the supplied owner canonical_id, locate the named type alias,
/// and resolve its annotation. The result is the boundary-stamped
/// `ResolvedElements` produced by `resolve_type_elements_with_ctx_ref`.
fn resolve_local_typed(source: &str, owner_canonical: &str) -> ResolvedElements {
    let allocator = Allocator::default();
    let parser = Parser::new(&allocator, source, SourceType::ts());
    let parsed = parser.parse();
    assert!(
        parsed.errors.is_empty(),
        "Source must parse cleanly: {:?}",
        parsed.errors
    );
    let mut ctx = build_type_context(&parsed.program, source.as_bytes(), 0);
    ctx.set_owner_canonical(owner_canonical);
    for stmt in &parsed.program.body {
        if let oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "ResolveTarget" {
                return resolve_type_elements_with_ctx_ref(&alias.type_annotation, 0, &ctx);
            }
        }
    }
    panic!("expected `type ResolveTarget = ...` declaration in source");
}

/// W0.3 / test #1 — local-SFC bare alias `Ref` preserved.
///
/// `defineProps<{ foo: SomeAlias }>()` with local `type SomeAlias = "a" | "b"`.
/// The published `type_expr` for `foo` must be `Ref { name: "SomeAlias", type_arguments: [] }`,
/// NOT the union body. Shallow-by-default holds — alias bodies stay
/// behind the `Ref`.
#[test]
fn local_path_bare_alias_ref_is_preserved_shallow() {
    let source = r#"
        type SomeAlias = "a" | "b";
        type ResolveTarget = { foo: SomeAlias };
    "#;
    let owner = "/virtual/local-owner.vue";
    let resolved = resolve_local_typed(source, owner);

    assert_eq!(
        resolved.props.len(),
        1,
        "expected exactly one prop, got {:?}",
        resolved.props
    );
    let foo = &resolved.props[0];
    assert_eq!(foo.key_name.as_deref(), Some("foo"));

    // Display string survives unchanged.
    assert_eq!(foo.type_text.as_deref(), Some("SomeAlias"));

    // Typed sidecar is the syntactic shape (a `Ref`, not the union body).
    let type_expr = foo
        .type_expr
        .as_ref()
        .expect("ResolvedProp.type_expr populated by parser at construction");
    match type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "SomeAlias", "alias root identity preserved");
            assert!(
                type_arguments.is_empty(),
                "no type arguments on the bare alias reference, got {:?}",
                type_arguments
            );
        }
        other => panic!(
            "expected TypeExpr::Ref preserving the alias root identity, got {:?}",
            other
        ),
    }

    // Negative assertion — the projector pipeline relies on this NOT being
    // pre-expanded into the union body. If the producer ever inlines, the
    // shallow-by-default contract breaks.
    assert!(
        !matches!(type_expr, TypeExpr::Union(_)),
        "type_expr must NOT be the inlined union body — projector relies on the Ref being preserved"
    );

    // Paired scope stamps the local SFC's canonical_id at the parser
    // boundary.
    assert_eq!(
        foo.type_expr_scope.as_ref(),
        Some(&TypeExprScope::new(owner)),
        "type_expr_scope must be stamped with the local owner_canonical at the parser boundary"
    );
}

/// W0.3 / test #2 — external-resolution bare alias `Ref` preserved.
///
/// `defineProps<{ foo: ImportedAlias }>()` from a sibling file that
/// declares `type ImportedAlias = "a" | "b"`. The external producer
/// lowers from the sibling file's parse arena and stamps the sibling's
/// canonical_id as scope. The union body is NOT inlined.
#[test]
fn external_path_bare_alias_ref_is_preserved_with_external_scope() {
    let dep = r#"
        type ImportedAlias = "a" | "b";
        export interface Props {
            foo: ImportedAlias;
        }
    "#;
    let alloc = Allocator::default();
    let external_canonical = "/virtual/sibling-types.ts";
    let resolved = resolve_external_type_with_canonical("Props", dep, &alloc, external_canonical)
        .expect("Props should resolve from sibling source");

    assert_eq!(resolved.props.len(), 1);
    let foo = &resolved.props[0];
    assert_eq!(foo.key_name.as_deref(), Some("foo"));

    let type_expr = foo
        .type_expr
        .as_ref()
        .expect("external producer must populate ResolvedProp.type_expr");
    match type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "ImportedAlias",
                "external alias root identity preserved"
            );
            assert!(
                type_arguments.is_empty(),
                "no type arguments on the bare alias reference"
            );
        }
        other => panic!(
            "expected TypeExpr::Ref preserving the imported alias root identity, got {:?}",
            other
        ),
    }
    assert!(
        !matches!(type_expr, TypeExpr::Union(_)),
        "external path must not inline the union body — it stays behind the Ref"
    );

    // Critical: scope is the EXTERNAL file's canonical_id, NOT the
    // consuming SFC. Consumers that resolve `ImportedAlias` must consult
    // the external file's symbol table.
    assert_eq!(
        foo.type_expr_scope.as_ref(),
        Some(&TypeExprScope::new(external_canonical)),
        "type_expr_scope must be the EXTERNAL file's canonical_id, not the consuming SFC"
    );
}

/// W0.3 / test #3 — external-resolution `IndexedAccess` member surfacing.
///
/// When an external file declares `interface Props { a: X['a'] }`,
/// the lowered `type_expr` for `a` must be
/// `IndexedAccess { object: Ref { name: "X" }, index: Literal("a") }`.
/// Still shallow — no expansion of `X['a']` into the underlying type.
#[test]
fn external_path_indexed_access_member_lowers_shallow() {
    let dep = r#"
        interface X {
            a: string;
            b: number;
        }
        export interface Props {
            a: X['a'];
        }
    "#;
    let alloc = Allocator::default();
    let external_canonical = "/virtual/sibling-indexed-access.ts";
    let resolved = resolve_external_type_with_canonical("Props", dep, &alloc, external_canonical)
        .expect("Props with X['a'] member should resolve");

    assert_eq!(resolved.props.len(), 1, "props: {:?}", resolved.props);
    let a = &resolved.props[0];
    assert_eq!(a.key_name.as_deref(), Some("a"));

    let type_expr = a
        .type_expr
        .as_ref()
        .expect("indexed-access annotation must lower to a populated type_expr");
    match type_expr {
        TypeExpr::IndexedAccess { object, index } => {
            match object.as_ref() {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    assert_eq!(name.as_ref(), "X");
                    assert!(type_arguments.is_empty());
                }
                other => panic!(
                    "expected indexed-access object to be Ref {{ name: \"X\" }}, got {:?}",
                    other
                ),
            }
            match index.as_ref() {
                TypeExpr::Literal(LiteralValue::String(s)) => {
                    assert_eq!(s, "a");
                }
                other => panic!(
                    "expected indexed-access index to be string literal \"a\", got {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "expected TypeExpr::IndexedAccess for `X['a']` annotation, got {:?}",
            other
        ),
    }

    assert_eq!(
        a.type_expr_scope.as_ref(),
        Some(&TypeExprScope::new(external_canonical)),
        "scope must be the external file's canonical_id"
    );
}

/// W0.3 / test #4 — parser-boundary invariant assertion discriminates.
///
/// Construct a real `ResolvedElements`, then artificially zero out
/// `type_expr` on the prop while leaving `type_expr_scope` populated.
/// `assert_typed_form_populated` must report the pairing violation by
/// exact message shape, proving the invariant discriminates rather than
/// rubber-stamping.
#[test]
fn parser_boundary_invariant_reports_pairing_violation() {
    let source = r#"
        type ResolveTarget = { foo: string };
    "#;
    let owner = "/virtual/invariant-discrimination.vue";
    let mut resolved = resolve_local_typed(source, owner);

    // Sanity: post-W0.3, resolved is in the all-populated state.
    resolved
        .assert_typed_form_populated()
        .expect("freshly resolved elements must satisfy the typed-form pairing invariant");
    assert!(resolved.props[0].type_expr.is_some());
    assert!(resolved.props[0].type_expr_scope.is_some());

    // Now break the pairing: drop type_expr, keep scope. This is exactly
    // the failure mode the invariant catches.
    let original_scope = resolved.props[0].type_expr_scope.clone();
    resolved.props[0].type_expr = None;
    let err = resolved
        .assert_typed_form_populated()
        .expect_err("artificially zeroed type_expr must trigger the pairing invariant");
    assert!(
        err.contains("foo"),
        "violation message must name the offending prop, got: {err}"
    );
    assert!(
        err.contains("pairing violated"),
        "violation message must mention the pairing rule, got: {err}"
    );

    // Restore type_expr, drop scope — the mirror failure case.
    resolved.props[0].type_expr =
        Some(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
    resolved.props[0].type_expr_scope = None;
    let err = resolved
        .assert_typed_form_populated()
        .expect_err("artificially zeroed scope must trigger the pairing invariant");
    assert!(err.contains("foo"));
    assert!(err.contains("pairing violated"));

    // Restore both and also drop the type_text/type_span annotation. With
    // both fields cleared and type_expr None, the invariant should pass.
    resolved.props[0].type_expr = None;
    resolved.props[0].type_expr_scope = original_scope.clone();
    // Even with scope but no type_expr, it's a violation.
    assert!(resolved.assert_typed_form_populated().is_err());

    // Set both to None — clean state without typed information.
    resolved.props[0].type_expr_scope = None;
    resolved.props[0].type_text = None;
    resolved.props[0].type_span = None;
    resolved
        .assert_typed_form_populated()
        .expect("clean (both-None, no annotation) state must satisfy the invariant");
}

/// W0.3 supplementary — emit pairing invariant.
///
/// Tuple-style emit `{ change: [id: number] }` lowers to a
/// `TypeExpr::Tuple` payload with the local SFC's scope.
#[test]
fn emit_tuple_payload_lowers_with_paired_scope() {
    let source = r#"
        type ResolveTarget = { change: [id: number] };
    "#;
    let owner = "/virtual/emit-tuple.vue";
    let resolved = resolve_local_typed(source, owner);

    assert_eq!(resolved.emits.len(), 1);
    let change = &resolved.emits[0];
    assert_eq!(change.name, "change");
    matches!(change.signature, ResolvedEmitSignature::Tuple { .. });

    let type_expr = change
        .type_expr
        .as_ref()
        .expect("tuple emit payload must lower to a populated type_expr");
    assert!(
        matches!(type_expr, TypeExpr::Tuple { .. }),
        "expected TypeExpr::Tuple for `[id: number]` payload, got {:?}",
        type_expr
    );
    assert_eq!(
        change.type_expr_scope.as_ref(),
        Some(&TypeExprScope::new(owner))
    );
}
