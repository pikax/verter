//! The public transports, built and invoked.
//!
//! A transport is part of a capability cell's identity: source showing that
//! `VerterHost#getVirtualFile` forwards to `VerterHost::get_virtual_file` is
//! route-identity evidence, not an executed result. So each transport is BUILT
//! and DRIVEN over representative cases, and its answers are compared against
//! the in-process host route for the same typed request.
//!
//! Per transport the probe covers:
//!
//! * a SUCCESS publishing its products (a supported Svelte client component's
//!   module and its scoped CSS);
//! * a typed REFUSAL (the same component under a server profile) and the
//!   ABSENCE of every other product alongside it;
//! * the OPTIONAL-PRODUCT axis (source map on and off);
//! * OPTION CONVERSION (the public-API `mode` argument, the compile profile's
//!   `ssr` / `sourceMap` / `isProduction` fields, the node-kind `index`);
//! * SERIALIZATION shape (the typed two-arm public-API envelope, the node list);
//! * and an independent EXPORT ENUMERATION taken from the BUILT ARTIFACT's own
//!   surface — never from source — with every exported spelling either executed
//!   here or classified out of scope with a reason.
//!
//! The probes themselves are `packages/native/scripts/probe-transport-surface.mjs`
//! and `packages/wasm/scripts/probe-transport-surface.mjs`. Each is a plain
//! Node script that prints JSON; this module runs it and does the comparing, so
//! the expected values are the in-process host's own answers rather than
//! transcribed constants.
//!
//! ## Build prerequisites
//!
//! These tests need the transports BUILT, so the whole module sits behind the
//! opt-in `transport-authoritative` cargo feature — a FIRST-CLASS surface a gate
//! can enable, not an invisible-by-default one. With the feature on they FAIL
//! loudly — never silently skip — when an artifact is missing, naming its
//! producing build command, so an unbuilt transport can never read as a pass.
//!
//! ```text
//! CARGO_BUILD_JOBS=4 pnpm --filter @verter/native run build:debug
//! CARGO_BUILD_JOBS=4 cargo build -p verter_wasm --target wasm32-unknown-unknown \
//!   && wasm-bindgen --target web --out-dir packages/wasm/wasm --out-name verter_wasm \
//!      target/wasm32-unknown-unknown/debug/verter_wasm.wasm
//! cargo test -p verter_session --lib --features transport-authoritative \
//!   transport_route_equivalence -- --test-threads=1
//! ```
//!
//! WITHOUT `--features transport-authoritative` this module is not compiled
//! in, so a filter naming it matches ZERO tests and `cargo test` still exits 0.
//! Read the `running N tests` line, never the exit code.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{
    CompileProfile, HostConfig, HostError, PublicApiMode, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

/// One probe run's ceiling. A transport probe loads a large native module or
/// instantiates a wasm binary, so a few seconds is normal and a hang is not.
const PROBE_TIMEOUT: Duration = Duration::from_secs(300);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The exact sources the probe scripts use. Kept here as the SAME bytes so the
/// in-process comparison is asking the identical question.
const SUPPORTED_SVELTE: &str = "<script>\n  let count = $state(0);\n</script>\n\n<div class=\"root\">{count}</div>\n\n<style>\n  .root { color: red; }\n</style>\n";
const VUE_SFC: &str = "<script setup>\nconst props = defineProps({ label: { type: String, required: true } });\n</script>\n\n<template>\n  <button>{{ label }}</button>\n</template>\n";
/// A second supported Svelte component, distinct from [`SUPPORTED_SVELTE`].
const SUPPORTED_TWO: &str =
    "<script>\n  let total = $state(7);\n</script>\n\n<span class=\"total\">{total}</span>\n";
/// The committed fixture whose Svelte runtime surface the client backend
/// refuses (`$props()` read from the instance script).
const ADVANCED_RUNE_REFUSAL: &str = "<script>\n  let { label, disabled = false, ontoggle } = $props();\n\n  function onClick() {\n    ontoggle?.(!disabled);\n  }\n</script>\n\n<button {disabled} onclick={onClick}>{label}</button>\n";

fn batch_input(canonical: &str, source: &str) -> crate::host_compile::CompileBatchInput {
    crate::host_compile::CompileBatchInput {
        canonical_id: canonical.to_string(),
        source: Arc::from(source),
        requested_mode: None,
        component_id: None,
    }
}

/// The batch render profile matching the probe's own option object field for
/// field, so both routes are asked the SAME typed question.
fn batch_render_profile(
    ssr: bool,
    source_map: bool,
) -> crate::host_compile::CompileBatchRenderProfile {
    crate::host_compile::CompileBatchRenderProfile {
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

/// Run a probe script to completion under an explicit deadline, draining both
/// pipes on their own threads so a full pipe buffer cannot deadlock the loop
/// that is supposed to kill the child.
fn run_probe(script: &Path) -> (Option<i32>, String, String) {
    use std::io::Read;

    assert!(
        script.exists(),
        "the transport probe script is missing at {}",
        script.display()
    );
    let mut child = Command::new("node")
        .arg(script)
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("cannot run the transport probe: node failed ({error})"));

    let mut out_pipe = child.stdout.take().expect("stdout was piped");
    let mut err_pipe = child.stderr.take().expect("stderr was piped");
    let out_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = out_pipe.read_to_end(&mut buffer);
        buffer
    });
    let err_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = err_pipe.read_to_end(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait().expect("the child can be polled") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    let stdout =
        String::from_utf8_lossy(&out_reader.join().expect("the stdout reader joins")).into_owned();
    let stderr =
        String::from_utf8_lossy(&err_reader.join().expect("the stderr reader joins")).into_owned();
    assert!(status.is_some(), "the transport probe timed out:\n{stderr}");
    (status.and_then(|status| status.code()), stdout, stderr)
}

/// Drive one transport's probe and return its parsed result.
///
/// A transport that could not be loaded FAILS here with the producing build
/// command, so a missing artifact is an execution prerequisite reported as
/// such — never a silent skip and never a pass.
fn probe(transport: &str, script: &str, build_command: &str) -> Value {
    let (code, stdout, stderr) = run_probe(&repo_root().join(script));
    let record: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("{transport}: the probe emitted no JSON ({error}).\nstdout:\n{stdout}\nstderr:\n{stderr}")
    });
    assert_eq!(
        record["loaded"], true,
        "{transport}: the built artifact could not be loaded ({}). Build it first:\n  \
         {build_command}",
        record["error"]
    );
    assert_eq!(
        code,
        Some(0),
        "{transport}: the probe exited with {code:?}.\nstderr:\n{stderr}"
    );
    record
}

