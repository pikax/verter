//! Discriminating guards for the two-sided positive-allowlist admission gate
//! (`docs/arch/u0-oracle-harness-design.md` §Q2 / §5). Each guard ADMITs a
//! genuinely-clean input AND REJECTs the specific construct it names — a guard
//! that cannot fail against the wrong behaviour is a stub, so every assertion
//! discriminates.

use std::sync::Arc;

use verter_type_expr::{MemberVisibility, TypeExpr};

use super::*;
use crate::typeinfo::typeinfo_tests::oracle::normalize::ProjectionModeKind;

const SHALLOW: ProjectionModeKind = ProjectionModeKind::Shallow;

/// Lower a type-text RHS to a `TypeExpr` for synthetic source bodies.
fn lower(rhs: &str) -> TypeExpr {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::Statement;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    let allocator = Allocator::default();
    let src = format!("type __T = {rhs};");
    let ret = Parser::new(&allocator, &src, SourceType::ts()).parse();
    assert!(!ret.panicked, "parse failed for `{rhs}`");
    let alias = ret
        .program
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Statement::TSTypeAliasDeclaration(a) if a.id.name == "__T" => Some(&a.type_annotation),
            _ => None,
        })
        .expect("missing __T alias");
    verter_type_expr_oxc::lower_ts_type(alias, &src)
}

/// An all-clean (admissible) `RawSourceSurface`. Each reject guard mutates ONE
/// field so the discrimination is attributable.
fn clean_surface() -> RawSourceSurface {
    RawSourceSurface {
        decl_canonical: "/fixtures/clean.ts".to_string(),
        decl_kind: RawDeclKind::TypeAlias,
        raw_member_keys: vec![RawKey::Static("a".to_string())],
        member_kinds: vec![RawMemberKind::Property],
        member_visibility: vec![MemberVisibility::Public],
        unique_symbol_ops: vec![],
        abstract_ctor: false,
        type_param_modifiers: vec![],
        this_type_or_param: false,
        value_const_assertion: None,
        overload_signatures: vec![],
        utility_referent_names: vec![],
        tuple_element_shape: vec![TupleElementShape::Plain],
        transitive_referents: vec![],
    }
}

fn bare_ref(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: name.into(),
        type_arguments: Arc::from(Vec::new()),
    }
}

fn contributor(raw: RawSourceSurface, body: TypeExpr) -> SourceContributor {
    SourceContributor {
        ordinal: 0,
        raw_surface: raw,
        lowered_body: body,
    }
}

// ---------------------------------------------------------------------------
// §Q2 hover-side positive allowlist (default-REJECT)
// ---------------------------------------------------------------------------

#[test]
fn hover_construct_whitelist() {
    // ADMIT: every construct on the closed positive list.
    for ok in [
        "string",
        "number",
        "boolean",
        "\"a\"",
        "42",
        "true",
        "{ a: number; b: string }",
        "{ [k: string]: number }",
        "string[]",
        "readonly number[]",
        "[string, number]",
        "string | number",
        "{ a: number } & { b: string }",
        "Foo",
        "Promise<string>",
    ] {
        assert_eq!(
            admit_hover_text(ok),
            AdmissionVerdict::Admit,
            "expected ADMIT for `{ok}`"
        );
    }

    // REJECT: each non-allowlisted construct, asserting the EXACT reason.
    let cases: &[(&str, RejectReason)] = &[
        ("unique symbol", RejectReason::UniqueSymbol),
        ("{ x: unique symbol }", RejectReason::UniqueSymbol),
        ("() => void", RejectReason::Callable),
        ("new () => Foo", RejectReason::Callable),
        ("{ m(): void }", RejectReason::Callable),
        ("{ (): void }", RejectReason::Callable),
        ("Color.Red", RejectReason::EnumMemberOrQualified),
        ("keyof T", RejectReason::DeferredConstruct("keyof")),
        ("typeof x", RejectReason::DeferredConstruct("typeof")),
        (
            "T[\"a\"]",
            RejectReason::DeferredConstruct("indexed-access"),
        ),
        (
            "A extends B ? C : D",
            RejectReason::DeferredConstruct("conditional"),
        ),
        (
            "{ [K in keyof T]: T[K] }",
            RejectReason::DeferredConstruct("mapped"),
        ),
        (
            "`a${string}`",
            RejectReason::DeferredConstruct("template-literal"),
        ),
        ("any", RejectReason::AnyKeyword),
        ("never", RejectReason::NeverKeyword),
        ("this", RejectReason::ThisTypeOrParam),
    ];
    for (text, want) in cases {
        assert_eq!(
            admit_hover_text(text),
            AdmissionVerdict::Reject(want.clone()),
            "expected REJECT({want:?}) for `{text}`"
        );
    }
}

