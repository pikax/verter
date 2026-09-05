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
    flow_graph_fixture_for_tests, join_product, solve_flow_products, DeclaredTypeProduct,
    DefiniteAssignment, FlowDomain, FlowNarrowingFact, FlowProductBudget, FlowProductBudgetAxis,
    FlowProductContext, FlowProductInputs, FlowProductKey, FlowProductKeyError, FlowProductSeeds,
    FlowProductSolveOutcome, FlowProductValue, FlowSemanticAlgebra, FlowTransferOutcome,
    GraphSemanticAlgebra, NarrowingProduct, ReachingTypeProduct, ReachingValueProduct,
    SemanticGraphStore,
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

    // The product-count axis is exact and retains nothing.
    let crowded = FlowProductBudget {
        max_products: 1,
        ..generous
    };
    let outcome = solve_flow_products(&ctx, &domains, &algebra, &crowded);
    let FlowProductSolveOutcome::BudgetExceeded(exceeded) = &outcome else {
        panic!("a solve exceeding the product cap must be budget-exhausted, got {outcome:?}")
    };
    assert_eq!(exceeded.axis, FlowProductBudgetAxis::Products);
    assert!(
        outcome.solution().is_none(),
        "an over-crowded solve retains no store either"
    );
}
