//! Discriminating guards for the canonical display policy
//! (`docs/arch/u2-query-value-domain-design.md` §14): `display()` is a
//! PROJECTION over an already-computed typed `SemanticQueryValue` driven by a
//! display-only `DisplayNeeds` bitset — NEVER a stored or re-parsed string, and
//! `display_needs` NEVER drives resolution / enters a typed-value family key.
//!
//! ## Projection-identity guards
//!
//! - `canonical_display_is_projection_not_stored_string` — one IMMUTABLE value
//!   renders to two DIFFERENT strings under two `DisplayNeeds`. A stored-string
//!   implementation returns the same string regardless and FAILS.
//! - `display_needs_is_display_only_never_drives_resolution` — two demands
//!   differing ONLY in `display_needs` collapse to the SAME typed-value slot
//!   (masked-equality), and one interned value renders two strings differing
//!   only by the display facets. Folding `display_needs` into the key FAILS the
//!   masked-equality.
//! - `display_needs_masked_out_of_typed_value_family_key` — even under
//!   `AxisMask::full()`, `apply_mask` forces `display_needs` to `⊥`, and there
//!   is NO `DemandAxis` for display. Adding a display axis or making the mask
//!   conditionally preserve `display_needs` FAILS.
//!
//! ## Render-fidelity guards
//!
//! Each derives its tokens STRUCTURALLY from node data and FAILS against an
//! impl that string-sniffs rendered output or drops live graph fields:
//!
//! - `precedence_is_structural_not_string_sniffing` — parenthesisation is by
//!   node kind (a `Conditional` operand wraps; a string literal whose text
//!   contains `" | "` does not).
//! - `object_signatures_use_colon_not_arrow_and_method_shorthand` — call /
//!   construct / method signatures use TS type-literal colon syntax.
//! - `mapped_type_renders_modifiers_and_name_remap` — readonly / optionality /
//!   `as` remap render distinctly.
//! - `function_type_parameters_render_constraint_and_default` — `<T extends
//!   C = D>` clause; a TypeParam reference stays name-only.
//! - `string_and_template_literals_are_escaped` — literal text is escaped.
//! - `vue_macro_elements_render_is_payload_derived_not_constant` — the carrier
//!   summary is derived from the live `ResolvedElements` payload.

use std::sync::Arc;

use verter_session::for_tests::{display, SemanticGraphStore};
use verter_session::semantic_query::demand::{
    apply_mask, relevant_demand_axes, AxisMask, Demand, DemandAxis, DisplayFacet, DisplayNeeds,
    ProjectionPath,
};
use verter_session::semantic_query::{
    DeclIdentity, DeclarationAnalysisValue, FunctionParam, IndexKey, LiteralValue, MapperKey,
    MapperKind, MemberMergeRole, NodeScopeId, OptionalityMod, PrimitiveKind, ReadonlyMod, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryValue, SurfaceMember, SurfaceView,
    TypeParamDecl, ValueRootKey,
};

/// A `Demand` that is `Expanded` on every semantic axis and carries `dn` as its
/// (display-only) `display_needs`. Two such demands differ ONLY in
/// `display_needs`.
fn expanded_demand_with_display(dn: DisplayNeeds) -> Demand {
    let mut d = Demand::expanded(ProjectionPath::empty());
    d.projection.display_needs = dn;
    d
}

// ----------------------------------------------------------------------
// GUARD 1 — display is a projection from `needs`, not a stored string.
// ----------------------------------------------------------------------

#[test]
fn canonical_display_is_projection_not_stored_string() {
    let store = SemanticGraphStore::new();

    // A readonly `string[]`. Its rendering DIFFERS under `IncludeReadonlyModifier`:
    //   facet off → `string[]`   facet on → `readonly string[]`.
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let array_id = store.intern_node(SemanticNodeData::Array {
        element: string_id,
        readonly: true,
    });

    // Construct the value ONCE — it is immutable across both renders.
    let value = SemanticQueryValue::TypeNode(array_id);

    let count_before = store.node_count();

    let without_ro = DisplayNeeds::empty();
    let with_ro = DisplayNeeds::single(DisplayFacet::IncludeReadonlyModifier);

    let rendered_without = display(&store, &value, without_ro);
    let rendered_with = display(&store, &value, with_ro);

    // The discriminating assertion: ONE immutable value renders to TWO different
    // strings ⇒ the string is PROJECTED from `needs`, not stored on the value.
    assert_ne!(
        rendered_without.as_str(),
        rendered_with.as_str(),
        "one immutable value must render differently under two DisplayNeeds — a \
         stored-string impl would return the same string"
    );
    assert_eq!(rendered_without.as_str(), "string[]");
    assert_eq!(rendered_with.as_str(), "readonly string[]");

    // A SECOND, independent facet on a DIFFERENT immutable value: a named decl
    // reference renders its bare name without `QualifyNames`, and a qualified
    // name with it.
    let decl_id = store.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity::synthetic("Foo"),
    });
    let decl_value = SemanticQueryValue::TypeNode(decl_id);
    let bare = display(&store, &decl_value, DisplayNeeds::empty());
    let qualified = display(
        &store,
        &decl_value,
        DisplayNeeds::single(DisplayFacet::QualifyNames),
    );
    assert_eq!(bare.as_str(), "Foo");
    assert_ne!(
        bare.as_str(),
        qualified.as_str(),
        "QualifyNames must change the rendering of the same immutable value"
    );
    assert_eq!(qualified.as_str(), "<synthetic>:Foo");

    // The value object / its node id is unchanged across renders, and display
    // mutated nothing in the store (interning the second fixture is the only
    // growth, and it happened AFTER the first two reads).
    match value {
        SemanticQueryValue::TypeNode(id) => assert_eq!(id, array_id),
        other => panic!("fixture value changed shape: {other:?}"),
    }
    // node_count after the first two renders (before interning decl_id) must
    // equal the pre-render count: display() never interns.
    assert_eq!(
        count_before + 1, // the decl fixture node
        store.node_count(),
        "display() must not mutate the store — only the explicit decl intern grew it"
    );
}

