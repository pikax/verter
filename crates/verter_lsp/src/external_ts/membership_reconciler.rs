//! The authoritative source-membership reconciler.
//!
//! ONE place owns the full source-membership transition, so a server path can never
//! resolve ownership, transition the provider buffer, and mutate durable state in
//! three different places that drift. The reconciler, for a single source:
//!
//! 1. resolves ownership EXACTLY ONCE (through the [`OwnershipAuthority`] seam — in
//!    production the shared `WorkspaceProjectResolver`);
//! 2. computes the [`DesiredMembership`] (an owned advertisement, a typed absent
//!    reason, or a bootstrap-unknown deferral) — `Absent` is a first-class outcome
//!    of the SAME decision, never a separate side-call a branch can forget;
//! 3. performs the provider-buffer transition through the resilient
//!    single-writer actor's command API (`register_carrier_member` / `close_file`),
//!    never by reaching into provider internals;
//! 4. commits the durable [`MembershipLedger`] mutation TRANSACTIONALLY.
//!
//! ## Atomic owner change (A→B)
//!
//! An owner change is an atomic source-membership REPLACEMENT: the new companions
//! are staged on the provider, then ONE source-indexed ledger entry is swapped from
//! A to B. It is never publish-then-prune — because the ledger is source-indexed,
//! the swap leaves nothing under the old project by construction.
//!
//! ## Bootstrap (cold ownership)
//!
//! Cold / not-yet-authoritative ownership is [`DesiredMembership::Unknown`], NOT
//! `Absent`/`NoProject`. It DEFERS without thrash: no provider transition, no ledger
//! mutation, and it is NOT reported as clean success (the outcome is
//! [`ReconcileOutcome::Deferred`], distinct from an advertisement). A transient cold
//! "no owner" therefore never retracts an existing advertisement and never
//! advertises a new carrier.
//!
//! ## Fail-closed
//!
//! `Ok(ReconcileOutcome)` is returned ONLY if the computed desired state was
//! actually reached: the membership commit succeeded, the provider transition
//! succeeded, AND the ledger commit's post-commit verification passed. A failure at
//! any step returns `Err` carrying the desired state, so a request caller can
//! propagate with `?` and a background caller can mark the source unhealthy + back
//! off + surface external-TS degradation — never a silent false "not published".
//!
//! This reconciler is the SINGLE authoritative entry for every source-membership
//! transition: the low-level publish / retract store mutators are sealed so a server
//! path cannot bypass it, and every production decision point routes through
//! [`reconcile_source_membership`](MembershipReconciler::reconcile_source_membership)
//! / [`remove_source_membership`](MembershipReconciler::remove_source_membership).

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use verter_session::external_ts::{ProjectBinding, ProjectResolution};

use super::membership_ledger::{
    AbsentReason, CanonicalSource, LedgerCompanion, MembershipLedger, MembershipRecord, ProjectUri,
    SessionGen,
};
use super::publish_coordinator::{CarrierCompanion, CarrierPublishError};
use crate::type_provider::traits::TypeProvider;

/// The error a membership commit / retract surfaces. Engine-agnostic at the seam:
/// the on-disk (tsserver) committer surfaces its store-mutation failures here, a
/// future in-memory-overlay committer surfaces its re-snapshot failures here, and
/// the reconciler maps EITHER to the same fail-closed [`ReconcileErr::MembershipCommit`]
/// without committing the ledger.
pub type CommitErr = CarrierPublishError;

/// A boxed, `Send` future — the return type for every [`CarrierMembershipCommitter`]
/// operation. Mirrors the provider's boxed-future convention so the seam stays
/// `dyn`-dispatched (the reconciler holds an `Arc<dyn CarrierMembershipCommitter>`)
/// while being genuinely asynchronous: a committer whose membership commit is async
/// (an in-memory overlay's `update_snapshot`) returns its real future, and the
/// reconciler `.await`s it before committing the ledger.
pub type CommitFuture<'a> = Pin<Box<dyn Future<Output = Result<(), CommitErr>> + Send + 'a>>;

