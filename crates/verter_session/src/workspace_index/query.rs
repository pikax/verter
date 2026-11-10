//! Bounded candidate enumeration over one captured snapshot.
//!
//! A query names its lookups, its finite budgets and optionally the population
//! it wants certified. Work spent on filtered or stale rows counts against the
//! budget. `Complete` is only reported when the walk proved exhaustion, every
//! looked-up key reads a partition inside the declared population, and a current
//! coverage record certifies those partitions.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::sync::Arc;

use imbl::OrdSet;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};

use super::delta::{ObservationOracle, PartialCause};
use super::identity::{ContributionId, PartitionFamily, PartitionId, ScopeId};
use super::records::{lexical_fold, IndexRecord, KeyFamily, PostingKey};
use super::snapshot::{Contribution, CoverageRecord, IndexSnapshot, PostingRef, SnapshotIdentity};

/// Finite budgets of one query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryBudgets {
    /// Posting visits admitted (including filtered or stale ones).
    pub postings: u32,
    /// Records returned.
    pub records: u32,
}

/// What to look up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Exact keys, in the caller's order.
    Keys(Vec<PostingKey>),
    /// An ordered walk of one partition's key family from a text prefix, in the
    /// projection that family posts under: a lexical prefix is folded the way
    /// its postings are. The empty prefix is a broad walk: it consumes the
    /// budget and reports `Partial` unless it proves exhaustion. A family and
    /// version no postings are keyed under is `UnsupportedCoverage`.
    Prefix {
        partition: PartitionFamily,
        family: KeyFamily,
        version: u32,
        prefix: Arc<str>,
    },
}

impl Lookup {
    /// Whether this lookup reads `family`.
    pub(super) fn reads(&self, family: PartitionFamily) -> bool {
        match self {
            Self::Keys(keys) => keys.iter().any(|key| key.partition() == family),
            Self::Prefix { partition, .. } => *partition == family,
        }
    }

    /// Whether any key of the whole lookup, not only the part one page walks,
    /// reads a partition `scope` does not.
    fn reaches_outside(&self, scope: &DeclaredScope) -> bool {
        match self {
            Self::Keys(keys) => keys.iter().any(|key| !scope.reads_family(key.partition())),
            Self::Prefix { partition, .. } => !scope.reads_family(*partition),
        }
    }

    fn encode(&self, encoder: &mut CanonicalEncoder) {
        let mut key_fields =
            |partition: PartitionFamily, family: KeyFamily, version, text: &str| {
                encoder.field_enum_discriminant(3, partition.encoding());
                encoder.field_enum_discriminant(4, key_family_encoding(family));
                encoder.field_u32(5, version);
                encoder.field_str(6, text);
            };
        match self {
            Self::Keys(keys) => {
                for key in keys {
                    key_fields(key.partition(), key.family(), key.version(), key.text());
                }
            }
            Self::Prefix {
                partition,
                family,
                version,
                prefix,
            } => {
                key_fields(*partition, *family, *version, prefix);
                encoder.field_bool(7, true);
            }
        }
    }
}

fn key_family_encoding(family: KeyFamily) -> u32 {
    match family {
        KeyFamily::Exact => 1,
        KeyFamily::Lexical => 2,
        KeyFamily::Forward => 3,
        KeyFamily::Reverse => 4,
    }
}

/// One declared population: its identity and the partitions it reads. Rows of
/// any other partition are outside the answer and are never returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredScope {
    id: ScopeId,
    partitions: BTreeSet<PartitionId>,
}

impl DeclaredScope {
    pub(crate) fn new(id: ScopeId, partitions: BTreeSet<PartitionId>) -> Self {
        Self { id, partitions }
    }

    /// The population's identity.
    #[must_use]
    pub fn id(&self) -> &ScopeId {
        &self.id
    }

    fn reads_family(&self, family: PartitionFamily) -> bool {
        self.partitions
            .iter()
            .any(|partition| partition.family() == family)
    }
}

