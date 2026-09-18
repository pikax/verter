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
    pack_handle, AppliedResult, BinderSpace, BinderSpaceId, BodyLocatorId, CallSubstitutionId,
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
    pub live_readers: AtomicU64,
    pub descriptor_chain_walks: AtomicU64,
    pub apply_count: AtomicU64,
}

impl EpochInner {
    fn new(epoch: GraphEpoch) -> Self {
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
            live_readers: AtomicU64::new(0),
            descriptor_chain_walks: AtomicU64::new(0),
            apply_count: AtomicU64::new(0),
        }
    }

    fn shard_lock_acquires(&self) -> u64 {
        self.shapes.shard_lock_acquires()
            + self.templates.shard_lock_acquires()
            + self.descriptors.shard_lock_acquires()
            + self.recipes.shard_lock_acquires()
            + self.provenances.shard_lock_acquires()
            + self.substitutions.shard_lock_acquires()
            + self.sets.shard_lock_acquires()
            + self.results.shard_lock_acquires()
    }
}

/// Epoch-safe signature store. Replacement publishes a new empty epoch;
/// retained results and live readers keep the old tables alive.
pub struct SignatureStore {
    current: ArcSwap<EpochInner>,
    retained_results: Mutex<Vec<AppliedResult>>,
    next_epoch: AtomicU64,
}