#[test]
fn tuple_optional_labelled_rejected() {
    assert_eq!(
        admit_hover_text("[string, number]"),
        AdmissionVerdict::Admit
    );
    assert_eq!(
        admit_hover_text("[string, number?]"),
        AdmissionVerdict::Reject(RejectReason::TupleElementShape)
    );
    assert_eq!(
        admit_hover_text("[label: string]"),
        AdmissionVerdict::Reject(RejectReason::TupleElementShape)
    );
    assert_eq!(
        admit_hover_text("[...string[]]"),
        AdmissionVerdict::Reject(RejectReason::TupleElementShape)
    );
}

// ---------------------------------------------------------------------------
// §Q2 pre-lowering soundness: a check AFTER lowering is unsound (OXC has
// already erased the lossy construct). This proves the gate must run on the RAW
// AST.
// ---------------------------------------------------------------------------

#[test]
fn pre_lowering_loss_rejected() {
    // `{ x: unique symbol }` lowers (OXC drops `unique`, oxc/lib.rs:171) to a
    // clean-LOOKING `{ x: symbol }` — so a POST-lowering check would ADMIT it.
    let lowered = lower("{ x: unique symbol }");
    assert_eq!(
        admit_type_expr(&lowered),
        AdmissionVerdict::Admit,
        "post-lowering the brand is gone — the unsound check would admit"
    );
    // The PRE-lowering hover walk catches it.
    assert_eq!(
        admit_hover_text("{ x: unique symbol }"),
        AdmissionVerdict::Reject(RejectReason::UniqueSymbol),
        "the pre-lowering walk must reject what lowering erases"
    );
}

#[test]
fn strict_lowering_drop_counter() {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::Statement;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn drops(rhs: &str) -> usize {
        let allocator = Allocator::default();
        let src = format!("type __T = {rhs};");
        let ret = Parser::new(&allocator, &src, SourceType::ts()).parse();
        assert!(!ret.panicked, "parse failed for `{rhs}`");
        let alias = ret
            .program
            .body
            .iter()
            .find_map(|stmt| match stmt {
                Statement::TSTypeAliasDeclaration(a) if a.id.name == "__T" => {
                    Some(&a.type_annotation)
                }
                _ => None,
            })
            .expect("missing __T alias");
        super::lower_with_drop_count(alias, &src).1
    }

    // A clean object drops nothing.
    assert_eq!(drops("{ a: number; b: string }"), 0);
    // A computed-key member is silently elided by OXC (oxc/lib.rs:99,921) — the
    // drop-counter sees it as a non-zero drop.
    assert_eq!(drops("{ [x]: number }"), 1);
    assert_eq!(drops("{ a: number; [x]: string }"), 1);
}

// ---------------------------------------------------------------------------
// §Q2 source-side admission (the COMBINED raw-fact + lowered-body walk)
// ---------------------------------------------------------------------------

