//! LIVE end-to-end proof of the SHARED editor-attach tsgo provider against the
//! REAL 7.0.2 engine, through the REAL relay-shim + control protocol.
//!
//! `[fake editor] -> [real verter-relay-shim] -> [real tsgo]`, with `verter_lsp`'s
//! `TsgoSharedProvider` as the SEPARATE control-client actor. A FAKE EDITOR spawns
//! the real shim as its `tsgo`, runs the LSP `initialize` handshake over the shim
//! stdio, and sends `initialized`. The provider then discovers the shim
//! advertisement, verifies the nonce, gates the in-band engine version, mints +
//! connects the `--api` session, and decides SHARED — after which Verter injects
//! the REAL Verter-IDE-codegen `.vue` carrier through the control protocol and
//! reads its `--api` semantic diagnostics.
//!
//! The macro-traversal proof (`shared_provider_serves_real_vue_macro_carrier`) is
//! the culminating case: a REAL `.vue` whose props come from an IMPORTED type,
//! compiled through the shared IDE codegen path (`CompileTarget::IDE`) — NOT a
//! handcrafted carrier — surfaces a deliberate TS2322 that maps back through the
//! production `ProviderPositionMapper` to the `.vue` script span (never a forged
//! `(0,0)`), and NO TS2307 (every carrier import resolves).
//!
//! Gating: NON-VACUOUS whenever tsgo is present. Under `VERTER_REQUIRE_TSGO` a
//! missing engine is a HARD failure (a skip would be a vacuous pass).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::position_map::PositionMapper;
use verter_lsp::documents::provider_projection::ProviderPositionMapper;
use verter_lsp::tsgo::composite::{SharedRendezvous, SharedTsgoOverlay, TsgoCompositeProvider};
use verter_lsp::tsgo::shared::{EstablishSharedParams, TsgoSharedProvider};
use verter_lsp::type_provider::lazy_managed::LazyManagedTypeProvider;
use verter_lsp::type_provider::merge::tsx_range_to_carrier_range;
use verter_lsp::type_provider::protocol::{
    CompletionResult, HoverInfo, InlayHint, ProviderDiagnosticContext, RenameLocation,
    SemanticToken, SignatureHelp, TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight,
    TypeLocation,
};
use verter_lsp::type_provider::traits::{ProviderFuture, TypeProvider};

use verter_session::external_ts::{
    CarrierOwnershipResolution, EnvDims, ExternalTsProjectResolver, WorkspaceProjectResolver,
};
use verter_session::file_artifact_store::ProjectIdentity;
use verter_session::{
    CompileProfile, CompileTarget, FileLanguage, HostConfig, PublicApiMode, UpsertRequest,
    VerterHost,
};

use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, WorkspaceAccess, WorkspaceRead};

use verter_tsgo_api::jsonrpc::{encode_message, MessageFramer};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Resolve the engine through the 4-tier toolchain resolver (version-checked),
/// honoring `VERTER_REQUIRE_TSGO` (a skip under that env is a vacuous-pass
/// failure).
fn engine_or_skip() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    match verter_tsgo_api::toolchain::discovery::find_version_checked(&request) {
        Ok(resolution) => Some(resolution.path),
        Err(e) => {
            if std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
                panic!("VERTER_REQUIRE_TSGO is set but tsgo was not found: {e}. A skip would be a vacuous pass.");
            }
            eprintln!("[skip] tsgo engine not found ({e}); set VERTER_REQUIRE_TSGO to require it");
            None
        }
    }
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn tempdir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "verter_shared_live_{tag}_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// The REAL `.vue` component: props from an IMPORTED type via `defineProps<T>()`,
/// plus a deliberate in-script mis-assignment (`props.label` is `string`, assigned
/// to `number` ⇒ TS2322). The imported prop type flows through the macro into the
/// carrier via the shared resolver; the error lands in a MAPPED script region.
const WIDGET_VUE: &str = "<script setup lang=\"ts\">\n\
import type { LabelProps } from './props'\n\
const props = defineProps<LabelProps>()\n\
const wrong: number = props.label\n\
</script>\n\
<template><div>{{ props.count }}</div></template>\n";

/// Compile a `.vue` through the REAL shared IDE codegen path (`CompileTarget::IDE`)
/// and its public API companion (`.verter.ts`). Returns `(ide_code,
/// source_map_json, companion_code)`. This is the SAME codegen the OWNED provider
/// uses — the carrier is NOT handcrafted. `@verter/types` / `vue` resolve through
/// the vendored hermetic node_modules stubs in the fixture (a real installed
/// project provides the real packages).
fn compile_vue_ide(vue_source: &str) -> (String, String, String) {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    // The imported prop type — an ordinary workspace module the macro traverses.
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/src/props.ts".to_string(),
        source: Arc::from("export interface LabelProps { label: string; count: number }\n"),
        file_language: FileLanguage::script_ts(),
        aliases: Vec::new(),
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/src/Widget.vue".to_string(),
        source: Arc::from(vue_source),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
    let profile = CompileProfile {
        source_map: true,
        target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
        ..CompileProfile::default()
    };
    let _ = host.ensure_ide_compiled("/src/Widget.vue", &profile);
    let ide = host
        .get_ide("/src/Widget.vue", &profile)
        .expect("real IDE codegen for the .vue carrier");
    let source_map = ide
        .source_map
        .as_ref()
        .map(|m| m.to_string())
        .expect("the IDE carrier carries a source map");
    let companion = host
        .get_public_api_with_mode("/src/Widget.vue", PublicApiMode::Public, None)
        .expect("the .verter.ts public API companion")
        .code
        .to_string();
    (ide.code.to_string(), source_map, companion)
}

