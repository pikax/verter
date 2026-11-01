//! Bounded candidate enumeration over one captured snapshot.
//!
//! A query names its lookups, its finite budgets and optionally the population
//! it wants certified. Work spent on filtered or stale rows counts against the
//! budget, and `Complete` is only reported when the walk proved exhaustion and
//! a current coverage record certifies the declared population.

use std::ops::Bound;
use std::sync::Arc;

use imbl::OrdSet;

use super::delta::{ObservationOracle, PartialCause};
use super::identity::{ContributionId, ScopeId, TableKind};
use super::records::{IndexRecord, KeyFamily, PostingKey};
use super::snapshot::{Contribution, IndexSnapshot, PostingRef};

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
    /// An ordered walk of one table's key family from a text prefix. The empty
    /// prefix is a broad walk: it consumes the budget and reports `Partial`
    /// unless it proves exhaustion.
    Prefix {
        table: TableKind,
        family: KeyFamily,
        version: u32,
        prefix: Arc<str>,
    },
}

/// One bounded query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexQuery {
    pub lookup: Lookup,
    pub budgets: QueryBudgets,
    /// The declared population to certify. Without one the walk cannot be
    /// `Complete`: nothing proves it saw everything.
    pub scope: Option<ScopeId>,
    /// Resume point from an earlier page of the same snapshot.
    pub continuation: Option<IndexContinuation>,
}

/// Exact-snapshot-qualified resume point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexContinuation {
    generation: u64,
    key: PostingKey,
    next: PostingRef,
}

/// What a partial answer did cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveredScope {
    pub keys_exhausted: u32,
    pub postings_visited: u32,
}

/// How much of the declared population an answer enumerated. Independent of
/// each row's fact quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumerationStatus {
    /// Every requested key was exhausted and a current coverage record
    /// certifies the population. An empty page then means no candidates in
    /// that population: never semantic absence, and never outside it.
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
    candidates: Vec<Candidate>,
    work: QueryWork,
    keys_exhausted: u32,
    saw_stale: bool,
}

enum Stop {
    /// A budget ran out before this posting.
    Budget {
        key: PostingKey,
        next: PostingRef,
    },
    Cancelled,
}

impl Walk<'_> {
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
                    next: posting.clone(),
                });
            }
            self.work.posting_visits += 1;
            let Some(contribution) = self.snapshot.contribution(&posting.contribution) else {
                continue;
            };
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

    fn is_stale(&self, contribution: &Contribution) -> bool {
        self.oracle.is_some_and(|oracle| {
            contribution
                .read_set
                .0
                .iter()
                .any(|witness| oracle.current(&witness.key) != Some(witness.fingerprint))
        })
    }
}

/// Answer `query` from one captured snapshot.
///
/// `oracle` revalidates each visited contribution's read set against its
/// owners; pass `None` only for a request explicitly tied to this snapshot's
/// basis. A stale row is never returned and leaves the answer `Partial`.
#[must_use]
pub fn query_candidates(
    snapshot: &IndexSnapshot,
    query: &IndexQuery,
    oracle: Option<&dyn ObservationOracle>,
    is_cancelled: &dyn Fn() -> bool,
) -> CandidatePage {
    let generation = snapshot.generation();
    let covered = |walk: &Walk<'_>| CoveredScope {
        keys_exhausted: walk.keys_exhausted,
        postings_visited: walk.work.posting_visits,
    };
    let mut walk = Walk {
        snapshot,
        budgets: query.budgets,
        oracle,
        is_cancelled,
        candidates: Vec::new(),
        work: QueryWork::default(),
        keys_exhausted: 0,
        saw_stale: false,
    };
    if let Some(continuation) = &query.continuation {
        if continuation.generation != generation {
            return CandidatePage {
                candidates: Vec::new(),
                status: EnumerationStatus::Partial {
                    cause: PartialCause::Stale,
                    covered: covered(&walk),
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

    let outcome = match &query.lookup {
        Lookup::Keys(keys) => walk_keys(&mut walk, keys, query.continuation.as_ref()),
        Lookup::Prefix {
            table,
            family,
            version,
            prefix,
        } => walk_prefix(
            &mut walk,
            *table,
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
                covered: covered(&walk),
                continuation: Some(IndexContinuation {
                    generation,
                    key,
                    next,
                }),
            };
            finish(walk, status, generation)
        }
        Ok(()) => {
            let status = exhausted_status(&walk, query.scope.as_ref(), covered(&walk));
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

/// The walk ran out of keys. That alone does not certify a population.
fn exhausted_status(
    walk: &Walk<'_>,
    scope: Option<&ScopeId>,
    covered: CoveredScope,
) -> EnumerationStatus {
    let partial = |cause| EnumerationStatus::Partial {
        cause,
        covered: covered.clone(),
        continuation: None,
    };
    if walk.saw_stale {
        return partial(PartialCause::Stale);
    }
    if walk.snapshot.is_return_only() {
        return partial(PartialCause::UnavailableInput);
    }
    let Some(record) = scope.and_then(|scope| walk.snapshot.coverage(scope)) else {
        return partial(PartialCause::OpenDiscovery);
    };
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
        keys.iter()
            .position(|key| *key == resume.key)
            .unwrap_or(keys.len())
    });
    let snapshot = walk.snapshot;
    for (position, key) in keys.iter().enumerate().skip(start) {
        walk.work.key_lookups += 1;
        let resume = continuation
            .filter(|_| position == start)
            .map(|resume| &resume.next);
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
    table: TableKind,
    family: KeyFamily,
    version: u32,
    prefix: &Arc<str>,
    continuation: Option<&IndexContinuation>,
) -> Result<(), Stop> {
    let start = continuation.map_or_else(
        || PostingKey {
            table,
            family,
            version,
            text: Arc::clone(prefix),
        },
        |resume| resume.key.clone(),
    );
    let snapshot = walk.snapshot;
    for (key, postings) in snapshot.postings.range(start..) {
        let in_family = key.table == table && key.family == family && key.version == version;
        if !in_family || !key.text.starts_with(prefix.as_ref()) {
            break;
        }
        walk.work.key_lookups += 1;
        let resume = continuation
            .filter(|resume| resume.key == *key)
            .map(|resume| &resume.next);
        walk.visit_key(key, postings, resume)?;
    }
    Ok(())
}