fn host_with(
    canonical: &str,
    source: &str,
    language: verter_language::FileLanguage,
) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {canonical}: {error:?}"));
    host
}

/// The profile the probes build through their transport option shape.
fn probe_profile(ssr: bool, source_map: bool) -> CompileProfile {
    CompileProfile {
        is_production: true,
        ssr,
        source_map,
        hmr_strategy: crate::types::HmrStrategy::None,
        ..CompileProfile::default()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum HostOutcome {
    Published {
        code: String,
        has_map: bool,
        lang: Option<String>,
    },
    Refused {
        diagnostic_code: String,
    },
    Missing,
}

fn host_node(
    host: &VerterHost,
    canonical: &str,
    kind: VirtualNodeKind,
    profile: &CompileProfile,
) -> HostOutcome {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(kind),
        compile_profile: profile.clone(),
    }) {
        Ok(response) => HostOutcome::Published {
            code: response.code.to_string(),
            has_map: response.source_map.is_some(),
            lang: response.lang.clone(),
        },
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => HostOutcome::Refused { diagnostic_code },
        Err(HostError::MissingVirtualNode { .. }) => HostOutcome::Missing,
        Err(other) => panic!("{canonical}: unmodelled host outcome {other:?}"),
    }
}

/// Assert one probe case equals the in-process host's answer for the same typed
/// request.
#[track_caller]
fn assert_case_matches_host(transport: &str, label: &str, case: &Value, expected: &HostOutcome) {
    match expected {
        HostOutcome::Published {
            code,
            has_map,
            lang,
        } => {
            assert_eq!(
                case["outcome"], "published",
                "{transport}/{label}: the host published but the transport returned {case}"
            );
            assert_eq!(
                case["code"].as_str(),
                Some(code.as_str()),
                "{transport}/{label}: the transport's bytes differ from the host route's"
            );
            assert_eq!(
                case["hasMap"], *has_map,
                "{transport}/{label}: map presence differs from the host route's"
            );
            assert_eq!(
                case["lang"].as_str(),
                lang.as_deref(),
                "{transport}/{label}: the reported output language differs"
            );
        }
        HostOutcome::Refused { diagnostic_code } => {
            assert_eq!(
                case["outcome"], "error",
                "{transport}/{label}: the host refused but the transport returned {case}"
            );
            let message = case["message"].as_str().unwrap_or_default();
            assert!(
                message.contains(diagnostic_code.as_str()),
                "{transport}/{label}: the transport's error does not carry the host's typed \
                 refusal code ({diagnostic_code}): {message}"
            );
        }
        HostOutcome::Missing => {
            // The two transports serialize a MISSING node differently — NAPI
            // returns a null response, WASM throws a typed
            // `HostError::MissingVirtualNode`. Both faithfully mean "no
            // product", and the divergence itself is pinned by
            // `the_transports_serialize_a_missing_node_differently` below, so
            // it is recorded rather than smoothed away here.
            let missing_shaped = case["outcome"] == "missing"
                || (case["outcome"] == "error"
                    && case["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("MissingVirtualNode")));
            assert!(
                missing_shaped,
                "{transport}/{label}: the host reported a missing node but the transport \
                 returned {case}"
            );
            // Whatever the shape, NO product crossed the boundary.
            assert_eq!(
                case["code"],
                Value::Null,
                "{transport}/{label}: the transport published code for a missing node"
            );
        }
    }
}

/// Every case the probes execute, compared against the in-process host.
fn assert_transport_matches_the_host_route(transport: &str, record: &Value) {
    // ── SUCCESS + the optional-product axis, on a supported Svelte component ──
    let svelte = host_with(
        "/probe/Ok.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    assert_case_matches_host(
        transport,
        "svelteMainWithMap",
        &record["cases"]["svelteMainWithMap"],
        &host_node(
            &svelte,
            "/probe/Ok.svelte",
            VirtualNodeKind::Main,
            &probe_profile(false, true),
        ),
    );
    assert_case_matches_host(
        transport,
        "svelteMainWithoutMap",
        &record["cases"]["svelteMainWithoutMap"],
        &host_node(
            &svelte,
            "/probe/Ok.svelte",
            VirtualNodeKind::Main,
            &probe_profile(false, false),
        ),
    );
    assert_case_matches_host(
        transport,
        "svelteStyleWithMap",
        &record["cases"]["svelteStyleWithMap"],
        &host_node(
            &svelte,
            "/probe/Ok.svelte",
            VirtualNodeKind::Style { index: 0 },
            &probe_profile(false, true),
        ),
    );
    // The optional-product axis is honoured across the boundary in BOTH
    // directions, and changes no module byte.
    assert_eq!(
        record["cases"]["svelteMainWithMap"]["hasMap"], true,
        "{transport}: a requested source map did not cross the transport boundary"
    );
    assert_eq!(
        record["cases"]["svelteMainWithoutMap"]["hasMap"], false,
        "{transport}: the transport published a source map that was never requested"
    );
    assert_eq!(
        record["cases"]["svelteMainWithMap"]["code"],
        record["cases"]["svelteMainWithoutMap"]["code"],
        "{transport}: the source-map axis changed the emitted module bytes"
    );

    // SERIALIZATION shape: the node list crosses as the typed kind/index pair.
    let listed = record["cases"]["svelteNodeList"]
        .as_array()
        .unwrap_or_else(|| panic!("{transport}: the node list did not serialize as an array"));
    let listed_kinds: Vec<(String, Option<u64>)> = listed
        .iter()
        .map(|node| {
            (
                node["kind"].as_str().unwrap_or("<absent>").to_string(),
                node["index"].as_u64(),
            )
        })
        .collect();
    assert_eq!(
        listed_kinds,
        vec![("main".to_string(), None), ("style".to_string(), Some(0)),],
        "{transport}: the node list's serialized shape moved"
    );
    assert_eq!(
        svelte.list_virtual_files("/probe/Ok.svelte"),
        vec![VirtualNodeKind::Main, VirtualNodeKind::Style { index: 0 }],
        "{transport}: the host route's node list moved, so the comparison above is stale"
    );

    // ── REFUSAL + artifact ABSENCE ──────────────────────────────────────────
    let server = host_with(
        "/probe/Server.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    let refusal = host_node(
        &server,
        "/probe/Server.svelte",
        VirtualNodeKind::Main,
        &probe_profile(true, true),
    );
    assert_eq!(
        refusal,
        HostOutcome::Refused {
            diagnostic_code: "svelte-runtime-unsupported-server-generate".to_string()
        },
        "{transport}: the host route no longer refuses the server profile, so this comparison \
         is measuring something else"
    );
    assert_case_matches_host(
        transport,
        "svelteServerRefusal",
        &record["cases"]["svelteServerRefusal"],
        &refusal,
    );
    // No CSS survives the refusal, across the boundary.
    assert_case_matches_host(
        transport,
        "svelteServerStyle",
        &record["cases"]["svelteServerStyle"],
        &host_node(
            &server,
            "/probe/Server.svelte",
            VirtualNodeKind::Style { index: 0 },
            &probe_profile(true, true),
        ),
    );
    assert!(
        record["cases"]["svelteServerStyle"]["code"] == Value::Null,
        "{transport}: a CSS product survived the runtime refusal across the transport: {}",
        record["cases"]["svelteServerStyle"]
    );

    // ── IDE/TSX: ensure + read, compared against the host's own answers ─────
    let ide_host = host_with(
        "/probe/Ide.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    let ide_profile = CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        source_map: true,
        hmr_strategy: crate::types::HmrStrategy::None,
        ..CompileProfile::default()
    };
    let host_ensured = ide_host
        .ensure_ide_compiled("/probe/Ide.svelte", &ide_profile)
        .unwrap_or_else(|error| panic!("{transport}: host ensure_ide_compiled failed: {error:?}"));
    assert_eq!(
        record["cases"]["ensureIdeCompiled"]["outcome"], "ok",
        "{transport}/ensureIdeCompiled: the transport errored: {}",
        record["cases"]["ensureIdeCompiled"]
    );
    assert_eq!(
        record["cases"]["ensureIdeCompiled"]["value"], host_ensured,
        "{transport}/ensureIdeCompiled: the transport's answer differs from the host's"
    );

    let host_ide = ide_host
        .get_ide("/probe/Ide.svelte", &ide_profile)
        .unwrap_or_else(|| panic!("{transport}: the host published no IDE product"));
    let transported = &record["cases"]["getIdeWithMap"];
    assert_eq!(
        transported["outcome"], "published",
        "{transport}/getIde: the transport published no IDE product: {transported}"
    );
    assert_eq!(
        transported["code"].as_str(),
        Some(host_ide.code.as_ref()),
        "{transport}/getIde: the transport's IDE bytes differ from the host's"
    );
    assert_eq!(
        transported["hasMap"],
        host_ide.source_map.is_some(),
        "{transport}/getIde: IDE map presence differs from the host's"
    );
    assert_eq!(
        transported["isJsx"], host_ide.is_jsx,
        "{transport}/getIde: the reported JSX flag differs from the host's"
    );
    // `get_ide` is a pure cached read keyed on the IDE-normalized profile: a
    // profile that was never ensured has nothing to hand back. The host says
    // the same, so the transport is not inventing an answer.
    let unensured_profile = CompileProfile {
        source_map: false,
        ..ide_profile.clone()
    };
    assert!(
        ide_host
            .get_ide("/probe/Ide.svelte", &unensured_profile)
            .is_none(),
        "{transport}: the host now serves an IDE product for a never-ensured profile, so the \
         comparison below is stale"
    );
    assert_eq!(
        record["cases"]["getIdeWithoutMap"]["outcome"], "missing",
        "{transport}/getIde: the transport answered for a never-ensured profile: {}",
        record["cases"]["getIdeWithoutMap"]
    );

    // ── OPTION CONVERSION: the public-API `mode` argument ───────────────────
    let vue = host_with(
        "/probe/Api.vue",
        VUE_SFC,
        verter_language::FileLanguage::vue(),
    );
    for (label, mode) in [
        ("vuePublicApiDefault", PublicApiMode::Public),
        ("vuePublicApiDeclaration", PublicApiMode::Declaration),
    ] {
        let expected = vue
            .get_public_api_with_mode("/probe/Api.vue", mode, None)
            .unwrap_or_else(|error| panic!("{transport}/{label}: the host route failed: {error:?}"))
            .unwrap_or_else(|| panic!("{transport}/{label}: the host route published nothing"));
        let case = &record["cases"][label];
        assert_eq!(
            case["outcome"], "published",
            "{transport}/{label}: the transport did not publish a declaration: {case}"
        );
        assert_eq!(
            case["code"].as_str(),
            Some(expected.ts_labeled_code().as_ref()),
            "{transport}/{label}: the transport's declaration bytes differ from the host's"
        );
        assert_eq!(
            case["error"],
            Value::Null,
            "{transport}/{label}: the typed envelope carries an error arm alongside a value"
        );
    }
    // The `mode` argument genuinely converted: the two modes produced DIFFERENT
    // surfaces, so a transport that dropped the argument would be caught.
    assert_ne!(
        record["cases"]["vuePublicApiDefault"]["code"],
        record["cases"]["vuePublicApiDeclaration"]["code"],
        "{transport}: both public-API modes produced identical bytes, so this probe cannot \
         detect a dropped `mode` argument"
    );
}

/// Every enumerated spelling belongs to EXACTLY ONE class.
///
/// A union of two lists cannot prove completeness: dropping a name from the
/// executed list stays green as long as the same name also sits in the
/// out-of-scope list. This is a PARTITION — in two classes, or in none, is a
/// failure — and each class is additionally required to name nothing the built
/// artifact does not export, so a stale row cannot silently absorb a dropped
/// one.
#[track_caller]
fn assert_partition(
    transport: &str,
    exported: &[String],
    executed: &[&str],
    out_of_scope: &[(&str, &str)],
    extra_out_of_scope: &[(&str, &str)],
) {
    use std::collections::BTreeSet;

    let exported_set: BTreeSet<&str> = exported.iter().map(String::as_str).collect();
    let executed_set: BTreeSet<&str> = executed.iter().copied().collect();
    let classified_set: BTreeSet<&str> = out_of_scope
        .iter()
        .chain(extra_out_of_scope.iter())
        .map(|(name, _)| *name)
        .collect();

    let both: Vec<&&str> = executed_set.intersection(&classified_set).collect();
    assert!(
        both.is_empty(),
        "{transport}: {} spelling(s) are classified BOTH executed and out-of-scope, so the \
         classification cannot prove completeness: {both:?}",
        both.len()
    );

    let unaccounted: Vec<&&str> = exported_set
        .iter()
        .filter(|name| !executed_set.contains(*name) && !classified_set.contains(*name))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "{transport}: the built artifact exports {} spelling(s) in NEITHER class: {unaccounted:?}",
        unaccounted.len()
    );

    let stale_executed: Vec<&&str> = executed_set
        .iter()
        .filter(|name| !exported_set.contains(*name))
        .collect();
    assert!(
        stale_executed.is_empty(),
        "{transport}: the executed list names {} spelling(s) the built artifact does not export: \
         {stale_executed:?}",
        stale_executed.len()
    );
    let stale_classified: Vec<&&str> = classified_set
        .iter()
        .filter(|name| !exported_set.contains(*name))
        .collect();
    assert!(
        stale_classified.is_empty(),
        "{transport}: the out-of-scope list names {} spelling(s) the built artifact does not \
         export: {stale_classified:?}",
        stale_classified.len()
    );
}

// ══════════════════════════════════════════════════════════════════════════
// NAPI
// ══════════════════════════════════════════════════════════════════════════

const NAPI_BUILD: &str = "CARGO_BUILD_JOBS=4 pnpm --filter @verter/native run build:debug";

/// Exported NAPI spellings this probe does NOT execute, each with the reason it
/// is out of the in-scope product surface.
///
/// This is one side of a PARTITION, not a fallback: [`assert_partition`]
/// requires every exported spelling to be in exactly one class, rejects any name
/// appearing in both, and rejects any row naming a spelling the built artifact
/// does not export.
const NAPI_OUT_OF_SCOPE: &[(&str, &str)] = &[
    (
        "analyzeWithAudit",
        "audit-record product, not a framework product surface",
    ),
    (
        "applyBlockOverrides",
        "block-content override input, not a product route",
    ),
    (
        "auditWorkspaceOp",
        "workspace audit op, not a framework product",
    ),
    ("close", "lifecycle"),
    (
        "collectResolvableModuleReferenceSpecifiers",
        "dependency-resolution input",
    ),
    (
        "computeCrossFileOptimizations",
        "cross-file optimizer, not a product route",
    ),
    ("configureProjects", "lifecycle/config"),
    ("evaluateTypeExpressionWithAudit", "type-resolution product"),
    ("evaluateTypes", "type-resolution product"),
    ("getAnalysis", "analysis product"),
    ("getAuditRecords", "audit records"),
    ("getBundlerBatchSummary", "audit aggregation"),
    ("getCodeActions", "IDE feature"),
    ("getDocumentStructure", "analysis product"),
    ("getDocumentSymbols", "IDE feature"),
    ("getLastAuditRecord", "audit records"),
    ("getLintRuleMetadata", "lint metadata"),
    ("getMetrics", "host metrics"),
    ("lint", "lint product"),
    ("listSymbols", "type-resolution product"),
    ("matchCssSelectors", "CSS analysis product"),
    ("remove", "lifecycle"),
    ("resolve", "id resolution"),
    ("resolveExports", "type-resolution product"),
    (
        "resolveFrameworkSurfaceWithAudit",
        "framework-surface wire operation",
    ),
    ("resolveImport", "import resolution"),
    (
        "resolveKnownModuleReferenceDependencies",
        "dependency resolution",
    ),
    ("resolveSymbolWithAudit", "type-resolution product"),
    ("resolveTypeWithAudit", "type-resolution product"),
    ("setImportDependencies", "dependency-resolution input"),
];

/// The `VerterHost` methods the NAPI probe actually executes.
const NAPI_EXECUTED: &[&str] = &[
    "compileWithAudit",
    "getVirtualFile",
    "listVirtualFiles",
    "getPublicApi",
    "compileMany",
    "ensureIdeCompiled",
    "getIde",
    "upsert",
];

#[test]
fn the_napi_transport_matches_the_in_process_host_route() {
    let record = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );
    assert_transport_matches_the_host_route("napi", &record);

    // The standalone CSS spelling crosses as its own product, and its
    // `sourcemap` axis is inert at the transport too — the same finding the
    // in-process route records.
    let css = &record["cases"]["processStyle"];
    // EQUALITY against the in-process compiler for the same options, not a
    // substring search in the transported bytes.
    let expected_css = verter_compiler::css::process_style(
        ".x{color:red}",
        &verter_compiler::css::ProcessStyleOptions {
            scope_id: "probe1234",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: true,
        },
    )
    .expect("the in-process CSS route processes the same block");
    assert_eq!(
        css["code"].as_str(),
        Some(expected_css.code.as_ref()),
        "napi/processStyle: the transported CSS differs from the in-process route's"
    );
    assert_eq!(
        css["hasMap"], false,
        "napi/processStyle: the `sourcemap` axis has become live at the transport; the recorded \
         route description must be updated"
    );
}

