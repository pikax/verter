//! The flow product lattice: per-domain product state over one store-bound
//! function flow graph.
//!
//! Three durable boundaries are pinned here.
//!
//! 1. **Joins are domain-specific.** Reaching definitions canonicalize as a
//!    SET; reaching types aggregate their contributors and let the
//!    CANONICAL type algebra construct the semantic result (the product
//!    layer owns product-state algebra, never type algebra); definite
//!    assignment uses its declared lattice; and a guard fact survives a
//!    merge only when EVERY incoming edge established it. Every route is
//!    idempotent and permutation-stable.
//! 2. **The worklist is deterministic.** Permuting the caller's domain
//!    list and the seed insertion order cannot move the visitation order,
//!    the products, or the solution's canonical bytes.
//! 3. **The budget boundary is exact and never warm.** A solve that
//!    stabilizes AT the iteration cap converges; one that would need
//!    another iteration returns typed budget exhaustion and hands back no
//!    store at all — a degraded solve has nothing to retain.

use verter_session::for_tests::{
    flow_graph_fixture_for_tests, join_product, product_route, solve_flow_products,
    transfer_product, DeclaredTypeProduct, DefiniteAssignment, FlowDomain, FlowNarrowingFact,
    FlowProductBudget, FlowProductBudgetAxis, FlowProductContext, FlowProductInputs,
    FlowProductKey, FlowProductKeyError, FlowProductSeeds, FlowProductSolveOutcome,
    FlowProductValue, FlowSemanticAlgebra, FlowTransferOutcome, GraphSemanticAlgebra,
    NarrowingProduct, ReachingTypeProduct, ReachingValueProduct, SemanticGraphStore,
};
use verter_session::semantic_query::{PrimitiveKind, SemanticNodeData, SemanticNodeId};

use verter_identity::encoding::CanonicalEncode;

/// A body with a parameter, two locals and a returned object literal: the
/// graph carries binding hubs, expression sites, a return site and a
/// region, so every product domain has a non-trivial subject set.
const FIXTURE: &str = r#"
function products(x) {
  const y = x;
  const z = { value: y };
  return { first: y, second: z };
}
"#;

fn graph_with(nodes: usize) -> (SemanticGraphStore, Vec<SemanticNodeId>) {
    let graph = SemanticGraphStore::new();
    let kinds = [
        PrimitiveKind::Number,
        PrimitiveKind::String,
        PrimitiveKind::Boolean,
    ];
    let ids = (0..nodes)
        .map(|index| graph.intern_node(SemanticNodeData::Primitive(kinds[index % kinds.len()])))
        .collect();
    (graph, ids)
}

fn inputs() -> FlowProductInputs {
    flow_graph_fixture_for_tests(FIXTURE, 41).product_inputs()
}

/// The first binding node key of `domain` — every fixture binding resolves
/// a stable cross-frame identity, so this never fabricates a slot.
fn binding_key(inputs: &FlowProductInputs, domain: FlowDomain, ordinal: usize) -> FlowProductKey {
    let mut seen = 0usize;
    for index in 0..inputs.graph().node_count() {
        let node = inputs.graph().node_at(index).expect("dense node space");
        let Ok(key) = inputs.key(domain, node) else {
            continue;
        };
        if key.binding().is_some() {
            if seen == ordinal {
                return key;
            }
            seen += 1;
        }
    }
    panic!("the fixture body has at least {} bindings", ordinal + 1);
}

/// The first binding-node key of `domain` whose graph site actually WRITES
/// its binding — a hub with a value-provider out-edge. That is the shape
/// the narrowing transfer's kill rule is defined over.
fn writing_binding_key(inputs: &FlowProductInputs, domain: FlowDomain) -> FlowProductKey {
    let graph = inputs.graph();
    for index in 0..graph.node_count() {
        let node = graph.node_at(index).expect("dense node space");
        let Ok(key) = inputs.key(domain, node) else {
            continue;
        };
        if key.binding().is_some()
            && graph.out_edges(node).iter().any(|edge| {
                matches!(
                    edge.kind.class(),
                    verter_semantic::analysis::flow::flow_graph::FlowEdgeClass::ValueDef
                        | verter_semantic::analysis::flow::flow_graph::FlowEdgeClass::PathWrite
                )
            })
        {
            return key;
        }
    }
    panic!("the fixture body has at least one written binding");
}

fn narrowing_fact(
    inputs: &FlowProductInputs,
    ordinal: usize,
    to: SemanticNodeId,
) -> FlowNarrowingFact {
    let key = binding_key(inputs, FlowDomain::Narrowing, ordinal);
    FlowNarrowingFact {
        binding: key.binding().expect("a binding-node key").clone(),
        narrowed_to: to,
    }
}

fn joined(
    algebra: &dyn FlowSemanticAlgebra,
    a: &FlowProductValue,
    b: &FlowProductValue,
) -> FlowProductValue {
    match join_product(algebra, &FlowProductBudget::default(), a, b) {
        FlowTransferOutcome::Unchanged => a.clone(),
        FlowTransferOutcome::Changed(value) => value,
        other => panic!("the join must produce a product, got {other:?}"),
    }
}

