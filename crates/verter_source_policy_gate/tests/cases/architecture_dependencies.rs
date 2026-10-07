#[allow(clippy::duplicate_mod)]
#[path = "../../../verter_session/tests/cases/support/map_in_parallel.rs"]
mod map_in_parallel;
use map_in_parallel::map_in_parallel;
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn read_workspace_file(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap()
}

fn production_dependency_names(manifest: &str) -> std::collections::BTreeSet<String> {
    production_dependency_names_with_workspace(manifest, None)
}

fn production_dependency_names_with_workspace(
    manifest: &str,
    workspace_manifest: Option<&str>,
) -> std::collections::BTreeSet<String> {
    let document = manifest
        .parse::<toml::Table>()
        .expect("Cargo manifest must be valid TOML");
    let workspace_document = workspace_manifest.and_then(|manifest| manifest.parse().ok());
    let workspace_dependencies = workspace_document
        .as_ref()
        .and_then(|document: &toml::Table| document.get("workspace"))
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table);
    let mut names = std::collections::BTreeSet::new();

    fn add_table(
        table: Option<&toml::Value>,
        workspace_dependencies: Option<&toml::map::Map<String, toml::Value>>,
        names: &mut std::collections::BTreeSet<String>,
    ) {
        let Some(table) = table.and_then(toml::Value::as_table) else {
            return;
        };
        for (alias, spec) in table {
            let inherited = spec
                .as_table()
                .and_then(|spec| spec.get("workspace"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            let package = if inherited {
                workspace_dependencies
                    .and_then(|deps| deps.get(alias))
                    .and_then(|spec| spec.as_table())
                    .and_then(|spec| spec.get("package"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias)
            } else {
                spec.as_table()
                    .and_then(|spec| spec.get("package"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias)
            };
            if package.starts_with("verter_") {
                names.insert(package.to_owned());
            }
        }
    }

    add_table(
        document.get("dependencies"),
        workspace_dependencies,
        &mut names,
    );
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            add_table(
                target
                    .as_table()
                    .and_then(|table| table.get("dependencies")),
                workspace_dependencies,
                &mut names,
            );
        }
    }
    names
}

fn forbidden_imports(file: &syn::File, forbidden: &[&str]) -> Vec<String> {
    use syn::visit::Visit;

    struct Visitor<'a> {
        forbidden: &'a [&'a str],
        found: Vec<String>,
    }

    impl Visitor<'_> {
        fn record(&mut self, ident: &syn::Ident) {
            let name = ident.to_string();
            if self.forbidden.contains(&name.as_str()) && !self.found.contains(&name) {
                self.found.push(name);
            }
        }

        fn visit_use_root(&mut self, tree: &syn::UseTree) {
            match tree {
                syn::UseTree::Path(path) => self.record(&path.ident),
                syn::UseTree::Name(name) => self.record(&name.ident),
                syn::UseTree::Rename(rename) => self.record(&rename.ident),
                syn::UseTree::Glob(_) => {}
                syn::UseTree::Group(group) => {
                    for tree in &group.items {
                        self.visit_use_root(tree);
                    }
                }
            }
        }
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if path.segments.len() > 1 {
                if let Some(root) = path.segments.first() {
                    self.record(&root.ident);
                }
            }
            syn::visit::visit_path(self, path);
        }

        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.visit_use_root(&item.tree);
        }

        fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
            self.record(&item.ident);
            syn::visit::visit_item_extern_crate(self, item);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(root) = mac.path.segments.first() {
                self.record(&root.ident);
            }
            syn::visit::visit_macro(self, mac);
        }
    }

    let mut visitor = Visitor {
        forbidden,
        found: Vec::new(),
    };
    visitor.visit_file(file);
    visitor.found
}

fn module_declaration_matches(file: &syn::File, expected: &syn::ItemMod) -> bool {
    fn cfg_attr_selects_path(list: &syn::MetaList) -> bool {
        let Ok(args) = list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) else {
            return false;
        };
        args.iter().skip(1).any(|meta| match meta {
            syn::Meta::NameValue(value) => value.path.is_ident("path"),
            syn::Meta::List(nested) if nested.path.is_ident("cfg_attr") => {
                cfg_attr_selects_path(nested)
            }
            _ => false,
        })
    }

    fn is_source_redirect(attr: &syn::Attribute) -> bool {
        if attr.path().is_ident("path") {
            return true;
        }
        if !attr.path().is_ident("cfg_attr") {
            return false;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };
        cfg_attr_selects_path(list)
    }

    fn is_unconditionally_active(attrs: &[syn::Attribute]) -> bool {
        !attrs
            .iter()
            .any(|attr| attr.path().is_ident("cfg") || is_source_redirect(attr))
    }

    fn visibility_matches(actual: &syn::Visibility, expected: &syn::Visibility) -> bool {
        match (actual, expected) {
            (syn::Visibility::Public(_), syn::Visibility::Public(_))
            | (syn::Visibility::Inherited, syn::Visibility::Inherited) => true,
            (syn::Visibility::Restricted(actual), syn::Visibility::Restricted(expected)) => {
                actual.path == expected.path
            }
            _ => false,
        }
    }

    file.items.iter().any(|item| {
        let syn::Item::Mod(module) = item else {
            return false;
        };
        is_unconditionally_active(&module.attrs)
            && module.ident == expected.ident
            && module.content.is_none()
            && visibility_matches(&module.vis, &expected.vis)
    })
}

// ===========================================================================
// guard 10 — no_cross_product_binary_imports
//
// `verter_lsp` and `verter_mcp` are two independent product surfaces on
// top of the shared `verter_session` core. Their binaries must ship as
// separate processes — the LSP binary must not pull `verter_mcp` (the
// MCP server crate) into its compile graph, and the MCP server binary
// must not pull `verter_lsp` into its compile graph.
//
// Concretely the guard scans each product's `Cargo.toml` and rejects
// any line that declares the cross-product crate as a dependency
// (regardless of `optional = true` / feature gating). The previous
// `lsp_mcp_dependency_direction` guard tolerated `optional = true`
// because earlier work decoupled MCP behind a Cargo feature; this
// guard supersedes that allowance — the cross-product dependency is
// removed in full.
//
// The companion structural guard
// `lsp_binary_compile_graph_cannot_reach_verter_mcp` then asserts the
// RESOLVED workspace dependency graph reflects that boundary
// transitively; the standalone HTTP launcher's liveness is owned by the
// behavioral spawn tests driving the shared serving contract in
// `crates/verter_mcp/tests/support/http_serving_contract.rs` (one per
// entry binary: `crates/verter_mcp/tests/cases/http_readiness.rs` and
// `crates/verter_mcp_server/tests/cases/http_serving.rs`).
// ===========================================================================

/// Predicate: scan a `Cargo.toml` snippet for any dependency declaration
/// that names `crate_name` as a dep (any form: bare path, table form,
/// `optional = true`, feature-gated, etc.). Returns `true` when at
/// least one matching declaration exists in the `[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]`, or
/// `[target.*.dependencies]` sections — including the
/// `[dependencies.<crate>]` section-header form.
fn cargo_toml_declares_dep(src: &str, crate_name: &str) -> bool {
    fn is_dep_section(section: &str) -> bool {
        section == "dependencies"
            || section == "dev-dependencies"
            || section == "build-dependencies"
            || (section.starts_with("target.")
                && (section.ends_with(".dependencies")
                    || section.ends_with(".dev-dependencies")
                    || section.ends_with(".build-dependencies")))
    }

    let mut in_deps_section = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[') {
            let header = rest.trim_end_matches(']').trim();
            // `[dependencies.<crate>]` form: split on the FIRST '.'
            // after a recognized dep section name and check the
            // crate suffix exactly.
            if let Some((section, suffix)) = header.split_once('.') {
                if is_dep_section(section) && suffix == crate_name {
                    return true;
                }
                in_deps_section = is_dep_section(section)
                    || (section == "target"
                        && (suffix.ends_with(".dependencies")
                            || suffix.ends_with(".dev-dependencies")
                            || suffix.ends_with(".build-dependencies")));
                continue;
            }
            in_deps_section = is_dep_section(header);
            continue;
        }
        if !in_deps_section {
            continue;
        }
        // Match `<crate_name> = ...` with optional whitespace.
        // Avoid matching prefixes (e.g. `verter_mcp_server` must NOT
        // match `verter_mcp`).
        if let Some((k, _)) = trimmed.split_once('=') {
            if k.trim() == crate_name {
                return true;
            }
        }
    }
    false
}

/// True iff `manifest` declares a dependency on `dep`. The real
/// `no_verter_semantic_to_verter_session_dep` assertion AND its self-test both
/// route through THIS predicate, so the self-test exercises the real detection
/// logic instead of a tautological `literal.contains(substring-of-literal)`.
fn manifest_declares_dep(manifest: &str, dep: &str) -> bool {
    // A dependency entry names the crate as a whole key (`dep =` or
    // `dep.workspace = true`), so a longer crate name that merely starts
    // with `dep` (`verter_session_query`) is not a declaration of `dep`.
    manifest.lines().any(|line| {
        line.trim_start()
            .strip_prefix(dep)
            .is_some_and(|rest| matches!(rest.trim_start().chars().next(), Some('=' | '.')))
    })
}

