//! Svelte batch route, executed.
//!
//! `compile_many` is a distinct capability from `get_virtual_file`. This
//! drives real Svelte inputs through the batch and compares each item
//! to the single-file route. Language comes from each input's canonical
//! id — not a caller field — so `.svelte` reaches the Svelte carrier.
//! `RuntimeRender` is the public request spelling; for Svelte its effective
//! route is the existing host-backed `get_virtual_file` path.
//!
//! Asserts input order, per-item independence, optional-product
//! (source-map), and per-entry atomicity
//! ([`a_genuinely_failing_batch_entry_publishes_no_partial_product`]).
//!
//! `cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1`
//!
//! Read the `running N tests` line, never the exit code.

use std::sync::Arc;

use crate::host_compile::{
    CompileBatchEntry, CompileBatchInput, CompileBatchOptions, CompileBatchRenderProfile,
    CompileManyTarget,
};
use crate::{
    CompileProfile, HostConfig, HostError, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

/// Mutual, compile-enforced census registration.
///
/// The census lives outside this module so emptying the suite cannot
/// delete the check. This test consumes a census-owned item and the
/// census names this function, so removing either `mod` is a compile
/// error — not a filter that matches nothing and still exits 0.
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

/// A component whose runtime surface the Svelte client backend refuses: an
/// instance-script prop WRITE, which official lowers through the prop SETTER
/// and this backend does not emit. An instance-script prop READ is a SUPPORTED
/// surface, so a read-only component is no longer a refusal witness.
const ADVANCED_RUNE_REFUSAL: &str = "<script>\n  let { count = 0 } = $props();\n  function inc() { count += 1; }\n</script>\n\n<button onclick={inc}>{count}</button>\n";

/// A Vue carrier, for the batches that need a NON-Svelte input beside a Svelte
/// one: a route that derives each input's language per path passes, while one
/// that simply swapped a fixed Vue carrier for a fixed Svelte carrier does not.
const VUE_SOURCE: &str =
    "<script setup>\nconst label = 'hi'\n</script>\n\n<template><button>{{ label }}</button></template>\n";

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
        style_processing: verter_compiler::compile_request::RuntimeStyleProcessing::Complete,
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

/// Single-file bytes for the same source under an explicit language.
/// Language is passed because the control must name a carrier the batch
/// would not derive.
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

/// Adapter id the host registered for `canonical`, from the file's
/// `FileLanguage` row — the same field dispatch reads, not generated bytes.
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

// The batch route's carrier selection

/// A `.svelte` batch input is registered under Svelte, and a `.vue` input
/// in the same batch under Vue. Language is derived from the canonical id
/// (`CompileBatchInput` has no language field). Fails if the batch uses one
/// fixed carrier, or if the single-file route stops agreeing.
#[test]
fn a_svelte_batch_input_is_registered_under_the_svelte_carrier() {
    let inputs = vec![
        batch_input("/batch/One.svelte", SUPPORTED),
        // A Vue carrier in the SAME batch. Without it, a batch that had simply
        // swapped one fixed carrier for another would pass everything below.
        batch_input("/batch/One.vue", VUE_SOURCE),
    ];
    let batch_host = host();
    let entries = batch_host.compile_many(
        inputs.clone(),
        CompileBatchOptions::default(),
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );
    for entry in &entries {
        assert!(
            entry.errors().is_empty(),
            "{}: the batch reported errors for a component the single-file route publishes: {:?}",
            entry.canonical_id,
            entry.errors()
        );
    }

    // Registered adapter per canonical — language is derived per path.
    assert_eq!(
        registered_adapter_id(&batch_host, "/batch/One.svelte").as_deref(),
        Some("svelte"),
        "the batch did not register `/batch/One.svelte` under the Svelte adapter"
    );
    assert_eq!(
        registered_adapter_id(&batch_host, "/batch/One.vue").as_deref(),
        Some("vue"),
        "the batch stopped registering a `.vue` input under the Vue adapter"
    );

    // The single-file route, same bytes, same axes.
    let single = host();
    upsert_svelte(&single, "/batch/One.svelte", SUPPORTED);
    let SingleFileOutcome::Published { code, .. } = single_file(
        &single,
        "/batch/One.svelte",
        &single_file_profile(false, true),
    ) else {
        panic!("the single-file route stopped publishing this component");
    };
    assert_eq!(
        registered_adapter_id(&single, "/batch/One.svelte").as_deref(),
        Some("svelte"),
        "the single-file route no longer registers this component under the Svelte adapter"
    );
    assert_eq!(
        entries[0].code(),
        code.as_str(),
        "the batch and single-file routes publish different bytes for the same Svelte source"
    );

    // Negative: batch bytes must not match a Vue-registered single-file
    // compile of the same `.svelte` source.
    let vue_registered = single_file_reference_under(
        "/batch/One.svelte",
        SUPPORTED,
        verter_language::FileLanguage::vue(),
        &single_file_profile(false, true),
    );
    assert_ne!(
        vue_registered,
        SingleFileOutcome::Published {
            code: entries[0].code().to_string(),
            has_map: entries[0].source_map().is_some(),
        },
        "the batch is publishing what the VUE-registered single-file route produces for a \
         `.svelte` source"
    );
}

/// Svelte runtime refusals fire on the batch with the same typed code as
/// the single-file route and no product beside them (advanced-rune and
/// `generate: "server"`).
#[test]
fn the_svelte_runtime_refusals_fire_on_the_batch_route() {
    // (a) the advanced-rune refusal.
    let advanced = run_batch(
        &[batch_input("/batch/Refused.svelte", ADVANCED_RUNE_REFUSAL)],
        CompileManyTarget::RuntimeRender {
            profile: render_profile(false, true),
        },
    );
    assert!(
        advanced[0]
            .errors()
            .iter()
            .any(|error| error.contains("svelte-runtime-unsupported-advanced-rune")),
        "the batch did not surface the advanced-rune refusal: {:?}",
        advanced[0].errors()
    );
    // The refusal is atomic: no product travels beside it.
    assert_publishes_no_product("advanced-rune refusal", &advanced[0]);
    // Same typed code as the single-file route, not the batch's own error text.
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
        server[0]
            .errors()
            .iter()
            .any(|error| error.contains("svelte-runtime-unsupported-server-generate")),
        "the batch did not surface the server-generate refusal: {:?}",
        server[0].errors()
    );
    assert_publishes_no_product("server-generate refusal", &server[0]);
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

    // Server profile refuses Svelte, not every carrier: Vue still publishes.
    let vue_server = run_batch(
        &[batch_input("/batch/ServerVue.vue", VUE_SOURCE)],
        CompileManyTarget::RuntimeRender {
            profile: render_profile(true, true),
        },
    );
    assert!(
        vue_server[0].errors().is_empty(),
        "the server profile now refuses a Vue carrier too, so the Svelte refusals above are not \
         carrier-derived: {:?}",
        vue_server[0].errors()
    );
    assert!(
        !vue_server[0].code().is_empty(),
        "the server profile published no module for a Vue carrier"
    );
}

