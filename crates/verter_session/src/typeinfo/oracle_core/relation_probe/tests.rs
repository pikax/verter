//! Discriminating guards for the v4 relation tuple-wire probe: synthesis /
//! strict inverse, strict wire decode, and operand canonicalization
//! (`docs/arch/refactor/rev11/charters/expansion-native-checker/NCK4.md`). tsgo-free — the
//! empirical any/never single-`true` behavior is proven by the checked-in
//! snapshots + the regeneration guard, not here.

use serde_json::json;
use verter_type_expr::{PrimitiveName, TypeExpr};

use super::super::identity::BinderLayoutEntry;
use super::{
    canonical_operand_ast, decode_tuple_wire, parse_probe_header, relation_probe_header,
    relation_probe_source, OperandCanonError, OperandRole, ProbeHeaderError, RelationVerdict,
    TupleWireError, BINDER_REF_PREFIX,
};

fn layout(names: &[&str]) -> Vec<BinderLayoutEntry> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| BinderLayoutEntry {
            ordinal: i as u16,
            name: (*n).to_string(),
            constraint: None,
        })
        .collect()
}

// -- probe_header_synthesis_is_the_fixed_tuple_wire --------------------------

#[test]
fn probe_header_synthesis_is_the_fixed_tuple_wire() {
    // The union wrapper: operands are ALWAYS 1-tuple-wrapped (the distribution
    // / any-both-branch suppressant), the branches the fixed readonly wire.
    let header = relation_probe_header(0, "string | number", "string", &layout(&[]));
    assert_eq!(
        header,
        "type __oracle_probe__0 = [string | number] extends [string] ? \
         readonly [true, readonly []] : readonly [false, readonly []];"
    );

    // Binder triples land in DECLARED (target-pattern preorder) order — NOT
    // name sort: B-then-A declared stays B-then-A in the wire.
    let header = relation_probe_header(3, "[1, 2]", "[infer B, infer A]", &layout(&["B", "A"]));
    assert_eq!(
        header,
        "type __oracle_probe__3 = [[1, 2]] extends [[infer B, infer A]] ? \
         readonly [true, readonly [readonly [0, \"B\", B], readonly [1, \"A\", A]]] : \
         readonly [false, readonly []];"
    );

    // The probe file is a full standalone source (header comment + one probe).
    let source = relation_probe_source("row_name", 0, "never", "string", &layout(&[]));
    assert!(source.starts_with("// @ai-generated - relation-verdict oracle probe (row row_name)\ntype __oracle_probe__0 = [never] extends [string]"));
    assert!(source.ends_with(";\n"));
}

// -- parse_probe_header_is_the_strict_inverse --------------------------------

