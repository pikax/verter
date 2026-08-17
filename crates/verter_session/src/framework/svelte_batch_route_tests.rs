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
//! It also owns this route's PER-ENTRY ATOMICITY regression — a table driven
//! over every failing-entry class the public `compile_many` API can genuinely
//! reach (duplicate-canonical conflict, compile failure, other typed host
//! error, caught panic), on both lanes where the class exists on that lane,
//! with ordinary-success and warning-only controls so a diagnostic is never
//! equated with a refusal. Two further classes are recorded as NOT REACHABLE
//! with their source reason rather than represented by a target that would
//! fail for some other reason; see
//! [`a_genuinely_failing_batch_entry_publishes_no_partial_product`].
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

/// This module's half of a MUTUAL, compile-enforced registration.
///
/// The census lives OUTSIDE this module deliberately — a check placed inside a
/// suite is deleted by the same edit that empties it. That leaves the reverse
/// hole: deleting the census too. This test consumes an item the census owns,
/// and the census in turn NAMES this test as an item, so removing EITHER `mod`
/// declaration is a COMPILE error rather than a filter that silently matches
/// nothing and still exits 0.
///
/// The identity the census counts by is this function ITEM, not a path this
/// module writes down: it is passed by reference and the compiler answers with
/// the definition's own path. A suite therefore cannot nominate a module it does
/// not live in, and the census requires a test with exactly that path to be
/// present in the binary's own listing before counting anything under it.
#[test]
pub(crate) fn this_suite_is_registered_with_the_census() {
    assert!(
        super::suite_census::covers(&this_suite_is_registered_with_the_census),
        "{}: the census carries no test for this suite, so this suite's documented invocation \
         could match nothing and still report success",
        super::suite_census::witness_identity(&this_suite_is_registered_with_the_census)
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
// Per-entry atomicity, over every failing-entry class the public batch API
// can genuinely reach
// ══════════════════════════════════════════════════════════════════════════

/// A Vue-carrier source whose TEMPLATE genuinely fails to parse.
///
/// The batch upserts every input as a Vue carrier, so a genuinely failing
/// entry is necessarily a Vue-carrier failure. Malformed SCRIPT is not one:
/// the carrier's error recovery still publishes a module for it (a
/// `<script setup>` body of `const a = (((` emits `const _sfc_main = { … }`
/// with the broken text passed through, no error at all). An unterminated
/// interpolation does fail, and each entry PROVES which construction produced
/// it — see [`assert_entry_entered_class`].
const TEMPLATE_THAT_FAILS_TO_PARSE: &str =
    "<template>\n  <div>{{ x }} {{ unclosed\n  <span>{{ also-unclosed\n</template>";

/// A Vue-carrier source that compiles SUCCESSFULLY while carrying a
/// non-error diagnostic: the member-position macro type `Missing` cannot be
/// resolved, so that member's runtime type degrades to `null` and the
/// carrier records a WARNING. This is the control that keeps a diagnostic
/// from being read as a refusal.
const WARNS_WITHOUT_FAILING: &str = "<script setup lang=\"ts\">\nimport type { Missing } from './nope'\ndefineProps<{ foo: Missing }>()\n</script>\n<template><div>{{ foo }}</div></template>\n";

/// Which public `compile_many` lane a row runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    RuntimeRender,
    HostBacked,
}

impl Lane {
    /// This lane's ordinary target — the one every row uses unless it is
    /// specifically driving a profile axis.
    fn target(self) -> CompileManyTarget {
        match self {
            Lane::RuntimeRender => CompileManyTarget::RuntimeRender {
                profile: render_profile(false, true),
            },
            Lane::HostBacked => CompileManyTarget::HostBacked,
        }
    }
}

/// A failing-entry class, named by the construction that produces it.
///
/// These are the classes a batch entry can genuinely enter through the
/// PUBLIC `compile_many` API. Two more exist in the source and are NOT
/// reachable from it at all, and one of the classes below —
/// [`FailingClass::OtherHostError`] — is reachable on only ONE of the two
/// lanes. All three unreachable facts are recorded on
/// [`a_genuinely_failing_batch_entry_publishes_no_partial_product`] rather
/// than represented here by a target that would fail for some other reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailingClass {
    /// Two inputs naming the same canonical with different sources. The
    /// batch diverts the whole group to its per-canonical error map and
    /// surfaces the error at EVERY original input position for that
    /// canonical.
    DuplicateCanonicalConflict,
    /// The compile itself failed: the lane's `CompileError` arm, which
    /// unpacks every error-severity diagnostic and prefixes each with the
    /// canonical id.
    CompileFailure,
    /// Any other typed host error from the lane's read — driven here as a
    /// grammar mismatch between the requested profile and the registered
    /// carrier grammar.
    OtherHostError,
    /// The worker panicked and the batch coordinator's catch boundary
    /// rendered the panic into this entry.
    Panic,
}