// ----------------------------------------------------------------------
// GUARD 2 — display_needs is display-only; it never drives resolution.
// ----------------------------------------------------------------------

#[test]
fn display_needs_is_display_only_never_drives_resolution() {
    // (a) Two demands identical EXCEPT projection.display_needs collapse to the
    //     SAME typed-value slot under the family mask (one compute, not two).
    let d_a = expanded_demand_with_display(DisplayNeeds::empty());
    let d_b = expanded_demand_with_display(DisplayNeeds::full());

    // Fixture precondition: they REALLY differ in display_needs only.
    assert_ne!(
        d_a.projection.display_needs, d_b.projection.display_needs,
        "fixture precondition: the two demands must differ in display_needs"
    );
    let mut a_no_display = d_a.clone();
    let mut b_no_display = d_b.clone();
    a_no_display.projection.display_needs = DisplayNeeds::empty();
    b_no_display.projection.display_needs = DisplayNeeds::empty();
    assert_eq!(
        a_no_display, b_no_display,
        "fixture precondition: the two demands are identical apart from display_needs"
    );

    // The typed-value family mask: every declarable demand axis. Since
    // display_needs is NOT a DemandAxis, this is the broadest possible semantic
    // mask — and apply_mask STILL collapses the two to one slot.
    let family_mask = relevant_demand_axes(DemandAxis::ORDERED);
    assert_eq!(
        apply_mask(&d_a, &family_mask),
        apply_mask(&d_b, &family_mask),
        "two demands differing only in display_needs must hash to the SAME typed-value \
         slot — folding display_needs into the key would induce a second slot"
    );

    // (b) With ONE interned value, the two display-only-differing needs render
    //     strings that differ ONLY by the display facet.
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let array_id = store.intern_node(SemanticNodeData::Array {
        element: string_id,
        readonly: true,
    });
    let value = SemanticQueryValue::TypeNode(array_id);

    let rendered_empty = display(&store, &value, d_a.projection.display_needs);
    let rendered_full = display(&store, &value, d_b.projection.display_needs);
    // full() includes IncludeReadonlyModifier; empty() does not.
    assert_eq!(rendered_empty.as_str(), "string[]");
    assert_eq!(rendered_full.as_str(), "readonly string[]");
    assert_ne!(
        rendered_empty.as_str(),
        rendered_full.as_str(),
        "the SAME value must render two strings under display-only-differing needs"
    );
}

// ----------------------------------------------------------------------
// GUARD 3 — display_needs is masked out of every typed-value family key.
// ----------------------------------------------------------------------

#[test]
fn display_needs_masked_out_of_typed_value_family_key() {
    // Even with EVERY declarable axis present, apply_mask forces display_needs
    // to ⊥.
    let d = expanded_demand_with_display(DisplayNeeds::full());
    assert_eq!(
        d.projection.display_needs,
        DisplayNeeds::full(),
        "fixture precondition: display_needs starts at ⊤"
    );
    assert_eq!(
        apply_mask(&d, &AxisMask::full()).projection.display_needs,
        DisplayNeeds::empty(),
        "display_needs must be masked to ⊥ even under the FULL axis mask (§14 invariant)"
    );

    // There is NO DemandAxis variant for display: the ordered axis set has
    // exactly the 14 semantic axes. Adding a display axis would bump this count
    // (and would let a mask preserve display_needs) — both failures are caught.
    assert_eq!(
        DemandAxis::ORDERED.len(),
        14,
        "DemandAxis must enumerate exactly the 14 semantic axes — no display axis"
    );

    // Building a mask from EVERY ordered axis still cannot keep display_needs,
    // because display_needs is structurally not an axis: masking is unconditional.
    let mask_from_all_axes = relevant_demand_axes(DemandAxis::ORDERED);
    assert_eq!(
        apply_mask(&d, &mask_from_all_axes).projection.display_needs,
        DisplayNeeds::empty(),
        "a mask built from all declarable axes must still mask display_needs to ⊥"
    );
}

// ======================================================================
// Render fidelity guards — the projection must derive every token
// STRUCTURALLY from node data, never from string-sniffing the rendered
// output and never by dropping live graph fields.
// ======================================================================

/// Project a single graph node with the empty (semantic-default) needs.
fn render(store: &SemanticGraphStore, id: SemanticNodeId) -> String {
    display(
        store,
        &SemanticQueryValue::TypeNode(id),
        DisplayNeeds::empty(),
    )
    .as_str()
    .to_string()
}

/// Intern a bare named declaration reference (`A`, `B`, …).
fn declref(store: &SemanticGraphStore, name: &str) -> SemanticNodeId {
    store.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity::synthetic(name),
    })
}

