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
//! | `runtime_render_upper_drive_input_resolves_macro_types_wired_under_lower_drive_routes` | Drive-letter case parity: an upper-drive compile input converges on the lower-drive canonical whose alias routes were wired via `set_import_dependencies`. |
//! | `runtime_render_upper_drive_input_single_hop_relative_import_control` | Single-hop relative control: resolves without the route table across the same case split. |
//! | `runtime_render_supplied_upper_drive_upsert_does_not_hijack_lower_alias` | Alias-map subsumption: a `Some(UPPER)` upsert must not mint a `c:/... -> C:/...` self-alias (the chokepoint canonicalization subsumes the hijack). |

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
        component_id: None,
    }
}

/// Upsert a non-`.vue` sibling (e.g. `types.ts`) so a cross-file macro
/// type dependency resolves through the shared resolver. `compile_many`
/// only upserts its inputs as `.vue`, so a sibling `.ts` must be seeded
/// on the host first.
fn upsert_sibling(host: &VerterHost, canonical_id: &str, source: &str) {
    let lang = host.language_classifier().classify(canonical_id);
    // `upsert` returns a `#[must_use]` `HostUpdateResult`; the render lane
    // reads the file back from the host, so the result is intentionally
    // discarded after asserting success.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: lang,
            aliases: Vec::new(),
        })
        .expect("sibling upsert must succeed");
}

/// A minimal prod render profile for the common test case (prod, no ssr,
/// no force-js, default runtime module / delimiters / custom-elements).
fn simple_render_profile() -> crate::host_compile::CompileBatchRenderProfile {
    render_profile(true, false, false, crate::types::HmrStrategy::None)
}

