//! The source-indexed active-membership ledger — the authority for what the
//! external-TS engine ADVERTISES per carrier source.
//!
//! ## Why a ledger (not "file exists ⇒ advertised")
//!
//! The on-disk [`CarrierPublishStore`](super::carrier_publish_store::CarrierPublishStore)
//! manifest is PROJECT-indexed (`project_uri → owned/ready files`). That shape
//! makes one question expensive to answer authoritatively: "is THIS source still
//! advertised, and under which project?" — answering it from the project manifest
//! means scanning every project. It also conflates "a carrier blob exists on disk"
//! with "this source is advertised": a stale ready file from a previous owner can
//! linger.
//!
//! This ledger is the SOURCE-indexed counterpart: `canonical source → its single
//! active membership record`. It is the authority for advertisement — "file exists"
//! is NOT "advertised"; the ledger is. Owner loss writes a cheap [`MembershipRecord::Tombstone`]
//! (no full-store crawl on the edit path). Because it is source-indexed, an
//! owner CHANGE (A→B) is a single-entry REPLACEMENT, never a publish-then-prune that
//! can leave a stale entry under the old project.
//!
//! ## Session epoch / lease (membership-validity granularity)
//!
//! Every record carries the [`SessionGen`] it was written under (its lease). The
//! ledger holds the CURRENT session generation; a record whose lease is not the
//! current generation is STALE and is never reported as advertised. A session
//! advance ([`MembershipLedger::advance_session`]) therefore invalidates every
//! prior-session advertisement at once — cheaply, without crawling the records — so
//! a stale old-session record can never be treated as advertised.
//!
//! This lease is SESSION/MEMBERSHIP-validity granularity ONLY (which session's
//! advertisement is valid). It is deliberately ORTHOGONAL to per-file CONTENT
//! versioning, which is owned by `verter_type_runtime`'s content-generation gate —
//! the ledger never duplicates that scheme.
//!
//! ## Internal transition bookkeeping (NOT the serve path)
//!
//! This ledger is INTERNAL transition bookkeeping ONLY — the reconciler's own state
//! for a membership transition (`current_session` / `record_snapshot` reads + the
//! commit post-verification), and `commit` is its SOLE writer. It has ZERO production
//! readers of the advertised / serve set. Live `getExternalFiles` is served
//! CROSS-PROCESS from the on-disk STORE `ready_files` (the Node plugin's `index.ts` →
//! `CarrierStoreReader.readyIdeCompanions` → `carrierStore.ts` reading the
//! `carrier_publish_store` manifest), NOT this in-process ledger. The
//! `ledger_is_off_the_serve_path` architecture guard pins that no production serve
//! path reads it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use verter_session::external_ts::{ScriptKind, SnapshotRole};

/// A session epoch / lease at MEMBERSHIP-validity granularity.
///
/// Identifies which session's advertisement is valid. A record's lease is the
/// generation it was written under; the ledger's current generation gates whether a
/// record is still advertised. NOT a per-file content version (that is a separate
/// layer owned by `verter_type_runtime`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionGen(u64);

impl SessionGen {
    /// The first session generation a fresh ledger starts at.
    pub const INITIAL: SessionGen = SessionGen(1);

    /// A session generation from an explicit value.
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// The underlying generation value.
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }

    /// The next session generation (monotonic advance).
    #[must_use]
    fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// A canonical carrier-source identity — the ledger's key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalSource(Arc<str>);

impl CanonicalSource {
    /// A canonical source from any string-like value.
    #[must_use]
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    /// The canonical source path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A cheap (refcount-only) clone of the backing `Arc<str>` — the map key form.
    #[must_use]
    fn arc(&self) -> Arc<str> {
        Arc::clone(&self.0)
    }
}