/// Intern a standalone `Function` node `(name: ty, …) => ret`.
fn func_node(
    store: &SemanticGraphStore,
    params: &[(&str, SemanticNodeId)],
    ret: SemanticNodeId,
) -> SemanticNodeId {
    let params: Vec<FunctionParam> = params
        .iter()
        .map(|(n, t)| FunctionParam::synthetic(Some(Arc::from(*n)), *t, false, false))
        .collect();
    store.intern_node(SemanticNodeData::Function {
        params: Arc::from(params.into_boxed_slice()),
        return_type: ret,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    })
}

/// A public, non-method object member `name: value` (or `name(): …` when
/// `is_method`).
fn member(name: &str, value: SemanticNodeId, is_method: bool) -> SurfaceMember {
    SurfaceMember {
        visibility: verter_type_expr::MemberVisibility::Public,
        name: Arc::from(name),
        value,
        optional: false,
        readonly: false,
        is_method,
        declared_in_macro_type_arg: false,
        merge_role: MemberMergeRole::Authored,
        spans: Default::default(),
        declaration_origin: None,
    }
}

fn member_with_role(
    name: &str,
    value: SemanticNodeId,
    is_method: bool,
    merge_role: MemberMergeRole,
) -> SurfaceMember {
    SurfaceMember {
        merge_role,
        ..member(name, value, is_method)
    }
}

/// An object surface with the given members / call / construct signatures.
fn object(
    store: &SemanticGraphStore,
    members: Vec<SurfaceMember>,
    call_signatures: Vec<SemanticNodeId>,
    construct_signatures: Vec<SemanticNodeId>,
) -> SemanticNodeId {
    store.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(call_signatures.into_boxed_slice()),
        construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }))
}

// ----------------------------------------------------------------------
// GUARD 4 — parenthesisation is decided STRUCTURALLY by node kind, not by
// scanning the rendered string for `" | "` / `" & "` / `" => "`.
// ----------------------------------------------------------------------

#[test]
fn precedence_is_structural_not_string_sniffing() {
    let store = SemanticGraphStore::new();
    let a = declref(&store, "A");
    let b = declref(&store, "B");
    let c = declref(&store, "C");
    let d = declref(&store, "D");

    let union_ab = store.intern_node(SemanticNodeData::Union(Arc::from([a, b])));
    let inter_ab = store.intern_node(SemanticNodeData::Intersection(Arc::from([a, b])));
    let cond = store.intern_node(SemanticNodeData::Conditional {
        check: a,
        extends: b,
        true_branch_ref: c,
        false_branch_ref: d,
        distributive: false,
    });

    // Regression: array element of a union parenthesises.
    let arr_union = store.intern_node(SemanticNodeData::Array {
        element: union_ab,
        readonly: false,
    });
    assert_eq!(render(&store, arr_union), "(A | B)[]");

    // DISCRIMINATING: a Conditional contains no `" | "`/`" & "`/`" => "`, so the
    // string-sniff wrapper MISSES it and renders `A extends B ? C : D[]`.
    let arr_cond = store.intern_node(SemanticNodeData::Array {
        element: cond,
        readonly: false,
    });
    assert_eq!(render(&store, arr_cond), "(A extends B ? C : D)[]");

    // Regression: keyof of an intersection parenthesises.
    let keyof_inter = store.intern_node(SemanticNodeData::KeyOf { base: inter_ab });
    assert_eq!(render(&store, keyof_inter), "keyof (A & B)");

    // DISCRIMINATING: keyof of a Conditional — string-sniff misses it.
    let keyof_cond = store.intern_node(SemanticNodeData::KeyOf { base: cond });
    assert_eq!(render(&store, keyof_cond), "keyof (A extends B ? C : D)");

    // DISCRIMINATING: the indexed-access OBJECT operand is rendered with NO
    // structural parens by the current impl → `A | B['x']`.
    let idx = store.intern_node(SemanticNodeData::IndexedAccess {
        object: union_ab,
        index: IndexKey::String(Arc::from("x")),
    });
    assert_eq!(render(&store, idx), "(A | B)['x']");

    // DISCRIMINATING: a string literal whose TEXT contains `" | "` is an ATOMIC
    // token — the array wrapper must NOT parenthesise it. The string-sniff impl
    // wrongly wraps it because the rendered child contains `" | "`.
    let lit = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a | b".to_string(),
    )));
    let arr_lit = store.intern_node(SemanticNodeData::Array {
        element: lit,
        readonly: false,
    });
    assert_eq!(render(&store, arr_lit), "'a | b'[]");

    // P3 DISCRIMINATING: a same-kind nested arm must NOT over-parenthesise. A
    // union arm that is itself a union (`A | (B | C)`) needs no parens — the
    // over-tight `min_prec` impl wraps it as `A | (B | C)`.
    let union_bc = store.intern_node(SemanticNodeData::Union(Arc::from([b, c])));
    let union_nested = store.intern_node(SemanticNodeData::Union(Arc::from([a, union_bc])));
    assert_eq!(render(&store, union_nested), "A | B | C");

    let inter_bc = store.intern_node(SemanticNodeData::Intersection(Arc::from([b, c])));
    let inter_nested = store.intern_node(SemanticNodeData::Intersection(Arc::from([a, inter_bc])));
    assert_eq!(render(&store, inter_nested), "A & B & C");

    // Cross-kind precedence is preserved: an intersection arm of a union needs
    // no parens (`&` binds tighter), but a union arm of an intersection does.
    let inter_in_union = store.intern_node(SemanticNodeData::Union(Arc::from([a, inter_bc])));
    assert_eq!(render(&store, inter_in_union), "A | B & C");
    let union_in_inter =
        store.intern_node(SemanticNodeData::Intersection(Arc::from([a, union_bc])));
    assert_eq!(render(&store, union_in_inter), "A & (B | C)");
}