/// Each product domain joins by ITS OWN rule, and every rule is idempotent
/// and permutation-stable. The reaching-TYPE route additionally proves the
/// ownership split: the substrate aggregates the flow contributors, and
/// the semantic composite over them is the one the CANONICAL type algebra
/// constructs — a flow-private union would not equal it.
#[test]
fn binding_domain_joins_are_domain_specific() {
    let inputs = inputs();
    let (graph, ids) = graph_with(3);
    let algebra = GraphSemanticAlgebra(&graph);
    let budget = FlowProductBudget::default();

    // ── Reaching definitions: a canonical SET union.
    let nodes: Vec<_> = (0..4)
        .map(|index| inputs.graph().node_at(index).expect("dense node space"))
        .collect();
    let left = FlowProductValue::ReachingValue(ReachingValueProduct::new([nodes[2], nodes[0]]));
    let right = FlowProductValue::ReachingValue(ReachingValueProduct::new([nodes[1], nodes[2]]));
    let union = joined(&algebra, &left, &right);
    let FlowProductValue::ReachingValue(product) = &union else {
        panic!("the reaching-value join stays in its domain")
    };
    assert_eq!(
        product.definitions(),
        &[nodes[0], nodes[1], nodes[2]],
        "reaching definitions canonicalize as a sorted, deduplicated set"
    );
    assert_eq!(
        union,
        joined(&algebra, &right, &left),
        "the reaching-value join is permutation-stable"
    );
    assert_eq!(
        join_product(&algebra, &budget, &union, &union),
        FlowTransferOutcome::Unchanged,
        "the reaching-value join is idempotent"
    );

    // ── Reaching types: contributors aggregate here, the composite is the
    // canonical algebra's. Contributor order must not reach the algebra.
    let left = FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[1]));
    let right = FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[0]));
    let union = joined(&algebra, &left, &right);
    let FlowProductValue::ReachingType(product) = &union else {
        panic!("the reaching-type join stays in its domain")
    };
    assert_eq!(
        product.contributors(),
        &[ids[0], ids[1]],
        "reaching-type contributors canonicalize as a sorted, deduplicated set"
    );
    let canonical = algebra.union(&[ids[0], ids[1]]);
    assert!(!canonical.incomplete);
    assert_eq!(
        product.united(),
        Some(canonical.node),
        "the united reaching type IS the canonical algebra's composite, not a \
         flow-private union"
    );
    assert_eq!(
        union,
        joined(&algebra, &right, &left),
        "the reaching-type join is permutation-stable"
    );
    assert_eq!(
        join_product(&algebra, &budget, &union, &union),
        FlowTransferOutcome::Unchanged,
        "the reaching-type join is idempotent"
    );

    // ── Definite assignment: the declared lattice, not a set union.
    let unassigned = FlowProductValue::DefiniteAssignment(DefiniteAssignment::Unassigned);
    let assigned = FlowProductValue::DefiniteAssignment(DefiniteAssignment::Assigned);
    assert_eq!(
        joined(&algebra, &unassigned, &assigned),
        FlowProductValue::DefiniteAssignment(DefiniteAssignment::MaybeAssigned),
        "one assigning and one non-assigning edge join to MaybeAssigned"
    );
    assert_eq!(
        joined(&algebra, &assigned, &unassigned),
        joined(&algebra, &unassigned, &assigned),
        "the definite-assignment join is permutation-stable"
    );
    assert_eq!(
        join_product(&algebra, &budget, &assigned, &assigned),
        FlowTransferOutcome::Unchanged,
        "the definite-assignment join is idempotent"
    );
    assert_eq!(
        joined(&algebra, &unassigned, &unassigned),
        unassigned,
        "two non-assigning edges stay Unassigned"
    );

    // ── Narrowing: a guard fact survives only on EVERY incoming edge.
    let shared = narrowing_fact(&inputs, 0, ids[0]);
    let only_left = narrowing_fact(&inputs, 1, ids[1]);
    let left =
        FlowProductValue::Narrowing(NarrowingProduct::new([shared.clone(), only_left.clone()]));
    let right = FlowProductValue::Narrowing(NarrowingProduct::new([shared.clone()]));
    let survived = joined(&algebra, &left, &right);
    let FlowProductValue::Narrowing(product) = &survived else {
        panic!("the narrowing join stays in its domain")
    };
    assert_eq!(
        product.facts(),
        std::slice::from_ref(&shared),
        "a narrowing established on only one incoming edge does not survive the join"
    );
    assert_eq!(
        survived,
        joined(&algebra, &right, &left),
        "the narrowing join is permutation-stable"
    );
    assert_eq!(
        join_product(&algebra, &budget, &survived, &survived),
        FlowTransferOutcome::Unchanged,
        "the narrowing join is idempotent"
    );

    // ── Declared types: a merge point never invents a declaration.
    let one = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(ids[0]));
    let other = FlowProductValue::DeclaredType(DeclaredTypeProduct::of(ids[1]));
    assert_eq!(
        join_product(&algebra, &budget, &one, &one),
        FlowTransferOutcome::Unchanged,
        "agreeing declarations join to themselves"
    );
    assert!(
        matches!(
            join_product(&algebra, &budget, &one, &other),
            FlowTransferOutcome::Gap(_)
        ),
        "conflicting declared types are a typed gap, never a fabricated merge"
    );

    // ── Cross-domain values never join into one another.
    assert!(
        matches!(
            join_product(&algebra, &budget, &assigned, &one),
            FlowTransferOutcome::Gap(_)
        ),
        "two different product kinds cannot be joined"
    );

    // ── A binding key carries the binding's stable cross-frame identity;
    // a non-binding node carries none. Both are the same key constructor.
    let binding = binding_key(&inputs, FlowDomain::ReachingValue, 0);
    let identity = binding.binding().expect("a binding node names its slot");
    assert_eq!(identity.name.as_ref(), "x");
    assert_eq!(
        binding,
        inputs
            .key(FlowDomain::ReachingValue, binding.node())
            .expect("the same node mints the same key"),
        "a product key is a function of its domain and node alone"
    );
    assert_ne!(
        binding,
        inputs
            .key(FlowDomain::Narrowing, binding.node())
            .expect("the same node in another domain"),
        "the domain is part of the slot identity"
    );

    // A productless registry domain has no key at all.
    assert_eq!(
        inputs.key(FlowDomain::Effects, binding.node()),
        Err(FlowProductKeyError::DomainCarriesNoProduct)
    );
}

