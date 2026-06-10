//! Discriminating guards for the two-sided positive-allowlist admission gate
//! (`docs/arch/u0-oracle-harness-design.md` §Q2 / §5). Each guard ADMITs a
//! genuinely-clean input AND REJECTs the specific construct it names — a guard
//! that cannot fail against the wrong behaviour is a stub, so every assertion
//! discriminates.

use std::sync::Arc;

use verter_type_expr::{MemberVisibility, TypeExpr};

use super::*;
// The raw-fact constructor types these guards build synthetic surfaces from. The
// parent `admission` module imports only the raw-fact types its (non-test)
// predicate references; these four are exercised only by the guards, so they are
// imported here at the consumer rather than left as an unused parent import in
// the non-test (`oracle-gen`) build.
use crate::typeinfo::oracle_core::normalize::ProjectionModeKind;
use verter_compiler::utils::oxc::vue::raw_surface::{
    OverloadSignature, RawDeclKind, TypeParamModifiers, UniqueSymbolOp,
};

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

/// A contributor whose body is NOT a carve-out shape (or whose carve-out root is
/// left UNBOUND). Used for every non-carve-out guard — `carve_out_root_def` is
/// `None`, which for a carve-out shape means "root did not bind same-file" and
/// for a non-carve-out shape is simply unused.
fn contributor(raw: RawSourceSurface, body: TypeExpr) -> SourceContributor {
    SourceContributor {
        ordinal: 0,
        def_canonical: "/fixtures/clean.ts".to_string(),
        raw_surface: raw,
        lowered_body: body,
        carve_out_root_def: None,
        carve_out_root_surfaces: Vec::new(),
    }
}

/// A carve-out contributor whose root `Ref` is stamped as resolving to
/// `root_def` — the source walk's same-file binding — with a CLEAN root
/// raw-fact record (the root-operand admission's positive case). Passing the
/// SAME file as `def_canonical` models a provably-same-file root (admitted); a
/// DIFFERENT file models an imported / cross-file root (rejected).
fn carve_out_contributor(
    raw: RawSourceSurface,
    body: TypeExpr,
    def_canonical: &str,
    root_def: Option<&str>,
) -> SourceContributor {
    carve_out_contributor_with_root_surfaces(
        raw,
        body,
        def_canonical,
        root_def,
        vec![clean_surface()],
    )
}

