//! Signature matching as owned machines: a step asks its holder what the
//! graph cannot tell at rest, and resumes where it asked with every
//! comparison it decided and every parameter position it composed kept.

use std::cell::{Cell, RefCell};

use crate::semantic_query::{IncompleteReason, SemanticNodeId};

use super::discovery::{
    publish_signature, BinderInput, DiscoveryTypes, MatchOptions, ParamInput, ResultInput,
    SignatureInput,
};
use super::lifetime::{EpochBinding, SignatureStore};
use super::matching::{
    ForcedResult, KernelStep, SignatureComparison, SignatureComposition, TypeAnswer, TypeRequest,
};
use super::positional::SlotTypeFacts;
use super::provenance::{
    DeclarationGroupId, DeclarationParentId, SignatureProvenance, SourceLocatorId,
};
use super::records::{SignatureCandidate, SignatureKind, SignatureSemanticFlags, TypeToken};
use super::test_support::RequestAnswers;

/// Types whose identity is token equality, except under a correspondence
/// that moves a binder: those questions are the holder's. Every question
/// asked at rest, every decided comparison and every composed parameter
/// position is counted, and a forced result is each candidate's declared
/// return.
struct CountingTypes<'b> {
    binding: &'b EpochBinding,
    asked_at_rest: Cell<u64>,
    decided: Cell<u64>,
    composed: Cell<u64>,
    returns: RefCell<Vec<(SignatureCandidate, TypeToken)>>,
}

impl<'b> CountingTypes<'b> {
    fn new(binding: &'b EpochBinding) -> Self {
        Self {
            binding,
            asked_at_rest: Cell::new(0),
            decided: Cell::new(0),
            composed: Cell::new(0),
            returns: RefCell::new(Vec::new()),
        }
    }

    /// `(a: param) => ret` authored from node `source`, with one binder
    /// declared by node `binder` when given; its declared return is the
    /// result its read answers.
    fn leaf(&self, source: u64, binder: Option<u64>, param: u64, ret: u64) -> SignatureCandidate {
        self.leaf_of(source, binder, &[param], ret)
    }

    /// `(a: params[0], b: params[1], ..) => ret`, authored as
    /// [`Self::leaf`] is.
    fn leaf_of(
        &self,
        source: u64,
        binder: Option<u64>,
        params: &[u64],
        ret: u64,
    ) -> SignatureCandidate {
        let candidate = publish_signature(
            self.binding,
            &SignatureInput {
                kind: SignatureKind::Call,
                source: SemanticNodeId(source),
                space_key: source,
                space_identity: u128::from(source),
                binders: binder
                    .map(|binder| BinderInput {
                        name: "T".into(),
                        param: SemanticNodeId(binder),
                        constraint: None,
                        default: None,
                    })
                    .into_iter()
                    .collect(),
                receiver: None,
                params: params
                    .iter()
                    .map(|param| ParamInput {
                        name: None,
                        ty: SemanticNodeId(*param),
                        optional: false,
                        includes_undefined: false,
                    })
                    .collect(),
                rest: None,
                flags: SignatureSemanticFlags::NONE,
                result: ResultInput::Declared {
                    return_type: SemanticNodeId(ret),
                    predicate_or_assertion: None,
                },
                provenance: SignatureProvenance::authored(
                    DeclarationGroupId::from_raw(source),
                    DeclarationParentId::from_raw(0),
                    0,
                    0,
                    SourceLocatorId::from_raw(source),
                ),
            },
        )
        .expect("a leaf publishes");
        let token = self.token(ret);
        self.returns.borrow_mut().push((candidate, token));
        candidate
    }

    fn token(&self, node: u64) -> TypeToken {
        self.binding
            .intern_type_token(SemanticNodeId(node), None)
            .expect("a token interns")
    }

    fn result_of(&self, candidate: SignatureCandidate) -> ForcedResult {
        RequestAnswers::forced_result(self, &candidate).expect("every leaf declares its return")
    }

    /// The answer to `request`, asked where `composition` stands: under its
    /// correspondence, its binder map and over its operands.
    fn answer(&self, request: TypeRequest, composition: &SignatureComposition) -> TypeAnswer {
        match request {
            TypeRequest::Identical { a, b } => TypeAnswer::Identical(
                RequestAnswers::identical(self, a, b, composition.correspondence())
                    .expect("every identity is decided"),
            ),
            TypeRequest::ForcedResult(candidate) => {
                TypeAnswer::ForcedResult(self.result_of(candidate))
            }
            TypeRequest::MapBinders { ty } => TypeAnswer::Type(
                RequestAnswers::map_binders(self, ty, composition.binder_map())
                    .expect("every type maps"),
            ),
            TypeRequest::Intersect => TypeAnswer::Type(
                RequestAnswers::intersect(self, composition.operands())
                    .expect("an intersection has operands"),
            ),
        }
    }