/// One driven row: the entries that must be atomic, and the entries that
/// must be untouched by them.
struct DrivenRow {
    /// Every entry that must have entered the row's failing class.
    failing: Vec<CompileBatchEntry>,
    /// Entries that must publish cleanly. For a PER-ENTRY class these are
    /// the failing entry's own neighbours in the SAME batch. For a
    /// BATCH-LEVEL class (see [`FailingClass::OtherHostError`]) no
    /// neighbour can survive, so these are the identical inputs run under
    /// the lane's ordinary profile — which proves the failure is the axis
    /// under test and not a property of the inputs.
    unaffected: Vec<CompileBatchEntry>,
}

/// Split a batch's entries into the ones for `failing_canonical` and the
/// rest.
fn partition_row(entries: Vec<CompileBatchEntry>, failing_canonical: &str) -> DrivenRow {
    let (failing, unaffected) = entries
        .into_iter()
        .partition(|entry| entry.canonical_id == failing_canonical);
    DrivenRow {
        failing,
        unaffected,
    }
}

/// Drive one `(class, lane)` row through the PUBLIC `compile_many` API.
fn drive_failing_class(class: FailingClass, lane: Lane) -> DrivenRow {
    let tag = format!("{class:?}{lane:?}");
    match class {
        FailingClass::DuplicateCanonicalConflict => {
            let clash = format!("/atomic/{tag}Clash.svelte");
            partition_row(
                host().compile_many(
                    vec![
                        batch_input(&format!("/atomic/{tag}Neighbour.svelte"), SUPPORTED),
                        batch_input(&clash, SUPPORTED),
                        // Same canonical, DIFFERENT source — the conflict.
                        batch_input(&clash, SUPPORTED_TWO),
                    ],
                    CompileBatchOptions::default(),
                    lane.target(),
                ),
                &clash,
            )
        }
        FailingClass::CompileFailure => {
            let broken = format!("/atomic/{tag}Broken.vue");
            partition_row(
                host().compile_many(
                    vec![
                        batch_input(&format!("/atomic/{tag}Neighbour.svelte"), SUPPORTED),
                        batch_input(&broken, TEMPLATE_THAT_FAILS_TO_PARSE),
                        batch_input(&format!("/atomic/{tag}NeighbourTwo.svelte"), SUPPORTED_TWO),
                    ],
                    CompileBatchOptions::default(),
                    lane.target(),
                ),
                &broken,
            )
        }
        FailingClass::OtherHostError => {
            // The grammar axis rides on the BATCH profile, so it is not a
            // per-entry property: every entry in the batch enters the class
            // and no neighbour can survive it. The `unaffected` half is
            // therefore the SAME inputs under the lane's ordinary profile,
            // which is what makes this a measurement of the axis rather than
            // of the inputs.
            assert_eq!(
                lane,
                Lane::RuntimeRender,
                "the grammar axis is only caller-settable on the render lane"
            );
            let mut mismatched = render_profile(false, true);
            mismatched.delimiters = Some(("[[".to_string(), "]]".to_string()));
            let inputs = vec![
                batch_input(&format!("/atomic/{tag}One.svelte"), SUPPORTED),
                batch_input(&format!("/atomic/{tag}Two.svelte"), SUPPORTED_TWO),
            ];
            DrivenRow {
                failing: host().compile_many(
                    inputs.clone(),
                    CompileBatchOptions::default(),
                    CompileManyTarget::RuntimeRender {
                        profile: mismatched,
                    },
                ),
                unaffected: host().compile_many(
                    inputs,
                    CompileBatchOptions::default(),
                    lane.target(),
                ),
            }
        }
        FailingClass::Panic => {
            // The sentinel is a `#[cfg(test)]` branch that ALREADY exists in
            // the batch worker, so the panic unwinds through the production
            // catch boundary exactly like a codegen panic. Nothing is added
            // to the production module to drive this row.
            let sentinel = crate::host_compile::PANIC_INJECT_SENTINEL;
            partition_row(
                host().compile_many(
                    vec![
                        batch_input(&format!("/atomic/{tag}Neighbour.svelte"), SUPPORTED),
                        batch_input(sentinel, SUPPORTED),
                        batch_input(&format!("/atomic/{tag}NeighbourTwo.svelte"), SUPPORTED_TWO),
                    ],
                    CompileBatchOptions::default(),
                    lane.target(),
                ),
                sentinel,
            )
        }
    }
}

