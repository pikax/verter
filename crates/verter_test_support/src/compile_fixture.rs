use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_manifest() -> (PathBuf, serde_json::Value) {
    let executable = std::env::current_exe().expect("locate the running test");
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
