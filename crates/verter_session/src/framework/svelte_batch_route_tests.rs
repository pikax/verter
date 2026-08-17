//! The Svelte batch route, executed.
//!
//! `compile_many` is a distinct capability cell from `get_virtual_file`: the
//! route is part of the cell's identity, and delegation shown in source is
//! route-identity evidence, not an executed result. So this drives real Svelte
//! inputs through the batch boundary and compares each item against the
//! single-file route for the same typed request.
//!
//! Executing it establishes that the two routes are NOT equivalent for Svelte.
//! [`CompileBatchInput`] carries no source-language field, and the batch upserts
//! every input as a Vue carrier
//! (`crates/verter_session/src/host_compile.rs:475` hardcodes
//! `file_language: verter_language::FileLanguage::vue()`), so a `.svelte` input
//! is parsed and compiled by the Vue carrier and never reaches the Svelte one.
//! The characterizations below pin that outcome; the `#[ignore]`d conformance
//! target states the equivalence the route must reach. This module owns no
//! correction.
//!
//! What the batch boundary DOES honour today is asserted green: input ordering,
//! per-item independence, and the optional-product (source-map) axis.
//!
//! Run with
//! `cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1`
//! (add `--ignored` for the conformance target, which fails by design).
//!
//! Read the `running N tests` line, never the exit code: libtest's filter is a
//! single literal substring with no alternation, so `"a\\|b"` matches nothing
//! and still exits 0.

use std::sync::Arc;

use crate::host_compile::{
    CompileBatchEntry, CompileBatchInput, CompileBatchOptions, CompileBatchRenderProfile,
    CompileManyTarget,
};
use crate::{
    CompileProfile, HostConfig, HostError, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

/// The full path of THIS module's own witness test, as the census names it.
///
/// Not merely the module path: the census requires a test with exactly this
/// path to be present in the binary's own listing, and derives the module it
/// counts from it. Pointing the constant at another module — even a sibling of
/// the right shape — therefore names a witness that module does not have. The
/// census READS this constant rather than repeating the string, so deleting
/// this module breaks the census's compile.
pub(crate) const CENSUS_WITNESS_PATH: &str =
    concat!(module_path!(), "::this_suite_is_registered_with_the_census");

/// The other half of that dependency.
///
/// The census lives OUTSIDE this module deliberately — a check placed inside a
/// suite is deleted by the same edit that empties it. That leaves the reverse
/// hole: deleting the census too. This test consumes an item the census owns,
/// so removing EITHER `mod` declaration is a COMPILE error rather than a filter
/// that silently matches nothing and still exits 0.
#[test]
fn this_suite_is_registered_with_the_census() {
    assert!(
        super::suite_census::covers(CENSUS_WITNESS_PATH),
        "{CENSUS_WITNESS_PATH}: the census carries no test for this suite, so this suite's \
         documented invocation could match nothing and still report success"
    );
}

/// A supported Svelte client component: through the single-file route it
/// publishes a runtime module.
const SUPPORTED: &str =
    "<script>\n  let count = $state(0);\n</script>\n\n<div class=\"root\">{count}</div>\n";

/// A second supported component, distinct from [`SUPPORTED`], so a fanned-out
/// result would be visible.
const SUPPORTED_TWO: &str =
    "<script>\n  let total = $state(7);\n</script>\n\n<span class=\"total\">{total}</span>\n";

/// The committed fixture whose runtime surface the Svelte client backend
/// refuses (`$props()` read from the instance script).
const ADVANCED_RUNE_REFUSAL: &str = include_str!(
    "../../../../packages/framework-conformance-harness/fixtures/svelte/props-events.svelte"
);

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_svelte(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: verter_language::FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {canonical}: {error:?}"));
}

fn batch_input(canonical: &str, source: &str) -> CompileBatchInput {
    CompileBatchInput {
        canonical_id: canonical.to_string(),
        source: Arc::from(source),
        requested_mode: None,
        component_id: None,
    }
}

/// The render profile whose axes match [`single_file_profile`], so both routes
/// are asked the SAME typed question.
fn render_profile(ssr: bool, source_map: bool) -> CompileBatchRenderProfile {
    CompileBatchRenderProfile {
        filename: None,
        is_production: true,
        custom_element: false,
        ssr,
        force_js: false,
        force_vapor: false,
        source_map,
        comments: None,
        hmr_strategy: crate::types::HmrStrategy::None,
        runtime_module_name: None,
        types_module_name: None,
        delimiters: None,
        custom_elements: None,
        ssr_module_id: None,
    }
}

fn single_file_profile(ssr: bool, source_map: bool) -> CompileProfile {
    CompileProfile {
        is_production: true,
        custom_element: false,
        ssr,
        source_map,
        hmr_strategy: crate::types::HmrStrategy::None,
        ..CompileProfile::default()
    }
}

/// What the single-file route returns for one canonical under one profile.
#[derive(Debug, PartialEq, Eq)]
enum SingleFileOutcome {
    Published { code: String, has_map: bool },
    Refused { diagnostic_code: String },
    Missing,
}

fn single_file(host: &VerterHost, canonical: &str, profile: &CompileProfile) -> SingleFileOutcome {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    }) {
        Ok(response) => SingleFileOutcome::Published {
            code: response.code.to_string(),
            has_map: response.source_map.is_some(),
        },
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => SingleFileOutcome::Refused { diagnostic_code },
        Err(HostError::MissingVirtualNode { .. }) => SingleFileOutcome::Missing,
        Err(other) => panic!("{canonical}: unmodelled single-file outcome {other:?}"),
    }
}