/// The batch route, driven through the NAPI boundary and compared item-for-item
/// against the in-process host's answers for the same typed request.
///
/// The batch shape is the one the in-process suite uses: a supported Svelte
/// component, the advanced-rune refusal input, and a second distinct supported
/// component — the refusal in the MIDDLE so a shifted or fanned-out result is
/// visible in both directions. Four option shapes are compared: the
/// runtime-render lane with and without maps, the host-backed lane, and the
/// server profile.
///
/// The expected values are the host's own answers, never transcribed constants.
#[test]
fn the_napi_batch_route_matches_the_in_process_batch_route_item_for_item() {
    use crate::host_compile::{CompileBatchOptions, CompileManyTarget};

    let record = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );

    // The exact inputs the probe sends, in the same order.
    let inputs = vec![
        batch_input("/napiBatch/One.svelte", SUPPORTED_SVELTE),
        batch_input("/napiBatch/Refused.svelte", ADVANCED_RUNE_REFUSAL),
        batch_input("/napiBatch/Two.svelte", SUPPORTED_TWO),
    ];

    for (label, target) in [
        (
            "batchRuntimeRender",
            CompileManyTarget::RuntimeRender {
                profile: batch_render_profile(false, true),
            },
        ),
        (
            "batchRuntimeRenderNoMap",
            CompileManyTarget::RuntimeRender {
                profile: batch_render_profile(false, false),
            },
        ),
        ("batchHostBacked", CompileManyTarget::HostBacked),
        (
            "batchServerProfile",
            CompileManyTarget::RuntimeRender {
                profile: batch_render_profile(true, true),
            },
        ),
    ] {
        let expected = VerterHost::new_standalone(HostConfig::default()).compile_many(
            inputs.clone(),
            CompileBatchOptions::default(),
            target,
        );
        let observed = record["cases"][label].as_array().unwrap_or_else(|| {
            panic!(
                "napi/{label}: the batch did not return an array: {}",
                record["cases"][label]
            )
        });

        assert_eq!(
            observed.len(),
            expected.len(),
            "napi/{label}: the transport returned {} entries for {} inputs",
            observed.len(),
            expected.len()
        );
        for (index, (entry, host_entry)) in observed.iter().zip(&expected).enumerate() {
            // ORDERING + attribution: entry N is the host's entry N, and both
            // name the caller's input N.
            assert_eq!(
                entry["canonicalId"].as_str(),
                Some(host_entry.canonical_id.as_str()),
                "napi/{label}[{index}]: the transport's entry order or attribution differs"
            );
            assert_eq!(
                entry["canonicalId"].as_str(),
                Some(inputs[index].canonical_id.as_str()),
                "napi/{label}[{index}]: the entry is not attributed to the caller's input"
            );
            assert_eq!(
                entry["code"].as_str(),
                Some(host_entry.code.as_ref()),
                "napi/{label}[{index}]: the transport's module bytes differ from the host's"
            );
            assert_eq!(
                entry["hasMap"],
                host_entry.source_map.is_some(),
                "napi/{label}[{index}]: map presence differs from the host's"
            );
            assert_eq!(
                entry["lang"].as_str(),
                host_entry.lang.as_deref(),
                "napi/{label}[{index}]: the reported module language differs from the host's"
            );
            let errors: Vec<&str> = entry["errors"]
                .as_array()
                .unwrap_or_else(|| panic!("napi/{label}[{index}]: `errors` is not an array"))
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(
                errors, host_entry.errors,
                "napi/{label}[{index}]: the transport's errors differ from the host's"
            );
        }

        // NON-CONTAMINATION, observed at the transport: the two distinct
        // supported inputs declare `count` and `total`, and neither entry
        // carries the other's declarations.
        let first = observed[0]["code"].as_str().unwrap_or_default();
        let third = observed[2]["code"].as_str().unwrap_or_default();
        assert!(
            first.contains("count") && !first.contains("total"),
            "napi/{label}: the first entry carries another input's declarations:\n{first}"
        );
        assert!(
            third.contains("total") && !third.contains("count"),
            "napi/{label}: the third entry carries another input's declarations:\n{third}"
        );
        assert_ne!(
            first, third,
            "napi/{label}: two distinct inputs produced identical bytes, so this batch cannot \
             detect a fanned-out result"
        );
    }

    // The optional-product axis at the transport's batch boundary.
    let with_map = record["cases"]["batchRuntimeRender"]
        .as_array()
        .expect("array");
    let without_map = record["cases"]["batchRuntimeRenderNoMap"]
        .as_array()
        .expect("array");
    for (index, (mapped, unmapped)) in with_map.iter().zip(without_map).enumerate() {
        assert_eq!(
            mapped["hasMap"], true,
            "napi/batch[{index}]: a requested map did not cross the transport boundary"
        );
        assert_eq!(
            unmapped["hasMap"], false,
            "napi/batch[{index}]: the transport published a map that was never requested"
        );
        assert_eq!(
            mapped["code"], unmapped["code"],
            "napi/batch[{index}]: the source-map axis changed the emitted module bytes"
        );
    }
}

