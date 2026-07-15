//! JS PARITY ORACLE — the primary automated wire-correctness rail.
//!
//! Runs the SAME `tsgo --api` operations through BOTH (a) this Rust client and
//! (b) the official JS client (`<pkg>/unstable/sync`, via
//! `tests/js/parity-oracle.mjs`) against the SAME engine and the SAME fixture,
//! then asserts the two result sets are IDENTICAL. Because the codec is
//! hand-written, this catches a hand-coding error or a wire divergence
//! automatically — independent of anyone running the maintainer version-bump
//! agent.
//!
//! NON-VACUOUS: it actually executes both clients and compares. The engine
//! binary is a PARAMETER (the rc `typescript@7.x` package) — never hardcoded.
//! Under `VERTER_REQUIRE_TSGO` a missing engine or missing node is a hard
//! failure (a skip there is a vacuous-pass FAILURE).

use super::common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use verter_tsgo_api::actor::spawn_actor;
use verter_tsgo_api::proto::types::{method, Diagnostic, TypeResponse, UpdateSnapshotResponse};
use verter_tsgo_api::snapshot::{AccessibleEntries, OverlaySnapshot, RealDirSource};
use verter_tsgo_api::transport::pipe::StdioPipeTransport;
use verter_tsgo_api::{ClientHandle, RequestOptions};

/// The comparable result both clients must agree on.
#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ParityResult {
    #[serde(rename = "projectOpened")]
    project_opened: bool,
    #[serde(rename = "carrierInRootFiles")]
    carrier_in_root_files: bool,
    #[serde(rename = "semanticDiagnostics")]
    semantic_diagnostics: Vec<DiagPoint>,
    #[serde(rename = "typeAtX")]
    type_at_x: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct DiagPoint {
    code: u32,
    pos: u32,
    end: u32,
}

#[derive(Debug)]
struct StdFsDirSource;
impl RealDirSource for StdFsDirSource {
    fn real_entries(&self, dir: &str) -> Option<AccessibleEntries> {
        let rd = std::fs::read_dir(dir).ok()?;
        let mut files = Vec::new();
        let mut directories = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            match entry.file_type() {
                Ok(t) if t.is_dir() => directories.push(name),
                Ok(_) => files.push(name),
                Err(_) => {}
            }
        }
        Some(AccessibleEntries { files, directories })
    }
}