#[test]
fn source_declaration_allowlist_clean() {
    // A clean contributor (clean raw facts + a clean lowered body) ADMITs.
    let clean = contributor(clean_surface(), lower("{ a: number }"));
    assert_eq!(admit_source_contributor(&clean), AdmissionVerdict::Admit);

    // A reject in the LOWERED BODY (the non-erased half) — a conditional.
    let cond_body = contributor(clean_surface(), lower("A extends B ? C : D"));
    assert_eq!(
        admit_source_contributor(&cond_body),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("conditional"))
    );

    // A reject in the RAW FACTS (the erased half) — a unique-symbol op the
    // lowered body would have lost.
    let mut raw = clean_surface();
    raw.unique_symbol_ops.push(UniqueSymbolOp);
    let raw_reject = contributor(raw, lower("{ a: number }"));
    assert_eq!(
        admit_source_contributor(&raw_reject),
        AdmissionVerdict::Reject(RejectReason::UniqueSymbol)
    );

    // Multi-contributor: one clean peer does NOT admit the merge if another
    // carries a reject construct.
    let walk = SourceWalkResult::Resolved {
        contributors: vec![contributor(clean_surface(), lower("{ a: number }")), {
            let mut r = clean_surface();
            r.abstract_ctor = true;
            contributor(r, lower("{ b: string }"))
        }],
    };
    assert_eq!(
        admit_source_walk(&walk),
        AdmissionVerdict::Reject(RejectReason::AbstractCtor)
    );
}

#[test]
fn class_visibility_accessor_rejected() {
    let mut priv_vis = clean_surface();
    priv_vis.member_visibility = vec![MemberVisibility::Private];
    assert_eq!(
        admit_raw_surface(&priv_vis),
        AdmissionVerdict::Reject(RejectReason::NonPublicVisibility)
    );

    let mut protected = clean_surface();
    protected.member_visibility = vec![MemberVisibility::Protected];
    assert_eq!(
        admit_raw_surface(&protected),
        AdmissionVerdict::Reject(RejectReason::NonPublicVisibility)
    );

    let mut getter = clean_surface();
    getter.member_kinds = vec![RawMemberKind::Getter];
    assert_eq!(
        admit_raw_surface(&getter),
        AdmissionVerdict::Reject(RejectReason::Accessor)
    );

    let mut setter = clean_surface();
    setter.member_kinds = vec![RawMemberKind::Setter];
    assert_eq!(
        admit_raw_surface(&setter),
        AdmissionVerdict::Reject(RejectReason::Accessor)
    );

    // Each remaining erased fact rejects with its own reason.
    let mut overload = clean_surface();
    overload.overload_signatures = vec![OverloadSignature, OverloadSignature];
    assert_eq!(
        admit_raw_surface(&overload),
        AdmissionVerdict::Reject(RejectReason::Callable)
    );

    let mut tp = clean_surface();
    tp.type_param_modifiers = vec![TypeParamModifiers {
        is_const: true,
        ..Default::default()
    }];
    assert_eq!(
        admit_raw_surface(&tp),
        AdmissionVerdict::Reject(RejectReason::TypeParamModifier)
    );

    let mut this_p = clean_surface();
    this_p.this_type_or_param = true;
    assert_eq!(
        admit_raw_surface(&this_p),
        AdmissionVerdict::Reject(RejectReason::ThisTypeOrParam)
    );

    let mut const_assert = clean_surface();
    const_assert.value_const_assertion = Some(true);
    assert_eq!(
        admit_raw_surface(&const_assert),
        AdmissionVerdict::Reject(RejectReason::ConstAssertion)
    );

    let mut computed = clean_surface();
    computed.raw_member_keys = vec![RawKey::Computed];
    assert_eq!(
        admit_raw_surface(&computed),
        AdmissionVerdict::Reject(RejectReason::NonStaticKey)
    );

    // The all-clean surface ADMITs (negative control).
    assert_eq!(admit_raw_surface(&clean_surface()), AdmissionVerdict::Admit);
}

