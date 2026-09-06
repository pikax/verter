//! Public transports, built and driven.
//!
//! Each transport is probed through its typed compile-request route and
//! compared to the in-process host for the same typed request — the
//! binding decodes the request and executes `VerterHost::compile_request`,
//! so the comparison is against that entry, not against a per-node
//! profile read.
//!
//! Probes (`packages/native/scripts/probe-transport-surface.mjs`,
//! `packages/wasm/scripts/probe-transport-surface.mjs`) print JSON; this
//! module compares against the host's own answers, not transcribed
//! constants. Covers success, typed refusal + product absence,
//! optional-product, option conversion, serialization, and export
//! enumeration from the built artifact (never from source).
//!
//! Behind `transport-authoritative`. Missing artifacts FAIL (name the
//! build command) — never skip.
//!
//! THREE artifacts, not two. Each transport this module compares is a
//! separately built product, and a run missing any one of them fails a
//! whole lane with that lane's build command — so build all three before
//! reading any result as evidence about the route:
//!
//! ```text
//! CARGO_BUILD_JOBS=4 pnpm --filter @verter/native run build:debug
//! CARGO_BUILD_JOBS=4 cargo build -p verter_wasm --target wasm32-unknown-unknown \
//!   && wasm-bindgen --target web --out-dir packages/wasm/wasm --out-name verter_wasm \
//!      target/wasm32-unknown-unknown/debug/verter_wasm.wasm
//! pnpm --filter @verter/unplugin build
//! cargo test -p verter_session --lib --features transport-authoritative \
//!   transport_route_equivalence -- --test-threads=1
//! ```
//!
//! The bundler lane additionally pins the built plugin against a committed
//! source/dist fingerprint (`packages/unplugin/scripts/
//! probe-bundler-route.freshness.json`). Editing that package WITHOUT
//! regenerating the record turns every bundler test into a `Build it first`
//! failure that a correct build cannot clear, which reads as a missing
//! artifact rather than as the stale pin it is. Rebuild the plugin, then
//! rewrite the record from the observed hashes the probe reports.
//!
//! Without the feature this module is not compiled: a filter naming it
//! matches ZERO tests and `cargo test` still exits 0. Read the
//! `running N tests` line, never the exit code.
//!
//! CI executes the native and WASM comparison lanes (`Transport route
//! equivalence` in `.github/workflows/ci.yml`) against freshly built
//! `@verter/native` and wasm-bindgen web artifacts. That job runs an
//! EXACT allowlist of the nine native/WASM/census names — substring
//! skips are not used, so a later test cannot be silently dropped for
//! its name's tokens — and fails if the `running N tests` line is
//! missing, below the allowlist floor (9), or does not name those two
//! comparisons. The bundler lane is not on the list: it also needs a
//! matching `@verter/unplugin` freshness pin, a different surface. Clippy
//! with `--features transport-authoritative` in `Rust Build
//! Configurations` stays the cheap signature-drift check.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{
    CompileProfile, HostConfig, HostError, PublicApiMode, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
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

/// One probe run's ceiling. A transport probe loads a large native module or
/// instantiates a wasm binary, so a few seconds is normal and a hang is not.
const PROBE_TIMEOUT: Duration = Duration::from_secs(300);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Same bytes the probe scripts use, so in-process comparison asks the
/// identical question.
const SUPPORTED_SVELTE: &str = "<script>\n  let count = $state(0);\n</script>\n\n<div class=\"root\">{count}</div>\n\n<style>\n  .root { color: red; }\n</style>\n";
const VUE_SFC: &str = "<script setup>\nconst props = defineProps({ label: { type: String, required: true } });\n</script>\n\n<template>\n  <button>{{ label }}</button>\n</template>\n";
/// Carrier ids this suite asks the bundler about. Assert against these,
/// never an id read back from the probe record (a retargeted map plus a
/// relabelled `id` can satisfy both).
const PROBE_VUE_ID: &str = "/probe/Plug.vue";
const PROBE_SVELTE_ID: &str = "/probe/Plug.svelte";

/// A second supported Svelte component, distinct from [`SUPPORTED_SVELTE`].
const SUPPORTED_TWO: &str =
    "<script>\n  let total = $state(7);\n</script>\n\n<span class=\"total\">{total}</span>\n";
/// Client-backend refusal: instance-script prop WRITE (official uses
/// the prop setter; this backend does not emit it). A prop READ is
/// supported, so a read-only component is not a refusal witness.
const ADVANCED_RUNE_REFUSAL: &str = "<script>\n  let { count = 0 } = $props();\n  function inc() { count += 1; }\n</script>\n\n<button onclick={inc}>{count}</button>\n";

fn batch_input(canonical: &str, source: &str) -> crate::host_compile::CompileBatchInput {
    crate::host_compile::CompileBatchInput {
        canonical_id: canonical.to_string(),
        source: Arc::from(source),
        requested_mode: None,
        component_id: None,
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
        // Pin NODE_ENV: non-Vite lanes use it for production codegen and
        // the scoped component id. An ambient value would fail for the
        // wrong reason.
        .env("NODE_ENV", "development")
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

/// Drive one transport probe. Missing artifact fails with the build
/// command — never skip. `erroredCases` is a failed run, not a datum
/// (typed-refusal rows do not publish the field).
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
    if let Some(errored) = record["erroredCases"].as_array() {
        assert!(
            errored.is_empty(),
            "{transport}: the probe drove {} required lane(s) into an error, so this record is \
             not an observation of them: {}\nstderr:\n{stderr}",
            errored.len(),
            record["cases"]
        );
    }
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

#[derive(Debug, PartialEq, Eq)]
enum HostOutcome {
    Published {
        code: String,
        /// The host's published map, VERBATIM (a JSON string), not a presence
        /// flag: a route that claims to preserve the host's map can only be
        /// tested against the artifact the host actually published.
        source_map: Option<String>,
        lang: Option<String>,
    },
    Refused {
        diagnostic_code: String,
    },
    Missing,
}

impl HostOutcome {
    /// The published map, or a panic naming what the host did instead.
    #[track_caller]
    fn published_map(&self, label: &str) -> String {
        match self {
            HostOutcome::Published {
                source_map: Some(map),
                ..
            } => map.clone(),
            other => panic!(
                "{label}: the host published no source map for the requested profile, so there is \
                 nothing to compare a bundler product against: {other:?}"
            ),
        }
    }
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
            source_map: response.source_map.as_ref().map(|map| map.to_string()),
            lang: response.lang.clone(),
        },
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => HostOutcome::Refused { diagnostic_code },
        Err(HostError::MissingVirtualNode { .. }) => HostOutcome::Missing,
        Err(other) => panic!("{canonical}: unmodelled host outcome {other:?}"),
    }
}

/// Which framework arm of the WASM probe's typed request builders a
/// mirrored in-process request states.
#[derive(Clone, Copy)]
enum ProbeFramework {
    Svelte,
    Vue,
}

/// The framework arm mirroring the probe's `runtimeRequest(fileKind, …)` /
/// `ideRequest(fileKind, …)` option objects field for field: Svelte `{}`
/// (all-default options), Vue `{ backend: "inferred", ssr, isCustomElement:
/// [], babelParserPlugins: [] }`.
fn probe_framework_request(
    framework: ProbeFramework,
    ssr: bool,
) -> verter_compiler::compile_request::FrameworkCompileRequest {
    use verter_compiler::compile_request::{
        FrameworkCompileRequest, SvelteCompileRequest, VueBackendRequest, VueCompileRequest,
    };
    match framework {
        ProbeFramework::Svelte => FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        ProbeFramework::Vue => FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            ssr,
            is_custom_element: Vec::new(),
            babel_parser_plugins: Vec::new(),
            ..VueCompileRequest::default()
        }),
    }
}

/// The response's single product row, mirroring the probe's `soleProductRow`
/// strictness: a one-product demand answered by anything else is a wire
/// break on the comparison side too, not a datum.
#[track_caller]
fn sole_typed_row<'a>(
    response: &'a crate::CompileRequestResponse,
    canonical: &str,
) -> &'a crate::CompiledProduct {
    let [row] = response.products.as_slice() else {
        panic!(
            "{canonical}: expected exactly one typed product row, got {}: {:?}",
            response.products.len(),
            response
                .products
                .iter()
                .map(|product| match product {
                    crate::CompiledProduct::Runtime { kind, .. } => kind.wire_tag(),
                    crate::CompiledProduct::Ide(_) => "ideCompanion",
                    crate::CompiledProduct::Analysis(_) => "analysis",
                })
                .collect::<Vec<_>>()
        )
    };
    row
}

/// The probes' `runtimeRequest("svelte", { sourceMap, ssr })` mirrored
/// field for field: the stated identity (`isProduction: true`, `forceJs:
/// false`, no filename, no component id), the one runtime product at the
/// stated map axis, default Svelte options. The Svelte arm carries no
/// second bit that could disagree with the product tag, so `ssr` selects
/// the product kind exactly as the probe's builder does.
fn probe_svelte_runtime_request(
    ssr: bool,
    source_map: bool,
) -> verter_compiler::compile_request::CompileRequest {
    use verter_compiler::compile_request::{CompileProduct, CompileRequest, RuntimeProductRequest};

    let runtime = RuntimeProductRequest {
        runtime_source_map: source_map,
        ..RuntimeProductRequest::default()
    };
    let product = if ssr {
        CompileProduct::RuntimeServer(runtime)
    } else {
        CompileProduct::RuntimeClient(runtime)
    };
    CompileRequest::new(
        vec![product],
        verter_compiler::compile_request::FrameworkCompileRequest::Svelte(
            verter_compiler::compile_request::SvelteCompileRequest::default(),
        ),
        None,
        None,
        None,
        true,
        false,
    )
    .unwrap_or_else(|error| panic!("the probe's own request shape no longer constructs: {error:?}"))
}

/// The in-process host's answer to the SAME typed request the probes issue
/// for one node of a carrier's runtime surface — `runtimeRequest(fileKind,
/// { sourceMap, ssr })` mirrored field for field: the stated identity
/// (`isProduction: true`, `forceJs: false`, no filename, no component id),
/// the one mapped runtime product, default style ownership, and the
/// framework's own default options.
///
/// The binding this probe drives decodes its JS request into the canonical
/// `CompileRequest` and calls `VerterHost::compile_request` (WASM:
/// `crates/verter_wasm/src/lib.rs`; NAPI:
/// `crates/verter_napi/src/host_compile_request.rs`), so the comparison the
/// typed route deserves is against this entry — never against a per-node
/// profile read,
/// which answers a different question through a second option authority: a
/// typed axis the profile happens not to model for this SFC would pass
/// unnoticed.
#[track_caller]
fn host_typed_runtime_node(
    host: &VerterHost,
    canonical: &str,
    framework: ProbeFramework,
    ssr: bool,
    source_map: bool,
    node: VirtualNodeKind,
) -> HostOutcome {
    use verter_compiler::compile_request::{CompileProduct, CompileRequest, RuntimeProductRequest};

    let runtime = RuntimeProductRequest {
        runtime_source_map: source_map,
        ..RuntimeProductRequest::default()
    };
    let product = if ssr {
        CompileProduct::RuntimeServer(runtime)
    } else {
        CompileProduct::RuntimeClient(runtime)
    };
    let request = CompileRequest::new(
        vec![product],
        probe_framework_request(framework, ssr),
        None,
        None,
        None,
        true,
        false,
    )
    .unwrap_or_else(|error| {
        panic!("{canonical}: the probe's own request shape no longer constructs: {error:?}")
    });

    match host.compile_request(canonical, request) {
        Ok(response) => {
            let crate::CompiledProduct::Runtime { nodes, .. } =
                sole_typed_row(&response, canonical)
            else {
                panic!("{canonical}: the completed typed runtime request carried no runtime row");
            };
            match nodes.iter().find(|candidate| candidate.node == node) {
                Some(row) => HostOutcome::Published {
                    code: row.code.to_string(),
                    source_map: row.source_map.as_ref().map(|map| map.to_string()),
                    lang: row.lang.clone(),
                },
                // A completed compile whose product carries no such node is
                // structural absence — the same class the cached read's
                // `MissingVirtualNode` answers.
                None => HostOutcome::Missing,
            }
        }
        Err(crate::CompileRequestFailure::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => HostOutcome::Refused { diagnostic_code },
        Err(other) => panic!("{canonical}: unmodelled typed request outcome {other:?}"),
    }
}

/// The in-process host's IDE projection for the SAME typed request the WASM
/// probe issues — its `ideRequest("svelte", sourceMap)` mirrored field for
/// field: `isProduction: false`, `forceJs: false`, default Svelte options,
/// and one `ideCompanion` product with every axis stated (`wantSourceMap`,
/// and `false` for `embedAmbientTypes`, `conditionalRootNarrowing`,
/// `strictSlots`, and the carrier-bridge-substituted `ideChunkBoundaries`).
#[track_caller]
fn host_typed_ide(host: &VerterHost, canonical: &str, source_map: bool) -> crate::IdeResponse {
    use verter_compiler::compile_request::{CompileProduct, CompileRequest, IdeProductRequest};

    let request = CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: source_map,
            embed_ambient_types: false,
            conditional_root_narrowing: false,
            strict_slots: false,
            types_module_name: None,
            ide_chunk_boundaries: false,
            ..IdeProductRequest::default()
        })],
        probe_framework_request(ProbeFramework::Svelte, false),
        None,
        None,
        None,
        false,
        false,
    )
    .unwrap_or_else(|error| {
        panic!("{canonical}: the probe's own request shape no longer constructs: {error:?}")
    });

    match host.compile_request(canonical, request) {
        Ok(response) => {
            let crate::CompiledProduct::Ide(ide) = sole_typed_row(&response, canonical) else {
                panic!("{canonical}: the completed typed IDE request carried no ideCompanion row");
            };
            ide.clone()
        }
        Err(other) => panic!("{canonical}: the typed IDE request refused: {other:?}"),
    }
}

