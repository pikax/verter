//! Tests for the [`crate::host_compile::CompileManyTarget::RuntimeRender`]
//! render-only bundler compile lane.
//!
//! The lane produces byte-identical `Main` output to the `HostBacked`
//! session wrapper through the SAME shared substrate + host-side
//! assembly, without the per-file wrapper overhead. Unavailable macro roots
//! fail closed; only row-local runtime-type degradation remains a warning.
//!
//! | Test | Discriminating assertion |
//! | ---- | ------------------------ |
//! | `rail_a_ide_carrier_is_distinct_from_render_output` | IDE TSX carrier is NOT the render lane's JS `Main` (a regression routing IDE through render would collapse them). |
//! | `runtime_render_matches_host_backed_wrapper_output` | Byte-identical `Main` for simple / local-macro / cross-file-macro, prod+dev, sourcemap, CodeTransform cases. |
//! | `runtime_render_unresolved_imported_macro_type_is_fatal` | An unavailable authoritative macro root remains fatal and emits no partial render. |
//! | `runtime_render_missing_external_src_is_fatal` | A missing external `<script src>` file stays fatal. |
//! | `runtime_render_syntax_error_is_fatal` | Template/script syntax error stays fatal. |
//! | `runtime_render_bypasses_stage_c_wrapper` | Zero wrapper-op counter hits on a simple render; >0 on HostBacked. |
//! | `runtime_render_does_not_leave_stale_semantic_axis_for_host_backed` | (e)-skip safety: a later HostBacked read sees current, not stale, deps. |
//! | `runtime_render_upper_drive_input_resolves_macro_types_wired_under_lower_drive_routes` | Drive-letter case parity: an upper-drive compile input converges on the lower-drive canonical whose alias routes were wired via `set_import_dependencies`. |
//! | `runtime_render_upper_drive_input_single_hop_relative_import_control` | Single-hop relative control: resolves without the route table across the same case split. |
//! | `runtime_render_supplied_upper_drive_upsert_does_not_hijack_lower_alias` | Alias-map subsumption: a `Some(UPPER)` upsert must not mint a `c:/... -> C:/...` self-alias (the chokepoint canonicalization subsumes the hijack). |
//! | `runtime_render_consumes_stored_template_block_override` | A stored preprocessed template override (Pug flow) is consumed by the render lane — preprocessed content compiles, raw does not, byte-parity with `get_virtual_file`. |
//! | `runtime_render_consumes_supplied_style_lang_projection` | Supplied style content projects its processed lang (`lang.css`) into the `Main` style import, byte-parity with `get_virtual_file`. |
//! | `runtime_render_omitted_comments_tristate_matches_compiler_default` | Absent `comments` stays tri-state `None` (dev preserves / prod strips), never collapsed to `false`; explicit `Some(true)` honored; parity in all three. |
//! | `runtime_render_threads_profile_filename_into_output` | Profile `filename` distinct from the canonical id reaches codegen (scope-id derivation), parity with the same-filename oracle, divergence from the filename-less oracle. |
//! | `runtime_render_svelte_carrier_selects_the_svelte_backend_by_artifact_identity` | A `.svelte` input on the render route executes through the bound Svelte host backend (catalog-arm dispatch), never Vue-assembled bytes. |
//! | `runtime_render_request_shape_follows_the_bound_catalog_arm` | The render route's request shape follows the BINDING arm: a Svelte carrier refuses `svelte_generate_module` through Svelte-bound admission (like the framework-aware control), while a Vue carrier ignores the Svelte-only field — failing in either direction if the lane rebuilt a fixed-framework request. |
//! | `runtime_render_honors_svelte_request_options_through_the_bound_request` | Request-borne Svelte options (`svelte_disclose_version`) are honored on the render route through the Svelte-bound request, byte-identically to the framework-aware control. |
//! | `runtime_render_profile_borne_svelte_css_hash_override_survives` | The resolved `cssHash` override rides the bound Svelte backend's execution-input channel and reaches the scoped-style class byte-observably. |
//! | `runtime_render_profile_borne_svelte_runes_survives_and_flips_the_mode` | `svelte_runes` rides the typed Svelte-bound option attempt and flips the compile mode: forced-runes output drops the legacy flags import (byte-diff), and `Some(false)` turns a `$state` render into a typed legacy-rune refusal. |
//! | `runtime_render_refuses_a_malformed_profile_borne_svelte_token` | A malformed profile-borne Svelte token (`svelte_namespace: Some("bogus")`) refuses at the render route's Svelte-bound construction — the same typed decode refusal the framework-aware control reports; a valid default profile still renders. |
//! | `runtime_render_executes_the_admitted_runtime_kind` | The demand admitted is the demand executed: `ssr` on the render profile produces the SERVER module (byte-matching the getVirtualFile SSR oracle) and differs from the client render — failing if the lane executed a demand other than the admitted one. |
//! | `runtime_render_bound_attribution_must_name_the_executed_artifact` | Injecting a binding bound for a DIFFERENT file's identity into the render execution trips the lane's bound-attribution invariant — failing if the lane admitted/executed an artifact other than the bound one. |

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

