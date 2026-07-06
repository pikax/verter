//! The ONE shared per-carrier project-binding helper for the tsgo admission layer.
//!
//! A framework carrier reaches the external-TS engine as a member of its REAL
//! configured project — never a config-less inferred / single-file Program. This
//! helper is the SOLE host-backed resolution path both the always-present OWNED
//! carrier-diagnostics gate ([`crate::tsgo::composite`]) and the optional SHARED
//! overlay drive: it resolves a carrier SOURCE to its owning configured project's
//! [`BoundProject`] witness over the host's LIVE published snapshot through the
//! shared [`WorkspaceProjectResolver`], minting the witness from the resolved
//! [`ProjectBinding`] through the tsgo [`EngineBackend`]. There is ONE binding path;
//! neither the OWNED gate nor SHARED resolves ownership on its own.
//!
//! Every non-bound state — a not-yet-ready published snapshot, `NoProject`,
//! `Ambiguous`, `SyntheticScratch`, or an `ensure_project` failure — is a DISTINCT
//! fail-closed outcome that yields NO `BoundProject`, so the caller serves no
//! external-TS result for the carrier (never an inferred/path-only fallback).

use std::sync::Arc;

use verter_session::external_ts::{
    AmbiguityCause, BoundProject, EngineBackend, EnvDims, ExternalTsProjectResolver,
    ProjectBinding, ProjectResolution, WorkspaceProjectResolver,
};
use verter_session::VerterHost;

use crate::external_ts::TsgoEngineBackend;

/// The bootstrap engine version the OWNED gate resolves + mints the witness with.
///
/// `ts_version` is carried onto the resolved binding's metadata and the minted
/// backend capabilities, but it is NOT load-bearing for the witness identity, the
/// binding's project identity / tsconfig / references, or the downstream `--api`
/// operation (OWNED user-facing diagnostics ride the `--lsp` pull; the SHARED `--api`
/// snapshot rail keys on the transport's own gate-observed version). So the coarse
/// bound-or-not gate decision — and the tsconfig the SHARED path reuses from the
/// witness — are version-independent, and this empty bootstrap is safe (it mirrors
/// the shared overlay's `Arc::from("")` shadow-safety probe).
const OWNED_GATE_BOOTSTRAP_VERSION: &str = "";

/// A carrier resolved to its owning configured project's [`BoundProject`] witness,
/// plus the resolved [`ProjectBinding`] and the published-snapshot generation it was
/// resolved at. The SHARED overlay reuses ALL THREE (the binding for its per-query
/// re-decision, the generation for the transport re-arm, and `bound.project()` — the
/// version-independent owning tsconfig — for the `--api` overlay), so a bound carrier
/// is resolved EXACTLY ONCE for both the OWNED gate and the SHARED union.
#[derive(Debug)]
pub struct BoundCarrier {
    bound: BoundProject,
    binding: ProjectBinding,
    generation: u64,
}

impl BoundCarrier {
    /// The minted project-bound witness (its `project()` is the owning tsconfig).
    #[must_use]
    pub fn bound(&self) -> &BoundProject {
        &self.bound
    }

    /// The resolved project binding (for the SHARED per-query re-decision + transport).
    #[must_use]
    pub fn binding(&self) -> &ProjectBinding {
        &self.binding
    }

    /// The published-snapshot / config generation the binding was resolved at.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// The outcome of resolving a carrier source to its owning configured project. Only
/// [`CarrierBinding::Bound`] admits an external-TS result; every other arm is a
/// DISTINCT fail-closed state (kept distinct for testing + diagnostics) that yields
/// NO `BoundProject` — the caller serves no external-TS diagnostics for the carrier,
/// never an inferred/path-only fallback.
#[derive(Debug)]
pub enum CarrierBinding {
    /// A resolved configured project — the ONLY state that admits an external-TS
    /// result. Carries the minted witness + binding + generation (boxed: the bound
    /// payload dwarfs the unit fail-closed variants).
    Bound(Box<BoundCarrier>),
    /// The host's published snapshot is not yet ready (`published_root() == None`) —
    /// fail closed to no result (matches the SHARED `published_root()?` semantics),
    /// NEVER recovered via path-only inferred discovery.
    PreSnapshot,
    /// The resolver found no owning tsconfig for the source.
    NoProject,
    /// Two configs claim the source with no deterministic leaf, or a carrier-path
    /// conflict (a real user file at a companion path / a same-stem rune module).
    Ambiguous(AmbiguityCause),
    /// An untitled buffer / file outside any tsconfig — the scratch lane, never a
    /// configured-project external-TS result.
    SyntheticScratch,
    /// The binding resolved but the engine backend refused to mint the witness.
    EnsureFailed,
}

impl CarrierBinding {
    /// Whether a real configured-project witness resolved (the admission gate).
    #[must_use]
    pub fn is_bound(&self) -> bool {
        matches!(self, CarrierBinding::Bound(_))
    }

