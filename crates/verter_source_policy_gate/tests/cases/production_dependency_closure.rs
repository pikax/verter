//! Architecture guard: the PRODUCTION dependency closure of the layered
//! engine, source, query-boundary, resolution, wire-protocol and audit crates
//! never reaches a crate above or beside its layer.
//!
//! The closure is computed by Cargo itself, never by reading manifests or
//! source text: `cargo metadata --format-version 1 --all-features` resolves
//! the graph, and the walk keeps only `resolve.nodes[].deps[]` edges whose
//! `dep_kinds` contain a normal (`kind: null`) or `build` entry. Dev-only
//! edges are dropped at EVERY hop, so test scaffolding may depend on anything,
//! while every optional dependency is in scope because all features are on.
//!
//! [`RULES`] is the single rule table. The planted fixtures below generate
//! tiny path-only workspaces in a temporary directory (`--offline`, so no
//! network) and prove each rule rejects a direct, a transitive, a
//! feature-gated optional and a build-only edge, and accepts a dev-only edge.
//! The live tests apply the same table to this repository; a rule whose root
//! crate is absent from the workspace FAILS rather than passing vacuously.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// How a forbidden crate is recognised in a resolved package name.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// Exactly this package name.
    Exact(&'static str),
    /// Every package whose name starts with this prefix (the `oxc*` family,
    /// including `oxc_span`).
    Prefix(&'static str),
}

impl Target {
    fn matches(self, package: &str) -> bool {
        match self {
            Target::Exact(name) => package == name,
            Target::Prefix(prefix) => package.starts_with(prefix),
        }
    }
}

/// One forbidden class: the targets plus why the layer may not reach them.
#[derive(Debug, Clone, Copy)]
struct Forbidden {
    class: &'static str,
    targets: &'static [Target],
}

/// One layering rule: `root`'s production closure reaches none of `forbidden`.
#[derive(Debug, Clone, Copy)]
struct ClosureRule {
    root: &'static str,
    forbidden: &'static [Forbidden],
}

const OXC: Forbidden = Forbidden {
    class: "OXC syntax front-end",
    targets: &[Target::Prefix("oxc")],
};
const PARSER: Forbidden = Forbidden {
    class: "carrier parser",
    targets: &[Target::Exact("verter_parser")],
};
const COMPILER: Forbidden = Forbidden {
    class: "compiler",
    targets: &[Target::Exact("verter_compiler")],
};
const SESSION: Forbidden = Forbidden {
    class: "session host",
    targets: &[Target::Exact("verter_session")],
};
const WORKSPACE: Forbidden = Forbidden {
    class: "workspace/VFS host",
    targets: &[Target::Exact("verter_workspace")],
};
const PROTOCOL: Forbidden = Forbidden {
    class: "transport protocol",
    targets: &[Target::Exact("verter_protocol")],
};
const SEMANTIC: Forbidden = Forbidden {
    class: "syntax-coupled semantic analysis",
    targets: &[Target::Exact("verter_semantic")],
};
const SEMANTIC_SOURCE: Forbidden = Forbidden {
    class: "semantic source lowering",
    targets: &[Target::Exact("verter_semantic_source")],
};
const RESOLUTION: Forbidden = Forbidden {
    class: "module resolution",
    targets: &[Target::Exact("verter_resolution")],
};
const TYPE_ENGINE: Forbidden = Forbidden {
    class: "type engine",
    targets: &[Target::Exact("verter_type_engine")],
};
const SCHEDULER: Forbidden = Forbidden {
    class: "concrete scheduler",
    targets: &[Target::Exact("verter_scheduler")],
};
/// The external-TypeScript provider crates: the tsgo `--api` client, the
/// tsserver/tsgo backend sessions, and the editor-facing tsgo relay.
const PROVIDERS: Forbidden = Forbidden {
    class: "external-TypeScript provider",
    targets: &[
        Target::Exact("verter_tsgo_api"),
        Target::Exact("verter_type_runtime"),
        Target::Exact("verter_relay_shim"),
    ],
};

/// THE rule table.
const RULES: &[ClosureRule] = &[
    ClosureRule {
        root: "verter_type_engine",
        forbidden: &[
            OXC,
            PARSER,
            COMPILER,
            SESSION,
            WORKSPACE,
            PROTOCOL,
            SEMANTIC,
            SEMANTIC_SOURCE,
            RESOLUTION,
            SCHEDULER,
            PROVIDERS,
        ],
    },
    ClosureRule {
        root: "verter_semantic_source",
        forbidden: &[
            TYPE_ENGINE,
            SESSION,
            WORKSPACE,
            PROTOCOL,
            PROVIDERS,
            COMPILER,
            SCHEDULER,
        ],
    },
    ClosureRule {
        root: "verter_session_query",
        forbidden: &[RESOLUTION, SEMANTIC, PARSER, OXC],
    },
    ClosureRule {
        root: "verter_resolution",
        forbidden: &[SEMANTIC, PARSER, OXC, SESSION, WORKSPACE, SCHEDULER],
    },
    ClosureRule {
        root: "verter_protocol",
        forbidden: &[SESSION],
    },
    ClosureRule {
        root: "verter_audit",
        forbidden: &[PROTOCOL, SESSION],
    },
];

fn rule(root: &str) -> &'static ClosureRule {
    RULES
        .iter()
        .find(|rule| rule.root == root)
        .unwrap_or_else(|| panic!("no closure rule for `{root}`"))
}