/// A minimal `vue` module stub so the carrier's `import("vue").*` references
/// RESOLVE (missing members would give TS2694, never the TS2307 the proof forbids).
const VUE_STUB: &str = "export type PublicProps = {};\n\
export type HTMLAttributes = Record<string, unknown>;\n\
export type ShallowUnwrapRef<T> = T;\n\
export type NativeElements = Record<string, unknown>;\n\
export type GlobalDirectives = Record<string, unknown>;\n\
export type Directive<T = any, V = any, M extends string = string> = unknown;\n\
export declare const Comment: unique symbol;\n\
export declare const Fragment: unique symbol;\n\
export type Ref<T> = { value: T };\n\
export type ExtractPropTypes<T> = T;\n";

/// A minimal `@verter/types` module stub — the IDE-carrier helper surface, declared
/// loosely so the `import from \"@verter/types\"` resolves cleanly.
const VERTER_TYPES_STUB: &str = "export type Prettify<T> = { [K in keyof T]: T[K] };\n\
export type ExtractComponentProps<T> = Record<string, unknown>;\n\
export type ExtractLeafElement<T> = unknown;\n\
export declare function shallowUnwrapRef<T>(value: T): T;\n\
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T;\n\
export declare function extractRenderComponent<T>(t: T): unknown;\n\
export declare function instantiateComponent<T>(t: T): unknown;\n\
export declare function extractArgumentsFromRenderSlot(...args: unknown[]): unknown;\n\
export declare function runCustomDirective(...args: unknown[]): unknown;\n\
export declare function retrieveSetupDirectives<T>(instance: T): unknown;\n\
export declare function strictRenderSlot(...args: unknown[]): unknown;\n\
export declare function checkRequiredSlots(...args: unknown[]): unknown;\n";

/// Write the hermetic configured-project fixture. Returns `(tsconfig_path, src_dir)`.
/// The carrier companions are OFF-DISK overlays injected through the provider; the
/// `include: ["src/**/*"]` glob makes them Program roots.
fn write_fixture(dir: &Path) -> (PathBuf, PathBuf) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    // The imported prop type (on disk — the carrier resolves `./props`).
    std::fs::write(
        src.join("props.ts"),
        "export interface LabelProps { label: string; count: number }\n",
    )
    .unwrap();
    // The Vue macro globals (ambient script `.d.ts`), so `defineProps<T>(): T` is
    // typed — the deliberate TS2322 depends on `props.label` being `string`.
    std::fs::write(
        src.join("globals.d.ts"),
        "declare function defineProps<T>(): T;\n\
         declare function defineEmits<T>(): T;\n\
         declare function defineExpose(exposed?: unknown): void;\n\
         declare function defineOptions(options?: unknown): void;\n\
         declare function defineSlots<T>(): T;\n\
         declare function defineModel<T>(...args: unknown[]): { value: T };\n\
         declare function withDefaults<P, D>(props: P, defaults: D): P;\n",
    )
    .unwrap();
    // A minimal `vue` package so the carrier's `import(\"vue\")` resolves.
    let vue_dir = dir.join("node_modules").join("vue");
    std::fs::create_dir_all(&vue_dir).unwrap();
    std::fs::write(
        vue_dir.join("package.json"),
        "{\n  \"name\": \"vue\",\n  \"version\": \"3.5.0\",\n  \"types\": \"index.d.ts\"\n}\n",
    )
    .unwrap();
    std::fs::write(vue_dir.join("index.d.ts"), VUE_STUB).unwrap();
    // A minimal `@verter/types` package so the carrier's `import from
    // "@verter/types"` RESOLVES (an inline `declare module` inside the module
    // carrier would be a no-op augmentation; a real installed project ships the
    // real package). The referenced helpers are declared loosely (any-typed) so the
    // import is clean — missing members would be TS2305, never the TS2307 forbidden.
    let vt_dir = dir.join("node_modules").join("@verter").join("types");
    std::fs::create_dir_all(&vt_dir).unwrap();
    std::fs::write(
        vt_dir.join("package.json"),
        "{\n  \"name\": \"@verter/types\",\n  \"version\": \"0.0.0\",\n  \"types\": \"index.d.ts\"\n}\n",
    )
    .unwrap();
    std::fs::write(vt_dir.join("index.d.ts"), VERTER_TYPES_STUB).unwrap();

    let tsconfig = dir.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "preserve",
    "noEmit": true,
    "skipLibCheck": true,
    "allowArbitraryExtensions": true,
    "allowImportingTsExtensions": true
  },
  "include": ["src/**/*"]
}
"#,
    )
    .unwrap();
    (tsconfig, src)
}

/// A fake editor over the shim stdio: writes LSP frames, records EVERY frame the
/// shim writes back (so a leak test can inspect the whole editor-visible stream),
/// and auto-answers server→client requests with `null` (as a real editor does).
struct FakeEditor {
    out_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    frames: Arc<StdMutex<Vec<serde_json::Value>>>,
}

