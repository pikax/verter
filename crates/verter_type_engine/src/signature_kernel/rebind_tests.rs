//! Signature records copied from one epoch into another, and publication
//! through a binding whose epoch a replacement retired.

use crate::semantic_query::{IncompleteReason, SemanticContextId, SemanticNodeId};

use super::discovery::{
    publish_signature, DiscoveryTypes, ParamInput, ResultInput, SignatureInput,
};
use super::lifetime::{EpochBinding, SignatureStore, StoreError};
use super::matching::ForcedResult;
use super::positional::SlotTypeFacts;
use super::provenance::{
    ConstituentSequence, DeclarationGroupId, DeclarationParentId, SignatureProvenance,
    SourceLocatorId,
};
use super::read_view::{ReadError, SemanticReadView};
use super::rebind::EpochRebind;
use super::records::{
    AppliedResult, AppliedResultId, ParameterLayout, SignatureCandidate, SignatureDescriptor,
    SignatureDescriptorId, SignatureInputShape, SignatureKind, SignatureResultRecipe,
    SignatureSemanticFlags, SignatureTemplate, TypeToken,
};
use super::substitution::CallSubstitution;
use super::test_support::{settle_union, RequestAnswers};

const STRING: u64 = 100;
const NUMBER: u64 = 101;

/// Types whose identity is token equality, interning into one binding.
struct TokenTypes<'b> {
    binding: &'b EpochBinding,
}

impl SlotTypeFacts for TokenTypes<'_> {
    fn accepts_void(&self, _: TypeToken) -> bool {
        false
    }
}

impl DiscoveryTypes for TokenTypes<'_> {
    fn identical_at_rest(
        &self,
        a: TypeToken,
        b: TypeToken,
        _: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        Some(a == b)
    }
    fn comparison_decided(&self) {}
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
        self.binding
            .intern_type_token(SemanticNodeId(9_000), None)
            .ok()
    }
    fn any(&self) -> Option<TypeToken> {
        self.binding
            .intern_type_token(SemanticNodeId(9_001), None)
            .ok()
    }
    fn is_any(&self, _: TypeToken) -> bool {
        false
    }
}

impl RequestAnswers for TokenTypes<'_> {
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        _: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool> {
        Some(a == b)
    }
    fn forced_result(&self, _: &SignatureCandidate) -> Result<ForcedResult, IncompleteReason> {
        Err(IncompleteReason::UnresolvedObligation)
    }
}

