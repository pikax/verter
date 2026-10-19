//! Long-churn soak measurement: the deterministic retention sample a soak
//! runner reads from a live host, and the two comparisons it is judged by.
//!
//! The host owns the figures (object counts and retention-account bytes,
//! read live with capture off); this module only defines the sample shape
//! and its arithmetic, so it stays a leaf: the runner maps the host's
//! snapshot into a [`SoakCounts`] and never hands this module a host.
//!
//! Every figure has a [`CountRole`]:
//!
//! - [`CountRole::Drains`]: owned by documents, requests or handles, so it
//!   returns to its pre-workload baseline once every document is closed and
//!   every handle released, through the host's close route;
//! - [`CountRole::DrainsAtTeardown`] (not held to the envelope): aggregate account bytes and the
//!   semantic store's own storage, which return to their baseline once the
//!   host itself is dropped (the account is process-wide and outlives it);
//! - [`CountRole::DrainsAtClose`]: returns to its baseline at the close
//!   route like a draining figure, but also carries the charges of amortised
//!   history, so it drifts mid-run and is not held to the envelope;
//! - [`CountRole::Bounded`]: history its owner retires in amortised passes
//!   past a fixed trigger, so it neither returns to its baseline at close nor
//!   stays flat while the trigger is approached; the judge holds it to an
//!   absolute bound at every sample;
//! - [`CountRole::Cumulative`]: a lifetime counter (refusals, the account's
//!   high-water mark), reported and never compared against a baseline.
//!
//! [`envelope_violations`] compares a later sample against the envelope a
//! run's opening window established, figure by figure, with no slack: the
//! envelope is the frozen workload's own maximum, never a tuned threshold.

use serde::{Deserialize, Serialize};

/// What a figure's baseline comparison means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CountRole {
    /// Returns to its baseline once documents and handles are gone.
    Drains,
    /// Returns to its baseline once the host is dropped.
    DrainsAtTeardown,
    /// Returns to its baseline at the close route, not held to the envelope.
    DrainsAtClose,
    /// Amortised history: bounded by its owner's retirement trigger.
    Bounded,
    /// A lifetime counter: reported, never compared.
    Cumulative,
}

