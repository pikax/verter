//! Epoch ownership and retirement.
//!
//! Roots are live project state, intentionally retained results, and live
//! readers. Coherent epoch replacement retires obsolete tables: old pinned
//! readers finish against their epoch; new requests use the replacement.
//! A stale handle (wrong epoch, or epoch 0) is rejected.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use parking_lot::Mutex;

use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

use super::provenance::{ConstituentSequence, SignatureProvenance};
use super::records::{
    AppliedResult, BinderSpace, BinderSpaceId, BodyLocatorId, CallSubstitutionId,
    DeclarationInstantiationId, GraphEpoch, ParameterLayout, ParameterLayoutId, ParameterSlot,
    ParameterSlotId, SignatureCandidate, SignatureDescriptor, SignatureDescriptorId,
    SignatureInputShape, SignatureInputShapeId, SignatureProvenanceId, SignatureResultRecipe,
    SignatureResultRecipeId, SignatureSet, SignatureSetId, SignatureSetRef, SignatureTemplate,
    SignatureTemplateId, SpellingId,
};
use super::storage::{AppendInterner, InternError};
use super::substitution::{
    compose_canonical, CallSubstitution, SubstError, SubstTerm, MAX_SUBSTITUTION_CHAIN_DEPTH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Cancelled,
    Overflow,
    Panicked,
    StaleHandle,
    Missing,
    EscapingInferenceVar,
    WrongBinderSpace,
    InvalidBinderToken,
    /// Two logical signature identities reduced to one binder-space key.
    BinderKeyCollision,
    UnresolvedCompose,
}

impl From<InternError> for StoreError {
    fn from(err: InternError) -> Self {
        match err {
            InternError::Cancelled => Self::Cancelled,
            InternError::Overflow => Self::Overflow,
            InternError::Panicked => Self::Panicked,
        }
    }
}

impl From<SubstError> for StoreError {
    fn from(err: SubstError) -> Self {
        match err {
            SubstError::EscapingInferenceVar => Self::EscapingInferenceVar,
            SubstError::WrongBinderSpace => Self::WrongBinderSpace,
            SubstError::UnresolvedCompose => Self::UnresolvedCompose,
        }
    }
}

pub(super) struct EpochInner {
    pub epoch: GraphEpoch,
    pub shapes: AppendInterner<SignatureInputShape>,
    pub templates: AppendInterner<SignatureTemplate>,
    pub descriptors: AppendInterner<SignatureDescriptor>,
    pub recipes: AppendInterner<SignatureResultRecipe>,
    pub provenances: AppendInterner<SignatureProvenance>,
    pub substitutions: AppendInterner<CallSubstitution>,
    pub layouts: AppendInterner<ParameterLayout>,
    pub slots: AppendInterner<ParameterSlot>,
    pub spaces: AppendInterner<BinderSpace>,
    pub sets: AppendInterner<SignatureSet>,
    pub results: AppendInterner<AppliedResult>,
    pub strings: AppendInterner<Arc<str>>,
    pub sequences: AppendInterner<ConstituentSequence>,
    pub environments: AppendInterner<CanonicalTypeSubstitution>,
    pub locators: AppendInterner<u64>,
    pub type_tokens: AppendInterner<SemanticNodeId>,
    /// Authored source node of a descriptor (graph `Signature` node token).
    /// Epoch-local, like every handle it is keyed by.
    /// Logical identity that owns each binder-space key, so a truncated key
    /// collision fails closed instead of merging unrelated binder regions.
    pub space_key_owners: Mutex<rustc_hash::FxHashMap<u64, u128>>,
    pub descriptor_sources: Mutex<rustc_hash::FxHashMap<u32, super::records::TypeToken>>,
    pub live_readers: AtomicU64,
    /// Shared across replacement epochs so `live_reader_count` includes
    /// still-pinned retired views.
    pub store_live_readers: Arc<AtomicU64>,
    pub descriptor_chain_walks: AtomicU64,
    pub apply_count: AtomicU64,
}