/// Same language derivation on the host-backed lane (IDE / analysis / TSC).
#[test]
fn the_host_backed_batch_lane_derives_the_svelte_language_too() {
    let batch_host = host();
    let entries = batch_host.compile_many(
        vec![
            batch_input("/batch/HostOne.svelte", SUPPORTED),
            batch_input("/batch/HostRefused.svelte", ADVANCED_RUNE_REFUSAL),
            batch_input("/batch/HostVue.vue", VUE_SOURCE),
        ],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    assert_eq!(entries.len(), 3, "one entry per input");
    assert_eq!(
        registered_adapter_id(&batch_host, "/batch/HostOne.svelte").as_deref(),
        Some("svelte"),
        "the host-backed lane did not register this canonical under the Svelte adapter"
    );
    assert_eq!(
        registered_adapter_id(&batch_host, "/batch/HostVue.vue").as_deref(),
        Some("vue"),
        "the host-backed lane stopped registering a `.vue` input under the Vue adapter"
    );
    assert!(
        entries[1]
            .errors()
            .iter()
            .any(|error| error.contains("svelte-runtime-unsupported-advanced-rune")),
        "the host-backed lane did not report the refusal for the refused component: {:?}",
        entries[1].errors()
    );
    assert_publishes_no_product("host-backed advanced-rune refusal", &entries[1]);

    // Host-backed bytes equal the single-file route under the bundler preset.
    let bundler = crate::host_compile::compile_profile_for_bundler();
    for (entry, source, language) in [
        (
            &entries[0],
            SUPPORTED,
            verter_language::FileLanguage::svelte(),
        ),
        (
            &entries[2],
            VUE_SOURCE,
            verter_language::FileLanguage::vue(),
        ),
    ] {
        assert_eq!(
            single_file_reference_under(&entry.canonical_id, source, language, &bundler),
            SingleFileOutcome::Published {
                code: entry.code().to_string(),
                has_map: entry.source_map().is_some(),
            },
            "{}: the host-backed lane's output is not what the single-file route produces for \
             the same source under its own language",
            entry.canonical_id
        );
    }
}

// The batch contract the route DOES honour — asserted green

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

    // Equality against the same input compiled alone — not a substring search.
    for (index, input) in inputs.iter().enumerate() {
        let alone = run_batch(
            std::slice::from_ref(input),
            CompileManyTarget::RuntimeRender {
                profile: render_profile(false, true),
            },
        );
        assert_eq!(
            entries[index].code(),
            alone[0].code(),
            "{}: the entry's bytes differ from the same input compiled alone, so a neighbour \
             influenced it",
            input.canonical_id
        );
        assert_eq!(
            entries[index].source_map().is_some(),
            alone[0].source_map().is_some(),
            "{}: map presence differs from the same input compiled alone",
            input.canonical_id
        );
    }
    assert_ne!(
        entries[0].code(),
        entries[2].code(),
        "the two distinct inputs produced identical bytes, so this batch cannot detect a \
         fanned-out result"
    );
    // The middle item leaves no residue in its neighbours.
    assert!(
        entries[0].errors().is_empty() && entries[2].errors().is_empty(),
        "a neighbouring item contaminated a sibling: {:?} / {:?}",
        entries[0].errors(),
        entries[2].errors()
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
        with_map[0].source_map().is_some(),
        "the batch withheld a requested source map"
    );
    assert!(
        without_map[0].source_map().is_none(),
        "the batch published a source map that was never requested"
    );
    assert_eq!(
        with_map[0].code(),
        without_map[0].code(),
        "the batch's source-map axis changed the emitted module bytes"
    );
}

