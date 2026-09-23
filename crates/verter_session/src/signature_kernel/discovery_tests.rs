//! Record-level discovery: precedence under delayed comparisons.

use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::semantic_query::{IncompleteReason, SemanticNodeId};

use super::discovery::{
    heritage_signatures, intersection_signatures, publish_signature, signatures_identical,
    union_signatures, BinderInput, DiscoveryError, DiscoveryTypes, MatchOptions, ParamInput,
    ResultInput, SignatureInput,
};
use super::lifetime::SignatureStore;
use super::positional::SlotTypeFacts;
use super::provenance::{
    DeclarationGroupId, DeclarationParentId, SignatureProvenance, SourceLocatorId,
};
use super::records::{SignatureCandidate, SignatureKind, SignatureSemanticFlags, TypeToken};

/// Types whose identity is token equality, except for pairs registered as
/// not-yet-settled: those answer `None` until the pair is released.
struct DelayedTypes<'s> {
    store: &'s SignatureStore,
    unsettled: RefCell<BTreeSet<(u64, u64)>>,
}

impl SlotTypeFacts for DelayedTypes<'_> {
    fn accepts_void(&self, _: TypeToken) -> bool {
        false
    }
}

fn ordered(a: TypeToken, b: TypeToken) -> (u64, u64) {
    (a.as_u64().min(b.as_u64()), a.as_u64().max(b.as_u64()))
}

impl DiscoveryTypes for DelayedTypes<'_> {
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        _: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        if self.unsettled.borrow().contains(&ordered(a, b)) {
            return None;
        }
        Some(a == b)
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
        self.store
            .intern_type_token(SemanticNodeId(9_000), None)
            .ok()
    }
    fn any(&self) -> Option<TypeToken> {
        self.store
            .intern_type_token(SemanticNodeId(9_001), None)
            .ok()
    }
    fn is_any(&self, _: TypeToken) -> bool {
        false
    }
    fn forced_return(&self, _: &SignatureCandidate) -> Result<TypeToken, IncompleteReason> {
        Err(IncompleteReason::UnresolvedObligation)
    }
}

fn sig(store: &SignatureStore, source: u64, param: u64, ret: u64) -> SignatureCandidate {
    sig_with(
        store,
        source,
        source,
        u128::from(source),
        vec![],
        param,
        ret,
    )
}

fn sig_with(
    store: &SignatureStore,
    source: u64,
    space_key: u64,
    space_identity: u128,
    binders: Vec<BinderInput>,
    param: u64,
    ret: u64,
) -> SignatureCandidate {
    publish_signature(
        store,
        &SignatureInput {
            kind: SignatureKind::Call,
            source: SemanticNodeId(source),
            space_key,
            space_identity,
            binders,
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
                DeclarationGroupId::from_raw(u64::from(source as u32)),
                DeclarationParentId::from_raw(0),
                0,
                0,
                SourceLocatorId::from_raw(0),
            ),
        },
    )
    .unwrap()
}

/// A later candidate that would match is never committed while an earlier
/// candidate of the same arm is unresolved: the union is incomplete until
/// the earlier comparison settles, and once it settles the answer does not
/// depend on which unrelated comparisons were still delayed.
#[test]
fn later_match_is_never_committed_while_an_earlier_candidate_is_unresolved() {
    let store = SignatureStore::new();
    let token = |n| store.intern_type_token(SemanticNodeId(n), None).unwrap();
    let (string, number, out_a, out_b) = (100, 101, 200, 201);
    let a = sig(&store, 1, string, out_a);
    // The arm lists its first overload BEFORE the one that matches `a`.
    let b_first = sig(&store, 2, number, out_b);
    let b_match = sig(&store, 3, string, out_b);
    let lists = vec![vec![a], vec![b_first, b_match]];

    let types = DelayedTypes {
        store: &store,
        unsettled: RefCell::new(BTreeSet::new()),
    };
    types
        .unsettled
        .borrow_mut()
        .insert(ordered(token(string), token(number)));

    assert_eq!(
        union_signatures(&store, &types, &lists),
        Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput)),
        "the earlier unresolved overload blocks the later successful one"
    );

    types.unsettled.borrow_mut().clear();
    let settled = union_signatures(&store, &types, &lists).unwrap();
    assert_eq!(settled.len(), 1);

    // Returns are not compared by a common match, so a delayed return pair
    // can neither block nor reorder it.
    let return_delay = DelayedTypes {
        store: &store,
        unsettled: RefCell::new(BTreeSet::from([ordered(token(out_a), token(out_b))])),
    };
    assert_eq!(
        union_signatures(&store, &return_delay, &lists).unwrap(),
        settled
    );
}