#[test]
fn parse_probe_header_is_the_strict_inverse() {
    // Round-trip: synthesis → inverse returns the operands + binder names.
    let binders = layout(&["H", "R"]);
    let header = relation_probe_header(0, "[1, 2, 3]", "[infer H, ...infer R]", &binders);
    let (source, target, names) =
        parse_probe_header(&header, "__oracle_probe__0").expect("synthesized header inverts");
    assert_eq!(source, "[1, 2, 3]");
    assert_eq!(target, "[infer H, ...infer R]");
    assert_eq!(names, vec!["H".to_string(), "R".to_string()]);

    // Discriminating: REMOVING the union wrapper (a bare distributive
    // conditional) fails the inverse — the wrapper is load-bearing.
    let bare = "type __oracle_probe__0 = string | number extends string ? \
                readonly [true, readonly []] : readonly [false, readonly []];";
    assert!(
        matches!(
            parse_probe_header(bare, "__oracle_probe__0"),
            Err(ProbeHeaderError::ConditionalShape(_))
        ),
        "an unwrapped (distributive) conditional must fail the strict inverse"
    );

    // Wrong probe name / modifier / type params reject.
    assert!(matches!(
        parse_probe_header(&header, "__oracle_probe__9"),
        Err(ProbeHeaderError::Alias)
    ));
    let exported = format!("export {header}");
    assert!(matches!(
        parse_probe_header(&exported, "__oracle_probe__0"),
        Err(ProbeHeaderError::Alias)
    ));

    // A binder triple whose type ref names a DIFFERENT binder rejects.
    let mismatched = "type __oracle_probe__0 = [number] extends [infer A] ? \
                      readonly [true, readonly [readonly [0, \"A\", B]]] : \
                      readonly [false, readonly []];";
    assert!(matches!(
        parse_probe_header(mismatched, "__oracle_probe__0"),
        Err(ProbeHeaderError::Wire(_))
    ));

    // Out-of-order ordinals reject (the wire's order IS the preorder).
    let shuffled = "type __oracle_probe__0 = [[1, 2]] extends [[infer A, infer B]] ? \
                    readonly [true, readonly [readonly [1, \"B\", B], readonly [0, \"A\", A]]] : \
                    readonly [false, readonly []];";
    assert!(matches!(
        parse_probe_header(shuffled, "__oracle_probe__0"),
        Err(ProbeHeaderError::OrdinalSequence)
    ));

    // A FALSE branch carrying bindings rejects — via the FALSE-branch
    // emptiness rail (the true branch is well-formed here, so the true-branch
    // verdict-literal check cannot be the rejecting rail).
    let false_with_bindings = "type __oracle_probe__0 = [number] extends [infer A] ? \
                               readonly [true, readonly []] : \
                               readonly [false, readonly [readonly [0, \"A\", A]]];";
    assert!(
        matches!(
            parse_probe_header(false_with_bindings, "__oracle_probe__0"),
            Err(ProbeHeaderError::Wire(_))
        ),
        "a false branch carrying bindings must fail the false-branch emptiness rail"
    );
}

// -- decode_tuple_wire_accepts_exactly_the_grammar ----------------------------

#[test]
fn decode_tuple_wire_accepts_exactly_the_grammar() {
    // The empty-true wire (the any / never empirical shape: ONE true, no
    // distribution union).
    let v = decode_tuple_wire("readonly [true, readonly []]").expect("empty true wire decodes");
    assert_eq!(v.verdict, RelationVerdict::Assignable);
    assert!(v.bindings.is_empty());

    let v = decode_tuple_wire("readonly [false, readonly []]").expect("false wire decodes");
    assert_eq!(v.verdict, RelationVerdict::NotAssignable);
    assert!(v.bindings.is_empty());

    // A bound-bearing wire: ordinal + name + the bound TYPE (a numeric literal
    // IS a legitimate bound — the `1` of `[1,2,3] → [infer H, …]`).
    let v = decode_tuple_wire("readonly [true, readonly [readonly [0, \"H\", 1]]]")
        .expect("single binding wire decodes");
    assert_eq!(v.verdict, RelationVerdict::Assignable);
    assert_eq!(v.bindings.len(), 1);
    assert_eq!(v.bindings[0].ordinal, 0);
    assert_eq!(v.bindings[0].name, "H");
    assert!(matches!(
        v.bindings[0].bound,
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(_))
    ));

    // MULTI-INFER preserves the TARGET (wire) order, NOT alphabetical /
    // worklist order: B at ordinal 0 precedes A at ordinal 1.
    let v = decode_tuple_wire(
        "readonly [true, readonly [readonly [0, \"B\", string], readonly [1, \"A\", number]]]",
    )
    .expect("two-binding wire decodes");
    let got: Vec<(&str, u16)> = v
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.ordinal))
        .collect();
    assert_eq!(got, vec![("B", 0), ("A", 1)], "bindings stay in wire order");

    // A false verdict with bindings rejects.
    assert!(matches!(
        decode_tuple_wire("readonly [false, readonly [readonly [0, \"A\", string]]]"),
        Err(TupleWireError::FalseWithBindings)
    ));

    // Non-readonly scaffold / wrong arity / non-literal verdict / non-integer
    // ordinal / duplicate binder all reject.
    assert!(matches!(
        decode_tuple_wire("[true, readonly []]"),
        Err(TupleWireError::Shape(_))
    ));
    assert!(matches!(
        decode_tuple_wire("readonly [true]"),
        Err(TupleWireError::Shape(_))
    ));
    assert!(matches!(
        decode_tuple_wire("readonly [1, readonly []]"),
        Err(TupleWireError::Verdict)
    ));
    assert!(matches!(
        decode_tuple_wire("readonly [true, readonly [readonly [1.5, \"A\", string]]]"),
        Err(TupleWireError::Triple(_))
    ));
    assert!(matches!(
        decode_tuple_wire(
            "readonly [true, readonly [readonly [0, \"A\", string], readonly [1, \"A\", number]]]"
        ),
        Err(TupleWireError::DuplicateBinder(_))
    ));
    assert!(matches!(
        decode_tuple_wire("readonly [true, readonly [readonly [7, \"A\", string]]]"),
        Err(TupleWireError::OrdinalSequence)
    ));
    // A garbage RHS is a parse failure, never a guess.
    assert!(matches!(
        decode_tuple_wire("not a type at all ((("),
        Err(TupleWireError::Parse)
    ));
}

