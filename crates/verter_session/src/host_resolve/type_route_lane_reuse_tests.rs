//! A request resolves each type-route lane once, and every answer and
//! witness is the one a fresh resolution yields.
//!
//! The type-route policy resolves the specifier's `TypeImport` lane, its
//! `EsmImport` fallback, and a second `TypeImport` lane on every delivered
//! target ([`crate::VerterHost::normalize_type_target`]). A request keeps
//! each lane's resolution ([`TypeRouteLanes`]); a kept lane records its
//! observations into the open witness scopes and re-notes an unretained
//! answer exactly as the resolution does, and a change to the resolution
//! world resolves it again.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::host_manage::import_route_witness::ResolutionWitnessScope;
use crate::host_resolve::TypeRouteLanes;
use crate::host_test_audit::ResolutionCurrencyObservation;
use crate::types::{CompileProfile, VirtualNodeKind, VirtualQuery};
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_workspace::ResolutionPublication;

const TYPE_IMPORT: verter_session_query::resolution::ResolveRequestKind =
    verter_session_query::resolution::ResolveRequestKind::TypeImport;
const OWNER: &str = "/proj_ttr/owner.ts";

/// A TypeScript dependency, a JavaScript one with a declaration companion,
/// a JavaScript one without, and a missing one.
const SPECIFIERS: [&str; 4] = ["./ts_dep", "./typed_js", "./plain_js", "./missing"];

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
}

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    for (path, source) in [
        (
            OWNER,
            "import { a } from './ts_dep';\n\
             import { b } from './typed_js';\n\
             import { c } from './plain_js';\n\
             import { d } from './missing';\n\
             export const all = [a, b, c, d];\n",
        ),
        ("/proj_ttr/ts_dep.ts", "export const a = 1;\n"),
        ("/proj_ttr/typed_js.js", "export const b = 2;\n"),
        (
            "/proj_ttr/typed_js.d.ts",
            "export declare const b: number;\nexport interface Extra { n: number }\n",
        ),
        ("/proj_ttr/plain_js.js", "export const c = 3;\n"),
    ] {
        upsert(&host, path, source);
    }
    host
}

type Answer = (
    &'static str,
    ResolutionPublication<String>,
    Vec<FactVersionRef>,
);

/// Three rounds over the specifiers in one request: each answer and the
/// observations it recorded into the witness scope around it.
fn rounds(host: &VerterHost, lanes: &TypeRouteLanes) -> Vec<Answer> {
    let mut answers = Vec::new();
    for _ in 0..3 {
        for specifier in SPECIFIERS {
            let scope = ResolutionWitnessScope::enter();
            let publication = host.resolve_type_dependency_canonical_for_request(
                None,
                OWNER,
                specifier,
                Some(lanes),
            );
            answers.push((specifier, publication, scope.collected()));
        }
    }
    answers
}

/// The engine resolutions since the observation window opened: of
/// delivered targets (absolute specifiers), and of the owner's own
/// specifiers (relative).
fn lane_resolutions(host: &VerterHost) -> (usize, usize) {
    let mut resolutions = (0, 0);
    for observation in host.take_resolution_currency_observations() {
        match observation {
            ResolutionCurrencyObservation::OwnerEdgeRecomputed { specifier, .. }
            | ResolutionCurrencyObservation::OwnerEdgeReused { specifier, .. } => {
                if specifier.starts_with('/') {
                    resolutions.0 += 1;
                } else {
                    resolutions.1 += 1;
                }
            }
            _ => {}
        }
    }
    resolutions
}

fn set_reuse(host: &VerterHost, reuse: bool) {
    host.test_force
        .disable_type_route_lane_reuse_for_tests
        .store(!reuse, Ordering::Relaxed);
}