// ----------------------------------------------------------------------
// GUARD 4b (P2-A) — a Conditional's own operands flow through the precedence
// lattice: a compound (Function / Union / Intersection / Conditional) check or
// extends parenthesises, a nested conditional in the TRUE branch parenthesises,
// and the trailing (right-associative) FALSE branch stays bare.
// ----------------------------------------------------------------------

#[test]
fn conditional_operands_apply_precedence() {
    let store = SemanticGraphStore::new();
    let a = declref(&store, "A");
    let b = declref(&store, "B");
    let c = declref(&store, "C");
    let d = declref(&store, "D");
    let x = declref(&store, "X");
    let r = declref(&store, "R");
    let t = declref(&store, "T");
    let f = declref(&store, "F");

    let cond = |check, extends, tb, fb| {
        store.intern_node(SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref: tb,
            false_branch_ref: fb,
            distributive: false,
        })
    };

    // DISCRIMINATING: a FUNCTION check parenthesises. The bare-render impl emits
    // `() => R extends X ? T : F`, which mis-parses as a function returning a
    // conditional.
    let func = func_node(&store, &[], r); // () => R
    assert_eq!(
        render(&store, cond(func, x, t, f)),
        "(() => R) extends X ? T : F"
    );

    // DISCRIMINATING: a UNION check parenthesises.
    let union_ab = store.intern_node(SemanticNodeData::Union(Arc::from([a, b])));
    assert_eq!(
        render(&store, cond(union_ab, x, t, f)),
        "(A | B) extends X ? T : F"
    );

    // A nested conditional in the TRUE branch parenthesises; the trailing FALSE
    // branch stays bare (conditional types are right-associative there).
    let inner = cond(c, d, a, b); // C extends D ? A : B
    assert_eq!(
        render(&store, cond(a, b, inner, d)),
        "A extends B ? (C extends D ? A : B) : D"
    );
    assert_eq!(
        render(&store, cond(a, b, c, inner)),
        "A extends B ? C : C extends D ? A : B"
    );

    // Regression: a conditional as an array element parenthesises.
    let arr_cond = store.intern_node(SemanticNodeData::Array {
        element: cond(a, b, c, d),
        readonly: false,
    });
    assert_eq!(render(&store, arr_cond), "(A extends B ? C : D)[]");
}

// ----------------------------------------------------------------------
// GUARD 5b (P2-B) — `is_method` only drives method-shorthand when the member
// value actually resolves to a `Function`. After intersection merging the flag
// can be ORed true while the value is an `Intersection` of overloads; that
// member MUST render property-style (with a colon), never `name(...)`.
// ----------------------------------------------------------------------

#[test]
fn method_member_with_non_function_value_renders_property_style() {
    let store = SemanticGraphStore::new();
    let a = declref(&store, "A");
    let b = declref(&store, "B");
    let inter = store.intern_node(SemanticNodeData::Intersection(Arc::from([a, b])));

    // is_method = true, but the value is an Intersection (merged overloads).
    let obj = object(&store, vec![member("foo", inter, true)], vec![], vec![]);
    let r = render(&store, obj);

    // DISCRIMINATING: the assume-Function impl emits `{ fooA & B }` (no colon,
    // bare `foo(`-less concatenation). The fix renders property-style.
    assert_eq!(r, "{ foo: A & B }");
    assert!(r.contains(": "), "must use property colon: {r}");
    assert!(!r.contains("foo("), "must not use method shorthand: {r}");
}

// ----------------------------------------------------------------------
// GUARD 5 — object call / construct / method signatures use TS type-literal
// COLON syntax, not the standalone-function arrow form; `is_method` is honoured.
// ----------------------------------------------------------------------

#[test]
fn object_signatures_use_colon_not_arrow_and_method_shorthand() {
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // DISCRIMINATING: a call signature renders `(x: string): number`, NOT the
    // arrow form `(x: string) => number`.
    let call_sig = func_node(&store, &[("x", string_id)], number_id);
    let obj_call = object(&store, vec![], vec![call_sig], vec![]);
    let r_call = render(&store, obj_call);
    assert_eq!(r_call, "{ (x: string): number }");
    assert!(
        !r_call.contains("=>"),
        "call signature must not use arrow: {r_call}"
    );

    // DISCRIMINATING: a construct signature renders `new (): number`, NOT
    // `new () => number`.
    let ctor_sig = func_node(&store, &[], number_id);
    let obj_ctor = object(&store, vec![], vec![], vec![ctor_sig]);
    let r_ctor = render(&store, obj_ctor);
    assert_eq!(r_ctor, "{ new (): number }");
    assert!(
        !r_ctor.contains("=>"),
        "construct signature must not use arrow: {r_ctor}"
    );

    // DISCRIMINATING: a METHOD member renders method shorthand
    // `foo(x: string): number`, NOT a property `foo: (x: string) => number`.
    let method_fn = func_node(&store, &[("x", string_id)], number_id);
    let obj_method = object(&store, vec![member("foo", method_fn, true)], vec![], vec![]);
    let r_method = render(&store, obj_method);
    assert_eq!(r_method, "{ foo(x: string): number }");

    // A PROPERTY whose value is a function keeps the arrow form — the standalone
    // `Function` rendering is unchanged in property position.
    let prop_fn = func_node(&store, &[("x", string_id)], number_id);
    let obj_prop = object(&store, vec![member("bar", prop_fn, false)], vec![], vec![]);
    assert_eq!(render(&store, obj_prop), "{ bar: (x: string) => number }");
}