// Conformance target — the equivalence the batch route must reach

/// Per item, the batch equals the single-file route: same bytes and map
/// presence, or the same typed refusal with no partial product. Refusal
/// sits in the middle so a shift is visible both ways.
#[test]
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
                if entry.code() != code.as_str() {
                    divergences.push(format!(
                        "{}: bytes differ\n  batch:  {}\n  single: {}",
                        entry.canonical_id,
                        entry.code(),
                        code
                    ));
                }
                if entry.source_map().is_some() != has_map {
                    divergences.push(format!("{}: map presence differs", entry.canonical_id));
                }
            }
            SingleFileOutcome::Refused { diagnostic_code } => {
                if !entry
                    .errors()
                    .iter()
                    .any(|error| error.contains(&diagnostic_code))
                {
                    divergences.push(format!(
                        "{}: the single-file route refused with `{diagnostic_code}`, the batch \
                         reported {:?}",
                        entry.canonical_id,
                        entry.errors()
                    ));
                }
                if !entry.code().is_empty() || entry.source_map().is_some() {
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

// Per-entry atomicity, over every failing-entry class the public batch API
// can genuinely reach

/// Vue-carrier template that fails to parse. Malformed script is not a
/// failure (recovery still publishes a module); unterminated interpolation
/// is. Each entry proves its construction via [`assert_entry_entered_class`].
const TEMPLATE_THAT_FAILS_TO_PARSE: &str =
    "<template>\n  <div>{{ x }} {{ unclosed\n  <span>{{ also-unclosed\n</template>";

/// Compiles successfully with a warning (unresolved macro type) — a
/// diagnostic must not be read as a refusal.
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

/// Failing-entry class reachable through public `compile_many`.
/// Unreachable classes (and the host-backed `OtherHostError` lane) are
/// recorded on [`a_genuinely_failing_batch_entry_publishes_no_partial_product`],
/// not represented by a target that would fail for another reason.
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
    /// Entries that must publish cleanly. Per-entry: neighbours in the
    /// same batch. Batch-level ([`FailingClass::OtherHostError`]): the
    /// same inputs under the ordinary profile.
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
            // Grammar axis is batch-level: every entry fails. `unaffected`
            // is the same inputs under the ordinary profile.
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
            // Existing `#[cfg(test)]` sentinel: panic unwinds through the
            // production catch boundary. Nothing is added to production.
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

/// Prove the entry entered THIS class, not merely that `errors` is non-empty.
#[track_caller]
fn assert_entry_entered_class(class: FailingClass, lane: Lane, entry: &CompileBatchEntry) {
    let id = &entry.canonical_id;
    let row = format!("{class:?}/{lane:?}");
    assert!(
        !entry.errors().is_empty(),
        "{row}: `{id}` reported no error at all, so this row is not measuring a failing entry"
    );
    match class {
        FailingClass::DuplicateCanonicalConflict => {
            assert_eq!(
                entry.errors(),
                vec!["duplicate canonical_id with conflicting source in batch".to_string()],
                "{row}: `{id}` did not fail with the batch's own per-canonical conflict error, so \
                 it entered some other class"
            );
        }
        FailingClass::CompileFailure => {
            // Failing-compile prefixes every message with the canonical id;
            // the successful-response arm does not (and publishes a product).
            let prefix = format!("[{id}] ");
            for message in entry.errors() {
                assert!(
                    message.starts_with(&prefix),
                    "{row}: `{id}` carries an error that is not prefixed with its canonical id, so \
                     it came from the successful-response construction rather than the \
                     failing-compile one: {message:?}"
                );
            }
            assert!(
                entry
                    .errors()
                    .iter()
                    .any(|message| message.contains("Interpolation end sign was not found.")),
                "{row}: `{id}` failed, but not with the template-parse diagnostic this row drives: \
                 {:?}",
                entry.errors()
            );
        }
        FailingClass::OtherHostError => {
            assert_eq!(
                entry.errors(),
                vec![format!(
                    "[{id}] host error: compile profile grammar differs from registered grammar"
                )],
                "{row}: `{id}` did not fail with the typed grammar-mismatch host error this row \
                 drives"
            );
        }
        FailingClass::Panic => {
            assert_eq!(
                entry.errors().len(),
                1,
                "{row}: a caught panic renders exactly one error: {:?}",
                entry.errors()
            );
            assert!(
                entry.errors()[0].starts_with(&format!("[{id}] compiler panic: ")),
                "{row}: `{id}` failed, but not through the coordinator's panic conversion: {:?}",
                entry.errors()
            );
            assert!(
                entry.errors()[0].contains("synthetic panic"),
                "{row}: the panic body is not the injected one, so some OTHER panic produced this \
                 entry: {:?}",
                entry.errors()
            );
        }
    }
}

/// A failing entry must publish no code, map, or output language.
#[track_caller]
fn assert_publishes_no_product(row: &str, entry: &CompileBatchEntry) {
    let id = &entry.canonical_id;
    assert!(
        entry.code().is_empty(),
        "{row}: `{id}` published {} bytes of code alongside its failure {:?}:\n{}",
        entry.code().len(),
        entry.errors(),
        entry.code()
    );
    assert!(
        entry.source_map().is_none(),
        "{row}: `{id}` published a source map alongside its failure {:?}",
        entry.errors()
    );
    assert!(
        entry.lang().is_none(),
        "{row}: `{id}` reported an output language ({:?}) alongside its failure {:?}",
        entry.lang(),
        entry.errors()
    );
}

/// Control: this entry reports no errors and publishes its module.
#[track_caller]
fn assert_publishes_cleanly(row: &str, entry: &CompileBatchEntry) {
    let id = &entry.canonical_id;
    assert!(
        entry.errors().is_empty(),
        "{row}: `{id}` should have published cleanly but reported {:?}",
        entry.errors()
    );
    assert!(
        !entry.code().is_empty(),
        "{row}: `{id}` reported no failure but published no code either"
    );
    assert!(
        entry.lang().is_some(),
        "{row}: `{id}` published code but no output language"
    );
}

/// Driven `(class, lane)` rows and the number of entries that must enter
/// the class (conflict fans out; grammar is batch-level).
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

/// A failing batch entry publishes no product. Each row is proven to have
/// entered its class first, then asked whether it published code, map, or
/// language.
///
/// Not reachable through this API (not represented by `#[ignore]` — that
/// would fail for a different reason):
///
/// - **Upsert failure.** Same per-canonical error map as a conflict, but
///   `compile_many` exposes no input that yields scheduler `Failed` /
///   `Superseded` / `Shutdown` or a post-commit generation-fence mismatch.
/// - **Svelte runtime refusal.** Reachable now (language is derived from
///   the canonical id); driven by
///   [`the_svelte_runtime_refusals_fire_on_the_batch_route`].
/// - **`OtherHostError` on the host-backed lane.** Driven on the render
///   lane. The grammar axis rides the compile profile, and the host-backed
///   lane uses a fixed bundler preset the caller cannot vary.
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

/// A diagnostic is not a refusal. Ordinary success and warning-only compile
/// both publish and report no errors. Vue render-only warnings live on the
/// entry; effective host-backed warnings live on the response.
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

        // Ordinary success.
        assert_publishes_cleanly(&row, &entries[0]);
        assert!(
            entries[0].diagnostics().is_empty(),
            "{row}: an ordinary success carries no diagnostics: {:?}",
            entries[0].diagnostics()
        );

        // Warning-only compile still publishes.
        assert_publishes_cleanly(&row, &entries[1]);
        match lane {
            Lane::RuntimeRender => {
                let warning = entries[1]
                    .diagnostics()
                    .iter()
                    .find(|diagnostic| diagnostic.code == "XUnresolvedImportedMacroType")
                    .unwrap_or_else(|| {
                        panic!(
                            "{row}: the warning-only input carried no unresolved-macro-type \
                             diagnostic, so this control is not measuring a warning: {:?}",
                            entries[1]
                                .diagnostics()
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
                    entries[1].diagnostics().is_empty(),
                    "{row}: the host-backed entry surfaces no success-warning list by \
                     construction, so a non-empty one means the lane changed: {:?}",
                    entries[1].diagnostics()
                );
                // Confirm the source is warning-carrying on this lane.
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

/// Host-backed construction: mixed product+errors is unrepresentable.
/// `Ok(response)` produces, `Err(HostError)` fails. Stays `#[ignore]` and
/// passing — named artifact, not a red gate. Live coverage:
/// [`an_ordinary_success_and_a_warning_only_compile_are_never_read_as_failures`]
/// and [`a_batch_outcome_cannot_express_a_product_beside_an_error`].
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

    // Success half.
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
        "{row}: a SUCCESSFUL host-backed response now carries an error-severity diagnostic. That \
         is no longer a hazard — the entry below is still atomic — but it is a change worth \
         knowing about: {:?}",
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
    // Produced arm: product, no errors.
    assert_publishes_cleanly(row, &entries[0]);
    assert!(
        matches!(
            entries[0].outcome,
            crate::host_compile::CompileBatchOutcome::Produced { .. }
        ),
        "{row}: a successful host-backed compile must take the PRODUCED arm"
    );

    // Failure half: `Err`, no product.
    assert_publishes_no_product(row, &entries[1]);
    assert!(
        matches!(
            entries[1].outcome,
            crate::host_compile::CompileBatchOutcome::Failed { .. }
        ),
        "{row}: a genuinely failing host-backed compile must take the FAILED arm"
    );
    let failure = batch_host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(failing_id.clone()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: crate::host_compile::compile_profile_for_bundler(),
    });
    assert!(
        failure.is_err(),
        "{row}: the failing input answered Ok, so its entry WAS built by the response-reading \
         construction rather than a failure arm"
    );
}

/// A batch outcome cannot express a product beside an error, or a failure
/// that reports nothing. Exhaustive `match` (no wildcard): a new
/// [`CompileBatchOutcome`] variant is a compile error. Both arms are
/// reached through the public batch API.
#[test]
fn a_batch_outcome_cannot_express_a_product_beside_an_error() {
    use crate::host_compile::CompileBatchOutcome;

    let row = "structure/CompileBatchOutcome";
    // Both arms in one batch.
    let ok_id = "/atomic/OutcomeShapeOk.vue".to_string();
    let bad_id = "/atomic/OutcomeShapeBad.vue".to_string();
    let entries = host().compile_many(
        vec![
            batch_input(&ok_id, WARNS_WITHOUT_FAILING),
            batch_input(&bad_id, TEMPLATE_THAT_FAILS_TO_PARSE),
        ],
        CompileBatchOptions::default(),
        Lane::HostBacked.target(),
    );
    assert_eq!(entries.len(), 2, "{row}: one entry per input");

    let mut produced_seen = 0usize;
    let mut failed_seen = 0usize;
    for entry in &entries {
        match &entry.outcome {
            CompileBatchOutcome::Produced {
                code,
                lang,
                source_map,
                diagnostics,
            } => {
                produced_seen += 1;
                // Produced arm has no `errors` field.
                assert!(
                    !code.is_empty(),
                    "{row}: `{}` took the produced arm with an EMPTY product",
                    entry.canonical_id
                );
                assert!(
                    lang.is_some(),
                    "{row}: `{}` produced code but no output language",
                    entry.canonical_id
                );
                let _ = source_map;
                assert!(
                    diagnostics
                        .iter()
                        .all(|d| d.severity != crate::HostSeverity::Error),
                    "{row}: `{}` carries an ERROR in its success diagnostics",
                    entry.canonical_id
                );
                // Read projection matches the arm.
                assert!(
                    entry.errors().is_empty(),
                    "{row}: a produced entry reported errors"
                );
            }
            CompileBatchOutcome::Failed { errors } => {
                failed_seen += 1;
                // Failed arm has no product fields.
                assert!(
                    !errors.as_slice().is_empty(),
                    "{row}: `{}` took the failed arm with NO error —                      NonEmptyErrors is supposed to make that unconstructible",
                    entry.canonical_id
                );
                // Read projection matches the arm.
                assert!(
                    entry.code().is_empty()
                        && entry.lang().is_none()
                        && entry.source_map().is_none()
                        && entry.diagnostics().is_empty(),
                    "{row}: a failed entry projected a product",
                );
            }
        }
    }
    assert_eq!(
        (produced_seen, failed_seen),
        (1, 1),
        "{row}: the batch must reach BOTH arms, or this test proves only one of them"
    );

    // Empty error list becomes the stated fallback, never an empty failure.
    let recovered = crate::host_compile::NonEmptyErrors::new(Vec::new(), || "fallback".to_string());
    assert_eq!(
        recovered.as_slice(),
        ["fallback".to_string()],
        "{row}: an empty error list must become the stated fallback, never an empty failure"
    );
    let kept = crate::host_compile::NonEmptyErrors::new(vec!["real".to_string()], || {
        panic!("the fallback must not run when the list is non-empty")
    });
    assert_eq!(
        kept.as_slice(),
        ["real".to_string()],
        "{row}: a non-empty error list must be preserved verbatim"
    );
}

/// Search (not a proof of unreachability) for a mixed product+errors
/// entry. Last-good serve can pair a previous product with new error
/// diagnostics if unchanged bytes newly fail. Residual: last-good peek
/// skips its validator when the fact rail is empty (a self-contained
/// component records no facts). Public-API sequences only; would turn
/// red if that crack became reachable.
#[test]
fn searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none() {
    /// The shared invariant: an entry that reports a failure publishes
    /// nothing, whatever produced it.
    #[track_caller]
    fn assert_atomic(step: &str, entry: &CompileBatchEntry) {
        if entry.errors().is_empty() {
            return;
        }
        assert!(
            entry.code().is_empty() && entry.source_map().is_none() && entry.lang().is_none(),
            "{step}: `{}` served a product alongside {} error(s) — the mixed outcome this test \
             searches for. errors={:?} lang={:?} map={} code:\n{}",
            entry.canonical_id,
            entry.errors().len(),
            entry.errors(),
            entry.lang(),
            entry.source_map().is_some(),
            entry.code()
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
        // Populate last-good, then fail the same canonical.
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
                // Untouched sibling: contamination vs last-good read.
                batch_input(&format!("/probe/Sibling{lane:?}.vue"), ZERO_FACT),
            ],
            CompileBatchOptions::default(),
            lane.target(),
        );
        assert!(
            !failed[0].errors().is_empty(),
            "{lane:?}: the recompile did not fail, so this sequence never reached the state it \
             searches: {:?}",
            failed[0].code()
        );
        assert_atomic(&format!("{lane:?}/fail-after-last-good"), &failed[0]);
        assert_publishes_cleanly(
            &format!("{lane:?}/fail-after-last-good sibling"),
            &failed[1],
        );

        // Same, against an advanced store view.
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
            !failed[0].errors().is_empty(),
            "{lane:?}: the advanced-view recompile did not fail, so this sequence never reached \
             the state it searches"
        );
        assert_atomic(&format!("{lane:?}/fail-after-advanced-view"), &failed[0]);

        // Unchanged bytes after the store view moved: cached slot, no error.
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
            reread[0].code(),
            cold[0].code(),
            "{lane:?}: the unchanged re-request served different bytes than the compile that \
             populated the slot"
        );

        // Fail then recover: fresh product, no failure residue.
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
            !broken[0].errors().is_empty(),
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
            recovered[0].code(),
            first[0].code(),
            "{lane:?}: the recovered request served different bytes than the first successful one"
        );
    }
}