macro_rules! soak_counts {
    ($( $(#[$doc:meta])* $field:ident: $role:ident, )*) => {
        /// One retention sample: object counts and retention-account bytes.
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct SoakCounts {
            $( $(#[$doc])* pub $field: u64, )*
        }

        impl SoakCounts {
            /// Every figure with its name and role, in declaration order.
            #[must_use]
            pub fn figures(&self) -> Vec<(&'static str, CountRole, u64)> {
                vec![$( (stringify!($field), CountRole::$role, self.$field), )*]
            }

            /// The figure-wise maximum of `self` and `other`.
            #[must_use]
            pub fn max_with(&self, other: &Self) -> Self {
                Self { $( $field: self.$field.max(other.$field), )* }
            }
        }
    };
}

soak_counts! {
    /// Live artifact versions of the indexed store.
    live_artifacts: Drains,
    /// Superseded artifact versions the store still retains.
    retired_versions: Drains,
    /// Live captured store roots.
    live_roots: Drains,
    /// Live parse-snapshot leases.
    snapshot_leases: Drains,
    /// Live interned semantic-graph nodes.
    semantic_nodes: Drains,
    /// Independently acquired semantic-graph roots (shared clones count once).
    semantic_roots: Drains,
    /// Retained child occurrences between semantic-graph nodes.
    semantic_internal_edges: Drains,
    /// Populated semantic memo slots.
    memo_entries: Drains,
    /// Retained memo candidates, superseded variants included.
    memo_candidates: Drains,
    /// Resident union member views.
    union_views: Drains,
    /// Live shape-cache entries.
    shape_cache_entries: Drains,
    /// Destroyed nodes whose sidecar entries wait for cleanup.
    pending_destroyed_nodes: Drains,
    /// Resolved-import fact entries.
    resolved_import_facts: Drains,
    /// Resolver component-meta states.
    component_meta_states: Drains,
    /// Registered source snapshots.
    registered_sources: Drains,
    /// Overlay resolution slots the workspace holds.
    overlay_resolution_slots: Drains,
    /// Overlay value versions the workspace holds.
    overlay_value_versions: Drains,
    /// Flow-slice graph bundles.
    flow_graphs: Drains,
    /// Flow-slice hash-node entries.
    flow_hash_entries: Drains,
    /// Flow-slice lowered-body entries.
    flow_lowered_entries: Drains,
    /// Complete carrier parses the publication store keeps for adoption.
    carrier_candidates: Drains,
    /// Publication lanes the carrier store retains.
    publication_lanes: Drains,
    /// Semantic-graph `unresolved_reach` entries.
    unresolved_reach: Drains,
    /// Stable-key classes the semantic store's key table holds.
    stable_key_classes: Drains,
    /// Records across the process-wide semantic identity indexes.
    identity_index_records: Drains,
    /// Names in the resident flow-skeleton name indexes.
    skeleton_names: Drains,
    /// Function positions in the resident closure-capture summaries.
    capture_frames: Drains,
    /// Mapper binder fingerprints across every canonical.
    mapper_fingerprints: Drains,
    /// Cached framework-surface DTO entries.
    framework_surface_entries: Drains,
    /// Fallthrough nodes the resolver runtime caches.
    fallthrough_nodes: Drains,
    /// Resolution slots the workspace's own lane holds.
    resolution_slots: Drains,
    /// Resolution candidates retained across those slots.
    resolution_candidates: Drains,
    /// Importers owning at least one workspace-lane resolution slot.
    resolution_owners: Drains,
    /// Live derived decision and owner-set nodes of the resolution roots.
    resolution_derived_nodes: Drains,
    /// Removed decisions whose tombstone is still stored, retired in
    /// amortised passes.
    resolution_retired_decisions: Bounded,
    /// Cold resolutions in flight.
    resolution_flights: Drains,
    /// Retained exact content-transition entries, leased or not, retired in
    /// amortised passes.
    freshness_exact_entries: Bounded,
    /// Retained subtree content-transition entries, retired in amortised
    /// passes.
    freshness_subtree_entries: Bounded,
    /// Exact content-transition entries pinned by a canonical lease.
    freshness_leased_entries: Drains,
    /// Live content-transition view leases.
    freshness_view_leases: Drains,
    /// Content-transition entries queued for retirement.
    freshness_queued_entries: Bounded,
    /// Dependency edges the scheduler's DAG stores.
    scheduler_dep_edges: Drains,
    /// Dependencies the scheduler's DAG holds gated on a blocker.
    scheduler_gated_deps: Drains,
    /// Per-file dependency records of the scheduler's edge index.
    scheduler_file_deps: Drains,
    /// `(owner, generation)` entries of the scheduler's blocker reference
    /// index.
    scheduler_blocker_refs: Drains,
    /// Bytes charged ACTIVE against the aggregate account.
    active_bytes: Drains,
    /// Bytes charged PINNED by live handles and leases: the account's pinned
    /// bytes less the semantic store's own storage.
    content_pinned_bytes: Drains,
    /// Bytes charged RETAINED against the aggregate account, including the
    /// charges of amortised history.
    retained_bytes: DrainsAtClose,
    /// Bytes charged PINNED against the aggregate account.
    pinned_bytes: DrainsAtTeardown,
    /// The semantic graph's and memo's own storage (arena chunks, dedup
    /// tables, bookkeeping), part of the pinned bytes.
    storage_bytes: DrainsAtTeardown,
    /// High-water mark of the account's charged bytes.
    peak_total_bytes: Cumulative,
    /// Reservations refused for pressure.
    refusals_pressure: Cumulative,
    /// Reservations refused as oversized.
    refusals_oversized: Cumulative,
    /// Reservations refused because the active class was exhausted.
    refusals_active: Cumulative,
}

/// One figure that differs from what its comparison requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountViolation {
    /// The figure's name.
    pub figure: String,
    /// The figure it is compared against (baseline or envelope).
    pub expected_at_most: u64,
    /// The figure observed.
    pub observed: u64,
}

/// Which figures a comparison judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainPoint {
    /// After the host's close route: [`CountRole::Drains`] figures.
    Closed,
    /// After the host is dropped: every figure that drains.
    Teardown,
    /// Against an opening-window envelope: the figures that stay flat while
    /// the workload repeats. The store's own storage grows by table doublings
    /// while it fills, so the figures that include it are judged at
    /// teardown and by absolute bound instead.
    Envelope,
}

impl DrainPoint {
    fn judges(self, role: CountRole) -> bool {
        matches!(
            (self, role),
            (_, CountRole::Drains)
                | (
                    DrainPoint::Closed | DrainPoint::Teardown,
                    CountRole::DrainsAtClose
                )
                | (DrainPoint::Teardown, CountRole::DrainsAtTeardown)
        )
    }
}

/// Figures `point` judges whose value in `after` is above `baseline`.
#[must_use]
pub fn drain_violations(
    baseline: &SoakCounts,
    after: &SoakCounts,
    point: DrainPoint,
) -> Vec<CountViolation> {
    baseline
        .figures()
        .into_iter()
        .zip(after.figures())
        .filter(|((_, role, base), (_, _, now))| point.judges(*role) && now > base)
        .map(|((name, _, base), (_, _, now))| CountViolation {
            figure: name.to_string(),
            expected_at_most: base,
            observed: now,
        })
        .collect()
}

/// The figure-wise maximum of `samples`: the envelope an opening window sets.
#[must_use]
pub fn envelope_of<'a>(samples: impl IntoIterator<Item = &'a SoakCounts>) -> SoakCounts {
    samples
        .into_iter()
        .fold(SoakCounts::default(), |envelope, sample| {
            envelope.max_with(sample)
        })
}

/// Every draining figure of `sample` above `envelope`.
#[must_use]
pub fn envelope_violations(envelope: &SoakCounts, sample: &SoakCounts) -> Vec<CountViolation> {
    drain_violations(envelope, sample, DrainPoint::Envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_reports_only_drained_figures_that_grew() {
        let baseline = SoakCounts {
            pinned_bytes: 10,
            refusals_pressure: 0,
            ..SoakCounts::default()
        };
        let after = SoakCounts {
            pinned_bytes: 11,
            refusals_pressure: 5,
            live_artifacts: 0,
            ..SoakCounts::default()
        };
        let violations = drain_violations(&baseline, &after, DrainPoint::Teardown);
        assert_eq!(
            violations,
            vec![CountViolation {
                figure: "pinned_bytes".into(),
                expected_at_most: 10,
                observed: 11,
            }]
        );
        assert!(
            drain_violations(&baseline, &after, DrainPoint::Closed).is_empty(),
            "pinned account bytes are judged at teardown, not at the close route"
        );
    }

    #[test]
    fn closed_route_judges_retained_account_bytes() {
        let baseline = SoakCounts::default();
        let closed = SoakCounts {
            retained_bytes: 7,
            ..SoakCounts::default()
        };
        let violations = drain_violations(&baseline, &closed, DrainPoint::Closed);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].figure, "retained_bytes");
    }

    #[test]
    fn envelope_is_the_figure_wise_maximum() {
        let a = SoakCounts {
            live_artifacts: 3,
            pinned_bytes: 1,
            ..SoakCounts::default()
        };
        let b = SoakCounts {
            live_artifacts: 1,
            pinned_bytes: 7,
            ..SoakCounts::default()
        };
        let envelope = envelope_of([&a, &b]);
        assert_eq!(envelope.live_artifacts, 3);
        assert_eq!(envelope.pinned_bytes, 7);
        assert!(envelope_violations(&envelope, &a).is_empty());
        let grown = SoakCounts {
            live_artifacts: 4,
            ..b
        };
        assert_eq!(
            envelope_violations(&envelope, &grown)[0].figure,
            "live_artifacts"
        );
    }
}
