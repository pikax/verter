//! The headless live-decision entry: the single pure function that ties the
//! external-TS session decision together.
//!
//! Given the resolved per-project inputs (facts + references), the candidate
//! engine sessions, and the warm cache, it: composes each project's
//! [`ProjectEligibility`], resolves references to a canonical-identity redirect
//! graph, selects the ONE serve mode for the queried component
//! ([`select_component_mode`]), and — only for a SHARED result — reuses or
//! establishes-and-caches the SHARED serving state under the composite warm key.
//! It also exposes whole-component failover and reconnect renegotiation.
//!
//! It is a pure function of its typed inputs and the warm-cache state. The actual
//! attach/relay transport lives in the tsgo API layer; the runtime facts are
//! supplied by the LSP host. Nothing here spawns or contacts an engine.

use std::sync::Arc;

use crate::file_artifact_store::ProjectIdentity;

use super::eligibility::{compose_eligibility, EligibilityFacts};
use super::identity_resolver::{
    build_redirect_reference_graph, ConfigPathProbe, ProjectGraphInput, ProjectIdentitySource,
    ReferenceInput,
};
use super::mode::{
    failover_component_to_owned, select_component_mode, ComponentModeDecision,
    EngineSessionCandidates, FailoverCause, OwnedSessionFacts, ServeMode,
};
use super::warm_cache::{EngineWarmCache, ReconnectGeneration, WarmCacheKey};

/// One project in the live decision snapshot: its identity, the directory and
/// canonical path of its tsconfig, the SHARED-precondition facts to compose its
/// eligibility from, and its declared references.
#[derive(Debug, Clone)]
pub struct LiveProjectInput<'a> {
    /// The project's canonical identity.
    pub identity: ProjectIdentity,
    /// The directory of this project's tsconfig (the base for its relative
    /// references).
    pub tsconfig_dir: &'a str,
    /// The canonical tsconfig path of this project (a warm-key dimension when
    /// this project is the served component's representative).
    pub canonical_tsconfig: Arc<str>,
    /// The five SHARED-precondition facts for this project.
    pub facts: EligibilityFacts,
    /// This project's declared references (redirect-disabled ones are excluded by
    /// the graph builder).
    pub references: &'a [ReferenceInput],
}

/// The live decision request: the queried root, the whole project snapshot, the
/// engine session candidates, and the warm-key context.
#[derive(Debug, Clone)]
pub struct LiveDecisionRequest<'a> {
    /// The project whose component is being decided.
    pub root: ProjectIdentity,
    /// Every project in the decision snapshot.
    pub projects: &'a [LiveProjectInput<'a>],
    /// The candidate engine sessions (OWNED always available; SHARED optional).
    pub engines: &'a EngineSessionCandidates,
    /// The project/config generation (a warm-key dimension).
    pub config_generation: u64,
    /// The editor-binding witness/fingerprint for the queried carrier (a
    /// warm-key dimension).
    pub editor_binding: ProjectIdentity,
}

/// Where a live decision's serving state came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServingProvenance {
    /// SHARED, reused from a pre-existing warm entry (the serving state — the
    /// `--api` session, snapshot, carriers — is shared, not re-established).
    WarmShared,
    /// SHARED, freshly established and cached this call (a cold miss).
    ColdShared,
    /// OWNED — the universal baseline, never cached and never warmed.
    Owned,
}

/// A live decision: the mode decision plus where its serving state came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDecision {
    decision: ComponentModeDecision,
    serving: ServingProvenance,
}

impl LiveDecision {
    /// The underlying component mode decision.
    #[must_use]
    pub fn decision(&self) -> &ComponentModeDecision {
        &self.decision
    }

    /// The serve mode.
    #[must_use]
    pub fn mode(&self) -> ServeMode {
        self.decision.mode()
    }

    /// Where the serving state came from.
    #[must_use]
    pub fn serving(&self) -> ServingProvenance {
        self.serving
    }
}

/// Decide the live serve mode for `request.root`'s component.
///
/// Composes each project's eligibility, builds the canonical-identity redirect
/// graph, and selects the ONE mode. For a SHARED result, the composite warm key
/// is consulted: a hit reuses the cached SHARED serving state
/// ([`ServingProvenance::WarmShared`]); a miss establishes it and caches it
/// ([`ServingProvenance::ColdShared`]). An OWNED result is returned as-is and is
/// never cached ([`ServingProvenance::Owned`]).
#[must_use]
pub fn decide_live(
    request: &LiveDecisionRequest,
    probe: &dyn ConfigPathProbe,
    identity: &dyn ProjectIdentitySource,
    warm_cache: &mut EngineWarmCache,
) -> LiveDecision {
    let decision = compute_decision(request, probe, identity);

    // OWNED is the universal baseline — never cached, never warmed.
    if decision.mode() != ServeMode::Shared {
        return LiveDecision {
            decision,
            serving: ServingProvenance::Owned,
        };
    }

    // SHARED: reuse the warm serving state on a composite-key hit, else establish
    // and cache it.
    let key = WarmCacheKey::for_decision(
        &decision,
        representative_tsconfig(request, &decision),
        request.config_generation,
        request.editor_binding,
    );
    match warm_cache.get(&key) {
        Some(state) => LiveDecision {
            decision: state.decision().clone(),
            serving: ServingProvenance::WarmShared,
        },
        None => {
            warm_cache
                .insert_shared(key, decision.clone())
                .expect("a SHARED decision is admissible");
            LiveDecision {
                decision,
                serving: ServingProvenance::ColdShared,
            }
        }
    }
}

