//! Surface 2 replacement (see
//! `docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md`).
//!
//! nextest isolates every test in its OWN process, so process-global
//! contamination — a leaked static, TLS that survives a test, a global cache
//! mutated by one test that changes another test's result — is invisible to
//! the normal universe. The former Surface 2 caught this class by replaying
//! every `verter_session` libtest binary in-process, but detection depended
//! on incidental test ordering, a leak could be masked by another test
//! resetting the state, and unrelated tests turned flaky merely by sharing a
//! process.
//!
//! This module replaces that blanket rerun with DELIBERATE, DETERMINISTIC
//! shared-process coverage: each `#[test]` here performs many operations
//! SEQUENTIALLY inside the ONE process nextest gives it — create, use,
//! drop, recreate; multiple hosts/projects live in one process at once;
//! cache/registry invalidation across repeated edits; scheduler shutdown
//! and restart; `OnceLock`/singleton lifecycle and environment-read timing;
//! failure then successful recovery; the `compile_many` batch entry point
//! across hosts; a typeinfo-graph wire request across hosts; an
//! audit-enabled host's TLS-observer lifecycle. It runs as part of the ONE
//! normal nextest universe (wired into `verter_session`'s consolidated
//! integration binary via `tests/cases/mod.rs`) — not a separate archive or
//! surface.

#![cfg(test)]

use std::sync::Arc;

use verter_protocol::typeinfo::graph::{self as wire};
use verter_protocol::verter::v1::{
    type_info_graph_request as wire_request, type_info_graph_response,
};
use verter_session::host_compile::{
    CompileBatchInput, CompileBatchOptions, CompileBatchOutcome, CompileManyTarget,
};
use verter_session::{
    CompileCacheMode, CompileErrorPolicy, CompileProfile, FileLanguage, HostConfig, UpsertRequest,
    VerterHost, VirtualNodeKind, VirtualQuery,
};

fn build_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert(host: &VerterHost, id: &str, src: &str, lang: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: lang,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {id}: {e:?}"));
}

fn prop_names(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> Vec<String> {
    let mut names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    names.sort_unstable();
    names
}

fn vue_with_props(fields: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\ndefineProps<{{ {fields} }}>();\n</script>\n<template><div/></template>\n"
    )
}

/// Create -> use -> drop -> recreate, in one process. A host's Drop chain
/// (`impl Drop for Scheduler` in verter_scheduler joins the driver thread
/// and signals shutdown to all DAG waiters) must leave nothing behind that
/// would make the SECOND host in this process observe the first host's
/// content.
#[test]
fn create_use_drop_recreate_hosts_stay_independent() {
    {
        let host_a = build_host();
        upsert(
            &host_a,
            "/src/Comp.vue",
            &vue_with_props("fromHostA: number"),
            FileLanguage::vue(),
        );
        let meta_a = host_a
            .get_component_meta("/src/Comp.vue")
            .expect("host A must resolve");
        assert_eq!(prop_names(&meta_a), vec!["fromHostA"]);
        // host_a drops here — exercises Scheduler::drop in-process.
    }

    let host_b = build_host();
    upsert(
        &host_b,
        "/src/Comp.vue",
        &vue_with_props("fromHostB: string"),
        FileLanguage::vue(),
    );
    let meta_b = host_b
        .get_component_meta("/src/Comp.vue")
        .expect("host B must resolve independently of the dropped host A");
    assert_eq!(
        prop_names(&meta_b),
        vec!["fromHostB"],
        "host B must not observe host A's content for the same canonical id \
         after host A was dropped and a fresh host was created in the same process"
    );
}