/// Assert one probe case equals the in-process host's answer for the same typed
/// request.
#[track_caller]
fn assert_case_matches_host(transport: &str, label: &str, case: &Value, expected: &HostOutcome) {
    match expected {
        HostOutcome::Published {
            code,
            source_map,
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
                case["hasMap"],
                source_map.is_some(),
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
            // Missing node: absent response, never a throw. Pinned by
            // `the_transports_report_a_missing_node_the_same_way`.
            assert_eq!(
                case["outcome"], "missing",
                "{transport}/{label}: the host reported a missing node but the transport \
                 returned {case}"
            );
            // No product crossed the boundary.
            assert_eq!(
                case["code"],
                Value::Null,
                "{transport}/{label}: the transport published code for a missing node"
            );
        }
    }
}

/// Every case the probes execute, compared against the in-process host.
///
/// Both transports demand every compiled product through the same typed
/// compile request — one `compileRequest` call per demand — so every case
/// is compared against the host's answer to that IDENTICAL typed request,
/// executed through `VerterHost::compile_request`. Comparing the typed arm
/// against a per-node profile read would route the expected value through
/// a second option authority: a typed axis the profile does not model for
/// this SFC would pass unnoticed.
fn assert_transport_matches_the_host_route(transport: &str, record: &Value) {
    // Success + optional-product axis (supported Svelte).
    let svelte = host_with(
        "/probe/Ok.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    let expected_svelte_node = |source_map: bool, kind: VirtualNodeKind| {
        host_typed_runtime_node(
            &svelte,
            "/probe/Ok.svelte",
            ProbeFramework::Svelte,
            false,
            source_map,
            kind,
        )
    };
    assert_case_matches_host(
        transport,
        "svelteMainWithMap",
        &record["cases"]["svelteMainWithMap"],
        &expected_svelte_node(true, VirtualNodeKind::Main),
    );
    assert_case_matches_host(
        transport,
        "svelteMainWithoutMap",
        &record["cases"]["svelteMainWithoutMap"],
        &expected_svelte_node(false, VirtualNodeKind::Main),
    );
    assert_case_matches_host(
        transport,
        "svelteStyleWithMap",
        &record["cases"]["svelteStyleWithMap"],
        &expected_svelte_node(true, VirtualNodeKind::Style { index: 0 }),
    );
    // Optional-product axis both ways; no module-byte change.
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

    // Node list serializes as typed kind/index.
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

    // Refusal + artifact absence.
    let server = host_with(
        "/probe/Server.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    // The demand is the typed server-runtime compile request, executed
    // in-process: the route refuses the whole transaction, so comparing a
    // per-node read instead would compare two different questions and would
    // not notice the typed route carrying a different code or a different
    // failure arm.
    let refusal = host_typed_runtime_node(
        &server,
        "/probe/Server.svelte",
        ProbeFramework::Svelte,
        true,
        true,
        VirtualNodeKind::Main,
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
    // No CSS survives the refusal, across the boundary. What "no CSS" looks
    // like on a complete-only wire: with nothing assembled, there is no
    // CSS-specific artifact to interrogate, so this case's record is
    // byte-identical to `svelteServerRefusal`'s and its residual value is
    // the no-bytes check below. That is the honest limit of a
    // complete-only wire, not a gap to paper over; per-product refusal
    // granularity returns here only if the wire ever gains partial-product
    // responses, and is the owner of that wire change's job.
    let expected_style = host_typed_runtime_node(
        &server,
        "/probe/Server.svelte",
        ProbeFramework::Svelte,
        true,
        true,
        VirtualNodeKind::Style { index: 0 },
    );
    assert_case_matches_host(
        transport,
        "svelteServerStyle",
        &record["cases"]["svelteServerStyle"],
        &expected_style,
    );
    assert!(
        record["cases"]["svelteServerStyle"]["code"] == Value::Null,
        "{transport}: a CSS product survived the runtime refusal across the transport: {}",
        record["cases"]["svelteServerStyle"]
    );

    // IDE/TSX vs host. The BYTES, the map presence and the JSX flag are
    // compared against the host's answer to the same typed IDE request.
    // The producibility case reports `outcome: "ok"`: no `value` pin exists
    // on this route and none is invented — a completed response cannot
    // lack the `ideCompanion` row (the probe's strict row reader aborts the
    // record if it did), so `outcome === "ok"` plus the published mapped
    // product below ARE the producibility proof.
    let ide_host = host_with(
        "/probe/Ide.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    assert_eq!(
        record["cases"]["ensureIdeCompiled"]["outcome"], "ok",
        "{transport}/ensureIdeCompiled: the transport errored: {}",
        record["cases"]["ensureIdeCompiled"]
    );
    let host_ide = host_typed_ide(&ide_host, "/probe/Ide.svelte", true);
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
    // The map-less IDE demand: a complete compile always publishes the
    // projection, so this is the IDE surface's optional-product axis — the
    // map is withheld, and withholding it does not change the generated
    // bytes.
    let unmapped = &record["cases"]["getIdeWithoutMap"];
    {
        let host_unmapped = host_typed_ide(&ide_host, "/probe/Ide.svelte", false);
        assert_eq!(
            unmapped["outcome"], "published",
            "{transport}/getIde: a complete IDE compile published nothing: {unmapped}"
        );
        assert_eq!(
            unmapped["code"].as_str(),
            Some(host_unmapped.code.as_ref()),
            "{transport}/getIde: the map-less IDE bytes differ from the host's"
        );
        assert_eq!(
            host_unmapped.source_map, None,
            "{transport}: the host now emits a map for a map-less IDE demand, so this axis \
             decides nothing"
        );
        assert_eq!(
            unmapped["hasMap"], false,
            "{transport}/getIde: the transport published an IDE map that was never requested: \
             {unmapped}"
        );
        assert_eq!(
            transported["code"], unmapped["code"],
            "{transport}/getIde: the source-map axis changed the generated IDE bytes"
        );
    }

    // Option conversion: public-API `mode`.
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
    // Modes produce different surfaces — dropping `mode` would be caught.
    assert_ne!(
        record["cases"]["vuePublicApiDefault"]["code"],
        record["cases"]["vuePublicApiDeclaration"]["code"],
        "{transport}: both public-API modes produced identical bytes, so this probe cannot \
         detect a dropped `mode` argument"
    );
}

/// Every exported spelling belongs to exactly one class. A union of two
/// lists is not completeness: a dropped name stays green if it sits in
/// both. In two classes, or in none, is a failure; stale rows cannot
/// name a spelling the artifact does not export.
#[track_caller]
fn assert_partition(
    transport: &str,
    exported: &[String],
    executed: &[&str],
    out_of_scope: &[(&str, &str)],
    extra_classified: &[&[(&str, &str)]],
) {
    use std::collections::{BTreeMap, BTreeSet};

    let exported_set: BTreeSet<&str> = exported.iter().map(String::as_str).collect();
    let mut executed_set: BTreeSet<&str> = BTreeSet::new();
    let mut duplicate_executed: Vec<&str> = Vec::new();
    for name in executed {
        if !executed_set.insert(*name) {
            duplicate_executed.push(*name);
        }
    }
    assert!(
        duplicate_executed.is_empty(),
        "{transport}: {} spelling(s) are listed executed more than once, so the classification \
         cannot prove uniqueness: {duplicate_executed:?}",
        duplicate_executed.len()
    );

    let mut classified: BTreeMap<&str, &str> = BTreeMap::new();
    let mut duplicate_classified: Vec<(&str, &str, &str)> = Vec::new();
    for (name, reason) in out_of_scope
        .iter()
        .chain(extra_classified.iter().copied().flatten())
    {
        if let Some(prior) = classified.insert(*name, *reason) {
            duplicate_classified.push((*name, prior, *reason));
        }
    }
    assert!(
        duplicate_classified.is_empty(),
        "{transport}: {} spelling(s) appear in two classified lists, so the classification \
         cannot prove uniqueness: {duplicate_classified:?}",
        duplicate_classified.len()
    );
    let classified_set: BTreeSet<&str> = classified.keys().copied().collect();

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

// NAPI

const NAPI_BUILD: &str = "CARGO_BUILD_JOBS=4 pnpm --filter @verter/native run build:debug";

/// NAPI spellings this probe does not execute, with reason. One side of
/// [`assert_partition`], not a fallback.
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
    "compileMany",
    "compileRequest",
    "compileRequests",
    "compileWithAudit",
    "listVirtualFiles",
    "getPublicApi",
    "upsert",
];

/// Legacy cached-route spellings this probe does not drive: it demands
/// every product through the typed `compileRequest`/`compileRequests`, and
/// these exports are the binding-local cached route that route supersedes.
///
/// A separate class from [`NAPI_OUT_OF_SCOPE`] because the reason is this
/// probe's route choice, not the method's product — each name IS a live
/// product route a per-keystroke consumer is steered toward. The rows make
/// NO coverage claim over them: this probe neither executes them nor
/// verifies that any other suite does. Their own behavioural coverage lives
/// in the binding suite `packages/native/index.spec.ts`.
const NAPI_CACHED_ROUTES_NOT_DRIVEN: &[(&str, &str)] = &[
    (
        "ensureIdeCompiled",
        "cached IDE-compile route; this probe demands IDE products through the typed \
         `compileRequest` instead",
    ),
    (
        "getIde",
        "cached IDE read; this probe demands IDE products through the typed \
         `compileRequest` instead",
    ),
    (
        "getVirtualFile",
        "cached per-node read; this probe demands runtime products through the typed \
         `compileRequest` instead",
    ),
];

#[test]
fn the_napi_transport_matches_the_in_process_host_route() {
    let record = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );
    assert_transport_matches_the_host_route("napi", &record);

    let transformed = &record["cases"]["transformVueStyle"];
    let parsed = verter_compiler::style_planner::parse_plain_css_for_verification(
        ".x{color:red}",
        verter_compiler::style_planner::StyleRewriteStage::AuthoredVBind,
    )
    .expect("the in-process CSS verification parse accepts the probe block");
    let verified =
        verter_compiler::style_planner::VerifiedPlainCss::from_parsed_native_css(&parsed)
            .expect("the verification parser produced native CSS syntax IR");
    let expected_transform = verter_compiler::style_planner::transform_vue_style(
        verified,
        // The probe hands over its own authored bytes with no external tool in
        // front of them, so reported spans address exactly those bytes.
        verter_compiler::style_planner::CascadeInput::Authored,
        "style.css",
        "style.css",
        "style.css",
        "probe1234",
        false,
        true,
        true,
    );
    assert!(expected_transform.stage_failures.is_empty());
    assert_eq!(
        transformed["code"].as_str(),
        Some(expected_transform.code()),
        "napi/transformVueStyle: the transported CSS differs from the in-process route's"
    );
    assert_eq!(
        transformed["hasMap"], true,
        "napi/transformVueStyle: the `sourcemap` axis is a live product — the transport \
         must publish a map when one is requested"
    );
}

/// NAPI batch vs in-process host, item-for-item. Refusal in the middle so
/// a shift is visible both ways. Host answers, never transcribed constants.
///
/// The render lanes demand their runtime surface through the typed plural
/// route (`compileRequests`), so they are compared item-for-item against
/// the in-process typed batch (`VerterHost::compile_request_many`) for the
/// same per-input requests. The profile-less `host-backed` lane owns no
/// profile-bearing case, stays on the legacy batch spelling, and is
/// compared against `compile_many`.
#[test]
fn the_napi_batch_route_matches_the_in_process_batch_route_item_for_item() {
    use crate::host_compile::{
        CompileBatchOptions, CompileManyTarget, CompileRequestBatchInput,
        CompileRequestBatchOptions,
    };

    let record = probe(
        "napi",
        "packages/native/scripts/probe-transport-surface.mjs",
        NAPI_BUILD,
    );

    // The exact carriers the probe sends, in the same order.
    let carriers = [
        ("/napiBatch/One.svelte", SUPPORTED_SVELTE),
        ("/napiBatch/Refused.svelte", ADVANCED_RUNE_REFUSAL),
        ("/napiBatch/Two.svelte", SUPPORTED_TWO),
    ];

    // The profile-less host-backed lane: the legacy batch spelling, with
    // the legacy per-entry envelope (code/map/lang/errors).
    {
        let expected = VerterHost::new_standalone(HostConfig::default()).compile_many(
            carriers
                .iter()
                .map(|(canonical, source)| batch_input(canonical, source))
                .collect(),
            CompileBatchOptions::default(),
            CompileManyTarget::HostBacked,
        );
        let observed = record["cases"]["batchHostBacked"]
            .as_array()
            .unwrap_or_else(|| {
                panic!(
                    "napi/batchHostBacked: the batch did not return an array: {}",
                    record["cases"]["batchHostBacked"]
                )
            });
        assert_eq!(
            observed.len(),
            expected.len(),
            "napi/batchHostBacked: the transport returned {} entries for {} inputs",
            observed.len(),
            expected.len()
        );
        for (index, (entry, host_entry)) in observed.iter().zip(&expected).enumerate() {
            // Entry N is the host's entry N, attributed to input N.
            assert_eq!(
                entry["canonicalId"].as_str(),
                Some(host_entry.canonical_id.as_str()),
                "napi/batchHostBacked[{index}]: the transport's entry order or attribution differs"
            );
            assert_eq!(
                entry["canonicalId"].as_str(),
                Some(carriers[index].0),
                "napi/batchHostBacked[{index}]: the entry is not attributed to the caller's input"
            );
            assert_eq!(
                entry["code"].as_str(),
                Some(host_entry.code()),
                "napi/batchHostBacked[{index}]: the transport's module bytes differ from the \
                 host's"
            );
            assert_eq!(
                entry["hasMap"],
                host_entry.source_map().is_some(),
                "napi/batchHostBacked[{index}]: map presence differs from the host's"
            );
            assert_eq!(
                entry["lang"].as_str(),
                host_entry.lang(),
                "napi/batchHostBacked[{index}]: the reported module language differs"
            );
            let errors: Vec<&str> = entry["errors"]
                .as_array()
                .unwrap_or_else(|| {
                    panic!("napi/batchHostBacked[{index}]: `errors` is not an array")
                })
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(
                errors,
                host_entry.errors(),
                "napi/batchHostBacked[{index}]: the transport's errors differ from the host's"
            );
        }
    }

    // The render lanes: the typed plural route. Each input states its own
    // runtime demand; a per-input refusal isolates to that entry's failure
    // arm while every sibling still compiles.
    for (label, ssr, source_map) in [
        ("batchRuntimeRender", false, true),
        ("batchRuntimeRenderNoMap", false, false),
        ("batchServerProfile", true, true),
    ] {
        let typed_inputs: Vec<CompileRequestBatchInput> = carriers
            .iter()
            .map(|(canonical, source)| CompileRequestBatchInput {
                canonical_id: canonical.to_string(),
                source: Arc::from(*source),
                request: probe_svelte_runtime_request(ssr, source_map),
            })
            .collect();
        let expected = VerterHost::new_standalone(HostConfig::default())
            .compile_request_many(typed_inputs, CompileRequestBatchOptions::default());
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
            // Entry N is the host's entry N, attributed to input N.
            assert_eq!(
                entry["canonicalId"].as_str(),
                Some(host_entry.canonical_id.as_str()),
                "napi/{label}[{index}]: the transport's entry order or attribution differs"
            );
            assert_eq!(
                entry["canonicalId"].as_str(),
                Some(carriers[index].0),
                "napi/{label}[{index}]: the entry is not attributed to the caller's input"
            );
            match &host_entry.outcome {
                Ok(response) => {
                    // The entry completed: its demanded runtime product's
                    // main node is the batch envelope this lane compares.
                    let crate::CompiledProduct::Runtime { nodes, .. } =
                        sole_typed_row(response, host_entry.canonical_id.as_str())
                    else {
                        panic!(
                            "napi/{label}[{index}]: the completed typed batch entry carried no \
                             runtime row"
                        );
                    };
                    let node = nodes
                        .iter()
                        .find(|candidate| candidate.node == VirtualNodeKind::Main)
                        .unwrap_or_else(|| {
                            panic!(
                                "napi/{label}[{index}]: the completed runtime product carried no \
                                 main node"
                            )
                        });
                    assert_eq!(
                        entry["outcome"], "published",
                        "napi/{label}[{index}]: the host published but the transport returned \
                         {entry}"
                    );
                    assert_eq!(
                        entry["code"].as_str(),
                        Some(node.code.as_ref()),
                        "napi/{label}[{index}]: the transport's module bytes differ from the host's"
                    );
                    assert_eq!(
                        entry["hasMap"],
                        node.source_map.is_some(),
                        "napi/{label}[{index}]: map presence differs from the host's"
                    );
                    assert_eq!(
                        entry["lang"].as_str(),
                        node.lang.as_deref(),
                        "napi/{label}[{index}]: the reported module language differs"
                    );
                }
                Err(crate::CompileRequestFailure::RuntimeSurfaceRefused {
                    diagnostic_code,
                    ..
                }) => {
                    // The whole entry refused: no product may travel beside
                    // the refusal, and the record carries the host's typed
                    // diagnostic code.
                    assert_eq!(
                        entry["outcome"], "error",
                        "napi/{label}[{index}]: the host refused but the transport returned \
                         {entry}"
                    );
                    let message = entry["message"].as_str().unwrap_or_default();
                    assert!(
                        message.contains(diagnostic_code.as_str()),
                        "napi/{label}[{index}]: the transport's error does not carry the host's \
                         typed refusal code ({diagnostic_code}): {message}"
                    );
                    assert_eq!(
                        entry["code"],
                        Value::Null,
                        "napi/{label}[{index}]: a product travelled beside the refusal"
                    );
                    assert_eq!(
                        entry["hasMap"], false,
                        "napi/{label}[{index}]: a source map travelled beside the refusal"
                    );
                }
                Err(other) => panic!(
                    "napi/{label}[{index}]: unmodelled typed batch outcome for {}: {other:?}",
                    host_entry.canonical_id
                ),
            }
        }

        if ssr {
            // Server lane: every entry is a typed refusal; no product.
            for (index, entry) in observed.iter().enumerate() {
                let message = entry["message"].as_str().unwrap_or_default();
                assert!(
                    message.contains("svelte-runtime-unsupported-server-generate"),
                    "napi/{label}[{index}]: the server lane did not surface the typed Svelte \
                     server refusal: {message}"
                );
                assert_eq!(
                    entry["code"],
                    Value::Null,
                    "napi/{label}[{index}]: a product travelled beside the refusal"
                );
                assert_eq!(
                    entry["hasMap"], false,
                    "napi/{label}[{index}]: a source map travelled beside the refusal"
                );
            }
            continue;
        }

        // Distinct inputs: `count` vs `total`; neither carries the other.
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

    // The optional-product axis at the transport's typed batch boundary.
    let with_map = record["cases"]["batchRuntimeRender"]
        .as_array()
        .expect("array");
    let without_map = record["cases"]["batchRuntimeRenderNoMap"]
        .as_array()
        .expect("array");
    let mut published_entries = 0;
    for (index, (mapped, unmapped)) in with_map.iter().zip(without_map).enumerate() {
        // Refused entries publish nothing; the axis must not decide refusal.
        let refused = mapped["outcome"] == "error";
        if refused {
            assert_eq!(
                unmapped["outcome"], "error",
                "napi/batch[{index}]: the source-map axis decided whether this entry was refused"
            );
            for (variant, entry) in [("with map", mapped), ("without map", unmapped)] {
                assert_eq!(
                    entry["hasMap"], false,
                    "napi/batch[{index}] ({variant}): a map travelled beside a refusal"
                );
                assert_eq!(
                    entry["code"],
                    Value::Null,
                    "napi/batch[{index}] ({variant}): a product travelled beside a refusal"
                );
            }
            continue;
        }
        published_entries += 1;
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
    assert!(
        published_entries > 0,
        "napi/batch: every entry was refused, so the source-map axis was never exercised"
    );
}

/// Every NAPI export is executed or classified out of scope, from the
/// built artifact — never from source.
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

    assert_partition(
        "napi",
        &methods,
        NAPI_EXECUTED,
        NAPI_OUT_OF_SCOPE,
        &[NAPI_CACHED_ROUTES_NOT_DRIVEN],
    );

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
        !module_exports.contains(&"processStyle".to_string()),
        "the retired processStyle spelling is still on the built artifact: {module_exports:?}"
    );
    for spelling in [
        "analyzeStyle",
        "prepareStyleForPreprocessor",
        "transformVueStyle",
    ] {
        assert!(
            module_exports.contains(&spelling.to_string()),
            "the standalone CSS spelling `{spelling}` is absent from the built artifact: \
             {module_exports:?}"
        );
    }
    assert!(
        module_exports.contains(&"VerterHost".to_string()),
        "the host class is absent from the built artifact: {module_exports:?}"
    );
}

