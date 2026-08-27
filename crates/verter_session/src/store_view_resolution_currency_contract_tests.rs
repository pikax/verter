#![doc = include_str!("../../../docs/arch/path-precise-resolution-currency.md")]

use std::sync::Arc;

use crate::host_test_audit::ResolutionCurrencyObservation;
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

const OWNER: &str = "/p/main.ts";
const BASE: &str = "/p/base.ts";
const OVERRIDE_TARGET: &str = "/p/override.ts";
const SPECIFIER: &str = "./base";

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {id} must succeed: {error:?}"));
}

fn exact_fixture() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, BASE, "export const value = 'base'\n");
    upsert_ts(&host, OVERRIDE_TARGET, "export const value = 'override'\n");
    upsert_ts(
        &host,
        OWNER,
        "import { value } from './base'\nexport { value }\n",
    );
    let _owner_artifact = host
        .ensure_indexed_ready_serve(OWNER)
        .expect("precondition: the unchanged owner is already materialised");
    let baseline = host.resolve_import_transient(OWNER, SPECIFIER);
    if baseline.as_deref() != Some(BASE) {
        panic!("precondition: the ordinary positive must resolve to {BASE}; got {baseline:?}");
    }
    host
}

fn set_exact_target(host: &VerterHost, target: Option<&str>) {
    let resolutions = target
        .map(|target| {
            vec![verter_workspace::ExactResolution {
                specifier: SPECIFIER.to_string(),
                phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                kind: verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some(target.to_string()),
                possible_canonical_ids: vec![target.to_string()],
            }]
        })
        .unwrap_or_default();
    host.set_exact_resolutions(OWNER, resolutions);
}

#[derive(Debug, Default, PartialEq, Eq)]
struct OwnerEdgeTrace {
    exact_witness_rejections: usize,
    exact_witness_acceptances: usize,
    rejected_exact_witness_targets: Vec<Option<String>>,
    recomputed_targets: Vec<Option<String>>,
    published_targets: Vec<Option<String>>,
    reused_targets: Vec<Option<String>>,
}

fn owner_edge_trace(observations: Vec<ResolutionCurrencyObservation>) -> OwnerEdgeTrace {
    let mut trace = OwnerEdgeTrace::default();
    for observation in observations {
        match observation {
            ResolutionCurrencyObservation::ExactWitnessValidation {
                owner,
                specifier,
                target,
                accepted,
            } if owner == OWNER && specifier == SPECIFIER => {
                if accepted {
                    trace.exact_witness_acceptances += 1;
                } else {
                    trace.exact_witness_rejections += 1;
                    trace.rejected_exact_witness_targets.push(target);
                }
            }
            ResolutionCurrencyObservation::OwnerEdgeRecomputed {
                owner,
                specifier,
                target,
            } if owner == OWNER && specifier == SPECIFIER => {
                trace.recomputed_targets.push(target);
            }
            ResolutionCurrencyObservation::OwnerEdgePublished {
                owner,
                specifier,
                target,
            } if owner == OWNER && specifier == SPECIFIER => {
                trace.published_targets.push(target);
            }
            ResolutionCurrencyObservation::OwnerEdgeReused {
                owner,
                specifier,
                target,
            } if owner == OWNER && specifier == SPECIFIER => {
                trace.reused_targets.push(target);
            }
            _ => {}
        }
    }
    trace
}

fn target(target: &str) -> Option<String> {
    Some(target.to_string())
}

#[test]
fn resolution_currency_exact_change_rejects_the_old_witness() {
    let host = exact_fixture();
    host.begin_resolution_currency_observation();
    set_exact_target(&host, Some(OVERRIDE_TARGET));

    let resolved = host.resolve_import_transient(OWNER, SPECIFIER);
    let trace = owner_edge_trace(host.take_resolution_currency_observations());

    assert_eq!(
        (
            resolved.as_deref(),
            trace.exact_witness_rejections,
            trace.exact_witness_acceptances,
        ),
        (Some(OVERRIDE_TARGET), 1, 0),
        "the changed exact fact must reject the old witness before the demand \
         returns the override target"
    );
}

