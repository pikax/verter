//! Integration-test surface for allocator canaries and lock probes.

use super::lifetime::SignatureStore;
use super::provenance::{DeclarationGroupId, DeclarationParentId, SignatureProvenance};
use super::read_view::SemanticReadView;
use super::records::{
    BinderSpace, ParameterLayout, ParameterSlotId, ReturnObligationKey, SignatureCandidate,
    SignatureDescriptor, SignatureInputShape, SignatureKind, SignatureResultRecipe,
    SignatureSemanticFlags, SignatureSetRef, SignatureTemplate,
};
use crate::semantic_query::{CanonicalTypeSubstitution, ResultEvaluationContextId};

/// Store plus one inline candidate used by the warm positional canary.
pub struct WarmPositionalStore {
    store: SignatureStore,
    set: SignatureSetRef,
    many: SignatureSetRef,
}

/// Shard-lock counts around a warm positional read.
pub struct WarmPositionalLockProbe {
    pub acquires_before: u64,
    pub acquires_after: u64,
}

impl WarmPositionalStore {
    /// One interned Call candidate, ready for repeated warm reads.
    #[must_use]
    pub fn fixture() -> Self {
        let store = SignatureStore::new();
        let set = intern_one_call(&store);
        let many = match set {
            SignatureSetRef::One(c) => store.set_ref_many(Box::from([c]), None).expect("many"),
            _ => panic!("fixture One"),
        };
        Self { store, set, many }
    }

    #[must_use]
    pub fn lock_probe(&self) -> WarmPositionalLockProbe {
        let view = SemanticReadView::pin(&self.store);
        let acquires_before = view.shard_lock_acquires();
        let _ = view
            .read_set(self.set)
            .expect("fixture set is live on this epoch");
        let _ = view
            .read_set(self.many)
            .expect("fixture many is live on this epoch");
        let acquires_after = view.shard_lock_acquires();
        WarmPositionalLockProbe {
            acquires_before,
            acquires_after,
        }
    }
}

/// Warm Empty/One positional read: no intern-shard lock, no allocation of the
/// set itself. Drives the borrowing `read_set` path, not the identity-only
/// `read_set_ref`.
#[must_use]
pub fn warm_positional_read(store: &WarmPositionalStore) -> SignatureSetRef {
    let view = SemanticReadView::pin(&store.store);
    match view
        .read_set(store.set)
        .expect("fixture set is live on this epoch")
    {
        super::read_view::BorrowedSet::One { candidate, .. } => SignatureSetRef::One(candidate),
        _ => panic!("fixture One"),
    }
}

/// Warm Many positional read: borrows the interned slice, no shard lock.
#[must_use]
pub fn warm_positional_read_many(store: &WarmPositionalStore) -> usize {
    let view = SemanticReadView::pin(&store.store);
    match view
        .read_set(store.many)
        .expect("fixture many is live on this epoch")
    {
        super::read_view::BorrowedSet::Many(candidates) => candidates.len(),
        _ => panic!("fixture Many"),
    }
}

pub(crate) fn intern_one_call(store: &SignatureStore) -> SignatureSetRef {
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .expect("space");
    let layout = store
        .intern_layout(
            ParameterLayout {
                parameters: Box::from([]),
                rest: None,
            },
            None,
        )
        .expect("layout");
    let shape = store
        .intern_shape(
            SignatureInputShape {
                kind: SignatureKind::Call,
                binder_declarations: space,
                this_parameter: Option::<ParameterSlotId>::None,
                parameter_layout: layout,
                declared_minimum: 0,
                signature_semantic_flags: SignatureSemanticFlags::NONE,
            },
            None,
        )
        .expect("shape");
    let locator = store.intern_body_locator(1, None).expect("locator");
    let recipe = store
        .intern_recipe(
            SignatureResultRecipe::Body {
                return_obligation_key: ReturnObligationKey {
                    body_locator: locator,
                    evaluation: ResultEvaluationContextId::from_raw(0),
                },
            },
            None,
        )
        .expect("recipe");
    let template = store
        .intern_template(
            SignatureTemplate {
                input_shape: shape,
                result_recipe: recipe,
            },
            None,
        )
        .expect("template");
    let env = store
        .intern_environment(CanonicalTypeSubstitution::empty(), None)
        .expect("env");
    let descriptor = store
        .intern_descriptor(
            SignatureDescriptor {
                template,
                declaration_environment: env,
                residual_binders: space,
            },
            None,
        )
        .expect("descriptor");
    let provenance = store
        .intern_provenance(
            SignatureProvenance::authored(
                DeclarationGroupId::from_raw(1),
                DeclarationParentId::from_raw(1),
                0,
                0,
            ),
            None,
        )
        .expect("provenance");
    let candidate = SignatureCandidate {
        signature: descriptor,
        provenance,
    };
    SignatureSetRef::One(candidate)
}