impl From<&str> for CanonicalSource {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

/// An owning project (tsconfig) URI a source is advertised under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectUri(Arc<str>);

impl ProjectUri {
    /// A project URI from any string-like value.
    #[must_use]
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    /// The project URI as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProjectUri {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

/// Why a source is NOT advertised — the typed, closed set of absent reasons.
///
/// `Absent` is a FIRST-CLASS outcome of the same ownership decision that yields an
/// owned membership; this enum is its reason. It is closed (no catch-all) so a new
/// absent cause is a deliberate, reviewed addition rather than an untyped string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbsentReason {
    /// No owning tsconfig resolved for the source.
    NoProject,
    /// Two configs claim the source, or a carrier-path conflict makes the owner
    /// undecidable.
    Ambiguous,
    /// An untitled buffer / a file outside any tsconfig (scratch) — not production
    /// project semantics.
    SyntheticScratch,
    /// The source was deleted.
    Deleted,
    /// The carrier failed to compile, so no valid surface can be advertised.
    CompileFailed,
    /// A carrier-path / same-stem conflict removed the source from advertisement.
    ConflictRemoved,
}

/// An advertised companion DESCRIPTOR as recorded in the ledger.
///
/// The ledger records WHICH companions are advertised (their provider path + role +
/// script kind) — NOT their carrier content. The content lives in the on-disk store
/// and the provider's position-conversion buffer; the ledger is the membership
/// authority, not a content store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerCompanion {
    /// The companion provider path (e.g. `/proj/src/Comp.vue.tsx`).
    pub provider_uri: Arc<str>,
    /// The companion's contract role (`CarrierIde` / `CarrierApi` / …).
    pub role: SnapshotRole,
    /// The companion's TypeScript script kind.
    pub script_kind: ScriptKind,
}

/// One source's single active membership record.
///
/// Source-indexed: there is exactly ONE record per source, so an owner change is a
/// REPLACEMENT of this single record (nothing can be left behind under the old
/// project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipRecord {
    /// The source is advertised: its companions are members of `project` under the
    /// session generation `lease`.
    Advertised {
        /// The owning project the companions are advertised under.
        project: ProjectUri,
        /// The advertised companion descriptors.
        companions: Vec<LedgerCompanion>,
        /// The session generation this advertisement was written under.
        lease: SessionGen,
    },
    /// Owner loss: a cheap tombstone recording the source is NOT advertised (no
    /// companions retained — owner-loss must not allocate a companion set).
    Tombstone {
        /// Why the source is not advertised.
        reason: AbsentReason,
        /// The session generation this tombstone was written under.
        lease: SessionGen,
    },
}

impl MembershipRecord {
    /// The session generation this record was written under.
    #[must_use]
    pub fn lease(&self) -> SessionGen {
        match self {
            MembershipRecord::Advertised { lease, .. }
            | MembershipRecord::Tombstone { lease, .. } => *lease,
        }
    }

    /// Whether this record is an advertisement (vs a tombstone).
    #[must_use]
    pub fn is_advertised(&self) -> bool {
        matches!(self, MembershipRecord::Advertised { .. })
    }

    /// The advertised companion provider paths, if this is an advertisement.
    #[must_use]
    pub fn advertised_companions(&self) -> Option<&[LedgerCompanion]> {
        match self {
            MembershipRecord::Advertised { companions, .. } => Some(companions),
            MembershipRecord::Tombstone { .. } => None,
        }
    }
}

/// A failed durable ledger commit — the commit did not reach the desired record.
///
/// Returned by [`MembershipLedger::commit`] when the post-commit verification finds
/// the ledger did NOT actually reach the desired record. The reconciler maps it to a
/// fail-closed error rather than reporting success.
#[derive(Debug, Clone)]
pub struct LedgerCommitError {
    /// Human-readable detail of why the commit did not take effect.
    pub detail: String,
}

impl std::fmt::Display for LedgerCommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "membership ledger commit failed: {}", self.detail)
    }
}

impl std::error::Error for LedgerCommitError {}

/// The source-indexed active-membership ledger.
///
/// Records are keyed by canonical source. `commit` is the sole durable mutation and
/// is TRANSACTIONAL: it applies the desired record and then VERIFIES the ledger
/// reached it, returning `Err` if it did not (the read-side fail-closed gate — no
/// success is reported unless the ledger is actually at the desired state).
#[derive(Debug)]
pub struct MembershipLedger {
    inner: Mutex<LedgerInner>,
    /// Test-only-armed fault-injection seam for the post-commit verification path.
    /// ALWAYS present (one byte), but ONLY ever armed by the `#[cfg(test)]`
    /// [`Self::arm_commit_failure`]; production never sets it, so production `commit`
    /// behaviour is unchanged (the verification always runs as a real fail-closed
    /// gate). When armed, the next `commit` SKIPS the apply so the verification
    /// observes a state that did not reach desired and fails closed.
    fail_next_commit: AtomicBool,
}

#[derive(Debug)]
struct LedgerInner {
    records: HashMap<Arc<str>, MembershipRecord>,
    current_session: SessionGen,
}

impl MembershipLedger {
    /// A fresh empty ledger at the given session generation.
    #[must_use]
    pub fn new(session: SessionGen) -> Self {
        Self {
            inner: Mutex::new(LedgerInner {
                records: HashMap::new(),
                current_session: session,
            }),
            fail_next_commit: AtomicBool::new(false),
        }
    }