/// [`carve_out_contributor`] with an explicit ROOT raw-fact contributor vector —
/// the root-operand raw-fact admission guards drive the dirty / empty cases
/// through this.
fn carve_out_contributor_with_root_surfaces(
    raw: RawSourceSurface,
    body: TypeExpr,
    def_canonical: &str,
    root_def: Option<&str>,
    root_surfaces: Vec<RawSourceSurface>,
) -> SourceContributor {
    SourceContributor {
        ordinal: 0,
        def_canonical: def_canonical.to_string(),
        raw_surface: raw,
        lowered_body: body,
        carve_out_root_def: root_def.map(str::to_string),
        carve_out_root_surfaces: root_surfaces,
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
    // `keyof <bare ref>` is now a source-ROOT carve-out shape (admitted), so the
    // genuinely-rejected source fixture here is a CONDITIONAL — never a carve-out
    // root, still rejected by the generic predicate.
    let bad_source = SourceWalkResult::Resolved {
        contributors: vec![contributor(clean_surface(), lower("A extends B ? C : D"))],
    };
    assert_eq!(
        admit_query("{ a: number }", &bad_source, SHALLOW),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("conditional"))
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

// ---------------------------------------------------------------------------
// Source-ROOT carve-out (the operator-reduction admission rule). The carve-out
// ADMITS exactly two operator-bodied ROOT shapes, and ONLY at the source-body
// root; the generic `admit_type_expr`, the hover side, and the oracle-VALUE side
// STILL reject `keyof` / indexed-access everywhere else (incl. every nested
// position). Each guard discriminates: it ADMITs a genuine carve-out shape AND
// REJECTs the specific still-rejected construct it names.
// ---------------------------------------------------------------------------

#[test]
fn source_root_carve_out_admits_the_two_shapes() {
    // The same-file fixture both the declaration AND its carve-out root live in.
    let same: &str = "/fixtures/keys.ts";
    // A carve-out contributor whose root binds SAME-FILE.
    let admit_root = |rhs: &str| {
        admit_source_root(&carve_out_contributor(
            clean_surface(),
            lower(rhs),
            same,
            Some(same),
        ))
    };

    // Shape 1: `keyof Root` (Root a bare same-file ref, empty args).
    assert_eq!(
        classify_source_root(&lower("keyof KeySource")),
        SourceRootShape::KeyofBareRef
    );
    assert_eq!(admit_root("keyof KeySource"), AdmissionVerdict::Admit);

    // Shape 2: `Root["a"]["b"]…` (every segment a string literal, bare-ref base).
    assert_eq!(
        classify_source_root(&lower("KeySource[\"nested\"][\"value\"]")),
        SourceRootShape::StringLiteralIndexChain
    );
    assert_eq!(
        admit_root("KeySource[\"nested\"][\"value\"]"),
        AdmissionVerdict::Admit
    );
    // Single-hop shape 2 is also admitted.
    assert_eq!(admit_root("KeySource[\"nested\"]"), AdmissionVerdict::Admit);

    // The carve-out admits via the SOURCE-CONTRIBUTOR walk too (the path the
    // generator's two-sided combiner takes): a clean raw surface + a carve-out
    // root body whose root binds same-file ADMITs the contributor.
    let keyof_contrib =
        carve_out_contributor(clean_surface(), lower("keyof KeySource"), same, Some(same));
    assert_eq!(
        admit_source_contributor(&keyof_contrib),
        AdmissionVerdict::Admit
    );
    let chain_contrib = carve_out_contributor(
        clean_surface(),
        lower("KeySource[\"nested\"][\"value\"]"),
        same,
        Some(same),
    );
    assert_eq!(
        admit_source_contributor(&chain_contrib),
        AdmissionVerdict::Admit
    );
}

#[test]
fn source_root_carve_out_rejects_imported_or_unresolved_root() {
    // The carve-out enforces SAME-FILE root identity, NOT just structure.
    // A structurally-valid `keyof Imported` / `Imported["x"]` whose root resolves
    // to ANOTHER file (an import) is NOT a same-file carve-out and must REJECT
    // through the generic predicate (which rejects the bare operator). A root that
    // did not bind at all (`None` stamp) likewise rejects.
    //
    // DISCRIMINATING: a carve-out that checked STRUCTURE ONLY — admitting the
    // shape without resolving the root — would wrongly ADMIT these. The
    // same-file identity check is what rejects them.
    let decl_file = "/fixtures/consumer.ts";
    let other_file = "/fixtures/leaf.ts";

    // (a) `keyof Imported` — root resolves to a DIFFERENT file → reject as keyof.
    let keyof_imported = carve_out_contributor(
        clean_surface(),
        lower("keyof Imported"),
        decl_file,
        Some(other_file),
    );
    assert_eq!(
        admit_source_root(&keyof_imported),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof")),
        "an imported `keyof` root is not a same-file carve-out"
    );
    assert_eq!(
        admit_source_contributor(&keyof_imported),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
    );

    // (b) `Imported["x"]` — root resolves to a DIFFERENT file → reject as
    //     indexed-access.
    let indexed_imported = carve_out_contributor(
        clean_surface(),
        lower("Imported[\"x\"]"),
        decl_file,
        Some(other_file),
    );
    assert_eq!(
        admit_source_root(&indexed_imported),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access")),
        "an imported indexed-access root is not a same-file carve-out"
    );

    // (c) Root did NOT bind (`None` stamp) → reject, even with a carve-out shape.
    let unbound = carve_out_contributor(clean_surface(), lower("keyof Imported"), decl_file, None);
    assert_eq!(
        admit_source_root(&unbound),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof")),
        "an unresolved carve-out root rejects"
    );

    // (d) Negative control: the SAME body with a SAME-FILE root ADMITs — proving
    //     the reject in (a)/(c) is attributable to the cross-file/unbound root,
    //     not to the shape.
    let keyof_same = carve_out_contributor(
        clean_surface(),
        lower("keyof Imported"),
        decl_file,
        Some(decl_file),
    );
    assert_eq!(admit_source_root(&keyof_same), AdmissionVerdict::Admit);
}

#[test]
fn source_root_carve_out_rejects_every_still_rejected_shape() {
    // The carve-out NEVER admits these; each defers to the generic predicate and
    // rejects with its specific reason.
    let cases: &[(&str, RejectReason)] = &[
        // Numeric index key — not a string literal.
        (
            "KeySource[42]",
            RejectReason::DeferredConstruct("indexed-access"),
        ),
        // `symbol` index key — not a string literal.
        (
            "KeySource[symbol]",
            RejectReason::DeferredConstruct("indexed-access"),
        ),
        // Union index key — not a single string literal.
        (
            "KeySource[\"a\" | \"b\"]",
            RejectReason::DeferredConstruct("indexed-access"),
        ),
        // NOTE: `Root[keyof Root]` (same shared root) is the KeyofSelfIndex
        // carve-out, owned by `keyof_self_index_source_root_carve_out`; a
        // DIFFERENT-root lookup stays a plain deferred operator.
        (
            "KeySource[keyof OtherSource]",
            RejectReason::DeferredConstruct("indexed-access"),
        ),
        // A type-argument-carrying root on the object side (`NonNullable<…>[…]`)
        // — the base is not a bare empty-args ref.
        (
            "NonNullable<KeySource>[\"value\"]",
            RejectReason::DeferredConstruct("indexed-access"),
        ),
        // A nested operator body (`{ x: keyof T }`) is NOT a carve-out root; the
        // generic predicate walks the object and rejects the nested `keyof`.
        (
            "{ x: keyof KeySource }",
            RejectReason::DeferredConstruct("keyof"),
        ),
        // `keyof Root<Arg>` — a type-arg-carrying keyof operand is not the bare
        // ref shape 1 requires; falls through to the keyof reject.
        (
            "keyof Foo<string>",
            RejectReason::DeferredConstruct("keyof"),
        ),
        // Other deferred operator roots stay rejected verbatim.
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
    ];
    for (text, want) in cases {
        assert_eq!(
            classify_source_root(&lower(text)),
            SourceRootShape::NotCarveOut,
            "`{text}` must NOT be a carve-out root"
        );
        // A NotCarveOut body rejects through the generic predicate regardless of
        // any root stamp; wrap it in a plain (no-root) contributor.
        assert_eq!(
            admit_source_root(&contributor(clean_surface(), lower(text))),
            AdmissionVerdict::Reject(want.clone()),
            "expected REJECT({want:?}) for source root `{text}`"
        );
    }
}

#[test]
fn source_root_typeof_and_infer_bodies_rejected() {
    // The source-root reject matrix must also pin `typeof` and `Infer` bodies:
    // neither is a carve-out shape, and each must reject with its EXACT reason
    // through the generic predicate. Without these the guard would not
    // discriminate against a future over-admission of either construct at the
    // source root.

    // `typeof x` lowers to `TypeExpr::TypeOf` — NotCarveOut, rejected as typeof.
    let typeof_body = lower("typeof x");
    assert_eq!(
        classify_source_root(&typeof_body),
        SourceRootShape::NotCarveOut,
        "`typeof x` is not a carve-out root"
    );
    assert_eq!(
        admit_source_root(&contributor(clean_surface(), typeof_body)),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("typeof"))
    );

    // A bare `infer U` cannot appear in valid source (it only occurs inside a
    // conditional's extends clause), so construct the lowered `TypeExpr::Infer`
    // directly — the predicate must reject it defensively at the source root.
    let infer_body = TypeExpr::Infer {
        name: "U".to_string(),
    };
    assert_eq!(
        classify_source_root(&infer_body),
        SourceRootShape::NotCarveOut,
        "`infer U` is not a carve-out root"
    );
    assert_eq!(
        admit_source_root(&contributor(clean_surface(), infer_body)),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("infer"))
    );
}

