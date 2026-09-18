use super::lifetime::SignatureStore;
use super::provenance::{
    ArmIdentity, ConstituentSequence, DeclarationGroupId, DeclarationParentId, MappedConstituent,
    OriginRelation, SignatureProvenance,
};
use super::test_support::intern_one_call;

#[test]
fn effective_overload_order_is_cached_on_the_record() {
    let p = SignatureProvenance::authored(
        DeclarationGroupId::from_raw(3),
        DeclarationParentId::from_raw(4),
        1,
        7,
    );
    assert_eq!(p.effective_overload_order.ordinal, 7);
    assert_eq!(p.effective_overload_order.group, p.declaration_group);
    assert!(matches!(p.origin, OriginRelation::Authored));
}

#[test]
fn constituent_sequence_retains_repeated_arms() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let super::records::SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let edge = MappedConstituent {
        arm: ArmIdentity {
            ordinal: 0,
            contributor: c.signature,
        },
        declaration: c.signature,
        residual: c.signature,
    };
    let seq = ConstituentSequence {
        edges: Box::from([edge, edge]),
    };
    let id = store.intern_sequence(seq, None).unwrap();
    let again = store
        .intern_sequence(
            ConstituentSequence {
                edges: Box::from([edge, edge]),
            },
            None,
        )
        .unwrap();
    assert_eq!(id, again);
}