/// The production edges of a resolved Cargo graph.
struct ProductionGraph {
    names: HashMap<String, String>,
    edges: HashMap<String, Vec<String>>,
    workspace_members: Vec<String>,
}

fn is_production_edge(dep: &Value) -> bool {
    dep["dep_kinds"]
        .as_array()
        .expect("`dep_kinds` array (cargo >= 1.41)")
        .iter()
        .any(|kind| match &kind["kind"] {
            Value::Null => true,
            Value::String(kind) => kind == "build",
            other => panic!("unexpected dep kind {other}"),
        })
}

impl ProductionGraph {
    fn from_metadata(metadata: &Value) -> Self {
        let names = metadata["packages"]
            .as_array()
            .expect("`packages` array")
            .iter()
            .map(|package| {
                (
                    package["id"].as_str().expect("package id").to_owned(),
                    package["name"].as_str().expect("package name").to_owned(),
                )
            })
            .collect();
        let edges = metadata["resolve"]["nodes"]
            .as_array()
            .expect("`resolve.nodes` array")
            .iter()
            .map(|node| {
                let deps = node["deps"]
                    .as_array()
                    .expect("`deps` array")
                    .iter()
                    .filter(|dep| is_production_edge(dep))
                    .map(|dep| dep["pkg"].as_str().expect("dep pkg").to_owned())
                    .collect();
                (node["id"].as_str().expect("node id").to_owned(), deps)
            })
            .collect();
        let workspace_members = metadata["workspace_members"]
            .as_array()
            .expect("`workspace_members` array")
            .iter()
            .map(|id| id.as_str().expect("member id").to_owned())
            .collect();
        Self {
            names,
            edges,
            workspace_members,
        }
    }

    fn name(&self, id: &str) -> &str {
        self.names
            .get(id)
            .unwrap_or_else(|| panic!("resolve node `{id}` has no package"))
    }

    fn package_names(&self) -> BTreeSet<&str> {
        self.names.values().map(String::as_str).collect()
    }

    /// Breadth-first walk of the production closure of `root`, returning
    /// each reached package with the shortest dependency chain to it.
    fn production_closure(&self, root: &str) -> Vec<Vec<String>> {
        let mut chains: HashMap<&str, Vec<String>> = HashMap::new();
        let mut queue = VecDeque::from([root]);
        chains.insert(root, vec![self.name(root).to_owned()]);
        let mut reached = Vec::new();
        while let Some(id) = queue.pop_front() {
            let chain = chains[id].clone();
            for dep in self.edges.get(id).into_iter().flatten() {
                if chains.contains_key(dep.as_str()) {
                    continue;
                }
                let mut next = chain.clone();
                next.push(self.name(dep).to_owned());
                reached.push(next.clone());
                chains.insert(dep, next);
                queue.push_back(dep);
            }
        }
        reached
    }