// WASM

const WASM_BUILD: &str = "CARGO_BUILD_JOBS=4 cargo build -p verter_wasm --target wasm32-unknown-unknown && wasm-bindgen --target web --out-dir packages/wasm/wasm --out-name verter_wasm target/wasm32-unknown-unknown/debug/verter_wasm.wasm";

/// WASM spellings this probe does not execute. Own list, not derived from
/// NAPI: WASM exports a different surface and executes `analyzeWithAudit`.
const WASM_OUT_OF_SCOPE: &[(&str, &str)] = &[
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
    "compileRequest",
    "compileWithAudit",
    "listVirtualFiles",
    "getPublicApi",
    "analyzeWithAudit",
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

    // wasm32 has no audited analysis; typed refusal, not empty success.
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

    // Anything the built artifact exports beyond these lists is unaccounted.
    assert_partition(
        "wasm",
        &methods,
        WASM_EXECUTED,
        WASM_OUT_OF_SCOPE,
        &[WASM_BINDING_RUNTIME],
    );
}

/// Audited compile on both transports: returns the audit record, not the
/// product (audit disabled → `null`). Vue stores a record; Svelte does not.
/// Characterization: fails if capture starts or stops for either carrier.
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

// The missing-node transport contract

/// Both transports report STRUCTURAL absence the same way: a completed
/// compile whose product carries no such node reads as an absent response,
/// never a throw. Absence is structural (no such node), not a failure — a
/// throw would require matching error text. A successful control on the
/// same carrier proves the file loaded, and neither transport leaks a
/// product.
///
/// Each probe's Vue-arm positive control is internal: a Vue carrier that
/// HAS a `<style>` block must publish `style[0]` on the same demand or the
/// probe aborts the record. It is not a probe output key.
///
/// Absence reached THROUGH a refusal is a second lane. Both probes demand
/// products through the same complete typed compile request, which has no
/// intermediate state: the refusal replaces the whole transaction, so the
/// CSS-node demand and the main-node demand of the refused surface produce
/// the same record, carrying the host's typed refusal code instead of a
/// product. What neither transport may do is publish a product.
#[test]
fn the_transports_report_a_missing_node_the_same_way() {
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

    // Host must still answer the same way, or the comparisons decide
    // nothing. The demands are the typed requests the probes issue.
    let server = host_with(
        "/probe/Server.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    assert_eq!(
        host_typed_runtime_node(
            &server,
            "/probe/Server.svelte",
            ProbeFramework::Svelte,
            true,
            true,
            VirtualNodeKind::Style { index: 0 },
        ),
        HostOutcome::Refused {
            diagnostic_code: "svelte-runtime-unsupported-server-generate".to_string()
        },
        "the host route no longer refuses the server demand for the refused Svelte carrier, so \
         this target is stale"
    );

    let no_style = host_with(
        "/probe/NoStyle.vue",
        VUE_SFC,
        verter_language::FileLanguage::vue(),
    );
    assert_eq!(
        host_typed_runtime_node(
            &no_style,
            "/probe/NoStyle.vue",
            ProbeFramework::Vue,
            false,
            true,
            VirtualNodeKind::Style { index: 0 },
        ),
        HostOutcome::Missing,
        "the host route no longer reports a missing node for a style-less SFC, so this target \
         is stale"
    );

    // The successful control, on the SAME carrier as the structural case:
    // the host's answer to the same typed client-runtime demand for the
    // node that exists.
    let control = host_typed_runtime_node(
        &no_style,
        "/probe/NoStyle.vue",
        ProbeFramework::Vue,
        false,
        true,
        VirtualNodeKind::Main,
    );

    // Structural absence, where both transports still answer identically.
    {
        let label = "structural absence";
        let napi_case = &napi["cases"]["vueMissingStyle"];
        let wasm_case = &wasm["cases"]["vueMissingStyle"];
        // A missing node is never published as a product.
        for (transport, case) in [("napi", napi_case), ("wasm", wasm_case)] {
            assert_ne!(
                case["outcome"], "published",
                "{transport}/{label}: a missing node was published as a product: {case}"
            );
            assert_eq!(
                case["code"],
                Value::Null,
                "{transport}/{label}: a product crossed the boundary for a missing node: {case}"
            );
        }

        // The parity assertion.
        assert_eq!(
            napi_case["outcome"], wasm_case["outcome"],
            "{label}: the transports still spell a missing node differently: napi \
             {napi_case}, wasm {wasm_case}"
        );

        // Parity is not enough (both throwing would pass); pin the spelling.
        for (transport, case) in [("napi", napi_case), ("wasm", wasm_case)] {
            assert_eq!(
                case["outcome"], "missing",
                "{transport}/{label}: a missing node is no longer reported as an absent \
                 response: {case}"
            );
            assert_eq!(
                case["message"],
                Value::Null,
                "{transport}/{label}: a missing node produced a thrown error rather than an \
                 absent response: {case}"
            );
        }
    }

    // Absence reached THROUGH a refusal. The demand is the same on both
    // transports — the CSS node of a server surface this carrier cannot
    // emit — and neither may answer with bytes. The complete typed compile
    // refuses the transaction, so the record carries the host's typed
    // refusal code.
    //
    // On the complete-request route this record is byte-identical to
    // `svelteServerRefusal`'s: nothing was assembled, so the CSS demand and
    // the main-node demand cannot produce different answers, and the
    // `kind`/`index` the case asked for never gets consulted. This is
    // therefore NOT a second independent rail — its residual value is the
    // no-bytes check above. Restoring per-product refusal granularity
    // needs a wire that can answer per product, and belongs to whoever
    // introduces partial-product responses.
    let napi_refused = &napi["cases"]["svelteServerStyle"];
    let wasm_refused = &wasm["cases"]["svelteServerStyle"];
    for (transport, case) in [("napi", napi_refused), ("wasm", wasm_refused)] {
        assert_ne!(
            case["outcome"], "published",
            "{transport}/absence through a refusal: a product was published for a refused \
             compile: {case}"
        );
        assert_eq!(
            case["code"],
            Value::Null,
            "{transport}/absence through a refusal: a product crossed the boundary for a refused \
             compile: {case}"
        );
        assert_eq!(
            case["outcome"], "error",
            "{transport}/absence through a refusal: the typed compile request no longer surfaces \
             the carrier's refusal for a node of the refused surface: {case}"
        );
        assert!(
            case["message"].as_str().is_some_and(
                |message| message.contains("svelte-runtime-unsupported-server-generate")
            ),
            "{transport}/absence through a refusal: the refusal does not carry the host's typed \
             diagnostic code: {case}"
        );
    }

    // Control: the node that exists is published — absence is about the node.
    for (transport, record) in [("napi", &napi), ("wasm", &wasm)] {
        let case = &record["cases"]["vueMissingStyleControl"];
        assert_eq!(
            case["outcome"], "published",
            "{transport}: the successful control did not publish, so the absent answers above \
             cannot be attributed to the requested node: {case}"
        );
        let HostOutcome::Published { ref code, .. } = control else {
            panic!(
                "{transport}: the host route no longer publishes the control node, so absence \
                 proves nothing: {control:?}"
            )
        };
        assert_eq!(
            case["code"].as_str(),
            Some(code.as_str()),
            "{transport}: the successful control published something other than the host's \
             own product"
        );
    }

    // Each probe's Vue-arm positive control is internal to the probe (a Vue
    // carrier with a `<style>` block must publish `style[0]` or the process
    // aborts with no JSON). It is not a case key: adding one would change
    // the probe's output set.
}