#[test]
fn carve_out_does_not_leak_into_generic_hover_or_value_paths() {
    // The generic recursive predicate (the oracle-VALUE path) STILL rejects
    // `keyof` / indexed-access — the carve-out is source-ROOT only.
    assert_eq!(
        admit_type_expr(&lower("keyof KeySource")),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
    );
    assert_eq!(
        admit_type_expr(&lower("KeySource[\"nested\"][\"value\"]")),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );

    // The HOVER side STILL rejects `keyof` / indexed-access (a hover-lowered
    // `TSTypeOperatorType(Keyof)` / `TSIndexedAccessType` is unchanged).
    assert_eq!(
        admit_hover_text("keyof KeySource"),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
    );
    assert_eq!(
        admit_hover_text("KeySource[\"value\"]"),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );

    // A NESTED carve-out-looking shape inside an object body is NOT admitted: the
    // source-root carve-out fires only at the top, so an object whose member is an
    // indexed access still rejects through the generic predicate. (Even stamped
    // with a same-file root, the object body is NotCarveOut so the stamp is
    // unused.)
    assert_eq!(
        admit_source_root(&carve_out_contributor(
            clean_surface(),
            lower("{ a: KeySource[\"nested\"][\"value\"] }"),
            "/fixtures/keys.ts",
            Some("/fixtures/keys.ts"),
        )),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );
}

