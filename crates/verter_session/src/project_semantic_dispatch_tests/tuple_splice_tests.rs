//! Splicing a tuple into a tuple is weighed before an element is copied, as
//! the checker weighs it (`createNormalizedTupleType`): the elements the
//! tuple has expanded so far and the spliced tuple's elements together
//! reach 10,000 and the tuple is the checker's TS2799 recovery (TS2800
//! while an expression is checked), a resource partial that reads as `any`;
//! under the limit
//! the splice keeps every position, label and marker an authored tuple of
//! the same elements has. A product the connected demand cannot pay for
//! returns a typed incomplete node — never an unadmitted authored copy or
//! a truncated tuple.
//!
//! Every limit row is TypeScript 7.0.2's, measured with `tsc --strict` on
//! the same shapes: `[...Z9999]`, `[1, ...Z9998]` and `[...Z5000,
//! ...Z4999]` build; `[...Z10000]`, `[1, ...Z9999]` and `[...Z5000,
//! ...Z5000]` are TS2799 and `any`; `[...Z9999, 1]` has 10,000 elements and
//! an authored 10,001-element tuple builds; `[...a, ...b] as const` over
//! values of 5,000 elements each is TS2800 and `any`, as is a call of
//! `f<T extends unknown[]>(a: T): [...T, ...T]` over one; `[2, ...R]` over a
//! refused `R` reports nothing more.

use std::sync::Arc;

use super::checker_probe_lane_tests::{
    default_probe_host, flow_return_audited_on_host, mismatches_in_one_host, with_probe_on_host,
    with_probe_outcome_on_host, with_recovered_probe, ProbeProject, PROBE_FILE,
};
use super::ProjectSemanticDispatch;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;
use verter_type_engine::semantic_query::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, PartialReasonSet,
    QueryError, RecoveryBasis, SemanticNodeData, SemanticNodeId, SemanticQueryKey, TupleElement,
};

type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;

const IS_ANY: &str = "type IsAny<T> = 0 extends 1 & T ? \"any\" : \"not-any\";\n";

const CONCAT: &str = "type Concat<X extends unknown[], Y extends unknown[]> = [...X, ...Y];\n";

const TS2799: CheckerDiagnostic = CheckerDiagnostic {
    code: CheckerDiagnosticCode::TupleTooLarge,
    operation: CheckerDiagnosticOperation::TupleSpread,
};

const TS2800: CheckerDiagnostic = CheckerDiagnostic {
    code: CheckerDiagnosticCode::ExpressionTupleTooLarge,
    operation: CheckerDiagnosticOperation::TupleSpread,
};

/// An authored tuple type of `len` zeros, written inline: a named alias is
/// a declaration carrier a spread keeps, so only an inline tuple splices.
fn zeros(len: usize) -> String {
    format!("[{}]", vec!["0"; len].join(", "))
}

/// `text` with every run of zeros an authored tuple spells shortened to
/// its length, so a failure report stays readable.
fn abbreviated(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(4_096));
    let mut rest = text;
    while let Some(start) = rest.find("0, 0") {
        out.push_str(&rest[..start]);
        let run = &rest[start..];
        let mut len = 0;
        let mut end = 0;
        while run[end..].starts_with('0') {
            len += 1;
            end += 1;
            if run[end..].starts_with(", 0") {
                end += 2;
            } else {
                break;
            }
        }
        out.push_str(&format!("0×{len}"));
        rest = &run[end..];
    }
    out.push_str(rest);
    out
}

/// [`mismatches_in_one_host`] over rows whose probes are built text.
fn mismatches_of(source: &str, rows: &[(String, &str)]) -> Vec<String> {
    let rows: Vec<(&str, &str)> = rows
        .iter()
        .map(|(probe, checker)| (probe.as_str(), *checker))
        .collect();
    mismatches_in_one_host(source, &rows)
        .iter()
        .map(|failure| abbreviated(failure))
        .collect()
}

/// One tuple element as a test reads it: its label, its `?` and rest
/// markers, and its value as printed.
type ElementShape = (Option<String>, bool, bool, String);

/// Whether the tuple `node` settles on is readonly, and each of its
/// elements.
fn tuple_shape(dispatch: &Dispatch<'_>, node: SemanticNodeId) -> Option<(bool, Vec<ElementShape>)> {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Tuple { elements, readonly }) => Some((
            *readonly,
            elements
                .iter()
                .map(|element| {
                    (
                        element.label.as_deref().map(str::to_owned),
                        element.optional,
                        element.rest,
                        render_node(dispatch, element.value, 0),
                    )
                })
                .collect(),
        )),
        _ => None,
    }
}

/// The checker recovery `node` is, with its basis and origin.
fn recovery(
    dispatch: &Dispatch<'_>,
    node: SemanticNodeId,
) -> Option<(CheckerDiagnostic, RecoveryBasis, Option<SemanticNodeId>)> {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
            diagnostic,
            basis,
            origin,
        })) => Some((*diagnostic, *basis, *origin)),
        _ => None,
    }
}