#[test]
fn arh12_structural_helpers_cover_aliases_targets_and_comments() {
    let manifest = r#"
[dependencies]
verter_span.workspace = true
alias = { package = "verter_lsp", version = "1" }
[target.'cfg(unix)'.dependencies]
verter_parser = { workspace = true }
"#;
    assert_eq!(
        production_dependency_names(manifest),
        ["verter_lsp", "verter_parser", "verter_span"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    let workspace_manifest = r#"
[workspace.dependencies]
transport = { package = "verter_lsp", path = "transport" }
"#;
    let inherited_member = r#"
[dependencies]
transport = { workspace = true }
"#;
    assert_eq!(
        production_dependency_names_with_workspace(inherited_member, Some(workspace_manifest)),
        ["verter_lsp"].into_iter().map(str::to_owned).collect()
    );

    let source =
        syn::parse_file("use verter_lsp::Server;\nextern crate verter_parser as parser;\n")
            .unwrap();
    assert_eq!(
        forbidden_imports(&source, &["verter_lsp", "verter_parser"]),
        vec!["verter_lsp", "verter_parser"]
    );
    let local_identifier =
        syn::parse_file("fn f() { let verter_lsp = 1; assert_eq!(verter_lsp, 1); }").unwrap();
    assert!(forbidden_imports(&local_identifier, &["verter_lsp"]).is_empty());

    let expected = syn::parse_str::<syn::ItemMod>("pub(crate) mod flow_return;").unwrap();
    let commented = syn::parse_file("pub(crate) /* owner */ mod flow_return;").unwrap();
    assert!(module_declaration_matches(&commented, &expected));
    let equivalent = syn::parse_file("pub(in crate) mod flow_return;").unwrap();
    assert!(module_declaration_matches(&equivalent, &expected));
    let widened = syn::parse_file("pub mod flow_return; // pub(crate) mod flow_return;").unwrap();
    assert!(!module_declaration_matches(&widened, &expected));
    let conditionally_widened = syn::parse_file(
        "#[cfg(any())] pub(crate) mod flow_return;\n#[cfg(not(any()))] pub mod flow_return;",
    )
    .unwrap();
    assert!(!module_declaration_matches(
        &conditionally_widened,
        &expected
    ));
    let nested = syn::parse_file("pub(crate) mod other { pub(crate) mod flow_return; }").unwrap();
    assert!(!module_declaration_matches(&nested, &expected));
    let inline = syn::parse_file("pub(crate) mod flow_return {}").unwrap();
    assert!(!module_declaration_matches(&inline, &expected));
    let direct_path =
        syn::parse_file("#[path = \"redirected.rs\"] pub(crate) mod flow_return;").unwrap();
    assert!(!module_declaration_matches(&direct_path, &expected));
    let cfg_attr_path =
        syn::parse_file("#[cfg_attr(unix, path = \"redirected.rs\")] pub(crate) mod flow_return;")
            .unwrap();
    assert!(!module_declaration_matches(&cfg_attr_path, &expected));
    let nested_cfg_attr_path = syn::parse_file(
        "#[cfg_attr(unix, cfg_attr(feature = \"redirect\", path = \"redirected.rs\"))] pub(crate) mod flow_return;",
    )
    .unwrap();
    assert!(!module_declaration_matches(
        &nested_cfg_attr_path,
        &expected
    ));
    let unrelated_cfg_attr =
        syn::parse_file("#[cfg_attr(unix, derive(Clone))] pub(crate) mod flow_return;").unwrap();
    assert!(module_declaration_matches(&unrelated_cfg_attr, &expected));

    let nested_comment =
        syn::parse_file("/* outer /* inner */ end */ use verter_lsp :: Server;").unwrap();
    assert_eq!(
        forbidden_imports(&nested_comment, &["verter_lsp"]),
        vec!["verter_lsp"]
    );
}

#[test]
fn arh12_dependency_and_visibility_contracts_are_enforced() {
    use serde_json::Value;
    use std::collections::BTreeSet;
    use walkdir::WalkDir;

    let contracts: Value = serde_json::from_str(&read_workspace_file(
        "tests/architecture-health/ARH1/products/dependency-contracts.json",
    ))
    .expect("ARH1 dependency contracts must be valid JSON");

    let workspace = workspace_root();
    let workspace_manifest =
        fs::read_to_string(workspace.join("Cargo.toml")).expect("read workspace Cargo.toml");
    let rules = contracts["layerRules"]
        .as_array()
        .expect("ARH1 layerRules must be an array");
    assert!(!rules.is_empty(), "ARH1 layerRules must not be empty");
    for rule in rules {
        let crate_rel = rule["crate"].as_str().expect("layer rule crate");
        let manifest = workspace.join(crate_rel).join("Cargo.toml");
        let manifest_text = fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
        let actual: BTreeSet<String> =
            production_dependency_names_with_workspace(&manifest_text, Some(&workspace_manifest));
        let declared: BTreeSet<String> = rule["mayImport"]
            .as_array()
            .expect("layer mayImport must be an array")
            .iter()
            .map(|name| name.as_str().expect("layer dependency name").to_owned())
            .collect();
        assert_eq!(actual, declared, "{} dependency contract drift", crate_rel);

        let forbidden: Vec<&str> = rule["mustNotImport"]
            .as_array()
            .expect("layer mustNotImport must be an array")
            .iter()
            .map(|name| name.as_str().expect("forbidden dependency name"))
            .collect();
        let src_root = workspace.join(crate_rel).join("src");
        let sources: Vec<PathBuf> = WalkDir::new(&src_root)
            .into_iter()
            .map(|entry| entry.unwrap_or_else(|e| panic!("walk {}: {e}", src_root.display())))
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs")
            })
            .map(|entry| entry.into_path())
            .collect();
        map_in_parallel(&sources, |path| {
            let source =
                fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let syntax = syn::parse_file(&source).unwrap_or_else(|e| {
                panic!(
                    "{} contains unsupported Rust syntax; refusing to skip dependency enforcement: {e}",
                    path.display()
                )
            });
            let found = forbidden_imports(&syntax, &forbidden);
            assert!(
                found.is_empty(),
                "{} imports forbidden crates: {:?}",
                path.display(),
                found
            );
        });
    }

    let hotspots = contracts["hotspots"]
        .as_array()
        .expect("ARH1 hotspots must be an array");
    assert!(!hotspots.is_empty(), "ARH1 hotspots must not be empty");
    for hotspot in hotspots {
        let surfaces = hotspot["surfaceDeclarations"]
            .as_array()
            .expect("hotspot surfaceDeclarations must be an array");
        assert!(
            !surfaces.is_empty(),
            "hotspot surfaceDeclarations must not be empty"
        );
        for surface in surfaces {
            let file = surface["file"].as_str().expect("surface file");
            let declaration = surface["declaration"]
                .as_str()
                .expect("surface declaration");
            let source = read_workspace_file(file);
            let parsed = syn::parse_file(&source)
                .unwrap_or_else(|e| panic!("parse visibility surface {file}: {e}"));
            let expected = syn::parse_str::<syn::ItemMod>(declaration)
                .unwrap_or_else(|e| panic!("parse visibility declaration {declaration}: {e}"));
            assert!(
                module_declaration_matches(&parsed, &expected),
                "visibility contract missing from {file}: {declaration}"
            );
        }
    }
}

// ===========================================================================
// Retired — `no_concrete_verter_host_in_seal_scope`
// ===========================================================================
//
// Ambient host, store and config access in resolver-tier code stays barred
// STRUCTURALLY (CLAUDE.md:500: landed enforcement is structural, never a
// name-keyed source scanner):
//
//   - `resolver_core::request_ports` is the five request-bound port set. Each
//     port returns owned records or typed demands; none returns a host,
//     store or config handle, so the engine carrier cannot obtain one.
//   - `crates/verter_session/tests/cases/compile-fail/engine_ports_*.rs`
//     (run by `scripts/compile-contracts.mjs`) are the compile-time
//     witnesses: a request that tries to reach ambient host state, a private
//     worker, a workspace or the execution graph fails to compile, and
//     `engine_ports_actual_host_positive.rs` pins the real port implementor.
//   - `RequestBoundAdapter`'s carrier field is `pub(super)` and the engine
//     holds only `&dyn ResolverContext`, so no engine-tier code can name or
//     read the carrier; `resolver_core/resolver_context.rs` pins the carrier's
//     field set at compile time.
//
// The residual source-spelling rule — a port implementation may reach the
// host, engine-tier code may not — is review-enforced, which is where a
// name-keyed landed scanner could not be.

// ===========================================================================
// Phase 9b — `no_napi_direct_verter_compiler_emitters`
// ===========================================================================
//
// `crates/verter_napi/src/**/*.rs` (production sources only — sibling
// `*_tests.rs` and `tests.rs` whitelisted) MUST NOT reference any
// compile-emitter symbol from `verter_compiler::compile::*` or
// `verter_compiler::compile_parallel::*`. The NAPI leaf must route
// batch/single SFC compile through the host-backed
// `VerterHost::compile_many` / `get_virtual_file` substrate.
//
// Forbidden inside `verter_compiler::compile::*` (explicit deny-list,
// no regex):
//   - `compile`
//   - `compile_from_parsed`
//
// The entire `verter_compiler::compile_parallel::*` namespace is
// forward-defense-forbidden — the module is intentionally never
// created.
//
// Allow-listed pure-data exports from `verter_compiler::compile::*`:
//   `CodegenOptions`, `VerterCompileOptions`, `VerterCompileResult`,
//   `TypesParserConfig`, `ParsedSfc`. Anything else inside the
//   `compile` namespace is default-deny.
//
// Three visitor methods (`visit_item_use`, `visit_expr_path`,
// `visit_type_path`) call shared `classify(segments)` to detect
// violations. Glob arms reject any glob whose prefix matches either
// compile namespace. `Rename` arms match on the ORIGINAL ident.
//
// Violation messages report file path + ident kind + leaf ident. No
// line numbers — `proc-macro2/span-locations` is not enabled in this
// workspace's `syn` dev-dep (see Cargo.toml:96).

mod napi_compiler_emitters {
    use std::path::{Path, PathBuf};

    use syn::visit::Visit;
    use syn::{
        ExprPath, ItemUse, Path as SynPath, PathSegment, TypePath, UseGlob, UseGroup, UseName,
        UsePath, UseRename, UseTree,
    };
    use walkdir::WalkDir;

    use super::workspace_root;

    /// Forbidden idents directly inside `verter_compiler::compile::*`.
    /// Inside `verter_compiler::compile_parallel::*` ALL leaves are
    /// forbidden (entire namespace).
    const COMPILE_DENY_LIST: &[&str] = &["compile", "compile_from_parsed"];

    /// Pure-data allow-list inside `verter_compiler::compile::*`.
    const COMPILE_ALLOW_LIST: &[&str] = &[
        "CodegenOptions",
        "CompileTarget",
        "VerterCompileOptions",
        "VerterCompileResult",
        "TypesParserConfig",
        "ParsedSfc",
    ];

    #[derive(Debug)]
    pub(super) struct Violation {
        pub(super) file: PathBuf,
        pub(super) kind: ViolationKind,
        pub(super) leaf: String,
    }

    // Same-postfix lint silenced — variant names are deliberate
    // taxonomy markers ("UsePath", "TypePath", "ExprPath" all refer to
    // distinct `syn::Visit` hooks; renaming would obscure the mapping).
    #[allow(clippy::enum_variant_names)]
    #[derive(Debug)]
    pub(super) enum ViolationKind {
        UsePath,
        UseGlob,
        TypePath,
        ExprPath,
    }