/// Two hosts alive AT THE SAME TIME in one process, given the SAME
/// canonical id but different content. A process-global cache keyed only
/// on canonical id (ignoring host identity) would leak one host's result
/// into the other's read.
#[test]
fn multiple_hosts_coexist_in_one_process_without_cross_contamination() {
    let host_a = build_host();
    let host_b = build_host();

    upsert(
        &host_a,
        "/src/Comp.vue",
        &vue_with_props("onA: number"),
        FileLanguage::vue(),
    );
    upsert(
        &host_b,
        "/src/Comp.vue",
        &vue_with_props("onB: string; onBExtra: boolean"),
        FileLanguage::vue(),
    );

    let meta_a = host_a
        .get_component_meta("/src/Comp.vue")
        .expect("host A resolve");
    let meta_b = host_b
        .get_component_meta("/src/Comp.vue")
        .expect("host B resolve");

    assert_eq!(prop_names(&meta_a), vec!["onA"]);
    assert_eq!(prop_names(&meta_b), vec!["onB", "onBExtra"]);

    // Re-read A again AFTER touching B, to catch a shared mutable slot that
    // only manifests once a second host has performed work.
    let meta_a_again = host_a
        .get_component_meta("/src/Comp.vue")
        .expect("host A re-resolve");
    assert_eq!(
        prop_names(&meta_a_again),
        vec!["onA"],
        "host A's result must be stable after host B (same process) did unrelated work"
    );
}

/// Cache/registry invalidation across many repeated edits to ONE host in
/// ONE process — many more edit cycles than any single per-test nextest
/// process would normally see, deliberately stressing whatever state a
/// leak would accumulate in.
#[test]
fn cache_and_registry_invalidation_survives_many_repeated_edits() {
    let host = build_host();
    for n in 1..=8usize {
        let fields: String = (0..n).map(|i| format!("p{i}: number; ")).collect();
        upsert(
            &host,
            "/src/Comp.vue",
            &vue_with_props(fields.trim_end()),
            FileLanguage::vue(),
        );
        let meta = host
            .get_component_meta("/src/Comp.vue")
            .unwrap_or_else(|| panic!("edit {n} must resolve"));
        assert_eq!(
            meta.props.len(),
            n,
            "edit {n}: registry/cache invalidation must reflect exactly the current content, \
             not an accumulation or a stale prior edit"
        );
    }
}

/// Scheduler shutdown and restart, stress-repeated in one process: many
/// hosts created and dropped in sequence. A driver-thread join that hangs,
/// or DAG-waiter state that leaks across a shutdown, would surface here
/// (timeout / wrong result) rather than in a fresh per-test process where
/// only one host ever existed.
#[test]
fn scheduler_shutdown_and_restart_repeats_cleanly_in_one_process() {
    for i in 0..6 {
        let host = build_host();
        let path = format!("/src/Comp{i}.vue");
        upsert(
            &host,
            &path,
            &vue_with_props("n: number"),
            FileLanguage::vue(),
        );
        let meta = host
            .get_component_meta(&path)
            .unwrap_or_else(|| panic!("iteration {i} must resolve after a fresh scheduler start"));
        assert_eq!(meta.props.len(), 1, "iteration {i}");
        drop(host);
    }
}

/// `MetaProject::shutdown` marks a project terminally dead. A SECOND
/// `MetaProject` created in the SAME process afterward must be unaffected —
/// it is a distinct instance, not a process-global singleton that a prior
/// shutdown could poison.
#[test]
fn meta_project_shutdown_then_new_project_in_same_process_is_clean() {
    use verter_session::meta::MetaProject;

    let project_a = MetaProject::new(build_host());
    assert!(!project_a.is_shutdown());
    project_a.shutdown();
    assert!(project_a.is_shutdown());
    // Idempotent — a second shutdown on the same instance must not panic.
    project_a.shutdown();

    let project_b = MetaProject::new(build_host());
    assert!(
        !project_b.is_shutdown(),
        "a fresh MetaProject in the same process must not inherit a prior \
         project's shutdown state"
    );
    upsert(
        project_b.host(),
        "/src/Comp.vue",
        &vue_with_props("live: boolean"),
        FileLanguage::vue(),
    );
    let meta = project_b
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("project B must resolve normally after project A shut down in the same process");
    assert_eq!(prop_names(&meta), vec!["live"]);
}