// The bundler route

const BUNDLER_BUILD: &str =
    "pnpm --filter @verter/unplugin build (the plugin's fingerprinted dist/index.mjs)";

/// A published map, in parsed form.
///
/// The bundler hooks return a map as either an object or a JSON string, so both
/// shapes are accepted and the string is parsed. Absence fails here: every
/// caller is asking a question about a map that was supposed to be published.
#[track_caller]
fn parse_source_map_artifact(label: &str, map: &Value) -> Value {
    assert!(
        !map.is_null(),
        "{label}: no source-map artifact was published (the map itself is null)"
    );
    match map {
        Value::String(text) => serde_json::from_str::<Value>(text).unwrap_or_else(|error| {
            panic!(
                "{label}: the published source map is a string that is not JSON ({error}): {text}"
            )
        }),
        other => other.clone(),
    }
}

/// Assert a published source-map artifact, never a probe `hasMap` flag
/// (hard-coding `true` while `map` is `null` would pass). Reads the map
/// itself: present, parsed if it crossed as JSON, usable v3 envelope.
#[track_caller]
fn assert_source_map_artifact(label: &str, map: &Value) {
    let object = &parse_source_map_artifact(label, map);
    assert_eq!(
        object["version"],
        Value::from(3),
        "{label}: the published source map is not a v3 map: {object}"
    );
    let mappings = object["mappings"].as_str().unwrap_or_else(|| {
        panic!("{label}: the published source map has no `mappings` string: {object}")
    });
    assert!(
        !mappings.is_empty(),
        "{label}: the published source map's `mappings` is empty, so it maps nothing: {object}"
    );
    let sources = object["sources"].as_array().unwrap_or_else(|| {
        panic!("{label}: the published source map has no `sources` array: {object}")
    });
    assert!(
        !sources.is_empty(),
        "{label}: the published source map names no sources: {object}"
    );
}

/// The measured structure of a v3 `mappings` string.
#[derive(Debug, PartialEq, Eq)]
struct MappingsShape {
    /// Generated lines, i.e. `;`-separated groups.
    lines: usize,
    /// Non-empty `,`-separated segments across all lines.
    segments: usize,
    /// Segments that name an authored position — four or five VLQ fields.
    /// A ONE-FIELD segment names only a generated column and maps to nothing a
    /// consumer can navigate to, so it is counted separately here.
    mapped_segments: usize,
}

/// The base64 alphabet source maps encode VLQ digits in.
const VLQ_BASE64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// How many VLQ fields one segment carries.
///
/// Each field is a run of digits ended by the first digit without the
/// continuation bit (0x20), so the fields are counted without decoding the
/// values themselves.
#[track_caller]
fn vlq_field_count(label: &str, segment: &str) -> usize {
    segment
        .chars()
        .filter(|character| {
            let digit = VLQ_BASE64.find(*character).unwrap_or_else(|| {
                panic!("{label}: `{segment}` is not a base64 VLQ segment ({character:?})")
            });
            digit & 0x20 == 0
        })
        .count()
}

/// Read a published map's `mappings` structure.
#[track_caller]
fn mappings_shape(label: &str, map: &Value) -> MappingsShape {
    let object = parse_source_map_artifact(label, map);
    let mappings = object["mappings"].as_str().unwrap_or_else(|| {
        panic!("{label}: the published source map has no `mappings` string: {object}")
    });
    let lines: Vec<&str> = mappings.split(';').collect();
    let segments: Vec<&str> = lines
        .iter()
        .flat_map(|line| line.split(','))
        .filter(|segment| !segment.is_empty())
        .collect();
    MappingsShape {
        lines: lines.len(),
        mapped_segments: segments
            .iter()
            .filter(|segment| vlq_field_count(label, segment) >= 4)
            .count(),
        segments: segments.len(),
    }
}

/// Whole-artifact comparison. Envelope checks cannot prove the route
/// preserved the map; comparing `sources` plus presence of `sourcesContent`
/// accepts forged content (what a debugger shows). Compare every member.
/// Normalize only key order (JSON objects are unordered) and absent vs
/// explicit `null`. Empty `""`/`[]` is a statement, not an absence.
#[track_caller]
fn normalized_source_map(label: &str, map: &Value) -> Value {
    let mut parsed = parse_source_map_artifact(label, map);
    if let Some(object) = parsed.as_object_mut() {
        object.retain(|_, value| !value.is_null());
    }
    parsed
}

/// The host's published map, normalized the same way.
#[track_caller]
fn host_normalized_source_map(label: &str, outcome: &HostOutcome) -> Value {
    normalized_source_map(label, &Value::String(outcome.published_map(label)))
}

/// Envelope plus at least one mapped segment (`"A"` is a valid envelope
/// that maps nothing). Not applied to the public Svelte virtual-script
/// product, whose map is that empty segment today.
#[track_caller]
fn assert_mapped_source_map_artifact(label: &str, map: &Value) {
    assert_source_map_artifact(label, map);
    let shape = mappings_shape(label, map);
    assert!(
        shape.mapped_segments > 0,
        "{label}: the published source map is a valid envelope that maps NOTHING — {} segment(s), \
         none naming an authored position ({shape:?})",
        shape.segments
    );
}

/// Pin the case's carrier ids to this test's constants before reading it.
#[track_caller]
fn assert_bundler_case_carriers(case: &Value, expected_id: &str, expected_opposite: &str) {
    assert_eq!(
        case["id"].as_str(),
        Some(expected_id),
        "the case this test reads as `{expected_id}` reports a different requested carrier, so \
         every assertion below would be about something else: {case}"
    );
    // Opposite-carrier include is the rejection half; missing `oppositeId`
    // would make that assertion vacuous.
    assert_eq!(
        case["oppositeId"].as_str(),
        Some(expected_opposite),
        "the case's opposite-carrier decision was taken against a carrier this test did not ask \
         about (expected `{expected_opposite}`): {case}"
    );
}

/// Every `sources` entry must name the requested carrier. A well-formed
/// map retargeted at another file would otherwise pass.
#[track_caller]
fn assert_source_map_names_only(label: &str, map: &Value, requested_id: &str) {
    let object = parse_source_map_artifact(label, map);
    let sources = object["sources"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("{label}: the published source map has no `sources` array: {object}")
        })
        .iter()
        .map(|source| source.as_str().unwrap_or("<non-string>").to_string())
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "{label}: the published source map names no sources, so it describes no request"
    );
    let foreign: Vec<&String> = sources
        .iter()
        .filter(|source| source.as_str() != requested_id)
        .collect();
    assert!(
        foreign.is_empty(),
        "{label}: the map published for `{requested_id}` names {} source(s) that are not that \
         carrier ({foreign:?}), so it describes a different compilation: sources={sources:?}",
        foreign.len()
    );
}

/// The counterpart of [`assert_source_map_artifact`]: the ABSENCE of a map is
/// asserted on the artifact too, not on the derived flag.
#[track_caller]
fn assert_no_source_map_artifact(label: &str, map: &Value) {
    assert_eq!(
        *map,
        Value::Null,
        "{label}: a source-map artifact was published where none was expected"
    );
}

/// Bundler route from the fingerprinted built entry (bundler → transport
/// → host), compared to the in-process host for the same source.
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

    // Svelte public wrapper's virtual-script load equals host `Main`.
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

    // Vue public entry + virtual-script load (content owned elsewhere).
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

/// `VerterVue.vite({})` accepts `.vue`; `VerterSvelte.vite({})` accepts
/// `.svelte`; each rejects the other extension.
#[test]
fn the_bundler_public_entries_apply_their_documented_include_contract() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let vue = &record["cases"]["vuePublicEntry"];
    let svelte = &record["cases"]["sveltePublicEntry"];

    // Carrier ids are this test's constants, not the probe record's.
    assert_bundler_case_carriers(vue, PROBE_VUE_ID, PROBE_SVELTE_ID);
    assert_bundler_case_carriers(svelte, PROBE_SVELTE_ID, PROBE_VUE_ID);

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

/// Independent reading of the same built bundler entry. The probe's JSON
/// is one program's word; this script lives here, imports the same entry,
/// and wraps each callable in an apply-counting Proxy so "executed" is
/// witnessed by an apply this test performed. `transformInclude` is asked
/// here too. Residue: this drives the factory and include decision, never
/// a carrier transform — product bytes stay the probe's word, judged by
/// host parity separately. Reuses the probe's freshness fingerprint.
const BUNDLER_OBSERVER_SCRIPT: &str = r#"
const { pathToFileURL } = await import("node:url");
const entry = process.env.VERTER_OBSERVED_ENTRY;
if (!entry) throw new Error("no observed entry was supplied");
const observed = await import(pathToFileURL(entry).href);
const names = Object.keys(observed).sort();
const viteConfig = { root: process.cwd(), command: "serve", build: { ssr: false } };
const observations = {};
for (const name of names) {
  const value = observed[name];
  const record = {
    valueType: typeof value,
    viteIsCallable: typeof value?.vite === "function",
    rollupIsCallable: typeof value?.rollup === "function",
    aliasOf:
      names
        .slice(0, names.indexOf(name))
        .find((earlier) => observed[earlier] === value) ?? null,
    applyCount: 0,
    pluginKeys: null,
    carrierIncludes: null,
    driveError: null,
  };
  observations[name] = record;
  // An alias is the same object as a spelling already driven; driving it twice
  // would attribute a second apply to a value that was invoked once.
  if (record.aliasOf !== null) continue;

  // WHICH callable is applied follows from the value, exactly as it does in
  // the probe: an unplugin object exposes a `.vite` factory, a raw unplugin
  // factory is itself the callable and takes the consumer's bundler meta.
  let target = null;
  let self;
  let args;
  if (value !== null && typeof value === "object" && typeof value.vite === "function") {
    target = value.vite;
    self = value;
    args = [{}];
  } else if (typeof value === "function") {
    target = value;
    self = undefined;
    args = [{}, { framework: "vite" }];
  }
  if (target === null) continue;

  const counted = new Proxy(target, {
    apply(fn, thisArg, applied) {
      record.applyCount += 1;
      return Reflect.apply(fn, thisArg, applied);
    },
  });
  try {
    const plugin = Reflect.apply(counted, self, args);
    if (typeof plugin?.configResolved === "function") {
      await plugin.configResolved(viteConfig);
    } else if (typeof plugin?.vite?.configResolved === "function") {
      await plugin.vite.configResolved.call({}, viteConfig);
    }
    record.pluginKeys = Object.keys(plugin).sort();
    // The include decision needs no host, so asking it here costs nothing and
    // takes the other half of the spelling discriminator off the probe's word.
    record.carrierIncludes = {
      vue: plugin.transformInclude.call({}, "/probe/Plug.vue"),
      svelte: plugin.transformInclude.call({}, "/probe/Plug.svelte"),
    };
    if (typeof plugin?.closeBundle === "function") await plugin.closeBundle.call({});
  } catch (error) {
    record.driveError = String(error?.message ?? error);
  }
}
process.stdout.write(
  JSON.stringify({
    exports: names,
    // The `default` alias, evaluated HERE rather than taken from the probe.
    defaultIsVerterVue: observed.default === observed.VerterVue,
    observations,
  }),
);
"#;

