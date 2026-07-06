//! Headless mechanics e2e for the relay shim: `[fake editor] -> [real shim] ->
//! [real tsgo]`.
//!
//! A FAKE EDITOR spawns the REAL shim binary and speaks LSP over its stdio
//! (`initialize` → capture the relayed real-tsgo `serverInfo.version` →
//! `initialized`). It then plays the CONTROL CLIENT: reads the shim's
//! advertisement, `verter/hello` (nonce verified), `verter/waitInitialized`,
//! `verter/initializeApiSession` → connects the returned `--api` pipe DIRECTLY
//! with [`ApiAttachClient`], injects an INLINE off-disk carrier overlay via
//! `verter/carrierDidOpenSynced`, `updateSnapshot(tsconfig)`, and reads semantic
//! diagnostics. The REAL checker seeing the carrier (a deliberate TS2322, and
//! the negative: no spurious TS2307) proves the whole transport chain.
//!
//! The negatives prove the relay's leak/id-demux/version guarantees hold
//! end-to-end through the real shim + tsgo.
//!
//! Gating: NON-VACUOUS whenever tsgo is present. Under `VERTER_REQUIRE_TSGO` a
//! missing engine is a HARD failure (a skip would be a vacuous pass). The REAL
//! Verter-IDE-codegen `.vue`-macro proof is a later concern — an inline carrier
//! is the honest mechanics-level proof here.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use verter_tsgo_api::api_attach::ApiAttachClient;
use verter_tsgo_api::control::messages::PROTOCOL_VERSION;
use verter_tsgo_api::control::{Advertisement, ControlClient};
use verter_tsgo_api::jsonrpc::{encode_message, JsonRpcConnection, MessageFramer};
use verter_tsgo_api::proto::types::ProjectResponse;
use verter_tsgo_api::transport::pipe_attach::connect_attach_pipe;
use verter_tsgo_api::transport::spawn::discover_tsgo;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Discover the engine, honoring `VERTER_REQUIRE_TSGO` (a skip under that env is
/// a vacuous-pass failure).
fn engine_or_skip() -> Option<PathBuf> {
    match discover_tsgo(&workspace_root()) {
        Ok(p) => Some(p),
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

/// Path comparison matching the tsgo engine's canonicalization (lowercased drive
/// letter, forward slashes, case-insensitive fold).
fn path_eq(a: &str, b: &str) -> bool {
    a.replace('\\', "/")
        .eq_ignore_ascii_case(&b.replace('\\', "/"))
}

/// The carrier's path AS THE ENGINE REPORTS IT in the project's root-file set.
fn engine_carrier_path<'a>(project: &'a ProjectResponse, carrier: &str) -> Option<&'a str> {
    project
        .root_files
        .iter()
        .find(|f| path_eq(f, carrier))
        .map(String::as_str)
}

fn tempdir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "verter_shim_live_{tag}_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A configured project on disk: `src/util.ts` + a `tsconfig.json` whose
/// `include` covers `src/**/*` (so an off-disk `src/Carrier.ts` overlay is a
/// member). Returns the tsconfig path.
fn write_fixture(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("util.ts"),
        "export function double(n: number): number {\n  return n * 2;\n}\n",
    )
    .unwrap();
    let tsconfig = dir.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["src/**/*"]
}
"#,
    )
    .unwrap();
    tsconfig
}

/// The off-disk carrier overlay: imports on-disk `./util` (must resolve — no
/// TS2307) and carries a uniquely-named deliberate TS2322 (`string → number`).
/// The unique symbol name + `Carrier.ts` basename + its URI are the leak probes.
fn carrier_fixture(dir: &Path) -> (String, String, String) {
    let carrier_path = dir.join("src").join("Carrier.ts");
    let carrier_norm = norm(&carrier_path);
    let carrier_uri = format!("file:///{}", carrier_norm.trim_start_matches('/'));
    let src = "import { double } from \"./util\";\n\
         export const ok: number = double(21);\n\
         export const verterCarrierLeakProbe: number = \"definitely not a number\";\n"
        .to_string();
    (carrier_norm, carrier_uri, src)
}

