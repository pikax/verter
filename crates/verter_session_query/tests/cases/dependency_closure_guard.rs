//! STRUCTURAL dependency-closure firewall for the query-boundary crate.
//!
//! Walks the REAL resolved dependency graph — `cargo metadata
//! --format-version 1 --all-features` (the same resolve cargo builds from,
//! with every feature-gated optional edge activated) — and asserts the
//! production closure (normal + build edges, transitively) of
//! `verter_session_query` never reaches the parser/compiler front-end:
//!
//! * `verter_parser`, `verter_compiler`, `verter_session`,
//!   `verter_type_expr_oxc` — forbidden outright;
//! * every `oxc`-prefixed crate — forbidden outright. `verter_span`, the
//!   workspace-wide span primitive, owns no AST conversion and depends on no
//!   `oxc` crate, so the boundary closure has no sanctioned exception.
//!
//! This is a resolve-graph walk over what cargo actually links, not a
//! source-text scan: adding a forbidden crate to the `Cargo.toml`
//! (plainly, optionally behind ANY feature, or transitively through a new
//! dependency) adds a resolved edge and fails the walk. Dev-dependencies
//! are deliberately NOT followed: they never link into the production
//! library, and this guard's own tooling dev-deps must not self-trip it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;

/// Crates whose presence anywhere in a query-boundary production closure is
/// a firewall breach, regardless of route.
const FORBIDDEN_EVERYWHERE: [&str; 4] = [
    "verter_parser",
    "verter_compiler",
    "verter_session",
    "verter_type_expr_oxc",
];

fn workspace_manifest() -> PathBuf {
    // tests/ lives at crates/verter_session_query/, two levels below the
    // workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates_dir| crates_dir.parent())
        .expect("crate dir must sit two levels below the workspace root")
        .join("Cargo.toml")
}

fn workspace_metadata() -> serde_json::Value {
    let output = Command::new(env!("CARGO"))
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--all-features")
        .arg("--locked")
        .arg("--manifest-path")
        .arg(workspace_manifest())
        .output()
        .expect("cargo metadata must spawn");
    assert!(
        output.status.success(),
        "cargo metadata --all-features must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata must emit valid JSON")
}

/// One resolved production dependency edge: `from` package id → `to`
/// package id. Dev edges are excluded at construction.
struct ResolveGraph {
    /// Package id → package name, for every package in the resolve.
    names: HashMap<String, String>,
    /// Package id → production-dependency package ids (normal + build
    /// kinds only).
    production_deps: HashMap<String, Vec<String>>,
}

impl ResolveGraph {
    fn from_metadata(metadata: &serde_json::Value) -> Self {
        let mut names = HashMap::new();
        for package in metadata["packages"]
            .as_array()
            .expect("metadata carries a packages array")
        {
            names.insert(
                package["id"]
                    .as_str()
                    .expect("package id is a string")
                    .to_string(),
                package["name"]
                    .as_str()
                    .expect("package name is a string")
                    .to_string(),
            );
        }

        let mut production_deps: HashMap<String, Vec<String>> = HashMap::new();
        let nodes = metadata["resolve"]["nodes"]
            .as_array()
            .expect("metadata carries a resolve graph (run without --no-deps)");
        for node in nodes {
            let id = node["id"]
                .as_str()
                .expect("resolve node id is a string")
                .to_string();
            let mut deps = Vec::new();
            for dep in node["deps"].as_array().expect("resolve node deps array") {
                let is_production = dep["dep_kinds"]
                    .as_array()
                    .expect("resolved dep carries dep_kinds")
                    .iter()
                    .any(|kind| match kind["kind"].as_str() {
                        // `null` = a normal dependency; `build` = a
                        // build-dependency. Both link into production.
                        None => true,
                        Some("build") => true,
                        Some(_) => false,
                    });
                if is_production {
                    deps.push(
                        dep["pkg"]
                            .as_str()
                            .expect("resolved dep pkg id is a string")
                            .to_string(),
                    );
                }
            }
            production_deps.insert(id, deps);
        }

        Self {
            names,
            production_deps,
        }
    }