/// A canonical already registered under another carrier does not keep that
/// carrier through a batch. The skip decision must see language, not source
/// bytes alone. Compared to a fresh host given the same batch.
#[test]
fn a_batch_re_registers_a_canonical_that_was_left_under_another_carrier() {
    let canonical = "/batch/Poisoned.svelte";

    // Pre-register the same bytes under the wrong carrier.
    let poisoned = host();
    let _ = poisoned
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(SUPPORTED),
            file_language: verter_language::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {canonical}: {error:?}"));
    assert_eq!(
        registered_adapter_id(&poisoned, canonical).as_deref(),
        Some("vue"),
        "the pre-registration did not take, so this test is not exercising the stale-carrier path"
    );

    let inputs = vec![batch_input(canonical, SUPPORTED)];
    let target = CompileManyTarget::RuntimeRender {
        profile: render_profile(false, true),
    };
    let poisoned_entries = poisoned.compile_many(
        inputs.clone(),
        CompileBatchOptions::default(),
        match &target {
            CompileManyTarget::RuntimeRender { profile } => CompileManyTarget::RuntimeRender {
                profile: profile.clone(),
            },
            CompileManyTarget::HostBacked => CompileManyTarget::HostBacked,
        },
    );

    // Re-registered under the path-implied carrier.
    assert_eq!(
        registered_adapter_id(&poisoned, canonical).as_deref(),
        Some("svelte"),
        "the batch kept the carrier a previous registration left behind"
    );

    // Same bytes as a fresh host that never saw the stale registration.
    let fresh_entries = run_batch(&inputs, target);
    assert_eq!(
        poisoned_entries[0].code(),
        fresh_entries[0].code(),
        "a canonical pre-registered under another carrier produced different bytes from a fresh \
         host given the same batch"
    );
    assert_eq!(
        poisoned_entries[0].errors(),
        fresh_entries[0].errors(),
        "a canonical pre-registered under another carrier produced different errors from a fresh \
         host given the same batch"
    );
    // Non-vacuity: the fresh host must publish something.
    assert!(
        !fresh_entries[0].code().is_empty() && fresh_entries[0].errors().is_empty(),
        "the fresh host published nothing for this input, so the comparison decides nothing: {:?}",
        fresh_entries[0].errors()
    );
}