// ----------------------------------------------------------------------
// GUARD 6 — mapped-type readonly / optionality modifiers and the `as` name
// remap are LIVE graph fields and must render distinctly, not be dropped.
// ----------------------------------------------------------------------

#[test]
fn mapped_type_renders_modifiers_and_name_remap() {
    let store = SemanticGraphStore::new();
    let t = declref(&store, "T");
    let v = declref(&store, "V");
    let f = declref(&store, "F");
    let k = store.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let key_space = store.intern_node(SemanticNodeData::KeyOf { base: t });

    let mapped = |readonly: ReadonlyMod, optionality: OptionalityMod, name_remap| {
        store.intern_node(SemanticNodeData::Mapped {
            source: t,
            mapper: MapperKey {
                parameter_node: k,
                key_space,
                value_expr: v,
                optionality,
                readonly,
                name_remap,
                kind: MapperKind::Computed,
            },
        })
    };

    let m_add = mapped(ReadonlyMod::Add, OptionalityMod::Add, None);
    let m_remove = mapped(ReadonlyMod::Remove, OptionalityMod::Remove, None);
    let m_remap = mapped(ReadonlyMod::Keep, OptionalityMod::Keep, Some(f));

    let r_add = render(&store, m_add);
    let r_remove = render(&store, m_remove);
    let r_remap = render(&store, m_remap);

    assert_eq!(r_add, "{ readonly [K in keyof T]?: V }");
    assert_eq!(r_remove, "{ -readonly [K in keyof T]-?: V }");
    assert_eq!(r_remap, "{ [K in keyof T as F]: V }");

    // DISCRIMINATING: the drop-everything impl renders all three identically.
    assert_ne!(r_add, r_remove);
    assert_ne!(r_add, r_remap);
    assert_ne!(r_remove, r_remap);
}

// ----------------------------------------------------------------------
// GUARD 7 — a function declaration clause renders type-parameter constraints
// and defaults (`<T extends C = D>`); a TypeParam REFERENCE stays name-only.
// ----------------------------------------------------------------------

#[test]
fn function_type_parameters_render_constraint_and_default() {
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let void_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));

    let func = store.intern_node(SemanticNodeData::Function {
        params: Arc::from(Vec::new().into_boxed_slice()),
        return_type: void_id,
        type_parameters: Arc::from([TypeParamDecl {
            name: Arc::from("T"),
            constraint: Some(string_id),
            default: Some(number_id),
        }]),
        signature_span: None,
        return_type_span: None,
    });
    // DISCRIMINATING: the name-only impl renders `<T>() => void`.
    assert_eq!(
        render(&store, func),
        "<T extends string = number>() => void"
    );

    // A standalone TypeParam REFERENCE renders by name only — constraint/default
    // are declaration-site metadata rendered in the `<…>` clause, never on a
    // reference. This is deliberately unchanged.
    let tref = store.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("U"),
        param_index: 0,
        constraint: Some(string_id),
        default: Some(number_id),
        display_name: Arc::from("U"),
    });
    assert_eq!(render(&store, tref), "U");
}

// ----------------------------------------------------------------------
// GUARD 8 — string and template-literal text is ESCAPED so the rendered token
// is a valid TS literal.
// ----------------------------------------------------------------------

#[test]
fn string_and_template_literals_are_escaped() {
    let store = SemanticGraphStore::new();

    // STRING literals are COOKED (the unescaped content); the renderer must
    // re-escape them. A literal containing a quote and a backslash.
    let lit = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a'b\\c".to_string(),
    )));
    // DISCRIMINATING: the raw impl emits `'a'b\c'` (broken).
    assert_eq!(render(&store, lit), r"'a\'b\\c'");

    // P2-D DISCRIMINATING: a non-printing control char (NUL) in a string literal
    // is escaped generically as `\u{..}`, not passed through raw. The
    // missing-control-char impl emits a raw NUL byte.
    let lit_ctrl = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a\0b".to_string(),
    )));
    assert_eq!(render(&store, lit_ctrl), r"'a\u{0}b'");

    // P2-C DISCRIMINATING: template quasis are stored as RAW source text (they
    // already carry source-level escapes), so they must round-trip VERBATIM —
    // re-escaping double-escapes them. A quasi holding a raw backslash and a
    // raw backtick renders byte-for-byte unchanged inside the backticks.
    let tmpl = store.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from([Arc::from(r"a\`b") as Arc<str>]),
        expressions: Arc::from(Vec::new().into_boxed_slice()),
    });
    // The double-escaping impl emits ``a\\\`b`` (backslash doubled, backtick
    // escaped); verbatim keeps the source text intact.
    assert_eq!(render(&store, tmpl), r"`a\`b`");
}

// ----------------------------------------------------------------------
// GUARD P1-A — a Function reached through a transparent `Alias` indirection
// renders the type-literal COLON form in object signature / method position,
// never the standalone arrow form. The arrow-hybrid impl emits the invalid
// `foo(x: string) => number` (method-name + arrow) because `render_signature_colon`
// matches `Function` DIRECTLY and falls to its defensive arm for an `Alias`.
// ----------------------------------------------------------------------

