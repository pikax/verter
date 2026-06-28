//! LIVE proof that [`TsgoOwnedProvider`] is ONE process serving BOTH surfaces as
//! ONE `TypeProvider`: diagnostics via the attached `--api` checker, features via
//! the `--lsp` interface, over the SAME spawned `tsgo --lsp` process.
//!
//! NON-VACUOUS: drives a real tsgo. Under `VERTER_REQUIRE_TSGO` a missing engine
//! is a HARD failure (a skip would be a vacuous pass).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use verter_tsgo_api::transport::spawn::discover_tsgo;
use verter_type_runtime::traits::TypeProvider;
use verter_type_runtime::tsgo::{TsgoOwnedProvider, TsgoTypeProvider};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

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

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn tempdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("verter_owned_prov_{}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A configured project: `util.ts` + a tsconfig including `src/**/*`.
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

async fn build_owned_provider(exe: &Path, dir: &Path, tsconfig: &Path) -> TsgoOwnedProvider {
    let root_uri = format!("file:///{}", slash(dir).trim_start_matches('/'));
    let lsp = TsgoTypeProvider::spawn(&exe.to_string_lossy(), &root_uri)
        .await
        .expect("spawn tsgo --lsp");
    TsgoOwnedProvider::attach(Arc::new(lsp), slash(tsconfig), exe)
        .await
        .expect("attach --api checker (one process)")
}

/// Diagnostics flow through the OWNED provider via the `--api` checker on an
/// off-disk carrier, and a `--lsp` feature answers on the SAME carrier — ONE
/// process, ONE query path, both surfaces, one shared Program.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process() {
    let Some(exe) = engine_or_skip() else {
        return;
    };
    let dir = tempdir();
    let tsconfig = write_fixture(&dir);
    let provider = build_owned_provider(&exe, &dir, &tsconfig).await;

    // The dual-surface provider identifies transparently as the tsgo engine (the
    // --api attach is an internal detail of the ONE provider).
    assert_eq!(provider.provider_id(), "tsgo");

    // ONE process: there is exactly one child PID (the inner --lsp process the
    // --api checker is attached to). No second spawn.
    let pid = provider.child_pid();
    assert!(
        pid.is_some(),
        "the owned provider must expose its single child PID"
    );

    let carrier = dir.join("src").join("Carrier.ts");
    let carrier_path = slash(&carrier);
    // A self-contained carrier (no cross-file import) so the proof isolates the ONE
    // thing under test: the attached `--api` checker SEES the off-disk overlay and
    // type-checks it as a configured-project member. The deliberate TS2322
    // (string → number) is that proof; cross-file relative-import resolution is
    // covered separately (the verter_tsgo_api attach_live tests).
    let carrier_src = "export const bad: number = \"definitely not a number\";\n\
         export const fine = bad + 1;\n";

    // Open the off-disk carrier through the provider (didOpen overlay + sync barrier).
    provider
        .open_file(&carrier_path, carrier_src)
        .await
        .expect("open carrier");

    // (1) TYPECHECK via the --api checker (the project-bound typecheck oracle): the
    //     deliberate TS2322 surfaces on the off-disk overlay carrier — proving the
    //     attached `--api` checker SEES the `--lsp` didOpen overlay and type-checks
    //     it as a member of the CONFIGURED project. This is the `--api` authority
    //     proof, distinct from the user-facing get_diagnostics surface (the `--lsp`
    //     pull). NON-VACUOUS: an empty/wrong-project result fails this assertion.
    let diags = tokio::time::timeout(
        Duration::from_secs(30),
        provider.semantic_diagnostics_for_carrier(&carrier_path),
    )
    .await
    .expect("--api semantic diagnostics timed out")
    .expect("--api semantic diagnostics");
    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("2322")),
        "the --api checker must report the deliberate TS2322 on the off-disk overlay \
         carrier (overlay visible + configured-project member); got: {diags:?}"
    );

    // (2) A --lsp FEATURE on the SAME carrier over the SAME process: definition on
    //     the `bad` reference returns at least one location (the --lsp surface sees
    //     the same overlay the --api checker did — one shared Program).
    let bad_ref = carrier_src.rfind("bad").expect("bad reference") as u32;
    let defs = provider
        .get_definition(&carrier_path, bad_ref)
        .await
        .expect("get_definition");
    assert!(
        !defs.is_empty(),
        "the --lsp feature surface must resolve definition on the overlay carrier \
         (same shared Program as --api); got: {defs:?}"
    );

    provider.shutdown().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&dir);
}

/// No-dual-path: the OWNED provider's `--api` attach rides the inner provider's
/// process — exactly ONE tsgo child PID, and that PID is the inner `--lsp`
/// provider's. There is no second spawn / parallel feature pipeline.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_provider_is_one_process_no_second_spawn() {
    let Some(exe) = engine_or_skip() else {
        return;
    };
    let dir = tempdir();
    let tsconfig = write_fixture(&dir);

    let root_uri = format!("file:///{}", slash(&dir).trim_start_matches('/'));
    let inner = Arc::new(
        TsgoTypeProvider::spawn(&exe.to_string_lossy(), &root_uri)
            .await
            .expect("spawn tsgo --lsp"),
    );
    let inner_pid = inner.child_pid();

    let provider = TsgoOwnedProvider::attach(Arc::clone(&inner), slash(&tsconfig), &exe)
        .await
        .expect("attach");

    // The owned provider's child PID IS the inner --lsp provider's PID — the --api
    // checker attached to the SAME process, not a new one.
    assert_eq!(
        provider.child_pid(),
        inner_pid,
        "the --api attach must ride the inner --lsp process (one process, no second spawn)"
    );

    provider.shutdown().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&dir);
}
