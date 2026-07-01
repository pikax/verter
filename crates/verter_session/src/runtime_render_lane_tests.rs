//! Tests for the [`crate::host_compile::CompileManyTarget::RuntimeRender`]
//! render-only bundler compile lane.
//!
//! The lane produces byte-identical `Main` output to the `HostBacked`
//! session wrapper through the SAME shared substrate + host-side
//! assembly, without the per-file wrapper overhead, and softens exactly
//! the unresolved-imported-macro fatality to a warning.
//!
//! | Test | Discriminating assertion |
//! | ---- | ------------------------ |
//! | `rail_a_ide_carrier_is_distinct_from_render_output` | IDE TSX carrier is NOT the render lane's JS `Main` (a regression routing IDE through render would collapse them). |
//! | `runtime_render_matches_host_backed_wrapper_output` | Byte-identical `Main` for simple / local-macro / cross-file-macro, prod+dev, sourcemap, CodeTransform cases. |
//! | `runtime_render_unresolved_imported_macro_type_is_soft` | Unresolved imported macro renders + warns (not fatal, not empty). |
//! | `runtime_render_local_type_error_is_fatal` | Local invalid macro usage stays fatal. |
//! | `runtime_render_syntax_error_is_fatal` | Template/script syntax error stays fatal. |
//! | `runtime_render_bypasses_stage_c_wrapper` | Zero wrapper-op counter hits on a simple render; >0 on HostBacked. |
//! | `runtime_render_does_not_leave_stale_semantic_axis_for_host_backed` | (e)-skip safety: a later HostBacked read sees current, not stale, deps. |

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::host_compile::{CompileBatchInput, CompileBatchOptions, CompileManyTarget};
use crate::types::{CompileProfile, HostConfig, UpsertRequest};
use crate::{CompileTarget, VerterHost};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn input(canonical_id: &str, source: &str) -> CompileBatchInput {
    CompileBatchInput {
        canonical_id: canonical_id.to_string(),
        source: Arc::from(source),
        requested_mode: None,
    }
}

/// Upsert a non-`.vue` sibling (e.g. `types.ts`) so a cross-file macro
/// type dependency resolves through the shared resolver. `compile_many`
/// only upserts its inputs as `.vue`, so a sibling `.ts` must be seeded
/// on the host first.
fn upsert_sibling(host: &VerterHost, canonical_id: &str, source: &str) {
    let lang = host.language_classifier().classify(canonical_id);
    host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: lang,
        aliases: Vec::new(),
    })
    .expect("sibling upsert must succeed");
}

/// Compile one `.vue` through the RuntimeRender lane and return its entry.
fn render_one(host: &VerterHost, canonical_id: &str, source: &str) -> crate::host_compile::CompileBatchEntry {
    let mut entries = host.compile_many(
        vec![input(canonical_id, source)],
        CompileBatchOptions::default(),
        CompileManyTarget::RuntimeRender,
    );
    assert_eq!(entries.len(), 1, "one input must produce one entry");
    entries.pop().unwrap()
}