/// Prove this entry genuinely entered the class the row claims.
///
/// Each arm asserts something only THAT construction produces, never merely
/// that `errors` is non-empty — otherwise a row would pass on any failure
/// whatsoever, including one from a class it is not measuring.
#[track_caller]
fn assert_entry_entered_class(class: FailingClass, lane: Lane, entry: &CompileBatchEntry) {
    let id = &entry.canonical_id;
    let row = format!("{class:?}/{lane:?}");
    assert!(
        !entry.errors.is_empty(),
        "{row}: `{id}` reported no error at all, so this row is not measuring a failing entry"
    );
    match class {
        FailingClass::DuplicateCanonicalConflict => {
            assert_eq!(
                entry.errors,
                vec!["duplicate canonical_id with conflicting source in batch".to_string()],
                "{row}: `{id}` did not fail with the batch's own per-canonical conflict error, so \
                 it entered some other class"
            );
        }
        FailingClass::CompileFailure => {
            // The DISCRIMINATOR between the two arms that can carry error
            // text: the failing-compile arm prefixes every message with the
            // canonical id, while the SUCCESSFUL-response arm surfaces the
            // response's own error-severity diagnostics verbatim — and that
            // arm publishes its product alongside them. An unprefixed
            // message here would mean this row is measuring the successful
            // arm, where "no product" is not the property under test.
            let prefix = format!("[{id}] ");
            for message in &entry.errors {
                assert!(
                    message.starts_with(&prefix),
                    "{row}: `{id}` carries an error that is not prefixed with its canonical id, so \
                     it came from the successful-response construction rather than the \
                     failing-compile one: {message:?}"
                );
            }
            assert!(
                entry
                    .errors
                    .iter()
                    .any(|message| message.contains("Interpolation end sign was not found.")),
                "{row}: `{id}` failed, but not with the template-parse diagnostic this row drives: \
                 {:?}",
                entry.errors
            );
        }
        FailingClass::OtherHostError => {
            assert_eq!(
                entry.errors,
                vec![format!(
                    "[{id}] host error: compile profile grammar differs from registered grammar"
                )],
                "{row}: `{id}` did not fail with the typed grammar-mismatch host error this row \
                 drives"
            );
        }
        FailingClass::Panic => {
            assert_eq!(
                entry.errors.len(),
                1,
                "{row}: a caught panic renders exactly one error: {:?}",
                entry.errors
            );
            assert!(
                entry.errors[0].starts_with(&format!("[{id}] compiler panic: ")),
                "{row}: `{id}` failed, but not through the coordinator's panic conversion: {:?}",
                entry.errors
            );
            assert!(
                entry.errors[0].contains("synthetic panic"),
                "{row}: the panic body is not the injected one, so some OTHER panic produced this \
                 entry: {:?}",
                entry.errors
            );
        }
    }
}

/// THE QUESTION: does a failing entry publish a product beside its failure?
#[track_caller]
fn assert_publishes_no_product(row: &str, entry: &CompileBatchEntry) {
    let id = &entry.canonical_id;
    assert!(
        entry.code.is_empty(),
        "{row}: `{id}` published {} bytes of code alongside its failure {:?}:\n{}",
        entry.code.len(),
        entry.errors,
        entry.code
    );
    assert!(
        entry.source_map.is_none(),
        "{row}: `{id}` published a source map alongside its failure {:?}",
        entry.errors
    );
    assert!(
        entry.lang.is_none(),
        "{row}: `{id}` reported an output language ({:?}) alongside its failure {:?}",
        entry.lang,
        entry.errors
    );
}

