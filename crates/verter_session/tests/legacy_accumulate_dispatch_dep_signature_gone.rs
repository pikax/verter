//! Architecture guard — `accumulate_dispatch_dep_signature` helper is
//! gone after Block 9 retires the legacy dispatch accumulator.
//!
//! The legacy `meta_resolve::dep_signature::accumulate_dispatch_dep_signature`
//! helper predates the fact-tracer fan-out substrate. Block 1.C / 1.G
//! introduced a dual-emit window: every legacy
//! `accumulate_dispatch_dep_signature(sig)` call is paired with an
//! `observe_fact_signature(...)` call inside one of two helpers
//! (`emit_slot_binding_graph_dispatch_facts`,
//! `emit_dispatch_dep_signature_facts`), so both the curated
//! `state.fact_versions` channel and the `ACTIVE_TRACERS` fan-out
//! observe the same dep facts.
//!
//! Block 9 deletes the helper and the legacy drain. Once the
//! `state.fact_versions` channel is retired and every consumer reads
//! from the tracer-finalised `ReadSetSignature.facts` only, the helper
//! becomes dead code and is deleted.
//!
//! `#[ignore]` reason: the helper still exists at HEAD (commit
//! `e79dbdb54`) and is actively invoked by the two dual-emit wrappers.
//! Un-ignoring this test before Block 9 lands would fail — so the
//! guard sits dormant until the retirement commit deletes the symbol,
//! at which point the `#[ignore]` line is removed alongside the
//! producer.
//!
//! When activated, the guard scans
//! `crates/verter_session/src/**/*.rs` (production source) for:
//!
//! 1. The function-definition shape
//!    `fn accumulate_dispatch_dep_signature(`.
//! 2. Any call site whose callee path's last segment is
//!    `accumulate_dispatch_dep_signature`.
//!
//! Either form is a Block-9 regression.

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

const SYMBOL: &str = "accumulate_dispatch_dep_signature";

#[derive(Debug)]
struct Hit {
    file: PathBuf,
    line_hint: String,
    kind: &'static str,
    enclosing_fn: String,
}

impl std::fmt::Display for Hit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: [{}] in `{}` -- {}",
            self.file.display(),
            self.kind,
            self.enclosing_fn,
            self.line_hint
        )
    }
}

struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<String>,
    cfg_test_depth: u32,
    hits: &'a mut Vec<Hit>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, hits: &'a mut Vec<Hit>) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            hits,
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
        // Function definition itself with the legacy name.
        if f.sig.ident == SYMBOL && self.cfg_test_depth == 0 {
            self.hits.push(Hit {
                file: self.file.to_path_buf(),
                line_hint: format!("fn {SYMBOL}(...) -- helper definition still present"),
                kind: "definition",
                enclosing_fn: "<file scope>".to_string(),
            });
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
        if f.sig.ident == SYMBOL && self.cfg_test_depth == 0 {
            self.hits.push(Hit {
                file: self.file.to_path_buf(),
                line_hint: format!("fn {SYMBOL}(...) -- impl method with legacy name"),
                kind: "definition",
                enclosing_fn: "<impl>".to_string(),
            });
        }
        self.fn_stack.push(f.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, f);
        self.fn_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_expr_call(&mut self, c: &'ast ExprCall) {
        if self.cfg_test_depth == 0 {
            if let syn::Expr::Path(p) = &*c.func {
                if let Some(last_seg) = p.path.segments.last() {
                    if last_seg.ident == SYMBOL {
                        self.hits.push(Hit {
                            file: self.file.to_path_buf(),
                            line_hint: format!("call site `{}(...)`", render_path(&p.path)),
                            kind: "call-site",
                            enclosing_fn: self.current_fn().to_string(),
                        });
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
}

fn render_path(path: &syn::Path) -> String {
    let mut out = String::new();
    if path.leading_colon.is_some() {
        out.push_str("::");
    }
    for (i, seg) in path.segments.iter().enumerate() {
        if i > 0 {
            out.push_str("::");
        }
        out.push_str(&seg.ident.to_string());
    }
    out
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

fn scan_file(path: &Path, hits: &mut Vec<Hit>) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut scanner = Scanner::new(path, hits);
    scanner.visit_file(&parsed);
}

fn format_hits(hits: &[Hit]) -> String {
    let mut by_file: BTreeMap<&Path, Vec<&Hit>> = BTreeMap::new();
    for h in hits {
        by_file.entry(h.file.as_path()).or_default().push(h);
    }
    let mut lines = Vec::new();
    for (file, hs) in by_file {
        lines.push(format!("  {}", file.display()));
        for h in hs {
            lines.push(format!(
                "    [{}] in `{}`: {}",
                h.kind, h.enclosing_fn, h.line_hint
            ));
        }
    }
    format!(
        "found {} `{}` symbol reference(s):\n{}",
        hits.len(),
        SYMBOL,
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Production-tree guard (block-gated).
// ---------------------------------------------------------------------------

/// `accumulate_dispatch_dep_signature` (the helper definition AND
/// every call site) is forbidden in production source after Block 9
/// retires the legacy dispatch accumulator.
///
/// Today the symbol still exists at HEAD (the dual-emit wrappers
/// `emit_slot_binding_graph_dispatch_facts` and
/// `emit_dispatch_dep_signature_facts` still call it). The test sits
/// `#[ignore]`'d until Block 9 deletes both helpers and the legacy
/// drain. The fact-tracer fan-out (`observe_fact_signature`) is the
/// sole dispatch-fact emission path after that.
///
/// Sibling guard: `no_accumulate_dispatch_dep_signature_outside_helpers.rs`
/// (Block 1.G) pins down which helpers may CALL this symbol during the
/// dual-emit window. That guard goes green now; THIS guard activates
/// only when the symbol itself disappears.
#[test]
#[ignore = "block-9 RED — closed by accumulate_dispatch_dep_signature deletion"]
fn no_accumulate_dispatch_dep_signature_in_production() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut hits = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut hits);
    }
    assert!(
        hits.is_empty(),
        "Block 5 `legacy_accumulate_dispatch_dep_signature_gone` violation (gated):\n{}\n\n\
         `{SYMBOL}` is the legacy dispatch accumulator. Block 9 retires \n\
         it; after that commit, every dispatch fact emission must route \n\
         through `observe_fact_signature(...)` via the fact-tracer \n\
         fan-out only. Re-introducing the symbol — either as a function \n\
         definition or as a call site — is a regression.",
        format_hits(&hits)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Drive the scanner against synthetic fixtures so the
/// production-tree guard cannot pass trivially when un-ignored.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: function definition with the legacy name — REJECTED.
    let fixture_a = r#"
        pub fn accumulate_dispatch_dep_signature(sig: &DepSignature) {
            let _ = sig;
        }
    "#;
    assert!(
        !scan_fixture_hits(fixture_a).is_empty(),
        "scanner failed to flag function definition `fn {SYMBOL}(...)`"
    );

    // Fixture B: free-function call — REJECTED.
    let fixture_b = r#"
        fn caller() {
            accumulate_dispatch_dep_signature(&sig);
        }
    "#;
    assert!(
        !scan_fixture_hits(fixture_b).is_empty(),
        "scanner failed to flag free-function call to `{SYMBOL}`"
    );

    // Fixture C: qualified-path call — REJECTED.
    let fixture_c = r#"
        fn caller() {
            super::dep_signature::accumulate_dispatch_dep_signature(&sig);
        }
    "#;
    assert!(
        !scan_fixture_hits(fixture_c).is_empty(),
        "scanner failed to flag qualified-path call to `{SYMBOL}`"
    );

    // Fixture D: clean code — ACCEPTED.
    let fixture_d = r#"
        fn caller() {
            observe_fact_signature(&fact);
        }
    "#;
    assert!(
        scan_fixture_hits(fixture_d).is_empty(),
        "scanner incorrectly flagged clean code: {:?}",
        scan_fixture_hits(fixture_d)
    );

    // Fixture E: `#[cfg(test)] mod` containing the symbol — ACCEPTED.
    // Test scaffolding is exempt, mirroring the dual-emit window guard.
    let fixture_e = r#"
        #[cfg(test)]
        mod tests {
            fn t() {
                accumulate_dispatch_dep_signature(&sig);
            }
        }
    "#;
    assert!(
        scan_fixture_hits(fixture_e).is_empty(),
        "scanner incorrectly flagged a #[cfg(test)] mod body: {:?}",
        scan_fixture_hits(fixture_e)
    );

    // Fixture F: `use` import is not a call — never flagged.
    let fixture_f = r#"
        use crate::meta_resolve::dep_signature::accumulate_dispatch_dep_signature;
        fn unrelated() {}
    "#;
    assert!(
        scan_fixture_hits(fixture_f).is_empty(),
        "scanner incorrectly flagged a `use` import: {:?}",
        scan_fixture_hits(fixture_f)
    );

    // Fixture G: impl method with the legacy name — REJECTED.
    let fixture_g = r#"
        impl Foo {
            fn accumulate_dispatch_dep_signature(&self) {}
        }
    "#;
    assert!(
        !scan_fixture_hits(fixture_g).is_empty(),
        "scanner failed to flag impl method `fn {SYMBOL}`"
    );
}

fn scan_fixture_hits(src: &str) -> Vec<Hit> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut hits = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner::new(fake_path, &mut hits);
    scanner.visit_file(&parsed);
    hits
}