#[test]
fn a_request_resolves_each_type_route_lane_once_with_the_answers_and_witnesses_of_fresh_lanes() {
    let host = host();
    // Every edge resolved once, so both runs below read the same warm
    // engine candidates.
    let _ = rounds(&host, &TypeRouteLanes::default());

    set_reuse(&host, false);
    host.begin_resolution_currency_observation();
    let fresh = rounds(&host, &TypeRouteLanes::default());
    let fresh_lanes = lane_resolutions(&host);

    set_reuse(&host, true);
    host.begin_resolution_currency_observation();
    let reused = rounds(&host, &TypeRouteLanes::default());
    let reused_lanes = lane_resolutions(&host);

    assert_eq!(
        reused, fresh,
        "every answer, and every observation recorded around it, is the fresh lane's"
    );
    let delivered = |answers: &[Answer]| {
        answers
            .iter()
            .filter(|(_, publication, _)| publication.is_some())
            .count()
    };
    assert_eq!(
        delivered(&fresh),
        3 * 3,
        "three of the four specifiers deliver a target: {fresh:?}"
    );
    assert!(
        fresh.iter().all(|(_, _, observed)| !observed.is_empty()),
        "every answer records observations: {fresh:?}"
    );
    let specifier_lanes = reused_lanes.1;
    assert!(
        specifier_lanes >= SPECIFIERS.len(),
        "every specifier resolves at least its TypeImport lane: {reused_lanes:?}"
    );
    assert_eq!(
        (fresh_lanes, reused_lanes),
        ((3 * 3, 3 * specifier_lanes), (3, specifier_lanes)),
        "a fresh request resolves every lane in every round; a reusing one in its first"
    );
}

#[test]
fn a_kept_lane_answers_only_in_the_world_it_was_resolved_in() {
    const COMPANION: &str = "/proj_ttr/plain_js.d.ts";
    const LATER: &str = "/proj_ttr/later.ts";
    let host = host();
    let lanes = TypeRouteLanes::default();
    let lane = |host: &VerterHost, specifier: &str| {
        host.request_lane(None, OWNER, specifier, TYPE_IMPORT, Some(&lanes))
            .delivered()
            .flatten()
            .cloned()
    };
    // A specifier of the owner that delivers nothing yet, and a delivered
    // JavaScript target without a declaration companion yet.
    let both = |host: &VerterHost| (lane(host, "./later"), lane(host, "/proj_ttr/plain_js.js"));
    let before = both(&host);
    assert_eq!(both(&host), before);
    assert_eq!(before.0, None);
    assert_ne!(before.1.as_deref(), Some(COMPANION));

    // The specifier's module appears, and a declaration companion appears
    // beside the JavaScript target: neither lane kept before answers.
    upsert(&host, LATER, "export const later = 4;\n");
    upsert(&host, COMPANION, "export declare const c: number;\n");
    let after = both(&host);
    set_reuse(&host, false);
    assert_eq!(
        after,
        both(&host),
        "the request answers as a fresh resolution does"
    );
    assert_eq!(
        (after.0.as_deref(), after.1.as_deref()),
        (Some(LATER), Some(COMPANION)),
        "the new modules are the answers"
    );
}

/// A component whose props are imported types: its compiled output and
/// its diagnostics are the same whether the request reuses its type-route
/// lanes or resolves every one again.
#[test]
fn a_compile_with_reused_lanes_outputs_and_diagnoses_as_with_fresh_ones() {
    const COMPONENT: &str = "/proj_ttr/Comp.vue";
    let compile = |reuse: bool| {
        let host = host();
        set_reuse(&host, reuse);
        upsert(
            &host,
            "/proj_ttr/props.ts",
            "import type { Extra } from './typed_js';\n\
             export interface Props { label: string; extra?: Extra }\n",
        );
        upsert(
            &host,
            COMPONENT,
            "<script setup lang=\"ts\">\n\
             import type { Props } from './props';\n\
             import type { Missing } from './missing';\n\
             defineProps<Props & { other?: Missing }>();\n\
             </script>\n\
             <template><div>{{ label }}</div></template>\n",
        );
        let profile = CompileProfile::default();
        let response = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(COMPONENT.to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("the component compiles");
        (
            response.code.to_string(),
            format!("{:?}", response.diagnostics),
            format!("{:?}", host.get_diagnostics(COMPONENT, &profile)),
        )
    };
    assert_eq!(compile(true), compile(false));
}
