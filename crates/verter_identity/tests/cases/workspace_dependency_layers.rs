//! Workspace dependency-direction authority.
//!
//! Walks `cargo metadata --format-version 1 --all-features` and asserts
//! every layered crate's production closure (normal + build, no dev) never
//! reaches a strictly higher layer, except one equality-pinned exception.
//!
//! Inward chain: identity/span/language/contracts → syntax frontends /
//! DTOs → semantic kernel → compiler → session/engine → adapters.
//! Harnesses sit outside; nothing may depend on them.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;

// Layer matrix. Lower layer = more foundational (inward). A crate's
// production closure may reach its own layer or any lower layer; reaching
// a strictly higher layer is a firewall breach unless explicitly,
// equality-pinned excepted.

const LAYER_1_IDENTITY_SPAN_LANGUAGE_CONTRACTS: &[&str] = &[
    "verter_span",
    "verter_language",
    "verter_ecma",
    "verter_analysis_inputs",
    "verter_audit",
    "verter_no_typeexpr",
    "verter_no_typeexpr_derive",
    "verter_no_storedspan",
    "verter_no_storedspan_derive",
    // The dependency-neutral typed-identity/profile/mapping/result-contract
    // vocabulary crate this test file itself lives in.
    "verter_identity",
    // Zero-dependency debug_assert!/debug_assert_eq!/debug_assert_ne! entry
    // point; sits below every crate that uses it.
    "verter_debug_assert",
];

const LAYER_2_SYNTAX_FRONTENDS_AND_NEUTRAL_DTOS: &[&str] = &[
    "verter_type_expr",
    "verter_type_expr_oxc",
    "verter_parser",
    "verter_css_syntax",
    "verter_macro_dto",
    "verter_session_query",
];

const LAYER_3_SEMANTIC_KERNEL: &[&str] =
    &["verter_semantic", "verter_diagnostics", "verter_actions"];

const LAYER_4_COMPILER: &[&str] = &["verter_compiler"];

const LAYER_5_MANAGED_ENGINE_SESSION: &[&str] = &[
    "verter_session",
    "verter_workspace",
    "verter_scheduler",
    "verter_tsgo_api",
    "verter_type_runtime",
    "verter_protocol",
];

const LAYER_6_ADAPTERS: &[&str] = &[
    "verter_lsp",
    "verter_napi",
    "verter_wasm",
    "verter_ffi",
    "verter_mcp",
    "verter_mcp_server",
    "verter_tsc",
    "verter_relay_shim",
    "verter-editor-client",
];

const LAYER_7_HARNESSES: &[&str] = &[
    "verter_bench",
    "verter_dx_baseline",
    "verter_vue_conformance",
    "verter_svelte_conformance",
    "verter_session_oracle_macro",
    // Test-only shared primitives (unique scratch paths, ephemeral ports,
    // deterministic counters). Consumed exclusively via `[dev-dependencies]`,
    // which is outside this test's tracked production closure, so its wide
    // dev-dependent fan-out never trips the "nothing may depend on a
    // harness" firewall.
    "verter_test_support",
    // Architecture/policy/portability guards relocated out of verter_session's
    // consolidated test binary (gate-performance step 2). A pure test-only
    // crate: its production [dependencies] is empty, and verter_session /
    // verter_span / verter_workspace are consumed exclusively via
    // [dev-dependencies] to check generated output against verter_session's
    // public API — never to be depended ON by anything.
    "verter_source_policy_gate",
    // The gate's shipped-cfg guard target (gate-performance step 3,
    // SINGLE-TEST-UNIVERSE directive): a pure test-only crate whose
    // production [dependencies] is empty and whose `verter_session` edge is
    // [dev-dependencies]-only — never depended ON by anything.
    "verter_shipped_cfg_contract",
];

/// Build/test tooling, not a production layer. Checked by
/// `repository_tooling_is_never_a_production_dependency_of_a_layered_crate`.
const REPOSITORY_TOOLING_NOT_IN_THE_LAYER_MATRIX: &[&str] = &[
    "xtask",
    "verter_compile_contracts",
    "verter_compile_contracts_bench",
    "verter_compile_contracts_session_variants",
];

fn layer_map() -> HashMap<&'static str, u8> {
    let mut m = HashMap::new();
    for &name in LAYER_1_IDENTITY_SPAN_LANGUAGE_CONTRACTS {
        m.insert(name, 1);
    }
    for &name in LAYER_2_SYNTAX_FRONTENDS_AND_NEUTRAL_DTOS {
        m.insert(name, 2);
    }
    for &name in LAYER_3_SEMANTIC_KERNEL {
        m.insert(name, 3);
    }
    for &name in LAYER_4_COMPILER {
        m.insert(name, 4);
    }
    for &name in LAYER_5_MANAGED_ENGINE_SESSION {
        m.insert(name, 5);
    }
    for &name in LAYER_6_ADAPTERS {
        m.insert(name, 6);
    }
    for &name in LAYER_7_HARNESSES {
        m.insert(name, 7);
    }
    m
}