#[test]
fn source_walk_unresolved_or_cycle_rejected() {
    assert_eq!(
        admit_source_walk(&SourceWalkResult::Unresolved),
        AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic)
    );
    assert_eq!(
        admit_source_walk(&SourceWalkResult::Cycle),
        AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic)
    );
    assert_eq!(
        admit_source_walk(&SourceWalkResult::Resolved {
            contributors: vec![]
        }),
        AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic)
    );
}

// ---------------------------------------------------------------------------
// §Q2 lowered-body backstop: any / never / Unknown over a TypeExpr.
// ---------------------------------------------------------------------------

#[test]
fn type_expr_backstop_rejects_any_never_unknown() {
    use verter_type_expr::PrimitiveName;
    assert_eq!(
        admit_type_expr(&TypeExpr::Primitive(PrimitiveName::Any)),
        AdmissionVerdict::Reject(RejectReason::AnyKeyword)
    );
    assert_eq!(
        admit_type_expr(&TypeExpr::Primitive(PrimitiveName::Never)),
        AdmissionVerdict::Reject(RejectReason::NeverKeyword)
    );
    assert_eq!(
        admit_type_expr(&TypeExpr::Unknown {
            raw: "??".to_string()
        }),
        AdmissionVerdict::Reject(RejectReason::UnknownOrParseLeftover)
    );
    // A lowered object carrying a callable (method) member rejects as an
    // overload-group surface — exercising `admit_type_expr`'s object branch.
    assert_eq!(
        admit_type_expr(&lower("{ m(): void }")),
        AdmissionVerdict::Reject(RejectReason::Callable)
    );
    // A clean lowered object ADMITs (negative control).
    assert_eq!(
        admit_type_expr(&lower("{ a: number }")),
        AdmissionVerdict::Admit
    );
}

// ---------------------------------------------------------------------------
// §Q2 two-sided combiner + backstops + shallow-expansion fence.
// ---------------------------------------------------------------------------

#[test]
fn hover_capture_is_lossless_or_rejected() {
    let clean_source = SourceWalkResult::Resolved {
        contributors: vec![contributor(clean_surface(), lower("{ a: number }"))],
    };

    // Both sides clean → Admit.
    assert_eq!(
        admit_query("{ a: number }", &clean_source, SHALLOW),
        AdmissionVerdict::Admit
    );

    // A truncation marker in the hover text → Reject (backstop 1), checked
    // before parsing.
    assert_eq!(
        admit_query("{ a: number; ... }", &clean_source, SHALLOW),
        AdmissionVerdict::Reject(RejectReason::TruncationMarker)
    );
    assert_eq!(
        admit_query("{ a: \u{2026} }", &clean_source, SHALLOW),
        AdmissionVerdict::Reject(RejectReason::TruncationMarker)
    );

    // A reject construct on the HOVER side fails the whole query even though the
    // source is clean.
    assert_eq!(
        admit_query("() => void", &clean_source, SHALLOW),
        AdmissionVerdict::Reject(RejectReason::Callable)
    );

    // A reject construct on the SOURCE side fails even though the hover is clean.
    let bad_source = SourceWalkResult::Resolved {
        contributors: vec![contributor(clean_surface(), lower("keyof T"))],
    };
    assert_eq!(
        admit_query("{ a: number }", &bad_source, SHALLOW),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
    );
}

#[test]
fn shallow_hover_expansion_rejected() {
    // Source kept the queried symbol as a bare userland `Ref`.
    let ref_source = SourceWalkResult::Resolved {
        contributors: vec![contributor(clean_surface(), bare_ref("Foo"))],
    };

    // Hover re-printed the alias NAME → Admit (the correct shallow surface).
    assert_eq!(
        admit_query("Foo", &ref_source, SHALLOW),
        AdmissionVerdict::Admit
    );

    // Hover EXPANDED the alias to its object body instead → display artefact,
    // REJECT.
    assert_eq!(
        admit_query("{ a: number }", &ref_source, SHALLOW),
        AdmissionVerdict::Reject(RejectReason::ShallowHoverExpansion)
    );
}
