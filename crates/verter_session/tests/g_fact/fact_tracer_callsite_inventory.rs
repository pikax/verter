//! Informational arch-guard — inventory of production
//! `install_fact_tracer` call sites in `crates/verter_session/src/`.
//!
//! This guard is INFORMATIONAL, not fail-loud. Its purpose is to give
//! reviewers a single audit point that lists every place the
//! fact-tracer is currently installed (i.e. every cold-compute entry
//! point that participates in the path-precise dep-signature
//! substrate). Used during plan-landed audits to verify that the
//! coverage matches the architectural map without requiring a fresh
//! repo-wide grep.
//!
//! The test always passes. Its assertions check:
//!
//! - The scan completed (the scanner was actually invoked).
//! - At least ONE call site was found — if zero were found, the
//!   scanner has either drifted out of sync with the codebase or the
//!   `install_fact_tracer` helper itself has been retired (which would
//!   be a separate, intentional retirement). Re-confirm the inventory
//!   in either case.
//!
//! Look at the test output (`cargo test -- --nocapture
//! fact_tracer_callsite_inventory`) to see the current list.
//!
//! Sibling guard: `fact_tracer_arch_guard.rs` (Blocks 1.6 / 1.5)
//! enforces the structural shape of the tracer substrate itself
//! (`with_fact_tracer`, `current_fact_tracer`, R18 carve-out).
//!
//! Note on filename: Windows applies a UAC manifest heuristic to test
//! binaries whose filenames contain `install*` and attempts to elevate
//! them, which fails inside the cargo build harness. The file is
//! therefore named `fact_tracer_callsite_inventory.rs` (instead of
//! the otherwise-natural `fact_tracer_install_site_inventory.rs`)
//! while preserving the intent — this is the inventory of
//! `install_fact_tracer` call sites.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Attribute, ExprCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta};
use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

const SYMBOL: &str = "install_fact_tracer";

#[derive(Debug, Clone)]
struct Site {
    file: PathBuf,
    enclosing_fn: String,
}

impl std::fmt::Display for Site {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} :: fn `{}`", self.file.display(), self.enclosing_fn)
    }
}

struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<String>,
    cfg_test_depth: u32,
    sites: &'a mut Vec<Site>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, sites: &'a mut Vec<Site>) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            sites,
        }
    }

    fn current_fn(&self) -> &str {
        self.fn_stack.last().map(String::as_str).unwrap_or("<root>")
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        let entered_test = has_cfg_test(&m.attrs) || m.ident == "tests";
        if entered_test {
            self.cfg_test_depth += 1;
        }
        syn::visit::visit_item_mod(self, m);
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let entered_test = has_cfg_test(&i.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        syn::visit::visit_item_impl(self, i);
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let entered_test = has_cfg_test(&f.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.fn_stack.push(f.sig.ident.to_string());
        syn::visit::visit_item_fn(self, f);
        self.fn_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        let entered_test = has_cfg_test(&f.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.fn_stack.push(f.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, f);
        self.fn_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_expr_call(&mut self, c: &'ast ExprCall) {
        // Skip test scaffolding entirely; the production inventory is
        // the audit target.
        if self.cfg_test_depth == 0 {
            if let syn::Expr::Path(p) = &*c.func {
                if let Some(last_seg) = p.path.segments.last() {
                    if last_seg.ident == SYMBOL {
                        // Skip the helper definition itself (which has
                        // its own `fn install_fact_tracer(...)` head).
                        if self.current_fn() != SYMBOL {
                            self.sites.push(Site {
                                file: self.file.to_path_buf(),
                                enclosing_fn: self.current_fn().to_string(),
                            });
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
}

fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let rendered = match &a.meta {
            Meta::List(list) => list.tokens.to_string(),
            _ => return false,
        };
        for token in rendered.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if token == "test" {
                return true;
            }
        }
        false
    })
}

fn walk_production_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let p = path.to_string_lossy().replace('\\', "/");
        if p.contains("/tests/") || p.contains("/benches/") || p.contains("/examples/") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
        }
        files.push(path.to_path_buf());
    }
    files
}

fn scan_file(path: &Path, sites: &mut Vec<Site>) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut scanner = Scanner::new(path, sites);
    scanner.visit_file(&parsed);
}

fn format_inventory(sites: &[Site]) -> String {
    let mut by_file: BTreeMap<&Path, Vec<&Site>> = BTreeMap::new();
    for s in sites {
        by_file.entry(s.file.as_path()).or_default().push(s);
    }
    let mut lines = Vec::new();
    for (file, ss) in by_file {
        lines.push(format!("  {}", file.display()));
        for s in ss {
            lines.push(format!("    fn `{}`", s.enclosing_fn));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Informational inventory.
// ---------------------------------------------------------------------------

/// Inventory of every production `install_fact_tracer(host, ...)` call
/// site in `crates/verter_session/src/`. Printed to stdout via
/// `--nocapture`; the test passes as long as the scanner found at
/// least one site.
///
/// This is a plan-landed-audit aid, NOT a fail-loud architectural
/// guard. If you need to verify a specific allow-list, write a
/// targeted guard alongside this informational one.
#[test]
fn fact_tracer_install_sites_inventory() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut sites = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut sites);
    }

    let inventory = format_inventory(&sites);
    println!(
        "\n=== `install_fact_tracer` call-site inventory ({} site(s)) ===\n{}\n=== end inventory ===",
        sites.len(),
        inventory
    );

    // Lower bound: the scanner must find at least one site. Today the
    // tracer is installed by at least the component-meta BFS cold path
    // (`component_meta_caches.rs`), the materialiser cold path
    // (`component_meta_materialize.rs`), the prepared-decl cold path
    // (`host_manage/prepared_decl.rs`), and the dispatch builder
    // (`project_semantic_dispatch/mod.rs`).
    //
    // If this assertion ever fires, either:
    // - the `install_fact_tracer` helper has been renamed (update the
    //   `SYMBOL` constant), OR
    // - the helper has been fully retired (delete this informational
    //   guard and the sibling `fact_tracer_arch_guard.rs` rationale
    //   comments together).
    assert!(
        !sites.is_empty(),
        "no production `install_fact_tracer` call sites found in \
         `crates/verter_session/src/`. Either the helper was renamed \
         or retired; reconcile this inventory guard with the current \
         tracer surface."
    );
}

// ---------------------------------------------------------------------------
// Sentinel: scanner discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Confirm the scanner classifies fixtures correctly. Without this,
/// the inventory could be silently empty.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: a free-function call — recorded.
    let fixture_a = r#"
        fn caller() {
            let (out, finalise) = install_fact_tracer(host, body);
            let _ = (out, finalise);
        }
    "#;
    let sites = scan_fixture_sites(fixture_a);
    assert_eq!(sites.len(), 1, "scanner failed to record a call site");
    assert_eq!(sites[0].enclosing_fn, "caller");

    // Fixture B: a qualified-path call — recorded.
    let fixture_b = r#"
        fn caller() {
            let (out, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, body);
            let _ = (out, finalise);
        }
    "#;
    let sites = scan_fixture_sites(fixture_b);
    assert_eq!(
        sites.len(),
        1,
        "scanner failed to record a qualified-path call site"
    );

    // Fixture C: the helper definition itself — NOT recorded (it
    // recursively names itself only via the outer `fn` head; no inner
    // call expression has the watched name).
    let fixture_c = r#"
        pub(crate) fn install_fact_tracer<F, R>(host: &Foo, f: F) -> (R, Finalise) {
            f(host)
        }
    "#;
    let sites = scan_fixture_sites(fixture_c);
    assert_eq!(
        sites.len(),
        0,
        "scanner unexpectedly recorded the helper definition"
    );

    // Fixture D: test scaffolding — NOT recorded.
    let fixture_d = r#"
        #[cfg(test)]
        mod tests {
            fn t() {
                let _ = install_fact_tracer(host, body);
            }
        }
    "#;
    let sites = scan_fixture_sites(fixture_d);
    assert_eq!(
        sites.len(),
        0,
        "scanner unexpectedly recorded a #[cfg(test)] call: {:?}",
        sites
    );
}

fn scan_fixture_sites(src: &str) -> Vec<Site> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut sites = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner::new(fake_path, &mut sites);
    scanner.visit_file(&parsed);
    sites
}