/// The other half, and the control that keeps "no product beside a failure"
/// from being satisfied by a route that withholds every product: this entry
/// must report nothing and publish its module.
#[track_caller]
fn assert_publishes_cleanly(row: &str, entry: &CompileBatchEntry) {
    let id = &entry.canonical_id;
    assert!(
        entry.errors.is_empty(),
        "{row}: `{id}` should have published cleanly but reported {:?}",
        entry.errors
    );
    assert!(
        !entry.code.is_empty(),
        "{row}: `{id}` reported no failure but published no code either"
    );
    assert!(
        entry.lang.is_some(),
        "{row}: `{id}` published code but no output language"
    );
}

/// Every `(class, lane)` this suite drives, with the number of ENTRIES that
/// must enter the class. The count is part of the measurement: the conflict
/// class fans its error out to every original input position for that
/// canonical, and the grammar axis is batch-level, so both report more than
/// one failing entry.
const ATOMICITY_ROWS: &[(FailingClass, Lane, usize)] = &[
    (
        FailingClass::DuplicateCanonicalConflict,
        Lane::RuntimeRender,
        2,
    ),
    (
        FailingClass::DuplicateCanonicalConflict,
        Lane::HostBacked,
        2,
    ),
    (FailingClass::CompileFailure, Lane::RuntimeRender, 1),
    (FailingClass::CompileFailure, Lane::HostBacked, 1),
    (FailingClass::OtherHostError, Lane::RuntimeRender, 2),
    (FailingClass::Panic, Lane::RuntimeRender, 1),
    (FailingClass::Panic, Lane::HostBacked, 1),
];

/// Does a batch entry ever publish a product alongside a genuine failure?
///
/// Driven over EVERY failing-entry class the public `compile_many` API can
/// reach, on both lanes where the class exists on that lane. For each row
/// the entry is first proven to have entered its intended class — the
/// conflict message, the canonical-prefixed compile diagnostic, the typed
/// grammar host error, the panic marker — and only then asked whether it
/// published code, a source map, or an output language.
///
/// Getting genuine failures needed care. The batch selects the VUE carrier
/// for every input, so a Svelte-shaped refusal never fires, and an "error
/// plus product" reading taken from a Vue-shaped SUCCESS would be an
/// artifact of the carrier defect rather than an atomicity answer. Malformed
/// Vue script is not a failure either — the carrier's error recovery still
/// emits a module for it.
///
/// **Recorded as NOT REACHABLE through this API, with their source reason**
/// — two whole classes, plus one lane of a class the table above does drive.
/// None is represented by an `#[ignore]`d target, because such a target
/// would fail for a reason other than the property it names:
///
/// - **Upsert failure.** It folds into the same per-canonical error map as
///   the conflict above and short-circuits the per-input worker, but the
///   upsert engine only yields an error from a scheduler `Failed` /
///   `Superseded` / `Shutdown` completion state or a post-commit
///   generation-fence mismatch (`crates/verter_session/src/host_upsert.rs`
///   `map_states` / `finish_upsert_post_commit`). `compile_many` exposes no
///   input that produces any of them: it deduplicates by canonical before
///   submitting, and the only in-tree driver of those states is a test-only
///   completion-state seam that bypasses the batch entirely. Both
///   constructions the class would reach hardcode an empty code, map and
///   language, exactly like the conflict row above.
/// - **A typed Svelte runtime refusal.** It cannot reach the batch at all:
///   `crates/verter_session/src/host_compile.rs:469-478` hardcodes
///   `file_language: FileLanguage::vue()` for every batch input, and the
///   render lane never reads the runtime-surface-refused flag. That carrier
///   defect is characterized separately in this module.
/// - **`OtherHostError` on the HOST-BACKED lane.** The class itself IS
///   driven, on the render lane. The grammar axis — the one caller-settable
///   route into `Err(other)` — rides on the compile profile, and the
///   host-backed lane's profile is the fixed bundler preset that
///   `compile_many` never lets a caller vary, so that lane has no input
///   that reaches the class. `drive_failing_class` asserts the restriction
///   rather than leaving it to this prose.
#[test]
fn a_genuinely_failing_batch_entry_publishes_no_partial_product() {
    for (class, lane, expected_failing) in ATOMICITY_ROWS {
        let row_name = format!("{class:?}/{lane:?}");
        let row = drive_failing_class(*class, *lane);
        assert_eq!(
            row.failing.len(),
            *expected_failing,
            "{row_name}: expected {expected_failing} entr(ies) in this class, drove {}",
            row.failing.len()
        );
        assert!(
            !row.unaffected.is_empty(),
            "{row_name}: the row drove no entry that must survive, so it cannot show a failing \
             entry leaves its neighbours alone"
        );
        for entry in &row.failing {
            assert_entry_entered_class(*class, *lane, entry);
            assert_publishes_no_product(&row_name, entry);
        }
        for entry in &row.unaffected {
            assert_publishes_cleanly(&row_name, entry);
        }
    }
}