/// Fail the whole component of a prior SHARED decision over to OWNED mid-flight,
/// discarding its stale SHARED warm state.
///
/// Moves the ENTIRE component (never a subset) via
/// [`failover_component_to_owned`], and evicts every warm entry serving that
/// component so no stale SHARED serving state (a dead `--api` handle, a torn
/// snapshot) can be reused.
#[must_use]
pub fn failover_live(
    prior: &ComponentModeDecision,
    cause: FailoverCause,
    owned_session: &OwnedSessionFacts,
    warm_cache: &mut EngineWarmCache,
) -> ComponentModeDecision {
    // Discard every stale SHARED warm entry for the failed-over component before
    // returning the OWNED decision, so no dead --api handle is reused.
    warm_cache.evict_component(prior.members());
    failover_component_to_owned(prior, cause, owned_session)
}

/// Renegotiate the decision on an editor reconnect: recompute with the FRESH
/// (bumped-generation) `request`, evict the superseded prior-generation warm
/// entries, and establish the fresh SHARED serving state.
///
/// A reconnect is NEVER a warm reuse — the fresh generation mints a fresh
/// [`super::mode::EngineIdentity`], so the recomputed SHARED decision is always
/// cold-established under the new key while the prior generation's entries are
/// purged. `request.engines.shared` must already carry the bumped reconnect
/// generation.
#[must_use]
pub fn renegotiate_on_reconnect(
    request: &LiveDecisionRequest,
    probe: &dyn ConfigPathProbe,
    identity: &dyn ProjectIdentitySource,
    warm_cache: &mut EngineWarmCache,
) -> LiveDecision {
    let decision = compute_decision(request, probe, identity);

    // A reconnect that no longer proves SHARED-safe drops to the OWNED baseline;
    // any prior warm state for this component is stale — discard it.
    if decision.mode() != ServeMode::Shared {
        warm_cache.evict_component(decision.members());
        return LiveDecision {
            decision,
            serving: ServingProvenance::Owned,
        };
    }

    // The reconnect minted a fresh generation. Purge the superseded
    // prior-generation entries for this component, then establish the fresh
    // SHARED serving state COLD under the new key — a reconnect is never a warm
    // reuse (the fresh identity would already miss the prior key; this makes the
    // orphaned entry unreachable AND gone).
    let current = decision.engine().editor_session_generation;
    let representative = decision
        .members()
        .members()
        .next()
        .expect("a SHARED decision covers at least the root member");
    warm_cache.evict_superseded_generations(representative, ReconnectGeneration(current));

    let key = WarmCacheKey::for_decision(
        &decision,
        representative_tsconfig(request, &decision),
        request.config_generation,
        request.editor_binding,
    );
    warm_cache
        .insert_shared(key, decision.clone())
        .expect("a SHARED decision is admissible");
    LiveDecision {
        decision,
        serving: ServingProvenance::ColdShared,
    }
}

/// Compute the raw mode decision for `request` — the pure pipeline (compose
/// eligibilities → build canonical graph → select mode), with no cache
/// interaction. Shared by every entry point.
fn compute_decision(
    request: &LiveDecisionRequest,
    probe: &dyn ConfigPathProbe,
    identity: &dyn ProjectIdentitySource,
) -> ComponentModeDecision {
    let graph_inputs: Vec<ProjectGraphInput> = request
        .projects
        .iter()
        .map(|p| ProjectGraphInput {
            identity: p.identity,
            eligibility: compose_eligibility(&p.facts),
            tsconfig_dir: p.tsconfig_dir,
            references: p.references,
        })
        .collect();
    let graph = build_redirect_reference_graph(&graph_inputs, probe, identity);
    select_component_mode(&graph, &request.root, request.engines)
}

/// The canonical tsconfig path of the served component's representative (the
/// byte-minimum member) — the warm-key `canonical_tsconfig` dimension. Empty when
/// the representative is not in the snapshot (unreachable for a SHARED decision,
/// whose members are all present eligible projects).
fn representative_tsconfig(
    request: &LiveDecisionRequest,
    decision: &ComponentModeDecision,
) -> Arc<str> {
    let representative = decision
        .members()
        .members()
        .next()
        .expect("a decision always covers at least the root member");
    request
        .projects
        .iter()
        .find(|p| p.identity == representative)
        .map(|p| Arc::clone(&p.canonical_tsconfig))
        .unwrap_or_else(|| Arc::from(""))
}

#[cfg(test)]
#[path = "live_decision_tests.rs"]
mod tests;