/// One bounded query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexQuery {
    pub lookup: Lookup,
    pub budgets: QueryBudgets,
    /// The declared population to certify. Without one the walk cannot be
    /// `Complete`: nothing proves it saw everything.
    pub scope: Option<DeclaredScope>,
    /// Resume point from an earlier page of the same query on the same snapshot.
    pub continuation: Option<IndexContinuation>,
}

/// Resume point of one query on one exact snapshot. Any other snapshot, lookup
/// or declared population refuses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexContinuation {
    snapshot: SnapshotIdentity,
    binding: CanonicalDigest,
    /// Which requested key to resume at. A lookup may repeat a key, so the key
    /// alone does not say where the walk stopped.
    position: u32,
    key: PostingKey,
    next: Box<PostingRef>,
}

/// What a partial answer did cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveredScope {
    pub keys_exhausted: u32,
    pub postings_visited: u32,
    /// Locators of the units this answer could not read. Empty unless an
    /// unavailable input is the reason the answer is partial.
    pub unavailable_units: Vec<Arc<str>>,
}

/// How much of the declared population an answer enumerated. Independent of
/// each row's fact quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumerationStatus {
    /// Every requested key was exhausted and a current coverage record
    /// certifies the partitions it reads. An empty page then means no
    /// candidates in that population: never semantic absence, and never
    /// outside it.
    Complete,
    /// Positive candidates only, for this request.
    Partial {
        cause: PartialCause,
        covered: CoveredScope,
        continuation: Option<IndexContinuation>,
    },
    /// Nothing is returned from cancelled work.
    Cancelled,
}

/// Counted work of one query. These are deterministic: exact in tests,
/// unaffected by timing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryWork {
    pub key_lookups: u32,
    pub posting_visits: u32,
    pub records_returned: u32,
}

/// One candidate. Its owner validates it before any semantic answer.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub contribution: Arc<Contribution>,
    pub record_index: u32,
}

impl Candidate {
    /// The record this candidate points at.
    #[must_use]
    pub fn record(&self) -> Option<&IndexRecord> {
        let index = usize::try_from(self.record_index).ok()?;
        self.contribution.records.get(index)
    }

    /// Stable identity of the candidate.
    #[must_use]
    pub fn identity(&self) -> (ContributionId, u32) {
        (self.contribution.id.clone(), self.record_index)
    }
}

/// One page of candidates in canonical order.
#[derive(Debug, Clone)]
pub struct CandidatePage {
    pub candidates: Vec<Candidate>,
    pub status: EnumerationStatus,
    pub work: QueryWork,
    /// The snapshot generation that answered.
    pub generation: u64,
}

impl CandidatePage {
    fn cancelled(generation: u64, work: QueryWork) -> Self {
        Self {
            candidates: Vec::new(),
            status: EnumerationStatus::Cancelled,
            work,
            generation,
        }
    }
}

struct Walk<'a> {
    snapshot: &'a IndexSnapshot,
    budgets: QueryBudgets,
    oracle: Option<&'a dyn ObservationOracle>,
    is_cancelled: &'a dyn Fn() -> bool,
    scope: Option<&'a DeclaredScope>,
    candidates: Vec<Candidate>,
    work: QueryWork,
    keys_exhausted: u32,
    saw_stale: bool,
    /// The requested key being walked.
    position: u32,
}

enum Stop {
    /// A budget ran out before this posting.
    Budget {
        key: PostingKey,
        next: Box<PostingRef>,
    },
    Cancelled,
}