    /// The [`BoundCarrier`] IFF a configured project was bound, else `None` — every
    /// non-bound state collapses to the ONE fail-closed `None` the caller gates on.
    #[must_use]
    pub fn into_bound(self) -> Option<BoundCarrier> {
        match self {
            CarrierBinding::Bound(bound) => Some(*bound),
            _ => None,
        }
    }
}

/// Resolve the carrier `source`'s owning project over the host's LIVE published
/// snapshot through the shared [`WorkspaceProjectResolver`], returning the FULL
/// [`ProjectResolution`] and the snapshot/config generation it was resolved at
/// (`None` when the published snapshot is not yet ready). The single host-backed
/// resolution entry the OWNED gate, the SHARED binding path, and the shadow-safety
/// gate all share — the env-dims closure reads the host's per-project R21 env-hash
/// reader (`host_view_env_hashes_for` / `host_view_project_identity_for`), never a
/// fabricated/default env identity.
///
/// `ts_version` is carried onto a resolved binding's metadata; it is NOT load-bearing
/// for the witness identity or the `--api` op, so a bootstrap value (the OWNED gate)
/// or an empty value (the shadow-safety probe) is safe.
#[must_use]
pub fn resolve_carrier(
    host: &Arc<VerterHost>,
    source: &str,
    ts_version: Arc<str>,
) -> Option<(ProjectResolution, u64)> {
    let ws_read = host.workspace_read();
    let published = ws_read.published_root()?;
    let generation = published.snapshot.generation.0;
    let env_dims_source = |tsconfig_uri: &str| {
        let env = host.host_view_env_hashes_for(tsconfig_uri);
        EnvDims {
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity_for(tsconfig_uri),
        }
    };
    let resolver = WorkspaceProjectResolver::new(
        published.snapshot.as_ref(),
        ws_read.as_ref(),
        ts_version,
        &env_dims_source,
    );
    Some((resolver.resolve(source), generation))
}

/// Resolve the carrier `source` to its owning configured project's [`BoundProject`]
/// witness — the ONE admission entry the always-present OWNED carrier-diagnostics
/// gate obtains a `BoundProject` from before delegating to `TsgoOwnedProvider`.
///
/// Published-snapshot → [`WorkspaceProjectResolver`] → `resolve(source)` → on
/// [`ProjectResolution::ProjectBinding`] mint the witness through
/// `TsgoEngineBackend::ensure_project(binding.ensure_project_request())`. Every other
/// state ([`ProjectResolution::NoProject`] / [`ProjectResolution::Ambiguous`] /
/// [`ProjectResolution::SyntheticScratch`], a pre-published snapshot, or an
/// `ensure_project` failure) is a DISTINCT fail-closed [`CarrierBinding`] variant
/// that yields NO witness — NEVER a path-only inferred fallback.
#[must_use]
pub fn resolve_carrier_bound(host: &Arc<VerterHost>, source: &str) -> CarrierBinding {
    let ts_version: Arc<str> = Arc::from(OWNED_GATE_BOOTSTRAP_VERSION);
    let Some((resolution, generation)) = resolve_carrier(host, source, Arc::clone(&ts_version))
    else {
        return CarrierBinding::PreSnapshot;
    };
    match resolution {
        ProjectResolution::ProjectBinding(binding) => {
            // Mint the BoundProject witness through the tsgo engine backend — the
            // project-bound contract's per-query witness discipline (no path-only
            // bypass). `ensure_project` is an infallible pure witness mint for a
            // resolved binding, but a refusal is a DISTINCT fail-closed state.
            let backend = TsgoEngineBackend::new(ts_version);
            match backend.ensure_project(binding.ensure_project_request()) {
                Ok(bound) => CarrierBinding::Bound(Box::new(BoundCarrier {
                    bound,
                    binding,
                    generation,
                })),
                Err(_) => CarrierBinding::EnsureFailed,
            }
        }
        ProjectResolution::NoProject => CarrierBinding::NoProject,
        ProjectResolution::Ambiguous(cause) => CarrierBinding::Ambiguous(cause),
        ProjectResolution::SyntheticScratch(_) => CarrierBinding::SyntheticScratch,
    }
}
