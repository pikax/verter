//! The authoritative source-membership reconciler.
//!
//! ONE place owns the full source-membership transition, so a server path can never
//! route the ownership decision, transition the provider buffer, and mutate durable
//! state in three different places that drift. The reconciler, for a single source:
//!
//! 1. receives the ONE captured [`CarrierOwnershipResolution`] — resolved exactly
//!    once by the caller through the shared `WorkspaceProjectResolver` — and routes
//!    it onto the membership outcome; it never re-resolves ownership itself;
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

use dashmap::DashMap;
use tokio::sync::Mutex as AsyncMutex;

use verter_semantic::analysis::types::Hash16;
use verter_session::external_ts::{CarrierOwnershipResolution, ProjectBinding, SnapshotRole};
use verter_workspace::workspace_snapshot::SnapshotGeneration;

use super::membership_ledger::{
    AbsentReason, CanonicalSource, LedgerCompanion, MembershipLedger, MembershipRecord, ProjectUri,
    SessionGen,
};
use super::publish_coordinator::{CarrierCompanion, CarrierPublishError};
use crate::type_provider::traits::TypeProvider;

/// The opaque provider-generation epoch stamped onto a [`ProviderReadyReceipt`] at
/// mint. It carries the producing engine's identity label (`tsserver` / `tsgo`) plus
/// the resolved binding's ownership generation. The shape is deliberately opaque so
/// it can later carry full per-project engine routing without changing the receipt's
/// public surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderGeneration {
    engine: Arc<str>,
    generation: SnapshotGeneration,
}

impl ProviderGeneration {
    /// The producing engine's identity label (`tsserver` / `tsgo`).
    #[must_use]
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// The ownership generation this readiness was minted at.
    #[must_use]
    pub fn generation(&self) -> SnapshotGeneration {
        self.generation
    }
}

/// A versioned fingerprint of ONE advertised companion, recorded on the readiness
/// receipt so a later consumer can validate the exact published identity without
/// re-reading the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionFingerprint {
    /// The companion provider path (`/proj/src/Comp.vue.tsx`).
    pub uri: Arc<str>,
    /// The companion's contract role (`CarrierIde` / `CarrierApi`).
    pub role: SnapshotRole,
    /// Content-addressed hash of the published companion bytes.
    pub content_hash: Hash16,
    /// Content-addressed hash of the companion's source-map JSON (all-zero when none).
    pub map_hash: Hash16,
    /// The monotonic provider version for this companion.
    pub version: u64,
}

/// A SEALED receipt attesting that a carrier source's provider-ready transition
/// completed at an exact identity/generation — the capability token that gates
/// committing a carrier's provider state.
///
/// Its fields are PRIVATE and `#[must_use]`; a receipt can only be minted through the
/// MODULE-PRIVATE [`ProviderReadyReceipt::mint`], which REQUIRES a resolved
/// [`ProjectBinding`]. A `ProjectBinding` is minted ONLY inside `verter_session`'s
/// [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
/// (its constructor is crate-private there), so a caller cannot fabricate a receipt
/// without a real carrier-ownership resolution — representing readiness without a
/// resolved owner is structurally impossible. `mint` is reachable ONLY from this
/// module: the tsserver path mints at the END of the ordered `apply_owned`
/// transaction (after the ledger commit + verify); the tsgo direct-open path mints
/// through [`PendingProviderReady::confirm_opened`] AFTER the site opens the companion
/// buffers — so a receipt never precedes its publish/open on either engine.
#[derive(Debug, Clone)]
#[must_use]
pub struct ProviderReadyReceipt {
    source_revision: u64,
    project_generation: SnapshotGeneration,
    /// The per-source LOCAL INTENT EPOCH captured when the carrier transaction began (the
    /// coordinator's owner-loss barrier value at that instant). The admission gate refuses a
    /// receipt whose epoch no longer matches the source's current barrier — an owner-loss /
    /// removal advances the barrier, so a receipt minted before that loss can never admit
    /// into a vacant or re-owned slot (the vacant-resurrection fence).
    intent_epoch: u64,
    binding: Box<ProjectBinding>,
    companions: Vec<CompanionFingerprint>,
    provider_generation: ProviderGeneration,
}