    /// Every forbidden package `rule.root` reaches, as human-readable lines.
    /// `Err` when the root crate is not a workspace member — absence is a
    /// failure, never a vacuous pass.
    fn violations(&self, rule: &ClosureRule) -> Result<Vec<String>, String> {
        let roots: Vec<&String> = self
            .workspace_members
            .iter()
            .filter(|id| self.name(id) == rule.root)
            .collect();
        if roots.is_empty() {
            return Err(format!(
                "`{}` is not a workspace member, so its production closure cannot be checked",
                rule.root
            ));
        }
        let mut lines = Vec::new();
        for root in roots {
            for chain in self.production_closure(root) {
                let reached = chain.last().expect("non-empty chain");
                for forbidden in rule.forbidden {
                    if forbidden.targets.iter().any(|t| t.matches(reached)) {
                        lines.push(format!(
                            "`{}` production closure reaches forbidden {} crate `{reached}` via {}",
                            rule.root,
                            forbidden.class,
                            chain.join(" -> ")
                        ));
                    }
                }
            }
        }
        lines.sort();
        Ok(lines)
    }

    /// The workspace members `root`'s production closure reaches, by name.
    fn workspace_closure(&self, root: &str) -> Result<BTreeSet<String>, String> {
        let members: BTreeSet<&str> = self
            .workspace_members
            .iter()
            .map(|id| self.name(id))
            .collect();
        let roots: Vec<&String> = self
            .workspace_members
            .iter()
            .filter(|id| self.name(id) == root)
            .collect();
        if roots.is_empty() {
            return Err(format!("`{root}` is not a workspace member"));
        }
        Ok(roots
            .into_iter()
            .flat_map(|id| self.production_closure(id))
            .map(|chain| chain.last().expect("non-empty chain").clone())
            .filter(|name| members.contains(name.as_str()))
            .collect())
    }

    /// Every difference between the workspace crates `root` reaches and
    /// `expected`, in both directions: a reached crate missing from the
    /// list, and a listed crate no longer reached.
    fn exact_closure_drift(&self, root: &str, expected: &[&str]) -> Result<Vec<String>, String> {
        let reached = self.workspace_closure(root)?;
        let expected: BTreeSet<&str> = expected.iter().copied().collect();
        let mut drift: Vec<String> = reached
            .iter()
            .filter(|name| !expected.contains(name.as_str()))
            .map(|name| format!("`{root}` reaches workspace crate `{name}`, which its exact closure does not list"))
            .collect();
        drift.extend(
            expected
                .iter()
                .filter(|name| !reached.contains(**name))
                .map(|name| {
                    format!("`{root}` lists workspace crate `{name}` but no longer reaches it")
                }),
        );
        Ok(drift)
    }

    fn check(&self, rule: &ClosureRule) -> Result<(), String> {
        let violations = self.violations(rule)?;
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations.join("\n"))
        }
    }
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO")))
}

