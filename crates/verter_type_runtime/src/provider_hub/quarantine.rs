//! Crash quarantine for read-only provider queries.
//!
//! Every crash implicates the requests in flight at the death; a request that
//! is implicated at consecutive deaths is quarantined so the hub never replays
//! the killer into the engine it just recovered.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

/// How long an errored query fingerprint stays eligible for crash attribution.
/// A child's death surfaces to in-flight callers as transport errors that can
/// race the crash monitor's wake by a scheduler tick; anything that errored
/// within this narrow window of the crash is treated as having been in flight
/// at the death. Deliberately SHORT: a wide window would implicate every
/// bystander that failed while the engine was down.
pub(super) const QUARANTINE_RECENT_ERROR_TTL: Duration = Duration::from_millis(500);

/// Bound on the recently-errored ring (memory fuse; oldest entries drop first).
pub(super) const QUARANTINE_RECENT_ERROR_CAP: usize = 64;

/// Crash implications before a fingerprint is quarantined.
///
/// Attribution is ambiguous at a single crash: the in-flight set holds the
/// killer AND innocent bystanders (diagnostics pulls, concurrent hovers), and a
/// bystander that merely failed during the crash must be servable after
/// recovery (the real-provider recovery regression pins that contract). A
/// GENUINE killer distinguishes itself by RECURRENCE: replayed after the
/// restart, it is in flight at the next death too. Two consecutive
/// implications quarantine it — breaking the crash-restart loop on the second
/// cycle, well inside the restart budget — while a bystander's strike is
/// erased by its first successful completion.
pub(super) const QUARANTINE_STRIKE_THRESHOLD: u32 = 2;

/// Identity of a read-only provider query, precise enough that quarantining it
/// blocks exactly the request shape that killed the engine and nothing else.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct QueryFingerprint {
    pub(super) method: &'static str,
    pub(super) path: String,
    scope: Option<String>,
    pub(super) offset: u64,
    extra: u64,
}

impl QueryFingerprint {
    pub(super) fn new(method: &'static str, path: &str, offset: u64, extra: u64) -> Self {
        Self {
            method,
            path: path.to_string(),
            scope: None,
            offset,
            extra,
        }
    }

    pub(super) fn in_scope(mut self, scope: &str) -> Self {
        self.scope = Some(scope.to_string());
        self
    }
}

/// Hash auxiliary request payload (trigger characters, resolve data) into the
/// fingerprint's `extra` dimension.
pub(super) fn hash_extra(payload: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    payload.hash(&mut hasher);
    hasher.finish()
}

/// Crash-quarantine bookkeeping.
///
/// Every crash implicates the requests in flight at the death (plus those that
/// errored within [`QUARANTINE_RECENT_ERROR_TTL`] of it — the transport-error
/// vs monitor-wake race). An implication is a STRIKE; at
/// [`QUARANTINE_STRIKE_THRESHOLD`] consecutive strikes the fingerprint is
/// QUARANTINED: never again replayed verbatim into the restarted engine — the
/// identical request killed the engine repeatedly and would burn the restart
/// budget in an infinite crash-restart loop. A quarantined request fails
/// closed (empty result) while the restarted engine keeps serving everything
/// else. A fingerprint's strikes are erased by its first successful
/// completion (bystanders self-heal), and a path's whole attribution clears
/// when its content changes (the same position against new content is a NEW
/// request).
#[derive(Default)]
pub(super) struct QueryWatch {
    /// Fingerprints currently in flight against the live provider (multiset).
    in_flight: HashMap<QueryFingerprint, u32>,
    /// Fingerprints that recently completed with an error, with their
    /// completion instant (crash attribution window).
    recent_errors: Vec<(QueryFingerprint, Instant)>,
    /// Consecutive crash implications per fingerprint (no successful
    /// completion in between).
    strikes: HashMap<QueryFingerprint, u32>,
    /// Fingerprints attributed to repeated engine crashes: never replayed.
    quarantined: HashSet<QueryFingerprint>,
}

impl QueryWatch {
    pub(super) fn begin(&mut self, fp: &QueryFingerprint) {
        *self.in_flight.entry(fp.clone()).or_insert(0) += 1;
    }

    pub(super) fn end(&mut self, fp: &QueryFingerprint, ok: bool) {
        if let Some(count) = self.in_flight.get_mut(fp) {
            if *count > 1 {
                *count -= 1;
            } else {
                self.in_flight.remove(fp);
            }
        }
        if ok {
            // A successful completion proves the request does not kill the
            // engine — erase its crash strikes (bystander self-heal).
            self.strikes.remove(fp);
        } else {
            let now = Instant::now();
            self.recent_errors
                .retain(|(_, at)| now.duration_since(*at) < QUARANTINE_RECENT_ERROR_TTL);
            if self.recent_errors.len() >= QUARANTINE_RECENT_ERROR_CAP {
                self.recent_errors.remove(0);
            }
            self.recent_errors.push((fp.clone(), now));
        }
    }

    pub(super) fn is_quarantined(&self, fp: &QueryFingerprint) -> bool {
        self.quarantined.contains(fp)
    }

    /// Consecutive crash implications currently held for `fp` (0 when none).
    /// Test seam for asserting strike persistence.
    #[cfg(test)]
    pub(super) fn strike_count(&self, fp: &QueryFingerprint) -> u32 {
        self.strikes.get(fp).copied().unwrap_or(0)
    }

    /// Attribute a crash: strike everything in flight now plus everything that
    /// errored within the race window; quarantine repeat offenders.
    pub(super) fn record_crash_implications(&mut self) {
        let now = Instant::now();
        let mut implicated: HashSet<QueryFingerprint> = self.in_flight.keys().cloned().collect();
        for (fp, at) in self.recent_errors.drain(..) {
            if now.duration_since(at) < QUARANTINE_RECENT_ERROR_TTL {
                implicated.insert(fp);
            }
        }
        for fp in implicated {
            let strikes = self.strikes.entry(fp.clone()).or_insert(0);
            *strikes += 1;
            if *strikes >= QUARANTINE_STRIKE_THRESHOLD {
                tracing::warn!(
                    "query {} {}@{} was in flight at {} consecutive engine crashes — \
                     quarantined (fails closed until the file changes)",
                    fp.method,
                    fp.path,
                    fp.offset,
                    strikes,
                );
                self.quarantined.insert(fp);
            }
        }
    }

    /// A content change for `path` invalidates its crash attribution.
    pub(super) fn clear_path(&mut self, path: &str) {
        self.quarantined.retain(|fp| fp.path != path);
        self.strikes.retain(|fp, _| fp.path != path);
        self.recent_errors.retain(|(fp, _)| fp.path != path);
    }
}

/// RAII in-flight registration: completes as ok/err, or — when dropped without
/// completing (caller cancellation racing the engine's death) — conservatively
/// as an error so the crash window still sees it.
pub(super) struct InFlightGuard {
    watch: Arc<StdMutex<QueryWatch>>,
    fp: Option<QueryFingerprint>,
}

impl InFlightGuard {
    pub(super) fn begin(watch: Arc<StdMutex<QueryWatch>>, fp: QueryFingerprint) -> Self {
        watch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .begin(&fp);
        Self {
            watch,
            fp: Some(fp),
        }
    }

    pub(super) fn complete(mut self, ok: bool) {
        if let Some(fp) = self.fp.take() {
            self.watch
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .end(&fp, ok);
        }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if let Some(fp) = self.fp.take() {
            self.watch
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .end(&fp, false);
        }
    }
}