    fn id_of(&self, package_name: &str) -> &str {
        let mut ids = self
            .names
            .iter()
            .filter(|(_, name)| name.as_str() == package_name)
            .map(|(id, _)| id.as_str());
        let id = ids
            .next()
            .unwrap_or_else(|| panic!("package `{package_name}` must exist in the resolve graph"));
        assert!(
            ids.next().is_none(),
            "package name `{package_name}` must be unambiguous in the resolve graph"
        );
        id
    }

    fn name_of(&self, id: &str) -> &str {
        self.names
            .get(id)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("resolve node `{id}` must have a package entry"))
    }

    /// BFS over production edges from `root_name`, returning every reached
    /// package id (including the root).
    fn production_closure(&self, root_name: &str) -> HashSet<String> {
        let root = self.id_of(root_name).to_string();
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        seen.insert(root.clone());
        queue.push_back(root);
        while let Some(id) = queue.pop_front() {
            let deps = self
                .production_deps
                .get(&id)
                .unwrap_or_else(|| panic!("resolve graph must carry a node for `{id}`"));
            for dep in deps {
                if seen.insert(dep.clone()) {
                    queue.push_back(dep.clone());
                }
            }
        }
        seen
    }
}

/// Asserts the full firewall contract over one root crate's production
/// closure. `extra_forbidden` names crates forbidden for this root beyond
/// the everywhere-forbidden set. `required_reachable` is the per-root
/// non-vacuity canary: crates the root GENUINELY reaches through real
/// (code-used) dependencies, so a broken walk that reached nothing cannot
/// pass — it must name the root's actual reach, never a generic list.
fn assert_firewalled_closure(
    graph: &ResolveGraph,
    root: &str,
    extra_forbidden: &[&str],
    required_reachable: &[&str],
) {
    let closure = graph.production_closure(root);
    let closure_names: HashSet<&str> = closure.iter().map(|id| graph.name_of(id)).collect();

    // Non-vacuity: a broken walk that reached nothing must not pass.
    for &required in required_reachable {
        assert!(
            closure_names.contains(required),
            "{root}: closure walk looks broken — expected `{required}` in the production \
             closure; got {closure_names:?}"
        );
    }

    for forbidden in FORBIDDEN_EVERYWHERE.iter().chain(extra_forbidden) {
        assert!(
            !closure_names.contains(forbidden),
            "{root}: FIREWALL BREACH — `{forbidden}` is reachable in the production \
             dependency closure; the query boundary must not reach the parser/compiler \
             front-end"
        );
    }

    // No oxc-prefixed package may appear anywhere in the closure.
    for id in &closure {
        let name = graph.name_of(id);
        assert!(
            !name.starts_with("oxc"),
            "{root}: FIREWALL BREACH — oxc-prefixed package `{name}` is reachable in              the production dependency closure"
        );
    }
}

#[test]
fn query_boundary_closure_excludes_parser_compiler_front_end() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);

    // The query crate: everything in FORBIDDEN_EVERYWHERE plus every oxc
    // crate. The non-vacuity canary names the root's GENUINE reach:
    // `verter_session_query` code-uses `verter_type_expr` (the port's
    // `AuthoredBodyLocator` / `TypeExpr` / `TypeParam`) and reaches
    // `verter_span` transitively through it.
    assert_firewalled_closure(
        &graph,
        "verter_session_query",
        &[],
        &["verter_type_expr", "verter_span"],
    );
}

/// The walk itself must be discriminating: `verter_session` DOES reach the
/// front-end (it owns the parser-facing machinery), so the same walk over
/// its closure must SEE those crates. If the closure walk ever went blind
/// (empty deps, dropped edges, name mismatches), this canary fails before
/// a blind firewall pass could mask a breach.
#[test]
fn closure_walk_sees_front_end_crates_from_verter_session() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);

    let closure = graph.production_closure("verter_session");
    let closure_names: HashSet<&str> = closure.iter().map(|id| graph.name_of(id)).collect();

    for expected in ["verter_parser", "verter_compiler", "verter_type_expr_oxc"] {
        assert!(
            closure_names.contains(expected),
            "walk canary: `verter_session`'s production closure must contain `{expected}`; \
             the closure walk has gone blind (got {} names)",
            closure_names.len()
        );
    }
    assert!(
        closure_names.iter().any(|name| name.starts_with("oxc")),
        "walk canary: `verter_session`'s production closure must contain oxc crates"
    );
}
