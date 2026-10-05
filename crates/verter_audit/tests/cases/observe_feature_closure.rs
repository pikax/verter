//! Resolve production features with Cargo, independently of the test binary's features.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolution,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    features: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Resolution {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    pkg: String,
}

fn cargo(root: &Path, args: &[&str]) -> String {
    // Keep toolchain discovery in the repository even for fixture manifests.
    // A fresh runner need not have a rustup default outside the pinned tree.
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .expect("launch Cargo feature resolver");
    assert!(
        output.status.success(),
        "cargo {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Cargo output is UTF-8")
}

fn metadata(root: &Path, target: &str) -> Metadata {
    serde_json::from_str(&cargo(
        root,
        &[
            "metadata",
            "--locked",
            "--all-features",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ],
    ))
    .expect("decode Cargo metadata")
}

// Metadata's resolve.nodes.features unifies the entire workspace, including
// other members' dev-dependencies. It is NOT the compiled production closure.
// Use metadata for feature implications and cargo tree for resolver-2 activation.
fn implied_features(metadata: &Metadata, seeds: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    let packages: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let nodes: BTreeMap<_, _> = metadata
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();
    let mut pending = Vec::new();
    for &(name, feature) in seeds {
        let package = metadata
            .packages
            .iter()
            .find(|p| p.name == name)
            .expect("owner crate exists");
        assert!(
            package.features.contains_key(feature),
            "{name}/{feature} must be declared"
        );
        pending.push((package.id.clone(), feature.to_owned()));
    }
    let mut visited = BTreeSet::new();
    while let Some((id, feature)) = pending.pop() {
        if !visited.insert((id.clone(), feature.clone())) {
            continue;
        }
        let package = packages[id.as_str()];
        for implication in &package.features[&feature] {
            // dep: enables a dependency, not one of its named features. An
            // explicit dependency/feature arm below resolves its named gates.
            if implication.starts_with("dep:") {
                continue;
            }
            if let Some((dependency, feature)) = implication.split_once('/') {
                let dependency = dependency.trim_end_matches('?').replace('-', "_");
                let edge = nodes[id.as_str()]
                    .deps
                    .iter()
                    .find(|d| d.name.replace('-', "_") == dependency)
                    .unwrap_or_else(|| panic!("Cargo must resolve {}/{implication}", package.name));
                pending.push((edge.pkg.clone(), feature.to_owned()));
            } else {
                pending.push((id.clone(), implication.clone()));
            }
        }
    }
    visited
        .into_iter()
        .map(|(id, feature)| (packages[id.as_str()].name.clone(), feature))
        .collect()
}

fn enabled_features(
    root: &Path,
    package: &str,
    target: &str,
    include_dev: bool,
) -> BTreeSet<(String, String)> {
    let output = cargo(
        root,
        &[
            "tree",
            "--locked",
            "-p",
            package,
            "--target",
            target,
            "-e",
            if include_dev {
                "normal,build,dev"
            } else {
                "normal,build"
            },
            "--prefix",
            "none",
            "--format",
            "{p}|{f}",
        ],
    );
    let mut enabled = BTreeSet::new();
    let mut reaches_audit = false;
    for line in output.lines() {
        let (package, features) = line
            .split_once('|')
            .expect("Cargo tree package/feature row");
        let name = package
            .split_whitespace()
            .next()
            .expect("Cargo tree package name");
        reaches_audit |= name == "verter_audit";
        for feature in features
            .trim_end_matches(" (*)")
            .split(',')
            .filter(|f| !f.is_empty())
        {
            enabled.insert((name.to_owned(), feature.to_owned()));
        }
    }
    assert!(reaches_audit, "production closure must reach audit");
    enabled
}

fn violations(
    enabled: &BTreeSet<(String, String)>,
    forbidden: &BTreeSet<(String, String)>,
) -> Vec<String> {
    enabled
        .intersection(forbidden)
        .map(|(package, feature)| format!("{package}/{feature}"))
        .collect()
}

fn host_target(root: &Path) -> String {
    // Cargo's target-aware resolver accepts the host triple without requiring
    // the target's standard library to be installed (metadata/tree do not build).
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .current_dir(root)
        .arg("-vV")
        .output()
        .expect("read host target");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("rustc output is UTF-8")
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .expect("rustc host triple")
}