/// The engine-agnostic membership-commit seam the reconciler drives as the durable
/// half of an authoritative source-membership transition.
///
/// "Committed to the engine" — NOT "written to disk". A carrier's membership is the
/// set of companions the external TypeScript engine ADVERTISES for a source under a
/// project. Different engines make that membership effective differently: the
/// tsserver engine writes the companions into an on-disk content-addressed store
/// (blobs + manifest) the out-of-process `@verter/typescript-plugin` reads
/// (`getExternalFiles`); an in-memory-overlay engine instead installs the companions
/// as an overlay and re-snapshots the project. BOTH satisfy this one seam, so the
/// reconciler is the SINGLE membership choke-point regardless of engine.
///
/// The commit is ASYNC (a boxed future): an in-memory-overlay engine's re-snapshot
/// is inherently asynchronous, and a synchronous seam would force it to block or hide
/// the failure on a background task. An engine whose commit is synchronous (the
/// on-disk store is a synchronous fsync'd swap) simply does its work inline inside the
/// returned future. The reconciler's transition methods are already `async`, so
/// awaiting the committer is natural and the failure stays on the fail-closed path.
///
/// Production wires this to the [`CarrierPublishCoordinator`](super::publish_coordinator::CarrierPublishCoordinator)
/// (the on-disk implementation, which owns the
/// [`TsserverEngineBackend`](super::tsserver_backend::TsserverEngineBackend)); the
/// reconciler's unit tests supply a recording mock. The seam takes the ALREADY
/// resolved [`ProjectBinding`] so the commit does not re-resolve ownership (the lone
/// resolution happens once through the [`OwnershipAuthority`]).
pub trait CarrierMembershipCommitter: Send + Sync {
    /// Commit the resolved owned carrier's companions as the engine's advertised
    /// membership for this source under this project, pruning the source from every
    /// OTHER project so an owner change leaves nothing under the old project. No
    /// ownership re-resolution. A failed commit returns `Err` (the reconciler does
    /// NOT commit the ledger).
    fn commit_owned<'a>(
        &'a self,
        binding: &'a ProjectBinding,
        source_canonical: &'a str,
        companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a>;

    /// Retract the source's advertised membership across EVERY project — the
    /// owner-loss / delete transition that stops the engine advertising it. A failed
    /// retract returns `Err` (the reconciler does NOT commit the tombstone).
    fn retract<'a>(&'a self, source_canonical: &'a str) -> CommitFuture<'a>;
}

/// Why a reconciliation was triggered — the caller's contextual reason.
///
/// Some triggers are CALLER-AUTHORITATIVE terminal-absent reasons that short-circuit
/// to a tombstone WITHOUT consulting ownership (a deleted / compile-failed /
/// conflict-removed source has no resolvable owner to advertise). The rest defer the
/// owned-vs-absent decision to the single ownership resolution (whose conflict pass
/// may itself yield `Ambiguous`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileReason {
    /// A source content / sync edit — the outcome is decided by ownership resolution.
    SourceSynced,
    /// A project-config (`include`/`exclude`/tsconfig) edit — outcome by ownership.
    ConfigChanged,
    /// CALLER-AUTHORITATIVE terminal: the source was deleted.
    Deleted,
    /// CALLER-AUTHORITATIVE terminal: the carrier failed to compile.
    CompileFailed,
    /// CALLER-AUTHORITATIVE terminal: a path conflict removed the source.
    ConflictRemoved,
}

impl ReconcileReason {
    /// The terminal absent reason this trigger asserts directly, if any. `None`
    /// triggers defer the owned-vs-absent decision to ownership resolution.
    #[must_use]
    fn terminal_absent_reason(self) -> Option<AbsentReason> {
        match self {
            ReconcileReason::Deleted => Some(AbsentReason::Deleted),
            ReconcileReason::CompileFailed => Some(AbsentReason::CompileFailed),
            ReconcileReason::ConflictRemoved => Some(AbsentReason::ConflictRemoved),
            ReconcileReason::SourceSynced | ReconcileReason::ConfigChanged => None,
        }
    }
}

