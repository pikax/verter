//! The pinned compatibility data snapshot and per-target support evidence.
//!
//! This module is the only reader of the compatibility snapshot and the only
//! place support facts are evaluated. A snapshot is minted solely by
//! [`SnapshotState::resolve`] from prepared bytes the host supplied; nothing
//! here reads a file, reaches the network or falls back to a default dataset.
//! Sibling compatibility modules consume it through the re-exports below, and
//! nothing outside `compat` can name it.
//!
//! Answers are static predictions over pinned data, never observed browser
//! behaviour. Baseline status travels beside an answer as provenance and is
//! never an input to one.

// The consumers are the sibling `compat` modules; until they call in, only this
// module's tests reach the surface.
#![allow(dead_code, unused_imports)]

mod snapshot;
mod support;
mod vocabulary;

#[cfg(test)]
mod tests;

pub(in crate::compat) use snapshot::{
    BaselineEvidence, BaselineStatus, BrowserRecord, BrowserRelease, CompatSnapshot,
    DatasetIdentity, Discouraged, FeatureFacts, NeedSnapshotInput, ReleaseStatus, SnapshotIdentity,
    SnapshotInput, SnapshotPin, SnapshotRejection, SnapshotState, SupportAnswer, WebFeatureAnswer,
    WebFeatureMembers,
};
pub(in crate::compat) use support::{
    BrowserTarget, BrowserVersion, ConditionKind, FlagCondition, IndeterminateBoundary,
    IntervalStatus, InvalidTargetSpelling, SchemaMismatch, SinceBound, SupportCondition,
    SupportStatement, SupportVerdict, UnknownReason, VersionAdded, VersionRemoved,
};
pub(in crate::compat) use vocabulary::{
    browserslist_family, BrowserslistQueryForm, QueryAdmission, UnsupportedQueryReason,
};
