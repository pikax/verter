use super::positional::{
    MinArityFlags, PositionalMode, PositionalShape, ProjectedKind, ProjectedTuple, SlotTypeFacts,
    TypeAt,
};
use super::records::{
    ParameterLayout, ParameterOptionality, ParameterSlot, RestKind, RestSlot,
    SignatureSemanticFlags, TypeToken,
};

const STRING: TypeToken = TypeToken::from_raw(1);
const NUMBER: TypeToken = TypeToken::from_raw(2);
const VOID: TypeToken = TypeToken::from_raw(3);
const BOOL: TypeToken = TypeToken::from_raw(4);
const GENERIC: TypeToken = TypeToken::from_raw(5);

struct Facts;
impl SlotTypeFacts for Facts {
    fn accepts_void(&self, ty: TypeToken) -> bool {
        ty == VOID
    }
}

const MODES: [PositionalMode; 4] = [
    PositionalMode::Comparison,
    PositionalMode::Applicability,
    PositionalMode::TupleProjection,
    PositionalMode::UtilityInference,
];

fn req(ty: TypeToken) -> ParameterSlot {
    ParameterSlot::new(ty, ParameterOptionality::required())
}
fn opt(ty: TypeToken) -> ParameterSlot {
    ParameterSlot::new(ty, ParameterOptionality::optional_with_undefined())
}
fn layout(parameters: Vec<ParameterSlot>, rest: Option<RestSlot>) -> ParameterLayout {
    ParameterLayout {
        parameters: parameters.into_boxed_slice(),
        rest,
    }
}
fn rest(element: TypeToken, tail: Vec<ParameterSlot>) -> RestSlot {
    RestSlot {
        slot: req(element),
        kind: RestKind::Array,
        tail: tail.into_boxed_slice(),
    }
}
fn shape<'a>(l: &'a ParameterLayout, flags: SignatureSemanticFlags) -> PositionalShape<'a> {
    PositionalShape::new(l, None, flags, &Facts)
}

/// Every mode must answer the same arity question identically.
fn min_all_modes(s: &PositionalShape<'_>) -> usize {
    let first = s.effective_minimum(MODES[0]);
    for m in MODES {
        assert_eq!(s.effective_minimum(m), first, "mode {m:?} disagrees");
    }
    first
}

#[test]
fn optional_before_required_makes_the_optional_required_by_position() {
    let l = layout(vec![opt(NUMBER), req(STRING)], None);
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(s.declared_minimum(), 2);
    assert_eq!(min_all_modes(&s), 2);
    assert_eq!(s.max_arity(), Some(2));
}

#[test]
fn trailing_optionals_lower_the_minimum_and_bound_the_maximum() {
    let l = layout(vec![req(STRING), opt(NUMBER), opt(BOOL)], None);
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(min_all_modes(&s), 1);
    for m in MODES {
        assert!(!s.accepts_argument_count(0, m));
        assert!(s.accepts_argument_count(1, m));
        assert!(s.accepts_argument_count(3, m));
        assert!(!s.accepts_argument_count(4, m));
        assert!(s.is_optional_at(1, m) && !s.is_optional_at(0, m));
    }
}

#[test]
fn trailing_void_parameters_are_optional_unless_void_is_non_optional() {
    let l = layout(vec![req(STRING), req(VOID)], None);
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(s.declared_minimum(), 2);
    assert_eq!(min_all_modes(&s), 1);
    assert_eq!(
        s.effective_minimum_with(MinArityFlags {
            void_is_non_optional: true,
            strong_arity_for_untyped_js: false
        }),
        2
    );
    // A void that is not trailing stays required.
    let mid = layout(vec![req(VOID), req(STRING)], None);
    assert_eq!(min_all_modes(&shape(&mid, SignatureSemanticFlags::NONE)), 2);
}

#[test]
fn untyped_js_signatures_are_optional_unless_strong_arity_is_requested() {
    let l = layout(vec![req(STRING), req(NUMBER)], None);
    let s = shape(&l, SignatureSemanticFlags::UNTYPED_JS);
    assert_eq!(min_all_modes(&s), 0);
    assert_eq!(
        s.effective_minimum_with(MinArityFlags {
            void_is_non_optional: false,
            strong_arity_for_untyped_js: true
        }),
        2
    );
}

#[test]
fn array_rest_is_open_ended_and_addresses_its_element() {
    let l = layout(vec![req(STRING)], Some(rest(NUMBER, vec![])));
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(s.max_arity(), None);
    assert_eq!(s.parameter_count(), 2);
    assert_eq!(min_all_modes(&s), 1);
    assert!(s.accepts_argument_count(50, PositionalMode::Applicability));
    assert_eq!(s.type_at(0), TypeAt::One(req(STRING)));
    assert_eq!(s.type_at(7), TypeAt::One(req(NUMBER)));
}

#[test]
fn required_tail_after_a_variadic_middle_counts_and_unions_the_run() {
    // (a: string, ...mid: number[], last: boolean)
    let l = layout(vec![req(STRING)], Some(rest(NUMBER, vec![req(BOOL)])));
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(min_all_modes(&s), 2);
    assert!(!s.accepts_argument_count(1, PositionalMode::Applicability));
    assert!(s.accepts_argument_count(2, PositionalMode::Applicability));
    assert!(s.accepts_argument_count(9, PositionalMode::Applicability));
    match s.type_at(4) {
        TypeAt::Run { element, tail } => {
            assert_eq!(element, NUMBER);
            assert_eq!(tail, &[req(BOOL)]);
        }
        other => panic!("expected run, got {other:?}"),
    }
}