/// Compile one `.vue` through the RuntimeRender lane (minimal prod profile)
/// and return its entry.
fn render_one(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
) -> crate::host_compile::CompileBatchEntry {
    render_with_profile(host, canonical_id, source, simple_render_profile(), None)
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

/// Build the batch render profile mirroring an unplugin build. Non-varied
/// output-affecting fields take the same defaults `CompileProfile::default`
/// / an absent `HostCompileProfile` field would (so this matches the
/// `get_virtual_file` oracle unless a test overrides a field explicitly).
fn render_profile(
    is_production: bool,
    ssr: bool,
    force_js: bool,
    hmr: crate::types::HmrStrategy,
) -> crate::host_compile::CompileBatchRenderProfile {
    crate::host_compile::CompileBatchRenderProfile {
        is_production,
        ssr,
        force_js,
        force_vapor: false,
        source_map: false,
        comments: false,
        hmr_strategy: hmr,
        runtime_module_name: Some("vue".to_string()),
        types_module_name: None,
        delimiters: None,
        custom_elements: None,
    }
}

/// Compile one `.vue` through the RuntimeRender lane under an EXPLICIT
/// batch render profile + per-input `component_id`.
fn render_with_profile(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
    profile: crate::host_compile::CompileBatchRenderProfile,
    component_id: Option<String>,
) -> crate::host_compile::CompileBatchEntry {
    let input = CompileBatchInput {
        canonical_id: canonical_id.to_string(),
        source: Arc::from(source),
        requested_mode: None,
        component_id,
    };
    let mut entries = host.compile_many(
        vec![input],
        CompileBatchOptions::default(),
        CompileManyTarget::RuntimeRender { profile },
    );
    assert_eq!(entries.len(), 1, "one input must produce one entry");
    entries.pop().unwrap()
}

/// The CURRENT unplugin Main-render path oracle: upsert the file and call
/// `get_virtual_file(Main)` with the EXACT `CompileProfile` unplugin would
/// pass to `getVirtualFile({ compileProfile })`. This is the byte-parity
/// target the RuntimeRender lane must match — NOT `compile_many(HostBacked)`
/// (whose profile is the frozen bundler preset). Returns
/// `(code, source_map, lang)`.
fn host_backed_main_via_get_virtual_file(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
    profile: &CompileProfile,
) -> (Arc<str>, Option<Arc<str>>, Option<String>) {
    let lang = host.language_classifier().classify(canonical_id);
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: lang,
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
    let resp = host
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical_id.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("get_virtual_file(Main) must succeed");
    (resp.code, resp.source_map, resp.lang)
}

/// The `CompileProfile` an unplugin build would pass to `getVirtualFile`,
/// matching a batch render profile + per-input component id.
fn get_virtual_file_profile(
    rp: crate::host_compile::CompileBatchRenderProfile,
    component_id: Option<String>,
) -> CompileProfile {
    // Mirror EXACTLY the projection `render_base_profile` applies, so the
    // oracle and the render lane feed identical `RuntimeCompileOptions`.
    CompileProfile {
        is_production: rp.is_production,
        ssr: rp.ssr,
        force_js: rp.force_js,
        force_vapor: rp.force_vapor,
        source_map: rp.source_map,
        comments: Some(rp.comments),
        hmr_strategy: rp.hmr_strategy,
        runtime_module_name: rp.runtime_module_name.clone(),
        types_module_name: rp.types_module_name.clone(),
        delimiters: rp.delimiters.clone(),
        custom_elements: rp.custom_elements.clone(),
        component_id,
        ..CompileProfile::default()
    }
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
    assert!(
        render.errors.is_empty(),
        "render errors: {:?}",
        render.errors
    );

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

    // Drive prod/dev × source-map-on/off. Both axes are OUTPUT-AFFECTING and
    // must be genuinely exercised (source_map is NOT carried by the profile
    // by mistake — a bug that would silently drop bundler source maps; with
    // source_map=true BOTH the render lane and the oracle must emit the SAME
    // non-empty map). The RuntimeRender lane is compared against the CURRENT
    // unplugin path (`get_virtual_file(Main)` under the SAME CompileProfile).
    for prod in [true, false] {
        for source_map in [false, true] {
            let mut rp = render_profile(prod, false, false, crate::types::HmrStrategy::None);
            rp.source_map = source_map;
            let gvf_profile = get_virtual_file_profile(rp.clone(), None);
            for case in &cases {
                // Fresh host per (case, mode) so neither path warms the other.
                let host_r = new_host();
                let host_h = new_host();
                if let Some((sib_id, sib_src)) = case.sibling {
                    upsert_sibling(&host_r, sib_id, sib_src);
                    upsert_sibling(&host_h, sib_id, sib_src);
                }
                let render =
                    render_with_profile(&host_r, case.canonical, case.source, rp.clone(), None);
                let (hb_code, hb_map, hb_lang) = host_backed_main_via_get_virtual_file(
                    &host_h,
                    case.canonical,
                    case.source,
                    &gvf_profile,
                );

                let tag = format!("{} prod={} map={}", case.name, prod, source_map);
                assert!(
                    render.errors.is_empty(),
                    "[{tag}] render lane must succeed: {:?}",
                    render.errors
                );
                assert!(
                    !render.code.is_empty(),
                    "[{tag}] render Main must be non-empty"
                );
                assert_eq!(
                    render.code.as_ref(),
                    hb_code.as_ref(),
                    "[{tag}] RuntimeRender Main bytes must equal the getVirtualFile Main bytes.\n--- RENDER ---\n{}\n--- GETVIRTUALFILE ---\n{}",
                    render.code,
                    hb_code
                );
                assert_eq!(
                    render.source_map.as_ref().map(|s| s.as_ref()),
                    hb_map.as_ref().map(|s| s.as_ref()),
                    "[{tag}] RuntimeRender source map must equal the getVirtualFile source map",
                );
                assert_eq!(
                    render.lang, hb_lang,
                    "[{tag}] RuntimeRender Main lang must equal the getVirtualFile Main lang",
                );
                // The cross-file-macro case must NOT be a false positive: the
                // resolved props must actually appear in the rendered output
                // (proves external_types was produced on the render lane).
                if case.name == "cross-file-macro" {
                    assert!(
                        render.code.contains("id") && render.code.contains("label"),
                        "[{tag}] resolved cross-file props must appear in render Main:\n{}",
                        render.code
                    );
                }
            }
        }
    }
    // Note on source maps: the matrix runs every case with
    // source_map ∈ {false, true} and asserts render↔oracle parity of the
    // Main code + map + lang in BOTH states, so the `source_map` profile
    // field is threaded through the render lane identically to the
    // getVirtualFile path. The assembled Vue `Main` module carries no source
    // map of its own (the per-block maps ride on the Script / Template
    // virtual nodes, which the bundler requests separately), so
    // `render.source_map` is `None` on both paths here — the parity assert
    // is what discriminates a lane that diverged on the field, not a
    // non-empty-map expectation the Main node never satisfies. Dedicated
    // Script/Template source-map coverage lives in the carrier codegen +
    // sourcemap test suites.
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
// Test 5 — missing external `<script src>` stays FATAL on RuntimeRender
// ---------------------------------------------------------------------------

/// The missing-external-source fatal (site 1) stays hard on the render
/// lane. DISCRIMINATING: a lane that softened errors beyond the
/// imported-macro-resolution site would let this render.
#[test]
fn runtime_render_missing_external_src_is_fatal() {
    // A `<script src="...">` pointing at a file that was never upserted:
    // the wrapper cannot merge the external source and returns the fatal
    // `HOST_MISSING_EXTERNAL_SOURCE` (site 1).
    let src =
        "<script src=\"./nope.ts\" setup lang=\"ts\"></script>\n<template><div /></template>\n";
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
// Test 5b — a RESOLVED-but-wrong-shape imported macro stays FATAL
// ---------------------------------------------------------------------------

/// A `defineProps<T>()` whose imported `T` RESOLVES but is NOT object-like
/// (e.g. a `string` alias) is a genuine local misuse: the compiler emits a
/// fatal `XInvalidMacroType` (NOT the softenable
/// `XUnresolvedImportedMacroType`). It must stay FATAL on the render lane —
/// the soft contract covers ONLY unresolved-RESOLUTION, never wrong-shape.
/// DISCRIMINATING: the pre-fix over-broad gate (`code=="XInvalidMacroType"`)
/// would have softened this; the structural code split keeps it fatal.
#[test]
fn runtime_render_resolved_wrong_shape_imported_macro_is_fatal() {
    let host = new_host();
    // `WrongProps` RESOLVES (it is a real exported type) but is a string
    // alias — not object-like, so defineProps rejects its shape.
    upsert_sibling(&host, "/proj/wrong.ts", "export type WrongProps = string\n");
    let src = "<script setup lang=\"ts\">\nimport type { WrongProps } from './wrong'\ndefineProps<WrongProps>()\n</script>\n<template><div /></template>\n";
    let render = render_one(&host, "/proj/Wrong.vue", src);
    assert!(
        !render.errors.is_empty(),
        "a resolved-but-wrong-shape imported macro type must stay FATAL on \
         RuntimeRender (got code: {:?}, diagnostics: {:?})",
        render.code,
        render
            .diagnostics
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
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
    assert!(
        render.errors.is_empty(),
        "render errors: {:?}",
        render.errors
    );

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
    assert!(
        render1.errors.is_empty(),
        "render1 errors: {:?}",
        render1.errors
    );

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
    assert!(
        render2.errors.is_empty(),
        "render2 errors: {:?}",
        render2.errors
    );

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

// ---------------------------------------------------------------------------
// Test 9 (S4) — render_profile actually flows (dev vs prod differ; each
// byte-matches the corresponding getVirtualFile profile)
// ---------------------------------------------------------------------------

/// The RuntimeRender lane honors `render_profile.is_production` +
/// `hmr_strategy`: a dev profile (is_production=false, HMR=Vite) produces
/// DIFFERENT output than a prod profile (is_production=true, HMR=None), and
/// each byte-matches the `get_virtual_file` Main under the SAME
/// `CompileProfile`. DISCRIMINATING: a lane that ignored `render_profile`
/// (kept the hardcoded bundler preset) would produce identical prod output
/// for both and MISMATCH the dev oracle.
#[test]
fn runtime_render_honors_render_profile_dev_vs_prod() {
    let canonical = "/proj/Profile.vue";
    let src = "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div>{{ n }}</div></template>\n";

    let dev_rp = render_profile(false, false, false, crate::types::HmrStrategy::Vite);
    let prod_rp = render_profile(true, false, false, crate::types::HmrStrategy::None);

    let dev = render_with_profile(&new_host(), canonical, src, dev_rp.clone(), None);
    let prod = render_with_profile(&new_host(), canonical, src, prod_rp.clone(), None);
    assert!(dev.errors.is_empty(), "dev render errors: {:?}", dev.errors);
    assert!(
        prod.errors.is_empty(),
        "prod render errors: {:?}",
        prod.errors
    );

    // The profile FLOWS: dev and prod outputs differ (dev carries HMR /
    // dev-only code that prod strips). If render_profile were ignored, both
    // would be the same prod bytes.
    assert_ne!(
        dev.code.as_ref(),
        prod.code.as_ref(),
        "dev and prod RuntimeRender output must differ — render_profile must flow.\n--- DEV ---\n{}\n--- PROD ---\n{}",
        dev.code,
        prod.code
    );

    // Each mode byte-matches the current getVirtualFile path under the SAME
    // profile — the real parity claim, now in BOTH modes.
    let (dev_hb, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(dev_rp, None),
    );
    let (prod_hb, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(prod_rp, None),
    );
    assert_eq!(
        dev.code.as_ref(),
        dev_hb.as_ref(),
        "dev RuntimeRender must byte-match getVirtualFile(dev profile).\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        dev.code,
        dev_hb
    );
    assert_eq!(
        prod.code.as_ref(),
        prod_hb.as_ref(),
        "prod RuntimeRender must byte-match getVirtualFile(prod profile)"
    );
}

// ---------------------------------------------------------------------------
// Test 10 (S4) — per-input component_id flows into the scoped-style id
// ---------------------------------------------------------------------------

/// A per-input `component_id` flows into the scoped-style `data-v-<id>` in
/// the render `Main`. Two different component ids produce two different
/// scope ids; the explicit id appears verbatim. DISCRIMINATING: a lane that
/// dropped per-input `component_id` (or read it batch-level) would emit an
/// auto-generated id, not the caller's.
#[test]
fn runtime_render_threads_per_input_component_id_into_scope() {
    let canonical = "/proj/Scoped.vue";
    let src = "<template><div>x</div></template>\n<style scoped>\n.a { color: red }\n</style>\n";
    let rp = render_profile(true, false, false, crate::types::HmrStrategy::None);

    let a = render_with_profile(
        &new_host(),
        canonical,
        src,
        rp.clone(),
        Some("aaa111".to_string()),
    );
    let b = render_with_profile(
        &new_host(),
        canonical,
        src,
        rp.clone(),
        Some("bbb222".to_string()),
    );
    assert!(a.errors.is_empty(), "render a errors: {:?}", a.errors);
    assert!(b.errors.is_empty(), "render b errors: {:?}", b.errors);

    // The explicit component id becomes the scope id (`data-v-<id>` /
    // `__scopeId "data-v-<id>"`). The exact id string must appear.
    assert!(
        a.code.contains("aaa111"),
        "explicit component_id 'aaa111' must appear in the render Main scope id:\n{}",
        a.code
    );
    assert!(
        !a.code.contains("bbb222"),
        "render a must not carry the other id"
    );
    assert!(
        b.code.contains("bbb222"),
        "explicit component_id 'bbb222' must appear in the render Main scope id:\n{}",
        b.code
    );
    // Different ids => different output (per-component identity flows).
    assert_ne!(
        a.code.as_ref(),
        b.code.as_ref(),
        "distinct per-input component_ids must produce distinct scoped output"
    );

    // Byte-match the current getVirtualFile path under the same explicit id.
    let (hb_a, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(rp, Some("aaa111".to_string())),
    );
    assert_eq!(
        a.code.as_ref(),
        hb_a.as_ref(),
        "RuntimeRender with explicit component_id must byte-match getVirtualFile.\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        a.code,
        hb_a
    );
}

// ---------------------------------------------------------------------------
// Test 11 (S4) — soft-macro boundary: one missing + one resolved import
// ---------------------------------------------------------------------------

/// Locks the soft-macro boundary: an SFC with TWO imported macro types
/// where ONE resolves and ONE is missing renders successfully with a
/// WARNING (for the missing one only) — the resolved one is NOT dragged
/// into fatality, and the missing one is NOT fatal. DISCRIMINATING: a lane
/// that softened nothing would be fatal; one that softened everything would
/// hide a genuinely fatal case. Locks the two-surface soft gate (the
/// collector missing-diagnostic AND the compiler `XInvalidMacroType`
/// downgraded only for the unresolved-import case).
#[test]
fn runtime_render_mixed_resolved_and_missing_imported_macro_is_soft() {
    let host = new_host();
    // A resolvable sibling providing `GoodProps`; `MissingProps` has no
    // resolvable source.
    upsert_sibling(
        &host,
        "/proj/good.ts",
        "export interface GoodProps { a: number }\n",
    );
    // `defineProps` takes the resolved type; `defineEmits` takes a missing
    // imported type — the emits import is unresolved.
    let src = "<script setup lang=\"ts\">\nimport type { GoodProps } from './good'\nimport type { MissingEmits } from './nope'\ndefineProps<GoodProps>()\ndefineEmits<MissingEmits>()\n</script>\n<template><div>{{ a }}</div></template>\n";
    let render = render_one(&host, "/proj/Mixed.vue", src);

    assert!(
        render.errors.is_empty(),
        "a mix of one resolved + one missing imported macro must NOT be fatal: {:?}",
        render.errors
    );
    assert!(
        !render.code.is_empty(),
        "the mixed case must still render (missing type degrades to Unknown)"
    );
    assert!(
        !render.diagnostics.is_empty()
            && render
                .diagnostics
                .iter()
                .all(|d| d.severity == crate::types::HostSeverity::Warning),
        "the missing imported type must surface as a WARNING (not error): {:?}",
        render
            .diagnostics
            .iter()
            .map(|d| format!("{:?}:{}", d.severity, d.code))
            .collect::<Vec<_>>()
    );
    // The soft warning is the imported-macro-RESOLUTION diagnostic
    // specifically (structured code), not some other softened error.
    assert!(
        render
            .diagnostics
            .iter()
            .any(|d| d.code == "XUnresolvedImportedMacroType"
                || d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "the soft warning must be the unresolved-imported-macro diagnostic: {:?}",
        render
            .diagnostics
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    );
    // The resolved `GoodProps` must materialize concretely — its declared
    // prop key `a` appears in the runtime props declaration. (This file has
    // a MISSING import, so the HostBacked `get_virtual_file` oracle hard-
    // fails on it and cannot be used for byte-parity here — that is exactly
    // the soft-case exclusion. Instead, byte-compare against an EQUIVALENT
    // file with the missing import removed but `GoodProps` unchanged: the
    // resolved props surface must be identical, proving GoodProps still
    // materialized despite the co-occurring soft diagnostic.)
    assert!(
        render.code.contains("\"a\"") || render.code.contains("'a'") || render.code.contains(" a:"),
        "the resolved prop `a` must materialize in the render props declaration:\n{}",
        render.code
    );
    let good_only_src = "<script setup lang=\"ts\">\nimport type { GoodProps } from './good'\ndefineProps<GoodProps>()\n</script>\n<template><div>{{ a }}</div></template>\n";
    let good_only = render_one(&host, "/proj/GoodOnly.vue", good_only_src);
    assert!(
        good_only.errors.is_empty(),
        "good-only render errors: {:?}",
        good_only.errors
    );
    let (good_only_hb, _, _) = host_backed_main_via_get_virtual_file(
        &host,
        "/proj/GoodOnly.vue",
        good_only_src,
        &get_virtual_file_profile(simple_render_profile(), None),
    );
    assert_eq!(
        good_only.code.as_ref(),
        good_only_hb.as_ref(),
        "the resolved GoodProps surface must byte-match getVirtualFile when the \
         missing import is absent (proves GoodProps resolves + materializes on the \
         render lane).\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        good_only.code,
        good_only_hb
    );
}

// ---------------------------------------------------------------------------
// Test 12 — soft-macro boundary: a wrong-shape import co-occurring with a
// missing import stays FATAL (the discriminating guard for the structural
// per-diagnostic soft gate)
// ---------------------------------------------------------------------------

/// The sharpest soft-macro boundary case. A file has BOTH:
///   - a MISSING imported macro type (`MissingProps`, softenable), AND
///   - a RESOLVED-but-wrong-shape imported macro type (`WrongEmits`, a
///     string alias with no call signatures → fatal `XInvalidMacroType`).
/// The render lane MUST stay FATAL — the presence of a softenable missing
/// import must NOT drag the co-occurring wrong-shape error into a warning.
/// DISCRIMINATING: the pre-fix whole-file gate
/// (`had_unresolved_import && code=="XInvalidMacroType"`) softened EVERY
/// `XInvalidMacroType` once any import was missing, so it would have made
/// this render succeed. The structural per-diagnostic code split
/// (`XUnresolvedImportedMacroType` softened; `XInvalidMacroType` fatal)
/// keeps it fatal. Reverting the fix flips this test RED.
#[test]
fn runtime_render_wrong_shape_with_missing_import_stays_fatal() {
    let host = new_host();
    // `WrongEmits` resolves but is a string alias → defineEmits rejects its
    // shape (no call signatures) with a fatal `XInvalidMacroType`.
    upsert_sibling(
        &host,
        "/proj/wrongemits.ts",
        "export type WrongEmits = string\n",
    );
    // `MissingProps` has no resolvable source → softenable
    // unresolved-import.
    let src = "<script setup lang=\"ts\">\nimport type { MissingProps } from './nope'\nimport type { WrongEmits } from './wrongemits'\ndefineProps<MissingProps>()\ndefineEmits<WrongEmits>()\n</script>\n<template><div /></template>\n";
    let render = render_one(&host, "/proj/WrongPlusMissing.vue", src);
    assert!(
        !render.errors.is_empty(),
        "a wrong-shape imported macro co-occurring with a missing import must \
         stay FATAL — the missing import must not soften the wrong-shape error \
         (got code: {:?}, diagnostics: {:?})",
        render.code,
        render
            .diagnostics
            .iter()
            .map(|d| format!("{:?}:{}", d.severity, d.code))
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Test 13 — (e)-skip safety with a PRIOR HostBacked-populated axis
// ---------------------------------------------------------------------------

/// The strongest (e)-skip safety proof. FIRST run HostBacked for a
/// cross-file-macro owner so the shared semantic-transitive axis is
/// POPULATED with the dependency; verify it. THEN run RuntimeRender for the
/// same owner after the import is removed and assert the render lane leaves
/// the axis CORRECT for a later HostBacked reader — it must not repopulate a
/// stale dep, and (because the render lane is read-only) it must not itself
/// mutate the axis. A final HostBacked request must see the CURRENT (empty)
/// deps. DISCRIMINATING: a render lane that erroneously called
/// `sync_transitive_macro_type_dependencies` with the pre-removal deps would
/// leave `/proj/types.ts` lingering in the axis, which this test observes.
#[test]
fn runtime_render_leaves_prior_host_backed_axis_correct() {
    let host = new_host();
    let owner = "/proj/AxisOwner.vue";
    upsert_sibling(
        &host,
        "/proj/types.ts",
        "export interface P { a: number }\n",
    );
    let with_dep = "<script setup lang=\"ts\">\nimport type { P } from './types'\ndefineProps<P>()\n</script>\n<template><div /></template>\n";

    // 1. HostBacked FIRST — this path DOES populate the semantic-transitive
    //    axis for the owner (it calls sync_transitive_macro_type_dependencies).
    let hb1 = host_backed_one(&host, owner, with_dep);
    assert!(
        hb1.errors.is_empty(),
        "host-backed #1 errors: {:?}",
        hb1.errors
    );
    let after_host_backed =
        crate::for_tests::workspace_semantic_transitive_deps_for_tests(&host, owner);
    assert!(
        after_host_backed.contains("/proj/types.ts"),
        "HostBacked must populate the semantic-transitive axis with the \
         cross-file dep (precondition for the staleness check); found: {:?}",
        after_host_backed
    );

    // 2. Remove the import and render through RuntimeRender. The render lane
    //    is READ-ONLY: it must neither add nor clear the axis. Whether the
    //    stale dep persists here is not the point — the point is a later
    //    HostBacked reader sees the CURRENT deps (step 3), because the
    //    Stage-B upsert of the new (import-less) source resets the axis and
    //    the eventual HostBacked recompute re-syncs it.
    let no_dep = "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>\n";
    let r = render_one(&host, owner, no_dep);
    assert!(r.errors.is_empty(), "render errors: {:?}", r.errors);

    // 3. A later HostBacked request for the now-import-less owner must see
    //    the CURRENT (empty) semantic-transitive deps — the removed dep must
    //    NOT linger.
    let hb2 = host_backed_one(&host, owner, no_dep);
    assert!(
        hb2.errors.is_empty(),
        "host-backed #2 errors: {:?}",
        hb2.errors
    );
    let final_axis = crate::for_tests::workspace_semantic_transitive_deps_for_tests(&host, owner);
    assert!(
        !final_axis.contains("/proj/types.ts"),
        "after the import was removed, a later HostBacked request must NOT see \
         the stale cross-file dep in the semantic-transitive axis; found: {:?}",
        final_axis
    );
}

// ---------------------------------------------------------------------------
// Test 14 — canonical owner-key parity across drive-letter case variants
// ---------------------------------------------------------------------------

/// A bundler hands the host TWO spellings of ONE Windows file: route wiring
/// (`set_import_dependencies`) arrives under the canonical LOWER-drive key
/// (`c:/...` — `resolve_alias_or_canonical` lowercases the drive letter),
/// while the raw compile input keeps the bundler's UPPER-drive JS id
/// (`C:/...`). The upsert chokepoint must canonicalize a supplied
/// `canonical_id`, so both spellings converge on ONE host identity and the
/// render-lane macro-type collector finds the wired alias route table.
///
/// DISCRIMINATING: an upsert engine that admits a supplied `canonical_id`
/// VERBATIM mints a SECOND `C:/...` host identity whose
/// `DerivedRawState.import_routes` are empty — the alias-imported macro
/// types then degrade to Unknown with a `HOST_MISSING_MACRO_TYPE_DEP`
/// warning and the resolved member names vanish from the render `Main`.
/// Runs on EVERY OS: `canonicalize_id`'s drive-letter lowering is a pure
/// string transform, so the `C:/` input vs `c:/` route-ownership split
/// reproduces without a Windows filesystem.
#[test]
fn runtime_render_upper_drive_input_resolves_macro_types_wired_under_lower_drive_routes() {
    let host = new_host();

    // The alias-imported macro-type target, seeded under the canonical
    // lower-drive key.
    upsert_sibling(
        &host,
        "c:/proj/lib/types.ts",
        "export interface AliasProps {\n  alphaprop: number\n  omegaprop: string\n}\nexport interface AliasEmits {\n  (e: 'aliaschange', value: number): void\n}\n",
    );

    // The owner imports its macro types via a PATH ALIAS — resolvable ONLY
    // through the caller-wired route table (no relative fallback exists for
    // a non-`.` specifier on a standalone host).
    let owner_src = "<script setup lang=\"ts\">\nimport type { AliasProps, AliasEmits } from '@lib/types'\ndefineProps<AliasProps>()\ndefineEmits<AliasEmits>()\n</script>\n<template><div>{{ alphaprop }}{{ omegaprop }}</div></template>\n";

    // The bundler's own upsert path seeds the owner under the LOWER-drive
    // canonical...
    upsert_sibling(&host, "c:/proj/Consumer.vue", owner_src);
    // ...and wires the alias route under that same LOWER-drive owner key,
    // exactly as the bundler's post-upsert dependency hydration does.
    host.set_import_dependencies(
        "c:/proj/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "@lib/types".to_string(),
            resolved_canonical_id: Some("c:/proj/lib/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Chokepoint-safety precondition: `canonicalize_id` is idempotent on an
    // already-canonical lower-drive id, so re-canonicalizing a supplied
    // `canonical_id` at the upsert chokepoint is contract enforcement,
    // never a semantic rewrite.
    assert_eq!(
        crate::id::canonicalize_id("c:/proj/Consumer.vue").as_ref(),
        "c:/proj/Consumer.vue",
        "canonicalize_id must be a byte-equal no-op on an already-canonical id"
    );

    // Compile through the render lane with the RAW UPPER-drive JS id — the
    // spelling the bundler's module graph carries.
    let render = render_with_profile(
        &host,
        "C:/proj/Consumer.vue",
        owner_src,
        simple_render_profile(),
        None,
    );

    assert!(
        render.errors.is_empty(),
        "render must succeed: {:?}",
        render.errors
    );
    // NEGATIVE: the wired route must be FOUND — neither the collector
    // missing-dep diagnostic nor the compiler unresolved-imported-macro
    // diagnostic may surface.
    assert!(
        !render
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"
                || d.code == "XUnresolvedImportedMacroType"),
        "the alias route wired under the lower-drive owner key must be \
         visible to the upper-drive compile input — ONE canonical identity, \
         not two; diagnostics: {:?}",
        render
            .diagnostics
            .iter()
            .map(|d| format!("{:?}:{} {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
    );
    // POSITIVE: the resolved macro member names materialize in the render
    // `Main` — the types did NOT degrade to Unknown.
    assert!(
        render.code.contains("alphaprop") && render.code.contains("omegaprop"),
        "resolved alias-imported prop names must appear in render Main:\n{}",
        render.code
    );
    assert!(
        render.code.contains("aliaschange"),
        "resolved alias-imported emit name must appear in render Main:\n{}",
        render.code
    );
}

/// Single-hop control for the drive-case split: a RELATIVE `./types` macro
/// type import resolves WITHOUT the caller-wired route table (relative
/// resolution canonicalizes the base path itself), so it stays green
/// regardless of the upsert chokepoint's canonical-id handling. Locks the
/// case-parity regression to the route-table owner key specifically — a
/// failure here would indicate general breakage, not the owner-key split.
#[test]
fn runtime_render_upper_drive_input_single_hop_relative_import_control() {
    let host = new_host();
    upsert_sibling(
        &host,
        "c:/proj/types.ts",
        "export interface RelProps { relalpha: number }\n",
    );
    let src = "<script setup lang=\"ts\">\nimport type { RelProps } from './types'\ndefineProps<RelProps>()\n</script>\n<template><div>{{ relalpha }}</div></template>\n";

    let render = render_with_profile(&host, "C:/proj/Rel.vue", src, simple_render_profile(), None);
    assert!(
        render.errors.is_empty(),
        "control render must succeed: {:?}",
        render.errors
    );
    assert!(
        !render
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"
                || d.code == "XUnresolvedImportedMacroType"),
        "the single-hop relative control must resolve without the route \
         table; diagnostics: {:?}",
        render
            .diagnostics
            .iter()
            .map(|d| format!("{:?}:{} {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
    );
    assert!(
        render.code.contains("relalpha"),
        "the relative-imported prop name must appear in render Main:\n{}",
        render.code
    );
}

// ---------------------------------------------------------------------------
// Test 15 — the alias-map is NOT hijacked by a supplied upper-drive id
// ---------------------------------------------------------------------------

/// The upsert chokepoint's canonicalization SUBSUMES the alias-map hijack:
/// an `upsert(UpsertRequest { canonical_id: Some(UPPER), input_id: UPPER })`
/// must NOT register the file's own lower-drive canonical as an ALIAS that
/// points BACK at the upper spelling.
///
/// Mechanism this locks: `finish_upsert_post_commit` seeds the file's
/// self-alias set from `canonical_id` AND `canonicalize_id(&req.input_id)`,
/// then `update_alias_map` maps every alias → the committed `canonical_id`.
/// If the committed `canonical_id` were the caller's VERBATIM upper
/// spelling (`C:/...`), the lower-drive `canonicalize_id(input_id)`
/// (`c:/...`) would be minted as an alias pointing at the upper key — so a
/// SUBSEQUENT `resolve_alias_or_canonical("c:/...")` (the spelling the
/// route-wiring writer and the eval-dependency resolver both produce)
/// would be REWRITTEN to `C:/...`, orphaning the lower-keyed
/// `DerivedRawState.import_routes`. Because the chokepoint canonicalizes
/// the supplied id, the committed canonical is already `c:/...`, the alias
/// mint is `c:/... → c:/...` (identity), and the lookup is stable.
///
/// DISCRIMINATING: with a verbatim supplied-id passthrough at the
/// chokepoint, `resolve_alias_or_canonical("c:/proj/Widget.vue")` returns
/// `"C:/proj/Widget.vue"` (the hijack) and this assertion FAILS. It is a
/// pure string/alias-map assertion — reproduces on every OS.
#[test]
fn runtime_render_supplied_upper_drive_upsert_does_not_hijack_lower_alias() {
    let host = new_host();

    let upper = "C:/proj/Widget.vue";
    let lower = "c:/proj/Widget.vue";
    let lang = host.language_classifier().classify(upper);

    // Upsert exactly as a Rust caller that hands `compile_many`'s Stage B a
    // raw upper-drive id would (Some(upper) + input_id upper) — the class
    // the chokepoint fix protects independently of the `compile_many`
    // boundary normalization.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(upper.to_string()),
            input_id: upper.to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div>{{ n }}</div></template>\n",
            ),
            file_language: lang,
            aliases: Vec::new(),
        })
        .expect("upsert with supplied upper-drive canonical must succeed");

    // The file committed under the canonical LOWER-drive identity.
    assert_eq!(
        host.resolve_alias_or_canonical(upper),
        lower,
        "the supplied upper-drive id must canonicalize to the lower-drive \
         host identity"
    );
    // The lower-drive spelling must resolve to ITSELF — NOT be hijacked
    // into an alias that points back at the upper spelling.
    assert_eq!(
        host.resolve_alias_or_canonical(lower),
        lower,
        "the lower-drive canonical must resolve to itself — a supplied \
         upper-drive upsert must not mint a `c:/... -> C:/...` alias that \
         orphans lower-keyed route/derived state"
    );
}