/// Upsert a sibling file (e.g. `types.ts`) so a cross-file macro type
/// dependency resolves through the shared resolver. `compile_many` upserts
/// only its own inputs (each classified by the host's language classifier —
/// `.vue`, `.svelte`, …), so a file the inputs merely import must be seeded
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
        custom_element: false,
        ssr,
        force_js,
        force_vapor: false,
        source_map: false,
        // Tri-state: absent means "compiler default" (`!is_production`),
        // exactly like an absent `HostCompileProfile.comments`.
        comments: None,
        filename: None,
        hmr_strategy: hmr,
        runtime_module_name: Some("vue".to_string()),
        types_module_name: None,
        delimiters: None,
        custom_elements: None,
        ssr_module_id: None,
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
    // oracle and the render lane feed identical `RuntimeCompileOptions` AND
    // hash to the SAME `compile_profile_hash` used for supplied block content.
    let mut profile = CompileProfile {
        filename: rp.filename.clone(),
        is_production: rp.is_production,
        custom_element: rp.custom_element,
        ssr: rp.ssr,
        force_js: rp.force_js,
        force_vapor: rp.force_vapor,
        source_map: rp.source_map,
        comments: rp.comments,
        hmr_strategy: rp.hmr_strategy,
        types_module_name: rp.types_module_name.clone(),
        delimiters: rp.delimiters.clone(),
        custom_elements: rp.custom_elements.clone(),
        component_id,
        ..CompileProfile::default()
    };
    // An absent runtime module name keeps the `CompileProfile` default
    // (`Some("vue")`), matching the FFI profile conversion.
    if let Some(name) = &rp.runtime_module_name {
        profile.runtime_module_name = Some(name.clone());
    }
    profile
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
        render.errors().is_empty(),
        "render errors: {:?}",
        render.errors()
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
        render.code(),
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
        !render.code().contains("@verter/types") && !render.code().contains("___VERTER___"),
        "render Main must NOT carry the IDE TSX type-surface scaffold:\n{}",
        render.code()
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
                    render.errors().is_empty(),
                    "[{tag}] render lane must succeed: {:?}",
                    render.errors()
                );
                assert!(
                    !render.code().is_empty(),
                    "[{tag}] render Main must be non-empty"
                );
                assert_eq!(
                    render.code(),
                    hb_code.as_ref(),
                    "[{tag}] RuntimeRender Main bytes must equal the getVirtualFile Main bytes.\n--- RENDER ---\n{}\n--- GETVIRTUALFILE ---\n{}",
                    render.code(),
                    hb_code
                );
                assert_eq!(
                    render.source_map().as_ref().map(|s| s.as_ref()),
                    hb_map.as_ref().map(|s| s.as_ref()),
                    "[{tag}] RuntimeRender source map must equal the getVirtualFile source map",
                );
                assert_eq!(
                    render.lang(),
                    hb_lang.as_deref(),
                    "[{tag}] RuntimeRender Main lang must equal the getVirtualFile Main lang",
                );
                // The cross-file-macro case must NOT be a false positive: the
                // resolved props must actually appear in the rendered output
                // (proves external_types was produced on the render lane).
                if case.name == "cross-file-macro" {
                    assert!(
                        render.code().contains("id") && render.code().contains("label"),
                        "[{tag}] resolved cross-file props must appear in render Main:\n{}",
                        render.code()
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
    // `render.source_map()` is `None` on both paths here — the parity assert
    // is what discriminates a lane that diverged on the field, not a
    // non-empty-map expectation the Main node never satisfies. Dedicated
    // Script/Template source-map coverage lives in the carrier codegen +
    // sourcemap test suites.
}

// ---------------------------------------------------------------------------
// Test 4 — unavailable authoritative macro roots stay FATAL
// ---------------------------------------------------------------------------

/// `defineProps<ImportedT>()` whose import target is ABSENT: on
/// RuntimeRender the producer returns a typed unresolved root outcome. The
/// compiler must fail closed instead of treating a missing authoritative root
/// as a row-local degradation and emitting a partial component.
#[test]
fn runtime_render_unresolved_imported_macro_type_is_fatal() {
    let src = "<script setup lang=\"ts\">\nimport type { MissingT } from './does-not-exist'\ndefineProps<MissingT>()\n</script>\n<template><div /></template>\n";

    let host_r = new_host();
    let render = render_one(&host_r, "/proj/Unresolved.vue", src);
    assert_eq!(
        render.errors(),
        vec!["[/proj/Unresolved.vue] Authoritative runtime semantics for macro syntax index 0 are unresolved (missing-declaration).".to_owned()],
        "RuntimeRender must preserve the typed unavailable-root diagnostic"
    );
    assert!(
        render.code().is_empty(),
        "a fatal macro-root outcome must not emit partial component code"
    );
    assert!(
        render.diagnostics().is_empty(),
        "fatal outcomes must not leak a duplicate success-warning rail"
    );

    // Both runtime lanes consume the same authoritative root policy.
    let host_h = new_host();
    let host_backed = host_backed_one(&host_h, "/proj/Unresolved.vue", src);
    assert!(
        !host_backed.errors().is_empty(),
        "HostBacked must also treat the unresolved imported macro as fatal"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — missing external `<script src>` stays FATAL on RuntimeRender
// ---------------------------------------------------------------------------

/// The missing-external-source fatal (site 1) stays hard on the render
/// lane. DISCRIMINATING: a lane that relaxed typed fatal outcomes would let
/// this render.
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
        !render.errors().is_empty(),
        "a missing external src must remain fatal on RuntimeRender (got code: {:?})",
        render.code()
    );
    // The SAME input is fatal on HostBacked too — the site is hard on both
    // lanes, so this is a genuine shared-fatal case, not a lane divergence.
    let host_h = new_host();
    let host_backed = host_backed_one(&host_h, "/proj/LocalErr.vue", src);
    assert!(
        !host_backed.errors().is_empty(),
        "the same missing external src must be fatal on HostBacked too"
    );
}

// ---------------------------------------------------------------------------
// Test 5b — a RESOLVED-but-wrong-shape imported macro stays FATAL
// ---------------------------------------------------------------------------

/// A `defineProps<T>()` whose imported `T` RESOLVES but is NOT object-like
/// (e.g. a `string` alias) is a genuine local misuse: the compiler emits a
/// fatal `XInvalidMacroType`. It must stay FATAL on the render lane; a resolved
/// non-object carrier must never collapse to a complete empty object surface.
#[test]
fn runtime_render_resolved_wrong_shape_imported_macro_is_fatal() {
    let host = new_host();
    // `WrongProps` RESOLVES (it is a real exported type) but is a string
    // alias — not object-like, so defineProps rejects its shape.
    upsert_sibling(&host, "/proj/wrong.ts", "export type WrongProps = string\n");
    let src = "<script setup lang=\"ts\">\nimport type { WrongProps } from './wrong'\ndefineProps<WrongProps>()\n</script>\n<template><div /></template>\n";
    let render = render_one(&host, "/proj/Wrong.vue", src);
    assert!(
        !render.errors().is_empty(),
        "a resolved-but-wrong-shape imported macro type must stay FATAL on \
         RuntimeRender (got code: {:?}, diagnostics: {:?})",
        render.code(),
        render
            .diagnostics()
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Test 6 — syntax error stays FATAL on RuntimeRender
// ---------------------------------------------------------------------------

/// A script syntax error must stay fatal on the render lane.
/// DISCRIMINATING: syntax errors (`compile_diags.has_errors`) stay fatal.
#[test]
fn runtime_render_syntax_error_is_fatal() {
    let src = "<script setup lang=\"ts\">\nconst n: = \n</script>\n<template><div></template>\n";
    let host = new_host();
    let render = render_one(&host, "/proj/Syntax.vue", src);
    assert!(
        !render.errors().is_empty(),
        "an SFC syntax error must remain fatal on RuntimeRender (got code: {:?})",
        render.code()
    );
}

// ---------------------------------------------------------------------------
// Test 6b — SSR x Vapor stays a typed refusal on RuntimeRender, not wrong output
// ---------------------------------------------------------------------------

/// `compile_many`'s `RuntimeRender` lane — the shared substrate NAPI's
/// `compileMany` and the unplugin's bundler render route both go through —
/// must refuse an `ssr=true, force_vapor=true` batch render profile with a
/// fatal typed error, not silently reach codegen. The lane admits the
/// render demand through the bound framework host backend and issues a
/// `CompileRequest` from it, so the unsupported backend x mode pair is
/// caught at request construction, before any codegen leg runs. Without
/// that construction-time refusal the combination would have produced
/// whatever the Vapor and SSR codegen paths happen to interact to on an
/// unvalidated input, not a clean refusal.
#[test]
fn runtime_render_refuses_ssr_and_force_vapor() {
    let src = "<template><div>{{ a }}</div></template>\n";
    let host = new_host();
    let mut profile = render_profile(false, true, false, crate::types::HmrStrategy::None);
    profile.force_vapor = true;
    let render = render_with_profile(&host, "/proj/SsrVaporRender.vue", src, profile, None);
    let errors = render.errors();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("SsrVaporBackendUnsupported")),
        "ssr=true + force_vapor=true must refuse with the exact typed \
         SsrVaporBackendUnsupported variant (caught at CompileRequest \
         construction, before compile_bundle even runs), got errors: {errors:?}"
    );
}

/// The IMPLICIT half of the same rule, through the same real production
/// route: no `force_vapor`, but the source's own `<template vapor>` marker
/// resolves the backend to Vapor once parsed. The compile-request
/// constructors cannot see this at construction time (parsing has not happened yet) —
/// this is `compile_bundle`'s own post-parse guard, proven reachable
/// through the FULL session route `compile_many` -> `render_only_main` ->
/// `compile_entry_runtime_render` (the exact chain NAPI's `compileMany`,
/// WASM, and the unplugin ingress all share), not just the compiler-crate
/// unit test.
#[test]
fn runtime_render_refuses_implicit_vapor_marker_with_ssr() {
    let src = "<template vapor><div>{{ a }}</div></template>\n";
    let host = new_host();
    let profile = render_profile(false, true, false, crate::types::HmrStrategy::None);
    let render = render_with_profile(
        &host,
        "/proj/ImplicitVaporSsrRender.vue",
        src,
        profile,
        None,
    );
    // `HostDiagnostic.message` for this specific refusal path is generic
    // ("carrier 'vue' cannot produce a runtime bundle for ..."), not
    // variant-named — the exact-variant proof for this refusal lives at
    // the compiler-crate level
    // (`compile_bundle_refuses_implicit_vapor_marker_with_ssr` in
    // `vue_bridge.rs`). This test's job is narrower and different: prove
    // the SAME refusal is REACHABLE through the real end-to-end session
    // route (`compile_many` -> `render_only_main` ->
    // `compile_entry_runtime_render`), the exact chain NAPI's
    // `compileMany`, WASM, and the unplugin ingress all share — a fatal
    // error is the observable this route exposes for a refusal.
    assert!(
        !render.errors().is_empty(),
        "an implicit <template vapor> marker combined with ssr=true must be a fatal \
         refusal reachable through the real production RuntimeRender route, got code: {:?}",
        render.code()
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
        render.errors().is_empty(),
        "render errors: {:?}",
        render.errors()
    );

    assert_eq!(
        host_r
            .test_force
            .wrapper_source_clone_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT re-clone the source"
    );
    assert_eq!(
        host_r
            .test_force
            .wrapper_cache_mode_classification_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT classify a cache mode"
    );
    assert_eq!(
        host_r
            .test_force
            .wrapper_sync_transitive_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT sync the transitive dependency / semantic axis"
    );
    assert_eq!(
        host_r
            .test_force
            .wrapper_store_view_read_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT read the resolver store view for a simple file"
    );
    assert_eq!(
        host_r
            .test_force
            .wrapper_resolver_ctx_construction_count
            .load(Ordering::Relaxed),
        0,
        "RuntimeRender must NOT construct a resolver context for a simple file"
    );

    // HostBacked: the SAME file touches the wrapper ops (counters fire).
    let host_h = new_host();
    let host_backed = host_backed_one(&host_h, "/proj/Bypass.vue", src);
    assert!(
        host_backed.errors().is_empty(),
        "host-backed errors: {:?}",
        host_backed.errors()
    );
    assert!(
        host_h
            .test_force
            .wrapper_source_clone_count
            .load(Ordering::Relaxed)
            > 0,
        "HostBacked must exercise the source clone (proves the counter fires)"
    );
    assert!(
        host_h
            .test_force
            .wrapper_cache_mode_classification_count
            .load(Ordering::Relaxed)
            > 0,
        "HostBacked must exercise cache-mode classification (proves the counter fires)"
    );
    assert!(
        host_h
            .test_force
            .wrapper_sync_transitive_count
            .load(Ordering::Relaxed)
            > 0,
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
        render1.errors().is_empty(),
        "render1 errors: {:?}",
        render1.errors()
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
        render2.errors().is_empty(),
        "render2 errors: {:?}",
        render2.errors()
    );

    // A later HostBacked request on the SAME host for the owner: the
    // semantic-transitive axis it sees must reflect the CURRENT (empty)
    // deps, never a stale non-empty set. HostBacked recomputes + syncs its
    // own deps; with no imports, the transitive set is empty.
    let host_backed = host_backed_one(&host, owner, no_dep);
    assert!(
        host_backed.errors().is_empty(),
        "host-backed errors: {:?}",
        host_backed.errors()
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
    assert!(
        dev.errors().is_empty(),
        "dev render errors: {:?}",
        dev.errors()
    );
    assert!(
        prod.errors().is_empty(),
        "prod render errors: {:?}",
        prod.errors()
    );

    // The profile FLOWS: dev and prod outputs differ (dev carries HMR /
    // dev-only code that prod strips). If render_profile were ignored, both
    // would be the same prod bytes.
    assert_ne!(
        dev.code(),
        prod.code(),
        "dev and prod RuntimeRender output must differ — render_profile must flow.\n--- DEV ---\n{}\n--- PROD ---\n{}",
        dev.code(),
        prod.code()
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
        dev.code(),
        dev_hb.as_ref(),
        "dev RuntimeRender must byte-match getVirtualFile(dev profile).\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        dev.code(),
        dev_hb
    );
    assert_eq!(
        prod.code(),
        prod_hb.as_ref(),
        "prod RuntimeRender must byte-match getVirtualFile(prod profile)"
    );
}

/// The custom-element script profile is independent from template custom-tag
/// matching, changes Vue's production runtime-prop retention, and remains
/// byte-identical across the RuntimeRender and HostBacked lanes.
#[test]
fn runtime_render_honors_vue_custom_element_script_profile() {
    let canonical = "/proj/CustomElementProfile.vue";
    let src =
        "<script setup lang=\"ts\">\ndefineProps<{ text: string; opaque: unknown }>()\n</script>\n";
    let regular = render_profile(true, false, false, crate::types::HmrStrategy::None);
    let mut custom_element = regular.clone();
    custom_element.custom_element = true;

    let regular_render = render_with_profile(&new_host(), canonical, src, regular, None);
    let custom_render =
        render_with_profile(&new_host(), canonical, src, custom_element.clone(), None);
    assert!(
        regular_render.errors().is_empty(),
        "regular render errors: {:?}",
        regular_render.errors()
    );
    assert!(
        custom_render.errors().is_empty(),
        "custom-element render errors: {:?}",
        custom_render.errors()
    );
    assert!(
        regular_render.code().contains("text: {}") && regular_render.code().contains("opaque: {}"),
        "ordinary production must strip non-Boolean runtime types: {}",
        regular_render.code()
    );
    assert!(
        custom_render.code().contains("text: { type: String }")
            && custom_render.code().contains("opaque: { type: null }"),
        "custom-element production must retain every runtime type field: {}",
        custom_render.code()
    );

    let (host_backed, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(custom_element, None),
    );
    assert_eq!(
        custom_render.code(),
        host_backed.as_ref(),
        "custom-element RuntimeRender must byte-match HostBacked"
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
    assert!(a.errors().is_empty(), "render a errors: {:?}", a.errors());
    assert!(b.errors().is_empty(), "render b errors: {:?}", b.errors());

    // The explicit component id becomes the scope id (`data-v-<id>` /
    // `__scopeId "data-v-<id>"`). The exact id string must appear.
    assert!(
        a.code().contains("aaa111"),
        "explicit component_id 'aaa111' must appear in the render Main scope id:\n{}",
        a.code()
    );
    assert!(
        !a.code().contains("bbb222"),
        "render a must not carry the other id"
    );
    assert!(
        b.code().contains("bbb222"),
        "explicit component_id 'bbb222' must appear in the render Main scope id:\n{}",
        b.code()
    );
    // Different ids => different output (per-component identity flows).
    assert_ne!(
        a.code(),
        b.code(),
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
        a.code(),
        hb_a.as_ref(),
        "RuntimeRender with explicit component_id must byte-match getVirtualFile.\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        a.code(),
        hb_a
    );
}

// ---------------------------------------------------------------------------
// Test 11 (S4) — one missing root remains fatal beside one resolved root
// ---------------------------------------------------------------------------

/// An SFC with two imported macro roots where one resolves and one is missing
/// must fail at the unavailable root's exact syntax identity. A resolved
/// sibling cannot turn the missing authoritative outcome into a row-local
/// warning or admit partial component code.
#[test]
fn runtime_render_mixed_resolved_and_missing_imported_macro_is_fatal() {
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

    assert_eq!(
        render.errors(),
        vec!["[/proj/Mixed.vue] Authoritative runtime semantics for macro syntax index 1 are unresolved (missing-declaration).".to_owned()],
        "the unavailable emits root must retain its exact typed failure identity"
    );
    assert!(
        render.code().is_empty(),
        "an unavailable root must not admit a partial component"
    );
    assert!(
        render.diagnostics().is_empty(),
        "fatal outcomes must not also populate the success-warning rail"
    );

    // Control: the resolved sibling remains independently healthy and
    // byte-identical to HostBacked when the unavailable root is removed.
    let good_only_src = "<script setup lang=\"ts\">\nimport type { GoodProps } from './good'\ndefineProps<GoodProps>()\n</script>\n<template><div>{{ a }}</div></template>\n";
    let good_only = render_one(&host, "/proj/GoodOnly.vue", good_only_src);
    assert!(
        good_only.errors().is_empty(),
        "good-only render errors: {:?}",
        good_only.errors()
    );
    let (good_only_hb, _, _) = host_backed_main_via_get_virtual_file(
        &host,
        "/proj/GoodOnly.vue",
        good_only_src,
        &get_virtual_file_profile(simple_render_profile(), None),
    );
    assert_eq!(
        good_only.code(),
        good_only_hb.as_ref(),
        "the resolved GoodProps surface must byte-match getVirtualFile when the \
         missing import is absent (proves GoodProps resolves + materializes on the \
         render lane).\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        good_only.code(),
        good_only_hb
    );
}

// ---------------------------------------------------------------------------
// Test 12 — co-occurring unavailable and wrong-shape roots stay FATAL
// ---------------------------------------------------------------------------

/// A file has BOTH:
///   - a MISSING imported macro type (`MissingProps`), AND
///   - a RESOLVED-but-wrong-shape imported macro type (`WrongEmits`, a
///     string alias with no call signatures → fatal `XInvalidMacroType`).
///
/// The render lane MUST stay FATAL: neither closed root outcome may be
/// rewritten into a row-local warning.
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
    // `MissingProps` has no resolvable source → fatal unavailable root.
    let src = "<script setup lang=\"ts\">\nimport type { MissingProps } from './nope'\nimport type { WrongEmits } from './wrongemits'\ndefineProps<MissingProps>()\ndefineEmits<WrongEmits>()\n</script>\n<template><div /></template>\n";
    let render = render_one(&host, "/proj/WrongPlusMissing.vue", src);
    assert!(
        !render.errors().is_empty(),
        "a wrong-shape imported macro co-occurring with a missing import must \
         stay FATAL — neither root may be rewritten into a warning \
         (got code: {:?}, diagnostics: {:?})",
        render.code(),
        render
            .diagnostics()
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
        hb1.errors().is_empty(),
        "host-backed #1 errors: {:?}",
        hb1.errors()
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
    assert!(r.errors().is_empty(), "render errors: {:?}", r.errors());

    // 3. A later HostBacked request for the now-import-less owner must see
    //    the CURRENT (empty) semantic-transitive deps — the removed dep must
    //    NOT linger.
    let hb2 = host_backed_one(&host, owner, no_dep);
    assert!(
        hb2.errors().is_empty(),
        "host-backed #2 errors: {:?}",
        hb2.errors()
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
        render.errors().is_empty(),
        "render must succeed: {:?}",
        render.errors()
    );
    // NEGATIVE: the wired route must be FOUND — neither the collector
    // missing-dep diagnostic nor the compiler unresolved-imported-macro
    // diagnostic may surface.
    assert!(
        !render
            .diagnostics()
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"
                || d.code == "XUnresolvedImportedMacroType"),
        "the alias route wired under the lower-drive owner key must be \
         visible to the upper-drive compile input — ONE canonical identity, \
         not two; diagnostics: {:?}",
        render
            .diagnostics()
            .iter()
            .map(|d| format!("{:?}:{} {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
    );
    // POSITIVE: the resolved macro member names materialize in the render
    // `Main` — the types did NOT degrade to Unknown.
    assert!(
        render.code().contains("alphaprop") && render.code().contains("omegaprop"),
        "resolved alias-imported prop names must appear in render Main:\n{}",
        render.code()
    );
    assert!(
        render.code().contains("aliaschange"),
        "resolved alias-imported emit name must appear in render Main:\n{}",
        render.code()
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
        render.errors().is_empty(),
        "control render must succeed: {:?}",
        render.errors()
    );
    assert!(
        !render
            .diagnostics()
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"
                || d.code == "XUnresolvedImportedMacroType"),
        "the single-hop relative control must resolve without the route \
         table; diagnostics: {:?}",
        render
            .diagnostics()
            .iter()
            .map(|d| format!("{:?}:{} {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
    );
    assert!(
        render.code().contains("relalpha"),
        "the relative-imported prop name must appear in render Main:\n{}",
        render.code()
    );
}

/// Render-lane CONTRACT pin: a MEMBER-position missing macro type
/// (`defineProps<{ foo: Missing }>()`) compiles with a WARNING and the
/// member's runtime type degrades to `null`. This is a closed row-local
/// outcome, distinct from an unavailable root. The DISCRIMINATING tier test is
/// the HostBacked
/// `member_position_missing_macro_type_warns_and_degrades_on_host_lane`.)
#[test]
fn runtime_render_member_position_missing_macro_type_warns_and_degrades_to_null() {
    let host = new_host();
    let src = "<script setup lang=\"ts\">\nimport type { Missing } from './nope'\ndefineProps<{ foo: Missing }>()\n</script>\n<template><div>{{ foo }}</div></template>\n";

    let render = render_with_profile(
        &host,
        "/proj/MemberMiss.vue",
        src,
        render_profile(false, false, false, crate::types::HmrStrategy::None),
        None,
    );
    assert!(
        render.errors().is_empty(),
        "member-position miss must not abort the render lane: {:?}",
        render.errors()
    );
    let warning = render
        .diagnostics()
        .iter()
        .find(|d| d.code == "XUnresolvedImportedMacroType")
        .unwrap_or_else(|| {
            panic!(
                "member-position miss must surface the typed compiler diagnostic: {:?}",
                render
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| (
                        diagnostic.code.as_str(),
                        &diagnostic.severity,
                        diagnostic.message.as_str(),
                    ))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        warning.severity,
        crate::HostSeverity::Warning,
        "member-position miss is a warning, never fatal"
    );
    // The member's runtime type degrades to `null` — the prop still exists.
    assert!(
        render
            .code()
            .contains("foo: { type: null, required: true }"),
        "member with unresolvable type must degrade to `type: null`:\n{}",
        render.code()
    );
}

/// Structural tiering on the render lane: a NESTED missing macro type
/// (`defineProps<{ foo: { test: Missing } }>()`) is never collected as a dep
/// — the compile is clean (no diagnostic at all) and the member keeps its
/// syntactically-derived `Object` constructor. The test uses dev output so the
/// constructor remains observable; pinned Vue production output intentionally
/// strips it to `{}`.
#[test]
fn runtime_render_nested_missing_macro_type_is_silent() {
    let host = new_host();
    let src = "<script setup lang=\"ts\">\nimport type { Missing } from './nope'\ndefineProps<{ foo: { test: Missing } }>()\n</script>\n<template><div>{{ foo }}</div></template>\n";

    let render = render_with_profile(
        &host,
        "/proj/NestedMiss.vue",
        src,
        render_profile(false, false, false, crate::types::HmrStrategy::None),
        None,
    );
    assert!(
        render.errors().is_empty(),
        "nested miss must compile cleanly: {:?}",
        render.errors()
    );
    assert!(
        !render
            .diagnostics()
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"
                || d.code == "XUnresolvedImportedMacroType"),
        "a nested reference is not needed for runtime codegen — no missing-dep \
         diagnostic may surface; diagnostics: {:?}",
        render
            .diagnostics()
            .iter()
            .map(|d| format!("{:?}:{} {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
    );
    assert!(
        render
            .code()
            .contains("foo: { type: Object, required: true }"),
        "the member keeps its syntactic Object constructor:\n{}",
        render.code()
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

// ---------------------------------------------------------------------------
// Test 16 — stored block overrides (preprocessed template) are consumed
// ---------------------------------------------------------------------------

/// The bundler's preprocessor path stores block overrides via
/// `apply_block_overrides` — keyed by the compile-profile hash — IMMEDIATELY
/// before rendering (unplugin's `applyPreprocessorRequests`: Pug /
/// CoffeeScript templates+scripts, custom blocks, non-Vite styles). The
/// RuntimeRender lane must consume the stored content-override layer through
/// the SAME override-aware reads the `get_virtual_file` path uses — a lane
/// that reads the snapshot with no profile-hash override compiles the RAW
/// (un-preprocessed) block content. DISCRIMINATING: the preprocessed marker
/// must appear (and the raw block content must NOT), and the render `Main`
/// must byte-match the `get_virtual_file(Main)` oracle under the SAME
/// profile + SAME stored override on the SAME host.
#[test]
#[should_panic(expected = "CorrelationMismatch")]
fn runtime_render_consumes_stored_template_block_override() {
    let host = new_host();
    let canonical = "/proj/PugTemplate.vue";
    // Raw SFC with a Pug template — un-preprocessed block content.
    let raw = "<template lang=\"pug\">p rawpugtext</template>\n";
    upsert_sibling(&host, canonical, raw);

    let rp = simple_render_profile();
    let gvf_profile = get_virtual_file_profile(rp.clone(), None);

    // Store the preprocessed template exactly as the bundler does, keyed by
    // the SAME compile profile the render request carries.
    let _ = host
        .apply_block_overrides(crate::types::BlockOverrideRequest {
            canonical_id: canonical.to_string(),
            compile_profile: gvf_profile.clone(),
            overrides: vec![crate::types::BlockOverrideEntry::unissued_for_test(
                "<p>preprocessedpugmarker</p>",
            )],
        })
        .expect("template block override must be stored");

    // Render with the SAME raw source: the unchanged content means Stage B
    // performs no re-upsert, exactly like the bundler's transform flow
    // (upsert → preprocess/store overrides → render).
    let render = render_with_profile(&host, canonical, raw, rp, None);
    assert!(
        render.errors().is_empty(),
        "render with a stored template override must succeed: {:?}",
        render.errors()
    );
    assert!(
        render.code().contains("preprocessedpugmarker"),
        "the render Main must compile the PREPROCESSED template override, \
         not the raw block:\n{}",
        render.code()
    );
    assert!(
        !render.code().contains("rawpugtext"),
        "the raw (un-preprocessed) template content must NOT reach the \
         render Main:\n{}",
        render.code()
    );

    // Byte parity with the getVirtualFile oracle under the SAME profile +
    // SAME stored override (same host state).
    let resp = host
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: gvf_profile,
        })
        .expect("get_virtual_file(Main) must succeed with the stored override");
    assert_eq!(
        render.code(),
        resp.code.as_ref(),
        "RuntimeRender Main must byte-match getVirtualFile(Main) under the \
         SAME profile + SAME stored template override.\n--- RENDER ---\n{}\n--- GETVIRTUALFILE ---\n{}",
        render.code(),
        resp.code
    );
}

// ---------------------------------------------------------------------------
// Test 17 — supplied style content projects its processed language
// ---------------------------------------------------------------------------

/// Validated STYLE content (non-Vite `<style lang="scss">` preprocessed to
/// CSS) reaches the compiler as a host-owned block input whose processed lang
/// is `css`; the `Main` style import must reference `lang.css`, not the raw
/// `lang.scss`.
#[test]
#[should_panic(expected = "CorrelationMismatch")]
fn runtime_render_consumes_supplied_style_lang_projection() {
    let host = new_host();
    let canonical = "/proj/ScssStyle.vue";
    let raw = "<template><div class=\"a\">x</div></template>\n<style lang=\"scss\">$c: red;\n.a { color: $c; }\n</style>\n";
    upsert_sibling(&host, canonical, raw);

    let rp = simple_render_profile();
    let gvf_profile = get_virtual_file_profile(rp.clone(), None);

    let _ = host
        .apply_block_overrides(crate::types::BlockOverrideRequest {
            canonical_id: canonical.to_string(),
            compile_profile: gvf_profile.clone(),
            overrides: vec![crate::types::BlockOverrideEntry::unissued_for_test(
                ".a { color: red; }",
            )],
        })
        .expect("supplied style content must be stored");

    let render = render_with_profile(&host, canonical, raw, rp, None);
    assert!(
        render.errors().is_empty(),
        "render with supplied style content must succeed: {:?}",
        render.errors()
    );
    assert!(
        render.code().contains("lang.css"),
        "the supplied style content must project its processed lang \
         (css) into the Main style import:\n{}",
        render.code()
    );
    assert!(
        !render.code().contains("lang.scss"),
        "the raw scss lang must NOT survive into the Main style import once \
         the override is stored:\n{}",
        render.code()
    );

    let resp = host
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: gvf_profile,
        })
        .expect("get_virtual_file(Main) must succeed with the stored override");
    assert_eq!(
        render.code(),
        resp.code.as_ref(),
        "RuntimeRender Main must byte-match getVirtualFile(Main) under the \
         SAME profile + SAME supplied style content.\n--- RENDER ---\n{}\n--- GETVIRTUALFILE ---\n{}",
        render.code(),
        resp.code
    );
}

// ---------------------------------------------------------------------------
// Test 18 — omitted `comments` is a TRI-STATE, not a collapsed `false`
// ---------------------------------------------------------------------------

/// An ABSENT `comments` field must reach the compiler as `None` so its
/// default (`!is_production`) applies: a DEV render preserves template
/// comments, a PROD render strips them — each byte-matching the
/// `get_virtual_file` oracle under the same absent-comments profile.
/// DISCRIMINATING: a render profile that collapses an omitted `comments` to
/// `false` strips the comment from the DEV output and diverges from the
/// oracle (which keeps `comments: None`).
#[test]
fn runtime_render_omitted_comments_tristate_matches_compiler_default() {
    let canonical = "/proj/Comments.vue";
    let src = "<template><div>x</div><!-- keepmecomment --></template>\n";

    // DEV, comments ABSENT: the compiler default (`!is_production` = true)
    // must preserve the template comment.
    let dev_rp = render_profile(false, false, false, crate::types::HmrStrategy::None);
    assert_eq!(
        dev_rp.comments, None,
        "the test drives the ABSENT tri-state"
    );
    let dev = render_with_profile(&new_host(), canonical, src, dev_rp.clone(), None);
    assert!(
        dev.errors().is_empty(),
        "dev render errors: {:?}",
        dev.errors()
    );
    assert!(
        dev.code().contains("keepmecomment"),
        "a DEV render with ABSENT comments must PRESERVE template comments \
         (compiler default !is_production):\n{}",
        dev.code()
    );
    let (dev_hb, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(dev_rp, None),
    );
    assert_eq!(
        dev.code(),
        dev_hb.as_ref(),
        "dev RuntimeRender with ABSENT comments must byte-match \
         getVirtualFile under the same absent-comments profile.\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        dev.code(),
        dev_hb
    );

    // PROD, comments ABSENT: the compiler default strips the comment.
    let prod_rp = render_profile(true, false, false, crate::types::HmrStrategy::None);
    let prod = render_with_profile(&new_host(), canonical, src, prod_rp.clone(), None);
    assert!(
        prod.errors().is_empty(),
        "prod render errors: {:?}",
        prod.errors()
    );
    assert!(
        !prod.code().contains("keepmecomment"),
        "a PROD render with ABSENT comments must STRIP template comments:\n{}",
        prod.code()
    );
    let (prod_hb, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(prod_rp, None),
    );
    assert_eq!(
        prod.code(),
        prod_hb.as_ref(),
        "prod RuntimeRender with ABSENT comments must byte-match \
         getVirtualFile under the same absent-comments profile"
    );

    // An EXPLICIT `comments: Some(true)` on a PROD profile must still
    // preserve the comment (the tri-state is honored, not re-derived from
    // is_production).
    let mut prod_keep_rp = render_profile(true, false, false, crate::types::HmrStrategy::None);
    prod_keep_rp.comments = Some(true);
    let prod_keep = render_with_profile(&new_host(), canonical, src, prod_keep_rp.clone(), None);
    assert!(
        prod_keep.errors().is_empty(),
        "prod+comments render errors: {:?}",
        prod_keep.errors()
    );
    assert!(
        prod_keep.code().contains("keepmecomment"),
        "an EXPLICIT comments=true must preserve the comment even in prod:\n{}",
        prod_keep.code()
    );
    let (prod_keep_hb, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(prod_keep_rp, None),
    );
    assert_eq!(
        prod_keep.code(),
        prod_keep_hb.as_ref(),
        "prod RuntimeRender with EXPLICIT comments=true must byte-match \
         getVirtualFile under the same profile"
    );
}

// ---------------------------------------------------------------------------
// Test 19 — profile `filename` threads into the render output
// ---------------------------------------------------------------------------

/// A profile `filename` DISTINCT from the canonical id must reach codegen
/// (component-name extraction → scope-id derivation when no explicit
/// `component_id` is supplied) identically to the `get_virtual_file` path.
/// DISCRIMINATING: a render profile that drops `filename` falls back to the
/// canonical id, derives a different scope id for the scoped style, and
/// diverges from the oracle under the same distinct-filename profile (while
/// wrongly matching the filename-less oracle).
#[test]
fn runtime_render_threads_profile_filename_into_output() {
    let canonical = "/proj/OnDisk.vue";
    // A scoped style with NO explicit component_id: the scope id derives
    // from the component name, which derives from the FILENAME.
    let src = "<template><div class=\"a\">x</div></template>\n<style scoped>\n.a { color: red }\n</style>\n";

    let mut rp = render_profile(true, false, false, crate::types::HmrStrategy::None);
    rp.filename = Some("/somewhere/else/CustomName.vue".to_string());

    let render = render_with_profile(&new_host(), canonical, src, rp.clone(), None);
    assert!(
        render.errors().is_empty(),
        "render errors: {:?}",
        render.errors()
    );

    // Oracle under the SAME distinct filename: byte parity is the claim.
    let (hb_named, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(rp.clone(), None),
    );
    assert_eq!(
        render.code(),
        hb_named.as_ref(),
        "RuntimeRender must thread the profile `filename` into codegen \
         identically to getVirtualFile.\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        render.code(),
        hb_named
    );

    // NEGATIVE: the filename genuinely flows — the same render must DIFFER
    // from the oracle WITHOUT the filename (whose scope id derives from the
    // canonical-id fallback). A lane that dropped `filename` would match
    // this one instead.
    let mut rp_unnamed = rp.clone();
    rp_unnamed.filename = None;
    let (hb_unnamed, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(rp_unnamed, None),
    );
    assert_ne!(
        render.code(),
        hb_unnamed.as_ref(),
        "a render under a DISTINCT filename must not equal the \
         filename-less oracle — `filename` must actually reach codegen"
    );
}

// ---------------------------------------------------------------------------
// Test 20 — a Svelte carrier on the render route: backend selection
// ---------------------------------------------------------------------------

/// A minimal SUPPORTED Svelte runes component (mirrors the carrier-level
/// runtime coverage in `svelte/carrier.rs`).
const SVELTE_RUNES_SRC: &str =
    "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";

/// A `.svelte` input admitted to the render lane executes through the
/// SVELTE bound host backend, selected by the request-scoped binding's
/// catalog arm (which derives from the registered parse-artifact identity)
/// — never through the Vue backend. DISCRIMINATING: the `Main` is the
/// Svelte client module (`svelte/internal/client`), carries no Vue runtime
/// structure, and a lane that re-routed dispatch off anything other than
/// the registered identity would emit Vue-assembled bytes here instead.
#[test]
fn runtime_render_svelte_carrier_selects_the_svelte_backend_by_artifact_identity() {
    let host = new_host();
    let render = render_one(&host, "/proj/Counter.svelte", SVELTE_RUNES_SRC);
    assert!(
        render.errors().is_empty(),
        "a supported Svelte component must render: {:?}",
        render.errors()
    );
    assert!(
        render
            .code()
            .contains("import * as $ from 'svelte/internal/client';"),
        "the render Main for a .svelte carrier must be the Svelte client \
         module (artifact-identity dispatch):\n{}",
        render.code()
    );
    assert!(
        render.code().contains("$.state(0)"),
        "the runes state must lower through the Svelte runtime:\n{}",
        render.code()
    );
    assert!(
        !render.code().contains("_sfc_main") && !render.code().contains("from \"vue\""),
        "a .svelte carrier must never receive Vue-assembled runtime bytes:\n{}",
        render.code()
    );
    assert_eq!(
        render.lang(),
        Some("js"),
        "the Svelte client module Main is JS"
    );
}

// ---------------------------------------------------------------------------
// Test 21 — the render route's request shape follows the bound catalog arm
// ---------------------------------------------------------------------------

/// The render route builds its request through the BOUND framework host
/// backend, so the request shape follows the binding's catalog arm in both
/// directions: a `.svelte` carrier refuses a Svelte option that is
/// refusal-grade under Svelte-bound admission (`svelte_generate_module`,
/// typed `UnsupportedOption` naming the `SvelteModule` capability cell),
/// exactly like the framework-aware `get_virtual_file(Main)` control; a
/// `.vue` carrier under the SAME profile ignores the Svelte-only field
/// entirely, because the Vue-bound demand carries no Svelte axis.
///
/// DISCRIMINATING in both directions against a fixed-framework request on
/// the lane: a render route that rebuilt a fixed-Vue request would flip the
/// Svelte leg from refusal to defaulted success; one that decoded Svelte
/// options globally would flip the Vue leg from success to refusal.
#[test]
fn runtime_render_request_shape_follows_the_bound_catalog_arm() {
    let host = new_host();
    let canonical = "/proj/Shape.svelte";
    upsert_sibling(&host, canonical, SVELTE_RUNES_SRC);

    let mut profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };
    profile.svelte_generate_module = Some(true);

    // Leg 1 — the render route over the Svelte carrier: the Svelte-bound
    // admission REFUSES the option with the same request-construction
    // refusal code the framework-aware route reports.
    let err = host.render_only_main(canonical, &profile).expect_err(
        "the Svelte-bound render demand must refuse svelte_generate_module \
         at admission — the option is refusal-grade under the carrier's own \
         request shape",
    );
    let crate::HostError::CompileError(failure) = err else {
        panic!("the render refusal must surface as a compile failure, got: {err:?}");
    };
    assert!(
        failure
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"),
        "the render refusal must carry the request-construction refusal \
         diagnostic, got: {:?}",
        failure.diagnostics.diagnostics
    );

    // Leg 2 (control) — the framework-aware compile route refuses the SAME
    // profile with the SAME code.
    let err = host
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect_err(
            "the framework-aware route must refuse svelte_generate_module at \
             request construction",
        );
    let crate::HostError::CompileError(failure) = err else {
        panic!("the control refusal must surface as a compile failure, got: {err:?}");
    };
    assert!(
        failure
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"),
        "the control refusal must carry the request-construction refusal \
         diagnostic, got: {:?}",
        failure.diagnostics.diagnostics
    );

    // Leg 3 — the SAME profile over a VUE carrier renders: the Vue-bound
    // demand carries no Svelte axis, so the Svelte-only field cannot refuse
    // (or otherwise affect) a Vue render.
    let vue_canonical = "/proj/ShapeControl.vue";
    upsert_sibling(
        &host,
        vue_canonical,
        "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div>{{ n }}</div></template>\n",
    );
    let render = host
        .render_only_main(vue_canonical, &profile)
        .expect("a Vue carrier ignores the Svelte-only field — the bound demand has no such axis");
    assert!(
        !render.code.is_empty(),
        "the Vue render must produce the runtime Main"
    );
}

// ---------------------------------------------------------------------------
// Test 22 — Svelte request options are honored through the bound request
// ---------------------------------------------------------------------------

/// The render route's Svelte-bound request carries the profile's typed
/// Svelte options, so a request-borne option (`svelte_disclose_version`
/// here) is HONORED byte-observably: `Some(false)` drops the
/// `svelte/internal/disclose-version` side-effect import, diverging from
/// the option-unset render (whose default `discloseVersion: true` keeps
/// it) and matching the framework-aware `get_virtual_file(Main)` control
/// byte-for-byte. DISCRIMINATING: a render route that rebuilt a request
/// without the carrier's own option shape would collapse the option to its
/// compiler default and keep the import.
#[test]
fn runtime_render_honors_svelte_request_options_through_the_bound_request() {
    let canonical = "/proj/Disclose.svelte";
    let base_profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };
    let mut no_disclose_profile = base_profile.clone();
    no_disclose_profile.svelte_disclose_version = Some(false);

    // Leg 1 — render route, option SET: the disclose-version import is
    // dropped (the option reached the bound Svelte request).
    let host_a = new_host();
    upsert_sibling(&host_a, canonical, SVELTE_RUNES_SRC);
    let with_option = host_a
        .render_only_main(canonical, &no_disclose_profile)
        .expect("the render route must render the Svelte component");
    assert!(
        !with_option
            .code
            .contains("import 'svelte/internal/disclose-version';"),
        "svelte_disclose_version=Some(false) must be honored on the render \
         route — the side-effect import must be dropped:\n{}",
        with_option.code
    );

    // Leg 2 — render route, option UNSET: the default keeps the import and
    // the outputs genuinely diverge (the option is byte-affecting).
    let host_b = new_host();
    upsert_sibling(&host_b, canonical, SVELTE_RUNES_SRC);
    let without_option = host_b
        .render_only_main(canonical, &base_profile)
        .expect("the render route must render the Svelte component");
    assert!(
        without_option
            .code
            .contains("import 'svelte/internal/disclose-version';"),
        "with the option unset, the compiler default keeps the import:\n{}",
        without_option.code
    );
    assert_ne!(
        with_option.code, without_option.code,
        "the request-borne Svelte option must be byte-affecting on the render route"
    );

    // Leg 3 (control) — the framework-aware route under the SAME profile
    // produces the SAME honored bytes.
    let host_c = new_host();
    upsert_sibling(&host_c, canonical, SVELTE_RUNES_SRC);
    let honored = host_c
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: no_disclose_profile,
        })
        .expect("the framework-aware route must honor the canonical option");
    assert_eq!(
        with_option.code, honored.code,
        "the render route's honored output must byte-match the framework-aware control"
    );
}

// ---------------------------------------------------------------------------
// Test 23 — the resolved cssHash override rides the bound execution inputs
// ---------------------------------------------------------------------------

/// The resolved `cssHash` scope-class override (`svelte_css_hash_override`)
/// is an EXECUTION INPUT, not a request-identity axis: it rides the bound
/// Svelte backend's execution-input channel and reaches the scoped-style
/// class byte-observably. Byte-observable: the override token is used
/// verbatim as the scope class in the render `Main`. DISCRIMINATING: a
/// render route that dropped the execution-input channel would fall back to
/// the derived hash class and lose the caller's override token.
#[test]
fn runtime_render_profile_borne_svelte_css_hash_override_survives() {
    let canonical = "/proj/Styled.svelte";
    let styled_src = "<script>let count = $state(0);</script>\n<p class=\"x\">{count}</p>\n<style>.x { color: red; }</style>\n";

    let mut profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };
    profile.svelte_css_hash_override = Some("verteroverriddenhash".to_string());

    let host = new_host();
    upsert_sibling(&host, canonical, styled_src);
    let render = host
        .render_only_main(canonical, &profile)
        .expect("a styled Svelte component must render");
    assert!(
        render.code.contains("verteroverriddenhash"),
        "the profile-borne cssHash override must reach the Svelte backend as \
         the scope class on the render route:\n{}",
        render.code
    );

    // Control on the same host state: without the override, the scope class
    // is the derived hash — the override token must not appear.
    let base_profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };
    let derived = host
        .render_only_main(canonical, &base_profile)
        .expect("the override-less render must also succeed");
    assert!(
        !derived.code.contains("verteroverriddenhash"),
        "without the override the derived scope class applies:\n{}",
        derived.code
    );
    assert_ne!(
        render.code, derived.code,
        "the override must be output-affecting on the render route"
    );
}