/// A fake editor over the shim's stdio: writes LSP frames to the shim's stdin
/// (via a serialized writer task), records EVERY frame the shim writes to its
/// stdout (so a leak test can inspect the whole editor-visible stream), AND
/// auto-answers each server→client request with `null` — exactly what a real
/// editor (and the crate's `JsonRpcConnection` default handler) does, so the
/// real tsgo never blocks on `workspace/configuration` / `client/registerCapability`.
struct FakeEditor {
    out_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    frames: Arc<StdMutex<Vec<serde_json::Value>>>,
}

impl FakeEditor {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        // A serialized writer task owns stdin: both `send` and the reader's
        // auto-answers push onto it, so no two writes interleave.
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(bytes) = out_rx.recv().await {
                if stdin.write_all(&bytes).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
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
                    // Auto-answer a server→client REQUEST (id + method) with a
                    // null result so tsgo does not block on configuration /
                    // capability round-trips.
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

/// Spawn the REAL shim binary as the editor's `tsgo`, forwarding `--lsp
/// --stdio` to the real engine.
fn spawn_shim(tsgo: &Path, control_dir: &Path, session_key: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_verter-relay-shim"))
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

/// Poll `control_dir` until the shim publishes its advertisement.
async fn wait_for_advertisement(control_dir: &Path, session_key: &str) -> Advertisement {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok((_, adv)) = Advertisement::find_for_session_key(control_dir, session_key) {
            return adv;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the shim never published its advertisement"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Await a control/api future under a bounded deadline so a mechanics hang fails
/// fast with the step name (never an unbounded wall-clock hang).
async fn with_timeout<F: std::future::Future>(step: &str, fut: F) -> F::Output {
    match tokio::time::timeout(Duration::from_secs(45), fut).await {
        Ok(v) => v,
        Err(_) => panic!("mechanics step {step:?} timed out (bounded deadline)"),
    }
}

fn init_params(root_uri: &str) -> serde_json::Value {
    serde_json::json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {},
        "workspaceFolders": [{ "uri": root_uri, "name": "verter" }],
    })
}

/// The wired session: shim + fake editor + control client + attached `--api`
/// checker, initialized up to (but excluding) carrier injection.
struct Harness {
    shim: Child,
    editor: FakeEditor,
    ctl: ControlClient,
    api: ApiAttachClient,
    server_version: String,
    dir: PathBuf,
    tsconfig_norm: String,
    /// The discovered shim advertisement — its endpoint + nonce let a test open a
    /// FRESH control connection after a detach (to prove the shim stayed alive).
    adv: Advertisement,
}

/// Drive the full chain up to a ready attached `--api` checker: spawn the shim,
/// run the editor LSP handshake over its stdio, read + verify the advertisement,
/// hello, waitInitialized, initializeApiSession, connect the `--api` pipe.
async fn setup(tsgo: &Path, tag: &str) -> Harness {
    let dir = tempdir(tag);
    let tsconfig = write_fixture(&dir);
    let tsconfig_norm = norm(&tsconfig);
    let root_uri = format!("file:///{}", norm(&dir).trim_start_matches('/'));
    let control_dir = dir.join("ctl");
    let session_key = tag.to_string();

    let mut shim = spawn_shim(tsgo, &control_dir, &session_key);
    let editor_stdin = shim.stdin.take().expect("shim stdin piped");
    let editor_stdout = shim.stdout.take().expect("shim stdout piped");
    let editor = FakeEditor::new(editor_stdin, editor_stdout);

    // LSP initialize over the shim stdio → the relayed REAL tsgo serverInfo.version.
    editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params(&root_uri),
        }))
        .await;
    let init_resp = editor
        .wait_for(|m| m["id"] == 1, Duration::from_secs(40))
        .await
        .expect("the relayed initialize response");
    let relayed_version = init_resp["result"]["serverInfo"]["version"]
        .as_str()
        .expect("the relayed initialize carries serverInfo.version")
        .to_string();
    eprintln!("[mechanics] relayed real-tsgo serverInfo.version = {relayed_version:?}");
    assert_eq!(
        relayed_version, "7.0.1-rc",
        "the fake editor observes the REAL relayed tsgo version"
    );
    editor
        .send(&serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
        .await;

    // The control client: discover the advertisement, verify the nonce on hello.
    let adv = wait_for_advertisement(&control_dir, &session_key).await;
    assert_eq!(adv.protocol, PROTOCOL_VERSION);
    let mut ctl = ControlClient::connect(&adv.endpoint)
        .await
        .expect("connect the control endpoint");
    let hello = with_timeout("hello", ctl.hello(&adv.nonce, "verter_lsp"))
        .await
        .expect("hello (nonce + protocol verified)");
    assert_eq!(
        hello.editor_session_generation,
        adv.editor_session_generation
    );
    assert_eq!(hello.wire_pin, adv.wire_pin);

    // waitInitialized: the in-band witness the relay captured.
    let witness = with_timeout("waitInitialized", ctl.wait_initialized())
        .await
        .expect("waitInitialized");
    let server_version = witness
        .server_info_version
        .clone()
        .expect("the in-band serverInfo.version witness");
    assert_eq!(server_version, "7.0.1-rc");
    assert_eq!(witness.root_uri.as_deref(), Some(root_uri.as_str()));

    // initializeApiSession → connect the minted `--api` pipe DIRECTLY.
    let api_session = with_timeout("initializeApiSession", ctl.initialize_api_session())
        .await
        .expect("initializeApiSession");
    assert_eq!(api_session.handle_kind, "integer");
    let endpoint = api_session.endpoint().expect("a minted --api endpoint");
    let (read, write) = connect_attach_pipe(endpoint)
        .await
        .expect("connect the minted --api pipe");
    let api = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
    with_timeout("--api initialize", api.initialize())
        .await
        .expect("--api initialize");

    Harness {
        shim,
        editor,
        ctl,
        api,
        server_version,
        dir,
        tsconfig_norm,
        adv,
    }
}

/// Retract Verter's overlays (a NON-DESTRUCTIVE `verter/detach`), then kill the
/// shim process (the test owns the shim's lifecycle here) + clean the temp dir.
async fn teardown(mut h: Harness) {
    let _ = h.ctl.detach(true).await;
    let _ = h.ctl.close().await;
    let _ = h.shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), h.shim.wait()).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// THE mechanics proof: `[fake editor] -> [real shim] -> [real tsgo]`. An inline