    impl std::fmt::Display for ViolationKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::UsePath => write!(f, "use"),
                Self::UseGlob => write!(f, "use ::*"),
                Self::TypePath => write!(f, "type"),
                Self::ExprPath => write!(f, "expr"),
            }
        }
    }

    /// What `classify` returns for a sequence of path segment idents.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Classification<'a> {
        /// Forbidden symbol inside one of the two compile namespaces.
        Forbidden(&'a str),
        /// Allowed pure-data import from `verter_compiler::compile`.
        AllowedDataType,
        /// Reference outside both namespaces — uninteresting.
        OutsideNamespace,
    }

    /// Classify a path by its leading two segments and final segment.
    /// `segments` is the leaf-relative ident chain after any
    /// `crate::` / `self::` / `super::` are skipped.
    fn classify<'a>(segments: &'a [String]) -> Classification<'a> {
        if segments.len() < 2 {
            return Classification::OutsideNamespace;
        }
        if segments[0] != "verter_compiler" {
            return Classification::OutsideNamespace;
        }
        let last: &'a str = segments.last().unwrap();
        match segments[1].as_str() {
            "compile_parallel" => {
                // Entire namespace is forward-defense-forbidden.
                Classification::Forbidden(last)
            }
            "compile" => {
                if COMPILE_DENY_LIST.contains(&last) {
                    Classification::Forbidden(last)
                } else if COMPILE_ALLOW_LIST.contains(&last) {
                    Classification::AllowedDataType
                } else {
                    // Default-deny inside the compile namespace.
                    Classification::Forbidden(last)
                }
            }
            _ => Classification::OutsideNamespace,
        }
    }

    /// Render a `syn::Path` to a flat list of segment ident strings,
    /// skipping leading `crate` / `self` / `super` to match the
    /// classifier's expected absolute-ish shape.
    fn path_idents(path: &SynPath) -> Vec<String> {
        let mut out: Vec<String> = path
            .segments
            .iter()
            .map(|s: &PathSegment| s.ident.to_string())
            .collect();
        while matches!(
            out.first().map(String::as_str),
            Some("crate" | "self" | "super")
        ) {
            out.remove(0);
        }
        out
    }

    pub(super) struct EmitterVisitor<'a> {
        path: &'a Path,
        violations: &'a mut Vec<Violation>,
    }

    impl<'a> EmitterVisitor<'a> {
        pub(super) fn new(path: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
            Self { path, violations }
        }

        /// Recursively walk a `UseTree` accumulating prefix segments,
        /// flagging Forbidden-classification leaves and Glob arms whose
        /// prefix matches either compile namespace.
        fn walk_use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
            match tree {
                UseTree::Path(UsePath { ident, tree, .. }) => {
                    prefix.push(ident.to_string());
                    self.walk_use_tree(tree, prefix);
                    prefix.pop();
                }
                UseTree::Name(UseName { ident, .. }) => {
                    prefix.push(ident.to_string());
                    self.classify_use_leaf(prefix);
                    prefix.pop();
                }
                UseTree::Rename(UseRename { ident, .. }) => {
                    // Match on the ORIGINAL ident, not the alias.
                    prefix.push(ident.to_string());
                    self.classify_use_leaf(prefix);
                    prefix.pop();
                }
                UseTree::Glob(UseGlob { .. }) => {
                    // A glob whose prefix is compile or compile_parallel
                    // is rejected outright.
                    let stripped = strip_use_anchors(prefix);
                    let is_target = stripped.len() >= 2
                        && stripped[0] == "verter_compiler"
                        && (stripped[1] == "compile" || stripped[1] == "compile_parallel");
                    if is_target {
                        self.violations.push(Violation {
                            file: self.path.to_path_buf(),
                            kind: ViolationKind::UseGlob,
                            leaf: format!("{}::*", stripped.join("::")),
                        });
                    }
                }
                UseTree::Group(UseGroup { items, .. }) => {
                    for item in items {
                        self.walk_use_tree(item, prefix);
                    }
                }
            }
        }

        fn classify_use_leaf(&mut self, prefix: &[String]) {
            let stripped = strip_use_anchors(prefix);
            if let Classification::Forbidden(leaf) = classify(&stripped) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::UsePath,
                    leaf: leaf.to_string(),
                });
            }
        }
    }

    fn strip_use_anchors(prefix: &[String]) -> Vec<String> {
        let mut out = prefix.to_vec();
        while matches!(
            out.first().map(String::as_str),
            Some("crate" | "self" | "super")
        ) {
            out.remove(0);
        }
        out
    }

    impl<'ast> Visit<'ast> for EmitterVisitor<'_> {
        fn visit_item_use(&mut self, item_use: &'ast ItemUse) {
            let mut prefix: Vec<String> = Vec::new();
            // Leading `::` doesn't change the absolute-ish form;
            // `tree` walks from the topmost crate ident.
            self.walk_use_tree(&item_use.tree, &mut prefix);
            syn::visit::visit_item_use(self, item_use);
        }

        fn visit_type_path(&mut self, tp: &'ast TypePath) {
            let segments = path_idents(&tp.path);
            if let Classification::Forbidden(leaf) = classify(&segments) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::TypePath,
                    leaf: leaf.to_string(),
                });
            }
            syn::visit::visit_type_path(self, tp);
        }

        fn visit_expr_path(&mut self, ep: &'ast ExprPath) {
            let segments = path_idents(&ep.path);
            if let Classification::Forbidden(leaf) = classify(&segments) {
                self.violations.push(Violation {
                    file: self.path.to_path_buf(),
                    kind: ViolationKind::ExprPath,
                    leaf: leaf.to_string(),
                });
            }
            syn::visit::visit_expr_path(self, ep);
        }
    }

    pub(super) fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("read {}: {}", path.display(), e),
        };
        let parsed =
            syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        let mut visitor = EmitterVisitor::new(path, violations);
        visitor.visit_file(&parsed);
    }

    pub(super) fn walk_rs_files(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Skip sibling `*_tests.rs` and `tests.rs` files (production
            // sources only).
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
            }
            files.push(path.to_path_buf());
        }
        files
    }

    pub(super) fn format_violations(violations: &[Violation]) -> String {
        use std::collections::BTreeMap;
        let mut by_file: BTreeMap<&Path, Vec<String>> = BTreeMap::new();
        for v in violations {
            by_file
                .entry(v.file.as_path())
                .or_default()
                .push(format!("{} `{}`", v.kind, v.leaf));
        }
        let mut lines = Vec::new();
        for (file, kinds) in by_file {
            lines.push(format!("  {} -- {}", file.display(), kinds.join(", ")));
        }
        format!(
            "found {} verter_compiler::compile{{,_parallel}} reference(s) in {} file(s):\n{}",
            violations.len(),
            lines.len(),
            lines.join("\n")
        )
    }

    pub(super) fn run() {
        let napi_root = workspace_root().join("crates/verter_napi/src");
        let mut violations: Vec<Violation> = Vec::new();
        for file in walk_rs_files(&napi_root) {
            scan_file(&file, &mut violations);
        }
        if !violations.is_empty() {
            panic!(
                "Phase 9b architecture guard violation:\n{}\n\nNAPI \
                 production sources MUST NOT reference \
                 `verter_compiler::compile::{{compile, compile_from_parsed}}` \
                 or any symbol under `verter_compiler::compile_parallel::*`. \
                 Batch and single SFC compile must route through \
                 `VerterHost::compile_many` / `VerterHost::get_virtual_file`. \
                 See sub-plan §5 for the full rule set.",
                format_violations(&violations)
            );
        }
    }
}

#[test]
fn no_napi_direct_verter_compiler_emitters() {
    // Phase 9b — un-ignored on commit 1 (RED on HEAD against the
    // bypass at `crates/verter_napi/src/lib.rs:2314`). Commit 3 deletes
    // the bypass, after which this test PASSES.
    napi_compiler_emitters::run();
}

#[test]
fn no_cross_product_binary_imports() {
    // `verter_lsp` (LSP product) must not depend on `verter_mcp`
    // (MCP product) in any form. The previous `optional = true`
    // tolerance is retired by Tier 3.
    let lsp_cargo = read_workspace_file("crates/verter_lsp/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&lsp_cargo, "verter_mcp"),
        "guard 10 (`no_cross_product_binary_imports`) violation: \
         `crates/verter_lsp/Cargo.toml` declares `verter_mcp` as a \
         dependency. The LSP and MCP products must ship as separate \
         binaries with no cross-product compile-graph coupling. \
         Spawn `verter_mcp_server` in its own process instead.",
    );

    // `verter_mcp` must not depend on `verter_lsp` either. This
    // direction is asserted symmetrically so future plan churn does
    // not silently re-couple the two products.
    let mcp_cargo = read_workspace_file("crates/verter_mcp/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&mcp_cargo, "verter_lsp"),
        "guard 10 (`no_cross_product_binary_imports`) violation: \
         `crates/verter_mcp/Cargo.toml` declares `verter_lsp` as a \
         dependency. The LSP and MCP products must ship as separate \
         binaries with no cross-product compile-graph coupling.",
    );

    let mcp_server_cargo = read_workspace_file("crates/verter_mcp_server/Cargo.toml");
    assert!(
        !cargo_toml_declares_dep(&mcp_server_cargo, "verter_lsp"),
        "guard 10 (`no_cross_product_binary_imports`) violation: \
         `crates/verter_mcp_server/Cargo.toml` declares `verter_lsp` \
         as a dependency. The standalone MCP server binary must \
         remain independent of the LSP product.",
    );
}

// ===========================================================================
// D26 — lsp_binary_compile_graph_cannot_reach_verter_mcp
//
// The LSP and MCP products ship as separate processes; the LSP binary
// must not embed the MCP server. That boundary is held STRUCTURALLY:
// `cargo metadata` — cargo's own parse of every workspace manifest
// (every declaration form, renames, dotted tables, target-gated
// sections) — must show NO dependency path from `verter_lsp` to
// `verter_mcp` over the dep kinds that link into the compiled binary
// (normal + build; dev-deps never ship). `verter_mcp` is an
// unpublished, path-only workspace crate, so any dependency path to it
// runs entirely through workspace members and the workspace-local BFS
// below is a complete transitive check. With the dep edge provably
// absent, the COMPILER rejects any `use verter_mcp` / `verter_mcp::`
// reference in LSP sources — the retired source-text greps of
// `verter_lsp/src/main.rs` proved strictly less (one file, direct
// references only) and are superseded, not weakened.
//
// The other half of the original acceptance — "the standalone MCP HTTP
// launcher still serves" — is owned by the shared BEHAVIORAL serving
// contract `crates/verter_mcp/tests/support/http_serving_contract.rs`
// (`assert_http_launcher_binds_announces_and_serves`), which runs a real
// entry binary with `--transport http --port 0`, requires the canonical
// readiness record as the FIRST stdout line, then POSTs an MCP
// `initialize` to the ANNOUNCED `/mcp` URL and requires a completed 200
// streamable-HTTP response (session id + `serverInfo`) — a launcher that
// binds and announces but parks before running the HTTP service fails,
// because the listener backlog alone satisfies only a bare TCP connect.
// Both shipped entry points delegate to the shared `verter_mcp::run::run`,
// and EACH is pinned by its own spawn test driving that contract:
// `crates/verter_mcp/tests/cases/http_readiness.rs` (`verter-mcp`) and
// `crates/verter_mcp_server/tests/cases/http_serving.rs`
// (`verter-mcp-server`), so a divergence between the twins is covered.
// ===========================================================================

#[test]
fn lsp_binary_compile_graph_cannot_reach_verter_mcp() {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(workspace_root())
        .output()
        .expect("run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse `cargo metadata` output");
    let packages = json
        .get("packages")
        .and_then(|v| v.as_array())
        .expect("cargo metadata reports packages");

    let member_names: BTreeSet<&str> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
        .collect();
    // The guard must fail LOUDLY if either endpoint vanishes — a silently
    // empty traversal would prove nothing.
    assert!(
        member_names.contains("verter_lsp"),
        "D26 guard integrity: workspace no longer contains `verter_lsp`; \
         re-point this guard at the LSP product crate."
    );
    assert!(
        member_names.contains("verter_mcp"),
        "D26 guard integrity: workspace no longer contains `verter_mcp`; \
         re-point this guard at the MCP product crate."
    );

    // Workspace-member dependency edges over LINKING kinds only: `null`
    // (normal) and `build`. Dev-deps do not enter the shipped binary and
    // guard 10 (`no_cross_product_binary_imports`) already rejects a direct
    // declaration of any kind.
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(|n| n.as_str())
            .expect("package has a name");
        let deps = package
            .get("dependencies")
            .and_then(|v| v.as_array())
            .expect("package lists dependencies");
        for dep in deps {
            let kind = dep.get("kind").and_then(|k| k.as_str());
            if kind == Some("dev") {
                continue;
            }
            let dep_name = dep
                .get("name")
                .and_then(|n| n.as_str())
                .expect("dependency has a name");
            if member_names.contains(dep_name) {
                edges.entry(name).or_default().push(dep_name);
            }
        }
    }

    // BFS from `verter_lsp` with predecessor tracking, so a violation
    // names the exact path that re-embedded MCP.
    let mut predecessor: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from(["verter_lsp"]);
    let mut visited: BTreeSet<&str> = BTreeSet::from(["verter_lsp"]);
    while let Some(current) = queue.pop_front() {
        for &next in edges.get(current).into_iter().flatten() {
            if visited.insert(next) {
                predecessor.insert(next, current);
                queue.push_back(next);
            }
        }
    }

    if visited.contains("verter_mcp") {
        let mut path = vec!["verter_mcp"];
        while let Some(&prev) = predecessor.get(path[path.len() - 1]) {
            path.push(prev);
        }
        path.reverse();
        panic!(
            "D26 violation: the LSP binary's compile graph reaches `verter_mcp` \
             via {} — the LSP and MCP products must ship as separate processes \
             with no cross-product compile-graph coupling. Spawn the standalone \
             `verter-mcp` binary instead.",
            path.join(" -> "),
        );
    }
}