/// Run one batch and return its entries.
fn run_batch(inputs: &[CompileBatchInput], target: CompileManyTarget) -> Vec<CompileBatchEntry> {
    host().compile_many(inputs.to_vec(), CompileBatchOptions::default(), target)
}

/// The bytes the SINGLE-FILE route produces for the same source registered
/// under the adapter the batch actually used.
///
/// This is the positive half of every characterization below: it pins what the
/// batch currently emits to an independently-produced reference rather than
/// merely asserting that something is absent, so arbitrarily different output
/// fails.
fn single_file_reference_under(
    canonical: &str,
    source: &str,
    language: verter_language::FileLanguage,
    profile: &CompileProfile,
) -> SingleFileOutcome {
    let reference = host();
    let _ = reference
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {canonical}: {error:?}"));
    single_file(&reference, canonical, profile)
}

/// The TYPED route evidence of which framework adapter a host registered a
/// canonical under: the file's own `FileLanguage` row, read from the host's
/// source snapshot.
///
/// This is the SAME field every dispatch decision reads — the carrier registry
/// looks its compiler up by the language's adapter id
/// (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2907-2911`),
/// and the declaration-carrier surface resolves its adapter from it
/// (`virtual_file_pipeline.rs:2340-2352`). So it is the route's own record of
/// which carrier handled the file, obtained without inspecting one byte of
/// generated output.
fn registered_adapter_id(host: &VerterHost, canonical: &str) -> Option<String> {
    host.scheduler
        .try_get_source(canonical)
        .and_then(|snapshot| {
            snapshot
                .downcast_data::<crate::host_executor::HostSourceData>()
                .and_then(|data| {
                    data.file_language
                        .adapter_id()
                        .map(|id| id.as_str().to_string())
                })
        })
}

// ══════════════════════════════════════════════════════════════════════════
// Characterization — the batch route's actual Svelte behaviour
// ══════════════════════════════════════════════════════════════════════════

