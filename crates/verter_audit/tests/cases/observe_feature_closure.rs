//! Resolve production features with Cargo, independently of the test binary's features.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[allow(clippy::duplicate_mod)]
#[path = "../../../verter_session/tests/cases/support/map_in_parallel.rs"]
mod map_in_parallel;

/// One feature, keyed by the full Cargo package id of the package that owns
/// it, so distinct versions or sources of one package never share a
/// comparison identity.
type PackageFeature = (String, String);

thread_local! {
    static TREE_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
/// `name` is the dependency package's real name; `rename` is the alias the
/// manifest table keys it under (also how Cargo's resolve graph names the
/// edge), and `kind` separates normal from build declarations.
#[derive(Deserialize)]
struct ManifestDependency {
    name: String,
    #[serde(default)]
    rename: Option<String>,
    features: Vec<String>,
    optional: bool,
    uses_default_features: bool,
    target: Option<String>,
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
    #[serde(default)]
    dep_kinds: Vec<DepKind>,
}

#[derive(Deserialize)]
struct DepKind {
    kind: Option<String>,
    target: Option<String>,
}

impl Dependency {
    /// Cargo never resolves a `dep:` reference onto a dev edge; the guard
    /// polices normal and build declarations only.
    fn activates(&self) -> bool {
        self.dep_kinds
            .iter()
            .any(|kind| kind.kind.as_deref() != Some("dev"))
    }
}

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root above the crate manifest")
}

fn cargo(root: &Path, args: &[&str]) -> String {
    // Keep toolchain discovery in the repository even for fixture manifests.
    // A fresh runner need not have a rustup default outside the pinned tree.
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"));
    // A fixture resolves only path members and its in-fixture directory
    // registry, so it gets a private Cargo home. Cargo's package-cache lock is
    // per home, and the shared one is held by any concurrent Cargo process
    // resolving the real workspace, for the whole download on a cold registry.
    if root != repository_root() {
        command.env("CARGO_HOME", root.join("cargo-home"));
    }
    let output = command.output().expect("launch Cargo feature resolver");
    assert!(
        output.status.success(),
        "cargo {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Cargo output is UTF-8")
}

// Unfiltered metadata: the resolve graph must be a superset of every tree
// the guard walks, including host-side build edges that `--filter-platform`
// would drop while `cargo tree --target` still shows them. Declared rows it
// adds for other platforms never survive the per-target activation
// intersection in `forbidden_closure`.
fn metadata(root: &Path, config: &[String]) -> Metadata {
    resolve_metadata(root, config, true)
}

