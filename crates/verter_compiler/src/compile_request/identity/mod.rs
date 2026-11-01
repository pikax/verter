//! Compiler stage identity — the one vocabulary every compiler stage keys
//! through (`docs/arch/compiler-request/compile-request-contract.md`,
//! "Stage identities").
//!
//! Each stage key is a typed value with stable canonical serialization
//! and hashing; a display rendering, a `Debug` format, a timestamp or an
//! ambient "latest" tag participates in no identity. Canonical bytes come
//! from the `verter_identity` `CanonicalEncoder` under one domain tag per
//! stage with fixed field numbers: set-like fields sort and deduplicate,
//! absent is distinct from present, and the bytes are the public
//! compatibility contract. A stage key addresses a cache slot; it is not
//! a host validity oracle — warm admission stays with the host store-view
//! validation that owns it.
//!
//! Composition law (each stage composes its predecessor's identity plus
//! its own classified option bytes and contract epoch, so a change in one
//! stage's inputs re-keys that stage and every later stage of the same
//! product):
//!
//! ```text
//! ParseKey            parser-owned identity (re-exported, never re-minted)
//! SemanticKey         sorted source-unit ParseKeys
//!                   + framework profile digest
//!                   + semantic option bytes + semantic epoch
//!                   + semantic admission basis digest
//! CompileStructureKey SemanticKey + demand-closure digest
//!                   + structure option bytes + structure epoch
//! TargetPlanKey       CompileStructureKey + target digest
//!                   + target-plan option bytes + target epoch
//! EmitKey             TargetPlanKey + emit option bytes
//!                   + map-mode digest + emit epoch
//! TerminalKey         EmitKey + terminal option bytes + terminal epoch
//! ```
//!
//! Option bytes and digests are supplied by their owners: the request
//! classifier classifies options and derives the values, and the emit
//! planner keys its plans by [`EmitKey`]. This module defines the typed
//! composition and its canonical bytes, never the classifier. The
//! reserved optimized-basis types ([`OptimizationRequestBasis`],
//! [`OptimizationObservationSet`], [`ArtifactBasis`]) are declared
//! alongside and constructible by nothing.
//!
//! These are pure value types with no incremental, cache or publication
//! authority; option-change invalidation is owned and tested where the
//! classifier derives keys.

mod optimized_basis;
mod stage_keys;

#[cfg(test)]
mod stage_key_contract_tests;

pub use optimized_basis::{ArtifactBasis, OptimizationObservationSet, OptimizationRequestBasis};
pub use stage_keys::{
    CompileStructureKey, EmitKey, SemanticAdmissionBasis, SemanticKey, StageEpoch, TargetPlanKey,
    TerminalKey,
};
pub use verter_identity::identity::ParseKey;
