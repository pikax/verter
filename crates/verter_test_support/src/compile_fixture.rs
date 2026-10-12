use std::fmt::Write;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_manifest() -> &'static (PathBuf, serde_json::Value) {
    static MANIFEST: OnceLock<(PathBuf, serde_json::Value)> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let executable = std::env::current_exe().expect("locate the running test");
        fixture_manifest_for(&executable)
    })
}

fn file_digest(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap_or_else(|error| panic!("read {path:?}: {error}"));
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).expect("read fixture build content");
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

fn fixture_manifest_for(executable: &Path) -> (PathBuf, serde_json::Value) {
    for root in executable.ancestors().skip(2) {
        let bundle = root.join("compile-fixtures");
        let Ok(bytes) = fs::read(bundle.join("manifest.json")) else {
            continue;
        };
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).expect("fixture artifact manifest must be valid JSON");
        let relative = executable
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        if manifest["tests"]
            .as_array()
            .expect("fixture manifest names its test binaries")
            .iter()
            .any(|test| test.as_str() == Some(&relative))
        {
            assert_eq!(
                manifest["test_digests"][&relative].as_str(),
                Some(file_digest(executable).as_str()),
                "mismatched fixture metadata for rebuilt test; run scripts/compile-fixture-artifacts.mjs prepare"
            );
            for (name, artifact) in manifest["artifacts"]
                .as_object()
                .expect("fixture manifest binds its external metadata")
            {
                assert_eq!(Path::new(name).file_name().unwrap(), name.as_str());
                let expected = artifact["sha256"].as_str().expect("artifact digest");
                assert_eq!(
                    file_digest(&bundle.join(name)),
                    expected,
                    "changed bundled metadata {name}"
                );
                let build_path = artifact["build_path"]
                    .as_str()
                    .expect("relative artifact build path");
                let build_path = root.join(build_path);
                // Archives can omit the original compiler metadata: their bundle
                // is bound to the exact archived executable. In a local target,
                // also detect metadata-only rebuilds with identical binary bytes.
                if build_path.is_file() {
                    assert_eq!(file_digest(&build_path), expected, "mismatched fixture metadata after rebuild; run scripts/compile-fixture-artifacts.mjs prepare");
                }
            }
            return (bundle, manifest);
        }
    }
    panic!(
        "missing or mismatched fixture metadata; run scripts/compile-fixture-artifacts.mjs prepare"
    );
}

/// Type-check a foreign-crate fixture against metadata from this test's build.
///
/// Prepare with `node scripts/compile-fixture-artifacts.mjs prepare` and the
/// same nextest build options. The archive builder packages that same metadata
/// beside the relocated test binary. No Cargo invocation or dependency build
/// occurs during the test, and unrelated cached artifacts are never selected.
pub fn compile_fixture(name: &str, source: &str) -> Output {
    let (bundle, manifest) = fixture_manifest();
    assert_eq!(Path::new(name).file_name().unwrap(), name);
    let scratch = Scratch(crate::unique_temp_dir("verter-compile-fixture"));
    fs::create_dir_all(&scratch.0).expect("create fixture scratch directory");
    let input = scratch.0.join(name);
    fs::write(&input, source).expect("write the foreign-crate fixture");
    let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    let working_dir = std::env::current_dir().expect("locate the test workspace");
    let workspace = working_dir
        .ancestors()
        .find(|path| path.join("rust-toolchain.toml").is_file())
        .expect("the test workspace pins its compiler");
    command
        .current_dir(workspace)
        .arg(&input)
        .args([
            "--edition=2021",
            "--crate-type=bin",
            "--emit=metadata",
            "--error-format=json",
            "--color=never",
            "--diagnostic-width=140",
            "-Awarnings",
        ])
        .arg("--out-dir")
        .arg(&scratch.0)
        .arg("--remap-path-prefix")
        .arg(format!("{}=tests/ui/fixtures", scratch.0.display()))
        .arg("-L")
        .arg(format!("dependency={}", bundle.display()));
    for (name, filename) in manifest["externs"]
        .as_object()
        .expect("fixture manifest names its external crates")
    {
        let filename = filename.as_str().expect("an artifact name is a string");
        assert_eq!(Path::new(filename).file_name().unwrap(), filename);
        let artifact = bundle.join(filename);
        assert!(
            artifact.is_file(),
            "missing fixture dependency {artifact:?}"
        );
        command
            .arg("--extern")
            .arg(format!("{name}={}", artifact.display()));
    }
    command.output().expect("launch rustc for the fixture")
}

/// Source backing an imported compiler span, packaged by the same build as
/// the metadata. An archive consumer need not have the builder's Cargo cache.
pub fn fixture_diagnostic_source(path: &str) -> String {
    let (bundle, manifest) = fixture_manifest();
    let filename = manifest["sources"][path]
        .as_str()
        .expect("the diagnostic source is packaged");
    assert_eq!(Path::new(filename).file_name().unwrap(), filename);
    fs::read_to_string(bundle.join(filename)).expect("read the packaged diagnostic source")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilt_test_rejects_metadata_at_the_same_filename() {
        let scratch = Scratch(crate::unique_temp_dir("fixture-build-binding"));
        let deps = scratch.0.join("debug/deps");
        let bundle = scratch.0.join("compile-fixtures");
        fs::create_dir_all(&deps).unwrap();
        fs::create_dir_all(&bundle).unwrap();
        let executable = deps.join("main-current");
        fs::write(&executable, b"before rebuild").unwrap();
        // SHA-256 of the original executable, independently produced by Node.
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "tests": ["debug/deps/main-current"],
                "test_digests": {
                    "debug/deps/main-current": "94d1d322c35dfa1926e9aa2a286b50eae8ea4ddfe0fe94d3890b068bf8cf1264"
                },
                "artifacts": {}
            })).unwrap(),
        ).unwrap();
        assert_eq!(fixture_manifest_for(&executable).0, bundle);
        let metadata = deps.join("libcurrent.rmeta");
        let packaged = bundle.join("libcurrent.rmeta");
        fs::write(&metadata, b"private metadata").unwrap();
        fs::copy(&metadata, &packaged).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
        manifest["artifacts"] = serde_json::json!({
            "libcurrent.rmeta": {
                "sha256": file_digest(&metadata),
                "build_path": "debug/deps/libcurrent.rmeta"
            }
        });
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(fixture_manifest_for(&executable).0, bundle);
        fs::write(&metadata, b"public metadata").unwrap();
        assert!(
            std::panic::catch_unwind(|| fixture_manifest_for(&executable)).is_err(),
            "metadata-only rebuilds must also invalidate the bundle"
        );
        fs::write(&metadata, b"private metadata").unwrap();
        // An extracted archive omits the original metadata and keeps the
        // content-bound bundle and executable together.
        fs::remove_file(&metadata).unwrap();
        assert_eq!(fixture_manifest_for(&executable).0, bundle);
        fs::write(&executable, b"after rebuild").unwrap();
        assert!(
            std::panic::catch_unwind(|| fixture_manifest_for(&executable)).is_err(),
            "a rebuilt test must reject the old bundle even when Cargo reuses its filename"
        );
    }
}