/// CONTROLS — a diagnostic is not a refusal.
///
/// Without these the atomicity table above would be satisfied by a route
/// that withheld every product, or that folded a warning into `errors`. Two
/// rows on each lane: an ordinary SUCCESS, and a compile that succeeds while
/// carrying a non-error diagnostic. Both must publish their product and
/// report NO errors.
///
/// The warning is measured differently per lane, because the two lanes
/// surface it differently by construction: the render lane carries a
/// successful compile's non-error diagnostics on the entry itself, while the
/// host-backed lane leaves that list empty and rides its warnings on the
/// response. So the host-backed half reads the warning off the response for
/// the SAME canonical the batch just compiled — proving the source really is
/// warning-carrying on that lane, and that the batch entry nonetheless
/// reported no error and published its module.
#[test]
fn an_ordinary_success_and_a_warning_only_compile_are_never_read_as_failures() {
    for lane in [Lane::RuntimeRender, Lane::HostBacked] {
        let row = format!("control/{lane:?}");
        let success_id = format!("/atomic/Control{lane:?}Success.svelte");
        let warning_id = format!("/atomic/Control{lane:?}Warns.vue");
        let batch_host = host();
        let entries = batch_host.compile_many(
            vec![
                batch_input(&success_id, SUPPORTED),
                batch_input(&warning_id, WARNS_WITHOUT_FAILING),
            ],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_eq!(entries.len(), 2, "{row}: one entry per input");

        // (a) ordinary success — product published, nothing reported.
        assert_publishes_cleanly(&row, &entries[0]);
        assert!(
            entries[0].diagnostics.is_empty(),
            "{row}: an ordinary success carries no diagnostics: {:?}",
            entries[0].diagnostics
        );

        // (b) warning-only — the compile SUCCEEDS while carrying a
        //     non-error diagnostic, and still publishes.
        assert_publishes_cleanly(&row, &entries[1]);
        match lane {
            Lane::RuntimeRender => {
                let warning = entries[1]
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code == "XUnresolvedImportedMacroType")
                    .unwrap_or_else(|| {
                        panic!(
                            "{row}: the warning-only input carried no unresolved-macro-type \
                             diagnostic, so this control is not measuring a warning: {:?}",
                            entries[1]
                                .diagnostics
                                .iter()
                                .map(|diagnostic| (
                                    diagnostic.code.as_str(),
                                    &diagnostic.severity,
                                    diagnostic.message.as_str()
                                ))
                                .collect::<Vec<_>>()
                        )
                    });
                assert_eq!(
                    warning.severity,
                    crate::HostSeverity::Warning,
                    "{row}: the degraded member is a warning, never fatal"
                );
            }
            Lane::HostBacked => {
                assert!(
                    entries[1].diagnostics.is_empty(),
                    "{row}: the host-backed entry surfaces no success-warning list by \
                     construction, so a non-empty one means the lane changed: {:?}",
                    entries[1].diagnostics
                );
                // The source really IS warning-carrying on this lane: read
                // the same canonical back off the response the batch used.
                let response = batch_host
                    .get_virtual_file(VirtualQuery {
                        raw_id: None,
                        canonical_id: Some(warning_id.clone()),
                        node_kind: Some(VirtualNodeKind::Main),
                        compile_profile: crate::host_compile::compile_profile_for_bundler(),
                    })
                    .unwrap_or_else(|error| {
                        panic!("{row}: the warning-only input stopped publishing: {error:?}")
                    });
                assert!(
                    response
                        .diagnostics
                        .diagnostics
                        .iter()
                        .any(
                            |diagnostic| diagnostic.severity == crate::HostSeverity::Warning
                                && diagnostic.code == "XUnresolvedImportedMacroType"
                        ),
                    "{row}: the response carries no warning, so this control is not measuring a \
                     warning-only compile: {:?}",
                    response
                        .diagnostics
                        .diagnostics
                        .iter()
                        .map(|diagnostic| (
                            diagnostic.code.as_str(),
                            &diagnostic.severity,
                            diagnostic.message.as_str()
                        ))
                        .collect::<Vec<_>>()
                );
                assert!(
                    !response
                        .diagnostics
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.severity == crate::HostSeverity::Error),
                    "{row}: the warning-only control now carries an ERROR, so it is no longer \
                     warning-only"
                );
            }
        }
    }
}