impl FakeEditor {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(bytes) = out_rx.recv().await {
                if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
        });
        let frames = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&frames);
        let answer_tx = out_tx.clone();
        tokio::spawn(async move {
            let mut out = stdout;
            let mut framer = MessageFramer::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = match out.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                framer.push(&chunk[..n]);
                while let Ok(Some(msg)) = framer.next_message() {
                    let has_id = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
                    if has_id && msg.get("method").is_some() {
                        let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        let reply =
                            serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null });
                        let _ = answer_tx.send(encode_message(&reply));
                    }
                    sink.lock().unwrap().push(msg);
                }
            }
        });
        Self { out_tx, frames }
    }

    async fn send(&self, msg: &serde_json::Value) {
        let _ = self.out_tx.send(encode_message(msg));
    }

    fn all_frames(&self) -> Vec<serde_json::Value> {
        self.frames.lock().unwrap().clone()
    }

    async fn wait_for(
        &self,
        pred: impl Fn(&serde_json::Value) -> bool,
        timeout: Duration,
    ) -> Option<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(found) = self
                .frames
                .lock()
                .unwrap()
                .iter()
                .find(|m| pred(m))
                .cloned()
            {
                return Some(found);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

/// Locate the `verter-relay-shim` binary in the workspace target profile dir.
/// `CARGO_BIN_EXE_*` is only exported to the DEFINING crate's tests, and the shim
/// lives in a sibling binary crate, so it is located next to this test executable
/// (`target/<profile>/deps/<test>` → `target/<profile>/verter-relay-shim`).
fn shim_binary_path() -> PathBuf {
    let mut dir = std::env::current_exe().expect("current test exe");
    dir.pop(); // deps/
    dir.pop(); // <profile>/
    let name = if cfg!(windows) {
        "verter-relay-shim.exe"
    } else {
        "verter-relay-shim"
    };
    dir.join(name)
}

/// Ensure the shim binary is current (self-contained: `cargo test -p verter_lsp`
/// does not build a sibling crate's bin). An existing path is not sufficient: Cargo
/// can leave a stale sibling binary after the control protocol changes. Build once per
/// test process; Cargo's incremental check makes the up-to-date case cheap.
fn ensure_shim_built() -> PathBuf {
    static CURRENT_SHIM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    CURRENT_SHIM
        .get_or_init(|| {
            let profile = if cfg!(debug_assertions) {
                "--profile=dev"
            } else {
                "--release"
            };
            let status = std::process::Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-p",
                    "verter_relay_shim",
                    "--bin",
                    "verter-relay-shim",
                ])
                .arg(profile)
                .status()
                .expect("spawn cargo build for the relay shim");
            assert!(status.success(), "cargo build of verter-relay-shim failed");
            let path = shim_binary_path();
            assert!(
                path.is_file(),
                "the relay shim binary is missing after build: {path:?}"
            );
            path
        })
        .clone()
}

fn spawn_shim(tsgo: &Path, control_dir: &Path, session_key: &str) -> Child {
    Command::new(ensure_shim_built())
        .arg("--real-tsgo")
        .arg(tsgo)
        .arg("--control-dir")
        .arg(control_dir)
        .arg("--session-key")
        .arg(session_key)
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn the relay shim binary")
}

fn init_params(root_uri: &str) -> serde_json::Value {
    serde_json::json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {},
        "workspaceFolders": [{ "uri": root_uri, "name": "verter" }],
    })
}

/// Resolve the carrier SOURCE's owning project through the PRODUCTION
/// `WorkspaceProjectResolver` over a real published fixture snapshot (the same
/// `include: ["src/**/*"]` configured project the composite test publishes) — the
/// production resolution path, NOT the `new_for_test` seam. The three live tests thus
/// exercise the real resolver, exactly as the production composite does.
fn resolved_binding(workspace_root: &str, tsconfig: &str) -> CarrierOwnershipResolution {
    let snapshot = fixture_snapshot(workspace_root, tsconfig);
    let vfs = MemoryWorkspace::new(MemoryOptions {
        roots: vec![workspace_root.to_string()],
        default_resolve_extensions: None,
    });
    let env_dims_source = |_tsconfig_uri: &str| EnvDims {
        parse_env_hash: [11u8; 16],
        resolve_env_hash: [22u8; 16],
        lib_env_hash: [33u8; 16],
        project_identity: ProjectIdentity([7u8; 16]),
    };
    let resolver = WorkspaceProjectResolver::new(
        &snapshot,
        &vfs as &dyn WorkspaceRead,
        "7.0.2",
        &env_dims_source,
        true,
    );
    resolver.resolve(&format!("{workspace_root}/src/Widget.vue"), None)
}

/// A wired SHARED session: the shim + fake editor + the established provider.
struct Harness {
    shim: Child,
    editor: FakeEditor,
    provider: TsgoSharedProvider,
    dir: PathBuf,
    src: PathBuf,
}

/// Drive the full chain up to an established SHARED provider: spawn the shim, run
/// the editor LSP handshake over its stdio, then establish the provider (which
/// discovers the advertisement + connects the control endpoint + `--api` session
/// itself) with a resolved binding.
async fn setup(tsgo: &Path, tag: &str) -> Harness {
    let dir = tempdir(tag);
    let (tsconfig, src) = write_fixture(&dir);
    let tsconfig_norm = norm(&tsconfig);
    let workspace_norm = norm(&dir);
    let root_uri = format!("file:///{}", workspace_norm.trim_start_matches('/'));
    let control_dir = dir.join("ctl");
    let session_key = tag.to_string();

    let mut shim = spawn_shim(tsgo, &control_dir, &session_key);
    let editor_stdin = shim.stdin.take().expect("shim stdin piped");
    let editor_stdout = shim.stdout.take().expect("shim stdout piped");
    let editor = FakeEditor::new(editor_stdin, editor_stdout);

    editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params(&root_uri),
        }))
        .await;
    let init_resp = editor
        .wait_for(|m| m["id"] == 1, Duration::from_secs(40))
        .await
        .expect("the relayed initialize response");
    assert_eq!(
        init_resp["result"]["serverInfo"]["version"].as_str(),
        Some("7.0.2"),
        "the fake editor observes the REAL relayed tsgo version"
    );
    editor
        .send(&serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
        .await;

    // Establish the SHARED provider — discovery + nonce + waitInitialized + gate +
    // `--api` session + the SHARED decision (resolved binding evidence).
    let provider = tokio::time::timeout(
        Duration::from_secs(45),
        TsgoSharedProvider::establish_shared(EstablishSharedParams {
            control_dir: &control_dir,
            session_key: &session_key,
            workspace_root: &workspace_norm,
            tsconfig_path: &tsconfig_norm,
            resolution: resolved_binding(&workspace_norm, &tsconfig_norm),
            // The published fixture snapshot's generation (`SnapshotGeneration(1)`).
            config_generation: 1,
            client_label: "verter_lsp",
        }),
    )
    .await
    .expect("establish_shared timed out")
    .expect("SHARED attach must be established (all-positive live evidence)");

    assert_eq!(
        provider.serve_mode(),
        verter_session::external_ts::ServeMode::Shared,
        "an all-positive live attach must decide SHARED"
    );

    Harness {
        shim,
        editor,
        provider,
        dir,
        src,
    }
}

