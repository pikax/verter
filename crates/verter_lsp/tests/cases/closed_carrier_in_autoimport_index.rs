//! Architecture guard (eager API index): a CLOSED `.vue`/`.svelte` component's
//! public-API surface (`CarrierApi`) is in the auto-import / find-references index
//! via the EAGER index — even though no editor buffer is open for it. The eager
//! index force-materializes the lightweight `CarrierApi` surface for EVERY
//! project-owned carrier source up front, IRRESPECTIVE of open state; full IDE TSX
//! (`CarrierIde`) stays LAZY for open/queried carriers. Framework-agnostic (Vue AND
//! Svelte).
//!
//! This is the project-bound external-TS-engine guard
//! `closed_carrier_in_autoimport_index`. It exercises the production eager-index
//! plan ([`verter_lsp::external_ts_sync::EagerApiIndexPlan`]). Discriminating
//! self-checks: the plan is fed sources explicitly tagged `OpenState::Closed` and
//! must index every one of them — a plan (or a production caller) that pre-filtered
//! to OPEN carriers would yield zero entries and fail; a plan that force-
//! materialized `CarrierIde` (not `CarrierApi`) would fail the role assertion.

use std::sync::Arc;

use verter_lsp::external_ts_sync::EagerApiIndexPlan;
use verter_session::external_ts::{OpenState, SnapshotRole};

const TSCONFIG: &str = "/proj/tsconfig.json";

/// A typed owned-carrier record: the source URI and its editor open state. The
/// eager index must cover every owned source REGARDLESS of `open_state`.
struct OwnedCarrier {
    source_uri: Arc<str>,
    open_state: OpenState,
}

/// A descriptor-style CarrierApi companion-path resolver (the `.verter.ts`
/// redirect-reached identity). `None` for a source with no companion.
fn api_companion(source: &str) -> Option<Arc<str>> {
    if source.ends_with(".vue") || source.ends_with(".svelte") {
        Some(Arc::from(format!("{source}.verter.ts").as_str()))
    } else {
        None
    }
}

/// Build the eager index from a TYPED owned-carrier enumeration, asserting up front
/// that the plan does NOT consult `open_state` (it indexes every owned source). The
/// helper passes ALL owned sources through — a production caller that pre-filtered
/// to `OpenState::Open` would be exactly the failure mode the closed-source asserts
/// below catch.
fn plan_over(owned: &[OwnedCarrier]) -> EagerApiIndexPlan {
    EagerApiIndexPlan::for_owned_sources(
        TSCONFIG,
        owned.iter().map(|c| Arc::clone(&c.source_uri)),
        api_companion,
    )
}

#[test]
fn closed_vue_and_svelte_components_are_in_the_index() {
    // Two owned carrier sources, BOTH explicitly CLOSED.
    let owned = [
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/Button.vue"),
            open_state: OpenState::Closed,
        },
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/Card.svelte"),
            open_state: OpenState::Closed,
        },
    ];
    // Sanity: the fixture models genuinely-closed carriers.
    assert!(owned.iter().all(|c| c.open_state == OpenState::Closed));

    let plan = plan_over(&owned);
    assert_eq!(
        plan.api_companions().len(),
        2,
        "every owned carrier source contributes its CarrierApi surface to the eager index"
    );
    assert!(
        plan.contains_api_for("/proj/src/Button.vue"),
        "a CLOSED .vue component's API surface is in the auto-import/find-refs index"
    );
    assert!(
        plan.contains_api_for("/proj/src/Card.svelte"),
        "framework parity: a CLOSED .svelte component's API surface is in the index"
    );
}

#[test]
fn closed_carriers_indexed_even_when_mixed_with_open() {
    // A mix of OPEN and CLOSED owned carriers: ALL are indexed (open state is not a
    // filter). A pre-filter-to-open production path would drop the closed ones.
    let owned = [
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/Open.vue"),
            open_state: OpenState::Open,
        },
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/Closed1.vue"),
            open_state: OpenState::Closed,
        },
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/Closed2.svelte"),
            open_state: OpenState::Closed,
        },
    ];
    let plan = plan_over(&owned);
    assert_eq!(
        plan.api_companions().len(),
        3,
        "open AND closed carriers are all indexed"
    );
    assert!(plan.contains_api_for("/proj/src/Closed1.vue"));
    assert!(plan.contains_api_for("/proj/src/Closed2.svelte"));
}

#[test]
fn eager_index_force_materializes_only_carrier_api_not_ide() {
    let owned = [OwnedCarrier {
        source_uri: Arc::from("/proj/src/Button.vue"),
        open_state: OpenState::Closed,
    }];
    let plan = plan_over(&owned);
    assert_eq!(
        plan.api_companions().len(),
        1,
        "non-vacuous: exactly one companion"
    );
    assert!(
        plan.api_companions()
            .iter()
            .all(|c| c.role == SnapshotRole::CarrierApi),
        "the eager index force-materializes ONLY CarrierApi; CarrierIde stays lazy"
    );
    assert!(
        plan.api_companions()
            .iter()
            .all(|c| c.provider_uri.ends_with(".verter.ts")),
        "the eager-indexed companion is the redirect-reached CarrierApi `.verter.ts` identity"
    );
}

#[test]
fn source_with_no_api_companion_is_skipped_fail_closed() {
    let owned = [
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/Button.vue"),
            open_state: OpenState::Closed,
        },
        OwnedCarrier {
            source_uri: Arc::from("/proj/src/plain.ts"), // not a carrier — no companion
            open_state: OpenState::Closed,
        },
    ];
    let plan = plan_over(&owned);
    assert_eq!(plan.api_companions().len(), 1);
    assert!(!plan.contains_api_for("/proj/src/plain.ts"));
    assert!(plan.contains_api_for("/proj/src/Button.vue"));
}

#[test]
fn index_covers_every_owned_source_not_a_filtered_subset() {
    let owned: Vec<OwnedCarrier> = (0..5)
        .map(|i| OwnedCarrier {
            source_uri: Arc::from(format!("/proj/src/Comp{i}.vue").as_str()),
            open_state: OpenState::Closed,
        })
        .collect();
    let plan = plan_over(&owned);
    assert_eq!(
        plan.api_companions().len(),
        5,
        "the eager index covers EVERY owned carrier source (all closed here), not a subset"
    );
    for i in 0..5 {
        assert!(plan.contains_api_for(&format!("/proj/src/Comp{i}.vue")));
    }
}

#[test]
fn empty_project_yields_empty_index() {
    let plan = plan_over(&[]);
    assert!(plan.api_companions().is_empty());
    assert_eq!(plan.project(), TSCONFIG);
}