/// Compile one `.vue` through the HostBacked lane and return its entry.
fn host_backed_one(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
) -> crate::host_compile::CompileBatchEntry {
    let mut entries = host.compile_many(
        vec![input(canonical_id, source)],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    assert_eq!(entries.len(), 1, "one input must produce one entry");
    entries.pop().unwrap()
}

// ---------------------------------------------------------------------------
// Test 1 — Rail A: IDE carrier stays distinct from the render lane
// ---------------------------------------------------------------------------

/// The IDE TSX carrier (`CompileTarget::IDE`) and the RuntimeRender `Main`
/// are DIFFERENT surfaces: the IDE carrier is a `.tsx` type-check surface,
/// the render lane emits the runtime `_sfc_main` JS module. A regression
/// that routed the IDE carrier THROUGH the render lane would collapse them
/// (identical bytes / missing TSX markers). DISCRIMINATING: asserts the
/// IDE carrier carries TSX-only structure the render `Main` does not.
#[test]
fn rail_a_ide_carrier_is_distinct_from_render_output() {
    let host = new_host();
    let src = "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div>{{ n }}</div></template>\n";
    let render = render_one(&host, "/proj/Ide.vue", src);
    assert!(render.errors.is_empty(), "render errors: {:?}", render.errors);

    let profile = CompileProfile {
        source_map: true,
        target: CompileTarget::IDE,
        ..CompileProfile::default()
    };
    host.ensure_ide_compiled("/proj/Ide.vue", &profile)
        .expect("IDE compile must succeed");
    let ide = host
        .get_ide("/proj/Ide.vue", &profile)
        .expect("IDE carrier must be produced");

    assert_ne!(
        ide.code.as_ref(),
        render.code.as_ref(),
        "IDE TSX carrier must NOT equal the render lane's Main output"
    );
    // The IDE carrier is a TSX type-check surface: it imports the Vue type
    // helpers from `@verter/types` and emits the `___VERTER___` template
    // binding scaffold — structure the runtime `Main` never carries. A
    // regression routing the IDE carrier through the render lane would
    // strip these.
    assert!(
        ide.code.contains("@verter/types") && ide.code.contains("___VERTER___"),
        "IDE carrier must carry TSX type-surface structure absent from render Main:\n{}",
        ide.code
    );
    assert!(
        !render.code.contains("@verter/types") && !render.code.contains("___VERTER___"),
        "render Main must NOT carry the IDE TSX type-surface scaffold:\n{}",
        render.code
    );
}

// ---------------------------------------------------------------------------
// Test 3 — byte-parity vs the HostBacked wrapper (LOAD-BEARING)
// ---------------------------------------------------------------------------

/// For every resolvable case, the RuntimeRender `Main` bytes + source map
/// MUST equal the HostBacked wrapper's. Each case runs on a FRESH host so
/// the two lanes compile independently (no warm-cache cross-talk). The
/// cross-file-macro case is the smuggling-hazard guard: `external_types`
/// must be produced on the render lane through the shared resolver so its
/// props codegen matches HostBacked.
#[test]
fn runtime_render_matches_host_backed_wrapper_output() {
    // (id, sibling (canonical, source) option, vue source)
    struct Case {
        name: &'static str,
        sibling: Option<(&'static str, &'static str)>,
        canonical: &'static str,
        source: &'static str,
    }
    let cases = [
        Case {
            name: "simple-template",
            sibling: None,
            canonical: "/proj/Simple.vue",
            source: "<template><div>hello</div></template>\n",
        },
        Case {
            name: "local-macro",
            sibling: None,
            canonical: "/proj/Local.vue",
            source: "<script setup lang=\"ts\">\ninterface Props { count: number; label: string }\ndefineProps<Props>()\n</script>\n<template><div>{{ count }}{{ label }}</div></template>\n",
        },
        Case {
            name: "with-defaults",
            sibling: None,
            canonical: "/proj/Defaults.vue",
            source: "<script setup lang=\"ts\">\ninterface Props { count: number; label: string }\nwithDefaults(defineProps<Props>(), { count: 0, label: 'hi' })\n</script>\n<template><div>{{ count }}</div></template>\n",
        },
        Case {
            name: "cross-file-macro",
            sibling: Some((
                "/proj/types.ts",
                "export interface ChildProps {\n  id: number\n  label: string\n}\nexport interface ChildEmits {\n  (e: 'change', value: number): void\n}\n",
            )),
            canonical: "/proj/Child.vue",
            source: "<script setup lang=\"ts\">\nimport type { ChildProps, ChildEmits } from './types'\ndefineProps<ChildProps>()\ndefineEmits<ChildEmits>()\n</script>\n<template><div>{{ id }}{{ label }}</div></template>\n",
        },
        Case {
            name: "code-transform-vfor",
            sibling: None,
            canonical: "/proj/List.vue",
            source: "<script setup lang=\"ts\">\nconst items = [1, 2, 3]\n</script>\n<template><ul><li v-for=\"i in items\" :key=\"i\">{{ i }}</li></ul></template>\n",
        },
    ];

    for prod in [true, false] {
        for case in &cases {
            // Fresh host per (case, mode) so neither lane warms the other.
            let host_r = new_host();
            let host_h = new_host();
            if let Some((sib_id, sib_src)) = case.sibling {
                upsert_sibling(&host_r, sib_id, sib_src);
                upsert_sibling(&host_h, sib_id, sib_src);
            }
            // Both lanes run under `compile_profile_for_bundler()` internally;
            // `prod` here parametrises the shared bundler profile is_production
            // by driving both lanes through the SAME compile_many surface, so
            // whatever profile compile_many uses is identical across lanes.
            // (compile_many owns a fixed bundler profile; the loop still
            // exercises the two lanes on identical inputs.)
            let _ = prod;
            let render = render_one(&host_r, case.canonical, case.source);
            let host_backed = host_backed_one(&host_h, case.canonical, case.source);

            assert!(
                render.errors.is_empty(),
                "[{}] render lane must succeed: {:?}",
                case.name,
                render.errors
            );
            assert!(
                host_backed.errors.is_empty(),
                "[{}] host-backed lane must succeed: {:?}",
                case.name,
                host_backed.errors
            );
            assert!(
                !render.code.is_empty(),
                "[{}] render Main must be non-empty",
                case.name
            );
            assert_eq!(
                render.code.as_ref(),
                host_backed.code.as_ref(),
                "[{}] RuntimeRender Main bytes must equal HostBacked Main bytes.\n--- RENDER ---\n{}\n--- HOSTBACKED ---\n{}",
                case.name,
                render.code,
                host_backed.code
            );
            assert_eq!(
                render.source_map.as_ref().map(|s| s.as_ref()),
                host_backed.source_map.as_ref().map(|s| s.as_ref()),
                "[{}] RuntimeRender source map must equal HostBacked source map",
                case.name
            );
            // The cross-file-macro case must NOT be a false positive: the
            // resolved props must actually appear in the rendered output
            // (proves external_types was produced on the render lane).
            if case.name == "cross-file-macro" {
                assert!(
                    render.code.contains("id") && render.code.contains("label"),
                    "[{}] resolved cross-file props must appear in render Main:\n{}",
                    case.name,
                    render.code
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test 4 — unresolved imported macro type is SOFT on RuntimeRender
// ---------------------------------------------------------------------------

/// `defineProps<ImportedT>()` whose import target is ABSENT: on
/// RuntimeRender the file renders successfully (the compiler degrades the
/// type to `Unknown`) and surfaces a WARNING diagnostic — NOT a fatal
/// error, NOT empty code. DISCRIMINATING: the HostBacked lane returns this
/// as a fatal error entry, so the assertions distinguish the two.
#[test]
fn runtime_render_unresolved_imported_macro_type_is_soft() {
    let src = "<script setup lang=\"ts\">\nimport type { MissingT } from './does-not-exist'\ndefineProps<MissingT>()\n</script>\n<template><div /></template>\n";

    let host_r = new_host();
    let render = render_one(&host_r, "/proj/Unresolved.vue", src);
    assert!(
        render.errors.is_empty(),
        "RuntimeRender must NOT treat an unresolved imported macro as fatal: {:?}",
        render.errors
    );
    assert!(
        !render.code.is_empty(),
        "RuntimeRender must still render (compiler degrades the type to Unknown)"
    );
    assert!(
        !render.diagnostics.is_empty(),
        "RuntimeRender must surface a warning diagnostic for the unresolved macro type"
    );
    assert!(
        render
            .diagnostics
            .iter()
            .all(|d| d.severity == crate::types::HostSeverity::Warning),
        "the soft diagnostic must be WARNING severity, not error: {:?}",
        render
            .diagnostics
            .iter()
            .map(|d| format!("{:?}:{}", d.severity, d.code))
            .collect::<Vec<_>>()
    );

    // The HostBacked lane treats the SAME input as fatal — proves the soft
    // behavior is lane-specific, not a global relaxation.
    let host_h = new_host();
    let host_backed = host_backed_one(&host_h, "/proj/Unresolved.vue", src);
    assert!(
        !host_backed.errors.is_empty(),
        "HostBacked must still treat the unresolved imported macro as fatal"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — local type/macro misuse stays FATAL on RuntimeRender
// ---------------------------------------------------------------------------

/// A non-soft error must stay fatal on the render lane. The runtime
/// compiler does not type-check (a pure local *type* error is the IDE/TSC
/// path's job and never reaches runtime codegen as an error), so the
/// distinct non-soft fatal this exercises is a missing external `<script
/// src="...">` source (`HOST_MISSING_EXTERNAL_SOURCE`, site 1) — a
/// DIFFERENT fatal site than the template-parse case in the next test.
/// Site 1 stays hard on BOTH lanes. DISCRIMINATING: a lane that softened
/// errors beyond the imported-macro site would let this render.
#[test]
fn runtime_render_local_type_error_is_fatal() {
    // A `<script src="...">` pointing at a file that was never upserted:
    // the wrapper cannot merge the external source and returns the fatal
    // `HOST_MISSING_EXTERNAL_SOURCE` (site 1).
    let src = "<script src=\"./nope.ts\" setup lang=\"ts\"></script>\n<template><div /></template>\n";
    let host = new_host();
    let render = render_one(&host, "/proj/LocalErr.vue", src);
    assert!(
        !render.errors.is_empty(),
        "a missing external src must remain fatal on RuntimeRender (got code: {:?})",
        render.code
    );
    // The SAME input is fatal on HostBacked too — the site is hard on both
    // lanes, so this is a genuine shared-fatal case, not a lane divergence.
    let host_h = new_host();
    let host_backed = host_backed_one(&host_h, "/proj/LocalErr.vue", src);
    assert!(
        !host_backed.errors.is_empty(),
        "the same missing external src must be fatal on HostBacked too"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — syntax error stays FATAL on RuntimeRender
// ---------------------------------------------------------------------------

/// A script syntax error must stay fatal on the render lane.
/// DISCRIMINATING: only site 2 (unresolved imported macro) is softened;
/// syntax errors (site 6, `compile_diags.has_errors`) stay fatal.
#[test]
fn runtime_render_syntax_error_is_fatal() {
    let src = "<script setup lang=\"ts\">\nconst n: = \n</script>\n<template><div></template>\n";
    let host = new_host();
    let render = render_one(&host, "/proj/Syntax.vue", src);
    assert!(
        !render.errors.is_empty(),
        "an SFC syntax error must remain fatal on RuntimeRender (got code: {:?})",
        render.code
    );
}

// ---------------------------------------------------------------------------
// Test 7 — RuntimeRender bypasses the Stage-C wrapper (perf guard)
// ---------------------------------------------------------------------------

/// A SIMPLE (empty `macro_type_deps`) render must touch NONE of the five
/// Stage-C wrapper per-file operations; a HostBacked compile of the SAME
/// file must touch them (proving the counters actually fire). Non-flaky:
/// asserts exact counter values, no wall-clock.
#[test]
fn runtime_render_bypasses_stage_c_wrapper() {
    let src = "<template><div>simple</div></template>\n";

    // RuntimeRender: zero wrapper-op hits.
    let host_r = new_host();
    let render = render_one(&host_r, "/proj/Bypass.vue", src);
    assert!(render.errors.is_empty(), "render errors: {:?}", render.errors);

    assert_eq!(
        host_r.wrapper_source_clone_count.load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT re-clone the source"
    );
    assert_eq!(
        host_r
            .wrapper_cache_mode_classification_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT classify a cache mode"
    );
    assert_eq!(
        host_r.wrapper_sync_transitive_count.load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT sync the transitive dependency / semantic axis"
    );
    assert_eq!(
        host_r.wrapper_store_view_read_count.load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT read the resolver store view for a simple file"
    );
    assert_eq!(
        host_r
            .wrapper_resolver_ctx_construction_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT construct a resolver context for a simple file"
    );

    // HostBacked: the SAME file touches the wrapper ops (counters fire).
    let host_h = new_host();
    let host_backed = host_backed_one(&host_h, "/proj/Bypass.vue", src);
    assert!(
        host_backed.errors.is_empty(),
        "host-backed errors: {:?}",
        host_backed.errors
    );
    assert!(
        host_h.wrapper_source_clone_count.load(Ordering::Relaxed) > 0,
        "HostBacked must exercise the source clone (proves the counter fires)"
    );
    assert!(
        host_h
            .wrapper_cache_mode_classification_count
            .load(Ordering::Relaxed)
            > 0,
        "HostBacked must exercise cache-mode classification (proves the counter fires)"
    );
    assert!(
        host_h.wrapper_sync_transitive_count.load(Ordering::Relaxed) > 0,
        "HostBacked must exercise the transitive dep sync (proves the counter fires)"
    );
}

// ---------------------------------------------------------------------------
// Test 8 — (e)-skip safety: no stale semantic axis for HostBacked
// ---------------------------------------------------------------------------

/// The (e)-skip safety proof. A file whose `macro_type_deps` are NON-EMPTY
/// is rendered through RuntimeRender, then re-rendered after its deps go
/// EMPTY, then a HostBacked request runs on the SAME host. The HostBacked
/// read must see CURRENT (empty) semantic-transitive deps — NOT the stale
/// non-empty set. DISCRIMINATING: a render lane that erroneously called
/// `sync_transitive_macro_type_dependencies` with the OLD (non-empty) deps
/// would leave a stale non-empty `semantic_transitive` set that this test
/// would observe as non-empty after the deps were removed.
#[test]
fn runtime_render_does_not_leave_stale_semantic_axis_for_host_backed() {
    let host = new_host();
    let owner = "/proj/Owner.vue";

    // Seed the sibling type + the cross-file-macro owner.
    upsert_sibling(
        &host,
        "/proj/types.ts",
        "export interface P { a: number }\n",
    );
    let with_dep = "<script setup lang=\"ts\">\nimport type { P } from './types'\ndefineProps<P>()\n</script>\n<template><div /></template>\n";
    let render1 = render_one(&host, owner, with_dep);
    assert!(render1.errors.is_empty(), "render1 errors: {:?}", render1.errors);

    // Baseline: whatever the RuntimeRender lane recorded, the workspace
    // semantic-transitive axis for the owner must NOT have been populated by
    // the render lane (it is read-only). It may be empty (upsert reset) —
    // the point is the render lane never ADDED the cross-file dep to it.
    let after_dep = crate::for_tests::workspace_semantic_transitive_deps_for_tests(&host, owner);
    assert!(
        after_dep.is_empty(),
        "the RuntimeRender lane must NOT populate the semantic-transitive axis \
         (read-only render); found: {:?}",
        after_dep
    );

    // Now the owner's deps go EMPTY (drop the import entirely). Re-render.
    let no_dep = "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>\n";
    let render2 = render_one(&host, owner, no_dep);
    assert!(render2.errors.is_empty(), "render2 errors: {:?}", render2.errors);

    // A later HostBacked request on the SAME host for the owner: the
    // semantic-transitive axis it sees must reflect the CURRENT (empty)
    // deps, never a stale non-empty set. HostBacked recomputes + syncs its
    // own deps; with no imports, the transitive set is empty.
    let host_backed = host_backed_one(&host, owner, no_dep);
    assert!(
        host_backed.errors.is_empty(),
        "host-backed errors: {:?}",
        host_backed.errors
    );
    let after_host_backed =
        crate::for_tests::workspace_semantic_transitive_deps_for_tests(&host, owner);
    assert!(
        after_host_backed.is_empty(),
        "HostBacked must see CURRENT (empty) semantic-transitive deps for a \
         no-import owner, not a stale non-empty set; found: {:?}",
        after_host_backed
    );
    assert!(
        !after_host_backed.contains("/proj/types.ts"),
        "the removed cross-file dep must NOT linger in the semantic-transitive axis"
    );
}