/// Run [`BUNDLER_OBSERVER_SCRIPT`] against the built entry.
///
/// A missing or failing `node` FAILS here. Skipping would turn the independent
/// half of the check into an optional one, which is the same
/// no-evidence-reads-as-pass shape these guards exist to reject.
fn observe_bundler_entry() -> Value {
    let entry = repo_root().join("packages/unplugin/dist/index.mjs");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", BUNDLER_OBSERVER_SCRIPT])
        .env("VERTER_OBSERVED_ENTRY", &entry)
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "the test-owned observation of {} could not run: node failed ({error}). This \
                 check is required, never optional.",
                entry.display()
            )
        });
    assert!(
        output.status.success(),
        "the test-owned observation of {} exited {:?}:\n{}",
        entry.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap_or_else(|error| {
        panic!(
            "the test-owned observation emitted no JSON ({error}):\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Whether the probe's recorded evidence says this export exposes a callable
/// adapter under `key` — derived here, so it is comparable with the
/// observation's own reading of the same value.
fn probe_says_adapter_callable(case: &Value, key: &str) -> bool {
    let evidence = &case["evidence"];
    if evidence["valueType"].as_str() != Some("object") {
        return false;
    }
    let (Some(keys), Some(types)) = (
        evidence["ownKeys"].as_array(),
        evidence["ownKeyTypes"].as_array(),
    ) else {
        return false;
    };
    keys.iter()
        .zip(types)
        .any(|(name, kind)| name == key && kind == "function")
}

/// The probe's record and the test-owned observation must describe the same
/// module.
#[track_caller]
fn assert_probe_agrees_with_the_test_owned_observation(record: &Value) {
    // The observation reads the entry the probe proved fresh; no second
    // fingerprint is introduced.
    assert_eq!(
        record["fresh"], true,
        "bundler: the probe did not prove the built entry fresh, so the independent observation \
         below would be reading an unverified artifact: {record}"
    );
    let observed = observe_bundler_entry();

    // ENUMERATION, independently: a probe that hid or invented an export is
    // caught here, where the export list was previously its word alone.
    assert_eq!(
        record["exports"], observed["exports"],
        "bundler: the probe's export enumeration disagrees with this test's own reading of the \
         same built entry"
    );

    // The `default` alias, read by this test rather than taken from the record.
    assert_eq!(
        record["defaultIsVerterVue"], observed["defaultIsVerterVue"],
        "bundler: the probe and this test's own reading disagree on whether `default` and \
         `VerterVue` are the same object"
    );

    for name in record["exports"]
        .as_array()
        .expect("the export list is an array")
        .iter()
        .filter_map(Value::as_str)
    {
        let case = &record["exportCases"][name];
        let seen = &observed["observations"][name];
        assert_eq!(
            case["evidence"]["valueType"], seen["valueType"],
            "bundler/{name}: the probe reports a {} where this test reads a {} in the same built \
             entry",
            case["evidence"]["valueType"], seen["valueType"]
        );
        for adapter in ["vite", "rollup"] {
            assert_eq!(
                Value::Bool(probe_says_adapter_callable(case, adapter)),
                seen[format!("{adapter}IsCallable")],
                "bundler/{name}: the probe's evidence and this test's own reading disagree on \
                 whether the value exposes a callable {adapter} adapter: {case}"
            );
        }
        assert_eq!(
            case["aliasOf"], seen["aliasOf"],
            "bundler/{name}: the probe's measured alias identity disagrees with this test's own \
             reading of the same module: {case}"
        );

        // Executed class is witnessed by an apply this test performed.
        if !derived_kind(name, case).is_executed() {
            continue;
        }
        assert_eq!(
            seen["driveError"],
            Value::Null,
            "bundler/{name}: this test classifies the spelling as executed, but this test's own \
             invocation of it failed ({}), so the probe's claim that it drove this export cannot \
             be corroborated",
            seen["driveError"]
        );
        let applied = seen["applyCount"].as_u64().unwrap_or_else(|| {
            panic!(
                "bundler/{name}: this test's own observation recorded no apply count for a \
                 spelling it classifies as executed: {seen}"
            )
        });
        assert!(
            applied >= 1,
            "bundler/{name}: this test classifies the spelling as executed, but this test's own \
             apply-counting invocation of it ran {applied} time(s) — the `executed` class would \
             then rest on the probe's claim alone: {seen}"
        );
        assert_eq!(
            case["pluginKeys"], seen["pluginKeys"],
            "bundler/{name}: the plugin the PROBE reports driving is not the plugin this test's \
             own invocation of the same spelling returned, so the probe's drive result was not \
             produced by applying this export: probe={} observed={}",
            case["pluginKeys"], seen["pluginKeys"]
        );
        for carrier in ["vue", "svelte"] {
            assert!(
                seen["carrierIncludes"][carrier].is_boolean(),
                "bundler/{name}: this test's own drive recorded no {carrier} include decision, so \
                 the contract row's `accepts_{carrier}` column would rest on the probe alone: \
                 {seen}"
            );
            assert_eq!(
                case["carriers"][carrier]["transformInclude"], seen["carrierIncludes"][carrier],
                "bundler/{name}: the {carrier} include decision the probe reports differs from \
                 the one this test's own invocation of the same spelling produced: {case}"
            );
        }
    }
}

/// Kind derived here from probe evidence (`typeof`, own keys, alias) —
/// never a probe-assigned `kind` string (a copied opinion agrees with
/// itself). Honest reading is
/// [`assert_probe_agrees_with_the_test_owned_observation`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum DerivedKind {
    /// A `createUnplugin` result: an object exposing a callable `.vite`.
    UnpluginObject,
    /// A bare unplugin factory a consumer calls with its own bundler meta.
    RawFactory,
    /// The same object as an earlier export, by measured identity.
    Alias(String),
    /// Neither, with the evidence-stated reason.
    NotDrivable(String),
}

impl DerivedKind {
    fn is_executed(&self) -> bool {
        matches!(self, DerivedKind::UnpluginObject | DerivedKind::RawFactory)
    }
}

/// Derive one export's kind from its recorded evidence alone.
#[track_caller]
fn derived_kind(export: &str, case: &Value) -> DerivedKind {
    if let Some(alias_of) = case["aliasOf"].as_str() {
        return DerivedKind::Alias(alias_of.to_string());
    }
    let evidence = &case["evidence"];
    let value_type = evidence["valueType"].as_str().unwrap_or_else(|| {
        panic!("bundler/{export}: the case records no `evidence.valueType`: {case}")
    });
    match value_type {
        "function" => DerivedKind::RawFactory,
        "object" => {
            let keys = evidence["ownKeys"].as_array().unwrap_or_else(|| {
                panic!("bundler/{export}: an object case records no `evidence.ownKeys`: {case}")
            });
            let types = evidence["ownKeyTypes"].as_array().unwrap_or_else(|| {
                panic!("bundler/{export}: an object case records no `evidence.ownKeyTypes`: {case}")
            });
            assert_eq!(
                keys.len(),
                types.len(),
                "bundler/{export}: the recorded own-key list and its type list disagree in \
                 length, so the evidence is not internally consistent: {evidence}"
            );
            let exposes_vite = keys
                .iter()
                .zip(types)
                .any(|(key, kind)| key == "vite" && kind == "function");
            if exposes_vite {
                DerivedKind::UnpluginObject
            } else {
                DerivedKind::NotDrivable(format!(
                    "an object exposing no callable `vite` (own keys {keys:?})"
                ))
            }
        }
        other => DerivedKind::NotDrivable(format!("a value of type `{other}`")),
    }
}

/// Evidence vs drive result. Wrapped entries flatten `configResolved`;
/// raw factories keep it nested. A copied sibling case cannot match both.
#[track_caller]
fn assert_evidence_matches_the_driven_plugin(export: &str, case: &Value, kind: &DerivedKind) {
    let keys: Vec<&str> = case["pluginKeys"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("bundler/{export}: a driven export records no `pluginKeys`: {case}")
        })
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        !keys.is_empty(),
        "bundler/{export}: the driven plugin exposed no hooks at all: {case}"
    );
    let flattened = keys.contains(&"configResolved");
    match kind {
        DerivedKind::UnpluginObject => assert!(
            flattened,
            "bundler/{export}: read as an unplugin object, but the plugin it returned does NOT \
             carry a flattened `configResolved` — the two observations disagree, so at least one \
             is not this value's: pluginKeys={keys:?}"
        ),
        DerivedKind::RawFactory => assert!(
            !flattened,
            "bundler/{export}: read as a raw factory, but the plugin it returned carries a \
             FLATTENED `configResolved`, which only `createUnplugin` produces — the two \
             observations disagree, so at least one is not this value's: pluginKeys={keys:?}"
        ),
        other => panic!("bundler/{export}: {other:?} is not a driven kind"),
    }
}

/// Discriminator: `kind` from [`derived_kind`], `accepts_*` from
/// `transformInclude`. Pairwise distinct — asserted by
/// [`the_bundler_public_spellings_are_distinguished_by_what_they_accept`].
struct BundlerSpelling {
    export: &'static str,
    kind: DerivedKind,
    accepts_vue: bool,
    accepts_svelte: bool,
}

fn bundler_spelling_contracts() -> Vec<BundlerSpelling> {
    vec![
        BundlerSpelling {
            export: "Verter",
            kind: DerivedKind::UnpluginObject,
            accepts_vue: true,
            accepts_svelte: true,
        },
        BundlerSpelling {
            export: "VerterSvelte",
            kind: DerivedKind::UnpluginObject,
            accepts_vue: false,
            accepts_svelte: true,
        },
        BundlerSpelling {
            export: "VerterVue",
            kind: DerivedKind::UnpluginObject,
            accepts_vue: true,
            accepts_svelte: false,
        },
        BundlerSpelling {
            export: "unpluginFactory",
            kind: DerivedKind::RawFactory,
            accepts_vue: true,
            accepts_svelte: false,
        },
    ]
}

/// Every bundler export is executed or classified out of scope, derived
/// from the probe's per-export records — not a list kept here.
#[test]
fn every_exported_bundler_spelling_is_executed_or_classified_out_of_scope() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let exported: Vec<String> = record["exports"]
        .as_array()
        .expect("the built plugin entry enumerates its exports")
        .iter()
        .map(|name| {
            name.as_str()
                .expect("an export name is a string")
                .to_string()
        })
        .collect();
    assert!(
        !exported.is_empty(),
        "the enumeration found no bundler exports, so it proves nothing"
    );
    assert_probe_agrees_with_the_test_owned_observation(&record);

    // The case set IS the export set — in both directions.
    let cases = record["exportCases"]
        .as_object()
        .expect("the probe records one case per enumerated export");
    assert_eq!(
        cases.len(),
        exported.len(),
        "bundler: the probe recorded {} case(s) for {} exported spelling(s), so the case set is \
         no longer the export set: cases={:?} exports={exported:?}",
        cases.len(),
        exported.len(),
        cases.keys().collect::<Vec<_>>()
    );
    for name in &exported {
        let case = &record["exportCases"][name];
        assert!(
            !case.is_null(),
            "bundler/{name}: the enumeration names this export but the probe recorded no case"
        );
        // The record must carry the name it was READ BY. A case produced for a
        // different spelling therefore cannot be filed under this one.
        assert_eq!(
            case["exportName"].as_str(),
            Some(name.as_str()),
            "bundler/{name}: the case filed under this export was produced by reading `{}` \
             instead, so the observation is not this spelling's",
            case["exportName"]
        );
    }

    // The two classes, derived HERE from each case's evidence. Nothing on this
    // path reads a classification the probe wrote down.
    let kinds: Vec<DerivedKind> = exported
        .iter()
        .map(|name| derived_kind(name, &record["exportCases"][name]))
        .collect();
    let mut executed: Vec<&str> = Vec::new();
    let mut classified: Vec<(String, String)> = Vec::new();
    for (name, kind) in exported.iter().zip(&kinds) {
        let case = &record["exportCases"][name];
        if kind.is_executed() {
            assert_ne!(
                case["outcome"], "error",
                "bundler/{name}: the probe drove this export and it errored: {case}"
            );
            // The two independent observations of this value must agree.
            assert_evidence_matches_the_driven_plugin(name, case, kind);
            // Driving an export means it answered for both carriers.
            for carrier in ["vue", "svelte"] {
                assert!(
                    case["carriers"][carrier]["transformInclude"].is_boolean(),
                    "bundler/{name}: its evidence says it is drivable, but no {carrier} include \
                     decision was recorded, so it was not driven: {case}"
                );
            }
            executed.push(name.as_str());
            continue;
        }
        let reason = match kind {
            DerivedKind::Alias(target) => {
                format!("the same object as the executed `{target}` spelling, by measured identity")
            }
            DerivedKind::NotDrivable(reason) => reason.clone(),
            _ => unreachable!("executed kinds are handled above"),
        };
        classified.push((name.clone(), reason));
    }
    let classified_refs: Vec<(&str, &str)> = classified
        .iter()
        .map(|(name, reason)| (name.as_str(), reason.as_str()))
        .collect();
    assert_partition("bundler", &exported, &executed, &classified_refs, &[]);

    // An ALIAS is only a classification if what it aliases actually ran, AND if
    // the two spellings' evidence agrees — a necessary consequence of the two
    // being the same object, and checkable without trusting the identity claim.
    for (name, kind) in exported.iter().zip(&kinds) {
        let DerivedKind::Alias(target) = kind else {
            continue;
        };
        assert!(
            executed.contains(&target.as_str()),
            "bundler/{name}: classified as an alias of `{target}`, which is not itself executed, \
             so neither spelling was driven"
        );
        assert_eq!(
            record["exportCases"][name]["evidence"], record["exportCases"][target]["evidence"],
            "bundler/{name}: claimed to be the same object as `{target}`, but the two spellings' \
             evidence differs, so they cannot be one value"
        );
    }

    // The historically documented alias, still measured rather than claimed —
    // and the measurement is now cross-checked against this test's own reading
    // of the same module by
    // [`assert_probe_agrees_with_the_test_owned_observation`] above.
    assert_eq!(
        record["defaultIsVerterVue"], true,
        "bundler: the `default` export is no longer the `VerterVue` object: {record}"
    );
    assert_eq!(
        record["exportCases"]["default"]["aliasOf"], "VerterVue",
        "bundler: the `default` export no longer aliases `VerterVue`: {}",
        record["exportCases"]["default"]
    );
}