/// Recorded upward exception: layer-3 `verter_diagnostics` → `verter_workspace` (layer 5) →
/// `verter_scheduler` (unconditional) and `verter_tsgo_api` (native-only).
/// Equality-pinned, never subset-checked — shrinking or growing the set
/// fails until this map is deliberately updated.
///
/// `cargo metadata` without `--filter-platform` unions every target-gated
/// edge, so both scheduler and tsgo_api appear. Target conditions are
/// pinned by
/// `the_ratified_exception_records_its_target_condition_precisely`.
fn ratified_upward_exceptions() -> HashMap<&'static str, BTreeSet<&'static str>> {
    let allowed: BTreeSet<&'static str> =
        ["verter_workspace", "verter_scheduler", "verter_tsgo_api"]
            .into_iter()
            .collect();
    let mut m = HashMap::new();
    m.insert("verter_diagnostics", allowed);
    m
}

// Resolve-graph plumbing (modelled exactly on
// `crates/verter_macro_dto/tests/cases/dependency_closure_guard.rs`).

fn workspace_manifest() -> PathBuf {
    // tests/cases/ lives at crates/verter_identity/, two levels below the
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

struct ResolveGraph {
    names: HashMap<String, String>,
    /// Package id -> production-dependency (package id, target cfg or
    /// `None` if unconditional) pairs. Normal + build kinds only; dev edges
    /// excluded at construction.
    production_deps: HashMap<String, Vec<(String, Option<String>)>>,
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

        let mut production_deps: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
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
                for dep_kind in dep["dep_kinds"]
                    .as_array()
                    .expect("resolved dep carries dep_kinds")
                {
                    let is_production = matches!(dep_kind["kind"].as_str(), None | Some("build"));
                    if is_production {
                        let target = dep_kind["target"].as_str().map(str::to_string);
                        deps.push((
                            dep["pkg"]
                                .as_str()
                                .expect("resolved dep pkg id is a string")
                                .to_string(),
                            target,
                        ));
                    }
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
    /// package NAME (self excluded).
    fn production_closure_names(&self, root_name: &str) -> BTreeSet<String> {
        self.production_closure_names_with_boundary(root_name, &BTreeSet::new())
    }

    /// BFS from `root_name` that does not expand past `boundary`. The
    /// boundary crate is still counted as reached; its own deps are not.
    /// Separates a crate's own upward edges from reach inherited through
    /// an already-excepted violator.
    fn production_closure_names_with_boundary(
        &self,
        root_name: &str,
        boundary: &BTreeSet<&str>,
    ) -> BTreeSet<String> {
        let root = self.id_of(root_name).to_string();
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        seen.insert(root.clone());
        queue.push_back(root);
        let mut names = BTreeSet::new();
        while let Some(id) = queue.pop_front() {
            // Reached, but do not walk its deps.
            if boundary.contains(self.name_of(&id)) && id != self.id_of(root_name) {
                continue;
            }
            let deps = self
                .production_deps
                .get(&id)
                .unwrap_or_else(|| panic!("resolve graph must carry a node for `{id}`"));
            for (dep, _target) in deps {
                if seen.insert(dep.clone()) {
                    names.insert(self.name_of(dep).to_string());
                    queue.push_back(dep.clone());
                }
            }
        }
        names
    }

    /// The `target` cfg string recorded on the DIRECT edge `from -> to`
    /// (`None` if the edge is unconditional, `Some(cfg)` if target-gated).
    /// Panics if the direct edge does not exist — callers use this only to
    /// verify an edge they already know is direct.
    fn direct_edge_target(&self, from_name: &str, to_name: &str) -> Option<String> {
        let from_id = self.id_of(from_name).to_string();
        let to_id = self.id_of(to_name).to_string();
        let deps = self
            .production_deps
            .get(&from_id)
            .expect("from crate must have a resolve node");
        deps.iter()
            .find(|(dep_id, _)| *dep_id == to_id)
            .unwrap_or_else(|| {
                panic!("expected a direct production edge `{from_name} -> {to_name}`")
            })
            .1
            .clone()
    }
}

// Tests

/// Exception roots. Closure-based (not direct-edge): the upward reach is
/// two-hop, and every legitimate consumer of these crates inherits it.
/// Inheritors are not separate violations.
const RATIFIED_ROOT_CRATES: &[&str] = &["verter_diagnostics"];

#[test]
fn workspace_production_closures_never_cross_upward_except_the_recorded_exception() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);
    let layers = layer_map();
    let exceptions = ratified_upward_exceptions();
    let root_boundary: BTreeSet<&str> = RATIFIED_ROOT_CRATES.iter().copied().collect();

    let mut layered_names: Vec<&str> = layers.keys().copied().collect();
    layered_names.sort_unstable();

    for &crate_name in &layered_names {
        let own_layer = layers[crate_name];

        // Roots: fully expanded closure must equal the recorded set.
        if RATIFIED_ROOT_CRATES.contains(&crate_name) {
            let closure = graph.production_closure_names(crate_name);
            let upward: BTreeSet<&str> = closure
                .iter()
                .filter_map(|reached| {
                    let reached_layer = *layers.get(reached.as_str())?;
                    (reached_layer > own_layer)
                        .then(|| layers.get_key_value(reached.as_str()).unwrap().0)
                        .copied()
                })
                .collect();
            let expected = exceptions.get(crate_name).cloned().unwrap_or_default();
            assert_eq!(
                upward, expected,
                "FIREWALL: root `{crate_name}` (layer {own_layer}) production closure diverged \
                 from the exact recorded exception. Reached-upward: {upward:?}; recorded: \
                 {expected:?}. This is the literal equality pin — a change here means either \
                 the underlying dependency was removed (update this constant to match) or a \
                 genuinely new upward edge appeared (which must be assessed on its own merits, \
                 never silently absorbed by widening this set)."
            );
            continue;
        }

        // Every other layered crate: any upward reach NOT explained by
        // passing through one of the two roots is a genuinely independent
        // violation and must be empty.
        let attributed_closure =
            graph.production_closure_names_with_boundary(crate_name, &root_boundary);
        let unexplained_upward: BTreeSet<&str> = attributed_closure
            .iter()
            .filter_map(|reached| {
                let reached_layer = *layers.get(reached.as_str())?;
                (reached_layer > own_layer)
                    .then(|| layers.get_key_value(reached.as_str()).unwrap().0)
                    .copied()
            })
            .collect();
        assert!(
            unexplained_upward.is_empty(),
            "FIREWALL: `{crate_name}` (layer {own_layer}) production closure reaches a \
             strictly higher layer through a path that does NOT pass through the recorded \
             root exception ({RATIFIED_ROOT_CRATES:?}). Unexplained upward reach: \
             {unexplained_upward:?}. Do not widen the exception and do not weaken this test — \
             a genuinely new upward edge here needs its own assessment."
        );
    }
}