/// `(p: param) => ret`, authored from node `source`, published through
/// `binding`.
fn leaf(binding: &EpochBinding, source: u64, param: u64, ret: u64) -> SignatureCandidate {
    publish_signature(
        binding,
        &SignatureInput {
            kind: SignatureKind::Call,
            source: SemanticNodeId(source),
            space_key: source,
            space_identity: u128::from(source),
            binders: vec![],
            receiver: None,
            params: vec![ParamInput {
                name: Some("p".into()),
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
    .expect("a leaf publishes")
}

/// `(p: string) => string`, `(p: string) => number`, and the one composite
/// candidate the union of the two publishes, all through `binding`.
fn union_of_two_leaves(
    binding: &EpochBinding,
) -> (SignatureCandidate, SignatureCandidate, SignatureCandidate) {
    let left = leaf(binding, 1, STRING, STRING);
    let right = leaf(binding, 2, STRING, NUMBER);
    let union = settle_union(binding, &TokenTypes { binding }, &[vec![left], vec![right]])
        .expect("the union publishes");
    assert_eq!(
        union.len(),
        1,
        "premise: the two arms share one composite candidate"
    );
    (left, right, union[0])
}

/// Every record a descriptor reaches, read in `view`.
#[derive(Debug, PartialEq)]
struct Records {
    descriptor: SignatureDescriptor,
    template: SignatureTemplate,
    shape: SignatureInputShape,
    layout: ParameterLayout,
    recipe: SignatureResultRecipe,
    sequence: Option<ConstituentSequence>,
}

fn records(view: &SemanticReadView, id: SignatureDescriptorId) -> Result<Records, ReadError> {
    let descriptor = *view.descriptor(id)?;
    let template = *view.template(descriptor.template)?;
    let shape = *view.shape(template.input_shape)?;
    let layout = view.layout(shape.parameter_layout)?.clone();
    let recipe = *view.recipe(template.result_recipe)?;
    let sequence = match recipe {
        SignatureResultRecipe::UnionCommon { constituents, .. } => {
            Some(view.sequence(constituents)?.clone())
        }
        _ => None,
    };
    Ok(Records {
        descriptor,
        template,
        shape,
        layout,
        recipe,
        sequence,
    })
}

/// A union's composite copied from a retired epoch is, handle for handle,
/// the candidate a fresh publication of the same leaves interns in the
/// current epoch: its descriptor, template, shape, layout, recipe and
/// constituent sequence, and its provenance chain. The pass copies the
/// composite and its two constituents once each, a descriptor it already
/// copied is not copied again, and a pass within one epoch copies nothing.
#[test]
fn a_rebound_descriptor_reads_as_the_record_a_fresh_publication_interns() {
    let store = SignatureStore::new();
    let first = store.bind_current();
    let (left, right, composite) = union_of_two_leaves(&first);
    store.replace_epoch().expect("the epoch is replaced");
    let current = store.bind_current();
    assert_ne!(
        first.epoch(),
        current.epoch(),
        "premise: the publishing epoch is retired"
    );

    let mut rebind = EpochRebind::new(&first.lease(), &current);
    let copied = rebind.candidate(composite).expect("the composite copies");
    assert_eq!(
        rebind.descriptors_copied(),
        3,
        "the composite and its two constituents, each once"
    );
    assert_eq!(
        rebind.descriptor(composite.signature),
        Ok(copied.signature),
        "a descriptor the pass copied names its copy"
    );
    let copied_leaves = (
        rebind.candidate(left).expect("a leaf copies"),
        rebind.candidate(right).expect("a leaf copies"),
    );
    assert_eq!(
        rebind.descriptors_copied(),
        3,
        "no descriptor the pass copied is copied again"
    );
    drop(rebind);

    let (fresh_left, fresh_right, fresh) = union_of_two_leaves(&current);
    assert_eq!(copied, fresh, "the composite's copy is the fresh one");
    assert_eq!(copied_leaves, (fresh_left, fresh_right));
    let copy = records(current.view(), copied.signature).expect("the copy reads in its epoch");
    assert_eq!(
        copy.sequence.as_ref().map(|sequence| sequence.edges.len()),
        Some(2),
        "the copy keeps one edge per arm"
    );
    assert_eq!(
        records(first.view(), copied.signature),
        Err(ReadError::StaleHandle),
        "the copy names the current epoch, not the retired one"
    );

    let mut same_epoch = EpochRebind::new(&current.lease(), &current);
    assert_eq!(same_epoch.candidate(fresh), Ok(fresh));
    assert_eq!(same_epoch.descriptors_copied(), 0);
}

/// A binding keeps publishing into its own epoch after a replacement: a
/// token and an applied result interned through it read back through its
/// view, and the store's current epoch holds neither.
#[test]
fn a_binding_publishes_into_its_own_epoch_after_a_replacement() {
    let store = SignatureStore::new();
    let binding = store.bind_current();
    let candidate = leaf(&binding, 1, STRING, NUMBER);
    store.replace_epoch().expect("the epoch is replaced");

    let token = binding
        .intern_type_token(SemanticNodeId(NUMBER), None)
        .expect("the binding interns after the replacement");
    let descriptor = *binding
        .view()
        .descriptor(candidate.signature)
        .expect("the bound epoch still reads");
    let template = *binding.view().template(descriptor.template).unwrap();
    let call = binding
        .intern_substitution(
            CallSubstitution::identity(descriptor.residual_binders),
            None,
        )
        .expect("a call map interns through the binding");
    let result = AppliedResultId::from_raw(
        binding
            .publish_result(
                AppliedResult {
                    return_type: Some(token),
                    ..AppliedResult::context_free(
                        candidate.signature,
                        call,
                        template.result_recipe,
                        SemanticContextId::production(),
                    )
                },
                None,
            )
            .expect("a result publishes through the binding"),
    );

    assert_eq!(
        binding.view().type_token_node(token),
        Ok(SemanticNodeId(NUMBER))
    );
    assert_eq!(
        binding
            .view()
            .applied_result(result)
            .map(|applied| applied.return_type),
        Ok(Some(token))
    );
    assert_eq!(
        store.type_token_node(token),
        Err(StoreError::StaleHandle),
        "the token is not the current epoch's"
    );
    assert!(
        SemanticReadView::pin(&store)
            .applied_result(result)
            .is_err(),
        "the result is not the current epoch's"
    );
    assert_eq!(
        store.interned_len(),
        0,
        "nothing was interned into the current epoch"
    );
}

/// A pass whose target epoch retires between two of its copies stays
/// coherent: both copies read through the target binding, and a pass from
/// that binding into the new current epoch yields the records a fresh
/// publication there interns.
#[test]
fn a_rebind_into_an_epoch_that_retires_mid_pass_stays_coherent() {
    let store = SignatureStore::new();
    let first = store.bind_current();
    let (_, right, composite) = union_of_two_leaves(&first);
    store.replace_epoch().expect("the epoch is replaced");
    let second = store.bind_current();

    let mut rebind = EpochRebind::new(&first.lease(), &second);
    let copied = rebind.candidate(composite).expect("the composite copies");
    store
        .replace_epoch()
        .expect("the target epoch is replaced mid-pass");
    let copied_right = rebind
        .candidate(right)
        .expect("the pass goes on into its own target");
    drop(rebind);
    for copy in [copied, copied_right] {
        assert_eq!(copy.signature.epoch(), second.epoch());
        assert!(
            records(second.view(), copy.signature).is_ok(),
            "{copy:?} reads through the target binding"
        );
    }

    let third = store.bind_current();
    let mut onward = EpochRebind::new(&second.lease(), &third);
    let moved = onward.candidate(copied).expect("the copy copies onward");
    assert_eq!(onward.descriptors_copied(), 3);
    drop(onward);
    let (_, _, fresh) = union_of_two_leaves(&third);
    assert_eq!(
        moved, fresh,
        "two passes give the record one fresh publication interns"
    );
}