// -- wire_bounds_normalize_exactly ---------------------------------------------

#[test]
fn wire_bounds_normalize_exactly() {
    // Object bound: `{ value: number }` lowers + normalizes to the object.
    let v = decode_tuple_wire("readonly [true, readonly [readonly [0, \"V\", { value: number }]]]")
        .expect("object bound wire decodes");
    let TypeExpr::Object(obj) = &v.bindings[0].bound else {
        panic!("expected object bound, got {:?}", v.bindings[0].bound);
    };
    assert_eq!(obj.properties.len(), 1);

    // Tuple-tail bound: `[2, 3]` normalizes to a 2-element tuple of literals.
    let v = decode_tuple_wire("readonly [true, readonly [readonly [0, \"R\", [2, 3]]]]")
        .expect("tuple-tail bound wire decodes");
    let TypeExpr::Tuple { elements, readonly } = &v.bindings[0].bound else {
        panic!("expected tuple bound, got {:?}", v.bindings[0].bound);
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);

    // Parameter-tuple bound: `[x: string, y?: number | undefined]` preserves
    // labels, optionality, and the union.
    let v = decode_tuple_wire(
        "readonly [true, readonly [readonly [0, \"A\", [x: string, y?: number | undefined]]]]",
    )
    .expect("parameter-tuple bound wire decodes");
    let TypeExpr::Tuple { elements, .. } = &v.bindings[0].bound else {
        panic!("expected tuple bound, got {:?}", v.bindings[0].bound);
    };
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].label.as_deref(), Some("x"));
    assert!(!elements[0].optional);
    assert_eq!(elements[1].label.as_deref(), Some("y"));
    assert!(elements[1].optional);

    // A string-literal bound keeps literal identity (not the `string` primitive).
    let v = decode_tuple_wire("readonly [true, readonly [readonly [0, \"R\", \"hello\"]]]")
        .expect("string-literal bound wire decodes");
    assert!(matches!(
        v.bindings[0].bound,
        TypeExpr::Literal(verter_type_expr::LiteralValue::String(_))
    ));

    // A bound carrying the reserved binder-ref prefix rejects (a raw binder
    // ref can never be a tsgo-produced bound).
    assert!(matches!(
        decode_tuple_wire(&format!(
            "readonly [true, readonly [readonly [0, \"A\", {BINDER_REF_PREFIX}A]]]"
        )),
        Err(TupleWireError::BoundProjection(_))
    ));
}

// -- canonical_operand_ast -----------------------------------------------------

