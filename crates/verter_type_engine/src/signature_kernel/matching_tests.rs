//! Signature matching as owned machines: a step asks its holder what the
//! graph cannot tell at rest, and resumes where it asked with every
//! comparison it decided kept.

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
/// asked at rest and every decided comparison is counted, and a forced
/// result is each candidate's declared return.
struct CountingTypes<'b> {
    binding: &'b EpochBinding,
    asked_at_rest: Cell<u64>,
    decided: Cell<u64>,
    returns: RefCell<Vec<(SignatureCandidate, TypeToken)>>,
}

impl<'b> CountingTypes<'b> {
    fn new(binding: &'b EpochBinding) -> Self {
        Self {
            binding,
            asked_at_rest: Cell::new(0),
            decided: Cell::new(0),
            returns: RefCell::new(Vec::new()),
        }
    }

    /// `(a: param) => ret` authored from node `source`, with one binder
    /// declared by node `binder` when given; its declared return is the
    /// result its read answers.
    fn leaf(&self, source: u64, binder: Option<u64>, param: u64, ret: u64) -> SignatureCandidate {
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
                params: vec![ParamInput {
                    name: None,
                    ty: SemanticNodeId(param),
                    optional: false,
                    includes_undefined: false,
                }],
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

    /// The answer to `request`, asked under `corr`.
    fn answer(
        &self,
        request: TypeRequest,
        corr: &[(SemanticNodeId, SemanticNodeId)],
    ) -> TypeAnswer {
        match request {
            TypeRequest::Identical { a, b } => TypeAnswer::Identical(
                RequestAnswers::identical(self, a, b, corr).expect("every identity is decided"),
            ),
            TypeRequest::ForcedResult(candidate) => {
                TypeAnswer::ForcedResult(self.result_of(candidate))
            }
        }
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
                given = Some(types.answer(request, composition.correspondence()));
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