impl SignatureStore {
    #[must_use]
    pub fn new() -> Self {
        let inner = Arc::new(EpochInner::new(GraphEpoch::FIRST));
        Self {
            current: ArcSwap::from(inner),
            retained_results: Mutex::new(Vec::new()),
            next_epoch: AtomicU64::new(GraphEpoch::FIRST.as_u32() as u64 + 1),
        }
    }

    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.current.load().epoch
    }

    #[must_use]
    pub(super) fn pin(&self) -> Arc<EpochInner> {
        let inner = self.current.load_full();
        inner.live_readers.fetch_add(1, Ordering::Relaxed);
        inner
    }

    pub(super) fn unpin(inner: &EpochInner) {
        inner.live_readers.fetch_sub(1, Ordering::Relaxed);
    }

    /// Live readers of the current epoch (plus any still-pinned retired epochs
    /// held by outstanding `Arc`s).
    #[must_use]
    pub fn live_reader_count(&self) -> u64 {
        self.current.load().live_readers.load(Ordering::Relaxed)
    }

    /// Replace the current epoch. Old pinned readers keep their `Arc`.
    pub fn replace_epoch(&self) -> Result<GraphEpoch, StoreError> {
        let next_raw = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        let next = u32::try_from(next_raw).map_err(|_| StoreError::Overflow)?;
        if next == 0 {
            return Err(StoreError::Overflow);
        }
        let epoch = GraphEpoch::from_raw(next);
        self.current.store(Arc::new(EpochInner::new(epoch)));
        Ok(epoch)
    }

    pub fn retain_result(&self, result: AppliedResult) {
        self.retained_results.lock().push(result);
    }

    pub fn drain_retained(&self) {
        self.retained_results.lock().clear();
    }

    #[must_use]
    pub fn retained_len(&self) -> usize {
        self.retained_results.lock().len()
    }

    fn inner(&self) -> arc_swap::Guard<Arc<EpochInner>> {
        self.current.load()
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

    pub fn intern_slot(
        &self,
        slot: ParameterSlot,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterSlotId, StoreError> {
        let inner = self.inner();
        let raw = inner.slots.intern(slot, cancelled)?;
        Ok(ParameterSlotId::from_raw(raw))
    }

    pub fn intern_layout(
        &self,
        layout: ParameterLayout,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterLayoutId, StoreError> {
        let inner = self.inner();
        let raw = inner.layouts.intern(layout, cancelled)?;
        Ok(ParameterLayoutId::from_raw(raw))
    }

    pub fn intern_binder_space(
        &self,
        space: BinderSpace,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BinderSpaceId, StoreError> {
        let inner = self.inner();
        let raw = inner.spaces.intern(space, cancelled)?;
        Ok(BinderSpaceId::from_raw(raw))
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
        let raw = inner.shapes.intern(shape, cancelled)?;
        Ok(SignatureInputShapeId::from_raw(raw))
    }

    pub fn intern_recipe(
        &self,
        recipe: SignatureResultRecipe,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureResultRecipeId, StoreError> {
        let inner = self.inner();
        let raw = inner.recipes.intern(recipe, cancelled)?;
        Ok(SignatureResultRecipeId::from_raw(raw))
    }

    pub fn intern_template(
        &self,
        template: SignatureTemplate,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureTemplateId, StoreError> {
        let inner = self.inner();
        let raw = inner.templates.intern(template, cancelled)?;
        Ok(SignatureTemplateId::from_raw(raw))
    }

    pub fn intern_descriptor(
        &self,
        descriptor: SignatureDescriptor,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureDescriptorId, StoreError> {
        let inner = self.inner();
        let raw = inner.descriptors.intern(descriptor, cancelled)?;
        Ok(SignatureDescriptorId::from_raw(raw))
    }

    pub fn intern_provenance(
        &self,
        provenance: SignatureProvenance,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureProvenanceId, StoreError> {
        let inner = self.inner();
        let raw = inner.provenances.intern(provenance, cancelled)?;
        Ok(SignatureProvenanceId::from_raw(raw))
    }

    pub fn intern_sequence(
        &self,
        sequence: ConstituentSequence,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::ConstituentSequenceId, StoreError> {
        let inner = self.inner();
        let raw = inner.sequences.intern(sequence, cancelled)?;
        Ok(super::records::ConstituentSequenceId::from_raw(raw))
    }

    pub fn intern_set(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetId, StoreError> {
        let inner = self.inner();
        let raw = inner.sets.intern(SignatureSet { candidates }, cancelled)?;
        Ok(SignatureSetId::from_raw(raw))
    }

    pub fn intern_substitution(
        &self,
        subst: CallSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        let inner = self.inner();
        let raw = inner.substitutions.intern(subst, cancelled)?;
        Ok(CallSubstitutionId::from_raw(raw))
    }

    pub fn intern_with_shape<F>(
        &self,
        cancelled: Option<&AtomicBool>,
        build: F,
    ) -> Result<SignatureInputShapeId, StoreError>
    where
        F: FnOnce() -> SignatureInputShape,
    {
        let inner = self.inner();
        let raw = inner.shapes.intern_with(cancelled, build)?;
        Ok(SignatureInputShapeId::from_raw(raw))
    }

    /// `compose_after(first, second)` interned. Identity is elided; chains
    /// deeper than [`MAX_SUBSTITUTION_CHAIN_DEPTH`] flatten.
    pub fn compose_after(
        &self,
        first: CallSubstitutionId,
        second: CallSubstitutionId,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        let inner = self.inner();
        let a = self.subst(&inner, first)?;
        let b = self.subst(&inner, second)?;
        if a.space() != b.space() {
            return Err(StoreError::WrongBinderSpace);
        }
        if a.is_identity() {
            return Ok(second);
        }
        if b.is_identity() {
            return Ok(first);
        }
        let depth = compose_depth(a) + 1;
        if depth > MAX_SUBSTITUTION_CHAIN_DEPTH {
            let flat = self.flatten_subst(&inner, first, second)?;
            return self.intern_substitution(flat, cancelled);
        }
        self.intern_substitution(
            CallSubstitution::Compose {
                space: a.space(),
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
        Ok(CallSubstitution::map(
            a.space(),
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
            CallSubstitution::Identity { space } => Ok(CallSubstitution::identity(*space)),
            CallSubstitution::Map { space, map } => Ok(CallSubstitution::map(*space, map.clone())),
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
        self.apply_inner(&inner, subst, term)
    }

    fn apply_inner(
        &self,
        inner: &EpochInner,
        subst: CallSubstitutionId,
        term: &SubstTerm,
    ) -> Result<SubstTerm, StoreError> {
        let node = self.subst(inner, subst)?;
        match node {
            CallSubstitution::Compose { first, second, .. } => {
                let mid = self.apply_inner(inner, *first, term)?;
                self.apply_inner(inner, *second, &mid)
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
        Ok(inner.results.intern(result, cancelled)?)
    }

    pub fn lookup_result(&self, result: &AppliedResult) -> Result<Option<u64>, StoreError> {
        let inner = self.inner();
        match inner.results.intern(result.clone(), None) {
            Ok(id) => Ok(Some(id)),
            Err(e) => Err(e.into()),
        }
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

    /// Binder token unique to `space` and ordinal (same spelling, other space
    /// is a different node).
    #[must_use]
    pub fn binder_token(space: BinderSpaceId, ordinal: u32) -> SemanticNodeId {
        SemanticNodeId(pack_handle(
            space.epoch(),
            (space.index() << 8) | (ordinal & 0xff),
        ))
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