#[test]
fn guard10_predicate_rejects_deliberate_cross_product_dep() {
    let bad_plain = "[dependencies]\nverter_mcp = { path = \"../verter_mcp\" }\nfoo = \"1\"\n";
    let bad_optional =
        "[dependencies]\nverter_mcp = { path = \"../verter_mcp\", optional = true }\nfoo = \"1\"\n";
    let bad_dotted = "[dependencies.verter_mcp]\npath = \"../verter_mcp\"\n";
    let bad_feature_gated =
        "[features]\nmcp = [\"dep:verter_mcp\"]\n\n[dependencies]\nverter_mcp = { path = \"../verter_mcp\", optional = true }\n";
    let good_no_dep = "[dependencies]\nfoo = \"1\"\nbar = \"2\"\n";
    let good_unrelated_section = "[features]\nmcp = []\n\n[dependencies]\nfoo = \"1\"\n";
    let good_prefix_only =
        "[dependencies]\nverter_mcp_server = { path = \"../verter_mcp_server\" }\nfoo = \"1\"\n";

    assert!(
        cargo_toml_declares_dep(bad_plain, "verter_mcp"),
        "guard 10 predicate must flag a plain `verter_mcp = ...` dep",
    );
    assert!(
        cargo_toml_declares_dep(bad_optional, "verter_mcp"),
        "guard 10 predicate must flag an `optional = true` dep — \
         Tier 3 retires the optional-dep tolerance",
    );
    assert!(
        cargo_toml_declares_dep(bad_dotted, "verter_mcp"),
        "guard 10 predicate must flag a `[dependencies.verter_mcp]` \
         section header",
    );
    assert!(
        cargo_toml_declares_dep(bad_feature_gated, "verter_mcp"),
        "guard 10 predicate must flag a feature-gated optional dep \
         even when the dep line is wrapped behind a `[features]` \
         section earlier in the file",
    );
    assert!(
        !cargo_toml_declares_dep(good_no_dep, "verter_mcp"),
        "guard 10 predicate must NOT flag a Cargo.toml that does not \
         depend on the cross-product crate",
    );
    assert!(
        !cargo_toml_declares_dep(good_unrelated_section, "verter_mcp"),
        "guard 10 predicate must NOT flag a `[features]` table entry \
         named `mcp` that lives outside any dependency section",
    );
    assert!(
        !cargo_toml_declares_dep(good_prefix_only, "verter_mcp"),
        "guard 10 predicate must NOT flag a dep with a name that has \
         `verter_mcp` as a strict prefix (e.g. `verter_mcp_server`)",
    );
}

/// Crate-ownership: `verter_session` owns the hot handle-bearing structs;
/// `verter_semantic` stays compat DTOs (`TypeExpr` / locators) and MUST NOT
/// depend on `verter_session`. The dependency direction is session →
/// semantic, never the reverse — a back-edge would let the lower compat-DTO
/// crate carry session `HotTypeRef` handles or grow a second resolution path.
#[test]
fn no_verter_semantic_to_verter_session_dep() {
    let manifest = read_workspace_file("crates/verter_semantic/Cargo.toml");
    assert!(
        !manifest_declares_dep(&manifest, "verter_session"),
        "verter_semantic/Cargo.toml must NOT reference verter_session — the \
         dependency direction is session → semantic, never the reverse. A \
         back-edge would let the lower compat-DTO crate carry session \
         HotTypeRef handles or grow a second resolution path."
    );
    // Self-discrimination through the SAME predicate (never a tautological
    // `literal.contains(substring-of-literal)`):
    //   POSITIVE — a manifest that DECLARES the dep is detected.
    assert!(
        manifest_declares_dep(
            "[dependencies]\nverter_session = { path = \"../verter_session\" }\n",
            "verter_session"
        ),
        "scanner self-test (positive): a declared verter_session dep must be detected"
    );
    //   NEGATIVE — a longer crate name that starts with the dep is NOT a
    //   declaration of it.
    assert!(
        !manifest_declares_dep(
            "[dependencies]
verter_session_query = { workspace = true }
",
            "verter_session"
        ),
        "scanner self-test (prefix): verter_session_query must not read as verter_session"
    );
    //   NEGATIVE — a manifest WITHOUT the dep is NOT detected. This is the
    //   discriminating half: it FAILS if the predicate vacuously returns true.
    assert!(
        !manifest_declares_dep("[dependencies]\nserde = \"1\"\n", "verter_session"),
        "scanner self-test (negative): a manifest without the dep must NOT be detected"
    );
}

// ===========================================================================
// Typed-IR bridge — ImportedMacroSurface containment guards
// ===========================================================================
//
// The `ImportedMacroSurface` lazy typed-IR bridge
// (`crates/verter_session/src/resolver_core/component_meta/imported_surface.rs`)
// MUST remain confined to `verter_session`'s resolver-core layer — it is a
// `pub(crate)`-dispatching internal abstraction that composes
// `SemanticQueryKey::ResolveDecl` + `ProjectPath` and does not belong
// in:
//
// - `verter_semantic` (the semantic extractor — owns analysis snapshots,
//   not host dispatch),
// - `verter_protocol` (transport-facing DTOs — must remain
//   serializable shapes, never typed-IR bridge identities),
// - `verter_ffi` (NAPI/WASM adapter — host objects must not leak the
//   bridge type into the FFI surface),
// - the TypeScript compat layers under `packages/component-meta/*`
//   (consumers of the public component-meta payload).
//
// Additionally, the bridge's public accessors MUST take an explicit
// `&dyn ResolverContext` parameter. Zero-arg `&self` accessors that
// secretly dispatch through TLS would violate R25 / R31: hidden lazy
// reads behind `&self` would hide dispatch cost, dep-signature merge,
// and cache-suppress propagation from the call site. The guard below
// scans the bridge module for any public method (`pub fn` /
// `pub(crate) fn`) on `impl ImportedMacroSurface` and asserts the
// signature carries a `ResolverContext` parameter.

/// Containment guard — `ImportedMacroSurface` does not appear in
/// `verter_semantic`.
#[test]
fn imported_macro_surface_not_in_verter_semantic() {
    let root = workspace_root();
    let semantic_src = root.join("crates/verter_semantic/src");
    let mut hits: Vec<String> = Vec::new();
    walk_dir_collect_rs(&semantic_src, &mut |path: &std::path::Path| {
        let src = std::fs::read_to_string(path).unwrap_or_default();
        for (lineno, line) in src.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            if line.contains("ImportedMacroSurface") {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                hits.push(format!("{rel}:{}: {}", lineno + 1, line.trim()));
            }
        }
    });
    assert!(
        hits.is_empty(),
        "guard `imported_macro_surface_not_in_verter_semantic`: \
         `verter_semantic/src/**` MUST NOT reference `ImportedMacroSurface`. \
         The bridge is `verter_session`-internal typed-IR dispatch \
         infrastructure; `verter_semantic` owns analysis snapshots, not \
         host dispatch. Offending lines:\n  {}",
        hits.join("\n  "),
    );
}

/// Containment guard — `ImportedMacroSurface` does not appear in
/// `verter_protocol`, `verter_ffi`, `verter_napi`, `verter_wasm`,
/// or the TypeScript compat layers under `packages/component-meta/`.
///
/// The bridge is internal typed-IR infrastructure. Leaking it into a
/// protocol DTO, an FFI host object, or a JS compat shape would
/// promote internal dispatch identity into the public API surface —
/// exactly the seam the single-engine rule prohibits.
#[test]
fn imported_macro_surface_not_in_protocol_or_ffi() {
    let root = workspace_root();
    // Substring-based scan across each scope. Substring is
    // sufficient because the bridge identifier is unique
    // (`ImportedMacroSurface`) and the scopes are small enough
    // that a per-file walk is fast.
    let scopes: &[&str] = &[
        "crates/verter_protocol/src",
        "crates/verter_ffi/src",
        "crates/verter_napi/src",
        "crates/verter_wasm/src",
        "packages/component-meta/src",
        "packages/component-meta/compat/src",
    ];
    let mut hits: Vec<String> = Vec::new();
    for scope in scopes {
        let scope_path = root.join(scope);
        if !scope_path.is_dir() {
            // Some scopes may not exist in every checkout (e.g.
            // `packages/component-meta/compat/src` if the compat
            // layer is still empty). The guard tolerates absent
            // scopes — what matters is that any extant source
            // file in any present scope is clean.
            continue;
        }
        walk_dir_collect_rs_and_ts(&scope_path, &mut |path: &std::path::Path| {
            let src = std::fs::read_to_string(path).unwrap_or_default();
            for (lineno, line) in src.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with("///") {
                    continue;
                }
                if line.contains("ImportedMacroSurface") {
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    hits.push(format!("{rel}:{}: {}", lineno + 1, line.trim()));
                }
            }
        });
    }
    assert!(
        hits.is_empty(),
        "guard `imported_macro_surface_not_in_protocol_or_ffi`: \
         `ImportedMacroSurface` MUST NOT appear in protocol DTOs, FFI \
         adapters, or JS compat layers. The bridge is internal typed-IR \
         dispatch infrastructure — leaking it into a public boundary \
         promotes an internal identity into a published API. Offending \
         lines:\n  {}",
        hits.join("\n  "),
    );
}

// ----------------------------------------------------------------
// Audit substrate isolation guards — created with the verter_audit
// crate and the cascade-move that retired the in-session DTO copies.
//
// `verter_audit_no_upward_deps` (manifest scan) is implied by
// `crates/verter_identity/tests/cases/workspace_dependency_layers.rs`
// (`verter_audit` production closure is `{verter_audit, verter_span}`).
// `audit_substrate_isolation` stays: the resolve-graph walk cannot see
// a bare `verter_*` token that is not a dependency (a local binding).
// Grandfathered scanner (CLAUDE.md forward-only rule).
// ----------------------------------------------------------------