/// Why a desired membership is bootstrap-unknown (cold ownership).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapKind {
    /// The session has not yet produced an authoritative ownership snapshot.
    ColdStart,
    /// Ownership resolution is pending for this source (no authoritative answer yet).
    OwnershipPending,
}

/// The bootstrap state of a deferred reconciliation — the session generation it was
/// observed under plus why it is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapState {
    /// The session generation at which the bootstrap deferral was observed.
    pub gen: SessionGen,
    /// Why the membership is unknown.
    pub kind: BootstrapKind,
}

/// The computed desired membership for a source — the closed outcome of the single
/// ownership decision.
///
/// `Absent` is a first-class arm of the SAME decision that produces `Owned`; it is
/// never a separate side-call. `Unknown` is the cold/bootstrap deferral (NOT
/// `Absent`).
#[derive(Debug, Clone)]
pub enum DesiredMembership {
    /// Owned by a configured project: advertise exactly these companions under it.
    Owned {
        /// The owning project.
        project: ProjectUri,
        /// The companions (with content) to advertise / stage on the provider.
        companions: Vec<CarrierCompanion>,
        /// The session generation this desired state was computed under.
        gen: SessionGen,
    },
    /// Not owned: advertise nothing for the source (a tombstone), for this reason.
    Absent {
        /// Why the source is not advertised.
        reason: AbsentReason,
        /// The session generation this desired state was computed under.
        gen: SessionGen,
    },
    /// Cold / not-yet-authoritative ownership — defer without thrash.
    Unknown {
        /// The bootstrap state (generation + why unknown).
        bootstrap: BootstrapState,
    },
}

/// The single ownership decision the reconciler consults exactly ONCE per reconcile.
///
/// Yields BOTH the owned and the absent outcomes from one resolution, plus the
/// cold/bootstrap signal — so the reconciler never resolves ownership twice and
/// `Absent` is never a forgotten side-call.
#[derive(Debug, Clone)]
pub enum OwnershipDecision {
    /// A configured project owns the source; advertise these companions under it.
    /// Carries the RESOLVED [`ProjectBinding`] (not just the project URI) so the
    /// reconciler's membership commit mints the engine witness + project env dims
    /// WITHOUT re-resolving ownership (the single resolution already happened).
    Owned {
        /// The resolved owning-project binding.
        binding: ProjectBinding,
        /// The companions to advertise.
        companions: Vec<CarrierCompanion>,
    },
    /// Ownership is authoritative and resolves no usable owner.
    Absent {
        /// Why there is no advertisement.
        reason: AbsentReason,
    },
    /// Ownership is not yet authoritative (cold) — defer.
    Bootstrap {
        /// Why the membership is unknown.
        kind: BootstrapKind,
    },
}

/// The ownership-resolution seam the reconciler consults exactly once per reconcile.
///
/// Production wires this to the shared `WorkspaceProjectResolver` (see
/// [`ResolverOwnershipAuthority`]); tests supply a deterministic decision. The seam
/// is the single resolution point — the reconciler never resolves ownership itself.
///
/// `Send + Sync`: the reconciler holds `&dyn OwnershipAuthority` while it `.await`s
/// the provider transition, so the trait object must be shareable across the LSP's
/// multi-thread executor. The production path resolves ownership SYNCHRONOUSLY off a
/// borrowing resolver and hands the reconciler a [`PrecomputedOwnershipAuthority`]
/// (all-owned, `Send + Sync`) — the borrowing resolver never crosses an await.
pub trait OwnershipAuthority: Send + Sync {
    /// Resolve the ownership decision for `source`. Called at most once per
    /// reconcile.
    fn resolve_membership(&self, source: &CanonicalSource) -> OwnershipDecision;
}

/// An [`OwnershipAuthority`] holding an ALREADY-resolved decision.
///
/// The production decision points resolve ownership synchronously (the
/// `WorkspaceProjectResolver` borrows non-`Sync` workspace state, so it must be
/// dropped before any `.await`), map the result through
/// [`ResolverOwnershipAuthority::resolve_membership`], and hand the reconciler this
/// precomputed, all-owned (`Send + Sync`) authority — so the reconciler future stays
/// `Send` without the borrowing resolver ever crossing an await.
pub struct PrecomputedOwnershipAuthority {
    decision: OwnershipDecision,
}