async fn teardown(mut h: Harness) {
    let _ = h.provider.shutdown().await;
    let _ = h.shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), h.shim.wait()).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// MACRO-TRAVERSAL — the culminating live proof. A REAL `.vue` whose props come
/// from an IMPORTED type, compiled through the shared IDE codegen path and injected
/// through the control protocol, surfaces TS2322 for the wrong prop usage; NO
/// TS2307; and the diagnostic maps back through the production
/// `ProviderPositionMapper` to the `.vue` script span (never a forged `(0,0)`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_provider_serves_real_vue_macro_carrier() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let h = setup(&tsgo, "macro").await;

    // REAL IDE codegen (NOT handcrafted): the imported-prop type flows through the
    // macro into the carrier via the shared resolver.
    let (ide_code, source_map, companion) = compile_vue_ide(WIDGET_VUE);
    assert!(
        ide_code.contains("defineProps<") && ide_code.contains("Widget.vue.verter.ts"),
        "the injected carrier is the real IDE codegen (defineProps + companion re-export)"
    );

    let carrier_tsx = norm(&h.src.join("Widget.vue.tsx"));
    let companion_ts = norm(&h.src.join("Widget.vue.verter.ts"));

    // Inject BOTH the carrier and its `.verter.ts` companion through the control
    // protocol (the shim's gated injection channel), NOT an OWNED `--lsp` didOpen.
    h.provider
        .open_file(&companion_ts, &companion)
        .await
        .expect("inject the .verter.ts companion");
    h.provider
        .open_file(&carrier_tsx, &ide_code)
        .await
        .expect("inject the real IDE carrier");

    let diags = tokio::time::timeout(
        Duration::from_secs(45),
        h.provider.semantic_diagnostics_for_carrier(&carrier_tsx),
    )
    .await
    .expect("shared semantic diagnostics timed out")
    .expect("shared semantic diagnostics");

    let codes: Vec<_> = diags.iter().filter_map(|d| d.code.clone()).collect();
    eprintln!("[macro] shared --api carrier diagnostics codes = {codes:?}");

    // THE PROOF: the deliberate TS2322 surfaces — the imported prop type flowed
    // through the REAL carrier, through the shared resolver, into the real tsgo.
    let ts2322 = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("2322"))
        .unwrap_or_else(|| {
            panic!("the imported-prop macro type must produce TS2322 through the shared carrier; got {codes:?}")
        });
    // NEGATIVE: every carrier import resolved — no spurious TS2307.
    assert!(
        !diags.iter().any(|d| d.code.as_deref() == Some("2307")),
        "every carrier import (`./props`, `@verter/types`, `vue`, the companion) must resolve — \
         no TS2307; got {codes:?}"
    );

    // Map the carrier-origin TS2322 back through the PRODUCTION
    // `ProviderPositionMapper` to the `.vue` source span — the SAME two-step the
    // OWNED path uses. Assert a REAL mapped range (never a forged `(0,0)`).
    let mapper = ProviderPositionMapper::source_map(
        PositionMapper::from_json(&source_map).expect("parse the IDE source map"),
    );
    let tsx_line_index = LineIndex::new_utf16(&ide_code);
    let vue_line_index = LineIndex::new_utf16(WIDGET_VUE);
    let mapped = tsx_range_to_carrier_range(
        ts2322.start,
        ts2322.end,
        &tsx_line_index,
        &mapper,
        &vue_line_index,
    )
    .expect("the carrier-origin TS2322 must map back to the .vue source (never forged)");
    eprintln!(
        "[macro] TS2322 carrier bytes {}..{} -> .vue {:?}",
        ts2322.start, ts2322.end, mapped
    );
    assert!(
        !(mapped.start.line == 0
            && mapped.start.character == 0
            && mapped.end.line == 0
            && mapped.end.character == 0),
        "the mapped span must be a real .vue location, never a forged (0,0)"
    );
    // The mapped `.vue` line must be the deliberate error line (`props.label`) —
    // discriminating: a mis-mapping would land elsewhere.
    let vue_lines: Vec<&str> = WIDGET_VUE.lines().collect();
    let mapped_line = vue_lines
        .get(mapped.start.line as usize)
        .copied()
        .unwrap_or("");
    assert!(
        mapped_line.contains("props.label") || mapped_line.contains("wrong"),
        "the TS2322 must map back to the deliberate error line in the .vue script; mapped to \
         line {} = {mapped_line:?}",
        mapped.start.line
    );

    teardown(h).await;
}