// ---------------------------------------------------------------------------
// §Q2 keyof-expansion extensions: the `Root[keyof Root]` self-index carve-out,
// the per-shape capture-kind postcondition, and the root-operand raw-fact
// admission (every carve-out shape).
// ---------------------------------------------------------------------------

#[test]
fn keyof_self_index_source_root_carve_out() {
    let same: &str = "/fixtures/keys.ts";

    // POSITIVE: `Root[keyof Root]` — BOTH refs the SAME bare unqualified root —
    // classifies as the self-index carve-out, names the shared root, and admits
    // through the same three-gate discipline (structure + same-file root +
    // clean root raw facts).
    let body = lower("Surface[keyof Surface]");
    assert_eq!(classify_source_root(&body), SourceRootShape::KeyofSelfIndex);
    assert_eq!(carve_out_root_ref_name(&body), Some("Surface"));
    assert_eq!(
        admit_source_contributor(&carve_out_contributor(
            clean_surface(),
            body.clone(),
            same,
            Some(same),
        )),
        AdmissionVerdict::Admit
    );

    // NEGATIVE: DIFFERENT roots (`Root[keyof Other]`) are NOT the self-index
    // shape — and not a string-literal chain either — so the generic predicate
    // rejects the bare operator.
    let different = lower("Surface[keyof Other]");
    assert_eq!(
        classify_source_root(&different),
        SourceRootShape::NotCarveOut
    );
    assert_eq!(carve_out_root_ref_name(&different), None);
    assert_eq!(
        admit_source_contributor(&carve_out_contributor(
            clean_surface(),
            different,
            same,
            Some(same),
        )),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );

    // NEGATIVE: a type-argument-carrying or qualified root is NOT a carve-out
    // root in either position.
    assert_eq!(
        classify_source_root(&lower("Box<string>[keyof Box<string>]")),
        SourceRootShape::NotCarveOut
    );
    assert_eq!(
        classify_source_root(&lower("A.B[keyof A.B]")),
        SourceRootShape::NotCarveOut
    );

    // NEGATIVE: a NESTED self-index (inside an object member) never reaches the
    // root carve-out.
    assert_eq!(
        classify_source_root(&lower("{ a: Surface[keyof Surface] }")),
        SourceRootShape::NotCarveOut
    );

    // NEGATIVE: an IMPORTED root (resolves to ANOTHER file) rejects through the
    // generic predicate; an UNBOUND root (no stamp) likewise.
    assert_eq!(
        admit_source_contributor(&carve_out_contributor(
            clean_surface(),
            body.clone(),
            same,
            Some("/fixtures/other.ts"),
        )),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );
    assert_eq!(
        admit_source_contributor(&carve_out_contributor(clean_surface(), body, same, None)),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );
}

