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
//! * every `oxc`-prefixed crate — forbidden, with ONE pinned exception:
//!   the span-primitive subtree. `verter_span` (the workspace-wide span
//!   primitive) depends unconditionally on `oxc_span`, which carries its
//!   own small oxc-prefixed tail (the miette diagnostics fork, the arena
//!   allocator, string/estree primitives). The sanctioned set is computed
//!   STRUCTURALLY as `oxc_span`'s own production closure and then
//!   equality-pinned: its `oxc`-prefixed membership must EQUAL the audited
//!   allowlist ([`OXC_SANCTIONED_ALLOWLIST`]), so an `oxc_span` release
//!   that grows (or shrinks) the tail is a re-audit gate, never a silent
//!   grant. The subtree is additionally canaried: it must contain no
//!   `verter_*` crate and none of the parser/AST front-end crates
//!   (`oxc_parser` / `oxc_ast` / `oxc_ast_visit` / `oxc_syntax`, which
//!   depend ON `oxc_span`, never vice versa — so the exception can never
//!   grow to admit a parser). Its sole entry edge from the query-boundary
//!   closure must be `verter_span → oxc_span`: a direct dependency on
//!   `oxc_span` (or any member of the tail) from any other closure crate
//!   fails.
//!
//! This is a resolve-graph walk over what cargo actually links, not a
//! source-text scan: adding a forbidden crate to the `Cargo.toml`
//! (plainly, optionally behind ANY feature, or transitively through a new
//! dependency) adds a resolved edge and fails the walk. Dev-dependencies
//! are deliberately NOT followed: they never link into the production
//! library, and this guard's own tooling dev-deps must not self-trip it.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
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

/// The root of the single sanctioned `oxc`-prefixed subtree (the span
/// primitive), tolerated ONLY via `verter_span`.
const OXC_EXCEPTION_ROOT: &str = "oxc_span";
const OXC_EXCEPTION_SOLE_PARENT: &str = "verter_span";

/// The AUDITED `oxc`-prefixed membership of the sanctioned span-primitive
/// subtree: `oxc_span` itself plus its production tail (the miette
/// diagnostics fork, the arena allocator, string/estree/data-structure
/// primitives and the ast-macros proc-macro crate — every one a leaf
/// utility, none a parser or resolver). Equality-pinned against the live
/// closure: an `oxc_span` release that introduces a NEW `oxc_*` crate (or
/// drops one) fails the guard and forces a deliberate re-audit of this
/// list, instead of being silently permitted by the structural walk alone.
const OXC_SANCTIONED_ALLOWLIST: [&str; 8] = [
    "oxc-miette",
    "oxc-miette-derive",
    "oxc_allocator",
    "oxc_ast_macros",
    "oxc_data_structures",
    "oxc_estree",
    "oxc_span",
    "oxc_str",
];

/// The parser/AST front-end crates the sanctioned span subtree must NEVER
/// contain — the honesty canary on the exception itself. These depend ON
/// `oxc_span`, never vice versa, so their appearance inside its closure
/// means the exception has rotted and must be re-audited.
const FRONT_END_NEVER_SANCTIONED: [&str; 4] =
    ["oxc_parser", "oxc_ast", "oxc_ast_visit", "oxc_syntax"];

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

/// The sanctioned span-primitive subtree: `oxc_span`'s OWN production
/// closure, computed structurally from the live resolve graph, canaried
/// against front-end rot, and equality-pinned against the audited
/// `oxc`-prefixed allowlist before use.
fn sanctioned_span_subtree(graph: &ResolveGraph) -> HashSet<String> {
    let subtree = graph.production_closure(OXC_EXCEPTION_ROOT);
    for id in &subtree {
        let name = graph.name_of(id);
        assert!(
            !FRONT_END_NEVER_SANCTIONED.contains(&name),
            "exception canary: parser/AST front-end crate `{name}` appeared inside \
             `{OXC_EXCEPTION_ROOT}`'s own production closure; the span-primitive \
             exception has rotted and must be re-audited"
        );
        assert!(
            !name.starts_with("verter"),
            "exception canary: workspace crate `{name}` appeared inside \
             `{OXC_EXCEPTION_ROOT}`'s own production closure; the span-primitive \
             exception has rotted and must be re-audited"
        );
    }

    // Equality pin: the subtree's `oxc`-prefixed membership must EQUAL the
    // audited allowlist — a new (or vanished) `oxc_*` crate inside
    // `oxc_span`'s closure is a review gate that forces a deliberate
    // re-audit, never a silent grant by the structural walk alone.
    let observed: BTreeSet<&str> = subtree
        .iter()
        .map(|id| graph.name_of(id))
        .filter(|name| name.starts_with("oxc"))
        .collect();
    let audited: BTreeSet<&str> = OXC_SANCTIONED_ALLOWLIST.into_iter().collect();
    assert_eq!(
        observed, audited,
        "exception audit: the oxc-prefixed membership of `{OXC_EXCEPTION_ROOT}`'s own \
         production closure diverged from OXC_SANCTIONED_ALLOWLIST; re-audit the \
         span-primitive exception and update the allowlist deliberately"
    );

    subtree
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
    let sanctioned = sanctioned_span_subtree(graph);

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

    // Every oxc-prefixed package must sit inside the sanctioned
    // span-primitive subtree. The parser/AST front-end crates depend ON
    // `oxc_span` (never vice versa), so they can never satisfy this.
    for id in &closure {
        let name = graph.name_of(id);
        if name.starts_with("oxc") {
            assert!(
                sanctioned.contains(id),
                "{root}: FIREWALL BREACH — oxc-prefixed package `{name}` is reachable in \
                 the production dependency closure outside the sanctioned span-primitive \
                 subtree (`{OXC_EXCEPTION_ROOT}`'s own closure, entered only via \
                 `{OXC_EXCEPTION_SOLE_PARENT}`)"
            );
        }
    }

    // Entry pin: the sanctioned subtree's oxc-prefixed members are entered
    // through EXACTLY ONE edge — `verter_span → oxc_span`. Any other
    // production edge from outside the subtree into an oxc-prefixed member
    // (a direct `oxc_span` dependency, a new route into the tail) fails.
    for id in &closure {
        let from = graph.name_of(id);
        let edge_is_internal = sanctioned.contains(id);
        for dep_id in &graph.production_deps[id] {
            let to = graph.name_of(dep_id);
            if to.starts_with("oxc") && !edge_is_internal {
                assert!(
                    from == OXC_EXCEPTION_SOLE_PARENT && to == OXC_EXCEPTION_ROOT,
                    "{root}: FIREWALL BREACH — `{from}` depends on oxc-prefixed \
                     `{to}` directly; the sole sanctioned entry into the span-primitive \
                     subtree is `{OXC_EXCEPTION_SOLE_PARENT} -> {OXC_EXCEPTION_ROOT}`"
                );
            }
        }
    }
}

#[test]
fn query_boundary_closure_excludes_parser_compiler_front_end() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);

    // The query crate: everything in FORBIDDEN_EVERYWHERE plus the oxc
    // rule. The non-vacuity canary names the root's GENUINE reach:
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