fn binder(default: Option<u64>) -> BinderInput {
    BinderInput {
        name: "T".into(),
        param: SemanticNodeId(5_000),
        constraint: None,
        default: default.map(SemanticNodeId),
    }
}

/// An omitted default is not an authored `unknown` default.
#[test]
fn absent_binder_default_is_not_identical_to_authored_unknown_default() {
    let store = SignatureStore::new();
    let types = DelayedTypes {
        store: &store,
        unsettled: RefCell::new(BTreeSet::new()),
    };
    let none_a = sig_with(&store, 1, 1, 1, vec![binder(None)], 100, 200);
    let none_b = sig_with(&store, 2, 2, 2, vec![binder(None)], 100, 200);
    let authored = sig_with(&store, 3, 3, 3, vec![binder(Some(9_000))], 100, 200);
    let same = |a, b| {
        signatures_identical(&store, &types, a, b, MatchOptions::EXACT_IGNORING_RETURNS).unwrap()
    };
    assert!(same(none_a, none_b));
    assert!(!same(none_a, authored));
    assert!(!same(authored, none_a));
}

/// Two logical identities that truncate to one binder-space key fail
/// closed rather than sharing binder tokens.
#[test]
fn colliding_space_keys_of_distinct_identities_are_rejected() {
    let store = SignatureStore::new();
    sig_with(&store, 1, 7, 1, vec![binder(None)], 100, 200);
    sig_with(&store, 2, 7, 1, vec![binder(None)], 100, 200);
    let collided = publish_signature(
        &store,
        &SignatureInput {
            kind: SignatureKind::Call,
            source: SemanticNodeId(3),
            space_key: 7,
            space_identity: 2,
            binders: vec![binder(None)],
            receiver: None,
            params: vec![],
            rest: None,
            flags: SignatureSemanticFlags::NONE,
            result: ResultInput::Declared {
                return_type: SemanticNodeId(200),
                predicate_or_assertion: None,
            },
            provenance: SignatureProvenance::authored(
                DeclarationGroupId::from_raw(3),
                DeclarationParentId::from_raw(0),
                0,
                0,
                SourceLocatorId::from_raw(0),
            ),
        },
    );
    assert_eq!(
        collided,
        Err(DiscoveryError::Store(
            super::lifetime::StoreError::BinderKeyCollision
        ))
    );
}

/// Two union arms settling to the same callable each keep their own edge,
/// so the result is a composite candidate rather than the bare authored one.
#[test]
fn repeated_arm_contributors_are_retained() {
    let store = SignatureStore::new();
    let types = DelayedTypes {
        store: &store,
        unsettled: RefCell::new(BTreeSet::new()),
    };
    let a = sig(&store, 1, 100, 200);
    let out = union_signatures(&store, &types, &[vec![a], vec![a]]).unwrap();
    assert_eq!(out.len(), 1);
    assert_ne!(out[0], a);
}

/// Types whose identity is token equality and whose forced return is each
/// candidate's DECLARED return, registered as the candidate is published.
struct DeclaredReturns<'s> {
    store: &'s SignatureStore,
    returns: RefCell<Vec<(SignatureCandidate, TypeToken)>>,
}

impl DeclaredReturns<'_> {
    fn declared(&self, source: u64, param: u64, ret: u64) -> SignatureCandidate {
        let candidate = sig(self.store, source, param, ret);
        let token = self
            .store
            .intern_type_token(SemanticNodeId(ret), None)
            .unwrap();
        self.returns.borrow_mut().push((candidate, token));
        candidate
    }
}

impl SlotTypeFacts for DeclaredReturns<'_> {
    fn accepts_void(&self, _: TypeToken) -> bool {
        false
    }
}