    /// The layout parameter types of `candidate`, read in the binding.
    fn parameter_types(&self, candidate: SignatureCandidate) -> Vec<TypeToken> {
        let view = self.binding.view();
        let descriptor = view
            .descriptor(candidate.signature)
            .expect("the candidate's descriptor reads");
        let template = view
            .template(descriptor.template)
            .expect("its template reads");
        let shape = view.shape(template.input_shape).expect("its shape reads");
        view.layout(shape.parameter_layout)
            .expect("its layout reads")
            .parameters
            .iter()
            .map(|slot| slot.ty)
            .collect()
    }
}

impl SlotTypeFacts for CountingTypes<'_> {
    fn accepts_void(&self, _: TypeToken) -> bool {
        false
    }
}

impl DiscoveryTypes for CountingTypes<'_> {
    fn identical_at_rest(
        &self,
        a: TypeToken,
        b: TypeToken,
        corr: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        self.asked_at_rest.set(self.asked_at_rest.get() + 1);
        if corr.iter().any(|(x, y)| x != y) {
            return None;
        }
        Some(a == b)
    }
    fn comparison_decided(&self) {
        self.decided.set(self.decided.get() + 1);
    }
    fn parameter_composed(&self) {
        self.composed.set(self.composed.get() + 1);
    }
    fn unknown(&self) -> Option<TypeToken> {
        Some(self.token(9_000))
    }
    fn any(&self) -> Option<TypeToken> {
        Some(self.token(9_001))
    }
    fn is_any(&self, _: TypeToken) -> bool {
        false
    }
}

impl RequestAnswers for CountingTypes<'_> {
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        _: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        Some(a == b)
    }
    fn forced_result(
        &self,
        candidate: &SignatureCandidate,
    ) -> Result<ForcedResult, IncompleteReason> {
        self.returns
            .borrow()
            .iter()
            .find(|(known, _)| known == candidate)
            .map(|(_, token)| ForcedResult {
                return_type: *token,
                effects: None,
            })
            .ok_or(IncompleteReason::UnresolvedObligation)
    }
    fn intersect(&self, members: &[TypeToken]) -> Option<TypeToken> {
        members.first().copied()
    }
    fn map_binders(
        &self,
        ty: TypeToken,
        _: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<TypeToken> {
        Some(ty)
    }
}

/// An exact comparison of two signatures that differ only in their declared
/// return asks for the source's result, then the target's, and decides
/// only once both are given. The parameter is compared once, before the
/// first request, and never again as the comparison resumes.
#[test]
fn a_comparison_asks_for_each_result_once_and_resumes_at_the_position_it_asked_from() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let source = types.leaf(1, None, 100, 200);
    let target = types.leaf(2, None, 100, 201);
    let mut comparison = SignatureComparison::new(source, target, MatchOptions::EXACT);

    assert_eq!(
        comparison.step(&binding, &types, None),
        Ok(KernelStep::Ask(TypeRequest::ForcedResult(source)))
    );
    assert_eq!(
        (types.asked_at_rest.get(), types.decided.get()),
        (1, 0),
        "the parameter compared, nothing decided"
    );

    assert_eq!(
        comparison.step(
            &binding,
            &types,
            Some(TypeAnswer::ForcedResult(types.result_of(source)))
        ),
        Ok(KernelStep::Ask(TypeRequest::ForcedResult(target)))
    );
    assert_eq!(
        types.asked_at_rest.get(),
        1,
        "the parameter is not compared again"
    );

    assert_eq!(
        comparison.step(
            &binding,
            &types,
            Some(TypeAnswer::ForcedResult(types.result_of(target)))
        ),
        Ok(KernelStep::Done(false))
    );
    assert_eq!(
        (types.asked_at_rest.get(), types.decided.get()),
        (2, 1),
        "the returns compared once and the pair decided once"
    );
}