fn resolve_metadata(root: &Path, config: &[String], locked: bool) -> Metadata {
    let mut args: Vec<String> = ["metadata", "--all-features", "--format-version", "1"]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    if locked {
        args.push("--locked".into());
    } else {
        // Fresh fixtures have only staged path/registry dependencies with
        // exact versions. Metadata creates their lockfile in the same call;
        // all subsequent activation trees still require that lockfile.
        args.push("--offline".into());
    }
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
// entire workspace instead of one root's edges. Rows print as
// `depth name vX.Y.Z [source]|features`; the display name and version only
// narrow the row down to its metadata package, and the resolve edge from the
// parent row pins the full Cargo package id, so equal name/version pairs
// from different sources stay distinct identities.
fn closure(
    root: &Path,
    package: &str,
    target: &str,
    feature: Option<&str>,
    include_dev: bool,
    config: &[String],
    metadata: &Metadata,
) -> BTreeSet<PackageFeature> {
    TREE_RUNS.set(TREE_RUNS.get() + 1);
    let mut args: Vec<String> = [
        "tree",
        "--locked",
        "--offline",
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
        "depth",
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
    let mut candidates: BTreeMap<(String, String), Vec<&str>> = BTreeMap::new();
    for package in &metadata.packages {
        candidates
            .entry((package.name.clone(), package.version.clone()))
            .or_default()
            .push(package.id.as_str());
    }
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for node in &metadata.resolve.nodes {
        edges
            .entry(node.id.as_str())
            .or_default()
            .extend(node.deps.iter().map(|dep| dep.pkg.as_str()));
    }
    let mut enabled = BTreeSet::new();
    let mut reaches_audit = false;
    let mut ancestry: Vec<(usize, String)> = Vec::new();
    for line in output.lines() {
        let digits = line
            .find(|c: char| !c.is_ascii_digit())
            .expect("tree row depth");
        let depth: usize = line[..digits].parse().expect("tree row depth");
        let (display, features) = line[digits..]
            .split_once('|')
            .expect("Cargo tree package/feature row");
        let mut identity = display.split_whitespace();
        let name = identity.next().expect("Cargo tree package name");
        // Tree rows print `name vX.Y.Z`; metadata ids carry the bare version.
        let version = identity
            .next()
            .expect("Cargo tree package version")
            .strip_prefix('v')
            .unwrap_or_else(|| panic!("Cargo tree package version: {display}"));
        reaches_audit |= name == "verter_audit";
        while ancestry.last().is_some_and(|(seen, _)| *seen >= depth) {
            ancestry.pop();
        }
        let mut matching: Vec<&str> = candidates
            .get(&(name.to_owned(), version.to_owned()))
            .expect("metadata knows every tree package")
            .clone();
        if matching.len() > 1 {
            if let Some((_, parent)) = ancestry.last() {
                // The tree only walks activated edges, which the resolve graph
                // resolved before activation: the parent's edge set disambiguates
                // same-named packages where the display cannot. A unique display
                // needs no help — resolve also omits a member's dev self-edge,
                // which the tree still prints as its own `(*)` row.
                matching.retain(|id| {
                    edges
                        .get(parent.as_str())
                        .is_some_and(|set| set.contains(id))
                });
            }
        }
        let [id] = matching[..] else {
            panic!("one Cargo identity for {name} v{version} under its parent");
        };
        ancestry.push((depth, id.to_owned()));
        for feature in features
            .trim_end_matches(" (*)")
            .split(',')
            .filter(|feature| !feature.is_empty())
        {
            enabled.insert((id.to_owned(), feature.to_owned()));
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
//
// Rows come back in two classes. Firm rows — feature-table implications and
// the features a crossed edge requests — are guaranteed active whenever the
// seed is, including when the owner enables the seed by default. Activation
// rows are the `default` pushes for crossed edges, which only Cargo's
// activation difference can confirm or drop.
fn declared_closure(
    metadata: &Metadata,
    seeds: &BTreeSet<(String, String)>,
) -> (BTreeSet<PackageFeature>, BTreeSet<PackageFeature>) {
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
        pending.push((package.id.clone(), feature.to_owned(), true));
    }
    let mut firm = BTreeSet::new();
    let mut activation = BTreeSet::new();
    let mut visited = BTreeSet::new();
    while let Some((id, feature, is_firm)) = pending.pop() {
        if !visited.insert((id.clone(), feature.clone())) {
            continue;
        }
        if is_firm {
            firm.insert((id.clone(), feature.clone()));
        } else {
            activation.insert((id.clone(), feature.clone()));
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
                pending.push((edge.pkg.clone(), feature.to_owned(), true));
                activate_edge(&nodes, &packages, &members, &id, &dependency, &mut pending);
                continue;
            }
            pending.push((id.clone(), implication.clone(), true));
        }
    }
    (firm, activation)
}

// Crossing a workspace-declared edge opts into everything the edge can turn
// on: the features the manifest requests on it — matched under the table key
// Cargo resolves the edge by (the rename when the dependency is renamed) and
// the declaration kind the edge resolved for, normal or build — and the
// dependency's defaults. A `dep:` arm activates every same-named edge, so a
// dependency declared as both normal and build contributes both targets.
// Whether a target really has defaults enabled is settled by the activation
// difference in `forbidden_closure`; pushing the default unconditionally
// keeps edge-declared default lists from needing activation knowledge here.
fn activate_edge(
    nodes: &BTreeMap<&str, &Node>,
    packages: &BTreeMap<&str, &Package>,
    members: &BTreeSet<&str>,
    owner: &str,
    dependency: &str,
    pending: &mut Vec<(String, String, bool)>,
) {
    let dependency = dependency.replace('-', "_");
    if !members.contains(owner) {
        return;
    }
    let edges: Vec<&Dependency> = nodes[owner]
        .deps
        .iter()
        .filter(|dep| dep.name.replace('-', "_") == dependency && dep.activates())
        .collect();
    assert!(!edges.is_empty(), "Cargo must resolve dep:{dependency}");
    for edge in edges {
        for kind in edge
            .dep_kinds
            .iter()
            .map(|dep| dep.kind.as_deref())
            .collect::<BTreeSet<_>>()
        {
            if kind == Some("dev") {
                continue;
            }
            if let Some(declared) = packages[owner].dependencies.iter().find(|dep| {
                dep.rename.as_deref().unwrap_or(&dep.name).replace('-', "_") == dependency
                    && dep.kind.as_deref() == kind
            }) {
                for feature in &declared.features {
                    pending.push((edge.pkg.clone(), feature.clone(), true));
                }
            }
        }
        pending.push((edge.pkg.clone(), "default".to_owned(), false));
    }
}

/// Defaults on unconditional mandatory paths from the selected root are active
/// in its without-seed tree. This is a lower bound only: optional/target-gated
/// paths remain Cargo's responsibility. Default implications stay in the firm
/// declaration set; only the default activation rows can be subtracted here.
fn mandatory_defaults(metadata: &Metadata, root: &str) -> BTreeSet<PackageFeature> {
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
    let root = metadata
        .packages
        .iter()
        .find(|p| p.name == root)
        .expect("seed owner exists");
    let mut defaults = BTreeSet::new();
    if root.features.contains_key("default") {
        defaults.insert((root.id.clone(), "default".to_owned()));
    }
    let mut visited = BTreeSet::new();
    let mut pending = vec![root.id.as_str()];
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        for edge in &nodes[id].deps {
            for kind in &edge.dep_kinds {
                if kind.kind.as_deref() == Some("dev") || kind.target.is_some() {
                    continue;
                }
                let declarations: Vec<_> = packages[id]
                    .dependencies
                    .iter()
                    .filter(|dep| {
                        dep.target.is_none()
                            && dep.kind == kind.kind
                            && dep.rename.as_deref().unwrap_or(&dep.name).replace('-', "_")
                                == edge.name.replace('-', "_")
                    })
                    .collect();
                // Cargo permits target-specific declarations of one alias.
                // Only a nonempty, wholly unconditional mandatory set proves
                // this exact resolved package is reached without the seed.
                if declarations.is_empty() || declarations.iter().any(|dep| dep.optional) {
                    continue;
                }
                pending.push(edge.pkg.as_str());
                if declarations.iter().all(|dep| dep.uses_default_features)
                    && packages[edge.pkg.as_str()].features.contains_key("default")
                {
                    defaults.insert((edge.pkg.clone(), "default".to_owned()));
                }
            }
        }
    }
    defaults
}

// The forbidden set is what the seed's declarations can turn on, restricted
// to features Cargo actually activates with the seed. Firm rows stay forbidden
// even when the without-seed closure already contains them. Activation rows
// require the with/without difference, after removing defaults proved active
// through unconditional mandatory paths. Default implications remain firm.
fn forbidden_closure(
    root: &Path,
    target: &str,
    metadata: &Metadata,
    seeds: &BTreeSet<(String, String)>,
    config: &[String],
    enabled: &BTreeSet<PackageFeature>,
    defaults: &mut BTreeMap<String, BTreeSet<PackageFeature>>,
) -> BTreeSet<PackageFeature> {
    let mut forbidden = BTreeSet::new();
    for seed @ (package, feature) in seeds {
        let (declared, activation) = declared_closure(metadata, &BTreeSet::from([seed.clone()]));
        let activation: BTreeSet<_> = activation
            .difference(&mandatory_defaults(metadata, package))
            .cloned()
            .collect();
        // The original forbidden set is a subset of these two declared sets.
        // A seed disjoint from every production closure cannot affect any
        // verdict. Keep full package identities and let Cargo settle every
        // potentially overlapping activation, including weak/default edges.
        if declared.is_disjoint(enabled) && activation.is_disjoint(enabled) {
            continue;
        }
        let without =
            if !activation.is_disjoint(enabled) {
                Some(defaults.entry(package.clone()).or_insert_with(|| {
                    closure(root, package, target, None, false, config, metadata)
                }))
            } else {
                None
            };
        // Only Cargo proves which defaults are active without the seed. If
        // all relevant activation rows are already present and no firm row
        // overlaps production, the enabled-seed tree cannot affect a verdict.
        if declared.is_disjoint(enabled)
            && without.as_ref().is_some_and(|without| {
                activation
                    .intersection(enabled)
                    .all(|row| without.contains(row))
            })
        {
            continue;
        }
        let with = closure(
            root,
            package,
            target,
            Some(feature),
            false,
            config,
            metadata,
        );
        forbidden.extend(declared.intersection(&with).cloned());
        if let Some(without) = without {
            let activated: BTreeSet<PackageFeature> = with.difference(without).cloned().collect();
            forbidden.extend(activation.intersection(&activated).cloned());
        }
    }
    forbidden
}

fn violations(
    metadata: &Metadata,
    enabled: &BTreeSet<PackageFeature>,
    forbidden: &BTreeSet<PackageFeature>,
) -> Vec<String> {
    let packages: BTreeMap<&str, &Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();
    enabled
        .intersection(forbidden)
        .map(|(id, feature)| {
            let package = packages[id.as_str()];
            format!("{} v{}/{feature}", package.name, package.version)
        })
        .collect()
}

fn host_target(root: &Path) -> String {
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HOST.get_or_init(|| read_host_target(root)).clone()
}

fn read_host_target(root: &Path) -> String {
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
    let root = repository_root();
    let host = host_target(root);
    let metadata = metadata(root, &[]);
    for target in [host.as_str(), "wasm32-unknown-unknown"] {
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
        let requests: Vec<_> = ["verter_napi", "verter_lsp", "verter_wasm", "verter_tsc"]
            .into_iter()
            .flat_map(|package| [false, true].map(|dev| (package, dev)))
            .collect();
        let production = map_in_parallel::map_in_parallel(&requests, |&(package, include_dev)| {
            let before = TREE_RUNS.get();
            let enabled = closure(root, package, target, None, include_dev, &[], &metadata);
            (package, include_dev, enabled, TREE_RUNS.get() - before)
        });
        let mut enabled_union = BTreeSet::new();
        let mut defaults = BTreeMap::new();
        for (package, include_dev, enabled, work) in &production {
            TREE_RUNS.set(TREE_RUNS.get() + work);
            enabled_union.extend(enabled.iter().cloned());
            if !include_dev {
                defaults.insert(package.to_string(), enabled.clone());
            }
        }
        let forbidden = forbidden_closure(
            root,
            target,
            &metadata,
            &seeds,
            &[],
            &enabled_union,
            &mut defaults,
        );
        // Cargo features are profile-independent: this same resolver-2
        // closure applies to default/dev, debug, release and no-debug-assertions.
        // Checking cfg(profile) with metadata would falsely claim build proof;
        // the repository's build lanes independently compile those profiles.
        for (package, include_dev, enabled, _) in production {
            let violations = violations(&metadata, &enabled, &forbidden);
            assert!(violations.is_empty(), "{package} target={target} dev={include_dev}: optional observation features enabled: {violations:?}");
        }
    }
    eprintln!("feature closure work: {} Cargo tree runs", TREE_RUNS.get());
}

struct Fixture {
    root: PathBuf,
    metadata: std::sync::OnceLock<Metadata>,
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
        Self::with_audit_default(
            "",
            &format!(
                "verter_audit = {{ path = \"../verter_audit\", default-features = false, features = [{production_features}] }}"
            ),
            dev_features,
        )
    }

    /// Observation owner whose `default` lists `audit_default`; the
    /// production edge is written verbatim so fixtures choose whether the
    /// owner's defaults flow into production.
    fn with_audit_default(
        audit_default: &str,
        production_edge: &str,
        dev_features: Option<&str>,
    ) -> Self {
        let root = verter_test_support::unique_temp_dir("observe-feature-closure");
        write_member(
            &root,
            "verter_audit",
            &format!(
                r#"[package]
name = "verter_audit"
version = "0.0.0"
edition = "2021"
[features]
default = [{audit_default}]
semantic-observe = ["attribution"]
attribution = []
test-support = []
"#
            ),
        );
        let mut manifest = format!(
            r#"[package]
name = "production"
version = "0.0.0"
edition = "2021"
[dependencies]
{production_edge}"#
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
            metadata: std::sync::OnceLock::new(),
            root,
            config: Vec::new(),
        };
        fixture
            .metadata
            .get_or_init(|| resolve_metadata(&fixture.root, &fixture.config, false));
        fixture
    }

    /// Observation owner whose `semantic-observe` reaches `collector` through
    /// `implication` (a `dep:` reference or an implicit one) and the edges in
    /// `audit_edges` (full `[dependencies]`/`[build-dependencies]` sections,
    /// so fixtures choose the declaration kind and any rename). The registry
    /// serves collector 1.0.0 with `collector_default` as its defaults and a
    /// bare 2.0.0; `vendor_collector` adds a path-source collector 1.0.0
    /// outside the declared members for same-name/version different-source
    /// controls; `production_edges` is the production dependency under test.
    fn with_collector(
        implication: &str,
        audit_edges: &str,
        collector_default: &[&str],
        production_edges: &str,
        vendor_collector: bool,
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
quiet = []
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
        if vendor_collector {
            let package = root.join("vendor").join("collector");
            fs::create_dir_all(package.join("src")).unwrap();
            fs::write(
                package.join("Cargo.toml"),
                r#"[package]
name = "collector"
version = "1.0.0"
edition = "2021"
[features]
default = []
instrument = []
"#,
            )
            .unwrap();
            fs::write(package.join("src/lib.rs"), "").unwrap();
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
{audit_edges}
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
{production_edges}
"#
            ),
        );
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"verter_audit\", \"production\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        let config = vec![
            "--config".into(),
            "source.crates-io.replace-with=\"fixture-registry\"".into(),
            "--config".into(),
            format!(
                "source.fixture-registry.directory='{}'",
                root.join("registry").display()
            ),
        ];
        let fixture = Self {
            metadata: std::sync::OnceLock::new(),
            root,
            config,
        };
        fixture
            .metadata
            .get_or_init(|| resolve_metadata(&fixture.root, &fixture.config, false));
        fixture
    }

    fn violations(&self, include_dev: bool) -> Vec<String> {
        let target = host_target(Path::new(env!("CARGO_MANIFEST_DIR")));
        let metadata = self.metadata.get().expect("fixture metadata prepared");
        let enabled = closure(
            &self.root,
            "production",
            &target,
            None,
            include_dev,
            &self.config,
            metadata,
        );
        let forbidden = forbidden_closure(
            &self.root,
            &target,
            metadata,
            &BTreeSet::from([("verter_audit".to_owned(), "semantic-observe".to_owned())]),
            &self.config,
            &enabled,
            &mut BTreeMap::new(),
        );
        violations(metadata, &enabled, &forbidden)
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
    let mandatory = Fixture::with_collector(
        "collector/quiet",
        "[dependencies]\ncollector = { version = \"=1.0.0\" }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\" }",
        false,
    );
    TREE_RUNS.set(0);
    assert!(mandatory.violations(false).is_empty());
    assert_eq!(
        TREE_RUNS.get(),
        1,
        "mandatory defaults are active independently of the observation seed"
    );
    let mandatory_instrument = Fixture::with_collector(
        "collector/quiet",
        "[dependencies]\ncollector = { version = \"=1.0.0\" }",
        &["instrument"],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\" }",
        false,
    );
    assert_eq!(
        mandatory_instrument.violations(false),
        ["collector v1.0.0/instrument"],
        "default implications remain firm even when the default itself needs no activation tree"
    );
    let requested = Fixture::with_collector(
        "dep:collector",
        "[dependencies]\ncollector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\", default-features = false, features = [\"instrument\"] }",
        false,
    );
    assert_eq!(requested.violations(false), ["collector v1.0.0/instrument"]);
    let defaults = Fixture::with_collector(
        "collector",
        "[dependencies]\ncollector = { version = \"=1.0.0\", optional = true }",
        &["instrument"],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\" }",
        false,
    );
    assert_eq!(
        defaults.violations(false),
        ["collector v1.0.0/default", "collector v1.0.0/instrument"]
    );
    let untouched = Fixture::with_collector(
        "dep:collector",
        "[dependencies]\ncollector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\", default-features = false }",
        false,
    );
    assert!(untouched.violations(false).is_empty());
}