/// A splice keeps every position, label, `?` and rest marker, readonly-ness
/// and length the directly authored tuple of the same elements has, through
/// nested spreads, an instantiated variadic and a product of 9,999
/// elements; an optional element a required one follows is the required
/// `T | undefined` slot, as the checker writes it.
#[test]
fn nested_splices_build_the_authored_tuple() {
    let source = format!("{CONCAT}type Open<T extends unknown[]> = [...[1, 2], ...T, ...[3]];\n");
    let rows: Vec<(String, String)> = vec![
        ("[...[1, ...[2, 3]], 4]".into(), "[1, 2, 3, 4]".into()),
        (
            "[...[1, ...[2, ...[3, ...[4]]]], ...[5, ...number[]]]".into(),
            "[1, 2, 3, 4, 5, ...number[]]".into(),
        ),
        (
            "[...[a: 1, b?: 2], ...string[]]".into(),
            "[a: 1, b?: 2, ...string[]]".into(),
        ),
        (
            "readonly [...[1, 2], 3]".into(),
            "readonly [1, 2, 3]".into(),
        ),
        ("Open<[9]>".into(), "[1, 2, 9, 3]".into()),
        ("Concat<[1], [2?]>".into(), "[1, 2?]".into()),
        (
            format!("[...{}, ...[...{}]]", zeros(4_999), zeros(5_000)),
            zeros(9_999),
        ),
    ];
    let host = default_probe_host();
    let mut failures = Vec::new();
    for (spliced, authored) in &rows {
        let shape = |probe: &str| {
            with_probe_on_host(
                &host,
                ProbeProject::default(),
                &source,
                probe,
                |dispatch, node| {
                    tuple_shape(dispatch, node).unwrap_or_else(|| {
                        panic!(
                            "`{}` must settle on a tuple, measured `{}`",
                            abbreviated(probe),
                            render_node(dispatch, node, 0)
                        )
                    })
                },
            )
        };
        let (spliced_shape, authored_shape) = (shape(spliced), shape(authored));
        if spliced_shape != authored_shape {
            failures.push(abbreviated(&format!(
                "`{spliced}` builds {spliced_shape:?}, the authored `{authored}` is {authored_shape:?}"
            )));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A rest element whose value is open stays in its position between the
/// spliced elements, verbatim: a splice never forces a generic.
#[test]
fn an_open_variadic_stays_between_the_spliced_elements() {
    let host = default_probe_host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let literal = |n: f64| {
        graph.intern_node(SemanticNodeData::Literal(
            verter_type_expr::LiteralValue::Number(n),
        ))
    };
    let element = |value, rest| TupleElement {
        label: None,
        value,
        optional: false,
        rest,
    };
    let tuple = |elements: Vec<TupleElement>| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: verter_type_engine::semantic_query::PayloadArray::from(
                elements.into_boxed_slice(),
            ),
            readonly: false,
        })
    };
    let open = graph.intern_node(SemanticNodeData::TypeParam {
        decl: verter_type_engine::semantic_query::DeclIdentity {
            canonical_id: verter_type_expr::OwnedStr::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: verter_type_engine::semantic_query::HashValue::default(),
            decl_name: verter_type_expr::OwnedStr::from("T"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: verter_type_expr::OwnedStr::from("T"),
    });
    let (one, two, three) = (literal(1.0), literal(2.0), literal(3.0));
    let front = tuple(vec![element(one, false), element(two, false)]);
    let back = tuple(vec![element(three, false)]);
    match dispatch.normalize_tuple_spread(
        &[
            element(front, true),
            element(open, true),
            element(back, true),
        ],
        false,
    ) {
        super::build::NormalizedTupleShape::Tuple(elements) => assert_eq!(
            elements
                .iter()
                .map(|element| (element.value, element.rest))
                .collect::<Vec<_>>(),
            vec![(one, false), (two, false), (open, true), (three, false)]
        ),
        other => panic!("the splice keeps a tuple, got {other:?}"),
    }
}

/// The first refused product is 10,000 elements, counted as the elements
/// expanded before the splice plus the spliced tuple's: 9,999 build, 10,000
/// and 10,001 are `any`, whether the elements come from one splice, a
/// written prefix or two splices of an instantiation. Elements written
/// after the last splice and a directly authored tuple are never counted.
#[test]
fn the_first_refused_splice_reaches_ten_thousand_elements() {
    let source = format!("{IS_ANY}{CONCAT}");
    let is_any = |probe: String| format!("IsAny<{probe}>");
    let rows: Vec<(String, &str)> = vec![
        (is_any(format!("[...{}]", zeros(9_999))), "\"not-any\""),
        (is_any(format!("[...{}]", zeros(10_000))), "any"),
        (is_any(format!("[...{}]", zeros(10_001))), "any"),
        (is_any(format!("[1, ...{}]", zeros(9_998))), "\"not-any\""),
        (is_any(format!("[1, ...{}]", zeros(9_999))), "any"),
        (is_any(format!("[1, ...{}]", zeros(10_000))), "any"),
        (
            is_any(format!("Concat<{}, {}>", zeros(5_000), zeros(4_999))),
            "\"not-any\"",
        ),
        (
            is_any(format!("Concat<{}, {}>", zeros(5_000), zeros(5_000))),
            "any",
        ),
        (
            is_any(format!("Concat<{}, {}>", zeros(5_000), zeros(5_001))),
            "any",
        ),
        (format!("[...{}][\"length\"]", zeros(9_999)), "9999"),
        (format!("[...{}, 1][\"length\"]", zeros(9_999)), "10000"),
        (format!("{}[\"length\"]", zeros(10_001)), "10001"),
    ];
    let failures = mismatches_of(&source, &rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The refused tuple is the checker's TS2799 recovery: a resource partial of
/// the operation budget alone, holding the authored tuple — its prefix and
/// its one spread — as its origin. An enclosing tuple spreading it keeps
/// its own positions with the recovery in the one variadic slot, still a
/// partial of the operation budget, and refuses nothing more.
#[test]
fn a_refused_splice_is_the_recovery_holding_the_authored_tuple() {
    let probe = format!("[1, ...{}]", zeros(9_999));
    with_recovered_probe(IS_ANY, &probe, |dispatch, node| {
        let Some((diagnostic, RecoveryBasis::Budget, Some(origin))) = recovery(dispatch, node)
        else {
            panic!(
                "the tuple must be a budget recovery with its origin, measured {:?}",
                dispatch.graph().node_data(node)
            );
        };
        assert_eq!(diagnostic, TS2799);
        let Some((false, origin)) = tuple_shape(dispatch, origin) else {
            panic!("the origin is the authored tuple");
        };
        assert_eq!(origin.len(), 2, "the prefix and the spread, unspliced");
        assert_eq!(origin[0].3, "1");
        assert!(!origin[0].2 && origin[1].2);
    });

    let probe = format!("[2, ...[1, ...{}]]", zeros(9_999));
    let host = default_probe_host();
    with_probe_outcome_on_host(
        &host,
        ProbeProject::default(),
        IS_ANY,
        &probe,
        |dispatch, outcome| {
            let super::evaluate::StructuralFactDemandOutcome::Recovered { node, reasons } = outcome
            else {
                panic!("the enclosing tuple is a partial, got {outcome:?}");
            };
            assert_eq!(reasons, PartialReasonSet::OPERATION_BUDGET);
            let Some(SemanticNodeData::Tuple { elements, .. }) = dispatch
                .graph()
                .node_data(node)
                .as_deref()
                .map(SemanticNodeData::copy_for_tests)
            else {
                panic!("the enclosing tuple keeps its positions");
            };
            assert_eq!(elements.len(), 2);
            assert_eq!(render_node(dispatch, elements[0].value, 0), "2");
            assert!(elements[1].rest);
            assert_eq!(
                recovery(dispatch, elements[1].value)
                    .map(|(diagnostic, basis, _)| (diagnostic, basis)),
                Some((TS2799, RecoveryBasis::Budget)),
                "the variadic slot holds the inner recovery"
            );
        },
    );
}

/// A tuple built while an expression is checked is refused with TS2800,
/// whoever writes it: an array literal under `as const` splicing the tuples
/// it spreads, and a generic call whose declared return type splices the
/// tuple its argument infers. Its value is
/// that recovery, the return degraded by the operation budget. Under the
/// limit the tuple is whole; elements after the last spread are not
/// counted.
#[test]
fn an_expression_splice_past_the_limit_is_ts2800() {
    let host = default_probe_host();
    let declare = |name: &str, len: usize| format!("declare const {name}: {};\n", zeros(len));
    let source = format!(
        "{}{}{}{}\
         {CONCAT}declare function twice<T extends unknown[]>(a: T): [...T, ...T];\n\
         declare function concatTwice<T extends unknown[]>(a: T): Concat<T, T>;\n\
         export function aliasAtLimit() {{ return concatTwice(a5000); }}\n\
         export function two() {{ return [...a5000, ...b4999] as const; }}\n\
         export function twoAtLimit() {{ return [...a5000, ...b5000] as const; }}\n\
         export function prefix() {{ return [1, ...c9999] as const; }}\n\
         export function suffix() {{ return [...c9999, 1] as const; }}\n\
         export function call() {{ return twice(b4999); }}\n\
         export function callAtLimit() {{ return twice(a5000); }}\n",
        declare("a5000", 5_000),
        declare("b4999", 4_999),
        declare("b5000", 5_000),
        declare("c9999", 9_999),
    );
    let mut failures = Vec::new();
    for (function, expected) in [
        ("two", Ok((true, 9_999))),
        ("twoAtLimit", Err(())),
        ("prefix", Err(())),
        ("suffix", Ok((true, 10_000))),
        ("call", Ok((false, 9_998))),
        ("callAtLimit", Err(())),
        ("aliasAtLimit", Err(())),
    ] {
        let result = flow_return_audited_on_host(&host, &source, function)
            .into_result()
            .unwrap_or_else(|error| panic!("`{function}` returns: {error:?}"));
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&ctx);
        let node = result.return_type();
        let measured = match expected {
            Ok((readonly, len)) => tuple_shape(&dispatch, node)
                .filter(|(built, elements)| *built == readonly && elements.len() == len)
                .map(|_| ())
                .ok_or(())
                .and_then(|()| result.degradation().is_none().then_some(()).ok_or(())),
            Err(()) => (recovery(&dispatch, node)
                .map(|(diagnostic, basis, _)| (diagnostic, basis))
                == Some((TS2800, RecoveryBasis::Budget))
                && result.degradation()
                    == Some(
                        verter_type_engine::semantic_query::FlowReturnDegradation::OperationBudget,
                    ))
            .then_some(())
            .ok_or(()),
        };
        if measured.is_err() {
            failures.push(format!(
                "`{function}` expected {expected:?}, measured degradation {:?}, recovery {:?} and `{}`",
                result.degradation(),
                recovery(&dispatch, node).map(|(diagnostic, basis, _)| (diagnostic, basis)),
                abbreviated(&format!(
                    "{:?}",
                    tuple_shape(&dispatch, node).map(|(readonly, e)| (readonly, e.len()))
                ))
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Checking for a tuple product must not materialize recursive nominal
/// members through a generic return alias either.
#[test]
fn nominal_returns_through_aliases_keep_their_complete_carrier() {
    let host = super::checker_probe_lane_tests::probe_host(ProbeProject {
        ambient_lib: Some(super::differential_global_library_tests::GLOBALS_LIB),
        ..ProbeProject::default()
    });
    let source = r#"
        type Wrapped<T> = Promise<T>;
        declare function wrapped<T>(value: T): Wrapped<T>;
        export function held() { return wrapped(1); }
    "#;
    let result = flow_return_audited_on_host(&host, source, "held")
        .into_result()
        .expect("the declared call return is available");
    assert!(
        result.fact().status().is_complete(),
        "{:?}",
        result.fact().status()
    );
    assert!(result.degradation().is_none(), "{:?}", result.degradation());
}

/// The product built, refused or left unpaid by one splice, read directly
/// inside a connected demand with `work` units and `bytes` construction
/// bytes: the shape, the work and bytes it charged, and the demand's trip.
fn splice_in_demand(
    host: &crate::VerterHost,
    lengths: [usize; 2],
    work: usize,
    bytes: Option<u64>,
) -> (
    super::build::NormalizedTupleShape,
    usize,
    u64,
    Option<PartialReasonSet>,
) {
    let dispatch = ProjectSemanticDispatch::new(host);
    let graph = dispatch.graph();
    let zero = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(0.0),
    ));
    let element = |value, rest| TupleElement {
        label: None,
        value,
        optional: false,
        rest,
    };
    let tuple = |len: usize| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: verter_type_engine::semantic_query::PayloadArray::from(
                (0..len)
                    .map(|_| element(zero, false))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            readonly: false,
        })
    };
    let (left, right) = (tuple(lengths[0]), tuple(lengths[1]));
    dispatch.set_connected_limits_for_tests(work, 24);
    if let Some(bytes) = bytes {
        dispatch.connected_demand().set_byte_limit_for_tests(bytes);
    }
    let _demand = dispatch.enter_connected_demand(false).0;
    let shape =
        dispatch.normalize_tuple_spread(&[element(left, true), element(right, true)], false);
    let ledger = dispatch.connected_demand();
    let (work, bytes) = (ledger.work_used_for_tests(), ledger.bytes_used_for_tests());
    // A tripped demand refuses every further unit with its trip.
    (shape, work, bytes, ledger.charge().err())
}

/// Planning and copying have distinct costs: the constructor charges every
/// inspected element, resolved spread and executed copy step. A size
/// refusal builds no expanded product; an admitted authored diagnostic
/// origin is charged separately. Work and construction-byte refusal return
/// typed incomplete nodes, and every consumed sizing prefix is recorded.
#[test]
fn a_splice_is_weighed_and_paid_for_before_it_is_built() {
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let element_bytes = std::mem::size_of::<TupleElement>() as u64;

    let (shape, work, bytes, trip) = splice_in_demand(&host, [5_000, 4_999], full, None);
    assert!(
        matches!(&shape, super::build::NormalizedTupleShape::Tuple(elements) if elements.len() == 9_999 && elements.iter().all(|element| !element.rest)),
        "9,999 elements splice: {:?}",
        abbreviated(&format!("{shape:?}"))
    );
    assert_eq!((work, trip), (20_006, None));
    assert!(
        bytes > 9_999 * element_bytes,
        "planning storage is reserved too"
    );

    let (shape, work, bytes, trip) = splice_in_demand(&host, [5_000, 5_000], full, None);
    assert!(
        bytes < 100 * element_bytes,
        "a size refusal reserves planning and its origin, never an expanded tuple"
    );
    assert!(matches!(
        shape,
        super::build::NormalizedTupleShape::Array(_)
    ));
    assert_eq!(
        (work, trip),
        (5_005, None),
        "sizing and the admitted two-element diagnostic origin are charged"
    );
    let (shape, work, _, trip) = splice_in_demand(&host, [9_999, 20_000], full, None);
    assert!(matches!(
        shape,
        super::build::NormalizedTupleShape::Array(_)
    ));
    assert_eq!(
        (work, trip),
        (30_005, None),
        "the child must be planned before joining: a later child splice may recover as one rest element"
    );

    let unspliced = |shape: &super::build::NormalizedTupleShape| {
        matches!(shape, super::build::NormalizedTupleShape::Array(_))
    };
    let (shape, _, _, trip) = splice_in_demand(&host, [5_000, 4_999], 20_000 - 1, None);
    assert!(
        unspliced(&shape),
        "an unpaid constructor returns a typed incomplete node"
    );
    assert_eq!(trip, Some(PartialReasonSet::PROJECTION_WORK_LIMIT));

    let (shape, _, _, trip) =
        splice_in_demand(&host, [5_000, 4_999], full, Some(9_999 * element_bytes - 1));
    assert!(
        unspliced(&shape),
        "an unpaid constructor returns a typed incomplete node"
    );
    assert_eq!(trip, Some(PartialReasonSet::CONNECTED_MEMORY_LIMIT));

    let (shape, work, _, trip) = splice_in_demand(&host, [5_000, 4_999], 300, None);
    assert!(
        unspliced(&shape),
        "an unpaid constructor returns a typed incomplete node"
    );
    assert_eq!(trip, Some(PartialReasonSet::PROJECTION_WORK_LIMIT));
    assert!(
        work == 300,
        "sizing pays as it reads and stops when the demand runs out, \
         long before the 10,001 elements sizing the product reads: charged {work}"
    );
}

/// What one isolated root demand answered.
#[derive(Debug, PartialEq)]
struct RootAnswer {
    answer: String,
    partial: bool,
    reasons: PartialReasonSet,
}

/// Run `key` as an isolated root demand with `work` units — inside an
/// enclosing demand that has already spent `preceding` units when that is
/// not zero — over a fresh view of `host`.
fn root_read(
    host: &Arc<crate::VerterHost>,
    key: &SemanticQueryKey,
    work: usize,
    bytes: Option<u64>,
    preceding: usize,
) -> RootAnswer {
    use verter_type_engine::semantic_query::{QueryResult, SemanticQueryValue};
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&ctx);
    dispatch.set_connected_limits_for_tests(work, 24);
    if let Some(bytes) = bytes {
        dispatch.connected_demand().set_byte_limit_for_tests(bytes);
    }
    let _enclosing = (preceding != 0).then(|| dispatch.enter_connected_demand(false).0);
    if preceding != 0 {
        dispatch.connected_demand().charge_units(preceding).unwrap();
    }
    let read = dispatch.execute_via_cold_build_helper(key.clone());
    let answer = match read.value() {
        QueryResult::Value(SemanticQueryValue::TypeNode(node)) => {
            match recovery(&dispatch, *node) {
                Some((diagnostic, basis, _)) => format!("recovery {diagnostic:?} {basis:?}"),
                None => match tuple_shape(&dispatch, *node) {
                    Some((_, elements)) => format!("tuple of {}", elements.len()),
                    None => render_node(&dispatch, *node, 0),
                },
            }
        }
        QueryResult::Value(other) => format!("value {:?}", other.tag()),
        QueryResult::Recursive(_) => "recursive".to_owned(),
        QueryResult::Error(error) => format!("error {error:?}"),
    };
    RootAnswer {
        answer,
        partial: read.result_is_partial(),
        reasons: read.partial_reason_classes(),
    }
}

/// `Concat<…>` instantiated over two tuples of `left` and `right` zeros,
/// as a root key on `host`: evaluating it builds the splice.
fn concat_key(host: &Arc<crate::VerterHost>, left: usize, right: usize) -> SemanticQueryKey {
    use verter_type_engine::semantic_query::{
        ProjectionMode, ProjectionReductionContext, ResolvedDeclSlotIdentity,
    };
    with_probe_on_host(host, ProbeProject::default(), CONCAT, "1", |_, _| ());
    let graph = host.project_type_store().semantic_graph();
    let zero = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(0.0),
    ));
    let tuple = |len: usize| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: verter_type_engine::semantic_query::PayloadArray::from(
                (0..len)
                    .map(|_| TupleElement {
                        label: None,
                        value: zero,
                        optional: false,
                        rest: false,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            readonly: false,
        })
    };
    crate::for_tests::instantiate_key_for_tests(
        host,
        ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(PROBE_FILE),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("Concat"),
        ),
        Arc::from([tuple(left), tuple(right)]),
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    )
}