impl Walk<'_> {
    /// `false` for a key that reads outside the declared population. Without a
    /// declared population nothing is outside, and nothing is certified either.
    fn admits_key(&self, partition: PartitionFamily) -> bool {
        self.scope.is_none_or(|scope| scope.reads_family(partition))
    }

    /// Visit one key's postings from `resume` on.
    fn visit_key(
        &mut self,
        key: &PostingKey,
        postings: &OrdSet<PostingRef>,
        resume: Option<&PostingRef>,
    ) -> Result<(), Stop> {
        let lower = resume.map_or(Bound::Unbounded, Bound::Included);
        for posting in postings.range::<_, PostingRef>((lower, Bound::Unbounded)) {
            if (self.is_cancelled)() {
                return Err(Stop::Cancelled);
            }
            let budget_spent = self.work.posting_visits >= self.budgets.postings
                || self.work.records_returned >= self.budgets.records;
            if budget_spent {
                return Err(Stop::Budget {
                    key: key.clone(),
                    next: Box::new(posting.clone()),
                });
            }
            self.work.posting_visits += 1;
            let Some(contribution) = self.snapshot.contribution(&posting.contribution) else {
                continue;
            };
            if !self.admits_contribution(contribution) {
                continue;
            }
            if self.is_stale(contribution) {
                self.saw_stale = true;
                continue;
            }
            self.work.records_returned += 1;
            self.candidates.push(Candidate {
                contribution: Arc::clone(contribution),
                record_index: posting.record_index,
            });
        }
        self.keys_exhausted += 1;
        Ok(())
    }

    /// Another producer's slot of the same family is outside this population.
    fn admits_contribution(&self, contribution: &Contribution) -> bool {
        self.scope
            .is_none_or(|scope| scope.partitions.contains(&contribution.key.partition_id()))
    }

    fn is_stale(&self, contribution: &Contribution) -> bool {
        self.oracle.is_some_and(|oracle| {
            contribution
                .read_set
                .0
                .iter()
                .any(|witness| oracle.current(&witness.key) != Some(witness.fingerprint))
        })
    }

    fn covered(&self) -> CoveredScope {
        CoveredScope {
            keys_exhausted: self.keys_exhausted,
            postings_visited: self.work.posting_visits,
            unavailable_units: Vec::new(),
        }
    }
}

/// What a continuation is bound to besides its snapshot.
fn query_binding(query: &IndexQuery) -> CanonicalDigest {
    let mut encoder = CanonicalEncoder::new("verter.session.workspace_index.continuation.v1");
    match &query.scope {
        Some(scope) => encoder.field_bytes(1, scope.id.digest().as_bytes()),
        None => encoder.field_bool(2, false),
    };
    query.lookup.encode(&mut encoder);
    encoder.digest()
}

/// Answer `query` from one captured snapshot.
///
/// `oracle` revalidates each visited contribution's read set against its
/// owners; pass `None` only for a request explicitly tied to this snapshot's
/// basis. A stale row is never returned and leaves the answer `Partial`, and so
/// does a continuation issued for any other snapshot, lookup or population.
#[must_use]
pub fn query_candidates(
    snapshot: &IndexSnapshot,
    query: &IndexQuery,
    oracle: Option<&dyn ObservationOracle>,
    is_cancelled: &dyn Fn() -> bool,
) -> CandidatePage {
    let generation = snapshot.generation();
    let mut walk = Walk {
        snapshot,
        budgets: query.budgets,
        oracle,
        is_cancelled,
        scope: query.scope.as_ref(),
        candidates: Vec::new(),
        work: QueryWork::default(),
        keys_exhausted: 0,
        saw_stale: false,
        position: 0,
    };
    if let Some(continuation) = &query.continuation {
        if continuation.snapshot != snapshot.identity()
            || continuation.binding != query_binding(query)
        {
            return CandidatePage {
                candidates: Vec::new(),
                status: EnumerationStatus::Partial {
                    cause: PartialCause::Stale,
                    covered: walk.covered(),
                    continuation: None,
                },
                work: walk.work,
                generation,
            };
        }
    }
    if is_cancelled() {
        return CandidatePage::cancelled(generation, walk.work);
    }
    // No producer posts under an unsupported projection, so a walk of it would
    // find nothing and prove nothing: it is unsupported, never complete-empty.
    if let Lookup::Prefix {
        family, version, ..
    } = &query.lookup
    {
        if !family.posts_under(*version) {
            let status = EnumerationStatus::Partial {
                cause: PartialCause::UnsupportedCoverage,
                covered: walk.covered(),
                continuation: None,
            };
            return finish(walk, status, generation);
        }
    }

    let outcome = match &query.lookup {
        Lookup::Keys(keys) => walk_keys(&mut walk, keys, query.continuation.as_ref()),
        Lookup::Prefix {
            partition,
            family,
            version,
            prefix,
        } => walk_prefix(
            &mut walk,
            *partition,
            *family,
            *version,
            prefix,
            query.continuation.as_ref(),
        ),
    };

    match outcome {
        Err(Stop::Cancelled) => CandidatePage::cancelled(generation, walk.work),
        Err(Stop::Budget { key, next }) => {
            let status = EnumerationStatus::Partial {
                cause: PartialCause::BudgetExhausted,
                covered: walk.covered(),
                continuation: Some(IndexContinuation {
                    snapshot: snapshot.identity(),
                    binding: query_binding(query),
                    position: walk.position,
                    key,
                    next,
                }),
            };
            finish(walk, status, generation)
        }
        Ok(()) => {
            let status = exhausted_status(&walk, &query.lookup);
            finish(walk, status, generation)
        }
    }
}