impl EpochInner {
    fn new(epoch: GraphEpoch, store_live_readers: Arc<AtomicU64>) -> Self {
        Self {
            epoch,
            shapes: AppendInterner::new(epoch),
            templates: AppendInterner::new(epoch),
            descriptors: AppendInterner::new(epoch),
            recipes: AppendInterner::new(epoch),
            provenances: AppendInterner::new(epoch),
            substitutions: AppendInterner::new(epoch),
            layouts: AppendInterner::new(epoch),
            slots: AppendInterner::new(epoch),
            spaces: AppendInterner::new(epoch),
            sets: AppendInterner::new(epoch),
            results: AppendInterner::new(epoch),
            strings: AppendInterner::new(epoch),
            sequences: AppendInterner::new(epoch),
            environments: AppendInterner::new(epoch),
            locators: AppendInterner::new(epoch),
            type_tokens: AppendInterner::new(epoch),
            space_key_owners: Mutex::new(rustc_hash::FxHashMap::default()),
            descriptor_sources: Mutex::new(rustc_hash::FxHashMap::default()),
            live_readers: AtomicU64::new(0),
            store_live_readers,
            descriptor_chain_walks: AtomicU64::new(0),
            apply_count: AtomicU64::new(0),
        }
    }

    pub(super) fn shard_lock_acquires(&self) -> u64 {
        self.shapes.shard_lock_acquires()
            + self.templates.shard_lock_acquires()
            + self.descriptors.shard_lock_acquires()
            + self.recipes.shard_lock_acquires()
            + self.provenances.shard_lock_acquires()
            + self.substitutions.shard_lock_acquires()
            + self.layouts.shard_lock_acquires()
            + self.slots.shard_lock_acquires()
            + self.spaces.shard_lock_acquires()
            + self.sets.shard_lock_acquires()
            + self.results.shard_lock_acquires()
            + self.strings.shard_lock_acquires()
            + self.sequences.shard_lock_acquires()
            + self.environments.shard_lock_acquires()
            + self.locators.shard_lock_acquires()
            + self.type_tokens.shard_lock_acquires()
    }
}

/// Intentionally retained result: owns the epoch tables it was published in.
struct RetainedRoot {
    epoch: Arc<EpochInner>,
    result: AppliedResult,
}

/// High bit marks binder tokens so they never collide with interned graph nodes.
const BINDER_TOKEN_NAMESPACE: u64 = 1 << 63;
const MAX_BINDER_SPACE_KEY: u64 = (1 << 31) - 1;

/// Epoch-safe signature store. Replacement publishes a new empty epoch;
/// retained results and live readers keep the old tables alive. The live
/// `ArcSwap` slot is the live-project-state root.
pub struct SignatureStore {
    current: ArcSwap<EpochInner>,
    retained_results: Mutex<Vec<RetainedRoot>>,
    next_epoch: AtomicU64,
    epoch_publish: Mutex<()>,
    bodies_forced: AtomicU64,
}