fn semantic_misses(host: &crate::VerterHost) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .misses
}

fn sealed_refusals(host: &crate::VerterHost) -> usize {
    host.project_type_store()
        .semantic_graph()
        .refusal_summary_count_for_tests()
}

/// A refused splice at an isolated root is sealed under its allowances: an
/// exact repeat answers it without building anything, a larger allowance is
/// another profile that evaluates — and is refused again, the limit being
/// the checker's, not an allowance — a root inside an enclosing demand
/// never inherits the refusal, and an edit to the module declaring the
/// generic whose instantiation splices evaluates afresh.
#[test]
fn a_refused_splice_answers_its_own_profile_without_rebuilding() {
    let host = default_probe_host();
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let key = concat_key(&host, 5_000, 5_000);
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let read = |work: usize, preceding: usize| root_read(&host, &key, work, None, preceding);

    let refused = read(full / 2, 0);
    assert!(
        refused.partial && refused.reasons == PartialReasonSet::OPERATION_BUDGET,
        "the splice is refused by its own allowance: {refused:?}"
    );
    assert_eq!(
        refused.answer,
        format!("recovery {TS2799:?} {:?}", RecoveryBasis::Budget),
        "an instantiated type alias is refused at the type site"
    );
    assert!(sealed_refusals(&host) >= 1, "the refusal is sealed");

    let before = semantic_misses(&host);
    assert_eq!(read(full / 2, 0), refused, "the same profile answers it");
    assert_eq!(semantic_misses(&host), before, "without building anything");

    assert_eq!(
        read(full, 0),
        refused,
        "a larger allowance is refused alike"
    );
    assert!(
        semantic_misses(&host) > before,
        "after evaluating for itself"
    );

    let before = semantic_misses(&host);
    assert_eq!(read(full, 1), refused, "an enclosed root is refused alike");
    assert!(
        semantic_misses(&host) > before,
        "after evaluating: an enclosing demand never inherits a sealed refusal"
    );

    // The same profile again answers from its summary; after an edit to
    // the declaring module it evaluates.
    assert_eq!(read(full / 2, 0), refused);
    with_probe_on_host(
        &host,
        ProbeProject::default(),
        &format!("{CONCAT}type Edited = 1;\n"),
        "1",
        |_, _| (),
    );
    let before = semantic_misses(&host);
    assert_eq!(
        read(full / 2, 0),
        refused,
        "the edited module refuses alike"
    );
    assert!(semantic_misses(&host) > before, "after evaluating afresh");
}