/// CARRIER-LEAK-LIVE — at the provider level, no carrier URI/text reaches the fake
/// editor. The provider injects the carrier over the control channel (the shim
/// taints + suppresses egress); an editor-side request whose server response would
/// reference the carrier is driven, and the editor stream is asserted carrier-free.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_provider_carrier_never_leaks_to_editor() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let h = setup(&tsgo, "leak").await;
    let (ide_code, _map, companion) = compile_vue_ide(WIDGET_VUE);
    let carrier_tsx = norm(&h.src.join("Widget.vue.tsx"));
    let companion_ts = norm(&h.src.join("Widget.vue.verter.ts"));
    let carrier_uri = format!("file:///{}", carrier_tsx.trim_start_matches('/'));

    h.provider.open_file(&companion_ts, &companion).await.ok();
    h.provider.open_file(&carrier_tsx, &ide_code).await.ok();
    // Process the carrier so the real tsgo genuinely holds it.
    let _ = h
        .provider
        .semantic_diagnostics_for_carrier(&carrier_tsx)
        .await;

    // Drive an editor request whose response could reference the carrier.
    h.editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 100, "method": "workspace/symbol",
            "params": { "query": "Widget" },
        }))
        .await;
    let _ = h
        .editor
        .wait_for(|m| m["id"] == 100, Duration::from_secs(15))
        .await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    for frame in h.editor.all_frames() {
        let text = frame.to_string();
        assert!(
            !text.contains(&carrier_uri)
                && !text.contains("Widget.vue.tsx")
                && !text.contains("Widget.vue.verter.ts")
                && !text.contains("___VERTER___"),
            "a Verter carrier leaked to the fake editor: {frame}"
        );
        if let Some(id) = frame.get("id").and_then(|v| v.as_str()) {
            assert!(
                !id.starts_with("verter:"),
                "a reserved verter:* id leaked to the editor: {frame}"
            );
        }
    }
    // Forwarding still works: the editor received the relayed initialize response.
    assert!(
        h.editor.all_frames().iter().any(|m| m["id"] == 1),
        "the relay must still forward non-carrier server frames (the initialize response)"
    );

    teardown(h).await;
}

/// SPLIT-BRAIN (live) — a reconnect (a fresh editor session generation) mints a
/// FRESH engine session and does NOT reuse the prior `--api` handle: two live
/// establishments over distinct generations each decide SHARED with a DISTINCT
/// engine identity (the mode-keyed generation dimension differs), and each
/// reference closure is served by exactly ONE mode. A stale warm entry from the
/// first generation is unreachable under the second.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_provider_reconnect_mints_fresh_engine_no_split_brain() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    // First live session (first editor generation).
    let h1 = setup(&tsgo, "reconnectA").await;
    let engine1 = h1
        .provider
        .decision()
        .decision()
        .engine()
        .editor_session_generation;
    assert_eq!(
        h1.provider.serve_mode(),
        verter_session::external_ts::ServeMode::Shared
    );
    assert_eq!(
        h1.provider.warm_cache().lock().len(),
        1,
        "the first SHARED establishment warmed exactly one entry"
    );

    // A reconnect = a SECOND shim/editor session (a fresh generation). The two are
    // independent live sessions; the shim mints a fresh editor-session generation
    // per spawn, so the SHARED decision's EngineIdentity differs.
    let h2 = setup(&tsgo, "reconnectB").await;
    let engine2 = h2
        .provider
        .decision()
        .decision()
        .engine()
        .editor_session_generation;
    assert_eq!(
        h2.provider.serve_mode(),
        verter_session::external_ts::ServeMode::Shared
    );

    assert_ne!(
        engine1, engine2,
        "a reconnect must mint a FRESH engine generation — never reuse the prior --api session"
    );
    assert_ne!(
        h1.provider.decision().decision().engine(),
        h2.provider.decision().decision().engine(),
        "the reconnect EngineIdentity must differ (no stale handle reuse across reconnect)"
    );
    // One closure, one mode: each provider's decision covers its own single-project
    // component served by exactly ONE mode (SHARED) — never split across engines.
    assert_eq!(h1.provider.decision().decision().members().len(), 1);
    assert_eq!(h2.provider.decision().decision().members().len(), 1);

    // The first session's warm entry is keyed on its own generation, so the second
    // session's cache (a fresh EngineWarmCache under a fresh generation) never holds
    // the stale entry — the stale --api handle is unreachable.
    assert_eq!(
        h2.provider.warm_cache().lock().len(),
        1,
        "the reconnect warms a fresh entry, never reusing the prior generation's"
    );

    teardown(h1).await;
    teardown(h2).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// COMPOSITE — the production `TsgoCompositeProvider` engages SHARED PER QUERY by
// resolving the queried carrier's owning project through the shared
// `WorkspaceProjectResolver` over the host's LIVE published snapshot (never the
// `new_for_test` seam, never a startup throwaway workspace). A resolved
// `ProjectBinding` mints the `BoundProject` witness and the SHARED `--api` carrier
// diagnostics are overlaid; every other state falls back to OWNED.
// ─────────────────────────────────────────────────────────────────────────────

/// A minimal managed-fallback double. Its answers are empty, so any successful
/// shared result is discriminating; in the topology test it is wrapped by
/// [`LazyManagedTypeProvider`] and must never be constructed at all.
#[derive(Default)]
struct OwnedBaselineDouble;