/// `verter_session::dump_decl_handoff_stats` / `reset_decl_handoff_stats`
/// (`decl_lowering.rs`) are backed by a process-global `OnceLock` that
/// resolves the `VERTER_DECL_HANDOFF_PROFILE` env gate exactly ONCE per
/// process, at first consultation — services capture the resolved sink at
/// construction rather than re-reading the env var per operation. This is
/// exactly the class of hazard nextest's per-test process isolation cannot
/// see (each test starts a fresh process, so "does the resolved value stay
/// resolved for the rest of THIS process" never gets exercised by any
/// individual test) and exactly what deliberate shared-process coverage
/// is for.
#[test]
fn decl_handoff_profile_sink_resolves_once_per_process_not_per_call() {
    // First consultation in this process, gate unset: must resolve OFF.
    assert!(
        verter_session::dump_decl_handoff_stats().is_none(),
        "the handoff-profile sink must read OFF on first consultation in a \
         fresh process with VERTER_DECL_HANDOFF_PROFILE unset"
    );

    let host = build_host();
    upsert(
        &host,
        "/src/props.ts",
        "export interface Foo { a: number; }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Foo } from './props';\ndefineProps<Foo>();\n</script>\n<template><div/></template>\n",
        FileLanguage::vue(),
    );
    let _ = host
        .get_component_meta("/src/Comp.vue")
        .expect("cross-file resolve to exercise decl lowering");

    // SAFETY: this test owns its process end to end (nextest gives every
    // test its own process) and no other thread in this binary reads or
    // writes this variable concurrently.
    unsafe {
        std::env::set_var("VERTER_DECL_HANDOFF_PROFILE", "1");
    }

    let host2 = build_host();
    upsert(
        &host2,
        "/src/Comp2.vue",
        &vue_with_props("z: number"),
        FileLanguage::vue(),
    );
    let _ = host2
        .get_component_meta("/src/Comp2.vue")
        .expect("second host's cross-file-free resolve");

    assert!(
        verter_session::dump_decl_handoff_stats().is_none(),
        "the sink resolved OFF on first consultation in this process; setting \
         the env var afterward must NOT retroactively enable it — proves the \
         gate is a true process-wide OnceLock, not re-read per call/per host"
    );

    // Restore the environment for anything else that might run later in
    // this same process (there is nothing else here, but this is the
    // documented shared-process discipline: never leave a mutated env var
    // behind).
    unsafe {
        std::env::remove_var("VERTER_DECL_HANDOFF_PROFILE");
    }
    assert!(std::env::var_os("VERTER_DECL_HANDOFF_PROFILE").is_none());
}

/// Failure then successful recovery, in one host/process: an upsert that
/// resolves to no component metadata (malformed script content) must not
/// poison later, valid operations against the same host.
#[test]
fn failure_then_recovery_in_one_host_and_process() {
    let host = build_host();

    // Deliberately malformed: an unclosed script tag / invalid TS. This is
    // NOT expected to yield usable component metadata.
    upsert(
        &host,
        "/src/Broken.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: number }(;\n</script>\n<template><div/></template>\n",
        FileLanguage::vue(),
    );
    let broken = host.get_component_meta("/src/Broken.vue");
    // Either `None` or a degraded/empty result is acceptable — but the
    // scenario must actually BE a failure, or the "recovery" assertions
    // below prove nothing. If a malformed macro body somehow resolved to
    // the exact well-formed prop set a correctly-closed
    // `defineProps<{ a: number }>()` would produce, this is not exercising
    // a real failure at all.
    if let Some(meta) = &broken {
        assert_ne!(
            prop_names(meta),
            vec!["a"],
            "the malformed `defineProps<{{ a: number }}(;` body must not resolve \
             to the SAME well-formed prop set a correctly-closed macro would \
             produce — otherwise this scenario never exercised a real failure, \
             and the recovery assertions below would prove nothing about recovery"
        );
    }

    upsert(
        &host,
        "/src/Recovered.vue",
        &vue_with_props("healthy: number"),
        FileLanguage::vue(),
    );
    let recovered = host
        .get_component_meta("/src/Recovered.vue")
        .expect("a valid file in the same host must resolve after a prior malformed upsert");
    assert_eq!(prop_names(&recovered), vec!["healthy"]);

    // And the ORIGINAL canonical id, corrected in place, must also recover.
    upsert(
        &host,
        "/src/Broken.vue",
        &vue_with_props("fixed: string"),
        FileLanguage::vue(),
    );
    let fixed = host
        .get_component_meta("/src/Broken.vue")
        .expect("the originally-malformed canonical id must resolve once corrected in place");
    assert_eq!(prop_names(&fixed), vec!["fixed"]);
}

