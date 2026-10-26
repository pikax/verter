//! The two test tiers in `.config/nextest.toml`. The gate (profiles `default`
//! and `ci`) runs every test except the nightly scale tier, and the `nightly`
//! profile runs exactly that tier. A test is in the tier when one segment of
//! its module path is `nightly_scale`, so membership is structural: moving a
//! test into or out of such a module is the whole change.
//!
//! This binary holds tier tests, so it is the probe. Under each profile,
//! `cargo nextest list` discovers every test of this already-compiled binary
//! (no nested build runs) and reports whether the profile selects it. The
//! expected selection is derived from the discovered names by the module rule
//! alone, independently of the filter expression the configuration spells.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The module name that places a test in the nightly scale tier.
const TIER_MODULE: &str = "nightly_scale";

fn in_tier(name: &str) -> bool {
    // The last segment is the test function; only module segments count.
    let mut segments: Vec<_> = name.split("::").collect();
    segments.pop();
    segments.contains(&TIER_MODULE)
}

fn output(command: &mut Command) -> String {
    let result = command.output().expect("start tier policy probe");
    assert!(
        result.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).expect("UTF-8 policy output")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_gate_runs_everything_but_the_nightly_scale_tier_and_nightly_runs_only_the_tier() {
    let binary = std::env::current_exe().expect("compiled test binary");
    // Listing-only metadata for this binary; manifest-only Cargo metadata and
    // an empty Cargo home, so an extracted archive needs no registry cache.
    let repo = repo_root();
    let temp = tempfile::tempdir().expect("policy probe directory");
    let cargo_home = temp.path().join("cargo-home");
    std::fs::create_dir(&cargo_home).expect("empty Cargo home");
    let (metadata, platform) = std::thread::scope(|scope| {
        let metadata = scope.spawn(|| {
            output(
                Command::new("cargo")
                    .current_dir(&repo)
                    .env("CARGO_HOME", &cargo_home)
                    .args([
                        "metadata",
                        "--no-deps",
                        "--locked",
                        "--offline",
                        "--format-version",
                        "1",
                    ]),
            )
        });
        let platform = output(Command::new("rustc").current_dir(&repo).args([
            "--print",
            "target-libdir",
            "--print",
            "host-tuple",
        ]));
        (metadata.join().expect("Cargo metadata probe"), platform)
    });
    let cargo: serde_json::Value = serde_json::from_str(&metadata).expect("Cargo metadata");
    let package = cargo["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|package| package["name"] == env!("CARGO_PKG_NAME"))
        .expect("compiled test package");
    let build_dir = binary
        .parent()
        .and_then(Path::parent)
        .expect("Cargo profile directory");
    let mut platform = platform.lines();
    let libdir = platform.next().expect("Rust library directory");
    let triple = platform.next().expect("Rust host triple");
    let binaries = serde_json::json!({
        "rust-build-meta": {
            "target-directory": build_dir.parent().expect("Cargo target directory"),
            "base-output-directories": [build_dir.file_name().unwrap().to_str().unwrap()],
            "non-test-binaries": {}, "linked-paths": [],
            "platforms": {
                "host": {
                    "platform": { "triple": triple, "target-features": "unknown" },
                    "libdir": { "status": "available", "path": libdir },
                },
                "targets": [],
            },
        },
        "rust-binaries": {
            "verter_session": {
                "binary-id": "verter_session", "binary-name": "verter_session",
                "package-id": package["id"], "kind": "lib",
                "binary-path": binary, "build-platform": "target",
            },
        },
    });
    std::fs::write(temp.path().join("cargo.json"), metadata).unwrap();
    std::fs::write(temp.path().join("binaries.json"), binaries.to_string()).unwrap();

    // The three listings are independent; run them together.
    let listings: Vec<(&str, Listing)> = std::thread::scope(|scope| {
        let probes: Vec<_> = ["default", "ci", "nightly"]
            .into_iter()
            .map(|profile| {
                let (repo, temp, cargo_home) = (&repo, temp.path(), &cargo_home);
                scope.spawn(move || (profile, nextest_selection(repo, temp, cargo_home, profile)))
            })
            .collect();
        probes
            .into_iter()
            .map(|probe| probe.join().expect("profile probe"))
            .collect()
    });
    let discovered = &listings[0].1.discovered;
    let tier: BTreeSet<_> = discovered
        .iter()
        .filter(|name| in_tier(name))
        .cloned()
        .collect();
    assert!(
        !tier.is_empty(),
        "this binary must hold nightly scale tests to probe the tier selection"
    );
    for name in &tier {
        assert!(
            !listings[0].1.ignored.contains(name),
            "{name}: a nightly scale test runs in its tier, never behind #[ignore]"
        );
    }
    let gate: BTreeSet<_> = discovered.difference(&tier).cloned().collect();
    for (profile, listing) in &listings {
        let selected = &listing.selected;
        assert_eq!(
            &listing.discovered, discovered,
            "profile {profile} discovers the same tests"
        );
        let expected = if *profile == "nightly" { &tier } else { &gate };
        let missing: Vec<_> = expected.difference(selected).collect();
        let extra: Vec<_> = selected.difference(expected).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "profile {profile}: not selected {missing:?}; selected outside its tier {extra:?}"
        );
    }
}

/// One profile's listing of this binary.
struct Listing {
    discovered: BTreeSet<String>,
    ignored: BTreeSet<String>,
    /// With ignored tests included, so that only the profile's filter decides.
    selected: BTreeSet<String>,
}

fn nextest_selection(repo: &Path, temp: &Path, cargo_home: &Path, profile: &str) -> Listing {
    let listing = output(
        Command::new("cargo-nextest")
            .current_dir(repo)
            .env("CARGO_HOME", cargo_home)
            .args(["nextest", "--color", "never", "--config-file"])
            .arg(repo.join(".config/nextest.toml"))
            .args([
                "--user-config-file",
                "none",
                "--profile",
                profile,
                "list",
                "--run-ignored",
                "all",
                "--message-format",
                "json",
                "--cargo-metadata",
            ])
            .arg(temp.join("cargo.json"))
            .arg("--binaries-metadata")
            .arg(temp.join("binaries.json")),
    );
    let listing: serde_json::Value = serde_json::from_str(&listing).expect("nextest list JSON");
    let cases = listing["rust-suites"]["verter_session"]["testcases"]
        .as_object()
        .unwrap_or_else(|| panic!("profile {profile}: no listing for this binary: {listing}"));
    let names = |keep: &dyn Fn(&serde_json::Value) -> bool| -> BTreeSet<String> {
        cases
            .iter()
            .filter(|(_, case)| keep(case))
            .map(|(name, _)| name.clone())
            .collect()
    };
    Listing {
        discovered: names(&|_| true),
        ignored: names(&|case| case["ignored"] == true),
        selected: names(&|case| case["filter-match"]["status"] == "matches"),
    }
}