/// The requested domain order and the seed insertion order are CALLER
/// input, not solve identity: the visitation order, the products, and the
/// solution's canonical bytes are identical across every permutation.
#[test]
fn flow_product_worklist_is_permutation_deterministic() {
    let inputs = inputs();
    let (graph, ids) = graph_with(3);
    let algebra = GraphSemanticAlgebra(&graph);
    let budget = FlowProductBudget::default();

    let domains = [
        FlowDomain::ReachingValue,
        FlowDomain::ReachingType,
        FlowDomain::Narrowing,
        FlowDomain::DefiniteAssignment,
    ];

    // The seeds, described independently of the order they are inserted in.
    let mut described: Vec<(FlowProductKey, FlowProductValue)> = Vec::new();
    for ordinal in 0..3 {
        let value_key = binding_key(&inputs, FlowDomain::ReachingValue, ordinal);
        described.push((
            value_key.clone(),
            FlowProductValue::ReachingValue(ReachingValueProduct::new([value_key.node()])),
        ));
        described.push((
            binding_key(&inputs, FlowDomain::ReachingType, ordinal),
            FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[ordinal % ids.len()])),
        ));
        described.push((
            binding_key(&inputs, FlowDomain::DefiniteAssignment, ordinal),
            FlowProductValue::DefiniteAssignment(if ordinal % 2 == 0 {
                DefiniteAssignment::Assigned
            } else {
                DefiniteAssignment::Unassigned
            }),
        ));
        described.push((
            binding_key(&inputs, FlowDomain::Narrowing, ordinal),
            FlowProductValue::Narrowing(NarrowingProduct::new([narrowing_fact(
                &inputs,
                ordinal,
                ids[ordinal % ids.len()],
            )])),
        ));
    }

    let solve = |domain_order: &[FlowDomain], seed_order: &[usize]| {
        let mut seeds = FlowProductSeeds::new();
        for index in seed_order {
            let (key, value) = described[*index].clone();
            seeds.insert(key, value).expect("a seed matches its domain");
        }
        let ctx = FlowProductContext::new(&inputs, &seeds);
        match solve_flow_products(&ctx, domain_order, &algebra, &budget) {
            FlowProductSolveOutcome::Converged(solution) => solution,
            other => panic!("the seeded solve converges, got {other:?}"),
        }
    };

    let identity_seed_order: Vec<usize> = (0..described.len()).collect();
    let baseline = solve(&domains, &identity_seed_order);
    assert!(
        !baseline.store().is_empty() && baseline.visitation().len() >= baseline.store().len(),
        "the baseline solve computed products for the whole key universe"
    );
    let baseline_bytes = baseline.canonical_bytes();
    let baseline_visitation: Vec<_> = baseline.visitation().to_vec();

    // A deterministic pseudo-random permutation sweep: equivalent inputs
    // in many different orders. Seeded, so a failure reproduces exactly.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..24 {
        let mut domain_order = domains.to_vec();
        let mut seed_order = identity_seed_order.clone();
        for index in (1..domain_order.len()).rev() {
            domain_order.swap(index, (next() % (index as u64 + 1)) as usize);
        }
        for index in (1..seed_order.len()).rev() {
            seed_order.swap(index, (next() % (index as u64 + 1)) as usize);
        }

        let permuted = solve(&domain_order, &seed_order);
        assert_eq!(
            permuted.visitation(),
            baseline_visitation.as_slice(),
            "a permuted domain / seed order must not move the visitation order"
        );
        assert_eq!(
            permuted.iterations(),
            baseline.iterations(),
            "a permuted order must not change the fixed point's iteration count"
        );
        assert_eq!(
            permuted.canonical_bytes(),
            baseline_bytes,
            "a permuted order must produce byte-identical solution evidence"
        );
    }

    // A duplicated domain is the same solve, not a second pass over it.
    let duplicated: Vec<FlowDomain> = domains.iter().chain(domains.iter()).copied().collect();
    assert_eq!(
        solve(&duplicated, &identity_seed_order).canonical_bytes(),
        baseline_bytes,
        "a duplicated requested domain collapses onto one product universe"
    );

    // The visitation order is a function of the CANONICAL slot order, not
    // of any insertion: the first round visits every minted slot exactly
    // once, grouped by requested domain in the solve's canonical domain
    // order and ascending node index within it. That is what makes the
    // permutation sweep above exhaustive over caller-representable orders
    // — the ready set is an ordered set keyed by that same rank, so an
    // "insertion order" for it is not representable at all.
    let node_count = inputs.graph().node_count();
    let first_round = &baseline_visitation[..domains.len() * node_count];
    for (position, key) in first_round.iter().enumerate() {
        let expected_domain = position / node_count;
        let expected_index = position % node_count;
        assert_eq!(
            key.node(),
            inputs
                .graph()
                .node_at(expected_index)
                .expect("dense node space"),
            "slot {position} of the first round is out of canonical node order"
        );
        assert_eq!(
            key.domain(),
            canonical_domain_order(&domains)[expected_domain],
            "slot {position} of the first round is out of canonical domain order"
        );
    }

    // Discharge evidence, not just the byte digest: the store's ordered
    // entries and the iteration count are compared directly, so a
    // permutation that produced the same digest by a different route
    // would still have to produce the same products.
    let baseline_entries = ordered_entry_bytes(&baseline);
    let reversed_seed_order: Vec<usize> = identity_seed_order.iter().rev().copied().collect();
    let mut reversed_domains = domains.to_vec();
    reversed_domains.reverse();
    let reversed = solve(&reversed_domains, &reversed_seed_order);
    assert_eq!(
        ordered_entry_bytes(&reversed),
        baseline_entries,
        "reversing both the domain list and the seed order must produce the same products"
    );
    assert_eq!(reversed.iterations(), baseline.iterations());
    assert_eq!(reversed.visitation(), baseline_visitation.as_slice());

    // A product is re-readied only along the edge classes its OWN route
    // declares. The reaching-TYPE product propagates along value-provider
    // definitions alone, so a moved product re-readies only the sites that
    // read it — never the whole control region it happens to sit in.
    assert_eq!(
        product_route(FlowDomain::ReachingType),
        Some(&[verter_semantic::analysis::flow::flow_graph::FlowEdgeClass::ValueDef][..]),
        "the reaching-type product propagates along value definitions"
    );
    let type_only = [FlowDomain::ReachingType];
    let type_solution = solve(&type_only, &identity_seed_order);
    let exact = FlowProductBudget {
        max_iterations: type_solution.iterations(),
        ..budget
    };
    let mut seeds = FlowProductSeeds::new();
    for (key, value) in &described {
        seeds
            .insert(key.clone(), value.clone())
            .expect("a seed matches its domain");
    }
    let ctx = FlowProductContext::new(&inputs, &seeds);
    assert!(
        solve_flow_products(&ctx, &type_only, &algebra, &exact)
            .solution()
            .is_some(),
        "the reaching-type solve converges within the rounds its own edge route needs"
    );
    assert_eq!(
        (type_solution.iterations(), type_solution.visitation().len()),
        REACHING_TYPE_ONLY_WORK,
        "the reaching-type product visits exactly the slots its value-definition route \
         re-readies; requeueing consumers along the control-region and path-write \
         classes as well would visit more slots for the same answer"
    );
}