/// An intersection of three single-signature members whose signatures
/// differ only in their results asks for each compared pair's results in
/// order, and a comparison decided before a later result was outstanding
/// is never decided again: three comparisons for three members.
#[test]
fn an_intersection_keeps_its_decided_comparisons_while_a_later_result_is_outstanding() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let p = types.leaf(1, None, 100, 200);
    let q = types.leaf(2, None, 100, 201);
    let r = types.leaf(3, None, 100, 202);
    let mut composition = SignatureComposition::intersection(
        &binding,
        &types,
        SignatureKind::Call,
        vec![vec![p], vec![q], vec![r]],
    )
    .expect("the members load");

    let mut requests = Vec::new();
    let mut decided_at_request = Vec::new();
    let mut given = None;
    let found = loop {
        match composition
            .step(&binding, &types, given.take())
            .expect("every question is answered")
        {
            KernelStep::Done(found) => break found,
            KernelStep::Ask(request) => {
                requests.push(request);
                decided_at_request.push(types.decided.get());
                given = Some(types.answer(request, &composition));
            }
        }
    };

    assert_eq!(
        requests,
        [p, q, p, r, q, r].map(TypeRequest::ForcedResult),
        "each compared pair's results, source first, in member order"
    );
    assert_eq!(decided_at_request, [0, 0, 1, 1, 2, 2]);
    assert_eq!(
        found,
        vec![p, q, r],
        "overloads differing in result all stay"
    );
    assert_eq!(types.decided.get(), 3, "each pair decided once");
    assert_eq!(
        types.asked_at_rest.get(),
        6,
        "each comparison's parameter and returns compared once"
    );
}

/// A generic signature matched against another arm's generic signature
/// compares its positions under the binder correspondence of the two
/// spaces. Where that correspondence moves a binder the graph cannot tell
/// identity at rest, so the question is the holder's, asked under the
/// correspondence from the list candidate's binder to the signature's.
#[test]
fn a_generic_match_asks_identity_under_its_binder_correspondence() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let (first_binder, second_binder) = (5_001, 5_002);
    let first = types.leaf(1, Some(first_binder), first_binder, first_binder);
    let second = types.leaf(2, Some(second_binder), second_binder, second_binder);
    let mut composition = SignatureComposition::union(vec![vec![first], vec![second]]);

    assert_eq!(
        composition.step(&binding, &types, None),
        Ok(KernelStep::Ask(TypeRequest::Identical {
            a: types.token(second_binder),
            b: types.token(first_binder),
        })),
        "the parameters, the other arm's candidate first"
    );
    assert_eq!(
        composition.correspondence(),
        [(SemanticNodeId(second_binder), SemanticNodeId(first_binder))],
        "the list candidate's binder corresponds to the signature's"
    );
    assert_eq!(types.decided.get(), 0, "nothing is decided before it");
}

/// A union whose arms' parameters agree needs nothing the graph cannot
/// tell: its first step settles to the one composite candidate, each arm's
/// signature compared once against the other.
#[test]
fn a_composition_with_nothing_to_ask_settles_in_one_step() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let left = types.leaf(1, None, 100, 200);
    let right = types.leaf(2, None, 100, 201);
    let mut composition = SignatureComposition::union(vec![vec![left], vec![right]]);

    let Ok(KernelStep::Done(found)) = composition.step(&binding, &types, None) else {
        panic!("the union settles in its first step");
    };
    assert_eq!(found.len(), 1, "one common candidate");
    assert_ne!(found[0], left, "a composite over both arms");
    assert_eq!(
        (types.decided.get(), types.asked_at_rest.get()),
        (2, 2),
        "the left signature matched in the right arm, then the right one represented"
    );
}

/// A union of two two-parameter signatures with no common match is
/// synthesized position by position: each position's intersection is asked
/// once, over the master's type then the other arm's, and a step given an
/// answer resumes at the position it asked from, keeping every composed
/// position and every decided comparison. The synthesized signature's
/// parameters are the answered intersections.
#[test]
fn a_parameter_composition_asks_for_each_intersection_once_and_resumes_at_the_position_it_asked_from(
) {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let left = types.leaf_of(1, None, &[100, 101], 200);
    let right = types.leaf_of(2, None, &[110, 111], 201);
    let mut composition = SignatureComposition::union(vec![vec![left], vec![right]]);

    assert_eq!(
        composition.step(&binding, &types, None),
        Ok(KernelStep::Ask(TypeRequest::Intersect))
    );
    assert_eq!(
        composition.operands(),
        [types.token(100), types.token(110)],
        "the first position, the master's type first"
    );
    assert_eq!(types.composed.get(), 0, "nothing composed before it");
    assert_eq!(
        (types.decided.get(), types.asked_at_rest.get()),
        (4, 4),
        "each arm's signature compared exactly, then partially, in the other arm"
    );

    assert_eq!(
        composition.step(&binding, &types, Some(TypeAnswer::Type(types.token(300)))),
        Ok(KernelStep::Ask(TypeRequest::Intersect))
    );
    assert_eq!(
        composition.operands(),
        [types.token(101), types.token(111)],
        "the second position"
    );
    assert_eq!(types.composed.get(), 1, "the first position composed once");
    assert_eq!(
        (types.decided.get(), types.asked_at_rest.get()),
        (4, 4),
        "no comparison is made again"
    );

    let Ok(KernelStep::Done(found)) =
        composition.step(&binding, &types, Some(TypeAnswer::Type(types.token(301))))
    else {
        panic!("the synthesis is done once the last position is answered");
    };
    assert_eq!(found.len(), 1, "one synthesized signature");
    assert_ne!(found[0], left, "a composite over both arms");
    assert_eq!(types.composed.get(), 2, "each position composed once");
    assert_eq!(
        types.parameter_types(found[0]),
        [types.token(300), types.token(301)],
        "the synthesized parameters are the answered intersections"
    );
}

