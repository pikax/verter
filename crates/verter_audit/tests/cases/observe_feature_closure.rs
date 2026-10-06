//! Resolve production features with Cargo, independently of the test binary's features.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// One feature, keyed by the Cargo identity (name, version) of the package
/// that owns it, so distinct versions of one package never share a comparison
/// identity.
type PackageFeature = (String, String, String);

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolution,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    features: BTreeMap<String, Vec<String>>,
    dependencies: Vec<ManifestDependency>,
}

/// The dependency declaration as written in a manifest: what the edge itself
/// requests, which activation-only semantics never show in feature tables.
#[derive(Deserialize)]
struct ManifestDependency {
    name: String,
    features: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
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

fn metadata(root: &Path, target: &str, config: &[String]) -> Metadata {
    let mut args: Vec<String> = [
        "metadata",
        "--locked",
        "--all-features",
        "--format-version",
        "1",
        "--filter-platform",
        target,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    args.extend(config.iter().cloned());
    serde_json::from_str(&cargo(
        root,
        &args.iter().map(|arg| arg.as_str()).collect::<Vec<_>>(),
    ))
    .expect("decode Cargo metadata")
}

// One `cargo tree` closure per `-p` root: the features Cargo actually
// activates there. Activation is the only authority for `dep:` edges,
// edge-requested features and dependency defaults — none of those exist in
// a manifest's feature tables, and metadata's resolve graph unifies the
// entire workspace instead of one root's edges.
fn closure(
    root: &Path,
    package: &str,
    target: &str,
    feature: Option<&str>,
    include_dev: bool,
    config: &[String],
) -> BTreeSet<PackageFeature> {
    let mut args: Vec<String> = [
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
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    if let Some(feature) = feature {
        args.push("--features".into());
        args.push(feature.into());
    }
    args.extend(config.iter().cloned());
    let output = cargo(
        root,
        &args.iter().map(|arg| arg.as_str()).collect::<Vec<_>>(),
    );
    let mut enabled = BTreeSet::new();
    let mut reaches_audit = false;
    for line in output.lines() {
        let (package, features) = line
            .split_once('|')
            .expect("Cargo tree package/feature row");
        let mut identity = package.split_whitespace();
        let name = identity.next().expect("Cargo tree package name");
        // Tree rows print `name vX.Y.Z`; metadata ids carry the bare version.
        let version = identity
            .next()
            .expect("Cargo tree package version")
            .strip_prefix('v')
            .unwrap_or_else(|| panic!("Cargo tree package version: {package}"));
        reaches_audit |= name == "verter_audit";
        for feature in features
            .trim_end_matches(" (*)")
            .split(',')
            .filter(|feature| !feature.is_empty())
        {
            enabled.insert((name.to_owned(), version.to_owned(), feature.to_owned()));
        }
    }
    assert!(reaches_audit, "closure must reach the audit crate");
    enabled
}

// The declarative half: every feature the seed owners' own declarations can
// name. Feature-table implications recurse at any depth, but the features
// requested on a dependency edge — and the dependency's defaults, which only
// activation can confirm — are resolved only for edges a workspace member
// declares. A third-party crate's internal `dep:` arms are not this
// repository's declarations to police, and reading them would pull shared
// library defaults into the forbidden set.
fn declared_closure(
    metadata: &Metadata,
    seeds: &BTreeSet<(String, String)>,
) -> BTreeSet<PackageFeature> {
    let packages: BTreeMap<&str, &Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();
    let nodes: BTreeMap<&str, &Node> = metadata
        .resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let members: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let mut pending = Vec::new();
    for (name, feature) in seeds {
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name == *name)
            .unwrap_or_else(|| panic!("owner crate {name} exists"));
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
        // Seeds may name a feature the owner does not declare (a `default`
        // pushed for an edge whose target has none): no declaration, no
        // implications, and the activation diff drops the row anyway.
        let Some(implications) = package.features.get(&feature) else {
            continue;
        };
        for implication in implications {
            if let Some(dependency) = implication.strip_prefix("dep:") {
                activate_edge(&nodes, &packages, &members, &id, dependency, &mut pending);
                continue;
            }
            if let Some((dependency, feature)) = implication.split_once('/') {
                let dependency = dependency.trim_end_matches('?').replace('-', "_");
                let edge = nodes[id.as_str()]
                    .deps
                    .iter()
                    .find(|dep| dep.name.replace('-', "_") == dependency)
                    .unwrap_or_else(|| panic!("Cargo must resolve {}/{implication}", package.name));
                pending.push((edge.pkg.clone(), feature.to_owned()));
                activate_edge(&nodes, &packages, &members, &id, &dependency, &mut pending);
                continue;
            }
            pending.push((id.clone(), implication.clone()));
        }
    }
    visited
        .into_iter()
        .map(|(id, feature)| {
            let package = packages[id.as_str()];
            (package.name.clone(), package.version.clone(), feature)
        })
        .collect()
}

// Crossing a workspace-declared edge opts into everything the edge can turn
// on: the features the manifest requests on it, and the dependency's
// defaults. Whether the edge really has defaults enabled is settled by the
// activation difference in `forbidden_closure`, which drops seeds Cargo never
// activates; pushing the default unconditionally keeps edge-declared default
// lists from needing activation knowledge here.
fn activate_edge(
    nodes: &BTreeMap<&str, &Node>,
    packages: &BTreeMap<&str, &Package>,
    members: &BTreeSet<&str>,
    owner: &str,
    dependency: &str,
    pending: &mut Vec<(String, String)>,
) {
    let dependency = dependency.replace('-', "_");
    if !members.contains(owner) {
        return;
    }
    let edge = nodes[owner]
        .deps
        .iter()
        .find(|dep| dep.name.replace('-', "_") == dependency)
        .unwrap_or_else(|| panic!("Cargo must resolve dep:{dependency}"));
    pending.push((edge.pkg.clone(), "default".to_owned()));
    let Some(declared) = packages[owner]
        .dependencies
        .iter()
        .find(|dep| dep.name.replace('-', "_") == dependency && dep.kind.is_none())
        .map(|dep| dep.features.clone())
    else {
        return;
    };
    for feature in declared {
        pending.push((edge.pkg.clone(), feature));
    }
}

// The forbidden set is what a seed activates beyond the same root built
// without it, restricted to features the workspace's own declarations name.
// The difference keeps features that are on anyway — shared dependency
// defaults, the owner's ambient edge requests — out of the set while
// `dep:`-edge activation and edge-requested instrumentation stay in.
fn forbidden_closure(
    root: &Path,
    target: &str,
    metadata: &Metadata,
    seeds: &BTreeSet<(String, String)>,
    config: &[String],
) -> BTreeSet<PackageFeature> {
    let mut forbidden = BTreeSet::new();
    for seed @ (package, feature) in seeds {
        let declared = declared_closure(metadata, &BTreeSet::from([seed.clone()]));
        let with = closure(root, package, target, Some(feature), false, config);
        let without = closure(root, package, target, None, false, config);
        let activated: BTreeSet<PackageFeature> = with.difference(&without).cloned().collect();
        forbidden.extend(declared.intersection(&activated).cloned());
    }
    forbidden
}

fn violations(
    enabled: &BTreeSet<PackageFeature>,
    forbidden: &BTreeSet<PackageFeature>,
) -> Vec<String> {
    enabled
        .intersection(forbidden)
        .map(|(name, version, feature)| format!("{name} v{version}/{feature}"))
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
        let metadata = metadata(root, target, &[]);
        let mut seeds: BTreeSet<(String, String)> = [
            ("verter_audit", "semantic-observe"),
            ("verter_scheduler", "semantic-observe"),
            ("verter_workspace", "semantic-observe"),
            ("verter_semantic", "semantic-observe"),
            ("verter_compiler", "semantic-observe"),
            ("verter_session", "attribution"),
            ("verter_session", "currency_probe"),
        ]
        .iter()
        .map(|(name, feature)| (name.to_string(), feature.to_string()))
        .collect();
        // Includes new forwarding owners as they are introduced, and the
        // measurement harness's combined opt-in, without a second allowlist.
        for package in &metadata.packages {
            if package.features.contains_key("semantic-observe") {
                seeds.insert((package.name.clone(), "semantic-observe".into()));
            }
        }
        let forbidden = forbidden_closure(root, target, &metadata, &seeds, &[]);
        // Cargo features are profile-independent: this same resolver-2
        // closure applies to default/dev, debug, release and no-debug-assertions.
        // Checking cfg(profile) with metadata would falsely claim build proof;
        // the repository's build lanes independently compile those profiles.
        for package in ["verter_napi", "verter_lsp", "verter_wasm", "verter_tsc"] {
            for include_dev in [false, true] {
                let enabled = closure(root, package, target, None, include_dev, &[]);
                let violations = violations(&enabled, &forbidden);
                assert!(violations.is_empty(), "{package} target={target} dev={include_dev}: optional observation features enabled: {violations:?}");
            }
        }
    }
}

struct Fixture {
    root: PathBuf,
    // Routes crates.io at an in-fixture registry directory, the one offline
    // source Cargo serves multiple versions of a package from; empty for the
    // fixtures that need no registry.
    config: Vec<String>,
}

fn write_member(root: &Path, name: &str, manifest: &str) {
    let package = root.join(name);
    fs::create_dir_all(package.join("src")).unwrap();
    fs::write(package.join("src/lib.rs"), "").unwrap();
    fs::write(package.join("Cargo.toml"), manifest).unwrap();
}

impl Fixture {
    fn new(production_features: &str, dev_features: Option<&str>) -> Self {
        let root = verter_test_support::unique_temp_dir("observe-feature-closure");
        write_member(
            &root,
            "verter_audit",
            r#"[package]
name = "verter_audit"
version = "0.0.0"
edition = "2021"
[features]
default = []
semantic-observe = ["attribution"]
attribution = []
test-support = []
"#,
        );
        let mut manifest = format!(
            r#"[package]
name = "production"
version = "0.0.0"
edition = "2021"
[dependencies]
verter_audit = {{ path = "../verter_audit", default-features = false, features = [{production_features}] }}"#
        );
        if let Some(features) = dev_features {
            manifest.push_str(&format!(
                "\n[dev-dependencies]\nverter_audit = {{ path = \"../verter_audit\", features = [{features}] }}\n"
            ));
        }
        write_member(&root, "production", &manifest);
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"verter_audit\", \"production\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        let fixture = Self {
            root,
            config: Vec::new(),
        };
        cargo(&fixture.root, &["generate-lockfile", "--offline"]);
        fixture
    }