/// Every executed spelling, on the contract that tells it apart from its
/// siblings — and on what its own hooks produced.
#[test]
fn the_bundler_public_spellings_are_distinguished_by_what_they_accept() {
    let contracts = bundler_spelling_contracts();

    // Pairwise distinct triples, or a row could satisfy another spelling.
    for (index, left) in contracts.iter().enumerate() {
        for right in contracts.iter().skip(index + 1) {
            assert!(
                (&left.kind, left.accepts_vue, left.accepts_svelte)
                    != (&right.kind, right.accepts_vue, right.accepts_svelte),
                "`{}` and `{}` are indistinguishable on (kind, accepts_vue, accepts_svelte), so \
                 neither row can tie an observation to its spelling",
                left.export,
                right.export
            );
        }
    }

    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );

    // Contract table and driven set stay in step (driven set from evidence).
    let exported: Vec<String> = record["exports"]
        .as_array()
        .expect("the built plugin entry enumerates its exports")
        .iter()
        .map(|name| name.as_str().unwrap_or_default().to_string())
        .collect();
    let executed: Vec<&str> = exported
        .iter()
        .filter(|name| derived_kind(name, &record["exportCases"][name.as_str()]).is_executed())
        .map(String::as_str)
        .collect();
    assert_probe_agrees_with_the_test_owned_observation(&record);
    let contracted: Vec<&str> = contracts.iter().map(|row| row.export).collect();
    assert_eq!(
        executed, contracted,
        "the executed spellings and the contract rows have diverged"
    );

    for row in &contracts {
        let case = &record["exportCases"][row.export];
        let observed = derived_kind(row.export, case);
        assert_eq!(
            observed, row.kind,
            "bundler/{}: its recorded evidence describes a {observed:?} where this spelling is a \
             {:?} — the case belongs to a different export shape: {case}",
            row.export, row.kind
        );
        assert_evidence_matches_the_driven_plugin(row.export, case, &observed);
        assert_eq!(
            case["carriers"]["vue"]["transformInclude"], row.accepts_vue,
            "bundler/{}: its own `transformInclude` disagrees with this spelling's documented \
             `.vue` contract: {case}",
            row.export
        );
        assert_eq!(
            case["carriers"]["svelte"]["transformInclude"], row.accepts_svelte,
            "bundler/{}: its own `transformInclude` disagrees with this spelling's documented \
             `.svelte` contract: {case}",
            row.export
        );
        // An accepted carrier must have produced a product, so "accepts" is not
        // satisfied by an include decision alone.
        for (carrier, accepts) in [("vue", row.accepts_vue), ("svelte", row.accepts_svelte)] {
            if !accepts {
                continue;
            }
            assert_eq!(
                case["carriers"][carrier]["loadedScriptOutcome"], "published",
                "bundler/{}: it accepted the {carrier} carrier but published no virtual-script \
                 product: {case}",
                row.export
            );
        }
    }

    // `VerterVue` is `createUnplugin(unpluginFactory)`, so the wrapped entry's
    // product IS the raw factory's product.
    let raw_vue = &record["exportCases"]["unpluginFactory"]["carriers"]["vue"]["loadedScriptCode"];
    assert!(
        raw_vue.as_str().is_some_and(|code| !code.is_empty()),
        "unpluginFactory: the raw factory published empty code, so the comparison below is vacuous"
    );
    assert_eq!(
        raw_vue, &record["exportCases"]["VerterVue"]["carriers"]["vue"]["loadedScriptCode"],
        "unpluginFactory: the raw factory's product differs from the public Vue entry's, so the \
         public entry is no longer wrapping this factory"
    );

    // The auto entry routed each carrier to ITS OWN product, and its Svelte
    // product is the in-process host route's `Main`.
    let auto = &record["exportCases"]["Verter"]["carriers"];
    assert_ne!(
        auto["vue"]["loadedScriptCode"], auto["svelte"]["loadedScriptCode"],
        "Verter: both carriers produced identical bytes, so this cannot detect a mis-routed carrier"
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
        panic!("Verter: the host route no longer publishes this module: {expected:?}");
    };
    assert_eq!(
        auto["svelte"]["loadedScriptCode"].as_str(),
        Some(host_code.as_str()),
        "Verter: the auto entry's loaded Svelte bytes differ from the host route's `Main`"
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
    assert_bundler_case_carriers(vue, PROBE_VUE_ID, PROBE_SVELTE_ID);
    assert_bundler_case_carriers(svelte, PROBE_SVELTE_ID, PROBE_VUE_ID);

    for (factory, case) in [("VerterVue.vite", vue), ("VerterSvelte.vite", svelte)] {
        assert_no_source_map_artifact(
            &format!("{factory}: the routing wrapper unexpectedly became the mapped product"),
            &case["wrapperMap"],
        );
        assert_eq!(
            case["loadedScriptOutcome"], "published",
            "{factory}: the public virtual-script load did not publish a product: {case}"
        );
    }

    // The VUE product's map is checked by the ENVELOPE ORACLE ALONE — there is
    // no host counterpart to compare it against (see the note at the end of
    // this test) — so the oracle has to carry the whole acceptance. Two things
    // are therefore required of it: that it maps SOMETHING, and that what it
    // maps is THIS request. Without the second, a map retargeted at an
    // unrelated file satisfies every structural check while describing a
    // different compilation entirely.
    assert_mapped_source_map_artifact(
        "VerterVue.vite: the mapped virtual-script product",
        &vue["loadedScriptMap"],
    );
    assert_source_map_names_only(
        "VerterVue.vite: the mapped virtual-script product",
        &vue["loadedScriptMap"],
        PROBE_VUE_ID,
    );
    // The SVELTE product is held to the same MAPPED oracle as the Vue one. It
    // was once envelope-only, because its map was a single segment naming no
    // authored position; the map builder was corrected and the coverage it now
    // has is pinned as a floor by
    // [`both_published_virtual_script_maps_cover_their_output`]. The acceptance
    // for this product remains the host PARITY comparison below, which is
    // stronger than either oracle.
    assert_mapped_source_map_artifact(
        "VerterSvelte.vite: the mapped virtual-script product",
        &svelte["loadedScriptMap"],
    );

    // the ACCEPTANCE: the Svelte product carries the HOST's map
    // This case already compares its loaded bytes against the host's `Main`
    // product, so its map is comparable the same way.
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
    assert_eq!(
        normalized_source_map(
            "VerterSvelte.vite: the virtual-script product's map",
            &svelte["loadedScriptMap"],
        ),
        host_normalized_source_map("the host's Svelte `Main` product's map", &expected),
        "VerterSvelte.vite: the published virtual-script map is not the map the host published \
         for the same requested profile"
    );

    // The VUE case has no established host counterpart: its `?vue&type=script`
    // sub-request is a Vite-only split of the SFC whose rendered content is
    // owned elsewhere, and the suite's route comparison likewise asserts only
    // that it published non-empty code. So it keeps the envelope precondition
    // above and no parity assertion — stated rather than silently omitted.
    let _ = vue;
}

/// Both published virtual-script maps navigate somewhere, measured at the
/// STRUCTURE level rather than through a presence flag.
///
/// The green acceptance target above
/// ([`the_bundler_virtual_script_loads_publish_requested_source_maps`]) asks
/// only whether a v3 map with a non-empty `mappings` string was published, and
/// the single segment `"A"` — one generated column, no authored position,
/// nothing a consumer can navigate to — satisfies that. This measures what the
/// maps actually cover.
///
/// Each carrier is pinned as a FLOOR at its measured values, so an improvement
/// keeps this green while a loss of coverage fails it. The Svelte floor was
/// once the empty single segment; the Svelte map builder was corrected and the
/// floor is re-measured here to what it now covers, on BOTH public Svelte
/// routes — the Vite virtual-script load and the Rollup one — which carry the
/// same map, so this is the map builder output rather than one bundler
/// adapter handling of it.
#[test]
fn both_published_virtual_script_maps_cover_their_output() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );

    let vue = mappings_shape(
        "VerterVue.vite: the virtual-script map",
        &record["cases"]["vuePublicEntry"]["loadedScriptMap"],
    );
    assert!(
        vue.lines >= 18 && vue.segments >= 16 && vue.mapped_segments >= 12,
        "VerterVue.vite: the virtual-script map covers less than the recorded measurement \
         (18 generated lines, 16 segments, 12 of them naming an authored position): {vue:?}"
    );

    for label in ["sveltePublicEntry", "svelteRollupEntry"] {
        let svelte = mappings_shape(
            &format!("bundler/{label}: the virtual-script map"),
            &record["cases"][label]["loadedScriptMap"],
        );
        assert!(
            svelte.lines >= 7 && svelte.segments >= 3 && svelte.mapped_segments >= 1,
            "bundler/{label}: the Svelte virtual-script map covers less than the recorded \
             measurement (7 generated lines, 3 segments, 1 of them naming an authored \
             position): {svelte:?}"
        );
    }

    // The floors above are counts, and a count can be met by a map that names
    // no authored position at all if the floor were ever lowered to zero. Both
    // carriers are therefore also required to map SOMETHING, stated separately
    // so a future re-measurement cannot quietly drop it.
    assert!(
        vue.mapped_segments > 0 && vue.segments > 0,
        "VerterVue.vite: the published map names no authored position: {vue:?}"
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
    // Envelope only, for the same recorded reason as the Vite route above: this
    // product's map is a single unmapped segment today, and its acceptance is
    // the host parity comparison that follows.
    assert_source_map_artifact(
        "VerterSvelte.rollup: the virtual-script product",
        &svelte["loadedScriptMap"],
    );
    // Same treatment as the Vite route: the Svelte Rollup virtual-script load
    // serves the host's `Main` product, so its map is PARITY-tested against the
    // host's, not merely envelope-checked.
    let svelte_host = host_with(
        "/probe/Plug.svelte",
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    assert_eq!(
        normalized_source_map(
            "VerterSvelte.rollup: the virtual-script product's map",
            &svelte["loadedScriptMap"],
        ),
        host_normalized_source_map(
            "the host's Svelte `Main` product's map",
            &host_node(
                &svelte_host,
                "/probe/Plug.svelte",
                VirtualNodeKind::Main,
                &CompileProfile {
                    source_map: true,
                    hmr_strategy: crate::types::HmrStrategy::None,
                    ..CompileProfile::default()
                },
            ),
        ),
        "VerterSvelte.rollup: the published virtual-script map is not the map the host published \
         for the same requested profile"
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
    let host_has_map = matches!(
        host_product,
        HostOutcome::Published {
            source_map: Some(_),
            ..
        }
    );
    assert!(
        host_has_map,
        "the matching source-map-requesting host profile did not publish a map, so Rollup parity \
         cannot be tested: {host_product:?}"
    );

    // Usable map that maps something — not the whole acceptance (an envelope
    // for an unrelated file would also pass).
    let inline_label = format!(
        "VerterVue.rollup: the public non-Vite inline product's requested map \
         (hostHasMap={host_has_map}, publicTransformIsInline={}, publicTransformHasMap={})",
        vue["publicTransformIsInline"], vue["publicTransformHasMap"]
    );
    assert_mapped_source_map_artifact(&inline_label, &vue["publicTransformMap"]);

    // Acceptance: same map the host published for this profile.
    assert_eq!(
        normalized_source_map(&inline_label, &vue["publicTransformMap"]),
        host_normalized_source_map("the host's Vue `Main` product's map", &host_product),
        "VerterVue.rollup: the public non-Vite inline product does not carry the map the host \
         published for the same requested profile"
    );
}

// The bundler route's remaining product lanes
//
// The lanes below are reached through the SAME built entry the tests above
// drive, but through hooks the wrapper/virtual-script pair never touches: the
// style artifacts a wrapper points at, a virtual request no transform cached,
// the render-only batch lane the carrier transform publishes from, the non-Vite
// CSS scoping pass, and the pre-compile pass `buildStart` owns.
//
// Each is an ALIAS of a host route already proven live in the product/route
// inventory, so what is asked of it is route identity plus publication — the
// bundler's product must BE the in-process host's product for the same typed
// request — not a new semantic case.

/// The lexical repository root, matching how the probe derives its own.
///
/// [`repo_root`] keeps `..` segments (it is only ever joined onto), which
/// cannot be string-compared against the probe's already-normalized answer.
fn repo_root_lexical() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the repository root")
        .to_path_buf()
}

/// The eight-hex-digit component id the bundler derives, restated here.
///
/// `generateComponentId` (`packages/unplugin/src/core/compiler.ts`) hashes the
/// root-relative carrier path — plus the source, in a production profile — and
/// keeps the first eight hex digits of the SHA-256. It is restated rather than
/// read back out of the probe record on purpose: a component id taken from the
/// record makes every assertion resting on it an assertion about whatever id
/// the record chose.
fn bundler_component_id(hashed: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(hashed.as_bytes())
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The profile `VerterVue.vite({})` builds under the probe's resolved config.
///
/// `command: "serve"` with no SSR makes it a DEV profile, which is what selects
/// the Vite HMR strategy and the path-only component id; a resolved Vite config
/// whose framework is Vite is also what leaves TS stripping to `vite:esbuild`
/// rather than to the host.
fn bundler_vue_vite_profile(carrier_id: &str) -> CompileProfile {
    CompileProfile {
        filename: Some(carrier_id.to_string()),
        is_production: false,
        custom_element: false,
        ssr: false,
        hmr_strategy: crate::types::HmrStrategy::Vite,
        component_id: Some(bundler_component_id(carrier_id)),
        source_map: true,
        force_js: false,
        ..CompileProfile::default()
    }
}

/// The profile `VerterSvelte.vite({})` builds under the same resolved config.
///
/// The Svelte lane pins HMR off (native Svelte HMR codegen is fail-closed) and
/// forces JS regardless of the bundler, so this is NOT the Vue profile with a
/// different carrier.
fn bundler_svelte_vite_profile(carrier_id: &str) -> CompileProfile {
    CompileProfile {
        filename: Some(carrier_id.to_string()),
        is_production: false,
        custom_element: false,
        ssr: false,
        hmr_strategy: crate::types::HmrStrategy::None,
        component_id: Some(bundler_component_id(carrier_id)),
        source_map: true,
        force_js: true,
        ..CompileProfile::default()
    }
}

/// The profile `VerterVue.rollup({})` builds: no resolved Vite config, so no
/// HMR strategy, no root to relativize against, and TS stripping owned by the
/// host because a non-Vite consumer has no `vite:esbuild` to hand it to.
fn bundler_vue_rollup_profile(carrier_id: &str) -> CompileProfile {
    CompileProfile {
        filename: Some(carrier_id.to_string()),
        is_production: false,
        custom_element: false,
        ssr: false,
        hmr_strategy: crate::types::HmrStrategy::None,
        component_id: Some(bundler_component_id(carrier_id)),
        source_map: true,
        force_js: true,
        ..CompileProfile::default()
    }
}

/// The same profile as [`bundler_vue_rollup_profile`], in the shape the
/// render-only batch lane requires — field for field, so both routes are asked
/// the identical typed question.
fn bundler_vue_rollup_render_profile(
    carrier_id: &str,
) -> crate::host_compile::CompileBatchRenderProfile {
    crate::host_compile::CompileBatchRenderProfile {
        style_processing: verter_compiler::compile_request::RuntimeStyleProcessing::Complete,
        filename: Some(carrier_id.to_string()),
        is_production: false,
        custom_element: false,
        ssr: false,
        force_js: true,
        force_vapor: false,
        source_map: true,
        comments: None,
        hmr_strategy: crate::types::HmrStrategy::None,
        runtime_module_name: None,
        types_module_name: None,
        delimiters: None,
        custom_elements: None,
        ssr_module_id: None,
    }
}

/// Assert a bundler-published product IS the host route's product.
///
/// Both halves are required. The bytes alone leave the map unchecked, and a
/// map PRESENCE flag is satisfied by any map at all — including one published
/// for a different compilation — so the map is compared as a whole artifact,
/// the same way the virtual-script parity comparisons above do it.
#[track_caller]
fn assert_published_product_matches_host(label: &str, case: &Value, expected: &HostOutcome) {
    let HostOutcome::Published {
        code, source_map, ..
    } = expected
    else {
        panic!("{label}: the host route publishes no product here, so there is nothing for the bundler route to match: {expected:?}");
    };
    assert_eq!(
        case["outcome"], "published",
        "{label}: the host route published a product but the bundler route returned {case}"
    );
    assert_eq!(
        case["code"].as_str(),
        Some(code.as_str()),
        "{label}: the bundler route's published bytes differ from the host route's"
    );
    match source_map {
        Some(_) => assert_eq!(
            normalized_source_map(label, &case["map"]),
            host_normalized_source_map(label, expected),
            "{label}: the bundler route published a different map than the host published for the \
             same requested profile"
        ),
        None => assert_no_source_map_artifact(label, &case["map"]),
    }
}

/// The virtual request this suite asks the load lane about, and the carrier it
/// asks the same question about without ever having transformed it.
const PROBE_VUE_TEMPLATE_REQUEST: &str = "/probe/Plug.vue?vue&type=template";
const PROBE_UNREGISTERED_TEMPLATE_REQUEST: &str = "/probe/NotRegistered.vue?vue&type=template";
/// The scoped, preprocessed style request the non-Vite lane is asked about.
const PROBE_VUE_SCOPED_STYLE_REQUEST: &str =
    "/probe/Plug.vue?vue&type=style&index=0&scoped&lang.scss";
/// The same bytes the probe hands that lane.
const NON_VITE_STYLE_SOURCE: &str = ".box { color: v-bind(primary); }\n";
/// The bytes the probe substitutes for the value the cross-file recompile block
/// reads back, in its substitution run and nowhere else.
///
/// Pinned on both sides: the probe appends exactly this to the value that one
/// call returns, and the assertion below is an EQUALITY against the host's own
/// product followed by exactly this — never a search for the marker inside
/// published output.
const RECOMPILE_RETURN_MARKER: &str = "\n/* verter-probe: recompile-return */\n";
/// The pre-compile fixture's sources, byte for byte.
const RECOMPILE_CHILD_VUE: &str =
    "<script setup>\ndefineProps({ msg: String })\n</script>\n\n<template><div>{{ msg }}</div></template>\n";
const RECOMPILE_PARENT_VUE: &str =
    "<script setup>\nimport Child from \"./Child.vue\"\n</script>\n\n<template><Child msg=\"hello\" /></template>\n";

/// The STYLE lane: the wrapper's style sub-requests, loaded, against the host's
/// own style nodes — and NONE published for a carrier with no `<style>`.
///
/// The two halves are one measurement. The count rests on the negative control:
/// the Vue carrier this suite drives carries no `<style>` block, so its wrapper
/// must publish zero style requests. Without that, "exactly one style request"
/// is satisfied by a plugin that emits one unconditionally, and the index and
/// language below would describe a request nobody asked for.
///
/// The count and index are also the only externally visible consequence of the
/// inventory listing the lane performs before it reads anything: the artifacts
/// are keyed by the style node indices the host reported, sorted.
#[test]
fn the_bundler_style_lane_publishes_the_hosts_style_products_and_none_without_a_style_block() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let vue = &record["cases"]["vuePublicEntry"];
    let svelte = &record["cases"]["sveltePublicEntry"];
    assert_bundler_case_carriers(vue, PROBE_VUE_ID, PROBE_SVELTE_ID);
    assert_bundler_case_carriers(svelte, PROBE_SVELTE_ID, PROBE_VUE_ID);

    // NEGATIVE CONTROL, stated first because the positive count rests on it.
    let vue_styles = vue["loadedStyles"].as_array().unwrap_or_else(|| {
        panic!("bundler/vue: the case records no loaded style list at all: {vue}")
    });
    assert!(
        vue_styles.is_empty(),
        "bundler/vue: the Vue carrier this suite drives has no `<style>` block, but its wrapper \
         published {} style request(s) — so a style-request count proves nothing about a carrier \
         that does have one: {:?}",
        vue_styles.len(),
        vue["styleRequests"]
    );

    let styles = svelte["loadedStyles"].as_array().unwrap_or_else(|| {
        panic!("bundler/svelte: the case records no loaded style list at all: {svelte}")
    });
    assert_eq!(
        styles.len(),
        1,
        "bundler/svelte: the Svelte carrier carries exactly one `<style>` block, so its wrapper \
         must publish exactly one style request: {:?}",
        svelte["styleRequests"]
    );
    let style = &styles[0];
    assert_eq!(
        style["index"],
        Value::from(0),
        "bundler/svelte: the published style request does not name the first authored style \
         element: {style}"
    );
    assert_eq!(
        style["lang"].as_str(),
        Some("css"),
        "bundler/svelte: the published style request names a different output language: {style}"
    );

    // ROUTE IDENTITY: the loaded artifact is the host's own `Style{0}` node for
    // the profile this entry builds.
    let host = host_with(
        PROBE_SVELTE_ID,
        SUPPORTED_SVELTE,
        verter_language::FileLanguage::svelte(),
    );
    let expected = host_node(
        &host,
        PROBE_SVELTE_ID,
        VirtualNodeKind::Style { index: 0 },
        &bundler_svelte_vite_profile(PROBE_SVELTE_ID),
    );
    assert_case_matches_host("bundler", "svelteStyleLane", style, &expected);
    assert_published_product_matches_host(
        "VerterSvelte.vite: the loaded style product",
        style,
        &expected,
    );

    // PUBLICATION: the product is a COMPILED style, not the authored bytes. The
    // parity comparison above would also hold if the host had started echoing
    // its input, so the scoped form is asserted directly.
    let published = style["code"].as_str().unwrap_or_default();
    assert!(
        published.contains(".root.svelte-"),
        "bundler/svelte: the loaded style product carries no scoped class, so it is not the \
         compiled style surface: {published:?}"
    );
    assert!(
        !published.contains(".root {"),
        "bundler/svelte: the loaded style product still carries the AUTHORED selector, so the \
         scoping pass did not reach it: {published:?}"
    );
}

/// The LOAD lane: a virtual request no transform cached, served from the host.
///
/// Every request the wrapper points at is answered out of a cache the carrier
/// transform filled. This one is not: a `?vue&type=template` request is never
/// cached, so answering it at all means the lane fell through to the host. The
/// negative control is the same request for a carrier this plugin never
/// transformed, which must publish nothing.
#[test]
fn the_bundler_load_lane_serves_the_hosts_node_and_nothing_for_an_unregistered_carrier() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let vue = &record["cases"]["vuePublicEntry"];
    assert_bundler_case_carriers(vue, PROBE_VUE_ID, PROBE_SVELTE_ID);

    let loaded = &vue["loadedTemplate"];
    // WHICH request this test is about is fixed here, never read off the record.
    assert_eq!(
        loaded["request"].as_str(),
        Some(PROBE_VUE_TEMPLATE_REQUEST),
        "the case this test reads as the template request drove something else: {loaded}"
    );

    // NEGATIVE CONTROL: same lane, same shape of request, carrier never
    // registered. A lane that answered this one would be answering from
    // somewhere other than the host's inventory.
    let unregistered = &vue["unregisteredTemplate"];
    assert_eq!(
        unregistered["request"].as_str(),
        Some(PROBE_UNREGISTERED_TEMPLATE_REQUEST),
        "the negative control drove something other than the unregistered carrier: {unregistered}"
    );
    assert_eq!(
        unregistered["outcome"], "missing",
        "the load lane published a product for a carrier it never transformed: {unregistered}"
    );
    assert_eq!(
        unregistered["code"],
        Value::Null,
        "the load lane published code for a carrier it never transformed: {unregistered}"
    );

    // ROUTE IDENTITY: the published product is the host's own `Template` node.
    let host = host_with(PROBE_VUE_ID, VUE_SFC, verter_language::FileLanguage::vue());
    let expected = host_node(
        &host,
        PROBE_VUE_ID,
        VirtualNodeKind::Template,
        &bundler_vue_vite_profile(PROBE_VUE_ID),
    );
    assert_published_product_matches_host(
        "VerterVue.vite: the loaded template product",
        loaded,
        &expected,
    );
    assert!(
        loaded["code"]
            .as_str()
            .is_some_and(|code| code.contains("function render(")),
        "VerterVue.vite: the loaded template product is not a render function: {loaded}"
    );
}

