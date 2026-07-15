//! The [`EngineIdentity`]-keyed warm-state cache for SHARED serving decisions,
//! with reconnect-generation discipline and the eviction/invalidation trigger
//! set.
//!
//! A SHARED serving decision is cacheable ONLY under a composite key that pins
//! every dimension whose change must invalidate it:
//!
//! - the canonical component representative ([`ProjectIdentity`]),
//! - the canonical tsconfig path (or bound-project id),
//! - the project/config generation,
//! - the FULL [`EngineIdentity`] (`mode` + `observed_version` + `wire_pin` +
//!   `editor_session_generation`),
//! - the editor-binding witness/fingerprint.
//!
//! ## Reconnect ALWAYS mints a fresh identity
//!
//! A reconnect is never a reuse. A new control session bumps
//! `editor_session_generation` (and typically `wire_pin` / `observed_version`),
//! which changes the [`EngineIdentity`] dimension of the key, so the prior warm
//! entry becomes UNREACHABLE — a lookup under the new generation MISSES rather
//! than returning a stale `--api` handle, snapshot, injected-carrier state, or
//! diagnostics (the split-brain hazard). The key change is the primary
//! discipline; [`EngineWarmCache::evict_superseded_generations`] additionally
//! purges the orphaned older-generation entries.
//!
//! ## No laundering
//!
//! Only a SHARED decision is admissible ([`EngineWarmCache::insert_shared`]
//! refuses an OWNED / failover decision), mirroring the mode-keyed
//! [`EngineIdentity`] discipline: OWNED serving state is never warmed, and an
//! OWNED identity over identical facts is never equal to a SHARED one, so the two
//! can never collide in this cache.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::file_artifact_store::ProjectIdentity;

use super::mode::{ComponentModeDecision, EngineIdentity, ReferenceComponent, ServeMode};

/// A reconnect generation / epoch. Monotonically increasing; each editor
/// reconnect bumps it. A bump changes the `editor_session_generation` dimension
/// of the [`EngineIdentity`] carried in a [`WarmCacheKey`], so every prior warm
/// entry keyed on the earlier generation is unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReconnectGeneration(pub u64);

/// The composite warm-cache key. Every dimension whose change must invalidate a
/// SHARED serving decision is a key field, so a stale decision is UNREACHABLE
/// after any relevant change — a miss, never a reuse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WarmCacheKey {
    /// The canonical component representative (the byte-minimum member identity),
    /// so a lookup from ANY member of the entry-independent component keys the
    /// same slot.
    component_root: ProjectIdentity,
    /// The canonical tsconfig path (or bound-project id) of the served component.
    canonical_tsconfig: Arc<str>,
    /// The project/config generation.
    config_generation: u64,
    /// The FULL serving-engine identity (mode + observed version + wire pin +
    /// reconnect generation).
    engine: EngineIdentity,
    /// The editor-binding witness/fingerprint (the project the editor bound the
    /// carrier to).
    editor_binding: ProjectIdentity,
}

impl WarmCacheKey {
    /// The composite key for a serving `decision`. The component representative
    /// is the decision's byte-minimum member, so the key is entry-independent
    /// (any member of the component keys the same slot); the engine identity is
    /// the decision's own [`EngineIdentity`], which carries the `mode` axis (an
    /// OWNED and a SHARED decision over identical facts key different slots).
    #[must_use]
    pub fn for_decision(
        decision: &ComponentModeDecision,
        canonical_tsconfig: impl Into<Arc<str>>,
        config_generation: u64,
        editor_binding: ProjectIdentity,
    ) -> Self {
        let component_root = decision
            .members()
            .members()
            .next()
            .expect("a component decision always covers at least the root member");
        Self {
            component_root,
            canonical_tsconfig: canonical_tsconfig.into(),
            config_generation,
            engine: decision.engine().clone(),
            editor_binding,
        }
    }

    /// The serving-engine identity this key pins.
    #[must_use]
    pub fn engine(&self) -> &EngineIdentity {
        &self.engine
    }

    /// The canonical component representative this key is anchored on.
    #[must_use]
    pub fn component_root(&self) -> ProjectIdentity {
        self.component_root
    }
}

/// An immutable per-decision SHARED serving state. Handed out as an
/// [`Arc`] so warm hits share one immutable value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarmSharedState {
    decision: ComponentModeDecision,
}

impl WarmSharedState {
    /// The cached SHARED serving decision.
    #[must_use]
    pub fn decision(&self) -> &ComponentModeDecision {
        &self.decision
    }

    /// The component the cached decision serves.
    #[must_use]
    pub fn members(&self) -> &ReferenceComponent {
        self.decision.members()
    }

    /// The serving-engine identity of the cached decision.
    #[must_use]
    pub fn engine(&self) -> &EngineIdentity {
        self.decision.engine()
    }
}

/// Why a decision was refused admission to the warm cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmAdmissionError {
    /// The decision is not SHARED. OWNED / failover serving state is never
    /// warmed — the no-poison invariant.
    NotShared,
}