/// Repeated initialization under different `HostConfig` presets, all in one
/// process — each host must reflect ONLY its own config, not a config a
/// prior host in the same process happened to use.
///
/// `HostConfig::default()` already ships `dev_mode: true` +
/// `CompileErrorPolicy::DevServeLastKnownGood` (`types.rs`'s `Default impl`).
/// The prior version of this test built its "different" middle config as
/// `HostConfig { dev_mode: true, ..HostConfig::default() }` — which
/// duplicates the default field-for-field, so all three configs in the
/// array were actually IDENTICAL. The `props.len() == 1` assertion passed
/// regardless of `dev_mode`, `compile_error_policy`, or any other field, so
/// the test could not have caught a `HostConfig` that leaked from a prior
/// host or was silently ignored by a new one in the same process.
///
/// This version uses a config that is GENUINELY different (a production
/// config: `dev_mode: false` + `CompileErrorPolicy::StrictError`) and
/// asserts an outcome that actually depends on it: under `dev_mode: true`
/// (default) `CompileCacheMode::Content` unconditionally downgrades to
/// `Stateless` (`HasDevLastGood` fires on every compile — see
/// `compile_cache_mode.rs::has_dev_last_good` and its documented downgrade
/// matrix), publishing NO content-addressed entry; under a production
/// config the same request runs as genuine `Content` and publishes exactly
/// one. A `HostConfig` leak between hosts in this process would surface as
/// the WRONG entry count here.
#[test]
fn repeated_init_under_different_configs_stays_isolated() {
    let configs = [
        (HostConfig::default(), 0usize),
        (
            HostConfig {
                dev_mode: false,
                compile_error_policy: CompileErrorPolicy::StrictError,
                ..HostConfig::default()
            },
            1usize,
        ),
        (HostConfig::default(), 0usize),
    ];

    // A fact-free SFC: no imports, no cross-file deps, so no OTHER
    // downgrade reason fires and the `dev_mode`-driven `HasDevLastGood`
    // reason is the only thing distinguishing the two config rows.
    const FACT_FREE: &str =
        "<script setup lang=\"ts\">const n = 1</script><template><div>{{ n }}</div></template>";

    for (i, (config, expected_content_entries)) in configs.into_iter().enumerate() {
        let host = VerterHost::new_standalone(config);
        let path = format!("/src/Comp{i}.vue");
        upsert(&host, &path, FACT_FREE, FileLanguage::vue());

        let profile = CompileProfile {
            requested_mode: CompileCacheMode::Content,
            ..CompileProfile::default()
        };
        let _ = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(path.clone()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile,
            })
            .unwrap_or_else(|e| panic!("config iteration {i}: compile: {e:?}"));

        assert_eq!(
            host.compile_output_pure_content_entry_count(),
            expected_content_entries,
            "config iteration {i}: a Content-mode request's actual publication \
             behavior must reflect THIS host's own dev_mode/compile_error_policy \
             — not a config a prior host in this process happened to use, and \
             not the config-agnostic ignore-everything default"
        );
    }
}

fn batch_produced_code(entry: &verter_session::host_compile::CompileBatchEntry) -> String {
    match &entry.outcome {
        CompileBatchOutcome::Produced { code, .. } => code.to_string(),
        CompileBatchOutcome::Failed { errors } => {
            panic!("expected a produced compile, got errors: {errors:?}")
        }
    }
}