#[test]
fn canonical_operand_ast_encodes_binder_refs_only_in_the_target() {
    // The source operand: a plain normalized TypeExpr AST, no substitution.
    let src = canonical_operand_ast("{ a?: string }", OperandRole::Source).expect("source canon");
    assert_eq!(src["kind"], json!("object"));

    // The target operand: `infer V` becomes the reserved binder ref inside the
    // canonical AST.
    let tgt =
        canonical_operand_ast("{ value: infer V }", OperandRole::Target).expect("target canon");
    let canonical = super::super::normalize::canonical_json_string(&tgt);
    assert!(
        canonical.contains(&format!("\"name\":\"{BINDER_REF_PREFIX}V\"")),
        "the target operand encodes infer V as the reserved binder ref: {canonical}"
    );

    // A rest-tuple target pattern lowers losslessly (rest element preserved).
    let tgt = canonical_operand_ast("[infer H, ...unknown[]]", OperandRole::Target)
        .expect("rest pattern canon");
    let decoded = verter_type_expr::type_expr_from_json(&tgt).expect("canonical AST re-decodes");
    let TypeExpr::Tuple { elements, .. } = &decoded else {
        panic!("expected a tuple target operand");
    };
    assert!(
        elements.iter().any(|el| el.rest),
        "the rest element survives"
    );

    // A SOURCE carrying `infer` rejects (sources never bind).
    assert!(matches!(
        canonical_operand_ast("[infer H]", OperandRole::Source),
        Err(OperandCanonError::InferInSource)
    ));

    // A duplicated binder name rejects.
    assert!(matches!(
        canonical_operand_ast("[infer A, infer A]", OperandRole::Target),
        Err(OperandCanonError::DuplicateBinder(_))
    ));

    // A primitive operand canonicalizes to the primitive node.
    let prim = canonical_operand_ast("never", OperandRole::Source).expect("primitive canon");
    assert_eq!(
        verter_type_expr::type_expr_from_json(&prim),
        Some(TypeExpr::Primitive(PrimitiveName::Never))
    );
}

// -- canonical_operand_ast: rest / optional binder positions -------------------

#[test]
fn canonical_operand_ast_substitutes_rest_and_optional_binder_positions() {
    // The rest-ELEMENT binder (`[unknown, ...infer R]`) is substituted — a
    // `TSTupleElement::TSRestType` wrapper `as_ts_type()` alone skips.
    let tgt = canonical_operand_ast("[unknown, ...infer R]", OperandRole::Target)
        .expect("rest-element binder canon");
    let canonical = super::super::normalize::canonical_json_string(&tgt);
    assert!(
        canonical.contains(&format!("\"name\":\"{BINDER_REF_PREFIX}R\"")),
        "the rest-element infer position encodes as the reserved binder ref: {canonical}"
    );

    // The head binder beside a rest (`[infer H, ...unknown[]]`).
    let tgt = canonical_operand_ast("[infer H, ...unknown[]]", OperandRole::Target)
        .expect("head binder canon");
    let canonical = super::super::normalize::canonical_json_string(&tgt);
    assert!(
        canonical.contains(&format!("\"name\":\"{BINDER_REF_PREFIX}H\"")),
        "the head infer position encodes as the reserved binder ref: {canonical}"
    );

    // The rest-PARAMETER binder (`(...args: infer A) => any`) — the infer lives
    // in `FormalParameters.rest`, not `items`.
    let tgt = canonical_operand_ast("(...args: infer A) => any", OperandRole::Target)
        .expect("rest-parameter binder canon");
    let canonical = super::super::normalize::canonical_json_string(&tgt);
    assert!(
        canonical.contains(&format!("\"name\":\"{BINDER_REF_PREFIX}A\"")),
        "the rest-parameter infer position encodes as the reserved binder ref: {canonical}"
    );

    // The return-type binder (`(...args: any[]) => infer R`).
    let tgt = canonical_operand_ast("(...args: any[]) => infer R", OperandRole::Target)
        .expect("return-type binder canon");
    let canonical = super::super::normalize::canonical_json_string(&tgt);
    assert!(
        canonical.contains(&format!("\"name\":\"{BINDER_REF_PREFIX}R\"")),
        "the return-type infer position encodes as the reserved binder ref: {canonical}"
    );

    // A single plain parameter (`(x: infer X) => any`).
    let tgt = canonical_operand_ast("((x: infer X) => any)", OperandRole::Target)
        .expect("parameter binder canon");
    let canonical = super::super::normalize::canonical_json_string(&tgt);
    assert!(
        canonical.contains(&format!("\"name\":\"{BINDER_REF_PREFIX}X\"")),
        "the parameter infer position encodes as the reserved binder ref: {canonical}"
    );
}