#[test]
fn alias_to_function_member_and_call_signature_render_colon_form() {
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // A method member whose value is Alias(Function). `resolves_to_function`
    // follows the alias and enters method shorthand, so the signature MUST also
    // follow the alias and render the colon form `foo(x: string): number` —
    // NOT the arrow hybrid `foo(x: string) => number`.
    let func = func_node(&store, &[("x", string_id)], number_id);
    let alias_func = store.intern_node(SemanticNodeData::Alias(func));
    let obj_method = object(
        &store,
        vec![member("foo", alias_func, true)],
        vec![],
        vec![],
    );
    let r = render(&store, obj_method);
    assert_eq!(r, "{ foo(x: string): number }");
    assert!(
        !r.contains("=>"),
        "alias method signature must not render an arrow hybrid: {r}"
    );

    // A call signature that is Alias(Function) likewise renders the colon form.
    let call_func = func_node(&store, &[("x", string_id)], number_id);
    let alias_call = store.intern_node(SemanticNodeData::Alias(call_func));
    let obj_call = object(&store, vec![], vec![alias_call], vec![]);
    let rc = render(&store, obj_call);
    assert_eq!(rc, "{ (x: string): number }");
    assert!(
        !rc.contains("=>"),
        "alias call signature must not render an arrow: {rc}"
    );
}

// ----------------------------------------------------------------------
// GUARD P1-B — a BigInt literal renders with the `n` suffix. The stored
// `LiteralValue::BigInt` carries base-10 DIGITS only (no suffix); the bare-clone
// impl silently turns the bigint-literal type `123n` into the number literal `123`.
// ----------------------------------------------------------------------

#[test]
fn bigint_literal_renders_with_n_suffix() {
    let store = SemanticGraphStore::new();
    let lit = store.intern_node(SemanticNodeData::Literal(LiteralValue::BigInt(
        "123".to_string(),
    )));
    // DISCRIMINATING: the bare-clone impl renders `123` (a number literal).
    assert_eq!(render(&store, lit), "123n");
}

// ----------------------------------------------------------------------
// GUARD P2-A — object member names that are NOT valid TS identifiers are
// single-quoted; valid identifiers (and bare numeric keys) stay unquoted. The
// raw-emit impl renders `{ foo-bar: T }` — invalid TS.
// ----------------------------------------------------------------------

#[test]
fn non_identifier_member_names_are_quoted() {
    let store = SemanticGraphStore::new();
    let t = declref(&store, "T");

    // DISCRIMINATING: a string-literal key with a hyphen must be quoted.
    let obj_hyphen = object(&store, vec![member("foo-bar", t, false)], vec![], vec![]);
    assert_eq!(render(&store, obj_hyphen), "{ 'foo-bar': T }");

    // A valid identifier stays bare.
    let obj_ident = object(&store, vec![member("foo", t, false)], vec![], vec![]);
    assert_eq!(render(&store, obj_ident), "{ foo: T }");

    // A bare numeric key is legal unquoted.
    let obj_numeric = object(&store, vec![member("123", t, false)], vec![], vec![]);
    assert_eq!(render(&store, obj_numeric), "{ 123: T }");
}

// ----------------------------------------------------------------------
// GUARD P2-B — a readonly array/tuple element of a mutable array parenthesises
// under the readonly facet, so the inner `readonly` does not re-read as the outer
// array's modifier. The flat impl emits `readonly string[][]` (= outer readonly).
// ----------------------------------------------------------------------

#[test]
fn readonly_array_element_is_parenthesised_under_readonly_facet() {
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let inner = store.intern_node(SemanticNodeData::Array {
        element: string_id,
        readonly: true,
    });
    let outer = store.intern_node(SemanticNodeData::Array {
        element: inner,
        readonly: false,
    });
    let value = SemanticQueryValue::TypeNode(outer);

    // DISCRIMINATING: with the readonly facet the inner `readonly` IS rendered,
    // so the inner array must parenthesise.
    let with_ro = display(
        &store,
        &value,
        DisplayNeeds::single(DisplayFacet::IncludeReadonlyModifier),
    );
    assert_eq!(with_ro.as_str(), "(readonly string[])[]");

    // Without the facet the readonly keyword is not shown → flat `string[][]`,
    // no parens.
    let without = display(&store, &value, DisplayNeeds::empty());
    assert_eq!(without.as_str(), "string[][]");
}

// ----------------------------------------------------------------------
// GUARD P2-C — the OTHER postfix base (the `IndexedAccess` object operand)
// parenthesises a readonly array element identically to the array-element base,
// so the leading `readonly` does not re-read as binding `(string[])[0]`. The
// pre-fix impl applied the readonly wrap ONLY at the array-element base and
// emits `readonly string[][0]` here.
// ----------------------------------------------------------------------

#[test]
fn indexed_access_object_readonly_is_parenthesised_under_readonly_facet() {
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let arr = store.intern_node(SemanticNodeData::Array {
        element: string_id,
        readonly: true,
    });
    let idx = store.intern_node(SemanticNodeData::IndexedAccess {
        object: arr,
        index: IndexKey::Number(
            verter_session::semantic_query::CanonicalIndexInt::from_canonical_i64(0)
                .expect("canonical"),
        ),
    });
    let value = SemanticQueryValue::TypeNode(idx);

    // DISCRIMINATING: with the readonly facet the object's leading `readonly`
    // IS rendered, so the object array must parenthesise.
    let with_ro = display(
        &store,
        &value,
        DisplayNeeds::single(DisplayFacet::IncludeReadonlyModifier),
    );
    assert_eq!(with_ro.as_str(), "(readonly string[])[0]");

    // Without the facet the readonly keyword is not shown → flat `string[][0]`.
    let without = display(&store, &value, DisplayNeeds::empty());
    assert_eq!(without.as_str(), "string[][0]");
}