impl PrecomputedOwnershipAuthority {
    /// Wrap an already-resolved ownership decision.
    #[must_use]
    pub fn new(decision: OwnershipDecision) -> Self {
        Self { decision }
    }
}

impl OwnershipAuthority for PrecomputedOwnershipAuthority {
    fn resolve_membership(&self, _source: &CanonicalSource) -> OwnershipDecision {
        self.decision.clone()
    }
}

/// Whether an ownership snapshot is authoritative yet, or still bootstrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityState {
    /// The ownership snapshot is authoritative — resolve owned-vs-absent.
    Ready,
    /// The ownership snapshot is cold — defer (bootstrap-unknown), do not thrash.
    Bootstrap,
}

/// The production [`OwnershipAuthority`] adapter over the shared project resolver.
///
/// Holds the authoritative-vs-cold [`AuthorityState`], a `resolve` closure that runs
/// the existing `WorkspaceProjectResolver` (called EXACTLY ONCE inside
/// [`OwnershipAuthority::resolve_membership`]), and the already-compiled companions
/// to advertise on an owned resolution. It maps the resolver's
/// [`ProjectResolution`] onto an [`OwnershipDecision`] — the single mapping from the
/// existing resolver's four states to the membership outcome.
///
/// Constructing this with the live resolver closure at each decision point is a
/// later step; this is the reusable bridge that wiring will use.
pub struct ResolverOwnershipAuthority<F> {
    authority_state: AuthorityState,
    resolve: F,
    companions: Vec<CarrierCompanion>,
}

impl<F> ResolverOwnershipAuthority<F>
where
    F: Fn(&str) -> ProjectResolution,
{
    /// Build the adapter over the authoritative-vs-cold state, the single-resolution
    /// closure (the existing resolver), and the companions to advertise when owned.
    #[must_use]
    pub fn new(
        authority_state: AuthorityState,
        resolve: F,
        companions: Vec<CarrierCompanion>,
    ) -> Self {
        Self {
            authority_state,
            resolve,
            companions,
        }
    }
}

impl<F> ResolverOwnershipAuthority<F>
where
    F: Fn(&str) -> ProjectResolution,
{
    /// Resolve the ownership decision SYNCHRONOUSLY (an inherent method, NOT the
    /// [`OwnershipAuthority`] trait — the borrowing resolver closure is not `Sync`,
    /// so this adapter never crosses an await; the caller maps its owned result into
    /// a [`PrecomputedOwnershipAuthority`] for the reconciler).
    #[must_use]
    pub fn resolve_membership(&self, source: &CanonicalSource) -> OwnershipDecision {
        // Cold snapshot ⇒ defer (bootstrap-unknown), do not resolve / thrash.
        if matches!(self.authority_state, AuthorityState::Bootstrap) {
            return OwnershipDecision::Bootstrap {
                kind: BootstrapKind::OwnershipPending,
            };
        }
        // Resolve ownership EXACTLY ONCE, mapping the existing resolver's four states
        // onto the membership decision. `Absent` is produced HERE, from the same
        // resolution — never a separate later call.
        match (self.resolve)(source.as_str()) {
            ProjectResolution::ProjectBinding(binding) => OwnershipDecision::Owned {
                binding,
                companions: self.companions.clone(),
            },
            ProjectResolution::NoProject => OwnershipDecision::Absent {
                reason: AbsentReason::NoProject,
            },
            ProjectResolution::Ambiguous(_) => OwnershipDecision::Absent {
                reason: AbsentReason::Ambiguous,
            },
            ProjectResolution::SyntheticScratch(_) => OwnershipDecision::Absent {
                reason: AbsentReason::SyntheticScratch,
            },
        }
    }
}