// -- relation_identity_from_spec: binder layout in target-pattern preorder ----

#[test]
fn relation_identity_from_spec_requires_target_preorder_layout() {
    use super::super::query_specs::{
        HostProjectSpec, HostSetupKindSpec, RelationBinderSpec, RelationQuerySpec,
    };
    use super::{relation_identity_from_spec, RelationSpecError};

    let spec = |binder_layout: &'static [RelationBinderSpec]| RelationQuerySpec {
        row_file: "relation_verdict_oracle.rs",
        row_function: "relation_preorder_synthetic",
        query_ordinal: 0,
        oracle_family: "relation_verdict",
        host_project: HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        source_text: "[1, 2]",
        target_text: "[infer B, infer A]",
        binder_layout,
        contract_rows: &["relation_preorder_synthetic_contract"],
        engine_pin: None,
    };

    // The target pattern's binder preorder is B-then-A. A declared layout in
    // that exact order (ordinal AND name at each position) ACCEPTS.
    const CORRECT: &[RelationBinderSpec] = &[
        RelationBinderSpec {
            ordinal: 0,
            name: "B",
            constraint: None,
        },
        RelationBinderSpec {
            ordinal: 1,
            name: "A",
            constraint: None,
        },
    ];
    let identity = relation_identity_from_spec(&spec(CORRECT))
        .expect("the target-pattern preorder layout must derive");
    assert_eq!(identity.binder_layout[0].name, "B");
    assert_eq!(identity.binder_layout[1].name, "A");

    // A SWAPPED declared layout (A@0, B@1) names the same SET but the WRONG
    // preorder — it must REJECT (a sorted set-match would accept it and record
    // reversed binder identities/bounds).
    const SWAPPED: &[RelationBinderSpec] = &[
        RelationBinderSpec {
            ordinal: 0,
            name: "A",
            constraint: None,
        },
        RelationBinderSpec {
            ordinal: 1,
            name: "B",
            constraint: None,
        },
    ];
    assert!(
        matches!(
            relation_identity_from_spec(&spec(SWAPPED)),
            Err(RelationSpecError::BinderLayoutMismatch { .. })
        ),
        "a layout in the wrong target-pattern preorder must reject"
    );

    // The canonical target operand's binder refs are returned in the SAME
    // preorder the layout is checked against.
    let (_ast, binders) =
        super::canonical_operand_ast_with_binders("[infer B, infer A]", OperandRole::Target)
            .expect("operand canon");
    let binder_names: Vec<&str> = binders.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        binder_names,
        vec!["B", "A"],
        "the operand's binder refs come back in target-pattern preorder (first occurrence)"
    );
    assert!(
        binders.iter().all(|b| b.constraint.is_none()),
        "an unconstrained pattern yields no constraint texts"
    );
}

// -- infer constraints survive canonicalization -------------------------

