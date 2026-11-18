//! The gated engine drives `verter-tsc --content-mapper` through the content
//! mapper protocol: `initialize`, `openProject`, `transform` for a `ts` and a
//! `tsx` carrier, and `closeProject` when the run ends.
//!
//! The project names the mapper in its `contentMappers` entry, so the engine
//! spawns the server itself and reports every diagnostic in the authored
//! `.vue` file. Each carrier with an error has a clean twin beside it. The
//! case SKIPs when `packages/example/node_modules/vue` or the gated engine is
//! absent, or when that engine has no content-mapper host.

use std::path::{Path, PathBuf};
use std::process::Command;

const MAPPER_PACKAGE: &str = "verter-content-mapper";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn resolve_engine() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    verter_tsgo_api::toolchain::discovery::resolve_blocking(&request)
        .ok()
        .map(|resolution| resolution.path)
}

#[cfg(windows)]
fn link_dir(src: &Path, dest: &Path) {
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if !status.is_ok_and(|status| status.success()) {
        let _ = std::os::windows::fs::symlink_dir(src, dest);
    }
}

#[cfg(not(windows))]
fn link_dir(src: &Path, dest: &Path) {
    let _ = std::os::unix::fs::symlink(src, dest);
}

const TS_ERROR: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const count: number = 1\n",
    "</script>\n",
    "<template>\n",
    "  <p>{{ count.missing }}</p>\n",
    "</template>\n",
);

const TS_CLEAN: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const count: number = 1\n",
    "</script>\n",
    "<template>\n",
    "  <p>{{ count.toFixed(1) }}</p>\n",
    "</template>\n",
);

const TSX_ERROR: &str = concat!(
    "<script setup lang=\"tsx\">\n",
    "const label: number = 'text'\n",
    "</script>\n",
    "<template>\n",
    "  <p>{{ label }}</p>\n",
    "</template>\n",
);

const TSX_CLEAN: &str = concat!(
    "<script setup lang=\"tsx\">\n",
    "const label: string = 'text'\n",
    "</script>\n",
    "<template>\n",
    "  <p>{{ label }}</p>\n",
    "</template>\n",
);

/// A project with the four carriers, `vue` linked from the example package
/// and the mapper package naming this crate's binary.
fn materialize() -> Option<(tempfile::TempDir, PathBuf)> {
    let vue = workspace_root()
        .join("packages")
        .join("example")
        .join("node_modules")
        .join("vue");
    if !vue.exists() {
        eprintln!("SKIP: packages/example/node_modules/vue not found; run `pnpm install` first");
        return None;
    }
    let temp = tempfile::TempDir::new().expect("temp dir");
    let root = temp.path().to_path_buf();
    let node_modules = root.join("node_modules");
    std::fs::create_dir_all(node_modules.join(MAPPER_PACKAGE)).expect("mapper package dir");
    link_dir(&vue, &node_modules.join("vue"));
    if !node_modules.join("vue").join("package.json").exists() {
        eprintln!("SKIP: could not link `vue` into the project");
        return None;
    }
    let manifest = serde_json::json!({
        "name": MAPPER_PACKAGE,
        "version": "0.0.0",
        "private": true,
        "typescript": {
            "contentMapper": {
                "exec": [env!("CARGO_BIN_EXE_verter-tsc"), "--content-mapper"],
            },
        },
    });
    std::fs::write(
        node_modules.join(MAPPER_PACKAGE).join("package.json"),
        serde_json::to_string_pretty(&manifest).expect("manifest"),
    )
    .expect("write manifest");
    let tsconfig = serde_json::json!({
        "compilerOptions": {
            "strict": true,
            "noEmit": true,
            "target": "ES2022",
            "module": "ESNext",
            "moduleResolution": "bundler",
            "skipLibCheck": true,
            "types": [],
        },
        "contentMappers": [{ "package": MAPPER_PACKAGE, "extensions": [".vue"] }],
        "include": ["./*.vue"],
    });
    let config = root.join("tsconfig.json");
    std::fs::write(
        &config,
        serde_json::to_string_pretty(&tsconfig).expect("tsconfig"),
    )
    .expect("write tsconfig");
    for (name, source) in [
        ("TsError.vue", TS_ERROR),
        ("TsClean.vue", TS_CLEAN),
        ("TsxError.vue", TSX_ERROR),
        ("TsxClean.vue", TSX_CLEAN),
    ] {
        std::fs::write(root.join(name), source).expect("write carrier");
    }
    Some((temp, config))
}

/// `(file name, line, TS code)` of each `file(line,col): error TSnnnn:` line.
fn diagnostics(output: &str) -> Vec<(String, u32, u32)> {
    let mut rows: Vec<(String, u32, u32)> = output
        .lines()
        .filter_map(|line| {
            let (location, rest) = line.split_once("): error TS")?;
            let (file, position) = location.rsplit_once('(')?;
            let line_number = position.split(',').next()?.parse().ok()?;
            let code = rest.split(':').next()?.parse().ok()?;
            let name = Path::new(file).file_name()?.to_string_lossy().into_owned();
            Some((name, line_number, code))
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn the_engine_checks_ts_and_tsx_carriers_through_the_mapper() {
    let Some(engine) = resolve_engine() else {
        eprintln!("SKIP: no gated tsgo engine");
        return;
    };
    let Some((_temp, config)) = materialize() else {
        return;
    };
    let output = Command::new(&engine)
        .arg("--runExternalCode")
        .arg("-p")
        .arg(&config)
        .args(["--pretty", "false", "--locale", "en"])
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .output()
        .expect("the engine runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if text.contains("runExternalCode") || text.contains("contentMappers") {
        eprintln!("SKIP: the gated engine has no content-mapper host:\n{text}");
        return;
    }
    assert_eq!(
        diagnostics(&text),
        vec![
            ("TsError.vue".to_string(), 5, 2339),
            ("TsxError.vue".to_string(), 2, 2322),
        ],
        "engine output:\n{text}"
    );
}