impl ProviderReadyReceipt {
    /// Mint a readiness receipt from a RESOLVED binding + the advertised companions.
    ///
    /// Requiring the `&ProjectBinding` is the structural gate: a binding can only come
    /// from a `CarrierOwnershipResolution::Bound` produced by the shared resolver, so
    /// readiness can never be represented without a real resolution. `engine` is the
    /// producing engine's identity label. `source_revision` is the source content
    /// revision the companions were produced from.
    ///
    /// MODULE-PRIVATE: reachable only from the tsserver end-of-`apply_owned` transaction
    /// and from [`PendingProviderReady::confirm_opened`] (the tsgo post-open mint). No
    /// site outside this module can mint a receipt — it either receives one from the
    /// carrier-sync gateway's tsserver `Published` arm or confirms a pending after its
    /// companion opens.
    fn mint(
        binding: &ProjectBinding,
        source_revision: u64,
        intent_epoch: u64,
        engine: impl Into<Arc<str>>,
        companions: &[CarrierCompanion],
    ) -> Self {
        let project_generation = binding.ownership_generation();
        let fingerprints = companions
            .iter()
            .map(|c| CompanionFingerprint {
                uri: Arc::clone(&c.provider_uri),
                role: c.role,
                content_hash: hash16_of_str(&c.content),
                map_hash: c
                    .map_json
                    .as_deref()
                    .map(hash16_of_str)
                    .unwrap_or([0u8; 16]),
                version: c.version,
            })
            .collect();
        Self {
            source_revision,
            project_generation,
            intent_epoch,
            provider_generation: ProviderGeneration {
                engine: engine.into(),
                generation: project_generation,
            },
            binding: Box::new(binding.clone()),
            companions: fingerprints,
        }
    }

    /// The source content revision the readiness attests.
    #[must_use]
    pub fn source_revision(&self) -> u64 {
        self.source_revision
    }

    /// The ownership/project generation the readiness was minted at.
    #[must_use]
    pub fn project_generation(&self) -> SnapshotGeneration {
        self.project_generation
    }

    /// The per-source local intent epoch captured when the transaction began — the
    /// coordinator's owner-loss barrier value the admission gate revalidates against the
    /// source's CURRENT barrier before committing.
    #[must_use]
    pub fn intent_epoch(&self) -> u64 {
        self.intent_epoch
    }

    /// Stamp the transaction's captured local intent epoch onto a freshly-minted tsserver
    /// receipt. The reconciler mints with a placeholder `0`; the carrier-sync gateway — the
    /// sole holder of the epoch captured at transaction start — calls this before returning
    /// the owned decision, so the token the admission gate validates carries the coherent
    /// captured epoch. Consumes and returns `self` (the token stays a single owned value).
    pub(crate) fn stamped_with_intent_epoch(mut self, intent_epoch: u64) -> Self {
        self.intent_epoch = intent_epoch;
        self
    }

    /// The resolved owning-project binding the readiness attests.
    #[must_use]
    pub fn binding(&self) -> &ProjectBinding {
        self.binding.as_ref()
    }

    /// The versioned fingerprints of the advertised companions.
    #[must_use]
    pub fn companions(&self) -> &[CompanionFingerprint] {
        &self.companions
    }

    /// The opaque provider-generation epoch (engine identity + generation).
    #[must_use]
    pub fn provider_generation(&self) -> &ProviderGeneration {
        &self.provider_generation
    }

    /// A receipt for tests that seed carrier provider state directly (bypassing a live
    /// reconcile). Test-only: production receipts are minted from a resolved binding.
    #[cfg(test)]
    pub(crate) fn for_test(binding: &ProjectBinding) -> Self {
        Self::mint(binding, 0, 0, "test", &[])
    }
}

/// A sealed, POST-OPEN authorization to mint a [`ProviderReadyReceipt`] for a tsgo
/// direct-open carrier.
///
/// The tsgo path has no store transaction to anchor the mint to: the carrier reaches
/// the provider as directly-opened companion buffers. Minting the receipt in the
/// carrier-sync gateway (BEFORE the site opens those buffers) would make it an
/// observation of a not-yet-open surface, not a fence. Instead the gateway resolves the
/// binding + companions and hands back this pending; the site opens the companion
/// buffers and, ONLY on success, calls [`Self::confirm_opened`] to mint the receipt —
/// so a tsgo receipt never precedes its buffer opens (mirroring the tsserver mint at the
/// END of the ordered `apply_owned` transaction).
///
/// Its fields are private and it is `#[must_use]`: an unconfirmed pending mints nothing,
/// so an open failure that early-returns before [`Self::confirm_opened`] drops the
/// pending and commits no state — the fail-closed default.
#[derive(Debug)]
#[must_use = "a PendingProviderReady mints no receipt until confirm_opened is called after the companion buffers open"]
pub struct PendingProviderReady {
    binding: Box<ProjectBinding>,
    source_revision: u64,
    /// The per-source local intent epoch captured when the tsgo transaction began (see
    /// [`ProviderReadyReceipt::intent_epoch`]). Carried onto the minted receipt so a pending
    /// confirmed AFTER an intervening owner-loss is refused by the admission gate.
    intent_epoch: u64,
    engine: Arc<str>,
    companions: Vec<CarrierCompanion>,
}