impl SignatureStore {
    #[must_use]
    pub fn new() -> Self {
        let inner = Arc::new(EpochInner::new(
            GraphEpoch::FIRST,
            Arc::new(AtomicU64::new(0)),
        ));
        Self {
            current: ArcSwap::from(inner),
            retained_results: Mutex::new(Vec::new()),
            next_epoch: AtomicU64::new(GraphEpoch::FIRST.as_u32() as u64 + 1),
            epoch_publish: Mutex::new(()),
            bodies_forced: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.current.load().epoch
    }

    /// Record that a body recipe was forced. Enumeration never calls this.
    pub fn note_body_forced(&self) {
        self.bodies_forced.fetch_add(1, Ordering::Relaxed);
    }

    /// How many body recipes have been forced through this store.
    #[must_use]
    pub fn bodies_forced(&self) -> u64 {
        self.bodies_forced.load(Ordering::Relaxed)
    }

    #[must_use]
    pub(super) fn pin(&self) -> Arc<EpochInner> {
        let inner = self.current.load_full();
        inner.live_readers.fetch_add(1, Ordering::Relaxed);
        inner.store_live_readers.fetch_add(1, Ordering::Relaxed);
        inner
    }

    pub(super) fn unpin(inner: &EpochInner) {
        inner.live_readers.fetch_sub(1, Ordering::Relaxed);
        inner.store_live_readers.fetch_sub(1, Ordering::Relaxed);
    }

    /// Live readers of the current epoch plus still-pinned retired epochs.
    #[must_use]
    pub fn live_reader_count(&self) -> u64 {
        self.current
            .load()
            .store_live_readers
            .load(Ordering::Relaxed)
    }

    /// Replace the current epoch. Old pinned readers keep their `Arc`.
    /// Publication is serialized so concurrent callers cannot leave a
    /// lower-numbered epoch current.
    pub fn replace_epoch(&self) -> Result<GraphEpoch, StoreError> {
        let _gate = self.epoch_publish.lock();
        let next_raw = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        let next = u32::try_from(next_raw).map_err(|_| StoreError::Overflow)?;
        if next == 0 {
            return Err(StoreError::Overflow);
        }
        let epoch = GraphEpoch::from_raw(next);
        let store_live_readers = Arc::clone(&self.current.load().store_live_readers);
        self.current
            .store(Arc::new(EpochInner::new(epoch, store_live_readers)));
        Ok(epoch)
    }

    /// Pin `result` as a lifetime root of the epoch it was published in.
    pub fn retain_result(&self, result: AppliedResult) -> Result<(), StoreError> {
        let inner = self.current.load_full();
        self.check_applied(&inner, &result)?;
        self.retained_results.lock().push(RetainedRoot {
            epoch: inner,
            result,
        });
        Ok(())
    }

    pub fn drain_retained(&self) {
        self.retained_results.lock().clear();
    }

    #[must_use]
    pub fn retained_len(&self) -> usize {
        self.retained_results.lock().len()
    }

    /// Read a retained result's descriptor against its pinned epoch.
    pub fn retained_descriptor(&self, index: usize) -> Result<SignatureDescriptor, StoreError> {
        let guard = self.retained_results.lock();
        let entry = guard.get(index).ok_or(StoreError::Missing)?;
        check_epoch(entry.epoch.epoch, entry.result.descriptor.epoch())?;
        entry
            .epoch
            .descriptors
            .get(entry.result.descriptor.index())
            .copied()
            .ok_or(StoreError::Missing)
    }

    pub fn retained_result(&self, index: usize) -> Result<AppliedResult, StoreError> {
        let guard = self.retained_results.lock();
        let entry = guard.get(index).ok_or(StoreError::Missing)?;
        Ok(entry.result.clone())
    }

    fn inner(&self) -> arc_swap::Guard<Arc<EpochInner>> {
        self.current.load()
    }

    fn require_id<T>(
        inner: &EpochInner,
        epoch: GraphEpoch,
        index: u32,
        table: &AppendInterner<T>,
    ) -> Result<(), StoreError> {
        check_epoch(inner.epoch, epoch)?;
        if table.get(index).is_none() {
            Err(StoreError::Missing)
        } else {
            Ok(())
        }
    }

    fn check_applied(&self, inner: &EpochInner, result: &AppliedResult) -> Result<(), StoreError> {
        Self::require_id(
            inner,
            result.descriptor.epoch(),
            result.descriptor.index(),
            &inner.descriptors,
        )?;
        Self::require_id(
            inner,
            result.substitution.epoch(),
            result.substitution.index(),
            &inner.substitutions,
        )?;
        Self::require_id(
            inner,
            result.recipe.epoch(),
            result.recipe.index(),
            &inner.recipes,
        )?;
        for token in result
            .return_type
            .into_iter()
            .chain(result.effects.and_then(|effect| effect.ty))
        {
            let raw = token.as_u64();
            Self::require_id(
                inner,
                super::records::handle_epoch(raw),
                super::records::handle_index(raw),
                &inner.type_tokens,
            )?;
        }
        Ok(())
    }

    fn check_slot(inner: &EpochInner, slot: &ParameterSlot) -> Result<(), StoreError> {
        let raw = slot.ty.as_u64();
        Self::require_id(
            inner,
            super::records::handle_epoch(raw),
            super::records::handle_index(raw),
            &inner.type_tokens,
        )?;
        match slot.name {
            Some(name) => Self::require_id(inner, name.epoch(), name.index(), &inner.strings),
            None => Ok(()),
        }
    }

    fn check_shape(
        &self,
        inner: &EpochInner,
        shape: &SignatureInputShape,
    ) -> Result<(), StoreError> {
        Self::require_id(
            inner,
            shape.binder_declarations.epoch(),
            shape.binder_declarations.index(),
            &inner.spaces,
        )?;
        Self::require_id(
            inner,
            shape.parameter_layout.epoch(),
            shape.parameter_layout.index(),
            &inner.layouts,
        )?;
        if let Some(slot) = shape.this_parameter {
            Self::require_id(inner, slot.epoch(), slot.index(), &inner.slots)?;
        }
        Ok(())
    }

    fn check_template(
        &self,
        inner: &EpochInner,
        template: &SignatureTemplate,
    ) -> Result<(), StoreError> {
        Self::require_id(
            inner,
            template.input_shape.epoch(),
            template.input_shape.index(),
            &inner.shapes,
        )?;
        Self::require_id(
            inner,
            template.result_recipe.epoch(),
            template.result_recipe.index(),
            &inner.recipes,
        )
    }

    fn check_descriptor(
        &self,
        inner: &EpochInner,
        descriptor: &SignatureDescriptor,
    ) -> Result<(), StoreError> {
        Self::require_id(
            inner,
            descriptor.template.epoch(),
            descriptor.template.index(),
            &inner.templates,
        )?;
        Self::require_id(
            inner,
            descriptor.declaration_environment.epoch(),
            descriptor.declaration_environment.index(),
            &inner.environments,
        )?;
        Self::require_id(
            inner,
            descriptor.residual_binders.epoch(),
            descriptor.residual_binders.index(),
            &inner.spaces,
        )
    }

    fn check_candidate(
        &self,
        inner: &EpochInner,
        candidate: &SignatureCandidate,
    ) -> Result<(), StoreError> {
        Self::require_id(
            inner,
            candidate.signature.epoch(),
            candidate.signature.index(),
            &inner.descriptors,
        )?;
        Self::require_id(
            inner,
            candidate.provenance.epoch(),
            candidate.provenance.index(),
            &inner.provenances,
        )
    }

    fn check_subst_node(
        &self,
        inner: &EpochInner,
        subst: &CallSubstitution,
    ) -> Result<(), StoreError> {
        match subst {
            CallSubstitution::Compose {
                domain,
                codomain,
                first,
                second,
                ..
            } => {
                Self::require_id(inner, first.epoch(), first.index(), &inner.substitutions)?;
                Self::require_id(inner, second.epoch(), second.index(), &inner.substitutions)?;
                Self::require_id(inner, domain.epoch(), domain.index(), &inner.spaces)?;
                Self::require_id(inner, codomain.epoch(), codomain.index(), &inner.spaces)
            }
            CallSubstitution::Identity { domain, codomain }
            | CallSubstitution::Map {
                domain, codomain, ..
            } => {
                Self::require_id(inner, domain.epoch(), domain.index(), &inner.spaces)?;
                Self::require_id(inner, codomain.epoch(), codomain.index(), &inner.spaces)
            }
        }
    }

    pub fn intern_spelling(
        &self,
        s: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SpellingId, StoreError> {
        let inner = self.inner();
        let raw = inner.strings.intern(Arc::<str>::from(s), cancelled)?;
        Ok(SpellingId::from_raw(raw))
    }

    pub fn intern_body_locator(
        &self,
        locator: u64,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BodyLocatorId, StoreError> {
        let inner = self.inner();
        let raw = inner.locators.intern(locator, cancelled)?;
        Ok(BodyLocatorId::from_raw(raw))
    }

    /// Mint the kernel token standing for graph node `node`. Tokens live in
    /// the kernel's own numbering space: the raw value is an epoch-qualified
    /// intern handle, never a node ordinal.
    pub fn intern_type_token(
        &self,
        node: SemanticNodeId,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::TypeToken, StoreError> {
        let inner = self.inner();
        let raw = inner.type_tokens.intern(node, cancelled)?;
        Ok(super::records::TypeToken::from_raw(raw))
    }

    /// The graph node a token stands for. A token from another epoch is
    /// stale.
    pub fn type_token_node(
        &self,
        token: super::records::TypeToken,
    ) -> Result<SemanticNodeId, StoreError> {
        let inner = self.inner();
        let raw = token.as_u64();
        check_epoch(inner.epoch, super::records::handle_epoch(raw))?;
        inner
            .type_tokens
            .get(super::records::handle_index(raw))
            .copied()
            .ok_or(StoreError::Missing)
    }

    /// Record the graph node a descriptor was authored from. Idempotent:
    /// the same descriptor always maps to the same node token.
    pub fn record_descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
        source: super::records::TypeToken,
    ) -> Result<(), StoreError> {
        let inner = self.inner();
        Self::require_id(
            &inner,
            descriptor.epoch(),
            descriptor.index(),
            &inner.descriptors,
        )?;
        let raw = source.as_u64();
        Self::require_id(
            &inner,
            super::records::handle_epoch(raw),
            super::records::handle_index(raw),
            &inner.type_tokens,
        )?;
        inner
            .descriptor_sources
            .lock()
            .entry(descriptor.index())
            .or_insert(source);
        Ok(())
    }

    pub fn descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
    ) -> Result<Option<super::records::TypeToken>, StoreError> {
        let inner = self.inner();
        check_epoch(inner.epoch, descriptor.epoch())?;
        let found = inner
            .descriptor_sources
            .lock()
            .get(&descriptor.index())
            .copied();
        Ok(found)
    }

    pub fn intern_slot(
        &self,
        slot: ParameterSlot,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterSlotId, StoreError> {
        let inner = self.inner();
        Self::check_slot(&inner, &slot)?;
        let raw = inner.slots.intern(slot, cancelled)?;
        Ok(ParameterSlotId::from_raw(raw))
    }

    pub fn intern_layout(
        &self,
        layout: ParameterLayout,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterLayoutId, StoreError> {
        let inner = self.inner();
        for slot in layout.parameters.iter().chain(
            layout
                .rest
                .iter()
                .flat_map(|r| std::iter::once(&r.slot).chain(r.tail.iter())),
        ) {
            Self::check_slot(&inner, slot)?;
        }
        let raw = inner.layouts.intern(layout, cancelled)?;
        Ok(ParameterLayoutId::from_raw(raw))
    }

    pub fn intern_binder_space(
        &self,
        space: BinderSpace,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BinderSpaceId, StoreError> {
        let inner = self.inner();
        for binder in space.binders.iter() {
            Self::require_id(
                &inner,
                binder.spelling.epoch(),
                binder.spelling.index(),
                &inner.strings,
            )?;
        }
        let raw = inner.spaces.intern(space, cancelled)?;
        Ok(BinderSpaceId::from_raw(raw))
    }

    /// Claim `key` for the logical signature identity `identity`. A second,
    /// different identity under the same key is a collision.
    pub fn claim_space_key(&self, key: u64, identity: u128) -> Result<(), StoreError> {
        let inner = self.inner();
        let mut owners = inner.space_key_owners.lock();
        match *owners.entry(key).or_insert(identity) {
            owner if owner == identity => Ok(()),
            _ => Err(StoreError::BinderKeyCollision),
        }
    }

    pub fn intern_environment(
        &self,
        env: CanonicalTypeSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<DeclarationInstantiationId, StoreError> {
        let inner = self.inner();
        let raw = inner.environments.intern(env, cancelled)?;
        Ok(DeclarationInstantiationId::from_raw(raw))
    }

    pub fn intern_shape(
        &self,
        shape: SignatureInputShape,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureInputShapeId, StoreError> {
        let inner = self.inner();
        self.check_shape(&inner, &shape)?;
        let raw = inner.shapes.intern(shape, cancelled)?;
        Ok(SignatureInputShapeId::from_raw(raw))
    }

    pub fn intern_recipe(
        &self,
        recipe: SignatureResultRecipe,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureResultRecipeId, StoreError> {
        let inner = self.inner();
        match recipe {
            SignatureResultRecipe::Body {
                return_obligation_key,
            } => {
                Self::require_id(
                    &inner,
                    return_obligation_key.body_locator.epoch(),
                    return_obligation_key.body_locator.index(),
                    &inner.locators,
                )?;
            }
            SignatureResultRecipe::UnionCommon { constituents, .. }
            | SignatureResultRecipe::UnionSynthesized { constituents, .. }
            | SignatureResultRecipe::IntersectionConstruct {
                mixins: constituents,
                ..
            } => {
                Self::require_id(
                    &inner,
                    constituents.epoch(),
                    constituents.index(),
                    &inner.sequences,
                )?;
            }
            SignatureResultRecipe::Declared { .. } => {}
        }
        let raw = inner.recipes.intern(recipe, cancelled)?;
        Ok(SignatureResultRecipeId::from_raw(raw))
    }

    pub fn intern_template(
        &self,
        template: SignatureTemplate,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureTemplateId, StoreError> {
        let inner = self.inner();
        self.check_template(&inner, &template)?;
        let raw = inner.templates.intern(template, cancelled)?;
        Ok(SignatureTemplateId::from_raw(raw))
    }

    pub fn intern_descriptor(
        &self,
        descriptor: SignatureDescriptor,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureDescriptorId, StoreError> {
        let inner = self.inner();
        self.check_descriptor(&inner, &descriptor)?;
        let raw = inner.descriptors.intern(descriptor, cancelled)?;
        Ok(SignatureDescriptorId::from_raw(raw))
    }

    pub fn intern_provenance(
        &self,
        provenance: SignatureProvenance,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureProvenanceId, StoreError> {
        let inner = self.inner();
        match provenance.origin {
            super::provenance::OriginRelation::Synthesized { from }
            | super::provenance::OriginRelation::Instantiated { from } => {
                Self::require_id(&inner, from.epoch(), from.index(), &inner.provenances)?;
            }
            super::provenance::OriginRelation::Authored => {}
        }
        let raw = inner.provenances.intern(provenance, cancelled)?;
        Ok(SignatureProvenanceId::from_raw(raw))
    }

    pub fn intern_sequence(
        &self,
        sequence: ConstituentSequence,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::ConstituentSequenceId, StoreError> {
        let inner = self.inner();
        for edge in sequence.edges.iter() {
            Self::require_id(
                &inner,
                edge.declaration.epoch(),
                edge.declaration.index(),
                &inner.descriptors,
            )?;
            Self::require_id(
                &inner,
                edge.residual.epoch(),
                edge.residual.index(),
                &inner.descriptors,
            )?;
            Self::require_id(
                &inner,
                edge.arm.contributor.epoch(),
                edge.arm.contributor.index(),
                &inner.descriptors,
            )?;
        }
        let raw = inner.sequences.intern(sequence, cancelled)?;
        Ok(super::records::ConstituentSequenceId::from_raw(raw))
    }

    pub fn intern_set(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetId, StoreError> {
        let inner = self.inner();
        for candidate in candidates.iter() {
            self.check_candidate(&inner, candidate)?;
        }
        let raw = inner.sets.intern(SignatureSet { candidates }, cancelled)?;
        Ok(SignatureSetId::from_raw(raw))
    }

    pub fn intern_substitution(
        &self,
        subst: CallSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        let inner = self.inner();
        let subst = self.canonicalize_subst(&inner, subst)?;
        self.check_subst_node(&inner, &subst)?;
        let raw = inner.substitutions.intern(subst, cancelled)?;
        Ok(CallSubstitutionId::from_raw(raw))
    }

    /// Recompute Compose depth/domain/codomain from operands so a
    /// hand-built node cannot bypass chain-depth control or embed a
    /// stale space. Identity children are elided; chains past the bound
    /// flatten. Same-space identity elision matches `compose_after`.
    fn canonicalize_subst(
        &self,
        inner: &EpochInner,
        subst: CallSubstitution,
    ) -> Result<CallSubstitution, StoreError> {
        match subst {
            CallSubstitution::Compose { first, second, .. } => {
                Self::require_id(inner, first.epoch(), first.index(), &inner.substitutions)?;
                Self::require_id(inner, second.epoch(), second.index(), &inner.substitutions)?;
                let a = self.subst(inner, first)?;
                let b = self.subst(inner, second)?;
                if a.codomain() != b.domain() {
                    return Err(StoreError::WrongBinderSpace);
                }
                if a.is_same_space_identity() {
                    return Ok(b.clone());
                }
                if b.is_same_space_identity() {
                    return Ok(a.clone());
                }
                let depth = compose_depth(a).max(compose_depth(b)).saturating_add(1);
                if depth >= MAX_SUBSTITUTION_CHAIN_DEPTH {
                    return self.flatten_subst(inner, first, second);
                }
                Ok(CallSubstitution::Compose {
                    domain: a.domain(),
                    codomain: b.codomain(),
                    first,
                    second,
                    depth,
                })
            }
            other => Ok(other),
        }
    }

    pub fn intern_with_shape<F>(
        &self,
        cancelled: Option<&AtomicBool>,
        build: F,
    ) -> Result<SignatureInputShapeId, StoreError>
    where
        F: FnOnce() -> SignatureInputShape,
    {
        if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(StoreError::Cancelled);
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
            Ok(shape) => self.intern_shape(shape, cancelled),
            Err(_) => Err(StoreError::Panicked),
        }
    }

    /// `compose_after(first, second)` interned. Identity is elided; chains
    /// deeper than [`MAX_SUBSTITUTION_CHAIN_DEPTH`] flatten. Requires
    /// `first.codomain == second.domain`.
    pub fn compose_after(
        &self,
        first: CallSubstitutionId,
        second: CallSubstitutionId,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        let inner = self.inner();
        let a = self.subst(&inner, first)?;
        let b = self.subst(&inner, second)?;
        if a.codomain() != b.domain() {
            return Err(StoreError::WrongBinderSpace);
        }
        if a.is_same_space_identity() {
            return Ok(second);
        }
        if b.is_same_space_identity() {
            return Ok(first);
        }
        let depth = compose_depth(a).max(compose_depth(b)).saturating_add(1);
        if depth >= MAX_SUBSTITUTION_CHAIN_DEPTH {
            let flat = self.flatten_subst(&inner, first, second)?;
            return self.intern_substitution(flat, cancelled);
        }
        self.intern_substitution(
            CallSubstitution::Compose {
                domain: a.domain(),
                codomain: b.codomain(),
                first,
                second,
                depth,
            },
            cancelled,
        )
    }

    fn subst<'a>(
        &'a self,
        inner: &'a EpochInner,
        id: CallSubstitutionId,
    ) -> Result<&'a CallSubstitution, StoreError> {
        check_epoch(inner.epoch, id.epoch())?;
        inner
            .substitutions
            .get(id.index())
            .ok_or(StoreError::Missing)
    }

    fn flatten_subst(
        &self,
        inner: &EpochInner,
        first: CallSubstitutionId,
        second: CallSubstitutionId,
    ) -> Result<CallSubstitution, StoreError> {
        let a = self.flatten_to_map(inner, first)?;
        let b = self.flatten_to_map(inner, second)?;
        inner.descriptor_chain_walks.fetch_add(1, Ordering::Relaxed);
        Ok(CallSubstitution::map_across(
            a.domain(),
            b.codomain(),
            compose_canonical(a_map(&a), a_map(&b)),
        ))
    }

    fn flatten_to_map(
        &self,
        inner: &EpochInner,
        id: CallSubstitutionId,
    ) -> Result<CallSubstitution, StoreError> {
        let node = self.subst(inner, id)?;
        match node {
            CallSubstitution::Identity { domain, codomain } => {
                Ok(CallSubstitution::identity_across(*domain, *codomain))
            }
            CallSubstitution::Map {
                domain,
                codomain,
                map,
            } => Ok(CallSubstitution::map_across(
                *domain,
                *codomain,
                map.clone(),
            )),
            CallSubstitution::Compose { first, second, .. } => {
                self.flatten_subst(inner, *first, *second)
            }
        }
    }

    /// Apply `subst` to `term`. Compose nodes apply first then second.
    pub fn apply(
        &self,
        subst: CallSubstitutionId,
        term: &SubstTerm,
    ) -> Result<SubstTerm, StoreError> {
        let inner = self.inner();
        inner.apply_count.fetch_add(1, Ordering::Relaxed);
        Self::apply_inner(&inner, subst, term)
    }

    pub(super) fn apply_inner(
        inner: &EpochInner,
        subst: CallSubstitutionId,
        term: &SubstTerm,
    ) -> Result<SubstTerm, StoreError> {
        check_epoch(inner.epoch, subst.epoch())?;
        let node = inner
            .substitutions
            .get(subst.index())
            .ok_or(StoreError::Missing)?;
        match node {
            CallSubstitution::Compose { first, second, .. } => {
                inner.descriptor_chain_walks.fetch_add(1, Ordering::Relaxed);
                let mid = Self::apply_inner(inner, *first, term)?;
                Self::apply_inner(inner, *second, &mid)
            }
            other => Ok(other.apply_term(term)?),
        }
    }

    /// Publish a result already in call space. Warm reads do not re-apply.
    pub fn publish_result(
        &self,
        result: AppliedResult,
        cancelled: Option<&AtomicBool>,
    ) -> Result<u64, StoreError> {
        let inner = self.inner();
        self.check_applied(&inner, &result)?;
        Ok(inner.results.intern(result, cancelled)?)
    }

    /// Look up a previously published result. Does not intern on miss.
    pub fn lookup_result(&self, result: &AppliedResult) -> Result<Option<u64>, StoreError> {
        let inner = self.inner();
        self.check_applied(&inner, result)?;
        Ok(inner.results.lookup(result))
    }

    /// The whole substitution as one canonical map (composes flatten).
    pub fn flatten_substitution(
        &self,
        id: CallSubstitutionId,
    ) -> Result<CanonicalTypeSubstitution, StoreError> {
        let inner = self.inner();
        let flat = self.flatten_to_map(&inner, id)?;
        Ok(a_map(&flat).clone())
    }

    /// The published result record behind a handle.
    pub fn applied_result(
        &self,
        id: super::records::AppliedResultId,
    ) -> Result<AppliedResult, StoreError> {
        let inner = self.inner();
        check_epoch(inner.epoch, id.epoch())?;
        inner
            .results
            .get(id.index())
            .cloned()
            .ok_or(StoreError::Missing)
    }

    pub fn substitution(&self, id: CallSubstitutionId) -> Result<CallSubstitution, StoreError> {
        let inner = self.inner();
        Ok(self.subst(&inner, id)?.clone())
    }

    #[must_use]
    pub fn apply_count(&self) -> u64 {
        self.inner().apply_count.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn descriptor_chain_walks(&self) -> u64 {
        self.inner().descriptor_chain_walks.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn shard_lock_acquires(&self) -> u64 {
        self.inner().shard_lock_acquires()
    }

    pub fn candidate(
        &self,
        signature: SignatureDescriptorId,
        provenance: SignatureProvenanceId,
    ) -> Result<SignatureCandidate, StoreError> {
        let inner = self.inner();
        check_epoch(inner.epoch, signature.epoch())?;
        check_epoch(inner.epoch, provenance.epoch())?;
        if inner.descriptors.get(signature.index()).is_none() {
            return Err(StoreError::Missing);
        }
        if inner.provenances.get(provenance.index()).is_none() {
            return Err(StoreError::Missing);
        }
        Ok(SignatureCandidate {
            signature,
            provenance,
        })
    }

    #[must_use]
    pub fn set_ref_one(&self, candidate: SignatureCandidate) -> SignatureSetRef {
        SignatureSetRef::One(candidate)
    }

    pub fn set_ref_many(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetRef, StoreError> {
        Ok(SignatureSetRef::Many(
            self.intern_set(candidates, cancelled)?,
        ))
    }

    /// Whether `node` lives in the kernel's binder-token namespace.
    #[must_use]
    pub fn is_binder_token(node: SemanticNodeId) -> bool {
        node.0 & BINDER_TOKEN_NAMESPACE != 0
    }

    /// Decode an ordinal only through the binder-token namespace boundary.
    #[must_use]
    pub fn binder_token_ordinal(node: SemanticNodeId) -> Option<u32> {
        Self::is_binder_token(node).then_some(node.0 as u32)
    }

    /// Logical binder token from a space key and ordinal. Independent of
    /// intern order. High bit is a dedicated namespace so tokens never
    /// collide with interned graph nodes. Rejects keys that do not fit.
    pub fn binder_token(space_key: u64, ordinal: u32) -> Result<SemanticNodeId, StoreError> {
        if space_key > MAX_BINDER_SPACE_KEY {
            return Err(StoreError::InvalidBinderToken);
        }
        Ok(SemanticNodeId(
            BINDER_TOKEN_NAMESPACE | (space_key << 32) | u64::from(ordinal),
        ))
    }

    /// Binder token for an interned space: identity is the space's logical
    /// key plus ordinal, not the intern slot.
    pub fn binder_token_for(
        &self,
        space: BinderSpaceId,
        ordinal: u32,
    ) -> Result<SemanticNodeId, StoreError> {
        let inner = self.inner();
        Self::require_id(&inner, space.epoch(), space.index(), &inner.spaces)?;
        let rec = inner.spaces.get(space.index()).ok_or(StoreError::Missing)?;
        Self::binder_token(rec.key, ordinal)
    }
}

impl Default for SignatureStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn check_epoch(live: GraphEpoch, handle: GraphEpoch) -> Result<(), StoreError> {
    if !handle.is_issued() || handle != live {
        Err(StoreError::StaleHandle)
    } else {
        Ok(())
    }
}

fn compose_depth(node: &CallSubstitution) -> u8 {
    match node {
        CallSubstitution::Compose { depth, .. } => *depth,
        _ => 0,
    }
}

fn a_map(node: &CallSubstitution) -> &CanonicalTypeSubstitution {
    static EMPTY: std::sync::OnceLock<CanonicalTypeSubstitution> = std::sync::OnceLock::new();
    match node {
        CallSubstitution::Map { map, .. } => map,
        _ => EMPTY.get_or_init(CanonicalTypeSubstitution::empty),
    }
}
