//! The oracle toolchain record: the exact TypeScript engine every recorded
//! checker column and every oracle snapshot was measured against, pinned by
//! CONTENT rather than by version string alone.
//!
//! `oracle_toolchain.json` (beside this module) names the `typescript`
//! package version, each supported platform's
//! `@typescript/typescript-<os>-<arch>` package, and per platform the SHA-256
//! of the engine binary (`lib/tsc[.exe]`) and of every `lib/*.d.ts` it ships.
//! Three pins are bound to it here: the oracle harness pin
//! (`identity::TSGO_VERSION`), the workspace `typescript` devDependency, and
//! the platform package actually installed under `node_modules`, whose bytes
//! are re-digested and compared. The engine is never spawned — the record is
//! checked from files alone, so the default gate stays tsgo-free.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
const RECORD_REL: &str = "src/typeinfo/typeinfo_tests/oracle_toolchain.json";

#[derive(Deserialize)]
struct ToolchainRecord {
    typescript: String,
    engine_version_report: String,
    platforms: BTreeMap<String, PlatformRecord>,
}

#[derive(Deserialize)]
struct PlatformRecord {
    package: String,
    version: String,
    executable: String,
    executable_sha256: String,
    lib_d_ts: Vec<LibFile>,
}

#[derive(Deserialize)]
struct LibFile {
    path: String,
    sha256: String,
}

fn record() -> ToolchainRecord {
    let path = Path::new(MANIFEST_DIR).join(RECORD_REL);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read the oracle toolchain record {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse the oracle toolchain record {}: {e}", path.display()))
}

/// The record key for the host this test binary runs on (`<npm os>-<npm cpu>`),
/// or `None` on a host with no TypeScript platform package.
fn host_platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("macos", "x86_64") => Some("darwin-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("windows", "aarch64") => Some("win32-arm64"),
        ("windows", "x86_64") => Some("win32-x64"),
        _ => None,
    }
}

/// The installed platform package root (the directory holding its
/// `package.json`), walking every ancestor `node_modules` of this crate in the
/// flat, pnpm-store and `typescript`-nested layouts.
fn installed_platform_package(key: &str, version: &str) -> Option<PathBuf> {
    let package_dir = Path::new("@typescript").join(format!("typescript-{key}"));
    for ancestor in Path::new(MANIFEST_DIR).ancestors() {
        let node_modules = ancestor.join("node_modules");
        if !node_modules.is_dir() {
            continue;
        }
        let candidates = [
            node_modules.join(&package_dir),
            node_modules
                .join(".pnpm")
                .join(format!("@typescript+typescript-{key}@{version}"))
                .join("node_modules")
                .join(&package_dir),
            node_modules
                .join("typescript")
                .join("node_modules")
                .join(&package_dir),
        ];
        if let Some(root) = candidates
            .into_iter()
            .find(|root| root.join("package.json").is_file())
        {
            return Some(root);
        }
    }
    None
}

fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// The record's version pins agree with the harness pin, with the workspace
/// `typescript` devDependency, and with themselves (every platform entry is
/// the same version, named after its key, and carries a sorted, non-empty lib
/// inventory) — the three places a version can be spelled cannot drift apart.
#[test]
fn oracle_toolchain_record_names_the_harness_pin() {
    let record = record();
    assert_eq!(
        record.typescript,
        super::oracle::identity::TSGO_VERSION,
        "the toolchain record and the oracle harness must pin the same engine version"
    );
    assert_eq!(
        record.engine_version_report,
        format!("Version {}", super::oracle::identity::TSGO_VERSION),
        "the record's `--version` report must be the harness pin's spelling"
    );

    let package_json = Path::new(MANIFEST_DIR).join("../../package.json");
    let root: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&package_json)
            .unwrap_or_else(|e| panic!("read {}: {e}", package_json.display())),
    )
    .expect("parse the workspace package.json");
    assert_eq!(
        root["devDependencies"]["typescript"].as_str(),
        Some(record.typescript.as_str()),
        "the workspace `typescript` devDependency must be the recorded toolchain version"
    );

    assert!(
        !record.platforms.is_empty(),
        "the record must carry at least one platform entry"
    );
    for (key, platform) in &record.platforms {
        assert_eq!(platform.version, record.typescript, "{key}: version");
        assert_eq!(
            platform.package,
            format!("@typescript/typescript-{key}"),
            "{key}: package name"
        );
        assert!(
            !platform.lib_d_ts.is_empty(),
            "{key}: the lib inventory must not be empty"
        );
        let paths: Vec<&str> = platform.lib_d_ts.iter().map(|f| f.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            paths, sorted,
            "{key}: the lib inventory must be sorted and duplicate-free"
        );
        for file in &platform.lib_d_ts {
            assert!(
                file.path.starts_with("lib/") && file.path.ends_with(".d.ts"),
                "{key}: `{}` is not a `lib/*.d.ts` entry",
                file.path
            );
            assert!(
                file.sha256.starts_with("sha256:") && file.sha256.len() == 7 + 64,
                "{key}: `{}` carries a malformed digest `{}`",
                file.path,
                file.sha256
            );
        }
    }
}

/// The platform package installed under `node_modules` is byte-for-byte the
/// recorded engine: its `package.json` version, its `lib/tsc[.exe]` and the
/// complete set of `lib/*.d.ts` files it ships all digest to the recorded
/// values. A host with no platform package mapping, or one where the package
/// is not installed, skips with an explicit reason — nothing else does.
#[test]
fn oracle_toolchain_record_matches_the_installed_engine_bytes() {
    let record = record();
    let Some(key) = host_platform_key() else {
        eprintln!(
            "SKIP oracle_toolchain_record_matches_the_installed_engine_bytes: no TypeScript \
             platform package exists for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        return;
    };
    let platform = record.platforms.get(key).unwrap_or_else(|| {
        panic!("the toolchain record has no entry for the host platform `{key}`")
    });
    let Some(root) = installed_platform_package(key, &record.typescript) else {
        eprintln!(
            "SKIP oracle_toolchain_record_matches_the_installed_engine_bytes: `{}` is not \
             installed under any ancestor node_modules (run `pnpm install --frozen-lockfile`)",
            platform.package
        );
        return;
    };

    let package_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("package.json"))
            .expect("read the platform package.json"),
    )
    .expect("parse the platform package.json");
    assert_eq!(
        package_json["name"].as_str(),
        Some(platform.package.as_str()),
        "{}: installed package name",
        root.display()
    );
    assert_eq!(
        package_json["version"].as_str(),
        Some(record.typescript.as_str()),
        "{}: the installed platform package is not the recorded toolchain version",
        root.display()
    );

    assert_eq!(
        sha256_of(&root.join(&platform.executable)),
        platform.executable_sha256,
        "{}: the installed engine binary `{}` differs from the recorded toolchain",
        root.display(),
        platform.executable
    );

    let lib = root.join("lib");
    let mut on_disk: Vec<(String, String)> = std::fs::read_dir(&lib)
        .unwrap_or_else(|e| panic!("read {}: {e}", lib.display()))
        .map(|entry| entry.expect("lib entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".d.ts"))
        })
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (format!("lib/{name}"), sha256_of(&path))
        })
        .collect();
    on_disk.sort();
    let recorded: Vec<(String, String)> = platform
        .lib_d_ts
        .iter()
        .map(|f| (f.path.clone(), f.sha256.clone()))
        .collect();
    assert_eq!(
        on_disk,
        recorded,
        "{}: the installed `lib/*.d.ts` set differs from the recorded toolchain",
        root.display()
    );
}
