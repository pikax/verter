use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use verter_session::for_tests::{
    join_product, FlowProductBudget, FlowProductValue, FlowSemanticAlgebra, FlowTransferOutcome,
    GraphSemanticAlgebra, LiteralFreshness, LiteralProvenance, LiteralProvenanceResult,
    ReachingTypeProduct, SemanticGraphStore, WideningMembership,
};
use verter_session::semantic_query::{FlowGap, LiteralValue, SemanticNodeData, SemanticNodeId};

use super::{alloc_bytes, alloc_count, reset_alloc_counter};

#[derive(Clone, Copy, Debug)]
enum Payload {
    String,
    BigInt,
}

struct Fixture {
    graph: SemanticGraphStore,
    roots: Vec<SemanticNodeId>,
    partial: Arc<[SemanticNodeId]>,
    result: SemanticNodeId,
}

impl Fixture {
    fn new(input_count: usize, arms: usize, payload: Payload, payload_len: usize) -> Self {
        let graph = SemanticGraphStore::new();
        let literals: Vec<_> = (0..arms)
            .map(|index| {
                let value = format!("{}{index:04}", "7".repeat(payload_len));
                graph.intern_node(SemanticNodeData::Literal(match payload {
                    Payload::String => LiteralValue::String(value),
                    Payload::BigInt => LiteralValue::BigInt(value),
                }))
            })
            .collect();
        let algebra = GraphSemanticAlgebra(&graph);
        // Keep the reaching semantic value identical across predecessors so
        // these windows isolate provenance work from composite construction.
        let union = algebra.union(&literals);
        assert!(!union.incomplete, "fixture construction must be complete");
        let result = union.node;
        let roots = vec![result; input_count];
        let mut partial = literals[..arms / 2].to_vec();
        partial.sort_unstable();
        Self {
            graph,
            roots,
            partial: partial.into(),
            result,
        }
    }

    fn inputs(&self) -> Vec<LiteralProvenance<'_>> {
        self.roots
            .iter()
            .enumerate()
            .map(|(index, root)| LiteralProvenance {
                root: Some(*root),
                fresh: match index % 3 {
                    0 => LiteralFreshness::All,
                    1 => LiteralFreshness::Partial(&self.partial),
                    _ => LiteralFreshness::Pinned,
                },
            })
            .collect()
    }

    fn products(&self) -> Vec<FlowProductValue> {
        self.roots
            .iter()
            .enumerate()
            .map(|(index, root)| {
                FlowProductValue::ReachingType(ReachingTypeProduct::of(*root).with_widening(
                    match index % 3 {
                        0 => Some(WideningMembership::All),
                        1 => Some(WideningMembership::Partial(Arc::clone(&self.partial))),
                        _ => None,
                    },
                ))
            })
            .collect()
    }

    fn expected(&self) -> LiteralProvenanceResult {
        if self.roots.len() == 2 {
            LiteralProvenanceResult::Partial(self.partial.to_vec())
        } else {
            LiteralProvenanceResult::None
        }
    }
}

#[derive(Debug)]
struct Cost {
    calls: u64,
    bytes: u64,
    ns: u128,
}

fn measure<T>(operation: impl FnOnce() -> T) -> (T, Cost) {
    reset_alloc_counter();
    let started = Instant::now();
    let output = black_box(operation());
    let ns = started.elapsed().as_nanos();
    let cost = Cost {
        calls: alloc_count(),
        bytes: alloc_bytes(),
        ns,
    };
    (output, cost)
}

fn fold_products(
    algebra: &GraphSemanticAlgebra<'_>,
    products: &[FlowProductValue],
) -> Result<FlowProductValue, FlowTransferOutcome> {
    let budget = FlowProductBudget {
        max_product_width: 4096,
        ..FlowProductBudget::default()
    };
    let mut result = products[0].clone();
    for input in &products[1..] {
        match join_product(algebra, &budget, &result, input) {
            FlowTransferOutcome::Unchanged => {}
            FlowTransferOutcome::Changed(next) => result = next,
            refusal => return Err(refusal),
        }
    }
    Ok(result)
}