/// `compile_many` (the batch host-backed compile entry point) across TWO
/// hosts that share the SAME canonical id but different content, in one
/// process. This is the batch-path sibling of
/// `multiple_hosts_coexist_in_one_process_without_cross_contamination`
/// above, which only exercises the single-file `get_component_meta` path.
/// None of the other scenarios in this file call `compile_many` at all — a
/// process-global cache the batch path added, keyed only on canonical id
/// and ignoring host identity, would leak one host's compiled output into
/// the other's batch result without this coverage.
#[test]
fn compile_many_across_hosts_with_overlapping_canonical_ids_stays_isolated() {
    let host_a = build_host();
    let host_b = build_host();

    let src_a = "<script setup lang=\"ts\">const marker = 111111</script>\
        <template><div>{{ marker }}</div></template>";
    let src_b = "<script setup lang=\"ts\">const marker = 222222</script>\
        <template><div>{{ marker }}</div></template>";

    let batch_input = |source: &str| CompileBatchInput {
        canonical_id: "/src/Shared.vue".to_string(),
        source: Arc::from(source),
        requested_mode: None,
        component_id: None,
    };

    let results_a = host_a.compile_many(
        vec![batch_input(src_a)],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    let results_b = host_b.compile_many(
        vec![batch_input(src_b)],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    assert_eq!(results_a.len(), 1);
    assert_eq!(results_b.len(), 1);

    let code_a = batch_produced_code(&results_a[0]);
    let code_b = batch_produced_code(&results_b[0]);
    assert!(
        code_a.contains("111111") && !code_a.contains("222222"),
        "host A's compile_many result for /src/Shared.vue must reflect ONLY \
         host A's own content, not host B's marker for the same canonical id \
         (same process): {code_a}"
    );
    assert!(
        code_b.contains("222222") && !code_b.contains("111111"),
        "host B's compile_many result for /src/Shared.vue must reflect ONLY \
         host B's own content, not host A's marker for the same canonical id \
         (same process): {code_b}"
    );

    // Re-run host A's batch AFTER host B has compiled the same canonical id,
    // to catch a shared slot that only manifests once a second host (same
    // canonical id, same process) has done work.
    let results_a_again = host_a.compile_many(
        vec![batch_input(src_a)],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    let code_a_again = batch_produced_code(&results_a_again[0]);
    assert!(
        code_a_again.contains("111111") && !code_a_again.contains("222222"),
        "host A's compile_many result must stay stable after host B (same \
         process, same canonical id) did unrelated batch work: {code_a_again}"
    );
}

/// Build a well-formed framework-surface `TypeInfoGraphRequest` envelope for
/// `canonical` + `adapter_id`, at the closed-contract schema version 3 (see
/// CLAUDE.md's Typeinfo Wire Contract). Mirrors the same helper in
/// `g_block/framework_surface_executor.rs` — kept local rather than shared
/// because `tests/cases/*.rs` modules are private siblings, not a library.
fn framework_envelope(canonical: &str, adapter_id: &str) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: 3,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: adapter_id.to_string(),
                }),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(
                        verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                            wire::ClosureOneLevel {},
                        ),
                    ),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: 3,
            },
        )),
    }
}

/// Resolve a props-kind entry's member names from a framework-surface
/// response, through the graph's interned string table.
fn resolved_props(response: &wire::TypeInfoGraphResponse) -> Vec<String> {
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected the framework_surface response arm, got {other:?}"),
    };
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    let props_entry = payload
        .surfaces
        .iter()
        .find(|e| e.kind == wire::FrameworkSurfaceKind::Props as i32)
        .expect("a PROPS entry");
    let mut names: Vec<String> = props_entry
        .members
        .iter()
        .map(|m| strings.get(m.name_id as usize).cloned().unwrap_or_default())
        .collect();
    names.sort_unstable();
    names
}