/// The successful outcome of a reconciliation.
///
/// `#[must_use]`: a caller must observe whether the source ended advertised,
/// tombstoned, or merely DEFERRED — a `Deferred` outcome is NOT clean success and a
/// caller that drops it would silently treat a bootstrap deferral as done.
#[derive(Debug, Clone)]
#[must_use]
pub enum ReconcileOutcome {
    /// The ledger now advertises exactly the source's companions under `project`.
    Advertised {
        /// The owning project the source is now advertised under.
        project: ProjectUri,
        /// How many companions are advertised.
        companions: usize,
        /// The session generation the advertisement was written under.
        gen: SessionGen,
        /// The previous owning project, when this reconciliation replaced an
        /// advertisement under a DIFFERENT project (an owner change A→B).
        replaced: Option<ProjectUri>,
    },
    /// The ledger advertises nothing for the source (a tombstone is present).
    Tombstoned {
        /// Why the source is not advertised.
        reason: AbsentReason,
        /// The session generation the tombstone was written under.
        gen: SessionGen,
    },
    /// Ownership was cold: the reconciliation DEFERRED. Not advertised, not clean
    /// success — the caller should retry once ownership is authoritative.
    Deferred {
        /// The bootstrap state of the deferral.
        bootstrap: BootstrapState,
    },
}

/// A failed reconciliation — the desired state was NOT reached.
#[derive(Debug, Clone)]
pub enum ReconcileErr {
    /// The membership commit (the engine-agnostic durable half — an on-disk
    /// blobs+manifest write for tsserver, an overlay re-snapshot for an in-memory
    /// engine) failed, so the engine's advertised view could not be reached. The
    /// ledger was NOT committed (fail-closed: the source is never reported advertised
    /// while the commit did not reach the desired state).
    MembershipCommit {
        /// The desired state that could not be reached.
        desired: DesiredMembership,
        /// Detail from the failing membership commit.
        detail: String,
    },
    /// The provider-buffer transition (a resilient-actor command) failed, so the
    /// desired state could not be staged. The ledger was NOT committed.
    ProviderTransition {
        /// The desired state that could not be reached.
        desired: DesiredMembership,
        /// Detail from the failing provider command.
        detail: String,
    },
    /// The durable ledger commit failed its post-commit verification — the ledger
    /// did not reach the desired record. Never reported as success.
    LedgerCommit {
        /// The desired state that could not be reached.
        desired: DesiredMembership,
        /// Detail from the failing commit.
        detail: String,
    },
}

impl std::fmt::Display for ReconcileErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconcileErr::MembershipCommit { detail, .. } => {
                write!(f, "carrier membership commit failed: {detail}")
            }
            ReconcileErr::ProviderTransition { detail, .. } => {
                write!(f, "provider-buffer transition failed: {detail}")
            }
            ReconcileErr::LedgerCommit { detail, .. } => {
                write!(f, "membership ledger commit failed: {detail}")
            }
        }
    }
}

impl std::error::Error for ReconcileErr {}

/// The authoritative source-membership reconciler.
///
/// Holds the source-indexed [`MembershipLedger`] (the membership authority) and the
/// type provider (the resilient single-writer actor, reached only through
/// its public command API). One reconciler per session; cheap to clone (`Arc`).
#[derive(Clone)]
pub struct MembershipReconciler {
    ledger: Arc<MembershipLedger>,
    provider: Arc<dyn TypeProvider>,
    committer: Arc<dyn CarrierMembershipCommitter>,
}

impl MembershipReconciler {
    /// Build the reconciler over the shared ledger, the active provider (the
    /// resilient single-writer actor), and the engine-agnostic membership-commit
    /// seam (the engine's advertised-membership surface).
    #[must_use]
    pub fn new(
        ledger: Arc<MembershipLedger>,
        provider: Arc<dyn TypeProvider>,
        committer: Arc<dyn CarrierMembershipCommitter>,
    ) -> Self {
        Self {
            ledger,
            provider,
            committer,
        }
    }

    /// The shared membership ledger (the source-indexed advertisement authority).
    #[must_use]
    pub fn ledger(&self) -> &Arc<MembershipLedger> {
        &self.ledger
    }