#[test]
fn canonical_literal_provenance_allocation_is_bounded_and_borrows_payloads() {
    let mut complete = 0;
    let mut refused = 0;
    for input_count in [2, 8, 32, 128] {
        for arms in [16, 64, 256, 512] {
            let fixture = Fixture::new(input_count, arms, Payload::String, 8);
            let inputs = fixture.inputs();
            let products = fixture.products();
            let algebra = GraphSemanticAlgebra(&fixture.graph);
            // Keep graph interning, fixture allocation, and lazy initialization
            // outside both recorded windows. Neither path has a result memo.
            black_box(algebra.literal_provenance(&inputs, fixture.result)).ok();
            black_box(fold_products(&algebra, &products)).ok();
            let (batch, batch_cost) =
                measure(|| algebra.literal_provenance(&inputs, fixture.result));
            let (fold, fold_cost) = measure(|| fold_products(&algebra, &products));
            let batch_status = match &batch {
                Ok(membership) => {
                    complete += 1;
                    assert_eq!(membership, &fixture.expected());
                    "complete"
                }
                Err(FlowGap::UnmodeledExpression) => {
                    // Every fixture node exists. The canonical inspection cap
                    // uses the existing incomplete-algebra typed gap contract.
                    refused += 1;
                    "inspection-budget-gap"
                }
                Err(gap) => panic!("unexpected provenance refusal: {gap:?}"),
            };
            let fold_status = match fold {
                Ok(FlowProductValue::ReachingType(product)) => {
                    let expected = fixture.expected();
                    match (product.widening(), expected) {
                        (None, LiteralProvenanceResult::None) => {}
                        (
                            Some(WideningMembership::Partial(actual)),
                            LiteralProvenanceResult::Partial(expected),
                        ) => assert_eq!(actual.as_ref(), expected),
                        other => panic!("unexpected folded membership: {other:?}"),
                    }
                    "complete"
                }
                Err(FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression)) => {
                    "canonical-budget-gap"
                }
                other => panic!("unexpected reaching-type fold result: {other:?}"),
            };
            eprintln!("literal-provenance inputs={input_count} arms={arms} batch={batch_status} batch_calls={} batch_bytes={} batch_ns={} fold={fold_status} fold_calls={} fold_bytes={} fold_ns={}", batch_cost.calls, batch_cost.bytes, batch_cost.ns, fold_cost.calls, fold_cost.bytes, fold_cost.ns);
        }
    }
    assert!(
        complete > 0 && refused > 0,
        "exercise admission and bounded refusal"
    );

    for payload in [Payload::String, Payload::BigInt] {
        let mut baseline = None;
        for payload_len in [8, 4096] {
            let fixture = Fixture::new(8, 64, payload, payload_len);
            let inputs = fixture.inputs();
            let algebra = GraphSemanticAlgebra(&fixture.graph);
            black_box(algebra.literal_provenance(&inputs, fixture.result)).unwrap();
            let (result, cost) = measure(|| algebra.literal_provenance(&inputs, fixture.result));
            assert_eq!(result.unwrap(), LiteralProvenanceResult::None);
            assert!(
                cost.calls > 0 && cost.bytes > 0,
                "measure actual structural scratch allocation"
            );
            let allocation = (cost.calls, cost.bytes);
            if let Some(short_payload) = baseline {
                assert_eq!(
                    allocation, short_payload,
                    "{payload:?} payload length must not cause literal payload copies"
                );
            } else {
                baseline = Some(allocation);
            }
            eprintln!("literal-provenance payload={payload:?} payload_len={payload_len} inputs=8 arms=64 calls={} bytes={} ns={}", cost.calls, cost.bytes, cost.ns);
        }
    }
}