fn finish(walk: Walk<'_>, status: EnumerationStatus, generation: u64) -> CandidatePage {
    CandidatePage {
        candidates: walk.candidates,
        status,
        work: walk.work,
        generation,
    }
}

/// `true` when `record` certifies every partition `lookup` reads.
fn certifies_lookup(record: &CoverageRecord, scope: &DeclaredScope, lookup: &Lookup) -> bool {
    scope
        .partitions
        .iter()
        .filter(|partition| lookup.reads(partition.family()))
        .all(|partition| record.partitions.contains(partition))
}

/// The walk ran out of keys. That alone does not certify a population.
fn exhausted_status(walk: &Walk<'_>, lookup: &Lookup) -> EnumerationStatus {
    let partial = |cause| EnumerationStatus::Partial {
        cause,
        covered: walk.covered(),
        continuation: None,
    };
    if walk.saw_stale {
        return partial(PartialCause::Stale);
    }
    let Some(scope) = walk.scope else {
        return partial(PartialCause::OpenDiscovery);
    };
    // Judged on the whole lookup: a resumed page never walks the keys before
    // its continuation, so what it walked cannot say what the lookup reads.
    if lookup.reaches_outside(scope) {
        return partial(PartialCause::OutsideDemand);
    }
    let Some(record) = walk.snapshot.coverage(&scope.id) else {
        return partial(PartialCause::OpenDiscovery);
    };
    if !certifies_lookup(record, scope, lookup) {
        return partial(PartialCause::OpenDiscovery);
    }
    let witness_current = walk.oracle.is_none_or(|oracle| {
        oracle.current(&record.witness.key) == Some(record.witness.fingerprint)
    });
    if witness_current {
        EnumerationStatus::Complete
    } else {
        partial(PartialCause::Stale)
    }
}

fn walk_keys(
    walk: &mut Walk<'_>,
    keys: &[PostingKey],
    continuation: Option<&IndexContinuation>,
) -> Result<(), Stop> {
    let start = continuation.map_or(0, |resume| {
        usize::try_from(resume.position).unwrap_or(usize::MAX)
    });
    let snapshot = walk.snapshot;
    for (position, key) in keys.iter().enumerate().skip(start) {
        walk.position = u32::try_from(position).unwrap_or(u32::MAX);
        if !walk.admits_key(key.partition()) {
            continue;
        }
        walk.work.key_lookups += 1;
        let resume = continuation
            .filter(|_| position == start)
            .map(|resume| resume.next.as_ref());
        if let Some(postings) = snapshot.postings.get(key) {
            walk.visit_key(key, postings, resume)?;
        } else {
            walk.keys_exhausted += 1;
        }
    }
    Ok(())
}