/// The RUNTIME-RENDER BATCH lane: the non-Vite inline product, against the
/// host's own render-only batch answer.
///
/// A non-Vite consumer has no script sub-request to point at, so the carrier
/// transform returns the rendered module INLINE. That module comes from
/// `compile_many` on the render-only lane, and this compares it against exactly
/// that call for the same canonical and an equivalent profile.
///
/// ## What the identity proves, and what it does not
///
/// The render-only lane is documented to produce the SAME `Main` bytes as the
/// host-backed lane through the same shared substrate, and this test MEASURES
/// that rather than assuming it. The consequence, stated plainly: byte identity
/// with the render lane does not by itself discriminate WHICH of the two host
/// lanes produced the bundler's bytes, because on this input both publish the
/// same bytes. What it does prove is that the bundler publishes the host's
/// runtime-render product rather than something of its own — and if the two
/// host lanes ever diverge, the measurement below fails and this test becomes
/// able to tell them apart.
#[test]
fn the_bundler_inline_transform_publishes_the_hosts_runtime_render_batch_product() {
    use crate::host_compile::{CompileBatchOptions, CompileManyTarget};

    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let vue = &record["cases"]["vueRollupEntry"];
    assert_eq!(
        vue["publicFactory"], "VerterVue.rollup",
        "the probe did not execute the public Vue Rollup entry: {vue}"
    );
    assert_eq!(
        vue["id"].as_str(),
        Some(PROBE_VUE_ID),
        "the case this test reads as the Vue carrier drove something else: {vue}"
    );
    assert_eq!(
        vue["transformInclude"], true,
        "VerterVue.rollup rejected its documented `.vue` carrier: {vue}"
    );
    assert_eq!(
        vue["publicTransformIsInline"], true,
        "VerterVue.rollup no longer returned the non-Vite inline product: {vue}"
    );
    let published = vue["publicTransformCode"].as_str().unwrap_or_else(|| {
        panic!("VerterVue.rollup: the case records no published inline product: {vue}")
    });
    assert!(
        !published.is_empty(),
        "VerterVue.rollup: the published inline product is empty, so the comparison below is \
         vacuous: {vue}"
    );

    let host = host_with(PROBE_VUE_ID, VUE_SFC, verter_language::FileLanguage::vue());
    let entries = host.compile_many(
        vec![crate::host_compile::CompileBatchInput {
            canonical_id: PROBE_VUE_ID.to_string(),
            source: Arc::from(VUE_SFC),
            requested_mode: None,
            component_id: Some(bundler_component_id(PROBE_VUE_ID)),
        }],
        CompileBatchOptions::default(),
        CompileManyTarget::RuntimeRender {
            profile: bundler_vue_rollup_render_profile(PROBE_VUE_ID),
        },
    );
    assert_eq!(
        entries.len(),
        1,
        "the render-only batch lane returned {} entries for one input",
        entries.len()
    );
    let entry = &entries[0];
    assert!(
        entry.errors().is_empty(),
        "the render-only batch lane failed for this input: {:?}",
        entry.errors()
    );
    assert_eq!(
        published,
        entry.code(),
        "VerterVue.rollup: the published inline product is not the host's render-only batch \
         product for the same canonical and profile"
    );

    // The measurement the paragraph above rests on, made rather than assumed.
    let host_backed = host_node(
        &host,
        PROBE_VUE_ID,
        VirtualNodeKind::Main,
        &bundler_vue_rollup_profile(PROBE_VUE_ID),
    );
    let HostOutcome::Published {
        code: main_code, ..
    } = &host_backed
    else {
        panic!("the host-backed `Main` route no longer publishes this module: {host_backed:?}");
    };
    assert_eq!(
        main_code.as_str(),
        entry.code(),
        "the render-only batch lane and the host-backed `Main` route no longer publish the same \
         bytes for this profile. That identity is exactly what stops the comparison above from \
         telling the two host lanes apart — if they have diverged, re-measure this test, which can \
         now discriminate between them."
    );
}

/// The NON-VITE CSS SCOPING lane, and the shared CSS processor beneath it.
///
/// Two things make this attributable rather than merely non-crashing. The
/// request carries `&scoped`, because an unscoped request is returned
/// byte-for-byte and would be indistinguishable from a lane that never ran; and
/// the CSS carries a `v-bind()` payload, whose rewrite names the component id
/// of the cached profile the lane read — so the product identifies WHICH
/// carrier's profile produced it, not merely that scoping happened.
///
/// The negative control is the same request for a carrier this plugin never
/// transformed: with no cached profile the lane returns its input untouched.
#[test]
fn the_non_vite_style_lane_scopes_through_the_shared_css_processor() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let case = &record["cases"]["vueRollupStyleScoping"];
    assert_eq!(
        case["outcome"], "transformed",
        "the non-Vite style lane did not run: {case}"
    );
    assert_eq!(
        case["publicFactory"], "VerterVue.rollup",
        "the probe did not drive the public Vue Rollup entry: {case}"
    );
    assert_eq!(
        case["id"].as_str(),
        Some(PROBE_VUE_ID),
        "the carrier whose profile this lane read is not the one this test asked about: {case}"
    );
    assert_eq!(
        case["styleId"].as_str(),
        Some(PROBE_VUE_SCOPED_STYLE_REQUEST),
        "the style request this lane answered is not the one this test asked about: {case}"
    );
    assert_eq!(
        case["styleSource"].as_str(),
        Some(NON_VITE_STYLE_SOURCE),
        "the bytes handed to this lane are not the bytes this test states its expectation against: \
         {case}"
    );
    // The include contract for this lane: a NON-`css` lang is what selects it
    // on a plugin with no resolved Vite config.
    assert_eq!(
        case["styleTransformInclude"], true,
        "the non-Vite entry refused the preprocessed scoped style request it owns: {case}"
    );

    let component_id = bundler_component_id(PROBE_VUE_ID);
    let scoped = case["scopedCode"]
        .as_str()
        .unwrap_or_else(|| panic!("the non-Vite style lane published no CSS at all: {case}"));
    assert!(
        scoped.contains(&format!("[data-v-{component_id}]")),
        "the non-Vite style lane published CSS with no `[data-v-{component_id}]` scoping \
         attribute, so it did not scope to the carrier whose profile it read: {scoped:?}"
    );
    assert!(
        scoped.contains(&format!("var(--{component_id}-primary)")),
        "the non-Vite style lane did not rewrite the `v-bind()` payload against \
         `{component_id}`, so the product cannot be attributed to this carrier's cached profile: \
         {scoped:?}"
    );
    assert_eq!(
        case["scopedMap"],
        Value::Null,
        "the non-Vite style lane published a source map where it publishes none: {case}"
    );

    // NEGATIVE CONTROL: no cached profile, no transform.
    assert_eq!(
        case["unregisteredCode"].as_str(),
        Some(NON_VITE_STYLE_SOURCE),
        "the non-Vite style lane transformed CSS for a carrier it never transformed, so a scoped \
         product proves nothing about which profile the lane consulted: {case}"
    );

    // ROUTE IDENTITY: the same shared CSS processor the free-function transport
    // spelling calls, on the same input and scope id.
    let parsed = verter_compiler::style_planner::parse_plain_css_for_verification(
        NON_VITE_STYLE_SOURCE,
        verter_compiler::style_planner::StyleRewriteStage::AuthoredVBind,
    )
    .expect("the shared CSS processor accepts this input");
    let verified =
        verter_compiler::style_planner::VerifiedPlainCss::from_parsed_native_css(&parsed)
            .expect("the verification parser produced native CSS syntax IR");
    let processed = verter_compiler::style_planner::transform_vue_style(
        verified,
        // The probe hands over its own authored bytes with no external tool in
        // front of them, so reported spans address exactly those bytes.
        verter_compiler::style_planner::CascadeInput::Authored,
        "style.css",
        "style.css",
        "style.css",
        &component_id,
        false,
        true,
        false,
    );
    assert!(processed.stage_failures.is_empty());
    assert_eq!(
        scoped,
        processed.code(),
        "the non-Vite style lane's product is not the shared CSS processor's product for the same \
         bytes and scope id"
    );
}