fn cargo_metadata(manifest: &Path, extra: &[&str]) -> ProductionGraph {
    let out = Command::new(cargo())
        .args(["metadata", "--format-version", "1", "--all-features"])
        .args(extra)
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("run `cargo metadata`");
    assert!(
        out.status.success(),
        "`cargo metadata` failed for {}: {}",
        manifest.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let metadata: Value = serde_json::from_slice(&out.stdout).expect("cargo metadata JSON");
    ProductionGraph::from_metadata(&metadata)
}

// ---------------------------------------------------------------------------
// Live workspace.
// ---------------------------------------------------------------------------

fn live_graph() -> ProductionGraph {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("Cargo.toml");
    cargo_metadata(&manifest, &["--locked"])
}

/// Rule roots that are not members of this workspace. A listed root is
/// checked for ABSENCE (its live check proves it is still missing); a root
/// that exists must not be listed, so its live closure check can never be
/// skipped by staying on this list.
const ABSENT_ROOTS: &[&str] = &[];

fn assert_live_rule(root: &str) {
    let graph = live_graph();
    if ABSENT_ROOTS.contains(&root) {
        let error = graph
            .check(rule(root))
            .expect_err("a root listed in ABSENT_ROOTS must not be a workspace member");
        assert!(
            error.contains("is not a workspace member"),
            "`{root}` is listed in ABSENT_ROOTS but its check failed differently: {error}"
        );
        return;
    }
    if let Err(message) = graph.check(rule(root)) {
        panic!("{message}");
    }
}

#[test]
fn session_query_production_closure_stays_resolution_semantic_and_syntax_free() {
    assert_live_rule("verter_session_query");
}

#[test]
fn resolution_production_closure_stays_syntax_semantic_and_host_free() {
    assert_live_rule("verter_resolution");
}

#[test]
fn protocol_production_closure_stays_host_free() {
    assert_live_rule("verter_protocol");
}

#[test]
fn audit_production_closure_stays_protocol_and_host_free() {
    assert_live_rule("verter_audit");
}

#[test]
fn type_engine_production_closure_stays_syntax_host_and_provider_free() {
    assert_live_rule("verter_type_engine");
}

#[test]
fn semantic_source_production_closure_stays_engine_host_and_provider_free() {
    assert_live_rule("verter_semantic_source");
}

/// The exact workspace crates each extracted layer's production closure
/// reaches. The forbidden lists above catch the known-dangerous crates; this
/// list catches everything else, so a new same-layer dependency of the type
/// engine or the source layer is a reviewed edit here, never an automatic pass.
/// External packages are not listed: they are reviewed through the manifests.
const EXACT_WORKSPACE_CLOSURES: &[(&str, &[&str])] = &[
    (
        "verter_type_engine",
        &[
            "verter_analysis_inputs",
            "verter_analyzer_mint",
            "verter_audit",
            "verter_debug_assert",
            "verter_ecma",
            "verter_execution",
            "verter_identity",
            "verter_language",
            "verter_macro_dto",
            "verter_no_storedspan",
            "verter_no_storedspan_derive",
            "verter_no_typeexpr",
            "verter_no_typeexpr_derive",
            "verter_session_query",
            "verter_span",
            "verter_type_expr",
        ],
    ),
    (
        "verter_semantic_source",
        &[
            "verter_analysis_inputs",
            "verter_analyzer_mint",
            "verter_audit",
            "verter_css_syntax",
            "verter_debug_assert",
            "verter_ecma",
            "verter_execution",
            "verter_identity",
            "verter_language",
            "verter_macro_dto",
            "verter_no_storedspan",
            "verter_no_storedspan_derive",
            "verter_no_typeexpr",
            "verter_no_typeexpr_derive",
            "verter_parser",
            "verter_semantic",
            "verter_session_query",
            "verter_span",
            "verter_type_expr",
            "verter_type_expr_oxc",
        ],
    ),
];

#[test]
fn extracted_layers_reach_exactly_their_listed_workspace_crates() {
    let graph = live_graph();
    let mut drift = Vec::new();
    for (root, expected) in EXACT_WORKSPACE_CLOSURES {
        drift.extend(
            graph
                .exact_closure_drift(root, expected)
                .unwrap_or_else(|e| panic!("{e}")),
        );
    }
    assert!(
        drift.is_empty(),
        "{}",
        drift.join(
            "
"
        )
    );
}

#[test]
fn exact_closure_reports_unlisted_and_stale_workspace_crates() {
    let graph = fixture_graph(
        &[
            "verter_type_engine",
            "verter_identity",
            "verter_span",
            "verter_new_leaf",
        ],
        &[
            ("verter_type_engine", "verter_identity", Kind::Normal),
            ("verter_identity", "verter_new_leaf", Kind::Normal),
        ],
    );
    let drift = graph
        .exact_closure_drift("verter_type_engine", &["verter_identity", "verter_span"])
        .expect("root is a fixture member");
    assert_eq!(
        drift,
        vec![
            "`verter_type_engine` reaches workspace crate `verter_new_leaf`, which its exact closure does not list".to_owned(),
            "`verter_type_engine` lists workspace crate `verter_span` but no longer reaches it".to_owned(),
        ]
    );
    assert_eq!(
        graph.exact_closure_drift(
            "verter_type_engine",
            &["verter_identity", "verter_new_leaf"]
        ),
        Ok(Vec::new())
    );
}

// ---------------------------------------------------------------------------
// Planted fixtures.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Normal,
    Build,
    Dev,
    /// A normal dependency marked `optional`, enabled only by the named
    /// feature.
    OptionalBehind(&'static str),
}

/// `(dependent, dependency, kind)` edges of a fixture workspace; every named
/// crate becomes an empty path-only library member.
type Edge = (&'static str, &'static str, Kind);

/// Write a path-only fixture workspace into a fresh temporary directory and
/// resolve it offline.
fn fixture_graph(crates: &[&'static str], edges: &[Edge]) -> ProductionGraph {
    fixture_graph_with(crates, edges, &[])
}

/// [`fixture_graph`] plus raw manifest text appended to named crates, for
/// dependency shapes the edge list cannot express.
fn fixture_graph_with(
    crates: &[&'static str],
    edges: &[Edge],
    raw: &[(&'static str, &'static str)],
) -> ProductionGraph {
    let dir = tempfile::tempdir().expect("fixture tempdir");
    let root = dir.path();
    let mut members = String::new();
    for name in crates {
        let _ = write!(members, "\"{name}\", ");
    }
    std::fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nresolver = \"2\"\nmembers = [{members}]\n"),
    )
    .expect("write fixture workspace manifest");
    for name in crates {
        let mut sections: [(&str, String); 3] = [
            ("dependencies", String::new()),
            ("build-dependencies", String::new()),
            ("dev-dependencies", String::new()),
        ];
        let mut features = String::new();
        for (_, to, kind) in edges.iter().filter(|(from, ..)| from == name) {
            let section = match kind {
                Kind::Normal | Kind::OptionalBehind(_) => 0,
                Kind::Build => 1,
                Kind::Dev => 2,
            };
            let optional = if let Kind::OptionalBehind(feature) = kind {
                let _ = writeln!(features, "{feature} = [\"dep:{to}\"]");
                ", optional = true"
            } else {
                ""
            };
            let _ = writeln!(
                sections[section].1,
                "{to} = {{ path = \"../{to}\"{optional} }}"
            );
        }
        let mut manifest = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n"
        );
        for (header, body) in &sections {
            let _ = write!(manifest, "\n[{header}]\n{body}");
        }
        let _ = write!(manifest, "\n[features]\n{features}");
        for (_, text) in raw.iter().filter(|(crate_name, _)| crate_name == name) {
            let _ = write!(manifest, "\n{text}\n");
        }
        let src = root.join(name).join("src");
        std::fs::create_dir_all(&src).expect("create fixture crate");
        std::fs::write(root.join(name).join("Cargo.toml"), manifest).expect("write fixture crate");
        std::fs::write(src.join("lib.rs"), "").expect("write fixture lib.rs");
    }
    cargo_metadata(&root.join("Cargo.toml"), &["--offline"])
}