/// A product its demand cannot pay for is a typed partial of the work rail,
/// sealed for its own profile: never a complete answer. The production
/// allowance builds it, and the complete result is kept for any demand that
/// can pay for it, while the smaller allowance keeps answering its refusal.
#[test]
fn an_unpaid_splice_heals_under_a_larger_allowance() {
    let host = default_probe_host();
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let key = concat_key(&host, 5_000, 4_999);
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let read = |work: usize| root_read(&host, &key, work, None, 0);

    let unpaid = read(64);
    assert!(
        unpaid.partial
            && unpaid
                .reasons
                .contains(PartialReasonSet::PROJECTION_WORK_LIMIT),
        "64 units cannot build 9,999 elements: {unpaid:?}"
    );
    assert!(!unpaid.reasons.contains(PartialReasonSet::OPERATION_BUDGET));
    let before = semantic_misses(&host);
    assert_eq!(read(64), unpaid, "the same profile answers it");
    assert_eq!(semantic_misses(&host), before, "without building anything");

    let complete = read(full);
    assert_eq!(
        (complete.answer.as_str(), complete.partial),
        ("tuple of 9999", false)
    );
    let healed = semantic_misses(&host);
    assert_eq!(read(full), complete, "the built product is kept");
    assert_eq!(semantic_misses(&host), healed, "for a demand that can pay");
    assert_eq!(read(64), unpaid, "never for one that cannot");
}