#[test]
fn resolution_currency_exact_change_store_view_performs_zero_routing_work() {
    let host = exact_fixture();
    set_exact_target(&host, Some(OVERRIDE_TARGET));
    host.begin_resolution_currency_observation();

    let _view = host.resolver_store_view_read().into_owned_view();
    let observations = host.take_resolution_currency_observations();

    assert_eq!(
        observations,
        Vec::<ResolutionCurrencyObservation>::new(),
        "StoreView capture must emit no witness-validation, owner-edge \
         recomputation, publication, or reuse observation"
    );
}

#[test]
fn resolution_currency_exact_change_next_demand_recomputes_and_publishes_once() {
    let host = exact_fixture();
    set_exact_target(&host, Some(OVERRIDE_TARGET));
    host.begin_resolution_currency_observation();
    let _view = host.resolver_store_view_read().into_owned_view();
    let _store_view_work = host.take_resolution_currency_observations();

    let resolved = host.resolve_import_transient(OWNER, SPECIFIER);
    let trace = owner_edge_trace(host.take_resolution_currency_observations());

    assert_eq!(
        (
            resolved.as_deref(),
            trace.recomputed_targets,
            trace.published_targets,
            trace.reused_targets,
        ),
        (
            Some(OVERRIDE_TARGET),
            vec![target(OVERRIDE_TARGET)],
            vec![target(OVERRIDE_TARGET)],
            Vec::new(),
        ),
        "the first real demand after the exact-fact change must perform one \
         owner-edge recomputation and publish that result exactly once"
    );
}

#[test]
fn resolution_currency_exact_change_subsequent_demand_is_a_warm_hit() {
    let host = exact_fixture();
    set_exact_target(&host, Some(OVERRIDE_TARGET));
    host.begin_resolution_currency_observation();
    let _view = host.resolver_store_view_read().into_owned_view();
    let _store_view_work = host.take_resolution_currency_observations();
    let _first_demand = host.resolve_import_transient(OWNER, SPECIFIER);
    let _first_demand_work = host.take_resolution_currency_observations();

    let resolved = host.resolve_import_transient(OWNER, SPECIFIER);
    let trace = owner_edge_trace(host.take_resolution_currency_observations());

    assert_eq!(
        (
            resolved.as_deref(),
            trace.recomputed_targets,
            trace.published_targets,
            trace.reused_targets,
        ),
        (
            Some(OVERRIDE_TARGET),
            Vec::new(),
            Vec::new(),
            vec![target(OVERRIDE_TARGET)],
        ),
        "the demand after publication must reuse the owner-edge candidate \
         without recomputation or another publication"
    );
}

#[test]
fn resolution_currency_exact_fact_aba_cannot_revive_the_original_candidate() {
    let host = exact_fixture();
    host.begin_resolution_currency_observation();
    set_exact_target(&host, Some(OVERRIDE_TARGET));
    let middle = host.resolve_import_transient(OWNER, SPECIFIER);
    let _middle_observations = host.take_resolution_currency_observations();

    set_exact_target(&host, None);
    let final_result = host.resolve_import_transient(OWNER, SPECIFIER);
    let trace = owner_edge_trace(host.take_resolution_currency_observations());

    assert_eq!(
        (middle.as_deref(), final_result.as_deref()),
        (Some(OVERRIDE_TARGET), Some(BASE)),
        "precondition: the ABA sequence must return the override target in the \
         middle and the base target on the final leg"
    );
    assert!(
        trace
            .rejected_exact_witness_targets
            .iter()
            .any(|target| target.as_deref() == Some(BASE)),
        "after exact miss → exact hit → exact miss, the final leg must identify \
         and reject the original base candidate's exact-fact witness"
    );
    assert_eq!(
        (
            trace.exact_witness_acceptances,
            trace.recomputed_targets,
            trace.published_targets,
            trace.reused_targets,
        ),
        (0, vec![target(BASE)], vec![target(BASE)], Vec::new(),),
        "after rejecting the original base candidate, the final demand must \
         recompute and publish a fresh base result with zero candidate reuse"
    );
}