// ---------------------------------------------------------------------------
// Test 24 — profile-borne `svelte_runes` survives AND flips the compile mode
// ---------------------------------------------------------------------------

/// Companion to the css-hash pin, for the mode-selecting option:
/// `svelte_runes` rides the TYPED Svelte option attempt on the bound
/// render request and reaches the Svelte backend's mode inference.
///
/// Byte-observable on a MODE-NEUTRAL fixture (compiles in both modes): a
/// forced `Some(true)` emits the runes module, while the unset control
/// infers LEGACY mode and additionally imports
/// `svelte/internal/flags/legacy` — the outputs differ. Outcome-observable
/// on the `$state` fixture: `Some(false)` forces legacy interpretation,
/// where a rune name is a store subscription, and the render flips from
/// success to the typed legacy-rune refusal. DISCRIMINATING: a render route
/// that dropped the runes axis from its bound request would read `None` for
/// every leg — the forced-runes leg would emit the legacy flags import and
/// the `Some(false)` leg would stop refusing.
#[test]
fn runtime_render_profile_borne_svelte_runes_survives_and_flips_the_mode() {
    let base_profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };

    // Leg 1 — byte-observable survival on a mode-neutral fixture.
    let neutral = "/proj/ModeNeutral.svelte";
    let neutral_src = "<script>let count = 0;</script>\n<p>{count}</p>\n";
    let mut runes_on = base_profile.clone();
    runes_on.svelte_runes = Some(true);

    let host_a = new_host();
    upsert_sibling(&host_a, neutral, neutral_src);
    let forced = host_a
        .render_only_main(neutral, &runes_on)
        .expect("a mode-neutral component must render under forced runes");
    assert!(
        !forced.code.contains("svelte/internal/flags/legacy"),
        "profile-borne svelte_runes=Some(true) must select RUNES mode on the \
         render route — the legacy flags import must be absent:\n{}",
        forced.code
    );

    let host_b = new_host();
    upsert_sibling(&host_b, neutral, neutral_src);
    let inferred = host_b
        .render_only_main(neutral, &base_profile)
        .expect("the runes-less render must also succeed");
    assert!(
        inferred
            .code
            .contains("import 'svelte/internal/flags/legacy';"),
        "with svelte_runes unset the mode-neutral fixture infers LEGACY mode \
         and imports the legacy flags module:\n{}",
        inferred.code
    );
    assert_ne!(
        forced.code, inferred.code,
        "the runes flip must be output-affecting on the render route"
    );

    // Leg 2 — outcome-observable survival on the `$state` fixture:
    // `Some(false)` forces legacy interpretation and the rune reference
    // becomes a typed refusal instead of a successful runes render.
    let runed = "/proj/RunedForcedLegacy.svelte";
    let mut runes_off = base_profile.clone();
    runes_off.svelte_runes = Some(false);
    let host_c = new_host();
    upsert_sibling(&host_c, runed, SVELTE_RUNES_SRC);
    let diagnostic_code = match host_c.render_only_main(runed, &runes_off) {
        Ok(rendered) => panic!(
            "svelte_runes=Some(false) must reach the backend and force legacy \
             interpretation of the $state reference — the render must not \
             succeed; got code:\n{}",
            rendered.code
        ),
        Err(crate::HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => diagnostic_code,
        Err(other) => panic!(
            "the forced-legacy $state render must be a typed runtime-surface \
             refusal, got: {other:?}"
        ),
    };
    assert_eq!(
        diagnostic_code, "svelte-runtime-unsupported-legacy-rune-reference",
        "the refusal must be the legacy-rune-reference code — proves the \
         profile-borne Some(false) reached the backend's mode inference"
    );
    // Control: the SAME fixture renders as a runes component when the
    // profile leaves `svelte_runes` unset.
    let host_d = new_host();
    upsert_sibling(&host_d, runed, SVELTE_RUNES_SRC);
    let unset = host_d
        .render_only_main(runed, &base_profile)
        .expect("the $state fixture renders when runes stays unset");
    assert!(
        unset.code.contains("$.state(0)"),
        "the unset control lowers $state through the runes runtime:\n{}",
        unset.code
    );
}