/// A product the soft construction-byte rail refuses is a partial of that
/// rail, distinct from the size refusal and never sealed: its refusal
/// depends on what its context spent. A size refusal whose summary the
/// retention account declines to keep is returned exactly as one it keeps,
/// and evaluates again on repeat.
#[test]
fn soft_byte_and_retention_refusals_stay_distinct() {
    let host = default_probe_host();
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let key = concat_key(&host, 5_000, 4_999);
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let element_bytes = std::mem::size_of::<TupleElement>() as u64;
    let read = || root_read(&host, &key, full, Some(9_999 * element_bytes - 1), 0);
    let sealed = sealed_refusals(&host);
    let short = read();
    assert!(
        short.partial
            && short
                .reasons
                .contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT),
        "the byte rail refuses the product: {short:?}"
    );
    assert!(!short.reasons.contains(PartialReasonSet::OPERATION_BUDGET));
    assert_eq!(
        sealed_refusals(&host),
        sealed,
        "a soft-byte refusal is never sealed"
    );
    let before = semantic_misses(&host);
    assert_eq!(read(), short);
    assert!(semantic_misses(&host) > before, "and evaluates again");

    let kept = {
        let host = default_probe_host();
        let _scope = host
            .project_type_store()
            .semantic_graph()
            .enter_node_root_scope();
        let key = concat_key(&host, 5_000, 5_000);
        root_read(&host, &key, full, None, 0)
    };
    let mut host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    let account = verter_session_query::retention::SemanticRetentionAccount::new(
        verter_session_query::retention::RetentionLimits {
            max_entry_bytes: 0,
            ..verter_session_query::retention::RetentionLimits::defaults()
        },
    );
    Arc::get_mut(&mut host)
        .expect("a fresh host is not shared")
        .project_type_store =
        Arc::new(crate::project_type_store::ProjectTypeStore::with_retention_account(account));
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let key = concat_key(&host, 5_000, 5_000);
    let declined = root_read(&host, &key, full, None, 0);
    assert_eq!(
        declined, kept,
        "the declined refusal is returned as a kept one"
    );
    assert_eq!(sealed_refusals(&host), 0, "and is not kept");
    let before = semantic_misses(&host);
    assert_eq!(root_read(&host, &key, full, None, 0), kept);
    assert!(semantic_misses(&host) > before, "so its repeat evaluates");
}