impl TypeProvider for OwnedBaselineDouble {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }
    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        Box::pin(async move {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: false,
            })
        })
    }
    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async move { Ok(None) })
    }
    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async move { Ok(None) })
    }
    fn get_code_actions(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_inlay_hints(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}

/// Build the real `WorkspaceSnapshot` with ONE configured project whose
/// `include: ["src/**/*"]` OWNS the carrier's directory (so the on-disk
/// `src/Widget.vue` SOURCE resolves to a `ProjectBinding`), driving the SAME
/// production membership parse/expansion chain the resolver's own tests use,
/// hermetically over an in-memory workspace mirroring the on-disk fixture paths.
fn fixture_snapshot(ws_root: &str, tsconfig: &str) -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        tsconfig.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    ws.inject_file(
        format!("{ws_root}/src/Widget.vue"),
        Arc::<str>::from("<template></template>"),
    );

    let root = CanonicalPath::new(ws_root);
    let raw_membership = load_project_membership(&ws, tsconfig);
    let compiler_options = load_compiler_options(&ws, tsconfig);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, tsconfig)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(ws_root),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1))
}

/// A TWO-project fixture: an `app` configured project that DECLARES a redirect-ON
/// project reference to a sibling `lib` project (both under one workspace root). The
/// `app` project OWNS the carrier via `include: ["src/**/*"]`, so `app/src/Widget.vue`
/// resolves to a `ProjectBinding` whose `references()` carries the `lib` project — the
/// closure edge threaded into the live decision so the two projects are
/// decided as ONE reference-connected component. Built through the SAME production
/// membership/reference loaders `fixture_snapshot` uses.
fn two_project_fixture_snapshot(ws_root: &str) -> WorkspaceSnapshot {
    let app_tsconfig = format!("{ws_root}/app/tsconfig.json");
    let lib_tsconfig = format!("{ws_root}/lib/tsconfig.json");
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        app_tsconfig.clone(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"], "references": [{ "path": "../lib" }] }"#),
    );
    ws.inject_file(
        lib_tsconfig.clone(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    ws.inject_file(
        format!("{ws_root}/app/src/Widget.vue"),
        Arc::<str>::from("<template></template>"),
    );

    let configured = |tsconfig: &str, id: u32| {
        let root = CanonicalPath::new(tsconfig.trim_end_matches("/tsconfig.json"));
        let raw_membership = load_project_membership(&ws, tsconfig);
        let compiler_options = load_compiler_options(&ws, tsconfig);
        let supported = supported_extensions_for(&compiler_options);
        let spec = membership_to_spec(&root, &raw_membership, &supported);
        let references = load_project_references(&ws, tsconfig)
            .into_iter()
            .map(|r| CanonicalPath::new(&r))
            .collect();
        OwnershipProject {
            id: ProjectId(id),
            root: root.clone(),
            workspace_root: CanonicalPath::new(ws_root),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig),
                membership: ConfiguredMembership {
                    spec,
                    materialized_files: Default::default(),
                },
                compiler_options,
                references,
                workspace_aliases: Vec::new(),
            },
        }
    };

    build_workspace_snapshot_simple(
        vec![configured(&app_tsconfig, 0), configured(&lib_tsconfig, 1)],
        SnapshotGeneration(1),
    )
}

/// REFERENCE-CLOSURE — the production `WorkspaceProjectResolver` resolves a
/// carrier in a project that DECLARES a redirect-ON project reference to a
/// `ProjectBinding` whose `references()` carries that referenced project. This is the
/// closure edge threaded into the live decision so the reference-connected component is
/// decided as ONE unit (the `shared.rs` unit test
/// `redirect_reference_to_absent_member_fails_closure_closed_to_owned` proves the
/// resulting fail-closed-to-OWNED behaviour). Pure over the production resolver — no
/// engine — so it runs unconditionally.
#[test]
fn two_project_reference_fixture_resolves_as_one_closure() {
    let ws_root = "/verter-closure-fixture";
    let snapshot = two_project_fixture_snapshot(ws_root);
    let vfs = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    let env_dims_source = |_tsconfig_uri: &str| EnvDims {
        parse_env_hash: [11u8; 16],
        resolve_env_hash: [22u8; 16],
        lib_env_hash: [33u8; 16],
        project_identity: ProjectIdentity([7u8; 16]),
    };
    let resolver = WorkspaceProjectResolver::new(
        &snapshot,
        &vfs as &dyn WorkspaceRead,
        "7.0.2",
        &env_dims_source,
        true,
    );
    let resolution = resolver.resolve(&format!("{ws_root}/app/src/Widget.vue"), None);
    let CarrierOwnershipResolution::Bound(binding) = resolution else {
        panic!("the app carrier must resolve to a ProjectBinding; got {resolution:?}");
    };

    // The closure edge is present on the resolved binding: the app project's redirect-ON
    // reference to `lib` is carried, so the live decision threads it and decides the two
    // projects as ONE reference-connected component (never per single tsconfig).
    let refs: Vec<String> = binding
        .references()
        .iter()
        .map(|r| r.replace('\\', "/"))
        .collect();
    assert!(
        !refs.is_empty(),
        "the app binding must carry its redirect-ON reference to `lib` (the closure edge); \
         got {refs:?}"
    );
    assert!(
        refs.iter().any(|r| r.contains("/lib")),
        "the app binding's reference must resolve to the sibling `lib` project; got {refs:?}"
    );
}

/// A wired composite session: the shim + fake editor + a host carrying the LIVE
/// published fixture snapshot + the production `TsgoCompositeProvider`.
struct CompositeHarness {
    shim: Child,
    /// Held to keep the shim's stdio (the fake-editor relay) alive for the test's
    /// duration — dropping it EOFs the shim's stdin.
    #[allow(dead_code)]
    editor: FakeEditor,
    composite: TsgoCompositeProvider,
    dir: PathBuf,
    src: PathBuf,
}