/// A typeinfo-graph wire request (`resolve_framework_surface_with_audit` —
/// the SOLE audited entry point for `TypeInfoGraphRequest` framework
/// surfaces; see CLAUDE.md's Framework Adapter Substrate) across TWO hosts
/// with the SAME canonical id and different content, in one process. None
/// of the other scenarios in this file exercise the typeinfo-graph wire
/// path (or its audit registration) at all — a process-global cache or
/// registry the executor/adapter path keyed only on canonical id would leak
/// one host's resolved surface into the other host's response.
#[test]
fn typeinfo_graph_request_across_hosts_stays_isolated() {
    let host_a = build_host();
    let host_b = build_host();

    upsert(
        &host_a,
        "/src/Comp.vue",
        &vue_with_props("onA: number"),
        FileLanguage::vue(),
    );
    upsert(
        &host_b,
        "/src/Comp.vue",
        &vue_with_props("onB: string; onBExtra: boolean"),
        FileLanguage::vue(),
    );

    let result_a =
        host_a.resolve_framework_surface_with_audit(framework_envelope("/src/Comp.vue", "vue"));
    let response_a = result_a
        .as_result()
        .expect("host A's framework-surface request must resolve structurally");
    assert_eq!(resolved_props(response_a), vec!["onA"]);

    let result_b =
        host_b.resolve_framework_surface_with_audit(framework_envelope("/src/Comp.vue", "vue"));
    let response_b = result_b
        .as_result()
        .expect("host B's framework-surface request must resolve structurally");
    assert_eq!(
        resolved_props(response_b),
        vec!["onB".to_string(), "onBExtra".to_string()]
    );

    // Re-read A again AFTER touching B (same canonical id, same process), to
    // catch a shared slot that only manifests once a second host has
    // resolved a typeinfo-graph request for the same canonical id.
    let result_a_again =
        host_a.resolve_framework_surface_with_audit(framework_envelope("/src/Comp.vue", "vue"));
    let response_a_again = result_a_again
        .as_result()
        .expect("host A's re-resolve must resolve structurally");
    assert_eq!(
        resolved_props(response_a_again),
        vec!["onA"],
        "host A's typeinfo-graph result must be stable after host B (same \
         process, same canonical id) resolved its own request"
    );
}

/// Audit-enabled host lifecycle: create -> audited call -> drop -> recreate,
/// in one process. `verter_audit::current_observer()` is thread-local
/// state — exactly the class nextest's per-test process isolation cannot
/// see (a leaked guard on the SAME thread would silently remain visible to
/// a LATER host's operations in this process). None of the other scenarios
/// in this file enable audit at all.
#[test]
fn audit_enabled_host_lifecycle_leaves_no_tls_or_registry_residue() {
    assert!(
        verter_audit::current_observer().is_none(),
        "no observer should be installed before this test performs any audited work"
    );

    let audited_config = || HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    };

    {
        let host_a = Arc::new(VerterHost::new_standalone(audited_config()));
        upsert(
            &host_a,
            "/src/CompA.vue",
            &vue_with_props("a: number"),
            FileLanguage::vue(),
        );
        let _ = host_a.analyze_with_audit("/src/CompA.vue");
        assert!(
            verter_audit::current_observer().is_none(),
            "an audited call's RequestContextGuard must unwind once the call \
             returns, leaving no observer visible to code that runs afterward \
             on this thread/process"
        );
        let snap_a = host_a.host_audit_runtime().snapshot();
        assert_eq!(
            snap_a.records_store_size, 1,
            "host A's one audited call must publish exactly one record"
        );
        // host_a drops here.
    }

    assert!(
        verter_audit::current_observer().is_none(),
        "dropping an audit-enabled host must not leave a stray observer installed"
    );

    let host_b = Arc::new(VerterHost::new_standalone(audited_config()));
    let snap_b_before = host_b.host_audit_runtime().snapshot();
    assert_eq!(
        snap_b_before.records_store_size, 0,
        "a freshly-created audit-enabled host, in the same process as a prior \
         (now-dropped) audit-enabled host, must start with an EMPTY records \
         store — not inherit host A's record count from a shared/leaked runtime"
    );

    upsert(
        &host_b,
        "/src/CompB.vue",
        &vue_with_props("b: string"),
        FileLanguage::vue(),
    );
    let _ = host_b.analyze_with_audit("/src/CompB.vue");
    let snap_b_after = host_b.host_audit_runtime().snapshot();
    assert_eq!(
        snap_b_after.records_store_size, 1,
        "host B's own audited call must publish exactly one record in its OWN \
         runtime, independent of host A's prior (dropped) runtime"
    );
    assert!(
        verter_audit::current_observer().is_none(),
        "host B's audited call must also unwind its guard cleanly"
    );
}