/// Top-level indexed calls suspend outside the flow evaluator. Their
/// declared tuple return is still constructed while checking an expression.
#[test]
fn indexed_calls_preserve_expression_tuple_recovery() {
    let host = default_probe_host();
    let source = format!(
        "declare const z: {};\n\
         declare function twice<T extends unknown[]>(a: T): [...T, ...T];\n\
         {CONCAT}declare function concat<T extends unknown[]>(a: T): Concat<T, T>;\n\
         export const direct = twice(z);\n\
         export const aliased = concat(z);\n\
         declare const typed: Concat<typeof z, typeof z>;\n",
        zeros(5_000)
    );
    for (probe, expected) in [
        ("typeof typed", TS2799),
        ("typeof direct", TS2800),
        ("typeof aliased", TS2800),
        ("typeof typed", TS2799),
        ("typeof aliased", TS2800),
    ] {
        with_probe_outcome_on_host(
            &host,
            ProbeProject::default(),
            &source,
            probe,
            |dispatch, outcome| {
                let super::evaluate::StructuralFactDemandOutcome::Recovered { node, reasons } =
                    outcome
                else {
                    panic!("{probe}: expected typed recovery, got {outcome:?}");
                };
                assert_eq!(reasons, PartialReasonSet::OPERATION_BUDGET);
                assert_eq!(
                    recovery(dispatch, node).map(|(diagnostic, basis, _)| (diagnostic, basis)),
                    Some((expected, RecoveryBasis::Budget)),
                    "{probe}"
                );
            },
        );
    }
}

/// An argument the frame does not lower (`concat(z)!`) evaluates through the
/// inline indexed run. Its call's lazy declared return is checked while the
/// expression is checked, exactly as a frame-lowered call argument's is: no
/// unchecked tuple carrier escapes into the outer call's result.
#[test]
fn inline_indexed_arguments_check_their_declared_tuple_return() {
    let host = default_probe_host();
    let source = format!(
        "declare const z: {};\n\
         {CONCAT}declare function concat<T extends unknown[]>(a: T): Concat<T, T>;\n\
         declare function id<T>(a: T): T;\n\
         declare function wrap<T>(a: T): {{ v: T }};\n\
         export function lowered() {{ return id(concat(z)); }}\n\
         export function indexed() {{ return id(concat(z)!); }}\n\
         export function loweredMember() {{ return wrap(concat(z)); }}\n\
         export function indexedMember() {{ return wrap(concat(z)!); }}\n",
        zeros(5_000)
    );
    let answer = |function: &str| {
        let result = flow_return_audited_on_host(&host, &source, function)
            .into_result()
            .unwrap_or_else(|error| panic!("`{function}` returns: {error:?}"));
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&ctx);
        (
            abbreviated(&render_node(&dispatch, result.return_type(), 0)),
            result.degradation(),
        )
    };
    for (lowered, indexed) in [("lowered", "indexed"), ("loweredMember", "indexedMember")] {
        let expected = answer(lowered);
        assert_eq!(
            expected.1,
            Some(verter_type_engine::semantic_query::FlowReturnDegradation::OperationBudget),
            "`{lowered}` is the checker's expression recovery"
        );
        assert_eq!(
            answer(indexed),
            expected,
            "`{indexed}` answers as `{lowered}`"
        );
    }
}

/// A traversal commits exactly the admitted prefix, even when its last
/// window is smaller than a batch; an unpaid product constructs no list.
#[test]
fn tuple_planning_stops_at_each_admitted_work_prefix() {
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    for allowance in [0, 1, 255, 256, 257, 300] {
        let (shape, work, _, trip) = splice_in_demand(&host, [1_000, 1], allowance, None);
        assert_eq!(work, allowance, "allowance {allowance}");
        assert_eq!(trip, Some(PartialReasonSet::PROJECTION_WORK_LIMIT));
        assert!(
            matches!(shape, super::build::NormalizedTupleShape::Array(_)),
            "a refused traversal must not copy the authored list: {allowance}"
        );
    }
}