/// Source files under `crates/verter_audit/src/` MUST `use` only
/// `verter_span`, `std`, and external crates.
#[test]
fn audit_substrate_isolation() {
    use std::path::PathBuf;
    let root = workspace_root();
    let audit_src: PathBuf = root.join("crates/verter_audit/src");
    let mut violations: Vec<String> = Vec::new();
    walk_dir_collect_rs(&audit_src, &mut |path: &std::path::Path| {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!(
                "audit_substrate_isolation: cannot read `{}`: {e}",
                path.display()
            )
        });
        for (line_no, line) in src.lines().enumerate() {
            // Skip comments and doc-comments — they discuss
            // `verter_*` crates as prose without importing them.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            // Reject any non-`verter_span` reference to a `verter_*`
            // crate on a non-comment line. The patterns we catch:
            // `use verter_<other>`, `pub use verter_<other>`,
            // `extern crate verter_<other>`, `verter_<other>::<...>`,
            // and bare references in attribute paths. Substring scan
            // is sufficient because the substrate's imports list is
            // tiny and we exclude `verter_span` and self-references
            // (`verter_audit` / `crate::`) explicitly.
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find("verter_") {
                let abs = search_from + rel;
                let after = abs + "verter_".len();
                // Capture the trailing identifier characters.
                let bytes = line.as_bytes();
                let mut end = after;
                while end < bytes.len() {
                    let c = bytes[end];
                    let alnum = c.is_ascii_alphanumeric() || c == b'_';
                    if !alnum {
                        break;
                    }
                    end += 1;
                }
                if end == after {
                    search_from = after;
                    continue;
                }
                let crate_name = &line[abs..end];
                search_from = end;
                if crate_name == "verter_span" {
                    continue;
                }
                if crate_name == "verter_audit" {
                    continue;
                }
                let rel_path = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                violations.push(format!("{rel_path}:{}: {}", line_no + 1, line.trim()));
                break;
            }
        }
    });
    assert!(
        violations.is_empty(),
        "audit_substrate_isolation: source files under \
         `crates/verter_audit/src/` reference non-leaf `verter_*` crates. \
         The substrate must use only `verter_span`, `std`, and external \
         crates. Offending lines:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn lsp_mcp_dependency_direction() {
    let cargo = read_workspace_file("crates/verter_lsp/Cargo.toml");
    let violation = cargo_toml_has_unmodified_verter_mcp_dep(&cargo);
    assert!(
        !violation,
        "Guard 3 (`lsp_mcp_dependency_direction`) violation: \
             `crates/verter_lsp/Cargo.toml` declares `verter_mcp` without \
             `optional = true`. The dependency direction must be \
             LSP -> optional MCP (gated by the `mcp` feature).",
    );
}

#[test]
fn guard3_predicate_rejects_deliberate_violation() {
    let bad = "[dependencies]\nverter_mcp = { path = \"../verter_mcp\" }\nother = \"1\"\n";
    let good = "[dependencies]\nverter_mcp = { path = \"../verter_mcp\", optional = true }\nother = \"1\"\n";
    let no_dep = "[dependencies]\nother = \"1\"\n";
    assert!(
        cargo_toml_has_unmodified_verter_mcp_dep(bad),
        "guard 3 must flag a non-optional verter_mcp dep",
    );
    assert!(
        !cargo_toml_has_unmodified_verter_mcp_dep(good),
        "guard 3 must NOT flag an optional verter_mcp dep",
    );
    assert!(
        !cargo_toml_has_unmodified_verter_mcp_dep(no_dep),
        "guard 3 must NOT flag a Cargo.toml that does not depend on verter_mcp",
    );
}

// ── Guard 3 — lsp_mcp_dependency_direction ──