    /// Reconcile a source's membership to the single ownership decision.
    ///
    /// Resolves ownership ONCE (via `ownership_authority`), computes the desired
    /// membership, performs the provider-buffer transition, and commits the ledger
    /// transactionally. A caller-authoritative terminal `reason`
    /// (deleted / compile-failed / conflict-removed) short-circuits to a tombstone
    /// without consulting ownership.
    ///
    /// Returns `Ok` ONLY if the desired state was reached (provider staged + ledger
    /// verified). A cold ownership snapshot returns `Ok(Deferred)` — deferred, not
    /// advertised, not clean success.
    pub async fn reconcile_source_membership(
        &self,
        source: &CanonicalSource,
        ownership_authority: &dyn OwnershipAuthority,
        reason: ReconcileReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        // A caller-authoritative terminal reason is an absent outcome on its own —
        // a deleted / compile-failed / conflict-removed source has no owner to
        // resolve.
        if let Some(absent) = reason.terminal_absent_reason() {
            return self.apply_absent(source, absent).await;
        }

        // Resolve ownership EXACTLY ONCE. `Absent` is a first-class arm of the same
        // decision; `Bootstrap` is the cold deferral.
        match ownership_authority.resolve_membership(source) {
            OwnershipDecision::Owned {
                binding,
                companions,
            } => self.apply_owned(source, binding, companions).await,
            OwnershipDecision::Absent { reason } => self.apply_absent(source, reason).await,
            OwnershipDecision::Bootstrap { kind } => {
                // Defer WITHOUT thrash: no provider transition, no ledger mutation,
                // and NOT reported as clean success.
                let gen = self.ledger.current_session();
                Ok(ReconcileOutcome::Deferred {
                    bootstrap: BootstrapState { gen, kind },
                })
            }
        }
    }

    /// Remove a source's membership — the explicit terminal-absent (delete / owner
    /// lost / conflict-removed) path. Closes any advertised companion buffers and
    /// commits a tombstone transactionally.
    pub async fn remove_source_membership(
        &self,
        source: &CanonicalSource,
        reason: AbsentReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        self.apply_absent(source, reason).await
    }

    /// Stage an owned advertisement: commit the companions as the engine's advertised
    /// membership (the on-disk committer writes blobs + manifest and prunes the old
    /// project; an in-memory committer re-snapshots its overlay), register the new
    /// companions on the provider buffer, close any stale companions the prior record
    /// no longer covers, then atomically swap the single ledger entry to the new
    /// advertisement. Fail-closed: any step failing returns `Err` and the ledger is
    /// NOT committed.
    async fn apply_owned(
        &self,
        source: &CanonicalSource,
        binding: ProjectBinding,
        companions: Vec<CarrierCompanion>,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        let gen = self.ledger.current_session();
        let project = ProjectUri::new(binding.tsconfig_uri());
        let desired = DesiredMembership::Owned {
            project: project.clone(),
            companions: companions.clone(),
            gen,
        };

        // Read the prior record (lock taken + dropped INSIDE the ledger; no guard is
        // held across the `.await`s below). Used to detect an owner change and to
        // close companions the new advertisement no longer covers.
        let prior = self.ledger.record_snapshot(source);
        let replaced = match &prior {
            Some(MembershipRecord::Advertised {
                project: prior_project,
                ..
            }) if *prior_project != project => Some(prior_project.clone()),
            _ => None,
        };
        let new_paths: HashSet<&str> = companions
            .iter()
            .map(|companion| companion.provider_uri.as_ref())
            .collect();
        let stale: Vec<Arc<str>> = match &prior {
            Some(MembershipRecord::Advertised {
                companions: prior_companions,
                ..
            }) => prior_companions
                .iter()
                .filter(|companion| !new_paths.contains(companion.provider_uri.as_ref()))
                .map(|companion| Arc::clone(&companion.provider_uri))
                .collect(),
            _ => Vec::new(),
        };

        // 1. The membership commit runs before the provider opens the companion
        // buffer — the engine serves the carrier's advertised membership from the
        // committed state (the tsserver plugin reads the on-disk store cross-process;
        // an in-memory engine re-snapshots its overlay), so the membership must be
        // committed ahead of the companion-buffer open.
        // The commit is source-indexed-pruning (the source is removed from every OTHER
        // project), so an owner change leaves nothing under the old project. A commit
        // failure fails closed (ledger NOT committed).
        self.committer
            .commit_owned(&binding, source.as_str(), &companions)
            .await
            .map_err(|err| ReconcileErr::MembershipCommit {
                desired: desired.clone(),
                detail: err.to_string(),
            })?;

        // 2. Provider-buffer transition through the resilient actor command API. NO
        // ledger lock is held across these awaits. Register the new companions
        // first (so an owner change re-points the buffer to the new project before
        // the old buffer is dropped), then close the stale ones.
        for companion in &companions {
            self.provider
                .register_carrier_member(
                    &companion.provider_uri,
                    &companion.content,
                    project.as_str(),
                )
                .await
                .map_err(|err| ReconcileErr::ProviderTransition {
                    desired: desired.clone(),
                    detail: err.to_string(),
                })?;
        }
        for stale_path in &stale {
            self.provider.close_file(stale_path).await.map_err(|err| {
                ReconcileErr::ProviderTransition {
                    desired: desired.clone(),
                    detail: err.to_string(),
                }
            })?;
        }

        // 3. Evict tsserver's sticky resolution for every published companion now
        // its content is on disk — best-effort (the store is warm; the next
        // interaction re-reads), so an eviction failure never fails the publish.
        for companion in &companions {
            let _ = self
                .provider
                .notify_carrier_changed(&companion.provider_uri)
                .await;
        }

        // 4. Durable ledger commit (transactional + post-commit verify). Source-indexed,
        // so this REPLACES the single entry — an owner change leaves nothing under
        // the old project.
        let record = MembershipRecord::Advertised {
            project: project.clone(),
            companions: companions
                .iter()
                .map(|companion| LedgerCompanion {
                    provider_uri: Arc::clone(&companion.provider_uri),
                    role: companion.role,
                    script_kind: companion.script_kind,
                })
                .collect(),
            lease: gen,
        };
        self.ledger
            .commit(source, record)
            .map_err(|err| ReconcileErr::LedgerCommit {
                desired,
                detail: err.to_string(),
            })?;

        Ok(ReconcileOutcome::Advertised {
            project,
            companions: companions.len(),
            gen,
            replaced,
        })
    }