/// The latent host-backed construction hazard's PRECONDITION, characterized.
///
/// This is the artifact for a finding that is NOT a demonstrated defect. Of
/// the nine `CompileBatchEntry` constructions on the batch route, eight write
/// a hardcoded empty product beside their errors; the host-backed
/// SUCCESSFUL-RESPONSE construction is the only one that reads the product and
/// the error list from the same response. That shape could express a product
/// beside a fatal-looking error list — but only if it is ever HANDED a
/// successful response carrying an error-severity diagnostic. That precondition
/// is what this test measures, and it is the half a public-API test can decide.
///
/// Two readings, on the host-backed lane:
///
/// 1. A compile that SUCCEEDS while carrying a diagnostic is served by that
///    construction from a response that carries NO error-severity diagnostic.
///    The response's diagnostics list is non-empty (it carries a warning), so
///    "no error" is a real reading and not a vacuous one over an empty list.
/// 2. A compile that genuinely FAILS never reaches that construction at all:
///    the same request answers `Err`, so its entry is built by one of the
///    hardcoded-empty arms, and it publishes nothing.
///
/// **What this deliberately does NOT prove.** It does not prove the
/// construction READS its error list from the response rather than writing a
/// hardcoded empty one — replacing that filter with an empty literal leaves
/// this test green, and it is written not to claim otherwise. Deciding that
/// half needs a synthetic response carrying a product and an error together,
/// which needs a seam in production code that this suite does not own. It is
/// recorded as an open residue for the correction owner, not smuggled in as a
/// claim here.
///
/// It is `#[ignore]`d deliberately, and it is NOT a RED target: it passes
/// today, by design. It is the re-examination artifact its correction owner
/// re-runs — the day a successful host-backed response carries an
/// error-severity diagnostic, reading 1 turns RED, the precondition holds, and
/// the hazard has become reachable. Making it a required-RED gate today would
/// mean asserting a defect nobody has reproduced.
///
/// That combination — `#[ignore]`d AND passing — is a deliberate category
/// mismatch with every other ignored target in this suite, which states a
/// correct behaviour the product does not yet have and therefore FAILS. This
/// one is the named artifact for a finding that is not a defect, so it has no
/// failing correct-behaviour to state. Nothing is lost by ignoring it: the same
/// precondition is asserted LIVE, on the same lane, by the non-ignored
/// [`an_ordinary_success_and_a_warning_only_compile_are_never_read_as_failures`],
/// whose host-backed half already requires the response to carry no
/// error-severity diagnostic. This test exists so that requirement has a name
/// the amended finding can point at.
#[ignore]
#[test]
fn the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error() {
    let row = "latent-hazard/HostBacked";
    let warning_id = "/atomic/LatentHazardWarns.vue".to_string();
    let failing_id = "/atomic/LatentHazardFails.vue".to_string();
    let batch_host = host();
    let entries = batch_host.compile_many(
        vec![
            batch_input(&warning_id, WARNS_WITHOUT_FAILING),
            batch_input(&failing_id, TEMPLATE_THAT_FAILS_TO_PARSE),
        ],
        CompileBatchOptions::default(),
        Lane::HostBacked.target(),
    );
    assert_eq!(entries.len(), 2, "{row}: one entry per input");

    // 1. The SUCCESS half — served by the response-reading construction. The
    //    PRECONDITION is read first, so a failure names it rather than naming
    //    the pairing it causes one line later.
    let response = batch_host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(warning_id.clone()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: crate::host_compile::compile_profile_for_bundler(),
        })
        .unwrap_or_else(|error| {
            panic!(
                "{row}: the diagnostic-carrying input must SUCCEED here, or this test is \
                    measuring a failure arm instead of the response-reading construction: \
                    {error:?}"
            )
        });
    assert!(
        !response.code.is_empty(),
        "{row}: the successful response published no product, so the construction under \
         characterization had nothing to pair"
    );
    assert!(
        response
            .diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::HostSeverity::Warning),
        "{row}: the response carries no diagnostic at all, so \"it carries no ERROR\" would be a \
         vacuous reading over an empty list rather than a measurement"
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::HostSeverity::Error),
        "{row}: a SUCCESSFUL host-backed response now carries an error-severity diagnostic — the \
         latent construction hazard is reachable, and this entry pairs that error list with the \
         response's product: {:?}",
        response
            .diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code.as_str(),
                &diagnostic.severity,
                diagnostic.message.as_str()
            ))
            .collect::<Vec<_>>()
    );
    // The precondition holds, so the entry this construction built is clean:
    // a product, and no errors beside it.
    assert_publishes_cleanly(row, &entries[0]);

    // 2. The FAILURE half — never reaches that construction.
    assert_publishes_no_product(row, &entries[1]);
    let failure = batch_host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(failing_id.clone()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: crate::host_compile::compile_profile_for_bundler(),
    });
    assert!(
        failure.is_err(),
        "{row}: the failing input answered Ok, so its entry WAS built by the response-reading \
         construction and the hazard is reachable through an ordinary compile failure"
    );
}