/// The `(rounds, slot visits)` a reaching-type-only solve over [`FIXTURE`]
/// performs. Pinned rather than recomputed: recomputing it would move with
/// any change to which edges re-ready a reaching-type consumer, which is
/// exactly what the assertion exists to catch.
const REACHING_TYPE_ONLY_WORK: (u32, usize) = (2, 18);

/// The requested domains in the order the solve canonicalizes them:
/// registry discriminant, deduplicated.
fn canonical_domain_order(domains: &[FlowDomain]) -> Vec<FlowDomain> {
    let mut ordered = domains.to_vec();
    ordered.sort_by_key(|domain| domain_rank(*domain));
    ordered.dedup();
    ordered
}

/// The registry rank of one domain, mirrored from the closed registry's
/// declared discriminants.
#[rustfmt::skip]
fn domain_rank(domain: FlowDomain) -> u32 {
    match domain {
        FlowDomain::ReachingValue => 1, FlowDomain::ReachingType => 2, FlowDomain::Narrowing => 3,
        FlowDomain::Completion => 4, FlowDomain::ClosureCapture => 5, FlowDomain::Freshness => 6,
        FlowDomain::Effects => 7, FlowDomain::CallResolution => 8, FlowDomain::Relation => 9,
        FlowDomain::ContextualTyping => 10, FlowDomain::Coverage => 11,
        FlowDomain::DeclaredType => 12, FlowDomain::DefiniteAssignment => 13,
    }
}