impl DiscoveryTypes for DeclaredReturns<'_> {
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        _: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        Some(a == b)
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
        self.store
            .intern_type_token(SemanticNodeId(9_000), None)
            .ok()
    }
    fn any(&self) -> Option<TypeToken> {
        self.store
            .intern_type_token(SemanticNodeId(9_001), None)
            .ok()
    }
    fn is_any(&self, _: TypeToken) -> bool {
        false
    }
    fn forced_return(&self, candidate: &SignatureCandidate) -> Result<TypeToken, IncompleteReason> {
        self.returns
            .borrow()
            .iter()
            .find(|(known, _)| known == candidate)
            .map(|(_, token)| *token)
            .ok_or(IncompleteReason::UnresolvedObligation)
    }
}

/// An intersection keeps overloads that differ ONLY in their result.
///
/// Measured on the pinned TypeScript 7.0.2 checker: with `fa: () => A` and
/// `fb: () => B`, `ReturnType<typeof fa & typeof fb>` is `B` and
/// `InstanceType<typeof CtorA & typeof CtorB>` is `B` — both signatures
/// survive and conditional inference reads the LAST. The checker's
/// `appendSignatures` compares with `ignoreReturnTypes: false`. Deduplicating
/// on parameters alone collapses the pair onto the first signature and
/// answers `A` instead.
///
/// The other half of the same rule: two signatures identical INCLUDING their
/// result still collapse (`ReturnType<typeof fa & typeof fa2>` with both
/// `() => A` is `A`), and the first one stays the representative.
#[test]
fn intersection_keeps_signatures_that_differ_only_in_their_result() {
    let store = SignatureStore::new();
    let types = DeclaredReturns {
        store: &store,
        returns: RefCell::new(Vec::new()),
    };
    let (param, returns_a, returns_b) = (100, 200, 201);
    let fa = types.declared(1, param, returns_a);
    let fb = types.declared(2, param, returns_b);
    let fa_again = types.declared(3, param, returns_a);

    assert_eq!(
        intersection_signatures(&store, &types, SignatureKind::Call, &[vec![fa], vec![fb]])
            .unwrap(),
        vec![fa, fb],
        "signatures that differ only in their result are distinct overloads, in authored order"
    );
    assert_eq!(
        intersection_signatures(
            &store,
            &types,
            SignatureKind::Call,
            &[vec![fa], vec![fa_again]]
        )
        .unwrap(),
        vec![fa],
        "signatures identical including their result collapse onto the first"
    );
}

/// A declaration with heritage is not an intersection type: TypeScript's
/// `resolveObjectTypeMembers` gives it its OWN signatures first, then each
/// base's in clause order, and drops nothing. Measured on TypeScript 7.0.2
/// over `interface A { (a: string): 'A' }`, `interface B1 extends A
/// { (b1: number): 'B1' }`, `interface B2 extends A { (b2: boolean): 'B2' }`
/// and `interface DM extends B1, B2 {}`: `ReturnType<DM>` is `"A"` — the
/// shared base's signature, reached again through `B2`, is the LAST — while
/// the intersection `B1 & B2` dedups it and `ReturnType<B1 & B2>` is `"B2"`.
/// Over `interface C extends A { (): number }` a call resolves to the own
/// `number` signature and `ReturnType<C>` reads the base's.
#[test]
fn heritage_lists_own_signatures_first_and_drops_nothing() {
    let store = SignatureStore::new();
    let types = DeclaredReturns {
        store: &store,
        returns: RefCell::new(Vec::new()),
    };
    let root = types.declared(1, 100, 200);
    let left = types.declared(2, 101, 201);
    let right = types.declared(3, 102, 202);
    let own = types.declared(4, 103, 203);

    // `DM`'s body: bases in clause order, the own body LAST. Each base is
    // itself a declaration listing its own signature before the root's.
    let members = [vec![left, root], vec![right, root], vec![own]];
    assert_eq!(
        heritage_signatures(&members),
        vec![own, left, root, right, root],
        "own signatures first, then every base's in clause order, the repeated root kept"
    );
    assert_eq!(
        intersection_signatures(&store, &types, SignatureKind::Call, &members).unwrap(),
        vec![left, root, right, own],
        "an intersection over the same members keeps authored arm order and drops the \
         identical repeat"
    );
    assert!(
        heritage_signatures(&[]).is_empty(),
        "a body with no member carries no signature"
    );
}
