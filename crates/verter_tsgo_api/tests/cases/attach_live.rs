//! LIVE, NON-VACUOUS proof of the OWNED one-instance dual-surface attach against a
//! REAL `tsgo` engine, exercising the PRODUCTION attach orchestration
//! ([`verter_tsgo_api::attach`]) — not a spike.
//!
//! The flow (mirrors the de-risk probe, now in production code): spawn one
//! `tsgo --lsp` via the production `SpawnOwnTsgoLsp` source, send
//! `custom/initializeAPISession` over its stdio, connect the minted pipe, drive
//! the `--api` checker over it. An OFF-DISK carrier is injected as a `--lsp`
//! `textDocument/didOpen` overlay; the attached `--api` checker — sharing the
//! `--lsp` server's `project.Session` — SEES that overlay and reports its
//! deliberate type error. This is the one-instance proof: ONE process, BOTH
//! surfaces, ONE shared Program.
//!
//! Gating: NON-VACUOUS whenever tsgo is present. Under `VERTER_REQUIRE_TSGO` a
//! missing engine is a HARD failure (a skip would be a vacuous pass).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use verter_tsgo_api::attach::{SpawnOwnTsgoLsp, TsgoAttach};
use verter_tsgo_api::transport::spawn::discover_tsgo;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Discover the engine, honoring `VERTER_REQUIRE_TSGO` (a skip under that env is a
/// vacuous-pass failure).
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

/// Path comparison matching the tsgo engine's canonicalization. The engine
/// lowercases the Windows drive letter (and is case-insensitive on a
/// case-insensitive filesystem) and uses forward slashes. Compare the
/// forward-slashed forms case-insensitively so a `C:` vs `c:` drive-letter
/// difference (or any NTFS/APFS case fold) does not spuriously miss the project /
/// carrier. This is the path-portability rule the provider must also apply.
fn path_eq(a: &str, b: &str) -> bool {
    let na = a.replace('\\', "/");
    let nb = b.replace('\\', "/");
    na.eq_ignore_ascii_case(&nb)
}

/// Find the off-disk carrier's path AS THE ENGINE REPORTS IT in the project's
/// root-file set (the engine's canonical form — lowercased drive letter etc.).
/// Diagnostics must be requested with the engine's own path form (a `file://`
/// URI or an upper-cased drive letter fails: "source file not found").
fn engine_carrier_path<'a>(
    project: &'a verter_tsgo_api::proto::types::ProjectResponse,
    carrier: &str,
) -> Option<&'a str> {
    project
        .root_files
        .iter()
        .find(|f| path_eq(f, carrier))
        .map(String::as_str)
}