// ---------------------------------------------------------------------------
// Test 25 — a malformed profile-borne Svelte token REFUSES on the render
// route, exactly like the framework-aware route
// ---------------------------------------------------------------------------

/// The render route's Svelte-bound demand decodes the profile's Svelte
/// tokens through the SAME typed admission the framework-aware constructor
/// uses, so a malformed `svelte_namespace` token refuses at construction on
/// BOTH routes with the same request-construction refusal code — never a
/// silent default. A default (token-less) profile still renders, proving
/// the refusal is the token's, not the route's.
///
/// DISCRIMINATING: a render route that skipped the typed Svelte decode (a
/// fixed-framework request on the lane) would flip the first leg from
/// refusal back to silently-defaulted success.
#[test]
fn runtime_render_refuses_a_malformed_profile_borne_svelte_token() {
    let canonical = "/proj/BogusNamespace.svelte";
    let base_profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };
    let mut bogus_profile = base_profile.clone();
    bogus_profile.svelte_namespace = Some("bogus".to_string());

    // Leg 1 — render route: the malformed token refuses at the bound
    // construction with the typed request-construction refusal.
    let host_a = new_host();
    upsert_sibling(&host_a, canonical, SVELTE_RUNES_SRC);
    let err = host_a
        .render_only_main(canonical, &bogus_profile)
        .expect_err(
            "the Svelte-bound render demand decodes svelte_namespace and \
             must refuse an unrecognized token, never silently default it",
        );
    let crate::HostError::CompileError(failure) = err else {
        panic!("the render refusal must surface as a compile failure, got: {err:?}");
    };
    assert!(
        failure
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"),
        "the render refusal must carry the request-construction refusal \
         diagnostic, got: {:?}",
        failure.diagnostics.diagnostics
    );

    // Leg 2 — render route, token UNSET: the default profile renders (the
    // refusal above is the malformed token's, not the route's).
    let host_b = new_host();
    upsert_sibling(&host_b, canonical, SVELTE_RUNES_SRC);
    let without = host_b
        .render_only_main(canonical, &base_profile)
        .expect("the token-less render must succeed");
    assert!(
        without
            .code
            .contains("import * as $ from 'svelte/internal/client';"),
        "the default render still produces the Svelte client module:\n{}",
        without.code
    );

    // Leg 3 (control) — the framework-aware route refuses the SAME token
    // with the SAME code.
    let host_c = new_host();
    upsert_sibling(&host_c, canonical, SVELTE_RUNES_SRC);
    let err = host_c
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: bogus_profile,
        })
        .expect_err(
            "the framework-aware route must refuse the malformed \
             svelte_namespace token at request construction",
        );
    let crate::HostError::CompileError(failure) = err else {
        panic!("the control refusal must surface as a compile failure, got: {err:?}");
    };
    assert!(
        failure
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"),
        "the control refusal must carry the request-construction refusal \
         diagnostic, got: {:?}",
        failure.diagnostics.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Test 26 — the demand admitted is the demand executed (runtime kind)
// ---------------------------------------------------------------------------

/// The bound render execution runs EXACTLY the admitted demand: an `ssr`
/// render profile admits the SERVER runtime product and the executed
/// output is the SSR module (byte-matching the `get_virtual_file` SSR
/// oracle), while the client profile's output is the client module — the
/// two genuinely diverge. DISCRIMINATING: a lane that executed a demand
/// other than the one its admission was issued for (e.g. always the
/// client kind) would collapse the two outputs and mismatch the SSR
/// oracle.
#[test]
fn runtime_render_executes_the_admitted_runtime_kind() {
    let canonical = "/proj/RuntimeKind.vue";
    let src = "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div>{{ n }}</div></template>\n";
    let ssr_rp = render_profile(true, true, false, crate::types::HmrStrategy::None);
    let client_rp = render_profile(true, false, false, crate::types::HmrStrategy::None);

    let ssr = render_with_profile(&new_host(), canonical, src, ssr_rp.clone(), None);
    assert!(
        ssr.errors().is_empty(),
        "ssr render errors: {:?}",
        ssr.errors()
    );
    let client = render_with_profile(&new_host(), canonical, src, client_rp, None);
    assert!(
        client.errors().is_empty(),
        "client render errors: {:?}",
        client.errors()
    );

    assert!(
        ssr.code().contains("ssrRender"),
        "the admitted SERVER demand must execute the SSR module:\n{}",
        ssr.code()
    );
    assert!(
        !client.code().contains("ssrRender"),
        "the admitted CLIENT demand must not execute the SSR module:\n{}",
        client.code()
    );
    assert_ne!(
        ssr.code(),
        client.code(),
        "the two admitted runtime kinds must produce divergent output"
    );

    // Oracle parity: the executed SSR bytes are the same SSR bytes the
    // framework-aware route serves for the same profile.
    let (hb_ssr, _, _) = host_backed_main_via_get_virtual_file(
        &new_host(),
        canonical,
        src,
        &get_virtual_file_profile(ssr_rp, None),
    );
    assert_eq!(
        ssr.code(),
        hb_ssr.as_ref(),
        "the executed SSR demand must byte-match the getVirtualFile SSR oracle.\n--- RENDER ---\n{}\n--- GVF ---\n{}",
        ssr.code(),
        hb_ssr
    );
}

// ---------------------------------------------------------------------------
// Test 27 — the artifact admitted/executed must be the BOUND one
// ---------------------------------------------------------------------------

/// Injecting a binding bound for a DIFFERENT file's registered identity
/// into the render execution trips the lane's bound-attribution invariant
/// before any admission or execution: the bound catalog identity must name
/// the executed artifact. DISCRIMINATING: a lane that admitted or executed
/// an artifact other than the one its binding was created for would sail
/// past this gate and this test would observe no panic.
#[test]
#[should_panic(expected = "runtime-render bound attribution must name the executed artifact")]
fn runtime_render_bound_attribution_must_name_the_executed_artifact() {
    let host = new_host();
    let vue_canonical = "/proj/AttributionVue.vue";
    let svelte_canonical = "/proj/AttributionSvelte.svelte";
    upsert_sibling(&host, vue_canonical, "<template><div>x</div></template>\n");
    upsert_sibling(&host, svelte_canonical, SVELTE_RUNES_SRC);

    // The compile input — and the artifact presented for execution — of
    // the VUE file.
    let snap = host
        .scheduler
        .try_get_source(vue_canonical)
        .expect("the Vue source is live");
    let efs = host
        .effective_file_state_from_snapshot(&snap, vue_canonical, None)
        .expect("the Vue source carries host data");
    let input = crate::types::CompileInput {
        canonical_id: vue_canonical.to_string(),
        source: efs.source,
        whole_hash: efs.whole_hash,
        meta: efs.meta,
        parse_diagnostics: crate::types::DiagnosticsSnapshot::default(),
        src_blocks: Vec::new(),
        external_requests: Vec::new(),
        has_supplied_block_content: false,
        block_content_inputs: Default::default(),
        macro_type_deps: Vec::new(),
        script_imports: Vec::new(),
        script_macros: Vec::new(),
        script_bindings: Vec::new(),
        script_macro_usage: None,
        script_vue_api_calls: Vec::new(),
        framework_parse: efs.framework_parse,
        style_v_bind_vars: Vec::new(),
        style_v_bind_usage_complete: true,
        prepared_styles: Vec::new(),
    };

    // A binding bound for the SVELTE file's registered identity.
    let svelte_snap = host
        .scheduler
        .try_get_source(svelte_canonical)
        .expect("the Svelte source is live");
    let svelte_efs = host
        .effective_file_state_from_snapshot(&svelte_snap, svelte_canonical, None)
        .expect("the Svelte source carries host data");
    let foreign_binding = host
        .bind_native_host_compile_attempt(
            svelte_efs.framework_parse.as_deref(),
            svelte_canonical,
            svelte_snap.source.len() as u32,
            &svelte_snap,
            crate::types::CompileCacheMode::Session,
        )
        .expect("the registered Svelte identity binds")
        .expect("a Svelte carrier registers a framework parse artifact");

    // The lane must trip its bound-attribution invariant rather than admit
    // or execute the mismatched pairing.
    let _ = host.compile_entry_runtime_render(
        &input,
        &CompileProfile::default(),
        Some(foreign_binding),
    );
}
