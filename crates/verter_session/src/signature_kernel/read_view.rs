//! Request-scoped borrowed reads.
//!
//! A view pins one graph epoch and snapshot. Warm positional Empty/One
//! reads borrow interned records: no per-candidate `Arc` clone and no
//! intern-shard lock.

use std::sync::Arc;

use super::lifetime::{check_epoch, EpochInner, SignatureStore, StoreError};
use super::provenance::SignatureProvenance;
use super::records::GraphEpoch;
use super::records::{
    SignatureCandidate, SignatureDescriptor, SignatureDescriptorId, SignatureProvenanceId,
    SignatureSet, SignatureSetId, SignatureSetRef,
};

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
        self.inner.shapes.shard_lock_acquires()
            + self.inner.templates.shard_lock_acquires()
            + self.inner.descriptors.shard_lock_acquires()
            + self.inner.recipes.shard_lock_acquires()
            + self.inner.provenances.shard_lock_acquires()
            + self.inner.sets.shard_lock_acquires()
            + self.inner.results.shard_lock_acquires()
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