/// carrier injected over the control protocol is seen by the attached `--api`
/// checker (deliberate TS2322 present; no spurious TS2307).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fake_editor_through_real_shim_and_tsgo_sees_injected_carrier() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let h = setup(&tsgo, "mechanics").await;
    let (carrier_norm, carrier_uri, carrier_src) = carrier_fixture(&h.dir);

    // Inject the off-disk carrier over the CONTROL protocol (relay injection +
    // sync barrier), then open the configured project on the --api side.
    with_timeout(
        "carrierDidOpenSynced",
        h.ctl
            .carrier_did_open_synced(&carrier_uri, "typescript", 1, &carrier_src),
    )
    .await
    .expect("carrier didOpenSynced through the control protocol");
    let snap = tokio::time::timeout(
        Duration::from_secs(30),
        h.api
            .update_snapshot_open_project(&h.tsconfig_norm, &h.server_version),
    )
    .await
    .expect("updateSnapshot timed out")
    .expect("updateSnapshot");

    let project = snap
        .project_for_config(|c| path_eq(c, &h.tsconfig_norm))
        .expect("the configured project is in the snapshot");
    let engine_carrier = engine_carrier_path(project, &carrier_norm).unwrap_or_else(|| {
        panic!(
            "the injected carrier must be a Program root of the configured project; roots: {:?}",
            project.root_files
        )
    });

    let diags = tokio::time::timeout(
        Duration::from_secs(30),
        h.api
            .get_semantic_diagnostics(&snap.snapshot, &project.id, engine_carrier),
    )
    .await
    .expect("getSemanticDiagnostics timed out")
    .expect("getSemanticDiagnostics");

    eprintln!(
        "[mechanics] --api carrier diagnostics codes = {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
    // THE PROOF: the deliberate TS2322 surfaces through the attached --api
    // checker — so the carrier injected over the CONTROL protocol reached the
    // REAL tsgo's shared project.Session.
    assert!(
        diags.iter().any(|d| d.code == 2322),
        "the --api checker must see the control-injected carrier's TS2322; got: {diags:?}"
    );
    // NEGATIVE: `./util` resolved — no spurious TS2307 (the carrier is a genuine
    // member of the configured project).
    assert!(
        !diags.iter().any(|d| d.code == 2307),
        "the carrier's ./util import must resolve (no false TS2307); got: {diags:?}"
    );

    teardown(h).await;
}