impl PendingProviderReady {
    /// Authorize a POST-OPEN receipt mint from a RESOLVED binding + the advertised
    /// companions. Requires the `&ProjectBinding` — the same structural gate as the
    /// module-private [`ProviderReadyReceipt::mint`] — but mints NOTHING yet: the
    /// receipt is minted only when the site calls [`Self::confirm_opened`] after opening
    /// the companion buffers. `intent_epoch` is the source's owner-loss barrier value
    /// captured at transaction start (the gateway reads it before the direct open).
    pub(crate) fn authorize(
        binding: &ProjectBinding,
        source_revision: u64,
        intent_epoch: u64,
        engine: impl Into<Arc<str>>,
        companions: &[CarrierCompanion],
    ) -> Self {
        Self {
            binding: Box::new(binding.clone()),
            source_revision,
            intent_epoch,
            engine: engine.into(),
            companions: companions.to_vec(),
        }
    }

    /// Mint the readiness receipt AFTER the site's companion buffers have opened,
    /// attesting EXACTLY the companion kinds that ACTUALLY opened this pass
    /// (`opened_kinds`). This is the SOLE tsgo mint path, keeping
    /// [`ProviderReadyReceipt::mint`] module-private and unreachable before the direct
    /// open. Consumes `self`, so a pending mints at most once.
    ///
    /// A tsgo direct open is PER-KIND and can partially fail (the IDE buffer opens while
    /// the API buffer does not, or vice-versa). The receipt must attest only the opened
    /// subset: a partial open that stamped the (unopened) new companion surface would let
    /// the gate install a surface identity the provider never actually serves — and reject
    /// the still-live prior surface. So a companion is attested IFF its kind opened
    /// (`Ide` ⇒ `CarrierIde`, `Api` ⇒ `CarrierApi`); the unopened companions are dropped.
    pub(crate) fn confirm_opened(
        self,
        opened_kinds: &[crate::provider_sync::ProviderPathKind],
    ) -> ProviderReadyReceipt {
        self.confirm_opened_with_ide_surface(opened_kinds, None)
    }

    /// Mint the post-open receipt using the exact successfully delivered IDE
    /// surface when the provider specialized it. The evidence value is sealed
    /// by `ProjectSync`; callers cannot fabricate it before a successful open.
    /// A path-mismatched value never substitutes another companion.
    pub(crate) fn confirm_opened_with_ide_surface(
        self,
        opened_kinds: &[crate::provider_sync::ProviderPathKind],
        ide_surface: Option<crate::type_provider::project_sync::SyncedTsxSurface>,
    ) -> ProviderReadyReceipt {
        use crate::provider_sync::ProviderPathKind;
        let attests = |role: SnapshotRole| {
            opened_kinds.iter().any(|kind| {
                matches!(
                    (kind, role),
                    (ProviderPathKind::Ide, SnapshotRole::CarrierIde)
                        | (ProviderPathKind::Api, SnapshotRole::CarrierApi)
                )
            })
        };
        let mut attested: Vec<CarrierCompanion> = self
            .companions
            .into_iter()
            .filter(|companion| attests(companion.role))
            .collect();
        if let Some(surface) = ide_surface {
            if let Some(companion) = attested.iter_mut().find(|companion| {
                companion.role == SnapshotRole::CarrierIde
                    && companion.provider_uri.as_ref() == surface.path()
            }) {
                companion.content = Arc::clone(surface.content());
            }
        }
        ProviderReadyReceipt::mint(
            &self.binding,
            self.source_revision,
            self.intent_epoch,
            self.engine,
            &attested,
        )
    }
}