/// A complete substitution pays its tuple construction cost on a warm
/// demand. Low allowances never become complete through prior warmth.
#[test]
fn substitution_reuse_pays_for_tuple_construction() {
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let owner = ProjectSemanticDispatch::new(&host);
    let graph = owner.graph();
    let parameter = graph.intern_node(SemanticNodeData::TypeParam {
        decl: verter_type_engine::semantic_query::DeclIdentity {
            canonical_id: verter_type_expr::OwnedStr::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: Default::default(),
            decl_name: verter_type_expr::OwnedStr::from("T"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: verter_type_expr::OwnedStr::from("T"),
    });
    let zero = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(0.0),
    ));
    let element = |value, rest| TupleElement {
        label: None,
        value,
        optional: false,
        rest,
    };
    let tuple = |elements: Vec<TupleElement>| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: verter_type_engine::semantic_query::PayloadArray::from(
                elements.into_boxed_slice(),
            ),
            readonly: false,
        })
    };
    let source = tuple(vec![element(parameter, true)]);
    let arg = tuple((0..64).map(|_| element(zero, false)).collect());
    let run = |allowance| {
        let dispatch = ProjectSemanticDispatch::new(&host);
        dispatch.set_connected_limits_for_tests(allowance, 24);
        let _demand = dispatch.enter_connected_demand(false).0;
        let result = dispatch.substitute_semantic_type_param(source, parameter, arg);
        let len = tuple_shape(&dispatch, result).map(|(_, elements)| elements.len());
        (
            len,
            dispatch.connected_demand().work_used_for_tests(),
            dispatch.connected_demand_tripped(),
        )
    };
    let low = run(1);
    assert_ne!(low.0, Some(64));
    assert!(low.2);
    assert_eq!(
        graph.substitute_memo_get(source, parameter, arg),
        None,
        "an unpaid substitution is never an ordinary memo result"
    );
    let cold = run(10_000);
    assert_eq!(cold.0, Some(64));
    assert!(!cold.2);
    assert!(cold.1 > 64);
    assert!(
        graph.substitute_memo_get(source, parameter, arg).is_some(),
        "a complete tuple substitution has a priced memo entry"
    );
    let warm = run(10_000);
    assert_eq!(warm, cold, "the warm demand pays the construction receipt");
    assert_eq!(run(1), low, "prior warmth cannot heal a low allowance");
    assert_eq!(
        run(10_000),
        cold,
        "the lower demand cannot poison the complete entry"
    );
}

/// A substitution over an operand the graph cannot root (an unallocated
/// sentinel) is an unpriced miss: it reads as an opaque miss and publishes
/// no memo entry, rather than failing to price the walk.
#[test]
fn an_unrootable_substitution_operand_is_an_unpriced_miss() {
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let parameter = graph.intern_node(SemanticNodeData::TypeParam {
        decl: verter_type_engine::semantic_query::DeclIdentity {
            canonical_id: verter_type_expr::OwnedStr::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: Default::default(),
            decl_name: verter_type_expr::OwnedStr::from("T"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: verter_type_expr::OwnedStr::from("T"),
    });
    let zero = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(0.0),
    ));
    let absent = SemanticNodeId(verter_type_engine::semantic_query_memo::UNALLOCATABLE_ID_FLOOR);
    let _demand = dispatch.enter_connected_demand(false).0;
    let result = dispatch.substitute_semantic_type_param(absent, parameter, zero);
    assert!(matches!(
        graph.node_data(result).as_deref(),
        Some(SemanticNodeData::Opaque(QueryError::Miss))
    ));
    assert_eq!(graph.substitute_memo_get(absent, parameter, zero), None);
}

/// Stored tuples need not have been normalized bottom-up by lowering. An
/// inner refusal occupies its own rest position, preserving the outer prefix.
#[test]
fn a_stored_inner_overflow_preserves_the_outer_tuple() {
    let host = default_probe_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = dispatch.graph();
    let zero = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(0.0),
    ));
    let element = |value, rest| TupleElement {
        label: None,
        value,
        optional: false,
        rest,
    };
    let tuple = |elements: Vec<TupleElement>| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: verter_type_engine::semantic_query::PayloadArray::from(
                elements.into_boxed_slice(),
            ),
            readonly: false,
        })
    };
    let large = tuple((0..9_999).map(|_| element(zero, false)).collect());
    let inner = tuple(vec![element(zero, false), element(large, true)]);
    let outer = [element(zero, false), element(inner, true)];
    let super::build::NormalizedTupleShape::Tuple(elements) =
        dispatch.normalize_tuple_spread(&outer, true)
    else {
        panic!("only the inner tuple refuses");
    };
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].value, zero);
    assert!(!elements[0].rest);
    assert!(elements[1].rest);
    assert_eq!(
        recovery(&dispatch, elements[1].value),
        Some((TS2799, RecoveryBasis::Budget, Some(inner)))
    );
}