/// Every spelling the BUILT NAPI artifact exports is either executed by the
/// probe or classified out of scope with a reason — enumerated from the
/// artifact's own surface, never from source.
#[test]
fn every_exported_napi_spelling_is_executed_or_classified_out_of_scope() {
    let record = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );
    let methods: Vec<String> = record["surface"]["classes"]["VerterHost"]
        .as_array()
        .expect("the built artifact exports a VerterHost class with methods")
        .iter()
        .map(|name| {
            name.as_str()
                .expect("a method name is a string")
                .to_string()
        })
        .collect();
    assert!(
        !methods.is_empty(),
        "the enumeration found no VerterHost methods, so it proves nothing"
    );

    assert_partition("napi", &methods, NAPI_EXECUTED, NAPI_OUT_OF_SCOPE, &[]);

    // The module-level exports are accounted for too.
    let module_exports: Vec<String> = record["surface"]["moduleExports"]
        .as_array()
        .expect("the built artifact has module exports")
        .iter()
        .map(|name| {
            name.as_str()
                .expect("an export name is a string")
                .to_string()
        })
        .collect();
    assert!(
        module_exports.contains(&"processStyle".to_string()),
        "the standalone CSS spelling is absent from the built artifact: {module_exports:?}"
    );
    assert!(
        module_exports.contains(&"VerterHost".to_string()),
        "the host class is absent from the built artifact: {module_exports:?}"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// WASM
// ══════════════════════════════════════════════════════════════════════════

const WASM_BUILD: &str = "CARGO_BUILD_JOBS=4 cargo build -p verter_wasm --target wasm32-unknown-unknown && wasm-bindgen --target web --out-dir packages/wasm/wasm --out-name verter_wasm target/wasm32-unknown-unknown/debug/verter_wasm.wasm";

/// Exported WASM spellings this probe does NOT execute, each with its reason.
///
/// Declared in its OWN right rather than derived from the NAPI class: the two
/// artifacts export different surfaces (the WASM one exports no `close`,
/// `configureProjects`, `evaluateTypes`, `getMetrics`, `resolveExports` or
/// `resolveImport`) and it EXECUTES `analyzeWithAudit`, which is out of scope on
/// NAPI. An explicit list is what lets the staleness half of the partition bite
/// on BOTH transports instead of being satisfied by construction.
const WASM_OUT_OF_SCOPE: &[(&str, &str)] = &[
    (
        "applyBlockOverrides",
        "block-content override input, not a product route",
    ),
    (
        "auditWorkspaceOp",
        "workspace audit op, not a framework product",
    ),
    (
        "collectResolvableModuleReferenceSpecifiers",
        "dependency-resolution input",
    ),
    (
        "computeCrossFileOptimizations",
        "cross-file optimizer, not a product route",
    ),
    ("evaluateTypeExpressionWithAudit", "type-resolution product"),
    ("getAnalysis", "analysis product"),
    ("getAuditRecords", "audit records"),
    ("getBundlerBatchSummary", "audit aggregation"),
    ("getCodeActions", "IDE feature"),
    ("getDocumentStructure", "analysis product"),
    ("getDocumentSymbols", "IDE feature"),
    ("getLastAuditRecord", "audit records"),
    ("getLintRuleMetadata", "lint metadata"),
    ("lint", "lint product"),
    ("listSymbols", "type-resolution product"),
    ("matchCssSelectors", "CSS analysis product"),
    ("remove", "lifecycle"),
    ("resolve", "id resolution"),
    (
        "resolveFrameworkSurfaceWithAudit",
        "framework-surface wire operation",
    ),
    (
        "resolveKnownModuleReferenceDependencies",
        "dependency resolution",
    ),
    ("resolveSymbolWithAudit", "type-resolution product"),
    ("resolveTypeWithAudit", "type-resolution product"),
    ("setImportDependencies", "dependency-resolution input"),
];

/// wasm-bindgen's own memory-management members on every generated class.
/// They are binding-runtime plumbing, not product spellings.
const WASM_BINDING_RUNTIME: &[(&str, &str)] = &[
    (
        "free",
        "wasm-bindgen memory management, not a product route",
    ),
    (
        "__destroy_into_raw",
        "wasm-bindgen memory management, not a product route",
    ),
];

/// The `VerterHost` methods the WASM probe actually executes.
const WASM_EXECUTED: &[&str] = &[
    "compileWithAudit",
    "getVirtualFile",
    "listVirtualFiles",
    "getPublicApi",
    "analyzeWithAudit",
    "ensureIdeCompiled",
    "getIde",
    "upsert",
];

#[test]
fn the_wasm_transport_matches_the_in_process_host_route() {
    let record = probe(
        "wasm",
        "packages/wasm/scripts/probe-transport-surface.mjs",
        WASM_BUILD,
    );
    assert_transport_matches_the_host_route("wasm", &record);

    // The transport's OWN typed refusal: audited analysis is not built for
    // wasm32, and the boundary says so rather than returning an empty success.
    let analyze = &record["cases"]["analyzeWithAudit"];
    assert_eq!(
        analyze["outcome"], "error",
        "wasm/analyzeWithAudit: the documented wasm32 refusal no longer fires: {analyze}"
    );
    assert!(
        analyze["message"]
            .as_str()
            .is_some_and(|message| message.contains("unavailable in WASM")),
        "wasm/analyzeWithAudit: the refusal message moved: {analyze}"
    );
}

/// Every spelling the BUILT WASM artifact exports is either executed by the
/// probe or classified out of scope with a reason.
#[test]
fn every_exported_wasm_spelling_is_executed_or_classified_out_of_scope() {
    let record = probe(
        "wasm",
        "packages/wasm/scripts/probe-transport-surface.mjs",
        WASM_BUILD,
    );
    let methods: Vec<String> = record["surface"]["classes"]["VerterHost"]
        .as_array()
        .expect("the built artifact exports a VerterHost class with methods")
        .iter()
        .map(|name| {
            name.as_str()
                .expect("a method name is a string")
                .to_string()
        })
        .collect();
    assert!(
        !methods.is_empty(),
        "the enumeration found no VerterHost methods, so it proves nothing"
    );

    // The WASM surface is the NAPI surface minus the native-only spellings, so
    // the same out-of-scope classification applies; anything the built artifact
    // exports beyond it is unaccounted for.
    assert_partition(
        "wasm",
        &methods,
        WASM_EXECUTED,
        WASM_OUT_OF_SCOPE,
        WASM_BINDING_RUNTIME,
    );
}

// ══════════════════════════════════════════════════════════════════════════
// A recorded cross-transport divergence
// ══════════════════════════════════════════════════════════════════════════

/// CHARACTERIZATION — the two transports serialize a MISSING virtual node
/// differently.
///
/// For the same typed request (a `style` node on a component whose runtime
/// surface was refused), the in-process host returns
/// `Err(HostError::MissingVirtualNode)`. NAPI converts that into a NULL
/// response; WASM converts it into a THROWN typed error. Both mean "no
/// product" and neither leaks one, but a consumer written against one
/// transport's shape does not port to the other unchanged.
///
/// Recorded, not corrected. It fails if either transport's shape moves.
/// The audited-compile spelling, driven on BOTH transports.
///
/// Two facts are pinned, and both were measured rather than assumed:
///
/// 1. The transport spelling returns the AUDIT RECORD, not the compiled
///    product. `crates/verter_napi/src/lib.rs:2525-2540` encodes `.audit()` and
///    drops the result; `crates/verter_wasm/src/lib.rs:874` does the same. With
///    audit disabled it projects to `null`
///    (`crates/verter_napi/src/audit.rs:60-65`).
/// 2. On an audit-ENABLED host the Vue carrier yields a stored record naming
///    the requested canonical, and the Svelte carrier yields `null` — i.e. the
///    audited compile of a Svelte component whose module serves normally
///    captures no stored record. Both transports agree, so this is the shared
///    host's behaviour, not a transport divergence.
///
/// This is a characterization: it fails in either direction if the audited
/// compile starts (or stops) capturing for either carrier.
#[test]
fn the_audited_compile_spelling_captures_for_vue_and_not_for_svelte_on_both_transports() {
    for (transport, script, build, vue_canonical) in [
        (
            "napi",
            "packages/native/scripts/probe-transport-surface.mjs",
            NAPI_BUILD,
            "/probe/Audited.vue",
        ),
        (
            "wasm",
            "packages/wasm/scripts/probe-transport-surface.mjs",
            WASM_BUILD,
            "/probe/AuditedW.vue",
        ),
    ] {
        let record = probe(transport, script, build);
        let cases = &record["cases"];

        let vue = &cases["auditedCompileVue"];
        assert_eq!(
            vue["outcome"].as_str(),
            Some("published"),
            "{transport}: the audited compile of a Vue carrier did not run: {vue}"
        );
        assert_eq!(
            vue["canonicalId"].as_str(),
            Some(vue_canonical),
            "{transport}: the audited record names a different canonical"
        );

        let svelte = &cases["auditedCompileSvelte"];
        assert_eq!(
            svelte["outcome"].as_str(),
            Some("published"),
            "{transport}: the audited compile of a Svelte carrier did not run: {svelte}"
        );
        assert_eq!(
            svelte["canonicalId"],
            Value::Null,
            "{transport}: the audited compile of a Svelte carrier now captures a stored record — \
             the recorded observation must be updated"
        );
    }

    // The disabled-audit projection is probed on NAPI only, where the host
    // config spelling exists.
    let napi = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );
    assert_eq!(
        napi["cases"]["auditedCompileDisabled"]["returned"],
        Value::Null,
        "napi: the audited compile returned a record on an audit-disabled host"
    );
}