/// The PRE-COMPILE lane, driven over a real project on disk.
///
/// `buildStart` is its only entry, and it needs files: the probe writes a
/// parent passing a LITERAL prop to a child — the shape the cross-file pass
/// records constness for — configures a production build, and runs the hook.
/// What it published is read back through the plugin's own load hook and
/// compared against the host's `Main` node for the same profile.
///
/// ## What this proves, and what it cannot
///
/// It proves exactly two things: `buildStart` completed over a real two-file
/// project on disk, and both modules it published are byte-identical to the
/// in-process host's products for the same profile.
///
/// It makes NO claim about the cross-file recompile block — not that it was
/// entered, and not that it iterated. A recompiled module is byte-identical to
/// the pre-compiled one (the runtime compile path passes no constness
/// overrides), so these products cannot tell the two apart, and this test does
/// not pretend otherwise.
///
/// That block is attributed separately, and against the SHIPPED artifact, by
/// [`the_bundler_cross_file_recompile_write_is_attributed_to_the_recompile_call`].
#[test]
fn the_bundler_pre_compile_lane_publishes_the_hosts_products_for_a_real_project() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );
    let case = &record["cases"]["vueRecompileLane"];
    assert_eq!(
        case["outcome"], "buildStarted",
        "the pre-compile lane did not complete: {case}"
    );

    let (parent_id, child_id) = recompile_fixture_ids("the pre-compile lane", case);

    // ROUTE IDENTITY, per file: what `buildStart` published is the host's own
    // `Main` node for the production profile this lane builds. A `buildStart`
    // that returned early publishes nothing here at all.
    let host = recompile_host(&parent_id, &child_id);
    for (label, key, id) in [
        ("Parent", "parentScript", &parent_id),
        ("Child", "childScript", &child_id),
    ] {
        assert_published_product_matches_host(
            &format!("VerterVue.vite buildStart: the pre-compiled {label} module"),
            &case[key],
            &recompile_host_product(&host, id),
        );
    }
}

/// The pinned fixture identity of ONE recompile-lane case: its parent and child
/// canonical ids.
///
/// The fixture is pinned to this suite's own expectation — without it the
/// products compared against it are about whatever project the probe happened
/// to build.
///
/// The LEAF is allocated per invocation, so that suffix cannot be pinned:
/// concurrent probes would otherwise share one directory and delete each
/// other's files. What IS pinned is everything that makes the fixture this
/// suite's: the stable parent inside this repository, the leaf's prefix, and
/// that it is a single directory level (so the reported root cannot wander off
/// into some other project below it). The two sources are pinned outright,
/// which is what ties the products to the stated expectation.
#[track_caller]
fn recompile_fixture_ids(label: &str, case: &Value) -> (String, String) {
    let fixture_parent = repo_root_lexical().join(".verter-probe-fixtures");
    let fixture_parent = fixture_parent.to_string_lossy().replace('\\', "/");
    let fixture_root = case["fixtureRoot"]
        .as_str()
        .unwrap_or_else(|| panic!("{label} reported no fixture root: {case}"))
        .to_string();
    let leaf = fixture_root
        .strip_prefix(&format!("{fixture_parent}/"))
        .unwrap_or_else(|| {
            panic!(
                "{label} was driven outside this suite's fixture parent \
                 (`{fixture_parent}`): {case}"
            )
        });
    assert!(
        leaf.starts_with("recompile-") && !leaf.contains('/'),
        "{label}'s fixture root is not one per-invocation directory under this suite's fixture \
         parent: {case}"
    );
    let parent_id = format!("{fixture_root}/Parent.vue");
    let child_id = format!("{fixture_root}/Child.vue");
    assert_eq!(case["parentId"].as_str(), Some(parent_id.as_str()));
    assert_eq!(case["childId"].as_str(), Some(child_id.as_str()));
    assert_eq!(
        case["parentSource"].as_str(),
        Some(RECOMPILE_PARENT_VUE),
        "{label}'s parent is not the source this suite states its expectation against: {case}"
    );
    assert_eq!(
        case["childSource"].as_str(),
        Some(RECOMPILE_CHILD_VUE),
        "{label}'s child is not the source this suite states its expectation against: {case}"
    );
    (parent_id, child_id)
}

/// An in-process host carrying one recompile-lane fixture's two files.
fn recompile_host(parent_id: &str, child_id: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (id, source) in [
        (child_id, RECOMPILE_CHILD_VUE),
        (parent_id, RECOMPILE_PARENT_VUE),
    ] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(id.to_string()),
                input_id: id.to_string(),
                source: Arc::from(source),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap_or_else(|error| panic!("upsert {id}: {error:?}"));
    }
    host
}

/// The host's own `Main` product for one recompile-lane file, under the
/// production profile that lane builds.
#[track_caller]
fn recompile_host_product(host: &VerterHost, id: &str) -> HostOutcome {
    let (relative, source) = if id.ends_with("/Parent.vue") {
        ("Parent.vue", RECOMPILE_PARENT_VUE)
    } else {
        ("Child.vue", RECOMPILE_CHILD_VUE)
    };
    // A production profile hashes the ROOT-RELATIVE path plus the source.
    let profile = CompileProfile {
        filename: Some(id.to_string()),
        is_production: true,
        custom_element: false,
        ssr: false,
        hmr_strategy: crate::types::HmrStrategy::None,
        component_id: Some(bundler_component_id(&format!("{relative}{source}"))),
        source_map: true,
        force_js: false,
        ..CompileProfile::default()
    };
    host_node(host, id, VirtualNodeKind::Main, &profile)
}

/// The published bytes of a host product, for a comparison that is not a plain
/// equality against the whole outcome.
#[track_caller]
fn recompile_host_code(label: &str, outcome: &HostOutcome) -> String {
    match outcome {
        HostOutcome::Published { code, .. } => code.clone(),
        other => panic!("{label}: the host route published no product here: {other:?}"),
    }
}

/// The `rawId`s the probe observed `getVirtualFile` reached with, during
/// `buildStart` and nowhere else.
#[track_caller]
fn build_start_virtual_file_reads(label: &str, case: &Value) -> Vec<String> {
    case["buildStartVirtualFileCalls"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("{label}: the probe recorded no `buildStart` read observations at all: {case}")
        })
        .iter()
        .map(|call| {
            call["rawId"]
                .as_str()
                .unwrap_or_else(|| panic!("{label}: an observation carries no `rawId`: {case}"))
                .to_string()
        })
        .collect()
}

/// The cross-file RECOMPILE write inside `buildStart`, attributed.
///
/// `buildStart` reaches `getVirtualFile` at TWO places in the shipped plugin:
/// the cross-file recompile block, and the compiled-style read the SVELTE
/// pre-compile branch performs. This lane's fixture is Vue-only, so the Svelte
/// branch cannot fire — and the two are distinguishable anyway, because the
/// style read asks for a `?verter&type=style&index=…` request while the
/// recompile asks for a BARE canonical. The assertions below are an equality
/// against the bare child canonical, so a style read would fail them rather
/// than be mistaken for the recompile call, and reading 2 pins the observation
/// to the cross-file block specifically by turning the flag off on the same
/// fixture.
///
/// The observation is taken at the NATIVE MODULE BOUNDARY, on the same
/// `@verter/native` the plugin's own `createRequire` resolves, by a wrapper
/// that delegates and returns the real value; the shipped plugin is not
/// modified, and the wrapper is installed only around this lane group.
///
/// Three readings, and each needs the other two:
///
/// 1. THE CALL — the ordinary lane observes exactly one read, and it is the
///    CHILD, the file whose constness hints the cross-file pass changed.
/// 2. THE NEGATIVE CONTROL — the same drive with `crossFileOptimize` off
///    observes NONE, while still publishing both host products. Zero is
///    therefore an absent recompile, not an absent lane, and the observation
///    channel is not a constant.
/// 3. THE WRITE — a run in which the boundary substitutes a marked value for
///    what that one call returns publishes, for the child and only the child,
///    the host's product followed by exactly that marker. The value the
///    recompile call returned is therefore what the route cached and served,
///    which is the write itself.
///
/// This supersedes the earlier `session_metrics` closure condition: the metrics
/// channel is one way to count the call, but it is not the only one, and this
/// runs against the SHIPPED native artifact rather than a feature-enabled build.
#[test]
fn the_bundler_cross_file_recompile_write_is_attributed_to_the_recompile_call() {
    let record = probe(
        "bundler",
        "packages/unplugin/scripts/probe-bundler-route.mjs",
        BUNDLER_BUILD,
    );

    // The wrapper has to sit on the module the PLUGIN loads. The empirical half
    // of that is reading 3 below — a marker cannot reach a published product
    // through a copy the plugin never used — and this is the structural half.
    let expected_native = repo_root_lexical().join("packages/native/index.js");
    let expected_native = expected_native.to_string_lossy().replace('\\', "/");
    assert_eq!(
        record["nativeEntry"].as_str(),
        Some(expected_native.as_str()),
        "the probe observed a different `@verter/native` than this repository's: {}",
        record["nativeEntry"]
    );

    // 1. THE CALL.
    let driven = &record["cases"]["vueRecompileLane"];
    assert_eq!(
        driven["outcome"], "buildStarted",
        "the pre-compile lane did not complete: {driven}"
    );
    let (_parent_id, child_id) = recompile_fixture_ids("the pre-compile lane", driven);
    assert_eq!(
        build_start_virtual_file_reads("the pre-compile lane", driven),
        vec![child_id.clone()],
        "`buildStart` did not reach the cross-file recompile block for the child exactly once: \
         {driven}"
    );

    // 2. THE NEGATIVE CONTROL.
    let without = &record["cases"]["vueRecompileLaneWithoutCrossFile"];
    assert_eq!(
        without["outcome"], "buildStarted",
        "the cross-file-off control did not complete: {without}"
    );
    let (control_parent_id, control_child_id) =
        recompile_fixture_ids("the cross-file-off control", without);
    assert_eq!(
        build_start_virtual_file_reads("the cross-file-off control", without),
        Vec::<String>::new(),
        "the recompile block ran with `crossFileOptimize` off: {without}"
    );
    let control_host = recompile_host(&control_parent_id, &control_child_id);
    for (label, key, id) in [
        ("Parent", "parentScript", &control_parent_id),
        ("Child", "childScript", &control_child_id),
    ] {
        assert_published_product_matches_host(
            &format!(
                "VerterVue.vite buildStart without cross-file optimization: the pre-compiled \
                 {label} module"
            ),
            &without[key],
            &recompile_host_product(&control_host, id),
        );
    }

    // 3. THE WRITE.
    let substituted = &record["cases"]["vueRecompileWriteAttribution"];
    assert_eq!(
        substituted["outcome"], "buildStarted",
        "the substitution run did not complete: {substituted}"
    );
    assert_eq!(
        substituted["recompileReturnMarker"].as_str(),
        Some(RECOMPILE_RETURN_MARKER),
        "the substitution run did not substitute the bytes this test states its expectation \
         against: {substituted}"
    );
    let (marked_parent_id, marked_child_id) =
        recompile_fixture_ids("the substitution run", substituted);
    assert_eq!(
        build_start_virtual_file_reads("the substitution run", substituted),
        vec![marked_child_id.clone()],
        "the substitution run did not reach the recompile block for the child exactly once: \
         {substituted}"
    );
    let marked_host = recompile_host(&marked_parent_id, &marked_child_id);

    let child_label = "VerterVue.vite buildStart with a substituted recompile return: the child";
    let child_outcome = recompile_host_product(&marked_host, &marked_child_id);
    let child_code = recompile_host_code(child_label, &child_outcome);
    assert_eq!(
        substituted["childScript"]["outcome"], "published",
        "{child_label}: nothing was published: {substituted}"
    );
    assert_eq!(
        substituted["childScript"]["code"].as_str(),
        Some(format!("{child_code}{RECOMPILE_RETURN_MARKER}").as_str()),
        "{child_label}: what the route served is not the value the recompile call returned"
    );
    assert_eq!(
        normalized_source_map(child_label, &substituted["childScript"]["map"]),
        host_normalized_source_map(child_label, &child_outcome),
        "{child_label}: the substituted return carried a different map than the host's"
    );

    // The PARENT is the other half of the same reading: the recompile block
    // reached one file, so the other file's published bytes are still the
    // host's own, marker-free.
    assert_published_product_matches_host(
        "VerterVue.vite buildStart with a substituted recompile return: the parent",
        &substituted["parentScript"],
        &recompile_host_product(&marked_host, &marked_parent_id),
    );
}