/// A synthesis of two generic signatures whose binder spaces match asks
/// for the other arm's types under the map from its binders onto the
/// master's, and intersects the master's type with the mapped one.
#[test]
fn a_generic_parameter_composition_asks_for_the_other_sides_types_under_its_binder_map() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let (first_binder, second_binder) = (5_001, 5_002);
    let first = types.leaf(1, Some(first_binder), first_binder, first_binder);
    let second = types.leaf(2, Some(second_binder), second_binder, second_binder);
    let mut composition = SignatureComposition::union(vec![vec![first], vec![second]]);

    let request = composition.step(&binding, &types, None);
    assert_eq!(
        request,
        Ok(KernelStep::Ask(TypeRequest::Identical {
            a: types.token(second_binder),
            b: types.token(first_binder),
        })),
        "the common match compares the parameters under the correspondence"
    );
    let Ok(KernelStep::Ask(request)) = request else {
        unreachable!("asserted above")
    };
    let answer = types.answer(request, &composition);

    assert_eq!(
        composition.step(&binding, &types, Some(answer)),
        Ok(KernelStep::Ask(TypeRequest::MapBinders {
            ty: types.token(second_binder),
        })),
        "the other arm's parameter, in its own binders"
    );
    assert_eq!(
        composition.binder_map(),
        [(SemanticNodeId(second_binder), SemanticNodeId(first_binder))],
        "the other arm's binder maps onto the master's"
    );

    assert_eq!(
        composition.step(
            &binding,
            &types,
            Some(TypeAnswer::Type(types.token(first_binder)))
        ),
        Ok(KernelStep::Ask(TypeRequest::Intersect))
    );
    assert_eq!(
        composition.operands(),
        [types.token(first_binder), types.token(first_binder)],
        "the master's parameter and the mapped one"
    );
    let answer = types.answer(TypeRequest::Intersect, &composition);

    let Ok(KernelStep::Done(found)) = composition.step(&binding, &types, Some(answer)) else {
        panic!("the synthesis is done once its one position is answered");
    };
    assert_eq!(found.len(), 1, "one synthesized signature");
    assert_eq!(types.composed.get(), 1, "its one position composed once");
}

/// Two generic signatures whose binder spaces are declared by the same
/// binder node combine with no binder rewritten: no type is asked for
/// under the binder map, and the position's intersection is the only
/// question the synthesis asks.
#[test]
fn binders_that_do_not_move_are_mapped_without_a_question() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let types = CountingTypes::new(&binding);
    let binder = 5_001;
    let first = types.leaf(1, Some(binder), binder, 200);
    let second = types.leaf(2, Some(binder), binder, 201);
    let mut composition = SignatureComposition::union(vec![vec![first], vec![second]]);

    let mut requests = Vec::new();
    let mut given = None;
    let found = loop {
        match composition
            .step(&binding, &types, given.take())
            .expect("every question is answered")
        {
            KernelStep::Done(found) => break found,
            KernelStep::Ask(request) => {
                if request == TypeRequest::Intersect {
                    assert_eq!(
                        composition.operands(),
                        [types.token(binder), types.token(binder)],
                        "both parameters as written"
                    );
                    assert_eq!(
                        composition.binder_map(),
                        [(SemanticNodeId(binder), SemanticNodeId(binder))],
                        "the map pairs the binder with itself"
                    );
                }
                requests.push(request);
                given = Some(types.answer(request, &composition));
            }
        }
    };

    assert_eq!(
        requests,
        [
            TypeRequest::ForcedResult(second),
            TypeRequest::ForcedResult(first),
            TypeRequest::Intersect,
        ],
        "the returns differ, so there is no common match; the synthesis maps nothing"
    );
    assert_eq!(found.len(), 1, "one synthesized signature");
    assert_eq!(types.composed.get(), 1, "its one position composed once");
}