/// CARRIER-LEAK-LIVE + ID-DEMUX: with a carrier injected + processed by the real
/// tsgo, the FAKE EDITOR never receives any frame carrying the carrier URI/text,
/// never receives a `verter:*`-id frame (the injected barrier/session responses
/// demux to the control side), and an editor-origin `verter:*` request is
/// dropped (never answered) while a normal server response still reaches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_suppresses_carrier_leak_and_demuxes_verter_ids_end_to_end() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let h = setup(&tsgo, "leakdemux").await;
    let (carrier_norm, carrier_uri, carrier_src) = carrier_fixture(&h.dir);

    // Inject + process the carrier so the real tsgo genuinely holds it (any
    // server→editor frame that would reference it must be suppressed).
    with_timeout(
        "carrierDidOpenSynced",
        h.ctl
            .carrier_did_open_synced(&carrier_uri, "typescript", 1, &carrier_src),
    )
    .await
    .expect("carrier didOpenSynced");
    let snap = with_timeout(
        "updateSnapshot",
        h.api
            .update_snapshot_open_project(&h.tsconfig_norm, &h.server_version),
    )
    .await
    .expect("updateSnapshot");
    if let Some(project) = snap.project_for_config(|c| path_eq(c, &h.tsconfig_norm)) {
        if let Some(engine_carrier) = engine_carrier_path(project, &carrier_norm) {
            let _ = h
                .api
                .get_semantic_diagnostics(&snap.snapshot, &project.id, engine_carrier)
                .await;
        }
    }

    // Drive an editor request whose server response could reference the carrier
    // (a workspace symbol for the carrier's unique export), plus an editor-origin
    // reserved `verter:*` request that MUST be dropped.
    h.editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 100, "method": "workspace/symbol",
            "params": { "query": "verterCarrierLeakProbe" },
        }))
        .await;
    h.editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": "verter:probe", "method": "workspace/symbol",
            "params": { "query": "anything" },
        }))
        .await;
    // Let the server answer + emit any pushed diagnostics.
    let _ = h
        .editor
        .wait_for(|m| m["id"] == 100, Duration::from_secs(15))
        .await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    let frames = h.editor.all_frames();
    // (1) No carrier URI / text / basename ever reaches the editor.
    for frame in &frames {
        let text = frame.to_string();
        assert!(
            !text.contains(&carrier_uri)
                && !text.contains("verterCarrierLeakProbe")
                && !text.contains("Carrier.ts"),
            "the carrier leaked to the fake editor: {frame}"
        );
    }
    // (2) No reserved `verter:*` id ever reaches the editor (the injected
    //     barrier / api-session responses demux to the control side).
    for frame in &frames {
        if let Some(id) = frame.get("id").and_then(|v| v.as_str()) {
            assert!(
                !id.starts_with("verter:"),
                "a reserved verter:* id leaked to the editor: {frame}"
            );
        }
    }
    // (3) The editor-origin `verter:*` request is DROPPED — no response ever
    //     comes back for it (a reservation violation, never forwarded).
    let reserved_answer = h
        .editor
        .wait_for(
            |m| m.get("id").and_then(|v| v.as_str()) == Some("verter:probe"),
            Duration::from_millis(400),
        )
        .await;
    assert!(
        reserved_answer.is_none(),
        "an editor-origin verter:* request must be dropped, not answered"
    );
    // (4) Forwarding still works: the editor DID receive the relayed initialize
    //     response (a non-carrier server→editor frame).
    assert!(
        frames.iter().any(|m| m["id"] == 1),
        "the relay must forward non-carrier server frames (the initialize response) to the editor"
    );

    teardown(h).await;
}