#[test]
fn constrained_infer_derives_a_distinct_identity_from_unconstrained() {
    use super::super::identity::derive_relation_snapshot_id;
    use super::super::query_specs::{
        HostProjectSpec, HostSetupKindSpec, RelationBinderSpec, RelationQuerySpec,
    };
    use super::{relation_identity_from_spec, RelationSpecError};

    fn spec(
        target: &'static str,
        binder_layout: &'static [RelationBinderSpec],
    ) -> RelationQuerySpec {
        RelationQuerySpec {
            row_file: "relation_verdict_oracle.rs",
            row_function: "relation_constraint_synthetic",
            query_ordinal: 0,
            oracle_family: "relation_verdict",
            host_project: HostProjectSpec {
                project_root: "/",
                workspace_root: "/",
                tsconfig_path: "/oracle.tsconfig.json",
                host_setup_kind: HostSetupKindSpec::Standalone,
            },
            source_text: "{ value: number }",
            target_text: target,
            binder_layout,
            contract_rows: &["relation_constraint_synthetic_contract"],
            engine_pin: None,
        }
    }
    const LAYOUT_CONSTRAINED: &[RelationBinderSpec] = &[RelationBinderSpec {
        ordinal: 0,
        name: "V",
        constraint: Some("string"),
    }];
    const LAYOUT_UNCONSTRAINED: &[RelationBinderSpec] = &[RelationBinderSpec {
        ordinal: 0,
        name: "V",
        constraint: None,
    }];
    const LAYOUT_MISMATCHED: &[RelationBinderSpec] = &[RelationBinderSpec {
        ordinal: 0,
        name: "V",
        constraint: Some("number"),
    }];
    const LAYOUT_COMPOUND: &[RelationBinderSpec] = &[RelationBinderSpec {
        ordinal: 0,
        name: "V",
        constraint: Some("string | number"),
    }];

    // The constraint is extracted (text sliced BEFORE the whole TSInferType
    // node is substituted) and canonicalized into the layout entry.
    let constrained = relation_identity_from_spec(&spec(
        "{ value: infer V extends string }",
        LAYOUT_CONSTRAINED,
    ))
    .expect("constrained spec derives");
    let entry = &constrained.binder_layout[0];
    let constraint_ast = entry
        .constraint
        .as_ref()
        .expect("the constraint survives canonicalization");
    assert_eq!(
        verter_type_expr::type_expr_from_json(constraint_ast),
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "the layout entry carries the canonical constraint AST"
    );

    // The unconstrained variant.
    let unconstrained =
        relation_identity_from_spec(&spec("{ value: infer V }", LAYOUT_UNCONSTRAINED))
            .expect("unconstrained spec derives");
    assert!(unconstrained.binder_layout[0].constraint.is_none());

    // DISCRIMINATING: `infer V extends string` and `infer V` derive DIFFERENT
    // snapshot ids (the constraint is an identity axis — pre-F1 the whole
    // TSInferType span was substituted, erasing it and aliasing the two).
    let env = super::super::driver::pinned_env();
    let id_constrained = derive_relation_snapshot_id(&constrained, &env);
    let id_unconstrained = derive_relation_snapshot_id(&unconstrained, &env);
    assert_ne!(
        id_constrained, id_unconstrained,
        "a constrained infer and a bare infer must NOT alias to the same identity"
    );

    // A declared constraint that does NOT match the target pattern's rejects.
    let mismatched = spec("{ value: infer V extends string }", LAYOUT_MISMATCHED);
    assert!(
        matches!(
            relation_identity_from_spec(&mismatched),
            Err(RelationSpecError::BinderConstraintMismatch { position: 0, .. })
        ),
        "a declared constraint diverging from the pattern's must reject"
    );

    // A declared constraint where the pattern has none rejects.
    let phantom = spec("{ value: infer V }", LAYOUT_CONSTRAINED);
    assert!(matches!(
        relation_identity_from_spec(&phantom),
        Err(RelationSpecError::BinderConstraintMismatch { .. })
    ));

    // A pattern constraint with none declared rejects.
    let undeclared = spec("{ value: infer V extends string }", LAYOUT_UNCONSTRAINED);
    assert!(matches!(
        relation_identity_from_spec(&undeclared),
        Err(RelationSpecError::BinderConstraintMismatch { .. })
    ));

    // A compound constraint (`string | number`) canonicalizes structurally.
    let compound = relation_identity_from_spec(&spec(
        "{ value: infer V extends string | number }",
        LAYOUT_COMPOUND,
    ))
    .expect("compound constraint derives");
    assert!(compound.binder_layout[0].constraint.is_some());

    // An `infer X = …` default is outside the capture grammar — rejected.
    assert!(matches!(
        super::canonical_operand_ast_with_binders("[infer X = 1]", OperandRole::Target),
        Err(OperandCanonError::InferWithDefault(_)) | Err(OperandCanonError::Parse(_))
    ));
}