#[test]
fn the_transports_serialize_a_missing_node_differently() {
    let napi = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );
    let wasm = probe(
        "wasm",
        "packages/wasm/scripts/probe-transport-surface.mjs",
        WASM_BUILD,
    );

    // The in-process host's own answer for this request is a missing node —
    // the fact both transports are converting.
    let server = host_with(
        "/probe/Server.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    assert_eq!(
        host_node(
            &server,
            "/probe/Server.svelte",
            VirtualNodeKind::Style { index: 0 },
            &probe_profile(true, true),
        ),
        HostOutcome::Missing,
        "the host route no longer reports a missing node here, so this comparison is stale"
    );

    assert_eq!(
        napi["cases"]["svelteServerStyle"]["outcome"], "missing",
        "the NAPI transport no longer returns a null response for a missing node: {}",
        napi["cases"]["svelteServerStyle"]
    );
    assert_eq!(
        wasm["cases"]["svelteServerStyle"]["outcome"], "error",
        "the WASM transport no longer throws for a missing node: {}",
        wasm["cases"]["svelteServerStyle"]
    );
    assert!(
        wasm["cases"]["svelteServerStyle"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("MissingVirtualNode")),
        "the WASM transport's missing-node error is no longer the typed one: {}",
        wasm["cases"]["svelteServerStyle"]
    );
    // Neither leaks a product, whichever shape it chose.
    for (transport, record) in [("napi", &napi), ("wasm", &wasm)] {
        assert_eq!(
            record["cases"]["svelteServerStyle"]["code"],
            Value::Null,
            "{transport}: a product crossed the boundary for a missing node"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════
// The bundler route
// ══════════════════════════════════════════════════════════════════════════

const BUNDLER_BUILD: &str =
    "pnpm --filter @verter/unplugin build (the plugin's fingerprinted dist/index.mjs)";

/// The BUNDLER route, executed: the shipped unplugin's public Vue- and
/// Svelte-pinned Vite and Rollup entries, loaded from the fingerprinted BUILT
/// entry and driven through `transform` and any virtual-script `load` it
/// publishes.
///
/// The plugin loads `@verter/native` internally, so this exercises the whole
/// bundler → transport → host chain rather than a citation of it. Its answers
/// are compared against the in-process host route for the same source.
#[test]
fn the_bundler_route_matches_the_in_process_host_route() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );

    assert_eq!(
        record["fresh"], true,
        "bundler: the ignored dist was not proven fresh against its production sources: {record}"
    );

    // SVELTE — `VerterSvelte.vite({})` publishes a wrapper, and the wrapper's
    // `?verter&type=script` load publishes the same module bytes as the host's
    // `Main` node. The wrapper itself is routing glue, not the mapped product.
    let svelte = &record["cases"]["sveltePublicEntry"];
    assert_eq!(
        svelte["outcome"], "transformed",
        "bundler/svelte: VerterSvelte.vite did not transform the source: {svelte}"
    );
    assert_eq!(
        svelte["publicFactory"], "VerterSvelte.vite",
        "bundler/svelte: the probe did not execute the public Svelte-pinned entry: {svelte}"
    );
    assert_eq!(
        svelte["loadedScriptOutcome"], "published",
        "bundler/svelte: the public wrapper's virtual-script load was missing: {svelte}"
    );
    let svelte_host = host_with(
        "/probe/Plug.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    let expected = host_node(
        &svelte_host,
        "/probe/Plug.svelte",
        VirtualNodeKind::Main,
        &CompileProfile {
            source_map: true,
            hmr_strategy: crate::types::HmrStrategy::None,
            ..CompileProfile::default()
        },
    );
    let HostOutcome::Published {
        code: host_code, ..
    } = &expected
    else {
        panic!("bundler/svelte: the host route no longer publishes this module: {expected:?}");
    };
    assert_eq!(
        svelte["loadedScriptCode"].as_str(),
        Some(host_code.as_str()),
        "bundler/svelte: the loaded virtual-script bytes differ from the host route's `Main`"
    );

    // VUE — execute the corresponding public Vue-pinned entry and its `?vue`
    // virtual-script load. Its rendered content is owned elsewhere.
    let vue = &record["cases"]["vuePublicEntry"];
    assert_eq!(
        vue["outcome"], "transformed",
        "bundler/vue: VerterVue.vite did not transform the source: {vue}"
    );
    assert_eq!(
        vue["publicFactory"], "VerterVue.vite",
        "bundler/vue: the probe did not execute the public Vue-pinned entry: {vue}"
    );
    assert_eq!(
        vue["loadedScriptOutcome"], "published",
        "bundler/vue: the public wrapper's virtual-script load was missing: {vue}"
    );
    assert!(
        vue["loadedScriptCode"]
            .as_str()
            .is_some_and(|code| !code.is_empty()),
        "bundler/vue: the public virtual-script load published empty code: {vue}"
    );
}