/// Predicate: check whether a `Cargo.toml` snippet declares
/// `verter_mcp` as a non-optional dependency. Returns `true`
/// when a violation is present.
pub fn cargo_toml_has_unmodified_verter_mcp_dep(src: &str) -> bool {
    let mut found_violation = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("verter_mcp = ") && !trimmed.starts_with("verter_mcp =") {
            continue;
        }
        if !line.contains("optional = true") {
            found_violation = true;
            break;
        }
    }
    found_violation
}
/// Static-scan guard for the `Compiled-Output Conformance (CRITICAL)` rule's third paragraph:
/// "Do not build printers, re-printers, redundant-paren canonicalizers, or other production
/// machinery whose role includes mimicking the official compiler's cosmetic JS carrier
/// formatting." This scanner walks the production `.rs` tree (and the `crates/*/Cargo.toml` files)
/// and FAILS if production code (re)introduces official-format-mimicry machinery whose role
/// includes mimicking the official compiler's COSMETIC JS carrier formatting. Modeled on
/// `no_macro_string_heuristics_in_resolver_core`: a
/// predicate returning `(rel, line, token)` triples, asserted empty.
mod cosmetic_reprinter_guard {
    use std::fs;
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        super::workspace_root()
    }

    /// Walk a production tree and yield every `.rs` file that is NOT a test file
    /// (`*_tests.rs` / `tests.rs`) and is not nested under a `tests/` / `benches/` /
    /// `examples/` / `target/` directory.
    fn walk_production_rs(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(it) => it,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "tests"
                        || name == "benches"
                        || name == "examples"
                        || name == "target"
                    {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with(".rs") {
                    continue;
                }
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
                out.push(path);
            }
        }
        out.sort();
        out
    }

    fn relative_to_root(abs: &Path) -> String {
        abs.strip_prefix(workspace_root())
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Strip `//` line comments (outside string literals) and `/* … */` block comments to spaces,
    /// preserving the bytes INSIDE string literals (so a forbidden token that appears only inside a
    /// `"…"` / `r"…"` literal — e.g. `"http://x"` — survives and a rationale token in a comment
    /// never trips). Mirrors the string-literal-aware `strip_comments` helpers elsewhere in this
    /// file. Line structure is preserved (newlines kept; comment chars become spaces) so per-line
    /// reporting stays accurate.
    pub fn strip_comments(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let n = bytes.len();
        let mut i = 0usize;
        while i < n {
            let c = bytes[i];
            // Raw string: r"..." / r#"..."# / r##"..."## ...
            if c == b'r' {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < n && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < n && bytes[j] == b'"' {
                    out.extend_from_slice(&bytes[i..=j]);
                    let close: Vec<u8> = std::iter::once(b'"')
                        .chain(std::iter::repeat_n(b'#', hashes))
                        .collect();
                    let mut k = j + 1;
                    let mut closed = false;
                    while k + close.len() <= n {
                        if &bytes[k..k + close.len()] == close.as_slice() {
                            out.extend_from_slice(&bytes[(j + 1)..(k + close.len())]);
                            i = k + close.len();
                            closed = true;
                            break;
                        }
                        out.push(bytes[k]);
                        k += 1;
                    }
                    if !closed {
                        out.extend_from_slice(&bytes[(j + 1)..n]);
                        i = n;
                    }
                    continue;
                }
                // Not a raw string — fall through to normal handling.
            }
            // Regular string literal "..." (with \" escape handling).
            if c == b'"' {
                out.push(b'"');
                let mut k = i + 1;
                while k < n {
                    if bytes[k] == b'\\' && k + 1 < n {
                        out.push(bytes[k]);
                        out.push(bytes[k + 1]);
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'"' {
                        out.push(b'"');
                        k += 1;
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                i = k;
                continue;
            }
            // Line comment // — replace with spaces up to the newline (keep the newline).
            if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
                let mut k = i;
                while k < n && bytes[k] != b'\n' {
                    out.push(b' ');
                    k += 1;
                }
                i = k;
                continue;
            }
            // Block comment /* ... */ (non-nested is sufficient; replace with spaces, keep
            // newlines so line numbers stay aligned).
            if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
                let mut k = i + 2;
                out.push(b' ');
                out.push(b' ');
                while k < n {
                    if bytes[k] == b'*' && k + 1 < n && bytes[k + 1] == b'/' {
                        out.push(b' ');
                        out.push(b' ');
                        k += 2;
                        break;
                    }
                    out.push(if bytes[k] == b'\n' { b'\n' } else { b' ' });
                    k += 1;
                }
                i = k;
                continue;
            }
            out.push(c);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Lowercase + drop `_` (NOT `.`), so casing and snake/Pascal styling are irrelevant while the
    /// `.`-separated `parent.canonicalize()` keeps `parent` and `canonicalize` distinct. The
    /// dropped-`_` fold collapses `value_parens` / `ValueParens` → `valueparens` and
    /// `canonicalize_paren` → `canonicalizeparen`, while leaving `parent_canonical_id` →
    /// `parentcanonicalid` (the char after `paren` is `t`, never `c`) so it can never collide with
    /// a `paren` + `c…` canonicalizer needle.
    fn normalize(line: &str) -> String {
        line.chars()
            .filter(|&c| c != '_')
            .flat_map(char::to_lowercase)
            .collect()
    }

    /// Production symbol / module / use-path tokens — names that exist only as cosmetic-mimicry
    /// machinery for the official compiler's COSMETIC formatting. Each is matched (casing- and
    /// `_`-agnostic) as a normalized substring, PATH-INDEPENDENTLY (they are unambiguous
    /// cosmetic-mimicry names, flagged anywhere in the production tree). The first char after `paren`
    /// discriminates the canonicalizer needles (`paren` + `c`/`n`) from the legitimate `parent…`
    /// (`paren` + `t`) identifier family.
    const FORBIDDEN_SUBSTRINGS: &[&str] = &[
        // A `value_parens`-style paren-canonicalizer module / symbol.
        "valueparens",
        // Paren canonicalizer / normalizer symbols (`paren_canonicaliz*`,
        // `canonicalize_paren*`, `paren_normaliz*`, `normalize_paren*`).
        "parencanonicaliz",
        "canonicalizeparen",
        "parennormaliz",
        "normalizeparen",
        // A cosmetic re-printer (`reprint*` / `re_print*` / `re_printer` — all fold to `reprint`).
        "reprint",
        // A cosmetic-format-parity symbol.
        "officialformat",
        "formatofficial",
        "cosmeticformat",
    ];

    /// NEUTRAL JS-printer / serializer symbol tokens (`print_module`, `format_js_module`,
    /// `pretty_print`, `serialize_ast`) — matched (casing- and `_`-agnostic) as a normalized
    /// substring, but ONLY when the file lives in a codegen/emit area (see [`path_is_codegen_emit`]).
    /// A neutral name is NOT a cosmetic-mimicry symbol on its own: a global needle would
    /// false-positive on unrelated debug / CSS-printer / config-serializer code, so these tokens are
    /// path-scoped to compiled-output emission, where a JS re-printer/serializer routing emission
    /// through a cosmetic-formatting pass would land. A CSS `PrinterOptions`, a benign
    /// `serialize_config`, or a static-HTML `serialize_*` skeleton helper do NOT contain any of these
    /// compound tokens, so they are unaffected even inside a codegen path.
    const NEUTRAL_PRINTER_SUBSTRINGS: &[&str] = &[
        "printmodule",
        "formatjsmodule",
        "prettyprint",
        "serializeast",
    ];

    /// Dependency keys / `package = "…"` rename targets that are a JS printer/codegen crate pulled in
    /// as an output re-printer. `esrap` is the official Svelte string serializer; `swc_ecma_codegen`
    /// and `oxc_codegen` are JS codegen crates that, used as an emission reprinter, mimic cosmetic
    /// formatting. (CSS printers such as `lightningcss` are NOT JS codegen and are intentionally
    /// absent from this set.)
    const FORBIDDEN_DEP_NAMES: &[&str] = &["esrap", "swc_ecma_codegen", "oxc_codegen"];

    /// Whether a `/`-normalized relative source path lives in a compiled-output / codegen / emit area
    /// where a JS re-printer would route emission. Discriminating areas actually used in this repo for
    /// emitted-output code: the Vue template `code_gen` tree, any `codegen` / `/emit` /
    /// `client_codegen` segment, the Svelte framework `runtime/` emitter dir, the script-codegen tree
    /// (`verter_compiler/src/script/`, which builds `CodeGenOutput`), the TSX/TSC-codegen tree
    /// (`verter_compiler/src/compile/`), and any file whose stem is `emit` or ends with `_emit`
    /// (`emit.rs` / `*_emit.rs` — e.g. `ide/template/emit.rs`, `svelte/ide/emit.rs`,
    /// `svelte/runtime/expr_emit.rs`, `ide/script/comp_emit.rs`). Path handling is portable: directory
    /// areas match on `/`-delimited segments and the `emit` rule matches the FILE STEM, not only a
    /// `/emit/` directory segment. Kept conservative: the neutral-printer needles fire ONLY under one
    /// of these areas.
    pub fn path_is_codegen_emit(rel: &str) -> bool {
        let rel = rel.replace('\\', "/");
        let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();

        // Directory-area segments: the Vue template `code_gen`/`codegen` tree, an `/emit/` dir, a
        // `client_codegen` segment, and the Svelte `runtime/` emitter dir.
        let in_codegen_dir = segments
            .iter()
            .any(|s| *s == "code_gen" || *s == "codegen" || *s == "emit" || *s == "client_codegen");
        if in_codegen_dir {
            return true;
        }
        // The Svelte framework `runtime/` emitter dir (`…/svelte/runtime/…`) — any adjacent
        // `svelte` then `runtime` segment pair.
        if segments.windows(2).any(|w| w == ["svelte", "runtime"]) {
            return true;
        }
        // The script-codegen tree (`crates/verter_compiler/src/script/…`, building `CodeGenOutput`)
        // and the TSX/TSC-codegen tree (`crates/verter_compiler/src/compile/…`) — an adjacent
        // `verter_compiler`, `src`, then `script`/`compile` segment run anywhere in the path.
        if segments.windows(3).any(|w| {
            w == ["verter_compiler", "src", "script"] || w == ["verter_compiler", "src", "compile"]
        }) {
            return true;
        }
        // A FILE whose stem is `emit` or ends with `_emit` (`emit.rs` / `*_emit.rs`), matched on the
        // file STEM (not only a `/emit/` directory segment).
        if let Some(stem) = std::path::Path::new(&rel)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            if stem == "emit" || stem.ends_with("_emit") {
                return true;
            }
        }
        false
    }

    /// Per-line predicate over a COMMENT-STRIPPED `.rs` source line. `in_codegen_path` is whether the
    /// owning file is in a compiled-output / codegen / emit area (see [`path_is_codegen_emit`]).
    /// Returns the first matched forbidden token (the original, human-readable needle) or `None`.
    /// Covers: the path-independent cosmetic-mimicry substring needles; the path-scoped neutral
    /// JS-printer/serializer needles (flagged ONLY when `in_codegen_path`); and the
    /// `esrap`-faithful re-printer use-path (`esrap::`, `::esrap`, a `use … esrap …` import under any
    /// visibility, or an `extern crate esrap …` declaration) — but NOT a bare `esrap@…` rationale
    /// token (which only appears in comments, already stripped, and lacks a `::`/`use`/`extern crate`
    /// import shape).
    pub fn line_flags_cosmetic_reprinter(
        stripped_line: &str,
        in_codegen_path: bool,
    ) -> Option<&'static str> {
        let norm = normalize(stripped_line);
        for needle in FORBIDDEN_SUBSTRINGS {
            if norm.contains(needle) {
                return Some(needle);
            }
        }
        // Neutral JS-printer / serializer names are flagged ONLY inside a codegen/emit path — a
        // neutral name in a non-codegen path (or a CSS / config serializer) is legitimate.
        if in_codegen_path {
            for needle in NEUTRAL_PRINTER_SUBSTRINGS {
                if norm.contains(needle) {
                    return Some(needle);
                }
            }
        }
        // esrap re-printer use-path: a `::esrap` / `esrap::` reference, an `extern crate esrap …`
        // declaration, or a `use` statement importing it under any visibility. The normalized form
        // drops `_` and lowercases but PRESERVES `::`, `:`, `.`, and whitespace.
        if norm.contains("esrap::") || norm.contains("::esrap") {
            return Some("esrap-use-path");
        }
        // `extern crate esrap;` / `extern crate esrap as printer;` — collapse interior whitespace so
        // any spacing matches, then look for the `extern crate esrap` head.
        let collapsed: String = norm.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.contains("extern crate esrap") {
            return Some("esrap-use-path");
        }
        // A `use`/`pub use`/`pub(crate) use`/`pub(super) use … esrap …` import: strip a leading
        // visibility modifier, then require a `use ` head plus an `esrap` token.
        if strip_leading_visibility(stripped_line.trim_start()).starts_with("use ")
            && norm.contains("esrap")
        {
            return Some("esrap-use-path");
        }
        None
    }

    /// Strip a leading `pub` / `pub(crate)` / `pub(super)` / `pub(self)` / `pub(in path)` visibility
    /// modifier (and the following whitespace) from a trimmed line, returning the remainder so a
    /// `use`-head check works under any visibility. Lines without a `pub` prefix pass through.
    fn strip_leading_visibility(trimmed: &str) -> &str {
        let Some(rest) = trimmed.strip_prefix("pub") else {
            return trimmed;
        };
        let rest = rest.trim_start();
        // An optional `(…)` visibility-restriction group.
        let rest = if let Some(after_paren) = rest.strip_prefix('(') {
            match after_paren.find(')') {
                Some(close) => after_paren[close + 1..].trim_start(),
                None => rest,
            }
        } else {
            rest
        };
        rest
    }

    /// Parse a Cargo manifest source and return each forbidden JS-printer/codegen dependency as a
    /// human-readable `"[<table>]: <name>"` identifier. STRUCTURAL (not line-based): walks every
    /// dependency table Cargo recognises — `[dependencies]`, `[dev-dependencies]`,
    /// `[build-dependencies]`, `[workspace.dependencies]`, the target-keyed
    /// `[target.<cfg>.{dependencies,…}]` tables, and the `[dependencies.<name>]` sub-table form
    /// (`toml` normalises all of these into nested tables). A dependency is forbidden when EITHER its
    /// KEY is in [`FORBIDDEN_DEP_NAMES`] (handles quoted keys natively) OR its `package = "…"` rename
    /// field equals a forbidden name (`svelte_printer = { package = "esrap" }`). A `# …` rationale
    /// comment is dropped by the TOML parser, so it never trips. A manifest that fails to parse
    /// yields no violations (manifest validity is owned by Cargo / other guards, not this scan).
    pub fn manifest_forbidden_deps(manifest_src: &str) -> Vec<String> {
        let Ok(parsed) = toml::from_str::<toml::Value>(manifest_src) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let dep_table_names = ["dependencies", "dev-dependencies", "build-dependencies"];

        // A dep / package name is forbidden hyphen-vs-underscore-agnostically (`swc-ecma-codegen`
        // and `swc_ecma_codegen` are the same crate) so the spelling cannot be a trivial evasion.
        fn is_forbidden_dep(raw_name: &str) -> bool {
            let folded = raw_name.replace('-', "_");
            FORBIDDEN_DEP_NAMES.contains(&folded.as_str())
        }

        // Flag every forbidden entry in a single dependency table.
        fn flag_table(table: &toml::Value, label: &str, out: &mut Vec<String>) {
            let Some(table) = table.as_table() else {
                return;
            };
            for (name, value) in table {
                if is_forbidden_dep(name) {
                    out.push(format!("[{label}]: {name}"));
                    continue;
                }
                // The rename form `alias = { package = "esrap" }`.
                if let Some(pkg) = value
                    .as_table()
                    .and_then(|t| t.get("package"))
                    .and_then(|p| p.as_str())
                {
                    if is_forbidden_dep(pkg) {
                        out.push(format!("[{label}]: {name} (package=\"{pkg}\")"));
                    }
                }
            }
        }

        for table_name in dep_table_names {
            if let Some(table) = parsed.get(table_name) {
                flag_table(table, table_name, &mut out);
            }
        }
        // `target.<cfg>.{dependencies,dev-dependencies,build-dependencies}`.
        if let Some(targets) = parsed.get("target").and_then(|v| v.as_table()) {
            for (cfg, body) in targets {
                for table_name in dep_table_names {
                    if let Some(table) = body.get(table_name) {
                        flag_table(table, &format!("target.{cfg}.{table_name}"), &mut out);
                    }
                }
            }
        }
        // `workspace.dependencies` (the workspace ROOT manifest).
        if let Some(ws) = parsed.get("workspace") {
            if let Some(table) = ws.get("dependencies") {
                flag_table(table, "workspace.dependencies", &mut out);
            }
        }
        out.sort();
        out
    }

    /// One cosmetic-reprinter violation: `(rel_path, 1-based-line, token)`.
    type ReprinterViolation = (String, usize, String);

    /// Process-wide cache for the full-tree scan: [`cosmetic_reprinter_violations`] is invoked TWICE
    /// in this binary (the guard test + the scanner self-test's live-tree non-vacuity assert), and
    /// the scan walks every `crates/*/src/**/*.rs` plus every manifest. Computing it ONCE here keeps
    /// the cost paid a single time per process.
    static COSMETIC_REPRINTER_VIOLATIONS: std::sync::OnceLock<Vec<ReprinterViolation>> =
        std::sync::OnceLock::new();

    /// The CACHED full-tree scan. Both callers (`.is_empty()` / `.iter()`) read this slice; the
    /// scan runs at most once per process via the `OnceLock`.
    pub fn cosmetic_reprinter_violations() -> &'static [ReprinterViolation] {
        COSMETIC_REPRINTER_VIOLATIONS
            .get_or_init(collect_cosmetic_reprinter_violations)
            .as_slice()
    }

    /// Walk `crates/*/src/**/*.rs` (production only) plus every `crates/*/Cargo.toml` AND the
    /// workspace ROOT `Cargo.toml`, collecting `(rel_path, 1-based-line, token)` for each
    /// cosmetic-reprinter violation. Source lines flag path-independent cosmetic-mimicry needles
    /// everywhere, plus the neutral JS-printer needles only inside a codegen/emit path (the
    /// `path_is_codegen_emit` scope). Manifests are parsed STRUCTURALLY (`manifest_forbidden_deps`)
    /// so a forbidden dependency is reported regardless of declaration form (inline table, `package
    /// = "…"` rename, `[dependencies.<name>]` sub-table, quoted key); a structural-only finding has
    /// no source line, so it is reported at line 0. Generated-data modules are skipped via the shared
    /// `is_generated_data_source` helper.
    fn collect_cosmetic_reprinter_violations() -> Vec<ReprinterViolation> {
        let root = workspace_root();
        let crates_root = root.join("crates");
        let mut violations = Vec::new();

        // Structurally scan one manifest path; a forbidden dep is reported at line 0 (the structural
        // parse does not carry source line numbers).
        let scan_manifest = |manifest: std::path::PathBuf, violations: &mut Vec<_>| {
            if !manifest.exists() {
                return;
            }
            if let Ok(src) = fs::read_to_string(&manifest) {
                let rel = relative_to_root(&manifest);
                for dep in manifest_forbidden_deps(&src) {
                    violations.push((
                        rel.clone(),
                        0usize,
                        format!("esrap/codegen-dependency {dep}"),
                    ));
                }
            }
        };

        // (a) The workspace ROOT manifest (its `[workspace.dependencies]` table feeds every crate).
        scan_manifest(root.join("Cargo.toml"), &mut violations);

        let entries = match fs::read_dir(&crates_root) {
            Ok(it) => it,
            Err(_) => return violations,
        };
        let mut sources: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let crate_dir = entry.path();
            if !crate_dir.is_dir() {
                continue;
            }
            // (b) Production `.rs` sources under `<crate>/src`, scanned below.
            let src_dir = crate_dir.join("src");
            if src_dir.exists() {
                for file in walk_production_rs(&src_dir) {
                    let rel = relative_to_root(&file);
                    if !super::is_generated_data_source(&rel) {
                        sources.push((rel, file));
                    }
                }
            }
            // (c) The crate manifest — a forbidden JS-printer/codegen dependency (structural parse).
            scan_manifest(crate_dir.join("Cargo.toml"), &mut violations);
        }
        let scanned = super::map_in_parallel(&sources, |(rel, file)| {
            let mut found = Vec::new();
            let Ok(src) = fs::read_to_string(file) else {
                return found;
            };
            let in_codegen = path_is_codegen_emit(rel);
            let stripped = strip_comments(&src);
            for (idx, line) in stripped.lines().enumerate() {
                if let Some(token) = line_flags_cosmetic_reprinter(line, in_codegen) {
                    found.push((rel.clone(), idx + 1, token.to_string()));
                }
            }
            found
        });
        violations.extend(scanned.into_iter().flatten());
        violations.sort();
        violations
    }

    #[test]
    fn no_compiled_output_cosmetic_reprinter_path() {
        let violations = cosmetic_reprinter_violations();
        assert!(
            violations.is_empty(),
            "`no_compiled_output_cosmetic_reprinter_path` violations: production source / \
             manifests (re)introduce official-format-mimicry machinery or route compiled-output \
             emission through a JS re-printer/serializer. Per the `Compiled-Output Conformance \
             (CRITICAL)` rule, emit correct code directly and make conformance oracles structural \
             for cosmetic categories — do NOT build or route emission through printers, \
             re-printers, redundant-paren canonicalizers, or pull in a JS-printer/codegen \
             dependency used as an output reprinter to match the official compiler's cosmetic \
             formatting.\n\n\
             Forbidden (path-independent): a `value_parens`-style paren-canonicalizer; a paren \
             canonicalizer / normalizer (`paren_canonicaliz*` / `canonicalize_paren*` / \
             `paren_normaliz*` / `normalize_paren*`); a cosmetic re-printer (`reprint*` / \
             `re_print*`); a cosmetic-format-parity symbol (`official_format` / `format_official` \
             / `cosmetic_format`). Forbidden inside a codegen/emit path: a neutral JS-printer / \
             serializer (`print_module` / `format_js_module` / `pretty_print` / `serialize_ast`). \
             Forbidden use-path: an `esrap` use-path (`use … esrap …` under any visibility / \
             `extern crate esrap …` / `::esrap` / `esrap::`). Forbidden dependency (any manifest, \
             any declaration form — inline table, `package = \"…\"` rename, `[dependencies.<name>]` \
             sub-table, quoted key): `esrap` / `swc_ecma_codegen` / `oxc_codegen`.\n\n\
             Violations:\n  {}",
            violations
                .iter()
                .map(|(rel, lineno, token)| format!("{rel}:{lineno}: [{token}]"))
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }

    #[test]
    fn cosmetic_reprinter_scanner_discriminates() {
        // ── FLAGGED (planted forbidden tokens the scanner MUST report) ──────────────────────────

        // A `value_parens`-style symbol — casing-agnostic (snake_case AND PascalCase). These
        // path-independent cosmetic-mimicry needles fire even OUTSIDE a codegen path (`false`).
        assert_eq!(
            line_flags_cosmetic_reprinter("fn value_parens(expr: &Expr) -> String {", false),
            Some("valueparens"),
            "a snake_case `value_parens` symbol must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("struct ValueParens;", false),
            Some("valueparens"),
            "a PascalCase `ValueParens` symbol must be reported (casing-agnostic)"
        );

        // A paren-canonicalizer symbol and a re-printer symbol (path-independent).
        assert_eq!(
            line_flags_cosmetic_reprinter("fn canonicalize_parens(node: &Node) {", false),
            Some("canonicalizeparen"),
            "a paren-canonicalizer symbol must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("    let out = paren_normalizer(ast);", false),
            Some("parennormaliz"),
            "a paren-normalizer symbol must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("pub fn reprint_module(p: &Program) -> String {", false),
            Some("reprint"),
            "a cosmetic re-printer symbol must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("mod re_printer;", false),
            Some("reprint"),
            "a `re_printer` module (folds to `reprint`) must be reported"
        );

        // A real `esrap` use-path — the `use` import form, the `esrap::` call form, and the
        // visibility/alias/`extern crate` evasion forms.
        assert_eq!(
            line_flags_cosmetic_reprinter("use esrap::print;", false),
            Some("esrap-use-path"),
            "a `use esrap::print;` import must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("    let s = esrap::print(ast);", false),
            Some("esrap-use-path"),
            "an `esrap::print(ast)` call must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("extern crate esrap as printer;", false),
            Some("esrap-use-path"),
            "an `extern crate esrap as printer;` declaration must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("extern crate esrap;", false),
            Some("esrap-use-path"),
            "a bare `extern crate esrap;` declaration must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("pub(crate) use esrap as printer;", false),
            Some("esrap-use-path"),
            "a `pub(crate) use esrap as printer;` import must be reported (any visibility)"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("pub(super) use esrap::quote;", false),
            Some("esrap-use-path"),
            "a `pub(super) use esrap::quote;` import must be reported (any visibility)"
        );

        // ── FLAGGED (codegen/emit-path neutral JS-printer / serializer names) ────────────────────
        // A neutral `print_module` / `pretty_print` / `serialize_ast` IS flagged when the owning
        // file is in a codegen/emit path (`true`). These are reachable JS-emission reprinter names.
        assert_eq!(
            line_flags_cosmetic_reprinter("fn print_module(p: &Program) -> String {", true),
            Some("printmodule"),
            "a neutral `print_module` in a CODEGEN path must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("    let s = pretty_print(ast);", true),
            Some("prettyprint"),
            "a neutral `pretty_print` in a CODEGEN path must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("fn serialize_ast(node: &Node) -> String {", true),
            Some("serializeast"),
            "a neutral `serialize_ast` in a CODEGEN path must be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("fn format_js_module(p: &Program) {", true),
            Some("formatjsmodule"),
            "a neutral `format_js_module` in a CODEGEN path must be reported"
        );

        // A neutral name in EACH newly-covered codegen area is flagged — driven through
        // `path_is_codegen_emit` so the broadened scope is exercised end-to-end, not just the bool.
        // (1) the script-codegen tree (`verter_compiler/src/script/`, builds `CodeGenOutput`).
        let script_codegen = "crates/verter_compiler/src/script/process.rs";
        assert_eq!(
            line_flags_cosmetic_reprinter(
                "fn print_module(p: &Program) -> String {",
                path_is_codegen_emit(script_codegen)
            ),
            Some("printmodule"),
            "a neutral `print_module` under `verter_compiler/src/script/` must be reported"
        );
        // (2) the TSX/TSC-codegen tree (`verter_compiler/src/compile/`).
        let compile_codegen = "crates/verter_compiler/src/compile/template_data.rs";
        assert_eq!(
            line_flags_cosmetic_reprinter(
                "    let s = pretty_print(ast);",
                path_is_codegen_emit(compile_codegen)
            ),
            Some("prettyprint"),
            "a neutral `pretty_print` under `verter_compiler/src/compile/` must be reported"
        );
        // (3) a FILE whose stem is `emit` / `*_emit` (not under a `/emit/` directory) — the
        // `ide/template/emit.rs` stem and the `svelte/runtime/expr_emit.rs` `_emit` suffix.
        let emit_stem = "crates/verter_compiler/src/ide/template/emit.rs";
        assert_eq!(
            line_flags_cosmetic_reprinter(
                "fn serialize_ast(node: &Node) -> String {",
                path_is_codegen_emit(emit_stem)
            ),
            Some("serializeast"),
            "a neutral `serialize_ast` in an `emit.rs` file (matched by stem) must be reported"
        );
        let underscore_emit_stem = "crates/verter_compiler/src/svelte/runtime/expr_emit.rs";
        assert_eq!(
            line_flags_cosmetic_reprinter(
                "fn format_js_module(p: &Program) {",
                path_is_codegen_emit(underscore_emit_stem)
            ),
            Some("formatjsmodule"),
            "a neutral `format_js_module` in a `*_emit.rs` file (matched by stem) must be reported"
        );

        // A real codegen-emit path is classified as such; a non-codegen path is not.
        assert!(
            path_is_codegen_emit("crates/verter_compiler/src/template/code_gen/vdom/element.rs"),
            "a `template/code_gen/` path must classify as a codegen/emit path"
        );
        assert!(
            path_is_codegen_emit("crates/verter_compiler/src/svelte/runtime/client.rs"),
            "a `svelte/runtime/` path must classify as a codegen/emit path"
        );
        // The newly-covered codegen areas each classify as a codegen/emit path.
        assert!(
            path_is_codegen_emit("crates/verter_compiler/src/script/process.rs"),
            "a `verter_compiler/src/script/` path must classify as a codegen/emit path"
        );
        assert!(
            path_is_codegen_emit("crates/verter_compiler/src/compile/template_data.rs"),
            "a `verter_compiler/src/compile/` path must classify as a codegen/emit path"
        );
        assert!(
            path_is_codegen_emit("crates/verter_compiler/src/ide/template/emit.rs"),
            "an `emit.rs` file (stem `emit`) must classify as a codegen/emit path"
        );
        assert!(
            path_is_codegen_emit("crates/verter_compiler/src/ide/script/comp_emit.rs"),
            "a `*_emit.rs` file (stem ends with `_emit`) must classify as a codegen/emit path"
        );
        assert!(
            path_is_codegen_emit(
                "crates/verter_compiler/src/svelte/runtime/client_spread_html_emit.rs"
            ),
            "a `client_spread_html_emit.rs` file (stem ends with `_emit`) must classify as codegen"
        );
        assert!(
            !path_is_codegen_emit("crates/verter_session/src/resolver_core/component_meta.rs"),
            "a resolver-core path must NOT classify as a codegen/emit path"
        );
        // NEGATIVE: a non-codegen file whose stem merely CONTAINS `emit` mid-word but is neither
        // `emit` nor `*_emit` (e.g. `emitter_config.rs`) must NOT classify as codegen via the stem
        // rule — the stem rule is exact-`emit`-or-`_emit`-suffix, not a substring.
        assert!(
            !path_is_codegen_emit("crates/verter_session/src/host/emitter_config.rs"),
            "a non-codegen `emitter_config.rs` (stem neither `emit` nor `*_emit`) must NOT classify"
        );

        // ── FLAGGED (structural manifest dependency scan) ────────────────────────────────────────
        // The rename form `svelte_printer = { package = "esrap", … }`.
        let renamed_esrap =
            "[dependencies]\nsvelte_printer = { package = \"esrap\", version = \"2.2.11\" }\n";
        assert!(
            manifest_forbidden_deps(renamed_esrap)
                .iter()
                .any(|v| v.contains("svelte_printer") && v.contains("package=\"esrap\"")),
            "a renamed `svelte_printer = {{ package = \"esrap\" }}` dependency must be reported"
        );
        // The `[dependencies.esrap]` sub-table form.
        let esrap_subtable = "[dependencies.esrap]\nversion = \"2.2.11\"\n";
        assert!(
            manifest_forbidden_deps(esrap_subtable)
                .iter()
                .any(|v| v.contains("esrap")),
            "a `[dependencies.esrap]` sub-table dependency must be reported"
        );
        // A quoted-key esrap dependency.
        let quoted_esrap = "[dependencies]\n\"esrap\" = \"2.2.11\"\n";
        assert!(
            manifest_forbidden_deps(quoted_esrap)
                .iter()
                .any(|v| v.contains("esrap")),
            "a quoted-key `\"esrap\" = \"…\"` dependency must be reported"
        );
        // A plain `esrap = "…"` inline dependency, and the `esrap.workspace` inline-table form.
        assert!(
            !manifest_forbidden_deps("[dependencies]\nesrap = \"2.2.11\"\n").is_empty(),
            "a plain `esrap = \"…\"` dependency must be reported"
        );
        assert!(
            !manifest_forbidden_deps("[dependencies]\nesrap.workspace = true\n").is_empty(),
            "an `esrap.workspace` dependency must be reported"
        );
        // A `swc_ecma_codegen` / `oxc_codegen` JS-codegen dependency pulled in as a reprinter.
        assert!(
            !manifest_forbidden_deps("[dependencies]\nswc_ecma_codegen = \"0.1\"\n").is_empty(),
            "a `swc_ecma_codegen` dependency must be reported"
        );
        assert!(
            !manifest_forbidden_deps("[dev-dependencies]\noxc_codegen = \"0.1\"\n").is_empty(),
            "an `oxc_codegen` dependency (dev table) must be reported"
        );
        // The HYPHEN spelling of the same JS-codegen crate must NOT be a trivial evasion.
        assert!(
            !manifest_forbidden_deps("[dependencies]\nswc-ecma-codegen = \"0.1\"\n").is_empty(),
            "a hyphen-spelled `swc-ecma-codegen` dependency must be reported (spelling-agnostic)"
        );
        // The `package = "…"` rename of a JS-codegen crate, too.
        assert!(
            manifest_forbidden_deps(
                "[dependencies]\njs_emit = { package = \"oxc_codegen\", version = \"0.1\" }\n"
            )
            .iter()
            .any(|v| v.contains("package=\"oxc_codegen\"")),
            "a `package = \"oxc_codegen\"` rename must be reported"
        );

        // ── NOT FLAGGED (the scanner MUST discriminate these) ───────────────────────────────────

        // An `esrap` rationale COMMENT — after comment-strip there is nothing to match.
        let comment_line =
            "/// Mirrors the official printer's string serializer (esrap@2.2.11 `quote`).";
        assert_eq!(
            line_flags_cosmetic_reprinter(&strip_comments(comment_line), true),
            None,
            "an `esrap` rationale comment must NOT be reported (it is stripped before scanning)"
        );
        // The same rationale token is also benign as a Cargo manifest comment line: the TOML parse
        // drops `#` comments, so no dependency is found.
        assert!(
            manifest_forbidden_deps("[dependencies]\n# Mirrors esrap's quote serializer.\n")
                .is_empty(),
            "an `esrap` rationale comment in a manifest must NOT be reported"
        );

        // The syntax-REQUIRED concise-arrow body wrap — legitimate, not cosmetic parity. Tested in
        // BOTH a non-codegen and a codegen path: it is never a forbidden token.
        assert_eq!(
            line_flags_cosmetic_reprinter(
                "pub(super) fn concise_arrow_expr_body(body: &str) {",
                false
            ),
            None,
            "the `concise_arrow_expr_body` syntax-required wrap must NOT be reported"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter(
                "pub(super) fn concise_arrow_expr_body(body: &str) {",
                true
            ),
            None,
            "the `concise_arrow_expr_body` wrap must NOT be reported even in a codegen path"
        );

        // Legitimate identifiers that merely share a substring (the `paren` + `t` family).
        assert_eq!(
            line_flags_cosmetic_reprinter("    parent_canonical_id: &str,", false),
            None,
            "`parent_canonical_id` must NOT be reported (non-collision: `paren` + `t`)"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("    let id = parent.canonicalize();", false),
            None,
            "`parent.canonicalize()` must NOT be reported (non-collision)"
        );

        // ── NOT FLAGGED (the neutral-name PATH-SCOPING discriminator) ────────────────────────────
        // The SAME neutral names that fire inside a codegen path are LEGITIMATE outside one: a debug
        // `pretty_print`, a config `serialize_*`, or a CSS `PrinterOptions` is not a JS reprinter.
        assert_eq!(
            line_flags_cosmetic_reprinter("fn pretty_print(value: &Debug) -> String {", false),
            None,
            "a neutral `pretty_print` in a NON-codegen path must NOT be reported (path-scoping)"
        );
        assert_eq!(
            line_flags_cosmetic_reprinter("fn print_module(p: &Program) -> String {", false),
            None,
            "a neutral `print_module` in a NON-codegen path must NOT be reported (path-scoping)"
        );
        // A CSS printer's `PrinterOptions` — NOT a JS reprinter, even inside a codegen path: it does
        // not contain any neutral JS-printer compound token.
        assert_eq!(
            line_flags_cosmetic_reprinter("    let opts = PrinterOptions::default();", true),
            None,
            "a CSS `PrinterOptions` must NOT be reported even in a codegen path (not JS codegen)"
        );
        // A benign config serializer — even inside a codegen path, `serialize_config` does not
        // contain the `serialize_ast` compound token.
        assert_eq!(
            line_flags_cosmetic_reprinter("fn serialize_config(c: &Config) -> String {", true),
            None,
            "a benign `serialize_config` must NOT be reported even in a codegen path"
        );
        // A static-HTML skeleton `serialize_*` helper (the real `svelte/runtime/html.rs` family) —
        // it is the in-contract static-HTML serializer, NOT a JS-AST serializer, and does not
        // contain the `serialize_ast` token even though it lives under a codegen path.
        assert_eq!(
            line_flags_cosmetic_reprinter("fn serialize_clean_items(ir: &Ir) -> String {", true),
            None,
            "a static-HTML `serialize_clean_items` skeleton helper must NOT be reported in a \
             codegen path (it is not a JS-AST serializer)"
        );
        // A CSS-only dependency (`lightningcss`) is NOT a JS-codegen reprinter dependency.
        assert!(
            manifest_forbidden_deps("[dependencies]\nlightningcss = \"1.0.0\"\n").is_empty(),
            "a CSS-only `lightningcss` dependency must NOT be reported (not JS codegen)"
        );

        // The comment-stripper must NOT cut inside a string literal: a forbidden-looking token (or
        // a `//`) inside a `"…"` survives, and a bare URL string is not a comment.
        let url_line = "    let u = \"http://x\";";
        assert_eq!(
            strip_comments(url_line),
            url_line,
            "the comment-stripper must NOT cut inside a string literal (`\"http://x\"` survives)"
        );
        let esrap_in_string = "    let note = \"esrap::print\";";
        assert_eq!(
            strip_comments(esrap_in_string),
            esrap_in_string,
            "a string-literal payload survives the comment-strip verbatim"
        );

        // ── NON-VACUITY: the live tree is clean today ──────────────────────────────────────────
        // The real scan over the LIVE production tree returns empty: the guard passes because the
        // tree is clean, not because the scanner is inert. (The FLAGGED asserts above prove the
        // scanner is NOT inert.)
        assert!(
            cosmetic_reprinter_violations().is_empty(),
            "NON-VACUITY: the live production tree must contain no cosmetic-reprinter machinery \
             today (the guard passes because the tree is clean, not because the scanner is inert)"
        );
    }
}

/// Relative paths (forward-slash, workspace-rooted) of GENERATED DATA
/// modules under `crates/*/src/**`. These files are rendered by a
/// generator script (each carries an "auto-generated" / "GENERATED ...
/// DATA" header and is byte-pinned by a dedicated freshness test); they
/// are NOT hand-authored source. The source-scanning architecture guards
/// enforce hand-authored-source rules
/// — final-state prose, no plan vocabulary, no revived symbols — which do
/// not apply to a generator's verbatim data rows. Scanning them is both
/// incorrect (a divergence-label or header path is generated data, not a
/// phase reference) and the bulk of the scanners' cost (thousands of data
/// rows), so they are excluded by path here. The gate runner's advisory
/// file-size scan also excludes these generated data modules.
///
/// Matched by path SUFFIX so the same set is usable against either a
/// workspace-relative path or an absolute path string.
const GENERATED_DATA_SOURCE_FILES: &[&str] = &[
    // The canonical HTML5 named-character-reference table (~2231 rows),
    // auto-generated from the pinned official svelte `entities.js` by
    // `scripts/generate-svelte-entities.mjs` and byte-pinned by the
    // `svelte_entity_table_in_sync` freshness test.
    "crates/verter_compiler/src/svelte/runtime/entity_table.rs",
    // The honest Svelte differential-parity divergence allow-list — one
    // `DivergenceRow` per REAL `(fixture, axis)` divergence, GENERATED
    // from the discovery pass over the pinned svelte@5.56.10 and byte-pinned
    // by `known_divergences_are_real`.
    "crates/verter_compiler/src/svelte/runtime/diff_oracle_divergences.rs",
];

/// True when `path_str` (a forward-slash path string, relative or
/// absolute) is one of the [`GENERATED_DATA_SOURCE_FILES`]. The source-
/// scanning guards skip these: they hold a generator's verbatim data
/// rows, not hand-authored source subject to the final-state / no-plan-
/// vocabulary / no-revived-symbol rules.
fn is_generated_data_source(path_str: &str) -> bool {
    GENERATED_DATA_SOURCE_FILES
        .iter()
        .any(|suffix| path_str == *suffix || path_str.ends_with(&format!("/{suffix}")))
}

fn walk_dir_collect_rs(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "walk_dir_collect_rs: cannot read directory `{}`: {e}",
            dir.display()
        )
    });
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_collect_rs(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            f(&path);
        }
    }
}

/// Walk a directory and apply `cb` to every `.rs` or `.ts` file.
/// Shared by the protocol / FFI / compat scan above.
fn walk_dir_collect_rs_and_ts(dir: &std::path::Path, cb: &mut dyn FnMut(&std::path::Path)) {
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "rs" | "ts" | "tsx") {
            cb(path);
        }
    }
}