/// The solved products as canonical `(key, value)` byte pairs, in the
/// store's canonical key order.
fn ordered_entry_bytes(
    solution: &verter_session::for_tests::FlowProductSolution,
) -> Vec<(String, String)> {
    solution
        .store()
        .ordered_entries()
        .into_iter()
        .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
        .collect()
}

/// The iteration budget is an EXACT boundary, and exhausting it retains
/// nothing: the budget-exhausted arm carries no store, so a partial solve
/// cannot be read, retained, or warmed.
#[test]
fn flow_product_budget_boundary_is_exact_and_never_warm() {
    let inputs = inputs();
    let (graph, ids) = graph_with(2);
    let algebra = GraphSemanticAlgebra(&graph);

    let mut seeds = FlowProductSeeds::new();
    for ordinal in 0..3 {
        let key = binding_key(&inputs, FlowDomain::ReachingValue, ordinal);
        seeds
            .insert(
                key.clone(),
                FlowProductValue::ReachingValue(ReachingValueProduct::new([key.node()])),
            )
            .expect("a seed matches its domain");
        seeds
            .insert(
                binding_key(&inputs, FlowDomain::ReachingType, ordinal),
                FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[ordinal % ids.len()])),
            )
            .expect("a seed matches its domain");
    }
    let ctx = FlowProductContext::new(&inputs, &seeds);
    let domains = [FlowDomain::ReachingValue, FlowDomain::ReachingType];

    // The number of iterations this solve genuinely needs.
    let generous = FlowProductBudget {
        max_iterations: 64,
        ..FlowProductBudget::default()
    };
    let FlowProductSolveOutcome::Converged(reference) =
        solve_flow_products(&ctx, &domains, &algebra, &generous)
    else {
        panic!("the fixture solve converges under a generous budget")
    };
    let needed = reference.iterations();
    assert!(
        needed >= 2,
        "the fixture must need a real fixed point, not a single pass"
    );

    // AT the cap: the solve stabilizes and completes.
    let exact = FlowProductBudget {
        max_iterations: needed,
        ..FlowProductBudget::default()
    };
    let at_cap = solve_flow_products(&ctx, &domains, &algebra, &exact);
    let solution = at_cap
        .solution()
        .expect("a solve that stabilizes at the cap completes");
    assert_eq!(solution.iterations(), needed);
    assert_eq!(
        solution.canonical_bytes(),
        reference.canonical_bytes(),
        "the at-cap solve is the same answer as the generous one"
    );

    // One iteration short: typed exhaustion, and NOTHING retained.
    let short = FlowProductBudget {
        max_iterations: needed - 1,
        ..FlowProductBudget::default()
    };
    let outcome = solve_flow_products(&ctx, &domains, &algebra, &short);
    let FlowProductSolveOutcome::BudgetExceeded(exceeded) = &outcome else {
        panic!("a solve needing another iteration must be budget-exhausted, got {outcome:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Iterations);
    assert_eq!(exceeded.limit, needed - 1);
    assert!(
        outcome.solution().is_none(),
        "a budget-exhausted solve hands back no store — there is no partial \
         candidate to retain or warm"
    );

    // The width axis is exact at the join itself: a two-element product is
    // admitted at a cap of two and refused at a cap of one, and the
    // refusal carries no product for a caller to keep.
    let left = FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[0]));
    let right = FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[1]));
    let at_width = FlowProductBudget {
        max_product_width: 2,
        ..generous
    };
    assert!(
        matches!(
            join_product(&algebra, &at_width, &left, &right),
            FlowTransferOutcome::Changed(_)
        ),
        "a product exactly at the width cap is admitted"
    );
    let over_width = FlowProductBudget {
        max_product_width: 1,
        ..generous
    };
    let refused = join_product(&algebra, &over_width, &left, &right);
    let FlowTransferOutcome::BudgetExceeded(exceeded) = &refused else {
        panic!("a product past the width cap must be budget-exhausted, got {refused:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Width);
    assert_eq!((exceeded.limit, exceeded.observed), (1, 2));

    // The product-count axis is exact and retains nothing. It bounds the
    // product UNIVERSE, so it refuses before the key table is built —
    // not after a store has already grown past it.
    let crowded = FlowProductBudget {
        max_products: 1,
        ..generous
    };
    let outcome = solve_flow_products(&ctx, &domains, &algebra, &crowded);
    let FlowProductSolveOutcome::BudgetExceeded(exceeded) = &outcome else {
        panic!("a solve exceeding the product cap must be budget-exhausted, got {outcome:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Products);
    assert_eq!(
        exceeded.observed as usize,
        domains.len() * inputs.graph().node_count(),
        "the product cap is measured against the universe the solve would store"
    );
    assert!(
        outcome.solution().is_none(),
        "an over-crowded solve retains no store either"
    );

    // The width axis is a STORE invariant, not a join-local one: an
    // oversized gen-kill seed and an oversized narrowing seed each refuse
    // the whole solve, and neither hands back a partial store.
    let wide_nodes: Vec<_> = (0..3)
        .map(|index| inputs.graph().node_at(index).expect("dense node space"))
        .collect();
    let mut wide_value_seeds = FlowProductSeeds::new();
    wide_value_seeds
        .insert(
            binding_key(&inputs, FlowDomain::ReachingValue, 0),
            FlowProductValue::ReachingValue(ReachingValueProduct::new(wide_nodes.clone())),
        )
        .expect("a seed matches its domain");
    let wide_ctx = FlowProductContext::new(&inputs, &wide_value_seeds);
    let capped = FlowProductBudget {
        max_product_width: 2,
        ..generous
    };
    let outcome = solve_flow_products(&wide_ctx, &[FlowDomain::ReachingValue], &algebra, &capped);
    let FlowProductSolveOutcome::BudgetExceeded(exceeded) = &outcome else {
        panic!("an oversized gen-kill seed must exhaust the width axis, got {outcome:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Width);
    assert_eq!((exceeded.limit, exceeded.observed), (2, 3));
    assert!(
        outcome.solution().is_none(),
        "a width-exhausted solve retains no store"
    );

    let (wide_graph, wide_ids) = graph_with(3);
    let wide_algebra = GraphSemanticAlgebra(&wide_graph);
    let subject = binding_key(&inputs, FlowDomain::Narrowing, 0);
    let base = subject.binding().expect("a binding-node key").clone();
    let mut wide_narrow_seeds = FlowProductSeeds::new();
    wide_narrow_seeds
        .insert(
            subject.clone(),
            FlowProductValue::Narrowing(NarrowingProduct::new(wide_ids.iter().map(|id| {
                FlowNarrowingFact {
                    binding: base.clone(),
                    narrowed_to: *id,
                }
            }))),
        )
        .expect("a seed matches its domain");
    let wide_ctx = FlowProductContext::new(&inputs, &wide_narrow_seeds);
    let outcome = solve_flow_products(&wide_ctx, &[FlowDomain::Narrowing], &wide_algebra, &capped);
    let FlowProductSolveOutcome::BudgetExceeded(exceeded) = &outcome else {
        panic!("an oversized narrowing seed must exhaust the width axis, got {outcome:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Width);
    assert_eq!((exceeded.limit, exceeded.observed), (2, 3));
    assert!(
        outcome.solution().is_none(),
        "a width-exhausted narrowing solve retains no store either"
    );
}

/// A guard fact's subject is its binding's COMPLETE cross-frame identity,
/// so two facts that differ ONLY in the frame that declares the binding
/// are two facts: they canonicalize into a stable order whichever way the
/// caller supplies them, they do not deduplicate into one, and they encode
/// to different bytes.
///
/// Dropping the defining frame from the canonicalization would leave the
/// pair in caller insertion order (so the reversed construction would
/// compare unequal), and dropping it from the encoding would give the two
/// products identical bytes.
#[test]
fn cross_frame_narrowing_facts_canonicalize_on_their_defining_frame() {
    let inputs = inputs();
    let (graph, ids) = graph_with(2);
    let algebra = GraphSemanticAlgebra(&graph);
    let budget = FlowProductBudget::default();

    let here = narrowing_fact(&inputs, 0, ids[0]);
    // The SAME slot, name and kind, declared by a different frame: only
    // the defining-function axis differs.
    let mut elsewhere = here.clone();
    elsewhere.binding.defining_function.overload_ordinal += 1;
    assert_eq!(elsewhere.binding.binding_slot, here.binding.binding_slot);
    assert_eq!(elsewhere.binding.name, here.binding.name);
    assert_ne!(
        elsewhere.binding.defining_function,
        here.binding.defining_function
    );

    let forward = NarrowingProduct::new([here.clone(), elsewhere.clone()]);
    let reversed = NarrowingProduct::new([elsewhere.clone(), here.clone()]);
    assert_eq!(
        forward.facts().len(),
        2,
        "two frames' facts about the same slot are two facts, not one"
    );
    assert_eq!(
        forward, reversed,
        "the canonical order of two facts differing only in defining frame must not \
         depend on the order they were supplied in"
    );

    // The two single-fact products are distinguishable end to end: the
    // join intersects them to nothing, and neither absorbs the other.
    let only_here = FlowProductValue::Narrowing(NarrowingProduct::new([here.clone()]));
    let only_elsewhere = FlowProductValue::Narrowing(NarrowingProduct::new([elsewhere]));
    let intersected = joined(&algebra, &only_here, &only_elsewhere);
    let FlowProductValue::Narrowing(product) = &intersected else {
        panic!("the narrowing join stays in its domain")
    };
    assert!(
        product.facts().is_empty(),
        "a fact declared by another frame is not the same fact, so nothing survives the \
         intersection"
    );
    assert_ne!(
        join_product(&algebra, &budget, &only_here, &only_elsewhere),
        FlowTransferOutcome::Unchanged,
        "the two products are distinct values"
    );
}

/// The ONE transfer route, exercised directly at the boundary the product
/// substrate publishes rather than only through a solve: every domain's
/// node-local rule, and the width bound that applies to what a transfer
/// installs — not only to what a join produces.
#[test]
fn transfer_product_applies_the_domains_own_node_local_rule() {
    let inputs = inputs();
    let (_graph, ids) = graph_with(3);
    let budget = FlowProductBudget::default();

    let value_key = binding_key(&inputs, FlowDomain::ReachingValue, 0);
    let declared_key = binding_key(&inputs, FlowDomain::DeclaredType, 0);
    let nodes: Vec<_> = (0..3)
        .map(|index| inputs.graph().node_at(index).expect("dense node space"))
        .collect();

    // Gen-kill: a seeded site REPLACES what reached it; an unseeded one is
    // transparent; a seed equal to the incoming value does not move.
    let reached = FlowProductValue::ReachingValue(ReachingValueProduct::new([nodes[1]]));
    let established = FlowProductValue::ReachingValue(ReachingValueProduct::new([nodes[2]]));
    let mut seeds = FlowProductSeeds::new();
    seeds
        .insert(value_key.clone(), established.clone())
        .expect("a seed matches its domain");
    let ctx = FlowProductContext::new(&inputs, &seeds);
    assert_eq!(
        transfer_product(&ctx, &budget, &value_key, &reached),
        FlowTransferOutcome::Changed(established.clone()),
        "a site that establishes the fact replaces what reached it"
    );
    assert_eq!(
        transfer_product(&ctx, &budget, &value_key, &established),
        FlowTransferOutcome::Unchanged,
        "a site that establishes exactly what reached it does not move the product"
    );
    let bare = FlowProductSeeds::new();
    let unseeded = FlowProductContext::new(&inputs, &bare);
    assert_eq!(
        transfer_product(&unseeded, &budget, &value_key, &reached),
        FlowTransferOutcome::Unchanged,
        "a site that establishes nothing is transparent"
    );

    // Declared types MERGE and a genuine conflict is a typed gap — the arm
    // a solve over the other four domains never reaches.
    let mut seeds = FlowProductSeeds::new();
    seeds
        .insert(
            declared_key.clone(),
            FlowProductValue::DeclaredType(DeclaredTypeProduct::of(ids[0])),
        )
        .expect("a seed matches its domain");
    let ctx = FlowProductContext::new(&inputs, &seeds);
    assert_eq!(
        transfer_product(
            &ctx,
            &budget,
            &declared_key,
            &FlowProductValue::DeclaredType(DeclaredTypeProduct::default()),
        ),
        FlowTransferOutcome::Changed(FlowProductValue::DeclaredType(DeclaredTypeProduct::of(
            ids[0]
        ))),
        "a site declaring a type establishes it over an undeclared incoming"
    );
    assert_eq!(
        transfer_product(
            &ctx,
            &budget,
            &declared_key,
            &FlowProductValue::DeclaredType(DeclaredTypeProduct::of(ids[0])),
        ),
        FlowTransferOutcome::Unchanged,
        "an agreeing declaration does not move the product"
    );
    assert!(
        matches!(
            transfer_product(
                &ctx,
                &budget,
                &declared_key,
                &FlowProductValue::DeclaredType(DeclaredTypeProduct::of(ids[1])),
            ),
            FlowTransferOutcome::Gap(_)
        ),
        "two different declared types are a typed gap, never a silently overwritten one"
    );

    // Narrowing: a write at the slot's OWN binding hub kills the facts
    // naming that binding. The killed fact is NOT re-supplied by the site,
    // so the kill is observable; a fact about ANOTHER binding flows
    // through the same site untouched.
    let bare = FlowProductSeeds::new();
    let ctx = FlowProductContext::new(&inputs, &bare);
    let written = writing_binding_key(&inputs, FlowDomain::Narrowing);
    let subject = written.binding().expect("a binding-node key").clone();
    let killed = FlowNarrowingFact {
        binding: subject.clone(),
        narrowed_to: ids[2],
    };
    let mut other = subject.clone();
    other.binding_slot += 1;
    other.name = std::sync::Arc::from("another");
    let survives = FlowNarrowingFact {
        binding: other,
        narrowed_to: ids[0],
    };
    let outcome = transfer_product(
        &ctx,
        &budget,
        &written,
        &FlowProductValue::Narrowing(NarrowingProduct::new([killed, survives.clone()])),
    );
    let FlowTransferOutcome::Changed(FlowProductValue::Narrowing(product)) = &outcome else {
        panic!("a write to the subject must move the narrowing product, got {outcome:?}")
    };
    assert_eq!(
        product.facts(),
        std::slice::from_ref(&survives),
        "a write at the binding hub kills the guard facts naming THAT binding and leaves \
         every other binding's facts alone"
    );

    // The kill reads the GRAPH, not another domain's seed table: the same
    // answer with no reaching-value seeds anywhere is what proves it.
    assert!(
        bare.get(
            &inputs
                .key(FlowDomain::ReachingValue, written.node())
                .expect("the same node in the reaching-value domain")
        )
        .is_none(),
        "the kill above fired with no reaching-value seed in scope"
    );

    // The width bound applies to what a TRANSFER installs, not only to
    // what a join produces: an oversized gen-kill seed is refused and
    // carries no product for a caller to keep.
    let wide =
        FlowProductValue::ReachingValue(ReachingValueProduct::new([nodes[0], nodes[1], nodes[2]]));
    let mut seeds = FlowProductSeeds::new();
    seeds
        .insert(value_key.clone(), wide)
        .expect("a seed matches its domain");
    let ctx = FlowProductContext::new(&inputs, &seeds);
    let narrow_budget = FlowProductBudget {
        max_product_width: 2,
        ..FlowProductBudget::default()
    };
    let refused = transfer_product(&ctx, &narrow_budget, &value_key, &reached);
    let FlowTransferOutcome::BudgetExceeded(exceeded) = &refused else {
        panic!("an oversized transfer must be budget-exhausted, got {refused:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Width);
    assert_eq!((exceeded.limit, exceeded.observed), (2, 3));

    // A key on a productless registry domain never mints, so the transfer
    // route has no productless entrance at all.
    assert_eq!(
        inputs.key(FlowDomain::Coverage, value_key.node()),
        Err(FlowProductKeyError::DomainCarriesNoProduct)
    );
}

/// A frame whose two `dup` declarations live in different lexical scopes:
/// two distinct binders sharing one name and one kind. Whether the frame's
/// slot domain can separate them is a property of the identity authority,
/// not of this substrate — which is exactly why the substrate must not
/// assume it.
const SHADOWED_BINDERS: &str = r#"
function shadowed(flag) {
  if (flag) {
    const dup = 1;
    return dup;
  }
  const dup = 2;
  return dup;
}
"#;

/// A binding subject names ONE binder. Products here are subject-keyed —
/// a guard fact carries its subject's identity and the narrowing kill rule
/// compares by it — so two binders answering to one identity would let one
/// binder's write erase the other's facts, silently and completely.
///
/// The boundary is therefore not "the identities happen to be distinct"
/// but "an identity is never shared": either the frame separates its
/// binders, or the mint refuses them and the solve fails closed with
/// nothing retained. A mint that hands out a shared subject fails the
/// first leg; a mint that refuses one while the solve still converges over
/// the rest fails the second.
#[test]
fn a_binding_subject_never_names_two_binders() {
    for (source, tag) in [(FIXTURE, 41), (SHADOWED_BINDERS, 44)] {
        let inputs = flow_graph_fixture_for_tests(source, tag).product_inputs();
        let graph = inputs.graph();

        let mut subjects: Vec<verter_semantic::analysis::function_program::FlowBindingIdentity> =
            Vec::new();
        let mut refused = 0usize;
        for index in 0..graph.node_count() {
            let node = graph.node_at(index).expect("dense node space");
            match inputs.key(FlowDomain::Narrowing, node) {
                Ok(key) => {
                    let Some(subject) = key.binding() else {
                        continue;
                    };
                    assert!(
                        !subjects.contains(subject),
                        "two binder nodes of one frame minted the same product subject \
                         ({subject:?}); one binder's write would erase the other's facts"
                    );
                    subjects.push(subject.clone());
                }
                Err(FlowProductKeyError::AliasedBindingIdentity) => refused += 1,
                Err(other) => panic!("an ordinary binding node must mint, got {other:?}"),
            }
        }

        // The refusal is whole-solve, not per-slot: a frame with a shared
        // subject computes no products at all and retains no store, and a
        // frame whose binders are all separable converges.
        let (semantic, _) = graph_with(1);
        let algebra = GraphSemanticAlgebra(&semantic);
        let seeds = FlowProductSeeds::new();
        let ctx = FlowProductContext::new(&inputs, &seeds);
        let outcome = solve_flow_products(
            &ctx,
            &[FlowDomain::Narrowing],
            &algebra,
            &FlowProductBudget::default(),
        );
        if refused > 0 {
            let FlowProductSolveOutcome::Rejected(FlowProductKeyError::AliasedBindingIdentity) =
                &outcome
            else {
                panic!("a frame with a shared subject must refuse the whole solve, got {outcome:?}")
            };
            assert!(
                outcome.solution().is_none(),
                "a refused solve retains no store"
            );
        } else {
            assert!(
                outcome.solution().is_some(),
                "a frame whose binders are all separable solves normally, got {outcome:?}"
            );
        }
    }
}
