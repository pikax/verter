//! Request-scoped borrowed reads.
//!
//! A view pins one graph epoch and snapshot. Warm positional Empty/One
//! reads borrow interned records: no per-candidate `Arc` clone and no
//! intern-shard lock.

use std::sync::Arc;

use super::lifetime::{check_epoch, EpochInner, SignatureStore, StoreError};
use super::provenance::ConstituentSequence;
use super::provenance::SignatureProvenance;
use super::records::GraphEpoch;
use super::records::{
    BinderSpace, BinderSpaceId, BodyLocatorId, ConstituentSequenceId, DeclarationInstantiationId,
    ParameterLayout, ParameterLayoutId, ParameterSlot, ParameterSlotId, SpellingId, TypeToken,
};
use super::records::{
    CallSubstitutionId, SignatureCandidate, SignatureDescriptor, SignatureDescriptorId,
    SignatureInputShape, SignatureInputShapeId, SignatureProvenanceId, SignatureResultRecipe,
    SignatureResultRecipeId, SignatureSet, SignatureSetId, SignatureSetRef, SignatureTemplate,
    SignatureTemplateId,
};
use super::substitution::{CallSubstitution, SubstTerm};
use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    StaleHandle,
    Missing,
}

impl From<StoreError> for ReadError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::StaleHandle => Self::StaleHandle,
            _ => Self::Missing,
        }
    }
}

/// Borrowed view of a [`SignatureSetRef`].
pub enum BorrowedSet<'a> {
    Empty,
    One {
        candidate: SignatureCandidate,
        descriptor: &'a SignatureDescriptor,
        provenance: &'a SignatureProvenance,
    },
    Many(&'a [SignatureCandidate]),
}

/// Pins one epoch for the request. Dropping unpins the reader root.
pub struct SemanticReadView {
    inner: Arc<EpochInner>,
}

impl SemanticReadView {
    #[must_use]
    pub fn pin(store: &SignatureStore) -> Self {
        Self { inner: store.pin() }
    }

    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.inner.epoch
    }

    #[must_use]
    pub fn descriptor_chain_walks(&self) -> u64 {
        self.inner
            .descriptor_chain_walks
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[must_use]
    pub fn shard_lock_acquires(&self) -> u64 {
        self.inner.shard_lock_acquires()
    }

    pub fn descriptor(&self, id: SignatureDescriptorId) -> Result<&SignatureDescriptor, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .descriptors
            .get(id.index())
            .ok_or(ReadError::Missing)
    }

    pub fn provenance(&self, id: SignatureProvenanceId) -> Result<&SignatureProvenance, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .provenances
            .get(id.index())
            .ok_or(ReadError::Missing)
    }

    pub fn set(&self, id: SignatureSetId) -> Result<&SignatureSet, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner.sets.get(id.index()).ok_or(ReadError::Missing)
    }

    pub fn shape(&self, id: SignatureInputShapeId) -> Result<&SignatureInputShape, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner.shapes.get(id.index()).ok_or(ReadError::Missing)
    }

    pub fn template(&self, id: SignatureTemplateId) -> Result<&SignatureTemplate, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .templates
            .get(id.index())
            .ok_or(ReadError::Missing)
    }

    pub fn recipe(&self, id: SignatureResultRecipeId) -> Result<&SignatureResultRecipe, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner.recipes.get(id.index()).ok_or(ReadError::Missing)
    }

    pub fn substitution(&self, id: CallSubstitutionId) -> Result<&CallSubstitution, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .substitutions
            .get(id.index())
            .ok_or(ReadError::Missing)
    }

    pub fn layout(&self, id: ParameterLayoutId) -> Result<&ParameterLayout, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner.layouts.get(id.index()).ok_or(ReadError::Missing)
    }

    pub fn slot(&self, id: ParameterSlotId) -> Result<&ParameterSlot, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner.slots.get(id.index()).ok_or(ReadError::Missing)
    }

    pub fn space(&self, id: BinderSpaceId) -> Result<&BinderSpace, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner.spaces.get(id.index()).ok_or(ReadError::Missing)
    }

    pub fn environment(
        &self,
        id: DeclarationInstantiationId,
    ) -> Result<&CanonicalTypeSubstitution, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .environments
            .get(id.index())
            .ok_or(ReadError::Missing)
    }

    pub fn sequence(&self, id: ConstituentSequenceId) -> Result<&ConstituentSequence, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .sequences
            .get(id.index())
            .ok_or(ReadError::Missing)
    }

    pub fn body_locator(&self, id: BodyLocatorId) -> Result<u64, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .locators
            .get(id.index())
            .copied()
            .ok_or(ReadError::Missing)
    }

    pub fn spelling(&self, id: SpellingId) -> Result<&str, ReadError> {
        check_epoch(self.inner.epoch, id.epoch()).map_err(ReadError::from)?;
        self.inner
            .strings
            .get(id.index())
            .map(|s| &**s)
            .ok_or(ReadError::Missing)
    }

    pub fn type_token_node(&self, token: TypeToken) -> Result<SemanticNodeId, ReadError> {
        let raw = token.as_u64();
        check_epoch(self.inner.epoch, super::records::handle_epoch(raw))
            .map_err(ReadError::from)?;
        self.inner
            .type_tokens
            .get(super::records::handle_index(raw))
            .copied()
            .ok_or(ReadError::Missing)
    }

    /// Apply against this pinned epoch, not the store's current epoch.
    pub fn apply(
        &self,
        subst: CallSubstitutionId,
        term: &SubstTerm,
    ) -> Result<SubstTerm, StoreError> {
        self.inner
            .apply_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SignatureStore::apply_inner(&self.inner, subst, term)
    }

    /// Warm positional read. Empty and One do not touch intern shards or
    /// allocate. Many borrows the interned slice.
    pub fn read_set(&self, set: SignatureSetRef) -> Result<BorrowedSet<'_>, ReadError> {
        match set {
            SignatureSetRef::Empty => Ok(BorrowedSet::Empty),
            SignatureSetRef::One(candidate) => {
                let descriptor = self.descriptor(candidate.signature)?;
                let provenance = self.provenance(candidate.provenance)?;
                Ok(BorrowedSet::One {
                    candidate,
                    descriptor,
                    provenance,
                })
            }
            SignatureSetRef::Many(id) => {
                let set = self.set(id)?;
                Ok(BorrowedSet::Many(&set.candidates))
            }
        }
    }

    /// Identity of the set itself (no table access for Empty/One).
    pub fn read_set_ref(&self, set: SignatureSetRef) -> Result<SignatureSetRef, ReadError> {
        match set {
            SignatureSetRef::Empty => Ok(SignatureSetRef::Empty),
            SignatureSetRef::One(candidate) => {
                check_epoch(self.inner.epoch, candidate.signature.epoch())
                    .map_err(ReadError::from)?;
                check_epoch(self.inner.epoch, candidate.provenance.epoch())
                    .map_err(ReadError::from)?;
                Ok(SignatureSetRef::One(candidate))
            }
            SignatureSetRef::Many(id) => {
                let _ = self.set(id)?;
                Ok(SignatureSetRef::Many(id))
            }
        }
    }
}

impl Drop for SemanticReadView {
    fn drop(&mut self) {
        SignatureStore::unpin(&self.inner);
    }
}