/// Drive the full chain to a production composite whose SHARED overlay binds
/// lazily per query: spawn the shim, run the editor LSP handshake, publish the real
/// fixture snapshot onto the host workspace, then build the composite (OWNED double
/// + a SHARED overlay over that host — NO fabricated binding, NO `new_for_test`).
async fn setup_composite(tsgo: &Path, tag: &str, owned: Arc<dyn TypeProvider>) -> CompositeHarness {
    let dir = tempdir(tag);
    let (tsconfig, src) = write_fixture(&dir);
    let tsconfig_norm = norm(&tsconfig);
    let workspace_norm = norm(&dir);
    let root_uri = format!("file:///{}", workspace_norm.trim_start_matches('/'));
    let control_dir = dir.join("ctl");
    let session_key = tag.to_string();

    let mut shim = spawn_shim(tsgo, &control_dir, &session_key);
    let editor_stdin = shim.stdin.take().expect("shim stdin piped");
    let editor_stdout = shim.stdout.take().expect("shim stdout piped");
    let editor = FakeEditor::new(editor_stdin, editor_stdout);

    editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params(&root_uri),
        }))
        .await;
    let init_resp = editor
        .wait_for(|m| m["id"] == 1, Duration::from_secs(40))
        .await
        .expect("the relayed initialize response");
    assert_eq!(
        init_resp["result"]["serverInfo"]["version"].as_str(),
        Some("7.0.2"),
        "the fake editor observes the REAL relayed tsgo version"
    );
    editor
        .send(&serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
        .await;

    // The host carrying the LIVE published snapshot the composite resolves the
    // carrier's owning project over (the SAME published-root the production LSP
    // publishes through `background_init`).
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(fixture_snapshot(
        &workspace_norm,
        &tsconfig_norm,
    ))));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);

    let overlay = SharedTsgoOverlay::new(
        Arc::clone(&host),
        SharedRendezvous {
            control_dir,
            session_key,
            workspace_root: workspace_norm,
        },
    );
    // The always-present host-aware admission layer wraps OWNED + the host, with the
    // SHARED overlay OPTIONAL (present here under the live rendezvous).
    let composite = TsgoCompositeProvider::new(owned, host, Some(overlay));

    CompositeHarness {
        shim,
        editor,
        composite,
        dir,
        src,
    }
}

async fn teardown_composite(mut h: CompositeHarness) {
    let _ = h.composite.shutdown().await;
    let _ = h.shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), h.shim.wait()).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// COMPOSITE PER-QUERY ENGAGEMENT — the production `TsgoCompositeProvider` overlays
/// the SHARED `--api` carrier TS2322 for a `.vue` whose props come from an IMPORTED
/// type, PROVING SHARED bound per query through the real `WorkspaceProjectResolver`
/// over the host's published snapshot (never the `new_for_test` seam). The
/// diagnostic maps back through the production `ProviderPositionMapper` to the `.vue`
/// script span (never a forged `(0,0)`), and NO TS2307 (every carrier import
/// resolves). Discriminator: a composite that never engages SHARED per query falls back
/// to the empty OWNED baseline, so no TS2322 would appear.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn composite_overlays_shared_diagnostics_via_live_resolver() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let h = setup_composite(&tsgo, "composite", Arc::new(OwnedBaselineDouble)).await;

    // REAL IDE codegen (NOT handcrafted): the imported-prop type flows through the
    // macro into the carrier via the shared resolver.
    let (ide_code, source_map, companion) = compile_vue_ide(WIDGET_VUE);
    let carrier_tsx = norm(&h.src.join("Widget.vue.tsx"));
    let companion_ts = norm(&h.src.join("Widget.vue.verter.ts"));

    // Open BOTH companions through the COMPOSITE (delegates to OWNED and feeds the
    // SHARED overlay for the bound carrier).
    h.composite
        .open_file(&companion_ts, &companion)
        .await
        .expect("composite open of the .verter.ts companion");
    h.composite
        .open_file(&carrier_tsx, &ide_code)
        .await
        .expect("composite open of the real IDE carrier");

    let diags = tokio::time::timeout(
        Duration::from_secs(45),
        h.composite.get_diagnostics(&carrier_tsx),
    )
    .await
    .expect("composite get_diagnostics timed out")
    .expect("composite diagnostics");

    let codes: Vec<_> = diags.iter().filter_map(|d| d.code.clone()).collect();
    eprintln!("[composite] diagnostics codes = {codes:?}");

    // THE PROOF: SHARED engaged PER QUERY — the carrier's owning project resolved to
    // a `ProjectBinding` over the published snapshot, minted the `BoundProject`
    // witness, and overlaid the deliberate TS2322. On the pre-fix code SHARED never
    // engages, so `diags` is the empty OWNED baseline and this assertion FIRES.
    let ts2322 = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("2322"))
        .unwrap_or_else(|| {
            panic!(
                "the composite must overlay the SHARED carrier TS2322 (per-query binding resolved \
                 over the published snapshot); got {codes:?} — SHARED did not engage"
            )
        });
    // NEGATIVE: every carrier import resolved — no spurious TS2307.
    assert!(
        !diags.iter().any(|d| d.code.as_deref() == Some("2307")),
        "every carrier import must resolve — no TS2307; got {codes:?}"
    );

    // Map the carrier-origin TS2322 back through the PRODUCTION
    // `ProviderPositionMapper` to the `.vue` source span (never a forged `(0,0)`).
    let mapper = ProviderPositionMapper::source_map(
        PositionMapper::from_json(&source_map).expect("parse the IDE source map"),
    );
    let tsx_line_index = LineIndex::new_utf16(&ide_code);
    let vue_line_index = LineIndex::new_utf16(WIDGET_VUE);
    let mapped = tsx_range_to_carrier_range(
        ts2322.start,
        ts2322.end,
        &tsx_line_index,
        &mapper,
        &vue_line_index,
    )
    .expect("the composite SHARED TS2322 must map back to the .vue source (never forged)");
    assert!(
        !(mapped.start.line == 0
            && mapped.start.character == 0
            && mapped.end.line == 0
            && mapped.end.character == 0),
        "the mapped span must be a real .vue location, never a forged (0,0)"
    );
    let vue_lines: Vec<&str> = WIDGET_VUE.lines().collect();
    let mapped_line = vue_lines
        .get(mapped.start.line as usize)
        .copied()
        .unwrap_or("");
    assert!(
        mapped_line.contains("props.label") || mapped_line.contains("wrong"),
        "the TS2322 must map back to the deliberate error line in the .vue script; mapped to \
         line {} = {mapped_line:?}",
        mapped.start.line
    );

    teardown_composite(h).await;
}