fn assert_rejects(root: &str, graph: &ProductionGraph, forbidden: &str) -> String {
    match graph.check(rule(root)) {
        Ok(()) => panic!("`{root}` rule accepted a planted edge to `{forbidden}`"),
        Err(message) => {
            assert!(
                message.contains(&format!("crate `{forbidden}` via")),
                "`{root}` rejection does not name `{forbidden}`:\n{message}"
            );
            message
        }
    }
}

/// One direct planted violation per rule and forbidden class. The forbidden
/// names are spelled independently of [`RULES`], so a misspelled table entry
/// fails here instead of silently matching nothing.
const PLANTED_DIRECT: &[(&str, &str)] = &[
    ("verter_type_engine", "oxc_span"),
    ("verter_type_engine", "verter_parser"),
    ("verter_type_engine", "verter_compiler"),
    ("verter_type_engine", "verter_session"),
    ("verter_type_engine", "verter_workspace"),
    ("verter_type_engine", "verter_protocol"),
    ("verter_type_engine", "verter_semantic"),
    ("verter_type_engine", "verter_semantic_source"),
    ("verter_type_engine", "verter_resolution"),
    ("verter_type_engine", "verter_scheduler"),
    ("verter_type_engine", "verter_tsgo_api"),
    ("verter_type_engine", "verter_type_runtime"),
    ("verter_type_engine", "verter_relay_shim"),
    ("verter_semantic_source", "verter_type_engine"),
    ("verter_semantic_source", "verter_session"),
    ("verter_semantic_source", "verter_workspace"),
    ("verter_semantic_source", "verter_protocol"),
    ("verter_semantic_source", "verter_compiler"),
    ("verter_semantic_source", "verter_scheduler"),
    ("verter_semantic_source", "verter_tsgo_api"),
    ("verter_semantic_source", "verter_type_runtime"),
    ("verter_semantic_source", "verter_relay_shim"),
    ("verter_session_query", "verter_resolution"),
    ("verter_session_query", "verter_semantic"),
    ("verter_session_query", "verter_parser"),
    ("verter_session_query", "oxc_allocator"),
    ("verter_resolution", "verter_semantic"),
    ("verter_resolution", "verter_parser"),
    ("verter_resolution", "oxc_span"),
    ("verter_resolution", "verter_session"),
    ("verter_resolution", "verter_workspace"),
    ("verter_resolution", "verter_scheduler"),
    ("verter_protocol", "verter_session"),
    ("verter_audit", "verter_protocol"),
    ("verter_audit", "verter_session"),
];