/// An id that names no carrier yields no component module. The `.vue` row
/// is the control so a batch that stopped compiling everything cannot
/// satisfy the negative rows.
#[test]
fn the_batch_classifies_by_canonical_id_so_a_non_carrier_id_yields_no_module() {
    let vue_source =
        "<script setup>\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>\n";

    for (canonical, compiles) in [
        ("/edge/Carrier.vue", true),
        ("/edge/Carrier.svelte", false), // Vue bytes are not a Svelte component
        ("/edge/Module.ts", false),
        ("/edge/NoExtension", false),
    ] {
        let entries = run_batch(
            &[batch_input(canonical, vue_source)],
            CompileManyTarget::HostBacked,
        );
        assert_eq!(entries.len(), 1, "{canonical}: one entry per input");
        let entry = &entries[0];
        if compiles {
            assert!(
                entry.errors().is_empty() && !entry.code().is_empty(),
                "{canonical}: an id naming the carrier its bytes are did not compile: {:?}",
                entry.errors()
            );
        } else {
            assert!(
                entry.code().is_empty(),
                "{canonical}: a module was published for an id that does not name this carrier"
            );
            assert!(
                !entry.errors().is_empty(),
                "{canonical}: the batch reported neither a module nor a reason"
            );
        }
    }
}

/// Characterized, not fixed: a registered non-carrier id is reported as
/// `HostError::MissingSource` (no `Main` node). Taxonomy lives on the
/// virtual-file route (`effective_file_state_from_snapshot` → `None`), not
/// the batch.
#[test]
#[ignore = "the virtual-file route reports a registered non-carrier file as a missing source"]
fn a_non_carrier_batch_id_is_not_reported_as_a_missing_source() {
    let entries = run_batch(
        &[batch_input("/edge/Module.ts", "export const a = 1;\n")],
        CompileManyTarget::HostBacked,
    );
    let errors = entries[0].errors().join("; ");
    assert!(
        !errors.contains("missing source"),
        "a file whose source is registered was reported as a missing source: {errors}"
    );
}