    /// Observation owner whose `semantic-observe` reaches `collector` through
    /// `implication` (a `dep:` reference or an implicit one) and `audit_edge`.
    /// The registry serves collector 1.0.0 with `collector_default` as its
    /// defaults and a bare 2.0.0; `production_edge` is the production
    /// dependency under test.
    fn with_collector(
        implication: &str,
        audit_edge: &str,
        collector_default: &[&str],
        production_edge: &str,
    ) -> Self {
        let root = verter_test_support::unique_temp_dir("observe-feature-closure");
        for (version, default) in [("1.0.0", collector_default), ("2.0.0", &[][..])] {
            let package = root.join("registry").join(format!("collector-{version}"));
            fs::create_dir_all(package.join("src")).unwrap();
            fs::write(
                package.join("Cargo.toml"),
                format!(
                    r#"[package]
name = "collector"
version = "{version}"
edition = "2021"
[features]
default = [{}]
instrument = []
"#,
                    default
                        .iter()
                        .map(|feature| format!("\"{feature}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .unwrap();
            fs::write(package.join("src/lib.rs"), "").unwrap();
            fs::write(
                package.join(".cargo-checksum.json"),
                r#"{"files":{},"package":null}"#,
            )
            .unwrap();
        }
        write_member(
            &root,
            "verter_audit",
            &format!(
                r#"[package]
name = "verter_audit"
version = "0.0.0"
edition = "2021"
[features]
default = []
semantic-observe = ["{implication}"]
[dependencies]
{audit_edge}
"#
            ),
        );
        write_member(
            &root,
            "production",
            &format!(
                r#"[package]
name = "production"
version = "0.0.0"
edition = "2021"
[dependencies]
verter_audit = {{ path = "../verter_audit", default-features = false }}
{production_edge}
"#
            ),
        );
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"verter_audit\", \"production\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        let fixture = Self {
            config: vec![
                "--config".into(),
                "source.crates-io.replace-with=\"fixture-registry\"".into(),
                "--config".into(),
                format!(
                    "source.fixture-registry.directory='{}'",
                    root.join("registry").display()
                ),
            ],
            root,
        };
        let mut lockfile = vec!["generate-lockfile".to_string(), "--offline".into()];
        lockfile.extend(fixture.config.iter().cloned());
        cargo(
            &fixture.root,
            &lockfile.iter().map(|arg| arg.as_str()).collect::<Vec<_>>(),
        );
        fixture
    }

    fn violations(&self, include_dev: bool) -> Vec<String> {
        let target = host_target(Path::new(env!("CARGO_MANIFEST_DIR")));
        let metadata = metadata(&self.root, &target, &self.config);
        let forbidden = forbidden_closure(
            &self.root,
            &target,
            &metadata,
            &BTreeSet::from([("verter_audit".to_owned(), "semantic-observe".to_owned())]),
            &self.config,
        );
        violations(
            &closure(
                &self.root,
                "production",
                &target,
                None,
                include_dev,
                &self.config,
            ),
            &forbidden,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn observe_feature_closure_rejects_direct_and_implied_manifest_enablement() {
    let direct = Fixture::new("\"semantic-observe\"", None);
    assert_eq!(
        direct.violations(false),
        [
            "verter_audit v0.0.0/attribution",
            "verter_audit v0.0.0/semantic-observe"
        ]
    );
    let implied = Fixture::new("\"attribution\"", None);
    assert_eq!(
        implied.violations(false),
        ["verter_audit v0.0.0/attribution"]
    );
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
        [
            "verter_audit v0.0.0/attribution",
            "verter_audit v0.0.0/semantic-observe"
        ]
    );
}

// An observation feature that reaches an optional dependency activates the
// features that dependency edge requests, and the dependency's defaults when
// the reference is implicit. Production enabling that instrumentation
// independently is the same measurement activation and must be rejected.
#[test]
fn observe_feature_closure_rejects_dependency_edge_activation() {
    let requested = Fixture::with_collector(
        "dep:collector",
        "collector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "collector = { version = \"=1.0.0\", default-features = false, features = [\"instrument\"] }",
    );
    assert_eq!(requested.violations(false), ["collector v1.0.0/instrument"]);
    let defaults = Fixture::with_collector(
        "collector",
        "collector = { version = \"=1.0.0\", optional = true }",
        &["instrument"],
        "collector = { version = \"=1.0.0\" }",
    );
    assert_eq!(
        defaults.violations(false),
        ["collector v1.0.0/default", "collector v1.0.0/instrument"]
    );
    let untouched = Fixture::with_collector(
        "dep:collector",
        "collector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "collector = { version = \"=1.0.0\", default-features = false }",
    );
    assert!(untouched.violations(false).is_empty());
}

// The observation closure names the collector version it would activate.
// An unrelated version of the same package carrying the same feature name is
// outside that closure: instrumentation the opt-in never reaches.
#[test]
fn observe_feature_closure_keys_features_by_package_version() {
    let unrelated = Fixture::with_collector(
        "dep:collector",
        "collector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "collector = { version = \"=2.0.0\", default-features = false, features = [\"instrument\"] }",
    );
    assert!(
        unrelated.violations(false).is_empty(),
        "production enables collector v2; the observation closure names v1"
    );
    let observed = Fixture::with_collector(
        "dep:collector",
        "collector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "collector = { version = \"=1.0.0\", default-features = false, features = [\"instrument\"] }",
    );
    assert_eq!(observed.violations(false), ["collector v1.0.0/instrument"]);
}