// The observation opt-in may live on an optional build dependency: `dep:`
// arms activate build edges too, and Cargo applies the edge's requests on
// the build side. Production enabling that instrumentation on its own build
// edge is the same measurement activation and must be rejected.
#[test]
fn observe_feature_closure_rejects_build_dependency_edge_activation() {
    let fixture = Fixture::with_collector(
        "dep:collector",
        "[build-dependencies]\ncollector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\n[build-dependencies]\ncollector = { version = \"=1.0.0\", default-features = false, features = [\"instrument\"] }",
        false,
    );
    assert_eq!(fixture.violations(false), ["collector v1.0.0/instrument"]);
}

// A renamed dependency edge resolves under its manifest key, not the package
// name behind it: `dep:meter` must find the declaration that renamed
// `collector` to `meter`, or the edge's requested instrumentation silently
// drops out of the forbidden set.
#[test]
fn observe_feature_closure_rejects_renamed_dependency_edge_activation() {
    let fixture = Fixture::with_collector(
        "dep:meter",
        "[dependencies]\nmeter = { package = \"collector\", version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\", default-features = false, features = [\"instrument\"] }",
        false,
    );
    assert_eq!(fixture.violations(false), ["collector v1.0.0/instrument"]);
}

// The observation closure names the Cargo package identity it would
// activate. An unrelated version of the same package — or an unrelated
// source carrying the same name and version — is outside that closure:
// instrumentation the opt-in never reaches.
#[test]
fn observe_feature_closure_keys_features_by_package_identity() {
    let unrelated = Fixture::with_collector(
        "dep:collector",
        "[dependencies]\ncollector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=2.0.0\", default-features = false, features = [\"instrument\"] }",
        false,
    );
    TREE_RUNS.set(0);
    assert!(
        unrelated.violations(false).is_empty(),
        "production enables collector v2; the observation closure names v1"
    );
    assert_eq!(
        TREE_RUNS.get(),
        1,
        "disjoint package identities need only the production tree, never seed activation trees"
    );
    let observed = Fixture::with_collector(
        "dep:collector",
        "[dependencies]\ncollector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { version = \"=1.0.0\", default-features = false, features = [\"instrument\"] }",
        false,
    );
    assert_eq!(observed.violations(false), ["collector v1.0.0/instrument"]);
    let split = Fixture::with_collector(
        "dep:collector",
        "[dependencies]\ncollector = { version = \"=1.0.0\", optional = true, default-features = false, features = [\"instrument\"] }",
        &[],
        "[dependencies]\nverter_audit = { path = \"../verter_audit\", default-features = false }\ncollector = { path = \"../vendor/collector\", default-features = false, features = [\"instrument\"] }",
        true,
    );
    assert!(
        split.violations(false).is_empty(),
        "production enables the path-source collector; the observation closure names the registry one"
    );
}

// The guard's promise is default-off capture: an owner that turns the seed
// on through its own defaults must not launder the feature out of the
// forbidden set via the with/without activation difference.
#[test]
fn observe_feature_closure_rejects_owner_default_activation() {
    let defaulted = Fixture::with_audit_default(
        "\"semantic-observe\"",
        "verter_audit = { path = \"../verter_audit\" }",
        None,
    );
    assert_eq!(
        defaulted.violations(false),
        [
            "verter_audit v0.0.0/attribution",
            "verter_audit v0.0.0/semantic-observe"
        ]
    );
    let control = Fixture::with_audit_default(
        "\"semantic-observe\"",
        "verter_audit = { path = \"../verter_audit\", default-features = false }",
        None,
    );
    assert!(
        control.violations(false).is_empty(),
        "production opts out of the owner defaults; nothing observation-shaped is active"
    );
}