/// CHARACTERIZATION — a `.svelte` batch input is currently compiled by the VUE
/// carrier, so the batch route is not equivalent to the single-file route.
///
/// `CompileBatchInput` carries no source-language field and the batch's own
/// upsert hardcodes `FileLanguage::vue()`
/// (`crates/verter_session/src/host_compile.rs:475`), so the carrier registry
/// dispatches the Vue compiler for a Svelte source. The single-file route,
/// given the same bytes under `FileLanguage::svelte()`, publishes a Svelte
/// client module.
///
/// Discriminating in BOTH directions: it fails if the batch starts producing
/// Svelte output AND if the single-file route stops.
#[test]
fn a_svelte_batch_input_is_currently_compiled_by_the_vue_carrier() {
    let inputs = vec![batch_input("/batch/One.svelte", SUPPORTED)];
    let batch_host = host();
    let entries = batch_host.compile_many(
        inputs.clone(),
        CompileBatchOptions::default(),
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );
    let batched = &entries[0];
    assert!(
        batched.errors.is_empty(),
        "the batch reported errors for a component the single-file route publishes: {:?}",
        batched.errors
    );
    // TYPED route evidence, not a look at the bytes: the batch host's own
    // registered adapter for this canonical is the VUE one.
    assert_eq!(
        registered_adapter_id(&batch_host, "/batch/One.svelte").as_deref(),
        Some("vue"),
        "the batch registered `/batch/One.svelte` under a different adapter than Vue; if it now \
         registers Svelte, un-ignore `a_svelte_batch_matches_the_single_file_route_item_for_item`"
    );

    // The single-file route, same bytes, same axes — a genuine Svelte module.
    let single = host();
    upsert_svelte(&single, "/batch/One.svelte", SUPPORTED);
    let SingleFileOutcome::Published { code, .. } = single_file(
        &single,
        "/batch/One.svelte",
        &single_file_profile(false, true),
    ) else {
        panic!("the single-file route stopped publishing this component");
    };
    // The same typed evidence on the single-file host names the SVELTE adapter,
    // so the two routes registered the same bytes under different adapters.
    assert_eq!(
        registered_adapter_id(&single, "/batch/One.svelte").as_deref(),
        Some("svelte"),
        "the single-file route no longer registers this component under the Svelte adapter"
    );
    assert_ne!(
        batched.code.as_ref(),
        code.as_str(),
        "the two routes now agree, so the characterized divergence is gone"
    );

    // The POSITIVE half: the batch's bytes are exactly what the single-file
    // route produces for the same source registered under the adapter the batch
    // actually used. Arbitrarily different batch output fails here, so this
    // characterization is discriminating in the worsening direction too.
    assert_eq!(
        single_file_reference_under(
            "/batch/One.svelte",
            SUPPORTED,
            verter_language::FileLanguage::vue(),
            &single_file_profile(false, true),
        ),
        SingleFileOutcome::Published {
            code: batched.code.to_string(),
            has_map: batched.source_map.is_some(),
        },
        "the batch's output is no longer what the Vue-registered single-file route produces for \
         the same source"
    );
}

/// CHARACTERIZATION — the Svelte runtime refusals never fire on the batch
/// route, because the Svelte carrier is never reached.
///
/// Both refusal cases are covered: the advanced-rune refusal (a per-component
/// property) and the `generate: "server"` refusal (a batch-level profile axis).
#[test]
fn the_svelte_runtime_refusals_do_not_fire_on_the_batch_route() {
    // (a) the advanced-rune refusal.
    let advanced = run_batch(
        &[batch_input("/batch/Refused.svelte", ADVANCED_RUNE_REFUSAL)],
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );
    assert!(
        !advanced[0]
            .errors
            .iter()
            .any(|error| error.contains("svelte-runtime-unsupported-advanced-rune")),
        "the batch now surfaces the advanced-rune refusal: {:?}",
        advanced[0].errors
    );
    // POSITIVE: the batch published exactly the Vue-registered single-file
    // route's bytes for these source bytes. Without this the assertion above
    // would stay green for arbitrarily worse output.
    assert_eq!(
        single_file_reference_under(
            "/batch/Refused.svelte",
            ADVANCED_RUNE_REFUSAL,
            verter_language::FileLanguage::vue(),
            &single_file_profile(false, true),
        ),
        SingleFileOutcome::Published {
            code: advanced[0].code.to_string(),
            has_map: advanced[0].source_map.is_some(),
        },
        "the batch's output for the refusal-shaped input is no longer what the Vue-registered \
         single-file route produces"
    );
    // The single-file route DOES refuse the same bytes — the comparison that
    // makes the absence above a divergence rather than a property of the input.
    let single = host();
    upsert_svelte(&single, "/batch/Refused.svelte", ADVANCED_RUNE_REFUSAL);
    assert_eq!(
        single_file(
            &single,
            "/batch/Refused.svelte",
            &single_file_profile(false, true)
        ),
        SingleFileOutcome::Refused {
            diagnostic_code: "svelte-runtime-unsupported-advanced-rune".to_string()
        },
        "the single-file route stopped refusing this component"
    );

    // (b) the server-generate refusal.
    let server = run_batch(
        &[batch_input("/batch/Server.svelte", SUPPORTED)],
        CompileManyTarget::RuntimeRender {
            profile: render_profile(true, true),
        },
    );
    assert!(
        !server[0]
            .errors
            .iter()
            .any(|error| error.contains("svelte-runtime-unsupported-server-generate")),
        "the batch now surfaces the server-generate refusal: {:?}",
        server[0].errors
    );
    // POSITIVE, same reasoning, on the server-profile lane.
    assert_eq!(
        single_file_reference_under(
            "/batch/Server.svelte",
            SUPPORTED,
            verter_language::FileLanguage::vue(),
            &single_file_profile(true, true),
        ),
        SingleFileOutcome::Published {
            code: server[0].code.to_string(),
            has_map: server[0].source_map.is_some(),
        },
        "the batch's server-lane output is no longer what the Vue-registered single-file route \
         produces"
    );
    let single_server = host();
    upsert_svelte(&single_server, "/batch/Server.svelte", SUPPORTED);
    assert_eq!(
        single_file(
            &single_server,
            "/batch/Server.svelte",
            &single_file_profile(true, true)
        ),
        SingleFileOutcome::Refused {
            diagnostic_code: "svelte-runtime-unsupported-server-generate".to_string()
        },
        "the single-file server route stopped refusing"
    );
}