/// PROTOCOL-VERSION-MISMATCH: a `verter/hello` with a wrong protocol version
/// fails closed (no attach); a correct-protocol hello on the same endpoint
/// succeeds — the discriminating pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_hello_wrong_protocol_fails_closed_live() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let dir = tempdir("protomismatch");
    let control_dir = dir.join("ctl");
    let session_key = "protomismatch";
    let mut shim = spawn_shim(&tsgo, &control_dir, session_key);
    // The shim advertises independently of any editor init.
    let adv = wait_for_advertisement(&control_dir, session_key).await;

    // Wrong protocol → fail closed (an error response, no attach).
    {
        let (read, write) = connect_attach_pipe(&adv.endpoint)
            .await
            .expect("connect control endpoint");
        let conn = JsonRpcConnection::connect(read, write);
        let result = conn
            .request(
                "verter/hello",
                serde_json::json!({
                    "protocol": PROTOCOL_VERSION + 1, "nonce": adv.nonce, "client": "verter_lsp",
                }),
            )
            .await;
        assert!(
            matches!(
                result,
                Err(verter_tsgo_api::error::TsgoApiError::Transport(_))
            ),
            "a wrong protocol version must fail closed (error response), got {result:?}"
        );
        let _ = conn.close().await;
    }

    // Correct protocol on a fresh connection → succeeds (discriminates the gate).
    {
        let mut ctl = ControlClient::connect(&adv.endpoint)
            .await
            .expect("connect control endpoint");
        let hello = ctl
            .hello(&adv.nonce, "verter_lsp")
            .await
            .expect("a correct-protocol hello must succeed");
        assert_eq!(hello.protocol, PROTOCOL_VERSION);
        let _ = ctl.close().await;
    }

    let _ = shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), shim.wait()).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// NON-DESTRUCTIVE DETACH (T3): `verter/detach` retracts Verter's overlays and drops
/// the Verter control pipe ONLY — it must NEVER tear the shim down or kill the shim's
/// OWNED tsgo child (doing so would destroy the editor's own type-checking). Proven
/// LIVE through the real shim + real tsgo: after a detach the shim process is still
/// running AND a FRESH control connection still hellos on the SAME advertised endpoint
/// (a torn-down shim would have removed the advertisement + dropped its listener).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verter_detach_is_non_destructive_shim_and_child_survive() {
    let Some(tsgo) = engine_or_skip() else {
        return;
    };
    let mut h = setup(&tsgo, "detachlive").await;

    // Verter detaches (retract overlays + drop the Verter control pipe).
    with_timeout("detach", h.ctl.detach(true))
        .await
        .expect("detach");
    let _ = h.ctl.close().await;

    // Give any (erroneous) shim teardown a chance to fire, then assert the shim — and
    // therefore its OWNED tsgo child — is STILL ALIVE. A pre-fix detach signaled the
    // shim to kill its child + stop the relay, exiting the shim.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        matches!(h.shim.try_wait(), Ok(None)),
        "the shim (and its OWNED tsgo child) must stay ALIVE after a non-destructive verter/detach"
    );

    // The DEFINITIVE discriminator: the relay + control endpoint are still live, so a
    // FRESH control connection hellos successfully on the SAME advertised endpoint.
    // A torn-down shim would have aborted its accept loop + removed the advertisement,
    // so this connect/hello would fail.
    let mut ctl2 = ControlClient::connect(&h.adv.endpoint)
        .await
        .expect("a fresh control connection after detach — the shim endpoint is still alive");
    let hello = with_timeout("re-hello", ctl2.hello(&h.adv.nonce, "verter_lsp"))
        .await
        .expect("a fresh hello after detach must succeed — the shim was NOT torn down by detach");
    assert_eq!(hello.protocol, PROTOCOL_VERSION);
    let _ = ctl2.close().await;

    // Explicit cleanup (the test owns the shim's lifecycle).
    let _ = h.shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), h.shim.wait()).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