/// The Resolved walk for one carve-out contributor (the shape `admit_query`'s
/// postcondition reads).
fn resolved_walk_of(body: TypeExpr) -> SourceWalkResult {
    let same = "/fixtures/keys.ts";
    SourceWalkResult::Resolved {
        contributors: vec![carve_out_contributor(
            clean_surface(),
            body,
            same,
            Some(same),
        )],
    }
}

#[test]
fn keyof_capture_kind_postcondition_requires_property_key_value() {
    const EXPANDED: ProjectionModeKind = ProjectionModeKind::Expanded;
    let keyof_walk = resolved_walk_of(lower("keyof KeySource"));

    // POSITIVE: string-literal unions and the string/number key primitives are
    // materialized property-key values — they ADMIT under the keyof kind.
    assert_eq!(
        admit_query("\"count\" | \"id\" | \"nested\"", &keyof_walk, EXPANDED),
        AdmissionVerdict::Admit
    );
    assert_eq!(
        admit_query("string | number", &keyof_walk, EXPANDED),
        AdmissionVerdict::Admit
    );
    assert_eq!(
        admit_query("\"a\" | 42", &keyof_walk, EXPANDED),
        AdmissionVerdict::Admit
    );

    // NEGATIVE (the DISCRIMINATING case): ordinary hover admission ADMITS
    // booleans / objects / `symbol`, so ONLY the capture-kind postcondition can
    // reject them under a KeyofBareRef-shaped row.
    assert!(
        matches!(
            admit_query("\"a\" | true", &keyof_walk, EXPANDED),
            AdmissionVerdict::Reject(RejectReason::KeyDomainViolation(_))
        ),
        "a boolean-bearing hover under the keyof capture kind must REJECT"
    );
    assert!(matches!(
        admit_query("{ a: string }", &keyof_walk, EXPANDED),
        AdmissionVerdict::Reject(RejectReason::KeyDomainViolation(_))
    ));
    assert!(
        matches!(
            admit_query("string | symbol", &keyof_walk, EXPANDED),
            AdmissionVerdict::Reject(RejectReason::KeyDomainViolation(_))
        ),
        "the `symbol` key-domain arm is NOT a materialized property-key value"
    );
    assert!(matches!(
        admit_query("string | null", &keyof_walk, EXPANDED),
        AdmissionVerdict::Reject(RejectReason::KeyDomainViolation(_))
    ));

    // The postcondition is scoped PER CARVE-OUT SHAPE: a `Root[keyof Root]`
    // (KeyofSelfIndex) row projects a VALUE union — ordinary admission only, so
    // the full member value union (incl. boolean / null) ADMITS.
    let self_index_walk = resolved_walk_of(lower("Surface[keyof Surface]"));
    assert_eq!(
        admit_query(
            "string | number | boolean | null",
            &self_index_walk,
            EXPANDED
        ),
        AdmissionVerdict::Admit
    );

    // …and a NON-carve-out row is untouched by the postcondition (a boolean
    // surface stays admissible).
    let plain_walk = SourceWalkResult::Resolved {
        contributors: vec![contributor(clean_surface(), bare_ref("Plain"))],
    };
    assert_eq!(
        admit_query("boolean", &plain_walk, EXPANDED),
        AdmissionVerdict::Admit
    );
}