/// Blind walk (empty deps / name mismatches) would pass the firewall
/// vacuously. This proves the walk still reaches several hops away.
#[test]
fn closure_walk_is_non_vacuous_for_known_deep_reaches() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);

    let session_closure = graph.production_closure_names("verter_session");
    for expected in [
        "verter_parser",
        "verter_compiler",
        "verter_semantic",
        "verter_type_expr",
        "verter_scheduler",
    ] {
        assert!(
            session_closure.contains(expected),
            "walk canary: `verter_session`'s production closure must contain `{expected}` \
             (got {} names) — the closure walk has gone blind",
            session_closure.len()
        );
    }

    let semantic_closure = graph.production_closure_names("verter_semantic");
    assert!(
        semantic_closure.contains("verter_parser"),
        "walk canary: `verter_semantic`'s production closure must contain `verter_parser` \
         (got {} names) — the closure walk has gone blind",
        semantic_closure.len()
    );
}

/// The resolver ownership edge points from the I/O-owning workspace into the
/// dependency-neutral semantic kernel, never back upward. Both assertions are
/// required: the negative half alone passes if the intended edge is deleted.
#[test]
fn workspace_to_semantic_is_present_and_semantic_to_workspace_is_absent() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);

    let workspace_closure = graph.production_closure_names("verter_workspace");
    assert!(
        workspace_closure.contains("verter_semantic"),
        "resolver ownership requires `verter_workspace -> verter_semantic`; deleting the edge \
         must fail this guard"
    );

    let semantic_closure = graph.production_closure_names("verter_semantic");
    assert!(
        !semantic_closure.contains("verter_workspace"),
        "the dependency-neutral semantic kernel must never reach `verter_workspace`"
    );
}

/// Same-layer direction: `verter_session` → `verter_scheduler`, never the
/// reverse. The 7-layer matrix cannot see this back-edge.
#[test]
fn verter_scheduler_closure_excludes_verter_session() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);
    let closure = graph.production_closure_names("verter_scheduler");
    assert!(
        !closure.contains("verter_session"),
        "`verter_scheduler`'s production closure must not contain `verter_session` — the \
         dependency direction is `verter_session -> verter_scheduler`, never the reverse"
    );
}