#[test]
fn planted_direct_violation_is_rejected_for_every_rule_and_class() {
    let mut failures = Vec::new();
    for &(root, forbidden) in PLANTED_DIRECT {
        let graph = fixture_graph(&[root, forbidden], &[(root, forbidden, Kind::Normal)]);
        match graph.check(rule(root)) {
            Ok(()) => failures.push(format!("{root} -> {forbidden}: accepted")),
            Err(message) if !message.contains(&format!("crate `{forbidden}` via")) => {
                failures.push(format!("{root} -> {forbidden}: wrong rejection {message}"));
            }
            Err(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn planted_direct_cases_cover_every_rule_class() {
    for rule in RULES {
        for forbidden in rule.forbidden {
            for target in forbidden.targets {
                let covered = PLANTED_DIRECT
                    .iter()
                    .any(|&(root, planted)| root == rule.root && target.matches(planted));
                assert!(
                    covered,
                    "`{}` forbids {target:?} ({}) but no planted case exercises it",
                    rule.root, forbidden.class
                );
            }
        }
    }
}

#[test]
fn planted_forbidden_names_are_real_packages() {
    let graph = live_graph();
    let live = graph.package_names();
    for &(root, forbidden) in PLANTED_DIRECT {
        for name in [root, forbidden] {
            assert!(
                live.contains(name) || ABSENT_ROOTS.contains(&name),
                "planted name `{name}` is neither a resolved package nor listed in ABSENT_ROOTS"
            );
        }
    }
    for name in ABSENT_ROOTS {
        assert!(
            RULES.iter().any(|rule| rule.root == *name),
            "absent root `{name}` has no closure rule"
        );
        assert!(
            !live.contains(name),
            "`{name}` is now a workspace package: remove it from ABSENT_ROOTS so its live closure check runs"
        );
    }
}

#[test]
fn planted_transitive_violation_is_rejected_with_its_chain() {
    let graph = fixture_graph(
        &["verter_type_engine", "verter_identity", "oxc_span"],
        &[
            ("verter_type_engine", "verter_identity", Kind::Normal),
            ("verter_identity", "oxc_span", Kind::Normal),
        ],
    );
    let message = assert_rejects("verter_type_engine", &graph, "oxc_span");
    assert!(
        message.contains("via verter_type_engine -> verter_identity -> oxc_span"),
        "{message}"
    );
}

#[test]
fn planted_feature_gated_optional_violation_is_rejected() {
    let graph = fixture_graph(
        &["verter_type_engine", "verter_parser"],
        &[(
            "verter_type_engine",
            "verter_parser",
            Kind::OptionalBehind("syntax"),
        )],
    );
    assert_rejects("verter_type_engine", &graph, "verter_parser");
}

#[test]
fn planted_renamed_target_specific_violation_is_rejected() {
    // A renamed dependency under a target table the host never matches: the
    // closure must follow the resolved package, not the local alias, and must
    // not filter edges by the host platform.
    let graph = fixture_graph_with(
        &["verter_type_engine", "oxc_span"],
        &[],
        &[(
            "verter_type_engine",
            "[target.'cfg(target_os = \"none\")'.dependencies]\n\
             span_alias = { path = \"../oxc_span\", package = \"oxc_span\" }",
        )],
    );
    assert_rejects("verter_type_engine", &graph, "oxc_span");
}

#[test]
fn planted_build_dependency_violation_is_rejected() {
    let graph = fixture_graph(
        &["verter_type_engine", "verter_compiler"],
        &[("verter_type_engine", "verter_compiler", Kind::Build)],
    );
    assert_rejects("verter_type_engine", &graph, "verter_compiler");
}

#[test]
fn planted_dev_only_dependency_is_accepted_at_every_hop() {
    let graph = fixture_graph(
        &[
            "verter_type_engine",
            "verter_session",
            "verter_identity",
            "verter_scheduler",
        ],
        &[
            ("verter_type_engine", "verter_session", Kind::Dev),
            ("verter_type_engine", "verter_identity", Kind::Normal),
            ("verter_identity", "verter_scheduler", Kind::Dev),
        ],
    );
    assert_eq!(graph.check(rule("verter_type_engine")), Ok(()));
}

#[test]
fn clean_layered_fixture_passes_every_rule() {
    let graph = fixture_graph(
        &[
            "verter_type_engine",
            "verter_semantic_source",
            "verter_session_query",
            "verter_resolution",
            "verter_identity",
            "verter_parser",
            "verter_semantic",
            "verter_protocol",
            "verter_audit",
        ],
        &[
            ("verter_type_engine", "verter_session_query", Kind::Normal),
            ("verter_type_engine", "verter_identity", Kind::Build),
            ("verter_semantic_source", "verter_parser", Kind::Normal),
            ("verter_semantic_source", "verter_semantic", Kind::Normal),
            ("verter_semantic_source", "verter_resolution", Kind::Normal),
            ("verter_resolution", "verter_session_query", Kind::Normal),
            ("verter_session_query", "verter_identity", Kind::Normal),
            ("verter_protocol", "verter_session_query", Kind::Normal),
            ("verter_protocol", "verter_audit", Kind::Normal),
        ],
    );
    for rule in RULES {
        assert_eq!(graph.check(rule), Ok(()), "`{}` rule", rule.root);
    }
}

#[test]
fn absent_root_crate_fails_instead_of_passing() {
    let graph = fixture_graph(&["verter_session_query"], &[]);
    let error = graph
        .check(rule("verter_type_engine"))
        .expect_err("an absent root crate must fail");
    assert!(error.contains("is not a workspace member"), "{error}");
}
