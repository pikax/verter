//! Compiler-layer dependency firewall.
//!
//! Walks the resolved production graph (`cargo metadata --format-version 1
//! --all-features`) and asserts `verter_compiler`'s production closure:
//!
//! * never reaches host/session/query/LSP/NAPI/FFI/WASM crates — those own
//!   TypeInfo execution and host integration, not compiler authority;
//! * still reaches `verter_semantic` and `verter_macro_dto` so the compiler
//!   consumes shared analysis and the neutral macro DTO instead of importing
//!   host TypeInfo types.
//!
//! This is a resolve-graph walk, not a source-text scan. Dev-dependencies
//! are not followed. The walk proves **host/session/transport crate closure
//! only**. It does not prove the runtime compiler owns no in-crate second
//! analyzer, and it does not prove an in-crate generic-versus-framework
//! module split.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;

const COMPILER_PACKAGE: &str = "verter_compiler";

/// Host, session, and transport crates the compiler production graph must
/// not reach. Reaching any of these would let the compiler import host
/// TypeInfo execution or session/transport types. This list is not an
/// in-crate analyzer inventory.
const FORBIDDEN_HOST_AND_ANALYZER: [&str; 7] = [
    "verter_session",
    "verter_session_query",
    "verter_lsp",
    "verter_napi",
    "verter_mcp",
    "verter_ffi",
    "verter_wasm",
];

fn workspace_manifest() -> PathBuf {
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

struct ResolveGraph {
    names: HashMap<String, String>,
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

    fn closure_names(&self, root_name: &str) -> HashSet<String> {
        self.production_closure(root_name)
            .iter()
            .map(|id| self.name_of(id).to_string())
            .collect()
    }
}

#[test]
fn compiler_production_closure_does_not_reach_host_session_or_transport_crates() {
    let graph = ResolveGraph::from_metadata(&workspace_metadata());
    let names = graph.closure_names(COMPILER_PACKAGE);

    let leaks: Vec<&str> = FORBIDDEN_HOST_AND_ANALYZER
        .iter()
        .copied()
        .filter(|forbidden| names.contains(*forbidden))
        .collect();
    assert!(
        leaks.is_empty(),
        "compiler production closure reached host/session/transport crates {leaks:?}; \
         the compiler crate must not depend on session, query, LSP, NAPI, MCP, FFI, \
         or WASM. This assertion does not claim in-crate analyzer absence. \
         Closure names include verter crates: {:?}",
        {
            let mut verter: Vec<_> = names
                .iter()
                .filter(|n| n.starts_with("verter_"))
                .cloned()
                .collect();
            verter.sort();
            verter
        }
    );

    assert!(
        names.contains("verter_semantic"),
        "compiler production closure must still reach shared `verter_semantic` \
         (the one analysis substrate, not a compiler-owned replica)"
    );
    assert!(
        names.contains("verter_macro_dto"),
        "compiler production closure must still reach the dependency-neutral \
         macro DTO instead of importing host TypeInfo types"
    );
}

/// Synthetic graph: a planted host edge is visible to the same walk the live
/// guard uses, so a missing-edge bug cannot make the live assertion vacuously
/// green.
#[test]
fn dependency_walk_detects_a_planted_session_edge() {
    let mut names = HashMap::new();
    names.insert("compiler-id".into(), "verter_compiler".into());
    names.insert("session-id".into(), "verter_session".into());
    names.insert("semantic-id".into(), "verter_semantic".into());
    let mut production_deps = HashMap::new();
    production_deps.insert(
        "compiler-id".into(),
        vec!["session-id".into(), "semantic-id".into()],
    );
    production_deps.insert("session-id".into(), vec![]);
    production_deps.insert("semantic-id".into(), vec![]);
    let graph = ResolveGraph {
        names,
        production_deps,
    };
    let closure = graph.closure_names("verter_compiler");
    assert!(
        closure.contains("verter_session"),
        "planted `verter_session` production edge must be visible; otherwise the \
         live firewall cannot fail. names={closure:?}"
    );
    assert!(closure.contains("verter_semantic"));
}
