//! Integration-test surface for allocator canaries and lock probes.

use std::sync::Arc;
use std::thread;

use super::lifetime::SignatureStore;
use super::provenance::{DeclarationGroupId, DeclarationParentId, SignatureProvenance};
use super::read_view::SemanticReadView;
use super::records::{
    BinderSpace, ParameterLayout, ParameterSlotId, ReturnObligationKey, SignatureCandidate,
    SignatureDescriptor, SignatureInputShape, SignatureKind, SignatureResultRecipe,
    SignatureSemanticFlags, SignatureSetRef, SignatureTemplate,
};
use crate::semantic_query::{
    CanonicalTypeSubstitution, ResultEvaluationContextId, SemanticContext, SemanticContextId,
    SemanticNodeId, SemanticPolicySet,
};
use verter_semantic::resolver_core::{EnvHashes, SemanticCompilerOptions};

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

/// Intern a real semantic context. Id 0 is whichever context interned first;
/// tests must not forge it.
#[must_use]
pub(crate) fn intern_test_context(project_identity: [u8; 16]) -> SemanticContextId {
    SemanticContext {
        effective_semantic_options: SemanticCompilerOptions::default(),
        resolver_library_project_environment: EnvHashes::default(),
        policy_set: SemanticPolicySet::production().intern(),
        project_identity,
    }
    .intern()
}

/// Duplicate publishers intern one logical One-set on a shared store.
#[must_use]
pub fn duplicate_publisher_one_sets(workers: usize) -> Vec<SignatureSetRef> {
    let store = Arc::new(SignatureStore::new());
    thread::scope(|scope| {
        let mut joins = Vec::new();
        // bounded-loop: one publisher per worker.
        for _ in 0..workers {
            let store = Arc::clone(&store);
            joins.push(scope.spawn(move || intern_one_call(&store)));
        }
        joins
            .into_iter()
            .map(|j| j.join().expect("publisher"))
            .collect()
    })
}

/// Opposite intern order on two stores: residual binder tokens must match.
#[must_use]
pub fn opposite_order_one_call_binder_tokens() -> (SemanticNodeId, SemanticNodeId) {
    let forward = SignatureStore::new();
    let reverse = SignatureStore::new();
    let one_f = intern_one_call(&forward);
    let one_r = intern_one_call(&reverse);
    let SignatureSetRef::One(cf) = one_f else {
        panic!("fixture One");
    };
    let SignatureSetRef::One(cr) = one_r else {
        panic!("fixture One");
    };
    let df = SemanticReadView::pin(&forward)
        .descriptor(cf.signature)
        .expect("fwd descriptor")
        .residual_binders;
    let dr = SemanticReadView::pin(&reverse)
        .descriptor(cr.signature)
        .expect("rev descriptor")
        .residual_binders;
    (
        forward.binder_token_for(df, 0).expect("fwd token"),
        reverse.binder_token_for(dr, 0).expect("rev token"),
    )
}