/// BLAKE3 → 16-byte content hash (the same identity the store + provider-surface stamp
/// use), for the companion fingerprints recorded on a [`ProviderReadyReceipt`].
fn hash16_of_str(s: &str) -> Hash16 {
    let digest = blake3::hash(s.as_bytes());
    let mut h = [0u8; 16];
    h.copy_from_slice(&digest.as_bytes()[..16]);
    h
}

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
/// resolution happens once in the caller through the shared
/// `WorkspaceProjectResolver`).
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
        /// The readiness receipt minted at the END of the ordered transaction — the
        /// capability token gating the carrier's provider-ready commit.
        receipt: ProviderReadyReceipt,
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
    /// Local provider actor for a Verter-managed engine. `None` when the editor's
    /// tsserver plugin reads the durable membership store directly.
    provider: Option<Arc<dyn TypeProvider>>,
    committer: Arc<dyn CarrierMembershipCommitter>,
    /// Per-source serialization gates for membership transitions.
    ///
    /// A membership transition (`apply_owned` / `apply_absent`) reads the prior ledger
    /// record, derives the stale companion set, commits the durable store, re-points
    /// the provider buffers, and swaps the single ledger entry — across several
    /// `.await`s, WITHOUT holding the ledger lock (see `apply_owned`). Two concurrent
    /// transitions for the SAME source would otherwise interleave that read-modify-write
    /// and produce a torn or superseded advertisement (e.g. a stale companion never
    /// closed, or an A→B owner change clobbered by a stale A commit). This map
    /// serializes transitions PER SOURCE while leaving different sources fully
    /// concurrent.
    ///
    /// The map is SHARED (`Arc`): the production reconciler is rebuilt per transition
    /// from the coordinator's fields, so the coordinator owns the one map and hands the
    /// same `Arc` to every rebuilt reconciler — otherwise each per-call reconciler would
    /// get a fresh (useless) map. Entries are never removed (bounded by the workspace's
    /// carrier count), which also avoids the keyed-lock cleanup race (a removed-then-
    /// reinserted gate would fail to serialize against an in-flight holder).
    source_gates: Arc<DashMap<String, Arc<AsyncMutex<()>>>>,
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
        source_gates: Arc<DashMap<String, Arc<AsyncMutex<()>>>>,
    ) -> Self {
        Self {
            ledger,
            provider: Some(provider),
            committer,
            source_gates,
        }
    }

    /// Build a durable store-only reconciler for an editor-owned tsserver plugin.
    /// No local provider exists on this route; membership publication and ledger
    /// admission remain the same authoritative transaction.
    #[must_use]
    pub fn new_store_only(
        ledger: Arc<MembershipLedger>,
        committer: Arc<dyn CarrierMembershipCommitter>,
        source_gates: Arc<DashMap<String, Arc<AsyncMutex<()>>>>,
    ) -> Self {
        Self {
            ledger,
            provider: None,
            committer,
            source_gates,
        }
    }

    /// The per-source serialization gate for `source` (created on first use). The
    /// returned `Arc<AsyncMutex>` is independent of the `DashMap` shard lock — the
    /// `entry` guard is dropped before this returns, so the caller `.lock().await`s
    /// WITHOUT holding a shard lock across the await. See [`Self::source_gates`].
    fn source_gate(&self, source: &str) -> Arc<AsyncMutex<()>> {
        Arc::clone(
            self.source_gates
                .entry(source.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .value(),
        )
    }

    /// The shared membership ledger (the source-indexed advertisement authority).
    #[must_use]
    pub fn ledger(&self) -> &Arc<MembershipLedger> {
        &self.ledger
    }

    /// Reconcile a source's membership to the ONE captured carrier-ownership
    /// resolution.
    ///
    /// The caller resolves the [`CarrierOwnershipResolution`] EXACTLY ONCE (through the
    /// shared `WorkspaceProjectResolver`) and hands it here — the reconciler never
    /// re-resolves. The four states map directly onto the membership outcome:
    /// `Bound` advertises (and mints the readiness receipt), `NoProject` / `Ambiguous`
    /// tombstone (fail closed, never served), and `NotReady` defers without thrash. A
    /// caller-authoritative terminal `reason` (deleted / compile-failed /
    /// conflict-removed) short-circuits to a tombstone without consulting ownership.
    ///
    /// Returns `Ok` ONLY if the desired state was reached (provider staged + ledger
    /// verified). A non-authoritative (`NotReady`) resolution returns `Ok(Deferred)` —
    /// deferred, not advertised, not clean success.
    pub async fn reconcile_source_membership(
        &self,
        source: &CanonicalSource,
        resolution: CarrierOwnershipResolution,
        companions: Vec<CarrierCompanion>,
        reason: ReconcileReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        // Serialize this source's membership transition against any concurrent one for
        // the SAME source: the transition below reads the prior ledger record and swaps
        // it across `.await`s without holding the ledger lock, so an unserialized
        // concurrent peer could interleave into a torn/superseded advertisement. Held
        // across the whole transition; different sources take different gates.
        let gate = self.source_gate(source.as_str());
        let _gate = gate.lock().await;

        // A caller-authoritative terminal reason is an absent outcome on its own —
        // a deleted / compile-failed / conflict-removed source has no owner to
        // resolve.
        if let Some(absent) = reason.terminal_absent_reason() {
            return self.apply_absent(source, absent).await;
        }

        // Route the ONE captured resolution directly onto the membership outcome.
        match resolution {
            CarrierOwnershipResolution::Bound(binding) => {
                self.apply_owned(source, binding, companions).await
            }
            CarrierOwnershipResolution::NoProject => {
                self.apply_absent(source, AbsentReason::NoProject).await
            }
            CarrierOwnershipResolution::Ambiguous { .. } => {
                self.apply_absent(source, AbsentReason::Ambiguous).await
            }
            CarrierOwnershipResolution::NotReady => {
                // Defer WITHOUT thrash: no provider transition, no ledger mutation,
                // and NOT reported as clean success.
                let gen = self.ledger.current_session();
                Ok(ReconcileOutcome::Deferred {
                    bootstrap: BootstrapState {
                        gen,
                        kind: BootstrapKind::OwnershipPending,
                    },
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
        // Same per-source serialization as `reconcile_source_membership` — a removal is
        // a membership transition (store retract + provider close + ledger tombstone)
        // and must not interleave with a concurrent publish/removal of the same source.
        let gate = self.source_gate(source.as_str());
        let _gate = gate.lock().await;
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
        // ledger lock is held across these awaits.
        //
        // Ordering splits by transition kind:
        //
        // - **Extension flip** (a stale companion and a NEW companion share the
        //   same path stem — e.g. `.tsx` → `.jsx` when a carrier's script kind is
        //   corrected): the stale path must be CLOSED BEFORE the new one
        //   registers. tsserver's wildcard membership check excludes a newly
        //   opened `.jsx` while a same-stem `.tsx` is still a program member
        //   ("Detected output file" — the `.jsx` looks like the `.tsx`'s emit),
        //   so register-first strands the new companion in an inferred project
        //   and every `projectFileName`-targeted query fails closed.
        // - **Owner change** (stale companions under a different project, no
        //   same-stem new companion): register first (so the new project's
        //   buffer is live before the old one drops), then close the stale ones.
        fn stem_of(path: &str) -> &str {
            // The path without its final extension: `Comp.vue.tsx` → `Comp.vue`.
            // Same-stem paths are the SAME carrier's companion before/after an
            // extension flip; different stems are unrelated buffers.
            match path.rsplit_once('.') {
                Some((stem, _)) => stem,
                None => path,
            }
        }
        let new_stems: HashSet<&str> = companions
            .iter()
            .map(|companion| stem_of(companion.provider_uri.as_ref()))
            .collect();
        let (flip_stale, owner_stale): (Vec<Arc<str>>, Vec<Arc<str>>) = stale
            .iter()
            .cloned()
            .partition(|path| new_stems.contains(stem_of(path.as_ref())));
        if let Some(provider) = &self.provider {
            for stale_path in &flip_stale {
                provider.close_file(stale_path).await.map_err(|err| {
                    ReconcileErr::ProviderTransition {
                        desired: desired.clone(),
                        detail: err.to_string(),
                    }
                })?;
            }
            for companion in &companions {
                provider
                    .register_carrier_member(
                        source.as_str(),
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
            for stale_path in &owner_stale {
                provider.close_file(stale_path).await.map_err(|err| {
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
                let _ = provider
                    .notify_carrier_changed(&companion.provider_uri)
                    .await;
            }
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

        // The ordered transaction is complete (store committed → provider staged →
        // ledger committed + verified). ONLY NOW mint the readiness receipt from the
        // resolved binding — the capability token that gates the carrier's
        // provider-ready commit. Representing readiness earlier is impossible: the
        // mint requires this resolved `binding`, and no transaction reaches here
        // without every prior step succeeding.
        // Mint with a placeholder intent epoch of 0; the carrier-sync gateway (the sole
        // caller that captured the transaction's owner-loss barrier value) stamps the real
        // epoch onto the receipt via `stamped_with_intent_epoch` before it returns the
        // owned decision, so the receipt the admission gate validates carries the coherent
        // captured epoch. The reconciler stays engine-agnostic and epoch-free.
        let source_revision = companions.iter().map(|c| c.version).max().unwrap_or(0);
        let receipt =
            ProviderReadyReceipt::mint(&binding, source_revision, 0, "tsserver", &companions);

        Ok(ReconcileOutcome::Advertised {
            project,
            companions: companions.len(),
            gen,
            replaced,
            receipt,
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
        if let Some(provider) = &self.provider {
            if let Some(MembershipRecord::Advertised { companions, .. }) = &prior {
                for companion in companions {
                    provider
                        .close_file(&companion.provider_uri)
                        .await
                        .map_err(|err| ReconcileErr::ProviderTransition {
                            desired: desired.clone(),
                            detail: err.to_string(),
                        })?;
                }
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