    /// Apply an absent membership: close any advertised companion buffers, then
    /// atomically commit a tombstone (cheap — no companion set retained).
    async fn apply_absent(
        &self,
        source: &CanonicalSource,
        reason: AbsentReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        let gen = self.ledger.current_session();
        let desired = DesiredMembership::Absent { reason, gen };

        // 1. Retract the membership FIRST so the engine stops advertising the carrier.
        // UNCONDITIONAL (not gated on a prior ledger advertisement): a commit path may
        // have left stale advertised membership the ledger never recorded, and owner
        // loss must clear it. Fail-closed: a retract failure returns `Err` and the
        // tombstone is NOT committed (the source may still be advertised — never
        // report a false absence).
        self.committer
            .retract(source.as_str())
            .await
            .map_err(|err| ReconcileErr::MembershipCommit {
                desired: desired.clone(),
                detail: err.to_string(),
            })?;

        // 2. Close the prior advertisement's companion buffers (if any). The actor's
        // close command also drops the carrier registration, so a respawn does not
        // replay a retracted carrier. NO ledger lock is held across these awaits.
        let prior = self.ledger.record_snapshot(source);
        if let Some(MembershipRecord::Advertised { companions, .. }) = &prior {
            for companion in companions {
                self.provider
                    .close_file(&companion.provider_uri)
                    .await
                    .map_err(|err| ReconcileErr::ProviderTransition {
                        desired: desired.clone(),
                        detail: err.to_string(),
                    })?;
            }
        }

        // Durable tombstone commit (transactional + post-commit verify).
        self.ledger
            .commit(source, MembershipRecord::Tombstone { reason, lease: gen })
            .map_err(|err| ReconcileErr::LedgerCommit {
                desired,
                detail: err.to_string(),
            })?;

        Ok(ReconcileOutcome::Tombstoned { reason, gen })
    }
}

#[cfg(test)]
#[path = "membership_reconciler_tests.rs"]
mod tests;