/// A SEARCH for one specific shape, not a proof that it cannot exist.
///
/// The host-backed lane's successful-response construction is the ONLY one
/// that reads a product and an error list independently, so it is the only
/// place a batch entry could express both at once. The known upstream way to
/// produce a successful response carrying error diagnostics is the dev
/// last-known-good serve, which pairs a PREVIOUS compile's outputs with a NEW
/// compile's error diagnostics. Reaching it needs a compile of UNCHANGED
/// bytes to newly fail while the last-good slot still holds that file's
/// previous product.
///
/// This drives, through the PUBLIC API only, the sequences most likely to
/// reach it: a zero-fact self-contained component compiled to populate the
/// last-good slot, then store-view-advancing operations that do NOT edit that
/// file's bytes, then a re-request; and the same file recompiled into a
/// genuine failure, with and without unrelated generations in between. In
/// every case the resulting entry must be atomic — never an error list
/// together with a product.
///
/// **What this does NOT do:** it does not prove the shape is unreachable. It
/// searches the sequences a public consumer can express and reports that none
/// of them produced it. A residual remains open in the source — the last-good
/// peek skips its validator when the fact rail is empty, and a self-contained
/// component records no facts — and this test is what would turn RED if some
/// change made that crack reachable from the public API.
#[test]
fn searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none() {
    /// The shared invariant: an entry that reports a failure publishes
    /// nothing, whatever produced it.
    #[track_caller]
    fn assert_atomic(step: &str, entry: &CompileBatchEntry) {
        if entry.errors.is_empty() {
            return;
        }
        assert!(
            entry.code.is_empty() && entry.source_map.is_none() && entry.lang.is_none(),
            "{step}: `{}` served a product alongside {} error(s) — the mixed outcome this test \
             searches for. errors={:?} lang={:?} map={} code:\n{}",
            entry.canonical_id,
            entry.errors.len(),
            entry.errors,
            entry.lang,
            entry.source_map.is_some(),
            entry.code
        );
    }

    /// A self-contained component: no import, no cross-file type, so its
    /// compile records no dependency facts. That is precisely the input for
    /// which the last-good peek skips its validator.
    const ZERO_FACT: &str = "<template><div class=\"zero\">{{ 1 + 1 }}</div></template>";

    fn advance_the_store_view(host: &VerterHost, seed: usize) {
        let canonical = format!("/probe/Unrelated{seed}.vue");
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(ZERO_FACT),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap_or_else(|error| panic!("upsert {canonical}: {error:?}"));
    }

    for lane in [Lane::RuntimeRender, Lane::HostBacked] {
        // ── (1) populate the last-good slot, then make the SAME canonical
        //        fail. The failing entry must not inherit the product the
        //        successful compile just published.
        let canonical = format!("/probe/Stale{lane:?}.vue");
        let single = host();
        let good = single.compile_many(
            vec![batch_input(&canonical, ZERO_FACT)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_publishes_cleanly(&format!("{lane:?}/populate-last-good"), &good[0]);

        let failed = single.compile_many(
            vec![
                batch_input(&canonical, TEMPLATE_THAT_FAILS_TO_PARSE),
                // A sibling that is untouched, so a stale product could also
                // arrive by contamination rather than by the last-good read.
                batch_input(&format!("/probe/Sibling{lane:?}.vue"), ZERO_FACT),
            ],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert!(
            !failed[0].errors.is_empty(),
            "{lane:?}: the recompile did not fail, so this sequence never reached the state it \
             searches: {:?}",
            failed[0].code
        );
        assert_atomic(&format!("{lane:?}/fail-after-last-good"), &failed[0]);
        assert_publishes_cleanly(
            &format!("{lane:?}/fail-after-last-good sibling"),
            &failed[1],
        );

        // ── (2) the same, with unrelated generations landing in between, so
        //        the failing request runs against an ADVANCED store view.
        let advanced = host();
        let advanced_id = format!("/probe/Advanced{lane:?}.vue");
        let good = advanced.compile_many(
            vec![batch_input(&advanced_id, ZERO_FACT)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_publishes_cleanly(&format!("{lane:?}/advanced-populate"), &good[0]);
        for seed in 0..3 {
            advance_the_store_view(&advanced, seed);
        }
        let failed = advanced.compile_many(
            vec![batch_input(&advanced_id, TEMPLATE_THAT_FAILS_TO_PARSE)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert!(
            !failed[0].errors.is_empty(),
            "{lane:?}: the advanced-view recompile did not fail, so this sequence never reached \
             the state it searches"
        );
        assert_atomic(&format!("{lane:?}/fail-after-advanced-view"), &failed[0]);

        // ── (3) a re-request of UNCHANGED bytes after the store view moved:
        //        the read that consults the cached slot. It must serve its
        //        product with no error at all.
        let warm = host();
        let warm_id = format!("/probe/Warm{lane:?}.vue");
        let cold = warm.compile_many(
            vec![batch_input(&warm_id, ZERO_FACT)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_publishes_cleanly(&format!("{lane:?}/warm-populate"), &cold[0]);
        advance_the_store_view(&warm, 7);
        let reread = warm.compile_many(
            vec![batch_input(&warm_id, ZERO_FACT)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_atomic(&format!("{lane:?}/reread-unchanged"), &reread[0]);
        assert_publishes_cleanly(&format!("{lane:?}/reread-unchanged"), &reread[0]);
        assert_eq!(
            reread[0].code, cold[0].code,
            "{lane:?}: the unchanged re-request served different bytes than the compile that \
             populated the slot"
        );

        // ── (4) fail, then recover: the recovered request must publish a
        //        FRESH product with no residue of the failure.
        let cycle = host();
        let cycle_id = format!("/probe/Cycle{lane:?}.vue");
        let first = cycle.compile_many(
            vec![batch_input(&cycle_id, ZERO_FACT)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_publishes_cleanly(&format!("{lane:?}/cycle-first"), &first[0]);
        let broken = cycle.compile_many(
            vec![batch_input(&cycle_id, TEMPLATE_THAT_FAILS_TO_PARSE)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert!(
            !broken[0].errors.is_empty(),
            "{lane:?}: the cycle's middle request did not fail"
        );
        assert_atomic(&format!("{lane:?}/cycle-broken"), &broken[0]);
        let recovered = cycle.compile_many(
            vec![batch_input(&cycle_id, ZERO_FACT)],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert_atomic(&format!("{lane:?}/cycle-recovered"), &recovered[0]);
        assert_publishes_cleanly(&format!("{lane:?}/cycle-recovered"), &recovered[0]);
        assert_eq!(
            recovered[0].code, first[0].code,
            "{lane:?}: the recovered request served different bytes than the first successful one"
        );
    }
}