/// SERVING ORDER + TOPOLOGY — successful diagnostics and hover both use the exact
/// editor-owned relay session. Lifecycle recording and repeated feature queries never
/// activate the lazy managed fallback, so the shim's real editor tsgo is the only
/// semantic engine process in the test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn composite_successful_shared_route_never_activates_managed_fallback() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let activation_count = Arc::new(AtomicUsize::new(0));
    let fallback = Arc::new(LazyManagedTypeProvider::new({
        let activation_count = Arc::clone(&activation_count);
        move || {
            let activation_count = Arc::clone(&activation_count);
            async move {
                activation_count.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(OwnedBaselineDouble) as Arc<dyn TypeProvider>)
            }
        }
    })) as Arc<dyn TypeProvider>;
    let h = setup_composite(&tsgo, "shared_only", fallback).await;

    let (ide_code, _source_map, companion) = compile_vue_ide(WIDGET_VUE);
    let carrier_tsx = norm(&h.src.join("Widget.vue.tsx"));
    let companion_ts = norm(&h.src.join("Widget.vue.verter.ts"));

    h.composite
        .open_file(&companion_ts, &companion)
        .await
        .expect("composite open of the .verter.ts companion");
    h.composite
        .open_file(&carrier_tsx, &ide_code)
        .await
        .expect("composite open of the real IDE carrier");

    assert_eq!(
        activation_count.load(Ordering::SeqCst),
        0,
        "carrier lifecycle must not activate managed tsgo"
    );
    assert_eq!(h.composite.child_pid(), None);

    let diagnostics = tokio::time::timeout(
        Duration::from_secs(45),
        h.composite.get_diagnostics(&carrier_tsx),
    )
    .await
    .expect("composite get_diagnostics timed out")
    .expect("composite diagnostics");

    let codes: Vec<_> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.clone())
        .collect();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("2322")),
        "strict pull diagnostics from the shared editor Program must include TS2322; got {codes:?}"
    );

    let label_offset = ide_code
        .find("props.label")
        .map(|offset| offset + "props.".len())
        .expect("real IDE code contains the deliberate props.label access")
        as u32;
    let hover = tokio::time::timeout(
        Duration::from_secs(45),
        h.composite.get_hover(&carrier_tsx, label_offset),
    )
    .await
    .expect("shared hover timed out")
    .expect("shared hover request")
    .expect("hover at props.label");
    assert!(
        hover.contents.contains("label") && hover.contents.contains("string"),
        "hover must come from the editor-owned Program's imported prop type: {hover:?}"
    );

    assert_eq!(
        activation_count.load(Ordering::SeqCst),
        0,
        "successful shared diagnostics + hover must never construct managed tsgo"
    );
    assert_eq!(h.composite.child_pid(), None);

    teardown_composite(h).await;
}

/// A missing advertised editor session is an observed attach failure. It admits the
/// managed tier on the first bound feature demand, but lifecycle alone remains cold and
/// repeated demands reuse the single memoized activation.
#[tokio::test]
async fn composite_attach_failure_activates_managed_fallback_exactly_once() {
    let dir = tempdir("missing_editor");
    let (tsconfig, src) = write_fixture(&dir);
    let workspace = norm(&dir);
    let tsconfig = norm(&tsconfig);

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(fixture_snapshot(
        &workspace, &tsconfig,
    ))));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);

    let overlay = SharedTsgoOverlay::new(
        Arc::clone(&host),
        SharedRendezvous {
            control_dir: dir.join("absent-control"),
            session_key: "no-editor-session".to_string(),
            workspace_root: workspace,
        },
    );
    let activation_count = Arc::new(AtomicUsize::new(0));
    let fallback = Arc::new(LazyManagedTypeProvider::new({
        let activation_count = Arc::clone(&activation_count);
        move || {
            let activation_count = Arc::clone(&activation_count);
            async move {
                activation_count.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(OwnedBaselineDouble) as Arc<dyn TypeProvider>)
            }
        }
    })) as Arc<dyn TypeProvider>;
    let composite = TsgoCompositeProvider::new(fallback, host, Some(overlay));

    let (ide_code, _source_map, companion) = compile_vue_ide(WIDGET_VUE);
    let carrier_tsx = norm(&src.join("Widget.vue.tsx"));
    let companion_ts = norm(&src.join("Widget.vue.verter.ts"));
    composite
        .open_file(&companion_ts, &companion)
        .await
        .unwrap();
    composite.open_file(&carrier_tsx, &ide_code).await.unwrap();
    assert_eq!(activation_count.load(Ordering::SeqCst), 0);

    let offset = ide_code.find("props.label").unwrap() as u32;
    assert!(composite
        .get_hover(&carrier_tsx, offset)
        .await
        .unwrap()
        .is_none());
    assert!(composite
        .get_hover(&carrier_tsx, offset)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        activation_count.load(Ordering::SeqCst),
        1,
        "the observed missing editor attach activates managed once; repeats reuse it"
    );

    composite.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