/// The same divergence on the HOST-BACKED lane — the one IDE / analysis / TSC
/// consumers use — so the finding is not confined to the render lane.
#[test]
fn the_host_backed_batch_lane_shows_the_same_svelte_language_divergence() {
    let batch_host = host();
    let entries = batch_host.compile_many(
        vec![
            batch_input("/batch/HostOne.svelte", SUPPORTED),
            batch_input("/batch/HostRefused.svelte", ADVANCED_RUNE_REFUSAL),
        ],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    assert_eq!(entries.len(), 2, "one entry per input");
    assert_eq!(
        registered_adapter_id(&batch_host, "/batch/HostOne.svelte").as_deref(),
        Some("vue"),
        "the host-backed lane registered this canonical under a different adapter than Vue"
    );
    assert!(
        entries[1].errors.is_empty(),
        "the host-backed lane now reports an error for the refused component: {:?}",
        entries[1].errors
    );
    // POSITIVE: both entries carry exactly the Vue-registered single-file
    // route's bytes under the host-backed lane's own bundler preset.
    let bundler = crate::host_compile::compile_profile_for_bundler();
    for (entry, source) in [
        (&entries[0], SUPPORTED),
        (&entries[1], ADVANCED_RUNE_REFUSAL),
    ] {
        assert_eq!(
            single_file_reference_under(
                &entry.canonical_id,
                source,
                verter_language::FileLanguage::vue(),
                &bundler,
            ),
            SingleFileOutcome::Published {
                code: entry.code.to_string(),
                has_map: entry.source_map.is_some(),
            },
            "{}: the host-backed lane's output is no longer what the Vue-registered single-file \
             route produces",
            entry.canonical_id
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════
// The batch contract the route DOES honour — asserted green
// ══════════════════════════════════════════════════════════════════════════

/// Ordering is the caller's input order, and no item's result contaminates
/// another's — with a refusal-shaped input in the MIDDLE so a shift would be
/// visible in both directions.
#[test]
fn batch_ordering_is_stable_and_items_do_not_contaminate_each_other() {
    let inputs = vec![
        batch_input("/batch/OrderOne.svelte", SUPPORTED),
        batch_input("/batch/OrderRefused.svelte", ADVANCED_RUNE_REFUSAL),
        batch_input("/batch/OrderTwo.svelte", SUPPORTED_TWO),
    ];
    let entries = run_batch(
        &inputs,
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );

    assert_eq!(entries.len(), inputs.len(), "one entry per input position");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.canonical_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/batch/OrderOne.svelte",
            "/batch/OrderRefused.svelte",
            "/batch/OrderTwo.svelte"
        ],
        "the batch reordered its results"
    );

    // Each entry's bytes belong to ITS OWN input, proven by EQUALITY against a
    // batch of that input alone — not by looking for a substring in the output.
    // A fanned-out or shifted result fails here, and so does arbitrarily
    // different output.
    for (index, input) in inputs.iter().enumerate() {
        let alone = run_batch(
            std::slice::from_ref(input),
            CompileManyTarget::RuntimeRender {
                profile: render_profile(false, true),
            },
        );
        assert_eq!(
            entries[index].code, alone[0].code,
            "{}: the entry's bytes differ from the same input compiled alone, so a neighbour \
             influenced it",
            input.canonical_id
        );
        assert_eq!(
            entries[index].source_map.is_some(),
            alone[0].source_map.is_some(),
            "{}: map presence differs from the same input compiled alone",
            input.canonical_id
        );
    }
    assert_ne!(
        entries[0].code, entries[2].code,
        "the two distinct inputs produced identical bytes, so this batch cannot detect a \
         fanned-out result"
    );
    // The middle item leaves no residue in its neighbours.
    assert!(
        entries[0].errors.is_empty() && entries[2].errors.is_empty(),
        "a neighbouring item contaminated a sibling: {:?} / {:?}",
        entries[0].errors,
        entries[2].errors
    );
    // And every entry reports the canonical it was asked about.
    for (entry, input) in entries.iter().zip(&inputs) {
        assert_eq!(
            entry.canonical_id, input.canonical_id,
            "an entry is attributed to the wrong input"
        );
    }
}

/// The optional-product axis at the batch boundary: a map is published only
/// when requested, and the axis changes no module byte.
#[test]
fn the_batch_source_map_axis_publishes_only_what_was_requested() {
    let inputs = vec![batch_input("/batch/MapAxis.svelte", SUPPORTED)];
    let with_map = run_batch(
        &inputs,
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );
    let without_map = run_batch(
        &inputs,
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, false),
        },
    );

    assert!(
        with_map[0].source_map.is_some(),
        "the batch withheld a requested source map"
    );
    assert!(
        without_map[0].source_map.is_none(),
        "the batch published a source map that was never requested"
    );
    assert_eq!(
        with_map[0].code, without_map[0].code,
        "the batch's source-map axis changed the emitted module bytes"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Conformance target — the equivalence the batch route must reach
// ══════════════════════════════════════════════════════════════════════════

/// CONFORMANCE TARGET — currently FAILS, deliberately `#[ignore]`d.
///
/// **What is wrong:** `CompileBatchInput` carries no source-language field and
/// the batch's own upsert hardcodes `FileLanguage::vue()`
/// (`crates/verter_session/src/host_compile.rs:465-478`), so every `.svelte`
/// input in a batch is parsed and compiled by the Vue carrier. The batch route
/// therefore publishes Vue-assembled bytes where the single-file route
/// publishes a Svelte client module, and neither Svelte runtime refusal fires.
///
/// **Behaviour this demands:** for each item, the batch result equals the
/// single-file route's result for the same typed request — the same module
/// bytes and map presence for a published item, and the same typed refusal code
/// with NO partial product for a refused one.
///
/// **Acceptance:** un-ignoring this test is the acceptance gate for that
/// correction. This module owns no correction.
#[test]
#[ignore = "conformance target: the batch route compiles .svelte inputs as Vue, so it is not equivalent to the single-file route"]
fn a_svelte_batch_matches_the_single_file_route_item_for_item() {
    let inputs = vec![
        batch_input("/batch/EqOne.svelte", SUPPORTED),
        batch_input("/batch/EqRefused.svelte", ADVANCED_RUNE_REFUSAL),
        batch_input("/batch/EqTwo.svelte", SUPPORTED_TWO),
    ];
    let entries = run_batch(
        &inputs,
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );

    let profile = single_file_profile(false, true);
    let single = host();
    for input in &inputs {
        upsert_svelte(&single, &input.canonical_id, &input.source);
    }

    let mut divergences = Vec::new();
    for (entry, input) in entries.iter().zip(&inputs) {
        match single_file(&single, &input.canonical_id, &profile) {
            SingleFileOutcome::Published { code, has_map } => {
                if entry.code.as_ref() != code.as_str() {
                    divergences.push(format!(
                        "{}: bytes differ\n  batch:  {}\n  single: {}",
                        entry.canonical_id, entry.code, code
                    ));
                }
                if entry.source_map.is_some() != has_map {
                    divergences.push(format!("{}: map presence differs", entry.canonical_id));
                }
            }
            SingleFileOutcome::Refused { diagnostic_code } => {
                if !entry
                    .errors
                    .iter()
                    .any(|error| error.contains(&diagnostic_code))
                {
                    divergences.push(format!(
                        "{}: the single-file route refused with `{diagnostic_code}`, the batch \
                         reported {:?}",
                        entry.canonical_id, entry.errors
                    ));
                }
                if !entry.code.is_empty() || entry.source_map.is_some() {
                    divergences.push(format!(
                        "{}: the batch published a partial product for a refused item",
                        entry.canonical_id
                    ));
                }
            }
            SingleFileOutcome::Missing => divergences.push(format!(
                "{}: the single-file route produced neither a module nor a refusal",
                entry.canonical_id
            )),
        }
    }
    assert!(
        divergences.is_empty(),
        "the batch route diverges from the single-file route for {} item(s):\n\n{}",
        divergences.len(),
        divergences.join("\n\n")
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Per-entry atomicity, on a GENUINE refusal
// ══════════════════════════════════════════════════════════════════════════

/// Does a batch entry publish a product alongside a GENUINE typed failure?
///
/// Getting a genuine failure needed care. The batch selects the VUE carrier for
/// every input, so a Svelte-shaped refusal never fires, and an "error plus
/// product" reading taken from a Vue-shaped SUCCESS would be an artifact of the
/// carrier defect rather than an atomicity finding. Malformed Vue script is not
/// a failure either — the carrier's error recovery still emits a module (a
/// `<script setup>` body of `const a = (((` publishes
/// `const _sfc_main = { … }` with the broken text passed through).
///
/// The failure driven here is the batch's OWN typed one: two inputs naming the
/// same canonical with DIFFERENT sources. `compile_many` diverts that to its
/// per-canonical `group_errors` map and surfaces the error at every original
/// input position for that canonical
/// (`crates/verter_session/src/host_compile.rs:479-485`). That is a genuine
/// batch-level failure on whichever carrier is selected, so what it publishes
/// beside the error is a real atomicity answer.
#[test]
fn a_genuinely_failing_batch_entry_publishes_no_partial_product() {
    let inputs = vec![
        batch_input("/batch/AtomicOk.svelte", SUPPORTED),
        batch_input("/batch/AtomicClash.svelte", SUPPORTED),
        // Same canonical, DIFFERENT source — the conflict the batch refuses.
        batch_input("/batch/AtomicClash.svelte", SUPPORTED_TWO),
    ];
    let entries = run_batch(
        &inputs,
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );
    assert_eq!(entries.len(), 3, "one entry per input position");

    let failing: Vec<&CompileBatchEntry> = entries
        .iter()
        .filter(|entry| entry.canonical_id == "/batch/AtomicClash.svelte")
        .collect();
    assert_eq!(failing.len(), 2, "both conflicting positions are reported");
    for entry in &failing {
        assert!(
            !entry.errors.is_empty(),
            "the conflicting input did not fail, so this test is not measuring a genuine \
             refusal: code={:?}",
            entry.code
        );

        // THE QUESTION: is a product published beside the failure?
        assert!(
            entry.code.is_empty(),
            "a batch entry published {} bytes of code alongside its typed failure {:?}:\n{}",
            entry.code.len(),
            entry.errors,
            entry.code
        );
        assert!(
            entry.source_map.is_none(),
            "a batch entry published a source map alongside its typed failure {:?}",
            entry.errors
        );
        assert!(
            entry.lang.is_none(),
            "a batch entry reported an output language alongside its typed failure {:?}",
            entry.lang
        );
    }

    // The neighbour is untouched: a failing entry neither suppresses a
    // succeeding one nor leaks into it.
    let ok = entries
        .iter()
        .find(|entry| entry.canonical_id == "/batch/AtomicOk.svelte")
        .expect("the succeeding input is present");
    assert!(
        ok.errors.is_empty() && !ok.code.is_empty(),
        "the failing entries contaminated their neighbour: {:?}",
        ok.errors
    );
}