/// BND-1 was measured against the legacy/default Vue-pinned raw factory. The
/// documented public contract is two pinned entries: `VerterVue.vite({})`
/// accepts `.vue`, and `VerterSvelte.vite({})` accepts `.svelte`; each rejects
/// the other carrier extension.
#[test]
fn the_bundler_public_entries_apply_their_documented_include_contract() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let vue = &record["cases"]["vuePublicEntry"];
    let svelte = &record["cases"]["sveltePublicEntry"];

    assert_eq!(
        vue["transformInclude"], true,
        "VerterVue.vite rejected its documented `.vue` carrier: {vue}"
    );
    assert_eq!(
        vue["oppositeTransformInclude"], false,
        "VerterVue.vite accepted the Svelte-pinned entry's `.svelte` carrier: {vue}"
    );
    assert_eq!(
        svelte["transformInclude"], true,
        "VerterSvelte.vite rejected its documented `.svelte` carrier: {svelte}"
    );
    assert_eq!(
        svelte["oppositeTransformInclude"], false,
        "VerterSvelte.vite accepted the Vue-pinned entry's `.vue` carrier: {svelte}"
    );
}

/// BND-2 previously inspected the synthetic wrapper's `map: null`. Public Vite
/// consumers resolve and load the wrapper's script request; that virtual-script
/// product must carry the map requested from and published by the host.
#[test]
fn the_bundler_virtual_script_loads_publish_requested_source_maps() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let vue = &record["cases"]["vuePublicEntry"];
    let svelte = &record["cases"]["sveltePublicEntry"];

    for (factory, case) in [("VerterVue.vite", vue), ("VerterSvelte.vite", svelte)] {
        assert_eq!(
            case["wrapperHasMap"], false,
            "{factory}: the routing wrapper unexpectedly became the mapped product: {case}"
        );
        assert_eq!(
            case["loadedScriptOutcome"], "published",
            "{factory}: the public virtual-script load did not publish a product: {case}"
        );
        assert_eq!(
            case["loadedScriptHasMap"], true,
            "{factory}: the mapped virtual-script product dropped its requested map: {case}"
        );
    }

    let host = host_with(
        "/probe/Plug.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    let expected = host_node(
        &host,
        "/probe/Plug.svelte",
        VirtualNodeKind::Main,
        &CompileProfile {
            source_map: true,
            hmr_strategy: crate::types::HmrStrategy::None,
            ..CompileProfile::default()
        },
    );
    assert!(
        matches!(expected, HostOutcome::Published { has_map: true, .. }),
        "the host did not publish the requested Svelte map, so bundler parity cannot be tested: \
         {expected:?}"
    );
}

