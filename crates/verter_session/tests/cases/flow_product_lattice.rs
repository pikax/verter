//! The flow product lattice: the per-domain product state the flow
//! evaluator holds its whole semantic state in, and the ONE join route
//! every merge point it models folds through.
//!
//! Two durable boundaries are pinned here.
//!
//! 1. **Joins are domain-specific.** Reaching types aggregate their
//!    contributors and let the CANONICAL type algebra construct the
//!    semantic result (the product layer owns product-state algebra, never
//!    type algebra); declared types agree or gap; definite assignment uses
//!    its declared lattice; and a guard fact survives a merge only when
//!    EVERY incoming edge established it.
//! 2. **The frame join is subject-order independent and normalizing.**
//!    The joined state does not depend on the order either side's products
//!    were written in, each domain applies its own missing-edge rule, and
//!    a join that intersects away every guard fact leaves the SLOT ABSENT
//!    — no fact stays ONE state rather than two, exactly as the write
//!    accessor represents it.
//!
//! The end-to-end legs of the same contract — the served value, the
//! discharge evidence, the warm candidate and the budget boundary under
//! equivalent request orders — live beside the evaluator in
//! `flow_return_frame_seal_tests`.

use verter_session::for_tests::{
    frame_product_key, join_frame_products, join_product, DeclaredTypeProduct, DefiniteAssignment,
    DefiniteAssignmentProduct, FlowBindingLayer, FlowDomain, FlowFrameBindings,
    FlowFrameJoinOutcome, FlowNarrowingFact, FlowProductBudget, FlowProductKeyError,
    FlowProductStore, FlowProductSubject, FlowProductValue, FlowSemanticAlgebra,
    FlowTransferOutcome, GraphSemanticAlgebra, NarrowingProduct, ReachingTypeProduct,
    SemanticGraphStore, FLOW_FRAME_DOMAINS,
};
use verter_session::semantic_query::{PrimitiveKind, SemanticNodeData, SemanticNodeId};

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

