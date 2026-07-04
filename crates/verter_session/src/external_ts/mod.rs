//! The project-bound external-TypeScript-engine contract.
//!
//! A provider-neutral, three-layer contract in which a config-less /
//! inferred-project operation for a production carrier source is NOT
//! representable. The shared layer owns ownership resolution, carrier identity,
//! and the fail-closed policy; each engine backend owns only its native
//! project/host mechanics.
//!
//! ```text
//! ExternalTsProjectResolver:  source_uri -> ProjectResolution
//!     { ProjectBinding | NoProject | Ambiguous | SyntheticScratch }
//! CarrierRegistry:            source_uri -> Option<CarrierArtifact>
//! EngineBackend (one per engine kind):
//!     ensure_project -> BoundProject   (the witness)
//!     publish_snapshot / query / diagnostics   (require &BoundProject)
//!     capabilities
//! ```
//!
//! This contract is a SEPARATE concern from Verter's own semantic resolver
//! (`SemanticGraphStore` / `ProjectSemanticDispatch`) and the `typeinfo` /
//! component-meta feature — the LSP/TSC path never routes through them.
//!
//! ## `provider_op_requires_resolved_project` (type-state, not a runtime check)
//!
//! The production-result ops live on [`EngineBackend`] and take a
//! [`BoundProject`]. A `BoundProject` is obtainable ONLY via
//! [`EngineBackend::ensure_project`], whose [`EnsureProject`] argument is mintable
//! ONLY from a resolved [`ProjectBinding`]. `NoProject` / `Ambiguous` carry no
//! binding, so they can reach no production op; `SyntheticScratch` carries a
//! distinct [`ScratchProject`] witness for non-cross-file features only. The
//! impossibility is enforced by the compiler; the `provider_op_requires_resolved_project`
//! architecture guard is the static backstop.
//!
//! The tsserver engine is wired live on this contract: the LSP's carrier-publish
//! coordinator resolves a `.vue`/`.svelte` source to its configured project,
//! mints the witness via `ensure_project`, and publishes the carrier companions
//! into the on-disk store the `@verter/typescript-plugin` reads — making the
//! carrier a configured-project member. The tsgo engine is migrated onto this
//! contract separately.

mod carrier;
mod engine;
mod mode;
mod resolver;

// Explicit re-exports only. We deliberately do NOT `pub use resolver::*`: that
// would risk putting a bare `ProjectResolver` next to the re-exported
// `verter_semantic::analysis::project_resolver::ProjectResolver`. The resolver
// trait is exported under its non-colliding name `ExternalTsProjectResolver`.
pub use carrier::{CarrierArtifact, CarrierRegistry, CarrierRole, InMemoryCarrierRegistry};
pub use engine::{
    BoundProject, BoundProjectSeal, Diagnostics, DiagnosticsOutcome, EngineBackend,
    EngineCapabilities, EngineError, EnsureProject, EnvDims, OpenState, PublishSnapshot, Query,
    QueryFeature, QueryOutcome, ScratchProject, ScratchProjectSeal, ScriptKind, SnapshotFile,
    SnapshotRole,
};
pub use mode::{
    editor_binding_matches, failover_component_to_owned, select_component_mode,
    ComponentModeDecision, EligibilityFailure, EngineIdentity, EngineSessionCandidates,
    EngineSessionFacts, FailoverCause, OwnedReason, OwnedSessionFacts, ProjectEligibility,
    RedirectRef, RedirectReferenceGraph, ReferenceComponent, ServeMode, SharedSessionFacts,
};
pub use resolver::{
    AmbiguityCause, ExternalTsProjectResolver, ProjectBinding, ProjectEnvDimsSource,
    ProjectResolution, ScratchBinding, WorkspaceProjectResolver,
};

#[cfg(test)]
#[path = "external_ts_tests.rs"]
mod tests;