/// The public Rollup Vue entry consumes the host's `Main` product inline
/// because non-Vite consumers have no Vue script sub-request. Its transform
/// must retain the map that the requested profile makes the host publish.
///
/// The Svelte Rollup entry is executed too, but it is not the same product
/// shape: it publishes a `?verter&type=script` wrapper whose loaded script
/// already carries the host map, like the green Vite route above.
#[test]
#[ignore = "BF3-BND-2-SOURCEMAP-PARITY: Rollup inline transform currently drops the host map"]
fn the_bundler_rollup_inline_transform_preserves_requested_source_maps() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    assert_eq!(
        record["fresh"], true,
        "bundler: the built public Rollup entry was not proven fresh: {record}"
    );

    let vue = &record["cases"]["vueRollupEntry"];
    assert_eq!(
        vue["outcome"], "transformed",
        "VerterVue.rollup did not publish its inline transform: {vue}"
    );
    assert_eq!(
        vue["publicFactory"], "VerterVue.rollup",
        "the probe did not execute the public Vue Rollup entry: {vue}"
    );
    assert_eq!(
        vue["transformInclude"], true,
        "VerterVue.rollup rejected its documented `.vue` carrier: {vue}"
    );
    assert_eq!(
        vue["publicTransformIsInline"], true,
        "VerterVue.rollup no longer returned the non-Vite inline product: {vue}"
    );

    let svelte = &record["cases"]["svelteRollupEntry"];
    assert_eq!(
        svelte["outcome"], "transformed",
        "VerterSvelte.rollup did not transform its documented carrier: {svelte}"
    );
    assert_eq!(
        svelte["publicFactory"], "VerterSvelte.rollup",
        "the probe did not execute the public Svelte Rollup entry: {svelte}"
    );
    assert_eq!(
        svelte["publicTransformIsInline"], false,
        "VerterSvelte.rollup unexpectedly joined the inline product contract: {svelte}"
    );
    assert_eq!(
        svelte["loadedScriptOutcome"], "published",
        "VerterSvelte.rollup did not publish its virtual-script product: {svelte}"
    );
    assert_eq!(
        svelte["loadedScriptHasMap"], true,
        "VerterSvelte.rollup dropped the map from its virtual-script product: {svelte}"
    );

    let host = host_with(
        "/probe/Plug.vue",
        VUE_SFC,
        verter_language::FileLanguage::vue(),
    );
    let requested_profile = CompileProfile {
        filename: Some("/probe/Plug.vue".to_string()),
        source_map: true,
        force_js: true,
        hmr_strategy: crate::types::HmrStrategy::None,
        ..CompileProfile::default()
    };
    let host_product = host_node(
        &host,
        "/probe/Plug.vue",
        VirtualNodeKind::Main,
        &requested_profile,
    );
    let host_has_map = matches!(host_product, HostOutcome::Published { has_map: true, .. });
    assert!(
        host_has_map,
        "the matching source-map-requesting host profile did not publish a map, so Rollup parity \
         cannot be tested: {host_product:?}"
    );

    assert_eq!(
        vue["publicTransformHasMap"], true,
        "VerterVue.rollup dropped the requested map from the public non-Vite inline product: \
         hostHasMap={host_has_map}, publicTransformIsInline={}, publicTransformMap={}, \
         publicTransformHasMap={}; case={vue}",
        vue["publicTransformIsInline"], vue["publicTransformMap"], vue["publicTransformHasMap"]
    );
}