#[test]
fn optional_before_a_required_tail_becomes_required_by_position() {
    let l = layout(vec![opt(STRING)], Some(rest(NUMBER, vec![req(BOOL)])));
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(min_all_modes(&s), 2);
}

#[test]
fn generic_rest_is_one_open_position_indexed_by_offset() {
    let l = layout(
        vec![req(STRING)],
        Some(RestSlot {
            slot: req(GENERIC),
            kind: RestKind::GenericTuple,
            tail: Box::from([]),
        }),
    );
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(s.max_arity(), None);
    assert_eq!(
        s.type_at(3),
        TypeAt::GenericRest {
            rest: GENERIC,
            index: 2
        }
    );
}

#[test]
fn receiver_is_never_a_positional_slot() {
    let l = layout(vec![req(STRING)], None);
    let recv = req(NUMBER);
    let with = PositionalShape::new(&l, Some(recv), SignatureSemanticFlags::NONE, &Facts);
    let without = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(with.receiver(), Some(recv));
    assert_eq!(with.parameter_count(), without.parameter_count());
    assert_eq!(min_all_modes(&with), min_all_modes(&without));
    assert_eq!(with.type_at(0), TypeAt::One(req(STRING)));
    assert!(!PositionalShape::types_equal(&with, &without));
}

#[test]
fn tuple_projection_marks_optionality_from_the_effective_minimum() {
    let l = layout(vec![req(STRING), req(VOID), opt(NUMBER)], None);
    let s = shape(&l, SignatureSemanticFlags::NONE);
    let ProjectedTuple::Elements(elements) = s.project_tuple(0, PositionalMode::TupleProjection)
    else {
        panic!("elements");
    };
    let kinds: Vec<_> = elements.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ProjectedKind::Required,
            ProjectedKind::Optional,
            ProjectedKind::Optional
        ]
    );
}

#[test]
fn tuple_projection_from_the_rest_position_is_the_rest_itself() {
    let l = layout(vec![req(STRING)], Some(rest(NUMBER, vec![])));
    let s = shape(&l, SignatureSemanticFlags::NONE);
    assert_eq!(
        s.project_tuple(1, PositionalMode::TupleProjection),
        ProjectedTuple::Rest {
            ty: NUMBER,
            exact: true
        }
    );
    assert_eq!(
        s.project_tuple(4, PositionalMode::TupleProjection),
        ProjectedTuple::Rest {
            ty: NUMBER,
            exact: false
        }
    );
    let ProjectedTuple::Elements(elements) = s.project_tuple(0, PositionalMode::UtilityInference)
    else {
        panic!("elements");
    };
    assert_eq!(elements.last().unwrap().kind, ProjectedKind::Variadic);
}

#[test]
fn comparison_arity_reads_effective_minimum_or_strict_parameter_count() {
    let source = layout(vec![req(STRING), req(NUMBER)], None);
    let target = layout(vec![req(STRING)], None);
    let open = layout(vec![], Some(rest(STRING, vec![])));
    let s = shape(&source, SignatureSemanticFlags::NONE);
    let t = shape(&target, SignatureSemanticFlags::NONE);
    let o = shape(&open, SignatureSemanticFlags::NONE);
    let m = PositionalMode::Comparison;
    assert!(PositionalShape::source_has_more_parameters(
        &s, &t, false, m
    ));
    assert!(!PositionalShape::source_has_more_parameters(
        &t, &s, false, m
    ));
    assert!(!PositionalShape::source_has_more_parameters(
        &s, &o, true, m
    ));
    assert!(PositionalShape::source_has_more_parameters(&o, &t, true, m));
}

#[test]
fn parameter_names_never_take_part_in_type_equality() {
    use super::lifetime::SignatureStore;
    let store = SignatureStore::new();
    let a = store.intern_spelling("a", None).unwrap();
    let b = store.intern_spelling("b", None).unwrap();
    let named = |n| ParameterSlot {
        name: Some(n),
        ..req(STRING)
    };
    let la = layout(vec![named(a)], None);
    let lb = layout(vec![named(b)], None);
    assert_ne!(la, lb, "layouts keep the names for diagnostics");
    assert!(PositionalShape::types_equal(
        &shape(&la, SignatureSemanticFlags::NONE),
        &shape(&lb, SignatureSemanticFlags::NONE)
    ));
    // A layout naming a spelling from another epoch is rejected.
    let stale = layout(vec![named(super::records::SpellingId::from_raw(0))], None);
    assert!(store.intern_layout(stale, None).is_err());
    assert!(store.intern_layout(la, None).is_ok());
}

#[test]
fn explicit_optionality_is_not_reconstructed_from_type() {
    // strictNullChecks off: an optional slot carries no synthesized undefined.
    let strict = ParameterOptionality::optional_with_undefined();
    let loose = ParameterOptionality {
        declared_optional: true,
        includes_undefined: false,
    };
    let ls = layout(vec![ParameterSlot::new(STRING, strict)], None);
    let ll = layout(vec![ParameterSlot::new(STRING, loose)], None);
    assert_ne!(ls, ll);
    assert!(!PositionalShape::types_equal(
        &shape(&ls, SignatureSemanticFlags::NONE),
        &shape(&ll, SignatureSemanticFlags::NONE)
    ));
    // Both still decide the same arity.
    assert_eq!(min_all_modes(&shape(&ls, SignatureSemanticFlags::NONE)), 0);
    assert_eq!(min_all_modes(&shape(&ll, SignatureSemanticFlags::NONE)), 0);
}