#[test]
fn observe_feature_closure_is_off_for_production_and_its_dev_unification() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let host = host_target(root);
    for target in [host.as_str(), "wasm32-unknown-unknown"] {
        let metadata = metadata(root, target);
        let mut seeds = vec![
            ("verter_audit", "semantic-observe"),
            ("verter_scheduler", "semantic-observe"),
            ("verter_workspace", "semantic-observe"),
            ("verter_semantic", "semantic-observe"),
            ("verter_compiler", "semantic-observe"),
            ("verter_session", "attribution"),
            ("verter_session", "currency_probe"),
        ];
        // Includes new forwarding owners as they are introduced, and the
        // measurement harness's combined opt-in, without a second allowlist.
        for package in &metadata.packages {
            if package.features.contains_key("semantic-observe") {
                seeds.push((&package.name, "semantic-observe"));
            }
        }
        let forbidden = implied_features(&metadata, &seeds);
        // Cargo features are profile-independent: this same resolver-2
        // closure applies to default/dev, debug, release and no-debug-assertions.
        // Checking cfg(profile) with metadata would falsely claim build proof;
        // the repository's build lanes independently compile those profiles.
        for package in ["verter_napi", "verter_lsp", "verter_wasm", "verter_tsc"] {
            for include_dev in [false, true] {
                let enabled = enabled_features(root, package, target, include_dev);
                let violations = violations(&enabled, &forbidden);
                assert!(violations.is_empty(), "{package} target={target} dev={include_dev}: optional observation features enabled: {violations:?}");
            }
        }
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(production_features: &str, dev_features: Option<&str>) -> Self {
        let fixture = Self(verter_test_support::unique_temp_dir(
            "observe-feature-closure",
        ));
        for package in ["verter_audit", "production"] {
            fs::create_dir_all(fixture.0.join(package).join("src")).unwrap();
            fs::write(fixture.0.join(package).join("src/lib.rs"), "").unwrap();
        }
        fs::write(
            fixture.0.join("Cargo.toml"),
            "[workspace]\nmembers = [\"verter_audit\", \"production\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        fs::write(
            fixture.0.join("verter_audit/Cargo.toml"),
            r#"
[package]
name = "verter_audit"
version = "0.0.0"
edition = "2021"
[features]
default = []
semantic-observe = ["attribution"]
attribution = []
test-support = []
"#,
        )
        .unwrap();
        let mut manifest = format!(
            r#"
[package]
name = "production"
version = "0.0.0"
edition = "2021"
[dependencies]
verter_audit = {{ path = "../verter_audit", default-features = false, features = [{production_features}] }}
"#
        );
        if let Some(features) = dev_features {
            manifest.push_str(&format!("\n[dev-dependencies]\nverter_audit = {{ path = \"../verter_audit\", features = [{features}] }}\n"));
        }
        fs::write(fixture.0.join("production/Cargo.toml"), manifest).unwrap();
        cargo(&fixture.0, &["generate-lockfile", "--offline"]);
        fixture
    }

    fn violations(&self, include_dev: bool) -> Vec<String> {
        let target = host_target(Path::new(env!("CARGO_MANIFEST_DIR")));
        let forbidden = implied_features(
            &metadata(&self.0, &target),
            &[("verter_audit", "semantic-observe")],
        );
        violations(
            &enabled_features(&self.0, "production", &target, include_dev),
            &forbidden,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn observe_feature_closure_rejects_direct_and_implied_manifest_enablement() {
    let direct = Fixture::new("\"semantic-observe\"", None);
    assert_eq!(
        direct.violations(false),
        ["verter_audit/attribution", "verter_audit/semantic-observe"]
    );
    let implied = Fixture::new("\"attribution\"", None);
    assert_eq!(implied.violations(false), ["verter_audit/attribution"]);
    let control = Fixture::new("", Some("\"test-support\""));
    assert!(control.violations(false).is_empty());
    assert!(control.violations(true).is_empty());
}

#[test]
fn observe_feature_closure_rejects_dev_dependency_unification() {
    let fixture = Fixture::new("", Some("\"semantic-observe\""));
    assert!(
        fixture.violations(false).is_empty(),
        "resolver 2 excludes dev features from the production build"
    );
    assert_eq!(
        fixture.violations(true),
        ["verter_audit/attribution", "verter_audit/semantic-observe"]
    );
}