/// The set of eviction / invalidation triggers that must drop warm SHARED state.
///
/// Some triggers are also shadowed by the key (a version, generation, config, or
/// binding change changes the key, so a fresh decision misses the stale entry
/// anyway); the explicit eviction actively purges the orphaned entries so they
/// do not linger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionTrigger {
    /// The editor or the relay shim disconnected.
    EditorOrShimDisconnect,
    /// The real engine process exited or restarted.
    RealEngineExitOrRestart,
    /// The control protocol version/handshake mismatched.
    ControlProtocolMismatch,
    /// The engine-version capability gate changed (green ↔ not-green, or version).
    GateOrVersionChange,
    /// A tsconfig or the project graph reloaded.
    TsconfigOrGraphReload,
    /// A component's redirect-reference closure changed.
    ReferenceClosureChange,
    /// A carrier companion path appeared or disappeared.
    CompanionPathAppearedOrDisappeared,
    /// The editor bound a carrier to a different project than Verter resolved.
    EditorBindingMismatch,
    /// A source or carrier document version changed.
    SourceOrCarrierDocVersionChange,
    /// The `--api` pipe/transport failed.
    ApiPipeFailure,
}

/// The blast radius of an [`EvictionTrigger`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionScope {
    /// Every SHARED entry — the shared editor engine, its `--api` transport, or
    /// the version that underpins ALL shared serving is gone or changed.
    EntireCache,
    /// Only entries serving a component that contains the affected project root.
    AffectedComponent,
}

impl EvictionTrigger {
    /// The blast radius this trigger evicts.
    #[must_use]
    pub fn scope(self) -> EvictionScope {
        match self {
            // The shared editor engine / transport / version underpins every
            // SHARED entry — its loss or change invalidates the whole cache.
            EvictionTrigger::EditorOrShimDisconnect
            | EvictionTrigger::RealEngineExitOrRestart
            | EvictionTrigger::ControlProtocolMismatch
            | EvictionTrigger::GateOrVersionChange
            | EvictionTrigger::ApiPipeFailure => EvictionScope::EntireCache,
            // A per-project / per-component change invalidates only the entries
            // serving the affected component.
            EvictionTrigger::TsconfigOrGraphReload
            | EvictionTrigger::ReferenceClosureChange
            | EvictionTrigger::CompanionPathAppearedOrDisappeared
            | EvictionTrigger::EditorBindingMismatch
            | EvictionTrigger::SourceOrCarrierDocVersionChange => EvictionScope::AffectedComponent,
        }
    }
}

/// The [`EngineIdentity`]-keyed warm cache of SHARED serving decisions.
#[derive(Debug, Default)]
pub struct EngineWarmCache {
    entries: FxHashMap<WarmCacheKey, Arc<WarmSharedState>>,
}

impl EngineWarmCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The warm SHARED state for `key`, or `None` on a miss. Hands out an
    /// immutable [`Arc`].
    #[must_use]
    pub fn get(&self, key: &WarmCacheKey) -> Option<Arc<WarmSharedState>> {
        self.entries.get(key).cloned()
    }

    /// Whether `key` currently has a warm entry.
    #[must_use]
    pub fn contains(&self, key: &WarmCacheKey) -> bool {
        self.entries.contains_key(key)
    }

    /// The number of warm entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Admit a SHARED serving `decision` under `key`, returning the stored
    /// immutable state. An OWNED / failover decision is REFUSED
    /// ([`WarmAdmissionError::NotShared`]) — OWNED serving state is never warmed.
    pub fn insert_shared(
        &mut self,
        key: WarmCacheKey,
        decision: ComponentModeDecision,
    ) -> Result<Arc<WarmSharedState>, WarmAdmissionError> {
        if decision.mode() != ServeMode::Shared {
            return Err(WarmAdmissionError::NotShared);
        }
        let state = Arc::new(WarmSharedState { decision });
        self.entries.insert(key, Arc::clone(&state));
        Ok(state)
    }

    /// Evict warm entries for `trigger`, scoped by [`EvictionTrigger::scope`].
    /// `affected_root` identifies the project whose change fired the trigger
    /// (used by [`EvictionScope::AffectedComponent`]; ignored by
    /// [`EvictionScope::EntireCache`]). Returns the number of entries removed.
    pub fn evict(&mut self, trigger: EvictionTrigger, affected_root: ProjectIdentity) -> usize {
        let before = self.entries.len();
        match trigger.scope() {
            EvictionScope::EntireCache => self.entries.clear(),
            EvictionScope::AffectedComponent => {
                // An entry's stored decision covers its WHOLE component, so
                // membership of the affected root identifies exactly the entries
                // serving that component.
                self.entries
                    .retain(|_key, state| !state.members().contains(&affected_root));
            }
        }
        before - self.entries.len()
    }

    /// Evict every warm entry serving any member of `component` — the
    /// whole-component discard a mid-flight SHARED failover performs, so no stale
    /// SHARED serving state for the failed-over component lingers. Returns the
    /// number removed.
    pub fn evict_component(&mut self, component: &ReferenceComponent) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|_key, state| !state.members().members().any(|m| component.contains(&m)));
        before - self.entries.len()
    }

    /// Purge SHARED entries serving `component_root`'s component whose engine
    /// generation is OLDER than `current` — the orphaned entries a reconnect
    /// superseded. Returns the number removed. The primary reconnect discipline
    /// is the key change (a fresh generation is a fresh key, hence a miss); this
    /// actively drops the stale entries so they cannot linger.
    pub fn evict_superseded_generations(
        &mut self,
        component_root: ProjectIdentity,
        current: ReconnectGeneration,
    ) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_key, state| {
            let serves_component = state.members().contains(&component_root);
            let superseded = state.engine().editor_session_generation < current.0;
            !(serves_component && superseded)
        });
        before - self.entries.len()
    }
}

#[cfg(test)]
#[path = "warm_cache_tests.rs"]
mod tests;