    /// A fresh empty ledger at [`SessionGen::INITIAL`].
    #[must_use]
    pub fn with_initial_session() -> Self {
        Self::new(SessionGen::INITIAL)
    }

    /// The current session generation (the lease new records are stamped with).
    #[must_use]
    pub fn current_session(&self) -> SessionGen {
        self.inner.lock().current_session
    }

    /// Advance to the next session generation, invalidating every prior-session
    /// advertisement at once (cheap — no record crawl). Returns the new generation.
    pub fn advance_session(&self) -> SessionGen {
        let mut guard = self.inner.lock();
        guard.current_session = guard.current_session.next();
        guard.current_session
    }

    /// A clone of the source's current record, if any.
    #[must_use]
    pub fn record_snapshot(&self, source: &CanonicalSource) -> Option<MembershipRecord> {
        self.inner.lock().records.get(source.as_str()).cloned()
    }

    /// Whether the source is CURRENTLY advertised — an advertisement record whose
    /// lease is the current session generation. A tombstone, a missing record, or a
    /// stale-lease advertisement all report `false`.
    #[must_use]
    pub fn is_advertised(&self, source: &CanonicalSource) -> bool {
        let guard = self.inner.lock();
        matches!(
            guard.records.get(source.as_str()),
            Some(MembershipRecord::Advertised { lease, .. }) if *lease == guard.current_session
        )
    }

    /// Every source CURRENTLY advertised under `project` (advertisement record,
    /// matching project, current-session lease). Used to assert that an owner change
    /// leaves NOTHING under the old project.
    #[must_use]
    pub fn advertised_under(&self, project: &ProjectUri) -> Vec<CanonicalSource> {
        let guard = self.inner.lock();
        let current = guard.current_session;
        guard
            .records
            .iter()
            .filter_map(|(source, record)| match record {
                MembershipRecord::Advertised {
                    project: owner,
                    lease,
                    ..
                } if owner == project && *lease == current => {
                    Some(CanonicalSource(Arc::clone(source)))
                }
                _ => None,
            })
            .collect()
    }

    /// Every carrier-companion provider path CURRENTLY advertised under `project`
    /// (advertisement record, matching project, current-session lease). One source
    /// contributes all its companion provider paths. This is the ledger's INTERNAL
    /// view of the advertised set — consumed by the reconciler's own bookkeeping and
    /// the production-path tests, NOT the production `getExternalFiles` serve path
    /// (which reads the on-disk store `ready_files` cross-process).
    #[must_use]
    pub fn advertised_provider_paths_under(&self, project: &ProjectUri) -> Vec<Arc<str>> {
        let guard = self.inner.lock();
        let current = guard.current_session;
        guard
            .records
            .values()
            .filter_map(|record| match record {
                MembershipRecord::Advertised {
                    project: owner,
                    companions,
                    lease,
                } if owner == project && *lease == current => Some(
                    companions
                        .iter()
                        .map(|companion| Arc::clone(&companion.provider_uri)),
                ),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Durably commit `record` as the source's single membership record, REPLACING
    /// any prior record (the atomic single-entry swap an owner change relies on).
    ///
    /// TRANSACTIONAL / fail-closed: after applying the record, the commit verifies
    /// the ledger reached exactly that record and returns `Err` if it did not. No
    /// caller may treat the membership as reached unless this returns `Ok`.
    pub fn commit(
        &self,
        source: &CanonicalSource,
        record: MembershipRecord,
    ) -> Result<(), LedgerCommitError> {
        let mut guard = self.inner.lock();
        let key = source.arc();

        // Test-only fault: when armed, skip the apply so the verification below
        // observes a state that did not reach desired (exercising the fail-closed
        // gate). Production never arms it, so the apply always runs.
        let skip_apply = self.fail_next_commit.swap(false, Ordering::SeqCst);
        if !skip_apply {
            guard.records.insert(Arc::clone(&key), record.clone());
        }

        // Post-commit verification — the read-side fail-closed gate. `Ok` ONLY if
        // the ledger actually reached the desired record.
        match guard.records.get(&key) {
            Some(stored) if *stored == record => Ok(()),
            _ => Err(LedgerCommitError {
                detail:
                    "post-commit verification failed: ledger did not reach the desired membership record"
                        .to_string(),
            }),
        }
    }

    /// Test-only: arm the NEXT [`Self::commit`] to skip its apply, so the
    /// post-commit verification fails closed. Exercises the "ledger-commit failure
    /// ⇒ Err, never a false Ok" path without a real durable backend.
    #[cfg(test)]
    pub fn arm_commit_failure(&self) {
        self.fail_next_commit.store(true, Ordering::SeqCst);
    }
}