// ----------------------------------------------------------------------
// GUARD P2-D — `DeclarationAnalysis` joins its contributors at intersection
// precedence, so a looser (Union) contributor parenthesises: `(A | B) & C`.
// The raw ` & `-join impl emits `A | B & C`, which mis-reads as `A | (B & C)`.
// ----------------------------------------------------------------------

#[test]
fn declaration_analysis_contributors_apply_intersection_precedence() {
    let store = SemanticGraphStore::new();
    let a = declref(&store, "A");
    let b = declref(&store, "B");
    let c = declref(&store, "C");
    let union_ab = store.intern_node(SemanticNodeData::Union(Arc::from([a, b])));
    let value = SemanticQueryValue::DeclarationAnalysis(DeclarationAnalysisValue {
        contributors: Arc::from([union_ab, c]),
    });

    // DISCRIMINATING: the looser Union contributor must parenthesise under the
    // implicit `&` merge. A raw bare-join impl emits `A | B & C`.
    let rendered = display(&store, &value, DisplayNeeds::empty());
    assert_eq!(rendered.as_str(), "(A | B) & C");
}

// ----------------------------------------------------------------------
// GUARD 10 — a `MergedDecl` carrier renders its ACTUAL peer-merged surface
// (member union + accumulated method overloads), NOT a placeholder/constant
// and NOT just the first contributor. It is also a PURE display projection:
// rendering must not intern any reduced node into the shared graph, AND the
// rendered string must be BYTE-IDENTICAL to the canonical mutating reducer's
// reduced surface. There is ONE peer-merge engine shared by both paths, so a
// display-only merge that diverges (separate-signature overload rendering, a
// display-only own-body-shadows-heritage branch) is a regression.
// ----------------------------------------------------------------------

#[test]
fn merged_decl_display_matches_graph_reduction_and_is_non_mutating() {
    let store = SemanticGraphStore::new();
    let number_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let boolean_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let method_number_to_string = func_node(&store, &[("value", number_id)], string_id);
    let method_string_to_number = func_node(&store, &[("value", string_id)], number_id);

    // Two same-name `interface` contributors in source order. `shadowed` is a
    // non-method CONFLICT — the canonical peer-merge takes the FIRST
    // contributor deterministically (NOT an own-body-shadows-heritage rule:
    // that precedence is owned by the intersection reducer over real
    // `extends`/`implements` arms, never by the own-surface peer-merge). `m` is
    // a same-name METHOD across both contributors → one accumulated overload
    // group. The per-member `merge_role` tags must NOT change the own-surface
    // peer-merge outcome.
    let part_a = object(
        &store,
        vec![
            member_with_role("shadowed", string_id, false, MemberMergeRole::Heritage),
            member("x", number_id, false),
            member("m", method_number_to_string, true),
        ],
        vec![],
        vec![],
    );
    let part_b = object(
        &store,
        vec![
            member_with_role("shadowed", boolean_id, false, MemberMergeRole::OwnBody),
            member("y", string_id, false),
            member("m", method_string_to_number, true),
        ],
        vec![],
        vec![],
    );
    let contributors = [part_a, part_b];

    let merged = store.intern_node(SemanticNodeData::MergedDecl {
        contributors: Arc::from(contributors),
    });

    // Canonical reduction through the single mutating peer-merge engine
    // (interns the reduced `Object` + the accumulated overload `Intersection`).
    let reduced =
        verter_session::for_tests::reduce_merged_decl_to_graph_node(&store, &contributors);
    let graph_render = render(&store, reduced);
    let count_after_reduction = store.node_count();

    // DISCRIMINATING #1 (non-mutating): display renders from a transient
    // surface and interns nothing — `node_count` is unchanged across the call.
    let display_render = render(&store, merged);
    assert_eq!(
        store.node_count(),
        count_after_reduction,
        "display(MergedDecl) must not intern reduced graph nodes"
    );

    // DISCRIMINATING #2 (no divergent second merge engine): display is
    // byte-identical to the canonical reduced surface. Pre-fix, the display
    // path rendered overloads as separate method signatures (`m(...): ...;
    // m(...): ...`) and applied a display-only shadow branch — both diverge
    // from the reduced Object and FAIL this equality.
    assert_eq!(
        display_render, graph_render,
        "display must match the canonical reduced surface byte-for-byte"
    );
    assert_eq!(
        display_render,
        "{ shadowed: string; x: number; m: ((value: number) => string) & ((value: string) => number); y: string }"
    );

    // Member union present; not first-contributor-only.
    assert!(display_render.contains("x: number"), "{display_render}");
    assert!(display_render.contains("y: string"), "{display_render}");
    assert_ne!(
        display_render, "{ x: number }",
        "must not be first-contributor-only"
    );
    // First-contributor precedence for the non-method conflict — NOT the
    // own-body value, and NOT a synthesised `never`.
    assert!(
        display_render.contains("shadowed: string"),
        "non-method conflict keeps the FIRST contributor: {display_render}"
    );
    assert!(
        !display_render.contains("shadowed: boolean"),
        "own-surface peer-merge must not special-case merge_role: {display_render}"
    );
    // Overloads accumulate as a property holding an intersection of function
    // types (exactly the reduced Object's shape), NOT as separate method
    // shorthand signatures.
    assert!(
        !display_render.contains("m(value:"),
        "accumulated overloads must not render as separate method signatures: {display_render}"
    );

    // NEGATIVE: a `MergedDecl` is atomic (`{…}`) — as an array element it must
    // NOT parenthesise (a non-Atom precedence arm would wrap it).
    let arr = store.intern_node(SemanticNodeData::Array {
        element: merged,
        readonly: false,
    });
    let ra = render(&store, arr);
    assert_eq!(ra, format!("{display_render}[]"));
    assert!(
        !ra.contains("({"),
        "merged decl is atomic — must not parenthesise: {ra}"
    );
}