async fn req<T: serde::de::DeserializeOwned>(
    handle: &ClientHandle,
    method: &str,
    params: serde_json::Value,
) -> T {
    let payload = serde_json::to_vec(&params).unwrap();
    let bytes = tokio::time::timeout(
        Duration::from_secs(30),
        handle.request(method, payload, RequestOptions::default()),
    )
    .await
    .unwrap_or_else(|_| panic!("`{method}` timed out"))
    .unwrap_or_else(|e| panic!("`{method}` failed: {e}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("`{method}` decode failed: {e}"))
}

/// Compute the parity result via the RUST client against the live engine.
async fn rust_result(
    exe: &Path,
    fixture_dir: &Path,
    tsconfig: &Path,
    carrier: &Path,
    content: &str,
) -> ParityResult {
    let src_dir = carrier.parent().unwrap();
    let carrier_norm = common::norm(carrier);

    let snapshot = OverlaySnapshot::builder()
        .file(&carrier_norm, content)
        .directory(common::norm(src_dir))
        .real_dir_source(Arc::new(StdFsDirSource))
        .build();
    let transport = StdioPipeTransport::spawn(exe, fixture_dir).expect("spawn tsgo");
    let handle = spawn_actor(transport, snapshot, 16);

    // initialize (handshake; not compared, but exercises the wire).
    let _: serde_json::Value = req(&handle, method::INITIALIZE, serde_json::Value::Null).await;

    let snap: UpdateSnapshotResponse = req(
        &handle,
        method::UPDATE_SNAPSHOT,
        serde_json::json!({ "openProjects": [common::norm(tsconfig)] }),
    )
    .await;

    let project = snap
        .projects
        .iter()
        .find(|p| common::norm(Path::new(&p.config_file_name)) == common::norm(tsconfig));
    let project_opened = project.is_some();
    let carrier_in_root_files = project
        .map(|p| {
            p.root_files
                .iter()
                .any(|f| common::norm(Path::new(f)) == carrier_norm)
        })
        .unwrap_or(false);

    let mut semantic_diagnostics: Vec<DiagPoint> = Vec::new();
    let mut type_at_x: Option<String> = None;
    if let Some(project) = project {
        let diags: Vec<Diagnostic> = req(
            &handle,
            method::GET_SEMANTIC_DIAGNOSTICS,
            serde_json::json!({ "snapshot": snap.snapshot, "project": project.id, "file": carrier_norm }),
        )
        .await;
        semantic_diagnostics = diags
            .iter()
            .map(|d| DiagPoint {
                code: d.code,
                pos: d.pos,
                end: d.end,
            })
            .collect();
        semantic_diagnostics.sort_by(|a, b| a.code.cmp(&b.code).then(a.pos.cmp(&b.pos)));

        // type-at-position at the `x:` declaration, then typeToString.
        if let Some(x_off) = content.find("x:") {
            let ty: Option<TypeResponse> = req(
                &handle,
                method::GET_TYPE_AT_POSITION,
                serde_json::json!({
                    "snapshot": snap.snapshot,
                    "project": project.id,
                    "file": carrier_norm,
                    "position": x_off,
                }),
            )
            .await;
            if let Some(ty) = ty {
                let s: String = req(
                    &handle,
                    method::TYPE_TO_STRING,
                    serde_json::json!({ "snapshot": snap.snapshot, "project": project.id, "type": ty.id }),
                )
                .await;
                type_at_x = Some(s);
            }
        }
    }

    handle.close().await.ok();

    ParityResult {
        project_opened,
        carrier_in_root_files,
        semantic_diagnostics,
        type_at_x,
    }
}

/// Compute the parity result via the OFFICIAL JS client (the oracle).
fn js_result(
    exe: &Path,
    fixture_dir: &Path,
    tsconfig: &Path,
    carrier: &Path,
    content: &str,
) -> ParityResult {
    let harness = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("js")
        .join("parity-oracle.mjs");
    use base64::Engine as _;
    let content_b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let ts7_source = std::env::var("TS7_SOURCE").unwrap_or_else(|_| "typescript".to_string());

    let output = Command::new("node")
        .arg(&harness)
        .arg(fixture_dir)
        .arg(tsconfig)
        .arg(carrier)
        .arg(&content_b64)
        .env("TS7_SOURCE", ts7_source)
        .env("TSGO_PATH", exe)
        .env("NM_BASE", common::workspace_root())
        .output()
        .expect("run node parity oracle");

    if !output.status.success() {
        panic!(
            "JS parity oracle failed (status {:?}):\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("JS oracle output is not the expected JSON ({e}): {stdout}"))
}

/// Confirm `node` is available; under VERTER_REQUIRE_TSGO a missing node is a
/// hard failure (the gate must run the oracle non-vacuously).
fn node_available() -> bool {
    let ok = Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok && std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
        panic!(
            "VERTER_REQUIRE_TSGO is set but `node` is unavailable; the JS parity oracle cannot run"
        );
    }
    ok
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rust_and_js_clients_agree_on_the_same_ops() {
    let Some(exe) = common::engine_or_skip() else {
        return;
    };
    if !node_available() {
        eprintln!("[skip] node unavailable; cannot run the JS parity oracle");
        return;
    }

    // One hermetic fixture, two clients, IDENTICAL ops. The carrier carries a
    // deliberate TS2345 so the diagnostic comparison is non-trivial.
    let tmp = tempdir();
    let tsconfig = common::write_fixture_project(&tmp);
    let carrier = tmp.join("src").join("Carrier.tsx");
    let content =
        "import { double } from \"./util\";\nexport const x: number = double(\"nope\");\n";

    let rust = rust_result(&exe, &tmp, &tsconfig, &carrier, content).await;
    let js = js_result(&exe, &tmp, &tsconfig, &carrier, content);

    // The crux: the two clients must produce byte-identical structured results.
    assert_eq!(
        rust, js,
        "Rust and JS clients diverged on identical ops:\n  rust = {rust:?}\n  js   = {js:?}"
    );

    // And the result must be MEANINGFUL (non-vacuous): the project opened, the
    // off-disk carrier was a root file, the deliberate TS2345 fired, and the
    // type printed as `number`. A degenerate empty-equal result is rejected.
    assert!(rust.project_opened, "project must have opened");
    assert!(rust.carrier_in_root_files, "carrier must be a root file");
    assert!(
        rust.semantic_diagnostics.iter().any(|d| d.code == 2345),
        "the deliberate TS2345 must appear in BOTH clients: {:?}",
        rust.semantic_diagnostics
    );
    assert_eq!(rust.type_at_x.as_deref(), Some("number"));
}

/// Also assert agreement on a CLEAN carrier (zero diagnostics on both sides),
/// proving the parity holds for the empty-diagnostic case too (not just errors).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rust_and_js_agree_on_clean_carrier() {
    let Some(exe) = common::engine_or_skip() else {
        return;
    };
    if !node_available() {
        return;
    }
    let tmp = tempdir();
    let tsconfig = common::write_fixture_project(&tmp);
    let carrier = tmp.join("src").join("Carrier.tsx");
    let content = "import { double } from \"./util\";\nexport const x: number = double(21);\n";

    let rust = rust_result(&exe, &tmp, &tsconfig, &carrier, content).await;
    let js = js_result(&exe, &tmp, &tsconfig, &carrier, content);
    assert_eq!(
        rust, js,
        "clean-carrier parity:\n rust={rust:?}\n js={js:?}"
    );
    assert!(
        rust.semantic_diagnostics.is_empty(),
        "clean carrier has zero diagnostics on both clients"
    );
    assert_eq!(rust.type_at_x.as_deref(), Some("number"));
}

fn tempdir() -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "verter_tsgo_api_parity_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