fn tempdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("verter_tsgo_attach_{}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A configured project on disk: a real `util.ts` + a `tsconfig.json` whose
/// `include` enumerates `src/**/*` (so an off-disk `src/Carrier.ts` overlay is a
/// member). Returns `(dir, tsconfig_path)`.
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

/// THE non-vacuous one-instance proof. Spawn one `tsgo --lsp`, attach `--api`,
/// inject an off-disk carrier via `--lsp` didOpen, and assert the attached `--api`
/// checker sees the overlay + its deliberate type error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_api_over_spawned_lsp_sees_didopen_overlay_carrier() {
    let Some(exe) = engine_or_skip() else {
        return;
    };

    let dir = tempdir();
    let tsconfig = write_fixture(&dir);
    let tsconfig_norm = norm(&tsconfig);
    let root_uri = format!("file:///{}", norm(&dir).trim_start_matches('/'));

    // The OFF-DISK carrier: a path under `src/` that is NOT written to disk. It is
    // injected purely as a `--lsp` didOpen overlay. A deliberate TS2322 (string →
    // number) must surface through the attached `--api` checker.
    let carrier_path = dir.join("src").join("Carrier.ts");
    let carrier_norm = norm(&carrier_path);
    let carrier_uri = format!("file:///{}", carrier_norm.trim_start_matches('/'));
    assert!(
        !carrier_path.exists(),
        "the carrier must be OFF-DISK (overlay only)"
    );
    let carrier_src = "import { double } from \"./util\";\n\
         export const ok: number = double(21);\n\
         export const bad: number = \"not a number\";\n";

    // 1. Establish the one-instance attach via the PRODUCTION spawn-own source.
    let source = SpawnOwnTsgoLsp::new(&exe, &dir);
    let attach = tokio::time::timeout(
        Duration::from_secs(40),
        TsgoAttach::establish(&source, &root_uri),
    )
    .await
    .expect("attach establish timed out")
    .expect("attach establish failed");

    // 2. Inject the off-disk carrier as a --lsp didOpen overlay AND synchronize it
    //    (the --api session shares the --lsp server's project.Session; the barrier
    //    forces the server to register the overlay before --api updateSnapshot
    //    enumerates roots on the shared session — the two ride different transports).
    attach
        .injection_channel()
        .did_open_synced(&carrier_uri, "typescript", 1, carrier_src)
        .await
        .expect("didOpen overlay + sync");

    // 3. Open the CONFIGURED project on the --api side (openProjects only — the
    //    --lsp server owns documents). This is the project-bound membership: the
    //    carrier rides the configured tsconfig, NOT a config-less inferred project.
    //    The updateSnapshot rail rides the STORED in-band serverInfo witness the
    //    handshake gate accepted (attach.update_snapshot), not a hardcoded version.
    let snap = tokio::time::timeout(
        Duration::from_secs(30),
        attach.update_snapshot(&tsconfig_norm),
    )
    .await
    .expect("updateSnapshot timed out")
    .expect("updateSnapshot failed");

    let project = snap
        .project_for_config(|c| path_eq(c, &tsconfig_norm))
        .expect("the opened configured project is in the snapshot");

    // The off-disk carrier must be a Program ROOT (the didOpen overlay made it a
    // member of the CONFIGURED project — the project-bound membership, NOT an
    // inferred config-less project).
    let engine_carrier = engine_carrier_path(project, &carrier_norm).unwrap_or_else(|| {
        panic!(
            "the off-disk carrier must be a Program root of the configured project (overlay \
             membership); roots: {:?}",
            project.root_files
        )
    });

    // 4. Drive --api getSemanticDiagnostics on the OFF-DISK carrier over the pipe,
    //    using the engine's own canonical path form.
    let diags = tokio::time::timeout(
        Duration::from_secs(30),
        attach
            .api()
            .get_semantic_diagnostics(&snap.snapshot, &project.id, engine_carrier),
    )
    .await
    .expect("getSemanticDiagnostics timed out")
    .expect("getSemanticDiagnostics failed");

    // THE PROOF (NON-VACUOUS): the deliberate TS2322 is reported on the off-disk
    // carrier — so the attached `--api` checker SAW the `--lsp` didOpen overlay.
    assert!(
        diags.iter().any(|d| d.code == 2322),
        "the attached --api checker must see the didOpen overlay carrier's deliberate \
         TS2322 (string -> number); got: {diags:?}"
    );
    // NEGATIVE: the import of `./util` RESOLVED (no false TS2307) — the carrier is a
    // genuine member of the CONFIGURED project, not a config-less inferred one.
    assert!(
        !diags.iter().any(|d| d.code == 2307),
        "the carrier's `./util` import must resolve under the configured project (no \
         false TS2307); got: {diags:?}"
    );

    // Ownership-dispatched teardown: this attach rides a SPAWNED (Owned)
    // connection, so teardown() takes the full shutdown arm (exit + kill).
    attach.teardown().await.expect("teardown");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The dual-interface proof: BOTH the `--api` checker AND a `--lsp` feature answer
/// on the same carrier over the ONE process, observing the SAME shared Program.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_process_serves_both_api_checker_and_lsp_feature() {
    let Some(exe) = engine_or_skip() else {
        return;
    };

    let dir = tempdir();
    let tsconfig = write_fixture(&dir);
    let tsconfig_norm = norm(&tsconfig);
    let root_uri = format!("file:///{}", norm(&dir).trim_start_matches('/'));

    let carrier_path = dir.join("src").join("Carrier.ts");
    let carrier_norm = norm(&carrier_path);
    let carrier_uri = format!("file:///{}", carrier_norm.trim_start_matches('/'));
    // A clean carrier with a hover-able symbol on a known line.
    let carrier_src = "import { double } from \"./util\";\n\
         export const value = double(21);\n";

    let source = SpawnOwnTsgoLsp::new(&exe, &dir);
    let attach = TsgoAttach::establish(&source, &root_uri)
        .await
        .expect("attach establish");

    attach
        .injection_channel()
        .did_open_synced(&carrier_uri, "typescript", 1, carrier_src)
        .await
        .expect("didOpen + sync");
    // The stored in-band witness drives the updateSnapshot rail here too.
    let snap = attach
        .update_snapshot(&tsconfig_norm)
        .await
        .expect("updateSnapshot");
    let project = snap
        .project_for_config(|c| path_eq(c, &tsconfig_norm))
        .expect("configured project present");
    let engine_carrier = engine_carrier_path(project, &carrier_norm)
        .expect("carrier is a Program root of the configured project");

    // (a) The --api checker answers on the carrier: clean file ⇒ no TS2307.
    let diags = attach
        .api()
        .get_semantic_diagnostics(&snap.snapshot, &project.id, engine_carrier)
        .await
        .expect("api diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == 2307),
        "the clean carrier resolves its import on the --api checker (no TS2307): {diags:?}"
    );

    // (b) A --lsp FEATURE answers on the SAME carrier over the SAME process. Hover
    //     on `double` (the imported symbol) must return content — proving the --lsp
    //     surface sees the same didOpen overlay the --api checker did.
    let offset_line = 1; // 0-based: line 2 = `export const value = double(21);`
    let double_col = carrier_src
        .lines()
        .nth(1)
        .and_then(|l| l.find("double"))
        .expect("double on line 2") as u32;
    let hover = attach
        .lsp()
        .request(
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": carrier_uri },
                "position": { "line": offset_line, "character": double_col },
            }),
        )
        .await
        .expect("lsp hover request");
    // A hover over a resolved symbol returns non-null contents — the --lsp surface
    // sees the overlay carrier (the dual-interface assertion).
    assert!(
        !hover.is_null() && hover.get("contents").is_some(),
        "the --lsp feature surface must answer hover on the overlay carrier (same \
         shared Program as --api): got {hover:?}"
    );

    // Owned connection ⇒ teardown() dispatches to the full shutdown arm.
    attach.teardown().await.expect("teardown");
    let _ = std::fs::remove_dir_all(&dir);
}
