//! Shared helpers for the real-engine integration test and the JS parity
//! oracle. TEST-ONLY: never compiled into the production crate.

use std::path::{Path, PathBuf};

use verter_tsgo_api::transport::spawn::discover_tsgo;

/// Resolve the workspace root (where `node_modules` lives) from the crate's
/// manifest dir.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .and_then(|p| p.parent()) // repo root
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Discover the tsgo engine binary, honoring the `VERTER_REQUIRE_TSGO` gate.
///
/// Returns `Some(path)` when the engine is found. Returns `None` ONLY when the
/// engine is genuinely absent AND `VERTER_REQUIRE_TSGO` is not set (a no-engine
/// environment may hermetic-skip). When `VERTER_REQUIRE_TSGO` is set and the
/// engine is absent, this panics — a skip in the gate is a vacuous-pass failure.
pub fn engine_or_skip() -> Option<PathBuf> {
    let root = workspace_root();
    match discover_tsgo(&root) {
        Ok(path) => Some(path),
        Err(e) => {
            if std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
                panic!(
                    "VERTER_REQUIRE_TSGO is set but the tsgo engine was not found: {e}. \
                     A skip here would be a vacuous pass."
                );
            }
            eprintln!("[skip] tsgo engine not found ({e}); set VERTER_REQUIRE_TSGO to require it");
            None
        }
    }
}

/// Write a minimal, self-contained TS project into `dir` and return the
/// tsconfig path. Hermetic: depends on nothing outside `dir` (no node_modules,
/// no external corpora). The project has one clean file and is configured for
/// strict type-checking so a deliberate error in an overlay file produces a
/// stable diagnostic code.
pub fn write_fixture_project(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");

    // A real on-disk module the overlay carrier can import.
    std::fs::write(
        src.join("util.ts"),
        "export function double(n: number): number {\n  return n * 2;\n}\n",
    )
    .expect("write util.ts");

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
  "include": ["src/**/*.ts", "src/**/*.tsx"]
}
"#,
    )
    .expect("write tsconfig.json");

    tsconfig
}

/// Forward-slash-normalize a path for the wire (the tsgo `--api` paths and the
/// gate harness both compare on forward slashes).
pub fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