/// A `MergedDecl` contributor with real `extends`/`implements` heritage (an
/// `Intersection` whose non-object arms are reference heritage) renders its
/// heritage arms preserved and joined with its own-body surface — byte-identical
/// to the canonical reducer's `Intersection([heritage…, own_object])`, and
/// without interning.
#[test]
fn merged_decl_display_preserves_heritage_arms_like_graph_reduction() {
    let store = SemanticGraphStore::new();
    let number_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let base_ref = declref(&store, "Base");
    let own = object(&store, vec![member("x", number_id, false)], vec![], vec![]);
    // `interface Foo extends Base { x: number }` lowers to an Intersection of
    // the heritage reference arm and the own-body object arm.
    let contributor = store.intern_node(SemanticNodeData::Intersection(Arc::from([base_ref, own])));
    let contributors = [contributor];
    let merged = store.intern_node(SemanticNodeData::MergedDecl {
        contributors: Arc::from(contributors),
    });

    let reduced =
        verter_session::for_tests::reduce_merged_decl_to_graph_node(&store, &contributors);
    let graph_render = render(&store, reduced);
    let count_after_reduction = store.node_count();

    let display_render = render(&store, merged);
    assert_eq!(
        store.node_count(),
        count_after_reduction,
        "display(MergedDecl) with heritage must not intern reduced graph nodes"
    );
    assert_eq!(
        display_render, graph_render,
        "display heritage surface must match the canonical reduced Intersection"
    );
    assert_eq!(display_render, "Base & { x: number }");
}

#[test]
fn display_source_does_not_call_graph_interning_or_dispatch() {
    let display_rs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("semantic_query")
        .join("display.rs");
    let source = std::fs::read_to_string(&display_rs)
        .unwrap_or_else(|err| panic!("read {}: {err}", display_rs.display()));
    for forbidden in [
        "intern_node(",
        "reduce_merged_decl_with_graph",
        "execute_type_node(",
        "execute_cooperative(",
    ] {
        assert!(
            !source.contains(forbidden),
            "display.rs must be a read-only projection; found forbidden `{forbidden}`"
        );
    }
}

// ----------------------------------------------------------------------
// GUARD 9 — the Vue-macro carrier renders content DERIVED from the live
// `ResolvedElements` payload, not a fixed `"<vue-macro>"` constant.
// ----------------------------------------------------------------------

#[test]
fn vue_macro_elements_render_is_payload_derived_not_constant() {
    use verter_parser::utils::oxc::script::type_surface::ResolvedElements;

    let store = SemanticGraphStore::new();

    let empty = store.intern_node(SemanticNodeData::VueMacroElements(Arc::new(
        ResolvedElements::default(),
    )));
    let callable = store.intern_node(SemanticNodeData::VueMacroElements(Arc::new(
        ResolvedElements {
            has_call_signature: true,
            ..ResolvedElements::default()
        },
    )));

    let r_empty = render(&store, empty);
    let r_callable = render(&store, callable);

    // DISCRIMINATING: a bare-constant impl renders both identically.
    assert_ne!(r_empty, r_callable);
    assert_eq!(r_empty, "<vue-macro props=0 emits=0>");
    assert_eq!(r_callable, "<vue-macro props=0 emits=0 callable>");
}

// ----------------------------------------------------------------------
// GUARD — the unresolved `BareRef` / `TypeOf` carriers render their
// `type_args` in a `<...>` segment (matching the `ImportType` /
// `InstantiationRef` arg-render convention), and an empty `type_args` stays
// bare. DISCRIMINATING: a renderer that drops `type_args` would emit `Foo` /
// `typeof factory.make` for the applied cases below.
// ----------------------------------------------------------------------

#[test]
fn bare_ref_carrier_renders_type_args() {
    let store = SemanticGraphStore::new();
    let number_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let bare = store.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    assert_eq!(
        render(&store, bare),
        "Foo",
        "a bare BareRef renders its name only"
    );

    let applied = store.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![number_id].into_boxed_slice()),
    ));
    assert_eq!(
        render(&store, applied),
        "Foo<number>",
        "BareRef.type_args must render in a `<...>` segment"
    );
}

#[test]
fn typeof_carrier_renders_type_args() {
    let store = SemanticGraphStore::new();
    let string_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let value_root = ValueRootKey {
        scope: ScopeId {
            canonical_id: Arc::from("/m.ts"),
            local_scope: None,
        },
        name: Arc::from("factory"),
    };

    let bare = store.intern_node(SemanticNodeData::new_typeof(
        value_root.clone(),
        Arc::from(vec![Arc::<str>::from("make")].into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    assert_eq!(
        render(&store, bare),
        "typeof factory.make",
        "a bare TypeOf renders root + path only"
    );

    let applied = store.intern_node(SemanticNodeData::new_typeof(
        value_root,
        Arc::from(vec![Arc::<str>::from("make")].into_boxed_slice()),
        Arc::from(vec![string_id].into_boxed_slice()),
    ));
    assert_eq!(
        render(&store, applied),
        "typeof factory.make<string>",
        "TypeOf.type_args must render in a `<...>` segment"
    );
}
