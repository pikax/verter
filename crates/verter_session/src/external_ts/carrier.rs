//! The carrier-registry layer of the project-bound external-TS contract.
//!
//! A `CarrierRegistry` maps a source URI to the generated carrier artifact the
//! external TS engine type-checks. The authoritative content store
//! (`ProviderSurfaceStore`) lives in `verter_lsp`, which depends ON
//! `verter_session` — so the live store is not reachable from here. This module
//! therefore models the TRAIT + its DTOs and ships a real in-memory test impl;
//! the live wiring onto the existing store is a downstream concern. No SECOND
//! content store is created here.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::types::Hash16;

/// The role of a carrier surface. Mirrors the existing `ProviderSurfaceStore`
/// roles (`CarrierIde`/`CarrierApi`/`Shadow`/`Real`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierRole {
    /// `{name}.vue.tsx` / `{name}.svelte.tsx` — the interactive IDE surface
    /// (script + template TSX, self-diagnostics); the bare-import-probed surface
    /// is the declaration carrier `{name}.d.vue.ts` / `{name}.d.svelte.ts`.
    CarrierIde,
    /// `{name}.vue.verter.ts` / `{name}.svelte.verter.ts` — the redirect-reached
    /// macro-derived public-API surface a cross-file rename resolves against.
    CarrierApi,
    /// A self-file shadow / rune-module surface (the `.svelte.ts`/`.svelte.js`
    /// rune source rewritten in place).
    Shadow,
    /// A genuine non-carrier `.ts`/`.tsx` synced verbatim for context.
    Real,
}

/// A generated carrier artifact: the engine surface for a source URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierArtifact {
    /// The engine file identity (the companion path the engine type-checks).
    pub provider_uri: Arc<str>,
    /// Which surface kind this artifact is.
    pub role: CarrierRole,
    /// The carrier content (valid TS/TSX).
    pub content: Arc<str>,
    /// BLAKE3-class content hash over `content` (fail-closed query identity).
    pub content_hash: Hash16,
    /// Hash over the `CodeTransform` source-map (mapped-result invalidation).
    pub map_hash: Hash16,
    /// Monotonic version of this artifact.
    pub version: u64,
}

/// The carrier-registry seam: a source URI → its carrier artifact.
///
/// Read-only: it never mutates a content store. The live impl reads the existing
/// `ProviderSurfaceStore`; the contract ships a [`InMemoryCarrierRegistry`] test impl.
pub trait CarrierRegistry {
    /// The carrier artifact for `source_uri`, or `None` if the source has no
    /// carrier (e.g. a plain `.ts` that is its own surface, or an unknown file).
    fn carrier_for(&self, source_uri: &str) -> Option<CarrierArtifact>;
}

/// An in-memory [`CarrierRegistry`] used to test the contract without the live
/// `ProviderSurfaceStore`. NOT a second production store — a test double.
#[derive(Debug, Default, Clone)]
pub struct InMemoryCarrierRegistry {
    artifacts: FxHashMap<Arc<str>, CarrierArtifact>,
}

impl InMemoryCarrierRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) the carrier artifact for a source URI.
    pub fn insert(&mut self, source_uri: impl Into<Arc<str>>, artifact: CarrierArtifact) {
        self.artifacts.insert(source_uri.into(), artifact);
    }
}

impl CarrierRegistry for InMemoryCarrierRegistry {
    fn carrier_for(&self, source_uri: &str) -> Option<CarrierArtifact> {
        self.artifacts.get(source_uri).cloned()
    }
}