fn walk_prefix(
    walk: &mut Walk<'_>,
    partition: PartitionFamily,
    family: KeyFamily,
    version: u32,
    prefix: &Arc<str>,
    continuation: Option<&IndexContinuation>,
) -> Result<(), Stop> {
    if !walk.admits_key(partition) {
        return Ok(());
    }
    let folded;
    // Only a supported projection reaches the walk, so a lexical one is the
    // current fold.
    let prefix: &str = if family == KeyFamily::Lexical {
        folded = lexical_fold(prefix);
        &folded
    } else {
        prefix
    };
    let start = continuation.map_or_else(
        || PostingKey::projected(partition, family, version, prefix),
        |resume| resume.key.clone(),
    );
    let snapshot = walk.snapshot;
    for (key, postings) in snapshot.postings.range(start..) {
        let in_family =
            key.partition() == partition && key.family() == family && key.version() == version;
        if !in_family || !key.text().starts_with(prefix) {
            break;
        }
        walk.work.key_lookups += 1;
        let resume = continuation
            .filter(|resume| resume.key == *key)
            .map(|resume| resume.next.as_ref());
        walk.visit_key(key, postings, resume)?;
    }
    Ok(())
}

/// One page of a declared partition's units: the slots its producer wrote,
/// one per unit the partition applies to.
#[derive(Debug, Clone)]
pub struct UnitPage {
    pub units: Vec<Arc<Contribution>>,
    pub status: EnumerationStatus,
    pub work: QueryWork,
    /// The snapshot generation that answered.
    pub generation: u64,
}

/// Enumerate the units of one partition of `scope` from one captured snapshot.
///
/// Each visited slot counts as a posting visit and each returned one as a
/// record, against `budgets`. The page is `Complete` only when the walk
/// exhausted the partition and a current coverage record of `scope` certifies
/// it; an exhausted budget answers `Partial` with no continuation, and a
/// partition outside `scope` answers nothing.
#[must_use]
pub fn query_units(
    snapshot: &IndexSnapshot,
    scope: &DeclaredScope,
    partition: &PartitionId,
    budgets: QueryBudgets,
    is_cancelled: &dyn Fn() -> bool,
) -> UnitPage {
    let generation = snapshot.generation();
    let mut work = QueryWork::default();
    let partial = |cause, work: &QueryWork, keys_exhausted| EnumerationStatus::Partial {
        cause,
        covered: CoveredScope {
            keys_exhausted,
            postings_visited: work.posting_visits,
            unavailable_units: Vec::new(),
        },
        continuation: None,
    };
    let cancelled = |work| UnitPage {
        units: Vec::new(),
        status: EnumerationStatus::Cancelled,
        work,
        generation,
    };
    if is_cancelled() {
        return cancelled(work);
    }
    if !scope.partitions.contains(partition) {
        return UnitPage {
            units: Vec::new(),
            status: partial(PartialCause::OutsideDemand, &work, 0),
            work,
            generation,
        };
    }
    work.key_lookups += 1;
    let mut units = Vec::new();
    for contribution in snapshot.partition_slots(partition) {
        if is_cancelled() {
            return cancelled(work);
        }
        if work.posting_visits >= budgets.postings || work.records_returned >= budgets.records {
            let status = partial(PartialCause::BudgetExhausted, &work, 0);
            return UnitPage {
                units,
                status,
                work,
                generation,
            };
        }
        work.posting_visits += 1;
        work.records_returned += 1;
        units.push(Arc::clone(contribution));
    }
    let certified = snapshot
        .coverage(&scope.id)
        .is_some_and(|record| record.partitions.contains(partition));
    let status = if certified {
        EnumerationStatus::Complete
    } else {
        partial(PartialCause::OpenDiscovery, &work, 1)
    };
    UnitPage {
        units,
        status,
        work,
        generation,
    }
}