/// A guard fact on `subject` narrowing the whole subject to `to`.
fn narrowing_fact(subject: &FlowProductSubject, to: SemanticNodeId) -> FlowNarrowingFact {
    FlowNarrowingFact {
        subject: subject.clone(),
        path: std::sync::Arc::from([]),
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

/// Each product domain joins by ITS OWN rule, and every rule is
/// idempotent. The reaching-TYPE route additionally proves the ownership
/// split: the substrate aggregates the flow contributors, and the semantic
/// composite over them is the one the CANONICAL type algebra constructs —
/// a flow-private union would not equal it.
#[test]
fn binding_domain_joins_are_domain_specific() {
    let (graph, ids) = graph_with(3);
    let algebra = GraphSemanticAlgebra(&graph);
    let budget = FlowProductBudget::default();
    let mut bindings = FlowFrameBindings::new();
    let first = bindings.subject(FlowBindingLayer::Lexical, "y");
    let second = bindings.subject(FlowBindingLayer::Lexical, "z");

    // Reaching types: contributors aggregate here, the composite is the
    // canonical algebra's. Contributor order is FIRST CONTRIBUTION: it is
    // the arm list the algebra unions, and arm order is observable in the
    // composite it constructs, so the substrate must hand the algebra the
    // order the flow produced rather than a re-sorted one.
    let left = FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[1]));
    let right = FlowProductValue::ReachingType(ReachingTypeProduct::of(ids[0]));
    let union = joined(&algebra, &left, &right);
    let FlowProductValue::ReachingType(product) = &union else {
        panic!("the reaching-type join stays in its domain")
    };
    assert_eq!(
        product.contributors(),
        &[ids[1], ids[0]],
        "reaching-type contributors aggregate in first-contribution order, \
         deduplicated"
    );
    let canonical = algebra.union(&[ids[1], ids[0]]);
    assert!(!canonical.incomplete);
    assert_eq!(
        product.united(),
        Some(canonical.node),
        "the united reaching type IS the canonical algebra's composite over \
         exactly the aggregated contributor list, not a flow-private union"
    );
    let mirrored = joined(&algebra, &right, &left);
    let FlowProductValue::ReachingType(mirrored) = &mirrored else {
        panic!("the reaching-type join stays in its domain")
    };
    assert_eq!(
        mirrored.contributors(),
        &[ids[0], ids[1]],
        "swapping the incoming edges hands the algebra the mirrored arm \
         order — the substrate never silently re-sorts what the algebra sees"
    );
    assert_eq!(
        join_product(&algebra, &budget, &union, &union),
        FlowTransferOutcome::Unchanged,
        "the reaching-type join is idempotent"
    );

    // Definite assignment: the declared lattice, not a set union.
    let unassigned = FlowProductValue::DefiniteAssignment(DefiniteAssignmentProduct::default());
    let assigned = FlowProductValue::DefiniteAssignment(DefiniteAssignmentProduct::assigned());
    assert_eq!(
        joined(&algebra, &unassigned, &assigned),
        FlowProductValue::DefiniteAssignment(
            DefiniteAssignmentProduct::default().with_state(DefiniteAssignment::MaybeAssigned)
        ),
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

    // Narrowing: a guard fact survives only on EVERY incoming edge.
    let shared = narrowing_fact(&first, ids[0]);
    let only_left = narrowing_fact(&second, ids[1]);
    let left = FlowProductValue::Narrowing(NarrowingProduct::new([shared.clone(), only_left]));
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

    // Declared types: a merge point never invents a declaration.
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

    // Cross-domain values never join into one another.
    assert!(
        matches!(
            join_product(&algebra, &budget, &assigned, &one),
            FlowTransferOutcome::Gap(_)
        ),
        "two different product kinds cannot be joined"
    );

    // The slot identity is the domain and the resolved subject, and
    // nothing else. Two scope layers of one authored name are two slots,
    // and a registry domain carrying no product has no key at all.
    let shadowing = bindings.subject(FlowBindingLayer::Function, "y");
    let lexical_slot = frame_product_key(FlowDomain::ReachingType, first.clone())
        .expect("the reaching-type domain carries a product");
    assert_eq!(
        lexical_slot,
        frame_product_key(FlowDomain::ReachingType, first.clone()).expect("the same slot"),
        "a product key is a function of its domain and subject alone"
    );
    assert_ne!(
        lexical_slot,
        frame_product_key(FlowDomain::ReachingType, shadowing)
            .expect("the function layer resolves its own slot"),
        "the two scope layers of one authored name are two slots"
    );
    assert_ne!(
        lexical_slot,
        frame_product_key(FlowDomain::Narrowing, first.clone()).expect("another domain"),
        "the domain is part of the slot identity"
    );
    assert_eq!(
        frame_product_key(FlowDomain::Effects, first),
        Err(FlowProductKeyError::DomainCarriesNoProduct),
        "a registry domain this substrate carries no product for has no slot"
    );
}

/// The frame join folds EVERY product domain by that domain's own rule,
/// independently of the order either side's products were written in, and
/// leaves a subject whose guard facts did not survive with NO narrowing
/// slot at all.
///
/// The last leg is the one a state diff cannot fake: `set_narrowing`
/// clears the slot on an empty fact set, so a join that filed an empty
/// product instead would make `narrowing(subject)` answer an empty product
/// after a merge and nothing after a write, for the same meaning.
#[test]
fn frame_join_folds_every_domain_by_its_own_rule() {
    let (graph, ids) = graph_with(3);
    let algebra = GraphSemanticAlgebra(&graph);
    let budget = FlowProductBudget::default();
    let mut bindings = FlowFrameBindings::new();
    let guarded = bindings.subject(FlowBindingLayer::Lexical, "guarded");
    let declared = bindings.subject(FlowBindingLayer::Lexical, "declared");
    let one_edge = bindings.subject(FlowBindingLayer::Function, "hoisted");
    let right_only = bindings.subject(FlowBindingLayer::Lexical, "rightOnly");
    let param = FlowFrameBindings::param(0);

    /// One product write, deferred so the SAME facts can be applied in
    /// either order.
    type Write<'a> = Box<dyn Fn(&mut FlowProductStore) + 'a>;

    // `reversed` decides only the order the SAME facts are written in.
    let build = |side: usize, reversed: bool| {
        let mut store = FlowProductStore::new();
        let mut writes: Vec<Write<'_>> = Vec::new();
        writes.push(Box::new(|store: &mut FlowProductStore| {
            store.set_reaching_type(&param, ReachingTypeProduct::of(ids[side]));
        }));
        writes.push(Box::new(|store: &mut FlowProductStore| {
            store.set_declared_type(&declared, Some(ids[2]));
        }));
        writes.push(Box::new(|store: &mut FlowProductStore| {
            store.set_assignment(
                &guarded,
                if side == 0 {
                    DefiniteAssignmentProduct::assigned()
                } else {
                    DefiniteAssignmentProduct::default()
                },
            );
        }));
        writes.push(Box::new(|store: &mut FlowProductStore| {
            // Both edges narrow `guarded`, but to DIFFERENT types, so the
            // intersection keeps nothing.
            store.set_narrowing(
                &guarded,
                NarrowingProduct::new([narrowing_fact(&guarded, ids[side])]),
            );
        }));
        if side == 0 {
            writes.push(Box::new(|store: &mut FlowProductStore| {
                store.set_reaching_type(&one_edge, ReachingTypeProduct::of(ids[0]));
            }));
        } else {
            // Narrowed on the RIGHT edge only: the intersection drops it,
            // and the slot it would be dropped from is already absent on
            // the left. A fold that called that a MOVE would re-ready the
            // subject every pass and exhaust the iteration budget instead
            // of converging.
            writes.push(Box::new(|store: &mut FlowProductStore| {
                store.set_narrowing(
                    &right_only,
                    NarrowingProduct::new([narrowing_fact(&right_only, ids[1])]),
                );
            }));
        }
        if reversed {
            writes.reverse();
        }
        for write in writes {
            write(&mut store);
        }
        store
    };

    let fold = |left: &FlowProductStore, right: &FlowProductStore| match join_frame_products(
        &algebra, &budget, left, right,
    ) {
        FlowFrameJoinOutcome::Joined(store) => store,
        other => panic!("the frame join converges over these states, got {other:?}"),
    };

    let baseline = fold(&build(0, false), &build(1, false));
    let permuted = fold(&build(0, true), &build(1, true));

    for domain in FLOW_FRAME_DOMAINS {
        assert_eq!(
            baseline.subjects_in(domain),
            permuted.subjects_in(domain),
            "{domain:?}: the joined subject set does not depend on write order"
        );
    }
    assert_eq!(
        baseline.reaching(&param),
        permuted.reaching(&param),
        "the joined parameter type does not depend on write order"
    );

    // Reaching types: both edges contribute, and the composite is the
    // canonical algebra's over exactly those contributors.
    let expected = algebra.union(&[ids[0], ids[1]]);
    assert!(!expected.incomplete);
    assert_eq!(
        baseline.reaching(&param),
        Some(expected.node),
        "both incoming edges contribute to the merged reaching type"
    );

    // Declared types agree, so the declaration survives untouched.
    assert_eq!(
        baseline.declared_type(&declared),
        Some(ids[2]),
        "agreeing declarations survive the merge"
    );

    // Definite assignment joins by its lattice.
    assert_eq!(
        baseline.assignment(&guarded),
        DefiniteAssignmentProduct::default().with_state(DefiniteAssignment::MaybeAssigned),
        "one assigning and one non-assigning edge join to MaybeAssigned"
    );

    // A value only one edge knows about survives as that edge's; a guard
    // fact only one edge established does not.
    assert_eq!(
        baseline.reaching(&one_edge),
        Some(ids[0]),
        "a reaching type only one edge holds survives the merge"
    );
    assert!(
        baseline.narrowing(&guarded).is_none(),
        "a guard fact that did not survive the intersection leaves NO \
         narrowing slot: an empty product is not a second way to say that \
         the subject holds no fact"
    );
    assert!(
        !baseline
            .subjects_in(FlowDomain::Narrowing)
            .contains(&guarded),
        "the emptied narrowing subject is gone from the domain's subject set"
    );
    assert!(
        baseline.narrowing(&right_only).is_none()
            && !baseline
                .subjects_in(FlowDomain::Narrowing)
                .contains(&right_only),
        "a guard fact only the incoming edge established survives nowhere, \
         and clearing its already-absent slot converges rather than \
         re-readying the subject"
    );
}