/// Pin the exception edges' target cfgs so a wasm32-only resolve cannot
/// silently shrink the recorded reach.
#[test]
fn the_ratified_exception_records_its_target_condition_precisely() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);

    assert_eq!(
        graph.direct_edge_target("verter_workspace", "verter_scheduler"),
        None,
        "`verter_workspace -> verter_scheduler` must be UNCONDITIONAL (every target); a \
         target-gated form would mean the recorded exception no longer matches the declared \
         `[dependencies]` reach it covers"
    );
    assert_eq!(
        graph.direct_edge_target("verter_workspace", "verter_tsgo_api"),
        Some("cfg(not(target_arch = \"wasm32\"))".to_string()),
        "`verter_workspace -> verter_tsgo_api` must be gated EXACTLY \
         `cfg(not(target_arch = \"wasm32\"))` (native only); a change here means the recorded \
         exception's target condition is stale and must be re-verified, not silently re-pinned"
    );
}

/// Repository tooling is never a production dependency of any
/// layered crate — it has no row in the layer matrix at all, so a silent
/// new edge into it would otherwise pass the main assertion vacuously
/// (nothing in `layers` would ever flag it as "upward").
#[test]
fn repository_tooling_is_never_a_production_dependency_of_a_layered_crate() {
    let metadata = workspace_metadata();
    let graph = ResolveGraph::from_metadata(&metadata);
    let layers = layer_map();

    for &tool in REPOSITORY_TOOLING_NOT_IN_THE_LAYER_MATRIX {
        for &crate_name in layers.keys() {
            let closure = graph.production_closure_names(crate_name);
            assert!(
                !closure.contains(tool),
                "`{crate_name}` production closure must not reach repository tooling `{tool}`"
            );
        }
    }
}

/// The layer matrix itself must stay a discriminating partition: every
/// entry is unique to one layer (no crate silently listed twice), and the
/// non-layered tooling list does not overlap it.
#[test]
fn layer_matrix_entries_are_each_listed_exactly_once() {
    let all_layers: &[&[&str]] = &[
        LAYER_1_IDENTITY_SPAN_LANGUAGE_CONTRACTS,
        LAYER_2_SYNTAX_FRONTENDS_AND_NEUTRAL_DTOS,
        LAYER_3_SEMANTIC_KERNEL,
        LAYER_4_COMPILER,
        LAYER_5_MANAGED_ENGINE_SESSION,
        LAYER_6_ADAPTERS,
        LAYER_7_HARNESSES,
    ];
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for layer in all_layers {
        for &name in *layer {
            *seen.entry(name).or_insert(0) += 1;
        }
    }
    let duplicates: Vec<(&str, usize)> = seen.into_iter().filter(|&(_, count)| count > 1).collect();
    assert!(
        duplicates.is_empty(),
        "layer matrix lists a crate more than once: {duplicates:?}"
    );

    for &tool in REPOSITORY_TOOLING_NOT_IN_THE_LAYER_MATRIX {
        assert!(
            !layer_map().contains_key(tool),
            "`{tool}` must not appear in the layer matrix (it is repository tooling, checked \
             separately)"
        );
    }
}

/// Every workspace member `cargo metadata` reports is accounted for
/// (either layered or explicitly named as non-layered tooling) — so adding
/// a new crate to the workspace without updating this matrix fails loudly
/// instead of silently exempting it from the firewall.
#[test]
fn every_workspace_member_is_accounted_for_in_the_layer_matrix() {
    let metadata = workspace_metadata();
    let workspace_members = metadata["workspace_members"]
        .as_array()
        .expect("workspace_members array");
    let packages = metadata["packages"].as_array().expect("packages array");
    let mut member_names: BTreeSet<String> = BTreeSet::new();
    for member_id in workspace_members {
        let member_id = member_id.as_str().expect("member id is a string");
        let pkg = packages
            .iter()
            .find(|p| p["id"].as_str() == Some(member_id))
            .unwrap_or_else(|| panic!("workspace member `{member_id}` must have a package entry"));
        member_names.insert(
            pkg["name"]
                .as_str()
                .expect("package name is a string")
                .to_string(),
        );
    }

    let layers = layer_map();
    let mut unaccounted: Vec<String> = Vec::new();
    for name in &member_names {
        if !layers.contains_key(name.as_str())
            && !REPOSITORY_TOOLING_NOT_IN_THE_LAYER_MATRIX.contains(&name.as_str())
        {
            unaccounted.push(name.clone());
        }
    }
    assert!(
        unaccounted.is_empty(),
        "workspace member(s) not accounted for in the layer matrix or the tooling exclusion \
         list: {unaccounted:?} — a new crate must be assigned a layer (or explicitly excluded) \
         before this test can prove anything about it"
    );
}