#[test]
fn carve_out_root_raw_facts_admission_checked_for_every_shape() {
    let same: &str = "/fixtures/keys.ts";
    let mut unique_symbol_root = clean_surface();
    unique_symbol_root.unique_symbol_ops = vec![UniqueSymbolOp];

    // Every carve-out shape's ROOT raw facts are admission-checked: a root
    // carrying a `unique symbol` member REJECTS loudly (the root's keyspace
    // is not faithfully representable, so neither is its keyof / self-index /
    // chain projection).
    for body_text in [
        "keyof KeySource",
        "KeySource[\"nested\"][\"value\"]",
        "Surface[keyof Surface]",
    ] {
        assert_eq!(
            admit_source_contributor(&carve_out_contributor_with_root_surfaces(
                clean_surface(),
                lower(body_text),
                same,
                Some(same),
                vec![unique_symbol_root.clone()],
            )),
            AdmissionVerdict::Reject(RejectReason::UniqueSymbol),
            "a unique-symbol-bearing ROOT must reject the carve-out `{body_text}`"
        );

        // A MERGED root rejects if ANY contributor is dirty (a single clean
        // contributor does not admit the merge).
        assert_eq!(
            admit_source_contributor(&carve_out_contributor_with_root_surfaces(
                clean_surface(),
                lower(body_text),
                same,
                Some(same),
                vec![clean_surface(), unique_symbol_root.clone()],
            )),
            AdmissionVerdict::Reject(RejectReason::UniqueSymbol),
        );

        // An EMPTY root raw-fact vector for a recognized same-file carve-out is
        // a pairing failure — conservatively REJECT, never best-effort admit.
        assert_eq!(
            admit_source_contributor(&carve_out_contributor_with_root_surfaces(
                clean_surface(),
                lower(body_text),
                same,
                Some(same),
                Vec::new(),
            )),
            AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic),
        );

        // POSITIVE control: the same shape with a CLEAN root admits.
        assert_eq!(
            admit_source_contributor(&carve_out_contributor(
                clean_surface(),
                lower(body_text),
                same,
                Some(same),
            )),
            AdmissionVerdict::Admit
        );
    }

    // Other root raw-fact rejects discriminate by their own reason: a computed /
    // non-static root member key.
    let mut computed_key_root = clean_surface();
    computed_key_root.raw_member_keys = vec![RawKey::Computed];
    assert_eq!(
        admit_source_contributor(&carve_out_contributor_with_root_surfaces(
            clean_surface(),
            lower("keyof KeySource"),
            same,
            Some(same),
            vec![computed_key_root],
        )),
        AdmissionVerdict::Reject(RejectReason::NonStaticKey),
    );
}

#[test]
fn scaffold_hover_keyof_still_rejected() {
    // The drift fence for the distributive-identity capture strategy: if a
    // future pinned-tsgo bump re-prints `keyof X` THROUGH the scaffold,
    // extraction yields a keyof-rooted RHS and the hover-side reject fires
    // loudly at generation — the row re-defers honestly, never a silent wrong
    // snapshot. The reject is RETAINED, not relaxed, under the scaffold
    // strategy's own carve-out walk.
    assert_eq!(
        admit_hover_text("keyof KeySource"),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
    );

    // Under the FULL two-sided combiner for a scaffold-class (keyof carve-out)
    // row: the source side admits, but the keyof-printing hover still rejects.
    let keyof_walk = resolved_walk_of(lower("keyof KeySource"));
    assert_eq!(
        admit_query("keyof KeySource", &keyof_walk, ProjectionModeKind::Expanded),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
    );
    // The self-index family keeps its hover-side operator reject too.
    let self_index_walk = resolved_walk_of(lower("Surface[keyof Surface]"));
    assert_eq!(
        admit_query(
            "Surface[keyof Surface]",
            &self_index_walk,
            ProjectionModeKind::Expanded
        ),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
    );
}