/// An empty tuple repeatedly shared on both sides of a spread is planned
/// once per node. Its compact copy plan skips every empty expansion.
#[test]
fn shared_empty_spreads_have_bounded_construction_work() {
    let host = default_probe_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = dispatch.graph();
    let mut node = graph.intern_node(SemanticNodeData::Tuple {
        elements: verter_type_engine::semantic_query::PayloadArray::from(
            Vec::new().into_boxed_slice(),
        ),
        readonly: false,
    });
    for _ in 0..30 {
        node = graph.intern_node(SemanticNodeData::Tuple {
            elements: verter_type_engine::semantic_query::PayloadArray::from(
                (0..2)
                    .map(|_| TupleElement {
                        label: None,
                        value: node,
                        optional: false,
                        rest: true,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            readonly: false,
        });
    }
    dispatch.set_connected_limits_for_tests(256, 24);
    let _demand = dispatch.enter_connected_demand(false).0;
    let shape = dispatch.normalize_tuple_spread(
        &[TupleElement {
            label: None,
            value: node,
            optional: false,
            rest: true,
        }],
        false,
    );
    assert!(
        matches!(shape, super::build::NormalizedTupleShape::Tuple(elements) if elements.is_empty())
    );
    assert!(dispatch.connected_demand().work_used_for_tests() <= 100);
    assert!(!dispatch.connected_demand_tripped());
}

/// Recovery context survives a Pending boundary and thread migration.
/// The held type flight cannot answer the expression demand's subscription.
#[test]
fn a_suspended_indexed_call_keeps_its_recovery_site_on_another_thread() {
    use verter_type_engine::resolver_core::fact_validation_port::FactValidation as _;
    use verter_type_engine::semantic_query::{
        ProjectionMode, ProjectionReductionContext, QueryResult, ScopeId, SemanticQueryValue,
        ValueRootKey,
    };
    use verter_type_engine::semantic_query_memo::{Claim, ReadCapture};
    let host = default_probe_host();
    let source = format!("declare const z: {};\n{CONCAT}declare function twice<T extends unknown[]>(a: T): Concat<T, T>;\nexport const result = twice(z);", zeros(5_000));
    with_probe_on_host(&host, ProbeProject::default(), &source, "1", |_, _| ());
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = dispatch.graph();
    let key = |name: &str| {
        dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    PROBE_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: verter_type_expr::OwnedStr::from(name),
            },
            ProjectionReductionContext::published(ProjectionMode::Navigate),
        )
    };
    let type_task = graph.task_registry_for_tests().register_task();
    let expression_task = graph.task_registry_for_tests().register_task();
    let mut type_capture = ReadCapture::default();
    let mut type_attempt = graph
        .begin_query_claim(&*host, host.request_flags(), key("z"), &mut type_capture)
        .unwrap_or_else(|_| panic!("the type demand is cold"));
    let Claim::Produce(type_lease) = graph.claim_query(
        &*host,
        host.request_flags(),
        &mut type_attempt,
        &type_task,
        &mut type_capture,
    ) else {
        panic!("the type demand produces");
    };
    let mut expression_capture = ReadCapture::expression_for_tests();
    let mut expression_attempt = graph
        .begin_query_claim(
            &*host,
            host.request_flags(),
            key("z"),
            &mut expression_capture,
        )
        .unwrap_or_else(|_| panic!("the expression demand is cold"));
    let Claim::Produce(expression_lease) = graph.claim_query(
        &*host,
        host.request_flags(),
        &mut expression_attempt,
        &expression_task,
        &mut expression_capture,
    ) else {
        panic!("different recovery contexts have separate flights");
    };
    let answer = dispatch.with_semantic_task_for_tests(key("result"), |slot| {
        let task = slot.as_mut().expect("the task is lent");
        let waker = std::task::Waker::noop();
        assert!(task.poll_for_tests(waker).is_pending());
        std::thread::scope(|scope| scope.spawn(|| assert!(task.poll_for_tests(waker).is_pending())).join().unwrap());
        drop(expression_lease);
        std::thread::scope(|scope| scope.spawn(|| {
            match task.poll_for_tests(waker) {
                std::task::Poll::Ready(verter_type_engine::project_semantic_dispatch::TaskOutcomeForTests::Answered(answer)) => answer,
                other => panic!("the expression resumes independently: {other:?}"),
            }
        }).join().unwrap())
    }).expect("the root is cold");
    let QueryResult::Value(SemanticQueryValue::TypeNode(node)) = answer else {
        panic!("a type is returned");
    };
    assert_eq!(
        recovery(&dispatch, node).map(|(diagnostic, basis, _)| (diagnostic, basis)),
        Some((TS2800, RecoveryBasis::Budget))
    );
    drop(type_lease);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    assert_eq!(dispatch.open_request_state_for_tests().open_cost_scopes, 0);
}

/// Rest discovery and transparent aliases use the same metered traversal
/// as nested tuples; sufficient credit preserves each unaffected position.
#[test]
fn long_aliases_and_authored_prefixes_obey_the_work_allowance() {
    let host = default_probe_host();
    let owner = ProjectSemanticDispatch::new(&*host);
    let graph = owner.graph();
    let zero = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(0.0),
    ));
    let element = |value, rest| TupleElement {
        label: None,
        value,
        optional: false,
        rest,
    };
    let tuple = graph.intern_node(SemanticNodeData::Tuple {
        elements: verter_type_engine::semantic_query::PayloadArray::from(
            vec![element(zero, false)].into_boxed_slice(),
        ),
        readonly: false,
    });
    let mut alias = tuple;
    for _ in 0..400 {
        alias = graph.intern_node(SemanticNodeData::Alias(alias));
    }
    let authored = (0..600)
        .map(|_| element(zero, false))
        .chain(std::iter::once(element(tuple, true)))
        .collect::<Vec<_>>();
    for (elements, expected_len) in [(vec![element(alias, true)], 1), (authored, 601)] {
        for allowance in [300, 2_000] {
            let dispatch = ProjectSemanticDispatch::new(&*host);
            dispatch.set_connected_limits_for_tests(allowance, 24);
            let _demand = dispatch.enter_connected_demand(false).0;
            let shape = dispatch.normalize_tuple_spread(&elements, true);
            if allowance == 300 {
                assert_eq!(dispatch.connected_demand().work_used_for_tests(), allowance);
                assert_eq!(
                    dispatch.connected_demand().charge(),
                    Err(PartialReasonSet::PROJECTION_WORK_LIMIT)
                );
                let super::build::NormalizedTupleShape::Array(node) = shape else {
                    panic!("unpaid traversal must not reconstruct the authored list");
                };
                assert!(
                    matches!(graph.node_data(node).as_deref(), Some(SemanticNodeData::Opaque(QueryError::IncompleteSemanticOperand { reasons })) if *reasons == PartialReasonSet::PROJECTION_WORK_LIMIT)
                );
            } else {
                assert!(
                    matches!(shape, super::build::NormalizedTupleShape::Tuple(elements) if elements.len() == expected_len && elements.iter().all(|e| e.value == zero && !e.rest))
                );
                assert!(!dispatch.connected_demand_tripped());
            }
        }
    }
}
