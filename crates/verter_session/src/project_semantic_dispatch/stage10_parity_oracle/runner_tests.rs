//! Dual-leg parity runner + the oracle test surface.
//!
//! Each case runs TWICE, on a FRESH hermetic host per leg (no cache
//! cross-seeding between legs): once on the production body source
//! ([`BodyLeg::NewLocator`]) and once with the retained prepared-body
//! implementation active ([`BodyLeg::LegacyPreparedBody`]). The two
//! canonical published-surface envelopes must be byte-identical.
//!
//! Counter rails (false-green resistance): the legacy leg must have
//! served at least one prepared-body read through the RAII seam, and the
//! new leg must have served ZERO reads through it.

use std::sync::Arc;

use super::cases_tests::all_cases;
use super::envelope_tests::OracleEnvelope;
use super::{
    legacy_prepared_body_reads, BodyLeg, FixtureFile, LegacyPreparedBodyLegGuard,
    PublishedSurfaceCase, Stage10SurfaceClass,
};
use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

/// Fresh hermetic host with the case's fixture files mounted.
fn fresh_project(files: &[FixtureFile]) -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    let project = MetaProject::new(host);
    for file in files {
        project
            .upsert_base(file.path, file.source)
            .unwrap_or_else(|e| panic!("fixture upsert {} failed: {e:?}", file.path));
    }
    project
}

/// Run one leg of `case` on a fresh host and return its envelope.
fn run_leg(case: &dyn PublishedSurfaceCase, leg: BodyLeg) -> OracleEnvelope {
    let project = fresh_project(case.files());
    let host = project.host();
    match leg {
        BodyLeg::LegacyPreparedBody => {
            let _guard = LegacyPreparedBodyLegGuard::activate();
            let envelope = case.run(host);
            assert!(
                legacy_prepared_body_reads() > 0,
                "{}: the legacy leg must serve at least one prepared-body read \
                 through the RAII seam (anti-vacuity)",
                case.id()
            );
            envelope
        }
        BodyLeg::NewLocator => {
            super::LEGACY_PREPARED_BODY_READS.with(|c| c.set(0));
            let envelope = case.run(host);
            assert_eq!(
                legacy_prepared_body_reads(),
                0,
                "{}: the production leg must never route through the retained \
                 prepared-body implementation",
                case.id()
            );
            envelope
        }
    }
}

/// Run both legs of `case` and assert byte-identical envelopes.
pub(crate) fn run_dual_leg_parity(case: &dyn PublishedSurfaceCase) {
    let legacy = run_leg(case, BodyLeg::LegacyPreparedBody);
    let new = run_leg(case, BodyLeg::NewLocator);
    case.assert_discriminating(&legacy);
    case.assert_discriminating(&new);
    assert_eq!(
        legacy.outcome,
        new.outcome,
        "{}: outcome status must match between body-source legs",
        case.id()
    );
    if legacy.canonical_json != new.canonical_json {
        let first_diff = legacy
            .canonical_json
            .bytes()
            .zip(new.canonical_json.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| legacy.canonical_json.len().min(new.canonical_json.len()));
        let start = first_diff.saturating_sub(120);
        let legacy_excerpt =
            &legacy.canonical_json[start..(first_diff + 200).min(legacy.canonical_json.len())];
        let new_excerpt =
            &new.canonical_json[start..(first_diff + 200).min(new.canonical_json.len())];
        panic!(
            "{}: published-surface envelopes DIVERGED between body-source legs at byte \
             {first_diff}\n  legacy: …{legacy_excerpt}…\n  new:    …{new_excerpt}…",
            case.id()
        );
    }
}

// ─── The oracle test surface ─────────────────────────────────────────────

#[test]
fn legacy_leg_guard_routes_body_lowering_through_retained_implementation() {
    // Seam discrimination: with the RAII guard active, decl-body lowering
    // must serve through the retained prepared-body implementation (the
    // counter moves); without it, the production path serves (no reads).
    let project = fresh_project(&[FixtureFile {
        path: "/oracle/seam.ts",
        source: "export interface SeamShape { a: string }\n",
    }]);
    let host = project.host();
    {
        let _guard = LegacyPreparedBodyLegGuard::activate();
        let resolved = host.resolve_named_symbol(
            "/oracle/seam.ts",
            "SeamShape",
            &[],
            Some(crate::semantic_query::ProjectionMode::Expanded),
        );
        assert!(resolved.is_some(), "SeamShape must resolve under the guard");
        assert!(
            legacy_prepared_body_reads() > 0,
            "the active legacy-leg guard must route decl-body lowering through \
             the retained prepared-body implementation"
        );
    }
    super::LEGACY_PREPARED_BODY_READS.with(|c| c.set(0));
    let project = fresh_project(&[FixtureFile {
        path: "/oracle/seam2.ts",
        source: "export interface SeamShapeTwo { a: string }\n",
    }]);
    let host = project.host();
    let resolved = host.resolve_named_symbol(
        "/oracle/seam2.ts",
        "SeamShapeTwo",
        &[],
        Some(crate::semantic_query::ProjectionMode::Expanded),
    );
    assert!(
        resolved.is_some(),
        "SeamShapeTwo must resolve without the guard"
    );
    assert_eq!(
        legacy_prepared_body_reads(),
        0,
        "without the guard the retained implementation must not serve"
    );
}

#[test]
fn manifest_every_surface_class_has_a_case() {
    let cases = all_cases();
    for class in Stage10SurfaceClass::ALL {
        assert!(
            cases.iter().any(|case| case.class() == *class),
            "missing published-surface parity case for {class:?}"
        );
    }
    // Case ids are unique (a duplicated id would hide a missing adapter).
    let mut ids: Vec<&str> = cases.iter().map(|c| c.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), cases.len(), "case ids must be unique");
}

#[test]
fn canonical_json_sorts_object_keys_deterministically() {
    use serde_json::json;
    // Two insertion orders, one canonical form.
    let a = json!({ "zeta": 1, "alpha": { "n": [1, 2], "m": true } });
    let mut b_map = serde_json::Map::new();
    b_map.insert("alpha".to_string(), json!({ "m": true, "n": [1, 2] }));
    b_map.insert("zeta".to_string(), json!(1));
    let b = serde_json::Value::Object(b_map);
    let mut out_a = String::new();
    let mut out_b = String::new();
    super::envelope_tests::write_canonical_for_test(&a, &mut out_a);
    super::envelope_tests::write_canonical_for_test(&b, &mut out_b);
    assert_eq!(
        out_a, out_b,
        "canonicalisation must be insertion-order independent"
    );
    assert_eq!(out_a, r#"{"alpha":{"m":true,"n":[1,2]},"zeta":1}"#);
}

#[test]
fn parity_component_meta_payload() {
    run_dual_leg_parity(&super::cases_tests::ComponentMetaPayloadCase);
}

#[test]
fn parity_fallthrough_root_inheritance() {
    run_dual_leg_parity(&super::cases_tests::FallthroughRootInheritanceCase);
}

#[test]
fn parity_macro_own_body_provenance() {
    run_dual_leg_parity(&super::cases_tests::MacroOwnBodyProvenanceCase);
}

#[test]
fn parity_open_key_domain_carrier_stop() {
    run_dual_leg_parity(&super::cases_tests::OpenKeyDomainCarrierStopCase);
}

#[test]
fn parity_module_augmentation_surface() {
    run_dual_leg_parity(&super::cases_tests::ModuleAugmentationSurfaceCase);
}

#[test]
fn parity_generic_substitution() {
    run_dual_leg_parity(&super::cases_tests::GenericSubstitutionCase);
}
